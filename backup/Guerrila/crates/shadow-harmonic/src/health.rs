// G7 – Coarse Harmonic lane health publisher
//
// Symmetric to `shadow-egress-jito/src/health.rs`.
//
// Maintains the `harmonic_state` and `harmonic_queue_age_class` fields inside
// `/dev/shm/shadow_egress_health`, publishing updates with hysteresis
// so that one noisy regional failure does not thrash the published state.
//
// Workers read the published health to gate pre-sign egress admission.

use shadow_ipc::egress_health::{
    EgressHealthPayload32B, EgressHealthSlot64B, EgressHealthSnapshot, LaneState, QueueAgeClass,
};
use shadow_ipc::segment::MappedSegment;

// ---------------------------------------------------------------------------
// Health state machine
// ---------------------------------------------------------------------------

/// Tracks consecutive success / failure counts and derives a coarse lane state
/// with hysteresis-gated transitions.
pub struct HarmonicHealthTracker {
    /// Current published lane state.
    state: LaneState,
    /// Current published queue-age class.
    queue_age_class: QueueAgeClass,
    /// Consecutive failures since last success.
    consecutive_failures: u32,
    /// Consecutive successes since last failure.
    consecutive_successes: u32,
    /// Failures required to transition healthy → degraded.
    degrade_threshold: u32,
    /// Failures required to transition degraded → down.
    down_threshold: u32,
    /// Consecutive successes required to recover from degraded → healthy.
    recover_batches: u32,
    /// Monotonic publish epoch (bumped on every write).
    publish_epoch: u64,
}

impl HarmonicHealthTracker {
    /// Create a new tracker.
    ///
    /// Starts in `Down` (fail-closed) — the proxy must prove connectivity
    /// before workers will admit envelopes to this lane.
    pub fn new(degrade_threshold: u32, down_threshold: u32, recover_batches: u32) -> Self {
        Self {
            state: LaneState::Down,
            queue_age_class: QueueAgeClass::Fresh,
            consecutive_failures: 0,
            consecutive_successes: 0,
            degrade_threshold,
            down_threshold,
            recover_batches,
            publish_epoch: 0,
        }
    }

    /// Record a successful send batch.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes = self.consecutive_successes.saturating_add(1);
        self.update_state();
    }

    /// Record a failed send attempt.
    pub fn record_failure(&mut self) {
        self.consecutive_successes = 0;
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.update_state();
    }

    /// Update the queue-age class based on ring occupancy.
    pub fn update_queue_age(&mut self, ring_len: u64, ring_capacity: u32) {
        let cap = ring_capacity as u64;
        if cap == 0 {
            self.queue_age_class = QueueAgeClass::Severe;
            return;
        }

        let fill_pct = (ring_len * 100) / cap;
        self.queue_age_class = match fill_pct {
            0..=24 => QueueAgeClass::Fresh,
            25..=49 => QueueAgeClass::Warm,
            50..=74 => QueueAgeClass::Backlogged,
            _ => QueueAgeClass::Severe,
        };
    }

    /// Get the current lane state.
    pub fn state(&self) -> LaneState {
        self.state
    }

    /// Get the current queue-age class.
    pub fn queue_age_class(&self) -> QueueAgeClass {
        self.queue_age_class
    }

    /// Build a payload suitable for writing to the egress health segment.
    ///
    /// Only the Harmonic fields are authoritative; Jito fields are set to
    /// placeholder values and will be overwritten by the read-modify-write
    /// in `publish_harmonic_health`.
    pub fn to_payload(
        &mut self,
        now_unix_ns: u64,
        hysteresis_batches: u16,
    ) -> EgressHealthPayload32B {
        self.publish_epoch += 1;
        EgressHealthPayload32B {
            jito_state: LaneState::Down as u8, // not our responsibility
            harmonic_state: self.state as u8,
            jito_queue_age_class: QueueAgeClass::Fresh as u8, // not our responsibility
            harmonic_queue_age_class: self.queue_age_class as u8,
            degraded_hysteresis_batches: hysteresis_batches,
            _pad_u16: 0,
            last_update_unix_ns: now_unix_ns,
            publish_epoch: self.publish_epoch,
            _reserved: [0u8; 8],
        }
    }

    // -- internal --

    fn update_state(&mut self) {
        match self.state {
            LaneState::Up => {
                if self.consecutive_failures >= self.degrade_threshold {
                    self.state = LaneState::Degraded;
                }
            }
            LaneState::Degraded => {
                if self.consecutive_failures >= self.down_threshold {
                    self.state = LaneState::Down;
                } else if self.consecutive_successes >= self.recover_batches {
                    self.state = LaneState::Up;
                }
            }
            LaneState::Down => {
                if self.consecutive_successes >= self.recover_batches {
                    self.state = LaneState::Up;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Publish helper
// ---------------------------------------------------------------------------

/// Write the Harmonic health fields into the egress health segment.
///
/// The Jito proxy owns the Jito fields; we must read-modify-write
/// to avoid clobbering them.  Symmetric to `publish_jito_health`.
pub fn publish_harmonic_health(
    segment: &MappedSegment<EgressHealthSnapshot>,
    tracker: &mut HarmonicHealthTracker,
    now_unix_ns: u64,
    hysteresis_batches: u16,
) {
    let health = segment.payload();

    // Read the currently-active payload so we can preserve Jito fields.
    let mut payload = match health.read_active() {
        Some(p) => p,
        None => {
            // Never published yet — start fresh.
            EgressHealthPayload32B {
                jito_state: LaneState::Down as u8,
                harmonic_state: LaneState::Down as u8,
                jito_queue_age_class: QueueAgeClass::Fresh as u8,
                harmonic_queue_age_class: QueueAgeClass::Fresh as u8,
                degraded_hysteresis_batches: hysteresis_batches,
                _pad_u16: 0,
                last_update_unix_ns: 0,
                publish_epoch: 0,
                _reserved: [0u8; 8],
            }
        }
    };

    // Overwrite only Harmonic fields.
    let fresh = tracker.to_payload(now_unix_ns, hysteresis_batches);
    payload.harmonic_state = fresh.harmonic_state;
    payload.harmonic_queue_age_class = fresh.harmonic_queue_age_class;
    payload.last_update_unix_ns = fresh.last_update_unix_ns;
    payload.publish_epoch = fresh.publish_epoch;
    payload.degraded_hysteresis_batches = fresh.degraded_hysteresis_batches;

    // Write to the next slot via double-buffer protocol.
    let active_idx = health
        .active_idx
        .active_idx
        .load(std::sync::atomic::Ordering::Acquire);
    let write_slot: &EgressHealthSlot64B = if active_idx == 0 {
        &health.slot1
    } else {
        &health.slot0
    };
    write_slot.write_payload(&payload);

    // Flip active index.
    let new_idx: u8 = if active_idx == 0 { 1 } else { 0 };
    health
        .active_idx
        .active_idx
        .store(new_idx, std::sync::atomic::Ordering::Release);
}

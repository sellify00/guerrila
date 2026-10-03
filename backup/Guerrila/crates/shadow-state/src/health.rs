//! Health snapshot computation and publication for `shadow-state`.
//!
//! Owns the shared-memory health segment and computes the coarse health
//! view that the rest of the system consumes.  Workers derive
//! `state_lag_slots = ingress_slot - health.snapshot_slot`, so this
//! module is the source-of-truth for the monotonic `snapshot_slot` and
//! the per-subsystem freshness contract.
//!
//! ## Published fields
//!
//! | Field                        | Meaning                                        |
//! |------------------------------|------------------------------------------------|
//! | `state_is_healthy`           | 0 = stale/unhealthy, 1 = healthy               |
//! | `snapshot_slot`              | monotonic — last rooted slot promoted to shm    |
//! | `last_pool_update_unix_ns`   | timestamp of last pool publish                  |
//! | `last_mint_update_unix_ns`   | timestamp of last mint publish                  |
//! | `last_alt_update_unix_ns`    | timestamp of last ALT publish                   |
//! | `leader_schedule_last_refresh_unix_ns` | timestamp of last leader schedule write |
//! | `tip_lut_last_refresh_unix_ns`         | timestamp of last transfer-fee update  |
//!
//! `tip_lut_last_refresh_unix_ns` is repurposed for transfer-fee
//! freshness (the `HealthPayload56B` layout in `shadow-ipc` predates
//! the rename, but the field matches 1:1 and no layout change is
//! needed).
//!
//! ## Staleness decision
//!
//! [`HealthPublisher::is_healthy`] evaluates the freshness thresholds
//! from [`FreshnessConfig`](crate::config::FreshnessConfig):
//!
//! 1. `snapshot_slot` must have advanced within `max_snapshot_age_ms`.
//! 2. Each per-subsystem timestamp must be within its configured max age.
//!
//! If any threshold is violated, `state_is_healthy` is set to `0` and
//! workers fail closed.

use shadow_ipc::health::{HealthPayload56B, HealthSnapshot};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::FreshnessConfig;
use crate::publish::PublishStats;

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Returns the current unix timestamp in nanoseconds.
pub fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

// ---------------------------------------------------------------------------
// Freshness tracker
// ---------------------------------------------------------------------------

/// Per-subsystem freshness timestamps maintained by the health publisher.
///
/// Each field records the unix-ns timestamp of the last successful
/// publish for that subsystem.  Zero means "never published" and will
/// cause [`HealthPublisher::is_healthy`] to return `false` (fail closed).
#[derive(Debug, Clone, Copy, Default)]
pub struct FreshnessTimestamps {
    pub last_pool_ns: u64,
    pub last_mint_ns: u64,
    pub last_alt_ns: u64,
    pub last_leader_ns: u64,
    pub last_fee_update_ns: u64,
}

// ---------------------------------------------------------------------------
// Health publisher
// ---------------------------------------------------------------------------

/// Computes and publishes health snapshots to shared memory.
///
/// Wraps a reference to an mmap-backed `HealthSnapshot`, maintains
/// per-subsystem freshness timestamps, and owns the staleness decision.
/// The caller drives this by recording events (state publishes, leader
/// schedule changes, fee updates) and periodically calling
/// [`publish_health`](Self::publish_health) to flush the health view
/// to shared memory.
pub struct HealthPublisher<'a> {
    /// The shared-memory health segment (double-buffered, RCU).
    snapshot: &'a HealthSnapshot,

    /// Per-subsystem freshness timestamps.
    pub freshness: FreshnessTimestamps,

    /// Monotonic snapshot slot — the highest rooted slot whose state
    /// has been published to shared memory.
    snapshot_slot: u64,

    /// Unix-ns timestamp of the last `snapshot_slot` advance.
    last_snapshot_advance_ns: u64,

    /// Coarse source-path liveness flag.  Mirrors
    /// `SourceHealthCounters::source_is_healthy`.  `false` initially
    /// (fail closed), set `true` after bootstrap completes or source
    /// recovers, `false` on degrade/disconnect.
    source_is_healthy: bool,
}

impl<'a> HealthPublisher<'a> {
    pub fn new(snapshot: &'a HealthSnapshot) -> Self {
        Self {
            snapshot,
            freshness: FreshnessTimestamps::default(),
            snapshot_slot: 0,
            last_snapshot_advance_ns: 0,
            source_is_healthy: false,
        }
    }

    // -------------------------------------------------------------------
    // Event recorders
    // -------------------------------------------------------------------

    /// Record a state publish cycle.
    ///
    /// Updates per-subsystem freshness timestamps based on what was
    /// actually published, and advances `snapshot_slot` monotonically.
    pub fn record_state_publish(&mut self, stats: &PublishStats, rooted_slot: u64) {
        let now = unix_ns_now();

        if stats.pools_published > 0 {
            self.freshness.last_pool_ns = now;
        }
        if stats.mints_published > 0 {
            self.freshness.last_mint_ns = now;
        }
        if stats.alts_published > 0 {
            self.freshness.last_alt_ns = now;
        }

        if rooted_slot > self.snapshot_slot {
            self.snapshot_slot = rooted_slot;
            self.last_snapshot_advance_ns = now;
        }
    }

    /// Record rooted-slot progress even when no dirty state had to be
    /// republished.
    ///
    /// This keeps quiet-but-connected state healthy after the initial
    /// pool/mint/ALT view has been published, while preserving fail-closed
    /// behavior before the first successful publish.
    pub fn record_root_progress(&mut self, rooted_slot: u64) {
        let now = unix_ns_now();

        if rooted_slot > self.snapshot_slot {
            self.snapshot_slot = rooted_slot;
            self.last_snapshot_advance_ns = now;
        }

        // Refresh only already-initialized subsystem views. Zero still means
        // "never published" and remains unhealthy.
        if self.freshness.last_pool_ns != 0 {
            self.freshness.last_pool_ns = now;
        }
        if self.freshness.last_mint_ns != 0 {
            self.freshness.last_mint_ns = now;
        }
        if self.freshness.last_alt_ns != 0 {
            self.freshness.last_alt_ns = now;
        }
    }

    /// Record a leader schedule publication.
    pub fn record_leader_publish(&mut self) {
        self.freshness.last_leader_ns = unix_ns_now();
    }

    /// Record a live transfer-fee update (SetTransferFee instruction
    /// observed and mint re-decoded).
    ///
    /// This is mandatory in Guerrila — stale transfer-fee metadata
    /// causes quote/guard drift and fee burn.
    pub fn record_fee_update(&mut self) {
        self.freshness.last_fee_update_ns = unix_ns_now();
    }

    /// Update the source-path liveness flag.
    ///
    /// Called from the ingest loop whenever `SourceHealthCounters`
    /// transitions (bootstrap complete, recovered, degraded,
    /// disconnected).  When `require_live_fee_updates` is enabled,
    /// `is_healthy()` gates on this flag.
    pub fn record_source_health(&mut self, healthy: bool) {
        self.source_is_healthy = healthy;
    }

    // -------------------------------------------------------------------
    // Staleness decision
    // -------------------------------------------------------------------

    /// Evaluate whether the state daemon is healthy given the configured
    /// freshness thresholds.
    ///
    /// Returns `true` only if:
    /// 1. `snapshot_slot` has advanced at least once, AND
    /// 2. the snapshot-slot advance is within `max_snapshot_age_ms`, AND
    /// 3. the last pool update is within `max_pool_age_ms`, AND
    /// 4. the last mint update is within `max_mint_age_ms`.
    ///
    /// ALT and leader-schedule freshness are evaluated when their
    /// respective timestamps are non-zero (i.e. once the subsystem has
    /// published at least once, it must stay fresh).
    pub fn is_healthy(&self, cfg: &FreshnessConfig) -> bool {
        let now = unix_ns_now();

        // Must have published at least once.
        if self.snapshot_slot == 0 {
            return false;
        }

        // Snapshot slot advance freshness.
        if !within_ms(self.last_snapshot_advance_ns, now, cfg.max_snapshot_age_ms) {
            return false;
        }

        // Pool freshness — mandatory.
        if !within_ms(self.freshness.last_pool_ns, now, cfg.max_pool_age_ms) {
            return false;
        }

        // Mint freshness — mandatory.
        if !within_ms(self.freshness.last_mint_ns, now, cfg.max_mint_age_ms) {
            return false;
        }

        // ALT freshness — enforced only after first ALT publish.
        if self.freshness.last_alt_ns > 0
            && !within_ms(self.freshness.last_alt_ns, now, cfg.max_alt_age_ms)
        {
            return false;
        }

        // Leader schedule freshness — enforced only after first publish.
        if self.freshness.last_leader_ns > 0
            && !within_ms(
                self.freshness.last_leader_ns,
                now,
                cfg.max_leader_schedule_age_ms,
            )
        {
            return false;
        }

        // Source-path liveness for live fee updates.
        //
        // `require_live_fee_updates` means the daemon must be *streaming*
        // live Token-2022 updates from Geyser, not relying on startup-only
        // state.  It does NOT mean a fee-change must occur within every
        // freshness window — fee changes are rare on-chain events.
        //
        // Enforcement: when the flag is set and the source path is
        // degraded / disconnected, health fails closed.
        if cfg.require_live_fee_updates && !self.source_is_healthy {
            return false;
        }

        true
    }

    // -------------------------------------------------------------------
    // Publish
    // -------------------------------------------------------------------

    /// Compute the health decision and publish a `HealthPayload56B` to
    /// the shared-memory segment using the RCU double-buffer protocol.
    ///
    /// This is the single callsite that writes the health segment.
    /// Call it on every rooted slot (after `record_state_publish`) so
    /// workers see a fresh `snapshot_slot`.
    pub fn publish_health(&self, cfg: &FreshnessConfig) {
        let healthy = self.is_healthy(cfg);

        let payload = HealthPayload56B {
            state_is_healthy: if healthy { 1 } else { 0 },
            _pad: [0u8; 7],
            snapshot_slot: self.snapshot_slot,
            last_pool_update_unix_ns: self.freshness.last_pool_ns,
            last_mint_update_unix_ns: self.freshness.last_mint_ns,
            last_alt_update_unix_ns: self.freshness.last_alt_ns,
            leader_schedule_last_refresh_unix_ns: self.freshness.last_leader_ns,
            tip_lut_last_refresh_unix_ns: self.freshness.last_fee_update_ns,
        };

        self.publish_raw(&payload);
    }

    /// Low-level publish — writes the payload to the inactive RCU slot
    /// and flips `active_idx`.
    ///
    /// Prefer [`publish_health`](Self::publish_health) for normal
    /// operation; this is exposed for tests and edge-case overrides.
    pub fn publish_raw(&self, payload: &HealthPayload56B) {
        let active = self.snapshot.active_idx.active_idx.load(Ordering::Acquire);
        let inactive = active ^ 1;

        let slot = if inactive & 1 == 0 {
            &self.snapshot.slot0
        } else {
            &self.snapshot.slot1
        };

        slot.write_payload(payload);
        self.snapshot
            .active_idx
            .active_idx
            .store(inactive, Ordering::Release);
    }

    // -------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------

    /// Current monotonic snapshot slot.
    pub fn snapshot_slot(&self) -> u64 {
        self.snapshot_slot
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `timestamp_ns` is within `max_age_ms` milliseconds
/// of `now_ns`.  Returns `false` if `timestamp_ns` is zero (never
/// published).
fn within_ms(timestamp_ns: u64, now_ns: u64, max_age_ms: u64) -> bool {
    if timestamp_ns == 0 {
        return false;
    }
    let elapsed_ns = now_ns.saturating_sub(timestamp_ns);
    let max_age_ns = max_age_ms * 1_000_000;
    elapsed_ns <= max_age_ns
}

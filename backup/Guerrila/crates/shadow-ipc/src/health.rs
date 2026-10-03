use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::blockhash::ActiveIdx128B;

// ---------------------------------------------------------------------------
// HealthPayload56B — state health channel data
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthPayload56B {
    /// 0 = unhealthy, 1 = healthy.  Initialised to 0 (fail closed).
    pub state_is_healthy: u8,
    pub _pad: [u8; 7],
    /// Slot of the last state snapshot consumed by the worker.
    pub snapshot_slot: u64,
    /// Unix nanosecond timestamp of the last pool account update.
    pub last_pool_update_unix_ns: u64,
    /// Unix nanosecond timestamp of the last mint account update.
    pub last_mint_update_unix_ns: u64,
    /// Unix nanosecond timestamp of the last ALT (address lookup table) update.
    pub last_alt_update_unix_ns: u64,
    /// Unix nanosecond timestamp of the last leader schedule refresh.
    pub leader_schedule_last_refresh_unix_ns: u64,
    /// Unix nanosecond timestamp of the last tip LUT refresh.
    pub tip_lut_last_refresh_unix_ns: u64,
}

// ---------------------------------------------------------------------------
// HealthSlot64B — one RCU slot (seqlock-protected)
// ---------------------------------------------------------------------------

#[repr(C, align(64))]
pub struct HealthSlot64B {
    pub sequence: AtomicU32,
    pub _pad_u32: u32,
    pub payload: UnsafeCell<HealthPayload56B>,
}

// SAFETY: The seqlock protocol ensures readers never see a torn write.
// Writers must hold exclusive access (only one writer at a time).
unsafe impl Sync for HealthSlot64B {}

impl HealthSlot64B {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            _pad_u32: 0,
            payload: UnsafeCell::new(HealthPayload56B {
                state_is_healthy: 0,
                _pad: [0u8; 7],
                snapshot_slot: 0,
                last_pool_update_unix_ns: 0,
                last_mint_update_unix_ns: 0,
                last_alt_update_unix_ns: 0,
                leader_schedule_last_refresh_unix_ns: 0,
                tip_lut_last_refresh_unix_ns: 0,
            }),
        }
    }

    /// Attempt a consistent read of the payload using seqlock protocol.
    ///
    /// Returns `None` if:
    /// - the sequence number is odd (writer in progress), or
    /// - the sequence number changed between pre- and post-read (torn read).
    pub fn read(&self) -> Option<HealthPayload56B> {
        let seq1 = self.sequence.load(Ordering::Acquire);

        if seq1 & 1 != 0 {
            return None;
        }

        // SAFETY: seqlock protocol detects torn reads via post-fence check.
        let snapshot = unsafe { core::ptr::read_volatile(self.payload.get()) };

        core::sync::atomic::fence(Ordering::Acquire);

        let seq2 = self.sequence.load(Ordering::Relaxed);

        if seq1 != seq2 || seq2 & 1 != 0 {
            return None;
        }

        Some(snapshot)
    }

    /// Write a new payload. Caller must ensure exclusive writer access.
    pub fn write_payload(&self, payload: &HealthPayload56B) {
        let seq = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(1), Ordering::Release);

        // SAFETY: single-writer guarantee from the caller.
        unsafe {
            core::ptr::write_volatile(self.payload.get(), *payload);
        }

        core::sync::atomic::fence(Ordering::Release);

        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}

impl Default for HealthSlot64B {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// HealthSnapshot — the full double-buffered segment (in-memory)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct HealthSnapshot {
    pub active_idx: ActiveIdx128B,
    pub slot0: HealthSlot64B,
    pub slot1: HealthSlot64B,
}

impl HealthSnapshot {
    pub fn new() -> Self {
        Self {
            active_idx: ActiveIdx128B::new(0),
            slot0: HealthSlot64B::new(),
            slot1: HealthSlot64B::new(),
        }
    }

    /// Read the currently active slot's payload.
    ///
    /// Returns `None` if:
    /// - the active slot's seqlock read fails (torn/in-progress write), or
    /// - the payload has never been published (`last_pool_update_unix_ns == 0`).
    pub fn read_active(&self) -> Option<HealthPayload56B> {
        let idx = self.active_idx.active_idx.load(Ordering::Acquire);
        let slot = if idx & 1 == 0 {
            &self.slot0
        } else {
            &self.slot1
        };
        let payload = slot.read()?;

        // Fail closed: reject never-published payloads.
        if payload.last_pool_update_unix_ns == 0 {
            return None;
        }

        Some(payload)
    }
}

impl Default for HealthSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Layout assertions
// ---------------------------------------------------------------------------

static_assertions::assert_eq_size!(HealthPayload56B, [u8; 56]);
static_assertions::assert_eq_size!(HealthSlot64B, [u8; 64]);

impl crate::segment::SegmentSpec for HealthSnapshot {
    const SEGMENT_ID: u64 = 5;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_health";

    fn init_payload(ptr: *mut Self) {
        // Zero-init: sequences = 0 (even, no writer), payloads zeroed.
        // read_active() will return None because last_pool_update_unix_ns == 0.
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_health_payload_is_56_bytes() {
        assert_eq!(core::mem::size_of::<HealthPayload56B>(), 56);
    }

    #[test]
    fn layout_health_slot_is_64_bytes() {
        assert_eq!(core::mem::size_of::<HealthSlot64B>(), 64);
    }

    #[test]
    fn layout_health_slot_alignment() {
        assert_eq!(core::mem::align_of::<HealthSlot64B>(), 64);
    }

    #[test]
    fn layout_health_snapshot_size() {
        // ActiveIdx128B (128) + 2 × HealthSlot64B (64 each) = 256
        assert_eq!(core::mem::size_of::<HealthSnapshot>(), 256);
    }

    #[test]
    fn health_read_returns_none_on_odd_seq() {
        let slot = HealthSlot64B::new();
        slot.sequence.store(1, Ordering::Relaxed);
        assert!(slot.read().is_none(), "odd sequence must return None");
    }

    #[test]
    fn health_read_returns_valid_payload() {
        let slot = HealthSlot64B::new();
        let payload = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot: 42,
            last_pool_update_unix_ns: 1_000_000,
            last_mint_update_unix_ns: 2_000_000,
            last_alt_update_unix_ns: 3_000_000,
            leader_schedule_last_refresh_unix_ns: 4_000_000,
            tip_lut_last_refresh_unix_ns: 5_000_000,
        };
        slot.write_payload(&payload);
        let got = slot.read().expect("should read valid payload");
        assert_eq!(got, payload);
    }

    #[test]
    fn health_snapshot_read_active_slot0() {
        let snap = HealthSnapshot::new();
        let payload = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot: 100,
            last_pool_update_unix_ns: 9999,
            last_mint_update_unix_ns: 0,
            last_alt_update_unix_ns: 0,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
        };
        snap.slot0.write_payload(&payload);
        snap.active_idx.active_idx.store(0, Ordering::Release);
        let got = snap.read_active().expect("should read slot0");
        assert_eq!(got.snapshot_slot, 100);
        assert_eq!(got.state_is_healthy, 1);
    }

    #[test]
    fn health_snapshot_read_active_slot1() {
        let snap = HealthSnapshot::new();
        let payload = HealthPayload56B {
            state_is_healthy: 0,
            _pad: [0u8; 7],
            snapshot_slot: 200,
            last_pool_update_unix_ns: 7777,
            last_mint_update_unix_ns: 0,
            last_alt_update_unix_ns: 0,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
        };
        snap.slot1.write_payload(&payload);
        snap.active_idx.active_idx.store(1, Ordering::Release);
        let got = snap.read_active().expect("should read slot1");
        assert_eq!(got.snapshot_slot, 200);
        assert_eq!(got.state_is_healthy, 0);
    }

    #[test]
    fn health_fresh_snapshot_returns_none() {
        let snap = HealthSnapshot::new();
        assert!(
            snap.read_active().is_none(),
            "never-published health snapshot must return None"
        );
    }

    #[test]
    fn health_init_is_fail_closed() {
        let snap = HealthSnapshot::new();
        // Fresh slot has valid seqlock (sequence=0, even), so the raw read
        // succeeds but the payload is all zeros.
        let raw = snap.slot0.read().expect("seqlock should be valid at init");
        assert_eq!(raw.state_is_healthy, 0);
        assert_eq!(raw.snapshot_slot, 0);
        assert_eq!(raw.last_pool_update_unix_ns, 0);
        assert_eq!(raw.last_mint_update_unix_ns, 0);
        assert_eq!(raw.last_alt_update_unix_ns, 0);
        assert_eq!(raw.leader_schedule_last_refresh_unix_ns, 0);
        assert_eq!(raw.tip_lut_last_refresh_unix_ns, 0);

        // read_active fails closed: returns None because timestamp is 0.
        assert!(snap.read_active().is_none());
    }

    #[test]
    fn health_write_then_read_roundtrip() {
        let snap = HealthSnapshot::new();
        let p = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot: 500,
            last_pool_update_unix_ns: 10_000_000,
            last_mint_update_unix_ns: 20_000_000,
            last_alt_update_unix_ns: 30_000_000,
            leader_schedule_last_refresh_unix_ns: 40_000_000,
            tip_lut_last_refresh_unix_ns: 50_000_000,
        };

        // Write to slot0, mark active.
        snap.slot0.write_payload(&p);
        snap.active_idx.active_idx.store(0, Ordering::Release);

        let got = snap.read_active().unwrap();
        assert_eq!(got.state_is_healthy, 1);
        assert_eq!(got.snapshot_slot, 500);
        assert_eq!(got.last_pool_update_unix_ns, 10_000_000);
        assert_eq!(got.last_mint_update_unix_ns, 20_000_000);
        assert_eq!(got.last_alt_update_unix_ns, 30_000_000);
        assert_eq!(got.leader_schedule_last_refresh_unix_ns, 40_000_000);
        assert_eq!(got.tip_lut_last_refresh_unix_ns, 50_000_000);
    }

    #[test]
    fn health_snapshot_slot_monotonicity() {
        let snap = HealthSnapshot::new();

        // Write slot 100.
        let p1 = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot: 100,
            last_pool_update_unix_ns: 1000,
            last_mint_update_unix_ns: 0,
            last_alt_update_unix_ns: 0,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
        };
        snap.slot0.write_payload(&p1);
        snap.active_idx.active_idx.store(0, Ordering::Release);

        let got1 = snap.read_active().unwrap();
        assert_eq!(got1.snapshot_slot, 100);

        // Write slot 200 to the other buffer, swap active.
        let p2 = HealthPayload56B {
            state_is_healthy: 1,
            _pad: [0u8; 7],
            snapshot_slot: 200,
            last_pool_update_unix_ns: 2000,
            last_mint_update_unix_ns: 0,
            last_alt_update_unix_ns: 0,
            leader_schedule_last_refresh_unix_ns: 0,
            tip_lut_last_refresh_unix_ns: 0,
        };
        snap.slot1.write_payload(&p2);
        snap.active_idx.active_idx.store(1, Ordering::Release);

        let got2 = snap.read_active().unwrap();
        assert_eq!(got2.snapshot_slot, 200);
        assert!(got2.snapshot_slot > got1.snapshot_slot);
    }

    #[test]
    fn health_segment_spec() {
        use crate::segment::SegmentSpec;
        assert_eq!(HealthSnapshot::SEGMENT_ID, 5);
        assert_eq!(HealthSnapshot::VERSION, 1);
        assert_eq!(HealthSnapshot::PATH, "/dev/shm/shadow_health");
    }
}

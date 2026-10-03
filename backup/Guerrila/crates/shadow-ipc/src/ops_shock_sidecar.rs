use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::blockhash::ActiveIdx128B;
use crate::segment::{MappedSegment, SegmentError, SegmentSpec};

pub const OPS_SHOCK_STALE_FLAG: u8 = 1 << 0;

/// Fixed-width observe-only OPSO/post-shock sidecar.
///
/// This sidecar is advisory only. Consumers must still enforce exact quote,
/// freshness, inventory fence, ShadowGuard, and pre-sign egress admission
/// checks independently.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpsShockSidecarV1 {
    pub epoch: u64,
    pub last_update_unix_ns: u64,

    pub episode_id_mod: u32,
    pub pool_index: u32,

    pub owner_mode: u8,
    pub fallback_mode: u8,
    pub runner_up_mode: u8,
    pub materiality_tier: u8,

    pub ownership_delta_lcb_bucket: u16,
    pub fallback_value_lcb_bucket: u16,
    pub bid_ceiling_bucket: u16,
    pub bid_density_bucket: u16,

    pub backrun_lcb_bucket: u16,
    pub housekeeping_lcb_bucket: u16,
    pub scout_lcb_bucket: u16,
    pub continuation_lcb_bucket: u16,
    pub rebalancing_lcb_bucket: u16,
    pub drop_lcb_bucket: u16,

    pub action_mask: u16,
    pub kill_mask: u16,

    pub strictness_state_bucket: u8,
    pub flags: u8,
    pub reserved: [u8; 10],
}

impl OpsShockSidecarV1 {
    pub fn is_published(&self) -> bool {
        self.last_update_unix_ns != 0
    }

    pub fn has_flag(&self, flag: u8) -> bool {
        (self.flags & flag) != 0
    }

    pub fn with_flag(mut self, flag: u8, enabled: bool) -> Self {
        if enabled {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
        self
    }

    pub fn is_stale(&self, now_unix_ns: u64, max_staleness_ns: u64) -> bool {
        if !self.is_published() {
            return true;
        }
        if self.has_flag(OPS_SHOCK_STALE_FLAG) {
            return true;
        }
        now_unix_ns.saturating_sub(self.last_update_unix_ns) > max_staleness_ns
    }
}

impl Default for OpsShockSidecarV1 {
    fn default() -> Self {
        Self {
            epoch: 0,
            last_update_unix_ns: 0,
            episode_id_mod: 0,
            pool_index: 0,
            owner_mode: 0,
            fallback_mode: 0,
            runner_up_mode: 0,
            materiality_tier: 0,
            ownership_delta_lcb_bucket: 0,
            fallback_value_lcb_bucket: 0,
            bid_ceiling_bucket: 0,
            bid_density_bucket: 0,
            backrun_lcb_bucket: 0,
            housekeeping_lcb_bucket: 0,
            scout_lcb_bucket: 0,
            continuation_lcb_bucket: 0,
            rebalancing_lcb_bucket: 0,
            drop_lcb_bucket: 0,
            action_mask: 0,
            kill_mask: 0,
            strictness_state_bucket: 0,
            flags: 0,
            reserved: [0u8; 10],
        }
    }
}

#[repr(C, align(64))]
pub struct OpsShockSidecarSlot128B {
    pub sequence: AtomicU32,
    pub _pad_u32: u32,
    pub payload: UnsafeCell<OpsShockSidecarV1>,
}

unsafe impl Sync for OpsShockSidecarSlot128B {}

impl OpsShockSidecarSlot128B {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            _pad_u32: 0,
            payload: UnsafeCell::new(OpsShockSidecarV1::default()),
        }
    }

    pub fn read(&self) -> Option<OpsShockSidecarV1> {
        let seq1 = self.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None;
        }

        let snapshot = unsafe { core::ptr::read_volatile(self.payload.get()) };
        core::sync::atomic::fence(Ordering::Acquire);

        let seq2 = self.sequence.load(Ordering::Relaxed);
        if seq1 != seq2 || seq2 & 1 != 0 {
            return None;
        }

        Some(snapshot)
    }

    pub fn write_payload(&self, payload: &OpsShockSidecarV1) {
        let seq = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(1), Ordering::Release);

        unsafe {
            core::ptr::write_volatile(self.payload.get(), *payload);
        }

        core::sync::atomic::fence(Ordering::Release);
        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}

impl Default for OpsShockSidecarSlot128B {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct OpsShockSidecarSnapshot {
    pub active_idx: ActiveIdx128B,
    pub slot0: OpsShockSidecarSlot128B,
    pub slot1: OpsShockSidecarSlot128B,
}

impl OpsShockSidecarSnapshot {
    pub fn new() -> Self {
        Self {
            active_idx: ActiveIdx128B::new(0),
            slot0: OpsShockSidecarSlot128B::new(),
            slot1: OpsShockSidecarSlot128B::new(),
        }
    }

    pub fn read_active(&self) -> Option<OpsShockSidecarV1> {
        let idx = self.active_idx.active_idx.load(Ordering::Acquire);
        let slot = if idx & 1 == 0 {
            &self.slot0
        } else {
            &self.slot1
        };
        let payload = slot.read()?;
        if !payload.is_published() {
            return None;
        }
        Some(payload)
    }

    pub fn read_active_fresh(
        &self,
        now_unix_ns: u64,
        max_staleness_ns: u64,
    ) -> Option<OpsShockSidecarV1> {
        let payload = self.read_active()?;
        if payload.is_stale(now_unix_ns, max_staleness_ns) {
            return None;
        }
        Some(payload)
    }

    pub fn publish(&self, payload: &OpsShockSidecarV1) {
        let old = self.active_idx.active_idx.load(Ordering::Relaxed);
        let next = if old & 1 == 0 { 1 } else { 0 };
        let slot = if next == 0 { &self.slot0 } else { &self.slot1 };
        slot.write_payload(payload);
        core::sync::atomic::fence(Ordering::Release);
        self.active_idx.active_idx.store(next, Ordering::Release);
    }
}

impl Default for OpsShockSidecarSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

static_assertions::assert_eq_size!(OpsShockSidecarV1, [u8; 64]);
static_assertions::assert_eq_size!(OpsShockSidecarSlot128B, [u8; 128]);

impl SegmentSpec for OpsShockSidecarSnapshot {
    const SEGMENT_ID: u64 = 25;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_ops_shock_sidecar";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
        }
    }
}

pub fn create_shadow_ops_shock_sidecar(
) -> Result<MappedSegment<OpsShockSidecarSnapshot>, SegmentError> {
    MappedSegment::<OpsShockSidecarSnapshot>::create_rw(std::path::Path::new(
        OpsShockSidecarSnapshot::PATH,
    ))
}

pub fn open_shadow_ops_shock_sidecar_ro(
) -> Result<MappedSegment<OpsShockSidecarSnapshot>, SegmentError> {
    MappedSegment::<OpsShockSidecarSnapshot>::open_ro(std::path::Path::new(
        OpsShockSidecarSnapshot::PATH,
    ))
}

pub fn open_shadow_ops_shock_sidecar_rw(
) -> Result<MappedSegment<OpsShockSidecarSnapshot>, SegmentError> {
    MappedSegment::<OpsShockSidecarSnapshot>::open_rw(std::path::Path::new(
        OpsShockSidecarSnapshot::PATH,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_sidecar_payload_is_64_bytes_and_aligned() {
        assert_eq!(core::mem::size_of::<OpsShockSidecarV1>(), 64);
        assert_eq!(core::mem::align_of::<OpsShockSidecarV1>(), 64);
    }

    #[test]
    fn layout_sidecar_slot_is_128_bytes_and_aligned() {
        assert_eq!(core::mem::size_of::<OpsShockSidecarSlot128B>(), 128);
        assert_eq!(core::mem::align_of::<OpsShockSidecarSlot128B>(), 64);
    }

    #[test]
    fn opso_sidecar_layout_64_or_128_byte_aligned() {
        assert_eq!(core::mem::size_of::<OpsShockSidecarV1>(), 64);
        assert_eq!(core::mem::align_of::<OpsShockSidecarV1>(), 64);
        assert_eq!(core::mem::size_of::<OpsShockSidecarSlot128B>(), 128);
        assert_eq!(core::mem::align_of::<OpsShockSidecarSlot128B>(), 64);
    }

    #[test]
    fn layout_sidecar_snapshot_size() {
        assert_eq!(core::mem::size_of::<OpsShockSidecarSnapshot>(), 384);
    }

    #[test]
    fn segment_constants_are_locked() {
        assert_eq!(OpsShockSidecarSnapshot::SEGMENT_ID, 25);
        assert_eq!(OpsShockSidecarSnapshot::VERSION, 1);
        assert_eq!(
            OpsShockSidecarSnapshot::PATH,
            "/dev/shm/shadow_ops_shock_sidecar"
        );
    }

    #[test]
    fn fresh_snapshot_returns_none_until_published() {
        let snap = OpsShockSidecarSnapshot::new();
        assert!(snap.read_active().is_none());
        assert!(snap.read_active_fresh(1_000, 100).is_none());
    }

    #[test]
    fn stale_flag_kills_consumption() {
        let snap = OpsShockSidecarSnapshot::new();
        let payload = OpsShockSidecarV1 {
            epoch: 7,
            last_update_unix_ns: 1_000,
            episode_id_mod: 9,
            pool_index: 11,
            flags: OPS_SHOCK_STALE_FLAG,
            ..OpsShockSidecarV1::default()
        };
        snap.publish(&payload);

        let got = snap
            .read_active()
            .expect("published payload should be visible");
        assert_eq!(got.epoch, 7);
        assert!(snap.read_active_fresh(1_010, 100).is_none());
    }

    #[test]
    fn opso_sidecar_stale_flag_kills_consumption() {
        let snap = OpsShockSidecarSnapshot::new();
        let payload = OpsShockSidecarV1 {
            epoch: 8,
            last_update_unix_ns: 1_000,
            episode_id_mod: 19,
            pool_index: 23,
            flags: OPS_SHOCK_STALE_FLAG,
            ..OpsShockSidecarV1::default()
        };
        snap.publish(&payload);

        assert!(snap.read_active_fresh(1_050, 400_000_000).is_none());
    }

    #[test]
    fn timestamp_staleness_kills_consumption() {
        let snap = OpsShockSidecarSnapshot::new();
        let payload = OpsShockSidecarV1 {
            epoch: 3,
            last_update_unix_ns: 1_000,
            ..OpsShockSidecarV1::default()
        };
        snap.publish(&payload);

        assert!(snap.read_active_fresh(1_500, 100).is_none());
        assert!(snap.read_active_fresh(1_050, 100).is_some());
    }

    #[test]
    fn publish_and_consume_roundtrip_across_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ops_shock_roundtrip");

        let seg1 = MappedSegment::<OpsShockSidecarSnapshot>::create_rw(&path).unwrap();
        let payload = OpsShockSidecarV1 {
            epoch: 42,
            last_update_unix_ns: 7_000,
            episode_id_mod: 99,
            pool_index: 1234,
            owner_mode: 1,
            fallback_mode: 2,
            runner_up_mode: 3,
            materiality_tier: 4,
            ownership_delta_lcb_bucket: 5,
            fallback_value_lcb_bucket: 6,
            bid_ceiling_bucket: 7,
            bid_density_bucket: 8,
            backrun_lcb_bucket: 9,
            housekeeping_lcb_bucket: 10,
            scout_lcb_bucket: 11,
            continuation_lcb_bucket: 12,
            rebalancing_lcb_bucket: 13,
            drop_lcb_bucket: 14,
            action_mask: 0x55AA,
            kill_mask: 0xAA55,
            strictness_state_bucket: 15,
            flags: 0,
            reserved: [0u8; 10],
        };
        seg1.payload().publish(&payload);
        seg1.flush().unwrap();

        let seg2 = MappedSegment::<OpsShockSidecarSnapshot>::open_ro(&path).unwrap();
        let got = seg2
            .payload()
            .read_active()
            .expect("published payload should read");
        assert_eq!(got, payload);
    }

    #[test]
    fn second_publish_flips_to_other_slot() {
        let snap = OpsShockSidecarSnapshot::new();
        let first = OpsShockSidecarV1 {
            epoch: 1,
            last_update_unix_ns: 100,
            episode_id_mod: 1,
            ..OpsShockSidecarV1::default()
        };
        let second = OpsShockSidecarV1 {
            epoch: 2,
            last_update_unix_ns: 200,
            episode_id_mod: 2,
            ..OpsShockSidecarV1::default()
        };

        snap.publish(&first);
        assert_eq!(snap.read_active().unwrap().episode_id_mod, 1);
        snap.publish(&second);
        assert_eq!(snap.read_active().unwrap().episode_id_mod, 2);
    }

    #[test]
    fn open_sidecar_rejects_wrong_version() {
        use crate::segment::SegmentHeader64B;
        use std::fs::OpenOptions;
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ops_shock_wrong_version");

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();

        let header = SegmentHeader64B {
            magic: crate::segment::SEGMENT_MAGIC,
            build_git_sha_prefix: [0u8; 8],
            checksum: 0,
            segment_id: OpsShockSidecarSnapshot::SEGMENT_ID,
            payload_size: core::mem::size_of::<OpsShockSidecarSnapshot>() as u64,
            total_size: 64 + core::mem::size_of::<OpsShockSidecarSnapshot>() as u64,
            payload_offset: 64,
            version: 9,
            header_size: 64,
            payload_align: 64,
            reserved_u16: 0,
            reserved_u32: 0,
        };

        unsafe {
            let header_bytes = core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<SegmentHeader64B>(),
            );
            file.write_all(header_bytes).unwrap();
        }
        file.set_len(header.total_size).unwrap();

        let res = MappedSegment::<OpsShockSidecarSnapshot>::open_ro(&path);
        match res {
            Err(SegmentError::WrongVersion { expected, found }) => {
                assert_eq!(expected, OpsShockSidecarSnapshot::VERSION);
                assert_eq!(found, 9);
            }
            _ => panic!("expected WrongVersion error"),
        }
    }
}

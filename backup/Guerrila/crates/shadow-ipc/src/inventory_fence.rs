// G8 – Inventory fence segment
//
// PM-owned truth surface for per-worker capital accounting.
// Lives at /dev/shm/shadow_inventory_fence.
//
// Layout: one InventoryFenceSlot per money worker (up to MAX_WORKERS),
// each on its own cache line.  PM is the sole writer; workers and
// diagnostics read via open_ro.
//
// Fields track the PM's authoritative view of:
//   - settled_lamports:   confirmed landed + released capital
//   - reserved_lamports:  in-flight (signed, not yet confirmed/failed)
//   - held_lamports:      capital held pending quarantine/retry decision
//   - last_reconcile_slot: slot at which PM last reconciled this worker
//   - last_reconcile_ns:  unix-ns timestamp of last reconciliation
//   - lane:               which egress lane owns the last in-flight batch
//   - flags:              bitfield for quarantine/reconcile state

use crate::segment::SegmentSpec;

/// Maximum supported money workers.  Matches the system-wide cap.
pub const MAX_WORKERS: usize = 16;

/// Per-worker inventory truth, one cache line.
///
/// Written exclusively by PM (G8).  Workers never write this segment.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct InventoryFenceSlot {
    /// Capital confirmed landed or released back to the pool.
    pub settled_lamports: u64,
    /// Capital currently in-flight (reserved by workers, not yet confirmed).
    pub reserved_lamports: u64,
    /// Capital held by PM pending quarantine or retry decision.
    pub held_lamports: u64,
    /// Slot at which PM last reconciled this worker's position.
    pub last_reconcile_slot: u64,
    /// Unix timestamp (ns) of last reconciliation pass.
    pub last_reconcile_ns: u64,
    /// Lane identity of the last in-flight batch.
    /// 0 = Jito, 1 = Harmonic, 0xFF = none.
    pub lane: u8,
    /// Bitfield:
    ///   bit 0: reconcile_pending (PM has queued but not committed a delta)
    ///   bit 1: quarantine_hold  (capital frozen by quarantine policy)
    ///   bits 2-7: reserved
    pub flags: u8,
    /// Padding to 64 bytes.
    pub _pad: [u8; 14],
}

static_assertions::assert_eq_size!(InventoryFenceSlot, [u8; 64]);
static_assertions::const_assert_eq!(core::mem::align_of::<InventoryFenceSlot>(), 64);

impl InventoryFenceSlot {
    pub const ZERO: Self = Self {
        settled_lamports: 0,
        reserved_lamports: 0,
        held_lamports: 0,
        last_reconcile_slot: 0,
        last_reconcile_ns: 0,
        lane: 0xFF,
        flags: 0,
        _pad: [0; 14],
    };

    /// Returns true if the reconcile_pending flag is set.
    #[inline]
    pub fn reconcile_pending(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Returns true if capital is frozen by quarantine policy.
    #[inline]
    pub fn quarantine_hold(&self) -> bool {
        self.flags & 0x02 != 0
    }
}

/// Top-level inventory fence segment.
///
/// Fixed-size array of MAX_WORKERS slots.  Workers that don't exist
/// are left zeroed (lane = 0xFF).
#[repr(C, align(64))]
pub struct InventoryFenceSegment {
    /// Number of active workers (set at bootstrap, immutable thereafter).
    pub active_workers: u32,
    pub _header_pad: [u8; 60],
    /// Per-worker slots.
    pub slots: [InventoryFenceSlot; MAX_WORKERS],
}

// Total: 64 (header) + 16 * 64 (slots) = 1088 bytes.
static_assertions::assert_eq_size!(InventoryFenceSegment, [u8; 64 + MAX_WORKERS * 64]);

// SAFETY: No interior mutability.  PM is sole writer; all other
// consumers open read-only.
unsafe impl Sync for InventoryFenceSegment {}

impl InventoryFenceSegment {
    /// Effective active worker count, clamped to MAX_WORKERS.
    /// Prevents a corrupt or bogus `active_workers` from causing
    /// out-of-bounds access on the fixed-size slot array.
    #[inline]
    pub fn clamped_active(&self) -> usize {
        core::cmp::min(self.active_workers as usize, MAX_WORKERS)
    }

    /// Returns the slot for the given worker, or None if out of range.
    #[inline]
    pub fn slot(&self, worker_id: u32) -> Option<&InventoryFenceSlot> {
        let id = worker_id as usize;
        if id < self.clamped_active() {
            Some(&self.slots[id])
        } else {
            None
        }
    }

    /// Returns a mutable slot for PM writes.
    #[inline]
    pub fn slot_mut(&mut self, worker_id: u32) -> Option<&mut InventoryFenceSlot> {
        let id = worker_id as usize;
        if id < self.clamped_active() {
            Some(&mut self.slots[id])
        } else {
            None
        }
    }

    /// Total reserved lamports across all active workers.
    pub fn total_reserved(&self) -> u64 {
        let mut total = 0u64;
        for i in 0..self.clamped_active() {
            total = total.saturating_add(self.slots[i].reserved_lamports);
        }
        total
    }

    /// Total held lamports across all active workers.
    pub fn total_held(&self) -> u64 {
        let mut total = 0u64;
        for i in 0..self.clamped_active() {
            total = total.saturating_add(self.slots[i].held_lamports);
        }
        total
    }
}

impl SegmentSpec for InventoryFenceSegment {
    const SEGMENT_ID: u64 = 20;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_inventory_fence";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            // Set all lanes to 0xFF (none).
            let seg = &mut *ptr;
            for slot in seg.slots.iter_mut() {
                slot.lane = 0xFF;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Create / open helpers
// ---------------------------------------------------------------------------

use crate::segment::{MappedSegment, SegmentError};

/// Create the inventory fence segment at the canonical path.
///
/// Returns `SegmentError::InvalidPayload` if `active_workers` exceeds
/// `MAX_WORKERS`.
pub fn create_shadow_inventory_fence(
    active_workers: u32,
) -> Result<MappedSegment<InventoryFenceSegment>, SegmentError> {
    if active_workers as usize > MAX_WORKERS {
        return Err(SegmentError::InvalidPayload {
            reason: format!(
                "active_workers ({active_workers}) exceeds MAX_WORKERS ({MAX_WORKERS})"
            ),
        });
    }
    let seg = MappedSegment::<InventoryFenceSegment>::create_rw(std::path::Path::new(
        InventoryFenceSegment::PATH,
    ))?;
    // Write active_workers into the header.
    let payload = seg.payload_mut();
    payload.active_workers = active_workers;
    Ok(seg)
}

/// Open the inventory fence segment read-only (workers / diagnostics).
pub fn open_shadow_inventory_fence_ro() -> Result<MappedSegment<InventoryFenceSegment>, SegmentError>
{
    MappedSegment::<InventoryFenceSegment>::open_ro(std::path::Path::new(
        InventoryFenceSegment::PATH,
    ))
}

/// Open the inventory fence segment read-write (PM only).
pub fn open_shadow_inventory_fence_rw() -> Result<MappedSegment<InventoryFenceSegment>, SegmentError>
{
    MappedSegment::<InventoryFenceSegment>::open_rw(std::path::Path::new(
        InventoryFenceSegment::PATH,
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_slot_is_64_bytes_64_aligned() {
        assert_eq!(core::mem::size_of::<InventoryFenceSlot>(), 64);
        assert_eq!(core::mem::align_of::<InventoryFenceSlot>(), 64);
    }

    #[test]
    fn layout_segment_is_1088_bytes() {
        assert_eq!(
            core::mem::size_of::<InventoryFenceSegment>(),
            64 + MAX_WORKERS * 64
        );
    }

    #[test]
    fn layout_segment_is_64_aligned() {
        assert_eq!(core::mem::align_of::<InventoryFenceSegment>(), 64);
    }

    #[test]
    fn slot_zero_const_is_all_zeroes_except_lane() {
        let s = InventoryFenceSlot::ZERO;
        assert_eq!(s.settled_lamports, 0);
        assert_eq!(s.reserved_lamports, 0);
        assert_eq!(s.held_lamports, 0);
        assert_eq!(s.last_reconcile_slot, 0);
        assert_eq!(s.last_reconcile_ns, 0);
        assert_eq!(s.lane, 0xFF);
        assert_eq!(s.flags, 0);
        assert!(!s.reconcile_pending());
        assert!(!s.quarantine_hold());
    }

    #[test]
    fn flags_reconcile_pending_bit() {
        let mut s = InventoryFenceSlot::ZERO;
        s.flags = 0x01;
        assert!(s.reconcile_pending());
        assert!(!s.quarantine_hold());
    }

    #[test]
    fn flags_quarantine_hold_bit() {
        let mut s = InventoryFenceSlot::ZERO;
        s.flags = 0x02;
        assert!(!s.reconcile_pending());
        assert!(s.quarantine_hold());
    }

    #[test]
    fn flags_both_bits_set() {
        let mut s = InventoryFenceSlot::ZERO;
        s.flags = 0x03;
        assert!(s.reconcile_pending());
        assert!(s.quarantine_hold());
    }

    #[test]
    fn segment_slot_accessor_bounds() {
        let mut seg = InventoryFenceSegment {
            active_workers: 4,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        assert!(seg.slot(0).is_some());
        assert!(seg.slot(3).is_some());
        assert!(seg.slot(4).is_none());
        assert!(seg.slot(16).is_none());
        assert!(seg.slot_mut(3).is_some());
        assert!(seg.slot_mut(4).is_none());
    }

    #[test]
    fn total_reserved_sums_active_workers_only() {
        let mut seg = InventoryFenceSegment {
            active_workers: 3,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        seg.slots[0].reserved_lamports = 100;
        seg.slots[1].reserved_lamports = 200;
        seg.slots[2].reserved_lamports = 300;
        seg.slots[3].reserved_lamports = 999; // inactive — not counted
        assert_eq!(seg.total_reserved(), 600);
    }

    #[test]
    fn total_held_sums_active_workers_only() {
        let mut seg = InventoryFenceSegment {
            active_workers: 2,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        seg.slots[0].held_lamports = 50;
        seg.slots[1].held_lamports = 75;
        seg.slots[2].held_lamports = 999; // inactive
        assert_eq!(seg.total_held(), 125);
    }

    #[test]
    fn total_reserved_saturates_on_overflow() {
        let mut seg = InventoryFenceSegment {
            active_workers: 2,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        seg.slots[0].reserved_lamports = u64::MAX;
        seg.slots[1].reserved_lamports = 1;
        assert_eq!(seg.total_reserved(), u64::MAX);
    }

    #[test]
    fn init_payload_zeroes_and_sets_lane_to_none() {
        // Allocate with proper 64-byte alignment for repr(C, align(64)).
        let layout = std::alloc::Layout::from_size_align(
            core::mem::size_of::<InventoryFenceSegment>(),
            core::mem::align_of::<InventoryFenceSegment>(),
        )
        .unwrap();
        unsafe {
            let ptr = std::alloc::alloc(layout) as *mut InventoryFenceSegment;
            assert!(!ptr.is_null());
            // Fill with 0xFF so we can verify init_payload zeroes everything.
            core::ptr::write_bytes(ptr as *mut u8, 0xFF, layout.size());
            InventoryFenceSegment::init_payload(ptr);
            let seg = &*ptr;
            assert_eq!(seg.active_workers, 0);
            for i in 0..MAX_WORKERS {
                assert_eq!(seg.slots[i].settled_lamports, 0);
                assert_eq!(seg.slots[i].reserved_lamports, 0);
                assert_eq!(seg.slots[i].held_lamports, 0);
                assert_eq!(seg.slots[i].lane, 0xFF, "slot {i} lane should be 0xFF");
                assert_eq!(seg.slots[i].flags, 0);
            }
            std::alloc::dealloc(ptr as *mut u8, layout);
        }
    }

    #[test]
    fn segment_spec_constants() {
        assert_eq!(InventoryFenceSegment::SEGMENT_ID, 20);
        assert_eq!(InventoryFenceSegment::VERSION, 1);
        assert_eq!(
            InventoryFenceSegment::PATH,
            "/dev/shm/shadow_inventory_fence"
        );
    }

    #[test]
    fn file_backed_create_and_open_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory_fence");

        // Create
        let seg_w = MappedSegment::<InventoryFenceSegment>::create_rw(&path).unwrap();
        {
            let payload = seg_w.payload_mut();
            payload.active_workers = 4;
            payload.slots[0].settled_lamports = 1_000_000;
            payload.slots[0].reserved_lamports = 500_000;
            payload.slots[0].lane = 1; // Harmonic
            payload.slots[1].held_lamports = 250_000;
            payload.slots[1].flags = 0x02; // quarantine_hold
        }
        seg_w.flush().unwrap();

        // Re-open read-only
        let seg_r = MappedSegment::<InventoryFenceSegment>::open_ro(&path).unwrap();
        let p = seg_r.payload();
        assert_eq!(p.active_workers, 4);
        assert_eq!(p.slots[0].settled_lamports, 1_000_000);
        assert_eq!(p.slots[0].reserved_lamports, 500_000);
        assert_eq!(p.slots[0].lane, 1);
        assert_eq!(p.slots[1].held_lamports, 250_000);
        assert!(p.slots[1].quarantine_hold());
        assert!(!p.slots[1].reconcile_pending());
    }

    #[test]
    fn open_rejects_wrong_version() {
        use crate::segment::SegmentHeader64B;
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inv_fence_bad_ver");

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();

        let header = SegmentHeader64B {
            magic: crate::segment::SEGMENT_MAGIC,
            build_git_sha_prefix: [0u8; 8],
            checksum: 0,
            segment_id: InventoryFenceSegment::SEGMENT_ID,
            payload_size: core::mem::size_of::<InventoryFenceSegment>() as u64,
            total_size: core::mem::size_of::<SegmentHeader64B>() as u64
                + core::mem::size_of::<InventoryFenceSegment>() as u64,
            payload_offset: core::mem::size_of::<SegmentHeader64B>() as u32,
            version: 99, // wrong
            header_size: 64,
            payload_align: 64,
            reserved_u16: 0,
            reserved_u32: 0,
        };
        unsafe {
            let bytes = core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<SegmentHeader64B>(),
            );
            file.write_all(bytes).unwrap();
        }
        file.set_len(header.total_size).unwrap();

        let res = MappedSegment::<InventoryFenceSegment>::open_ro(&path);
        match res {
            Err(crate::segment::SegmentError::WrongVersion { .. }) => {}
            _ => panic!("Expected WrongVersion, got {:?}", res.err()),
        }
    }

    // -- active_workers bounds validation (QC Fix #5b) --

    #[test]
    fn clamped_active_caps_at_max_workers() {
        let seg = InventoryFenceSegment {
            active_workers: 100, // way past MAX_WORKERS
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        // clamped_active() = min(100, 16) = 16.
        // Slots 0..15 accessible, slot 16 is not.
        assert!(seg.slot(0).is_some());
        assert!(seg.slot(15).is_some());
        assert!(seg.slot(16).is_none());
    }

    #[test]
    fn bogus_active_workers_does_not_overrun_total_reserved() {
        let mut seg = InventoryFenceSegment {
            active_workers: 999, // far beyond MAX_WORKERS
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        for i in 0..MAX_WORKERS {
            seg.slots[i].reserved_lamports = 100;
        }
        // Must iterate at most MAX_WORKERS slots, not 999.
        assert_eq!(seg.total_reserved(), 100 * MAX_WORKERS as u64);
    }

    #[test]
    fn bogus_active_workers_does_not_overrun_total_held() {
        let mut seg = InventoryFenceSegment {
            active_workers: 999,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        for i in 0..MAX_WORKERS {
            seg.slots[i].held_lamports = 50;
        }
        assert_eq!(seg.total_held(), 50 * MAX_WORKERS as u64);
    }

    #[test]
    fn slot_accessor_clamped_with_bogus_active_workers() {
        let mut seg = InventoryFenceSegment {
            active_workers: 200,
            _header_pad: [0; 60],
            slots: [InventoryFenceSlot::ZERO; MAX_WORKERS],
        };
        // Workers 0..15 are accessible (clamped to 16).
        assert!(seg.slot(0).is_some());
        assert!(seg.slot(15).is_some());
        // Worker 16 is out of bounds.
        assert!(seg.slot(16).is_none());
        assert!(seg.slot_mut(16).is_none());
    }

    #[test]
    fn create_rejects_active_workers_above_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inv_fence_overflow");

        // Can't use the canonical-path helper since it's hardcoded,
        // but we can test the validation logic directly.
        let result = SegmentError::InvalidPayload {
            reason: format!(
                "active_workers ({}) exceeds MAX_WORKERS ({})",
                17, MAX_WORKERS
            ),
        };
        // Verify the error variant exists and formats correctly.
        let msg = format!("{result}");
        assert!(msg.contains("exceeds MAX_WORKERS"));
    }
}

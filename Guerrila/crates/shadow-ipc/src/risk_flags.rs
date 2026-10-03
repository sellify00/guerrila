use core::sync::atomic::Ordering;

// ---------------------------------------------------------------------------
// RiskFlags64B — shared risk state visible to money workers
// ---------------------------------------------------------------------------

/// Minimal shared risk flags segment.
///
/// In production this lives in `/dev/shm` and is written by `shadow-pm`.
/// Workers read `harmonic_quarantined` in O(1) to bypass the Harmonic lane
/// after a 1-Strike Lethal Catch event.
#[repr(C, align(64))]
pub struct RiskFlags64B {
    pub harmonic_quarantined: core::sync::atomic::AtomicU8,
    pub tactic_class_count: core::sync::atomic::AtomicU8,
    pub _pad_u16: u16,
    pub _pad_u32: u32,
    pub harmonic_tactic_mask: core::sync::atomic::AtomicU64,
    _pad: [u8; 48],
}

// SAFETY: All fields are atomic or padding. No interior mutability issues.
unsafe impl Sync for RiskFlags64B {}

impl RiskFlags64B {
    pub fn new() -> Self {
        Self {
            harmonic_quarantined: core::sync::atomic::AtomicU8::new(0),
            tactic_class_count: core::sync::atomic::AtomicU8::new(8), // Assuming 8 classes max as per Guerrila config
            _pad_u16: 0,
            _pad_u32: 0,
            harmonic_tactic_mask: core::sync::atomic::AtomicU64::new(0xFF), // Permit first 8 classes
            _pad: [0u8; 48],
        }
    }

    /// Returns `true` if the Harmonic lane has been quarantined
    /// (1-Strike Lethal Catch tripped).
    pub fn harmonic_quarantined(&self) -> bool {
        self.harmonic_quarantined.load(Ordering::Acquire) != 0
    }

    /// Set (or clear) the quarantine flag. Used by `shadow-pm`.
    pub fn set_harmonic_quarantined(&self, quarantined: bool) {
        self.harmonic_quarantined
            .store(quarantined as u8, Ordering::Release);
    }

    pub fn tactic_class_count(&self) -> Option<u8> {
        let count = self.tactic_class_count.load(Ordering::Acquire);
        if count > 8 {
            None
        } else {
            Some(count)
        }
    }

    pub fn harmonic_tactic_mask(&self) -> u64 {
        self.harmonic_tactic_mask.load(Ordering::Acquire)
    }

    pub fn set_harmonic_tactic_mask(&self, mask: u64) {
        self.harmonic_tactic_mask.store(mask, Ordering::Release);
    }

    pub fn set_tactic_class_count(&self, count: u8) {
        self.tactic_class_count.store(count, Ordering::Release);
    }

    pub fn harmonic_allowed_for_class(&self, class_id: u8) -> bool {
        if class_id >= 8 {
            return false;
        }

        let count = match self.tactic_class_count() {
            Some(c) => c,
            None => return false,
        };

        if class_id >= count {
            return false;
        }

        let mask = self.harmonic_tactic_mask();
        (mask & (1 << class_id)) != 0
    }
}

impl Default for RiskFlags64B {
    fn default() -> Self {
        Self::new()
    }
}

static_assertions::assert_eq_size!(RiskFlags64B, [u8; 64]);

impl crate::segment::SegmentSpec for RiskFlags64B {
    const SEGMENT_ID: u64 = 4;
    const VERSION: u16 = 2; // Bumped to 2 for Guerrila expansion (tactic_class_count, harmonic_tactic_mask)
    const PATH: &'static str = "/dev/shm/shadow_risk_flags";

    fn init_payload(ptr: *mut Self) {
        // Zero-init: harmonic_quarantined = 0 (not quarantined).
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_risk_flags_is_64_bytes_64_aligned() {
        assert_eq!(core::mem::size_of::<RiskFlags64B>(), 64);
        assert_eq!(core::mem::align_of::<RiskFlags64B>(), 64);
    }

    #[test]
    fn risk_flags_default_not_quarantined() {
        let flags = RiskFlags64B::new();
        assert!(!flags.harmonic_quarantined());
    }

    #[test]
    fn risk_flags_set_and_read() {
        let flags = RiskFlags64B::new();
        flags.set_harmonic_quarantined(true);
        assert!(flags.harmonic_quarantined());
        flags.set_harmonic_quarantined(false);
        assert!(!flags.harmonic_quarantined());
    }

    #[test]
    fn new_gates_and_masks_default_values() {
        let flags = RiskFlags64B::new();
        assert_eq!(flags.harmonic_tactic_mask(), 0xFF);
        assert_eq!(flags.tactic_class_count(), Some(8));

        // Assert true for indices < 8
        for i in 0..8 {
            assert!(flags.harmonic_allowed_for_class(i));
        }

        // Assert false for indices >= 8
        assert!(!flags.harmonic_allowed_for_class(8));
    }

    #[test]
    fn harmonic_allowed_for_class_tests() {
        let flags = RiskFlags64B::new();
        flags.set_harmonic_tactic_mask(0x01); // Only class 0 allowed
        assert!(flags.harmonic_allowed_for_class(0));
        assert!(!flags.harmonic_allowed_for_class(1));

        // Test fails closed on invalid count
        flags.set_tactic_class_count(9);
        assert!(!flags.harmonic_allowed_for_class(0));
    }

    #[test]
    fn risk_flags_v2_layout_is_64_aligned() {
        assert_eq!(core::mem::size_of::<RiskFlags64B>(), 64);
        assert_eq!(core::mem::align_of::<RiskFlags64B>(), 64);
    }

    #[test]
    fn harmonic_allowed_for_class_respects_mask_bits() {
        let flags = RiskFlags64B::new();
        flags.set_harmonic_tactic_mask(0b0000_0101); // Classes 0 and 2 allowed
        flags.set_tactic_class_count(8);
        assert!(flags.harmonic_allowed_for_class(0));
        assert!(!flags.harmonic_allowed_for_class(1));
        assert!(flags.harmonic_allowed_for_class(2));
        assert!(!flags.harmonic_allowed_for_class(3));
    }

    #[test]
    fn harmonic_allowed_for_class_fails_closed_on_out_of_range_class() {
        let flags = RiskFlags64B::new();
        flags.set_harmonic_tactic_mask(0xFF); // All classes allowed
        flags.set_tactic_class_count(4);
        assert!(flags.harmonic_allowed_for_class(3));
        assert!(!flags.harmonic_allowed_for_class(4)); // Fails closed due to count
        assert!(!flags.harmonic_allowed_for_class(8)); // Fails closed due to hard limit
    }

    #[test]
    fn risk_flags_v2_roundtrip_across_two_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk_flags");
        let seg1 = crate::segment::MappedSegment::<RiskFlags64B>::create_rw(&path).unwrap();

        seg1.payload().set_harmonic_quarantined(true);
        seg1.payload().set_harmonic_tactic_mask(0xABCD);
        seg1.flush().unwrap();

        let seg2 = crate::segment::MappedSegment::<RiskFlags64B>::open_ro(&path).unwrap();
        assert!(seg2.payload().harmonic_quarantined());
        assert_eq!(seg2.payload().harmonic_tactic_mask(), 0xABCD);
    }

    #[test]
    fn open_risk_flags_rejects_old_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk_flags");

        // Create an old version manually
        use crate::segment::{SegmentHeader64B, SegmentSpec};
        use std::fs::OpenOptions;
        use std::io::Write;

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
            segment_id: RiskFlags64B::SEGMENT_ID,
            payload_size: core::mem::size_of::<RiskFlags64B>() as u64,
            total_size: core::mem::size_of::<SegmentHeader64B>() as u64
                + core::mem::size_of::<RiskFlags64B>() as u64,
            payload_offset: core::mem::size_of::<SegmentHeader64B>() as u32,
            version: 1, // OLD VERSION
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

        let res = crate::segment::MappedSegment::<RiskFlags64B>::open_ro(&path);
        match res {
            Err(crate::segment::SegmentError::WrongVersion { .. }) => {}
            _ => panic!("Expected WrongVersion error, got {:?}", res.err()),
        }
    }
}

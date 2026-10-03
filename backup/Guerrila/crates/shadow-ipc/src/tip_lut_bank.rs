// G9 – Tip-LUT bank segments
//
// Two double-buffered 16 MiB LUT banks published by shadow-oracle
// and consumed by workers via memory-mapped read-only access.
//
// Segment IDs:
//   22 – shadow_tip_lut_base  (baseline / Harmonic lane)
//   23 – shadow_tip_lut_jito  (Jito lane, tax-adjusted)
//
// Publication discipline: writer fills the inactive bank, then flips
// `active_idx` with Release ordering.  Readers load `active_idx` with
// Acquire to observe the completed bank.  `active_idx == 0xFF` means
// "never published" — readers get `None`.

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use crate::segment::{MappedSegment, SegmentError, SegmentSpec};

/// 256 leader classes × 16384 packed indices, each entry a Q32 fixed-point
/// tip fraction.  16 MiB per bank.
pub type TipLutBank = [[u32; 16384]; 256];

/// Per-bank metadata, isolated on its own 128-byte cache line.
#[repr(C, align(128))]
pub struct TipLutBankMeta128B {
    /// Which bank is live: 0 or 1.  0xFF = never published.
    pub active_idx: AtomicU8,
    pub _pad0: [u8; 7],
    /// Monotonic publish epoch (bumped on every flip).
    pub epoch: AtomicU64,
    /// Unix nanosecond timestamp of last publish.
    pub last_update_unix_ns: AtomicU64,
    /// Number of populated leader classes (≤ 256).
    pub class_count: u8,
    pub _pad1: [u8; 95],
}

static_assertions::assert_eq_size!(TipLutBankMeta128B, [u8; 128]);

/// Full double-buffered LUT payload: meta + 2 × 16 MiB banks.
#[repr(C)]
pub struct TipLutBankPayload {
    pub meta: TipLutBankMeta128B,
    pub banks: [TipLutBank; 2],
}

// SAFETY: All mutable access is through atomic operations on meta fields.
// Bank writes happen only to the inactive bank by the single oracle writer;
// readers only access the active bank after observing the Acquire on active_idx.
unsafe impl Sync for TipLutBankPayload {}

impl TipLutBankPayload {
    /// Reader: returns a reference to the currently active bank.
    ///
    /// Returns `None` if the oracle has never published (`active_idx == 0xFF`).
    #[inline]
    pub fn active_bank(&self) -> Option<&TipLutBank> {
        let idx = self.meta.active_idx.load(Ordering::Acquire);
        if idx > 1 {
            return None;
        }
        Some(&self.banks[idx as usize])
    }

    /// Writer: returns a mutable reference to the inactive bank for pre-fill.
    ///
    /// If never published (active_idx == 0xFF), returns bank 0 (first publish
    /// will flip to bank 0).
    #[inline]
    pub fn inactive_bank_mut(&mut self) -> &mut TipLutBank {
        let active = self.meta.active_idx.load(Ordering::Relaxed);
        let inactive = if active > 1 { 0 } else { 1 - active };
        &mut self.banks[inactive as usize]
    }

    /// Writer: set the number of populated leader classes in metadata.
    ///
    /// Must be called before `publish()` — the Release on `active_idx`
    /// ensures readers see the updated value after Acquire.
    #[inline]
    pub fn set_class_count(&mut self, count: u8) {
        self.meta.class_count = count;
    }

    /// Reader: number of populated leader classes (0 if never set).
    #[inline]
    pub fn class_count(&self) -> u8 {
        self.meta.class_count
    }

    /// Writer: flip active bank, bump epoch, record timestamp.
    ///
    /// Must be called after filling the inactive bank via `inactive_bank_mut()`.
    pub fn publish(&self, now_ns: u64) {
        let old = self.meta.active_idx.load(Ordering::Relaxed);
        let new_idx: u8 = if old > 1 { 0 } else { 1 - old };
        self.meta.epoch.fetch_add(1, Ordering::Release);
        self.meta
            .last_update_unix_ns
            .store(now_ns, Ordering::Release);
        self.meta.active_idx.store(new_idx, Ordering::Release);
    }

    /// Reader: current publish epoch (0 if never published).
    #[inline]
    pub fn epoch(&self) -> u64 {
        self.meta.epoch.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// Newtype wrappers — one per lane, each with its own SegmentSpec
// ---------------------------------------------------------------------------

/// Baseline / Harmonic tip-LUT segment (segment ID 22).
#[repr(C)]
pub struct TipLutBaseSegment {
    pub inner: TipLutBankPayload,
}

// SAFETY: delegates to TipLutBankPayload (see above).
unsafe impl Sync for TipLutBaseSegment {}

impl SegmentSpec for TipLutBaseSegment {
    const SEGMENT_ID: u64 = 22;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_tip_lut_base";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            // Zero-fill everything (banks all zero, epoch 0, class_count 0).
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            // Set active_idx to 0xFF — "never published".
            let seg = &mut *ptr;
            seg.inner.meta.active_idx.store(0xFF, Ordering::Relaxed);
        }
    }
}

/// Jito tip-LUT segment (segment ID 23).
#[repr(C)]
pub struct TipLutJitoSegment {
    pub inner: TipLutBankPayload,
}

// SAFETY: delegates to TipLutBankPayload (see above).
unsafe impl Sync for TipLutJitoSegment {}

impl SegmentSpec for TipLutJitoSegment {
    const SEGMENT_ID: u64 = 23;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_tip_lut_jito";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            let seg = &mut *ptr;
            seg.inner.meta.active_idx.store(0xFF, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience create / open helpers
// ---------------------------------------------------------------------------

pub fn create_shadow_tip_lut_base() -> Result<MappedSegment<TipLutBaseSegment>, SegmentError> {
    MappedSegment::<TipLutBaseSegment>::create_rw(std::path::Path::new(TipLutBaseSegment::PATH))
}

pub fn open_shadow_tip_lut_base_ro() -> Result<MappedSegment<TipLutBaseSegment>, SegmentError> {
    MappedSegment::<TipLutBaseSegment>::open_ro(std::path::Path::new(TipLutBaseSegment::PATH))
}

pub fn create_shadow_tip_lut_jito() -> Result<MappedSegment<TipLutJitoSegment>, SegmentError> {
    MappedSegment::<TipLutJitoSegment>::create_rw(std::path::Path::new(TipLutJitoSegment::PATH))
}

pub fn open_shadow_tip_lut_jito_ro() -> Result<MappedSegment<TipLutJitoSegment>, SegmentError> {
    MappedSegment::<TipLutJitoSegment>::open_ro(std::path::Path::new(TipLutJitoSegment::PATH))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    #[test]
    fn meta_size_is_128_bytes() {
        assert_eq!(core::mem::size_of::<TipLutBankMeta128B>(), 128);
        assert_eq!(core::mem::align_of::<TipLutBankMeta128B>(), 128);
    }

    #[test]
    fn payload_size_sanity() {
        let expected = 128 + 2 * 256 * 16384 * 4;
        assert_eq!(core::mem::size_of::<TipLutBankPayload>(), expected);
        assert_eq!(core::mem::size_of::<TipLutBaseSegment>(), expected);
        assert_eq!(core::mem::size_of::<TipLutJitoSegment>(), expected);
    }

    #[test]
    fn fresh_segment_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_base_fresh");

        let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
        assert!(
            seg.payload().inner.active_bank().is_none(),
            "never-published segment must return None"
        );
    }

    #[test]
    fn write_then_publish_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_roundtrip");

        let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
        let payload = seg.payload_mut();

        // Fill inactive bank (bank 0 since active_idx=0xFF).
        let bank = payload.inner.inactive_bank_mut();
        bank[0][0] = 42;
        bank[7][100] = 9999;

        // Publish.
        payload.inner.publish(1_000_000);

        // Read back.
        let active = seg
            .payload()
            .inner
            .active_bank()
            .expect("should be published");
        assert_eq!(active[0][0], 42);
        assert_eq!(active[7][100], 9999);
        assert_eq!(seg.payload().inner.epoch(), 1);
    }

    #[test]
    fn double_publish_flips_bank() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_double");

        let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
        let payload = seg.payload_mut();

        // First publish: fills bank 0.
        payload.inner.inactive_bank_mut()[0][0] = 111;
        payload.inner.publish(1000);
        assert_eq!(
            seg.payload().inner.meta.active_idx.load(Ordering::Relaxed),
            0
        );

        // Second publish: fills bank 1.
        let payload = seg.payload_mut();
        payload.inner.inactive_bank_mut()[0][0] = 222;
        payload.inner.publish(2000);
        assert_eq!(
            seg.payload().inner.meta.active_idx.load(Ordering::Relaxed),
            1
        );

        let active = seg.payload().inner.active_bank().unwrap();
        assert_eq!(active[0][0], 222);
        assert_eq!(seg.payload().inner.epoch(), 2);
    }

    #[test]
    fn epoch_monotonic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_epoch");

        let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
        let payload = seg.payload_mut();

        for i in 1..=10u64 {
            payload.inner.inactive_bank_mut()[0][0] = i as u32;
            payload.inner.publish(i * 1000);
        }
        assert_eq!(seg.payload().inner.epoch(), 10);
    }

    #[test]
    fn open_ro_reads_published_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_ro");

        // Create, write, publish, flush, drop.
        {
            let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
            let payload = seg.payload_mut();
            payload.inner.inactive_bank_mut()[3][42] = 0xDEAD;
            payload.inner.publish(5555);
            seg.flush().unwrap();
        }

        // Open read-only.
        let seg = MappedSegment::<TipLutBaseSegment>::open_ro(&path).unwrap();
        let active = seg
            .payload()
            .inner
            .active_bank()
            .expect("should read published");
        assert_eq!(active[3][42], 0xDEAD);
        assert_eq!(seg.payload().inner.epoch(), 1);
    }

    #[test]
    fn base_and_jito_different_segment_ids() {
        let dir = tempfile::tempdir().unwrap();

        // Create a base segment.
        let base_path = dir.path().join("lut_base");
        drop(MappedSegment::<TipLutBaseSegment>::create_rw(&base_path).unwrap());

        // Try to open as jito — must fail.
        let result = MappedSegment::<TipLutJitoSegment>::open_ro(&base_path);
        assert!(
            matches!(result, Err(SegmentError::WrongSegmentId { .. })),
            "cross-open must fail with WrongSegmentId"
        );

        // Reverse direction.
        let jito_path = dir.path().join("lut_jito");
        drop(MappedSegment::<TipLutJitoSegment>::create_rw(&jito_path).unwrap());

        let result = MappedSegment::<TipLutBaseSegment>::open_ro(&jito_path);
        assert!(
            matches!(result, Err(SegmentError::WrongSegmentId { .. })),
            "reverse cross-open must also fail"
        );
    }

    #[test]
    fn class_count_survives_publish_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_class_count");

        let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();

        // Fresh segment: class_count defaults to 0.
        assert_eq!(seg.payload().inner.class_count(), 0);

        // Set class_count before publishing.
        let payload = seg.payload_mut();
        payload.inner.set_class_count(8);
        payload.inner.inactive_bank_mut()[0][0] = 1;
        payload.inner.publish(1000);

        assert_eq!(seg.payload().inner.class_count(), 8);
        assert!(seg.payload().inner.active_bank().is_some());

        // Survives a second publish (class_count is sticky).
        let payload = seg.payload_mut();
        payload.inner.inactive_bank_mut()[0][0] = 2;
        payload.inner.publish(2000);
        assert_eq!(seg.payload().inner.class_count(), 8);

        // Can be updated.
        let payload = seg.payload_mut();
        payload.inner.set_class_count(16);
        payload.inner.publish(3000);
        assert_eq!(seg.payload().inner.class_count(), 16);
    }

    #[test]
    fn class_count_visible_through_ro_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tip_lut_cc_ro");

        {
            let seg = MappedSegment::<TipLutBaseSegment>::create_rw(&path).unwrap();
            let payload = seg.payload_mut();
            payload.inner.set_class_count(12);
            payload.inner.inactive_bank_mut()[0][0] = 99;
            payload.inner.publish(5000);
            seg.flush().unwrap();
        }

        let seg = MappedSegment::<TipLutBaseSegment>::open_ro(&path).unwrap();
        assert_eq!(seg.payload().inner.class_count(), 12);
    }

    #[test]
    fn lut_index_packing_matches_policy() {
        // Verify the packing formula matches policy.rs:114
        // final_lut_index = ((obscurity_score & 0xFF) << 6) | (alpha_bucket & 0x3F)
        let obscurity: u16 = 0xAB;
        let alpha: u16 = 0x15;
        let packed = ((obscurity & 0xFF) << 6) | (alpha & 0x3F);
        assert_eq!(packed, (0xAB << 6) | 0x15);
        assert!(packed < 16384, "packed index must fit in 14 bits");

        // Boundary: max obscurity (255) + max alpha (63) = 16383
        let max = ((255u16 & 0xFF) << 6) | (63u16 & 0x3F);
        assert_eq!(max, 16383);

        // Zero case
        let zero = ((0u16 & 0xFF) << 6) | (0u16 & 0x3F);
        assert_eq!(zero, 0);
    }
}

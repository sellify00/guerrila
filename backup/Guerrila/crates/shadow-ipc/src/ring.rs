//! Generic fixed-layout SPSC ring for `T: Copy` payloads.
//!
//! - Power-of-two capacity
//! - Producer cursor isolated on its own cache line
//! - Consumer cursor isolated on its own cache line
//! - Stats isolated on their own cache line
//! - Drop-on-full semantics
//! - No blocking, no spinning, no retries

use std::fs::{File, OpenOptions};
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use memmap2::{Mmap, MmapMut};

use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Wire types — all 64-byte, cache-line isolated
// ---------------------------------------------------------------------------

/// Producer or consumer cursor — one `AtomicU64` sequence counter per cache
/// line. Producer owns `tail`, consumer owns `head`.
#[repr(C, align(64))]
pub struct Cursor64B {
    pub seq: AtomicU64,
    _pad: [u8; 56],
}

static_assertions::assert_eq_size!(Cursor64B, [u8; 64]);

/// Ring metadata — written once at creation, immutable thereafter.
#[repr(C, align(64))]
pub struct RingMeta64B {
    pub capacity: u32,
    pub mask: u32,
    pub elem_size: u32,
    pub slot_align: u16,
    pub version: u16,
    pub data_offset: u32,
    // 4 bytes implicit padding (u64 alignment)
    pub total_size: u64,
    _pad: [u8; 32],
}

static_assertions::assert_eq_size!(RingMeta64B, [u8; 64]);

/// Ring statistics — drop counter and high-water mark.
#[repr(C, align(64))]
pub struct RingStats64B {
    pub drops: AtomicU64,
    pub high_watermark: AtomicU64,
    _pad: [u8; 48],
}

static_assertions::assert_eq_size!(RingStats64B, [u8; 64]);

// ---------------------------------------------------------------------------
// Occupancy snapshot
// ---------------------------------------------------------------------------

/// Non-blocking snapshot of a ring's fill level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingOccupancy {
    pub used: u64,
    pub capacity: u32,
}

impl RingOccupancy {
    /// Returns `true` if occupancy is at or above `percent`% of capacity.
    ///
    /// Uses integer arithmetic: `used * 100 >= capacity * percent`.
    pub fn over_percent(&self, percent: u8) -> bool {
        let percent = percent.min(100) as u64;
        self.used * 100 >= self.capacity as u64 * percent
    }
}

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const HEADER_SIZE: usize = core::mem::size_of::<SegmentHeader64B>(); // 64
const META_OFFSET: usize = HEADER_SIZE; // 64
const PRODUCER_OFFSET: usize = META_OFFSET + core::mem::size_of::<RingMeta64B>(); // 128
const CONSUMER_OFFSET: usize = PRODUCER_OFFSET + core::mem::size_of::<Cursor64B>(); // 192
const STATS_OFFSET: usize = CONSUMER_OFFSET + core::mem::size_of::<Cursor64B>(); // 256
const CONTROL_SIZE: usize = STATS_OFFSET + core::mem::size_of::<RingStats64B>(); // 320

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RingError {
    Io(io::Error),
    BadMagic,
    WrongSegmentId { expected: u64, found: u64 },
    WrongVersion { expected: u16, found: u16 },
    CapacityNotPowerOfTwo(u32),
    WrongElemSize { expected: u32, found: u32 },
    WrongSlotAlign { expected: u16, found: u16 },
    WrongDataOffset { expected: u32, found: u32 },
    WrongTotalSize { expected: u64, found: u64 },
    FileTooSmall,
}

impl From<io::Error> for RingError {
    fn from(e: io::Error) -> Self {
        RingError::Io(e)
    }
}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::BadMagic => write!(f, "bad magic bytes"),
            Self::WrongSegmentId { expected, found } => {
                write!(f, "wrong segment ID: expected {expected}, found {found}")
            }
            Self::WrongVersion { expected, found } => {
                write!(f, "wrong version: expected {expected}, found {found}")
            }
            Self::CapacityNotPowerOfTwo(c) => {
                write!(f, "capacity {c} is not a power of two")
            }
            Self::WrongElemSize { expected, found } => {
                write!(f, "wrong elem_size: expected {expected}, found {found}")
            }
            Self::WrongSlotAlign { expected, found } => {
                write!(f, "wrong slot_align: expected {expected}, found {found}")
            }
            Self::WrongDataOffset { expected, found } => {
                write!(f, "wrong data_offset: expected {expected}, found {found}")
            }
            Self::WrongTotalSize { expected, found } => {
                write!(f, "wrong total_size: expected {expected}, found {found}")
            }
            Self::FileTooSmall => write!(f, "file too small for ring"),
        }
    }
}

impl std::error::Error for RingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_slots_offset<T>() -> usize {
    align_up(CONTROL_SIZE, core::cmp::max(64, core::mem::align_of::<T>()))
}

fn compute_total_size<T>(capacity: u32) -> usize {
    compute_slots_offset::<T>() + (capacity as usize) * core::mem::size_of::<T>()
}

// ---------------------------------------------------------------------------
// MappedSpscRing
// ---------------------------------------------------------------------------

enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

/// A file-backed, memory-mapped SPSC ring buffer for `T: Copy` payloads.
///
/// The ring file layout is:
///
/// | SegmentHeader64B | RingMeta64B | Cursor64B (producer) | Cursor64B (consumer) | RingStats64B | slots... |
pub struct MappedSpscRing<T: Copy> {
    inner: MmapInner,
    slots_offset: usize,
    capacity: u32,
    mask: u32,
    _phantom: PhantomData<T>,
}

impl<T: Copy> MappedSpscRing<T> {
    // -- Construction --------------------------------------------------------

    /// Create a new ring file, write header + meta, zero cursors/stats.
    pub fn create_rw(
        path: &Path,
        segment_id: u64,
        version: u16,
        capacity: u32,
    ) -> Result<Self, RingError> {
        if capacity == 0 || !capacity.is_power_of_two() {
            return Err(RingError::CapacityNotPowerOfTwo(capacity));
        }

        let slots_offset = compute_slots_offset::<T>();
        let total_size = compute_total_size::<T>(capacity);
        let mask = capacity - 1;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_size as u64)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Write segment header.
        let header = SegmentHeader64B {
            magic: SEGMENT_MAGIC,
            build_git_sha_prefix: [0; 8],
            checksum: 0,
            segment_id,
            payload_size: (total_size - HEADER_SIZE) as u64,
            total_size: total_size as u64,
            payload_offset: HEADER_SIZE as u32,
            version,
            header_size: HEADER_SIZE as u16,
            payload_align: core::cmp::max(64, core::mem::align_of::<T>() as u16),
            reserved_u16: 0,
            reserved_u32: 0,
        };
        unsafe {
            core::ptr::write(mmap.as_mut_ptr() as *mut SegmentHeader64B, header);
        }

        // Write ring meta.
        let meta = RingMeta64B {
            capacity,
            mask,
            elem_size: core::mem::size_of::<T>() as u32,
            slot_align: core::mem::align_of::<T>() as u16,
            version,
            data_offset: slots_offset as u32,
            total_size: total_size as u64,
            _pad: [0; 32],
        };
        unsafe {
            core::ptr::write(mmap.as_mut_ptr().add(META_OFFSET) as *mut RingMeta64B, meta);
        }

        // Cursors and stats are zero from file creation (empty ring, no drops).

        mmap.flush()?;

        Ok(Self {
            inner: MmapInner::ReadWrite(mmap),
            slots_offset,
            capacity,
            mask,
            _phantom: PhantomData,
        })
    }

    /// Open an existing ring file read-write and validate header + meta.
    pub fn open_rw(path: &Path, segment_id: u64, version: u16) -> Result<Self, RingError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let base = mmap.as_ptr();
        let len = mmap.len();
        Self::validate_and_build(MmapInner::ReadWrite(mmap), base, len, segment_id, version)
    }

    /// Open an existing ring file read-only and validate header + meta.
    pub fn open_ro(path: &Path, segment_id: u64, version: u16) -> Result<Self, RingError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let base = mmap.as_ptr();
        let len = mmap.len();
        Self::validate_and_build(MmapInner::ReadOnly(mmap), base, len, segment_id, version)
    }

    fn validate_and_build(
        inner: MmapInner,
        base: *const u8,
        len: usize,
        segment_id: u64,
        version: u16,
    ) -> Result<Self, RingError> {
        if len < CONTROL_SIZE {
            return Err(RingError::FileTooSmall);
        }

        // — Validate segment header —
        let header = unsafe { &*(base as *const SegmentHeader64B) };
        if header.magic != SEGMENT_MAGIC {
            return Err(RingError::BadMagic);
        }
        if header.segment_id != segment_id {
            return Err(RingError::WrongSegmentId {
                expected: segment_id,
                found: header.segment_id,
            });
        }
        if header.version != version {
            return Err(RingError::WrongVersion {
                expected: version,
                found: header.version,
            });
        }

        // — Validate ring meta —
        let meta = unsafe { &*(base.add(META_OFFSET) as *const RingMeta64B) };

        if meta.capacity == 0 || !meta.capacity.is_power_of_two() {
            return Err(RingError::CapacityNotPowerOfTwo(meta.capacity));
        }

        let expected_elem_size = core::mem::size_of::<T>() as u32;
        if meta.elem_size != expected_elem_size {
            return Err(RingError::WrongElemSize {
                expected: expected_elem_size,
                found: meta.elem_size,
            });
        }

        let expected_slot_align = core::mem::align_of::<T>() as u16;
        if meta.slot_align != expected_slot_align {
            return Err(RingError::WrongSlotAlign {
                expected: expected_slot_align,
                found: meta.slot_align,
            });
        }

        let slots_offset = compute_slots_offset::<T>();
        if meta.data_offset != slots_offset as u32 {
            return Err(RingError::WrongDataOffset {
                expected: slots_offset as u32,
                found: meta.data_offset,
            });
        }

        let expected_total = compute_total_size::<T>(meta.capacity);
        if meta.total_size != expected_total as u64 {
            return Err(RingError::WrongTotalSize {
                expected: expected_total as u64,
                found: meta.total_size,
            });
        }

        if len < expected_total {
            return Err(RingError::FileTooSmall);
        }

        Ok(Self {
            inner,
            slots_offset,
            capacity: meta.capacity,
            mask: meta.mask,
            _phantom: PhantomData,
        })
    }

    // -- Accessors -----------------------------------------------------------

    fn base_ptr(&self) -> *const u8 {
        match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        }
    }

    fn producer(&self) -> &Cursor64B {
        unsafe { &*(self.base_ptr().add(PRODUCER_OFFSET) as *const Cursor64B) }
    }

    fn consumer(&self) -> &Cursor64B {
        unsafe { &*(self.base_ptr().add(CONSUMER_OFFSET) as *const Cursor64B) }
    }

    /// Access to the ring statistics (drop count, high-water mark).
    pub fn stats(&self) -> &RingStats64B {
        unsafe { &*(self.base_ptr().add(STATS_OFFSET) as *const RingStats64B) }
    }

    fn slot_ptr(&self, idx: usize) -> *const T {
        unsafe {
            self.base_ptr()
                .add(self.slots_offset + idx * core::mem::size_of::<T>()) as *const T
        }
    }

    // -- Occupancy ------------------------------------------------------------

    /// Non-blocking snapshot of ring fill level.
    pub fn occupancy(&self) -> RingOccupancy {
        let tail = self.producer().seq.load(Ordering::Acquire);
        let head = self.consumer().seq.load(Ordering::Acquire);
        RingOccupancy {
            used: tail.wrapping_sub(head),
            capacity: self.capacity,
        }
    }

    /// Returns `true` if the ring is at or above `percent`% full.
    ///
    /// `percent` is clamped to `[0, 100]`. A value of 0 always returns `true`
    /// (any occupancy is "above 0%"), 100 returns `true` only when completely
    /// full.
    pub fn over_threshold(&self, percent: u8) -> bool {
        let occ = self.occupancy();
        occ.over_percent(percent)
    }

    // -- SPSC operations -----------------------------------------------------

    /// Push an item onto the ring. Returns `true` on success, `false` if full
    /// (the item is dropped and the drop counter is incremented).
    ///
    /// # Panics
    ///
    /// Panics if called on a read-only mapping.
    pub fn push(&self, item: &T) -> bool {
        assert!(
            matches!(self.inner, MmapInner::ReadWrite(_)),
            "push requires a read-write mapping"
        );

        let producer = self.producer();
        let consumer = self.consumer();
        let stats = self.stats();

        let tail = producer.seq.load(Ordering::Relaxed);
        let head = consumer.seq.load(Ordering::Acquire);
        let used = tail.wrapping_sub(head);

        if used >= self.capacity as u64 {
            stats.drops.fetch_add(1, Ordering::Relaxed);
            stats.high_watermark.fetch_max(used, Ordering::Relaxed);
            return false;
        }

        let slot_idx = (tail & self.mask as u64) as usize;
        unsafe {
            let dst = self.slot_ptr(slot_idx) as *mut T;
            core::ptr::copy_nonoverlapping(item as *const T, dst, 1);
        }

        let new_used = used + 1;
        stats.high_watermark.fetch_max(new_used, Ordering::Relaxed);
        producer.seq.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Pop an item from the ring. Returns `None` if the ring is empty.
    ///
    /// # Panics
    ///
    /// Panics if called on a read-only mapping.
    pub fn pop(&self) -> Option<T> {
        assert!(
            matches!(self.inner, MmapInner::ReadWrite(_)),
            "pop requires a read-write mapping"
        );

        let consumer = self.consumer();
        let producer = self.producer();

        let head = consumer.seq.load(Ordering::Relaxed);
        let tail = producer.seq.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let slot_idx = (head & self.mask as u64) as usize;
        let item = unsafe { core::ptr::read(self.slot_ptr(slot_idx)) };

        consumer.seq.store(head.wrapping_add(1), Ordering::Release);
        Some(item)
    }

    /// Current number of items in the ring (snapshot, may race).
    pub fn len(&self) -> u64 {
        let tail = self.producer().seq.load(Ordering::Acquire);
        let head = self.consumer().seq.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Returns `true` if the ring contains no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ring capacity (number of slots).
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Flush dirty pages to the backing file.
    pub fn flush(&self) -> Result<(), RingError> {
        match &self.inner {
            MmapInner::ReadWrite(m) => m.flush().map_err(RingError::Io),
            MmapInner::ReadOnly(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Layout tests
    // -----------------------------------------------------------------------

    #[test]
    fn layout_cursor64_is_64_bytes() {
        assert_eq!(core::mem::size_of::<Cursor64B>(), 64);
        assert_eq!(core::mem::align_of::<Cursor64B>(), 64);
    }

    #[test]
    fn layout_ring_meta64_is_64_bytes() {
        assert_eq!(core::mem::size_of::<RingMeta64B>(), 64);
        assert_eq!(core::mem::align_of::<RingMeta64B>(), 64);
    }

    #[test]
    fn layout_ring_stats64_is_64_bytes() {
        assert_eq!(core::mem::size_of::<RingStats64B>(), 64);
        assert_eq!(core::mem::align_of::<RingStats64B>(), 64);
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    struct TestItem {
        value: u64,
        data: [u8; 56],
    }

    impl TestItem {
        fn new(v: u64) -> Self {
            let mut data = [0u8; 56];
            data[0..8].copy_from_slice(&v.to_le_bytes());
            Self { value: v, data }
        }
    }

    const TEST_SEG_ID: u64 = 99;
    const TEST_VERSION: u16 = 1;

    fn create_temp_ring<T: Copy>(capacity: u32) -> (tempfile::TempDir, MappedSpscRing<T>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_ring");
        let ring =
            MappedSpscRing::<T>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, capacity).unwrap();
        (dir, ring)
    }

    // -----------------------------------------------------------------------
    // Ring behavior tests
    // -----------------------------------------------------------------------

    #[test]
    fn spsc_ring_empty_on_create() {
        let (_dir, ring) = create_temp_ring::<TestItem>(16);
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn spsc_ring_push_pop_roundtrip() {
        let (_dir, ring) = create_temp_ring::<TestItem>(16);
        let item = TestItem::new(42);
        assert!(ring.push(&item));
        assert_eq!(ring.len(), 1);
        let popped = ring.pop().unwrap();
        assert_eq!(popped, item);
        assert!(ring.is_empty());
    }

    #[test]
    fn spsc_ring_wraparound_preserves_fifo_order() {
        let (_dir, ring) = create_temp_ring::<TestItem>(4);

        // Fill, drain, fill again to force index wraparound.
        for i in 0..4u64 {
            assert!(ring.push(&TestItem::new(i)));
        }
        for i in 0..4u64 {
            assert_eq!(ring.pop().unwrap().value, i);
        }

        // Second pass wraps around the internal slot array.
        for i in 100..104u64 {
            assert!(ring.push(&TestItem::new(i)));
        }
        for i in 100..104u64 {
            assert_eq!(ring.pop().unwrap().value, i);
        }
    }

    #[test]
    fn spsc_ring_drop_on_full_returns_false() {
        let (_dir, ring) = create_temp_ring::<TestItem>(4);
        for i in 0..4u64 {
            assert!(ring.push(&TestItem::new(i)));
        }
        // Ring is full now.
        assert!(!ring.push(&TestItem::new(999)));
    }

    #[test]
    fn spsc_ring_drop_counter_increments() {
        let (_dir, ring) = create_temp_ring::<TestItem>(4);
        for i in 0..4u64 {
            assert!(ring.push(&TestItem::new(i)));
        }
        assert_eq!(ring.stats().drops.load(Ordering::Relaxed), 0);

        // Drop 3 items.
        assert!(!ring.push(&TestItem::new(100)));
        assert!(!ring.push(&TestItem::new(101)));
        assert!(!ring.push(&TestItem::new(102)));
        assert_eq!(ring.stats().drops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn spsc_ring_high_watermark_tracks_peak() {
        let (_dir, ring) = create_temp_ring::<TestItem>(8);
        assert_eq!(ring.stats().high_watermark.load(Ordering::Relaxed), 0);

        // Push 3: watermark rises to 3.
        for i in 0..3u64 {
            ring.push(&TestItem::new(i));
        }
        assert_eq!(ring.stats().high_watermark.load(Ordering::Relaxed), 3);

        // Pop 2: watermark stays at 3.
        ring.pop();
        ring.pop();
        assert_eq!(ring.stats().high_watermark.load(Ordering::Relaxed), 3);

        // Push 5 more (currently 1 in ring → peaks at 6): watermark = 6.
        for i in 10..15u64 {
            ring.push(&TestItem::new(i));
        }
        assert_eq!(ring.stats().high_watermark.load(Ordering::Relaxed), 6);
    }

    // -----------------------------------------------------------------------
    // File-backed validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn ring_create_and_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip_ring");

        {
            let ring = MappedSpscRing::<TestItem>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, 16)
                .unwrap();
            ring.push(&TestItem::new(1));
            ring.push(&TestItem::new(2));
            ring.flush().unwrap();
        }

        let ring2 = MappedSpscRing::<TestItem>::open_rw(&path, TEST_SEG_ID, TEST_VERSION).unwrap();
        assert_eq!(ring2.len(), 2);
        assert_eq!(ring2.pop().unwrap().value, 1);
        assert_eq!(ring2.pop().unwrap().value, 2);
    }

    #[test]
    fn open_rejects_wrong_elem_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong_elem");

        MappedSpscRing::<TestItem>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, 16).unwrap();

        // TestItem is 64 bytes; [u8; 32] is 32 bytes — elem_size mismatch.
        let result = MappedSpscRing::<[u8; 32]>::open_rw(&path, TEST_SEG_ID, TEST_VERSION);
        assert!(matches!(result, Err(RingError::WrongElemSize { .. })));
    }

    #[test]
    fn open_rejects_wrong_capacity_non_power_of_two() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_capacity");

        {
            let ring = MappedSpscRing::<TestItem>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, 16)
                .unwrap();
            ring.flush().unwrap();
        }

        // Corrupt capacity field: RingMeta64B.capacity at META_OFFSET + 0.
        let mut data = std::fs::read(&path).unwrap();
        data[META_OFFSET..META_OFFSET + 4].copy_from_slice(&3u32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let result = MappedSpscRing::<TestItem>::open_rw(&path, TEST_SEG_ID, TEST_VERSION);
        assert!(matches!(result, Err(RingError::CapacityNotPowerOfTwo(3))));
    }

    #[test]
    fn open_rejects_wrong_data_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_offset");

        {
            let ring = MappedSpscRing::<TestItem>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, 16)
                .unwrap();
            ring.flush().unwrap();
        }

        // Corrupt data_offset: at META_OFFSET + 16.
        let mut data = std::fs::read(&path).unwrap();
        let offset = META_OFFSET + 16;
        data[offset..offset + 4].copy_from_slice(&999u32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let result = MappedSpscRing::<TestItem>::open_rw(&path, TEST_SEG_ID, TEST_VERSION);
        assert!(matches!(result, Err(RingError::WrongDataOffset { .. })));
    }

    #[test]
    fn open_rejects_wrong_total_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_total");

        {
            let ring = MappedSpscRing::<TestItem>::create_rw(&path, TEST_SEG_ID, TEST_VERSION, 16)
                .unwrap();
            ring.flush().unwrap();
        }

        // Corrupt total_size: at META_OFFSET + 24.
        let mut data = std::fs::read(&path).unwrap();
        let offset = META_OFFSET + 24;
        data[offset..offset + 8].copy_from_slice(&12345u64.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let result = MappedSpscRing::<TestItem>::open_rw(&path, TEST_SEG_ID, TEST_VERSION);
        assert!(matches!(result, Err(RingError::WrongTotalSize { .. })));
    }

    // -----------------------------------------------------------------------
    // Occupancy tests
    // -----------------------------------------------------------------------

    #[test]
    fn occupancy_empty_ring() {
        let (_dir, ring) = create_temp_ring::<TestItem>(16);
        let occ = ring.occupancy();
        assert_eq!(occ.used, 0);
        assert_eq!(occ.capacity, 16);
        assert!(!ring.over_threshold(80));
        assert!(!ring.over_threshold(1));
    }

    #[test]
    fn occupancy_tracks_fill_level() {
        let (_dir, ring) = create_temp_ring::<TestItem>(16);
        for i in 0..10u64 {
            ring.push(&TestItem::new(i));
        }
        let occ = ring.occupancy();
        assert_eq!(occ.used, 10);
        assert_eq!(occ.capacity, 16);
    }

    #[test]
    fn over_threshold_80_percent() {
        let (_dir, ring) = create_temp_ring::<TestItem>(16);

        // 12/16 = 75% → not over 80%
        for i in 0..12u64 {
            ring.push(&TestItem::new(i));
        }
        assert!(!ring.over_threshold(80));

        // 13/16 = 81.25% → over 80%
        ring.push(&TestItem::new(100));
        assert!(ring.over_threshold(80));
    }

    #[test]
    fn over_threshold_boundary_exact() {
        let (_dir, ring) = create_temp_ring::<TestItem>(4);
        // 0/4 = 0% — over_percent(0) is true (anything >= 0%)
        assert!(ring.over_threshold(0));
        assert!(!ring.over_threshold(1));

        // Fill to 100%: 4/4
        for i in 0..4u64 {
            ring.push(&TestItem::new(i));
        }
        assert!(ring.over_threshold(100));
    }

    #[test]
    fn ring_occupancy_over_percent_unit() {
        // Pure unit tests on RingOccupancy without a ring.
        let occ = RingOccupancy {
            used: 8,
            capacity: 10,
        };
        assert!(occ.over_percent(80)); // 8*100 >= 10*80 → 800 >= 800
        assert!(!occ.over_percent(81)); // 800 < 810

        let empty = RingOccupancy {
            used: 0,
            capacity: 16,
        };
        assert!(empty.over_percent(0));
        assert!(!empty.over_percent(1));

        let full = RingOccupancy {
            used: 16,
            capacity: 16,
        };
        assert!(full.over_percent(100));
    }

    #[test]
    fn over_threshold_clamps_above_100() {
        let occ = RingOccupancy {
            used: 16,
            capacity: 16,
        };
        // percent=255 is clamped to 100
        assert!(occ.over_percent(255));

        let half = RingOccupancy {
            used: 8,
            capacity: 16,
        };
        assert!(!half.over_percent(255));
    }
}

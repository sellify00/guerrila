//! `RawArchiveRecord` contract and raw-archive ring helpers.
//!
//! The AF_XDP ingress proxy writes one `RawArchiveRecord` per processed shred
//! into `/dev/shm/shadow_raw_archive` for the control plane (Rosetta Stone
//! calibrator / Flight Data Recorder).
//!
//! Records store **UMEM offsets** (not virtual pointers) so that the control
//! plane can reconstruct a local pointer from its own shared UMEM mapping.
//!
//! The archive ring is drop-on-full — ingress MUST never block or stall.

use crate::ring::{MappedSpscRing, RingError};
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::sync::atomic::{AtomicU64, Ordering};
use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Segment identity
// ---------------------------------------------------------------------------

pub const RAW_ARCHIVE_RING_SEGMENT_ID: u64 = 11;
pub const RAW_ARCHIVE_RING_VERSION: u16 = 2;

/// Base path for the raw archive ring segment.
pub const RAW_ARCHIVE_RING_PATH_BASE: &str = "/dev/shm/shadow_raw_archive";

/// Default archive ring capacity (power-of-two).
pub const RAW_ARCHIVE_RING_DEFAULT_CAPACITY: u32 = 65536;

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Canonical path for the shared raw archive segment.
///
/// Returns `/dev/shm/shadow_raw_archive` — a single multi-lane segment
/// file shared by all AF_XDP producers.
pub fn raw_archive_segment_path() -> PathBuf {
    PathBuf::from(RAW_ARCHIVE_RING_PATH_BASE)
}

// ---------------------------------------------------------------------------
// RawArchiveRecord — the frozen contract
// ---------------------------------------------------------------------------

/// A single shred observation recorded by the AF_XDP ingress proxy for the
/// control plane's Flight Data Recorder.
///
/// **Layout contract (frozen):**
/// - 64 bytes, 64-byte aligned, `#[repr(C)]`.
/// - No heap, no pointers — only a UMEM offset for the raw shred payload.
/// - The control plane reconstructs a local pointer via:
///   `local_ptr = control_umem_base + umem_offset_bytes`
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct RawArchiveRecord {
    /// Slot from the Turbine shred header.
    pub slot: u64,
    /// Monotonic publication generation for the producing archive lane.
    pub generation: u64,
    /// RX hardware timestamp captured at archive publication time.
    pub rx_timestamp_ns: u64,
    /// 64-bit fingerprint of the archived frame bytes.
    pub frame_hash64: u64,
    /// Byte offset into the shared UMEM mapping (fits 4 GiB UMEM in u32).
    pub umem_offset_bytes: u32,
    /// Shred index within the slot.
    pub shred_index: u32,
    /// Payload length in bytes.
    pub len: u16,
    /// `1` if this is a data shred, `0` if coding shred.
    pub is_data_shred: u8,
    /// Which ingress producer wrote this record.
    pub producer_id: u8,
    /// Reserved — must be zero.
    pub _reserved: [u8; 20],
}

// Compile-time layout assertions.
static_assertions::assert_eq_size!(RawArchiveRecord, [u8; 64]);
static_assertions::const_assert_eq!(core::mem::align_of::<RawArchiveRecord>(), 64);

impl RawArchiveRecord {
    /// Construct a new archive record.
    pub fn new(
        slot: u64,
        umem_offset_bytes: u32,
        shred_index: u32,
        len: u16,
        is_data_shred: bool,
        producer_id: u8,
    ) -> Self {
        Self::new_guarded(
            slot,
            0,
            0,
            0,
            umem_offset_bytes,
            shred_index,
            len,
            is_data_shred,
            producer_id,
        )
    }

    /// Construct a guarded archive record that carries the immutable
    /// overwrite-detection metadata used by G11 FDR consumers.
    pub fn new_guarded(
        slot: u64,
        generation: u64,
        rx_timestamp_ns: u64,
        frame_hash64: u64,
        umem_offset_bytes: u32,
        shred_index: u32,
        len: u16,
        is_data_shred: bool,
        producer_id: u8,
    ) -> Self {
        Self {
            slot,
            generation,
            rx_timestamp_ns,
            frame_hash64,
            umem_offset_bytes,
            shred_index,
            len,
            is_data_shred: if is_data_shred { 1 } else { 0 },
            producer_id,
            _reserved: [0; 20],
        }
    }
}

/// Stable 64-bit frame fingerprint for overwrite-guard validation.
pub fn frame_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// Concrete archive-ring helpers
// ---------------------------------------------------------------------------

/// Create a new raw archive ring at the given path.
pub fn create_raw_archive_ring(
    path: &Path,
    capacity: u32,
) -> Result<MappedSpscRing<RawArchiveRecord>, RingError> {
    MappedSpscRing::create_rw(
        path,
        RAW_ARCHIVE_RING_SEGMENT_ID,
        RAW_ARCHIVE_RING_VERSION,
        capacity,
    )
}

/// Open an existing raw archive ring for reading and writing.
pub fn open_raw_archive_ring_rw(
    path: &Path,
) -> Result<MappedSpscRing<RawArchiveRecord>, RingError> {
    MappedSpscRing::open_rw(path, RAW_ARCHIVE_RING_SEGMENT_ID, RAW_ARCHIVE_RING_VERSION)
}

/// Open an existing raw archive ring read-only.
pub fn open_raw_archive_ring_ro(
    path: &Path,
) -> Result<MappedSpscRing<RawArchiveRecord>, RingError> {
    MappedSpscRing::open_ro(path, RAW_ARCHIVE_RING_SEGMENT_ID, RAW_ARCHIVE_RING_VERSION)
}

// ---------------------------------------------------------------------------
// Multi-lane raw archive segment
// ---------------------------------------------------------------------------

/// Per-lane control block size: producer cursor (64) + consumer cursor (64) +
/// stats (64) = 192 bytes, three cache lines, zero false sharing.
const LANE_CONTROL_SIZE: usize = 192;
const ARCHIVE_META_OFFSET: usize = 64; // right after SegmentHeader64B
const ARCHIVE_LANES_OFFSET: usize = 128; // right after meta

/// Metadata block embedded at offset 64 in the archive segment file.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct ArchiveMeta64B {
    pub producer_count: u32,
    pub lane_capacity: u32,
    pub lane_mask: u32,
    pub record_size: u32,
    pub cursors_offset: u32,
    pub slots_offset: u32,
    pub total_size: u64,
    pub _pad: [u8; 32],
}

static_assertions::assert_eq_size!(ArchiveMeta64B, [u8; 64]);

/// A single segment file containing one SPSC lane per ingress producer.
///
/// Layout:
/// ```text
/// SegmentHeader64B                            @ 0
/// ArchiveMeta64B                              @ 64
/// [ProducerCursor | ConsumerCursor | Stats]   @ 128  (per lane, 192B each)
/// ... (aligned padding) ...
/// Lane 0 slots: [RawArchiveRecord; capacity]  @ slots_offset
/// Lane 1 slots ...
/// ```
pub struct RawArchiveSegment {
    inner: MmapMut,
    producer_count: u32,
    lane_capacity: u32,
    lane_mask: u32,
    slots_offset: usize,
}

// SAFETY: RawArchiveSegment uses atomic load/store for all lane control
// (producer_seq, consumer_seq) and each lane's record slots are disjoint.
// Concurrent push(lane_a, ..) and push(lane_b, ..) access non-overlapping
// memory, and push/pop on the same lane are synchronized via atomics.
// The underlying MmapMut is sound to share because we never resize or
// remap while references exist.
unsafe impl Sync for RawArchiveSegment {}

impl RawArchiveSegment {
    /// Create a new multi-lane archive segment file.
    pub fn create(path: &Path, producer_count: u32, lane_capacity: u32) -> Result<Self, RingError> {
        if lane_capacity == 0 || !lane_capacity.is_power_of_two() {
            return Err(RingError::CapacityNotPowerOfTwo(lane_capacity));
        }

        let lane_mask = lane_capacity - 1;
        let control_end = ARCHIVE_LANES_OFFSET + (producer_count as usize) * LANE_CONTROL_SIZE;
        let slots_offset = align_up(control_end, 64);
        let record_size = core::mem::size_of::<RawArchiveRecord>();
        let total_size =
            slots_offset + (producer_count as usize) * (lane_capacity as usize) * record_size;

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
            segment_id: RAW_ARCHIVE_RING_SEGMENT_ID,
            payload_size: (total_size - 64) as u64,
            total_size: total_size as u64,
            payload_offset: ARCHIVE_META_OFFSET as u32,
            version: RAW_ARCHIVE_RING_VERSION,
            header_size: 64,
            payload_align: 64,
            reserved_u16: 0,
            reserved_u32: 0,
        };
        unsafe {
            core::ptr::write(mmap.as_mut_ptr() as *mut SegmentHeader64B, header);
        }

        // Write archive meta.
        let meta = ArchiveMeta64B {
            producer_count,
            lane_capacity,
            lane_mask,
            record_size: record_size as u32,
            cursors_offset: ARCHIVE_LANES_OFFSET as u32,
            slots_offset: slots_offset as u32,
            total_size: total_size as u64,
            _pad: [0; 32],
        };
        unsafe {
            core::ptr::write(
                mmap.as_mut_ptr().add(ARCHIVE_META_OFFSET) as *mut ArchiveMeta64B,
                meta,
            );
        }

        // Cursors, stats, and slots are zero from file creation.
        mmap.flush()?;

        Ok(Self {
            inner: mmap,
            producer_count,
            lane_capacity,
            lane_mask,
            slots_offset,
        })
    }

    /// Open an existing multi-lane archive segment file.
    pub fn open(path: &Path) -> Result<Self, RingError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        if mmap.len() < ARCHIVE_LANES_OFFSET {
            return Err(RingError::FileTooSmall);
        }

        // Validate segment header.
        let header = unsafe { &*(mmap.as_ptr() as *const SegmentHeader64B) };
        if header.magic != SEGMENT_MAGIC {
            return Err(RingError::BadMagic);
        }
        if header.segment_id != RAW_ARCHIVE_RING_SEGMENT_ID {
            return Err(RingError::WrongSegmentId {
                expected: RAW_ARCHIVE_RING_SEGMENT_ID,
                found: header.segment_id,
            });
        }
        if header.version != RAW_ARCHIVE_RING_VERSION {
            return Err(RingError::WrongVersion {
                expected: RAW_ARCHIVE_RING_VERSION,
                found: header.version,
            });
        }

        let meta = unsafe { &*(mmap.as_ptr().add(ARCHIVE_META_OFFSET) as *const ArchiveMeta64B) };

        Ok(Self {
            producer_count: meta.producer_count,
            lane_capacity: meta.lane_capacity,
            lane_mask: meta.lane_mask,
            slots_offset: meta.slots_offset as usize,
            inner: mmap,
        })
    }

    // -- Accessors -----------------------------------------------------------

    pub fn producer_count(&self) -> u32 {
        self.producer_count
    }

    pub fn lane_capacity(&self) -> u32 {
        self.lane_capacity
    }

    /// Monotonic publication generation for the next record written to `lane`.
    pub fn next_generation(&self, lane: usize) -> u64 {
        assert!(lane < self.producer_count as usize);
        self.producer_seq(lane)
            .load(Ordering::Acquire)
            .wrapping_add(1)
    }

    // -- Per-lane cursor / stats pointers ------------------------------------

    fn producer_seq(&self, lane: usize) -> &AtomicU64 {
        let off = ARCHIVE_LANES_OFFSET + lane * LANE_CONTROL_SIZE;
        unsafe { &*(self.inner.as_ptr().add(off) as *const AtomicU64) }
    }

    fn consumer_seq(&self, lane: usize) -> &AtomicU64 {
        let off = ARCHIVE_LANES_OFFSET + lane * LANE_CONTROL_SIZE + 64;
        unsafe { &*(self.inner.as_ptr().add(off) as *const AtomicU64) }
    }

    fn lane_drops(&self, lane: usize) -> &AtomicU64 {
        let off = ARCHIVE_LANES_OFFSET + lane * LANE_CONTROL_SIZE + 128;
        unsafe { &*(self.inner.as_ptr().add(off) as *const AtomicU64) }
    }

    fn slot_ptr(&self, lane: usize, idx: u32) -> *const RawArchiveRecord {
        let record_size = core::mem::size_of::<RawArchiveRecord>();
        let lane_base = self.slots_offset + lane * (self.lane_capacity as usize) * record_size;
        let slot_off = lane_base + (idx as usize) * record_size;
        unsafe { self.inner.as_ptr().add(slot_off) as *const RawArchiveRecord }
    }

    // -- SPSC operations (per lane) ------------------------------------------

    /// Push a record into the given lane. Returns `false` (drop) if full.
    pub fn push(&self, lane: usize, record: &RawArchiveRecord) -> bool {
        assert!(lane < self.producer_count as usize);

        let tail = self.producer_seq(lane).load(Ordering::Relaxed);
        let head = self.consumer_seq(lane).load(Ordering::Acquire);

        if tail.wrapping_sub(head) >= self.lane_capacity as u64 {
            self.lane_drops(lane).fetch_add(1, Ordering::Relaxed);
            return false;
        }

        let idx = (tail as u32) & self.lane_mask;
        unsafe {
            core::ptr::write(self.slot_ptr(lane, idx) as *mut RawArchiveRecord, *record);
        }
        self.producer_seq(lane)
            .store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Pop a record from the given lane. Returns `None` if empty.
    pub fn pop(&self, lane: usize) -> Option<RawArchiveRecord> {
        assert!(lane < self.producer_count as usize);

        let head = self.consumer_seq(lane).load(Ordering::Relaxed);
        let tail = self.producer_seq(lane).load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let idx = (head as u32) & self.lane_mask;
        let record = unsafe { core::ptr::read(self.slot_ptr(lane, idx)) };
        self.consumer_seq(lane)
            .store(head.wrapping_add(1), Ordering::Release);
        Some(record)
    }

    /// Number of drops recorded for the given lane.
    pub fn lane_drop_count(&self, lane: usize) -> u64 {
        self.lane_drops(lane).load(Ordering::Relaxed)
    }

    /// Flush dirty pages to the backing file.
    pub fn flush(&self) -> Result<(), std::io::Error> {
        self.inner.flush()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    // -- G4-A2-T1: exact size / alignment / offset freeze --------------------

    #[test]
    fn g4_a2_raw_archive_record_size() {
        assert_eq!(core::mem::size_of::<RawArchiveRecord>(), 64);
    }

    #[test]
    fn g4_a2_raw_archive_record_align() {
        assert_eq!(core::mem::align_of::<RawArchiveRecord>(), 64);
    }

    #[test]
    fn g4_a2_raw_archive_record_field_offsets() {
        use core::mem::offset_of;

        assert_eq!(offset_of!(RawArchiveRecord, slot), 0);
        assert_eq!(offset_of!(RawArchiveRecord, generation), 8);
        assert_eq!(offset_of!(RawArchiveRecord, rx_timestamp_ns), 16);
        assert_eq!(offset_of!(RawArchiveRecord, frame_hash64), 24);
        assert_eq!(offset_of!(RawArchiveRecord, umem_offset_bytes), 32);
        assert_eq!(offset_of!(RawArchiveRecord, shred_index), 36);
        assert_eq!(offset_of!(RawArchiveRecord, len), 40);
        assert_eq!(offset_of!(RawArchiveRecord, is_data_shred), 42);
        assert_eq!(offset_of!(RawArchiveRecord, producer_id), 43);
        assert_eq!(offset_of!(RawArchiveRecord, _reserved), 44);
    }

    // -- G4-A2-T2: UMEM offset encode/decode roundtrip -----------------------

    #[test]
    fn g4_a2_umem_offset_roundtrip() {
        let rec = RawArchiveRecord::new(
            300_000_000, // slot
            0x8000_0000, // umem_offset_bytes (2 GiB mark)
            42,          // shred_index
            1200,        // len
            true,        // is_data_shred
            0,           // producer_id
        );

        assert_eq!(rec.slot, 300_000_000);
        assert_eq!(rec.generation, 0);
        assert_eq!(rec.rx_timestamp_ns, 0);
        assert_eq!(rec.frame_hash64, 0);
        assert_eq!(rec.umem_offset_bytes, 0x8000_0000);
        assert_eq!(rec.shred_index, 42);
        assert_eq!(rec.len, 1200);
        assert_eq!(rec.is_data_shred, 1);
        assert_eq!(rec.producer_id, 0);
    }

    #[test]
    fn g11_raw_archive_guard_metadata_roundtrip() {
        let rec = RawArchiveRecord::new_guarded(
            777,
            9,
            1_700_000_000_123,
            0xdead_beef_cafe_f00d,
            4096,
            11,
            512,
            true,
            3,
        );

        assert_eq!(rec.slot, 777);
        assert_eq!(rec.generation, 9);
        assert_eq!(rec.rx_timestamp_ns, 1_700_000_000_123);
        assert_eq!(rec.frame_hash64, 0xdead_beef_cafe_f00d);
        assert_eq!(rec.umem_offset_bytes, 4096);
        assert_eq!(rec.shred_index, 11);
        assert_eq!(rec.len, 512);
        assert_eq!(rec.is_data_shred, 1);
        assert_eq!(rec.producer_id, 3);
    }

    #[test]
    fn g4_a2_umem_offset_max_value() {
        // UMEM is 4 GiB → max offset = u32::MAX.
        let rec = RawArchiveRecord::new(0, u32::MAX, 0, 0, false, 0);
        assert_eq!(rec.umem_offset_bytes, u32::MAX);
    }

    // -- G4-A2-T3: pointer-free / fixed-size assertion -----------------------

    #[test]
    fn g4_a2_raw_archive_record_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<RawArchiveRecord>();
    }

    #[test]
    fn g4_a2_raw_archive_record_size_is_multiple_of_align() {
        let size = core::mem::size_of::<RawArchiveRecord>();
        let align = core::mem::align_of::<RawArchiveRecord>();
        assert_eq!(size % align, 0);
    }

    // -- Archive ring SPSC roundtrip -----------------------------------------

    #[test]
    fn g4_a2_archive_ring_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_raw_archive");

        let ring = create_raw_archive_ring(&path, 16).unwrap();

        let rec = RawArchiveRecord::new(100, 4096, 7, 1200, true, 0);
        assert!(ring.push(&rec));

        let popped = ring.pop().unwrap();
        assert_eq!(popped.slot, 100);
        assert_eq!(popped.umem_offset_bytes, 4096);
        assert_eq!(popped.shred_index, 7);
        assert_eq!(popped.len, 1200);
        assert_eq!(popped.is_data_shred, 1);
        assert_eq!(popped.producer_id, 0);
    }

    #[test]
    fn g4_a2_archive_ring_drop_on_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full_archive");

        let ring = create_raw_archive_ring(&path, 4).unwrap();
        for i in 0..4u32 {
            assert!(ring.push(&RawArchiveRecord::new(i as u64, i, 0, 0, true, 0)));
        }
        assert!(!ring.push(&RawArchiveRecord::new(999, 0, 0, 0, true, 0)));
        assert_eq!(ring.stats().drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn g4_a2_archive_ring_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reopen_archive");

        {
            let ring = create_raw_archive_ring(&path, 16).unwrap();
            ring.push(&RawArchiveRecord::new(42, 8192, 3, 500, false, 1));
            ring.flush().unwrap();
        }

        let ring2 = open_raw_archive_ring_rw(&path).unwrap();
        let rec = ring2.pop().unwrap();
        assert_eq!(rec.slot, 42);
        assert_eq!(rec.umem_offset_bytes, 8192);
        assert_eq!(rec.shred_index, 3);
        assert_eq!(rec.len, 500);
        assert_eq!(rec.is_data_shred, 0);
        assert_eq!(rec.producer_id, 1);
    }

    // -- Constants frozen test -----------------------------------------------

    #[test]
    fn g4_a2_constants_frozen() {
        assert_eq!(RAW_ARCHIVE_RING_SEGMENT_ID, 11);
        assert_eq!(RAW_ARCHIVE_RING_VERSION, 2);
        assert_eq!(RAW_ARCHIVE_RING_PATH_BASE, "/dev/shm/shadow_raw_archive");
    }

    // -- Path helpers --------------------------------------------------------

    #[test]
    fn g4_a2_archive_segment_path_singular() {
        assert_eq!(
            raw_archive_segment_path().to_str().unwrap(),
            "/dev/shm/shadow_raw_archive"
        );
    }

    // =========================================================================
    // G4-A6 — RawArchiveRecord contract freeze
    // =========================================================================

    #[test]
    fn g4_a6_raw_archive_record_size_stable() {
        assert_eq!(core::mem::size_of::<RawArchiveRecord>(), 64);
    }

    #[test]
    fn g4_a6_raw_archive_record_alignment_stable() {
        assert_eq!(core::mem::align_of::<RawArchiveRecord>(), 64);
    }

    #[test]
    fn g4_a6_raw_archive_record_offsets_stable() {
        use core::mem::offset_of;

        assert_eq!(offset_of!(RawArchiveRecord, slot), 0);
        assert_eq!(offset_of!(RawArchiveRecord, generation), 8);
        assert_eq!(offset_of!(RawArchiveRecord, rx_timestamp_ns), 16);
        assert_eq!(offset_of!(RawArchiveRecord, frame_hash64), 24);
        assert_eq!(offset_of!(RawArchiveRecord, umem_offset_bytes), 32);
        assert_eq!(offset_of!(RawArchiveRecord, shred_index), 36);
        assert_eq!(offset_of!(RawArchiveRecord, len), 40);
        assert_eq!(offset_of!(RawArchiveRecord, is_data_shred), 42);
        assert_eq!(offset_of!(RawArchiveRecord, producer_id), 43);
        assert_eq!(offset_of!(RawArchiveRecord, _reserved), 44);
    }

    #[test]
    fn g4_a6_raw_archive_umem_offset_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a6_umem");

        let ring = create_raw_archive_ring(&path, 16).unwrap();

        // Test boundary UMEM offsets through the ring.
        let cases: &[(u32, &str)] = &[
            (0, "zero"),
            (4096, "one page"),
            (0x8000_0000, "2 GiB mark"),
            (u32::MAX, "4 GiB - 1 (max)"),
        ];

        for &(offset, label) in cases {
            let rec = RawArchiveRecord::new(500_000_000, offset, 42, 1200, true, 0);
            assert!(ring.push(&rec));
            let got = ring.pop().unwrap();
            assert_eq!(
                got.umem_offset_bytes, offset,
                "UMEM offset roundtrip failed for {label}"
            );
            assert_eq!(got.slot, 500_000_000);
            assert_eq!(got.shred_index, 42);
            assert_eq!(got.len, 1200);
            assert_eq!(got.is_data_shred, 1);
        }
    }

    // =========================================================================
    // G4-A7 — Multi-lane raw archive segment
    // =========================================================================

    #[test]
    fn g4_a7_raw_archive_segment_create_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a7_reopen");

        {
            let seg = RawArchiveSegment::create(&path, 2, 16).unwrap();
            let rec = RawArchiveRecord::new(12345, 4096, 7, 800, true, 0);
            assert!(seg.push(0, &rec));
            seg.flush().unwrap();
        }

        let seg2 = RawArchiveSegment::open(&path).unwrap();
        assert_eq!(seg2.producer_count(), 2);
        assert_eq!(seg2.lane_capacity(), 16);

        let got = seg2.pop(0).unwrap();
        assert_eq!(got.slot, 12345);
        assert_eq!(got.umem_offset_bytes, 4096);
        assert_eq!(got.shred_index, 7);
        assert_eq!(got.len, 800);
        assert_eq!(got.is_data_shred, 1);
        assert_eq!(got.producer_id, 0);
    }

    #[test]
    fn g4_a7_raw_archive_lane_count_stable() {
        let dir = tempfile::tempdir().unwrap();

        for count in [1u32, 2, 4, 8] {
            let path = dir.path().join(format!("a7_lanes_{count}"));
            let seg = RawArchiveSegment::create(&path, count, 16).unwrap();
            assert_eq!(seg.producer_count(), count);
            seg.flush().unwrap();

            let seg2 = RawArchiveSegment::open(&path).unwrap();
            assert_eq!(
                seg2.producer_count(),
                count,
                "lane count unstable for {count}"
            );
        }
    }

    #[test]
    fn g4_a7_raw_archive_each_lane_is_spsc() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a7_spsc");

        let seg = RawArchiveSegment::create(&path, 4, 8).unwrap();

        // Push distinct records into each lane.
        for lane in 0..4usize {
            for i in 0..4u64 {
                let rec = RawArchiveRecord::new(
                    lane as u64 * 100 + i,
                    (lane as u32) * 1000 + i as u32,
                    i as u32,
                    100,
                    true,
                    lane as u8,
                );
                assert!(seg.push(lane, &rec), "push failed lane={lane} i={i}");
            }
        }

        // Pop from each lane and verify ordering + content.
        for lane in 0..4usize {
            for i in 0..4u64 {
                let got = seg.pop(lane).unwrap();
                assert_eq!(
                    got.slot,
                    lane as u64 * 100 + i,
                    "FIFO broken lane={lane} i={i}"
                );
                assert_eq!(got.producer_id, lane as u8);
            }
            assert!(seg.pop(lane).is_none(), "lane {lane} not empty");
        }
    }

    #[test]
    fn g4_a7_raw_archive_drop_on_full_non_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a7_drop");

        let seg = RawArchiveSegment::create(&path, 2, 4).unwrap();

        // Fill lane 0 to capacity.
        for i in 0..4u64 {
            assert!(seg.push(0, &RawArchiveRecord::new(i, 0, 0, 0, true, 0)));
        }

        // Next push must drop — never block.
        assert!(!seg.push(0, &RawArchiveRecord::new(999, 0, 0, 0, true, 0)));
        assert_eq!(seg.lane_drop_count(0), 1);

        // Lane 1 is still empty — must accept.
        assert!(seg.push(1, &RawArchiveRecord::new(0, 0, 0, 0, true, 1)));
        assert_eq!(seg.lane_drop_count(1), 0);
    }

    #[test]
    fn g4_a7_raw_archive_cross_lane_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a7_isolation");

        let seg = RawArchiveSegment::create(&path, 3, 8).unwrap();

        // Push only to lane 1.
        let rec = RawArchiveRecord::new(42, 8192, 5, 600, false, 1);
        assert!(seg.push(1, &rec));

        // Lane 0 and lane 2 must be empty.
        assert!(seg.pop(0).is_none(), "lane 0 leaked");
        assert!(seg.pop(2).is_none(), "lane 2 leaked");

        // Lane 1 returns the record.
        let got = seg.pop(1).unwrap();
        assert_eq!(got.slot, 42);
        assert_eq!(got.umem_offset_bytes, 8192);
        assert_eq!(got.producer_id, 1);
    }

    // =========================================================================
    // G4-A8 — Compatibility gate: versioning, fixtures, cross-process
    // =========================================================================

    #[test]
    fn g4_a8_bad_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a8_bad_magic");

        RawArchiveSegment::create(&path, 2, 4).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        data[0..8].copy_from_slice(b"XXXXXXXX");
        std::fs::write(&path, &data).unwrap();

        let result = RawArchiveSegment::open(&path);
        assert!(
            matches!(result, Err(crate::ring::RingError::BadMagic)),
            "corrupt magic must be rejected"
        );
    }

    #[test]
    fn g4_a8_wrong_segment_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a8_bad_segid");

        RawArchiveSegment::create(&path, 2, 4).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        // segment_id is at offset 24 in SegmentHeader64B (u64 LE).
        data[24..32].copy_from_slice(&99u64.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let result = RawArchiveSegment::open(&path);
        assert!(
            matches!(result, Err(crate::ring::RingError::WrongSegmentId { .. })),
            "wrong segment ID must be rejected"
        );
    }

    #[test]
    fn g4_a8_wrong_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a8_bad_ver");

        RawArchiveSegment::create(&path, 2, 4).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        // version is at offset 52 in SegmentHeader64B (u16 LE).
        data[52] = 99;
        data[53] = 0;
        std::fs::write(&path, &data).unwrap();

        let result = RawArchiveSegment::open(&path);
        assert!(
            matches!(result, Err(crate::ring::RingError::WrongVersion { .. })),
            "wrong version must be rejected"
        );
    }

    #[test]
    fn g4_a8_truncated_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a8_truncated");

        // Write only 64 bytes — too small for header + meta.
        std::fs::write(&path, [0u8; 64]).unwrap();

        let result = RawArchiveSegment::open(&path);
        assert!(
            matches!(
                result,
                Err(crate::ring::RingError::FileTooSmall) | Err(crate::ring::RingError::BadMagic)
            ),
            "truncated file must be rejected"
        );
    }

    #[test]
    fn g4_a8_cross_process_attach_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a8_cross");

        // "Process A" creates the segment and pushes a record.
        let seg_a = RawArchiveSegment::create(&path, 2, 16).unwrap();
        let rec = RawArchiveRecord::new(88888, 16384, 11, 900, true, 0);
        assert!(seg_a.push(0, &rec));
        seg_a.flush().unwrap();

        // "Process B" opens an independent mapping of the same file.
        let seg_b = RawArchiveSegment::open(&path).unwrap();
        assert_eq!(seg_b.producer_count(), 2);
        assert_eq!(seg_b.lane_capacity(), 16);

        let got = seg_b.pop(0).unwrap();
        assert_eq!(got.slot, 88888);
        assert_eq!(got.umem_offset_bytes, 16384);
        assert_eq!(got.shred_index, 11);
        assert_eq!(got.len, 900);
        assert_eq!(got.is_data_shred, 1);
        assert_eq!(got.producer_id, 0);

        // Lane 1 remains empty across both mappings.
        assert!(seg_b.pop(1).is_none());
    }

    // =========================================================================
    // G4-A-GATE — aggregate contract freeze assertion
    // =========================================================================

    #[test]
    fn g4_a_gate() {
        use crate::ingress_record::{
            IngressRecord, ACCT_INDEX_ABSENT, FEED_RING_SEGMENT_ID, FEED_RING_VERSION,
            FLAT_ACCOUNTS_CAP, OBSCURITY_SCORE_MAX, POOL_INDEX_HINT_NONE,
        };

        // --- IngressRecord is frozen ---
        assert_eq!(core::mem::size_of::<IngressRecord>(), 2304);
        assert_eq!(core::mem::align_of::<IngressRecord>(), 64);
        fn assert_copy_ir<T: Copy>() {}
        assert_copy_ir::<IngressRecord>();

        // --- IngressRecord constants frozen ---
        assert_eq!(FLAT_ACCOUNTS_CAP, 64);
        assert_eq!(OBSCURITY_SCORE_MAX, 3);
        assert_eq!(ACCT_INDEX_ABSENT, 0xFF);
        assert_eq!(POOL_INDEX_HINT_NONE, u32::MAX);
        assert_eq!(FEED_RING_SEGMENT_ID, 10);
        assert_eq!(FEED_RING_VERSION, 1);

        // --- RawArchiveRecord is frozen ---
        assert_eq!(core::mem::size_of::<RawArchiveRecord>(), 64);
        assert_eq!(core::mem::align_of::<RawArchiveRecord>(), 64);
        fn assert_copy_ra<T: Copy>() {}
        assert_copy_ra::<RawArchiveRecord>();

        // --- RawArchive constants frozen ---
        assert_eq!(RAW_ARCHIVE_RING_SEGMENT_ID, 11);
        assert_eq!(RAW_ARCHIVE_RING_VERSION, 2);

        // --- ArchiveMeta frozen ---
        assert_eq!(core::mem::size_of::<ArchiveMeta64B>(), 64);
        assert_eq!(core::mem::align_of::<ArchiveMeta64B>(), 64);

        // --- Feed ring bootstrap works ---
        let dir = tempfile::tempdir().unwrap();
        let feed_path = dir.path().join("gate_feed");
        let feed_ring = crate::ingress_record::create_feed_ring(&feed_path, 4).unwrap();
        let mut ir = IngressRecord::zeroed();
        ir.slot = 1;
        assert!(feed_ring.push(&ir));
        assert_eq!(feed_ring.pop().unwrap().slot, 1);

        // --- Raw archive bootstrap works ---
        let archive_path = dir.path().join("gate_archive");
        let archive = RawArchiveSegment::create(&archive_path, 2, 4).unwrap();
        let ar = RawArchiveRecord::new(2, 0, 0, 0, true, 0);
        assert!(archive.push(0, &ar));
        assert_eq!(archive.pop(0).unwrap().slot, 2);

        // --- Drop-on-full proven ---
        for i in 0..4u64 {
            ir.slot = i;
            feed_ring.push(&ir);
        }
        assert!(!feed_ring.push(&ir), "feed ring must drop on full");

        for i in 0..4u64 {
            archive.push(0, &RawArchiveRecord::new(i, 0, 0, 0, true, 0));
        }
        assert!(
            !archive.push(0, &RawArchiveRecord::new(99, 0, 0, 0, true, 0)),
            "archive lane must drop on full"
        );

        // --- Version mismatch fails closed ---
        // (proven by g4_a5 + g4_a8 bad_magic/wrong_version tests above)
    }
}

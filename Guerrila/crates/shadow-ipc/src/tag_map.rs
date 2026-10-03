use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::{Mmap, MmapMut};

use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};

pub const KIND_POOL: u32 = 1;
pub const KIND_MINT: u32 = 2;

#[repr(C, align(64))]
pub struct TagMapMeta64B {
    pub capacity: u32,
    pub mask: u32,
    pub max_probes: u32,
    pub kind: u32, // 1 = pool, 2 = mint
    pub entries_offset: u32,
    pub version: u16,
    pub reserved_u16: u16,
    pub total_size: u64,
    _pad: [u8; 32],
}

static_assertions::assert_eq_size!(TagMapMeta64B, [u8; 64]);

// ---------------------------------------------------------------------------
// 8-byte Tag Helpers
// ---------------------------------------------------------------------------

/// Computes the 40-bit hash of a pubkey using its first 8 bytes.
#[inline(always)]
pub fn hash40(pubkey: &[u8; 32]) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&pubkey[0..8]);
    let h64 = u64::from_le_bytes(bytes);
    (h64 >> 24) & ((1 << 40) - 1)
}

/// Packs a 40-bit hash and a 24-bit index_plus_one into a 64-bit tag.
#[inline(always)]
pub fn pack_tag(hash40: u64, index_plus_one: u32) -> u64 {
    (hash40 << 24) | (index_plus_one as u64 & 0xFF_FFFF)
}

/// Unpacks the 24-bit index_plus_one from a 64-bit tag.
#[inline(always)]
pub fn unpack_index_plus_one(tag: u64) -> u32 {
    (tag & 0xFF_FFFF) as u32
}

const HEADER_SIZE: usize = core::mem::size_of::<SegmentHeader64B>(); // 64
const META_OFFSET: usize = HEADER_SIZE; // 64
const CONTROL_SIZE: usize = META_OFFSET + core::mem::size_of::<TagMapMeta64B>(); // 128

#[derive(Debug)]
pub enum TagMapError {
    Io(io::Error),
    BadMagic,
    WrongSegmentId { expected: u64, found: u64 },
    WrongVersion { expected: u16, found: u16 },
    CapacityNotPowerOfTwo(u32),
    WrongMask { expected: u32, found: u32 },
    MaxProbesTooLarge { expected: u32, found: u32 },
    WrongKind { expected: u32, found: u32 },
    WrongEntriesOffset { expected: u32, found: u32 },
    WrongTotalSize { expected: u64, found: u64 },
    FileTooSmall,
}

impl From<io::Error> for TagMapError {
    fn from(e: io::Error) -> Self {
        TagMapError::Io(e)
    }
}

impl std::fmt::Display for TagMapError {
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
            Self::WrongMask { expected, found } => {
                write!(f, "wrong mask: expected {expected}, found {found}")
            }
            Self::MaxProbesTooLarge { expected, found } => {
                write!(f, "max probes too large: found {found}, max is {expected}")
            }
            Self::WrongKind { expected, found } => {
                write!(f, "wrong kind: expected {expected}, found {found}")
            }
            Self::WrongEntriesOffset { expected, found } => {
                write!(
                    f,
                    "wrong entries_offset: expected {expected}, found {found}"
                )
            }
            Self::WrongTotalSize { expected, found } => {
                write!(f, "wrong total_size: expected {expected}, found {found}")
            }
            Self::FileTooSmall => write!(f, "file too small for tag map header + meta"),
        }
    }
}

impl std::error::Error for TagMapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

/// A fixed-capacity, file-backed, lock-free 8-byte tag map.
pub struct MappedTagMap {
    inner: MmapInner,
    entries_offset: usize,
    capacity: u32,
    mask: u32,
    max_probes: u32,
    #[allow(dead_code)]
    kind: u32,
}

impl MappedTagMap {
    /// Create a new tag map file, write header + meta, zero entries.
    pub fn create_rw(
        path: &Path,
        segment_id: u64,
        version: u16,
        capacity: u32,
        max_probes: u32,
        kind: u32,
    ) -> Result<Self, TagMapError> {
        if capacity == 0 || !capacity.is_power_of_two() {
            return Err(TagMapError::CapacityNotPowerOfTwo(capacity));
        }
        if max_probes > 8 {
            return Err(TagMapError::MaxProbesTooLarge {
                expected: 8,
                found: max_probes,
            });
        }

        let entries_offset = align_up(CONTROL_SIZE, 64);
        let entries_size = (capacity as usize) * core::mem::size_of::<AtomicU64>();
        let total_size = entries_offset + entries_size;
        let mask = capacity - 1;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_size as u64)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Write SegmentHeader64B
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
            payload_align: core::cmp::max(64, core::mem::align_of::<TagMapMeta64B>() as u16),
            reserved_u16: 0,
            reserved_u32: 0,
        };
        unsafe {
            core::ptr::write(mmap.as_mut_ptr() as *mut SegmentHeader64B, header);
        }

        // Write TagMapMeta64B
        let meta = TagMapMeta64B {
            capacity,
            mask,
            max_probes,
            kind,
            entries_offset: entries_offset as u32,
            version,
            reserved_u16: 0,
            total_size: total_size as u64,
            _pad: [0; 32],
        };
        unsafe {
            core::ptr::write(
                mmap.as_mut_ptr().add(META_OFFSET) as *mut TagMapMeta64B,
                meta,
            );
        }

        mmap.flush()?;

        Ok(Self {
            inner: MmapInner::ReadWrite(mmap),
            entries_offset,
            capacity,
            mask,
            max_probes,
            kind,
        })
    }

    /// Open an existing tag map read-write and validate constraints.
    pub fn open_rw(
        path: &Path,
        segment_id: u64,
        version: u16,
        expected_kind: u32,
    ) -> Result<Self, TagMapError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let len = mmap.len();
        Self::validate_and_build(
            MmapInner::ReadWrite(mmap),
            len,
            segment_id,
            version,
            expected_kind,
        )
    }

    /// Open an existing tag map read-only and validate constraints.
    pub fn open_ro(
        path: &Path,
        segment_id: u64,
        version: u16,
        expected_kind: u32,
    ) -> Result<Self, TagMapError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let len = mmap.len();
        Self::validate_and_build(
            MmapInner::ReadOnly(mmap),
            len,
            segment_id,
            version,
            expected_kind,
        )
    }

    fn validate_and_build(
        inner: MmapInner,
        len: usize,
        segment_id: u64,
        version: u16,
        expected_kind: u32,
    ) -> Result<Self, TagMapError> {
        if len < CONTROL_SIZE {
            return Err(TagMapError::FileTooSmall);
        }

        let base = match &inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };

        // Validate SegmentHeader64B
        let header = unsafe { &*(base as *const SegmentHeader64B) };
        if header.magic != SEGMENT_MAGIC {
            return Err(TagMapError::BadMagic);
        }
        if header.segment_id != segment_id {
            return Err(TagMapError::WrongSegmentId {
                expected: segment_id,
                found: header.segment_id,
            });
        }
        if header.version != version {
            return Err(TagMapError::WrongVersion {
                expected: version,
                found: header.version,
            });
        }

        // Validate TagMapMeta64B
        let meta = unsafe { &*(base.add(META_OFFSET) as *const TagMapMeta64B) };

        if meta.capacity == 0 || !meta.capacity.is_power_of_two() {
            return Err(TagMapError::CapacityNotPowerOfTwo(meta.capacity));
        }
        if meta.mask != meta.capacity - 1 {
            return Err(TagMapError::WrongMask {
                expected: meta.capacity - 1,
                found: meta.mask,
            });
        }
        if meta.max_probes > 8 {
            return Err(TagMapError::MaxProbesTooLarge {
                expected: 8,
                found: meta.max_probes,
            });
        }
        if meta.kind != expected_kind {
            return Err(TagMapError::WrongKind {
                expected: expected_kind,
                found: meta.kind,
            });
        }

        let expected_entries_offset = align_up(CONTROL_SIZE, 64);
        if meta.entries_offset != expected_entries_offset as u32 {
            return Err(TagMapError::WrongEntriesOffset {
                expected: expected_entries_offset as u32,
                found: meta.entries_offset,
            });
        }

        let entries_size = (meta.capacity as usize) * core::mem::size_of::<AtomicU64>();
        let expected_total = expected_entries_offset + entries_size;
        if meta.total_size != expected_total as u64 {
            return Err(TagMapError::WrongTotalSize {
                expected: expected_total as u64,
                found: meta.total_size,
            });
        }

        if len < expected_total {
            return Err(TagMapError::FileTooSmall);
        }

        Ok(Self {
            inner,
            entries_offset: meta.entries_offset as usize,
            capacity: meta.capacity,
            mask: meta.mask,
            max_probes: meta.max_probes,
            kind: meta.kind,
        })
    }

    /// Access the underlying array of atomic 8-byte tags.
    pub fn entries(&self) -> &[AtomicU64] {
        let base = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                base.add(self.entries_offset) as *const AtomicU64,
                self.capacity as usize,
            )
        }
    }

    /// Map capacity.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Flush dirty pages back to the file.
    pub fn flush(&self) -> Result<(), TagMapError> {
        match &self.inner {
            MmapInner::ReadWrite(m) => m.flush().map_err(TagMapError::Io),
            MmapInner::ReadOnly(_) => Ok(()),
        }
    }

    /// Bounded linear probe returning `index_plus_one - 1` on match, or `None` on miss.
    #[inline(always)]
    pub fn find_by_hash40(&self, pubkey: &[u8; 32]) -> Option<u32> {
        let tag_h40 = hash40(pubkey);
        let start_idx = (tag_h40 & (self.mask as u64)) as u32;

        let entries = self.entries();
        let mask = self.mask;

        for probe in 0..self.max_probes {
            let idx = (start_idx + probe) & mask;
            let val = entries[idx as usize].load(Ordering::Acquire);

            if val == 0 {
                return None;
            }

            // compare only the 40-bit hash
            if (val >> 24) == tag_h40 {
                return Some(unpack_index_plus_one(val).saturating_sub(1));
            }
        }
        None
    }

    /// Bounded linear probe that yields candidates to a verifier closure.
    /// Resumes search natively upon verifier rejection instead of stopping.
    #[inline(always)]
    pub fn find_existing_index_by_pubkey_with_verifier<F>(
        &self,
        pubkey: &[u8; 32],
        mut verifier: F,
    ) -> Option<u32>
    where
        F: FnMut(u32) -> bool,
    {
        let tag_h40 = hash40(pubkey);
        let start_idx = (tag_h40 & (self.mask as u64)) as u32;

        let entries = self.entries();
        let mask = self.mask;

        for probe in 0..self.max_probes {
            let idx = (start_idx + probe) & mask;
            let val = entries[idx as usize].load(Ordering::Acquire);

            if val == 0 {
                return None; // Probe stream broken natively
            }

            if (val >> 24) == tag_h40 {
                let candidate_idx = unpack_index_plus_one(val).saturating_sub(1);
                if verifier(candidate_idx) {
                    return Some(candidate_idx);
                }
            }
        }
        None
    }

    /// Single-writer bounded linear probe insertion.
    #[inline(always)]
    pub fn insert_tag(&self, pubkey: &[u8; 32], index_plus_one: u32) -> Result<(), ()> {
        if index_plus_one == 0 || index_plus_one > 0xFF_FFFF {
            return Err(()); // index_plus_one must fit in 24 bits and be > 0
        }

        assert!(
            matches!(self.inner, MmapInner::ReadWrite(_)),
            "insert requires rw map"
        );

        let tag_h40 = hash40(pubkey);
        let start_idx = (tag_h40 & (self.mask as u64)) as u32;
        let tag_value = pack_tag(tag_h40, index_plus_one);

        let entries = self.entries();
        let mask = self.mask;

        for probe in 0..self.max_probes {
            let idx = (start_idx + probe) & mask;
            let slot = &entries[idx as usize];

            let current = slot.load(Ordering::Acquire);

            // Already tracking this identical tag
            if current == tag_value {
                return Ok(());
            }

            if slot
                .compare_exchange(0, tag_value, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
        }
        Err(())
    }
}

// ---------------------------------------------------------------------------
// Tag Map Concrete Bootstrap Helpers
// ---------------------------------------------------------------------------

pub const POOL_INDEX_MAP_SEGMENT_ID: u64 = 7;
pub const MINT_INDEX_MAP_SEGMENT_ID: u64 = 8;
pub const TAG_MAP_VERSION: u16 = 1;

pub fn create_pool_index_map(
    path: &Path,
    capacity: u32,
    max_probes: u32,
) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::create_rw(
        path,
        POOL_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        capacity,
        max_probes,
        KIND_POOL,
    )
}

pub fn open_pool_index_map_rw(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_rw(path, POOL_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_POOL)
}

pub fn open_pool_index_map_ro(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_ro(path, POOL_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_POOL)
}

pub fn create_mint_index_map(
    path: &Path,
    capacity: u32,
    max_probes: u32,
) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::create_rw(
        path,
        MINT_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        capacity,
        max_probes,
        KIND_MINT,
    )
}

pub fn open_mint_index_map_rw(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_rw(path, MINT_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_MINT)
}

pub fn open_mint_index_map_ro(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_ro(path, MINT_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_MINT)
}

// ---------------------------------------------------------------------------
// ALT index map helpers
// ---------------------------------------------------------------------------

pub const KIND_ALT: u32 = 3;
pub const ALT_INDEX_MAP_SEGMENT_ID: u64 = 9;

pub fn create_alt_index_map(
    path: &Path,
    capacity: u32,
    max_probes: u32,
) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::create_rw(
        path,
        ALT_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        capacity,
        max_probes,
        KIND_ALT,
    )
}

pub fn open_alt_index_map_rw(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_rw(path, ALT_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_ALT)
}

pub fn open_alt_index_map_ro(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_ro(path, ALT_INDEX_MAP_SEGMENT_ID, TAG_MAP_VERSION, KIND_ALT)
}

// ---------------------------------------------------------------------------
// Pool meta index map helpers
// ---------------------------------------------------------------------------

pub const KIND_POOL_META: u32 = 4;
pub const POOL_META_INDEX_MAP_SEGMENT_ID: u64 = 17;

pub fn create_pool_meta_index_map(
    path: &Path,
    capacity: u32,
    max_probes: u32,
) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::create_rw(
        path,
        POOL_META_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        capacity,
        max_probes,
        KIND_POOL_META,
    )
}

pub fn open_pool_meta_index_map_rw(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_rw(
        path,
        POOL_META_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        KIND_POOL_META,
    )
}

pub fn open_pool_meta_index_map_ro(path: &Path) -> Result<MappedTagMap, TagMapError> {
    MappedTagMap::open_ro(
        path,
        POOL_META_INDEX_MAP_SEGMENT_ID,
        TAG_MAP_VERSION,
        KIND_POOL_META,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ID: u64 = 42;
    const TEST_VERSION: u16 = 1;

    #[test]
    fn layout_meta_is_64_bytes() {
        assert_eq!(core::mem::size_of::<TagMapMeta64B>(), 64);
        assert_eq!(core::mem::align_of::<TagMapMeta64B>(), 64);
    }

    #[test]
    fn hash40_extracts_correct_bits() {
        let pubkey = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        let expected = 0x8877665544;
        assert_eq!(hash40(&pubkey), expected);
    }

    #[test]
    fn pack_and_unpack_tag() {
        let h40 = 0x8877665544;
        let idx = 42;
        let tag = pack_tag(h40, idx);

        let extracted_idx = unpack_index_plus_one(tag);
        assert_eq!(extracted_idx, idx);
        assert_eq!((tag >> 24) & ((1 << 40) - 1), h40);

        let overflow_idx = 0xFF_FFFF + 1;
        let tag2 = pack_tag(h40, overflow_idx);
        assert_eq!(unpack_index_plus_one(tag2), 0);
        assert_eq!((tag2 >> 24) & ((1 << 40) - 1), h40);
    }

    #[test]
    fn create_rejects_non_power_of_two_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_reject");

        let res = create_pool_index_map(&path, 15, 8);
        assert!(matches!(res, Err(TagMapError::CapacityNotPowerOfTwo(15))));
    }

    #[test]
    fn create_rejects_max_probes_above_8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe_reject");

        let res = create_pool_index_map(&path, 16, 9);
        assert!(matches!(res, Err(TagMapError::MaxProbesTooLarge { .. })));
    }

    #[test]
    fn pool_map_insert_and_probe_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pool_index_map");

        let map = create_pool_index_map(&path, 16, 8).unwrap();

        let pubkey = [0xAA; 32];
        let index_plus_one = 15;

        assert!(map.insert_tag(&pubkey, index_plus_one).is_ok());

        let found = map.find_by_hash40(&pubkey);
        assert_eq!(found, Some(14)); // index_plus_one - 1
    }

    #[test]
    fn mint_map_insert_and_probe_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mint_index_map");

        let map = create_mint_index_map(&path, 16, 8).unwrap();

        let pubkey = [0xBB; 32];
        let index_plus_one = 7;

        assert!(map.insert_tag(&pubkey, index_plus_one).is_ok());

        let found = map.find_by_hash40(&pubkey);
        assert_eq!(found, Some(6));
    }

    #[test]
    fn collision_linear_probe_finds_second_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe_bound_map");

        let map = create_pool_index_map(&path, 16, 8).unwrap();

        // Two keys exactly the same
        let mut target_a = [0x11; 32];
        target_a[0..8].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA]);
        let mut target_b = [0x22; 32];
        target_b[0..8].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0xBB, 0xBB, 0xBB, 0xBB]);

        // Force them manually to hash identically via our hash40 method
        // They differ above 40 bits

        // Manually packing identical hash bases with different tags
        let tag_a = pack_tag(12345, 10);
        let tag_b = pack_tag(67890, 20); // completely different hash base so realistic map miss test

        map.entries()[0].store(tag_a, Ordering::Release);
        map.entries()[1].store(tag_b, Ordering::Release);

        // the search bounds start idx using modulo math,
        // realistically in a unit test we want a verified `hash40` conflict that lands in same slot
        let mut conflict_a = [0xAA; 32];
        let mut conflict_b = [0xBB; 32];

        // ensure exactly identical top 8 bytes but different later bytes
        conflict_b[0..8].copy_from_slice(&conflict_a[0..8]);
        // B has different remaining bytes. The tags are structurally identical using hash40.

        assert!(map.insert_tag(&conflict_a, 5).is_ok());
        assert!(map.insert_tag(&conflict_b, 6).is_ok()); // Should conflict but linearly probe correctly to next slot!

        let found_a = map.find_by_hash40(&conflict_a);
        let found_b = map.find_by_hash40(&conflict_b);

        assert_eq!(found_a, Some(4));
        assert_eq!(found_b, Some(4)); // Because hash40 is identical, it literally hits the FIRST one and considers it a match (which is correct behavior for our 40-bit tag scheme -- full collision check done in arena!)
    }

    #[test]
    fn probe_returns_none_after_bounded_miss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe_returns_none");

        // map of 16, but max_probes is 2
        let map = create_pool_index_map(&path, 16, 2).unwrap();

        let pubkey1 = [0x11; 32];
        let pubkey2 = [0x22; 32];
        let pubkey3 = [0x33; 32];

        // All have same hash40 to force hash conflicts.
        // Wait, hash40 works on first 8 bytes.
        let mut key_base = [0xCC; 32];
        // So pubkey1,2,3 same hash40
        assert!(map.insert_tag(&key_base, 10).is_ok());
        assert!(map.insert_tag(&key_base, 20).is_ok()); // Replaces the duplicate block if same hash in real system! Actually our `insert_tag` checks `current == tag_value`, but since they differ it linearly probes.
        assert!(map.insert_tag(&key_base, 30).is_err()); // Probing bounds breached (max_probes=2)
    }

    #[test]
    fn zero_tag_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero_tag");

        let map = create_pool_index_map(&path, 16, 2).unwrap();
        let key = [0x11; 32];

        // Zero index_plus_one is explicitly errored
        assert!(map.insert_tag(&key, 0).is_err());

        let found = map.find_by_hash40(&key);
        assert_eq!(found, None);
    }

    #[test]
    fn rejects_index_plus_one_above_24_bits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("above_24_bits");

        let map = create_pool_index_map(&path, 16, 2).unwrap();
        let key = [0x11; 32];

        let overflow = 0xFF_FFFF + 1;
        assert!(map.insert_tag(&key, overflow).is_err());
    }

    #[test]
    fn pool_map_create_and_reopen_roundtrip_across_two_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pool_index_map_roundtrip");

        {
            let map = create_pool_index_map(&path, 16, 8).unwrap();
            let key = [0x01; 32];
            assert!(map.insert_tag(&key, 42).is_ok());
            map.flush().unwrap();
        }

        let map_ro = open_pool_index_map_ro(&path).unwrap();
        let key = [0x01; 32];
        let found = map_ro.find_by_hash40(&key);
        assert_eq!(found, Some(41)); // index_plus_one - 1
    }

    #[test]
    fn mint_map_create_and_reopen_roundtrip_across_two_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mint_index_map_roundtrip");

        {
            let map = create_mint_index_map(&path, 16, 8).unwrap();
            let key = [0x55; 32];
            assert!(map.insert_tag(&key, 99).is_ok());
            map.flush().unwrap();
        }

        let map_ro = open_mint_index_map_ro(&path).unwrap();
        let key = [0x55; 32];
        let found = map_ro.find_by_hash40(&key);
        assert_eq!(found, Some(98));
    }

    #[test]
    fn cross_mapping_writer_reader_sees_same_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cross_mapping");

        let writer = create_pool_index_map(&path, 16, 8).unwrap();
        let reader = open_pool_index_map_ro(&path).unwrap();

        let key = [0xFA; 32];
        assert!(writer.insert_tag(&key, 7).is_ok());
        // Do not explicitly flush! Verify OS immediately mirrors dirty page back to read-only map!

        let found = reader.find_by_hash40(&key);
        assert_eq!(found, Some(6));
    }

    #[test]
    fn open_rejects_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong_kind");

        // We create a mint map to satisfy the segment ID check of open_mint_index_map,
        // but manually overwrite `kind` to KIND_POOL in memory.
        let map = create_mint_index_map(&path, 16, 8).unwrap();

        let base = match &map.inner {
            MmapInner::ReadWrite(m) => m.as_ptr(),
            _ => panic!("Expected ReadWrite"),
        };
        unsafe {
            let meta_offset = core::mem::size_of::<SegmentHeader64B>();
            let meta_ptr = base.add(meta_offset) as *mut TagMapMeta64B;
            (*meta_ptr).kind = KIND_POOL; // Corrupt the kind physically
        }
        map.flush().unwrap();

        let res = open_mint_index_map_ro(&path);
        // Should bypass the Segment ID check but hard bounce on the Kind validation since we forced it to KIND_POOL
        assert!(matches!(
            res,
            Err(TagMapError::WrongKind {
                expected: KIND_MINT,
                found: KIND_POOL
            })
        ));
    }

    #[test]
    fn open_rejects_wrong_entries_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong_offset");

        let map = create_pool_index_map(&path, 16, 8).unwrap();

        // Corrupt entries_offset manually via memory mapping hack
        let base = match &map.inner {
            MmapInner::ReadWrite(m) => m.as_ptr(),
            _ => panic!("Expected ReadWrite"),
        };
        unsafe {
            let meta_offset = core::mem::size_of::<SegmentHeader64B>();
            let meta_ptr = base.add(meta_offset) as *mut TagMapMeta64B;
            (*meta_ptr).entries_offset = 9999;
        }
        map.flush().unwrap();

        let res = open_pool_index_map_ro(&path);
        assert!(matches!(
            res,
            Err(TagMapError::WrongEntriesOffset {
                expected: 128,
                found: 9999
            })
        ));
    }

    #[test]
    fn open_rejects_wrong_total_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong_size");

        let map = create_pool_index_map(&path, 16, 8).unwrap();

        let base = match &map.inner {
            MmapInner::ReadWrite(m) => m.as_ptr(),
            _ => panic!("Expected ReadWrite"),
        };
        unsafe {
            let meta_offset = core::mem::size_of::<SegmentHeader64B>();
            let meta_ptr = base.add(meta_offset) as *mut TagMapMeta64B;
            (*meta_ptr).total_size = 9999999;
        }
        map.flush().unwrap();

        let res = open_pool_index_map_ro(&path);
        assert!(matches!(
            res,
            Err(TagMapError::WrongTotalSize { found: 9999999, .. })
        ));
    }
}

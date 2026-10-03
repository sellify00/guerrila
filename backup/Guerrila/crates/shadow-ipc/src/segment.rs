use std::fs::{File, OpenOptions};
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use memmap2::{Mmap, MmapMut};

/// Magic bytes identifying a Shadow Engine segment file.
pub const SEGMENT_MAGIC: [u8; 8] = *b"SHDWSEG\0";

/// Segment file header — always the first 64 bytes of every segment file.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct SegmentHeader64B {
    pub magic: [u8; 8],
    pub build_git_sha_prefix: [u8; 8],
    pub checksum: u64,
    pub segment_id: u64,
    pub payload_size: u64,
    pub total_size: u64,
    pub payload_offset: u32,
    pub version: u16,
    pub header_size: u16,
    pub payload_align: u16,
    pub reserved_u16: u16,
    pub reserved_u32: u32,
}

static_assertions::assert_eq_size!(SegmentHeader64B, [u8; 64]);

/// Round `offset` up to the next multiple of `align`.
///
/// `align` must be a power of two.
pub const fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

/// Trait implemented by each payload type that can back a mapped segment.
pub trait SegmentSpec: Sized {
    const SEGMENT_ID: u64;
    const VERSION: u16;
    const PATH: &'static str;

    /// Initialize a freshly allocated payload region.
    ///
    /// # Safety
    ///
    /// `ptr` must point to `size_of::<Self>()` writable, properly aligned bytes.
    fn init_payload(ptr: *mut Self);
}

/// Errors from segment create / open / validate.
#[derive(Debug)]
pub enum SegmentError {
    Io(io::Error),
    BadMagic,
    WrongSegmentId { expected: u64, found: u64 },
    WrongVersion { expected: u16, found: u16 },
    BadHeaderSize(u16),
    BadPayloadOffset,
    BadPayloadSize,
    BadTotalSize,
    FileTooSmall,
    InvalidPayload { reason: String },
}

impl From<io::Error> for SegmentError {
    fn from(e: io::Error) -> Self {
        SegmentError::Io(e)
    }
}

impl std::fmt::Display for SegmentError {
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
            Self::BadHeaderSize(s) => write!(f, "bad header_size: {s}"),
            Self::BadPayloadOffset => write!(f, "bad payload_offset"),
            Self::BadPayloadSize => write!(f, "bad payload_size"),
            Self::BadTotalSize => write!(f, "bad total_size"),
            Self::FileTooSmall => write!(f, "file too small for header + payload"),
            Self::InvalidPayload { reason } => write!(f, "invalid payload: {reason}"),
        }
    }
}

impl std::error::Error for SegmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

/// A memory-mapped segment file backed by a typed payload `T`.
pub struct MappedSegment<T: SegmentSpec> {
    inner: MmapInner,
    payload_offset: usize,
    _phantom: PhantomData<T>,
}

impl<T: SegmentSpec> MappedSegment<T> {
    /// Compute the canonical header for type `T`.
    fn header_for_type() -> SegmentHeader64B {
        let payload_align = core::mem::align_of::<T>();
        let payload_offset = align_up(64, payload_align);
        let payload_size = core::mem::size_of::<T>();
        let total_size = payload_offset + payload_size;

        SegmentHeader64B {
            magic: SEGMENT_MAGIC,
            build_git_sha_prefix: [0u8; 8],
            checksum: 0,
            segment_id: T::SEGMENT_ID,
            payload_size: payload_size as u64,
            total_size: total_size as u64,
            payload_offset: payload_offset as u32,
            version: T::VERSION,
            header_size: 64,
            payload_align: payload_align as u16,
            reserved_u16: 0,
            reserved_u32: 0,
        }
    }

    /// Validate a header read from a file against the expected values for `T`.
    fn validate_header(header: &SegmentHeader64B) -> Result<(), SegmentError> {
        if header.magic != SEGMENT_MAGIC {
            return Err(SegmentError::BadMagic);
        }
        if header.segment_id != T::SEGMENT_ID {
            return Err(SegmentError::WrongSegmentId {
                expected: T::SEGMENT_ID,
                found: header.segment_id,
            });
        }
        if header.version != T::VERSION {
            return Err(SegmentError::WrongVersion {
                expected: T::VERSION,
                found: header.version,
            });
        }
        if header.header_size != 64 {
            return Err(SegmentError::BadHeaderSize(header.header_size));
        }
        let expected_offset = align_up(64, core::mem::align_of::<T>());
        if header.payload_offset as usize != expected_offset {
            return Err(SegmentError::BadPayloadOffset);
        }
        if header.payload_size != core::mem::size_of::<T>() as u64 {
            return Err(SegmentError::BadPayloadSize);
        }
        let expected_total = expected_offset + core::mem::size_of::<T>();
        if header.total_size != expected_total as u64 {
            return Err(SegmentError::BadTotalSize);
        }
        Ok(())
    }

    /// Create a new segment file, write the header, and initialise the payload.
    pub fn create_rw(path: &Path) -> Result<Self, SegmentError> {
        let header = Self::header_for_type();
        let total_size = header.total_size;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        file.set_len(total_size)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Write header bytes.
        let header_bytes: &[u8; 64] =
            unsafe { &*((&header) as *const SegmentHeader64B as *const [u8; 64]) };
        mmap[..64].copy_from_slice(header_bytes);

        // Initialise payload via T::init_payload.
        let payload_offset = header.payload_offset as usize;
        let payload_ptr = mmap[payload_offset..].as_mut_ptr() as *mut T;
        T::init_payload(payload_ptr);

        mmap.flush()?;

        Ok(Self {
            inner: MmapInner::ReadWrite(mmap),
            payload_offset,
            _phantom: PhantomData,
        })
    }

    /// Open an existing segment file read-write and validate its header.
    pub fn open_rw(path: &Path) -> Result<Self, SegmentError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let metadata = file.metadata()?;
        if metadata.len() < 64 {
            return Err(SegmentError::FileTooSmall);
        }

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let header: SegmentHeader64B =
            unsafe { core::ptr::read(mmap.as_ptr() as *const SegmentHeader64B) };
        Self::validate_header(&header)?;

        if metadata.len() < header.total_size {
            return Err(SegmentError::FileTooSmall);
        }

        Ok(Self {
            payload_offset: header.payload_offset as usize,
            inner: MmapInner::ReadWrite(mmap),
            _phantom: PhantomData,
        })
    }

    /// Open an existing segment file read-only and validate its header.
    pub fn open_ro(path: &Path) -> Result<Self, SegmentError> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if metadata.len() < 64 {
            return Err(SegmentError::FileTooSmall);
        }

        let mmap = unsafe { Mmap::map(&file)? };

        let header: SegmentHeader64B =
            unsafe { core::ptr::read(mmap.as_ptr() as *const SegmentHeader64B) };
        Self::validate_header(&header)?;

        if metadata.len() < header.total_size {
            return Err(SegmentError::FileTooSmall);
        }

        Ok(Self {
            payload_offset: header.payload_offset as usize,
            inner: MmapInner::ReadOnly(mmap),
            _phantom: PhantomData,
        })
    }

    /// Get a shared reference to the payload.
    pub fn payload(&self) -> &T {
        let base = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe { &*(base.add(self.payload_offset) as *const T) }
    }

    /// Get a mutable reference to the payload (read-write mappings only).
    ///
    /// # Panics
    ///
    /// Panics if the segment was opened read-only.
    pub fn payload_mut(&self) -> &mut T {
        match &self.inner {
            MmapInner::ReadWrite(m) => unsafe {
                &mut *(m.as_ptr().add(self.payload_offset) as *mut T)
            },
            MmapInner::ReadOnly(_) => panic!("payload_mut called on read-only segment"),
        }
    }

    /// Flush dirty pages to the backing file.
    pub fn flush(&self) -> Result<(), io::Error> {
        match &self.inner {
            MmapInner::ReadWrite(m) => m.flush(),
            MmapInner::ReadOnly(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockhash::{BlockhashPayload56B, BlockhashSnapshot};
    use crate::leader_schedule::{EngineTag, LeaderSchedulePayload};
    use crate::risk_flags::RiskFlags64B;
    use std::sync::atomic::Ordering;

    #[test]
    fn header_size_is_64_bytes() {
        assert_eq!(core::mem::size_of::<SegmentHeader64B>(), 64);
    }

    #[test]
    fn header_alignment_is_64() {
        assert_eq!(core::mem::align_of::<SegmentHeader64B>(), 64);
    }

    #[test]
    fn align_up_powers_of_two() {
        assert_eq!(align_up(1, 64), 64);
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(65, 64), 128);
        assert_eq!(align_up(0, 128), 0);
        assert_eq!(align_up(1, 128), 128);
        assert_eq!(align_up(64, 128), 128);
        assert_eq!(align_up(129, 128), 256);
    }

    #[test]
    fn align_up_already_aligned_is_noop() {
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(128, 128), 128);
        assert_eq!(align_up(256, 128), 256);
        assert_eq!(align_up(0, 64), 0);
    }

    #[test]
    fn blockhash_create_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blockhash");

        let seg = MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap();
        let bh = seg.payload();

        // Fresh: read_active returns None (never published, last_update_unix_ns == 0).
        assert!(bh.read_active().is_none());

        // Write a real payload.
        let p = BlockhashPayload56B {
            blockhash: [0xCC; 32],
            blockhash_slot: 99,
            last_update_unix_ns: 7777,
            next_scheduled_slot: 100,
        };
        bh.slot0.write_payload(&p);
        bh.active_idx.active_idx.store(0, Ordering::Release);

        let got = bh.read_active().unwrap();
        assert_eq!(got.next_scheduled_slot, 100);
        assert_eq!(got.blockhash, [0xCC; 32]);
    }

    #[test]
    fn leader_sched_create_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leader_sched");

        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&path).unwrap();
        let ls = seg.payload();

        // Fresh: all 0xFF, engine_for_slot returns None.
        assert_eq!(ls.engine_for_slot(0), None);
        assert_eq!(ls.engine_for_slot(999_999), None);

        // Write a tag and read it back.
        ls.set_engine_for_slot(42, EngineTag::Jito);
        assert_eq!(ls.engine_for_slot(42), Some(EngineTag::Jito));
    }

    #[test]
    fn risk_flags_create_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk_flags");

        let seg = MappedSegment::<RiskFlags64B>::create_rw(&path).unwrap();
        let rf = seg.payload();

        // Fresh: not quarantined.
        assert!(!rf.harmonic_quarantined());

        // Set quarantine and read back.
        rf.set_harmonic_quarantined(true);
        assert!(rf.harmonic_quarantined());
    }

    #[test]
    fn open_wrong_segment_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blockhash_seg");

        // Create a blockhash segment (segment_id = 1).
        drop(MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap());

        // Try to open as leader schedule (segment_id = 2).
        let result = MappedSegment::<LeaderSchedulePayload>::open_ro(&path);
        assert!(matches!(result, Err(SegmentError::WrongSegmentId { .. })));
    }

    #[test]
    fn open_wrong_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_version");

        drop(MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap());

        // Corrupt the version field (offset 52).
        let mut data = std::fs::read(&path).unwrap();
        data[52] = 99;
        data[53] = 0;
        std::fs::write(&path, &data).unwrap();

        let result = MappedSegment::<BlockhashSnapshot>::open_ro(&path);
        assert!(matches!(result, Err(SegmentError::WrongVersion { .. })));
    }

    #[test]
    fn open_wrong_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_magic");

        // Write a file with wrong magic bytes.
        std::fs::write(&path, [0u8; 384]).unwrap();

        let result = MappedSegment::<BlockhashSnapshot>::open_ro(&path);
        assert!(matches!(result, Err(SegmentError::BadMagic)));
    }

    #[test]
    fn open_truncated_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated");

        // Write only 32 bytes — too small for header.
        std::fs::write(&path, [0u8; 32]).unwrap();

        let result = MappedSegment::<BlockhashSnapshot>::open_ro(&path);
        assert!(matches!(result, Err(SegmentError::FileTooSmall)));
    }

    #[test]
    fn blockhash_fresh_read_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh_bh");

        let seg = MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap();
        assert!(
            seg.payload().read_active().is_none(),
            "never-published segment must return None"
        );
    }

    #[test]
    fn leader_sched_unwritten_slot_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh_ls");

        let seg = MappedSegment::<LeaderSchedulePayload>::create_rw(&path).unwrap();
        // All bytes are 0xFF — not a valid EngineTag.
        assert_eq!(seg.payload().engine_for_slot(0), None);
        assert_eq!(seg.payload().engine_for_slot(500_000), None);
        assert_eq!(seg.payload().engine_for_slot(1_048_575), None);
    }

    #[test]
    fn cross_mapping_smoke_test() {
        let dir = tempfile::tempdir().unwrap();

        // Create a blockhash segment.
        let bh_path = dir.path().join("bh");
        drop(MappedSegment::<BlockhashSnapshot>::create_rw(&bh_path).unwrap());

        // Try to open as leader schedule — must fail.
        assert!(matches!(
            MappedSegment::<LeaderSchedulePayload>::open_ro(&bh_path),
            Err(SegmentError::WrongSegmentId { .. })
        ));

        // Create a risk flags segment.
        let rf_path = dir.path().join("rf");
        drop(MappedSegment::<RiskFlags64B>::create_rw(&rf_path).unwrap());

        // Try to open as blockhash — must fail.
        assert!(matches!(
            MappedSegment::<BlockhashSnapshot>::open_ro(&rf_path),
            Err(SegmentError::WrongSegmentId { .. })
        ));
    }

    #[test]
    fn blockhash_open_ro_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bh_ro");

        // Create segment and write a valid payload.
        {
            let seg = MappedSegment::<BlockhashSnapshot>::create_rw(&path).unwrap();
            let bh = seg.payload();
            let p = BlockhashPayload56B {
                blockhash: [0xBB; 32],
                blockhash_slot: 10,
                last_update_unix_ns: 5000,
                next_scheduled_slot: 11,
            };
            bh.slot0.write_payload(&p);
            bh.active_idx.active_idx.store(0, Ordering::Release);
            seg.flush().unwrap();
        }

        // Open read-only and verify.
        let seg = MappedSegment::<BlockhashSnapshot>::open_ro(&path).unwrap();
        let got = seg.payload().read_active().unwrap();
        assert_eq!(got.next_scheduled_slot, 11);
        assert_eq!(got.blockhash, [0xBB; 32]);
    }
}

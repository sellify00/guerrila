use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const POOL_META_SEGMENT_ID: u64 = 16;
pub const POOL_META_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// Pool Metadata Payload Schema
// ---------------------------------------------------------------------------

/// Per-pool vault and mint pubkeys published by shadow-state.
///
/// Layout: 5 x 32-byte pubkeys = 160 bytes.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PoolMetaPayload160B {
    pub pool_pubkey: [u8; 32],
    pub coin_vault: [u8; 32],
    pub pc_vault: [u8; 32],
    pub coin_mint: [u8; 32],
    pub pc_mint: [u8; 32],
}

static_assertions::assert_eq_size!(PoolMetaPayload160B, [u8; 160]);

impl Default for PoolMetaPayload160B {
    fn default() -> Self {
        Self {
            pool_pubkey: [0; 32],
            coin_vault: [0; 32],
            pc_vault: [0; 32],
            coin_mint: [0; 32],
            pc_mint: [0; 32],
        }
    }
}

/// Seqlock-protected slot for a single pool metadata entry.
///
/// Padded to 256 bytes to contain the 160-byte payload + seqlock overhead
/// within a single element boundary.
#[repr(C, align(256))]
pub struct PoolMetaSlot256B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<PoolMetaPayload160B>,
    _pad: [u8; 88],
}

// 4 (sequence) + 4 (flags) + 160 (payload) + 88 (pad) = 256
static_assertions::assert_eq_size!(PoolMetaSlot256B, [u8; 256]);

unsafe impl Send for PoolMetaSlot256B {}
unsafe impl Sync for PoolMetaSlot256B {}

impl Default for PoolMetaSlot256B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(PoolMetaPayload160B::default()),
            _pad: [0; 88],
        }
    }
}

// ---------------------------------------------------------------------------
// Pool Meta Arena Element Layout
// ---------------------------------------------------------------------------

/// RCU double-buffered element: ActiveIdx + 2 seqlock slots.
///
/// Total: 128 (ActiveIdx128B) + 2 x 256 (slots) = 640 bytes.
/// Aligned to 256 for cache-line friendliness.
#[repr(C, align(256))]
pub struct PoolMetaArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [PoolMetaSlot256B; 2],
}

// 128 + 2 * 256 = 640, padded to 768 by align(256)
static_assertions::assert_eq_size!(PoolMetaArenaElement, [u8; 768]);

impl PoolMetaArenaElement {
    /// Wait-free tear-detecting reader for the active slot.
    #[inline]
    pub fn read_active(&self) -> Option<PoolMetaPayload160B> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None;
        }
        let slot = &self.slots[active as usize];

        let seq1 = slot.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None; // Writer in progress
        }

        let snapshot = unsafe { core::ptr::read_volatile(slot.payload.get()) };

        core::sync::atomic::fence(Ordering::Acquire);

        let seq2 = slot.sequence.load(Ordering::Relaxed);

        if seq1 != seq2 || (seq2 & 1) != 0 {
            return None;
        }

        Some(snapshot)
    }
}

// ---------------------------------------------------------------------------
// Mapped Arena
// ---------------------------------------------------------------------------

pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedPoolMetaArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum PoolMetaArenaError {
    Io(io::Error),
    BadMagic,
    WrongSegmentId,
    WrongVersion,
    WrongHeaderSize,
    WrongPayloadAlign,
    WrongPayloadSize { expected: u64, found: u64 },
    WrongTotalSize { expected: u64, found: u64 },
    WrongEntriesOffset { expected: u32, found: u32 },
    FileTooSmall,
}

impl From<io::Error> for PoolMetaArenaError {
    fn from(e: io::Error) -> Self {
        PoolMetaArenaError::Io(e)
    }
}

impl MappedPoolMetaArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, PoolMetaArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size = (capacity as usize) * core::mem::size_of::<PoolMetaArenaElement>();
        let total_size = elements_offset + elements_size;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_size as u64)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        let header = SegmentHeader64B {
            magic: SEGMENT_MAGIC,
            build_git_sha_prefix: [0; 8],
            checksum: 0,
            segment_id: POOL_META_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: POOL_META_VERSION,
            header_size: core::mem::size_of::<SegmentHeader64B>() as u16,
            payload_align: 256,
            reserved_u16: 0,
            reserved_u32: 0,
        };

        unsafe {
            core::ptr::write(mmap.as_mut_ptr() as *mut SegmentHeader64B, header);
            core::ptr::write_bytes(mmap.as_mut_ptr().add(elements_offset), 0, elements_size);
        }

        mmap.flush()?;

        Ok(Self {
            inner: MmapInner::ReadWrite(mmap),
            capacity,
            elements_offset,
        })
    }

    fn open_common(file: &File, write: bool) -> Result<Self, PoolMetaArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(PoolMetaArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(PoolMetaArenaError::BadMagic);
        }
        if header.segment_id != POOL_META_SEGMENT_ID {
            return Err(PoolMetaArenaError::WrongSegmentId);
        }
        if header.version != POOL_META_VERSION {
            return Err(PoolMetaArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(PoolMetaArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(PoolMetaArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(PoolMetaArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(PoolMetaArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<PoolMetaArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            return Err(PoolMetaArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;

        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(PoolMetaArenaError::WrongTotalSize {
                expected: expected_total,
                found: header.total_size,
            });
        }

        let inner = if write {
            MmapInner::ReadWrite(unsafe { memmap2::MmapMut::map_mut(file)? })
        } else {
            MmapInner::ReadOnly(unsafe { memmap2::Mmap::map(file)? })
        };

        Ok(Self {
            inner,
            capacity,
            elements_offset,
        })
    }

    pub fn open_rw(path: &Path) -> Result<Self, PoolMetaArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, PoolMetaArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[PoolMetaArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const PoolMetaArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [PoolMetaArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut PoolMetaArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn flush(&self) -> Result<(), io::Error> {
        match &self.inner {
            MmapInner::ReadWrite(m) => m.flush(),
            MmapInner::ReadOnly(_) => Ok(()),
        }
    }
}

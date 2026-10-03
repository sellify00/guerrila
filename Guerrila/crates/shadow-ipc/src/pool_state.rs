use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const POOL_STATE_SEGMENT_ID: u64 = 12;
pub const POOL_STATE_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// Pool State Hot Slot Schema
// ---------------------------------------------------------------------------
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PoolStatePayload56B {
    pub reserve_in: u64,
    pub reserve_out: u64,
    pub last_update_unix_ns: u64,
    pub pool_pubkey: [u8; 32],
}

static_assertions::assert_eq_size!(PoolStatePayload56B, [u8; 56]);

impl Default for PoolStatePayload56B {
    fn default() -> Self {
        Self {
            reserve_in: 0,
            reserve_out: 0,
            last_update_unix_ns: 0,
            pool_pubkey: [0; 32],
        }
    }
}

#[repr(C, align(64))]
pub struct PoolStateSlot64B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<PoolStatePayload56B>,
}

static_assertions::assert_eq_size!(PoolStateSlot64B, [u8; 64]);

unsafe impl Send for PoolStateSlot64B {}
unsafe impl Sync for PoolStateSlot64B {}

impl Default for PoolStateSlot64B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(PoolStatePayload56B::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// Pool Arena Element Layout
// ---------------------------------------------------------------------------
#[repr(C, align(256))]
pub struct PoolArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [PoolStateSlot64B; 2],
}

static_assertions::assert_eq_size!(PoolArenaElement, [u8; 256]);

impl PoolArenaElement {
    /// Wait-free tear-detecting reader for the hot slot.
    #[inline]
    pub fn read_active(&self) -> Option<PoolStatePayload56B> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None; // Fail closed on corruption
        }
        let slot = &self.slots[active as usize];

        // 1. Load sequence 1
        let seq1 = slot.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None; // Writer in progress
        }

        // 2. Volatile copy
        let snapshot = unsafe { core::ptr::read_volatile(slot.payload.get()) };

        core::sync::atomic::fence(Ordering::Acquire);

        // 3. Load sequence 2
        let seq2 = slot.sequence.load(Ordering::Relaxed);

        // 4. Tear detection checking
        if seq1 != seq2 || (seq2 & 1) != 0 {
            return None;
        }

        Some(snapshot)
    }
}

// ---------------------------------------------------------------------------
// Wait-Free Arena Layout Builder
// ---------------------------------------------------------------------------
pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedPoolStateArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum PoolArenaError {
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

impl From<io::Error> for PoolArenaError {
    fn from(e: io::Error) -> Self {
        PoolArenaError::Io(e)
    }
}

impl MappedPoolStateArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, PoolArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size = (capacity as usize) * core::mem::size_of::<PoolArenaElement>();
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
            segment_id: POOL_STATE_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: POOL_STATE_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, PoolArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(PoolArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(PoolArenaError::BadMagic);
        }
        if header.segment_id != POOL_STATE_SEGMENT_ID {
            return Err(PoolArenaError::WrongSegmentId);
        }
        if header.version != POOL_STATE_VERSION {
            return Err(PoolArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(PoolArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(PoolArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(PoolArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(PoolArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<PoolArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            // Hardening: Reject if payload size isn't an exact multiple of the element size
            return Err(PoolArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;

        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(PoolArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, PoolArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, PoolArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[PoolArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const PoolArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [PoolArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut PoolArenaElement,
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

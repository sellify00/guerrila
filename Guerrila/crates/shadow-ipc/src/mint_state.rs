use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const MINT_STATE_SEGMENT_ID: u64 = 13;
pub const MINT_STATE_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// Mint State Hot Slot Schema
// ---------------------------------------------------------------------------
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MintStatePayload56B {
    pub mint_pubkey: [u8; 32],
    pub maximum_fee_lamports: u64,
    pub last_update_unix_ns: u64,
    pub transfer_fee_basis_points: u16,
    pub decimals: u8,
    pub owner_program: u8, // 0 = Token, 1 = Token-2022
    pub _pad: [u8; 4],
}

static_assertions::assert_eq_size!(MintStatePayload56B, [u8; 56]);

impl Default for MintStatePayload56B {
    fn default() -> Self {
        Self {
            mint_pubkey: [0; 32],
            maximum_fee_lamports: 0,
            last_update_unix_ns: 0,
            transfer_fee_basis_points: 0,
            decimals: 0,
            owner_program: 0,
            _pad: [0; 4],
        }
    }
}

#[repr(C, align(64))]
pub struct MintStateSlot64B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<MintStatePayload56B>,
}

static_assertions::assert_eq_size!(MintStateSlot64B, [u8; 64]);

unsafe impl Send for MintStateSlot64B {}
unsafe impl Sync for MintStateSlot64B {}

impl Default for MintStateSlot64B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(MintStatePayload56B::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// Mint Arena Element Layout
// ---------------------------------------------------------------------------
#[repr(C, align(256))]
pub struct MintArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [MintStateSlot64B; 2],
}

static_assertions::assert_eq_size!(MintArenaElement, [u8; 256]);

impl MintArenaElement {
    /// Wait-free tear-detecting reader for the hot slot.
    #[inline]
    pub fn read_active(&self) -> Option<MintStatePayload56B> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None; // Fail closed on corruption
        }
        let slot = &self.slots[active as usize];

        let seq1 = slot.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None;
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
// Wait-Free Arena Layout Builder
// ---------------------------------------------------------------------------
pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedMintStateArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum MintArenaError {
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

impl From<io::Error> for MintArenaError {
    fn from(e: io::Error) -> Self {
        MintArenaError::Io(e)
    }
}

impl MappedMintStateArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, MintArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size = (capacity as usize) * core::mem::size_of::<MintArenaElement>();
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
            segment_id: MINT_STATE_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: MINT_STATE_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, MintArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(MintArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(MintArenaError::BadMagic);
        }
        if header.segment_id != MINT_STATE_SEGMENT_ID {
            return Err(MintArenaError::WrongSegmentId);
        }
        if header.version != MINT_STATE_VERSION {
            return Err(MintArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(MintArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(MintArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(MintArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(MintArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<MintArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            // Hardening: Reject if payload size isn't an exact multiple of the element
            return Err(MintArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;

        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(MintArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, MintArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, MintArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[MintArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const MintArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [MintArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut MintArenaElement,
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

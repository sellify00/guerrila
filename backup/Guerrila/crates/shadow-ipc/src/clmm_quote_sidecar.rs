use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const CLMM_QUOTE_CORE_SEGMENT_ID: u64 = 29;
pub const CLMM_QUOTE_CORE_VERSION: u16 = 1;

pub const CLMM_QUOTE_FLAG_CORE_FIELDS_VALID: u8 = 1 << 0;
pub const CLMM_QUOTE_FLAG_FEE_RATE_UNAVAILABLE: u8 = 1 << 1;
pub const CLMM_QUOTE_FLAG_TICK_ARRAYS_UNAVAILABLE: u8 = 1 << 2;

/// Per-pool Raydium CLMM quote-core fields published by shadow-state.
///
/// These are the stable `PoolState` fields already available from the pool
/// account itself. They intentionally do not include decoded tick arrays or
/// amm-config fee values, which must be sourced separately before exact CLMM
/// quote support can be enabled.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClmmQuoteCorePayload128B {
    pub pool_pubkey: [u8; 32],
    pub amm_config_pubkey: [u8; 32],
    pub sqrt_price_x64: u128,
    pub liquidity: u128,
    pub last_update_unix_ns: u64,
    pub trade_fee_rate: u32,
    pub tick_current: i32,
    pub tick_spacing: u16,
    pub mint_decimals_0: u8,
    pub mint_decimals_1: u8,
    pub pool_status: u8,
    pub flags: u8,
    pub reserved: [u8; 10],
}

static_assertions::assert_eq_size!(ClmmQuoteCorePayload128B, [u8; 128]);

impl Default for ClmmQuoteCorePayload128B {
    fn default() -> Self {
        Self {
            pool_pubkey: [0; 32],
            amm_config_pubkey: [0; 32],
            sqrt_price_x64: 0,
            liquidity: 0,
            last_update_unix_ns: 0,
            trade_fee_rate: 0,
            tick_current: 0,
            tick_spacing: 0,
            mint_decimals_0: 0,
            mint_decimals_1: 0,
            pool_status: 0,
            flags: 0,
            reserved: [0; 10],
        }
    }
}

#[repr(C, align(256))]
pub struct ClmmQuoteCoreSlot256B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<ClmmQuoteCorePayload128B>,
    _pad: [u8; 112],
}

static_assertions::assert_eq_size!(ClmmQuoteCoreSlot256B, [u8; 256]);

unsafe impl Send for ClmmQuoteCoreSlot256B {}
unsafe impl Sync for ClmmQuoteCoreSlot256B {}

impl Default for ClmmQuoteCoreSlot256B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(ClmmQuoteCorePayload128B::default()),
            _pad: [0; 112],
        }
    }
}

#[repr(C, align(256))]
pub struct ClmmQuoteCoreArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [ClmmQuoteCoreSlot256B; 2],
}

static_assertions::assert_eq_size!(ClmmQuoteCoreArenaElement, [u8; 768]);

impl ClmmQuoteCoreArenaElement {
    #[inline]
    pub fn read_active(&self) -> Option<ClmmQuoteCorePayload128B> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None;
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

pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedClmmQuoteCoreArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum ClmmQuoteCoreArenaError {
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

impl From<io::Error> for ClmmQuoteCoreArenaError {
    fn from(e: io::Error) -> Self {
        ClmmQuoteCoreArenaError::Io(e)
    }
}

impl MappedClmmQuoteCoreArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, ClmmQuoteCoreArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size = (capacity as usize) * core::mem::size_of::<ClmmQuoteCoreArenaElement>();
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
            segment_id: CLMM_QUOTE_CORE_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: CLMM_QUOTE_CORE_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, ClmmQuoteCoreArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(ClmmQuoteCoreArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(ClmmQuoteCoreArenaError::BadMagic);
        }
        if header.segment_id != CLMM_QUOTE_CORE_SEGMENT_ID {
            return Err(ClmmQuoteCoreArenaError::WrongSegmentId);
        }
        if header.version != CLMM_QUOTE_CORE_VERSION {
            return Err(ClmmQuoteCoreArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(ClmmQuoteCoreArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(ClmmQuoteCoreArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(ClmmQuoteCoreArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(ClmmQuoteCoreArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<ClmmQuoteCoreArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            return Err(ClmmQuoteCoreArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;
        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(ClmmQuoteCoreArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, ClmmQuoteCoreArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, ClmmQuoteCoreArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[ClmmQuoteCoreArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const ClmmQuoteCoreArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [ClmmQuoteCoreArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut ClmmQuoteCoreArenaElement,
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

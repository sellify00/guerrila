use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const CLMM_EXACT_QUOTE_SEGMENT_ID: u64 = 30;
pub const CLMM_EXACT_QUOTE_VERSION: u16 = 2;

pub const CLMM_EXACT_QUOTE_FLAG_VALID: u8 = 1 << 0;
pub const CLMM_EXACT_QUOTE_FLAG_NO_QUOTE: u8 = 1 << 1;

pub const CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED: u8 = 0;
pub const CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM: u8 = 1;
pub const CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY: u8 = 2;
pub const CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE: u8 = 3;
pub const CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL: u8 = 4;

#[inline]
pub fn clmm_exact_quote_status_str(status: u8) -> &'static str {
    match status {
        CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED => "unspecified",
        CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM => "no_current_array_upstream",
        CLMM_EXACT_QUOTE_STATUS_NO_LIQUIDITY_BOUNDARY => "no_liquidity_boundary",
        CLMM_EXACT_QUOTE_STATUS_OUT_OF_RANGE => "out_of_range",
        CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL => "missing_tick_array_local",
        _ => "unknown",
    }
}

/// Per-pool exact-input Raydium CLMM quote result published off the worker hot path.
///
/// This segment is keyed by `pool_index`. Workers consume it read-only and fail
/// closed unless the active payload is valid, fresh, and matches the exact
/// `(pool, mint_in, mint_out, amount_in, direction)` they are about to build.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClmmExactQuotePayload192B {
    pub pool_pubkey: [u8; 32],
    pub mint_in_pubkey: [u8; 32],
    pub mint_out_pubkey: [u8; 32],
    pub amount_in: u64,
    pub expected_out: u64,
    pub min_out: u64,
    pub projected_reserve_in: u64,
    pub projected_reserve_out: u64,
    pub quote_epoch: u64,
    pub last_update_unix_ns: u64,
    pub post_sqrt_price_x64: u128,
    pub post_tick: i32,
    pub crossed_tick_count: u16,
    pub direction: u8,
    pub validity_flags: u8,
    pub status: u8,
    pub reserved: [u8; 7],
}

static_assertions::assert_eq_size!(ClmmExactQuotePayload192B, [u8; 192]);

impl Default for ClmmExactQuotePayload192B {
    fn default() -> Self {
        Self {
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            amount_in: 0,
            expected_out: 0,
            min_out: 0,
            projected_reserve_in: 0,
            projected_reserve_out: 0,
            quote_epoch: 0,
            last_update_unix_ns: 0,
            post_sqrt_price_x64: 0,
            post_tick: 0,
            crossed_tick_count: 0,
            direction: 0,
            validity_flags: 0,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        }
    }
}

impl ClmmExactQuotePayload192B {
    #[inline]
    pub fn request_id(&self) -> u32 {
        u32::from_le_bytes(self.reserved[..4].try_into().unwrap())
    }

    #[inline]
    pub fn set_request_id(&mut self, request_id: u32) {
        self.reserved[..4].copy_from_slice(&request_id.to_le_bytes());
    }
}

#[repr(C, align(256))]
pub struct ClmmExactQuoteSlot256B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<ClmmExactQuotePayload192B>,
    _pad: [u8; 48],
}

static_assertions::assert_eq_size!(ClmmExactQuoteSlot256B, [u8; 256]);

unsafe impl Send for ClmmExactQuoteSlot256B {}
unsafe impl Sync for ClmmExactQuoteSlot256B {}

impl Default for ClmmExactQuoteSlot256B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(ClmmExactQuotePayload192B::default()),
            _pad: [0; 48],
        }
    }
}

#[repr(C, align(256))]
pub struct ClmmExactQuoteArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [ClmmExactQuoteSlot256B; 2],
}

static_assertions::assert_eq_size!(ClmmExactQuoteArenaElement, [u8; 768]);

impl ClmmExactQuoteArenaElement {
    #[inline]
    pub fn read_active(&self) -> Option<ClmmExactQuotePayload192B> {
        self.read_active_with_sequence()
            .map(|(_, snapshot)| snapshot)
    }

    #[inline]
    pub fn read_active_with_sequence(&self) -> Option<(u32, ClmmExactQuotePayload192B)> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None;
        }

        self.read_slot_with_sequence(active as usize)
    }

    #[inline]
    fn read_slot_with_sequence(
        &self,
        slot_index: usize,
    ) -> Option<(u32, ClmmExactQuotePayload192B)> {
        let slot = &self.slots[slot_index];

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

        Some((seq2, snapshot))
    }

    #[inline]
    pub fn read_stable_with_sequences(&self) -> [Option<(u32, ClmmExactQuotePayload192B)>; 2] {
        [
            self.read_slot_with_sequence(0),
            self.read_slot_with_sequence(1),
        ]
    }
}

enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedClmmExactQuoteArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum ClmmExactQuoteArenaError {
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

impl From<io::Error> for ClmmExactQuoteArenaError {
    fn from(e: io::Error) -> Self {
        ClmmExactQuoteArenaError::Io(e)
    }
}

impl MappedClmmExactQuoteArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, ClmmExactQuoteArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size =
            (capacity as usize) * core::mem::size_of::<ClmmExactQuoteArenaElement>();
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
            segment_id: CLMM_EXACT_QUOTE_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: CLMM_EXACT_QUOTE_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, ClmmExactQuoteArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(ClmmExactQuoteArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(ClmmExactQuoteArenaError::BadMagic);
        }
        if header.segment_id != CLMM_EXACT_QUOTE_SEGMENT_ID {
            return Err(ClmmExactQuoteArenaError::WrongSegmentId);
        }
        if header.version != CLMM_EXACT_QUOTE_VERSION {
            return Err(ClmmExactQuoteArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(ClmmExactQuoteArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(ClmmExactQuoteArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(ClmmExactQuoteArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(ClmmExactQuoteArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<ClmmExactQuoteArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            return Err(ClmmExactQuoteArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;
        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(ClmmExactQuoteArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, ClmmExactQuoteArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, ClmmExactQuoteArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[ClmmExactQuoteArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const ClmmExactQuoteArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [ClmmExactQuoteArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut ClmmExactQuoteArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn read_active_at(&self, index: u32) -> Option<ClmmExactQuotePayload192B> {
        self.elements().get(index as usize)?.read_active()
    }

    pub fn read_stable_with_sequences_at(
        &self,
        index: u32,
    ) -> [Option<(u32, ClmmExactQuotePayload192B)>; 2] {
        self.elements()
            .get(index as usize)
            .map(ClmmExactQuoteArenaElement::read_stable_with_sequences)
            .unwrap_or([None, None])
    }

    pub fn publish_at(&mut self, index: u32, update: ClmmExactQuotePayload192B) -> Result<(), ()> {
        if index >= self.capacity {
            return Err(());
        }

        let element = &mut self.elements_mut()[index as usize];
        let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
        let inactive_idx = active_idx ^ 1;
        let slot = &mut element.slots[inactive_idx as usize];

        let seq = slot.sequence.load(Ordering::Acquire);
        slot.sequence.store(seq | 1, Ordering::Release);
        core::sync::atomic::compiler_fence(Ordering::Release);

        let payload_ptr = slot.payload.get();
        let src_ptr = &update as *const _ as *const u8;
        unsafe {
            for i in 0..core::mem::size_of::<ClmmExactQuotePayload192B>() {
                core::ptr::write_volatile(
                    payload_ptr.cast::<u8>().add(i),
                    core::ptr::read_volatile(src_ptr.add(i)),
                );
            }
        }
        core::sync::atomic::compiler_fence(Ordering::Release);

        let next_even_seq = (seq | 1).wrapping_add(1);
        slot.sequence.store(next_even_seq, Ordering::Release);
        element
            .active_idx
            .active_idx
            .store(inactive_idx, Ordering::Release);

        Ok(())
    }

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

    #[test]
    fn exact_quote_round_trip_reads_back_active_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 4).unwrap();

        let payload = ClmmExactQuotePayload192B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [8; 32],
            mint_out_pubkey: [9; 32],
            amount_in: 50_000_000,
            expected_out: 58_735_364_186,
            min_out: 57_000_000_000,
            projected_reserve_in: 342_245_016_188,
            projected_reserve_out: 397_235_503_043_457,
            quote_epoch: 42,
            last_update_unix_ns: 123_456_789,
            post_sqrt_price_x64: 999,
            post_tick: -123,
            crossed_tick_count: 2,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };
        arena.publish_at(2, payload).unwrap();
        arena.flush().unwrap();

        let arena_ro = MappedClmmExactQuoteArena::open_ro(&path).unwrap();
        assert_eq!(arena_ro.read_active_at(2), Some(payload));
    }

    #[test]
    fn exact_quote_stable_reads_include_previous_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote");
        let mut arena = MappedClmmExactQuoteArena::create_rw(&path, 4).unwrap();

        let payload_a = ClmmExactQuotePayload192B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [8; 32],
            mint_out_pubkey: [9; 32],
            amount_in: 50_000_000,
            expected_out: 58_735_364_186,
            min_out: 57_000_000_000,
            projected_reserve_in: 1,
            projected_reserve_out: 2,
            quote_epoch: 41,
            last_update_unix_ns: 123_456_700,
            post_sqrt_price_x64: 998,
            post_tick: -124,
            crossed_tick_count: 1,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_VALID,
            status: CLMM_EXACT_QUOTE_STATUS_UNSPECIFIED,
            reserved: [0; 7],
        };
        let payload_b = ClmmExactQuotePayload192B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [9; 32],
            mint_out_pubkey: [8; 32],
            amount_in: 60_000_000,
            expected_out: 68_735_364_186,
            min_out: 67_000_000_000,
            projected_reserve_in: 3,
            projected_reserve_out: 4,
            quote_epoch: 42,
            last_update_unix_ns: 123_456_789,
            post_sqrt_price_x64: 999,
            post_tick: -123,
            crossed_tick_count: 2,
            direction: 1,
            validity_flags: CLMM_EXACT_QUOTE_FLAG_NO_QUOTE,
            status: CLMM_EXACT_QUOTE_STATUS_NO_CURRENT_ARRAY_UPSTREAM,
            reserved: [0; 7],
        };
        arena.publish_at(2, payload_a).unwrap();
        arena.publish_at(2, payload_b).unwrap();
        arena.flush().unwrap();

        let arena_ro = MappedClmmExactQuoteArena::open_ro(&path).unwrap();
        let stable = arena_ro.read_stable_with_sequences_at(2);
        let snapshots: Vec<_> = stable
            .into_iter()
            .flatten()
            .map(|(_, payload)| payload)
            .collect();
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots.contains(&payload_a));
        assert!(snapshots.contains(&payload_b));
        assert_eq!(arena_ro.read_active_at(2), Some(payload_b));
    }
}

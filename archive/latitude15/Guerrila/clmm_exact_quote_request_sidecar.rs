use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::sync::atomic::{AtomicU32, Ordering};
use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const CLMM_EXACT_QUOTE_REQUEST_SEGMENT_ID: u64 = 31;
pub const CLMM_EXACT_QUOTE_REQUEST_VERSION: u16 = 1;

pub const CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID: u8 = 1 << 0;

/// Per-pool CLMM exact-quote request published by the worker hot path.
///
/// Workers write the concrete tuple they need quoted; `shadow-state` reads the
/// latest request and publishes the matching exact quote into
/// `clmm_exact_quote`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClmmExactQuoteRequestPayload128B {
    pub pool_pubkey: [u8; 32],
    pub mint_in_pubkey: [u8; 32],
    pub mint_out_pubkey: [u8; 32],
    pub target_amount_in: u64,
    pub amount_in: u64,
    pub request_unix_ns: u64,
    pub slip_bps: u16,
    pub direction: u8,
    pub validity_flags: u8,
    pub reserved: [u8; 4],
}

static_assertions::assert_eq_size!(ClmmExactQuoteRequestPayload128B, [u8; 128]);

impl Default for ClmmExactQuoteRequestPayload128B {
    fn default() -> Self {
        Self {
            pool_pubkey: [0; 32],
            mint_in_pubkey: [0; 32],
            mint_out_pubkey: [0; 32],
            target_amount_in: 0,
            amount_in: 0,
            request_unix_ns: 0,
            slip_bps: 0,
            direction: 0,
            validity_flags: 0,
            reserved: [0; 4],
        }
    }
}

impl ClmmExactQuoteRequestPayload128B {
    #[inline]
    pub fn request_id(&self) -> u32 {
        u32::from_le_bytes(self.reserved)
    }

    #[inline]
    pub fn set_request_id(&mut self, request_id: u32) {
        self.reserved = request_id.to_le_bytes();
    }
}

#[repr(C, align(256))]
pub struct ClmmExactQuoteRequestSlot256B {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<ClmmExactQuoteRequestPayload128B>,
    _pad: [u8; 120],
}

static_assertions::assert_eq_size!(ClmmExactQuoteRequestSlot256B, [u8; 256]);

unsafe impl Send for ClmmExactQuoteRequestSlot256B {}
unsafe impl Sync for ClmmExactQuoteRequestSlot256B {}

impl Default for ClmmExactQuoteRequestSlot256B {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(ClmmExactQuoteRequestPayload128B::default()),
            _pad: [0; 120],
        }
    }
}

#[repr(C, align(256))]
pub struct ClmmExactQuoteRequestArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [ClmmExactQuoteRequestSlot256B; 2],
}

static_assertions::assert_eq_size!(ClmmExactQuoteRequestArenaElement, [u8; 768]);

impl ClmmExactQuoteRequestArenaElement {
    #[inline]
    pub fn read_active(&self) -> Option<ClmmExactQuoteRequestPayload128B> {
        self.read_active_with_sequence()
            .map(|(_, snapshot)| snapshot)
    }

    #[inline]
    pub fn read_active_with_sequence(&self) -> Option<(u32, ClmmExactQuoteRequestPayload128B)> {
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
    ) -> Option<(u32, ClmmExactQuoteRequestPayload128B)> {
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
    pub fn read_stable_with_sequences(
        &self,
    ) -> [Option<(u32, ClmmExactQuoteRequestPayload128B)>; 2] {
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

pub struct MappedClmmExactQuoteRequestArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum ClmmExactQuoteRequestArenaError {
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

impl From<io::Error> for ClmmExactQuoteRequestArenaError {
    fn from(e: io::Error) -> Self {
        ClmmExactQuoteRequestArenaError::Io(e)
    }
}

impl MappedClmmExactQuoteRequestArena {
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, ClmmExactQuoteRequestArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size =
            (capacity as usize) * core::mem::size_of::<ClmmExactQuoteRequestArenaElement>();
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
            segment_id: CLMM_EXACT_QUOTE_REQUEST_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: CLMM_EXACT_QUOTE_REQUEST_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, ClmmExactQuoteRequestArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(ClmmExactQuoteRequestArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(ClmmExactQuoteRequestArenaError::BadMagic);
        }
        if header.segment_id != CLMM_EXACT_QUOTE_REQUEST_SEGMENT_ID {
            return Err(ClmmExactQuoteRequestArenaError::WrongSegmentId);
        }
        if header.version != CLMM_EXACT_QUOTE_REQUEST_VERSION {
            return Err(ClmmExactQuoteRequestArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(ClmmExactQuoteRequestArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(ClmmExactQuoteRequestArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(ClmmExactQuoteRequestArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(ClmmExactQuoteRequestArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<ClmmExactQuoteRequestArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            return Err(ClmmExactQuoteRequestArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;
        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(ClmmExactQuoteRequestArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, ClmmExactQuoteRequestArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, ClmmExactQuoteRequestArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[ClmmExactQuoteRequestArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const ClmmExactQuoteRequestArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn read_active_at(&self, index: u32) -> Option<ClmmExactQuoteRequestPayload128B> {
        self.elements().get(index as usize)?.read_active()
    }

    pub fn read_active_with_sequence_at(
        &self,
        index: u32,
    ) -> Option<(u32, ClmmExactQuoteRequestPayload128B)> {
        self.elements()
            .get(index as usize)?
            .read_active_with_sequence()
    }

    pub fn read_stable_with_sequences_at(
        &self,
        index: u32,
    ) -> [Option<(u32, ClmmExactQuoteRequestPayload128B)>; 2] {
        self.elements()
            .get(index as usize)
            .map(ClmmExactQuoteRequestArenaElement::read_stable_with_sequences)
            .unwrap_or([None, None])
    }

    pub fn publish_at(
        &self,
        index: u32,
        update: ClmmExactQuoteRequestPayload128B,
    ) -> Result<(), ()> {
        if index >= self.capacity {
            return Err(());
        }
        let ptr = match &self.inner {
            MmapInner::ReadOnly(_) => return Err(()),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        let element = unsafe {
            &*(ptr.add(self.elements_offset) as *const ClmmExactQuoteRequestArenaElement)
                .add(index as usize)
        };

        loop {
            let active_idx = element.active_idx.active_idx.load(Ordering::Acquire);
            let inactive_idx = active_idx ^ 1;
            let slot = &element.slots[inactive_idx as usize];

            let seq = slot.sequence.load(Ordering::Acquire);
            if seq & 1 != 0 {
                spin_loop();
                continue;
            }
            if slot
                .sequence
                .compare_exchange(seq, seq | 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                spin_loop();
                continue;
            }

            core::sync::atomic::compiler_fence(Ordering::Release);
            let payload_ptr = slot.payload.get();
            let src_ptr = &update as *const _ as *const u8;
            unsafe {
                for i in 0..core::mem::size_of::<ClmmExactQuoteRequestPayload128B>() {
                    core::ptr::write_volatile(
                        payload_ptr.cast::<u8>().add(i),
                        core::ptr::read_volatile(src_ptr.add(i)),
                    );
                }
            }
            core::sync::atomic::compiler_fence(Ordering::Release);

            slot.sequence
                .store((seq | 1).wrapping_add(1), Ordering::Release);
            element
                .active_idx
                .active_idx
                .store(inactive_idx, Ordering::Release);
            return Ok(());
        }
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
    fn exact_quote_request_round_trip_reads_back_active_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote_request");
        let arena = MappedClmmExactQuoteRequestArena::create_rw(&path, 4).unwrap();

        let payload = ClmmExactQuoteRequestPayload128B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [8; 32],
            mint_out_pubkey: [9; 32],
            target_amount_in: 25_000_000,
            amount_in: 50_000_000,
            request_unix_ns: 123_456_789,
            slip_bps: 100,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
            reserved: [0; 4],
        };
        arena.publish_at(2, payload).unwrap();
        arena.flush().unwrap();

        let arena_ro = MappedClmmExactQuoteRequestArena::open_ro(&path).unwrap();
        assert_eq!(arena_ro.read_active_at(2), Some(payload));
        let (seq, snapshot) = arena_ro.read_active_with_sequence_at(2).unwrap();
        assert!(seq != 0);
        assert_eq!(snapshot, payload);
    }

    #[test]
    fn exact_quote_request_stable_reads_include_previous_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clmm_exact_quote_request");
        let arena = MappedClmmExactQuoteRequestArena::create_rw(&path, 4).unwrap();

        let payload_a = ClmmExactQuoteRequestPayload128B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [8; 32],
            mint_out_pubkey: [9; 32],
            target_amount_in: 25_000_000,
            amount_in: 50_000_000,
            request_unix_ns: 123_456_700,
            slip_bps: 100,
            direction: 0,
            validity_flags: CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
            reserved: [0; 4],
        };
        let payload_b = ClmmExactQuoteRequestPayload128B {
            pool_pubkey: [7; 32],
            mint_in_pubkey: [9; 32],
            mint_out_pubkey: [8; 32],
            target_amount_in: 35_000_000,
            amount_in: 60_000_000,
            request_unix_ns: 123_456_789,
            slip_bps: 50,
            direction: 1,
            validity_flags: CLMM_EXACT_QUOTE_REQUEST_FLAG_VALID,
            reserved: [0; 4],
        };
        arena.publish_at(2, payload_a).unwrap();
        arena.publish_at(2, payload_b).unwrap();
        arena.flush().unwrap();

        let arena_ro = MappedClmmExactQuoteRequestArena::open_ro(&path).unwrap();
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

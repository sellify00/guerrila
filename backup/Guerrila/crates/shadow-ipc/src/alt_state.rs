//! Address Lookup Table (ALT) shared-memory state arena.
//!
//! Provides the same RCU double-buffer discipline as `pool_state` and
//! `mint_state`, but with a larger fixed-size payload that inlines up
//! to 256 addresses (the Solana runtime maximum).
//!
//! Ingress reads this arena to expand ALT indices into concrete pubkeys
//! without an RPC round-trip.  Workers never touch this segment.
//!
//! ## File layout
//!
//! ```text
//! [0..64)                                      SegmentHeader64B
//! [64..256)                                    padding (align to 256)
//! [256..256 + capacity * ELEMENT_SIZE)         AltArenaElement[capacity]
//! ```
//!
//! Each element is 16_896 bytes (66 × 256), aligned to 256 bytes.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use crate::blockhash::ActiveIdx128B;
use crate::segment::{align_up, SegmentHeader64B, SEGMENT_MAGIC};

pub const ALT_STATE_SEGMENT_ID: u64 = 15;
pub const ALT_STATE_VERSION: u16 = 1;

/// Maximum number of addresses in a Solana ALT (runtime limit).
pub const ALT_MAX_ADDRESSES: usize = 256;

// ---------------------------------------------------------------------------
// ALT State Payload
// ---------------------------------------------------------------------------

/// Fixed-size payload for a single Address Lookup Table.
///
/// Contains the ALT metadata plus an inline array of up to 256
/// addresses.  Only the first `address_count` entries in `addresses`
/// are meaningful; the rest are zeroed.
///
/// ## Size
///
/// 96 bytes metadata + 8192 bytes addresses = 8288 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AltStatePayload {
    // --- 96 bytes of metadata ---
    /// The ALT account pubkey.
    pub alt_pubkey: [u8; 32],
    /// Authority pubkey — all zeros means frozen / immutable.
    pub authority: [u8; 32],
    /// Deactivation slot — `u64::MAX` means active.
    pub deactivation_slot: u64,
    /// Slot at which the table was last extended.
    pub last_extended_slot: u64,
    /// Unix-ns timestamp of the last shadow-state update.
    pub last_update_unix_ns: u64,
    /// Number of valid addresses (0..=256).
    pub address_count: u16,
    /// Start index of the most recent extension batch.
    pub last_extended_slot_start_index: u8,
    /// 1 = active, 0 = deactivated.
    pub is_active: u8,
    /// Padding to 8-byte alignment.
    pub _pad: [u8; 4],

    // --- 8192 bytes of inline addresses ---
    /// Fixed-size address array.  Only `addresses[0..address_count]`
    /// are valid; the remainder is zeroed.
    pub addresses: [[u8; 32]; ALT_MAX_ADDRESSES],
}

// 96 + 8192 = 8288
static_assertions::assert_eq_size!(AltStatePayload, [u8; 8288]);

impl Default for AltStatePayload {
    fn default() -> Self {
        // Safety: all-zeros is a valid default for every field.
        unsafe { core::mem::zeroed() }
    }
}

impl core::fmt::Debug for AltStatePayload {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AltStatePayload")
            .field("alt_pubkey", &hex_short(&self.alt_pubkey))
            .field("authority", &hex_short(&self.authority))
            .field("deactivation_slot", &self.deactivation_slot)
            .field("last_extended_slot", &self.last_extended_slot)
            .field("last_update_unix_ns", &self.last_update_unix_ns)
            .field("address_count", &self.address_count)
            .field(
                "last_extended_slot_start_index",
                &self.last_extended_slot_start_index,
            )
            .field("is_active", &self.is_active)
            .finish()
    }
}

fn hex_short(bytes: &[u8; 32]) -> String {
    format!(
        "{:02x}{:02x}..{:02x}{:02x}",
        bytes[0], bytes[1], bytes[30], bytes[31]
    )
}

// ---------------------------------------------------------------------------
// ALT State Slot  (seqlock-guarded)
// ---------------------------------------------------------------------------

/// Seqlock-guarded slot holding one `AltStatePayload`.
///
/// 4 (sequence) + 4 (flags) + 8288 (payload) = 8296 bytes,
/// rounded up to 8320 (next multiple of 64) by `align(64)`.
#[repr(C, align(64))]
pub struct AltStateSlot {
    pub sequence: AtomicU32,
    pub flags: u32,
    pub payload: UnsafeCell<AltStatePayload>,
}

// 8296 rounded to align(64) = 8320
static_assertions::assert_eq_size!(AltStateSlot, [u8; 8320]);

unsafe impl Send for AltStateSlot {}
unsafe impl Sync for AltStateSlot {}

impl Default for AltStateSlot {
    fn default() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            flags: 0,
            payload: UnsafeCell::new(AltStatePayload::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// ALT Arena Element  (double-buffered RCU)
// ---------------------------------------------------------------------------

/// One ALT entry in the arena: ActiveIdx + two seqlock slots.
///
/// 128 (ActiveIdx128B) + 2 × 8320 (AltStateSlot) = 16768,
/// rounded up to 16896 (next multiple of 256) by `align(256)`.
#[repr(C, align(256))]
pub struct AltArenaElement {
    pub active_idx: ActiveIdx128B,
    pub slots: [AltStateSlot; 2],
}

// 128 + 2*8320 = 16768, rounded to 256 = 16896
static_assertions::assert_eq_size!(AltArenaElement, [u8; 16896]);

impl AltArenaElement {
    /// Wait-free tear-detecting reader for the active slot.
    #[inline]
    pub fn read_active(&self) -> Option<AltStatePayload> {
        let active = self.active_idx.active_idx.load(Ordering::Acquire);
        if active > 1 {
            return None;
        }
        let slot = &self.slots[active as usize];

        // 1. Load sequence (pre)
        let seq1 = slot.sequence.load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            return None; // writer in progress
        }

        // 2. Volatile copy of the full payload
        let snapshot = unsafe { core::ptr::read_volatile(slot.payload.get()) };

        core::sync::atomic::fence(Ordering::Acquire);

        // 3. Load sequence (post)
        let seq2 = slot.sequence.load(Ordering::Relaxed);

        // 4. Tear detection
        if seq1 != seq2 || (seq2 & 1) != 0 {
            return None;
        }

        Some(snapshot)
    }
}

// ---------------------------------------------------------------------------
// Mapped ALT State Arena
// ---------------------------------------------------------------------------

pub enum MmapInner {
    ReadOnly(Mmap),
    ReadWrite(MmapMut),
}

pub struct MappedAltStateArena {
    inner: MmapInner,
    capacity: u32,
    elements_offset: usize,
}

#[derive(Debug)]
pub enum AltArenaError {
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

impl From<io::Error> for AltArenaError {
    fn from(e: io::Error) -> Self {
        AltArenaError::Io(e)
    }
}

impl MappedAltStateArena {
    /// Create a new ALT state arena file with the given capacity.
    pub fn create_rw(path: &Path, capacity: u32) -> Result<Self, AltArenaError> {
        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        let elements_size = (capacity as usize) * core::mem::size_of::<AltArenaElement>();
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
            segment_id: ALT_STATE_SEGMENT_ID,
            payload_size: elements_size as u64,
            total_size: total_size as u64,
            payload_offset: elements_offset as u32,
            version: ALT_STATE_VERSION,
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

    fn open_common(file: &File, write: bool) -> Result<Self, AltArenaError> {
        let meta = file.metadata()?;
        if meta.len() < core::mem::size_of::<SegmentHeader64B>() as u64 {
            return Err(AltArenaError::FileTooSmall);
        }

        let header_mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(core::mem::size_of::<SegmentHeader64B>())
                .map(file)?
        };
        let header = unsafe { &*(header_mmap.as_ptr() as *const SegmentHeader64B) };

        if header.magic != SEGMENT_MAGIC {
            return Err(AltArenaError::BadMagic);
        }
        if header.segment_id != ALT_STATE_SEGMENT_ID {
            return Err(AltArenaError::WrongSegmentId);
        }
        if header.version != ALT_STATE_VERSION {
            return Err(AltArenaError::WrongVersion);
        }
        if header.header_size != core::mem::size_of::<SegmentHeader64B>() as u16 {
            return Err(AltArenaError::WrongHeaderSize);
        }
        if header.payload_align != 256 {
            return Err(AltArenaError::WrongPayloadAlign);
        }

        let elements_offset = align_up(core::mem::size_of::<SegmentHeader64B>(), 256);
        if header.payload_offset != elements_offset as u32 {
            return Err(AltArenaError::WrongEntriesOffset {
                expected: elements_offset as u32,
                found: header.payload_offset,
            });
        }

        let expected_payload_size = meta.len() - header.payload_offset as u64;
        if header.payload_size != expected_payload_size {
            return Err(AltArenaError::WrongPayloadSize {
                expected: expected_payload_size,
                found: header.payload_size,
            });
        }

        let element_size = core::mem::size_of::<AltArenaElement>() as u64;
        if header.payload_size % element_size != 0 {
            return Err(AltArenaError::FileTooSmall);
        }

        let capacity = (header.payload_size / element_size) as u32;

        let expected_total = elements_offset as u64 + header.payload_size;
        if header.total_size != expected_total {
            return Err(AltArenaError::WrongTotalSize {
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

    pub fn open_rw(path: &Path) -> Result<Self, AltArenaError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::open_common(&file, true)
    }

    pub fn open_ro(path: &Path) -> Result<Self, AltArenaError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Self::open_common(&file, false)
    }

    pub fn elements(&self) -> &[AltArenaElement] {
        let ptr = match &self.inner {
            MmapInner::ReadOnly(m) => m.as_ptr(),
            MmapInner::ReadWrite(m) => m.as_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts(
                ptr.add(self.elements_offset) as *const AltArenaElement,
                self.capacity as usize,
            )
        }
    }

    pub fn elements_mut(&mut self) -> &mut [AltArenaElement] {
        let ptr = match &mut self.inner {
            MmapInner::ReadOnly(_) => panic!("Cannot mutate read-only mapping"),
            MmapInner::ReadWrite(m) => m.as_mut_ptr(),
        };
        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.add(self.elements_offset) as *mut AltArenaElement,
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

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool_state::MappedPoolStateArena;

    // -------------------------------------------------------------------
    // Create + re-open smoke test
    // -------------------------------------------------------------------

    #[test]
    fn create_and_reopen_rw() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alt_arena");

        {
            let arena = MappedAltStateArena::create_rw(&path, 8).unwrap();
            assert_eq!(arena.capacity(), 8);
            arena.flush().unwrap();
        }

        let arena = MappedAltStateArena::open_rw(&path).unwrap();
        assert_eq!(arena.capacity(), 8);
    }

    #[test]
    fn create_and_reopen_ro() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alt_arena");

        {
            let arena = MappedAltStateArena::create_rw(&path, 4).unwrap();
            arena.flush().unwrap();
        }

        let arena = MappedAltStateArena::open_ro(&path).unwrap();
        assert_eq!(arena.capacity(), 4);
    }

    // -------------------------------------------------------------------
    // Bad magic
    // -------------------------------------------------------------------

    #[test]
    fn open_bad_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_magic");

        // Write a file large enough to contain a header, but with wrong magic.
        std::fs::write(&path, vec![0u8; 256 + 16896]).unwrap();

        let result = MappedAltStateArena::open_ro(&path);
        assert!(matches!(result, Err(AltArenaError::BadMagic)));
    }

    // -------------------------------------------------------------------
    // Wrong segment ID (cross-open from pool arena)
    // -------------------------------------------------------------------

    #[test]
    fn open_wrong_segment_id_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pool_arena");

        // Create a pool state arena (segment_id = 12).
        {
            let arena = MappedPoolStateArena::create_rw(&path, 4).unwrap();
            arena.flush().unwrap();
        }

        // Try to open as ALT arena (segment_id = 15) — must fail.
        let result = MappedAltStateArena::open_ro(&path);
        assert!(matches!(result, Err(AltArenaError::WrongSegmentId)));
    }

    // -------------------------------------------------------------------
    // Wrong version (corrupt version field)
    // -------------------------------------------------------------------

    #[test]
    fn open_wrong_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_version");

        {
            let arena = MappedAltStateArena::create_rw(&path, 4).unwrap();
            arena.flush().unwrap();
        }

        // Corrupt the version field at offset 52 in SegmentHeader64B.
        let mut data = std::fs::read(&path).unwrap();
        data[52] = 99;
        data[53] = 0;
        std::fs::write(&path, &data).unwrap();

        let result = MappedAltStateArena::open_ro(&path);
        assert!(matches!(result, Err(AltArenaError::WrongVersion)));
    }

    // -------------------------------------------------------------------
    // File too small
    // -------------------------------------------------------------------

    #[test]
    fn open_truncated_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated");

        // Write only 32 bytes — too small for header.
        std::fs::write(&path, [0u8; 32]).unwrap();

        let result = MappedAltStateArena::open_ro(&path);
        assert!(matches!(result, Err(AltArenaError::FileTooSmall)));
    }

    // -------------------------------------------------------------------
    // RCU read_active — initial state
    // -------------------------------------------------------------------

    #[test]
    fn read_active_returns_zeroed_payload_initially() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alt_arena");
        let arena = MappedAltStateArena::create_rw(&path, 2).unwrap();

        // Active index starts at 0, sequence at 0 (even = clean).
        let payload = arena.elements()[0].read_active();
        assert!(payload.is_some());
        let p = payload.unwrap();
        assert_eq!(p.alt_pubkey, [0u8; 32]);
        assert_eq!(p.address_count, 0);
        assert_eq!(p.is_active, 0);
    }
}

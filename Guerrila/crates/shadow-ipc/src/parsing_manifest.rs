use core::cell::UnsafeCell;
use core::sync::atomic::{fence, Ordering};

use crate::blockhash::ActiveIdx128B;
use crate::segment::{MappedSegment, SegmentError, SegmentSpec};

pub const PARSING_MANIFEST_MAX_ENTRIES: usize = 256;
pub const MANIFEST_FIELD_EXECUTABLE_HASH: u16 = 1;
pub const MANIFEST_FIELD_POOL_PUBKEY: u16 = 2;
pub const MANIFEST_FIELD_MINT_IN: u16 = 3;
pub const MANIFEST_FIELD_MINT_OUT: u16 = 4;
pub const MANIFEST_FIELD_DIRECTION: u16 = 5;
pub const MANIFEST_FIELD_AMOUNT_IN: u16 = 6;
pub const MANIFEST_FIELD_INSTRUCTION_DISCRIMINATOR: u16 = 7;
pub const MANIFEST_FIELD_ACCOUNT_COUNT: u16 = 8;
pub const MANIFEST_FIELD_VAULT_MARKER: u16 = 9;
pub const MANIFEST_FIELD_SIGNER_MARKER: u16 = 10;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsingManifestStatus {
    Enabled = 0,
    Disabled = 1,
    GrpcOnly = 2,
    PendingValidation = 3,
    VenueDisabled = 4,
}

impl ParsingManifestStatus {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManifestOffsetV1 {
    pub field_id: u16,
    pub byte_offset: u16,
    pub width: u8,
    pub relative_base: u8,
    pub endian: u8,
    pub flags: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsingManifestEntryV1 {
    pub venue_id: u8,
    pub parser_family: u8,
    pub status: u8,
    pub confidence_bucket: u8,

    pub parser_version: u16,
    pub offset_count: u16,

    pub executable_hash: [u8; 32],

    pub offsets: [ManifestOffsetV1; 16],

    pub min_tx_len: u16,
    pub max_tx_len: u16,
    pub last_validated_slot: u64,
    pub golden_pass_count: u16,
    pub live_pass_count: u16,

    pub reserved: [u8; 32],
}

impl ParsingManifestEntryV1 {
    pub fn offsets(&self) -> &[ManifestOffsetV1] {
        let count = (self.offset_count as usize).min(self.offsets.len());
        &self.offsets[..count]
    }
}

impl Default for ParsingManifestEntryV1 {
    fn default() -> Self {
        Self {
            venue_id: 0,
            parser_family: 0,
            status: ParsingManifestStatus::Disabled.as_u8(),
            confidence_bucket: 0,
            parser_version: 0,
            offset_count: 0,
            executable_hash: [0; 32],
            offsets: [ManifestOffsetV1::default(); 16],
            min_tx_len: 0,
            max_tx_len: 0,
            last_validated_slot: 0,
            golden_pass_count: 0,
            live_pass_count: 0,
            reserved: [0; 32],
        }
    }
}

#[repr(C, align(64))]
pub struct ParsingManifestBankV1 {
    pub publish_epoch: u64,
    pub last_update_unix_ns: u64,
    pub entry_count: u16,
    pub _pad_u16: u16,
    pub _pad_u32: u32,
    pub reserved: [u8; 40],
    pub entries: [ParsingManifestEntryV1; PARSING_MANIFEST_MAX_ENTRIES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsingManifestReadSnapshot {
    pub publish_epoch: u64,
    pub last_update_unix_ns: u64,
    pub entries: Vec<ParsingManifestEntryV1>,
}

impl ParsingManifestBankV1 {
    pub fn new() -> Self {
        Self {
            publish_epoch: 0,
            last_update_unix_ns: 0,
            entry_count: 0,
            _pad_u16: 0,
            _pad_u32: 0,
            reserved: [0; 40],
            entries: [ParsingManifestEntryV1::default(); PARSING_MANIFEST_MAX_ENTRIES],
        }
    }

    pub fn is_published(&self) -> bool {
        self.last_update_unix_ns != 0
    }

    pub fn entries(&self) -> &[ParsingManifestEntryV1] {
        let count = (self.entry_count as usize).min(PARSING_MANIFEST_MAX_ENTRIES);
        &self.entries[..count]
    }

    pub fn find_entry(&self, venue_id: u8) -> Option<&ParsingManifestEntryV1> {
        self.entries()
            .iter()
            .find(|entry| entry.venue_id == venue_id)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for ParsingManifestBankV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsingManifestWriteError {
    TooManyEntries { attempted: usize, max: usize },
}

impl core::fmt::Display for ParsingManifestWriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooManyEntries { attempted, max } => {
                write!(
                    f,
                    "too many manifest entries: attempted {attempted}, max {max}"
                )
            }
        }
    }
}

impl std::error::Error for ParsingManifestWriteError {}

/// Fixed-size G11 parsing manifest segment.
///
/// Writer discipline is RCU-style: populate the inactive bank completely,
/// then flip `active_idx` with Release ordering. Readers load the active index
/// with Acquire and therefore see either the old complete bank or the new
/// complete bank, never a partially published manifest.
#[repr(C)]
pub struct ParsingManifestSnapshot {
    pub active_idx: ActiveIdx128B,
    pub bank0: UnsafeCell<ParsingManifestBankV1>,
    pub bank1: UnsafeCell<ParsingManifestBankV1>,
}

unsafe impl Sync for ParsingManifestSnapshot {}

impl ParsingManifestSnapshot {
    pub fn new() -> Self {
        Self {
            active_idx: ActiveIdx128B::new(0xFF),
            bank0: UnsafeCell::new(ParsingManifestBankV1::new()),
            bank1: UnsafeCell::new(ParsingManifestBankV1::new()),
        }
    }

    pub fn active_manifest(&self) -> Option<&ParsingManifestBankV1> {
        let idx = self.active_idx.active_idx.load(Ordering::Acquire);
        if idx > 1 {
            return None;
        }
        let bank = unsafe {
            if idx == 0 {
                &*self.bank0.get()
            } else {
                &*self.bank1.get()
            }
        };
        if !bank.is_published() {
            return None;
        }
        Some(bank)
    }

    pub fn read_active_entries(&self) -> Option<&[ParsingManifestEntryV1]> {
        Some(self.active_manifest()?.entries())
    }

    pub fn read_active_snapshot(&self) -> Option<ParsingManifestReadSnapshot> {
        loop {
            let idx_before = self.active_idx.active_idx.load(Ordering::Acquire);
            if idx_before > 1 {
                return None;
            }

            let bank = unsafe {
                if idx_before == 0 {
                    &*self.bank0.get()
                } else {
                    &*self.bank1.get()
                }
            };
            if !bank.is_published() {
                return None;
            }

            let publish_epoch = bank.publish_epoch;
            let last_update_unix_ns = bank.last_update_unix_ns;
            let entries = bank.entries().to_vec();

            let idx_after = self.active_idx.active_idx.load(Ordering::Acquire);
            if idx_after != idx_before {
                continue;
            }

            let stable_bank = unsafe {
                if idx_after == 0 {
                    &*self.bank0.get()
                } else {
                    &*self.bank1.get()
                }
            };
            if stable_bank.publish_epoch != publish_epoch
                || stable_bank.last_update_unix_ns != last_update_unix_ns
            {
                continue;
            }

            return Some(ParsingManifestReadSnapshot {
                publish_epoch,
                last_update_unix_ns,
                entries,
            });
        }
    }

    pub fn publish_manifest(
        &self,
        entries: &[ParsingManifestEntryV1],
        publish_epoch: u64,
        last_update_unix_ns: u64,
    ) -> Result<(), ParsingManifestWriteError> {
        if entries.len() > PARSING_MANIFEST_MAX_ENTRIES {
            return Err(ParsingManifestWriteError::TooManyEntries {
                attempted: entries.len(),
                max: PARSING_MANIFEST_MAX_ENTRIES,
            });
        }

        let old = self.active_idx.active_idx.load(Ordering::Acquire);
        let next = if old > 1 { 0 } else { 1 - old };
        let inactive = unsafe {
            if next == 0 {
                &mut *self.bank0.get()
            } else {
                &mut *self.bank1.get()
            }
        };

        inactive.reset();
        inactive.publish_epoch = publish_epoch;
        inactive.last_update_unix_ns = last_update_unix_ns;
        inactive.entry_count = entries.len() as u16;
        inactive.entries[..entries.len()].copy_from_slice(entries);

        fence(Ordering::Release);
        self.active_idx.active_idx.store(next, Ordering::Release);
        Ok(())
    }
}

impl Default for ParsingManifestSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

static_assertions::assert_eq_size!(ManifestOffsetV1, [u8; 8]);
static_assertions::assert_eq_size!(ParsingManifestEntryV1, [u8; 224]);

impl SegmentSpec for ParsingManifestSnapshot {
    const SEGMENT_ID: u64 = 26;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_parsing_manifest";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            (*ptr).active_idx.active_idx.store(0xFF, Ordering::Relaxed);
        }
    }
}

pub fn create_shadow_parsing_manifest(
) -> Result<MappedSegment<ParsingManifestSnapshot>, SegmentError> {
    MappedSegment::<ParsingManifestSnapshot>::create_rw(std::path::Path::new(
        ParsingManifestSnapshot::PATH,
    ))
}

pub fn open_shadow_parsing_manifest_ro(
) -> Result<MappedSegment<ParsingManifestSnapshot>, SegmentError> {
    MappedSegment::<ParsingManifestSnapshot>::open_ro(std::path::Path::new(
        ParsingManifestSnapshot::PATH,
    ))
}

pub fn open_shadow_parsing_manifest_rw(
) -> Result<MappedSegment<ParsingManifestSnapshot>, SegmentError> {
    MappedSegment::<ParsingManifestSnapshot>::open_rw(std::path::Path::new(
        ParsingManifestSnapshot::PATH,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{SegmentHeader64B, SEGMENT_MAGIC};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering as StdOrdering};
    use std::sync::Arc;
    use std::thread;

    fn make_entry(tag: u8, venue_id: u8, base_offset: u16) -> ParsingManifestEntryV1 {
        let mut entry = ParsingManifestEntryV1 {
            venue_id,
            parser_family: tag.wrapping_add(1),
            status: ParsingManifestStatus::Enabled.as_u8(),
            confidence_bucket: tag,
            parser_version: tag as u16,
            offset_count: 2,
            executable_hash: [tag; 32],
            min_tx_len: 32 + tag as u16,
            max_tx_len: 128 + tag as u16,
            last_validated_slot: 1_000 + tag as u64,
            golden_pass_count: 10 + tag as u16,
            live_pass_count: 20 + tag as u16,
            ..ParsingManifestEntryV1::default()
        };
        entry.offsets[0] = ManifestOffsetV1 {
            field_id: 1,
            byte_offset: base_offset,
            width: 8,
            relative_base: 0,
            endian: 0,
            flags: tag,
        };
        entry.offsets[1] = ManifestOffsetV1 {
            field_id: 2,
            byte_offset: base_offset + 4,
            width: 4,
            relative_base: 1,
            endian: 1,
            flags: tag,
        };
        entry
    }

    fn validate_entries(entries: &[ParsingManifestEntryV1]) {
        assert!(!entries.is_empty());
        let tag = entries[0].confidence_bucket;
        let expected_count = if tag == 0x11 { 1 } else { 3 };
        assert_eq!(entries.len(), expected_count);
        for (idx, entry) in entries.iter().enumerate() {
            assert_eq!(entry.status, ParsingManifestStatus::Enabled.as_u8());
            assert_eq!(entry.confidence_bucket, tag);
            assert_eq!(entry.parser_version, tag as u16);
            assert_eq!(entry.executable_hash, [tag; 32]);
            assert_eq!(entry.offset_count, 2);
            let expected_base = if tag == 0x11 { 16 } else { 64 + idx as u16 * 8 };
            assert_eq!(entry.offsets[0].byte_offset, expected_base);
            assert_eq!(entry.offsets[1].byte_offset, expected_base + 4);
        }
    }

    #[test]
    fn g11_manifest_layout_size_alignment() {
        assert_eq!(core::mem::size_of::<ManifestOffsetV1>(), 8);
        assert_eq!(core::mem::align_of::<ManifestOffsetV1>(), 2);
        assert_eq!(core::mem::size_of::<ParsingManifestEntryV1>(), 224);
        assert_eq!(core::mem::align_of::<ParsingManifestEntryV1>(), 8);
        assert_eq!(core::mem::align_of::<ParsingManifestBankV1>(), 64);
        assert_eq!(core::mem::align_of::<ParsingManifestSnapshot>(), 128);
    }

    #[test]
    fn g11_manifest_version_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parsing_manifest_wrong_version");

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();

        let header = SegmentHeader64B {
            magic: SEGMENT_MAGIC,
            build_git_sha_prefix: [0; 8],
            checksum: 0,
            segment_id: ParsingManifestSnapshot::SEGMENT_ID,
            payload_size: core::mem::size_of::<ParsingManifestSnapshot>() as u64,
            total_size: 64 + core::mem::size_of::<ParsingManifestSnapshot>() as u64,
            payload_offset: 64,
            version: 9,
            header_size: 64,
            payload_align: core::mem::align_of::<ParsingManifestSnapshot>() as u16,
            reserved_u16: 0,
            reserved_u32: 0,
        };

        unsafe {
            let header_bytes = core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<SegmentHeader64B>(),
            );
            file.write_all(header_bytes).unwrap();
        }
        file.set_len(header.total_size).unwrap();

        match MappedSegment::<ParsingManifestSnapshot>::open_ro(&path) {
            Err(SegmentError::WrongVersion { expected, found }) => {
                assert_eq!(expected, ParsingManifestSnapshot::VERSION);
                assert_eq!(found, 9);
            }
            Err(other) => panic!("expected WrongVersion, got {other}"),
            Ok(_) => panic!("expected WrongVersion, got Ok(_)"),
        }
    }

    #[test]
    fn g11_manifest_publish_consume_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parsing_manifest_roundtrip");

        let seg = MappedSegment::<ParsingManifestSnapshot>::create_rw(&path).unwrap();
        let entries = [make_entry(0x21, 7, 48), make_entry(0x21, 9, 56)];
        seg.payload().publish_manifest(&entries, 1, 10_000).unwrap();
        seg.flush().unwrap();

        let ro = MappedSegment::<ParsingManifestSnapshot>::open_ro(&path).unwrap();
        let active = ro.payload().active_manifest().unwrap();
        assert_eq!(active.publish_epoch, 1);
        assert_eq!(active.last_update_unix_ns, 10_000);
        assert_eq!(active.entries().len(), 2);
        assert_eq!(active.find_entry(7).unwrap().offsets()[0].byte_offset, 48);
        assert_eq!(active.find_entry(9).unwrap().offsets()[1].byte_offset, 60);
    }

    #[test]
    fn g11_manifest_rcu_no_torn_reads() {
        let snapshot = Arc::new(ParsingManifestSnapshot::new());
        let stop = Arc::new(AtomicBool::new(false));

        let manifest_a = [make_entry(0x11, 1, 16)];
        let manifest_b = [
            make_entry(0x22, 2, 64),
            make_entry(0x22, 3, 72),
            make_entry(0x22, 4, 80),
        ];

        snapshot.publish_manifest(&manifest_a, 1, 100).unwrap();

        let writer_snapshot = Arc::clone(&snapshot);
        let writer_stop = Arc::clone(&stop);
        let writer = thread::spawn(move || {
            for epoch in 2..2_000_u64 {
                let manifest = if epoch & 1 == 0 {
                    &manifest_b[..]
                } else {
                    &manifest_a[..]
                };
                writer_snapshot
                    .publish_manifest(manifest, epoch, 100 + epoch)
                    .unwrap();
            }
            writer_stop.store(true, StdOrdering::Release);
        });

        let reader_snapshot = Arc::clone(&snapshot);
        let reader_stop = Arc::clone(&stop);
        let reader = thread::spawn(move || {
            let mut observed = 0usize;
            while !reader_stop.load(StdOrdering::Acquire) {
                if let Some(snapshot) = reader_snapshot.read_active_snapshot() {
                    validate_entries(&snapshot.entries);
                    observed += 1;
                }
            }
            for _ in 0..1_000 {
                if let Some(snapshot) = reader_snapshot.read_active_snapshot() {
                    validate_entries(&snapshot.entries);
                    observed += 1;
                }
            }
            observed
        });

        writer.join().unwrap();
        let observed = reader.join().unwrap();
        assert!(observed > 0, "reader should observe published manifests");
    }

    #[test]
    fn g11_manifest_wrong_offsets_can_be_replaced_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parsing_manifest_replace");

        let rw = MappedSegment::<ParsingManifestSnapshot>::create_rw(&path).unwrap();
        let ro = MappedSegment::<ParsingManifestSnapshot>::open_ro(&path).unwrap();

        let wrong = [make_entry(0x31, 12, 14)];
        rw.payload().publish_manifest(&wrong, 1, 1_000).unwrap();
        rw.flush().unwrap();

        let first = ro.payload().active_manifest().unwrap();
        assert_eq!(first.find_entry(12).unwrap().offsets()[0].byte_offset, 14);

        let corrected = [make_entry(0x32, 12, 28)];
        rw.payload().publish_manifest(&corrected, 2, 2_000).unwrap();
        rw.flush().unwrap();

        let second = ro.payload().active_manifest().unwrap();
        assert_eq!(second.publish_epoch, 2);
        assert_eq!(second.find_entry(12).unwrap().offsets()[0].byte_offset, 28);
    }

    #[test]
    fn g11_manifest_hot_swap_wrong_offsets_healed_without_restart() {
        g11_manifest_wrong_offsets_can_be_replaced_without_restart();
    }
}

use core::array::from_fn;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::ingress_record::IngressRecord;
use crate::segment::{MappedSegment, SegmentError, SegmentSpec};

pub const CONTROL_FLAG_GRPC_ONLY: u64 = 1 << 0;
pub const CONTROL_FLAG_MANIFEST_STALE: u64 = 1 << 1;
pub const CONTROL_FLAG_CALIBRATOR_ACTIVE: u64 = 1 << 2;
pub const CONTROL_FLAG_AF_XDP_BLIND: u64 = 1 << 3;
pub const TRIPWIRE_BUCKET_COUNT: usize = 256;
pub const TRIPWIRE_SLOT_BUCKET_SHIFT: u8 = 2;
pub const HOT_POOL_HINT_SLOT_COUNT: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TripwireBucketKeyV1 {
    pub slot_bucket: u16,
    pub venue_id: u8,
    pub program_family: u8,
    pub fingerprint_bucket: u16,
}

static_assertions::assert_eq_size!(TripwireBucketKeyV1, [u8; 6]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotPoolHintRecord {
    pub sequence: u32,
    pub signal_source: u8,
    pub pool_pubkey: [u8; 32],
}

pub fn ingress_tripwire_key(record: &IngressRecord) -> TripwireBucketKeyV1 {
    TripwireBucketKeyV1 {
        slot_bucket: (record.slot >> TRIPWIRE_SLOT_BUCKET_SHIFT) as u16,
        venue_id: record.venue_id,
        // The ingress contract currently exports pattern_id rather than a
        // dedicated program-family discriminator, so we use the routed pattern
        // as the fixed-width family surrogate for G11 bucketization.
        program_family: record.pattern_id,
        fingerprint_bucket: signature_fingerprint_bucket(&record.signature),
    }
}

pub fn tripwire_bucket_index(key: &TripwireBucketKeyV1) -> u8 {
    let mut hash = key.slot_bucket as u32;
    hash ^= ((key.venue_id as u32) << 16) | ((key.program_family as u32) << 8);
    hash = hash.rotate_left(7) ^ key.fingerprint_bucket as u32;
    hash = hash.wrapping_mul(0x45d9f3b);
    ((hash ^ (hash >> 16)) & 0xff) as u8
}

fn signature_fingerprint_bucket(signature: &[u8; 64]) -> u16 {
    let mut acc = 0u16;
    for chunk in signature.chunks_exact(2).take(8) {
        acc ^= u16::from_le_bytes([chunk[0], chunk[1]]);
        acc = acc.rotate_left(3);
    }
    acc
}

/// Fixed-width G11 control-plane surface read cheaply by Tier-0.
///
/// The hot-path reader only needs the top-level atomics (`flags`,
/// `disabled_venue_mask`, `grpc_only_until_slot`, `epoch`, `last_update_unix_ns`).
/// The tripwire bucket arrays are for off-path control-plane diagnosis.
#[repr(C, align(64))]
pub struct ShadowControlFlagsV1 {
    pub flags: AtomicU64,
    pub epoch: AtomicU64,
    pub last_update_unix_ns: AtomicU64,

    pub disabled_venue_mask: AtomicU64,
    pub grpc_only_until_slot: AtomicU64,

    pub tripwire_slot_bucket: [AtomicU16; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_venue_id: [AtomicU8; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_program_family: [AtomicU8; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_fingerprint_bucket: [AtomicU16; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_af_xdp_hits: [AtomicU32; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_grpc_hits: [AtomicU32; TRIPWIRE_BUCKET_COUNT],
    pub tripwire_miss_count: [AtomicU32; TRIPWIRE_BUCKET_COUNT],
    pub hot_pool_hint_seq: [AtomicU32; HOT_POOL_HINT_SLOT_COUNT],
    pub hot_pool_hint_source: [AtomicU8; HOT_POOL_HINT_SLOT_COUNT],
    pub hot_pool_hint_word0: [AtomicU64; HOT_POOL_HINT_SLOT_COUNT],
    pub hot_pool_hint_word1: [AtomicU64; HOT_POOL_HINT_SLOT_COUNT],
    pub hot_pool_hint_word2: [AtomicU64; HOT_POOL_HINT_SLOT_COUNT],
    pub hot_pool_hint_word3: [AtomicU64; HOT_POOL_HINT_SLOT_COUNT],
}

unsafe impl Sync for ShadowControlFlagsV1 {}

impl ShadowControlFlagsV1 {
    pub fn new() -> Self {
        Self {
            flags: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            last_update_unix_ns: AtomicU64::new(0),
            disabled_venue_mask: AtomicU64::new(0),
            grpc_only_until_slot: AtomicU64::new(0),
            tripwire_slot_bucket: from_fn(|_| AtomicU16::new(0)),
            tripwire_venue_id: from_fn(|_| AtomicU8::new(0)),
            tripwire_program_family: from_fn(|_| AtomicU8::new(0)),
            tripwire_fingerprint_bucket: from_fn(|_| AtomicU16::new(0)),
            tripwire_af_xdp_hits: from_fn(|_| AtomicU32::new(0)),
            tripwire_grpc_hits: from_fn(|_| AtomicU32::new(0)),
            tripwire_miss_count: from_fn(|_| AtomicU32::new(0)),
            hot_pool_hint_seq: from_fn(|_| AtomicU32::new(0)),
            hot_pool_hint_source: from_fn(|_| AtomicU8::new(0)),
            hot_pool_hint_word0: from_fn(|_| AtomicU64::new(0)),
            hot_pool_hint_word1: from_fn(|_| AtomicU64::new(0)),
            hot_pool_hint_word2: from_fn(|_| AtomicU64::new(0)),
            hot_pool_hint_word3: from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub fn has_flag(&self, flag: u64) -> bool {
        (self.flags.load(Ordering::Acquire) & flag) != 0
    }

    pub fn set_flag(&self, flag: u64, enabled: bool) {
        let mut current = self.flags.load(Ordering::Acquire);
        loop {
            let updated = if enabled {
                current | flag
            } else {
                current & !flag
            };
            match self
                .flags
                .compare_exchange(current, updated, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn set_epoch(&self, epoch: u64) {
        self.epoch.store(epoch, Ordering::Release);
    }

    pub fn last_update_unix_ns(&self) -> u64 {
        self.last_update_unix_ns.load(Ordering::Acquire)
    }

    pub fn set_last_update_unix_ns(&self, now_unix_ns: u64) {
        self.last_update_unix_ns
            .store(now_unix_ns, Ordering::Release);
    }

    pub fn disabled_venue_mask(&self) -> u64 {
        self.disabled_venue_mask.load(Ordering::Acquire)
    }

    pub fn set_disabled_venue_mask(&self, mask: u64) {
        self.disabled_venue_mask.store(mask, Ordering::Release);
    }

    pub fn venue_disabled(&self, venue_id: u8) -> bool {
        if venue_id >= 64 {
            return false;
        }
        (self.disabled_venue_mask() & (1u64 << venue_id)) != 0
    }

    pub fn set_venue_disabled(&self, venue_id: u8, disabled: bool) -> bool {
        if venue_id >= 64 {
            return false;
        }

        let bit = 1u64 << venue_id;
        let mut current = self.disabled_venue_mask.load(Ordering::Acquire);
        loop {
            let updated = if disabled {
                current | bit
            } else {
                current & !bit
            };
            match self.disabled_venue_mask.compare_exchange(
                current,
                updated,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn grpc_only_until_slot(&self) -> u64 {
        self.grpc_only_until_slot.load(Ordering::Acquire)
    }

    pub fn set_grpc_only_until_slot(&self, slot: u64) {
        self.grpc_only_until_slot.store(slot, Ordering::Release);
    }

    pub fn clear_grpc_only(&self) {
        self.grpc_only_until_slot.store(0, Ordering::Release);
        self.set_flag(CONTROL_FLAG_GRPC_ONLY, false);
    }

    pub fn bucket_key(&self, bucket: u8) -> TripwireBucketKeyV1 {
        let idx = bucket as usize;
        TripwireBucketKeyV1 {
            slot_bucket: self.tripwire_slot_bucket[idx].load(Ordering::Acquire),
            venue_id: self.tripwire_venue_id[idx].load(Ordering::Acquire),
            program_family: self.tripwire_program_family[idx].load(Ordering::Acquire),
            fingerprint_bucket: self.tripwire_fingerprint_bucket[idx].load(Ordering::Acquire),
        }
    }

    fn store_bucket_key(&self, bucket: u8, key: &TripwireBucketKeyV1) {
        let idx = bucket as usize;
        self.tripwire_slot_bucket[idx].store(key.slot_bucket, Ordering::Release);
        self.tripwire_venue_id[idx].store(key.venue_id, Ordering::Release);
        self.tripwire_program_family[idx].store(key.program_family, Ordering::Release);
        self.tripwire_fingerprint_bucket[idx].store(key.fingerprint_bucket, Ordering::Release);
    }

    pub fn af_xdp_hits(&self, bucket: u8) -> u32 {
        self.tripwire_af_xdp_hits[bucket as usize].load(Ordering::Acquire)
    }

    pub fn grpc_hits(&self, bucket: u8) -> u32 {
        self.tripwire_grpc_hits[bucket as usize].load(Ordering::Acquire)
    }

    pub fn miss_count(&self, bucket: u8) -> u32 {
        self.tripwire_miss_count[bucket as usize].load(Ordering::Acquire)
    }

    pub fn increment_af_xdp_hit(&self, bucket: u8) -> u32 {
        self.tripwire_af_xdp_hits[bucket as usize].fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn increment_grpc_hit(&self, bucket: u8) -> u32 {
        self.tripwire_grpc_hits[bucket as usize].fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn increment_miss_count(&self, bucket: u8) -> u32 {
        self.tripwire_miss_count[bucket as usize].fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn set_miss_count(&self, bucket: u8, count: u32) {
        self.tripwire_miss_count[bucket as usize].store(count, Ordering::Release);
    }

    pub fn record_af_xdp_hit(&self, key: TripwireBucketKeyV1) -> u32 {
        let bucket = tripwire_bucket_index(&key);
        self.store_bucket_key(bucket, &key);
        self.increment_af_xdp_hit(bucket)
    }

    pub fn record_grpc_hit(&self, key: TripwireBucketKeyV1) -> u32 {
        let bucket = tripwire_bucket_index(&key);
        self.store_bucket_key(bucket, &key);
        self.increment_grpc_hit(bucket)
    }

    pub fn record_af_xdp_hit_from_ingress(&self, record: &IngressRecord) -> u32 {
        self.record_af_xdp_hit(ingress_tripwire_key(record))
    }

    pub fn record_grpc_hit_from_ingress(&self, record: &IngressRecord) -> u32 {
        self.record_grpc_hit(ingress_tripwire_key(record))
    }

    pub fn snapshot_bucket_keys(&self) -> [TripwireBucketKeyV1; TRIPWIRE_BUCKET_COUNT] {
        from_fn(|idx| self.bucket_key(idx as u8))
    }

    pub fn snapshot_af_xdp_hits(&self) -> [u32; TRIPWIRE_BUCKET_COUNT] {
        from_fn(|idx| self.tripwire_af_xdp_hits[idx].load(Ordering::Acquire))
    }

    pub fn snapshot_grpc_hits(&self) -> [u32; TRIPWIRE_BUCKET_COUNT] {
        from_fn(|idx| self.tripwire_grpc_hits[idx].load(Ordering::Acquire))
    }

    pub fn snapshot_miss_counts(&self) -> [u32; TRIPWIRE_BUCKET_COUNT] {
        from_fn(|idx| self.tripwire_miss_count[idx].load(Ordering::Acquire))
    }

    pub fn record_hot_pool_hint(
        &self,
        worker_slot: u8,
        pool_pubkey: [u8; 32],
        signal_source: u8,
    ) -> bool {
        let idx = worker_slot as usize % HOT_POOL_HINT_SLOT_COUNT;
        let deduped = self
            .snapshot_hot_pool_hint_slot(idx)
            .map(|hint| hint.pool_pubkey == pool_pubkey && hint.signal_source == signal_source)
            .unwrap_or(false);
        let seq = self.hot_pool_hint_seq[idx].load(Ordering::Acquire);
        let odd_seq = seq.wrapping_add(1) | 1;
        self.hot_pool_hint_seq[idx].store(odd_seq, Ordering::Release);
        let [w0, w1, w2, w3] = hot_pool_hint_pubkey_words(pool_pubkey);
        self.hot_pool_hint_source[idx].store(signal_source, Ordering::Relaxed);
        self.hot_pool_hint_word0[idx].store(w0, Ordering::Relaxed);
        self.hot_pool_hint_word1[idx].store(w1, Ordering::Relaxed);
        self.hot_pool_hint_word2[idx].store(w2, Ordering::Relaxed);
        self.hot_pool_hint_word3[idx].store(w3, Ordering::Relaxed);
        self.hot_pool_hint_seq[idx].store(odd_seq.wrapping_add(1) & !1, Ordering::Release);
        !deduped
    }

    pub fn snapshot_hot_pool_hint_slot(&self, idx: usize) -> Option<HotPoolHintRecord> {
        if idx >= HOT_POOL_HINT_SLOT_COUNT {
            return None;
        }
        let seq_before = self.hot_pool_hint_seq[idx].load(Ordering::Acquire);
        if seq_before == 0 || (seq_before & 1) != 0 {
            return None;
        }
        let signal_source = self.hot_pool_hint_source[idx].load(Ordering::Relaxed);
        let w0 = self.hot_pool_hint_word0[idx].load(Ordering::Relaxed);
        let w1 = self.hot_pool_hint_word1[idx].load(Ordering::Relaxed);
        let w2 = self.hot_pool_hint_word2[idx].load(Ordering::Relaxed);
        let w3 = self.hot_pool_hint_word3[idx].load(Ordering::Relaxed);
        let seq_after = self.hot_pool_hint_seq[idx].load(Ordering::Acquire);
        if seq_before != seq_after || (seq_after & 1) != 0 {
            return None;
        }
        Some(HotPoolHintRecord {
            sequence: seq_after,
            signal_source,
            pool_pubkey: hot_pool_hint_pubkey_from_words([w0, w1, w2, w3]),
        })
    }

    pub fn snapshot_hot_pool_hints(&self) -> [Option<HotPoolHintRecord>; HOT_POOL_HINT_SLOT_COUNT] {
        from_fn(|idx| self.snapshot_hot_pool_hint_slot(idx))
    }
}

fn hot_pool_hint_pubkey_words(pool_pubkey: [u8; 32]) -> [u64; 4] {
    let mut words = [0u64; 4];
    for (idx, chunk) in pool_pubkey.chunks_exact(8).enumerate() {
        words[idx] = u64::from_le_bytes(chunk.try_into().expect("pubkey chunk length"));
    }
    words
}

fn hot_pool_hint_pubkey_from_words(words: [u64; 4]) -> [u8; 32] {
    let mut pool_pubkey = [0u8; 32];
    for (idx, word) in words.into_iter().enumerate() {
        pool_pubkey[idx * 8..(idx + 1) * 8].copy_from_slice(&word.to_le_bytes());
    }
    pool_pubkey
}

impl Default for ShadowControlFlagsV1 {
    fn default() -> Self {
        Self::new()
    }
}

static_assertions::const_assert!(core::mem::size_of::<ShadowControlFlagsV1>() <= 8192);

impl SegmentSpec for ShadowControlFlagsV1 {
    const SEGMENT_ID: u64 = 27;
    const VERSION: u16 = 1;
    const PATH: &'static str = "/dev/shm/shadow_control_flags";

    fn init_payload(ptr: *mut Self) {
        unsafe {
            core::ptr::write(ptr, Self::new());
        }
    }
}

pub fn create_shadow_control_flags() -> Result<MappedSegment<ShadowControlFlagsV1>, SegmentError> {
    MappedSegment::<ShadowControlFlagsV1>::create_rw(std::path::Path::new(
        ShadowControlFlagsV1::PATH,
    ))
}

pub fn open_shadow_control_flags_ro() -> Result<MappedSegment<ShadowControlFlagsV1>, SegmentError> {
    MappedSegment::<ShadowControlFlagsV1>::open_ro(std::path::Path::new(ShadowControlFlagsV1::PATH))
}

pub fn open_shadow_control_flags_rw() -> Result<MappedSegment<ShadowControlFlagsV1>, SegmentError> {
    MappedSegment::<ShadowControlFlagsV1>::open_rw(std::path::Path::new(ShadowControlFlagsV1::PATH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{SegmentHeader64B, SEGMENT_MAGIC};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn g11_control_flags_grpc_only_set_clear() {
        let flags = ShadowControlFlagsV1::new();

        assert!(!flags.has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(flags.grpc_only_until_slot(), 0);

        flags.set_flag(CONTROL_FLAG_GRPC_ONLY, true);
        flags.set_grpc_only_until_slot(1_234);

        assert!(flags.has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(flags.grpc_only_until_slot(), 1_234);

        flags.clear_grpc_only();
        assert!(!flags.has_flag(CONTROL_FLAG_GRPC_ONLY));
        assert_eq!(flags.grpc_only_until_slot(), 0);
    }

    #[test]
    fn g11_control_flags_venue_disable_mask() {
        let flags = ShadowControlFlagsV1::new();

        assert_eq!(flags.disabled_venue_mask(), 0);
        assert!(flags.set_venue_disabled(3, true));
        assert!(flags.set_venue_disabled(9, true));
        assert!(flags.venue_disabled(3));
        assert!(flags.venue_disabled(9));
        assert_eq!(flags.disabled_venue_mask(), (1u64 << 3) | (1u64 << 9));

        assert!(flags.set_venue_disabled(3, false));
        assert!(!flags.venue_disabled(3));
        assert!(flags.venue_disabled(9));
        assert!(!flags.set_venue_disabled(64, true));
    }

    #[test]
    fn g11_control_flags_version_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control_flags_wrong_version");

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
            segment_id: ShadowControlFlagsV1::SEGMENT_ID,
            payload_size: core::mem::size_of::<ShadowControlFlagsV1>() as u64,
            total_size: 64 + core::mem::size_of::<ShadowControlFlagsV1>() as u64,
            payload_offset: 64,
            version: 7,
            header_size: 64,
            payload_align: 64,
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

        match MappedSegment::<ShadowControlFlagsV1>::open_ro(&path) {
            Err(SegmentError::WrongVersion { expected, found }) => {
                assert_eq!(expected, ShadowControlFlagsV1::VERSION);
                assert_eq!(found, 7);
            }
            Err(other) => panic!("expected WrongVersion, got {other}"),
            Ok(_) => panic!("expected WrongVersion, got Ok(_)"),
        }
    }

    #[test]
    fn g11_control_flags_atomic_hit_counters() {
        let flags = Arc::new(ShadowControlFlagsV1::new());
        let mut threads = Vec::new();

        for _ in 0..4 {
            let flags = Arc::clone(&flags);
            threads.push(thread::spawn(move || {
                for _ in 0..1000 {
                    flags.increment_af_xdp_hit(7);
                    flags.increment_grpc_hit(7);
                    flags.increment_miss_count(7);
                }
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(flags.af_xdp_hits(7), 4_000);
        assert_eq!(flags.grpc_hits(7), 4_000);
        assert_eq!(flags.miss_count(7), 4_000);
        assert_eq!(flags.af_xdp_hits(8), 0);
    }

    #[test]
    fn g11_control_flags_bucket_key_roundtrip() {
        let flags = ShadowControlFlagsV1::new();
        let key = TripwireBucketKeyV1 {
            slot_bucket: 111,
            venue_id: 9,
            program_family: 3,
            fingerprint_bucket: 0x55aa,
        };

        let bucket = tripwire_bucket_index(&key);
        flags.record_grpc_hit(key);

        assert_eq!(flags.grpc_hits(bucket), 1);
        assert_eq!(flags.bucket_key(bucket), key);
    }

    #[test]
    fn g11_control_flags_hot_pool_hint_roundtrip_and_dedup() {
        let flags = ShadowControlFlagsV1::new();
        let pool_pubkey = [7u8; 32];

        assert!(flags.record_hot_pool_hint(3, pool_pubkey, 11));
        let hint = flags
            .snapshot_hot_pool_hint_slot(3)
            .expect("hint must be visible after record");
        assert_eq!(hint.signal_source, 11);
        assert_eq!(hint.pool_pubkey, pool_pubkey);
        assert_eq!(hint.sequence, 2);

        assert!(!flags.record_hot_pool_hint(3, pool_pubkey, 11));
        let updated = flags
            .snapshot_hot_pool_hint_slot(3)
            .expect("deduped hint must still be visible");
        assert_eq!(updated.pool_pubkey, pool_pubkey);
        assert!(updated.sequence > hint.sequence);
    }
}

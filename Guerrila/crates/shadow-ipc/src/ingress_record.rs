//! `IngressRecord` contract and feed-ring helpers.
//!
//! The ingress proxy (AF_XDP shred capture or gRPC fallback) emits one
//! `IngressRecord` per decoded swap intent into a per-worker SPSC feed ring.
//! The record is **fixed-size, fixed-layout, no heap, no pointers**.
//!
//! Feed rings live at `/dev/shm/shadow_feed_ring_w{worker}_p{producer}`.

use crate::ring::{MappedSpscRing, RingError};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Signal source constants (canonical enum for IngressRecord.signal_source)
// ---------------------------------------------------------------------------

/// gRPC: public Geyser subscription.
pub const SIGNAL_SOURCE_PUBLIC_GEYSER: u8 = 0;
/// gRPC: Jito Searcher API (block-engine bundles).
pub const SIGNAL_SOURCE_JITO_SEARCHER_API: u8 = 1;
/// gRPC: private mempool feed.
pub const SIGNAL_SOURCE_PRIVATE_MEMPOOL: u8 = 2;
/// gRPC: harmonic Searcher API lane.
pub const SIGNAL_SOURCE_HARMONIC_SEARCHER_API: u8 = 3;
/// AF_XDP: shred-capture primary ingress path.
pub const SIGNAL_SOURCE_AFXDP: u8 = 4;
/// gRPC: external Yellowstone/Triton-style Geyser.
pub const SIGNAL_SOURCE_EXTERNAL_GEYSER: u8 = 5;
/// gRPC: local validator Yellowstone/Geyser.
pub const SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER: u8 = 6;
/// DPDK: private packet-ingress producer path.
pub const SIGNAL_SOURCE_DPDK: u8 = 7;
/// gRPC: Helius LaserStream continuity/enrichment feed.
pub const SIGNAL_SOURCE_LASERSTREAM: u8 = 8;

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------

/// Maximum number of flattened account pubkeys in an `IngressRecord`.
/// If a decoded transaction exceeds this, the ingress proxy MUST drop it.
pub const FLAT_ACCOUNTS_CAP: usize = 64;

/// Maximum valid `obscurity_score` value. Valid range is `0..=OBSCURITY_SCORE_MAX`.
pub const OBSCURITY_SCORE_MAX: u8 = 3;

/// Sentinel for unused `pattern_acct_indices` entries.
pub const ACCT_INDEX_ABSENT: u8 = 0xFF;
/// Maximum number of matched instruction account indices preserved on the record.
pub const IX_ACCOUNT_INDEX_CAP: usize = 32;

/// Sentinel for "no pool index hint available".
pub const POOL_INDEX_HINT_NONE: u32 = u32::MAX;
/// Sentinel marker for SH5 source-proof unsupported-program records.
pub const SOURCE_PROBE_UNSUPPORTED_PROGRAM_ENTITY_ID: u32 = u32::MAX;
/// Sentinel venue marker for SH5 source-proof unsupported-program records.
pub const SOURCE_PROBE_UNSUPPORTED_PROGRAM_VENUE_ID: u8 = u8::MAX;
/// Sentinel pattern marker for SH5 source-proof unsupported-program records.
pub const SOURCE_PROBE_UNSUPPORTED_PROGRAM_PATTERN_ID: u8 = u8::MAX;
/// Sentinel marker for SH8 AF_XDP parsed-shred source-proof records.
pub const SOURCE_PROBE_AFXDP_PARSED_SHRED_ENTITY_ID: u32 = u32::MAX - 1;
/// Sentinel venue marker for SH8 AF_XDP parsed-shred source-proof records.
pub const SOURCE_PROBE_AFXDP_PARSED_SHRED_VENUE_ID: u8 = u8::MAX;
/// Sentinel pattern marker for SH8 AF_XDP parsed-shred source-proof records.
pub const SOURCE_PROBE_AFXDP_PARSED_SHRED_PATTERN_ID: u8 = u8::MAX - 1;

// ---------------------------------------------------------------------------
// Segment identity
// ---------------------------------------------------------------------------

pub const FEED_RING_SEGMENT_ID: u64 = 10;
pub const FEED_RING_VERSION: u16 = 1;

/// Default feed-ring capacity (power-of-two).
pub const FEED_RING_DEFAULT_CAPACITY: u32 = 256;

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Canonical `/dev/shm` path for a feed ring.
pub fn feed_ring_path(worker_id: usize, producer_id: usize) -> PathBuf {
    PathBuf::from(format!(
        "/dev/shm/shadow_feed_ring_w{worker_id}_p{producer_id}"
    ))
}

// ---------------------------------------------------------------------------
// IngressRecord — the frozen contract
// ---------------------------------------------------------------------------

/// A single decoded swap intent emitted by an ingress proxy.
///
/// **Layout contract (frozen):**
/// - 2304 bytes, 64-byte aligned, `#[repr(C)]`.
/// - No `Vec`, no pointers, no heap ownership.
/// - `flat_accounts` slots beyond `flat_accounts_len` MUST be zeroed.
/// - If flattened accounts exceed [`FLAT_ACCOUNTS_CAP`], the ingress proxy
///   MUST drop the record (never truncate).
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct IngressRecord {
    // ---- byte 0..8: flags / enums / counts ----
    /// Signal source. See `SIGNAL_SOURCE_*` constants for canonical values.
    pub signal_source: u8,
    /// Venue identifier (0 = RaydiumV4, etc.).
    pub venue_id: u8,
    /// Swap pattern identifier (0 = SwapBaseIn, etc.).
    pub pattern_id: u8,
    /// Obscurity score: 0..=3. Derived from O(1) header-adjacent features only.
    pub obscurity_score: u8,
    /// Number of valid entries in `flat_accounts` (0..=64).
    pub flat_accounts_len: u8,
    /// Number of static message keys before ALT-expanded addresses.
    pub static_accounts_len: u8,
    /// Solana message header field.
    pub num_required_signatures: u8,
    /// Solana message header field.
    pub num_readonly_signed: u8,

    // ---- byte 8..16: entity + pool hint ----
    /// Target entity index from the allowlist.
    pub entity_id: u32,
    /// Pool index hint from ingress (worker re-verifies via tag map).
    /// [`POOL_INDEX_HINT_NONE`] if unavailable.
    pub pool_index_hint: u32,

    // ---- byte 16..40: scalars ----
    /// Slot from Turbine header or gRPC metadata.
    pub slot: u64,
    /// Ingress timestamp in nanoseconds (from RX metadata, not syscall).
    pub unix_ns: u64,
    /// Decoded swap `amount_in` in lamports.
    pub amount_in: u64,

    // ---- byte 40..48: pattern account indices ----
    /// Fixed indices into `flat_accounts` for the key accounts required by
    /// this pattern (so the worker never scans). 0xFF = absent.
    pub pattern_acct_indices: [u8; 8],

    // ---- byte 48..112: signature ----
    /// Target transaction Ed25519 signature (64 bytes).
    pub signature: [u8; 64],

    // ---- byte 112..208: pubkeys ----
    /// Pool pubkey (used for routing, state lookup, and ShadowGuard).
    pub pool_pubkey: [u8; 32],
    /// Input mint pubkey.
    pub mint_in: [u8; 32],
    /// Output mint pubkey.
    pub mint_out: [u8; 32],

    // ---- byte 208..256: builder metadata + reserved padding ----
    /// Solana message header field.
    pub num_readonly_unsigned: u8,
    /// Count of ALT-expanded writable addresses; the remainder are readonly.
    pub alt_writable_count: u8,
    /// Venue-specific instruction template variant. `0` = default / unspecified.
    pub template_variant: u8,
    /// Number of valid entries in `ix_account_indices` (0..=32).
    pub ix_account_count: u8,
    /// Matched compiled-instruction account indices into `flat_accounts`, in
    /// original instruction order. Unused slots are `ACCT_INDEX_ABSENT`.
    pub ix_account_indices: [u8; IX_ACCOUNT_INDEX_CAP],
    /// Reserved metadata:
    /// - bytes `0..8`: decoded victim `minimum_amount_out` in lamports
    /// - bytes `8..12`: scorecard replay dev-proof marker (zero when unset)
    ///
    /// Still aligns `flat_accounts` to offset 256 without changing the frozen
    /// record size.
    pub _pad_1: [u8; 12],

    // ---- byte 256..2304: flattened accounts ----
    /// Flattened, ALT-expanded account pubkeys. Unused slots MUST be zeroed.
    pub flat_accounts: [[u8; 32]; FLAT_ACCOUNTS_CAP],
}

// Compile-time layout assertions.
static_assertions::assert_eq_size!(IngressRecord, [u8; 2304]);
static_assertions::const_assert_eq!(core::mem::align_of::<IngressRecord>(), 64);

impl IngressRecord {
    const SCORECARD_REPLAY_DEV_PROOF_MARKER: u32 = u32::from_le_bytes(*b"SR40");

    /// Create a zeroed `IngressRecord` with sentinel defaults.
    pub fn zeroed() -> Self {
        let mut rec = unsafe { core::mem::zeroed::<Self>() };
        rec.pool_index_hint = POOL_INDEX_HINT_NONE;
        rec.pattern_acct_indices = [ACCT_INDEX_ABSENT; 8];
        rec.ix_account_indices = [ACCT_INDEX_ABSENT; IX_ACCOUNT_INDEX_CAP];
        rec
    }

    /// Build the SH5 observe-only source-proof record emitted when local
    /// Yellowstone traffic is unsupported by the current strategy universe.
    pub fn source_probe_unsupported_program(
        signal_source: u8,
        slot: u64,
        unix_ns: u64,
        signature: [u8; 64],
    ) -> Self {
        let mut rec = Self::zeroed();
        rec.signal_source = signal_source;
        rec.slot = slot;
        rec.unix_ns = unix_ns;
        rec.signature = signature;
        rec.entity_id = SOURCE_PROBE_UNSUPPORTED_PROGRAM_ENTITY_ID;
        rec.venue_id = SOURCE_PROBE_UNSUPPORTED_PROGRAM_VENUE_ID;
        rec.pattern_id = SOURCE_PROBE_UNSUPPORTED_PROGRAM_PATTERN_ID;
        rec.amount_in = 0;
        rec
    }

    /// Build the SH8 observe-only AF_XDP source-proof record emitted after a
    /// successful shred-header parse but before transaction reconstruction.
    pub fn source_probe_afxdp_parsed_shred(slot: u64, unix_ns: u64, signature: [u8; 64]) -> Self {
        let mut rec = Self::zeroed();
        rec.signal_source = SIGNAL_SOURCE_AFXDP;
        rec.slot = slot;
        rec.unix_ns = unix_ns;
        rec.signature = signature;
        rec.entity_id = SOURCE_PROBE_AFXDP_PARSED_SHRED_ENTITY_ID;
        rec.venue_id = SOURCE_PROBE_AFXDP_PARSED_SHRED_VENUE_ID;
        rec.pattern_id = SOURCE_PROBE_AFXDP_PARSED_SHRED_PATTERN_ID;
        rec.amount_in = 0;
        rec
    }

    /// Returns true if this record is the explicit SH5 source-proof marker.
    pub fn is_source_probe_unsupported_program(&self) -> bool {
        self.entity_id == SOURCE_PROBE_UNSUPPORTED_PROGRAM_ENTITY_ID
            && self.venue_id == SOURCE_PROBE_UNSUPPORTED_PROGRAM_VENUE_ID
            && self.pattern_id == SOURCE_PROBE_UNSUPPORTED_PROGRAM_PATTERN_ID
            && self.pool_index_hint == POOL_INDEX_HINT_NONE
            && self.flat_accounts_len == 0
            && self.pool_pubkey == [0u8; 32]
            && self.mint_in == [0u8; 32]
            && self.mint_out == [0u8; 32]
    }

    /// Returns true if this record is the explicit SH8 AF_XDP parsed-shred
    /// source-proof marker.
    pub fn is_source_probe_afxdp_parsed_shred(&self) -> bool {
        self.signal_source == SIGNAL_SOURCE_AFXDP
            && self.entity_id == SOURCE_PROBE_AFXDP_PARSED_SHRED_ENTITY_ID
            && self.venue_id == SOURCE_PROBE_AFXDP_PARSED_SHRED_VENUE_ID
            && self.pattern_id == SOURCE_PROBE_AFXDP_PARSED_SHRED_PATTERN_ID
            && self.pool_index_hint == POOL_INDEX_HINT_NONE
            && self.flat_accounts_len == 0
            && self.pool_pubkey == [0u8; 32]
            && self.mint_in == [0u8; 32]
            && self.mint_out == [0u8; 32]
    }

    /// Set the flattened, ALT-expanded account list.
    ///
    /// **Behavioral contract (frozen):**
    /// - If `accounts.len() > FLAT_ACCOUNTS_CAP` → returns `false` (hard drop,
    ///   never truncate).
    /// - Accounts are copied in caller-supplied order (deterministic).
    /// - Slots beyond `accounts.len()` are zeroed.
    pub fn set_flat_accounts(&mut self, accounts: &[[u8; 32]]) -> bool {
        if accounts.len() > FLAT_ACCOUNTS_CAP {
            return false;
        }
        self.flat_accounts_len = accounts.len() as u8;
        for (dst, src) in self.flat_accounts[..accounts.len()]
            .iter_mut()
            .zip(accounts.iter())
        {
            *dst = *src;
        }
        for slot in &mut self.flat_accounts[accounts.len()..] {
            *slot = [0u8; 32];
        }
        true
    }

    /// Set the matched instruction account indices preserved for template builders.
    pub fn set_ix_account_indices(&mut self, indices: &[u8]) -> bool {
        if indices.len() > IX_ACCOUNT_INDEX_CAP {
            return false;
        }
        self.ix_account_count = indices.len() as u8;
        self.ix_account_indices[..indices.len()].copy_from_slice(indices);
        for slot in &mut self.ix_account_indices[indices.len()..] {
            *slot = ACCT_INDEX_ABSENT;
        }
        true
    }

    /// Store the decoded victim `minimum_amount_out` without changing the fixed
    /// record layout.
    pub fn set_decoded_minimum_amount_out(&mut self, value: u64) {
        self._pad_1[..8].copy_from_slice(&value.to_le_bytes());
        self._pad_1[8..].fill(0);
    }

    /// Read the decoded victim `minimum_amount_out` carried in the reserved
    /// metadata bytes.
    pub fn decoded_minimum_amount_out(&self) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self._pad_1[..8]);
        u64::from_le_bytes(bytes)
    }

    /// Mark this record as the single injected scorecard-replay dev-proof row.
    pub fn mark_scorecard_replay_dev_proof(&mut self) {
        self._pad_1[8..12].copy_from_slice(&Self::SCORECARD_REPLAY_DEV_PROOF_MARKER.to_le_bytes());
    }

    /// Returns true when this record carries the injected scorecard-replay marker.
    pub fn is_scorecard_replay_dev_proof(&self) -> bool {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self._pad_1[8..12]);
        u32::from_le_bytes(bytes) == Self::SCORECARD_REPLAY_DEV_PROOF_MARKER
    }

    /// Set the obscurity score.
    ///
    /// **Behavioral contract (frozen):**
    /// - Valid range: `0..=OBSCURITY_SCORE_MAX` (0..=3).
    /// - Values outside the valid range are rejected (returns `false`);
    ///   the field is left unchanged.
    pub fn set_obscurity_score(&mut self, score: u8) -> bool {
        if score > OBSCURITY_SCORE_MAX {
            return false;
        }
        self.obscurity_score = score;
        true
    }
}

// ---------------------------------------------------------------------------
// Concrete feed-ring helpers
// ---------------------------------------------------------------------------

/// Create a new feed ring at the given path.
pub fn create_feed_ring(
    path: &Path,
    capacity: u32,
) -> Result<MappedSpscRing<IngressRecord>, RingError> {
    MappedSpscRing::create_rw(path, FEED_RING_SEGMENT_ID, FEED_RING_VERSION, capacity)
}

/// Open an existing feed ring for reading and writing.
pub fn open_feed_ring_rw(path: &Path) -> Result<MappedSpscRing<IngressRecord>, RingError> {
    MappedSpscRing::open_rw(path, FEED_RING_SEGMENT_ID, FEED_RING_VERSION)
}

/// Open an existing feed ring read-only.
pub fn open_feed_ring_ro(path: &Path) -> Result<MappedSpscRing<IngressRecord>, RingError> {
    MappedSpscRing::open_ro(path, FEED_RING_SEGMENT_ID, FEED_RING_VERSION)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    // -- G4-A1-T1: exact size / alignment / offset freeze --------------------

    #[test]
    fn g4_a1_ingress_record_size() {
        assert_eq!(core::mem::size_of::<IngressRecord>(), 2304);
    }

    #[test]
    fn g4_a1_ingress_record_align() {
        assert_eq!(core::mem::align_of::<IngressRecord>(), 64);
    }

    #[test]
    fn g4_a1_source_probe_unsupported_program_marker_roundtrips() {
        let rec = IngressRecord::source_probe_unsupported_program(6, 42, 99, [7u8; 64]);
        assert!(rec.is_source_probe_unsupported_program());
        assert_eq!(rec.signal_source, 6);
        assert_eq!(rec.slot, 42);
        assert_eq!(rec.unix_ns, 99);
        assert_eq!(rec.signature, [7u8; 64]);
        assert_eq!(rec.flat_accounts_len, 0);
        assert_eq!(rec.pool_pubkey, [0u8; 32]);
        assert_eq!(rec.mint_in, [0u8; 32]);
        assert_eq!(rec.mint_out, [0u8; 32]);
    }

    #[test]
    fn sh8_source_probe_afxdp_parsed_shred_marker_roundtrips() {
        let rec = IngressRecord::source_probe_afxdp_parsed_shred(77, 1234, [9u8; 64]);
        assert!(rec.is_source_probe_afxdp_parsed_shred());
        assert_eq!(rec.signal_source, SIGNAL_SOURCE_AFXDP);
        assert_eq!(rec.slot, 77);
        assert_eq!(rec.unix_ns, 1234);
        assert_eq!(rec.signature, [9u8; 64]);
        assert_eq!(rec.flat_accounts_len, 0);
        assert_eq!(rec.pool_pubkey, [0u8; 32]);
        assert_eq!(rec.mint_in, [0u8; 32]);
        assert_eq!(rec.mint_out, [0u8; 32]);
    }

    #[test]
    fn g4_a1_ingress_record_field_offsets() {
        use core::mem::offset_of;

        assert_eq!(offset_of!(IngressRecord, signal_source), 0);
        assert_eq!(offset_of!(IngressRecord, venue_id), 1);
        assert_eq!(offset_of!(IngressRecord, pattern_id), 2);
        assert_eq!(offset_of!(IngressRecord, obscurity_score), 3);
        assert_eq!(offset_of!(IngressRecord, flat_accounts_len), 4);
        assert_eq!(offset_of!(IngressRecord, static_accounts_len), 5);
        assert_eq!(offset_of!(IngressRecord, num_required_signatures), 6);
        assert_eq!(offset_of!(IngressRecord, num_readonly_signed), 7);
        assert_eq!(offset_of!(IngressRecord, entity_id), 8);
        assert_eq!(offset_of!(IngressRecord, pool_index_hint), 12);
        assert_eq!(offset_of!(IngressRecord, slot), 16);
        assert_eq!(offset_of!(IngressRecord, unix_ns), 24);
        assert_eq!(offset_of!(IngressRecord, amount_in), 32);
        assert_eq!(offset_of!(IngressRecord, pattern_acct_indices), 40);
        assert_eq!(offset_of!(IngressRecord, signature), 48);
        assert_eq!(offset_of!(IngressRecord, pool_pubkey), 112);
        assert_eq!(offset_of!(IngressRecord, mint_in), 144);
        assert_eq!(offset_of!(IngressRecord, mint_out), 176);
        assert_eq!(offset_of!(IngressRecord, num_readonly_unsigned), 208);
        assert_eq!(offset_of!(IngressRecord, alt_writable_count), 209);
        assert_eq!(offset_of!(IngressRecord, template_variant), 210);
        assert_eq!(offset_of!(IngressRecord, ix_account_count), 211);
        assert_eq!(offset_of!(IngressRecord, ix_account_indices), 212);
        assert_eq!(offset_of!(IngressRecord, _pad_1), 244);
        assert_eq!(offset_of!(IngressRecord, flat_accounts), 256);
    }

    // -- G4-A1-T2: zero-copy roundtrip via SPSC ring -------------------------

    #[test]
    fn g4_a1_ingress_record_ring_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_feed_ring");

        let ring = create_feed_ring(&path, 4).unwrap();

        let mut rec = IngressRecord::zeroed();
        rec.signal_source = 1;
        rec.venue_id = 0;
        rec.pattern_id = 0;
        rec.obscurity_score = 2;
        rec.flat_accounts_len = 3;
        rec.entity_id = 42;
        rec.slot = 300_000_000;
        rec.unix_ns = 1_700_000_000_000_000_000;
        rec.amount_in = 5_000_000_000;
        rec.signature = [0xAB; 64];
        rec.pool_pubkey = [0x01; 32];
        rec.mint_in = [0x02; 32];
        rec.mint_out = [0x03; 32];
        rec.flat_accounts[0] = [0x10; 32];
        rec.flat_accounts[1] = [0x11; 32];
        rec.flat_accounts[2] = [0x12; 32];

        assert!(ring.push(&rec));
        let popped = ring.pop().unwrap();

        assert_eq!(popped.signal_source, 1);
        assert_eq!(popped.venue_id, 0);
        assert_eq!(popped.obscurity_score, 2);
        assert_eq!(popped.flat_accounts_len, 3);
        assert_eq!(popped.entity_id, 42);
        assert_eq!(popped.slot, 300_000_000);
        assert_eq!(popped.unix_ns, 1_700_000_000_000_000_000);
        assert_eq!(popped.amount_in, 5_000_000_000);
        assert_eq!(popped.signature, [0xAB; 64]);
        assert_eq!(popped.pool_pubkey, [0x01; 32]);
        assert_eq!(popped.mint_in, [0x02; 32]);
        assert_eq!(popped.mint_out, [0x03; 32]);
        assert_eq!(popped.flat_accounts[0], [0x10; 32]);
        assert_eq!(popped.flat_accounts[1], [0x11; 32]);
        assert_eq!(popped.flat_accounts[2], [0x12; 32]);
        // Unused slots must remain zeroed.
        assert_eq!(popped.flat_accounts[3], [0u8; 32]);
        assert_eq!(popped.flat_accounts[63], [0u8; 32]);
    }

    // -- G4-A1-T3: fixed-size and pointer-free assertion ----------------------

    #[test]
    fn g4_a1_ingress_record_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<IngressRecord>();
    }

    #[test]
    fn g4_a1_ingress_record_size_is_multiple_of_align() {
        let size = core::mem::size_of::<IngressRecord>();
        let align = core::mem::align_of::<IngressRecord>();
        assert_eq!(size % align, 0, "size must be a multiple of alignment");
    }

    // -- Constants frozen tests -----------------------------------------------

    #[test]
    fn g4_a1_constants_frozen() {
        assert_eq!(FLAT_ACCOUNTS_CAP, 64);
        assert_eq!(OBSCURITY_SCORE_MAX, 3);
        assert_eq!(ACCT_INDEX_ABSENT, 0xFF);
        assert_eq!(POOL_INDEX_HINT_NONE, u32::MAX);
        assert_eq!(FEED_RING_SEGMENT_ID, 10);
        assert_eq!(FEED_RING_VERSION, 1);
    }

    // -- Feed ring open/reopen ------------------------------------------------

    #[test]
    fn g4_a1_feed_ring_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reopen_feed");

        {
            let ring = create_feed_ring(&path, 4).unwrap();
            let mut rec = IngressRecord::zeroed();
            rec.slot = 12345;
            ring.push(&rec);
            ring.flush().unwrap();
        }

        let ring2 = open_feed_ring_rw(&path).unwrap();
        let rec2 = ring2.pop().unwrap();
        assert_eq!(rec2.slot, 12345);
    }

    #[test]
    fn g4_a1_feed_ring_drop_on_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full_feed");

        let ring = create_feed_ring(&path, 4).unwrap();
        for i in 0..4u64 {
            let mut rec = IngressRecord::zeroed();
            rec.slot = i;
            assert!(ring.push(&rec));
        }
        // Ring is full — 5th push must return false.
        let mut rec = IngressRecord::zeroed();
        rec.slot = 999;
        assert!(!ring.push(&rec));
        assert_eq!(ring.stats().drops.load(Ordering::Relaxed), 1);
    }

    // -- Path helpers ---------------------------------------------------------

    #[test]
    fn g4_a1_feed_ring_path_format() {
        let p = feed_ring_path(3, 1);
        assert_eq!(p.to_str().unwrap(), "/dev/shm/shadow_feed_ring_w3_p1");
    }

    // -- zeroed() constructor -------------------------------------------------

    #[test]
    fn g4_a1_zeroed_has_sentinel_defaults() {
        let rec = IngressRecord::zeroed();
        assert_eq!(rec.pool_index_hint, POOL_INDEX_HINT_NONE);
        assert_eq!(rec.pattern_acct_indices, [ACCT_INDEX_ABSENT; 8]);
        assert_eq!(rec.signal_source, 0);
        assert_eq!(rec.flat_accounts_len, 0);
        assert_eq!(rec.flat_accounts[0], [0u8; 32]);
        assert_eq!(rec.decoded_minimum_amount_out(), 0);
    }

    // =========================================================================
    // G4-A2 — IngressRecord contract freeze tests
    // =========================================================================

    #[test]
    fn g4_a2_ingress_record_size_stable() {
        assert_eq!(core::mem::size_of::<IngressRecord>(), 2304);
    }

    #[test]
    fn g4_a2_ingress_record_alignment_stable() {
        assert_eq!(core::mem::align_of::<IngressRecord>(), 64);
    }

    #[test]
    fn g4_a2_ingress_record_offsets_stable() {
        use core::mem::offset_of;

        assert_eq!(offset_of!(IngressRecord, signal_source), 0);
        assert_eq!(offset_of!(IngressRecord, venue_id), 1);
        assert_eq!(offset_of!(IngressRecord, pattern_id), 2);
        assert_eq!(offset_of!(IngressRecord, obscurity_score), 3);
        assert_eq!(offset_of!(IngressRecord, flat_accounts_len), 4);
        assert_eq!(offset_of!(IngressRecord, entity_id), 8);
        assert_eq!(offset_of!(IngressRecord, pool_index_hint), 12);
        assert_eq!(offset_of!(IngressRecord, slot), 16);
        assert_eq!(offset_of!(IngressRecord, unix_ns), 24);
        assert_eq!(offset_of!(IngressRecord, amount_in), 32);
        assert_eq!(offset_of!(IngressRecord, pattern_acct_indices), 40);
        assert_eq!(offset_of!(IngressRecord, signature), 48);
        assert_eq!(offset_of!(IngressRecord, pool_pubkey), 112);
        assert_eq!(offset_of!(IngressRecord, mint_in), 144);
        assert_eq!(offset_of!(IngressRecord, mint_out), 176);
        assert_eq!(offset_of!(IngressRecord, flat_accounts), 256);
    }

    #[test]
    fn g4_a2_ingress_record_zeroed_unused_accounts() {
        let rec = IngressRecord::zeroed();
        // All 64 flat_accounts slots must be zeroed in a freshly-zeroed record.
        for (i, acct) in rec.flat_accounts.iter().enumerate() {
            assert_eq!(*acct, [0u8; 32], "flat_accounts[{i}] not zeroed");
        }
    }

    #[test]
    fn g4_a2_ingress_record_no_heap_fields() {
        // IngressRecord is Copy → no Vec, no String, no Box, no heap.
        fn assert_copy<T: Copy>() {}
        assert_copy::<IngressRecord>();
        // Size is a compile-time constant, proving no dynamic allocation.
        assert_eq!(
            core::mem::size_of::<IngressRecord>(),
            2304,
            "size must be fixed — no heap indirection"
        );
    }

    #[test]
    fn g4_a2_ingress_record_preserves_decoded_minimum_amount_out() {
        let mut rec = IngressRecord::zeroed();
        rec.set_decoded_minimum_amount_out(348_264_726_421);
        assert_eq!(rec.decoded_minimum_amount_out(), 348_264_726_421);
        assert_eq!(&rec._pad_1[8..], &[0u8; 4]);
    }

    #[test]
    fn g4_a2_ingress_record_marks_scorecard_replay_dev_proof_without_touching_min_out() {
        let mut rec = IngressRecord::zeroed();
        rec.set_decoded_minimum_amount_out(348_264_726_421);
        rec.mark_scorecard_replay_dev_proof();
        assert_eq!(rec.decoded_minimum_amount_out(), 348_264_726_421);
        assert!(rec.is_scorecard_replay_dev_proof());
    }

    // =========================================================================
    // G4-A3 — Flattened-account behavioral contract freeze
    // =========================================================================

    #[test]
    fn g4_a3_flatten_deterministic() {
        // ALT-expanded account order must be preserved exactly.
        let accts: Vec<[u8; 32]> = (0u8..10)
            .map(|i| {
                let mut k = [0u8; 32];
                k[0] = i;
                k
            })
            .collect();

        let mut rec = IngressRecord::zeroed();
        assert!(rec.set_flat_accounts(&accts));

        for (i, acct) in accts.iter().enumerate() {
            assert_eq!(rec.flat_accounts[i], *acct, "order diverged at index {i}");
        }
        assert_eq!(rec.flat_accounts_len, 10);
    }

    #[test]
    fn g4_a3_flatten_overflow_is_drop() {
        // > 64 accounts → hard drop (returns false), record unchanged.
        let too_many: Vec<[u8; 32]> = (0..65)
            .map(|i| {
                let mut k = [0u8; 32];
                k[0] = i as u8;
                k
            })
            .collect();

        let mut rec = IngressRecord::zeroed();
        assert!(
            !rec.set_flat_accounts(&too_many),
            "must reject >64 accounts"
        );
        // Record must remain untouched after rejection.
        assert_eq!(rec.flat_accounts_len, 0);
        assert_eq!(rec.flat_accounts[0], [0u8; 32]);
    }

    #[test]
    fn g4_a3_flatten_zero_fill_stable() {
        // Slots beyond flat_accounts_len must be zeroed after set.
        let accts: [[u8; 32]; 3] = [[0xAA; 32], [0xBB; 32], [0xCC; 32]];

        let mut rec = IngressRecord::zeroed();
        // First dirty all slots so zero-fill is actually tested.
        rec.flat_accounts = [[0xFF; 32]; FLAT_ACCOUNTS_CAP];
        rec.flat_accounts_len = FLAT_ACCOUNTS_CAP as u8;

        assert!(rec.set_flat_accounts(&accts));
        assert_eq!(rec.flat_accounts_len, 3);
        assert_eq!(rec.flat_accounts[0], [0xAA; 32]);
        assert_eq!(rec.flat_accounts[1], [0xBB; 32]);
        assert_eq!(rec.flat_accounts[2], [0xCC; 32]);
        for i in 3..FLAT_ACCOUNTS_CAP {
            assert_eq!(
                rec.flat_accounts[i], [0u8; 32],
                "slot {i} not zeroed after set_flat_accounts"
            );
        }
    }

    #[test]
    fn g4_a3_flatten_golden_fixture() {
        // Golden fixture: exact 5-account payload, roundtrip through ring.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g4a3_golden");
        let ring = create_feed_ring(&path, 4).unwrap();

        let accts: [[u8; 32]; 5] = [[0x01; 32], [0x02; 32], [0x03; 32], [0x04; 32], [0x05; 32]];

        let mut rec = IngressRecord::zeroed();
        rec.slot = 400_000_000;
        rec.signal_source = 2;
        rec.venue_id = 0;
        rec.pattern_id = 0;
        rec.obscurity_score = 1;
        rec.entity_id = 77;
        rec.amount_in = 1_000_000;
        rec.signature = [0xEE; 64];
        rec.pool_pubkey = [0xF0; 32];
        rec.mint_in = [0xF1; 32];
        rec.mint_out = [0xF2; 32];
        assert!(rec.set_flat_accounts(&accts));

        assert!(ring.push(&rec));
        let got = ring.pop().unwrap();

        // Scalar fields.
        assert_eq!(got.slot, 400_000_000);
        assert_eq!(got.signal_source, 2);
        assert_eq!(got.obscurity_score, 1);
        assert_eq!(got.entity_id, 77);
        assert_eq!(got.amount_in, 1_000_000);

        // Flattened accounts — exact order.
        assert_eq!(got.flat_accounts_len, 5);
        for (i, expected) in accts.iter().enumerate() {
            assert_eq!(got.flat_accounts[i], *expected, "golden mismatch at {i}");
        }
        // Unused slots zeroed.
        for i in 5..FLAT_ACCOUNTS_CAP {
            assert_eq!(got.flat_accounts[i], [0u8; 32], "golden slot {i} not zero");
        }
    }

    // =========================================================================
    // G4-A4 — obscurity_score contract freeze
    // =========================================================================

    #[test]
    fn g4_a4_obscurity_range() {
        let mut rec = IngressRecord::zeroed();
        // Every value in 0..=3 is accepted.
        for score in 0..=OBSCURITY_SCORE_MAX {
            assert!(rec.set_obscurity_score(score), "score {score} rejected");
            assert_eq!(rec.obscurity_score, score);
        }
    }

    #[test]
    fn g4_a4_obscurity_pack_index() {
        // obscurity_score is a direct fixed-width index into a [T; 4] table.
        let table: [&str; (OBSCURITY_SCORE_MAX as usize) + 1] =
            ["transparent", "low", "medium", "opaque"];

        let mut rec = IngressRecord::zeroed();
        for score in 0..=OBSCURITY_SCORE_MAX {
            rec.set_obscurity_score(score);
            // Direct index — no branching, no enum match.
            let label = table[rec.obscurity_score as usize];
            assert!(!label.is_empty(), "pack-index {score} broken");
        }
    }

    #[test]
    fn g4_a4_obscurity_invalid_value_behavior() {
        let mut rec = IngressRecord::zeroed();
        rec.set_obscurity_score(2);

        // Values > 3 are rejected; field stays at previous value.
        assert!(!rec.set_obscurity_score(4));
        assert_eq!(rec.obscurity_score, 2, "field mutated on reject");

        assert!(!rec.set_obscurity_score(u8::MAX));
        assert_eq!(rec.obscurity_score, 2, "field mutated on reject (u8::MAX)");
    }

    // =========================================================================
    // G4-A5 — feed-ring wrapper contract freeze
    // =========================================================================

    #[test]
    fn g4_a5_feed_ring_create_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a5_reopen");

        {
            let ring = create_feed_ring(&path, 16).unwrap();
            let mut rec = IngressRecord::zeroed();
            rec.slot = 77_777;
            rec.signal_source = 3;
            rec.amount_in = 42_000;
            rec.signature = [0xDD; 64];
            assert!(ring.push(&rec));
            ring.flush().unwrap();
        }

        // Reopen with matching segment ID + version.
        let ring2 = open_feed_ring_rw(&path).unwrap();
        let got = ring2.pop().unwrap();
        assert_eq!(got.slot, 77_777);
        assert_eq!(got.signal_source, 3);
        assert_eq!(got.amount_in, 42_000);
        assert_eq!(got.signature, [0xDD; 64]);
    }

    #[test]
    fn g4_a5_feed_ring_bad_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a5_bad_magic");

        // Create a valid feed ring then corrupt the magic bytes.
        create_feed_ring(&path, 4).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        // Magic is the first 8 bytes of the segment header.
        data[0..8].copy_from_slice(b"XXXXXXXX");
        std::fs::write(&path, &data).unwrap();

        let result = open_feed_ring_rw(&path);
        assert!(
            matches!(result, Err(crate::ring::RingError::BadMagic)),
            "corrupt magic must be rejected"
        );
    }

    #[test]
    fn g4_a5_feed_ring_wrong_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a5_bad_ver");

        create_feed_ring(&path, 4).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        // Version is at offset 52 in SegmentHeader64B (after magic[8]+git[8]+checksum[8]+seg_id[8]+payload_size[8]+total_size[8]+payload_offset[4]).
        data[52] = 99;
        data[53] = 0;
        std::fs::write(&path, &data).unwrap();

        let result = open_feed_ring_rw(&path);
        assert!(
            matches!(result, Err(crate::ring::RingError::WrongVersion { .. })),
            "wrong version must be rejected"
        );
    }

    #[test]
    fn g4_a5_feed_ring_wraparound_fifo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a5_wrap");

        let ring = create_feed_ring(&path, 4).unwrap();

        // Fill ring, drain, then fill again to force wraparound.
        for round in 0..3u64 {
            for i in 0..4u64 {
                let mut rec = IngressRecord::zeroed();
                rec.slot = round * 100 + i;
                assert!(ring.push(&rec), "push failed round={round} i={i}");
            }
            for i in 0..4u64 {
                let got = ring.pop().unwrap();
                assert_eq!(
                    got.slot,
                    round * 100 + i,
                    "FIFO order broken at round={round} i={i}"
                );
            }
        }
        // Ring must be empty after draining.
        assert!(ring.pop().is_none());
    }

    #[test]
    fn g4_a5_feed_ring_drop_on_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a5_full");

        let ring = create_feed_ring(&path, 4).unwrap();
        for i in 0..4u64 {
            let mut rec = IngressRecord::zeroed();
            rec.slot = i;
            assert!(ring.push(&rec));
        }
        // 5th push must fail — drop-on-full, never block.
        let mut overflow = IngressRecord::zeroed();
        overflow.slot = 999;
        assert!(!ring.push(&overflow));
        assert_eq!(ring.stats().drops.load(Ordering::Relaxed), 1);

        // Existing data intact.
        let first = ring.pop().unwrap();
        assert_eq!(first.slot, 0);
    }
}

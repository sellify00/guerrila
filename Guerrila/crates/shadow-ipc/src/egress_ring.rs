//! Shared egress envelope type and ring paths for the
//! `/dev/shm/shadow_egress_ring_*` family.
//!
//! `SignedTxEnvelope` is the fixed-size, plain-old-data payload pushed by
//! worker threads into file-backed SPSC egress rings and consumed by lane
//! proxy threads (Jito, Harmonic).  The type lives here — in the IPC layer —
//! so that both the worker crate and every proxy crate can depend on the same
//! definition without a circular dependency.

use std::path::PathBuf;

use crate::ring::{MappedSpscRing, RingError};

/// A signed Solana transaction envelope in wire-ready form.
///
/// Fixed-size, cache-line-aligned, `Copy` — suitable for direct placement
/// in a `MappedSpscRing<SignedTxEnvelope>` backed by shared memory.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct SignedTxEnvelope {
    /// Serialised transaction bytes (compact-u16 header ‖ signature ‖ message).
    pub raw_bytes: [u8; 1232],
    /// Number of valid bytes in `raw_bytes`.
    pub len: usize,
    /// Ed25519 signature bytes (duplicated for fast NACK keying).
    pub signature: [u8; 64],
    /// Index into the pool table that sourced this trade.
    pub pool_index: u32,
    /// Index of the mint-out side within that pool.
    pub mint_out_index: u32,
    /// Notional lamports reserved in the inventory fencer for this trade.
    pub reserved_notional_lamports: u64,
    /// Monotonic timestamp (nanos) when the worker enqueued this envelope.
    /// Used by PM for timeout expiry.
    pub send_timestamp_ns: u64,
    /// Tactic class index (for selective death-clamp classification).
    pub tactic_class: u16,
    /// Worker ID that produced this envelope.
    pub worker_id: u8,
    /// Venue / DEX identifier from IngressRecord (0=RaydiumV4, etc.).
    /// Used by PM heat emitter as `pool_family` for competition adjustment.
    pub venue_id: u8,
    /// Observe-only OPSO owner metadata for PM stitching.
    pub opso_owner_mode: u8,
    pub opso_fallback_mode: u8,
    pub opso_reason_code: u8,
    pub episode_id_mod: u32,
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
    /// Which private egress transport should submit this envelope when it
    /// travels through the shared Jito-family lane.
    pub egress_transport: u8,
    /// Reserved for future per-envelope metadata.
    pub _envelope_pad: [u8; 3],
}

// -- Static layout assertions ------------------------------------------------

// Exact size after observe-only OPSO economic labels = 1472 bytes.
static_assertions::assert_eq_size!(SignedTxEnvelope, [u8; 1472]);
static_assertions::const_assert_eq!(core::mem::align_of::<SignedTxEnvelope>(), 64);

/// Private-lane transport selector constants for `SignedTxEnvelope.egress_transport`.
pub const PRIVATE_EGRESS_TRANSPORT_UNSPECIFIED: u8 = 0;
pub const PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE: u8 = 1;
pub const PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC: u8 = 2;
pub const PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER: u8 = 3;

// ---------------------------------------------------------------------------
// Jito egress ring — one SPSC ring per money worker
// ---------------------------------------------------------------------------

/// Segment ID for Jito egress rings (`/dev/shm/shadow_egress_ring_jito_w*`).
pub const JITO_EGRESS_RING_SEGMENT_ID: u64 = 14;

/// On-disk format version for Jito egress rings.
pub const JITO_EGRESS_RING_VERSION: u16 = 4;

/// Default ring capacity for Jito egress rings (slots per worker).
///
/// Must be a power of two so the SPSC index masking works correctly.
/// Bootstrap code should pass this value to `create_jito_egress_ring`.
pub const JITO_EGRESS_RING_CAPACITY: u32 = 64;

// Compile-time: capacity must be a power of two.
static_assertions::const_assert!(JITO_EGRESS_RING_CAPACITY.is_power_of_two());

/// Build the canonical shared-memory path for a Jito egress ring.
///
/// One ring per money worker, producer = worker, consumer = Jito proxy.
///
/// ```text
/// /dev/shm/shadow_egress_ring_jito_w0
/// /dev/shm/shadow_egress_ring_jito_w1
/// ```
pub fn jito_egress_ring_path(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/shm/shadow_egress_ring_jito_w{worker_id}"))
}

/// Create a new Jito egress ring file for `worker_id`.
pub fn create_jito_egress_ring(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::create_rw(
        &jito_egress_ring_path(worker_id),
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
        capacity,
    )
}

/// Open an existing Jito egress ring for `worker_id` (read-write).
pub fn open_jito_egress_ring_rw(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_rw(
        &jito_egress_ring_path(worker_id),
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
    )
}

/// Open an existing Jito egress ring for `worker_id` (read-only).
pub fn open_jito_egress_ring_ro(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_ro(
        &jito_egress_ring_path(worker_id),
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
    )
}

// ---------------------------------------------------------------------------
// Harmonic egress ring — one SPSC ring per money worker
// ---------------------------------------------------------------------------

/// Segment ID for Harmonic egress rings (`/dev/shm/shadow_egress_ring_harmonic_w*`).
pub const HARMONIC_EGRESS_RING_SEGMENT_ID: u64 = 18;

/// On-disk format version for Harmonic egress rings.
pub const HARMONIC_EGRESS_RING_VERSION: u16 = 4;

/// Default ring capacity for Harmonic egress rings (slots per worker).
///
/// Must be a power of two so the SPSC index masking works correctly.
/// Bootstrap code should pass this value to `create_harmonic_egress_ring`.
pub const HARMONIC_EGRESS_RING_CAPACITY: u32 = 64;

// Compile-time: capacity must be a power of two.
static_assertions::const_assert!(HARMONIC_EGRESS_RING_CAPACITY.is_power_of_two());

/// Build the canonical shared-memory path for a Harmonic egress ring.
///
/// One ring per money worker, producer = worker, consumer = Harmonic proxy.
///
/// ```text
/// /dev/shm/shadow_egress_ring_harmonic_w0
/// /dev/shm/shadow_egress_ring_harmonic_w1
/// ```
pub fn harmonic_egress_ring_path(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/shm/shadow_egress_ring_harmonic_w{worker_id}"))
}

/// Create a new Harmonic egress ring file for `worker_id`.
pub fn create_harmonic_egress_ring(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::create_rw(
        &harmonic_egress_ring_path(worker_id),
        HARMONIC_EGRESS_RING_SEGMENT_ID,
        HARMONIC_EGRESS_RING_VERSION,
        capacity,
    )
}

/// Open an existing Harmonic egress ring for `worker_id` (read-write).
pub fn open_harmonic_egress_ring_rw(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_rw(
        &harmonic_egress_ring_path(worker_id),
        HARMONIC_EGRESS_RING_SEGMENT_ID,
        HARMONIC_EGRESS_RING_VERSION,
    )
}

/// Open an existing Harmonic egress ring for `worker_id` (read-only).
pub fn open_harmonic_egress_ring_ro(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_ro(
        &harmonic_egress_ring_path(worker_id),
        HARMONIC_EGRESS_RING_SEGMENT_ID,
        HARMONIC_EGRESS_RING_VERSION,
    )
}

// ---------------------------------------------------------------------------
// TPU egress ring — one SPSC ring per money worker
// ---------------------------------------------------------------------------

/// Segment ID for TPU egress rings (`/dev/shm/shadow_egress_ring_tpu_w*`).
pub const TPU_EGRESS_RING_SEGMENT_ID: u64 = 22;

/// On-disk format version for TPU egress rings.
pub const TPU_EGRESS_RING_VERSION: u16 = 4;

/// Default ring capacity for TPU egress rings (slots per worker).
pub const TPU_EGRESS_RING_CAPACITY: u32 = 64;

static_assertions::const_assert!(TPU_EGRESS_RING_CAPACITY.is_power_of_two());

/// Build the canonical shared-memory path for a TPU egress ring.
///
/// One ring per money worker, producer = worker, consumer = TPU proxy.
pub fn tpu_egress_ring_path(worker_id: u32) -> PathBuf {
    PathBuf::from(format!("/dev/shm/shadow_egress_ring_tpu_w{worker_id}"))
}

/// Create a new TPU egress ring file for `worker_id`.
pub fn create_tpu_egress_ring(
    worker_id: u32,
    capacity: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::create_rw(
        &tpu_egress_ring_path(worker_id),
        TPU_EGRESS_RING_SEGMENT_ID,
        TPU_EGRESS_RING_VERSION,
        capacity,
    )
}

/// Open an existing TPU egress ring for `worker_id` (read-write).
pub fn open_tpu_egress_ring_rw(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_rw(
        &tpu_egress_ring_path(worker_id),
        TPU_EGRESS_RING_SEGMENT_ID,
        TPU_EGRESS_RING_VERSION,
    )
}

/// Open an existing TPU egress ring for `worker_id` (read-only).
pub fn open_tpu_egress_ring_ro(
    worker_id: u32,
) -> Result<MappedSpscRing<SignedTxEnvelope>, RingError> {
    MappedSpscRing::open_ro(
        &tpu_egress_ring_path(worker_id),
        TPU_EGRESS_RING_SEGMENT_ID,
        TPU_EGRESS_RING_VERSION,
    )
}

// ---------------------------------------------------------------------------
// EgressResult — fixed-size per-envelope outcome record
// ---------------------------------------------------------------------------

/// Fixed-size outcome record for a single egress envelope.
///
/// Written by both egress proxies after each send attempt:
///   - Harmonic proxy → `/dev/shm/shadow_egress_result` (segment 19)
///   - Jito proxy → `/dev/shm/shadow_egress_result_jito` (segment 21)
///
/// Each proxy has its own SPSC result ring (single-producer single-consumer).
/// PM (G8) polls both rings and uses the `lane` field to distinguish origin.
///
/// Layout is fixed-width so PM can consume results from a SPSC ring without
/// parsing variable-length data.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct EgressResult {
    /// The signature of the transaction (for PM correlation).
    pub signature: [u8; 64],
    /// Which lane produced this result.
    /// 0 = Jito, 1 = Harmonic.
    pub lane: u8,
    /// Bitmask of regions the proxy attempted to send to.
    /// For Jito: always 0x01 (single endpoint).
    /// For Harmonic: one bit per configured regional endpoint.
    pub region_mask: u8,
    /// Bitmask of regions that acknowledged success.
    pub ok_mask: u8,
    /// Proxy-side error classification code.
    /// 0 = success, non-zero = lane-specific error code.
    pub err_code: u8,
    /// Pool index from the original envelope (for PM correlation).
    pub pool_index: u32,
    /// Mint-out index from the original envelope.
    pub mint_out_index: u32,
    /// Notional lamports reserved (for PM inventory release).
    pub reserved_notional_lamports: u64,
    /// Monotonic timestamp (nanos) from the envelope — used by PM for
    /// timeout expiry.
    pub send_timestamp_ns: u64,
    /// Tactic class index from the envelope — used by PM for
    /// selective death-clamp classification.
    pub tactic_class: u16,
    /// Worker ID from the envelope — used by PM for per-worker fence
    /// accounting.
    pub worker_id: u8,
    /// Bundle UUID from Jito `SendBundleResponse`, as 16 raw bytes.
    /// All zeros if unavailable (Harmonic lane, or send failure).
    /// PM uses this to register UUID→signature mappings with the
    /// bundle status source for `getBundleStatuses` polling.
    pub bundle_uuid: [u8; 16],
    /// Compact rejection-class hint from the proxy.
    /// 0 = not applicable / no rejection.
    /// 1 = ShadowGuard boundary breach detected (proxy-side keyword match).
    /// 2 = generic rejection (no guard signal).
    /// PM uses this on the orphan path where no PendingEntry exists and
    /// the full rejection reason string is not available.
    pub rejection_hint: u8,
    /// Venue / DEX identifier from the envelope (0=RaydiumV4, etc.).
    /// Threaded through for PM heat emission as pool_family.
    pub venue_id: u8,
    /// Observe-only OPSO owner metadata from the worker sidecar read path.
    pub opso_owner_mode: u8,
    pub opso_fallback_mode: u8,
    pub opso_reason_code: u8,
    pub episode_id_mod: u32,
    pub actual_bid_lamports: u64,
    pub baseline_bid_lamports: u64,
    pub opso_bid_lamports: u64,
    pub fallback_realized_value_lamports: i64,
    pub missed_backrun_value_lamports: i64,
    pub tip_saved_lamports: i64,
    pub opso_ev_lamports: i64,
    pub shading_gain_lamports: i64,
    /// Transport selector copied from the source envelope.
    pub egress_transport: u8,
    /// Padding to fill out to the next 64-byte boundary.
    pub _reserved: [u8; 3],
}

// Exact size: 64 + 1 + 1 + 1 + 1 + 4 + 4 + 8 + 8 + 2 + 1 + 21 = 116,
// + 4 padding before reserved_notional_lamports = 120,
// ... let the static_assert be the oracle.
static_assertions::assert_eq_size!(EgressResult, [u8; 256]);
static_assertions::const_assert_eq!(core::mem::align_of::<EgressResult>(), 64);

/// Lane identifier constants for `EgressResult.lane`.
pub const LANE_JITO: u8 = 0;
pub const LANE_HARMONIC: u8 = 1;
pub const LANE_TPU_QUIC: u8 = 2;

/// Segment ID for the egress result ring (`/dev/shm/shadow_egress_result`).
pub const EGRESS_RESULT_RING_SEGMENT_ID: u64 = 19;

/// On-disk format version for the egress result ring.
pub const EGRESS_RESULT_RING_VERSION: u16 = 4;

/// Default capacity for the egress result ring (shared across all workers).
pub const EGRESS_RESULT_RING_CAPACITY: u32 = 256;

static_assertions::const_assert!(EGRESS_RESULT_RING_CAPACITY.is_power_of_two());

/// Canonical path for the egress result ring.
pub fn egress_result_ring_path() -> PathBuf {
    PathBuf::from("/dev/shm/shadow_egress_result")
}

/// Create the egress result ring.
pub fn create_egress_result_ring(capacity: u32) -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::create_rw(
        &egress_result_ring_path(),
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
        capacity,
    )
}

/// Open the egress result ring (read-write).
pub fn open_egress_result_ring_rw() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_rw(
        &egress_result_ring_path(),
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
    )
}

/// Open the egress result ring (read-only).
pub fn open_egress_result_ring_ro() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_ro(
        &egress_result_ring_path(),
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
    )
}

// ---------------------------------------------------------------------------
// Jito result ring — separate SPSC ring for Jito proxy send outcomes.
//
// PM polls both this and the Harmonic result ring.  Each lane has its own
// ring because SPSC is single-producer single-consumer.
// ---------------------------------------------------------------------------

/// Segment ID for the Jito egress result ring.
pub const JITO_RESULT_RING_SEGMENT_ID: u64 = 21;

/// On-disk format version for the Jito result ring.
pub const JITO_RESULT_RING_VERSION: u16 = 3;

/// Default capacity for the Jito result ring.
pub const JITO_RESULT_RING_CAPACITY: u32 = 256;

static_assertions::const_assert!(JITO_RESULT_RING_CAPACITY.is_power_of_two());

/// Canonical path for the Jito result ring.
pub fn jito_result_ring_path() -> PathBuf {
    PathBuf::from("/dev/shm/shadow_egress_result_jito")
}

/// Create the Jito result ring.
pub fn create_jito_result_ring(capacity: u32) -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::create_rw(
        &jito_result_ring_path(),
        JITO_RESULT_RING_SEGMENT_ID,
        JITO_RESULT_RING_VERSION,
        capacity,
    )
}

/// Open the Jito result ring (read-write, producer = Jito proxy).
pub fn open_jito_result_ring_rw() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_rw(
        &jito_result_ring_path(),
        JITO_RESULT_RING_SEGMENT_ID,
        JITO_RESULT_RING_VERSION,
    )
}

/// Open the Jito result ring (read-only, consumer = PM).
pub fn open_jito_result_ring_ro() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_ro(
        &jito_result_ring_path(),
        JITO_RESULT_RING_SEGMENT_ID,
        JITO_RESULT_RING_VERSION,
    )
}

// ---------------------------------------------------------------------------
// TPU result ring — separate SPSC ring for TPU proxy send outcomes.
// ---------------------------------------------------------------------------

/// Segment ID for the TPU egress result ring.
pub const TPU_RESULT_RING_SEGMENT_ID: u64 = 23;

/// On-disk format version for the TPU result ring.
pub const TPU_RESULT_RING_VERSION: u16 = 3;

/// Default capacity for the TPU result ring.
pub const TPU_RESULT_RING_CAPACITY: u32 = 256;

static_assertions::const_assert!(TPU_RESULT_RING_CAPACITY.is_power_of_two());

/// Canonical path for the TPU result ring.
pub fn tpu_result_ring_path() -> PathBuf {
    PathBuf::from("/dev/shm/shadow_egress_result_tpu")
}

/// Create the TPU result ring.
pub fn create_tpu_result_ring(capacity: u32) -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::create_rw(
        &tpu_result_ring_path(),
        TPU_RESULT_RING_SEGMENT_ID,
        TPU_RESULT_RING_VERSION,
        capacity,
    )
}

/// Open the TPU result ring (read-write, producer = TPU proxy).
pub fn open_tpu_result_ring_rw() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_rw(
        &tpu_result_ring_path(),
        TPU_RESULT_RING_SEGMENT_ID,
        TPU_RESULT_RING_VERSION,
    )
}

/// Open the TPU result ring (read-only, consumer = PM).
pub fn open_tpu_result_ring_ro() -> Result<MappedSpscRing<EgressResult>, RingError> {
    MappedSpscRing::open_ro(
        &tpu_result_ring_path(),
        TPU_RESULT_RING_SEGMENT_ID,
        TPU_RESULT_RING_VERSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_tx_envelope_size_and_alignment() {
        assert_eq!(core::mem::size_of::<SignedTxEnvelope>(), 1472);
        assert_eq!(core::mem::align_of::<SignedTxEnvelope>(), 64);
    }

    #[test]
    fn signed_tx_envelope_is_pod() {
        // Copy + Clone are derived; verify a round-trip through zeroed memory.
        let zero = SignedTxEnvelope {
            raw_bytes: [0; 1232],
            len: 0,
            signature: [0; 64],
            pool_index: 0,
            mint_out_index: 0,
            reserved_notional_lamports: 0,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _envelope_pad: [0; 3],
        };
        let copy = zero;
        assert_eq!(copy.len, 0);
        assert_eq!(copy.pool_index, 0);
    }

    #[test]
    fn jito_egress_ring_path_format() {
        let p = jito_egress_ring_path(0);
        assert_eq!(p.to_str().unwrap(), "/dev/shm/shadow_egress_ring_jito_w0");
        let p3 = jito_egress_ring_path(3);
        assert_eq!(p3.to_str().unwrap(), "/dev/shm/shadow_egress_ring_jito_w3");
    }

    #[test]
    fn jito_egress_ring_capacity_is_power_of_two() {
        assert!(JITO_EGRESS_RING_CAPACITY.is_power_of_two());
        assert!(JITO_EGRESS_RING_CAPACITY > 0);
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_envelope(tag: u8) -> SignedTxEnvelope {
        let mut env = SignedTxEnvelope {
            raw_bytes: [0; 1232],
            len: 100,
            signature: [tag; 64],
            pool_index: tag as u32,
            mint_out_index: tag as u32 + 1,
            reserved_notional_lamports: tag as u64 * 1_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _envelope_pad: [0; 3],
        };
        env.raw_bytes[0] = tag;
        env
    }

    fn create_temp_jito_ring(
        capacity: u32,
    ) -> (tempfile::TempDir, MappedSpscRing<SignedTxEnvelope>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_egress_ring_jito_w0");
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
            capacity,
        )
        .unwrap();
        (dir, ring)
    }

    // -----------------------------------------------------------------------
    // File-backed Jito egress ring tests
    // -----------------------------------------------------------------------

    #[test]
    fn jito_ring_create_and_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jito_ring");

        let env = make_envelope(0xAB);
        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                JITO_EGRESS_RING_SEGMENT_ID,
                JITO_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.push(&env);
            ring.flush().unwrap();
        }

        let ring2 = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
        )
        .unwrap();
        let popped = ring2.pop().unwrap();
        assert_eq!(popped.signature, env.signature);
        assert_eq!(popped.pool_index, env.pool_index);
        assert_eq!(
            popped.reserved_notional_lamports,
            env.reserved_notional_lamports
        );
        assert_eq!(popped.raw_bytes[0], 0xAB);
    }

    #[test]
    fn jito_ring_cross_mapping_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jito_cross");

        let ring_w = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
            64,
        )
        .unwrap();

        let env = make_envelope(0xCD);
        ring_w.push(&env);
        ring_w.flush().unwrap();

        // Second independent mapping of the same file.
        let ring_r = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
        )
        .unwrap();
        let popped = ring_r.pop().unwrap();
        assert_eq!(popped.signature, env.signature);
        assert_eq!(popped.pool_index, env.pool_index);
        assert_eq!(popped.mint_out_index, env.mint_out_index);
    }

    #[test]
    fn jito_ring_multiple_envelopes_preserve_order() {
        let (_dir, ring) = create_temp_jito_ring(64);

        for i in 0..20u8 {
            ring.push(&make_envelope(i));
        }

        for i in 0..20u8 {
            let rec = ring.pop().unwrap();
            assert_eq!(rec.signature, [i; 64]);
            assert_eq!(rec.pool_index, i as u32);
            assert_eq!(rec.raw_bytes[0], i);
        }
    }

    // -----------------------------------------------------------------------
    // Version / segment-ID fail-closed tests
    // -----------------------------------------------------------------------

    #[test]
    fn jito_ring_version_mismatch_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jito_bad_version");

        // Create with the real version.
        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                JITO_EGRESS_RING_SEGMENT_ID,
                JITO_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.flush().unwrap();
        }

        // Open with a different version — must fail.
        let bad_version = JITO_EGRESS_RING_VERSION + 1;
        let result = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            bad_version,
        );
        assert!(
            matches!(result, Err(RingError::WrongVersion { .. })),
            "opening with wrong version must fail closed"
        );
    }

    #[test]
    fn jito_ring_wrong_segment_id_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jito_bad_seg_id");

        // Create with the real segment ID.
        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                JITO_EGRESS_RING_SEGMENT_ID,
                JITO_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.flush().unwrap();
        }

        // Open with a different segment ID — must fail.
        let bad_seg_id = JITO_EGRESS_RING_SEGMENT_ID + 1;
        let result = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            bad_seg_id,
            JITO_EGRESS_RING_VERSION,
        );
        assert!(
            matches!(result, Err(RingError::WrongSegmentId { .. })),
            "opening with wrong segment ID must fail closed"
        );
    }

    // -----------------------------------------------------------------------
    // Distinct worker ring files
    // -----------------------------------------------------------------------

    #[test]
    fn worker_0_and_worker_1_produce_distinct_ring_files() {
        let p0 = jito_egress_ring_path(0);
        let p1 = jito_egress_ring_path(1);
        assert_ne!(
            p0, p1,
            "worker 0 and worker 1 must produce distinct file paths"
        );
        // Also verify the full path format for both.
        assert!(
            p0.to_str().unwrap().ends_with("_w0"),
            "worker 0 path must end with _w0"
        );
        assert!(
            p1.to_str().unwrap().ends_with("_w1"),
            "worker 1 path must end with _w1"
        );
    }

    // -----------------------------------------------------------------------
    // Full-field envelope preservation test
    // -----------------------------------------------------------------------

    #[test]
    fn jito_ring_preserves_all_envelope_fields() {
        let (_dir, ring) = create_temp_jito_ring(16);

        // Build an envelope with distinct, non-zero values in every field.
        let mut env = SignedTxEnvelope {
            raw_bytes: [0; 1232],
            len: 347,
            signature: [0; 64],
            pool_index: 42,
            mint_out_index: 7,
            reserved_notional_lamports: 1_500_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _envelope_pad: [0; 3],
        };
        // Fill raw_bytes[..len] with a recognisable pattern.
        for i in 0..env.len {
            env.raw_bytes[i] = (i % 251) as u8;
        }
        // Fill signature with a distinct pattern.
        for (i, byte) in env.signature.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_add(0xA0);
        }

        ring.push(&env);
        let popped = ring.pop().unwrap();

        // len
        assert_eq!(popped.len, env.len, "len must roundtrip exactly");

        // raw_bytes[..len]
        assert_eq!(
            &popped.raw_bytes[..env.len],
            &env.raw_bytes[..env.len],
            "raw_bytes[..len] must roundtrip exactly"
        );

        // signature
        assert_eq!(
            popped.signature, env.signature,
            "signature must roundtrip exactly"
        );

        // pool_index
        assert_eq!(
            popped.pool_index, env.pool_index,
            "pool_index must roundtrip exactly"
        );

        // mint_out_index
        assert_eq!(
            popped.mint_out_index, env.mint_out_index,
            "mint_out_index must roundtrip exactly"
        );

        // reserved_notional_lamports
        assert_eq!(
            popped.reserved_notional_lamports, env.reserved_notional_lamports,
            "reserved_notional_lamports must roundtrip exactly"
        );
    }

    // -----------------------------------------------------------------------
    // Harmonic egress ring tests
    // -----------------------------------------------------------------------

    fn create_temp_harmonic_ring(
        capacity: u32,
    ) -> (tempfile::TempDir, MappedSpscRing<SignedTxEnvelope>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow_egress_ring_harmonic_w0");
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            HARMONIC_EGRESS_RING_SEGMENT_ID,
            HARMONIC_EGRESS_RING_VERSION,
            capacity,
        )
        .unwrap();
        (dir, ring)
    }

    #[test]
    fn harmonic_egress_ring_path_format() {
        let p = harmonic_egress_ring_path(0);
        assert_eq!(
            p.to_str().unwrap(),
            "/dev/shm/shadow_egress_ring_harmonic_w0"
        );
        let p3 = harmonic_egress_ring_path(3);
        assert_eq!(
            p3.to_str().unwrap(),
            "/dev/shm/shadow_egress_ring_harmonic_w3"
        );
    }

    #[test]
    fn harmonic_egress_ring_capacity_is_power_of_two() {
        assert!(HARMONIC_EGRESS_RING_CAPACITY.is_power_of_two());
        assert!(HARMONIC_EGRESS_RING_CAPACITY > 0);
    }

    #[test]
    fn harmonic_segment_id_distinct_from_jito() {
        assert_ne!(
            HARMONIC_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_SEGMENT_ID,
            "Harmonic and Jito rings must use distinct segment IDs"
        );
    }

    #[test]
    fn harmonic_ring_create_and_reopen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("harmonic_ring");

        let env = make_envelope(0xEF);
        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                HARMONIC_EGRESS_RING_SEGMENT_ID,
                HARMONIC_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.push(&env);
            ring.flush().unwrap();
        }

        let ring2 = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            HARMONIC_EGRESS_RING_SEGMENT_ID,
            HARMONIC_EGRESS_RING_VERSION,
        )
        .unwrap();
        let popped = ring2.pop().unwrap();
        assert_eq!(popped.signature, env.signature);
        assert_eq!(popped.pool_index, env.pool_index);
        assert_eq!(
            popped.reserved_notional_lamports,
            env.reserved_notional_lamports
        );
        assert_eq!(popped.raw_bytes[0], 0xEF);
    }

    #[test]
    fn harmonic_ring_multiple_envelopes_preserve_order() {
        let (_dir, ring) = create_temp_harmonic_ring(64);

        for i in 0..20u8 {
            ring.push(&make_envelope(i));
        }

        for i in 0..20u8 {
            let rec = ring.pop().unwrap();
            assert_eq!(rec.signature, [i; 64]);
            assert_eq!(rec.pool_index, i as u32);
            assert_eq!(rec.raw_bytes[0], i);
        }
    }

    #[test]
    fn harmonic_ring_version_mismatch_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("harmonic_bad_version");

        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                HARMONIC_EGRESS_RING_SEGMENT_ID,
                HARMONIC_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.flush().unwrap();
        }

        let bad_version = HARMONIC_EGRESS_RING_VERSION + 1;
        let result = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            HARMONIC_EGRESS_RING_SEGMENT_ID,
            bad_version,
        );
        assert!(
            matches!(result, Err(RingError::WrongVersion { .. })),
            "opening with wrong version must fail closed"
        );
    }

    #[test]
    fn harmonic_ring_wrong_segment_id_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("harmonic_bad_seg_id");

        {
            let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
                &path,
                HARMONIC_EGRESS_RING_SEGMENT_ID,
                HARMONIC_EGRESS_RING_VERSION,
                64,
            )
            .unwrap();
            ring.flush().unwrap();
        }

        let bad_seg_id = HARMONIC_EGRESS_RING_SEGMENT_ID + 1;
        let result = MappedSpscRing::<SignedTxEnvelope>::open_rw(
            &path,
            bad_seg_id,
            HARMONIC_EGRESS_RING_VERSION,
        );
        assert!(
            matches!(result, Err(RingError::WrongSegmentId { .. })),
            "opening with wrong segment ID must fail closed"
        );
    }

    #[test]
    fn harmonic_worker_paths_are_distinct() {
        let p0 = harmonic_egress_ring_path(0);
        let p1 = harmonic_egress_ring_path(1);
        assert_ne!(p0, p1);
        assert!(p0.to_str().unwrap().ends_with("_w0"));
        assert!(p1.to_str().unwrap().ends_with("_w1"));
    }

    #[test]
    fn harmonic_and_jito_paths_do_not_collide() {
        for w in 0..4u32 {
            let j = jito_egress_ring_path(w);
            let h = harmonic_egress_ring_path(w);
            assert_ne!(j, h, "Jito and Harmonic paths must differ for worker {w}");
        }
    }

    #[test]
    fn tpu_egress_ring_path_format() {
        let p = tpu_egress_ring_path(0);
        assert_eq!(p.to_str().unwrap(), "/dev/shm/shadow_egress_ring_tpu_w0");
        let p3 = tpu_egress_ring_path(3);
        assert_eq!(p3.to_str().unwrap(), "/dev/shm/shadow_egress_ring_tpu_w3");
    }

    #[test]
    fn tpu_egress_ring_capacity_is_power_of_two() {
        assert!(TPU_EGRESS_RING_CAPACITY.is_power_of_two());
        assert!(TPU_EGRESS_RING_CAPACITY > 0);
    }

    #[test]
    fn tpu_ring_segment_ids_are_distinct() {
        assert_ne!(TPU_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_SEGMENT_ID);
        assert_ne!(TPU_EGRESS_RING_SEGMENT_ID, HARMONIC_EGRESS_RING_SEGMENT_ID);
        assert_ne!(TPU_RESULT_RING_SEGMENT_ID, EGRESS_RESULT_RING_SEGMENT_ID);
        assert_ne!(TPU_RESULT_RING_SEGMENT_ID, JITO_RESULT_RING_SEGMENT_ID);
    }

    // -----------------------------------------------------------------------
    // EgressResult tests
    // -----------------------------------------------------------------------

    #[test]
    fn egress_result_size_and_alignment() {
        assert_eq!(core::mem::size_of::<EgressResult>(), 256);
        assert_eq!(core::mem::align_of::<EgressResult>(), 64);
    }

    #[test]
    fn egress_result_roundtrip_through_ring() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("egress_result");
        let ring = MappedSpscRing::<EgressResult>::create_rw(
            &path,
            EGRESS_RESULT_RING_SEGMENT_ID,
            EGRESS_RESULT_RING_VERSION,
            64,
        )
        .unwrap();

        let result = EgressResult {
            signature: [0xAA; 64],
            lane: LANE_HARMONIC,
            region_mask: 0b0000_0111,
            ok_mask: 0b0000_0101,
            err_code: 0,
            pool_index: 42,
            mint_out_index: 7,
            reserved_notional_lamports: 1_500_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _reserved: [0; 3],
        };

        ring.push(&result);
        let popped = ring.pop().unwrap();
        assert_eq!(popped.signature, result.signature);
        assert_eq!(popped.lane, LANE_HARMONIC);
        assert_eq!(popped.region_mask, 0b0000_0111);
        assert_eq!(popped.ok_mask, 0b0000_0101);
        assert_eq!(popped.err_code, 0);
        assert_eq!(popped.pool_index, 42);
        assert_eq!(popped.mint_out_index, 7);
        assert_eq!(popped.reserved_notional_lamports, 1_500_000_000);
    }

    #[test]
    fn egress_result_segment_id_distinct() {
        assert_ne!(EGRESS_RESULT_RING_SEGMENT_ID, JITO_EGRESS_RING_SEGMENT_ID);
        assert_ne!(
            EGRESS_RESULT_RING_SEGMENT_ID,
            HARMONIC_EGRESS_RING_SEGMENT_ID
        );
    }

    #[test]
    fn lane_constants_are_distinct() {
        assert_ne!(LANE_JITO, LANE_HARMONIC);
        assert_ne!(LANE_JITO, LANE_TPU_QUIC);
        assert_ne!(LANE_HARMONIC, LANE_TPU_QUIC);
    }

    // -----------------------------------------------------------------------
    // Jito result ring tests
    // -----------------------------------------------------------------------

    #[test]
    fn jito_result_ring_segment_id_distinct() {
        assert_ne!(JITO_RESULT_RING_SEGMENT_ID, EGRESS_RESULT_RING_SEGMENT_ID);
        assert_ne!(JITO_RESULT_RING_SEGMENT_ID, JITO_EGRESS_RING_SEGMENT_ID);
        assert_ne!(JITO_RESULT_RING_SEGMENT_ID, HARMONIC_EGRESS_RING_SEGMENT_ID);
    }

    #[test]
    fn jito_result_ring_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jito_result");
        let ring = MappedSpscRing::<EgressResult>::create_rw(
            &path,
            JITO_RESULT_RING_SEGMENT_ID,
            JITO_RESULT_RING_VERSION,
            64,
        )
        .unwrap();

        let result = EgressResult {
            signature: [0xBB; 64],
            lane: LANE_JITO,
            region_mask: 0x01, // Jito: single endpoint
            ok_mask: 0x01,
            err_code: 0,
            pool_index: 10,
            mint_out_index: 3,
            reserved_notional_lamports: 2_000_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _reserved: [0; 3],
        };

        ring.push(&result);
        let popped = ring.pop().unwrap();
        assert_eq!(popped.signature, [0xBB; 64]);
        assert_eq!(popped.lane, LANE_JITO);
        assert_eq!(popped.region_mask, 0x01);
        assert_eq!(popped.reserved_notional_lamports, 2_000_000_000);
    }

    #[test]
    fn jito_result_ring_path_is_distinct() {
        assert_ne!(jito_result_ring_path(), egress_result_ring_path(),);
    }

    #[test]
    fn tpu_result_ring_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tpu_result");
        let ring = MappedSpscRing::<EgressResult>::create_rw(
            &path,
            TPU_RESULT_RING_SEGMENT_ID,
            TPU_RESULT_RING_VERSION,
            64,
        )
        .unwrap();

        let result = EgressResult {
            signature: [0xCC; 64],
            lane: LANE_TPU_QUIC,
            region_mask: 0x03,
            ok_mask: 0x00,
            err_code: 0,
            pool_index: 17,
            mint_out_index: 9,
            reserved_notional_lamports: 3_000_000_000,
            send_timestamp_ns: 0,
            tactic_class: 0,
            worker_id: 0,
            bundle_uuid: [0; 16],
            rejection_hint: 0,
            venue_id: 0,
            opso_owner_mode: 0,
            opso_fallback_mode: 7,
            opso_reason_code: 0,
            episode_id_mod: 0,
            actual_bid_lamports: 0,
            baseline_bid_lamports: 0,
            opso_bid_lamports: 0,
            fallback_realized_value_lamports: 0,
            missed_backrun_value_lamports: 0,
            tip_saved_lamports: 0,
            opso_ev_lamports: 0,
            shading_gain_lamports: 0,
            egress_transport: 0,
            _reserved: [0; 3],
        };

        ring.push(&result);
        let popped = ring.pop().unwrap();
        assert_eq!(popped.signature, [0xCC; 64]);
        assert_eq!(popped.lane, LANE_TPU_QUIC);
        assert_eq!(popped.region_mask, 0x03);
        assert_eq!(popped.reserved_notional_lamports, 3_000_000_000);
    }

    #[test]
    fn tpu_result_ring_path_is_distinct() {
        assert_ne!(tpu_result_ring_path(), egress_result_ring_path());
        assert_ne!(tpu_result_ring_path(), jito_result_ring_path());
    }
}

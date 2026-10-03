// G7 – Harmonic proxy core
//
// The proxy loop mirrors the Jito proxy (G6) with two additions:
//   - Regional fanout: send_bundle returns RegionalResult per region
//   - Death Clamp: any failure → NACK + drop; no TPU/public fallback
//
// Per iteration:
//   1. poll session (drive reconnect / keepalive per region)
//   2. round-robin over all worker rings, pop one envelope per ring
//   3. if session usable (Up | Degraded), send_bundle to all regions
//   4. if session not usable or send fails, emit NACK (Death Clamp)
//   5. publish fixed-width EgressResult for PM correlation
//   6. update health tracker
//
// Lane-local byte-blast only:
//   - The proxy never reads worker-only state.
//   - The proxy never touches routing decisions.
//   - The proxy never inspects economics, fees, or template contents
//     beyond lane-semantic validation.
//   - The worker has already decided route, fee, and template.
//   - The proxy blasts raw bytes, validates lane semantics, and NACKs failures.
//
// Post-sign immutability:
//   - Signed bytes are sent exactly as dequeued: envelope.raw_bytes[..envelope.len].
//   - The proxy must NOT patch the tip account.
//   - The proxy must NOT patch compute budget.
//   - The proxy must NOT patch the blockhash.
//   - The proxy must NOT rebuild the transaction from fields.
//
// Tactic-lane separation:
//   - Envelopes with atba=true or apex_partial=true must never reach Harmonic.
//   - The worker is responsible for routing; the proxy enforces nothing beyond
//     what `can_send()` and the lane-gate provide. Tactic filtering is a
//     worker-side responsibility (money_worker.rs route gate).

use crate::config::HarmonicEgressConfig;
use crate::health::{publish_harmonic_health, HarmonicHealthTracker};
use crate::session::{BundleRequest, HarmonicSession, SendError, SessionState};
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::egress_ring::{
    open_egress_result_ring_rw, open_harmonic_egress_ring_rw, EgressResult, SignedTxEnvelope,
    LANE_HARMONIC,
};
use shadow_ipc::nack::{open_shadow_nack_ring_rw, NackRecord128B};
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::segment::MappedSegment;

// ---------------------------------------------------------------------------
// Lane-semantic validation (read-only inspection, never mutates bytes)
// ---------------------------------------------------------------------------

/// ComputeBudget111111111111111111111111111111 (base58-decoded)
const COMPUTE_BUDGET_PROGRAM_ID: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];
/// System Program (all zeroes)
const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

/// ComputeBudget SetComputeUnit*Price* discriminator byte (0x03).
const CB_PRIORITY_FEE_DISC: u8 = 0x03;
/// SystemProgram Transfer discriminator (first 4 LE bytes).
const SYS_TRANSFER_DISC: [u8; 4] = [0x02, 0x00, 0x00, 0x00];

// ---------------------------------------------------------------------------
// Death Clamp error codes (written into EgressResult.err_code)
//
// PM (G8) uses these to classify proxy-side failures for quarantine and
// capital release.  Every non-zero code means the envelope was dropped.
// ---------------------------------------------------------------------------

/// Session not connected or not authenticated.
const DC_ERR_AUTH: u8 = 10;
/// RPC timed out on all attempted regions.
const DC_ERR_TIMEOUT: u8 = 11;
/// gRPC transport-level error on all attempted regions.
const DC_ERR_TRANSPORT: u8 = 12;
/// Server rejected the bundle (invalid, duplicate, etc.).
const DC_ERR_REJECTED: u8 = 13;
/// All regions attempted but zero acknowledged (aggregate failure).
const DC_ERR_ALL_REGIONS: u8 = 14;
/// Envelope failed lane-semantic validation (wrong template on lane).
const DC_ERR_LANE_VIOLATION: u8 = 15;

/// Why an envelope failed lane-semantic validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneViolationReason {
    /// No ComputeBudget::SetComputeUnitPrice instruction found.
    MissingCuPrice,
    /// A SystemProgram::Transfer (Jito tip pattern) was detected.
    JitoTipDetected,
    /// Message bytes could not be parsed (truncated or malformed).
    Unparseable,
}

/// Classify a rejection message into a compact hint byte for IPC threading.
///   0 = not applicable / no rejection
///   1 = ShadowGuard boundary breach detected
///   2 = generic rejection (no guard signal)
fn rejection_hint_for_msg(msg: &str) -> u8 {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("shadow_guard")
        || lower.contains("boundary")
        || lower.contains("assert_reserves")
    {
        1
    } else {
        2
    }
}

/// Read-only scan of the signed transaction bytes inside `raw_bytes[..len]`.
///
/// Returns `Ok(())` if the envelope satisfies Harmonic lane semantics:
///   - At least one ComputeBudget::SetComputeUnitPrice instruction exists
///   - No SystemProgram::Transfer instruction exists (Jito tip pattern)
///
/// Returns `Err(reason)` on any violation. Never modifies the buffer.
pub fn validate_lane_semantics(raw_bytes: &[u8], len: usize) -> Result<(), LaneViolationReason> {
    // raw_bytes layout: [1-byte sig_count] [64-byte sig] [message...]
    // Message starts at offset 65 for a single-signer tx.
    if len < 68 {
        return Err(LaneViolationReason::Unparseable);
    }
    let msg = &raw_bytes[65..len];
    // msg[0] = num_required_signatures, msg[1] = num_readonly_signed, msg[2] = num_readonly_unsigned
    // msg[3..] = compact-u16 account count, then account keys
    let (num_accounts, acct_start) = read_compact_u16(msg, 3)?;
    let keys_end = acct_start + (num_accounts as usize) * 32;
    // After account keys: 32-byte blockhash, then compact-u16 ix count
    let blockhash_end = keys_end + 32;
    if blockhash_end >= msg.len() {
        return Err(LaneViolationReason::Unparseable);
    }
    let (ix_count, mut cursor) = read_compact_u16(msg, blockhash_end)?;

    let mut has_priority_fee = false;
    for _ in 0..ix_count {
        if cursor >= msg.len() {
            return Err(LaneViolationReason::Unparseable);
        }
        let prog_idx = msg[cursor] as usize;
        cursor += 1;
        let (num_accts, next) = read_compact_u16(msg, cursor)?;
        cursor = next + num_accts as usize; // skip account indices
        if cursor >= msg.len() {
            return Err(LaneViolationReason::Unparseable);
        }
        let (data_len, data_start) = read_compact_u16(msg, cursor)?;
        let data_end = data_start + data_len as usize;
        if data_end > msg.len() {
            return Err(LaneViolationReason::Unparseable);
        }

        // Resolve program ID from the account keys table.
        let key = account_key(msg, acct_start, num_accounts, prog_idx)?;

        // Check: ComputeBudget::SetComputeUnitPrice (disc 0x03, data_len == 9)
        if key == &COMPUTE_BUDGET_PROGRAM_ID && data_len == 9 && data_start < msg.len() {
            if msg[data_start] == CB_PRIORITY_FEE_DISC {
                has_priority_fee = true;
            }
        }

        // Check: SystemProgram::Transfer (disc [2,0,0,0], data_len == 12)
        if key == &SYSTEM_PROGRAM_ID && data_len == 12 && data_start + 4 <= msg.len() {
            if msg[data_start..data_start + 4] == SYS_TRANSFER_DISC {
                return Err(LaneViolationReason::JitoTipDetected);
            }
        }

        cursor = data_end;
    }

    if !has_priority_fee {
        return Err(LaneViolationReason::MissingCuPrice);
    }
    Ok(())
}

/// Read a compact-u16 value starting at `offset` in `buf`.
fn read_compact_u16(buf: &[u8], offset: usize) -> Result<(u16, usize), LaneViolationReason> {
    if offset >= buf.len() {
        return Err(LaneViolationReason::Unparseable);
    }
    let b0 = buf[offset] as u16;
    if b0 < 0x80 {
        return Ok((b0, offset + 1));
    }
    if offset + 1 >= buf.len() {
        return Err(LaneViolationReason::Unparseable);
    }
    let b1 = buf[offset + 1] as u16;
    if b1 < 0x80 {
        return Ok(((b0 & 0x7f) | (b1 << 7), offset + 2));
    }
    if offset + 2 >= buf.len() {
        return Err(LaneViolationReason::Unparseable);
    }
    let b2 = buf[offset + 2] as u16;
    Ok(((b0 & 0x7f) | ((b1 & 0x7f) << 7) | (b2 << 14), offset + 3))
}

/// Return a reference to the 32-byte account key at `idx` in the message's
/// account table (not the raw_bytes table — the message-relative table).
fn account_key<'a>(
    msg: &'a [u8],
    acct_start: usize,
    num_accounts: u16,
    idx: usize,
) -> Result<&'a [u8; 32], LaneViolationReason> {
    if idx >= num_accounts as usize {
        return Err(LaneViolationReason::Unparseable);
    }
    let start = acct_start + idx * 32;
    let end = start + 32;
    if end > msg.len() {
        return Err(LaneViolationReason::Unparseable);
    }
    // SAFETY: slice is exactly 32 bytes and within bounds.
    Ok(msg[start..end].try_into().unwrap())
}

/// Result of a single dequeue-send attempt for one envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendAttemptResult {
    /// Bundle sent to Harmonic regions; no NACK emitted.
    Sent(RegionalResultSummary),
    /// Local failure before any network transmission; one NACK emitted (Death Clamp).
    NackEmitted,
    /// NACK should have been emitted but the NACK ring was full.
    NackRingFull,
    /// Egress ring was empty; nothing to do.
    RingEmpty,
    /// Envelope failed lane-semantic validation; one NACK emitted.
    LaneViolation,
}

/// Compact summary of a successful regional send (stored in SendAttemptResult).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionalResultSummary {
    pub region_mask: u8,
    pub ok_mask: u8,
    pub err_code: u8,
}

#[derive(Debug, thiserror::Error)]
pub enum HarmonicProxyError {
    #[error("failed to open harmonic egress ring for worker {worker_id}: {source}")]
    RingOpen {
        worker_id: u32,
        source: shadow_ipc::ring::RingError,
    },
    #[error("failed to open NACK ring: {0}")]
    NackRingOpen(shadow_ipc::ring::RingError),
    #[error("failed to open egress health segment: {0}")]
    HealthOpen(shadow_ipc::segment::SegmentError),
    #[error("failed to open egress result ring: {0}")]
    ResultRingOpen(shadow_ipc::ring::RingError),
}

// ---------------------------------------------------------------------------
// Startup contract: ProxyResources
// ---------------------------------------------------------------------------

/// All shared-memory resources the proxy requires at boot.
///
/// Opened (not created) by [`ProxyResources::open`].  If any resource
/// cannot be opened or has a version/segment-ID mismatch, the proxy fails
/// closed — it never creates ring files itself.  Bootstrap (`shadow-core`
/// startup) is responsible for creation.
pub struct ProxyResources {
    /// Per-worker Harmonic egress rings (consumer side).
    pub egress_rings: Vec<MappedSpscRing<SignedTxEnvelope>>,
    /// NACK ring (producer side).
    pub nack_ring: MappedSpscRing<NackRecord128B>,
    /// Egress health segment (writer side).
    pub health_segment: MappedSegment<EgressHealthSnapshot>,
    /// Egress result ring (producer side) — Harmonic lane.
    /// Fixed-width EgressResult records for PM correlation + capital release.
    /// The Jito proxy publishes to a separate result ring (segment 21).
    pub result_ring: MappedSpscRing<EgressResult>,
}

impl ProxyResources {
    /// Open all shared-memory resources.
    ///
    /// Opens exactly:
    ///   - one `open_harmonic_egress_ring_rw(w)` for each `w` in `0..money_workers`
    ///   - one `open_shadow_nack_ring_rw()`
    ///   - one `open_shadow_egress_health_rw()`
    ///   - one `open_egress_result_ring_rw()`
    ///
    /// Returns `Err` on the first failure; the proxy must treat this as fatal.
    pub fn open(config: &HarmonicEgressConfig) -> Result<Self, HarmonicProxyError> {
        let mut egress_rings = Vec::with_capacity(config.money_workers as usize);
        for w in 0..config.money_workers {
            let ring =
                open_harmonic_egress_ring_rw(w).map_err(|e| HarmonicProxyError::RingOpen {
                    worker_id: w,
                    source: e,
                })?;
            egress_rings.push(ring);
        }

        let nack_ring = open_shadow_nack_ring_rw().map_err(HarmonicProxyError::NackRingOpen)?;

        let health_segment =
            shadow_ipc::egress_health::open_shadow_egress_health_rw().map_err(|e| {
                HarmonicProxyError::HealthOpen(shadow_ipc::segment::SegmentError::Io(e))
            })?;

        let result_ring =
            open_egress_result_ring_rw().map_err(HarmonicProxyError::ResultRingOpen)?;

        Ok(ProxyResources {
            egress_rings,
            nack_ring,
            health_segment,
            result_ring,
        })
    }
}

/// The Harmonic egress proxy.
///
/// Mirrors the Jito proxy from G6 with regional fanout and Death Clamp.
/// Lane-local byte-blast only.
pub struct HarmonicProxy<S: HarmonicSession> {
    pub config: HarmonicEgressConfig,
    session: S,
    health_tracker: HarmonicHealthTracker,
    /// Running count of bundles sent since proxy start.
    pub bundles_sent: u64,
    /// Running count of NACKs emitted since proxy start.
    pub nacks_emitted: u64,
    /// Running count of envelopes death-clamped (dropped without successful send).
    pub death_clamped: u64,
    /// Running count of EgressResult records published since proxy start.
    pub results_published: u64,
    /// Next ring to service in round-robin order.
    next_ring_idx: usize,
}

impl<S: HarmonicSession> HarmonicProxy<S> {
    /// Create a new proxy with the given config and session implementation.
    pub fn new(config: HarmonicEgressConfig, session: S) -> Self {
        let health_tracker = HarmonicHealthTracker::new(
            config.degrade_failure_threshold,
            config.down_failure_threshold,
            config.hysteresis_recover_batches,
        );
        HarmonicProxy {
            config,
            session,
            health_tracker,
            bundles_sent: 0,
            nacks_emitted: 0,
            death_clamped: 0,
            results_published: 0,
            next_ring_idx: 0,
        }
    }

    /// Process one envelope from a worker ring.
    ///
    /// Death Clamp (exhaustive):
    ///   Once an envelope is dequeued from the Harmonic ring, it either gets
    ///   sent successfully to at least one region, or it is **dropped** with a
    ///   NACK emitted.  There is no third option — no TPU relay, no QUIC
    ///   fallback, no public RPC, no re-enqueue to another lane.
    ///
    ///   Drop triggers:
    ///     - Lane-semantic violation (wrong template on Harmonic lane)
    ///     - Auth failure / session not connected (pre-send)
    ///     - RPC timeout on all regions
    ///     - gRPC transport error on all regions
    ///     - Server rejection (invalid, duplicate, permission denied, etc.)
    ///     - All-regions-failed (ok_mask == 0 after fanout)
    ///     - Any other `send_bundle` error
    ///
    ///   On every outcome (success or drop), a fixed-width `EgressResult` is
    ///   published so PM can correlate capital release.
    pub fn process_one(
        &mut self,
        egress_ring: &MappedSpscRing<SignedTxEnvelope>,
        nack_ring: &MappedSpscRing<NackRecord128B>,
        result_ring: &MappedSpscRing<EgressResult>,
    ) -> SendAttemptResult {
        let envelope = match egress_ring.pop() {
            Some(e) => e,
            None => return SendAttemptResult::RingEmpty,
        };

        // Lane-semantic validation: reject Jito-shaped or malformed envelopes.
        // Read-only inspection — signed bytes are never modified.
        if let Err(_reason) = validate_lane_semantics(&envelope.raw_bytes, envelope.len) {
            return match self.death_clamp(&envelope, DC_ERR_LANE_VIOLATION, nack_ring, result_ring)
            {
                SendAttemptResult::NackEmitted => SendAttemptResult::LaneViolation,
                other => other,
            };
        }

        // No authenticated session → Death Clamp.
        if !self.session.can_send() {
            return self.death_clamp(&envelope, DC_ERR_AUTH, nack_ring, result_ring);
        }

        // Frame as a single-tx bundle and attempt regional fanout.
        let request = BundleRequest::single_tx(&envelope.raw_bytes[..envelope.len]);
        match self.session.send_bundle(&request) {
            Ok(result) => {
                // Death Clamp: if ALL regions failed, drop.
                // This is a defense-in-depth check — GrpcHarmonicSession already
                // returns Err(AllRegionsFailed) when ok_mask == 0, but other
                // HarmonicSession impls (stubs, mocks) may not.
                if result.ok_mask == 0 {
                    self.emit_result(
                        &envelope,
                        result.region_mask,
                        0,
                        result.err_code,
                        result.bundle_uuid,
                        0,
                        result_ring,
                    );
                    self.health_tracker.record_failure();
                    self.death_clamped += 1;
                    return self.emit_nack(&envelope, nack_ring);
                }
                self.bundles_sent += 1;
                self.health_tracker.record_success();
                self.emit_result(
                    &envelope,
                    result.region_mask,
                    result.ok_mask,
                    result.err_code,
                    result.bundle_uuid,
                    0,
                    result_ring,
                );
                SendAttemptResult::Sent(RegionalResultSummary {
                    region_mask: result.region_mask,
                    ok_mask: result.ok_mask,
                    err_code: result.err_code,
                })
            }
            // Exhaustive Death Clamp: every SendError variant → drop + NACK.
            // No reroute to TPU, QUIC, or public lanes. Ever.
            Err(SendError::NotConnected) => {
                self.death_clamp(&envelope, DC_ERR_AUTH, nack_ring, result_ring)
            }
            Err(SendError::Timeout) => {
                self.death_clamp(&envelope, DC_ERR_TIMEOUT, nack_ring, result_ring)
            }
            Err(SendError::Transport(_)) => {
                self.death_clamp(&envelope, DC_ERR_TRANSPORT, nack_ring, result_ring)
            }
            Err(SendError::Rejected(msg)) => self.death_clamp_with_hint(
                &envelope,
                DC_ERR_REJECTED,
                rejection_hint_for_msg(&msg),
                nack_ring,
                result_ring,
            ),
            Err(SendError::AllRegionsFailed {
                region_mask,
                err_code,
            }) => self.death_clamp_with_region_detail(
                &envelope,
                DC_ERR_ALL_REGIONS,
                region_mask,
                err_code,
                nack_ring,
                result_ring,
            ),
        }
    }

    /// Drive session reconnect / keepalive.
    pub fn poll_session(&mut self) {
        self.session.poll();
    }

    /// Update health publication based on aggregate ring occupancy.
    pub fn publish_health(
        &mut self,
        health_segment: &MappedSegment<EgressHealthSnapshot>,
        aggregate_ring_len: u64,
        aggregate_ring_capacity: u32,
        now_unix_ns: u64,
    ) {
        self.health_tracker
            .update_queue_age(aggregate_ring_len, aggregate_ring_capacity);

        publish_harmonic_health(
            health_segment,
            &mut self.health_tracker,
            now_unix_ns,
            self.config.degraded_hysteresis_batches,
        );
    }

    /// One round-robin pass over all worker egress rings.
    ///
    /// Returns the number of envelopes processed (sent + NACKed).
    pub fn drain_once(&mut self, resources: &ProxyResources) -> u64 {
        self.poll_session();

        let n = resources.egress_rings.len();
        let start = self.next_ring_idx;
        self.next_ring_idx = if n == 0 { 0 } else { (start + 1) % n };

        let mut processed: u64 = 0;
        for i in 0..n {
            let idx = (start + i) % n;
            match self.process_one(
                &resources.egress_rings[idx],
                &resources.nack_ring,
                &resources.result_ring,
            ) {
                SendAttemptResult::RingEmpty => {}
                _ => processed += 1,
            }
        }

        if processed == 0 {
            if self.session.can_send() {
                self.health_tracker.record_success();
            } else {
                self.health_tracker.record_failure();
            }
        }

        // Aggregate ring occupancy across all workers for health publication.
        let aggregate_len: u64 = resources.egress_rings.iter().map(|r| r.len()).sum();
        let aggregate_cap: u32 = resources.egress_rings.iter().map(|r| r.capacity()).sum();

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        self.publish_health(
            &resources.health_segment,
            aggregate_len,
            aggregate_cap,
            now_ns,
        );

        processed
    }

    /// Single-threaded blocking run loop.
    ///
    /// One proxy thread, one session, one pass per iteration over all
    /// per-worker Harmonic egress rings.  Yield on zero processed to
    /// avoid busy-spin when all rings are empty.
    pub fn run(&mut self, resources: &ProxyResources) -> ! {
        loop {
            let processed = self.drain_once(resources);
            if processed == 0 {
                std::thread::yield_now();
            }
        }
    }

    /// Current session state (for diagnostics).
    pub fn session_state(&self) -> SessionState {
        self.session.state()
    }

    /// Current health state (for diagnostics).
    pub fn health_state(&self) -> shadow_ipc::egress_health::LaneState {
        self.health_tracker.state()
    }

    /// Borrow the session (test-only).
    #[cfg(test)]
    pub fn session_ref(&self) -> &S {
        &self.session
    }

    // -- internal ---------------------------------------------------------

    /// Death Clamp: record failure, publish result, emit NACK, increment counter.
    ///
    /// Single call site for every proxy-side drop.  The envelope is consumed
    /// (dropped) and never forwarded to any other lane.
    fn death_clamp(
        &mut self,
        envelope: &SignedTxEnvelope,
        err_code: u8,
        nack_ring: &MappedSpscRing<NackRecord128B>,
        result_ring: &MappedSpscRing<EgressResult>,
    ) -> SendAttemptResult {
        self.health_tracker.record_failure();
        self.death_clamped += 1;
        self.emit_result(envelope, 0, 0, err_code, [0; 16], 0, result_ring);
        self.emit_nack(envelope, nack_ring)
    }

    /// Death Clamp with a rejection-class hint for PM classification.
    fn death_clamp_with_hint(
        &mut self,
        envelope: &SignedTxEnvelope,
        err_code: u8,
        rejection_hint: u8,
        nack_ring: &MappedSpscRing<NackRecord128B>,
        result_ring: &MappedSpscRing<EgressResult>,
    ) -> SendAttemptResult {
        self.health_tracker.record_failure();
        self.death_clamped += 1;
        self.emit_result(
            envelope,
            0,
            0,
            err_code,
            [0; 16],
            rejection_hint,
            result_ring,
        );
        self.emit_nack(envelope, nack_ring)
    }

    /// Death Clamp variant that preserves attempted-region detail.
    ///
    /// Used on the `AllRegionsFailed` path where the session already computed
    /// `region_mask` and `err_code` from the fanout attempt.  PM (G8) uses
    /// these to classify which regions were unreachable.
    fn death_clamp_with_region_detail(
        &mut self,
        envelope: &SignedTxEnvelope,
        dc_err: u8,
        region_mask: u8,
        region_err_code: u8,
        nack_ring: &MappedSpscRing<NackRecord128B>,
        result_ring: &MappedSpscRing<EgressResult>,
    ) -> SendAttemptResult {
        self.health_tracker.record_failure();
        self.death_clamped += 1;
        // Use the session's err_code if non-zero, otherwise fall back to dc_err.
        let published_err = if region_err_code != 0 {
            region_err_code
        } else {
            dc_err
        };
        self.emit_result(
            envelope,
            region_mask,
            0,
            published_err,
            [0; 16],
            0,
            result_ring,
        );
        self.emit_nack(envelope, nack_ring)
    }

    fn emit_nack(
        &mut self,
        envelope: &SignedTxEnvelope,
        nack_ring: &MappedSpscRing<NackRecord128B>,
    ) -> SendAttemptResult {
        let nack = NackRecord128B::new(
            envelope.signature,
            envelope.mint_out_index,
            envelope.reserved_notional_lamports,
        );
        if nack_ring.push(&nack) {
            self.nacks_emitted += 1;
            SendAttemptResult::NackEmitted
        } else {
            SendAttemptResult::NackRingFull
        }
    }

    /// Publish a fixed-width EgressResult to the shared result ring.
    ///
    /// Called on every outcome (success, partial failure, total failure).
    /// PM (G8) consumes these for capital release and quarantine classification.
    fn emit_result(
        &mut self,
        envelope: &SignedTxEnvelope,
        region_mask: u8,
        ok_mask: u8,
        err_code: u8,
        bundle_uuid: [u8; 16],
        rejection_hint: u8,
        result_ring: &MappedSpscRing<EgressResult>,
    ) {
        let result = EgressResult {
            signature: envelope.signature,
            lane: LANE_HARMONIC,
            region_mask,
            ok_mask,
            err_code,
            pool_index: envelope.pool_index,
            mint_out_index: envelope.mint_out_index,
            reserved_notional_lamports: envelope.reserved_notional_lamports,
            send_timestamp_ns: envelope.send_timestamp_ns,
            tactic_class: envelope.tactic_class,
            worker_id: envelope.worker_id,
            bundle_uuid,
            rejection_hint,
            venue_id: envelope.venue_id,
            opso_owner_mode: envelope.opso_owner_mode,
            opso_fallback_mode: envelope.opso_fallback_mode,
            opso_reason_code: envelope.opso_reason_code,
            episode_id_mod: envelope.episode_id_mod,
            actual_bid_lamports: envelope.actual_bid_lamports,
            baseline_bid_lamports: envelope.baseline_bid_lamports,
            opso_bid_lamports: envelope.opso_bid_lamports,
            fallback_realized_value_lamports: envelope.fallback_realized_value_lamports,
            missed_backrun_value_lamports: envelope.missed_backrun_value_lamports,
            tip_saved_lamports: envelope.tip_saved_lamports,
            opso_ev_lamports: envelope.opso_ev_lamports,
            shading_gain_lamports: envelope.shading_gain_lamports,
            egress_transport: 0,
            _reserved: [0u8; 3],
        };
        if result_ring.push(&result) {
            self.results_published += 1;
        }
        // If result ring is full, silently drop — PM uses timeout-based
        // fallback recovery for stranded capital. The proxy must not block.
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn rejection_hint_shadow_guard_keyword() {
        assert_eq!(rejection_hint_for_msg("shadow_guard boundary breach"), 1);
        assert_eq!(rejection_hint_for_msg("SHADOW_GUARD violation"), 1);
        assert_eq!(rejection_hint_for_msg("assert_reserves check failed"), 1);
        assert_eq!(rejection_hint_for_msg("boundary exceeded"), 1);
    }

    #[test]
    fn rejection_hint_generic_rejection() {
        assert_eq!(rejection_hint_for_msg("simulation failed"), 2);
        assert_eq!(rejection_hint_for_msg("slippage exceeded"), 2);
        assert_eq!(rejection_hint_for_msg(""), 2);
    }
}

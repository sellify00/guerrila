// G6 – Jito proxy core
//
// The proxy loop does exactly one thing per iteration:
//   1. poll session (drive reconnect / keepalive)
//   2. round-robin over all worker rings, pop one envelope per ring
//   3. if session usable (Up | Degraded), send raw bytes
//   4. if session not usable (Down | Connecting) or send fails, emit NACK
//   5. update health tracker
//
// Lane-local byte-blast only:
//   - The proxy never reads worker-only state.
//   - The proxy never touches routing decisions.
//   - The proxy never inspects economics, fees, or template contents.
//   - The worker has already decided route, fee, and template.
//   - The proxy only blasts raw bytes and NACKs failures.
//
// Post-sign immutability:
//   - Signed bytes are sent exactly as dequeued: envelope.raw_bytes[..envelope.len].
//   - The proxy must NOT patch the tip account.
//   - The proxy must NOT patch compute budget.
//   - The proxy must NOT patch the blockhash.
//   - The proxy must NOT rebuild the transaction from fields.
//   - Jito tip account rotation happens before hashing and signing (worker side).
//   - The egress proxy must never mutate anything post-sign.

use crate::config::JitoEgressConfig;
use crate::diag;
use crate::health::{publish_jito_health, JitoHealthTracker};
use crate::session::{BundleRequest, SearcherSession, SendError, SessionState};
use shadow_ipc::egress_health::EgressHealthSnapshot;
use shadow_ipc::egress_ring::{
    open_jito_egress_ring_rw, open_jito_result_ring_rw, EgressResult, SignedTxEnvelope, LANE_JITO,
    PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC,
};
use shadow_ipc::nack::{open_shadow_nack_ring_rw, NackRecord128B};
use shadow_ipc::ring::MappedSpscRing;
use shadow_ipc::segment::MappedSegment;

/// Parse a UUID string (with or without hyphens) into 16 raw bytes.
/// Returns all zeros on malformed input — PM treats all-zero UUID as
/// "unavailable" and falls back to sig-status polling.
fn parse_uuid_to_bytes(s: &str) -> [u8; 16] {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return [0; 16];
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = match u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) {
            Ok(b) => b,
            Err(_) => return [0; 16],
        };
    }
    out
}

/// Result of a single dequeue-send attempt for one envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendAttemptResult {
    /// Bundle sent to Jito; no NACK emitted.
    Sent,
    /// Local failure before any network transmission; one NACK emitted.
    NackEmitted,
    /// NACK should have been emitted but the NACK ring was full.
    NackRingFull,
    /// Egress ring was empty; nothing to do.
    RingEmpty,
}

#[derive(Debug, thiserror::Error)]
pub enum JitoProxyError {
    #[error("failed to open egress ring for worker {worker_id}: {source}")]
    RingOpen {
        worker_id: u32,
        source: shadow_ipc::ring::RingError,
    },
    #[error("failed to open NACK ring: {0}")]
    NackRingOpen(shadow_ipc::ring::RingError),
    #[error("failed to open Jito result ring: {0}")]
    ResultRingOpen(shadow_ipc::ring::RingError),
    #[error("failed to open egress health segment: {0}")]
    HealthOpen(shadow_ipc::segment::SegmentError),
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
    /// Per-worker Jito egress rings (consumer side).
    /// Index `i` corresponds to `worker_id == i`.
    pub egress_rings: Vec<MappedSpscRing<SignedTxEnvelope>>,
    /// NACK ring (producer side) — the single release path for proxy-side
    /// local failures.  No second drop queue exists or should be invented.
    pub nack_ring: MappedSpscRing<NackRecord128B>,
    /// Result ring (producer side) — per-envelope send outcome for PM (G8).
    /// PM uses this to insert Jito sends into the pending table.
    pub result_ring: MappedSpscRing<EgressResult>,
    /// Egress health segment (writer side) at `/dev/shm/shadow_egress_health`.
    pub health_segment: MappedSegment<EgressHealthSnapshot>,
}

impl ProxyResources {
    /// Open all shared-memory resources for the proxy.
    ///
    /// Opens exactly:
    ///   - one `open_jito_egress_ring_rw(w)` for each `w` in `0..money_workers`
    ///   - one `open_shadow_nack_ring_rw()`
    ///   - one `open_shadow_egress_health_rw()`
    ///
    /// Returns `Err(JitoProxyError)` on the first failure; the proxy must
    /// treat this as fatal (fail closed).
    pub fn open(config: &JitoEgressConfig) -> Result<Self, JitoProxyError> {
        let mut egress_rings = Vec::with_capacity(config.money_workers as usize);
        for w in 0..config.money_workers {
            let ring = open_jito_egress_ring_rw(w).map_err(|e| JitoProxyError::RingOpen {
                worker_id: w,
                source: e,
            })?;
            egress_rings.push(ring);
        }

        let nack_ring = open_shadow_nack_ring_rw().map_err(JitoProxyError::NackRingOpen)?;

        let result_ring = open_jito_result_ring_rw().map_err(JitoProxyError::ResultRingOpen)?;

        let health_segment = shadow_ipc::egress_health::open_shadow_egress_health_rw()
            .map_err(|e| JitoProxyError::HealthOpen(shadow_ipc::segment::SegmentError::Io(e)))?;

        Ok(ProxyResources {
            egress_rings,
            nack_ring,
            result_ring,
            health_segment,
        })
    }
}

/// The Jito egress proxy.
///
/// Lane-local byte-blast only.  The proxy never inspects worker-side state
/// (routing decisions, economics, fee selection, template contents).  The
/// worker has already decided everything; the proxy only pops envelopes,
/// sends raw bytes, and NACKs on local failure.
///
/// Owns per-worker egress rings (consumer side), the NACK ring (producer side),
/// the egress health segment (writer side), and a Searcher session.
pub struct JitoProxy<S: SearcherSession> {
    pub config: JitoEgressConfig,
    session: S,
    health_tracker: JitoHealthTracker,
    /// Running count of bundles sent since proxy start.
    pub bundles_sent: u64,
    /// Running count of NACKs emitted since proxy start.
    pub nacks_emitted: u64,
    /// Running count of EgressResult records published since proxy start.
    pub results_published: u64,
    /// Next ring to service in round-robin order.
    /// Ensures fair drain across workers when some rings are hotter than others.
    next_ring_idx: usize,
}

impl<S: SearcherSession> JitoProxy<S> {
    /// Create a new proxy with the given config and session implementation.
    pub fn new(config: JitoEgressConfig, session: S) -> Self {
        let health_tracker = JitoHealthTracker::new(
            config.degrade_failure_threshold,
            config.down_failure_threshold,
            config.hysteresis_recover_batches,
        );
        JitoProxy {
            config,
            session,
            health_tracker,
            bundles_sent: 0,
            nacks_emitted: 0,
            results_published: 0,
            next_ring_idx: 0,
        }
    }

    /// Process one envelope from a worker ring.
    ///
    /// Pops at most one `SignedTxEnvelope`, sends `raw_bytes[..len]` verbatim
    /// via the Searcher session, and emits a NACK on any local failure.
    ///
    /// Post-sign immutability: the proxy never mutates the dequeued bytes.
    /// No tip-account patching, no compute-budget patching, no blockhash
    /// patching, no transaction rebuilding.  The worker signed it; the
    /// proxy blasts it.
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

        // Frame as a single-tx bundle and attempt send.
        // G6 framing: one envelope → one bundle → one transaction.
        // No batching, no coalescing, no multi-envelope packing.
        let request = BundleRequest::single_tx_with_transport(
            &envelope.raw_bytes[..envelope.len],
            envelope.egress_transport,
        );
        // No authenticated session for the selected transport → fast-NACK,
        // never block on handshake. Route-aware muxes must fail closed when
        // this envelope's private lane is down even if another lane is healthy.
        if !self.session.can_send_request(&request) {
            self.health_tracker.record_failure();
            return self.emit_nack(&envelope, nack_ring);
        }
        if envelope.egress_transport == PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC {
            diag::record_public_rpc_transport_attempt(&envelope);
        }
        match self.session.send_bundle(&request) {
            Ok(uuid_string) => {
                self.bundles_sent += 1;
                self.health_tracker.record_success();
                let bundle_uuid = parse_uuid_to_bytes(&uuid_string);
                self.emit_result(&envelope, result_ring, 0, bundle_uuid);
                SendAttemptResult::Sent
            }
            Err(err) => {
                self.health_tracker.record_failure();
                if envelope.egress_transport == PRIVATE_EGRESS_TRANSPORT_PUBLIC_RPC {
                    match &err {
                        SendError::Timeout | SendError::Transport(_) => {
                            diag::record_public_rpc_transport_failure();
                        }
                        SendError::Rejected(message) => {
                            diag::record_public_rpc_transport_rejected(&envelope, message);
                        }
                        SendError::NotConnected => {}
                    }
                }
                self.emit_nack(&envelope, nack_ring)
            }
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

        publish_jito_health(
            health_segment,
            &mut self.health_tracker,
            now_unix_ns,
            self.config.degraded_hysteresis_batches,
        );
    }

    /// One round-robin pass over all worker egress rings: pop one
    /// `SignedTxEnvelope` per ring, send or NACK each, publish health.
    ///
    /// Starts from `next_ring_idx` (advanced on each call) so that no
    /// single worker ring is systematically favoured when some rings
    /// are hotter than others.
    ///
    /// Returns the number of envelopes processed (sent + NACKed).
    /// Callers can use a zero return to decide whether to yield.
    ///
    /// Lane-local contract: the proxy never inspects envelope contents
    /// beyond `raw_bytes[..len]`.  No routing, no economics, no worker
    /// state — the worker decided everything before pushing.
    pub fn drain_once(&mut self, resources: &ProxyResources) -> u64 {
        self.poll_session();

        let n = resources.egress_rings.len();
        let start = self.next_ring_idx;
        // Advance for next call — wraps naturally.
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
    /// per-worker Jito egress rings.  This is the Guerrila baseline —
    /// do not parallelise unless one proxy thread measurably becomes the
    /// bottleneck.
    pub fn run(&mut self, resources: &ProxyResources) -> ! {
        loop {
            let processed = self.drain_once(resources);
            if processed == 0 {
                std::thread::yield_now();
            }
        }
    }

    /// Current session state (for diagnostics / main loop decisions).
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
            if self.config.public_rpc.enabled {
                diag::record_public_rpc_nack_recorded();
            }
            SendAttemptResult::NackEmitted
        } else {
            // NACK ring full — capital may be stranded until PM's expiry
            // timeout recovers it.  This is a serious operational issue
            // but the proxy must not block.
            SendAttemptResult::NackRingFull
        }
    }

    /// Publish an EgressResult to the Jito result ring for PM consumption.
    ///
    /// `err_code` is 0 on success, non-zero on failure.  Jito always has
    /// region_mask = 0x01, ok_mask = 0x01 on success (single endpoint).
    fn emit_result(
        &mut self,
        envelope: &SignedTxEnvelope,
        result_ring: &MappedSpscRing<EgressResult>,
        err_code: u8,
        bundle_uuid: [u8; 16],
    ) {
        let result = EgressResult {
            signature: envelope.signature,
            lane: LANE_JITO,
            region_mask: 0x01,
            ok_mask: if err_code == 0 { 0x01 } else { 0x00 },
            err_code,
            pool_index: envelope.pool_index,
            mint_out_index: envelope.mint_out_index,
            reserved_notional_lamports: envelope.reserved_notional_lamports,
            send_timestamp_ns: envelope.send_timestamp_ns,
            tactic_class: envelope.tactic_class,
            worker_id: envelope.worker_id,
            bundle_uuid,
            rejection_hint: 0,
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
            egress_transport: envelope.egress_transport,
            _reserved: [0; 3],
        };
        if result_ring.push(&result) {
            self.results_published += 1;
        }
        // Result ring full: PM will still see this tx via signature-status
        // polling.  The result ring is an optimization for fast pending-table
        // insert, not the only path.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uuid_standard_format() {
        let bytes = parse_uuid_to_bytes("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(
            bytes,
            [
                0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00
            ]
        );
    }

    #[test]
    fn parse_uuid_no_hyphens() {
        let bytes = parse_uuid_to_bytes("550e8400e29b41d4a716446655440000");
        assert_eq!(
            bytes,
            [
                0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00
            ]
        );
    }

    #[test]
    fn parse_uuid_malformed_returns_zeros() {
        assert_eq!(parse_uuid_to_bytes("too-short"), [0; 16]);
        assert_eq!(parse_uuid_to_bytes(""), [0; 16]);
    }
}

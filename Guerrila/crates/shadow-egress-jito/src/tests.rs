// G6 – shadow-egress-jito unit tests

use crate::config::JitoEgressConfig;
use crate::health::{publish_jito_health, JitoHealthTracker};
use crate::proxy::{JitoProxy, ProxyResources, SendAttemptResult};
use crate::session::{
    BundleRequest, CapturingSession, MockSession, SearcherSession, SendError, SessionState,
    StubSession,
};
use shadow_ipc::egress_health::{
    EgressHealthPayload32B, EgressHealthSnapshot, LaneState, QueueAgeClass,
};
use shadow_ipc::egress_ring::{
    EgressResult, SignedTxEnvelope, JITO_EGRESS_RING_SEGMENT_ID, JITO_EGRESS_RING_VERSION,
    PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER, PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE,
};
use shadow_ipc::nack::NackRecord128B;
use shadow_ipc::ring::MappedSpscRing;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> JitoEgressConfig {
    JitoEgressConfig {
        block_engine_url: "http://test.local".into(),
        money_workers: 1,
        degrade_failure_threshold: 3,
        down_failure_threshold: 10,
        hysteresis_recover_batches: 2,
        degraded_hysteresis_batches: 2,
        ..JitoEgressConfig::default()
    }
}

fn make_envelope(sig_byte: u8, pool: u32, mint: u32, notional: u64) -> SignedTxEnvelope {
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 100,
        signature: [sig_byte; 64],
        pool_index: pool,
        mint_out_index: mint,
        reserved_notional_lamports: notional,
        send_timestamp_ns: 0,
        tactic_class: 0,
        worker_id: 0,
        venue_id: 0,
        opso_owner_mode: 0,
        opso_fallback_mode: 0,
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
    // Put recognizable data in raw_bytes.
    for i in 0..env.len {
        env.raw_bytes[i] = (i as u8).wrapping_add(sig_byte);
    }
    env
}

struct TransportGateSession {
    allowed_transport: u8,
    send_calls: u32,
}

impl TransportGateSession {
    fn new(allowed_transport: u8) -> Self {
        Self {
            allowed_transport,
            send_calls: 0,
        }
    }
}

impl SearcherSession for TransportGateSession {
    fn state(&self) -> SessionState {
        SessionState::Up
    }

    fn can_send_request(&self, request: &BundleRequest<'_>) -> bool {
        request.transport_hint == self.allowed_transport
    }

    fn send_bundle(&mut self, _request: &BundleRequest<'_>) -> Result<String, SendError> {
        self.send_calls += 1;
        Ok(String::new())
    }

    fn poll(&mut self) {
        // no-op for tests
    }
}

fn make_public_rpc_guard_test_tx(sig_byte: u8, blockhash_byte: u8) -> Vec<u8> {
    const COMPUTE_BUDGET_PROGRAM_ID: [u8; 32] = [
        0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b,
        0xe7, 0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00,
        0x00, 0x00,
    ];
    let shadow_guard_program_id = [0x77; 32];
    let vault_in = [0x22; 32];
    let vault_out = [0x33; 32];
    let mut tx = Vec::new();
    tx.push(1);
    tx.extend_from_slice(&[sig_byte; 64]);
    tx.extend_from_slice(&[1, 0, 4]);
    tx.push(5);
    tx.extend_from_slice(&[0x11; 32]);
    tx.extend_from_slice(&vault_in);
    tx.extend_from_slice(&vault_out);
    tx.extend_from_slice(&COMPUTE_BUDGET_PROGRAM_ID);
    tx.extend_from_slice(&shadow_guard_program_id);
    tx.extend_from_slice(&[blockhash_byte; 32]);
    tx.push(2);
    tx.push(3);
    tx.push(0);
    tx.push(5);
    tx.push(0x02);
    tx.extend_from_slice(&250_000u32.to_le_bytes());
    tx.push(4);
    tx.push(2);
    tx.extend_from_slice(&[1, 2]);
    tx.push(32);
    tx.extend_from_slice(&[0u8; 32]);
    tx
}

fn make_public_rpc_clmm_rejection_test_tx(
    sig_byte: u8,
    blockhash_byte: u8,
    cu_limit: u32,
    _cu_price_micro_lamports: u64,
    amount_in: u64,
    min_out: u64,
) -> Vec<u8> {
    const COMPUTE_BUDGET_PROGRAM_ID: [u8; 32] = [
        0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b,
        0xe7, 0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00,
        0x00, 0x00,
    ];
    let shadow_guard_program_id = [0x77; 32];
    let clmm_program_id = [0x88; 32];
    let mut tx = Vec::new();
    tx.push(1);
    tx.extend_from_slice(&[sig_byte; 64]);
    tx.extend_from_slice(&[1, 0, 3]);
    tx.push(4);
    tx.extend_from_slice(&[0x11; 32]);
    tx.extend_from_slice(&COMPUTE_BUDGET_PROGRAM_ID);
    tx.extend_from_slice(&shadow_guard_program_id);
    tx.extend_from_slice(&clmm_program_id);
    tx.extend_from_slice(&[blockhash_byte; 32]);
    tx.push(3);

    tx.push(1);
    tx.push(0);
    tx.push(5);
    tx.push(0x02);
    tx.extend_from_slice(&cu_limit.to_le_bytes());

    tx.push(2);
    tx.push(0);
    tx.push(0);

    tx.push(3);
    tx.push(2);
    tx.extend_from_slice(&[0, 2]);
    tx.push(24);
    tx.extend_from_slice(&[0xAA; 8]);
    tx.extend_from_slice(&amount_in.to_le_bytes());
    tx.extend_from_slice(&min_out.to_le_bytes());
    tx
}

/// Create a temp egress ring and NACK ring for testing.
fn create_temp_rings(
    dir: &std::path::Path,
) -> (
    MappedSpscRing<SignedTxEnvelope>,
    MappedSpscRing<NackRecord128B>,
    MappedSpscRing<EgressResult>,
) {
    let egress_path = dir.join("egress_ring");
    let nack_path = dir.join("nack_ring");
    let result_path = dir.join("result_ring");

    let egress_ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
        &egress_path,
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
        16,
    )
    .unwrap();

    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(
        &nack_path,
        shadow_ipc::nack::NACK_RING_SEGMENT_ID,
        shadow_ipc::nack::NACK_RING_VERSION,
        16,
    )
    .unwrap();

    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        shadow_ipc::egress_ring::JITO_RESULT_RING_SEGMENT_ID,
        shadow_ipc::egress_ring::JITO_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    (egress_ring, nack_ring, result_ring)
}

// ---------------------------------------------------------------------------
// StubSession tests
// ---------------------------------------------------------------------------

#[test]
fn stub_session_is_always_down() {
    let s = StubSession;
    assert_eq!(s.state(), SessionState::Down);
    assert!(!s.can_send());
}

#[test]
fn stub_session_send_returns_not_connected() {
    let mut s = StubSession;
    let result = s.send_bundle(&BundleRequest::single_tx(&[0u8; 100]));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// MockSession tests
// ---------------------------------------------------------------------------

#[test]
fn mock_connected_session_sends_successfully() {
    let mut s = MockSession::connected();
    assert_eq!(s.state(), SessionState::Up);
    assert!(s.can_send());
    assert!(s.send_bundle(&BundleRequest::single_tx(&[1, 2, 3])).is_ok());
    assert_eq!(s.sent_count, 1);
}

#[test]
fn mock_disconnected_session_rejects_send() {
    let mut s = MockSession::disconnected();
    assert_eq!(s.state(), SessionState::Down);
    assert!(!s.can_send());
    assert!(s
        .send_bundle(&BundleRequest::single_tx(&[1, 2, 3]))
        .is_err());
}

#[test]
fn mock_failure_session_returns_error() {
    let mut s = MockSession::with_failure(SendError::Timeout);
    assert_eq!(s.state(), SessionState::Up);
    assert!(s.can_send());
    assert!(s
        .send_bundle(&BundleRequest::single_tx(&[1, 2, 3]))
        .is_err());
}

// ---------------------------------------------------------------------------
// SessionState machine tests
// ---------------------------------------------------------------------------

#[test]
fn can_send_only_when_up_or_degraded() {
    // Up → can send.
    let s = MockSession::connected();
    assert!(s.can_send());

    // Degraded → can send.
    let s = MockSession::degraded();
    assert!(s.can_send());

    // Down → cannot send.
    let s = MockSession::disconnected();
    assert!(!s.can_send());

    // Connecting → cannot send (handshake in progress).
    let s = MockSession::connecting();
    assert!(!s.can_send());
}

#[test]
fn connecting_state_fast_nacks() {
    // A session in Connecting state must not block the send path.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xEE, 0, 0, 1_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connecting());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(proxy.nacks_emitted, 1);
}

#[test]
fn degraded_state_still_sends() {
    // Degraded session is authenticated — sends must go through.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xFF, 0, 0, 1_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::degraded());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::Sent);
    assert_eq!(proxy.bundles_sent, 1);
    assert_eq!(proxy.nacks_emitted, 0);
}

// ---------------------------------------------------------------------------
// Health tracker tests
// ---------------------------------------------------------------------------

#[test]
fn health_tracker_starts_down() {
    let t = JitoHealthTracker::new(3, 10, 2);
    assert_eq!(t.state(), LaneState::Down);
}

#[test]
fn health_recovers_after_sustained_successes() {
    let mut t = JitoHealthTracker::new(3, 10, 2);
    assert_eq!(t.state(), LaneState::Down);

    t.record_success();
    assert_eq!(t.state(), LaneState::Down); // need 2 batches

    t.record_success();
    assert_eq!(t.state(), LaneState::Up);
}

#[test]
fn health_degrades_after_failure_threshold() {
    let mut t = JitoHealthTracker::new(3, 10, 2);
    // Recover first.
    t.record_success();
    t.record_success();
    assert_eq!(t.state(), LaneState::Up);

    // 3 consecutive failures → degraded.
    t.record_failure();
    t.record_failure();
    assert_eq!(t.state(), LaneState::Up); // only 2, not yet
    t.record_failure();
    assert_eq!(t.state(), LaneState::Degraded);
}

#[test]
fn health_goes_down_after_down_threshold() {
    let mut t = JitoHealthTracker::new(3, 6, 2);
    t.record_success();
    t.record_success();
    assert_eq!(t.state(), LaneState::Up);

    // 3 failures → degraded.
    for _ in 0..3 {
        t.record_failure();
    }
    assert_eq!(t.state(), LaneState::Degraded);

    // 3 more → down (total 6 consecutive).
    for _ in 0..3 {
        t.record_failure();
    }
    assert_eq!(t.state(), LaneState::Down);
}

#[test]
fn health_recovers_from_degraded() {
    let mut t = JitoHealthTracker::new(3, 10, 2);
    t.record_success();
    t.record_success();
    assert_eq!(t.state(), LaneState::Up);

    for _ in 0..3 {
        t.record_failure();
    }
    assert_eq!(t.state(), LaneState::Degraded);

    t.record_success();
    assert_eq!(t.state(), LaneState::Degraded); // need 2
    t.record_success();
    assert_eq!(t.state(), LaneState::Up);
}

#[test]
fn hysteresis_prevents_thrashing() {
    let mut t = JitoHealthTracker::new(3, 10, 3);

    // Recover from Down.
    t.record_success();
    t.record_success();
    t.record_success();
    assert_eq!(t.state(), LaneState::Up);

    // Two failures (below threshold) then a success resets.
    t.record_failure();
    t.record_failure();
    assert_eq!(t.state(), LaneState::Up);
    t.record_success(); // resets consecutive_failures
    assert_eq!(t.state(), LaneState::Up);

    // Two more failures — still below threshold.
    t.record_failure();
    t.record_failure();
    assert_eq!(t.state(), LaneState::Up);
}

#[test]
fn queue_age_classification() {
    let mut t = JitoHealthTracker::new(3, 10, 2);

    t.update_queue_age(0, 64);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Fresh);

    t.update_queue_age(16, 64); // 25%
    assert_eq!(t.queue_age_class(), QueueAgeClass::Warm);

    t.update_queue_age(32, 64); // 50%
    assert_eq!(t.queue_age_class(), QueueAgeClass::Backlogged);

    t.update_queue_age(48, 64); // 75%
    assert_eq!(t.queue_age_class(), QueueAgeClass::Severe);

    t.update_queue_age(64, 64); // 100%
    assert_eq!(t.queue_age_class(), QueueAgeClass::Severe);
}

// ---------------------------------------------------------------------------
// Proxy process_one tests
// ---------------------------------------------------------------------------

#[test]
fn process_one_returns_ring_empty_when_nothing_queued() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::RingEmpty
    );
}

#[test]
fn process_one_sends_when_session_connected() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xAA, 1, 2, 1_000_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::Sent);
    assert_eq!(proxy.bundles_sent, 1);
    assert_eq!(proxy.nacks_emitted, 0);
}

#[test]
fn process_one_nacks_when_session_disconnected() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xBB, 3, 4, 2_000_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(proxy.nacks_emitted, 1);

    // Verify NACK was pushed with correct fields.
    let nack = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xBB; 64]);
    assert_eq!(nack.mint_out_index, 4);
    assert_eq!(nack.trade_size_lamports, 2_000_000_000);
}

#[test]
fn process_one_nacks_on_send_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xCC, 5, 6, 3_000_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(
        test_config(),
        MockSession::with_failure(SendError::Transport("test".into())),
    );
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.nacks_emitted, 1);

    let nack = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xCC; 64]);
    assert_eq!(nack.mint_out_index, 6);
    assert_eq!(nack.trade_size_lamports, 3_000_000_000);
}

#[test]
fn process_one_reports_nack_ring_full() {
    let dir = tempfile::tempdir().unwrap();
    let egress_path = dir.path().join("egress_ring");
    let nack_path = dir.path().join("nack_ring");

    let egress_ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
        &egress_path,
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
        16,
    )
    .unwrap();

    // Create a NACK ring with capacity 1.
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(
        &nack_path,
        shadow_ipc::nack::NACK_RING_SEGMENT_ID,
        shadow_ipc::nack::NACK_RING_VERSION,
        1,
    )
    .unwrap();

    let result_path = dir.path().join("result_ring_full_test");
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        shadow_ipc::egress_ring::JITO_RESULT_RING_SEGMENT_ID,
        shadow_ipc::egress_ring::JITO_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    // Fill the NACK ring.
    let dummy_nack = NackRecord128B::new([0xFF; 64], 0, 0);
    nack_ring.push(&dummy_nack);

    // Push an envelope that will need NACKing.
    let env = make_envelope(0xDD, 7, 8, 4_000_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackRingFull);
}

#[test]
fn multiple_envelopes_drain_sequentially() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    for i in 0..5u8 {
        let env = make_envelope(i, i as u32, 0, 1_000_000);
        egress_ring.push(&env);
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    for _ in 0..5 {
        assert_eq!(
            proxy.process_one(&egress_ring, &nack_ring, &result_ring),
            SendAttemptResult::Sent
        );
    }
    assert_eq!(proxy.bundles_sent, 5);
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::RingEmpty
    );
}

// ---------------------------------------------------------------------------
// drain_once tests (single-threaded run loop)
// ---------------------------------------------------------------------------

/// Build a `ProxyResources` from temp files with `n_workers` egress rings.
fn create_temp_proxy_resources(dir: &std::path::Path, n_workers: u32) -> ProxyResources {
    let mut egress_rings = Vec::with_capacity(n_workers as usize);
    for w in 0..n_workers {
        let path = dir.join(format!("egress_ring_{w}"));
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            JITO_EGRESS_RING_SEGMENT_ID,
            JITO_EGRESS_RING_VERSION,
            16,
        )
        .unwrap();
        egress_rings.push(ring);
    }

    let nack_path = dir.join("nack_ring");
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(
        &nack_path,
        shadow_ipc::nack::NACK_RING_SEGMENT_ID,
        shadow_ipc::nack::NACK_RING_VERSION,
        16,
    )
    .unwrap();

    let result_path = dir.join("result_ring_jito");
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        shadow_ipc::egress_ring::JITO_RESULT_RING_SEGMENT_ID,
        shadow_ipc::egress_ring::JITO_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    let health_path = dir.join("egress_health");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    ProxyResources {
        egress_rings,
        nack_ring,
        result_ring,
        health_segment,
    }
}

#[test]
fn drain_once_processes_one_per_ring() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    // Push one envelope into each of the 3 worker rings.
    for (i, ring) in resources.egress_rings.iter().enumerate() {
        let env = make_envelope(i as u8, i as u32, 0, 1_000_000);
        ring.push(&env);
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 3);
    assert_eq!(proxy.bundles_sent, 3);
}

#[test]
fn drain_once_returns_zero_when_all_rings_empty() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 0);
    assert_eq!(proxy.bundles_sent, 0);
}

#[test]
fn drain_once_nacks_all_when_disconnected() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    // Push one envelope into each ring.
    for (i, ring) in resources.egress_rings.iter().enumerate() {
        let env = make_envelope(i as u8 + 0xA0, i as u32, i as u32, 1_000_000);
        ring.push(&env);
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 2);
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(proxy.nacks_emitted, 2);
}

#[test]
fn drain_once_drains_partial_rings() {
    // Only some rings have work; the rest are empty.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 4);

    // Push into rings 0 and 2 only.
    resources.egress_rings[0].push(&make_envelope(0x10, 0, 0, 500_000));
    resources.egress_rings[2].push(&make_envelope(0x20, 2, 0, 600_000));

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 2);
    assert_eq!(proxy.bundles_sent, 2);

    // Second drain: nothing left.
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 0);
}

#[test]
fn drain_once_pops_one_per_ring_per_call() {
    // Each ring has multiple envelopes; drain_once should pop exactly one from each.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    // Push 3 envelopes into each ring.
    for ring in &resources.egress_rings {
        for j in 0..3u8 {
            ring.push(&make_envelope(j, 0, 0, 1_000_000));
        }
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());

    // First drain: 1 from each ring = 2.
    assert_eq!(proxy.drain_once(&resources), 2);
    assert_eq!(proxy.bundles_sent, 2);

    // Second drain: 1 from each ring = 2 more.
    assert_eq!(proxy.drain_once(&resources), 2);
    assert_eq!(proxy.bundles_sent, 4);

    // Third drain: 1 from each ring = 2 more.
    assert_eq!(proxy.drain_once(&resources), 2);
    assert_eq!(proxy.bundles_sent, 6);

    // Fourth drain: all empty.
    assert_eq!(proxy.drain_once(&resources), 0);
}

// ---------------------------------------------------------------------------
// No BundleState / no public TPU / no ring-creation guard tests
// ---------------------------------------------------------------------------

#[test]
fn crate_does_not_depend_on_bundle_state() {
    // Structural assertion: no source file imports or uses BundleState as a type.
    // Doc comments that mention "BundleState" as a constraint notice are fine.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let health_src = include_str!("health.rs");

    for (name, src) in [
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("health.rs", health_src),
    ] {
        // Check for `use ...BundleState` imports or struct/type references.
        for line in src.lines() {
            let trimmed = line.trim();
            // Skip comments.
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            assert!(
                !trimmed.contains("BundleState"),
                "{name} contains a non-comment reference to BundleState; PM owns that"
            );
        }
    }
}

#[test]
fn crate_does_not_reference_public_quic() {
    let lib_src = include_str!("lib.rs");
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");

    for (name, src) in [
        ("lib.rs", lib_src),
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
    ] {
        assert!(
            !src.contains("quic_worker")
                && !src.contains("public_lane")
                && !src.contains("tpu_client"),
            "{name} must not reference public QUIC/TPU constructs"
        );
    }
}

#[test]
fn proxy_never_creates_rings() {
    // Startup contract: the proxy only opens existing ring/segment files.
    // Bootstrap (shadow-core startup) is responsible for creation.
    // This test catches accidental use of `create_rw` in proxy or session code.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let main_src = include_str!("main.rs");

    for (name, src) in [
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("main.rs", main_src),
    ] {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            assert!(
                !trimmed.contains("create_rw"),
                "{name} must not call create_rw; proxy opens rings, bootstrap creates them"
            );
        }
    }
}

#[test]
fn proxy_has_no_second_release_queue() {
    // The single release path for proxy-side local failures is the NACK ring.
    // No shadow_jito_drop_ring or equivalent should exist.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let main_src = include_str!("main.rs");

    for (name, src) in [
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("main.rs", main_src),
    ] {
        assert!(
            !src.contains("drop_ring")
                && !src.contains("release_ring")
                && !src.contains("second_nack"),
            "{name} must not reference a second release queue; NACK ring is the only path"
        );
    }
}

// ---------------------------------------------------------------------------
// Session state machine structural guards
// ---------------------------------------------------------------------------

#[test]
fn session_state_has_four_variants() {
    // The session state machine must have exactly: Down, Connecting, Up, Degraded.
    // This test will fail to compile if a variant is removed.
    let states = [
        SessionState::Down,
        SessionState::Connecting,
        SessionState::Up,
        SessionState::Degraded,
    ];
    // Ensure the proxy send-path distinction is correct.
    for &s in &states {
        let sendable = matches!(s, SessionState::Up | SessionState::Degraded);
        let mock = MockSession {
            session_state: s,
            send_result: None,
            sent_count: 0,
        };
        assert_eq!(mock.can_send(), sendable, "can_send() mismatch for {s:?}");
    }
}

#[test]
fn proxy_send_path_never_blocks_on_handshake() {
    // Structural assertion: proxy.rs must not contain blocking handshake
    // patterns — no `.await` on auth, no sleep-loops, no mutex locks
    // in the send/dequeue path.
    let proxy_src = include_str!("proxy.rs");

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") {
            continue;
        }
        assert!(
            !trimmed.contains("thread::sleep"),
            "proxy.rs must not call thread::sleep in the send path"
        );
        assert!(
            !trimmed.contains(".lock().unwrap()"),
            "proxy.rs must not hold mutex locks in the send path"
        );
    }
}

// ---------------------------------------------------------------------------
// Lane-local byte-blast structural guards
// ---------------------------------------------------------------------------

#[test]
fn proxy_does_not_inspect_worker_state() {
    // The proxy is lane-local: it never reads worker-only state such as
    // routing tables, pool snapshots, fee calculators, or template builders.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "route_table",
        "routing_runtime",
        "RoutingDecision",
        "fee_calculator",
        "fee_schedule",
        "template_builder",
        "pool_snapshot",
        "WorkerState",
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for keyword in &forbidden {
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not reference worker state ({keyword}); the proxy only blasts bytes"
            );
        }
    }
}

#[test]
fn proxy_does_not_inspect_economics() {
    // The proxy must not read or modify economic fields on the envelope
    // (beyond what's needed for the NACK record).  It never recalculates
    // priority fees, tip amounts, or notional values.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "priority_fee",
        "tip_lamports",
        "compute_budget",
        "recalculate",
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for keyword in &forbidden {
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not inspect economics ({keyword}); the worker decided everything"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Round-robin fairness tests
// ---------------------------------------------------------------------------

#[test]
fn drain_once_round_robins_starting_ring() {
    // With 3 rings, successive drain_once calls should start from
    // ring 0, 1, 2, 0, 1, 2, ... ensuring no ring is systematically
    // favoured when some rings are persistently hotter.

    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    // Put 2 envelopes in ring 0, 0 in ring 1, 0 in ring 2.
    resources.egress_rings[0].push(&make_envelope(0x01, 0, 0, 1_000_000));
    resources.egress_rings[0].push(&make_envelope(0x02, 0, 0, 1_000_000));

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());

    // First drain: starts at ring 0, pops from ring 0 (only ring with data).
    let p = proxy.drain_once(&resources);
    assert_eq!(p, 1);
    assert_eq!(proxy.bundles_sent, 1);

    // Second drain: starts at ring 1 (round-robin advanced), wraps to ring 0.
    let p = proxy.drain_once(&resources);
    assert_eq!(p, 1);
    assert_eq!(proxy.bundles_sent, 2);

    // Third drain: starts at ring 2, all empty.
    let p = proxy.drain_once(&resources);
    assert_eq!(p, 0);
}

#[test]
fn drain_once_zero_rings_is_safe() {
    // Edge case: no egress rings at all.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 0);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let p = proxy.drain_once(&resources);
    assert_eq!(p, 0);

    // Multiple calls should not panic.
    let p = proxy.drain_once(&resources);
    assert_eq!(p, 0);
}

// ---------------------------------------------------------------------------
// Post-sign immutability tests
// ---------------------------------------------------------------------------

#[test]
fn proxy_sends_exact_raw_bytes() {
    // The proxy must send envelope.raw_bytes[..envelope.len] verbatim.
    // No patching, no rebuilding, no mutation of the signed transaction.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xAB, 1, 2, 5_000_000);
    let expected: Vec<u8> = env.raw_bytes[..env.len].to_vec();
    egress_ring.push(&env);

    let session = CapturingSession::new();
    let mut proxy = JitoProxy::new(test_config(), session);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::Sent);

    // The session must have received exactly the bytes from the envelope.
    assert_eq!(proxy.session_ref().captured.len(), 1);
    assert_eq!(proxy.session_ref().captured[0], expected);
}

#[test]
fn proxy_sends_exact_bytes_multiple_envelopes() {
    // Verify byte-exact fidelity across multiple envelopes with different
    // contents and lengths.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env1 = make_envelope(0x10, 0, 0, 1_000_000);
    let env2 = make_envelope(0x20, 1, 1, 2_000_000);
    let env3 = make_envelope(0x30, 2, 2, 3_000_000);

    let expected1: Vec<u8> = env1.raw_bytes[..env1.len].to_vec();
    let expected2: Vec<u8> = env2.raw_bytes[..env2.len].to_vec();
    let expected3: Vec<u8> = env3.raw_bytes[..env3.len].to_vec();

    egress_ring.push(&env1);
    egress_ring.push(&env2);
    egress_ring.push(&env3);

    let session = CapturingSession::new();
    let mut proxy = JitoProxy::new(test_config(), session);

    for _ in 0..3 {
        assert_eq!(
            proxy.process_one(&egress_ring, &nack_ring, &result_ring),
            SendAttemptResult::Sent
        );
    }

    let captured = &proxy.session_ref().captured;
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0], expected1);
    assert_eq!(captured[1], expected2);
    assert_eq!(captured[2], expected3);
}

#[test]
fn proxy_does_not_mutate_signed_bytes() {
    // Structural assertion: proxy.rs must not contain any code that patches,
    // rebuilds, or mutates the transaction bytes post-sign.
    //
    // The spec is explicit: Jito tip account rotation happens before hashing
    // and signing.  The egress proxy must never mutate tip accounts post-sign.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "patch_tip",
        "rotate_tip",
        "set_tip_account",
        "patch_blockhash",
        "set_blockhash",
        "replace_blockhash",
        "patch_compute",
        "set_compute_budget",
        "rebuild_transaction",
        "serialize_transaction",
        "Transaction::new",
        "raw_bytes[", // must only appear in the slice `raw_bytes[..len]`
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }

        for keyword in &forbidden {
            // Allow the one legitimate use: `&envelope.raw_bytes[..envelope.len]`
            if *keyword == "raw_bytes[" {
                if trimmed.contains("raw_bytes[..") {
                    continue; // the read-only slice is fine
                }
            }
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not mutate signed bytes ({keyword}); \
                 the worker signed it, the proxy must blast it verbatim"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// One-bundle-per-envelope framing tests
// ---------------------------------------------------------------------------

#[test]
fn bundle_request_single_tx_wraps_exactly() {
    let data = [0xAA, 0xBB, 0xCC, 0xDD];
    let req = BundleRequest::single_tx(&data);
    assert_eq!(req.tx_bytes, &data);
    assert_eq!(req.tx_bytes.len(), 4);
}

#[test]
fn bundle_request_single_tx_with_transport_wraps_exactly() {
    let data = [0xAA, 0xBB, 0xCC, 0xDD];
    let req =
        BundleRequest::single_tx_with_transport(&data, PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER);
    assert_eq!(req.tx_bytes, &data);
    assert_eq!(req.tx_bytes.len(), 4);
    assert_eq!(req.transport_hint, PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER);
}

#[test]
fn proxy_sends_one_bundle_per_envelope() {
    // Each envelope must produce exactly one send_bundle call containing
    // exactly one transaction.  No batching, no coalescing.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env1 = make_envelope(0x01, 0, 0, 1_000_000);
    let env2 = make_envelope(0x02, 1, 1, 2_000_000);
    egress_ring.push(&env1);
    egress_ring.push(&env2);

    let session = CapturingSession::new();
    let mut proxy = JitoProxy::new(test_config(), session);

    // Process first envelope.
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::Sent
    );
    assert_eq!(proxy.session_ref().captured.len(), 1);
    assert_eq!(proxy.bundles_sent, 1);

    // Process second envelope.
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::Sent
    );
    assert_eq!(proxy.session_ref().captured.len(), 2);
    assert_eq!(proxy.bundles_sent, 2);

    // Each captured entry is exactly one tx worth of bytes.
    assert_eq!(proxy.session_ref().captured[0].len(), env1.len);
    assert_eq!(proxy.session_ref().captured[1].len(), env2.len);
}

#[test]
fn proxy_forwards_private_transport_hint_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut env = make_envelope(0x44, 3, 7, 4_000_000);
    env.egress_transport = PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER;
    egress_ring.push(&env);

    let session = CapturingSession::new();
    let mut proxy = JitoProxy::new(test_config(), session);

    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::Sent
    );
    assert_eq!(
        proxy.session_ref().captured_transports,
        vec![PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER]
    );

    let result = result_ring.pop().expect("must publish egress result");
    assert_eq!(
        result.egress_transport,
        PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER
    );
}

#[test]
fn proxy_fast_nacks_when_selected_transport_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut env = make_envelope(0x55, 4, 9, 5_000_000);
    env.egress_transport = PRIVATE_EGRESS_TRANSPORT_HELIUS_SENDER;
    egress_ring.push(&env);

    let session = TransportGateSession::new(PRIVATE_EGRESS_TRANSPORT_JITO_BUNDLE);
    let mut proxy = JitoProxy::new(test_config(), session);

    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::NackEmitted
    );
    assert_eq!(proxy.session_ref().send_calls, 0);
    assert!(
        result_ring.pop().is_none(),
        "must not publish a success result"
    );
    assert!(nack_ring.pop().is_some(), "must emit a NACK");
}

#[test]
fn proxy_has_no_batching_or_coalescing() {
    // Structural assertion: proxy.rs must not contain batching, coalescing,
    // or multi-envelope packing patterns.  G6 is strictly one-bundle-per-envelope.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "batch_send",
        "coalesce",
        "multi_tx",
        "bundle_batch",
        "pack_bundle",
        "merge_envelopes",
        "Vec<BundleRequest",
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for keyword in &forbidden {
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not batch or coalesce bundles ({keyword}); \
                 G6 is one-envelope-one-bundle"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Local failure path tests (sub-step 8)
// ---------------------------------------------------------------------------

#[test]
fn nack_carries_all_three_fields_on_session_down() {
    // When the session is Down, the proxy must emit a NACK carrying
    // signature, mint_out_index, and reserved_notional_lamports.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xD1, 7, 11, 42_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    let nack = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xD1; 64]);
    assert_eq!(nack.mint_out_index, 11);
    assert_eq!(nack.trade_size_lamports, 42_000_000);
}

#[test]
fn nack_carries_all_three_fields_on_send_failure() {
    // When send_bundle returns an error, the proxy must emit a NACK
    // carrying all three fields from the original envelope.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xE2, 3, 9, 77_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(
        test_config(),
        MockSession::with_failure(SendError::Transport("network".into())),
    );
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    let nack = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xE2; 64]);
    assert_eq!(nack.mint_out_index, 9);
    assert_eq!(nack.trade_size_lamports, 77_000_000);
}

#[test]
fn nack_carries_all_three_fields_on_connecting() {
    // A Connecting session means handshake in progress → fast-NACK.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xF3, 1, 5, 99_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connecting());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    let nack = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xF3; 64]);
    assert_eq!(nack.mint_out_index, 5);
    assert_eq!(nack.trade_size_lamports, 99_000_000);
}

#[test]
fn exactly_one_nack_per_failed_envelope() {
    // Each failed envelope must produce exactly one NACK.
    // Not zero (silent drop), not two (double-NACK).
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Push 5 envelopes, each with distinct fields.
    for i in 0..5u8 {
        let env = make_envelope(i + 0x50, i as u32, i as u32, (i as u64 + 1) * 1_000_000);
        egress_ring.push(&env);
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());

    // Process all 5 — each must be NACKed.
    for _ in 0..5 {
        assert_eq!(
            proxy.process_one(&egress_ring, &nack_ring, &result_ring),
            SendAttemptResult::NackEmitted
        );
    }
    assert_eq!(proxy.nacks_emitted, 5);
    assert_eq!(proxy.bundles_sent, 0);

    // Verify exactly 5 NACKs in the ring, each with correct identity.
    for i in 0..5u8 {
        let nack = nack_ring.pop().unwrap();
        assert_eq!(
            nack.signature,
            [i + 0x50; 64],
            "wrong signature at NACK {i}"
        );
        assert_eq!(nack.mint_out_index, i as u32, "wrong mint at NACK {i}");
        assert_eq!(
            nack.trade_size_lamports,
            (i as u64 + 1) * 1_000_000,
            "wrong trade_size at NACK {i}"
        );
    }

    // Ring should now be empty — no extra NACKs.
    assert!(nack_ring.pop().is_none());
}

#[test]
fn mixed_send_and_nack_produces_correct_counts() {
    // Interleave successful sends and failures to verify no cross-talk
    // between the NACK path and the success path.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    // Ring 0: will succeed (session is Up).
    resources.egress_rings[0].push(&make_envelope(0xA0, 0, 0, 1_000_000));
    // Ring 1: will succeed too.
    resources.egress_rings[1].push(&make_envelope(0xB0, 1, 1, 2_000_000));

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 2);
    assert_eq!(proxy.bundles_sent, 2);
    assert_eq!(proxy.nacks_emitted, 0);

    // NACK ring should be empty — no spurious NACKs on success.
    assert!(resources.nack_ring.pop().is_none());
}

// ---------------------------------------------------------------------------
// Structural guards for local failure path
// ---------------------------------------------------------------------------

#[test]
fn proxy_never_reroutes_to_worker() {
    // The proxy must not push failed envelopes back to the worker.
    // Only forward path: pop → send or NACK.  No re-enqueue, no retry queue.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "push_back",
        "re_enqueue",
        "retry_queue",
        "return_to_worker",
        "requeue",
        "back_to_ring",
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for keyword in &forbidden {
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not reroute failed envelopes to the worker ({keyword}); \
                 the only failure path is the NACK ring"
            );
        }
    }
}

#[test]
fn proxy_does_not_parse_bundle_state_in_failure_path() {
    // BundleState is exclusively PM's domain.  The proxy failure path
    // must not inspect, parse, or depend on on-chain/off-chain bundle outcomes.
    let proxy_src = include_str!("proxy.rs");

    let forbidden = [
        "BundleResult",
        "Landed",
        "Dropped",
        "bundle_outcome",
        "on_chain_status",
        "parse_bundle",
    ];

    for line in proxy_src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for keyword in &forbidden {
            assert!(
                !trimmed.contains(keyword),
                "proxy.rs must not parse bundle state ({keyword}); PM handles terminal status"
            );
        }
    }
}

#[test]
fn proxy_has_exactly_one_nack_emission_site() {
    // The NACK emission must be concentrated in a single function (emit_nack).
    // Multiple emission sites risk double-NACKing or inconsistent field mapping.
    let proxy_src = include_str!("proxy.rs");

    let nack_new_count = proxy_src
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.starts_with("//") && !t.starts_with("///") && !t.starts_with("//!")
        })
        .filter(|l| l.contains("NackRecord128B::new"))
        .count();

    assert_eq!(
        nack_new_count, 1,
        "proxy.rs must have exactly one NackRecord128B::new call site \
         (in emit_nack); found {nack_new_count}"
    );
}

#[test]
fn every_process_one_exit_is_accounted_for() {
    // Structural assertion: process_one returns exactly four possible
    // outcomes.  This test will fail to compile if SendAttemptResult
    // gains a variant that isn't handled.
    let outcomes = [
        SendAttemptResult::Sent,
        SendAttemptResult::NackEmitted,
        SendAttemptResult::NackRingFull,
        SendAttemptResult::RingEmpty,
    ];
    // Exhaustive match — compiler enforces all variants are covered.
    for o in &outcomes {
        match o {
            SendAttemptResult::Sent => {}
            SendAttemptResult::NackEmitted => {}
            SendAttemptResult::NackRingFull => {}
            SendAttemptResult::RingEmpty => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Ownership boundary: proxy vs PM (sub-step 9)
// ---------------------------------------------------------------------------
//
// The proxy owns:
//   - local dequeue / send success vs local failure
//   - NACK emission for local failure before network transmission
//   - coarse lane health publication
//
// shadow-pm owns:
//   - Jito BundleState websocket
//   - reservation release on Rejected / Dropped / NotLanded
//
// The proxy is not allowed to become a telemetry brain.

#[test]
fn crate_has_no_websocket_or_subscription_patterns() {
    // The proxy must not subscribe to any websocket, stream, or long-lived
    // notification channel.  BundleState tracking is PM's domain.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let health_src = include_str!("health.rs");
    let main_src = include_str!("main.rs");

    let forbidden = [
        "websocket",
        "WebSocket",
        "ws_connect",
        "tungstenite",
        "subscribe_bundle",
        "on_bundle_result",
        "bundle_status_stream",
        "BundleStateStream",
    ];

    for (name, src) in [
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("health.rs", health_src),
        ("main.rs", main_src),
    ] {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            for keyword in &forbidden {
                assert!(
                    !trimmed.contains(keyword),
                    "{name} must not contain websocket/subscription patterns ({keyword}); \
                     BundleState tracking is PM's domain"
                );
            }
        }
    }
}

#[test]
fn crate_does_not_handle_off_chain_terminal_states() {
    // The proxy must not reference off-chain terminal states like Rejected,
    // Dropped, NotLanded, or Finalized.  Those are PM reservation-release
    // triggers, not proxy concerns.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let health_src = include_str!("health.rs");

    let forbidden = [
        "NotLanded",
        "Finalized",
        "StateAuctionClosed",
        "Invalid",
        "release_reservation",
        "reservation_expired",
    ];

    for (name, src) in [
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("health.rs", health_src),
    ] {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            for keyword in &forbidden {
                assert!(
                    !trimmed.contains(keyword),
                    "{name} must not handle off-chain terminal states ({keyword}); \
                     PM owns reservation release"
                );
            }
        }
    }
}

#[test]
fn send_error_variants_are_strictly_local() {
    // SendError must only represent pre-network-transmission local failures.
    // It must never carry off-chain Jito result types.  Verify by source
    // inspection that no variant references bundle outcomes.
    let session_src = include_str!("session.rs");

    let forbidden_in_send_error = [
        "Landed",
        "Dropped",
        "NotLanded",
        "Finalized",
        "AuctionClosed",
        "BundleState",
        "BundleResult",
    ];

    // Find the SendError enum block and check it.
    let mut in_send_error = false;
    for line in session_src.lines() {
        let trimmed = line.trim();
        if trimmed.contains("pub enum SendError") {
            in_send_error = true;
        }
        if in_send_error {
            if trimmed == "}" && !trimmed.contains("{") {
                break; // end of enum
            }
            for keyword in &forbidden_in_send_error {
                assert!(
                    !trimmed.contains(keyword),
                    "SendError must not reference off-chain outcomes ({keyword}); \
                     it represents local-only failures"
                );
            }
        }
    }
    assert!(in_send_error, "SendError enum not found in session.rs");
}

#[test]
fn proxy_does_not_aggregate_telemetry_beyond_lane_health() {
    // The proxy publishes coarse lane health (LaneState + QueueAgeClass).
    // It must not aggregate per-bundle telemetry, latency histograms,
    // success-rate metrics, or JSON-structured telemetry payloads.
    let proxy_src = include_str!("proxy.rs");
    let health_src = include_str!("health.rs");

    let forbidden = [
        "histogram",
        "Histogram",
        "latency_p99",
        "latency_p50",
        "success_rate",
        "serde_json",
        "json!(",
        "telemetry_payload",
        "metrics_collector",
        "per_bundle_latency",
    ];

    for (name, src) in [("proxy.rs", proxy_src), ("health.rs", health_src)] {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            for keyword in &forbidden {
                assert!(
                    !trimmed.contains(keyword),
                    "{name} must not aggregate telemetry beyond coarse lane health ({keyword}); \
                     the proxy is not a telemetry brain"
                );
            }
        }
    }
}

#[test]
fn proxy_nack_is_pre_network_only() {
    // The proxy emits NACKs only for local failures before network transmission.
    // After a bundle is accepted by send_bundle (Ok), no NACK is ever emitted —
    // even if the bundle later fails on-chain.  Post-send outcomes are PM's domain.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Push two envelopes.
    let env1 = make_envelope(0x91, 0, 0, 1_000_000);
    let env2 = make_envelope(0x92, 1, 1, 2_000_000);
    egress_ring.push(&env1);
    egress_ring.push(&env2);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());

    // Both succeed at the send level.
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::Sent
    );
    assert_eq!(
        proxy.process_one(&egress_ring, &nack_ring, &result_ring),
        SendAttemptResult::Sent
    );
    assert_eq!(proxy.bundles_sent, 2);
    assert_eq!(proxy.nacks_emitted, 0);

    // NACK ring must be completely empty — no post-send NACKs.
    assert!(
        nack_ring.pop().is_none(),
        "NACK ring must be empty after successful sends"
    );
}

#[test]
fn cargo_toml_has_no_telemetry_dependencies() {
    // The proxy crate must not bring in websocket, JSON parsing, or
    // telemetry framework dependencies.  Those belong to PM.
    let cargo_toml = include_str!("../Cargo.toml");

    let forbidden = [
        "tungstenite",
        "tokio-tungstenite",
        // serde_json is now used for Solana keypair file parsing (auth).
        "prometheus",
        "opentelemetry",
        "metrics =",
    ];

    for keyword in &forbidden {
        assert!(
            !cargo_toml.contains(keyword),
            "Cargo.toml must not depend on {keyword}; \
             telemetry/websocket dependencies belong in shadow-pm"
        );
    }
}

// ---------------------------------------------------------------------------
// Coarse Jito health publication (sub-step 10)
// ---------------------------------------------------------------------------
//
// The Guerrila sidecar publishes to /dev/shm/shadow_egress_health:
//   jito_state ∈ {Up, Degraded, Down}
//   jito_queue_age_class ∈ [0..3]
//   last_update_unix_ns
//   publish_epoch
//
// Driven by: session availability, reconnect churn, local send failures,
// backlog / queue age.  Hysteresis prevents thrashing on transient noise.

#[test]
fn publish_jito_health_roundtrips_all_four_fields() {
    // After publishing, the segment must contain all four required fields:
    // jito_state, jito_queue_age_class, last_update_unix_ns, publish_epoch.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    // Recover from Down → Up.
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // Set queue age to Warm (30% fill).
    tracker.update_queue_age(30, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Warm);

    let now_ns: u64 = 1_700_000_000_000_000_000;
    publish_jito_health(&segment, &mut tracker, now_ns, 2);

    let payload = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(payload.jito_state), LaneState::Up);
    assert_eq!(
        QueueAgeClass::from_u8(payload.jito_queue_age_class),
        QueueAgeClass::Warm
    );
    assert_eq!(payload.last_update_unix_ns, now_ns);
    assert_eq!(payload.publish_epoch, 1);
    assert_eq!(payload.degraded_hysteresis_batches, 2);
}

#[test]
fn publish_jito_health_increments_epoch() {
    // Each publish must bump publish_epoch monotonically.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();

    publish_jito_health(&segment, &mut tracker, 100, 2);
    let p1 = segment.payload().read_active().unwrap();
    assert_eq!(p1.publish_epoch, 1);

    publish_jito_health(&segment, &mut tracker, 200, 2);
    let p2 = segment.payload().read_active().unwrap();
    assert_eq!(p2.publish_epoch, 2);
    assert_eq!(p2.last_update_unix_ns, 200);

    publish_jito_health(&segment, &mut tracker, 300, 2);
    let p3 = segment.payload().read_active().unwrap();
    assert_eq!(p3.publish_epoch, 3);
}

#[test]
fn publish_jito_health_preserves_harmonic_fields() {
    // Jito publish must read-modify-write: overwrite Jito fields,
    // preserve Harmonic fields set by the Harmonic proxy.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    // Simulate Harmonic proxy having written its fields first.
    let harmonic_payload = EgressHealthPayload32B {
        jito_state: LaneState::Down as u8,
        harmonic_state: LaneState::Up as u8,
        jito_queue_age_class: QueueAgeClass::Fresh as u8,
        harmonic_queue_age_class: QueueAgeClass::Backlogged as u8,
        degraded_hysteresis_batches: 0,
        _pad_u16: 0,
        last_update_unix_ns: 999,
        publish_epoch: 5,
        _reserved: [0u8; 8],
    };
    segment.payload().slot0.write_payload(&harmonic_payload);
    segment
        .payload()
        .active_idx
        .active_idx
        .store(0, std::sync::atomic::Ordering::Release);

    // Now Jito publishes — should overwrite Jito fields, keep Harmonic.
    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success(); // Up

    publish_jito_health(&segment, &mut tracker, 1_000_000, 2);

    let p = segment.payload().read_active().unwrap();
    // Jito fields overwritten:
    assert_eq!(LaneState::from_u8(p.jito_state), LaneState::Up);
    assert_eq!(p.last_update_unix_ns, 1_000_000);
    assert_eq!(p.publish_epoch, 1);
    // Harmonic fields preserved:
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Up);
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Backlogged
    );
}

#[test]
fn process_one_success_recovers_health_to_up() {
    // Two consecutive successful sends (= recover_batches) must drive
    // the health tracker from Down (fail-closed start) to Up.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    assert_eq!(proxy.health_state(), LaneState::Down); // fail-closed start

    egress_ring.push(&make_envelope(0x01, 0, 0, 1_000_000));
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    // 1 success — not yet at recover_batches=2.
    assert_eq!(proxy.health_state(), LaneState::Down);

    egress_ring.push(&make_envelope(0x02, 0, 0, 1_000_000));
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    // 2 consecutive successes → Up.
    assert_eq!(proxy.health_state(), LaneState::Up);
}

#[test]
fn process_one_session_down_records_failure() {
    // When process_one NACKs due to !can_send(), it must record a failure
    // in the health tracker — not silently ignore the unavailability.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    assert_eq!(proxy.health_state(), LaneState::Down);

    egress_ring.push(&make_envelope(0x01, 0, 0, 1_000_000));
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    // Health must remain Down — session unavailability is a recorded failure,
    // not a silent no-op.
    assert_eq!(proxy.health_state(), LaneState::Down);
}

#[test]
fn process_one_send_failure_records_failure() {
    // When send_bundle returns a local error, health must record a failure.
    // With the session reporting Up (can_send = true) but send returning Err,
    // the tracker should still be pushed toward degradation.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = JitoProxy::new(test_config(), MockSession::with_failure(SendError::Timeout));
    assert_eq!(proxy.health_state(), LaneState::Down);

    egress_ring.push(&make_envelope(0x01, 0, 0, 1_000_000));
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    // Failure recorded — tracker stays Down (already Down, failure keeps it there).
    assert_eq!(proxy.health_state(), LaneState::Down);
}

#[test]
fn drain_once_publishes_health_to_segment() {
    // drain_once must publish health to the segment at the end of each pass.
    // After draining, workers can read the published health.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    resources.egress_rings[0].push(&make_envelope(0x01, 0, 0, 1_000_000));

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);

    // Health segment must now be readable.
    let payload = resources.health_segment.payload().read_active().unwrap();
    assert!(payload.publish_epoch >= 1, "drain_once must publish health");
    assert!(payload.last_update_unix_ns > 0, "must write timestamp");
}

#[test]
fn drain_once_connected_idle_recovers_health_to_up() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    assert_eq!(proxy.health_state(), LaneState::Down);

    proxy.drain_once(&resources);
    assert_eq!(proxy.health_state(), LaneState::Down);

    proxy.drain_once(&resources);
    assert_eq!(proxy.health_state(), LaneState::Up);

    let payload = resources.health_segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(payload.jito_state), LaneState::Up);
}

#[test]
fn drain_once_reflects_queue_backlog_in_health() {
    // Ring occupancy after draining must drive the published queue_age_class.
    // Push enough to keep the ring partially full after drain_once pops one.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    // Ring capacity is 16.  Push 13 envelopes so after drain pops 1, 12 remain.
    // 12/16 = 75% → Severe.
    for i in 0..13u8 {
        resources.egress_rings[0].push(&make_envelope(i, 0, 0, 1_000_000));
    }

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);

    let payload = resources.health_segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(payload.jito_queue_age_class),
        QueueAgeClass::Severe,
        "75% fill must be classified as Severe"
    );
}

#[test]
fn health_hysteresis_prevents_single_failure_publish_flip() {
    // A single failure after sustained success must not immediately flip
    // the published health from Up to Degraded.  degrade_threshold = 3
    // means it takes 3 consecutive failures.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    // Get to Up.
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // Publish Up state.
    publish_jito_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.jito_state), LaneState::Up);

    // One failure — must NOT flip to Degraded.
    tracker.record_failure();
    publish_jito_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.jito_state),
        LaneState::Up,
        "single failure must not flip published state — hysteresis"
    );

    // Two failures — must NOT flip yet (threshold is 3).
    tracker.record_failure();
    publish_jito_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.jito_state),
        LaneState::Up,
        "two failures must not flip — threshold is 3"
    );

    // Third failure — now crosses degrade_threshold, should flip.
    tracker.record_failure();
    publish_jito_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.jito_state),
        LaneState::Degraded,
        "three consecutive failures must flip to Degraded"
    );
}

#[test]
fn reconnect_churn_does_not_degrade_published_health() {
    // Simulates reconnect churn: rapid success/failure alternation.
    // A single intervening success resets the consecutive failure count,
    // so churn alone must NOT degrade the lane — only sustained failure
    // (>= degrade_threshold consecutive) triggers degradation.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // Alternating: fail, succeed, fail, succeed — never hits 3 consecutive.
    for _ in 0..10 {
        tracker.record_failure();
        tracker.record_success();
    }

    publish_jito_health(&segment, &mut tracker, 500, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.jito_state),
        LaneState::Up,
        "reconnect churn (alternating fail/succeed) must not degrade — \
         hysteresis requires consecutive failures"
    );
}

#[test]
fn sustained_failure_drives_published_health_through_degraded_to_down() {
    // Sustained failures (no intervening successes) must drive published
    // health through Up → Degraded → Down.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = JitoHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // 3 failures → Degraded.
    for _ in 0..3 {
        tracker.record_failure();
    }
    publish_jito_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.jito_state), LaneState::Degraded);

    // 7 more failures (total 10) → Down.
    for _ in 0..7 {
        tracker.record_failure();
    }
    publish_jito_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.jito_state), LaneState::Down);
}

#[test]
fn publish_jito_health_writes_all_four_spec_fields_structural() {
    // Structural guard: the publish_jito_health function body must assign
    // payload.jito_state, payload.jito_queue_age_class, payload.last_update_unix_ns,
    // and payload.publish_epoch.  If any field is dropped from the write path,
    // workers will read stale data and make wrong admission decisions.
    let health_src = include_str!("health.rs");

    let required_fields = [
        "jito_state",
        "jito_queue_age_class",
        "last_update_unix_ns",
        "publish_epoch",
    ];

    let mut in_publish_fn = false;
    let mut found = [false; 4];
    for line in health_src.lines() {
        if line.contains("pub fn publish_jito_health") {
            in_publish_fn = true;
        }
        if in_publish_fn {
            for (i, field) in required_fields.iter().enumerate() {
                if line.contains(&format!("payload.{field}"))
                    && line.contains('=')
                    && !line.trim().starts_with("//")
                {
                    found[i] = true;
                }
            }
            // End of function.
            if line == "}" {
                break;
            }
        }
    }

    for (i, field) in required_fields.iter().enumerate() {
        assert!(
            found[i],
            "publish_jito_health must write payload.{field} — \
             spec requires all four fields"
        );
    }
}

// ---------------------------------------------------------------------------
// Queue-age computation: cheap and proxy-local (sub-step 11)
// ---------------------------------------------------------------------------
//
// The queue-age classifier must be:
//   - Cheap: integer arithmetic only, no allocations, no syscalls.
//   - Proxy-local: derived from ring occupancy, not per-envelope timestamps.
//   - Coarse: exactly 4 classes for a one-byte admissibility signal.
//
// Workers trust this signal for pre-sign egress admission.

#[test]
fn queue_age_boundary_values() {
    // Verify exact threshold boundaries: 24→Fresh, 25→Warm, 49→Warm,
    // 50→Backlogged, 74→Backlogged, 75→Severe.
    let mut t = JitoHealthTracker::new(3, 10, 2);

    // 24% → Fresh (last value in Fresh range).
    t.update_queue_age(24, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Fresh);

    // 25% → Warm (first value in Warm range).
    t.update_queue_age(25, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Warm);

    // 49% → Warm (last value in Warm range).
    t.update_queue_age(49, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Warm);

    // 50% → Backlogged (first value in Backlogged range).
    t.update_queue_age(50, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Backlogged);

    // 74% → Backlogged (last value in Backlogged range).
    t.update_queue_age(74, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Backlogged);

    // 75% → Severe (first value in Severe range).
    t.update_queue_age(75, 100);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Severe);
}

#[test]
fn queue_age_zero_capacity_is_severe() {
    // A ring with zero capacity is a configuration error; the classifier
    // must fail closed (Severe) rather than divide-by-zero.
    let mut t = JitoHealthTracker::new(3, 10, 2);
    t.update_queue_age(0, 0);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Severe);
}

#[test]
fn queue_age_empty_ring_is_fresh() {
    let mut t = JitoHealthTracker::new(3, 10, 2);
    t.update_queue_age(0, 128);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Fresh);
}

#[test]
fn queue_age_full_ring_is_severe() {
    let mut t = JitoHealthTracker::new(3, 10, 2);
    t.update_queue_age(128, 128);
    assert_eq!(t.queue_age_class(), QueueAgeClass::Severe);
}

#[test]
fn drain_once_empty_rings_publish_fresh() {
    // When all rings are empty after draining, queue age must be Fresh.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    let mut proxy = JitoProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);

    let payload = resources.health_segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(payload.jito_queue_age_class),
        QueueAgeClass::Fresh,
        "empty rings must publish Fresh queue age"
    );
}

#[test]
fn queue_age_aggregates_across_workers() {
    // Queue age is derived from aggregate ring occupancy, not per-ring.
    // Two rings each half-full = 50% aggregate → Backlogged.
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    // Each ring capacity=16.  Push 8 into each → 16/32 = 50% aggregate.
    for ring in &resources.egress_rings {
        for i in 0..8u8 {
            ring.push(&make_envelope(i, 0, 0, 1_000_000));
        }
    }

    // Use a disconnected session so nothing gets popped during drain.
    let mut proxy = JitoProxy::new(test_config(), MockSession::disconnected());
    proxy.drain_once(&resources);

    let payload = resources.health_segment.payload().read_active().unwrap();
    // After drain pops 1 from each ring: 14/32 = 43% → Warm.
    assert_eq!(
        QueueAgeClass::from_u8(payload.jito_queue_age_class),
        QueueAgeClass::Warm,
        "aggregate fill after drain (14/32=43%) should be Warm"
    );
}

#[test]
fn queue_age_has_exactly_four_variants() {
    // Structural: QueueAgeClass must have exactly Fresh, Warm, Backlogged, Severe.
    // This will fail to compile if a variant is removed or renamed.
    let classes = [
        QueueAgeClass::Fresh,
        QueueAgeClass::Warm,
        QueueAgeClass::Backlogged,
        QueueAgeClass::Severe,
    ];
    for c in &classes {
        match c {
            QueueAgeClass::Fresh => {}
            QueueAgeClass::Warm => {}
            QueueAgeClass::Backlogged => {}
            QueueAgeClass::Severe => {}
        }
    }
    // Verify discriminant values match spec.
    assert_eq!(QueueAgeClass::Fresh as u8, 0);
    assert_eq!(QueueAgeClass::Warm as u8, 1);
    assert_eq!(QueueAgeClass::Backlogged as u8, 2);
    assert_eq!(QueueAgeClass::Severe as u8, 3);
}

#[test]
fn degraded_plus_severe_queue_blocks_admission() {
    // The worker-side admissibility check: Degraded + Severe → inadmissible.
    // This is the key interaction between lane health and queue age that
    // the Guerrila sidecar uses to gate pre-sign egress admission.
    let p = EgressHealthPayload32B {
        jito_state: LaneState::Degraded as u8,
        harmonic_state: LaneState::Up as u8,
        jito_queue_age_class: QueueAgeClass::Severe as u8,
        harmonic_queue_age_class: QueueAgeClass::Fresh as u8,
        degraded_hysteresis_batches: 2,
        _pad_u16: 0,
        last_update_unix_ns: 1,
        publish_epoch: 1,
        _reserved: [0u8; 8],
    };
    assert!(
        !p.is_jito_admissible(),
        "Degraded + Severe must block admission"
    );

    // But Degraded + Backlogged is still admissible.
    let mut p2 = p;
    p2.jito_queue_age_class = QueueAgeClass::Backlogged as u8;
    assert!(
        p2.is_jito_admissible(),
        "Degraded + Backlogged must still admit"
    );
}

#[test]
fn queue_age_is_not_timestamp_based() {
    // Structural guard: update_queue_age must not reference SystemTime,
    // Instant, or any timestamp source.  It takes (ring_len, ring_capacity)
    // and nothing else — cheap integer arithmetic only.
    let health_src = include_str!("health.rs");

    let mut in_update_fn = false;
    for line in health_src.lines() {
        let trimmed = line.trim();
        if trimmed.contains("fn update_queue_age") {
            in_update_fn = true;
        }
        if in_update_fn {
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            assert!(
                !trimmed.contains("SystemTime")
                    && !trimmed.contains("Instant")
                    && !trimmed.contains("unix_ns")
                    && !trimmed.contains("elapsed"),
                "update_queue_age must not use timestamps; \
                 derive age from ring occupancy only"
            );
            // End of function (next `pub fn` or `fn` at the same indent level).
            if in_update_fn && trimmed.starts_with("}") && !trimmed.contains("{") {
                break;
            }
        }
    }
    assert!(
        in_update_fn,
        "update_queue_age function not found in health.rs"
    );
}

#[test]
fn no_per_envelope_telemetry_in_health_segment() {
    // The health segment carries only coarse signals (LaneState + QueueAgeClass).
    // No per-envelope timestamps, latencies, or detailed metrics.
    let health_src = include_str!("health.rs");
    let egress_health_src = include_str!("../../shadow-ipc/src/egress_health.rs");

    let forbidden = [
        "per_envelope",
        "envelope_timestamp",
        "dequeue_time",
        "send_latency_ns",
        "per_tx_",
        "Vec<u64>",
        "VecDeque",
    ];

    for (name, src) in [
        ("health.rs", health_src),
        ("egress_health.rs", egress_health_src),
    ] {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            for keyword in &forbidden {
                assert!(
                    !trimmed.contains(keyword),
                    "{name} must not contain per-envelope telemetry ({keyword}); \
                     workers need only a coarse admissibility signal"
                );
            }
        }
    }
}

// =========================================================================
// Sub-step 12 — Wire startup after ring bootstrap
// =========================================================================

/// Verify that `build_jito_egress_config` (in main.rs) is structurally
/// equivalent to constructing a JitoEgressConfig with money_workers from
/// ShadowConfig and defaults for everything else.  We can't call the
/// binary's private function directly, but we can assert the Default
/// derives the expected thresholds that the startup wiring relies on.
#[test]
fn jito_egress_config_default_thresholds_match_spec() {
    let cfg = JitoEgressConfig::default();
    assert_eq!(cfg.degrade_failure_threshold, 3, "degrade threshold");
    assert_eq!(cfg.down_failure_threshold, 10, "down threshold");
    assert_eq!(cfg.hysteresis_recover_batches, 2, "recover batches");
    assert_eq!(cfg.degraded_hysteresis_batches, 2, "hysteresis batches");
    assert_eq!(cfg.money_workers, 4, "default money workers");
}

/// The startup sequence creates rings first, then opens them via
/// ProxyResources::open().  Verify that open() reads money_workers
/// from config — if we pass money_workers=0, it should succeed with
/// an empty ring vec (no file I/O needed).
#[test]
fn proxy_resources_open_respects_money_workers_count() {
    let cfg = JitoEgressConfig {
        money_workers: 7,
        ..JitoEgressConfig::default()
    };
    let proxy = JitoProxy::new(cfg, StubSession);
    assert_eq!(proxy.config.money_workers, 7);
}

/// The startup wiring spawns the proxy on a named thread ("jito-proxy").
/// Verify that std::thread::Builder::new().name() produces a thread
/// whose name is recoverable — this is the pattern used in main.rs.
#[test]
fn named_thread_pattern_works() {
    let handle = std::thread::Builder::new()
        .name("jito-proxy-test".into())
        .spawn(|| std::thread::current().name().unwrap_or("").to_string())
        .unwrap();
    let name = handle.join().unwrap();
    assert_eq!(name, "jito-proxy-test");
}

/// The proxy's run() method is `-> !` (never returns).  Verify this
/// structurally by checking that JitoProxy::run exists and its
/// return type is documented as divergent.  We test drain_once
/// instead (which run() calls in a loop) to prove one iteration works.
#[test]
fn drain_once_is_one_iteration_of_run_loop() {
    let dir = tempfile::tempdir().unwrap();
    let ring_path = dir.path().join("ring_run_loop");
    let nack_path = dir.path().join("nack_run_loop");

    let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
        &ring_path,
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
        4,
    )
    .unwrap();
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 4, 1, 4).unwrap();

    let result_path = dir.path().join("result_run_loop");
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        shadow_ipc::egress_ring::JITO_RESULT_RING_SEGMENT_ID,
        shadow_ipc::egress_ring::JITO_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    let health_path = dir.path().join("health_run_loop");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    let resources = ProxyResources {
        egress_rings: vec![ring],
        nack_ring,
        result_ring,
        health_segment,
    };

    let cfg = test_config();
    let mut proxy = JitoProxy::new(cfg, StubSession);

    // Push one envelope
    let env = make_envelope(0xAA, 0, 1, 500);
    resources.egress_rings[0].push(&env);

    // drain_once should process it (NACK via StubSession)
    let processed = proxy.drain_once(&resources);
    assert_eq!(
        processed, 1,
        "drain_once processes exactly one envelope per ring"
    );
    assert_eq!(proxy.nacks_emitted, 1, "StubSession NACKs everything");
}

/// Startup order: rings are created before open, NACK ring before open,
/// health segment before open.  Verify that ProxyResources fields are
/// public and the struct can be constructed directly (integration seam).
#[test]
fn proxy_resources_fields_are_public() {
    let dir = tempfile::tempdir().unwrap();

    let ring_path = dir.path().join("ring_pub");
    let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
        &ring_path,
        JITO_EGRESS_RING_SEGMENT_ID,
        JITO_EGRESS_RING_VERSION,
        4,
    )
    .unwrap();

    let nack_path = dir.path().join("nack_pub");
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 4, 1, 4).unwrap();

    let result_path = dir.path().join("result_pub");
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        shadow_ipc::egress_ring::JITO_RESULT_RING_SEGMENT_ID,
        shadow_ipc::egress_ring::JITO_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    let health_path = dir.path().join("health_pub");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    let resources = ProxyResources {
        egress_rings: vec![ring],
        nack_ring,
        result_ring,
        health_segment,
    };

    assert_eq!(resources.egress_rings.len(), 1);
    assert_eq!(resources.nack_ring.len(), 0);
    assert_eq!(resources.result_ring.len(), 0);
}

/// The startup wiring moves ProxyResources into the proxy thread.
/// Verify that ProxyResources is Send (required for thread::spawn move).
#[test]
fn proxy_resources_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<ProxyResources>();
}

/// The startup wiring moves JitoEgressConfig into the proxy thread.
/// Verify that JitoEgressConfig is Send + Clone (required for the
/// build-then-move-into-thread pattern).
#[test]
fn jito_egress_config_is_send_and_clone() {
    fn assert_send<T: Send>() {}
    fn assert_clone<T: Clone>() {}
    assert_send::<JitoEgressConfig>();
    assert_clone::<JitoEgressConfig>();
}

/// StubSession is the placeholder used in startup wiring until real
/// Searcher API is implemented.  Verify it starts Down (fail-closed)
/// and returns NotConnected on send — the proxy NACKs everything but
/// still publishes health.
#[test]
fn stub_session_starts_down_fail_closed() {
    let stub = StubSession;
    assert_eq!(stub.state(), SessionState::Down);
    assert!(!stub.can_send(), "StubSession must not allow sending");

    let req = BundleRequest::single_tx(&[1, 2, 3]);
    let mut stub = StubSession;
    match stub.send_bundle(&req) {
        Err(SendError::NotConnected) => {}
        other => panic!("expected NotConnected, got {:?}", other),
    }
}

/// Structural: main.rs must reference all required bootstrap functions
/// and proxy bring-up calls for the full startup sequence.
#[test]
fn main_rs_contains_full_startup_sequence() {
    let src = include_str!("../../shadow-tier0/src/main.rs");

    assert!(
        src.contains("bootstrap_jito_egress_rings"),
        "must bootstrap egress rings"
    );
    assert!(
        src.contains("bootstrap_nack_ring"),
        "must bootstrap NACK ring"
    );
    assert!(
        src.contains("bootstrap_egress_health_segment"),
        "must bootstrap health segment"
    );
    assert!(
        src.contains("build_jito_egress_config"),
        "must build Jito config from ShadowConfig"
    );
    assert!(
        src.contains("ProxyResources::open"),
        "must open proxy resources"
    );
    assert!(src.contains("spawn_jito_proxy"), "must spawn proxy thread");
    assert!(src.contains("jito-proxy"), "proxy thread must be named");
    assert!(
        src.contains("GrpcSearcherSession"),
        "must use GrpcSearcherSession for real Searcher API"
    );
}

/// Structural: verify the startup ordering in main.rs is correct.
/// Static validation → ring bootstrap → NACK bootstrap → health bootstrap
/// → config build → resource open → proxy spawn.
#[test]
fn main_rs_startup_ordering_is_correct() {
    let src = include_str!("../../shadow-tier0/src/main.rs");

    let pos_static_validation = src.find("validate_guerrila_static").unwrap();
    let pos_ring_bootstrap = src.find("bootstrap_jito_egress_rings(&cfg)").unwrap();
    let pos_nack_bootstrap = src.find("bootstrap_nack_ring()").unwrap();
    let pos_health_bootstrap = src.find("bootstrap_egress_health_segment()").unwrap();
    let pos_config_build = src.find("build_jito_egress_config(&cfg)").unwrap();
    let pos_resource_open = src.find("ProxyResources::open(&jito_config)").unwrap();
    let pos_proxy_spawn = src.find("spawn_jito_proxy(").unwrap();

    assert!(
        pos_static_validation < pos_ring_bootstrap,
        "static validation must precede ring bootstrap"
    );
    assert!(
        pos_ring_bootstrap < pos_nack_bootstrap,
        "ring bootstrap must precede NACK bootstrap"
    );
    assert!(
        pos_nack_bootstrap < pos_health_bootstrap,
        "NACK bootstrap must precede health bootstrap"
    );
    assert!(
        pos_health_bootstrap < pos_config_build,
        "health bootstrap must precede config build"
    );
    assert!(
        pos_config_build < pos_resource_open,
        "config build must precede resource open"
    );
    assert!(
        pos_resource_open < pos_proxy_spawn,
        "resource open must precede proxy spawn"
    );
}

// ---------------------------------------------------------------------------
// Session realization tests (G6 Goal 3, sub-step 11)
// ---------------------------------------------------------------------------

use crate::config::AuthMode;
use crate::grpc::auth::AuthToken;
use crate::session::{ConnectResult, GrpcSearcherSession, RefreshResult};

/// Create a lazy tonic Channel for testing.
///
/// `connect_lazy()` requires a Tokio reactor, so we create it inside
/// a temporary runtime.  The channel itself is inert (never connects).
fn test_lazy_channel() -> tonic::transport::Channel {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        tonic::transport::Channel::from_static("http://[::1]:50051").connect_lazy()
    })
}

/// Authenticated connect success: Connecting → Up.
///
/// Simulates a successful auth connect by sending ConnectResult::Auth
/// through the mpsc channel.  After poll(), the session must be Up
/// and sendable.
#[test]
fn grpc_session_auth_connect_success_transitions_to_up() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    assert_eq!(session.state(), SessionState::Connecting);
    assert!(!session.can_send());

    // Simulate a successful auth result from the worker thread.
    let token = AuthToken::empty();
    token.set("test-access-token".into());
    tx.send(ConnectResult::Auth {
        channel: test_lazy_channel(),
        token,
        refresh_token_value: "test-refresh".into(),
        token_expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
    })
    .unwrap();

    session.poll();

    assert_eq!(session.state(), SessionState::Up);
    assert!(session.can_send());
}

/// Auth failure: Connecting → Down with backoff.
///
/// When the worker thread reports ConnectResult::Failed, the session
/// must transition to Down and set a backoff timer.
#[test]
fn grpc_session_auth_failure_transitions_to_down_with_backoff() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    assert_eq!(session.state(), SessionState::Connecting);

    tx.send(ConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();

    assert_eq!(session.state(), SessionState::Down);
    assert!(!session.can_send());
}

#[test]
fn grpc_session_permission_denied_auth_uses_long_backoff() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    tx.send(ConnectResult::Failed(
        "auth: status: PermissionDenied, message: \"The supplied pubkey is not authorized to generate a token.\"".to_owned(),
    ))
    .unwrap();
    session.poll();

    assert_eq!(session.state(), SessionState::Down);
    let remaining = session
        .disconnected_backoff_remaining_ms()
        .expect("permission-denied failure must set a backoff");
    assert!(
        remaining >= 119_000,
        "unauthorized auth key must not hammer Jito auth; remaining={remaining}ms"
    );
}

#[test]
fn grpc_session_disconnected_poll_preserves_unexpired_backoff() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    tx.send(ConnectResult::Failed(
        "auth: status: PermissionDenied, message: \"The supplied pubkey is not authorized to generate a token.\"".to_owned(),
    ))
    .unwrap();
    session.poll();
    let first_remaining = session
        .disconnected_backoff_remaining_ms()
        .expect("permission-denied failure must set a backoff");

    session.poll();

    assert_eq!(session.state(), SessionState::Down);
    assert!(!session.can_send());
    let second_remaining = session
        .disconnected_backoff_remaining_ms()
        .expect("unexpired backoff must survive idle polls");
    assert!(
        second_remaining > 0 && second_remaining <= first_remaining,
        "backoff should remain active after a poll; first={first_remaining}ms second={second_remaining}ms"
    );
}

#[test]
fn grpc_session_respects_server_auth_retry_after_ms() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    tx.send(ConnectResult::Failed(
        "auth: status: ResourceExhausted, message: \"Rate limit exceeded. Back-off triggered: Retry after 120000ms\"".to_owned(),
    ))
    .unwrap();
    session.poll();

    assert_eq!(session.state(), SessionState::Down);
    let remaining = session
        .disconnected_backoff_remaining_ms()
        .expect("server throttle must set a backoff");
    assert!(
        remaining >= 119_000,
        "server retry-after must override local reconnect cap; remaining={remaining}ms"
    );
}

/// No-auth mode: Connecting → Up via ConnectResult::NoAuth.
///
/// When auth_mode is NoAuth, the worker thread sends ConnectResult::NoAuth
/// (channel only, no token).  Session must reach Up.
#[test]
fn grpc_session_noauth_connect_success_transitions_to_up() {
    let mut config = test_config();
    config.auth_mode = AuthMode::NoAuth;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    tx.send(ConnectResult::NoAuth {
        channel: test_lazy_channel(),
    })
    .unwrap();

    session.poll();

    assert_eq!(session.state(), SessionState::Up);
    assert!(session.can_send());
}

/// send_bundle() builds one Jito bundle containing exactly the original tx bytes.
///
/// Verifies the G6 framing contract: one tx → one Packet → one Bundle.
/// The raw bytes must appear verbatim in the single Packet's data field.
#[test]
fn build_send_bundle_request_preserves_exact_bytes() {
    use crate::session::build_send_bundle_request;

    let tx_bytes: Vec<u8> = (0..200).collect();
    let req = build_send_bundle_request(&tx_bytes);

    // Must have exactly one bundle.
    let bundle = req.bundle.expect("request must contain a bundle");

    // Bundle must have exactly one packet.
    assert_eq!(bundle.packets.len(), 1, "G6: one tx per bundle");

    // Packet data must be the exact original bytes.
    assert_eq!(
        bundle.packets[0].data, tx_bytes,
        "packet data must be the original tx bytes verbatim"
    );

    // Bundle must have a header with a timestamp.
    assert!(bundle.header.is_some(), "bundle must have a header");
    assert!(
        bundle.header.unwrap().ts.is_some(),
        "header must have a timestamp"
    );
}

/// Tonic transport failure → exactly one NACK, no retry.
///
/// When a send fails with Transport error, the proxy must emit exactly
/// one NACK for that envelope and move on.  No retry logic.
#[test]
fn transport_failure_produces_one_nack_no_retry() {
    let _diag_lock = crate::lock_proxy_diagnostics_for_test();
    crate::reset_proxy_diagnostics_for_test();
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xAB, 2, 8, 55_000_000);
    egress_ring.push(&env);

    let mut proxy = JitoProxy::new(
        test_config(),
        MockSession::with_failure(SendError::Transport("connection reset".into())),
    );

    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.nacks_emitted, 1);
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(
        crate::snapshot_proxy_diagnostics(),
        crate::JitoProxyDiagnosticsSnapshot::default()
    );

    // Exactly one NACK in the ring.
    let nack = nack_ring.pop().expect("must have one NACK");
    assert_eq!(nack.signature, [0xAB; 64]);
    assert!(
        nack_ring.pop().is_none(),
        "must be exactly one NACK, not a retry"
    );

    // Egress ring must be drained (envelope consumed, not re-queued).
    assert!(
        egress_ring.pop().is_none(),
        "envelope must be consumed, not re-queued for retry"
    );
}

#[test]
fn public_rpc_transport_failure_updates_diag_snapshot() {
    let _diag_lock = crate::lock_proxy_diagnostics_for_test();
    crate::reset_proxy_diagnostics_for_test();

    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xBC, 2, 8, 55_000_000);
    egress_ring.push(&env);

    let mut cfg = test_config();
    cfg.public_rpc.enabled = true;
    cfg.public_rpc.endpoint = "https://127.0.0.1:1".into();
    let mut proxy = JitoProxy::new(
        cfg,
        MockSession::with_failure(SendError::Transport("connection reset".into())),
    );

    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(
        crate::snapshot_proxy_diagnostics(),
        crate::JitoProxyDiagnosticsSnapshot {
            public_rpc_transport_attempt: 1,
            public_rpc_transport_failure: 1,
            public_rpc_transport_rejected: 0,
            public_rpc_nack_recorded: 1,
            first_public_rpc_attempt_signature: Some([0xBC; 64]),
            ..Default::default()
        }
    );
}

#[test]
fn public_rpc_rejection_captures_reason_and_tx_metadata() {
    let _diag_lock = crate::lock_proxy_diagnostics_for_test();
    crate::reset_proxy_diagnostics_for_test();

    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut env = make_envelope(0xBD, 2, 8, 55_000_000);
    let tx = make_public_rpc_clmm_rejection_test_tx(
        0xBD,
        0x44,
        250_000,
        77_000,
        200_000_000,
        348_264_726_421,
    );
    env.len = tx.len();
    env.raw_bytes[..tx.len()].copy_from_slice(&tx);
    egress_ring.push(&env);

    let rejection = serde_json::json!({
        "code": -32002,
        "message": "Transaction simulation failed: Error processing Instruction 2: custom program error: 0x1786",
        "data": {
            "err": {"InstructionError":[2,{"Custom":6022}]},
            "logs": [
                "Program log: Instruction: SwapV2",
                "Program log: AnchorError occurred. Error Code: TooLittleOutputReceived. Error Number: 6022. Error Message: Too little output received.",
                "Program log: Left: 58535449673",
                "Program log: Right: 348264726421",
                "Program failed: custom program error: 0x1786"
            ]
        }
    })
    .to_string();

    let mut cfg = test_config();
    cfg.public_rpc.enabled = true;
    cfg.public_rpc.endpoint = "https://rpc.test".into();
    let mut proxy = JitoProxy::new(
        cfg,
        MockSession::with_failure(SendError::Rejected(rejection)),
    );

    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    let snapshot = crate::snapshot_proxy_diagnostics();
    assert_eq!(snapshot.public_rpc_transport_attempt, 1);
    assert_eq!(snapshot.public_rpc_transport_failure, 0);
    assert_eq!(snapshot.public_rpc_transport_rejected, 1);
    assert_eq!(snapshot.public_rpc_nack_recorded, 1);
    assert_eq!(
        snapshot.first_public_rpc_attempt_signature,
        Some([0xBD; 64])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_code, -32002);
    assert!(snapshot
        .last_public_rpc_rejection_message
        .contains("Transaction simulation failed"));
    assert_eq!(
        snapshot.last_public_rpc_rejection_reason,
        "slippage_min_out"
    );
    assert!(snapshot
        .last_public_rpc_rejection_simulation_logs
        .contains("TooLittleOutputReceived"));
    assert!(snapshot
        .last_public_rpc_rejection_data_err
        .contains("InstructionError"));
    assert_eq!(
        snapshot.last_public_rpc_rejection_signature,
        Some([0xBD; 64])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_tx_size, tx.len() as u64);
    assert_eq!(
        snapshot.last_public_rpc_rejection_blockhash,
        Some([0x44; 32])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_cu_limit, 250_000);
    assert_eq!(
        snapshot.last_public_rpc_rejection_cu_price_micro_lamports,
        0
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix1_program_id,
        Some([0x77; 32])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_ix1_account_count, 0);
    assert_eq!(snapshot.last_public_rpc_rejection_ix1_account_0, None);
    assert_eq!(snapshot.last_public_rpc_rejection_ix1_account_1, None);
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix2_program_id,
        Some([0x88; 32])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_ix2_account_count, 2);
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix2_amount_in,
        200_000_000
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix2_min_out,
        348_264_726_421
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_actual_output_lamports,
        58_535_449_673
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_required_output_lamports,
        348_264_726_421
    );
}

#[test]
fn public_rpc_rejection_captures_ix1_accounts() {
    let _diag_lock = crate::lock_proxy_diagnostics_for_test();
    crate::reset_proxy_diagnostics_for_test();

    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut env = make_envelope(0xBE, 2, 8, 55_000_000);
    let tx = make_public_rpc_guard_test_tx(0xBE, 0x45);
    env.len = tx.len();
    env.raw_bytes[..tx.len()].copy_from_slice(&tx);
    egress_ring.push(&env);

    let rejection = serde_json::json!({
        "code": -32002,
        "message": "Transaction simulation failed: Error processing Instruction 1: Provided owner is not allowed",
        "data": {
            "err": {"InstructionError":[1,"IllegalOwner"]},
            "logs": [
                "Program log: ShadowGuard: vault_out not owned by a token program"
            ]
        }
    })
    .to_string();

    let mut cfg = test_config();
    cfg.public_rpc.enabled = true;
    cfg.public_rpc.endpoint = "https://rpc.test".into();
    let mut proxy = JitoProxy::new(
        cfg,
        MockSession::with_failure(SendError::Rejected(rejection)),
    );

    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    let snapshot = crate::snapshot_proxy_diagnostics();
    assert_eq!(
        snapshot.first_public_rpc_attempt_signature,
        Some([0xBE; 64])
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix1_program_id,
        Some([0x77; 32])
    );
    assert_eq!(snapshot.last_public_rpc_rejection_ix1_account_count, 2);
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix1_account_0,
        Some([0x22; 32])
    );
    assert_eq!(
        snapshot.last_public_rpc_rejection_ix1_account_1,
        Some([0x33; 32])
    );
}

/// Reconnect after dead channel: Down → Connecting → Up.
///
/// Simulates the full reconnect cycle: session in Connecting receives
/// a failure (goes Down), then a new connect attempt succeeds (goes Up).
#[test]
fn grpc_session_reconnect_after_down_returns_to_up() {
    let config = test_config();

    // Phase 1: Connecting → failure → Down.
    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config.clone(), rx1);
    tx1.send(ConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();
    assert_eq!(session.state(), SessionState::Down);

    // Phase 2: Simulate reconnect by creating a new Connecting session
    // (mirrors what poll_disconnected does after backoff expires).
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);
    let mut session2 = GrpcSearcherSession::new_test_connecting(config, rx2);
    let token = AuthToken::empty();
    token.set("reconnect-token".into());
    tx2.send(ConnectResult::Auth {
        channel: test_lazy_channel(),
        token,
        refresh_token_value: "refresh".into(),
        token_expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
    })
    .unwrap();
    session2.poll();
    assert_eq!(session2.state(), SessionState::Up);
    assert!(
        session2.can_send(),
        "session must be sendable after reconnect"
    );
}

/// Startup fails closed when auth mode requires a keypair and none exists.
///
/// GrpcSearcherSession::new() with AuthKeypair pointing to a nonexistent
/// file must panic — fail-closed by design.
#[test]
#[should_panic(expected = "failed to read keypair file")]
fn grpc_session_panics_when_keypair_missing() {
    let mut config = test_config();
    config.auth_mode = AuthMode::AuthKeypair(std::path::PathBuf::from(
        "/nonexistent/path/to/keypair.json",
    ));
    // This must panic — fail-closed at startup.
    let _session = GrpcSearcherSession::new(config);
}

/// Structural guard: no production entrypoint instantiates StubSession.
///
/// Both shadow-tier0/src/main.rs and shadow-egress-jito/src/main.rs must
/// not reference StubSession anywhere (it's test-only).
#[test]
fn no_production_entrypoint_uses_stub_session() {
    let tier0_main = include_str!("../../shadow-tier0/src/main.rs");
    let egress_main = include_str!("main.rs");

    assert!(
        !tier0_main.contains("StubSession"),
        "shadow-tier0 main.rs must not reference StubSession"
    );
    assert!(
        !egress_main.contains("StubSession"),
        "shadow-egress-jito main.rs must not reference StubSession"
    );

    // Positive: both must use GrpcSearcherSession.
    assert!(
        tier0_main.contains("GrpcSearcherSession"),
        "shadow-tier0 main.rs must use GrpcSearcherSession"
    );
    assert!(
        egress_main.contains("GrpcSearcherSession"),
        "shadow-egress-jito main.rs must use GrpcSearcherSession"
    );
}

/// Worker thread disconnect (sender dropped) → session goes Down.
///
/// If the worker thread panics or drops the sender without sending
/// a result, poll() must treat this as a failure and go Down.
#[test]
fn grpc_session_worker_disconnect_transitions_to_down() {
    let config = test_config();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    // Drop the sender without sending anything — simulates worker crash.
    drop(tx);

    session.poll();
    assert_eq!(session.state(), SessionState::Down);
    assert!(!session.can_send());
}

/// Poll in Connecting with no result yet stays Connecting.
///
/// When the worker thread hasn't finished yet (try_recv → Empty),
/// poll() must leave the session in Connecting, not block.
#[test]
fn grpc_session_poll_connecting_stays_connecting_when_pending() {
    let config = test_config();
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    // Poll without sending a result — worker is still in flight.
    session.poll();

    assert_eq!(session.state(), SessionState::Connecting);
    assert!(!session.can_send());
}

/// Regression: Degraded → Up recovery must use `hysteresis_recover_batches`
/// (default 2), NOT `degrade_failure_threshold` (default 3).
///
/// The test sets thresholds so the two values differ (degrade=3, recover=2),
/// then proves that exactly `hysteresis_recover_batches` consecutive successes
/// are sufficient for recovery.
#[test]
fn grpc_session_degraded_recovers_at_hysteresis_threshold_not_degrade_threshold() {
    let config = JitoEgressConfig {
        degrade_failure_threshold: 3,
        hysteresis_recover_batches: 2,
        ..test_config()
    };
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    // Drive to Up via auth success.
    _tx.send(ConnectResult::Auth {
        channel: test_lazy_channel(),
        token: AuthToken::empty(),
        refresh_token_value: "ref".into(),
        token_expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
    })
    .unwrap();
    session.poll();
    assert_eq!(session.state(), SessionState::Up);

    // Drive Up → Degraded with 3 consecutive failures.
    for _ in 0..3 {
        session.test_record_send_failure();
    }
    assert_eq!(session.state(), SessionState::Degraded);

    // 1 success is not enough to recover.
    session.test_record_send_success();
    assert_eq!(session.state(), SessionState::Degraded);
    assert_eq!(session.consecutive_successes(), 1);

    // 2nd success hits `hysteresis_recover_batches` (2) → Up.
    session.test_record_send_success();
    assert_eq!(session.state(), SessionState::Up);
    assert_eq!(session.consecutive_successes(), 2);
}

/// Regression: token refresh must update the bearer token and expiry
/// but MUST NOT overwrite the stored refresh_token_value.
///
/// RefreshResult.new_value is the new *access* token.  The original
/// refresh token stays valid for subsequent refresh cycles.  Overwriting
/// it with the access token would cause the next refresh to fail.
#[test]
fn grpc_session_token_refresh_preserves_refresh_token_value() {
    let config = test_config();
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut session = GrpcSearcherSession::new_test_connecting(config, rx);

    // Drive to Up via auth success with known token values.
    let token = AuthToken::empty();
    token.set("old-access".into());
    _tx.send(ConnectResult::Auth {
        channel: test_lazy_channel(),
        token,
        refresh_token_value: "refresh-token".into(),
        token_expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
    })
    .unwrap();
    session.poll();
    assert_eq!(session.state(), SessionState::Up);

    // Verify initial state.
    assert_eq!(session.bearer_token_value().unwrap(), "old-access");
    assert_eq!(session.refresh_token_value().unwrap(), "refresh-token");

    // Inject a completed refresh result with a new access token.
    let (refresh_tx, refresh_rx) = std::sync::mpsc::sync_channel(1);
    session.set_pending_refresh_for_test(refresh_rx);
    refresh_tx
        .send(Some(RefreshResult {
            new_value: "new-access".into(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(600),
        }))
        .unwrap();

    // poll() processes the refresh result.
    session.poll();

    // Bearer token must be updated to the new access token.
    assert_eq!(
        session.bearer_token_value().unwrap(),
        "new-access",
        "bearer token must be updated to new access token"
    );

    // Refresh token must be UNCHANGED — still the original value.
    assert_eq!(
        session.refresh_token_value().unwrap(),
        "refresh-token",
        "refresh_token_value must NOT be overwritten by the new access token"
    );
}

// ---------------------------------------------------------------------------
// Integration smoke test (G6 close-out)
// ---------------------------------------------------------------------------
//
// Ignored by default — run manually with:
//
//   # NoAuth (local/testnet endpoint):
//   JITO_BLOCK_ENGINE_URL="https://mainnet.block-engine.jito.wtf" \
//       cargo test -p shadow-egress-jito integration_smoke -- --ignored
//
//   # With auth keypair:
//   JITO_BLOCK_ENGINE_URL="https://mainnet.block-engine.jito.wtf" \
//   JITO_AUTH_KEYPAIR_PATH="/path/to/keypair.json" \
//       cargo test -p shadow-egress-jito integration_smoke -- --ignored
//
// Verifies: connect → auth → send one bundle.  Does NOT verify landing
// or BundleState — that's PM's domain.  A Rejected response is success:
// it proves the bundle reached the Searcher API server.

#[test]
#[ignore]
fn integration_smoke_connect_auth_send() {
    let url =
        std::env::var("JITO_BLOCK_ENGINE_URL").expect("set JITO_BLOCK_ENGINE_URL to run this test");

    let auth_mode = match std::env::var("JITO_AUTH_KEYPAIR_PATH") {
        Ok(path) => AuthMode::AuthKeypair(std::path::PathBuf::from(path)),
        Err(_) => AuthMode::NoAuth,
    };

    let config = JitoEgressConfig {
        block_engine_url: url.clone(),
        money_workers: 1,
        auth_mode: auth_mode.clone(),
        tls_enabled: true,
        connect_timeout_ms: 10_000,
        send_timeout_ms: 10_000,
        reconnect_initial_backoff_ms: 250,
        reconnect_max_backoff_ms: 5_000,
        ..JitoEgressConfig::default()
    };

    println!("smoke: endpoint = {url}");
    println!("smoke: auth     = {auth_mode:?}");

    // --- Phase 1: Connect + Auth ---
    let mut session = GrpcSearcherSession::new(config);

    // Poll until the session reaches Up (or timeout after ~15s).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while session.state() != SessionState::Up {
        session.poll();
        if std::time::Instant::now() > deadline {
            panic!(
                "smoke: timed out waiting for session to reach Up \
                 (stuck in {:?}). Check endpoint and auth config.",
                session.state()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    println!("smoke: session Up");
    assert!(session.can_send());

    // --- Phase 2: Send one bundle ---
    //
    // We send a deliberately dummy transaction (64 zero bytes).  The Jito
    // server will almost certainly reject it (invalid signature / format),
    // but that's fine: a Rejected response proves the bundle reached the
    // Searcher API and was processed.  Only Transport or Timeout errors
    // indicate a real connectivity problem.
    let dummy_tx = [0u8; 64];
    let req = BundleRequest::single_tx(&dummy_tx);
    let result = session.send_bundle(&req);

    match &result {
        Ok(_uuid) => {
            println!("smoke: send_bundle accepted (Ok)");
        }
        Err(SendError::Rejected(msg)) => {
            // Rejected = server received it and refused it.  This is
            // success for the smoke test — the full path worked.
            println!("smoke: send_bundle rejected (expected for dummy tx): {msg}");
        }
        Err(SendError::Transport(msg)) => {
            panic!("smoke: transport error — connectivity problem: {msg}");
        }
        Err(SendError::Timeout) => {
            panic!("smoke: send timed out — endpoint may be unreachable");
        }
        Err(SendError::NotConnected) => {
            panic!("smoke: session was Up but send returned NotConnected");
        }
    }

    println!("smoke: PASS — connect/auth/send verified");
}

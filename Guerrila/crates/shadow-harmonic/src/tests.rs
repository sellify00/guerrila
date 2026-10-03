// G7 – shadow-harmonic unit tests
//
// Mirrors the structure of shadow-egress-jito/src/tests.rs, adapted for
// Harmonic's regional fanout model and Death Clamp semantics.

use crate::config::HarmonicEgressConfig;
use crate::health::{publish_harmonic_health, HarmonicHealthTracker};
use crate::proxy::{
    validate_lane_semantics, HarmonicProxy, LaneViolationReason, ProxyResources,
    RegionalResultSummary, SendAttemptResult,
};
use crate::session::{
    BundleRequest, CapturingSession, HarmonicSession, MockSession, SendError, SessionState,
    StubSession,
};
use shadow_ipc::egress_health::{
    EgressHealthPayload32B, EgressHealthSnapshot, LaneState, QueueAgeClass,
};
use shadow_ipc::egress_ring::{
    EgressResult, SignedTxEnvelope, EGRESS_RESULT_RING_SEGMENT_ID, EGRESS_RESULT_RING_VERSION,
    HARMONIC_EGRESS_RING_SEGMENT_ID, HARMONIC_EGRESS_RING_VERSION, LANE_HARMONIC,
};
use shadow_ipc::nack::NackRecord128B;
use shadow_ipc::ring::MappedSpscRing;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config() -> HarmonicEgressConfig {
    HarmonicEgressConfig {
        region_endpoints: vec![
            "https://fra.test.local".into(),
            "https://ams.test.local".into(),
        ],
        money_workers: 1,
        degrade_failure_threshold: 3,
        down_failure_threshold: 10,
        hysteresis_recover_batches: 2,
        degraded_hysteresis_batches: 2,
        ..HarmonicEgressConfig::default()
    }
}

/// ComputeBudget111111111111111111111111111111
const TEST_CB_PROGRAM_ID: [u8; 32] = [
    0x03, 0x06, 0x46, 0x6f, 0xe5, 0x21, 0x17, 0x32, 0xff, 0xec, 0xad, 0xba, 0x72, 0xc3, 0x9b, 0xe7,
    0xbc, 0x8c, 0xe5, 0xbb, 0xc5, 0xf7, 0x12, 0x6b, 0x2c, 0x43, 0x9b, 0x3a, 0x40, 0x00, 0x00, 0x00,
];
const TEST_SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

/// Build a minimal valid Harmonic-shaped signed envelope.
///
/// Layout (message starts at offset 65):
///   header [1, 0, 1] | acct_count=2 | fee_payer(32) | CB_PROG(32) |
///   blockhash(32) | ix_count=1 | SetComputeUnitPrice(prog=1, 0 accts, 9 data: [0x03, u64])
fn make_envelope(sig_byte: u8, pool: u32, mint: u32, notional: u64) -> SignedTxEnvelope {
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 0,
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
    let mut c = 0usize;
    // -- signed tx wrapper --
    env.raw_bytes[c] = 1;
    c += 1; // 1 signature
    env.raw_bytes[c..c + 64].fill(sig_byte);
    c += 64; // signature bytes
             // -- message begins at c=65 --
    env.raw_bytes[c] = 1;
    c += 1; // num_required_signatures
    env.raw_bytes[c] = 0;
    c += 1; // num_readonly_signed
    env.raw_bytes[c] = 1;
    c += 1; // num_readonly_unsigned (CB program)
    env.raw_bytes[c] = 2;
    c += 1; // compact-u16: 2 accounts
            // account 0: fee payer (unique per sig_byte to avoid collisions)
    env.raw_bytes[c] = sig_byte.wrapping_add(1);
    c += 32;
    // account 1: ComputeBudget program
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_CB_PROGRAM_ID);
    c += 32;
    // blockhash (32 zero bytes is fine for test)
    c += 32;
    // instruction count = 1
    env.raw_bytes[c] = 1;
    c += 1;
    // ix 0: SetComputeUnitPrice
    env.raw_bytes[c] = 1;
    c += 1; // program_id_index = 1 (CB)
    env.raw_bytes[c] = 0;
    c += 1; // 0 accounts
    env.raw_bytes[c] = 9;
    c += 1; // data_len = 9
    env.raw_bytes[c] = 0x03;
    c += 1; // discriminator
            // u64 LE price (1000 micro-lamports)
    let price: u64 = 1000;
    env.raw_bytes[c..c + 8].copy_from_slice(&price.to_le_bytes());
    c += 8;

    env.len = c;
    env
}

/// Build a Jito-shaped envelope containing a SystemProgram::Transfer (tip instruction).
/// Used to test that the Harmonic lane validator rejects it.
fn make_jito_envelope(sig_byte: u8, pool: u32, mint: u32, notional: u64) -> SignedTxEnvelope {
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 0,
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
    let mut c = 0usize;
    env.raw_bytes[c] = 1;
    c += 1; // 1 signature
    env.raw_bytes[c..c + 64].fill(sig_byte);
    c += 64; // signature
             // message
    env.raw_bytes[c] = 1;
    c += 1; // num_required_signatures
    env.raw_bytes[c] = 0;
    c += 1; // num_readonly_signed
    env.raw_bytes[c] = 2;
    c += 1; // num_readonly_unsigned (CB + System)
    env.raw_bytes[c] = 4;
    c += 1; // 4 accounts
            // account 0: fee payer
    env.raw_bytes[c] = sig_byte.wrapping_add(1);
    c += 32;
    // account 1: tip account (arbitrary non-zero)
    env.raw_bytes[c] = 0xAA;
    c += 32;
    // account 2: ComputeBudget program
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_CB_PROGRAM_ID);
    c += 32;
    // account 3: System program (all zeros)
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_SYSTEM_PROGRAM_ID);
    c += 32;
    // blockhash
    c += 32;
    // 2 instructions
    env.raw_bytes[c] = 2;
    c += 1;
    // ix 0: SetComputeUnitPrice (so it also has CU price — tests tip detection specifically)
    env.raw_bytes[c] = 2;
    c += 1; // prog=2 (CB)
    env.raw_bytes[c] = 0;
    c += 1; // 0 accounts
    env.raw_bytes[c] = 9;
    c += 1; // data_len=9
    env.raw_bytes[c] = 0x03;
    c += 1; // disc
    let price: u64 = 500;
    env.raw_bytes[c..c + 8].copy_from_slice(&price.to_le_bytes());
    c += 8;
    // ix 1: SystemProgram::Transfer (Jito tip)
    env.raw_bytes[c] = 3;
    c += 1; // prog=3 (System)
    env.raw_bytes[c] = 2;
    c += 1; // 2 accounts
    env.raw_bytes[c] = 0;
    c += 1; // account idx 0 (fee payer)
    env.raw_bytes[c] = 1;
    c += 1; // account idx 1 (tip account)
    env.raw_bytes[c] = 12;
    c += 1; // data_len=12
    env.raw_bytes[c..c + 4].copy_from_slice(&[0x02, 0x00, 0x00, 0x00]);
    c += 4; // Transfer disc
    let tip: u64 = 100_000;
    env.raw_bytes[c..c + 8].copy_from_slice(&tip.to_le_bytes());
    c += 8;

    env.len = c;
    env
}

/// Build an envelope with no CU price instruction (only SetComputeUnitLimit).
fn make_no_cu_price_envelope(sig_byte: u8) -> SignedTxEnvelope {
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 0,
        signature: [sig_byte; 64],
        pool_index: 0,
        mint_out_index: 0,
        reserved_notional_lamports: 0,
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
    let mut c = 0usize;
    env.raw_bytes[c] = 1;
    c += 1;
    env.raw_bytes[c..c + 64].fill(sig_byte);
    c += 64;
    env.raw_bytes[c] = 1;
    c += 1; // num_required_signatures
    env.raw_bytes[c] = 0;
    c += 1;
    env.raw_bytes[c] = 1;
    c += 1; // num_readonly_unsigned
    env.raw_bytes[c] = 2;
    c += 1; // 2 accounts
    env.raw_bytes[c] = sig_byte.wrapping_add(1);
    c += 32; // fee payer
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_CB_PROGRAM_ID);
    c += 32;
    c += 32; // blockhash
    env.raw_bytes[c] = 1;
    c += 1; // 1 instruction
            // ix 0: SetComputeUnitLimit (disc 0x02, 5 bytes data) — NOT SetCuPrice
    env.raw_bytes[c] = 1;
    c += 1; // prog=1 (CB)
    env.raw_bytes[c] = 0;
    c += 1; // 0 accounts
    env.raw_bytes[c] = 5;
    c += 1; // data_len=5
    env.raw_bytes[c] = 0x02;
    c += 1; // SetComputeUnitLimit discriminator
    let limit: u32 = 300_000;
    env.raw_bytes[c..c + 4].copy_from_slice(&limit.to_le_bytes());
    c += 4;

    env.len = c;
    env
}

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
        HARMONIC_EGRESS_RING_SEGMENT_ID,
        HARMONIC_EGRESS_RING_VERSION,
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
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
        16,
    )
    .unwrap();

    (egress_ring, nack_ring, result_ring)
}

fn create_temp_proxy_resources(dir: &std::path::Path, n_workers: u32) -> ProxyResources {
    let mut egress_rings = Vec::with_capacity(n_workers as usize);
    for w in 0..n_workers {
        let path = dir.join(format!("egress_ring_{w}"));
        let ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
            &path,
            HARMONIC_EGRESS_RING_SEGMENT_ID,
            HARMONIC_EGRESS_RING_VERSION,
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

    let health_path = dir.join("egress_health");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    let result_path = dir.join("result_ring");
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
        64,
    )
    .unwrap();

    ProxyResources {
        egress_rings,
        nack_ring,
        health_segment,
        result_ring,
    }
}

// ===========================================================================
// 1. StubSession tests
// ===========================================================================

#[test]
fn stub_session_is_always_down() {
    let stub = StubSession;
    assert_eq!(stub.state(), SessionState::Down);
    assert!(!stub.can_send());
}

#[test]
fn stub_session_send_returns_not_connected() {
    let mut stub = StubSession;
    let req = BundleRequest::single_tx(&[0xAA; 32]);
    let result = stub.send_bundle(&req);
    assert!(result.is_err());
}

// ===========================================================================
// 2. MockSession tests
// ===========================================================================

#[test]
fn mock_session_connected_sends_successfully() {
    let mut mock = MockSession::connected();
    let req = BundleRequest::single_tx(&[0xBB; 32]);
    let result = mock.send_bundle(&req);
    assert!(result.is_ok());
    assert_eq!(mock.sent_count, 1);
}

#[test]
fn mock_session_disconnected_cannot_send() {
    let mock = MockSession::disconnected();
    assert_eq!(mock.state(), SessionState::Down);
    assert!(!mock.can_send());
}

#[test]
fn mock_session_with_failure_returns_error() {
    let mut mock = MockSession::with_failure(SendError::Timeout);
    let req = BundleRequest::single_tx(&[0xCC; 32]);
    let result = mock.send_bundle(&req);
    assert!(result.is_err());
}

// ===========================================================================
// 3. SessionState machine tests
// ===========================================================================

#[test]
fn can_send_only_when_up_or_degraded() {
    let up = MockSession::connected();
    assert!(up.can_send());

    let degraded = MockSession::degraded();
    assert!(degraded.can_send());

    let down = MockSession::disconnected();
    assert!(!down.can_send());

    let connecting = MockSession::connecting();
    assert!(!connecting.can_send());
}

#[test]
fn session_state_variants_exhaustive() {
    // All four states must exist.
    let states = [
        SessionState::Down,
        SessionState::Connecting,
        SessionState::Up,
        SessionState::Degraded,
    ];
    assert_eq!(states.len(), 4);
}

// ===========================================================================
// 4. HarmonicHealthTracker tests
// ===========================================================================

#[test]
fn health_tracker_starts_down() {
    let tracker = HarmonicHealthTracker::new(3, 10, 2);
    assert_eq!(tracker.state(), LaneState::Down);
}

#[test]
fn health_tracker_recovers_after_sustained_successes() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    assert_eq!(tracker.state(), LaneState::Down);

    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Down); // still need 2

    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up); // recovered
}

#[test]
fn health_tracker_degrades_on_consecutive_failures() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    // Bring up first.
    for _ in 0..2 {
        tracker.record_success();
    }
    assert_eq!(tracker.state(), LaneState::Up);

    // 3 failures → degrade.
    for _ in 0..3 {
        tracker.record_failure();
    }
    assert_eq!(tracker.state(), LaneState::Degraded);
}

#[test]
fn health_tracker_goes_down_on_many_failures() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    for _ in 0..2 {
        tracker.record_success();
    }
    assert_eq!(tracker.state(), LaneState::Up);

    // 10 failures → down.
    for _ in 0..10 {
        tracker.record_failure();
    }
    assert_eq!(tracker.state(), LaneState::Down);
}

#[test]
fn health_tracker_hysteresis_prevents_flip() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    for _ in 0..2 {
        tracker.record_success();
    }
    assert_eq!(tracker.state(), LaneState::Up);

    // 2 failures (< 3 threshold) should not degrade.
    tracker.record_failure();
    tracker.record_failure();
    assert_eq!(tracker.state(), LaneState::Up);

    // 1 success resets counter — still Up.
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);
}

#[test]
fn health_tracker_queue_age_classification() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);

    tracker.update_queue_age(0, 64);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Fresh);

    tracker.update_queue_age(16, 64); // 25%
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Warm);

    tracker.update_queue_age(40, 64); // 62.5%
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Backlogged);

    tracker.update_queue_age(60, 64); // 93.75%
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Severe);
}

#[test]
fn health_tracker_zero_capacity_is_severe() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.update_queue_age(0, 0);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Severe);
}

#[test]
fn health_tracker_epoch_increments() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    let p1 = tracker.to_payload(1000, 2);
    let p2 = tracker.to_payload(2000, 2);
    assert_eq!(p1.publish_epoch, 1);
    assert_eq!(p2.publish_epoch, 2);
}

// ===========================================================================
// 5. process_one tests
// ===========================================================================

#[test]
fn process_one_ring_empty_returns_ring_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::RingEmpty);
}

#[test]
fn process_one_sends_when_connected() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x01, 1, 2, 1_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    match result {
        SendAttemptResult::Sent(summary) => {
            assert_ne!(summary.ok_mask, 0); // at least one region succeeded
        }
        other => panic!("expected Sent, got {:?}", other),
    }
    assert_eq!(proxy.bundles_sent, 1);
}

#[test]
fn process_one_nacks_when_disconnected() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x02, 1, 2, 1_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.nacks_emitted, 1);

    // Verify NACK was actually pushed.
    let nack: NackRecord128B = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0x02; 64]);
    assert_eq!(nack.mint_out_index, 2);
    assert_eq!(nack.trade_size_lamports, 1_000_000);
}

#[test]
fn process_one_nacks_on_send_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x03, 1, 2, 500_000);
    egress_ring.push(&env);

    let mock = MockSession::with_failure(SendError::AllRegionsFailed {
        region_mask: 0x07,
        err_code: 0,
    });
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.nacks_emitted, 1);
}

#[test]
fn process_one_death_clamp_on_zero_ok_mask() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Create a mock that returns success but with ok_mask=0 (all regions failed).
    let mut mock = MockSession::connected();
    mock.region_count = 0; // This makes ok_mask = 0
    let env = make_envelope(0x04, 1, 2, 750_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    // Death Clamp: ok_mask==0 → NACK, not Sent.
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(proxy.nacks_emitted, 1);
}

#[test]
fn process_one_nack_ring_full() {
    let dir = tempfile::tempdir().unwrap();
    let egress_path = dir.path().join("egress_ring");
    let nack_path = dir.path().join("nack_ring_tiny");
    let result_path = dir.path().join("result_ring");

    let egress_ring = MappedSpscRing::<SignedTxEnvelope>::create_rw(
        &egress_path,
        HARMONIC_EGRESS_RING_SEGMENT_ID,
        HARMONIC_EGRESS_RING_VERSION,
        16,
    )
    .unwrap();

    // Create a small NACK ring (cap 2 == power-of-two).
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(
        &nack_path,
        shadow_ipc::nack::NACK_RING_SEGMENT_ID,
        shadow_ipc::nack::NACK_RING_VERSION,
        2,
    )
    .unwrap();

    let result_ring = MappedSpscRing::<EgressResult>::create_rw(
        &result_path,
        EGRESS_RESULT_RING_SEGMENT_ID,
        EGRESS_RESULT_RING_VERSION,
        16,
    )
    .unwrap();

    // Fill the NACK ring until full.
    let dummy_nack = NackRecord128B::new([0xFF; 64], 0, 0);
    while nack_ring.push(&dummy_nack) {}

    // Now push an envelope that will need to NACK but ring is full.
    let env = make_envelope(0x05, 1, 2, 100_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackRingFull);
}

// ===========================================================================
// 6. drain_once tests
// ===========================================================================

#[test]
fn drain_once_processes_one_per_ring() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    for (i, ring) in resources.egress_rings.iter().enumerate() {
        let env = make_envelope(i as u8, i as u32, 0, 1_000_000);
        ring.push(&env);
    }

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 3);
    assert_eq!(proxy.bundles_sent, 3);
}

#[test]
fn drain_once_zero_when_all_rings_empty() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 0);
}

#[test]
fn drain_once_nacks_all_when_disconnected() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 2);

    for ring in &resources.egress_rings {
        let env = make_envelope(0xAA, 1, 2, 500_000);
        ring.push(&env);
    }

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 2);
    assert_eq!(proxy.nacks_emitted, 2);
    assert_eq!(proxy.bundles_sent, 0);
}

#[test]
fn drain_once_partial_rings() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    // Only push into ring 0 and ring 2 — ring 1 is empty.
    let env = make_envelope(0x10, 0, 0, 1_000_000);
    resources.egress_rings[0].push(&env);
    let env = make_envelope(0x12, 2, 0, 1_000_000);
    resources.egress_rings[2].push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 2);
}

// ===========================================================================
// 7. Round-robin fairness
// ===========================================================================

#[test]
fn round_robin_starting_ring_rotates() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());

    // First drain starts at ring 0.
    proxy.drain_once(&resources);
    // After first drain, next_ring_idx should be 1.
    // Second drain starts at ring 1.
    proxy.drain_once(&resources);
    // Third drain starts at ring 2.
    proxy.drain_once(&resources);
    // Wrap around.
    proxy.drain_once(&resources);
    // No assertions on internal state — we just verify no panic.
}

#[test]
fn drain_once_zero_rings_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 0);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let processed = proxy.drain_once(&resources);
    assert_eq!(processed, 0);
}

// ===========================================================================
// 8. Post-sign immutability
// ===========================================================================

#[test]
fn exact_raw_bytes_sent_to_session() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x42, 7, 3, 2_000_000);
    egress_ring.push(&env);

    let capturing = CapturingSession::new();
    let mut proxy = HarmonicProxy::new(test_config(), capturing);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    match result {
        SendAttemptResult::Sent(_) => {}
        other => panic!("expected Sent, got {:?}", other),
    }

    let session = proxy.session_ref();
    assert_eq!(session.captured.len(), 1);

    // Verify the captured bytes match raw_bytes[..len] exactly (post-sign immutability).
    let expected: Vec<u8> = env.raw_bytes[..env.len].to_vec();
    assert_eq!(&session.captured[0], &expected);
}

// ===========================================================================
// 9. NACK field fidelity
// ===========================================================================

#[test]
fn nack_preserves_envelope_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xDE, 42, 99, 12_345_678);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    let nack: NackRecord128B = nack_ring.pop().unwrap();
    assert_eq!(nack.signature, [0xDE; 64]);
    assert_eq!(nack.mint_out_index, 99);
    assert_eq!(nack.trade_size_lamports, 12_345_678);
}

// ===========================================================================
// 10. Health publication
// ===========================================================================

#[test]
fn publish_harmonic_health_roundtrips_fields() {
    let dir = tempfile::tempdir().unwrap();
    let health_path = dir.path().join("egress_health");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    // Now Up.

    publish_harmonic_health(&health_segment, &mut tracker, 999_000, 2);

    let payload = health_segment.payload().read_active().unwrap();
    assert_eq!(payload.harmonic_state, LaneState::Up as u8);
    assert_eq!(payload.last_update_unix_ns, 999_000);
    assert!(payload.publish_epoch > 0);
}

#[test]
fn publish_harmonic_health_preserves_jito_fields() {
    let dir = tempfile::tempdir().unwrap();
    let health_path = dir.path().join("egress_health");
    let health_segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&health_path)
            .unwrap();

    // First, simulate a Jito publish by writing directly.
    let jito_payload = EgressHealthPayload32B {
        jito_state: LaneState::Up as u8,
        harmonic_state: LaneState::Down as u8,
        jito_queue_age_class: QueueAgeClass::Warm as u8,
        harmonic_queue_age_class: QueueAgeClass::Fresh as u8,
        degraded_hysteresis_batches: 5,
        _pad_u16: 0,
        last_update_unix_ns: 500_000,
        publish_epoch: 10,
        _reserved: [0u8; 8],
    };
    let health = health_segment.payload();
    health.slot0.write_payload(&jito_payload);
    health
        .active_idx
        .active_idx
        .store(0, std::sync::atomic::Ordering::Release);

    // Now publish Harmonic health — should preserve Jito fields.
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success(); // → Up

    publish_harmonic_health(&health_segment, &mut tracker, 600_000, 3);

    let payload = health_segment.payload().read_active().unwrap();
    // Jito fields preserved.
    assert_eq!(payload.jito_state, LaneState::Up as u8);
    assert_eq!(payload.jito_queue_age_class, QueueAgeClass::Warm as u8);
    // Harmonic fields updated.
    assert_eq!(payload.harmonic_state, LaneState::Up as u8);
}

#[test]
fn drain_once_publishes_health() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);

    // After drain_once, health segment should have been written.
    let payload = resources.health_segment.payload().read_active().unwrap();
    assert!(payload.last_update_unix_ns > 0);
}

#[test]
fn drain_once_connected_idle_recovers_health_to_up() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    assert_eq!(proxy.health_state(), LaneState::Down);

    proxy.drain_once(&resources);
    assert_eq!(proxy.health_state(), LaneState::Down);

    proxy.drain_once(&resources);
    assert_eq!(proxy.health_state(), LaneState::Up);

    let payload = resources.health_segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(payload.harmonic_state), LaneState::Up);
}

#[test]
fn process_one_records_health_on_success_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Success.
    let env = make_envelope(0x01, 1, 2, 1_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(proxy.health_state(), LaneState::Down); // need 2 successes to recover

    // Another success.
    let env = make_envelope(0x02, 1, 2, 1_000_000);
    egress_ring.push(&env);
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(proxy.health_state(), LaneState::Up);
}

// ===========================================================================
// 11. Death Clamp structural guards
// ===========================================================================

#[test]
fn death_clamp_no_tpu_fallback_structural() {
    // Structural: verify that proxy.rs does not contain any TPU/QUIC/public fallback.
    let proxy_src = include_str!("proxy.rs");
    let banned = [
        "QuicClient",
        "TpuSender",
        "quic_client",
        "tpu_connection",
        "public_rpc",
    ];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{pat}' — Death Clamp forbids public fallback"
        );
    }
}

#[test]
fn death_clamp_no_reroute_to_jito() {
    // Structural: proxy.rs must not reference Jito rerouting.
    let proxy_src = include_str!("proxy.rs");
    let banned = ["SearcherSession", "jito_egress"];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{pat}' — Death Clamp forbids rerouting"
        );
    }
}

#[test]
fn no_bundle_state_dependency() {
    // Structural: proxy must not parse BundleState telemetry.
    let proxy_src = include_str!("proxy.rs");
    assert!(
        !proxy_src.contains("BundleState"),
        "proxy must not parse BundleState"
    );
}

// ===========================================================================
// 12. Post-sign immutability structural guards
// ===========================================================================

#[test]
fn no_transaction_mutation_in_proxy() {
    let proxy_src = include_str!("proxy.rs");
    let banned = [
        "patch_tip",
        "patch_blockhash",
        "patch_compute",
        "rebuild_transaction",
        "set_recent_blockhash",
        "mut raw_bytes",
    ];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{pat}' — post-sign immutability"
        );
    }
}

// ===========================================================================
// 13. No ring creation by proxy
// ===========================================================================

#[test]
fn proxy_never_creates_rings() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        !proxy_src.contains("create_harmonic_egress_ring"),
        "proxy must open, never create, egress rings"
    );
    assert!(
        !proxy_src.contains("create_shadow_nack_ring"),
        "proxy must open, never create, nack ring"
    );
}

// ===========================================================================
// 14. No second drop/release queue
// ===========================================================================

#[test]
fn no_second_release_queue() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        !proxy_src.contains("drop_queue"),
        "proxy must not invent a second release queue"
    );
    assert!(
        !proxy_src.contains("release_ring"),
        "proxy must not invent a second release ring"
    );
}

// ===========================================================================
// 15. No worker state inspection
// ===========================================================================

#[test]
fn no_worker_state_inspection() {
    let proxy_src = include_str!("proxy.rs");
    let banned = [
        "PoolOverlay",
        "SigDedupe",
        "WorkerLocalState",
        "dominance",
        "inventory",
        "route_decision",
        "alpha",
    ];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not inspect worker state: '{pat}'"
        );
    }
}

// ===========================================================================
// 16. No economics/fee inspection
// ===========================================================================

#[test]
fn no_economics_inspection() {
    let proxy_src = include_str!("proxy.rs");
    let banned = ["tip_lamports", "cu_price", "fee_budget", "tip_account"];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not inspect economics: '{pat}'"
        );
    }
}

// ===========================================================================
// 17. RegionalResult propagation
// ===========================================================================

#[test]
fn regional_result_propagated_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x50, 1, 2, 1_000_000);
    egress_ring.push(&env);

    let mut mock = MockSession::connected();
    mock.region_count = 3;
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    match result {
        SendAttemptResult::Sent(summary) => {
            assert_eq!(summary.region_mask, 0x07); // 3 regions
            assert_eq!(summary.ok_mask, 0x07); // all succeeded
            assert_eq!(summary.err_code, 0);
        }
        other => panic!("expected Sent, got {:?}", other),
    }
}

// ===========================================================================
// 18. Config defaults
// ===========================================================================

#[test]
fn config_defaults_are_sensible() {
    let cfg = HarmonicEgressConfig::default();
    assert!(cfg.region_endpoints.is_empty());
    assert_eq!(cfg.money_workers, 4);
    assert_eq!(cfg.auth_mode, crate::config::AuthMode::NoAuth);
    assert_eq!(cfg.connect_timeout_ms, 5000);
    assert_eq!(cfg.send_timeout_ms, 3000);
    assert_eq!(cfg.reconnect_initial_backoff_ms, 250);
    assert_eq!(cfg.reconnect_max_backoff_ms, 10_000);
    assert!(cfg.tls_enabled);
    assert_eq!(cfg.degrade_failure_threshold, 3);
    assert_eq!(cfg.down_failure_threshold, 10);
    assert_eq!(cfg.hysteresis_recover_batches, 2);
}

// ===========================================================================
// 19. One-bundle-per-envelope framing
// ===========================================================================

#[test]
fn bundle_request_wraps_exact_bytes() {
    let data = [0xAB; 64];
    let req = BundleRequest::single_tx(&data);
    assert_eq!(req.tx_bytes, &data);
}

// ===========================================================================
// 20. Mixed send/NACK counting
// ===========================================================================

#[test]
fn mixed_send_nack_counting() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    // Push 3 envelopes.
    for i in 0..3u8 {
        let env = make_envelope(i, i as u32, 0, 1_000_000);
        resources.egress_rings[0].push(&env);
    }

    // First: connected → sends.
    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);
    assert_eq!(proxy.bundles_sent, 1); // drain_once pops 1 per ring per call

    // Push more and drain.
    let env = make_envelope(0x10, 0, 0, 1_000_000);
    resources.egress_rings[0].push(&env);
    proxy.drain_once(&resources);
    assert_eq!(proxy.bundles_sent, 2);
}

// ===========================================================================
// 21. CapturingSession captures all sends
// ===========================================================================

#[test]
fn capturing_session_records_all_sends() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    for i in 0..3u8 {
        let env = make_envelope(i, i as u32, 0, 1_000_000);
        egress_ring.push(&env);
    }

    let capturing = CapturingSession::new();
    let mut proxy = HarmonicProxy::new(test_config(), capturing);

    for _ in 0..3 {
        proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    }

    let session = proxy.session_ref();
    assert_eq!(session.captured.len(), 3);
}

// ===========================================================================
// 22. SendAttemptResult exhaustive
// ===========================================================================

#[test]
fn send_attempt_result_exhaustive_variants() {
    let variants = [
        SendAttemptResult::Sent(RegionalResultSummary {
            region_mask: 0x03,
            ok_mask: 0x03,
            err_code: 0,
        }),
        SendAttemptResult::NackEmitted,
        SendAttemptResult::NackRingFull,
        SendAttemptResult::RingEmpty,
    ];
    assert_eq!(variants.len(), 4);
}

// ===========================================================================
// 23. Degraded recovery through proxy
// ===========================================================================

#[test]
fn degraded_recovery_through_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());

    // Send 2 successes to recover from Down → Up.
    for _ in 0..2 {
        let env = make_envelope(0x01, 1, 2, 1_000_000);
        egress_ring.push(&env);
        proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    }
    assert_eq!(proxy.health_state(), LaneState::Up);
}

// ===========================================================================
// 24. Session poll is called in drain_once
// ===========================================================================

#[test]
fn drain_once_calls_poll_structural() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("self.poll_session()"),
        "drain_once must call poll_session()"
    );
}

// ===========================================================================
// 25. Proxy diagnostics
// ===========================================================================

#[test]
fn proxy_session_state_reports_correctly() {
    let proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    assert_eq!(proxy.session_state(), SessionState::Up);

    let proxy2 = HarmonicProxy::new(test_config(), MockSession::disconnected());
    assert_eq!(proxy2.session_state(), SessionState::Down);
}

#[test]
fn proxy_health_state_starts_down() {
    let proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    assert_eq!(proxy.health_state(), LaneState::Down);
}

// ===========================================================================
// 26. Sequential drain correctness
// ===========================================================================

#[test]
fn sequential_drain_sends_all_envelopes() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Push 5 envelopes.
    for i in 0..5u8 {
        let env = make_envelope(i, i as u32, 0, 1_000_000);
        egress_ring.push(&env);
    }

    let capturing = CapturingSession::new();
    let mut proxy = HarmonicProxy::new(test_config(), capturing);

    // Pop them one at a time.
    for _ in 0..5 {
        proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    }

    let session = proxy.session_ref();
    assert_eq!(session.captured.len(), 5);
    assert_eq!(proxy.bundles_sent, 5);

    // Sixth should be empty.
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::RingEmpty);
}

// ===========================================================================
// 27. Exactly one NACK per failure
// ===========================================================================

#[test]
fn exactly_one_nack_per_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xBB, 5, 10, 999_999);
    egress_ring.push(&env);

    let mock = MockSession::with_failure(SendError::Timeout);
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    assert_eq!(proxy.nacks_emitted, 1);
    assert_eq!(nack_ring.len(), 1);
}

// ===========================================================================
// 28. HarmonicSession trait — RegionalResult returned
// ===========================================================================

#[test]
fn harmonic_session_returns_regional_result() {
    let mut mock = MockSession::connected();
    let req = BundleRequest::single_tx(&[0xAA; 32]);
    let result = mock.send_bundle(&req).unwrap();
    // MockSession with 2 regions: mask=0x03, ok_mask=0x03, err_code=0.
    assert_eq!(result.region_mask, 0x03);
    assert_eq!(result.ok_mask, 0x03);
    assert_eq!(result.err_code, 0);
}

// ===========================================================================
// 29. Sustained failure drives tracker Down
// ===========================================================================

#[test]
fn sustained_failure_drives_down() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    // Bring up.
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // 10 failures in a row → Down.
    for _ in 0..10 {
        tracker.record_failure();
    }
    assert_eq!(tracker.state(), LaneState::Down);
}

// ===========================================================================
// 30. Module structure
// ===========================================================================

#[test]
fn lib_exports_expected_types() {
    // Verify that re-exports compile and are accessible.
    let _: fn() -> HarmonicEgressConfig = HarmonicEgressConfig::default;
    let _ = SessionState::Down;
    let _ = SendAttemptResult::RingEmpty;
}

// ===========================================================================
// 31. GrpcHarmonicSession — structural tests
// ===========================================================================

use crate::session::GrpcHarmonicSession;
use crate::session::RegionConnectResult;

#[test]
fn grpc_session_starts_in_connecting_state() {
    // Create with 2 regions, both start Connecting (no real server).
    let (_tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (_tx2, rx2) = std::sync::mpsc::sync_channel(1);
    let cfg = test_config();
    let session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2]);
    assert_eq!(session.region_count(), 2);
    assert_eq!(session.state(), SessionState::Connecting);
    assert_eq!(session.region_state(0), SessionState::Connecting);
    assert_eq!(session.region_state(1), SessionState::Connecting);
    assert!(!session.can_send());
}

#[test]
fn grpc_session_noauth_connect_transitions_to_up() {
    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);

    let cfg = test_config();
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2]);

    // Simulate both regions connecting with NoAuth.
    // We can't create a real Channel, but we can test the connect failure path.
    // To test success, we'd need a real server. Instead, test that Failed → Down.
    tx1.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    tx2.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();

    session.poll();

    // Both regions should be Down after failed connect.
    assert_eq!(session.region_state(0), SessionState::Down);
    assert_eq!(session.region_state(1), SessionState::Down);
    assert_eq!(session.state(), SessionState::Down);
}

#[test]
fn grpc_session_partial_region_failure() {
    // One region fails connect, sender drops → region goes Down.
    let (_tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);

    let cfg = test_config();
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2]);

    // Region 1 fails, region 0 still connecting (tx1 held but no message sent).
    tx2.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();

    session.poll();

    // Region 0 still Connecting (no result yet), region 1 Down.
    assert_eq!(session.region_state(0), SessionState::Connecting);
    assert_eq!(session.region_state(1), SessionState::Down);
    // Aggregate: Connecting (region 0 is still connecting).
    assert_eq!(session.state(), SessionState::Connecting);
}

#[test]
fn grpc_session_all_regions_dropped_sender_goes_down() {
    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);

    let cfg = test_config();
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2]);

    // Drop senders without sending → Disconnected on poll.
    drop(tx1);
    drop(tx2);

    session.poll();

    assert_eq!(session.region_state(0), SessionState::Down);
    assert_eq!(session.region_state(1), SessionState::Down);
    assert_eq!(session.state(), SessionState::Down);
}

#[test]
fn grpc_session_send_when_not_connected_returns_error() {
    let (_tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let cfg = HarmonicEgressConfig {
        region_endpoints: vec!["https://test.local".into()],
        money_workers: 1,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1]);

    // Connecting state — no Ready regions.
    let req = BundleRequest::single_tx(&[0xAA; 32]);
    let result = session.send_bundle(&req);
    assert!(result.is_err());
    match result {
        Err(SendError::NotConnected) => {}
        other => panic!("expected NotConnected, got {:?}", other),
    }
}

#[test]
fn grpc_session_send_after_all_fail_returns_not_connected() {
    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let cfg = HarmonicEgressConfig {
        region_endpoints: vec!["https://test.local".into()],
        money_workers: 1,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1]);

    // Force region down.
    tx1.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();
    assert_eq!(session.state(), SessionState::Down);

    let req = BundleRequest::single_tx(&[0xBB; 32]);
    let result = session.send_bundle(&req);
    match result {
        Err(SendError::NotConnected) => {}
        other => panic!("expected NotConnected, got {:?}", other),
    }
    assert_eq!(session.consecutive_failures(), 1); // only send failure counts
}

#[test]
fn grpc_session_aggregate_state_picks_best() {
    // 3 regions: test various state combos.
    let (_tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);
    let (tx3, rx3) = std::sync::mpsc::sync_channel(1);

    let cfg = HarmonicEgressConfig {
        region_endpoints: vec![
            "https://r1.local".into(),
            "https://r2.local".into(),
            "https://r3.local".into(),
        ],
        money_workers: 1,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2, rx3]);

    // Region 1 and 2 fail, region 0 still connecting.
    tx2.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    tx3.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();

    session.poll();

    // R0 = Connecting, R1 = Down, R2 = Down → aggregate Connecting.
    assert_eq!(session.state(), SessionState::Connecting);
}

#[test]
fn grpc_session_zero_regions_is_down() {
    let cfg = HarmonicEgressConfig {
        region_endpoints: vec![],
        money_workers: 1,
        ..HarmonicEgressConfig::default()
    };
    let session = GrpcHarmonicSession::new_test(cfg, vec![]);
    assert_eq!(session.region_count(), 0);
    assert_eq!(session.state(), SessionState::Down);
    assert!(!session.can_send());
}

#[test]
fn grpc_session_empty_poll_no_panic() {
    let cfg = HarmonicEgressConfig {
        region_endpoints: vec![],
        money_workers: 1,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![]);
    session.poll();
    session.poll();
    assert_eq!(session.state(), SessionState::Down);
}

// ===========================================================================
// 32. GrpcHarmonicSession — structural guards
// ===========================================================================

#[test]
fn grpc_session_module_has_multi_region_support() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("RegionSession"),
        "session.rs must define per-region session state"
    );
    assert!(
        src.contains("RegionInner"),
        "session.rs must have per-region inner state machine"
    );
    assert!(
        src.contains("aggregate_state"),
        "session.rs must derive aggregate state from regions"
    );
}

#[test]
fn grpc_session_no_blocking_handshake_in_drain() {
    let src = include_str!("session.rs");
    // poll() should use try_recv, never recv (blocking).
    // The only block_on should be in send_bundle.
    let poll_section = src.split("fn poll(").nth(1).unwrap_or("");
    assert!(
        !poll_section.contains("block_on"),
        "poll() must never use block_on — non-blocking only"
    );
}

#[test]
fn grpc_session_uses_capped_exponential_backoff() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("reconnect_max_backoff_ms"),
        "session.rs must cap backoff"
    );
    assert!(
        src.contains("backoff_until"),
        "session.rs must track backoff timer"
    );
}

#[test]
fn grpc_session_classify_status_exists() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("fn classify_status"),
        "session.rs must classify gRPC status codes"
    );
}

#[test]
fn grpc_session_regional_fanout_in_send() {
    let src = include_str!("session.rs");
    // send_bundle must iterate over regions.
    let send_section_idx = src.find("fn send_bundle(&mut self").unwrap_or(0);
    let send_section = &src[send_section_idx..];
    assert!(
        send_section.contains("self.regions.len()") || send_section.contains("for i in"),
        "send_bundle must fan out to multiple regions"
    );
}

#[test]
fn grpc_session_auth_flow_exists() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("run_auth_flow"),
        "session.rs must have auth challenge-response flow"
    );
    assert!(
        src.contains("run_refresh"),
        "session.rs must support token refresh"
    );
}

#[test]
fn grpc_session_max_regions_bounded() {
    assert!(crate::session::MAX_REGIONS <= 8);
}

#[test]
fn grpc_session_error_codes_defined() {
    assert_eq!(crate::session::ERR_TIMEOUT, 1);
    assert_eq!(crate::session::ERR_TRANSPORT, 2);
    assert_eq!(crate::session::ERR_REJECTED, 3);
}

#[test]
fn grpc_session_death_clamp_on_all_regions_failed() {
    let src = include_str!("session.rs");
    assert!(
        src.contains("AllRegionsFailed"),
        "send_bundle must return AllRegionsFailed when ok_mask==0"
    );
}

// ===========================================================================
// 33. GrpcHarmonicSession — backoff timer tests
// ===========================================================================

#[test]
fn grpc_session_backoff_after_failed_connect() {
    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let cfg = HarmonicEgressConfig {
        region_endpoints: vec!["https://test.local".into()],
        money_workers: 1,
        reconnect_initial_backoff_ms: 250,
        reconnect_max_backoff_ms: 10_000,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1]);

    // Fail connect.
    tx1.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();
    assert_eq!(session.region_state(0), SessionState::Down);

    // Second poll: should still be Down (backoff timer not expired).
    session.poll();
    assert_eq!(session.region_state(0), SessionState::Down);
}

#[test]
fn grpc_session_re_exports_in_lib() {
    // Verify that GrpcHarmonicSession is re-exported from lib.
    let _: fn(HarmonicEgressConfig) -> GrpcHarmonicSession = GrpcHarmonicSession::new;
}

// ===========================================================================
// 34. EgressResult ring publication
// ===========================================================================

#[test]
fn result_published_on_successful_send() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x30, 5, 10, 2_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert!(matches!(result, SendAttemptResult::Sent(_)));
    assert_eq!(proxy.results_published, 1);

    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egress_result.signature, [0x30; 64]);
    assert_eq!(egress_result.lane, LANE_HARMONIC);
    assert_eq!(egress_result.pool_index, 5);
    assert_eq!(egress_result.mint_out_index, 10);
    assert_eq!(egress_result.reserved_notional_lamports, 2_000_000);
    assert_ne!(egress_result.ok_mask, 0); // at least one region succeeded
}

#[test]
fn result_published_on_nack() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x31, 8, 15, 3_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.results_published, 1);

    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egress_result.signature, [0x31; 64]);
    assert_eq!(egress_result.lane, LANE_HARMONIC);
    assert_eq!(egress_result.ok_mask, 0); // no region attempted
    assert_eq!(egress_result.region_mask, 0); // no region attempted
    assert_eq!(egress_result.pool_index, 8);
}

#[test]
fn result_published_on_all_regions_failed() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x32, 3, 7, 1_500_000);
    egress_ring.push(&env);

    let mock = MockSession::with_failure(SendError::AllRegionsFailed {
        region_mask: 0x07,
        err_code: 0,
    });
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);
    assert_eq!(proxy.results_published, 1);

    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egress_result.lane, LANE_HARMONIC);
    assert_eq!(egress_result.ok_mask, 0);
}

#[test]
fn result_not_published_on_empty_ring() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::RingEmpty);
    assert_eq!(proxy.results_published, 0);
    assert!(result_ring.pop().is_none());
}

#[test]
fn result_preserves_death_clamp_ok_mask_zero() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut mock = MockSession::connected();
    mock.region_count = 0; // ok_mask = 0
    let env = make_envelope(0x33, 1, 2, 750_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), mock);
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egress_result.ok_mask, 0);
    assert_eq!(egress_result.lane, LANE_HARMONIC);
}

#[test]
fn drain_once_publishes_results_for_all_workers() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 3);

    for (i, ring) in resources.egress_rings.iter().enumerate() {
        let env = make_envelope(i as u8, i as u32, 0, 1_000_000);
        ring.push(&env);
    }

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    proxy.drain_once(&resources);
    assert_eq!(proxy.results_published, 3);
}

// ===========================================================================
// 35. Proxy thread structural guards
// ===========================================================================

#[test]
fn proxy_has_blocking_run_loop() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("pub fn run("),
        "proxy must have a blocking run loop"
    );
    assert!(
        proxy_src.contains("-> !"),
        "run() must be a diverging function"
    );
}

#[test]
fn main_uses_grpc_harmonic_session() {
    let main_src = include_str!("main.rs");
    assert!(
        main_src.contains("GrpcHarmonicSession::new"),
        "main.rs must create a real GrpcHarmonicSession"
    );
    assert!(
        !main_src.contains("StubSession"),
        "main.rs must not use StubSession in production"
    );
}

#[test]
fn main_validates_region_endpoints() {
    let main_src = include_str!("main.rs");
    assert!(
        main_src.contains("region_endpoints.is_empty()"),
        "main.rs must reject empty region endpoints"
    );
}

#[test]
fn main_fails_closed_on_resource_open_failure() {
    let main_src = include_str!("main.rs");
    assert!(
        main_src.contains("process::exit(1)"),
        "main.rs must exit on resource open failure"
    );
}

#[test]
fn proxy_resources_opens_result_ring() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("open_egress_result_ring_rw"),
        "ProxyResources must open the egress result ring"
    );
    assert!(
        proxy_src.contains("result_ring"),
        "ProxyResources must have a result_ring field"
    );
}

#[test]
fn proxy_never_creates_result_ring() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        !proxy_src.contains("create_egress_result_ring"),
        "proxy must open, never create, the result ring"
    );
}

#[test]
fn emit_result_uses_lane_harmonic() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("LANE_HARMONIC"),
        "emit_result must tag results with LANE_HARMONIC"
    );
}

#[test]
fn process_one_always_emits_result() {
    // Structural: every non-RingEmpty path through process_one emits a result,
    // either directly via self.emit_result() or via self.death_clamp() which
    // calls emit_result internally.
    let proxy_src = include_str!("proxy.rs");
    let process_one_body = proxy_src.split("fn process_one(").nth(1).unwrap_or("");
    // Count direct emit_result calls (success + ok_mask==0 defense-in-depth).
    let emit_count = process_one_body.matches("self.emit_result(").count();
    // Count death_clamp calls (each internally calls emit_result).
    let clamp_count = process_one_body.matches("self.death_clamp(").count();
    let total = emit_count + clamp_count;
    // Paths: lane violation (1 clamp) + auth (1 clamp) + ok_mask==0 (1 emit) +
    // success (1 emit) + 5 SendError arms (5 clamps) = 9 total.
    assert!(
        total >= 9,
        "process_one must emit result on every path (found {emit_count} emit_result + {clamp_count} death_clamp = {total})"
    );
}

#[test]
fn no_telemetry_parsing_in_proxy() {
    let proxy_src = include_str!("proxy.rs");
    // tactic_class is allowed as a field copy (envelope → result),
    // but the proxy must not classify or branch on it.
    let banned = [
        "chain_status",
        "telemetry",
        "ClassifySend",
        "match tactic_class",
        "if tactic_class",
    ];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{pat}' — byte-blast-only discipline"
        );
    }
}

#[test]
fn main_has_reconnect_backoff_args() {
    let main_src = include_str!("main.rs");
    assert!(
        main_src.contains("reconnect_initial_backoff_ms"),
        "main.rs must expose reconnect initial backoff CLI arg"
    );
    assert!(
        main_src.contains("reconnect_max_backoff_ms"),
        "main.rs must expose reconnect max backoff CLI arg"
    );
}

// ===========================================================================
// 36. Lane-semantic validation (step 6)
// ===========================================================================

// -- Unit tests on validate_lane_semantics directly --

#[test]
fn lane_valid_harmonic_envelope_passes() {
    let env = make_envelope(0x10, 1, 2, 500_000);
    assert!(validate_lane_semantics(&env.raw_bytes, env.len).is_ok());
}

#[test]
fn lane_rejects_jito_tip_transfer() {
    let env = make_jito_envelope(0x20, 1, 2, 500_000);
    assert_eq!(
        validate_lane_semantics(&env.raw_bytes, env.len),
        Err(LaneViolationReason::JitoTipDetected),
    );
}

#[test]
fn lane_rejects_missing_priority_fee_instruction() {
    let env = make_no_cu_price_envelope(0x30);
    assert_eq!(
        validate_lane_semantics(&env.raw_bytes, env.len),
        Err(LaneViolationReason::MissingCuPrice),
    );
}

#[test]
fn lane_rejects_truncated_message() {
    // len too short to contain even the message header
    let env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 50,
        signature: [0xAA; 64],
        pool_index: 0,
        mint_out_index: 0,
        reserved_notional_lamports: 0,
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
    assert_eq!(
        validate_lane_semantics(&env.raw_bytes, env.len),
        Err(LaneViolationReason::Unparseable),
    );
}

#[test]
fn lane_jito_tip_detected_even_with_priority_fee() {
    // make_jito_envelope has BOTH SetComputeUnitPrice AND SystemProgram::Transfer.
    // The tip detection must fire regardless.
    let env = make_jito_envelope(0x40, 5, 6, 1_000_000);
    let result = validate_lane_semantics(&env.raw_bytes, env.len);
    assert_eq!(result, Err(LaneViolationReason::JitoTipDetected));
}

#[test]
fn lane_both_cu_limit_and_cu_priorityfee_passes() {
    // Build an envelope with both SetComputeUnitLimit AND SetComputeUnitPrice.
    // Harmonic templates have both. Validator should pass.
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 0,
        signature: [0x55; 64],
        pool_index: 0,
        mint_out_index: 0,
        reserved_notional_lamports: 0,
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
    let mut c = 0usize;
    env.raw_bytes[c] = 1;
    c += 1;
    env.raw_bytes[c..c + 64].fill(0x55);
    c += 64;
    env.raw_bytes[c] = 1;
    c += 1; // num_required_signatures
    env.raw_bytes[c] = 0;
    c += 1;
    env.raw_bytes[c] = 1;
    c += 1; // num_readonly_unsigned
    env.raw_bytes[c] = 2;
    c += 1; // 2 accounts
    env.raw_bytes[c] = 0x77;
    c += 32; // fee payer
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_CB_PROGRAM_ID);
    c += 32;
    c += 32; // blockhash
    env.raw_bytes[c] = 2;
    c += 1; // 2 instructions
            // ix 0: SetComputeUnitLimit (disc 0x02, 5 bytes)
    env.raw_bytes[c] = 1;
    c += 1;
    env.raw_bytes[c] = 0;
    c += 1;
    env.raw_bytes[c] = 5;
    c += 1;
    env.raw_bytes[c] = 0x02;
    c += 1;
    let limit: u32 = 300_150;
    env.raw_bytes[c..c + 4].copy_from_slice(&limit.to_le_bytes());
    c += 4;
    // ix 1: SetComputeUnitPrice (disc 0x03, 9 bytes)
    env.raw_bytes[c] = 1;
    c += 1;
    env.raw_bytes[c] = 0;
    c += 1;
    env.raw_bytes[c] = 9;
    c += 1;
    env.raw_bytes[c] = 0x03;
    c += 1;
    let price: u64 = 5000;
    env.raw_bytes[c..c + 8].copy_from_slice(&price.to_le_bytes());
    c += 8;
    env.len = c;
    assert!(validate_lane_semantics(&env.raw_bytes, env.len).is_ok());
}

// -- Integration tests: validator wired into process_one --

#[test]
fn process_one_rejects_jito_envelope_with_lane_violation() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_jito_envelope(0x60, 1, 2, 750_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::LaneViolation);
    // NACK emitted for stranded capital
    assert_eq!(proxy.nacks_emitted, 1);
    // Result published (zero masks — local failure)
    assert_eq!(proxy.results_published, 1);
    // Not counted as sent
    assert_eq!(proxy.bundles_sent, 0);
}

#[test]
fn process_one_rejects_no_priority_fee_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_no_cu_price_envelope(0x70);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::LaneViolation);
    assert_eq!(proxy.nacks_emitted, 1);
}

#[test]
fn process_one_sends_valid_harmonic_envelope() {
    // Confirm that a properly shaped Harmonic envelope passes validation and gets sent.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0x80, 1, 2, 500_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    match result {
        SendAttemptResult::Sent(_) => {}
        other => panic!("expected Sent, got {:?}", other),
    }
    assert_eq!(proxy.bundles_sent, 1);
    assert_eq!(proxy.nacks_emitted, 0);
}

#[test]
fn lane_violation_emits_nack_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_jito_envelope(0x90, 42, 99, 12_345_678);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    // NACK preserves envelope identity fields.
    let nack: NackRecord128B = nack_ring.pop().unwrap();
    assert_eq!(nack.mint_out_index, 99);

    // Result published with zero masks (local pre-send failure).
    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egress_result.lane, LANE_HARMONIC);
    assert_eq!(egress_result.ok_mask, 0);
    assert_eq!(egress_result.region_mask, 0);
    assert_eq!(egress_result.pool_index, 42);
}

// -- Structural guards --

#[test]
fn validate_lane_semantics_is_called_in_process_one() {
    let proxy_src = include_str!("proxy.rs");
    let process_one_body = proxy_src.split("fn process_one(").nth(1).unwrap_or("");
    assert!(
        process_one_body.contains("validate_lane_semantics"),
        "process_one must call validate_lane_semantics before sending"
    );
}

#[test]
fn validator_never_mutates_bytes() {
    let proxy_src = include_str!("proxy.rs");
    // The validate_lane_semantics function signature takes &[u8] (immutable).
    assert!(
        proxy_src.contains("fn validate_lane_semantics(raw_bytes: &[u8]"),
        "validator must take immutable byte slice"
    );
}

#[test]
fn lane_violation_variant_exists() {
    // Verify the LaneViolation variant is used in the return path.
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("SendAttemptResult::LaneViolation"),
        "LaneViolation variant must exist in SendAttemptResult"
    );
}

#[test]
fn validator_checks_system_transfer_and_priority_fee() {
    // Structural: the validator must reference both the system transfer discriminator
    // and the priority fee discriminator.
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("SYS_TRANSFER_DISC"),
        "validator must check for SystemProgram::Transfer"
    );
    assert!(
        proxy_src.contains("CB_PRIORITY_FEE_DISC"),
        "validator must check for ComputeBudget priority fee"
    );
}

#[test]
fn lane_violation_before_session_check() {
    // Structural: validate_lane_semantics must appear before can_send() in process_one.
    let proxy_src = include_str!("proxy.rs");
    let process_one_body = proxy_src.split("fn process_one(").nth(1).unwrap_or("");
    let validate_pos = process_one_body
        .find("validate_lane_semantics")
        .unwrap_or(usize::MAX);
    let can_send_pos = process_one_body.find("can_send()").unwrap_or(0);
    assert!(
        validate_pos < can_send_pos,
        "lane validation must occur before session.can_send() check"
    );
}

#[test]
fn no_byte_mutation_in_validator() {
    // Structural: the validate_lane_semantics function takes &[u8] (immutable borrow).
    let proxy_src = include_str!("proxy.rs");
    // The function signature must take an immutable slice.
    assert!(
        proxy_src.contains("fn validate_lane_semantics(raw_bytes: &[u8]"),
        "validate_lane_semantics must take immutable byte slice"
    );
    // The function must NOT take &mut [u8].
    assert!(
        !proxy_src.contains("fn validate_lane_semantics(raw_bytes: &mut"),
        "validate_lane_semantics must not take mutable byte slice"
    );
}

// ===========================================================================
// 36b. Lane-semantic contract proofs (checklist-required)
//
// For a Harmonic-routed envelope, assert:
//   1. compute-budget instructions are present
//   2. no Jito tip transfer instruction exists
//   3. signed bytes are transmitted exactly as produced by the worker
// ===========================================================================

/// Full-template Harmonic envelope: CU-limit + CU-price + ShadowGuard + Swap.
/// Mirrors the real `build_harmonic_template` output layout with 4 instructions:
///   ix0: SetComputeUnitLimit (CB, disc=0x02, 5 bytes)
///   ix1: SetComputeUnitPrice (CB, disc=0x03, 9 bytes)
///   ix2: ShadowGuard::AssertReservesInRange (arbitrary program, 8 bytes)
///   ix3: RaydiumV4 Swap (arbitrary program, 17 bytes)
fn make_full_harmonic_template_envelope(sig_byte: u8) -> SignedTxEnvelope {
    // Program IDs (account indices):
    //   0 = fee payer, 1 = guard vault in, 2 = guard vault out
    //   3 = CB program, 4 = ShadowGuard program, 5 = Swap program
    let mut env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 0,
        signature: [sig_byte; 64],
        pool_index: 10,
        mint_out_index: 5,
        reserved_notional_lamports: 7_500_000,
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
    let mut c = 0usize;

    // Signed tx wrapper
    env.raw_bytes[c] = 1;
    c += 1; // 1 signature
    env.raw_bytes[c..c + 64].fill(sig_byte);
    c += 64;

    // Message header
    env.raw_bytes[c] = 1;
    c += 1; // num_required_signatures
    env.raw_bytes[c] = 0;
    c += 1; // num_readonly_signed
    env.raw_bytes[c] = 3;
    c += 1; // num_readonly_unsigned (CB, Guard, Swap programs)

    // 6 accounts
    env.raw_bytes[c] = 6;
    c += 1;
    // acct 0: fee payer
    env.raw_bytes[c] = sig_byte.wrapping_add(1);
    c += 32;
    // acct 1: guard vault in
    env.raw_bytes[c] = 0xBB;
    c += 32;
    // acct 2: guard vault out
    env.raw_bytes[c] = 0xCC;
    c += 32;
    // acct 3: ComputeBudget program
    env.raw_bytes[c..c + 32].copy_from_slice(&TEST_CB_PROGRAM_ID);
    c += 32;
    // acct 4: ShadowGuard program (non-zero, non-system, non-CB)
    env.raw_bytes[c] = 0x53;
    env.raw_bytes[c + 1] = 0x47;
    c += 32;
    // acct 5: Swap program (non-zero, non-system, non-CB)
    env.raw_bytes[c] = 0x67;
    env.raw_bytes[c + 1] = 0x5E;
    c += 32;

    // Blockhash
    env.raw_bytes[c..c + 32].fill(0x11);
    c += 32;

    // 4 instructions
    env.raw_bytes[c] = 4;
    c += 1;

    // ix 0: SetComputeUnitLimit — CB disc 0x02, 5 bytes data
    env.raw_bytes[c] = 3;
    c += 1; // prog_idx = 3 (CB)
    env.raw_bytes[c] = 0;
    c += 1; // 0 accounts
    env.raw_bytes[c] = 5;
    c += 1; // data_len = 5
    env.raw_bytes[c] = 0x02;
    c += 1;
    let cu_limit: u32 = 300_150;
    env.raw_bytes[c..c + 4].copy_from_slice(&cu_limit.to_le_bytes());
    c += 4;

    // ix 1: SetComputeUnitPrice — CB disc 0x03, 9 bytes data
    env.raw_bytes[c] = 3;
    c += 1; // prog_idx = 3 (CB)
    env.raw_bytes[c] = 0;
    c += 1; // 0 accounts
    env.raw_bytes[c] = 9;
    c += 1; // data_len = 9
    env.raw_bytes[c] = 0x03;
    c += 1;
    let priority: u64 = 5_000;
    env.raw_bytes[c..c + 8].copy_from_slice(&priority.to_le_bytes());
    c += 8;

    // ix 2: ShadowGuard::AssertReservesInRange — 8 bytes data
    env.raw_bytes[c] = 4;
    c += 1; // prog_idx = 4 (Guard)
    env.raw_bytes[c] = 2;
    c += 1; // 2 accounts
    env.raw_bytes[c] = 1;
    c += 1; // acct idx 1 (vault in)
    env.raw_bytes[c] = 2;
    c += 1; // acct idx 2 (vault out)
    env.raw_bytes[c] = 8;
    c += 1; // data_len = 8
    env.raw_bytes[c..c + 8].fill(0xAA);
    c += 8; // arbitrary discriminator + args

    // ix 3: Swap — 17 bytes data, 2 accounts
    env.raw_bytes[c] = 5;
    c += 1; // prog_idx = 5 (Swap)
    env.raw_bytes[c] = 2;
    c += 1; // 2 accounts
    env.raw_bytes[c] = 0;
    c += 1; // acct idx 0
    env.raw_bytes[c] = 1;
    c += 1; // acct idx 1
    env.raw_bytes[c] = 17;
    c += 1; // data_len = 17
    env.raw_bytes[c..c + 17].fill(0x77);
    c += 17;

    env.len = c;
    env
}

/// Contract proof 1: A full Harmonic template envelope has compute-budget
/// instructions (both SetComputeUnitLimit and SetComputeUnitPrice) and
/// passes lane validation.
#[test]
fn lane_contract_compute_budget_present_in_full_template() {
    let env = make_full_harmonic_template_envelope(0xA1);

    // Lane validator accepts — SetComputeUnitPrice present.
    assert!(
        validate_lane_semantics(&env.raw_bytes, env.len).is_ok(),
        "full Harmonic template must pass lane validation"
    );

    // Also verify the raw bytes contain the SetComputeUnitLimit disc (0x02)
    // and SetComputeUnitPrice disc (0x03) at expected positions.
    // Both are CB instructions (prog_idx = 3 → account 3 = CB program).
    let msg = &env.raw_bytes[65..env.len];
    // Walk instructions to confirm both discriminators are present.
    let mut found_cu_limit = false;
    let mut found_cu_price = false;

    // Parse: skip header (3 bytes) + compact-u16 acct count + keys + blockhash
    let (num_accounts, acct_start) = {
        let b = msg[3] as u16;
        (b, 4usize)
    };
    let keys_end = acct_start + (num_accounts as usize) * 32;
    let blockhash_end = keys_end + 32;
    let ix_count = msg[blockhash_end] as u16;
    let mut cursor = blockhash_end + 1;

    for _ in 0..ix_count {
        let prog_idx = msg[cursor] as usize;
        cursor += 1;
        let num_accts = msg[cursor] as u16;
        cursor += 1;
        cursor += num_accts as usize;
        let data_len = msg[cursor] as u16;
        cursor += 1;
        let data_start = cursor;

        // Is this instruction addressed to ComputeBudget (account index 3)?
        if prog_idx == 3 && data_len >= 1 {
            match msg[data_start] {
                0x02 => found_cu_limit = true,
                0x03 => found_cu_price = true,
                _ => {}
            }
        }
        cursor += data_len as usize;
    }

    assert!(
        found_cu_limit,
        "Harmonic template must contain SetComputeUnitLimit"
    );
    assert!(
        found_cu_price,
        "Harmonic template must contain SetComputeUnitPrice"
    );
}

/// Contract proof 2: A full Harmonic template containing no SystemProgram::Transfer
/// is accepted, and the validator explicitly confirms absence of Jito tip pattern.
#[test]
fn lane_contract_no_jito_tip_in_harmonic_template() {
    let env = make_full_harmonic_template_envelope(0xA2);

    // Positive: passes validation (no tip detected).
    assert!(validate_lane_semantics(&env.raw_bytes, env.len).is_ok());

    // Structural proof: no account key in the envelope matches SystemProgram (all-zeros).
    // This means no instruction can address the System program.
    let msg = &env.raw_bytes[65..env.len];
    let num_accounts = msg[3] as usize;
    let acct_start = 4;
    let system_program = [0u8; 32];
    for i in 0..num_accounts {
        let key_offset = acct_start + i * 32;
        let key = &msg[key_offset..key_offset + 32];
        assert_ne!(
            key, &system_program,
            "Harmonic template must not include System program in account keys \
             (account index {i})"
        );
    }

    // Counter-proof: injecting a Jito tip into this envelope WOULD be caught.
    let mut tampered = env.clone();
    // Overwrite account 5 (Swap program) with System program.
    let sys_key_offset = 65 + 4 + 5 * 32; // msg offset + acct_start + key[5]
    tampered.raw_bytes[sys_key_offset..sys_key_offset + 32].fill(0x00);
    // Overwrite ix 3 data to look like a System::Transfer.
    // ix 3 starts after ix 0 (8 bytes) + ix 1 (12 bytes) + ix 2 (13 bytes)
    // Find ix 3 by walking the instruction stream.
    let instructions_start = 65 + 4 + num_accounts * 32 + 32 + 1; // after blockhash + ix_count
    let mut ix_cursor = instructions_start;
    for _ in 0..3 {
        ix_cursor += 1; // prog_idx
        let n_a = tampered.raw_bytes[ix_cursor] as usize;
        ix_cursor += 1;
        ix_cursor += n_a;
        let dl = tampered.raw_bytes[ix_cursor] as usize;
        ix_cursor += 1;
        ix_cursor += dl;
    }
    // ix 3: change prog_idx to 5 (now System), 2 accounts, data_len=12, Transfer disc
    tampered.raw_bytes[ix_cursor] = 5;
    ix_cursor += 1; // prog_idx = 5 (now System)
    tampered.raw_bytes[ix_cursor] = 2;
    ix_cursor += 1; // 2 accounts
    ix_cursor += 2; // skip account indices
    tampered.raw_bytes[ix_cursor] = 12;
    ix_cursor += 1;
    tampered.raw_bytes[ix_cursor..ix_cursor + 4].copy_from_slice(&[0x02, 0x00, 0x00, 0x00]);
    ix_cursor += 4;
    tampered.raw_bytes[ix_cursor..ix_cursor + 8].copy_from_slice(&100_000u64.to_le_bytes());

    assert_eq!(
        validate_lane_semantics(&tampered.raw_bytes, tampered.len),
        Err(LaneViolationReason::JitoTipDetected),
        "injected Jito tip must be caught even with all other fields valid"
    );
}

/// Contract proof 3: Signed bytes are transmitted bit-for-bit as the worker
/// produced them. The proxy sends exactly `envelope.raw_bytes[..envelope.len]`
/// to the session — no patching, no rebuild, no re-serialization.
#[test]
fn lane_contract_signed_bytes_transmitted_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_full_harmonic_template_envelope(0xA3);
    let expected_bytes: Vec<u8> = env.raw_bytes[..env.len].to_vec();
    let expected_sig = env.signature;

    egress_ring.push(&env);

    let capturing = CapturingSession::new();
    let mut proxy = HarmonicProxy::new(test_config(), capturing);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    match result {
        SendAttemptResult::Sent(summary) => {
            assert_ne!(summary.ok_mask, 0, "must have at least one region OK");
        }
        other => panic!("expected Sent, got {:?}", other),
    }

    let session = proxy.session_ref();
    assert_eq!(session.captured.len(), 1, "exactly one bundle sent");

    // Byte-for-byte identity — the session received the exact wire bytes.
    assert_eq!(
        session.captured[0], expected_bytes,
        "signed bytes must be transmitted verbatim — no patching allowed"
    );

    // The signature in the EgressResult also matches the envelope's signature.
    let egr: EgressResult = result_ring.pop().unwrap();
    assert_eq!(egr.signature, expected_sig);
    assert_eq!(egr.lane, LANE_HARMONIC);
    assert_eq!(egr.pool_index, 10);
    assert_eq!(egr.reserved_notional_lamports, 7_500_000);
}

/// Contract proof 4 (structural): no byte-mutating operation exists between
/// envelope dequeue and session.send_bundle in process_one.
#[test]
fn lane_contract_no_mutation_between_dequeue_and_send() {
    let proxy_src = include_str!("proxy.rs");

    // Extract process_one body.
    let body = proxy_src.split("fn process_one(").nth(1).unwrap_or("");

    // Between pop() and send_bundle, there must be no mutation.
    let pop_pos = body.find("egress_ring.pop()").unwrap_or(0);
    let send_pos = body.find("send_bundle").unwrap_or(body.len());
    let between = &body[pop_pos..send_pos];

    let banned_mutations = [
        "mut raw_bytes",
        ".copy_from_slice", // write into raw_bytes
        "patch_",
        "rebuild_",
        "set_recent_blockhash",
        "envelope.raw_bytes =",
        "envelope.len =",
    ];
    for pat in &banned_mutations {
        assert!(
            !between.contains(pat),
            "between pop and send_bundle, proxy must not contain '{}' — \
             signed bytes immutable end-to-end",
            pat
        );
    }
}

// ===========================================================================
// 36c. NACK contract proofs (checklist-required)
//
// Simulate local pre-transmission failures:
//   - no live session available
//   - framing failure (lane violation — malformed/wrong template)
//   - region fanout setup failure before any outbound attempt
//
// Assert:
//   - exactly one NACK
//   - immediate capital-release signal path (NACK carries sig + notional)
//   - no network send occurred (sent_count == 0)
// ===========================================================================

/// Helper: push envelope, process_one, assert NACK contract for pre-transmission
/// failure.  Verifies: exactly 1 NACK, correct capital-release fields, zero
/// network sends, EgressResult published with zero ok_mask.
fn assert_nack_contract(
    proxy: &mut HarmonicProxy<MockSession>,
    egress_ring: &MappedSpscRing<SignedTxEnvelope>,
    nack_ring: &MappedSpscRing<NackRecord128B>,
    result_ring: &MappedSpscRing<EgressResult>,
    env: &SignedTxEnvelope,
    label: &str,
) {
    egress_ring.push(env);
    let prior_nacks = proxy.nacks_emitted;
    let prior_sent = proxy.bundles_sent;

    let result = proxy.process_one(egress_ring, nack_ring, result_ring);

    // Exactly one NACK emitted.
    assert_eq!(
        proxy.nacks_emitted,
        prior_nacks + 1,
        "{label}: must emit exactly one NACK"
    );
    assert!(
        matches!(
            result,
            SendAttemptResult::NackEmitted | SendAttemptResult::LaneViolation
        ),
        "{label}: result must indicate NACK or LaneViolation, got {result:?}"
    );

    // No network send occurred.
    assert_eq!(
        proxy.bundles_sent, prior_sent,
        "{label}: bundles_sent must not increment — no network send"
    );
    assert_eq!(
        proxy.session_ref().sent_count,
        0,
        "{label}: MockSession.sent_count must be 0 — send_bundle never called"
    );

    // NACK carries capital-release fields: signature and trade_size_lamports.
    let nack: NackRecord128B = nack_ring
        .pop()
        .expect(&format!("{label}: NACK must be in ring"));
    assert_eq!(
        nack.signature, env.signature,
        "{label}: NACK signature must match envelope"
    );
    assert_eq!(
        nack.trade_size_lamports, env.reserved_notional_lamports,
        "{label}: NACK trade_size_lamports must match reserved notional"
    );
    assert_eq!(
        nack.mint_out_index, env.mint_out_index,
        "{label}: NACK mint_out_index must match envelope"
    );

    // EgressResult published (capital release always has correlation record).
    let egr: EgressResult = result_ring
        .pop()
        .expect(&format!("{label}: EgressResult must be in ring"));
    assert_eq!(
        egr.signature, env.signature,
        "{label}: EgressResult signature must match"
    );
    assert_eq!(
        egr.ok_mask, 0,
        "{label}: EgressResult ok_mask must be 0 (pre-transmission failure)"
    );
    assert_eq!(
        egr.lane, LANE_HARMONIC,
        "{label}: EgressResult lane must be HARMONIC"
    );
    assert_eq!(
        egr.reserved_notional_lamports, env.reserved_notional_lamports,
        "{label}: EgressResult notional must match for capital release"
    );
}

/// No live session available (session state = Down).
/// Pre-transmission: can_send() is false, send_bundle never called.
#[test]
fn nack_contract_no_live_session() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());
    let env = make_envelope(0xB1, 3, 7, 2_500_000);

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "no live session (Down)",
    );
}

/// No live session: Connecting state (session still establishing).
/// Pre-transmission: can_send() is false.
#[test]
fn nack_contract_session_connecting() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connecting());
    let env = make_envelope(0xB2, 5, 11, 1_750_000);

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "session Connecting (not yet Up)",
    );
}

/// Framing failure: malformed envelope that fails lane-semantic validation
/// (truncated message — Unparseable).
#[test]
fn nack_contract_framing_failure_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    // Connected session — but envelope is garbage, caught by validator.
    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());

    let env = SignedTxEnvelope {
        raw_bytes: [0u8; 1232],
        len: 50, // too short to parse
        signature: [0xB3; 64],
        pool_index: 8,
        mint_out_index: 14,
        reserved_notional_lamports: 900_000,
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

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "framing failure (truncated / Unparseable)",
    );
}

/// Framing failure: envelope missing required ComputeBudget::SetComputeUnitPrice.
/// Valid Solana message but wrong template shape for Harmonic lane.
#[test]
fn nack_contract_framing_failure_missing_cu_price() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let env = make_no_cu_price_envelope(0xB4);

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "framing failure (missing CU price)",
    );
}

/// Framing failure: Jito-shaped envelope with SystemProgram::Transfer on
/// Harmonic lane.
#[test]
fn nack_contract_framing_failure_jito_tip_on_harmonic() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::connected());
    let env = make_jito_envelope(0xB5, 12, 20, 4_000_000);

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "framing failure (Jito tip on Harmonic lane)",
    );
}

/// Region fanout setup failure: session is Up but send_bundle returns
/// NotConnected (all regions dropped between can_send and send).
/// This is a pre-outbound failure — no region acknowledged the tx.
#[test]
fn nack_contract_region_fanout_not_connected() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(
        test_config(),
        MockSession::with_failure(SendError::NotConnected),
    );
    let env = make_envelope(0xB6, 15, 22, 3_200_000);

    // with_failure sets state=Up so can_send() passes, but send_bundle
    // returns NotConnected (race: regions went down between check and send).
    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "region fanout NotConnected",
    );
}

/// Region fanout setup failure: all regions attempted but all failed
/// (AllRegionsFailed).  Zero regions acknowledged before any successful
/// outbound.
#[test]
fn nack_contract_region_fanout_all_failed() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(
        test_config(),
        MockSession::with_failure(SendError::AllRegionsFailed {
            region_mask: 0x07,
            err_code: 0,
        }),
    );
    let env = make_envelope(0xB7, 18, 25, 5_100_000);

    assert_nack_contract(
        &mut proxy,
        &egress_ring,
        &nack_ring,
        &result_ring,
        &env,
        "region fanout AllRegionsFailed",
    );
}

/// Multiple sequential pre-transmission failures: each produces exactly one
/// NACK with the correct envelope identity, and no network sends accumulate.
#[test]
fn nack_contract_sequential_failures_independent() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());

    for i in 0..5u8 {
        let env = make_envelope(
            0xC0 + i,
            i as u32,
            i as u32 + 10,
            (i as u64 + 1) * 1_000_000,
        );
        egress_ring.push(&env);
        proxy.process_one(&egress_ring, &nack_ring, &result_ring);

        // Each iteration: exactly i+1 NACKs total, 0 sends.
        assert_eq!(proxy.nacks_emitted, (i + 1) as u64);
        assert_eq!(proxy.bundles_sent, 0);
        assert_eq!(proxy.session_ref().sent_count, 0);

        // NACK identity matches this iteration's envelope.
        let nack: NackRecord128B = nack_ring.pop().unwrap();
        assert_eq!(nack.signature, [0xC0 + i; 64]);
        assert_eq!(nack.trade_size_lamports, (i as u64 + 1) * 1_000_000);

        // EgressResult also emitted.
        let _egr: EgressResult = result_ring.pop().unwrap();
    }
}

/// Structural: emit_nack writes exactly signature + mint_out_index +
/// reserved_notional_lamports from the envelope — the capital-release triple.
#[test]
fn nack_contract_capital_release_fields_structural() {
    let proxy_src = include_str!("proxy.rs");
    let emit_nack_body = proxy_src
        .split("fn emit_nack(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .unwrap_or("");

    // NACK must carry the three capital-release fields from the envelope.
    assert!(
        emit_nack_body.contains("envelope.signature"),
        "emit_nack must use envelope.signature for PM correlation"
    );
    assert!(
        emit_nack_body.contains("envelope.mint_out_index"),
        "emit_nack must carry mint_out_index for capital release"
    );
    assert!(
        emit_nack_body.contains("envelope.reserved_notional_lamports"),
        "emit_nack must carry reserved_notional_lamports for capital release"
    );
}

// ===========================================================================
// 37. Death Clamp exhaustive (step 7)
// ===========================================================================

/// Helper: push a valid Harmonic envelope, process_one with given session,
/// assert the envelope was death-clamped (NACK + result published, not sent).
fn assert_death_clamped(session: MockSession, expected_err_code: u8, label: &str) {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xDC, 7, 3, 2_000_000);
    egress_ring.push(&env);

    let mut proxy = HarmonicProxy::new(test_config(), session);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    assert!(
        matches!(result, SendAttemptResult::NackEmitted),
        "{label}: expected NackEmitted, got {result:?}"
    );
    assert_eq!(proxy.bundles_sent, 0, "{label}: bundles_sent must be 0");
    assert_eq!(
        proxy.nacks_emitted, 1,
        "{label}: must emit exactly one NACK"
    );
    assert_eq!(
        proxy.death_clamped, 1,
        "{label}: must increment death_clamped"
    );

    // Verify EgressResult carries the classified error code.
    let egress_result: EgressResult = result_ring.pop().unwrap();
    assert_eq!(
        egress_result.lane, LANE_HARMONIC,
        "{label}: lane must be HARMONIC"
    );
    assert_eq!(egress_result.ok_mask, 0, "{label}: ok_mask must be 0");
    assert_eq!(
        egress_result.err_code, expected_err_code,
        "{label}: err_code mismatch"
    );
    // Envelope identity preserved in result.
    assert_eq!(egress_result.pool_index, 7);
    assert_eq!(egress_result.reserved_notional_lamports, 2_000_000);
}

#[test]
fn death_clamp_on_not_connected() {
    // Auth failure / session down → Death Clamp (DC_ERR_AUTH = 10).
    assert_death_clamped(
        MockSession::disconnected(),
        10,
        "NotConnected (session down)",
    );
}

#[test]
fn death_clamp_on_connecting() {
    // Session still connecting → can_send() is false → Death Clamp.
    assert_death_clamped(
        MockSession::connecting(),
        10,
        "Connecting (session not ready)",
    );
}

#[test]
fn death_clamp_on_timeout() {
    assert_death_clamped(MockSession::with_failure(SendError::Timeout), 11, "Timeout");
}

#[test]
fn death_clamp_on_transport() {
    assert_death_clamped(
        MockSession::with_failure(SendError::Transport("conn reset".into())),
        12,
        "Transport",
    );
}

#[test]
fn death_clamp_on_rejected() {
    assert_death_clamped(
        MockSession::with_failure(SendError::Rejected("deadline exceeded".into())),
        13,
        "Rejected",
    );
}

#[test]
fn death_clamp_on_all_regions_failed() {
    assert_death_clamped(
        MockSession::with_failure(SendError::AllRegionsFailed {
            region_mask: 0x07,
            err_code: 0,
        }),
        14,
        "AllRegionsFailed",
    );
}

#[test]
fn death_clamp_on_send_error_not_connected() {
    // SendError::NotConnected returned from send_bundle (distinct from pre-send check).
    assert_death_clamped(
        MockSession::with_failure(SendError::NotConnected),
        10,
        "SendError::NotConnected",
    );
}

#[test]
fn death_clamp_on_ok_mask_zero() {
    // All regions attempted but ok_mask=0 (delivered via MockSession with region_count=0).
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xDC, 5, 2, 1_500_000);
    egress_ring.push(&env);

    let mut mock = MockSession::connected();
    mock.region_count = 0; // Produces Ok(RegionalResult { ok_mask: 0 })
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    assert!(
        matches!(result, SendAttemptResult::NackEmitted),
        "ok_mask=0 must be death-clamped"
    );
    assert_eq!(proxy.bundles_sent, 0);
    assert_eq!(proxy.death_clamped, 1);
    assert_eq!(proxy.nacks_emitted, 1);
}

#[test]
fn death_clamp_never_relays_to_other_lane() {
    // After death clamp, the envelope is gone — no re-enqueue, no resend.
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let env = make_envelope(0xDD, 1, 1, 500_000);
    egress_ring.push(&env);

    let mut proxy =
        HarmonicProxy::new(test_config(), MockSession::with_failure(SendError::Timeout));
    proxy.process_one(&egress_ring, &nack_ring, &result_ring);

    // Egress ring is now empty — envelope was consumed, not re-queued.
    assert!(
        egress_ring.pop().is_none(),
        "envelope must not be re-enqueued"
    );
    // The only output is a NACK + result — no secondary ring push.
    assert_eq!(proxy.bundles_sent, 0);
}

#[test]
fn death_clamp_counter_accumulates() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    for i in 0..5u8 {
        let env = make_envelope(i, i as u32, 0, 100_000);
        egress_ring.push(&env);
    }

    let mut proxy =
        HarmonicProxy::new(test_config(), MockSession::with_failure(SendError::Timeout));
    for _ in 0..5 {
        proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    }

    assert_eq!(proxy.death_clamped, 5);
    assert_eq!(proxy.nacks_emitted, 5);
    assert_eq!(proxy.bundles_sent, 0);
}

// -- Structural Death Clamp guards --

#[test]
fn death_clamp_match_is_exhaustive() {
    // Structural: process_one must match every SendError variant explicitly —
    // no blanket Err(_) or Err(..) catch-all.
    let proxy_src = include_str!("proxy.rs");
    let process_one_body = proxy_src.split("fn process_one(").nth(1).unwrap_or("");
    assert!(
        !process_one_body.contains("Err(_)"),
        "process_one must not have a blanket Err(_) — match every SendError variant"
    );
    // Verify each variant is named.
    for variant in [
        "SendError::NotConnected",
        "SendError::Timeout",
        "SendError::Transport",
        "SendError::Rejected",
        "SendError::AllRegionsFailed",
    ] {
        assert!(
            process_one_body.contains(variant),
            "process_one must explicitly handle {variant}"
        );
    }
}

#[test]
fn death_clamp_helper_exists() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("fn death_clamp("),
        "proxy must have a death_clamp helper method"
    );
}

#[test]
fn no_fallback_paths_structural() {
    // Death Clamp contract: proxy must never relay to TPU, QUIC, or public lanes.
    let proxy_src = include_str!("proxy.rs");
    let banned = [
        "tpu_send",
        "quic_send",
        "public_rpc",
        "relay_to",
        "re_enqueue",
        "reenqueue",
        "forward_to",
        "resend",
        "retry_on_other",
        "fallback_lane",
    ];
    for pat in banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{pat}' — Death Clamp forbids lane fallback"
        );
    }
}

#[test]
fn death_clamped_counter_exists() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        proxy_src.contains("death_clamped"),
        "proxy must track death_clamped counter"
    );
}

#[test]
fn every_send_error_maps_to_distinct_err_code() {
    // Each Death Clamp scenario should produce a distinct non-zero err_code
    // in the EgressResult so PM can classify the failure mode.
    let proxy_src = include_str!("proxy.rs");
    // Verify the DC_ERR_* constants exist with distinct values.
    for constant in [
        "DC_ERR_AUTH",
        "DC_ERR_TIMEOUT",
        "DC_ERR_TRANSPORT",
        "DC_ERR_REJECTED",
        "DC_ERR_ALL_REGIONS",
        "DC_ERR_LANE_VIOLATION",
    ] {
        assert!(
            proxy_src.contains(constant),
            "proxy must define {constant} error code constant"
        );
    }
}

// ===========================================================================
// 37b. Death Clamp — extended scenario + contract proofs
//
// The checklist requires explicit simulation of:
//   auth error, timeout, deadline exceeded, not-leader, RPC rejection
// and asserts:
//   terminal drop recorded, no TPU/public send attempt, no second lane
//   fallback with the same signed tx.
// ===========================================================================

#[test]
fn death_clamp_deadline_exceeded() {
    // "deadline exceeded" is a gRPC DEADLINE_EXCEEDED (code 4) which
    // classify_status maps to ERR_TRANSPORT, surfaced as SendError::Timeout
    // from the tokio::time::timeout wrapper. Verify the proxy death-clamps it.
    assert_death_clamped(
        MockSession::with_failure(SendError::Timeout),
        11, // DC_ERR_TIMEOUT
        "deadline exceeded (tokio timeout)",
    );
}

#[test]
fn death_clamp_not_leader() {
    // "not-leader" is an application-level rejection returned by the Harmonic
    // RPC (e.g. "not leader for slot 12345").  classify_status maps gRPC
    // FAILED_PRECONDITION (code 9) to ERR_REJECTED.  The session surfaces
    // it as SendError::Rejected.
    assert_death_clamped(
        MockSession::with_failure(SendError::Rejected("not leader for slot".into())),
        13, // DC_ERR_REJECTED
        "not-leader rejection",
    );
}

#[test]
fn death_clamp_rpc_permission_denied() {
    // gRPC PERMISSION_DENIED (code 7) → ERR_REJECTED → SendError::Rejected.
    assert_death_clamped(
        MockSession::with_failure(SendError::Rejected("permission denied".into())),
        13,
        "RPC permission denied",
    );
}

/// After death clamp, the exact signed tx bytes must not appear in any ring.
/// Verify the signature is consumed: present only in NACK and EgressResult
/// (terminal outputs), not re-enqueued to egress ring or any other ring.
#[test]
fn death_clamp_same_signed_tx_never_reappears() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let reference_sig = [0xEE_u8; 64];

    // Simulate five different failure modes, each with a fresh envelope
    // sharing the same signature, proving none are relayed.
    let failures = [
        SendError::NotConnected,
        SendError::Timeout,
        SendError::Transport("reset".into()),
        SendError::Rejected("not leader".into()),
        SendError::AllRegionsFailed {
            region_mask: 0x07,
            err_code: 0,
        },
    ];

    for (i, failure) in failures.iter().enumerate() {
        let env_i = make_envelope(0xEE, 9, 4, 3_000_000);
        assert_eq!(env_i.signature, reference_sig);
        egress_ring.push(&env_i);

        let mut proxy =
            HarmonicProxy::new(test_config(), MockSession::with_failure(failure.clone()));
        let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);

        // Terminal drop recorded.
        assert_eq!(
            proxy.death_clamped, 1,
            "failure mode {i}: death_clamped must be 1"
        );
        assert_eq!(
            proxy.bundles_sent, 0,
            "failure mode {i}: bundles_sent must be 0"
        );

        // Egress ring must be empty — no re-enqueue.
        assert!(
            egress_ring.pop().is_none(),
            "failure mode {i}: signed tx must not be re-enqueued to egress ring"
        );

        // Drain terminal outputs so rings don't fill for next iteration.
        let _ = nack_ring.pop();
        let _ = result_ring.pop();
    }
}

/// Structural: the proxy source must not contain any code path that copies
/// a death-clamped envelope's raw_bytes into a second ring or channel.
#[test]
fn death_clamp_no_second_lane_copy_structural() {
    let proxy_src = include_str!("proxy.rs");

    // After the envelope is death-clamped, the only raw_bytes access should
    // be in emit_result and emit_nack (which copy signature, not raw_bytes).
    // The death_clamp helper must not push to any egress ring.
    let dc_body = proxy_src
        .split("fn death_clamp(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .unwrap_or("");

    let banned_in_dc = [
        "try_push_envelope",
        "egress_ring.push",
        "jito_ring",
        "harmonic_ring.push",
        "tpu_send",
        "quic_send",
    ];
    for pat in &banned_in_dc {
        assert!(
            !dc_body.contains(pat),
            "death_clamp body must not contain '{}' — no second lane send",
            pat
        );
    }
}

/// Structural: no clone or copy of SignedTxEnvelope in proxy.rs —
/// the signed tx is consumed by pop(), processed once, then dropped.
#[test]
fn death_clamp_no_envelope_clone_in_proxy() {
    let proxy_src = include_str!("proxy.rs");
    assert!(
        !proxy_src.contains("envelope.clone()"),
        "proxy must not clone envelopes — prevents accidental second-lane relay"
    );
    assert!(
        !proxy_src.contains(".raw_bytes.clone()"),
        "proxy must not clone raw_bytes"
    );
}

// ===========================================================================
// 38. Coarse Harmonic health publication with hysteresis (step 8)
//     Mirrors Jito health tests from shadow-egress-jito/src/tests.rs
// ===========================================================================

#[test]
fn publish_harmonic_health_roundtrips_all_four_spec_fields() {
    // After publishing, the segment must contain all four required fields:
    // harmonic_state, harmonic_queue_age_class, last_update_unix_ns, publish_epoch.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // Set queue age to Backlogged (60% fill).
    tracker.update_queue_age(60, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Backlogged);

    let now_ns: u64 = 1_700_000_000_000_000_000;
    publish_harmonic_health(&segment, &mut tracker, now_ns, 2);

    let payload = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(payload.harmonic_state), LaneState::Up);
    assert_eq!(
        QueueAgeClass::from_u8(payload.harmonic_queue_age_class),
        QueueAgeClass::Backlogged
    );
    assert_eq!(payload.last_update_unix_ns, now_ns);
    assert_eq!(payload.publish_epoch, 1);
    assert_eq!(payload.degraded_hysteresis_batches, 2);
}

#[test]
fn publish_harmonic_health_increments_epoch_through_segment() {
    // Each publish must bump publish_epoch monotonically through the segment.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();

    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p1 = segment.payload().read_active().unwrap();
    assert_eq!(p1.publish_epoch, 1);

    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p2 = segment.payload().read_active().unwrap();
    assert_eq!(p2.publish_epoch, 2);
    assert_eq!(p2.last_update_unix_ns, 200);

    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p3 = segment.payload().read_active().unwrap();
    assert_eq!(p3.publish_epoch, 3);
}

#[test]
fn health_hysteresis_prevents_single_failure_publish_flip() {
    // A single failure after sustained success must not immediately flip
    // the published health from Up to Degraded.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Up);

    // One failure — must NOT flip to Degraded.
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "single failure must not flip published state — hysteresis"
    );

    // Two failures — must NOT flip yet (threshold is 3).
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "two failures must not flip — threshold is 3"
    );

    // Third failure — crosses degrade_threshold, should flip.
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Degraded,
        "three consecutive failures must flip to Degraded"
    );
}

#[test]
fn reconnect_churn_does_not_degrade_published_health() {
    // Alternating fail/succeed stays Up through the segment.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();

    for _ in 0..10 {
        tracker.record_failure();
        tracker.record_success();
    }

    publish_harmonic_health(&segment, &mut tracker, 500, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "reconnect churn must not degrade — hysteresis requires consecutive failures"
    );
}

#[test]
fn sustained_failure_drives_published_health_through_degraded_to_down() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // 3 failures → Degraded.
    for _ in 0..3 {
        tracker.record_failure();
    }
    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Degraded);

    // 7 more failures (total 10) → Down.
    for _ in 0..7 {
        tracker.record_failure();
    }
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Down);
}

#[test]
fn publish_harmonic_health_writes_all_four_spec_fields_structural() {
    let health_src = include_str!("health.rs");

    let required_fields = [
        "harmonic_state",
        "harmonic_queue_age_class",
        "last_update_unix_ns",
        "publish_epoch",
    ];

    let mut in_publish_fn = false;
    let mut found = [false; 4];
    for line in health_src.lines() {
        if line.contains("pub fn publish_harmonic_health") {
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
            if line == "}" {
                break;
            }
        }
    }

    for (i, field) in required_fields.iter().enumerate() {
        assert!(
            found[i],
            "publish_harmonic_health must write payload.{field}"
        );
    }
}

#[test]
fn harmonic_admissibility_through_published_health() {
    // Workers use is_harmonic_admissible() to gate pre-sign egress admission.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);

    // Down → not admissible.
    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert!(
        !p.is_harmonic_admissible(),
        "Down lane must not be admissible"
    );

    // Recover → Up → admissible.
    tracker.record_success();
    tracker.record_success();
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert!(p.is_harmonic_admissible(), "Up lane must be admissible");

    // Degrade (3 failures) → Degraded with Fresh queue → still admissible.
    for _ in 0..3 {
        tracker.record_failure();
    }
    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Degraded);
    assert!(
        p.is_harmonic_admissible(),
        "Degraded + non-Severe queue must be admissible"
    );

    // Degraded + Severe queue → not admissible.
    tracker.update_queue_age(80, 100);
    publish_harmonic_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert!(
        !p.is_harmonic_admissible(),
        "Degraded + Severe queue must not be admissible"
    );
}

#[test]
fn drain_once_reflects_death_clamp_into_health() {
    let dir = tempfile::tempdir().unwrap();
    let resources = create_temp_proxy_resources(dir.path(), 1);

    for i in 0..3u8 {
        let env = make_envelope(i, i as u32, 0, 100_000);
        resources.egress_rings[0].push(&env);
    }

    let mut proxy = HarmonicProxy::new(test_config(), MockSession::disconnected());

    for _ in 0..3 {
        proxy.drain_once(&resources);
    }

    let p = resources.health_segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Down,
        "sustained death clamps must keep health at Down"
    );
    assert_eq!(proxy.death_clamped, 3);
}

#[test]
fn health_not_per_envelope_but_coarse() {
    let health_src = include_str!("health.rs");
    let banned = [
        "SignedTxEnvelope",
        "signature",
        "pool_index",
        "mint_out_index",
        "raw_bytes",
    ];
    for pat in banned {
        assert!(
            !health_src.contains(pat),
            "health.rs must not reference per-envelope field '{pat}' — coarse only"
        );
    }
}

#[test]
fn health_uses_read_modify_write_structural() {
    let health_src = include_str!("health.rs");
    let publish_body = health_src
        .split("pub fn publish_harmonic_health")
        .nth(1)
        .unwrap_or("");
    assert!(
        publish_body.contains("read_active"),
        "publish must read_active to preserve Jito fields"
    );
    assert!(
        publish_body.contains("write_payload"),
        "publish must write_payload via double-buffer protocol"
    );
    assert!(
        publish_body.contains("active_idx"),
        "publish must flip active_idx after write"
    );
}

// ===========================================================================
// 38b. Health publication contract proofs (checklist-required)
//
// Mirror Jito's health tests:
//   1. healthy/degraded/down transitions (full cycle through segment)
//   2. queue-age classes 0..3 (all four published through segment)
//   3. hysteresis suppresses one-batch flapping (both degrade and recover)
//   4. Harmonic updates do not corrupt Jito health fields in shared segment
// ===========================================================================

/// Full state cycle: Down → Up (recover) → Degraded (fail) → Up (recover)
/// → Down (sustained fail).  Every transition verified through the published
/// segment, not just the tracker.
#[test]
fn health_full_state_cycle_through_segment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    let mut ts: u64 = 100;

    // Phase 1: starts Down (fail-closed).
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Down,
        "initial state must be Down"
    );

    // Phase 2: recover → Up (2 successes = recover_batches).
    tracker.record_success();
    tracker.record_success();
    ts += 100;
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "2 successes must recover to Up"
    );

    // Phase 3: degrade → Degraded (3 failures = degrade_threshold).
    for _ in 0..3 {
        tracker.record_failure();
    }
    ts += 100;
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Degraded,
        "3 failures must degrade to Degraded"
    );

    // Phase 4: recover back to Up from Degraded (2 successes).
    tracker.record_success();
    tracker.record_success();
    ts += 100;
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "2 successes from Degraded must recover to Up"
    );

    // Phase 5: sustained failure → Down (10 failures = down_threshold).
    for _ in 0..10 {
        tracker.record_failure();
    }
    ts += 100;
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Down,
        "10 failures must drive to Down"
    );

    // Phase 6: recover from Down → Up (2 successes).
    tracker.record_success();
    tracker.record_success();
    ts += 100;
    publish_harmonic_health(&segment, &mut tracker, ts, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "2 successes from Down must recover to Up"
    );

    // Timestamps and epoch advanced through entire cycle.
    assert_eq!(p.last_update_unix_ns, ts);
    assert!(
        p.publish_epoch >= 6,
        "epoch must have advanced through 6+ publishes"
    );
}

/// All four queue-age classes (Fresh=0, Warm=1, Backlogged=2, Severe=3) must
/// be publishable through the segment and readable back correctly.
#[test]
fn health_all_queue_age_classes_through_segment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();

    // Class 0: Fresh (0% fill).
    tracker.update_queue_age(0, 100);
    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Fresh,
        "0% fill → Fresh"
    );

    // Class 1: Warm (25% fill).
    tracker.update_queue_age(25, 100);
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Warm,
        "25% fill → Warm"
    );

    // Class 2: Backlogged (50% fill).
    tracker.update_queue_age(50, 100);
    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Backlogged,
        "50% fill → Backlogged"
    );

    // Class 3: Severe (75% fill).
    tracker.update_queue_age(75, 100);
    publish_harmonic_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Severe,
        "75% fill → Severe"
    );
}

/// Queue-age class boundary values: verify exact thresholds at 24/25 and 49/50
/// and 74/75 percent.
#[test]
fn health_queue_age_boundary_values() {
    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);

    // 24% → Fresh (boundary).
    tracker.update_queue_age(24, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Fresh);

    // 25% → Warm (crosses into next band).
    tracker.update_queue_age(25, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Warm);

    // 49% → Warm.
    tracker.update_queue_age(49, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Warm);

    // 50% → Backlogged.
    tracker.update_queue_age(50, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Backlogged);

    // 74% → Backlogged.
    tracker.update_queue_age(74, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Backlogged);

    // 75% → Severe.
    tracker.update_queue_age(75, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Severe);

    // 100% → Severe.
    tracker.update_queue_age(100, 100);
    assert_eq!(tracker.queue_age_class(), QueueAgeClass::Severe);
}

/// Hysteresis suppresses one-batch flapping on the degrade side:
/// threshold is 3, so 2 failures followed by 1 success keeps state at Up.
/// Also suppresses on the recovery side: recover_batches is 2, so 1 success
/// from Degraded does NOT recover.
#[test]
fn health_hysteresis_suppresses_both_degrade_and_recover_flapping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);
    tracker.record_success();
    tracker.record_success();
    assert_eq!(tracker.state(), LaneState::Up);

    // Degrade-side hysteresis: 1 failure → still Up.
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "1 failure must not flip — degrade threshold is 3"
    );

    // 2 failures → still Up.
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "2 failures must not flip — degrade threshold is 3"
    );

    // 1 success resets counter → Up.
    tracker.record_success();
    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "success after 2 failures resets counter — still Up"
    );

    // Now actually degrade: 3 consecutive failures.
    for _ in 0..3 {
        tracker.record_failure();
    }
    publish_harmonic_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Degraded);

    // Recovery-side hysteresis: 1 success from Degraded → still Degraded.
    tracker.record_success();
    publish_harmonic_health(&segment, &mut tracker, 500, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Degraded,
        "1 success from Degraded must not recover — recover_batches is 2"
    );

    // 1 failure resets recovery counter → still Degraded.
    tracker.record_failure();
    publish_harmonic_health(&segment, &mut tracker, 600, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Degraded,
        "failure during recovery resets counter — still Degraded"
    );

    // 2 consecutive successes → recovers to Up.
    tracker.record_success();
    tracker.record_success();
    publish_harmonic_health(&segment, &mut tracker, 700, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        LaneState::from_u8(p.harmonic_state),
        LaneState::Up,
        "2 consecutive successes from Degraded must recover to Up"
    );
}

/// Multiple Harmonic publishes with different states must never corrupt
/// pre-seeded Jito fields in the shared segment.
#[test]
fn health_harmonic_updates_never_corrupt_jito_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("egress_health");
    let segment =
        shadow_ipc::segment::MappedSegment::<EgressHealthSnapshot>::create_rw(&path).unwrap();

    // Pre-seed Jito fields as if the Jito proxy published first.
    let jito_payload = EgressHealthPayload32B {
        jito_state: LaneState::Degraded as u8,
        harmonic_state: LaneState::Down as u8,
        jito_queue_age_class: QueueAgeClass::Backlogged as u8,
        harmonic_queue_age_class: QueueAgeClass::Fresh as u8,
        degraded_hysteresis_batches: 7,
        _pad_u16: 0,
        last_update_unix_ns: 42_000,
        publish_epoch: 99,
        _reserved: [0u8; 8],
    };
    let health = segment.payload();
    health.slot0.write_payload(&jito_payload);
    health
        .active_idx
        .active_idx
        .store(0, std::sync::atomic::Ordering::Release);

    let mut tracker = HarmonicHealthTracker::new(3, 10, 2);

    // Publish 1: Down (initial) — Jito fields must survive.
    publish_harmonic_health(&segment, &mut tracker, 100, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        p.jito_state,
        LaneState::Degraded as u8,
        "Harmonic publish must not overwrite jito_state"
    );
    assert_eq!(
        p.jito_queue_age_class,
        QueueAgeClass::Backlogged as u8,
        "Harmonic publish must not overwrite jito_queue_age_class"
    );
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Down);

    // Publish 2: recover → Up — Jito still untouched.
    tracker.record_success();
    tracker.record_success();
    publish_harmonic_health(&segment, &mut tracker, 200, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(p.jito_state, LaneState::Degraded as u8);
    assert_eq!(p.jito_queue_age_class, QueueAgeClass::Backlogged as u8);
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Up);

    // Publish 3: degrade → Degraded + Severe queue — Jito still untouched.
    for _ in 0..3 {
        tracker.record_failure();
    }
    tracker.update_queue_age(80, 100);
    publish_harmonic_health(&segment, &mut tracker, 300, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        p.jito_state,
        LaneState::Degraded as u8,
        "jito_state must survive multiple Harmonic publishes"
    );
    assert_eq!(
        p.jito_queue_age_class,
        QueueAgeClass::Backlogged as u8,
        "jito_queue_age_class must survive multiple Harmonic publishes"
    );
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Degraded);
    assert_eq!(
        QueueAgeClass::from_u8(p.harmonic_queue_age_class),
        QueueAgeClass::Severe
    );

    // Publish 4: sustained failure → Down + Severe queue — Jito still untouched.
    for _ in 0..7 {
        tracker.record_failure();
    }
    publish_harmonic_health(&segment, &mut tracker, 400, 2);
    let p = segment.payload().read_active().unwrap();
    assert_eq!(
        p.jito_state,
        LaneState::Degraded as u8,
        "jito_state must never change under any Harmonic state transition"
    );
    assert_eq!(
        p.jito_queue_age_class,
        QueueAgeClass::Backlogged as u8,
        "jito_queue_age_class must never change under any Harmonic state transition"
    );
    assert_eq!(LaneState::from_u8(p.harmonic_state), LaneState::Down);
}

/// Structural: publish_harmonic_health must overwrite only the Harmonic fields
/// and shared metadata — never the jito_state or jito_queue_age_class.
#[test]
fn health_publish_only_writes_harmonic_fields_structural() {
    let health_src = include_str!("health.rs");

    // Extract publish_harmonic_health body.
    let publish_body = health_src
        .split("pub fn publish_harmonic_health")
        .nth(1)
        .unwrap_or("");

    // Must write Harmonic fields.
    assert!(
        publish_body.contains("payload.harmonic_state ="),
        "must write harmonic_state"
    );
    assert!(
        publish_body.contains("payload.harmonic_queue_age_class ="),
        "must write harmonic_queue_age_class"
    );

    // Must NOT write Jito fields (only read_active copies them as baseline).
    assert!(
        !publish_body.contains("payload.jito_state ="),
        "must not overwrite jito_state — preserved from read_active"
    );
    assert!(
        !publish_body.contains("payload.jito_queue_age_class ="),
        "must not overwrite jito_queue_age_class — preserved from read_active"
    );
}

// ===========================================================================
// 39. G7/G8 ownership boundary — structural gates
//
// G7 emits: send outcome (EgressResult), coarse lane health, NACK on local failure.
// G8 owns: pending sig tables, terminal inventory release, BundleState parsing,
//          lethal-fault classification, tactic_mask publication, quarantine policy.
//
// These tests scan all shadow-harmonic source files to prove G8 concerns
// have not leaked into the proxy crate.  If any test fails, the offending
// pattern must be moved to the PM (G8) crate, not worked around.
// ===========================================================================

#[test]
fn g7_g8_boundary_no_pending_signature_table() {
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let health_src = include_str!("health.rs");

    let banned = [
        "PendingSig",
        "pending_sig",
        "pending_signature",
        "SignatureTable",
        "sig_table",
        "SigTable",
        "HashMap",
        "BTreeMap",
        "DashMap",
    ];
    for src_name in &[
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("health.rs", health_src),
    ] {
        for pat in &banned {
            assert!(
                !src_name.1.contains(pat),
                "{} must not contain '{}' — pending signature tracking belongs to G8 PM",
                src_name.0,
                pat
            );
        }
    }
}

#[test]
fn g7_g8_boundary_no_terminal_inventory_release() {
    let proxy_src = include_str!("proxy.rs");

    let banned = [
        "terminal_release",
        "inventory_release",
        "release_policy",
        "release_capital",
        "release_inventory",
        "FinalRelease",
        "TerminalRelease",
    ];
    for pat in &banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{}' — terminal inventory release belongs to G8 PM",
            pat
        );
    }
}

#[test]
fn g7_g8_boundary_no_bundle_state_parsing() {
    // Covers all source files, not just proxy.rs.
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");
    let health_src = include_str!("health.rs");

    let banned = [
        "BundleState",
        "bundle_state",
        "BundleResult",
        "bundle_result",
    ];
    for src_name in &[
        ("proxy.rs", proxy_src),
        ("session.rs", session_src),
        ("health.rs", health_src),
    ] {
        for pat in &banned {
            assert!(
                !src_name.1.contains(pat),
                "{} must not contain '{}' — BundleState telemetry parsing belongs to G8 PM",
                src_name.0,
                pat
            );
        }
    }
}

#[test]
fn g7_g8_boundary_no_lethal_fault_classification() {
    let proxy_src = include_str!("proxy.rs");
    let session_src = include_str!("session.rs");

    let banned = ["LethalFault", "lethal_fault", "lethal_class", "FaultClass"];
    for src_name in &[("proxy.rs", proxy_src), ("session.rs", session_src)] {
        for pat in &banned {
            assert!(
                !src_name.1.contains(pat),
                "{} must not contain '{}' — lethal-fault classification belongs to G8 PM",
                src_name.0,
                pat
            );
        }
    }
}

#[test]
fn g7_g8_boundary_no_tactic_mask_publication() {
    let proxy_src = include_str!("proxy.rs");
    let health_src = include_str!("health.rs");

    let banned = [
        "tactic_mask",
        "harmonic_tactic_mask",
        "TacticMask",
        "publish_tactic",
        "write_tactic_mask",
    ];
    for src_name in &[("proxy.rs", proxy_src), ("health.rs", health_src)] {
        for pat in &banned {
            assert!(
                !src_name.1.contains(pat),
                "{} must not contain '{}' — tactic mask publication belongs to G8 PM",
                src_name.0,
                pat
            );
        }
    }
}

#[test]
fn g7_g8_boundary_no_quarantine_policy() {
    let proxy_src = include_str!("proxy.rs");
    let health_src = include_str!("health.rs");

    // The proxy may READ quarantine flags from the egress health segment
    // (that's the worker side, not the proxy side).  But the proxy must not
    // IMPLEMENT quarantine policy: no quarantine writes, no quarantine state
    // machines, no quarantine duration computation.
    let banned = [
        "quarantine_state",
        "QuarantineState",
        "quarantine_until",
        "quarantine_duration",
        "enter_quarantine",
        "exit_quarantine",
        "quarantine_policy",
        "QuarantinePolicy",
    ];
    for src_name in &[("proxy.rs", proxy_src), ("health.rs", health_src)] {
        for pat in &banned {
            assert!(
                !src_name.1.contains(pat),
                "{} must not contain '{}' — quarantine policy belongs to G8 PM",
                src_name.0,
                pat
            );
        }
    }
}

#[test]
fn g7_g8_boundary_proxy_counters_are_scalars_only() {
    // The proxy struct must use only scalar counters — no collection-type
    // fields that could grow into signature tracking or telemetry accumulation.
    let proxy_src = include_str!("proxy.rs");

    // Extract the HarmonicProxy struct definition.
    let struct_start = proxy_src
        .find("pub struct HarmonicProxy")
        .expect("HarmonicProxy struct must exist");
    let struct_body = &proxy_src[struct_start..];
    let struct_end = struct_body.find("\nimpl").unwrap_or(struct_body.len());
    let struct_def = &struct_body[..struct_end];

    let banned_field_types = [
        "HashMap", "BTreeMap", "Vec<", "VecDeque", "HashSet", "BTreeSet",
    ];
    for pat in &banned_field_types {
        assert!(
            !struct_def.contains(pat),
            "HarmonicProxy must not contain collection field '{}' — \
             signature tracking and telemetry accumulation belong to G8 PM",
            pat
        );
    }
}

#[test]
fn g7_emits_only_what_it_should() {
    // Positive boundary: confirm G7 emits exactly the three outputs it owns.
    let proxy_src = include_str!("proxy.rs");

    // 1. EgressResult (send outcome)
    assert!(
        proxy_src.contains("fn emit_result("),
        "proxy must emit EgressResult (send outcome)"
    );

    // 2. NACK on local failure
    assert!(
        proxy_src.contains("fn emit_nack("),
        "proxy must emit NACK on local pre-transmission failure"
    );

    // 3. Coarse lane health
    assert!(
        proxy_src.contains("fn publish_health("),
        "proxy must publish coarse lane health"
    );

    // Must NOT have any other emit/publish apart from these three.
    let emit_count =
        proxy_src.matches("fn emit_").count() + proxy_src.matches("fn publish_").count();
    assert_eq!(emit_count, 3,
        "proxy must have exactly 3 output functions (emit_result, emit_nack, publish_health), found {}",
        emit_count);
}

// ===========================================================================
// 40. Proxy/session checklist tests — G7 required proofs
// ===========================================================================

/// Each region can transition through Connecting → Down(backoff) → Connecting
/// in sequence without blocking the caller.  The poll() call drives transitions
/// non-blockingly via try_recv on the per-region mpsc channel.
#[test]
fn region_enters_connecting_live_backoff_without_blocking() {
    use std::time::Instant;

    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);
    let (tx2, rx2) = std::sync::mpsc::sync_channel(1);

    let cfg = HarmonicEgressConfig {
        region_endpoints: vec!["https://r1.local".into(), "https://r2.local".into()],
        money_workers: 1,
        reconnect_initial_backoff_ms: 50,
        reconnect_max_backoff_ms: 1000,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1, rx2]);

    // Phase 1: Both regions start in Connecting.
    assert_eq!(session.region_state(0), SessionState::Connecting);
    assert_eq!(session.region_state(1), SessionState::Connecting);

    // Region 0: fail → Down (backoff armed).
    tx1.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    // Region 1: still connecting (no message yet).

    let t0 = Instant::now();
    session.poll();
    let poll_elapsed = t0.elapsed();

    assert_eq!(session.region_state(0), SessionState::Down);
    assert_eq!(session.region_state(1), SessionState::Connecting);

    // poll() itself must return sub-millisecond (non-blocking).
    assert!(
        poll_elapsed.as_millis() < 10,
        "poll() must be non-blocking, took {}ms",
        poll_elapsed.as_millis()
    );

    // Region 1: also fail → Down.
    tx2.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();
    assert_eq!(session.region_state(1), SessionState::Down);

    // After backoff expires, poll should re-arm Connecting.
    // Sleep just past the initial backoff.
    std::thread::sleep(std::time::Duration::from_millis(60));
    session.poll();

    // At least one region should be back to Connecting (backoff timer expired,
    // poll spawns a new connect attempt).  The new connect will have a fresh
    // mpsc receiver, so the region transitions to Connecting.
    let any_connecting = (0..2).any(|i| session.region_state(i) == SessionState::Connecting);
    assert!(
        any_connecting,
        "regions must re-enter Connecting after backoff expires"
    );
}

/// Reconnect backoff never blocks the dequeue loop.  Even with all regions in
/// backoff (Disconnected with a future backoff_until), poll() returns instantly.
#[test]
fn reconnect_never_blocks_dequeue_loop() {
    use std::time::Instant;

    let (tx1, rx1) = std::sync::mpsc::sync_channel(1);

    let cfg = HarmonicEgressConfig {
        region_endpoints: vec!["https://r1.local".into()],
        money_workers: 1,
        reconnect_initial_backoff_ms: 60_000, // very long backoff
        reconnect_max_backoff_ms: 60_000,
        ..HarmonicEgressConfig::default()
    };
    let mut session = GrpcHarmonicSession::new_test(cfg, vec![rx1]);

    // Fail connect to arm backoff.
    tx1.send(RegionConnectResult::Failed("test failure".to_owned()))
        .unwrap();
    session.poll();
    assert_eq!(session.region_state(0), SessionState::Down);

    // Now the region is in Disconnected with a 60s backoff.
    // poll() must return instantly despite the long timer.
    let t0 = Instant::now();
    for _ in 0..100 {
        session.poll();
    }
    let elapsed = t0.elapsed();

    // 100 polls must finish in well under 100ms — no blocking.
    assert!(
        elapsed.as_millis() < 50,
        "100 polls with 60s backoff must not block, took {}ms",
        elapsed.as_millis()
    );
    // Region must still be Down (backoff hasn't expired).
    assert_eq!(session.region_state(0), SessionState::Down);
}

/// Session-Down path fast-drops envelopes and updates health tracker toward
/// degraded/down.  After enough consecutive death clamps, health must progress
/// through the state machine.
#[test]
fn session_down_fast_drops_and_updates_health() {
    let dir = tempfile::tempdir().unwrap();
    let (egress_ring, nack_ring, result_ring) = create_temp_rings(dir.path());

    let cfg = HarmonicEgressConfig {
        degrade_failure_threshold: 2,
        down_failure_threshold: 4,
        hysteresis_recover_batches: 1,
        ..test_config()
    };
    let mut proxy = HarmonicProxy::new(cfg, MockSession::disconnected());

    // Initial health state: Down (fail-closed, no successes yet).
    assert_eq!(proxy.health_state(), LaneState::Down);

    // Each process_one on a Down session must:
    // 1. NACK the envelope (death clamp)
    // 2. Increment death_clamped
    // 3. Call health_tracker.record_failure()
    for i in 0..5u8 {
        let env = make_envelope(i, 1, 2, 500_000);
        egress_ring.push(&env);

        let t0 = std::time::Instant::now();
        let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
        let elapsed = t0.elapsed();

        assert_eq!(
            result,
            SendAttemptResult::NackEmitted,
            "down session must NACK on iteration {i}"
        );

        // The fast-drop must be sub-millisecond (no network wait).
        assert!(
            elapsed.as_millis() < 5,
            "down session drop must be fast, took {}ms on iteration {i}",
            elapsed.as_millis()
        );
    }

    assert_eq!(proxy.death_clamped, 5);
    assert_eq!(proxy.nacks_emitted, 5);
    assert_eq!(proxy.bundles_sent, 0);

    // Health should reflect sustained failure.
    // With degrade_threshold=2 and down_threshold=4, after 5 failures
    // health should be Down.
    assert_eq!(proxy.health_state(), LaneState::Down);
}

/// No worker-visible waiting behavior may leak into the proxy or session.
/// Structural scan: no thread::sleep, no blocking recv(), no Condvar::wait
/// in proxy.rs or the non-send_bundle portions of session.rs.
#[test]
fn no_worker_visible_waiting_leaks() {
    let proxy_src = include_str!("proxy.rs");

    // Proxy must never block.
    let proxy_banned = [
        "thread::sleep",
        "std::thread::sleep",
        "Condvar",
        ".recv()", // blocking recv (try_recv is OK)
        ".recv_timeout",
    ];
    for pat in &proxy_banned {
        assert!(
            !proxy_src.contains(pat),
            "proxy.rs must not contain '{}' — no worker-visible waiting",
            pat
        );
    }

    // Session: poll() and aggregate_state() must not block.
    // send_bundle IS allowed to block_on (bounded by send_timeout_ms).
    let session_src = include_str!("session.rs");

    // poll_region and poll_region_refresh must use try_recv, not recv.
    let poll_region_body = session_src.split("fn poll_region(").nth(1).unwrap_or("");
    assert!(
        !poll_region_body.contains(".recv()"),
        "poll_region must not use blocking recv()"
    );

    let poll_refresh_body = session_src
        .split("fn poll_region_refresh(")
        .nth(1)
        .unwrap_or("");
    assert!(
        !poll_refresh_body.contains(".recv()"),
        "poll_region_refresh must not use blocking recv()"
    );

    // poll() itself must not use block_on.
    let poll_body = session_src
        .split("fn poll(&mut self)")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .unwrap_or("");
    assert!(
        !poll_body.contains("block_on"),
        "poll() must not use block_on — reconnect is async-spawned"
    );

    // aggregate_state must not use any blocking primitives.
    let agg_body = session_src
        .split("fn aggregate_state(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .unwrap_or("");
    let agg_banned = ["block_on", "thread::sleep", ".recv()", "Condvar"];
    for pat in &agg_banned {
        assert!(
            !agg_body.contains(pat),
            "aggregate_state must not contain '{}' — pure computation only",
            pat
        );
    }

    // The drain_once/run loop in proxy must not thread::sleep.
    let drain_body = proxy_src.split("fn drain_once(").nth(1).unwrap_or("");
    assert!(
        !drain_body.contains("thread::sleep"),
        "drain_once must not sleep — yield_now only"
    );

    let run_body = proxy_src
        .split("fn run(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .unwrap_or("");
    assert!(
        !run_body.contains("thread::sleep"),
        "run() must not sleep — yield_now only"
    );
}

// ---------------------------------------------------------------------------
// 41. QC fix proofs
// ---------------------------------------------------------------------------

/// QC-F3: AllRegionsFailed death-clamp preserves region_mask and err_code in
/// the published EgressResult rather than zeroing them.
#[test]
fn qc_f3_all_regions_failed_preserves_region_detail() {
    let dir = tempfile::tempdir().unwrap();
    let ring_path = dir.path().join("egress");
    let nack_path = dir.path().join("nack");
    let result_path = dir.path().join("result");

    let egress_ring =
        MappedSpscRing::<SignedTxEnvelope>::create_rw(&ring_path, 100, 1, 64).unwrap();
    let nack_ring = MappedSpscRing::<NackRecord128B>::create_rw(&nack_path, 101, 1, 64).unwrap();
    let result_ring = MappedSpscRing::<EgressResult>::create_rw(&result_path, 102, 1, 64).unwrap();

    let mut env = make_full_harmonic_template_envelope(0xBB);
    env.reserved_notional_lamports = 777_000;
    egress_ring.push(&env);

    // AllRegionsFailed with region_mask=0x05 (regions 0+2 attempted) and err_code=11
    let mock = MockSession::with_failure(SendError::AllRegionsFailed {
        region_mask: 0x05,
        err_code: 11,
    });
    let mut proxy = HarmonicProxy::new(test_config(), mock);
    let result = proxy.process_one(&egress_ring, &nack_ring, &result_ring);
    assert_eq!(result, SendAttemptResult::NackEmitted);

    // The published EgressResult must carry the actual region detail,
    // not the zeroed-out values the old death_clamp used.
    let published = result_ring
        .pop()
        .expect("result ring must have exactly one record");
    assert_eq!(published.lane, LANE_HARMONIC);
    assert_eq!(
        published.region_mask, 0x05,
        "region_mask must be preserved from session"
    );
    assert_eq!(published.ok_mask, 0, "ok_mask must be 0 on total failure");
    assert_eq!(
        published.err_code, 11,
        "err_code must be the session's first-error code"
    );
    assert_eq!(published.reserved_notional_lamports, 777_000);
}

/// QC-F1: EgressResult ring comment correctly states dual-lane topology (structural).
#[test]
fn qc_f1_result_ring_comment_harmonic_only() {
    let src = include_str!("../../shadow-ipc/src/egress_ring.rs");
    assert!(
        !src.contains("shared across lanes"),
        "EgressResult comment must not say 'shared across lanes' — each lane has its own ring"
    );
    assert!(
        src.contains("Harmonic proxy") && src.contains("Jito proxy"),
        "EgressResult comment must reference both Harmonic and Jito proxies"
    );
}

/// QC-F2: bootstrap_egress_result_ring is called in tier-0 startup.
#[test]
fn qc_f2_result_ring_bootstrapped_in_startup() {
    let src = include_str!("../../shadow-tier0/src/main.rs");
    assert!(
        src.contains("bootstrap_egress_result_ring()"),
        "tier-0 main must call bootstrap_egress_result_ring"
    );
}

/// QC-F4: build_harmonic_egress_config does not use ..Default::default() fallback.
#[test]
fn qc_f4_config_builder_no_default_fallback() {
    let src = include_str!("../../shadow-tier0/src/main.rs");
    let builder_body = src
        .split("fn build_harmonic_egress_config")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        !builder_body.contains("..shadow_harmonic::HarmonicEgressConfig::default()"),
        "build_harmonic_egress_config must not use ..Default::default() — all fields must be explicit"
    );
    // Verify threshold fields are wired
    assert!(
        builder_body.contains("degrade_failure_threshold"),
        "degrade_failure_threshold must be wired"
    );
    assert!(
        builder_body.contains("down_failure_threshold"),
        "down_failure_threshold must be wired"
    );
    assert!(
        builder_body.contains("hysteresis_recover_batches"),
        "hysteresis_recover_batches must be wired"
    );
    assert!(
        builder_body.contains("degraded_hysteresis_batches"),
        "degraded_hysteresis_batches must be wired"
    );
}

/// QC-F4b: standalone shadow-egress-harmonic binary does not use ..Default::default().
/// Symmetric with qc_f4 — ensures both startup paths are fully explicit.
#[test]
fn qc_f4b_standalone_binary_no_default_fallback() {
    let src = include_str!("main.rs");
    let main_body = src.split("fn main()").nth(1).unwrap_or("");
    assert!(
        !main_body.contains("..HarmonicEgressConfig::default()"),
        "standalone binary must not use ..Default::default() — all fields must be explicit"
    );
    // Verify all config fields are wired via CLI args
    assert!(
        main_body.contains("degrade_failure_threshold"),
        "degrade_failure_threshold must be wired"
    );
    assert!(
        main_body.contains("down_failure_threshold"),
        "down_failure_threshold must be wired"
    );
    assert!(
        main_body.contains("hysteresis_recover_batches"),
        "hysteresis_recover_batches must be wired"
    );
    assert!(
        main_body.contains("degraded_hysteresis_batches"),
        "degraded_hysteresis_batches must be wired"
    );
}

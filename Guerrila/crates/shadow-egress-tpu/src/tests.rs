use crate::config::RunMode;
use crate::diag::TpuProbeSummary;
use crate::rpc::ClusterNode;
use shadow_ipc::egress_ring::{EgressResult, LANE_TPU_QUIC};
use shadow_ipc::ring::MappedSpscRing;
use tempfile::tempdir;

#[test]
fn cluster_node_contact_detection_accepts_tpu_or_tpu_quic() {
    let tpu = ClusterNode {
        pubkey: String::new(),
        tpu: Some("127.0.0.1:8001".to_owned()),
        tpu_quic: None,
    };
    assert!(tpu.has_tpu_contact());

    let tpu_quic = ClusterNode {
        pubkey: String::new(),
        tpu: None,
        tpu_quic: Some("127.0.0.1:8002".to_owned()),
    };
    assert!(tpu_quic.has_tpu_contact());

    let empty = ClusterNode {
        pubkey: String::new(),
        tpu: Some(String::new()),
        tpu_quic: None,
    };
    assert!(!empty.has_tpu_contact());
}

#[test]
fn blocked_summary_sets_not_ready_status() {
    let summary = TpuProbeSummary::blocked(
        RunMode::NoSend,
        "blocked_tpu_quic_no_send_unimplemented",
        "pending route wiring",
    );
    assert!(!summary.tpu_quic_sender_ready);
    assert_eq!(summary.mode, RunMode::NoSend);
    assert_eq!(summary.status, "blocked_tpu_quic_no_send_unimplemented");
}

#[test]
fn no_send_summary_renders_tpu_counters() {
    let summary = TpuProbeSummary {
        mode: RunMode::NoSend,
        status: "tpu_quic_no_send_ready".to_owned(),
        tpu_quic_sender_binary_exists: true,
        tpu_quic_sender_ready: true,
        tpu_ring_ready: true,
        tpu_result_ring_ready: true,
        nack_ring_ready: true,
        leader_cache_ok: true,
        local_rpc_status_ok: true,
        get_slot_ok: true,
        get_slot_leaders_ok: true,
        get_cluster_nodes_ok: true,
        get_latest_blockhash_ok: true,
        tpu_contact_count: 1,
        current_slot: Some(42),
        next_leader_count: 2,
        latest_blockhash: Some("abc".to_owned()),
        error_text: None,
        tpu_quic_sender_dequeue_count: 3,
        tpu_quic_no_send_accept_count: 3,
        tpu_quic_pm_reserve_simulated: 3,
        tpu_quic_pm_release_simulated: 3,
        tpu_quic_last_signature: Some("sig".to_owned()),
        tpu_quic_last_route_family: Some("tpu_quic_public".to_owned()),
        tpu_wire_send_count: 0,
        tpu_quic_tiny_live_enabled: 0,
        tpu_quic_send_attempt_count: 0,
        tpu_quic_send_ok_count: 0,
        tpu_quic_send_error_count: 0,
        tpu_quic_status_poll_count: 0,
        tpu_quic_landed_count: 0,
        tpu_quic_failed_count: 0,
        tpu_quic_expired_count: 0,
        tpu_quic_timeout_count: 0,
        tpu_quic_first_signature: None,
        tpu_quic_stop_reason: None,
        tpu_quic_pm_release_count: 0,
        pm_release_or_landed_reconcile: 0,
        attempt_count: 0,
        max_attempts: 1,
        max_inflight: 1,
        selected_signature: None,
        live_landed: 0,
        stop_reason: None,
        loss_budget_not_exceeded: true,
    };
    let rendered = summary.render();
    assert!(rendered.contains("tpu_quic_sender_dequeue_count=3"));
    assert!(rendered.contains("tpu_quic_no_send_accept_count=3"));
    assert!(rendered.contains("tpu_quic_pm_reserve_simulated=3"));
    assert!(rendered.contains("tpu_quic_pm_release_simulated=3"));
    assert!(rendered.contains("tpu_quic_last_route_family=tpu_quic_public"));
}

#[test]
fn tpu_result_roundtrip_preserves_tpu_lane() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tpu_result_test");
    let ring = MappedSpscRing::<EgressResult>::create_rw(&path, 998, 1, 8).unwrap();
    let result = EgressResult {
        signature: [7u8; 64],
        lane: LANE_TPU_QUIC,
        region_mask: 0,
        ok_mask: 0,
        err_code: 1,
        pool_index: 12,
        mint_out_index: 4,
        reserved_notional_lamports: 9,
        send_timestamp_ns: 11,
        tactic_class: 2,
        worker_id: 1,
        bundle_uuid: [0u8; 16],
        rejection_hint: 0,
        venue_id: 3,
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
        _reserved: [0u8; 3],
    };
    assert!(ring.push(&result));
    let popped = ring.pop().unwrap();
    assert_eq!(popped.lane, LANE_TPU_QUIC);
    assert_eq!(popped.reserved_notional_lamports, 9);
}

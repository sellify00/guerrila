use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use shadow_control::rollout_gate::{
    evaluate_gate_report_from_input, write_gate_evaluator_input, G12AdditiveQuantTacticsEvidence,
    G12GateEvaluatorInput, G12TacticalLaneEvidence, G12WorkerPoolStateMissRecent,
    G12WorkerPoolStateMissTopRecent, RolloutGateError,
};
use shadow_core::config::{
    ApexPartialRouteMode, AtbaRouteMode, ExhaustScavengingRouteMode, RolloutCapRung, RolloutMode,
    ShadowConfig, ShadowStrategyMode, SparseClassAction, StrategyMode, TickBoundarySabotageMode,
};
use shadow_core::pm::pm_loop::PmLoopStats;
use shadow_egress_jito::snapshot_proxy_diagnostics;
use shadow_ipc::clmm_exact_quote_sidecar::{
    clmm_exact_quote_status_str, CLMM_EXACT_QUOTE_FLAG_NO_QUOTE, CLMM_EXACT_QUOTE_FLAG_VALID,
    CLMM_EXACT_QUOTE_STATUS_INVALID_MINT_PAIR, CLMM_EXACT_QUOTE_STATUS_MISSING_AMM_CONFIG,
    CLMM_EXACT_QUOTE_STATUS_MISSING_POOL_QUOTE_CORE, CLMM_EXACT_QUOTE_STATUS_MISSING_RESERVES,
    CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL, CLMM_EXACT_QUOTE_STATUS_NO_MINT_IN,
    CLMM_EXACT_QUOTE_STATUS_QUOTE_BUILD_FAILED,
};
use shadow_ipc::egress_health::EgressHealthPayload32B;
use shadow_ipc::rollout_shadow::G12DropReason;

use crate::{
    grpc_diag::snapshot_grpc_ingest_diagnostics,
    money_worker::{
        snapshot_runls5b_autopsy, snapshot_runls5c_process_intent_subreason_rows,
        snapshot_worker_diagnostics, RunLs5bAutopsySnapshot, RunLs5bRejectReason,
        RunLs5bRejectReasonCounts, RunLs5bSourceAutopsy, RunLs5bWorkerStageCounts,
        RunLs5cProcessIntentSubreasonRow,
    },
    no_send_sink::snapshot_research_no_send_sink_report,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AfxdpRawGateSnapshot {
    frames_seen: u64,
    udp_8000_8025_seen: u64,
    xsk_rx_batches: u64,
    shred_parse_ok: u64,
    shred_parse_err: u64,
    shred_parse_data_ok: u64,
    shred_parse_coding_ok: u64,
    feed_enqueue_ok: u64,
    feed_enqueue_full: u64,
    reassembly_insert_ok: u64,
    reassembly_complete_entries: u64,
    fec_recovered_entries: u64,
    data_shred_payload_ok: u64,
    tx_extract_attempt: u64,
    tx_extract_ok: u64,
    tx_extract_err: u64,
    tx_extract_err_empty_payload: u64,
    tx_extract_err_short_vec: u64,
    tx_extract_err_bad_entry_header: u64,
    tx_extract_err_bad_compact_u16: u64,
    tx_extract_err_sanitize_failed: u64,
    tx_extract_err_no_transactions: u64,
    tx_extract_err_unsupported_format: u64,
    tx_record_emit_attempt: u64,
    tx_record_emit_ok: u64,
    tx_record_emit_full: u64,
    pipeline_drop_unsupported_program: u64,
    pipeline_drop_allowlist_miss: u64,
    pipeline_drop_decode_fail: u64,
    pipeline_drop_route_fail: u64,
    pipeline_drop_mint_fail: u64,
    pipeline_drop_other: u64,
    unsupported_program_top_0_id: String,
    unsupported_program_top_0_count: u64,
    unsupported_program_top_1_id: String,
    unsupported_program_top_1_count: u64,
    unsupported_program_top_2_id: String,
    unsupported_program_top_2_count: u64,
    unsupported_program_top_3_id: String,
    unsupported_program_top_3_count: u64,
    unsupported_program_top_4_id: String,
    unsupported_program_top_4_count: u64,
    unsupported_program_top_5_id: String,
    unsupported_program_top_5_count: u64,
    unsupported_program_top_6_id: String,
    unsupported_program_top_6_count: u64,
    unsupported_program_top_7_id: String,
    unsupported_program_top_7_count: u64,
    unsupported_program_top_8_id: String,
    unsupported_program_top_8_count: u64,
    unsupported_program_top_9_id: String,
    unsupported_program_top_9_count: u64,
    unsupported_program_top_10_id: String,
    unsupported_program_top_10_count: u64,
    unsupported_program_top_11_id: String,
    unsupported_program_top_11_count: u64,
    unsupported_program_top_12_id: String,
    unsupported_program_top_12_count: u64,
    unsupported_program_top_13_id: String,
    unsupported_program_top_13_count: u64,
    unsupported_program_top_14_id: String,
    unsupported_program_top_14_count: u64,
    unsupported_program_top_15_id: String,
    unsupported_program_top_15_count: u64,
    source_probe_emit_attempt: u64,
    source_probe_emit_ok: u64,
    source_probe_emit_full: u64,
    source_probe_emit_err: u64,
}

fn clmm_exact_quote_timeout_dependency_missing(status: u8) -> bool {
    matches!(
        status,
        CLMM_EXACT_QUOTE_STATUS_MISSING_TICK_ARRAY_LOCAL
            | CLMM_EXACT_QUOTE_STATUS_MISSING_POOL_QUOTE_CORE
            | CLMM_EXACT_QUOTE_STATUS_MISSING_AMM_CONFIG
            | CLMM_EXACT_QUOTE_STATUS_NO_MINT_IN
            | CLMM_EXACT_QUOTE_STATUS_INVALID_MINT_PAIR
            | CLMM_EXACT_QUOTE_STATUS_MISSING_RESERVES
    )
}

fn classify_last_clmm_exact_quote_response_timeout(
    worker_diag: &crate::money_worker::WorkerDiagnosticsSnapshot,
) -> &'static str {
    let flags = worker_diag.last_clmm_exact_quote_response_validity_flags;
    let status = worker_diag.last_clmm_exact_quote_response_status;
    let mismatch = worker_diag
        .last_clmm_exact_quote_response_mismatch_reason
        .as_str();
    if (flags & CLMM_EXACT_QUOTE_FLAG_NO_QUOTE) != 0 {
        if status == CLMM_EXACT_QUOTE_STATUS_QUOTE_BUILD_FAILED {
            "state_quote_build_failed"
        } else if clmm_exact_quote_timeout_dependency_missing(status) {
            "state_tick_array_or_mint_dependency_missing"
        } else {
            "state_published_terminal_no_quote"
        }
    } else if (flags & CLMM_EXACT_QUOTE_FLAG_VALID) != 0 {
        if mismatch == "request_id_mismatch" {
            "state_published_stale_request_id"
        } else {
            "state_published_quote_ready_but_tier0_rejected"
        }
    } else {
        ""
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
struct DpdkRawGateSnapshot {
    dpdk_rx_packets_total: u64,
    dpdk_rx_bytes_total: u64,
    dpdk_udp_8000_8025_seen: u64,
    dpdk_large_udp_8000_8025_seen: u64,
    dpdk_classify_ok: u64,
    dpdk_classify_drop_non_udp: u64,
    dpdk_classify_drop_wrong_port: u64,
    dpdk_classify_drop_too_small: u64,
    dpdk_shred_parse_attempt: u64,
    dpdk_shred_parse_ok: u64,
    dpdk_shred_parse_fail: u64,
    dpdk_tx_extract_attempt: u64,
    dpdk_tx_extract_ok: u64,
    dpdk_pipeline_drop_unsupported_program: u64,
    dpdk_pipeline_drop_allowlist_miss: u64,
    dpdk_pipeline_drop_decode_fail: u64,
    dpdk_pipeline_drop_route_fail: u64,
    dpdk_pipeline_drop_mint_fail: u64,
    dpdk_pipeline_drop_other: u64,
    dpdk_unsupported_program_top_0_id: String,
    dpdk_unsupported_program_top_0_count: u64,
    dpdk_unsupported_program_top_1_id: String,
    dpdk_unsupported_program_top_1_count: u64,
    dpdk_unsupported_program_top_2_id: String,
    dpdk_unsupported_program_top_2_count: u64,
    dpdk_unsupported_program_top_3_id: String,
    dpdk_unsupported_program_top_3_count: u64,
    dpdk_unsupported_program_top_4_id: String,
    dpdk_unsupported_program_top_4_count: u64,
    dpdk_unsupported_program_top_5_id: String,
    dpdk_unsupported_program_top_5_count: u64,
    dpdk_unsupported_program_top_6_id: String,
    dpdk_unsupported_program_top_6_count: u64,
    dpdk_unsupported_program_top_7_id: String,
    dpdk_unsupported_program_top_7_count: u64,
    dpdk_unsupported_program_top_8_id: String,
    dpdk_unsupported_program_top_8_count: u64,
    dpdk_unsupported_program_top_9_id: String,
    dpdk_unsupported_program_top_9_count: u64,
    dpdk_unsupported_program_top_10_id: String,
    dpdk_unsupported_program_top_10_count: u64,
    dpdk_unsupported_program_top_11_id: String,
    dpdk_unsupported_program_top_11_count: u64,
    dpdk_unsupported_program_top_12_id: String,
    dpdk_unsupported_program_top_12_count: u64,
    dpdk_unsupported_program_top_13_id: String,
    dpdk_unsupported_program_top_13_count: u64,
    dpdk_unsupported_program_top_14_id: String,
    dpdk_unsupported_program_top_14_count: u64,
    dpdk_unsupported_program_top_15_id: String,
    dpdk_unsupported_program_top_15_count: u64,
    dpdk_first_unsupported_tx: Option<DpdkUnsupportedTxCaptureExport>,
    dpdk_ingress_record_emit_attempt: u64,
    dpdk_ingress_record_emit_ok: u64,
    dpdk_ingress_record_emit_full: u64,
    dpdk_feed_enqueue_ok: u64,
    dpdk_feed_enqueue_full: u64,
    dpdk_worker_records_seen: u64,
    dpdk_worker_health_drop: u64,
    dpdk_worker_stale_slot_drop: u64,
    dpdk_worker_pool_state_miss: u64,
    dpdk_worker_pool_state_hit: u64,
    dpdk_worker_route_actionable: u64,
    dpdk_worker_route_dropped: u64,
    dpdk_worker_route_unavailable: u64,
    dpdk_worker_route_unavailable_unknown_signal: u64,
    dpdk_worker_route_unavailable_hydration_none: u64,
    dpdk_worker_r1_admission_drop: u64,
    dpdk_worker_candidates_entered_process_intent: u64,
    dpdk_worker_pipeline_error: u64,
    dpdk_worker_rollout_shadow_published: u64,
    dpdk_worker_final_no_send_published: u64,
    dpdk_pool_state_miss_tag_map: u64,
    dpdk_pool_state_miss_arena_oob: u64,
    dpdk_pool_state_miss_arena_torn: u64,
    dpdk_pool_state_miss_pubkey_mismatch: u64,
    dpdk_pool_state_miss_hint_enqueued: u64,
    dpdk_pool_state_miss_hint_deduped: u64,
    dpdk_last_pool_state_miss_pool_pubkey_b58: String,
    dpdk_last_pool_state_miss_venue_id: u8,
    dpdk_last_pool_state_miss_reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
struct DpdkUnsupportedTxCaptureExport {
    signature_b58: String,
    signer_pubkeys: Vec<String>,
    static_account_keys: Vec<String>,
    flattened_account_keys: Vec<String>,
    instruction_program_ids: Vec<String>,
    unsupported_program_ids: Vec<String>,
    raw_tx_hex: String,
    is_versioned: bool,
    instruction_count: usize,
}

#[cfg(target_os = "linux")]
fn snapshot_afxdp_gate_diagnostics() -> AfxdpRawGateSnapshot {
    let snapshot = crate::afxdp_runtime::snapshot_afxdp_raw_diagnostics();
    let unsupported = encode_unsupported_program_top(&snapshot.unsupported_program_top);
    AfxdpRawGateSnapshot {
        frames_seen: snapshot.frames_seen,
        udp_8000_8025_seen: snapshot.udp_8000_8025_seen,
        xsk_rx_batches: snapshot.xsk_rx_batches,
        shred_parse_ok: snapshot.shred_parse_ok,
        shred_parse_err: snapshot.shred_parse_err,
        shred_parse_data_ok: snapshot.shred_parse_data_ok,
        shred_parse_coding_ok: snapshot.shred_parse_coding_ok,
        feed_enqueue_ok: snapshot.feed_enqueue_ok,
        feed_enqueue_full: snapshot.feed_enqueue_full,
        reassembly_insert_ok: snapshot.reassembly_insert_ok,
        reassembly_complete_entries: snapshot.reassembly_complete_entries,
        fec_recovered_entries: snapshot.fec_recovered_entries,
        data_shred_payload_ok: snapshot.data_shred_payload_ok,
        tx_extract_attempt: snapshot.tx_extract_attempt,
        tx_extract_ok: snapshot.tx_extract_ok,
        tx_extract_err: snapshot.tx_extract_err,
        tx_extract_err_empty_payload: snapshot.tx_extract_err_empty_payload,
        tx_extract_err_short_vec: snapshot.tx_extract_err_short_vec,
        tx_extract_err_bad_entry_header: snapshot.tx_extract_err_bad_entry_header,
        tx_extract_err_bad_compact_u16: snapshot.tx_extract_err_bad_compact_u16,
        tx_extract_err_sanitize_failed: snapshot.tx_extract_err_sanitize_failed,
        tx_extract_err_no_transactions: snapshot.tx_extract_err_no_transactions,
        tx_extract_err_unsupported_format: snapshot.tx_extract_err_unsupported_format,
        tx_record_emit_attempt: snapshot.tx_record_emit_attempt,
        tx_record_emit_ok: snapshot.tx_record_emit_ok,
        tx_record_emit_full: snapshot.tx_record_emit_full,
        pipeline_drop_unsupported_program: snapshot.pipeline_drop_unsupported_program,
        pipeline_drop_allowlist_miss: snapshot.pipeline_drop_allowlist_miss,
        pipeline_drop_decode_fail: snapshot.pipeline_drop_decode_fail,
        pipeline_drop_route_fail: snapshot.pipeline_drop_route_fail,
        pipeline_drop_mint_fail: snapshot.pipeline_drop_mint_fail,
        pipeline_drop_other: snapshot.pipeline_drop_other,
        unsupported_program_top_0_id: unsupported[0].0.clone(),
        unsupported_program_top_0_count: unsupported[0].1,
        unsupported_program_top_1_id: unsupported[1].0.clone(),
        unsupported_program_top_1_count: unsupported[1].1,
        unsupported_program_top_2_id: unsupported[2].0.clone(),
        unsupported_program_top_2_count: unsupported[2].1,
        unsupported_program_top_3_id: unsupported[3].0.clone(),
        unsupported_program_top_3_count: unsupported[3].1,
        unsupported_program_top_4_id: unsupported[4].0.clone(),
        unsupported_program_top_4_count: unsupported[4].1,
        unsupported_program_top_5_id: unsupported[5].0.clone(),
        unsupported_program_top_5_count: unsupported[5].1,
        unsupported_program_top_6_id: unsupported[6].0.clone(),
        unsupported_program_top_6_count: unsupported[6].1,
        unsupported_program_top_7_id: unsupported[7].0.clone(),
        unsupported_program_top_7_count: unsupported[7].1,
        unsupported_program_top_8_id: unsupported[8].0.clone(),
        unsupported_program_top_8_count: unsupported[8].1,
        unsupported_program_top_9_id: unsupported[9].0.clone(),
        unsupported_program_top_9_count: unsupported[9].1,
        unsupported_program_top_10_id: unsupported[10].0.clone(),
        unsupported_program_top_10_count: unsupported[10].1,
        unsupported_program_top_11_id: unsupported[11].0.clone(),
        unsupported_program_top_11_count: unsupported[11].1,
        unsupported_program_top_12_id: unsupported[12].0.clone(),
        unsupported_program_top_12_count: unsupported[12].1,
        unsupported_program_top_13_id: unsupported[13].0.clone(),
        unsupported_program_top_13_count: unsupported[13].1,
        unsupported_program_top_14_id: unsupported[14].0.clone(),
        unsupported_program_top_14_count: unsupported[14].1,
        unsupported_program_top_15_id: unsupported[15].0.clone(),
        unsupported_program_top_15_count: unsupported[15].1,
        source_probe_emit_attempt: snapshot.source_probe_emit_attempt,
        source_probe_emit_ok: snapshot.source_probe_emit_ok,
        source_probe_emit_full: snapshot.source_probe_emit_full,
        source_probe_emit_err: snapshot.source_probe_emit_err,
    }
}

#[cfg(not(target_os = "linux"))]
fn snapshot_afxdp_gate_diagnostics() -> AfxdpRawGateSnapshot {
    AfxdpRawGateSnapshot::default()
}

#[cfg(all(target_os = "linux", feature = "dpdk"))]
fn snapshot_dpdk_gate_diagnostics() -> DpdkRawGateSnapshot {
    let snapshot = crate::dpdk_runtime::snapshot_dpdk_raw_diagnostics();
    let worker_diag = crate::money_worker::snapshot_worker_diagnostics();
    let unsupported = encode_dpdk_unsupported_program_top(&snapshot.unsupported_program_top);
    DpdkRawGateSnapshot {
        dpdk_rx_packets_total: snapshot.rx_packets_total,
        dpdk_rx_bytes_total: snapshot.rx_bytes_total,
        dpdk_udp_8000_8025_seen: snapshot.udp_8000_8025_seen,
        dpdk_large_udp_8000_8025_seen: snapshot.large_udp_8000_8025_seen,
        dpdk_classify_ok: snapshot.classify_ok,
        dpdk_classify_drop_non_udp: snapshot.classify_drop_non_udp,
        dpdk_classify_drop_wrong_port: snapshot.classify_drop_wrong_port,
        dpdk_classify_drop_too_small: snapshot.classify_drop_too_small,
        dpdk_shred_parse_attempt: snapshot.shred_parse_attempt,
        dpdk_shred_parse_ok: snapshot.shred_parse_ok,
        dpdk_shred_parse_fail: snapshot.shred_parse_fail,
        dpdk_tx_extract_attempt: snapshot.tx_extract_attempt,
        dpdk_tx_extract_ok: snapshot.tx_extract_ok,
        dpdk_pipeline_drop_unsupported_program: snapshot.pipeline_drop_unsupported_program,
        dpdk_pipeline_drop_allowlist_miss: snapshot.pipeline_drop_allowlist_miss,
        dpdk_pipeline_drop_decode_fail: snapshot.pipeline_drop_decode_fail,
        dpdk_pipeline_drop_route_fail: snapshot.pipeline_drop_route_fail,
        dpdk_pipeline_drop_mint_fail: snapshot.pipeline_drop_mint_fail,
        dpdk_pipeline_drop_other: snapshot.pipeline_drop_other,
        dpdk_unsupported_program_top_0_id: unsupported[0].0.clone(),
        dpdk_unsupported_program_top_0_count: unsupported[0].1,
        dpdk_unsupported_program_top_1_id: unsupported[1].0.clone(),
        dpdk_unsupported_program_top_1_count: unsupported[1].1,
        dpdk_unsupported_program_top_2_id: unsupported[2].0.clone(),
        dpdk_unsupported_program_top_2_count: unsupported[2].1,
        dpdk_unsupported_program_top_3_id: unsupported[3].0.clone(),
        dpdk_unsupported_program_top_3_count: unsupported[3].1,
        dpdk_unsupported_program_top_4_id: unsupported[4].0.clone(),
        dpdk_unsupported_program_top_4_count: unsupported[4].1,
        dpdk_unsupported_program_top_5_id: unsupported[5].0.clone(),
        dpdk_unsupported_program_top_5_count: unsupported[5].1,
        dpdk_unsupported_program_top_6_id: unsupported[6].0.clone(),
        dpdk_unsupported_program_top_6_count: unsupported[6].1,
        dpdk_unsupported_program_top_7_id: unsupported[7].0.clone(),
        dpdk_unsupported_program_top_7_count: unsupported[7].1,
        dpdk_unsupported_program_top_8_id: unsupported[8].0.clone(),
        dpdk_unsupported_program_top_8_count: unsupported[8].1,
        dpdk_unsupported_program_top_9_id: unsupported[9].0.clone(),
        dpdk_unsupported_program_top_9_count: unsupported[9].1,
        dpdk_unsupported_program_top_10_id: unsupported[10].0.clone(),
        dpdk_unsupported_program_top_10_count: unsupported[10].1,
        dpdk_unsupported_program_top_11_id: unsupported[11].0.clone(),
        dpdk_unsupported_program_top_11_count: unsupported[11].1,
        dpdk_unsupported_program_top_12_id: unsupported[12].0.clone(),
        dpdk_unsupported_program_top_12_count: unsupported[12].1,
        dpdk_unsupported_program_top_13_id: unsupported[13].0.clone(),
        dpdk_unsupported_program_top_13_count: unsupported[13].1,
        dpdk_unsupported_program_top_14_id: unsupported[14].0.clone(),
        dpdk_unsupported_program_top_14_count: unsupported[14].1,
        dpdk_unsupported_program_top_15_id: unsupported[15].0.clone(),
        dpdk_unsupported_program_top_15_count: unsupported[15].1,
        dpdk_first_unsupported_tx: snapshot.first_unsupported_tx.clone().map(Into::into),
        dpdk_ingress_record_emit_attempt: snapshot.ingress_record_emit_attempt,
        dpdk_ingress_record_emit_ok: snapshot.ingress_record_emit_ok,
        dpdk_ingress_record_emit_full: snapshot.ingress_record_emit_full,
        dpdk_feed_enqueue_ok: snapshot.feed_enqueue_ok,
        dpdk_feed_enqueue_full: snapshot.feed_enqueue_full,
        dpdk_worker_records_seen: worker_diag.dpdk_worker_records_seen,
        dpdk_worker_health_drop: worker_diag.dpdk_worker_health_drops,
        dpdk_worker_stale_slot_drop: worker_diag.dpdk_worker_stale_slot_drops,
        dpdk_worker_pool_state_miss: worker_diag.dpdk_worker_pool_state_miss,
        dpdk_worker_pool_state_hit: worker_diag.dpdk_worker_pool_state_hit,
        dpdk_worker_route_actionable: worker_diag.dpdk_worker_route_actionable,
        dpdk_worker_route_dropped: worker_diag.dpdk_worker_route_dropped,
        dpdk_worker_route_unavailable: worker_diag.dpdk_worker_route_unavailable,
        dpdk_worker_route_unavailable_unknown_signal: worker_diag
            .dpdk_worker_route_unavailable_unknown_signal,
        dpdk_worker_route_unavailable_hydration_none: worker_diag
            .dpdk_worker_route_unavailable_hydration_none,
        dpdk_worker_r1_admission_drop: worker_diag.dpdk_worker_r1_admission_drop,
        dpdk_worker_candidates_entered_process_intent: worker_diag
            .dpdk_worker_candidates_entered_process_intent,
        dpdk_worker_pipeline_error: worker_diag.dpdk_worker_pipeline_error,
        dpdk_worker_rollout_shadow_published: worker_diag.dpdk_worker_rollout_shadow_published,
        dpdk_worker_final_no_send_published: worker_diag.dpdk_worker_final_no_send_published,
        dpdk_pool_state_miss_tag_map: worker_diag.dpdk_pool_state_miss_tag_map,
        dpdk_pool_state_miss_arena_oob: worker_diag.dpdk_pool_state_miss_arena_oob,
        dpdk_pool_state_miss_arena_torn: worker_diag.dpdk_pool_state_miss_arena_torn,
        dpdk_pool_state_miss_pubkey_mismatch: worker_diag.dpdk_pool_state_miss_pubkey_mismatch,
        dpdk_pool_state_miss_hint_enqueued: worker_diag.dpdk_pool_state_miss_hint_enqueued,
        dpdk_pool_state_miss_hint_deduped: worker_diag.dpdk_pool_state_miss_hint_deduped,
        dpdk_last_pool_state_miss_pool_pubkey_b58: if worker_diag
            .dpdk_last_pool_state_miss_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.dpdk_last_pool_state_miss_pool_pubkey).into_string()
        },
        dpdk_last_pool_state_miss_venue_id: worker_diag.dpdk_last_pool_state_miss_venue_id,
        dpdk_last_pool_state_miss_reason: encode_dpdk_pool_state_miss_reason(
            worker_diag.dpdk_last_pool_state_miss_reason_code,
        ),
    }
}

#[cfg(not(all(target_os = "linux", feature = "dpdk")))]
fn snapshot_dpdk_gate_diagnostics() -> DpdkRawGateSnapshot {
    DpdkRawGateSnapshot::default()
}

#[cfg(target_os = "linux")]
fn encode_unsupported_program_top(
    top: &[crate::afxdp_producer::AfxdpUnsupportedProgramTopEntry;
         crate::afxdp_producer::AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN],
) -> [(String, u64); crate::afxdp_producer::AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN] {
    std::array::from_fn(|idx| {
        let entry = top[idx];
        if entry.count == 0 {
            (String::new(), 0)
        } else {
            (bs58::encode(entry.program_id).into_string(), entry.count)
        }
    })
}

#[cfg(all(target_os = "linux", feature = "dpdk"))]
fn encode_dpdk_unsupported_program_top(
    top: &[crate::dpdk_runtime::DpdkUnsupportedProgramTopEntry;
         crate::dpdk_runtime::DPDK_UNSUPPORTED_PROGRAM_TOP_LEN],
) -> [(String, u64); crate::dpdk_runtime::DPDK_UNSUPPORTED_PROGRAM_TOP_LEN] {
    std::array::from_fn(|idx| {
        let entry = top[idx];
        if entry.count == 0 {
            (String::new(), 0)
        } else {
            (bs58::encode(entry.program_id).into_string(), entry.count)
        }
    })
}

fn encode_grpc_unsupported_program_top(
    top: &[crate::grpc_diag::GrpcUnsupportedProgramTopEntry;
         crate::grpc_diag::GRPC_UNSUPPORTED_PROGRAM_TOP_LEN],
) -> [(String, u64); crate::grpc_diag::GRPC_UNSUPPORTED_PROGRAM_TOP_LEN] {
    std::array::from_fn(|idx| {
        let entry = top[idx];
        if entry.count == 0 {
            (String::new(), 0)
        } else {
            (bs58::encode(entry.program_id).into_string(), entry.count)
        }
    })
}

fn encode_dpdk_pool_state_miss_reason(reason_code: u8) -> String {
    match reason_code {
        1 => "tag_map_absent",
        2 => "arena_index_oob",
        3 => "arena_torn",
        4 => "pubkey_mismatch",
        _ => "none",
    }
    .to_string()
}

fn encode_lookup_failure_reason(
    reason: Option<shadow_core::lookup::LookupFailureReason>,
) -> String {
    reason
        .map(|reason| reason.as_str().to_string())
        .unwrap_or_default()
}

#[cfg(all(target_os = "linux", feature = "dpdk"))]
impl From<crate::dpdk_runtime::DpdkUnsupportedTxCapture> for DpdkUnsupportedTxCaptureExport {
    fn from(value: crate::dpdk_runtime::DpdkUnsupportedTxCapture) -> Self {
        Self {
            signature_b58: value.signature_b58,
            signer_pubkeys: value.signer_pubkeys,
            static_account_keys: value.static_account_keys,
            flattened_account_keys: value.flattened_account_keys,
            instruction_program_ids: value.instruction_program_ids,
            unsupported_program_ids: value.unsupported_program_ids,
            raw_tx_hex: value.raw_tx_hex,
            is_versioned: value.is_versioned,
            instruction_count: value.instruction_count,
        }
    }
}

pub const ROLLOUT_GATE_EXPORT_INTERVAL_NS: u64 = 1_000_000_000;

const DEFAULT_PLACEHOLDER_PROGRAM_ID: &str = "11111111111111111111111111111111";

#[derive(Debug)]
pub enum RolloutGateExportError {
    Gate(RolloutGateError),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for RolloutGateExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gate(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RolloutGateExportError {}

impl From<RolloutGateError> for RolloutGateExportError {
    fn from(value: RolloutGateError) -> Self {
        Self::Gate(value)
    }
}

impl From<std::io::Error> for RolloutGateExportError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RolloutGateExportError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn rollout_gate_input_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("g12_gate_input.json")
}

pub fn latency_by_source_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("g12_latency_by_source.json")
}

pub fn dpdk_path_diagnostics_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("g12_dpdk_path_diagnostics.json")
}

pub fn runls5b_feed_to_process_intent_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5b_feed_to_process_intent_report.json")
}

pub fn runls5b_source_reject_reasons_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5b_source_reject_reasons.json")
}

pub fn runls5b_worker_stage_counts_by_source_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5b_worker_stage_counts_by_source.json")
}

pub fn runls5c_process_intent_subreason_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5c_process_intent_subreason_report.json")
}

pub fn runls5c_process_intent_subreason_by_source_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5c_process_intent_subreason_by_source.json")
}

pub fn runls5c_process_intent_subreason_rows_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5c_process_intent_subreason_rows.jsonl")
}

pub fn runls5d_process_intent_subreason_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5d_process_intent_subreason_report.json")
}

pub fn runls5d_rollout_gate_subreason_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5d_rollout_gate_subreason_report.json")
}

pub fn runls5d_no_send_profile_gate_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5d_no_send_profile_gate_report.json")
}

pub fn runls5d_source_stage_counts_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5d_source_stage_counts.json")
}

pub fn runls5e_research_sink_report_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("runls5e_research_sink_report.json")
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5bWorkerStageCountsBySource {
    afxdp: RunLs5bWorkerStageCounts,
    laserstream: RunLs5bWorkerStageCounts,
    external_geyser: RunLs5bWorkerStageCounts,
    local_agave: RunLs5bWorkerStageCounts,
    other: RunLs5bWorkerStageCounts,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5bRejectReasonsBySource {
    afxdp: RunLs5bSourceRejectReasonSummary,
    laserstream: RunLs5bSourceRejectReasonSummary,
    external_geyser: RunLs5bSourceRejectReasonSummary,
    local_agave: RunLs5bSourceRejectReasonSummary,
    other: RunLs5bSourceRejectReasonSummary,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5bSourceRejectReasonSummary {
    top_reject_reason: String,
    counts: RunLs5bRejectReasonCounts,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5bFeedToProcessIntentReport {
    laserstream_normalized_tx: u64,
    laserstream_feed_to_worker: u64,
    laserstream_worker_read: u64,
    laserstream_process_intent_attempt: u64,
    laserstream_rollout_emitted: u64,
    afxdp_private_ingress: u64,
    afxdp_feed_to_worker: u64,
    afxdp_worker_read: u64,
    afxdp_process_intent_attempt: u64,
    afxdp_rollout_emitted: u64,
    top_reject_reason_laserstream: String,
    top_reject_reason_afxdp: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5cOwnerModeSummary {
    total: u64,
    top_pi_disabled_subreason: String,
    counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5cSourceSummary {
    total: u64,
    top_pi_disabled_subreason: String,
    counts: BTreeMap<String, u64>,
    by_owner_mode: BTreeMap<String, RunLs5cOwnerModeSummary>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5cProcessIntentSubreasonBySource {
    afxdp: RunLs5cSourceSummary,
    laserstream: RunLs5cSourceSummary,
    external_geyser: RunLs5cSourceSummary,
    local_agave: RunLs5cSourceSummary,
    other: RunLs5cSourceSummary,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5cProcessIntentSubreasonReport {
    laserstream_normalized_tx: u64,
    laserstream_feed_to_worker: u64,
    laserstream_worker_read: u64,
    laserstream_process_intent_attempt: u64,
    laserstream_process_intent_ok: u64,
    laserstream_rollout_emitted: u64,
    laserstream_top_pi_disabled_subreason: String,
    afxdp_private_ingress: u64,
    afxdp_feed_to_worker: u64,
    afxdp_worker_read: u64,
    afxdp_process_intent_attempt: u64,
    afxdp_process_intent_ok: u64,
    afxdp_rollout_emitted: u64,
    afxdp_top_pi_disabled_subreason: String,
    by_owner_mode: BTreeMap<String, BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
struct RunLs5dProcessIntentSubreasonReport {
    laserstream_feed_to_worker: u64,
    laserstream_process_intent_attempt: u64,
    laserstream_process_intent_ok: u64,
    laserstream_rollout_emitted: u64,
    laserstream_top_no_send_subreason: String,
    laserstream_top_rollout_subreason: String,
    afxdp_feed_to_worker: u64,
    afxdp_process_intent_attempt: u64,
    afxdp_process_intent_ok: u64,
    afxdp_rollout_emitted: u64,
    afxdp_top_no_send_subreason: String,
    afxdp_top_rollout_subreason: String,
    by_owner_mode: BTreeMap<String, BTreeMap<String, u64>>,
}

fn runls5b_top_reject_reason(counts: &RunLs5bRejectReasonCounts) -> &'static str {
    let ranked = [
        (
            RunLs5bRejectReason::ObserveOnlyMode,
            counts.reject_observe_only_mode,
        ),
        (RunLs5bRejectReason::SourceGrade, counts.reject_source_grade),
        (
            RunLs5bRejectReason::NotPrivateGrade,
            counts.reject_not_private_grade,
        ),
        (RunLs5bRejectReason::NoPool, counts.reject_no_pool),
        (
            RunLs5bRejectReason::PoolTagMiss,
            counts.reject_pool_tag_miss,
        ),
        (
            RunLs5bRejectReason::UnsupportedVenue,
            counts.reject_unsupported_venue,
        ),
        (RunLs5bRejectReason::NoRoute, counts.reject_no_route),
        (RunLs5bRejectReason::NoQuote, counts.reject_no_quote),
        (RunLs5bRejectReason::NoMintMeta, counts.reject_no_mint_meta),
        (
            RunLs5bRejectReason::ProcessIntentDisabled,
            counts.reject_process_intent_disabled,
        ),
        (
            RunLs5bRejectReason::OwnerModeDisabled,
            counts.reject_owner_mode_disabled,
        ),
        (
            RunLs5bRejectReason::PolicySource,
            counts.reject_policy_source,
        ),
        (
            RunLs5bRejectReason::HealthSnapshotMissing,
            counts.reject_health_snapshot_missing,
        ),
        (
            RunLs5bRejectReason::HealthUnhealthy,
            counts.reject_health_unhealthy,
        ),
        (RunLs5bRejectReason::StaleSlot, counts.reject_stale_slot),
        (
            RunLs5bRejectReason::UnknownSignal,
            counts.reject_unknown_signal,
        ),
    ];
    let mut best = ("none", 0u64);
    for (reason, count) in ranked {
        if count > best.1 {
            best = (
                match reason {
                    RunLs5bRejectReason::ObserveOnlyMode => "reject_observe_only_mode",
                    RunLs5bRejectReason::SourceGrade => "reject_source_grade",
                    RunLs5bRejectReason::NotPrivateGrade => "reject_not_private_grade",
                    RunLs5bRejectReason::NoPool => "reject_no_pool",
                    RunLs5bRejectReason::PoolTagMiss => "reject_pool_tag_miss",
                    RunLs5bRejectReason::UnsupportedVenue => "reject_unsupported_venue",
                    RunLs5bRejectReason::NoRoute => "reject_no_route",
                    RunLs5bRejectReason::NoQuote => "reject_no_quote",
                    RunLs5bRejectReason::NoMintMeta => "reject_no_mint_meta",
                    RunLs5bRejectReason::ProcessIntentDisabled => "reject_process_intent_disabled",
                    RunLs5bRejectReason::OwnerModeDisabled => "reject_owner_mode_disabled",
                    RunLs5bRejectReason::PolicySource => "reject_policy_source",
                    RunLs5bRejectReason::HealthSnapshotMissing => "reject_health_snapshot_missing",
                    RunLs5bRejectReason::HealthUnhealthy => "reject_health_unhealthy",
                    RunLs5bRejectReason::StaleSlot => "reject_stale_slot",
                    RunLs5bRejectReason::UnknownSignal => "reject_unknown_signal",
                },
                count,
            );
        }
    }
    best.0
}

fn runls5b_reject_summary(source: &RunLs5bSourceAutopsy) -> RunLs5bSourceRejectReasonSummary {
    RunLs5bSourceRejectReasonSummary {
        top_reject_reason: runls5b_top_reject_reason(&source.reject_reasons).to_string(),
        counts: source.reject_reasons.clone(),
    }
}

fn runls5b_worker_stage_counts_by_source(
    snapshot: &RunLs5bAutopsySnapshot,
) -> RunLs5bWorkerStageCountsBySource {
    RunLs5bWorkerStageCountsBySource {
        afxdp: snapshot.afxdp.worker_stages.clone(),
        laserstream: snapshot.laserstream.worker_stages.clone(),
        external_geyser: snapshot.external_geyser.worker_stages.clone(),
        local_agave: snapshot.local_agave.worker_stages.clone(),
        other: snapshot.other.worker_stages.clone(),
    }
}

fn runls5b_source_reject_reasons(
    snapshot: &RunLs5bAutopsySnapshot,
) -> RunLs5bRejectReasonsBySource {
    RunLs5bRejectReasonsBySource {
        afxdp: runls5b_reject_summary(&snapshot.afxdp),
        laserstream: runls5b_reject_summary(&snapshot.laserstream),
        external_geyser: runls5b_reject_summary(&snapshot.external_geyser),
        local_agave: runls5b_reject_summary(&snapshot.local_agave),
        other: runls5b_reject_summary(&snapshot.other),
    }
}

fn runls5b_feed_to_process_intent_report(
    snapshot: &RunLs5bAutopsySnapshot,
    grpc_diag: &crate::grpc_diag::GrpcIngestDiagnosticsSnapshot,
    afxdp_raw_diag: &AfxdpRawGateSnapshot,
) -> RunLs5bFeedToProcessIntentReport {
    RunLs5bFeedToProcessIntentReport {
        laserstream_normalized_tx: grpc_diag.grpc_normalize_ok_laserstream,
        laserstream_feed_to_worker: grpc_diag.grpc_feed_enqueue_ok_laserstream,
        laserstream_worker_read: snapshot.laserstream.worker_stages.worker_ring_read_ok,
        laserstream_process_intent_attempt: snapshot
            .laserstream
            .worker_stages
            .worker_process_intent_attempt,
        laserstream_rollout_emitted: snapshot
            .laserstream
            .worker_stages
            .worker_rollout_event_emitted,
        afxdp_private_ingress: afxdp_raw_diag.tx_record_emit_ok,
        afxdp_feed_to_worker: afxdp_raw_diag.feed_enqueue_ok,
        afxdp_worker_read: snapshot.afxdp.worker_stages.worker_ring_read_ok,
        afxdp_process_intent_attempt: snapshot.afxdp.worker_stages.worker_process_intent_attempt,
        afxdp_rollout_emitted: snapshot.afxdp.worker_stages.worker_rollout_event_emitted,
        top_reject_reason_laserstream: runls5b_top_reject_reason(
            &snapshot.laserstream.reject_reasons,
        )
        .to_string(),
        top_reject_reason_afxdp: runls5b_top_reject_reason(&snapshot.afxdp.reject_reasons)
            .to_string(),
    }
}

fn runls5c_top_subreason(counts: &BTreeMap<String, u64>) -> String {
    counts
        .iter()
        .max_by_key(|(reason, count)| (**count, std::cmp::Reverse(reason.as_str())))
        .map(|(reason, _)| reason.clone())
        .unwrap_or_else(|| "none".to_string())
}

fn runls5c_source_summary(
    rows: &[RunLs5cProcessIntentSubreasonRow],
    source_name: &str,
) -> RunLs5cSourceSummary {
    let mut counts = BTreeMap::new();
    let mut by_owner_mode: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut total = 0u64;

    for row in rows.iter().filter(|row| row.source == source_name) {
        total += 1;
        *counts.entry(row.first_subreason.clone()).or_insert(0) += 1;
        *by_owner_mode
            .entry(row.owner_mode.clone())
            .or_default()
            .entry(row.first_subreason.clone())
            .or_insert(0) += 1;
    }

    let by_owner_mode = by_owner_mode
        .into_iter()
        .map(|(owner_mode, counts)| {
            let total = counts.values().copied().sum();
            let top = runls5c_top_subreason(&counts);
            (
                owner_mode,
                RunLs5cOwnerModeSummary {
                    total,
                    top_pi_disabled_subreason: top,
                    counts,
                },
            )
        })
        .collect();

    RunLs5cSourceSummary {
        total,
        top_pi_disabled_subreason: runls5c_top_subreason(&counts),
        counts,
        by_owner_mode,
    }
}

fn runls5d_matches_prefixes(subreason: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| subreason.starts_with(prefix))
}

fn runls5d_source_summary_with_prefixes(
    rows: &[RunLs5cProcessIntentSubreasonRow],
    source_name: &str,
    prefixes: &[&str],
) -> RunLs5cSourceSummary {
    let mut counts = BTreeMap::new();
    let mut by_owner_mode: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut total = 0u64;

    for row in rows.iter().filter(|row| {
        row.source == source_name && runls5d_matches_prefixes(&row.first_subreason, prefixes)
    }) {
        total += 1;
        *counts.entry(row.first_subreason.clone()).or_insert(0) += 1;
        *by_owner_mode
            .entry(row.owner_mode.clone())
            .or_default()
            .entry(row.first_subreason.clone())
            .or_insert(0) += 1;
    }

    let by_owner_mode = by_owner_mode
        .into_iter()
        .map(|(owner_mode, counts)| {
            let total = counts.values().copied().sum();
            let top = runls5c_top_subreason(&counts);
            (
                owner_mode,
                RunLs5cOwnerModeSummary {
                    total,
                    top_pi_disabled_subreason: top,
                    counts,
                },
            )
        })
        .collect();

    RunLs5cSourceSummary {
        total,
        top_pi_disabled_subreason: runls5c_top_subreason(&counts),
        counts,
        by_owner_mode,
    }
}

fn runls5c_process_intent_subreason_by_source(
    rows: &[RunLs5cProcessIntentSubreasonRow],
) -> RunLs5cProcessIntentSubreasonBySource {
    RunLs5cProcessIntentSubreasonBySource {
        afxdp: runls5c_source_summary(rows, "afxdp"),
        laserstream: runls5c_source_summary(rows, "laserstream"),
        external_geyser: runls5c_source_summary(rows, "external_geyser"),
        local_agave: runls5c_source_summary(rows, "local_agave"),
        other: runls5c_source_summary(rows, "other"),
    }
}

fn runls5d_process_intent_subreason_by_source_with_prefixes(
    rows: &[RunLs5cProcessIntentSubreasonRow],
    prefixes: &[&str],
) -> RunLs5cProcessIntentSubreasonBySource {
    RunLs5cProcessIntentSubreasonBySource {
        afxdp: runls5d_source_summary_with_prefixes(rows, "afxdp", prefixes),
        laserstream: runls5d_source_summary_with_prefixes(rows, "laserstream", prefixes),
        external_geyser: runls5d_source_summary_with_prefixes(rows, "external_geyser", prefixes),
        local_agave: runls5d_source_summary_with_prefixes(rows, "local_agave", prefixes),
        other: runls5d_source_summary_with_prefixes(rows, "other", prefixes),
    }
}

fn runls5c_by_owner_mode_totals(
    rows: &[RunLs5cProcessIntentSubreasonRow],
) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut by_owner_mode: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for row in rows {
        *by_owner_mode
            .entry(row.owner_mode.clone())
            .or_default()
            .entry(row.first_subreason.clone())
            .or_insert(0) += 1;
    }
    by_owner_mode
}

fn runls5c_process_intent_subreason_report(
    rows: &[RunLs5cProcessIntentSubreasonRow],
    snapshot: &RunLs5bAutopsySnapshot,
    grpc_diag: &crate::grpc_diag::GrpcIngestDiagnosticsSnapshot,
    afxdp_raw_diag: &AfxdpRawGateSnapshot,
) -> RunLs5cProcessIntentSubreasonReport {
    let by_source = runls5c_process_intent_subreason_by_source(rows);
    RunLs5cProcessIntentSubreasonReport {
        laserstream_normalized_tx: grpc_diag.grpc_normalize_ok_laserstream,
        laserstream_feed_to_worker: grpc_diag.grpc_feed_enqueue_ok_laserstream,
        laserstream_worker_read: snapshot.laserstream.worker_stages.worker_ring_read_ok,
        laserstream_process_intent_attempt: snapshot
            .laserstream
            .worker_stages
            .worker_process_intent_attempt,
        laserstream_process_intent_ok: snapshot.laserstream.worker_stages.worker_process_intent_ok,
        laserstream_rollout_emitted: snapshot
            .laserstream
            .worker_stages
            .worker_rollout_event_emitted,
        laserstream_top_pi_disabled_subreason: by_source
            .laserstream
            .top_pi_disabled_subreason
            .clone(),
        afxdp_private_ingress: afxdp_raw_diag.tx_record_emit_ok,
        afxdp_feed_to_worker: afxdp_raw_diag.feed_enqueue_ok,
        afxdp_worker_read: snapshot.afxdp.worker_stages.worker_ring_read_ok,
        afxdp_process_intent_attempt: snapshot.afxdp.worker_stages.worker_process_intent_attempt,
        afxdp_process_intent_ok: snapshot.afxdp.worker_stages.worker_process_intent_ok,
        afxdp_rollout_emitted: snapshot.afxdp.worker_stages.worker_rollout_event_emitted,
        afxdp_top_pi_disabled_subreason: by_source.afxdp.top_pi_disabled_subreason.clone(),
        by_owner_mode: runls5c_by_owner_mode_totals(rows),
    }
}

fn runls5d_process_intent_subreason_report(
    rows: &[RunLs5cProcessIntentSubreasonRow],
    snapshot: &RunLs5bAutopsySnapshot,
    grpc_diag: &crate::grpc_diag::GrpcIngestDiagnosticsSnapshot,
    afxdp_raw_diag: &AfxdpRawGateSnapshot,
) -> RunLs5dProcessIntentSubreasonReport {
    let no_send_prefixes = [
        "reject_pi_no_send_",
        "reject_pi_live_disabled_but_research_allowed",
        "reject_pi_owner_mode_not_allowed_in_profile",
        "reject_pi_route_family_not_allowed_in_profile",
        "reject_pi_source_allowed_observe_only",
        "reject_pi_private_route_requires_bundle_lane",
        "reject_pi_strategy_requires_live_profile",
        "reject_pi_unknown_no_send_mode",
    ];
    let rollout_prefixes = ["reject_rollout_gate_"];
    let no_send_by_source =
        runls5d_process_intent_subreason_by_source_with_prefixes(rows, &no_send_prefixes);
    let rollout_by_source =
        runls5d_process_intent_subreason_by_source_with_prefixes(rows, &rollout_prefixes);
    RunLs5dProcessIntentSubreasonReport {
        laserstream_feed_to_worker: grpc_diag.grpc_feed_enqueue_ok_laserstream,
        laserstream_process_intent_attempt: snapshot
            .laserstream
            .worker_stages
            .worker_process_intent_attempt,
        laserstream_process_intent_ok: snapshot.laserstream.worker_stages.worker_process_intent_ok,
        laserstream_rollout_emitted: snapshot
            .laserstream
            .worker_stages
            .worker_rollout_event_emitted,
        laserstream_top_no_send_subreason: no_send_by_source
            .laserstream
            .top_pi_disabled_subreason
            .clone(),
        laserstream_top_rollout_subreason: rollout_by_source
            .laserstream
            .top_pi_disabled_subreason
            .clone(),
        afxdp_feed_to_worker: afxdp_raw_diag.feed_enqueue_ok,
        afxdp_process_intent_attempt: snapshot.afxdp.worker_stages.worker_process_intent_attempt,
        afxdp_process_intent_ok: snapshot.afxdp.worker_stages.worker_process_intent_ok,
        afxdp_rollout_emitted: snapshot.afxdp.worker_stages.worker_rollout_event_emitted,
        afxdp_top_no_send_subreason: no_send_by_source.afxdp.top_pi_disabled_subreason.clone(),
        afxdp_top_rollout_subreason: rollout_by_source.afxdp.top_pi_disabled_subreason.clone(),
        by_owner_mode: runls5c_by_owner_mode_totals(rows),
    }
}

pub fn write_rollout_gate_artifacts(
    cfg: &ShadowConfig,
    pm_stats: &PmLoopStats,
    egress_health: Option<EgressHealthPayload32B>,
) -> Result<Option<(PathBuf, PathBuf)>, RolloutGateExportError> {
    let Some(report_path) = rollout_gate_report_path(cfg) else {
        return Ok(None);
    };
    let input_path = rollout_gate_input_path(&report_path);
    let latency_path = latency_by_source_path(&report_path);
    let dpdk_diag_path = dpdk_path_diagnostics_path(&report_path);
    let runls5b_report_path = runls5b_feed_to_process_intent_report_path(&report_path);
    let runls5b_rejects_path = runls5b_source_reject_reasons_path(&report_path);
    let runls5b_worker_counts_path = runls5b_worker_stage_counts_by_source_path(&report_path);
    let runls5c_report_path = runls5c_process_intent_subreason_report_path(&report_path);
    let runls5c_by_source_path = runls5c_process_intent_subreason_by_source_path(&report_path);
    let runls5c_rows_path = runls5c_process_intent_subreason_rows_path(&report_path);
    let runls5d_report_path = runls5d_process_intent_subreason_report_path(&report_path);
    let runls5d_rollout_path = runls5d_rollout_gate_subreason_report_path(&report_path);
    let runls5d_no_send_path = runls5d_no_send_profile_gate_report_path(&report_path);
    let runls5d_stage_counts_path = runls5d_source_stage_counts_path(&report_path);
    let runls5e_research_sink_path = runls5e_research_sink_report_path(&report_path);
    let input = build_gate_evaluator_input(cfg, pm_stats, egress_health);
    write_gate_evaluator_input(&input_path, &input)?;
    write_latency_by_source_snapshot(&latency_path)?;
    write_dpdk_path_diagnostics_snapshot(&dpdk_diag_path)?;
    write_runls5b_artifacts(
        &runls5b_report_path,
        &runls5b_rejects_path,
        &runls5b_worker_counts_path,
    )?;
    write_runls5c_artifacts(
        &runls5c_report_path,
        &runls5c_by_source_path,
        &runls5c_rows_path,
    )?;
    write_runls5d_artifacts(
        &runls5d_report_path,
        &runls5d_rollout_path,
        &runls5d_no_send_path,
        &runls5d_stage_counts_path,
    )?;
    write_runls5e_research_sink_artifact(&runls5e_research_sink_path)?;
    let _ = evaluate_gate_report_from_input(&input, &report_path)?;
    Ok(Some((input_path, report_path)))
}

pub fn build_gate_evaluator_input(
    cfg: &ShadowConfig,
    pm_stats: &PmLoopStats,
    egress_health: Option<EgressHealthPayload32B>,
) -> G12GateEvaluatorInput {
    let rollout = cfg
        .rollout
        .as_ref()
        .expect("rollout config required when building G12 gate input");
    let ledger = pm_stats.rollout_ledger;
    let observe_candidates = ledger.observed;
    let live_landed = pm_stats.release.landed;
    let worker_diag = snapshot_worker_diagnostics();
    let grpc_diag = snapshot_grpc_ingest_diagnostics();
    let afxdp_raw_diag = snapshot_afxdp_gate_diagnostics();
    let grpc_unsupported =
        encode_grpc_unsupported_program_top(&grpc_diag.grpc_unsupported_program_top);
    let slot_window = if observe_candidates == 0 {
        0
    } else {
        ledger
            .last_observed_slot
            .saturating_sub(ledger.first_observed_slot)
            .saturating_add(1)
    };
    let guard_program_id_present = cfg
        .execution
        .as_ref()
        .map(|execution| {
            let raw = execution.shadow_guard_program_id.trim();
            !raw.is_empty() && raw != DEFAULT_PLACEHOLDER_PROGRAM_ID
        })
        .unwrap_or(false);
    let egress_gate_enabled = cfg.submit.routing.pre_sign_egress_admission_gate;
    let report_path = rollout_gate_report_path(cfg).unwrap_or_else(|| {
        PathBuf::from(shadow_control::rollout_gate::DEFAULT_G12_GATE_REPORT_PATH)
    });
    let private_lane_admissibility_ok = egress_health
        .map(|payload| {
            payload.is_jito_admissible()
                && (!cfg.submit.routing.enable_harmonic_lane || payload.is_harmonic_admissible())
        })
        .unwrap_or(false);
    let proof_paths = proof_paths_for_report(&report_path);
    let latency_proof = read_latency_proof(&proof_paths.latency_proof_path);
    let tail_latency = crate::tail_latency::snapshot();
    let shadow_guard_preflight =
        read_shadow_guard_preflight(&proof_paths.shadow_guard_preflight_path);
    let proxy_diag = snapshot_proxy_diagnostics();
    let effective_public_scout_attempt_latch_first_signature = worker_diag
        .public_scout_attempt_latch_first_signature
        .or(proxy_diag.first_public_rpc_attempt_signature)
        .or(proxy_diag.last_public_rpc_rejection_signature);
    let effective_public_scout_attempt_latch_set = if worker_diag.public_scout_attempt_latch_set > 0
    {
        worker_diag.public_scout_attempt_latch_set
    } else if effective_public_scout_attempt_latch_first_signature.is_some()
        || proxy_diag.public_rpc_transport_attempt > 0
    {
        1
    } else {
        0
    };
    let inventory_truth_ok = pm_stats
        .last_drift
        .map(|drift| drift.is_zero())
        .unwrap_or(false);
    let fee_leak_accounted = live_landed == 0;
    let phantom_inventory_risk = pm_stats
        .last_drift
        .map(|drift| !drift.is_zero())
        .unwrap_or(true);
    let sparse_classes_fallback_safe = cfg
        .oracle
        .as_ref()
        .and_then(|oracle| oracle.guerrila.as_ref())
        .and_then(|guerrila| guerrila.sparse_class_action)
        == Some(SparseClassAction::FallbackToBaseline)
        && cfg
            .oracle
            .as_ref()
            .and_then(|oracle| oracle.opso.as_ref())
            .map(|opso| opso.sparse_class_action == SparseClassAction::FallbackToBaseline)
            .unwrap_or(false);
    let baseline_shadow_comparator_enabled = cfg
        .oracle
        .as_ref()
        .and_then(|oracle| oracle.guerrila.as_ref())
        .map(|guerrila| {
            guerrila.primary_strategy_mode == Some(StrategyMode::Baseline)
                && guerrila.shadow_strategy_mode == Some(ShadowStrategyMode::CompetitionAdjusted)
        })
        .unwrap_or(false);
    let tick_boundary_shadow_only = cfg
        .quant_tactics
        .as_ref()
        .and_then(|qt| qt.tick_boundary_sabotage.as_ref())
        .map(|tick| tick.enabled && tick.mode == TickBoundarySabotageMode::ShadowObserveOnly)
        .unwrap_or(false);
    let tactical_lane = G12TacticalLaneEvidence {
        harmonic_atba_routes: if cfg.submit.routing.atba_route_mode == Some(AtbaRouteMode::JitoOnly)
        {
            0
        } else {
            1
        },
        harmonic_apex_partial_routes: if cfg.submit.routing.apex_partial_route_mode
            == Some(ApexPartialRouteMode::JitoOnly)
        {
            0
        } else {
            1
        },
        harmonic_exhaust_scavenge_routes: if cfg
            .quant_tactics
            .as_ref()
            .and_then(|qt| qt.post_trade_exhaust_scavenging.as_ref())
            .map(|exhaust| {
                exhaust.enabled
                    && exhaust.route_mode
                        == ExhaustScavengingRouteMode::DeterministicJitoBackrunOnly
            })
            .unwrap_or(false)
        {
            0
        } else {
            1
        },
    };

    G12GateEvaluatorInput {
        rung: rollout_cap_rung_code(rollout.cap_rung),
        sample_slots: slot_window,
        observe_candidates,
        observe_candidates_total: observe_candidates,
        observe_candidates_external_geyser: ledger.observe_candidates_external_geyser,
        observe_candidates_local_validator_geyser: ledger.observe_candidates_local_validator_geyser,
        observe_candidates_private_grade: ledger.observe_candidates_private_grade,
        live_landed,
        min_sample_slots: rollout.min_sample_slots,
        min_observe_candidates: rollout.min_observe_candidates,
        min_live_landed: rollout.min_live_landed,
        worker_records_seen: worker_diag.records_seen,
        worker_health_snapshot_missing: worker_diag.health_snapshot_missing,
        worker_health_drops: worker_diag.health_drops,
        worker_stale_slot_drops: worker_diag.stale_slot_drops,
        worker_pool_state_miss: worker_diag.pool_state_miss,
        worker_pool_state_miss_tag_map_absent: worker_diag.worker_pool_state_miss_tag_map_absent,
        worker_pool_state_miss_arena_oob: worker_diag.worker_pool_state_miss_arena_oob,
        worker_pool_state_miss_arena_torn: worker_diag.worker_pool_state_miss_arena_torn,
        worker_pool_state_miss_pubkey_mismatch: worker_diag.worker_pool_state_miss_pubkey_mismatch,
        last_worker_pool_state_miss_pool_pubkey: if worker_diag
            .last_worker_pool_state_miss_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_worker_pool_state_miss_pool_pubkey).into_string()
        },
        last_worker_pool_state_miss_venue_id: worker_diag.last_worker_pool_state_miss_venue_id,
        last_worker_pool_state_miss_signal_source: worker_diag
            .last_worker_pool_state_miss_signal_source,
        last_worker_pool_state_miss_reason_code: worker_diag
            .last_worker_pool_state_miss_reason_code,
        worker_pool_state_miss_recent_ring: worker_diag
            .worker_pool_state_miss_recent_ring
            .iter()
            .map(|entry| G12WorkerPoolStateMissRecent {
                timestamp_ns: entry.timestamp_ns,
                pool_pubkey: if entry.pool_pubkey == [0u8; 32] {
                    String::new()
                } else {
                    bs58::encode(entry.pool_pubkey).into_string()
                },
                venue_id: entry.venue_id,
                signal_source: entry.signal_source,
                reason_code: entry.reason_code,
                record_slot: entry.record_slot,
                health_snapshot_slot: entry.health_snapshot_slot,
            })
            .collect(),
        worker_pool_state_miss_top_recent: worker_diag
            .worker_pool_state_miss_top_recent
            .iter()
            .map(|entry| G12WorkerPoolStateMissTopRecent {
                pool_pubkey: if entry.pool_pubkey == [0u8; 32] {
                    String::new()
                } else {
                    bs58::encode(entry.pool_pubkey).into_string()
                },
                count: entry.count,
                first_seen_ns: entry.first_seen_ns,
                last_seen_ns: entry.last_seen_ns,
                reason_code: entry.reason_code,
            })
            .collect(),
        worker_pool_state_hit: worker_diag.pool_state_hit,
        worker_early_drop_mint_in_tag_missing: worker_diag.early_drop_mint_in_tag_missing,
        worker_early_drop_mint_out_tag_missing: worker_diag.early_drop_mint_out_tag_missing,
        worker_shadow_events_published: worker_diag.shadow_events_published,
        worker_rollout_would_send_events_published: worker_diag.rollout_would_send_events_published,
        worker_rollout_live_send_suppressed_events_published: worker_diag
            .rollout_live_send_suppressed_events_published,
        worker_candidates_entered_process_intent: worker_diag.candidates_entered_process_intent,
        worker_candidates_dropped_before_process_intent: worker_diag
            .candidates_dropped_before_process_intent,
        worker_process_intent_ok: worker_diag.process_intent_ok,
        worker_process_intent_err: worker_diag.process_intent_err,
        last_process_intent_error: worker_diag.last_process_intent_error.clone(),
        last_process_intent_error_stage: worker_diag.last_process_intent_error_stage.clone(),
        last_process_intent_error_reason: worker_diag.last_process_intent_error_reason.clone(),
        worker_quote_ok: worker_diag.quote_ok,
        worker_quote_err: worker_diag.quote_err,
        worker_lookup_ok: worker_diag.lookup_ok,
        worker_policy_ok: worker_diag.policy_ok,
        worker_assembly_ready: worker_diag.assembly_ready,
        worker_pm_reservation_created: worker_diag.pm_reservation_created,
        worker_route_resolved_jito: worker_diag.route_resolved_jito,
        worker_route_resolved_public_rpc_scout: worker_diag.route_resolved_public_rpc_scout,
        worker_route_resolved_harmonic: worker_diag.route_resolved_harmonic,
        worker_route_resolved_drop: worker_diag.route_resolved_drop,
        worker_public_rpc_scout_emit_attempt: worker_diag.public_rpc_scout_emit_attempt,
        worker_public_rpc_scout_emit_ok: worker_diag.public_rpc_scout_emit_ok,
        run38_target_public_scout_clmm_route_resolved: worker_diag
            .run38_target_public_scout_clmm_route_resolved,
        run38_target_public_scout_clmm_r1_attempt: worker_diag
            .run38_target_public_scout_clmm_r1_attempt,
        run38_target_public_scout_clmm_reject_proof_filter: worker_diag
            .run38_target_public_scout_clmm_reject_proof_filter,
        run38_target_public_scout_clmm_reject_micro_market: worker_diag
            .run38_target_public_scout_clmm_reject_micro_market,
        run38_target_public_scout_clmm_reject_inventory: worker_diag
            .run38_target_public_scout_clmm_reject_inventory,
        run38_target_public_scout_clmm_reject_inventory_missing_input_ata: worker_diag
            .run38_target_public_scout_clmm_reject_inventory_missing_input_ata,
        run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: worker_diag
            .run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized,
        run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: worker_diag
            .run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient,
        run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: worker_diag
            .run38_target_public_scout_clmm_reject_inventory_token_program_mismatch,
        run38_target_public_scout_clmm_reject_inventory_other: worker_diag
            .run38_target_public_scout_clmm_reject_inventory_other,
        run38_target_public_scout_clmm_reject_template_executability: worker_diag
            .run38_target_public_scout_clmm_reject_template_executability,
        run38_target_public_scout_clmm_reject_attempt_latch: worker_diag
            .run38_target_public_scout_clmm_reject_attempt_latch,
        run38_target_public_scout_clmm_admit: worker_diag.run38_target_public_scout_clmm_admit,
        run38_target_public_scout_clmm_process_intent: worker_diag
            .run38_target_public_scout_clmm_process_intent,
        run38_target_public_scout_clmm_emit_attempt: worker_diag
            .run38_target_public_scout_clmm_emit_attempt,
        run38_target_public_scout_clmm_transport_attempt: worker_diag
            .run38_target_public_scout_clmm_transport_attempt,
        run38_target_public_scout_clmm_pm_created: worker_diag
            .run38_target_public_scout_clmm_pm_created,
        public_scout_attempt_latch_set: effective_public_scout_attempt_latch_set,
        public_scout_attempt_latch_drop_count: worker_diag.public_scout_attempt_latch_drop_count,
        public_scout_attempt_latch_first_signature:
            effective_public_scout_attempt_latch_first_signature
                .map(|signature| bs58::encode(signature).into_string())
                .unwrap_or_default(),
        pm_reserve_attempt: worker_diag.pm_reserve_attempt,
        pm_reserve_ok: worker_diag.pm_reserve_ok,
        pm_reserve_reject_no_live_mode: worker_diag.pm_reserve_reject_no_live_mode,
        pm_reserve_reject_no_send_mode: worker_diag.pm_reserve_reject_no_send_mode,
        pm_reserve_reject_route_not_live: worker_diag.pm_reserve_reject_route_not_live,
        pm_reserve_reject_cap: worker_diag.pm_reserve_reject_cap,
        pm_reserve_reject_inventory: worker_diag.pm_reserve_reject_inventory,
        pm_reserve_reject_missing_signature: worker_diag.pm_reserve_reject_missing_signature,
        pm_reserve_reject_other: worker_diag.pm_reserve_reject_other,
        last_pm_reserve_reject_reason: worker_diag.last_pm_reserve_reject_reason.clone(),
        last_pm_reserve_route: worker_diag.last_pm_reserve_route.clone(),
        last_pm_reserve_pool: if worker_diag.last_pm_reserve_pool_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_pm_reserve_pool_pubkey).into_string()
        },
        last_pm_reserve_mint_in: if worker_diag.last_pm_reserve_mint_in_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_pm_reserve_mint_in_pubkey).into_string()
        },
        last_pm_reserve_mint_out: if worker_diag.last_pm_reserve_mint_out_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_pm_reserve_mint_out_pubkey).into_string()
        },
        last_pm_reserve_amount: worker_diag.last_pm_reserve_amount,
        clmm_input_ata_missing: worker_diag.clmm_input_ata_missing,
        clmm_output_ata_missing: worker_diag.clmm_output_ata_missing,
        clmm_input_ata_uninitialized: worker_diag.clmm_input_ata_uninitialized,
        clmm_output_ata_uninitialized: worker_diag.clmm_output_ata_uninitialized,
        clmm_input_balance_insufficient: worker_diag.clmm_input_balance_insufficient,
        clmm_token_program_mismatch: worker_diag.clmm_token_program_mismatch,
        last_clmm_input_mint: if worker_diag.last_clmm_input_mint_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_input_mint_pubkey).into_string()
        },
        last_clmm_output_mint: if worker_diag.last_clmm_output_mint_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_output_mint_pubkey).into_string()
        },
        last_clmm_input_token_account: if worker_diag.last_clmm_input_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_input_token_account_pubkey).into_string()
        },
        last_clmm_output_token_account: if worker_diag.last_clmm_output_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_output_token_account_pubkey).into_string()
        },
        template_drop_no_template: worker_diag.template_drop_no_template,
        template_drop_wrong_route_family: worker_diag.template_drop_wrong_route_family,
        template_drop_missing_clmm_accounts: worker_diag.template_drop_missing_clmm_accounts,
        template_drop_missing_input_ata: worker_diag.template_drop_missing_input_ata,
        template_drop_missing_output_ata: worker_diag.template_drop_missing_output_ata,
        template_drop_invalid_swap_program: worker_diag.template_drop_invalid_swap_program,
        template_drop_invalid_guard_template: worker_diag.template_drop_invalid_guard_template,
        template_drop_unsupported_token_program: worker_diag
            .template_drop_unsupported_token_program,
        template_drop_other: worker_diag.template_drop_other,
        last_template_drop_reason: worker_diag.last_template_drop_reason.clone(),
        last_template_drop_pool: if worker_diag.last_template_drop_pool_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_pool_pubkey).into_string()
        },
        last_template_drop_mint_in: if worker_diag.last_template_drop_mint_in_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_mint_in_pubkey).into_string()
        },
        last_template_drop_mint_out: if worker_diag.last_template_drop_mint_out_pubkey == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_mint_out_pubkey).into_string()
        },
        last_template_drop_route: worker_diag.last_template_drop_route.clone(),
        last_template_drop_template_program: if worker_diag
            .last_template_drop_template_program_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_template_program_pubkey).into_string()
        },
        last_template_drop_input_ata: if worker_diag.last_template_drop_input_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_input_token_account_pubkey).into_string()
        },
        last_template_drop_output_ata: if worker_diag.last_template_drop_output_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_template_drop_output_token_account_pubkey).into_string()
        },
        worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing,
        worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out,
        worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_zero_min_out,
        worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected,
        worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts,
        worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price,
        worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: worker_diag
            .pipeline_err_quote_clmm_no_quote_no_current_array_upstream,
        worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: worker_diag
            .pipeline_err_quote_clmm_no_quote_no_liquidity_boundary,
        worker_pipeline_err_quote_clmm_no_quote_out_of_range: worker_diag
            .pipeline_err_quote_clmm_no_quote_out_of_range,
        worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: worker_diag
            .pipeline_err_quote_clmm_no_quote_missing_tick_array_local,
        last_clmm_invalid_quote_pool: if worker_diag.last_clmm_invalid_quote_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_invalid_quote_pool_pubkey).into_string()
        },
        last_clmm_invalid_quote_mint_in: if worker_diag.last_clmm_invalid_quote_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_invalid_quote_mint_in_pubkey).into_string()
        },
        last_clmm_invalid_quote_mint_out: if worker_diag.last_clmm_invalid_quote_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_invalid_quote_mint_out_pubkey).into_string()
        },
        last_clmm_invalid_quote_amount_in: worker_diag.last_clmm_invalid_quote_amount_in,
        last_clmm_invalid_quote_expected_out: worker_diag.last_clmm_invalid_quote_expected_out,
        last_clmm_invalid_quote_min_out: worker_diag.last_clmm_invalid_quote_min_out,
        last_clmm_invalid_quote_projected_reserve_in: worker_diag
            .last_clmm_invalid_quote_projected_reserve_in,
        last_clmm_invalid_quote_projected_reserve_out: worker_diag
            .last_clmm_invalid_quote_projected_reserve_out,
        last_clmm_invalid_quote_quote_epoch: worker_diag.last_clmm_invalid_quote_quote_epoch,
        last_clmm_invalid_quote_last_update_unix_ns: worker_diag
            .last_clmm_invalid_quote_last_update_unix_ns,
        last_clmm_invalid_quote_post_sqrt_price_x64: worker_diag
            .last_clmm_invalid_quote_post_sqrt_price_x64
            .to_string(),
        last_clmm_invalid_quote_post_tick: worker_diag.last_clmm_invalid_quote_post_tick,
        last_clmm_invalid_quote_crossed_tick_count: worker_diag
            .last_clmm_invalid_quote_crossed_tick_count
            as u64,
        last_clmm_invalid_quote_direction: worker_diag.last_clmm_invalid_quote_direction as u64,
        last_clmm_invalid_quote_validity_flags: worker_diag.last_clmm_invalid_quote_validity_flags
            as u64,
        public_rpc_scout_enqueued: worker_diag.public_rpc_scout_emit_ok,
        public_rpc_transport_attempt: proxy_diag.public_rpc_transport_attempt,
        public_rpc_transport_failure: proxy_diag.public_rpc_transport_failure,
        public_rpc_transport_rejected: proxy_diag.public_rpc_transport_rejected,
        public_rpc_nack_recorded: proxy_diag.public_rpc_nack_recorded,
        last_public_rpc_rejection_code: proxy_diag.last_public_rpc_rejection_code,
        last_public_rpc_rejection_message: proxy_diag.last_public_rpc_rejection_message,
        last_public_rpc_rejection_body: proxy_diag.last_public_rpc_rejection_body,
        last_public_rpc_rejection_data_err: proxy_diag.last_public_rpc_rejection_data_err,
        last_public_rpc_rejection_simulation_logs: proxy_diag
            .last_public_rpc_rejection_simulation_logs,
        last_public_rpc_rejection_signature: proxy_diag
            .last_public_rpc_rejection_signature
            .map(|signature| bs58::encode(signature).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_tx_size: proxy_diag.last_public_rpc_rejection_tx_size,
        last_public_rpc_rejection_blockhash: proxy_diag
            .last_public_rpc_rejection_blockhash
            .map(|blockhash| bs58::encode(blockhash).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_blockhash_matches_current: proxy_diag
            .last_public_rpc_rejection_blockhash_matches_current,
        last_public_rpc_rejection_blockhash_age_ns: proxy_diag
            .last_public_rpc_rejection_blockhash_age_ns,
        last_public_rpc_rejection_cu_limit: proxy_diag.last_public_rpc_rejection_cu_limit,
        last_public_rpc_rejection_cu_price_micro_lamports: proxy_diag
            .last_public_rpc_rejection_cu_price_micro_lamports,
        last_public_rpc_rejection_reason: proxy_diag.last_public_rpc_rejection_reason,
        last_public_rpc_rejection_ix1_program_id: proxy_diag
            .last_public_rpc_rejection_ix1_program_id
            .map(|pubkey| bs58::encode(pubkey).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_ix1_account_0: proxy_diag
            .last_public_rpc_rejection_ix1_account_0
            .map(|pubkey| bs58::encode(pubkey).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_ix1_account_1: proxy_diag
            .last_public_rpc_rejection_ix1_account_1
            .map(|pubkey| bs58::encode(pubkey).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_ix1_account_count: proxy_diag
            .last_public_rpc_rejection_ix1_account_count,
        last_public_rpc_rejection_ix2_program_id: proxy_diag
            .last_public_rpc_rejection_ix2_program_id
            .map(|pubkey| bs58::encode(pubkey).into_string())
            .unwrap_or_default(),
        last_public_rpc_rejection_ix2_amount_in: proxy_diag.last_public_rpc_rejection_ix2_amount_in,
        last_public_rpc_rejection_ix2_min_out: proxy_diag.last_public_rpc_rejection_ix2_min_out,
        last_public_rpc_rejection_ix2_account_count: proxy_diag
            .last_public_rpc_rejection_ix2_account_count,
        last_public_rpc_rejection_actual_output_lamports: proxy_diag
            .last_public_rpc_rejection_actual_output_lamports,
        last_public_rpc_rejection_required_output_lamports: proxy_diag
            .last_public_rpc_rejection_required_output_lamports,
        pm_reservation_released: pm_stats.release.total_releases,
        pm_release_send_failed: pm_stats.release.send_failed,
        pm_release_nacked: pm_stats.release.nacked,
        worker_presign_gate_reject: worker_diag.presign_gate_reject,
        worker_template_build_ok: worker_diag.template_build_ok,
        worker_template_build_err: worker_diag.template_build_err,
        worker_no_send_backend_called: worker_diag.no_send_backend_called,
        worker_no_send_backend_ok: worker_diag.no_send_backend_ok,
        worker_no_send_backend_err: worker_diag.no_send_backend_err,
        tpu_quic_candidate_seen: worker_diag.tpu_quic_candidate_seen,
        tpu_quic_would_enqueue: worker_diag.tpu_quic_would_enqueue,
        tpu_quic_ring_enqueue_ok: worker_diag.tpu_quic_ring_enqueue_ok,
        tpu_quic_ring_enqueue_err: worker_diag.tpu_quic_ring_enqueue_err,
        tpu_quic_pm_reserve_simulated: worker_diag.tpu_quic_pm_reserve_simulated,
        tpu_quic_last_signature: worker_diag
            .tpu_quic_last_signature
            .map(|sig| bs58::encode(sig).into_string())
            .unwrap_or_default(),
        tpu_quic_last_route_family: worker_diag.tpu_quic_last_route_family.clone(),
        scorecard_replay_stale_slot_bypass_seen: worker_diag
            .scorecard_replay_stale_slot_bypass_seen,
        scorecard_replay_stale_slot_bypass_applied: worker_diag
            .scorecard_replay_stale_slot_bypass_applied,
        scorecard_replay_stale_slot_bypass_signature_match: worker_diag
            .scorecard_replay_stale_slot_bypass_signature_match,
        scorecard_replay_stale_slot_bypass_pool_match: worker_diag
            .scorecard_replay_stale_slot_bypass_pool_match,
        scorecard_replay_original_slot: worker_diag.scorecard_replay_original_slot,
        scorecard_replay_current_slot: worker_diag.scorecard_replay_current_slot,
        scorecard_replay_slot_gap: worker_diag.scorecard_replay_slot_gap,
        scorecard_replay_route_override_seen: worker_diag.scorecard_replay_route_override_seen,
        scorecard_replay_route_override_applied: worker_diag
            .scorecard_replay_route_override_applied,
        scorecard_replay_route_override_signature_match: worker_diag
            .scorecard_replay_route_override_signature_match,
        scorecard_replay_route_override_pool_match: worker_diag
            .scorecard_replay_route_override_pool_match,
        scorecard_replay_route_override_from_public_target: worker_diag
            .scorecard_replay_route_override_from_public_target,
        scorecard_replay_route_override_to_tpu_quic_public: worker_diag
            .scorecard_replay_route_override_to_tpu_quic_public,
        scorecard_replay_source_override_seen: worker_diag.scorecard_replay_source_override_seen,
        scorecard_replay_source_override_applied: worker_diag
            .scorecard_replay_source_override_applied,
        scorecard_replay_source_override_signature_match: worker_diag
            .scorecard_replay_source_override_signature_match,
        scorecard_replay_source_override_pool_match: worker_diag
            .scorecard_replay_source_override_pool_match,
        scorecard_replay_source_override_live_send_blocked: worker_diag
            .scorecard_replay_source_override_live_send_blocked,
        worker_final_no_send_event_constructed: worker_diag.final_no_send_event_constructed,
        worker_final_no_send_event_published: worker_diag.final_no_send_event_published,
        worker_final_no_send_event_publish_err: worker_diag.final_no_send_event_publish_err,
        worker_final_no_send_drop_reason_last: worker_diag.final_no_send_drop_reason_last,
        worker_geyser_shadow_no_send_admitted: worker_diag.geyser_shadow_no_send_admitted,
        worker_geyser_shadow_no_send_rejected: worker_diag.geyser_shadow_no_send_rejected,
        worker_geyser_shadow_no_send_source_external: worker_diag
            .geyser_shadow_no_send_source_external,
        worker_geyser_shadow_no_send_source_local_validator: worker_diag
            .geyser_shadow_no_send_source_local_validator,
        r1_auto_admission_attempt: worker_diag.r1_auto_admission_attempt,
        r1_auto_admission_admit: worker_diag.r1_auto_admission_admit,
        r1_auto_admission_admit_to_process_intent_attempt: worker_diag
            .r1_auto_admission_admit_to_process_intent_attempt,
        r1_auto_admission_admit_to_process_intent_ok: worker_diag
            .r1_auto_admission_admit_to_process_intent_ok,
        r1_auto_admission_admit_to_process_intent_drop: worker_diag
            .r1_auto_admission_admit_to_process_intent_drop,
        clmm_exact_quote_request_publish_attempt: worker_diag
            .clmm_exact_quote_request_publish_attempt,
        clmm_exact_quote_request_publish_ok: worker_diag.clmm_exact_quote_request_publish_ok,
        clmm_exact_quote_request_publish_full: worker_diag.clmm_exact_quote_request_publish_full,
        clmm_exact_quote_request_overwrite_same_pool: worker_diag
            .clmm_exact_quote_request_overwrite_same_pool,
        clmm_exact_quote_request_suppressed_outstanding_same_pool: worker_diag
            .clmm_exact_quote_request_suppressed_outstanding_same_pool,
        clmm_exact_quote_request_outstanding_same_pool_hit: worker_diag
            .clmm_exact_quote_request_outstanding_same_pool_hit,
        clmm_exact_quote_request_outstanding_cleared_matched: worker_diag
            .clmm_exact_quote_request_outstanding_cleared_matched,
        clmm_exact_quote_request_outstanding_cleared_timeout: worker_diag
            .clmm_exact_quote_request_outstanding_cleared_timeout,
        clmm_exact_quote_request_outstanding_cleared_no_quote: worker_diag
            .clmm_exact_quote_request_outstanding_cleared_no_quote,
        clmm_exact_quote_response_matched: worker_diag.clmm_exact_quote_response_matched,
        clmm_exact_quote_response_timeout: worker_diag.clmm_exact_quote_response_timeout,
        clmm_exact_quote_response_no_snapshot: worker_diag.clmm_exact_quote_response_no_snapshot,
        clmm_exact_quote_response_stale: worker_diag.clmm_exact_quote_response_stale,
        clmm_exact_quote_response_pool_mismatch: worker_diag
            .clmm_exact_quote_response_pool_mismatch,
        clmm_exact_quote_response_mint_mismatch: worker_diag
            .clmm_exact_quote_response_mint_mismatch,
        clmm_exact_quote_response_amount_mismatch: worker_diag
            .clmm_exact_quote_response_amount_mismatch,
        clmm_exact_quote_response_request_id_mismatch: worker_diag
            .clmm_exact_quote_response_request_id_mismatch,
        clmm_exact_quote_response_invalid_flags: worker_diag
            .clmm_exact_quote_response_invalid_flags,
        clmm_exact_quote_response_invalid_flags_zero: worker_diag
            .clmm_exact_quote_response_invalid_flags_zero,
        clmm_exact_quote_response_invalid_flags_no_valid_bit: worker_diag
            .clmm_exact_quote_response_invalid_flags_no_valid_bit,
        clmm_exact_quote_response_invalid_flags_no_quote_bit: worker_diag
            .clmm_exact_quote_response_invalid_flags_no_quote_bit,
        clmm_exact_quote_response_invalid_flags_unknown_status: worker_diag
            .clmm_exact_quote_response_invalid_flags_unknown_status,
        clmm_exact_quote_response_invalid_valid_quote_shape: worker_diag
            .clmm_exact_quote_response_invalid_valid_quote_shape,
        clmm_exact_quote_response_no_quote_no_current_array_upstream: worker_diag
            .clmm_exact_quote_response_no_quote_no_current_array_upstream,
        clmm_exact_quote_response_no_quote_no_liquidity_boundary: worker_diag
            .clmm_exact_quote_response_no_quote_no_liquidity_boundary,
        clmm_exact_quote_response_no_quote_out_of_range: worker_diag
            .clmm_exact_quote_response_no_quote_out_of_range,
        clmm_exact_quote_response_no_quote_missing_tick_array_local: worker_diag
            .clmm_exact_quote_response_no_quote_missing_tick_array_local,
        clmm_exact_quote_response_timeout_terminal_no_quote: worker_diag
            .clmm_exact_quote_response_timeout_terminal_no_quote,
        clmm_exact_quote_response_timeout_quote_ready_rejected: worker_diag
            .clmm_exact_quote_response_timeout_quote_ready_rejected,
        clmm_exact_quote_response_timeout_stale_request_id: worker_diag
            .clmm_exact_quote_response_timeout_stale_request_id,
        clmm_exact_quote_response_timeout_quote_build_failed: worker_diag
            .clmm_exact_quote_response_timeout_quote_build_failed,
        clmm_exact_quote_response_timeout_dependency_missing: worker_diag
            .clmm_exact_quote_response_timeout_dependency_missing,
        r1_auto_admission_drop_source: worker_diag.r1_auto_admission_drop_source,
        r1_auto_admission_drop_non_private_grade: worker_diag
            .r1_auto_admission_drop_non_private_grade,
        r1_auto_admission_drop_shadowguard: worker_diag.r1_auto_admission_drop_shadowguard,
        r1_auto_admission_drop_micro_market: worker_diag.r1_auto_admission_drop_micro_market,
        r1_auto_admission_drop_egress: worker_diag.r1_auto_admission_drop_egress,
        r1_auto_admission_drop_inventory: worker_diag.r1_auto_admission_drop_inventory,
        r1_auto_admission_drop_cap: worker_diag.r1_auto_admission_drop_cap,
        r1_auto_admission_drop_stale: worker_diag.r1_auto_admission_drop_stale,
        r1_auto_admission_drop_public_route: worker_diag.r1_auto_admission_drop_public_route,
        r1_auto_admission_reject_source: worker_diag.r1_auto_admission_reject_source,
        r1_auto_admission_reject_non_private_grade: worker_diag
            .r1_auto_admission_reject_non_private_grade,
        r1_auto_admission_reject_shadowguard: worker_diag.r1_auto_admission_reject_shadowguard,
        r1_auto_admission_reject_micro_market: worker_diag.r1_auto_admission_reject_micro_market,
        r1_auto_admission_reject_egress: worker_diag.r1_auto_admission_reject_egress,
        r1_auto_admission_reject_inventory: worker_diag.r1_auto_admission_reject_inventory,
        r1_auto_admission_reject_cap: worker_diag.r1_auto_admission_reject_cap,
        r1_auto_admission_reject_stale: worker_diag.r1_auto_admission_reject_stale,
        r1_auto_admission_reject_public_route: worker_diag.r1_auto_admission_reject_public_route,
        r1_auto_admission_reject_proof_filter: worker_diag.r1_auto_admission_reject_proof_filter,
        r1_auto_admission_reject_template_executability: worker_diag
            .r1_auto_admission_reject_template_executability,
        r1_auto_admission_reject_attempt_latch: worker_diag.r1_auto_admission_reject_attempt_latch,
        r1_auto_admission_reject_proof_filter_no_quote: worker_diag
            .r1_auto_admission_reject_proof_filter_no_quote,
        r1_auto_admission_reject_proof_filter_no_positive_ev: worker_diag
            .r1_auto_admission_reject_proof_filter_no_positive_ev,
        r1_auto_admission_reject_proof_filter_no_shadow_guard: worker_diag
            .r1_auto_admission_reject_proof_filter_no_shadow_guard,
        r1_auto_admission_reject_proof_filter_no_micro_market: worker_diag
            .r1_auto_admission_reject_proof_filter_no_micro_market,
        r1_auto_admission_reject_proof_filter_public_scout_shape: worker_diag
            .r1_auto_admission_reject_proof_filter_public_scout_shape,
        r1_auto_admission_reject_proof_filter_attempt_latch: worker_diag
            .r1_auto_admission_reject_proof_filter_attempt_latch,
        r1_auto_admission_reject_proof_filter_other: worker_diag
            .r1_auto_admission_reject_proof_filter_other,
        r1_auto_admission_reject_public_scout_shape_route_not_public_scout: worker_diag
            .r1_auto_admission_reject_public_scout_shape_route_not_public_scout,
        r1_auto_admission_reject_public_scout_shape_wrong_source: worker_diag
            .r1_auto_admission_reject_public_scout_shape_wrong_source,
        r1_auto_admission_reject_public_scout_shape_wrong_venue: worker_diag
            .r1_auto_admission_reject_public_scout_shape_wrong_venue,
        r1_auto_admission_reject_public_scout_shape_missing_template_family: worker_diag
            .r1_auto_admission_reject_public_scout_shape_missing_template_family,
        r1_auto_admission_reject_public_scout_shape_route_template_mismatch: worker_diag
            .r1_auto_admission_reject_public_scout_shape_route_template_mismatch,
        r1_auto_admission_reject_public_scout_shape_other: worker_diag
            .r1_auto_admission_reject_public_scout_shape_other,
        r1_auto_admission_reject_micro_market_no_uplift: worker_diag
            .r1_auto_admission_reject_micro_market_no_uplift,
        r1_auto_admission_reject_micro_market_negative_ev: worker_diag
            .r1_auto_admission_reject_micro_market_negative_ev,
        r1_auto_admission_reject_micro_market_below_floor: worker_diag
            .r1_auto_admission_reject_micro_market_below_floor,
        r1_auto_admission_reject_micro_market_other: worker_diag
            .r1_auto_admission_reject_micro_market_other,
        last_public_scout_shape_reject_reason: worker_diag
            .last_public_scout_shape_reject_reason
            .clone(),
        last_public_scout_shape_reject_pool: if worker_diag
            .last_public_scout_shape_reject_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_public_scout_shape_reject_pool_pubkey).into_string()
        },
        last_public_scout_shape_reject_mint_in: if worker_diag
            .last_public_scout_shape_reject_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_public_scout_shape_reject_mint_in_pubkey).into_string()
        },
        last_public_scout_shape_reject_mint_out: if worker_diag
            .last_public_scout_shape_reject_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_public_scout_shape_reject_mint_out_pubkey).into_string()
        },
        last_public_scout_shape_reject_route: worker_diag
            .last_public_scout_shape_reject_route
            .clone(),
        last_public_scout_shape_reject_source: worker_diag
            .last_public_scout_shape_reject_source
            .clone(),
        last_public_scout_shape_reject_venue: worker_diag
            .last_public_scout_shape_reject_venue
            .clone(),
        last_run38_target_pool: if worker_diag.last_run38_target_pool_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_pool_pubkey).into_string()
        },
        last_run38_target_mint_in: if worker_diag.last_run38_target_mint_in_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_mint_in_pubkey).into_string()
        },
        last_run38_target_mint_out: if worker_diag.last_run38_target_mint_out_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_mint_out_pubkey).into_string()
        },
        last_run38_target_route: worker_diag.last_run38_target_route.clone(),
        last_run38_target_source: worker_diag.last_run38_target_source.clone(),
        last_run38_target_venue: worker_diag.last_run38_target_venue.clone(),
        last_run38_target_stage: worker_diag.last_run38_target_stage.clone(),
        last_run38_target_reject_reason: worker_diag.last_run38_target_reject_reason.clone(),
        last_run38_target_inventory_reject_reason: worker_diag
            .last_run38_target_inventory_reject_reason
            .clone(),
        last_run38_target_inventory_pool: if worker_diag.last_run38_target_inventory_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_inventory_pool_pubkey).into_string()
        },
        last_run38_target_inventory_mint_in: if worker_diag
            .last_run38_target_inventory_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_inventory_mint_in_pubkey).into_string()
        },
        last_run38_target_inventory_mint_out: if worker_diag
            .last_run38_target_inventory_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_inventory_mint_out_pubkey).into_string()
        },
        last_run38_target_inventory_input_ata: if worker_diag
            .last_run38_target_inventory_input_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_inventory_input_token_account_pubkey)
                .into_string()
        },
        last_run38_target_inventory_input_balance: worker_diag
            .last_run38_target_inventory_input_balance,
        last_run38_target_inventory_required_input: worker_diag
            .last_run38_target_inventory_required_input,
        last_run38_target_inventory_token_program: if worker_diag
            .last_run38_target_inventory_token_program_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_run38_target_inventory_token_program_pubkey).into_string()
        },
        last_r1_auto_admission_reject_reason: worker_diag
            .last_r1_auto_admission_reject_reason
            .clone(),
        last_r1_auto_admission_reject_stage: worker_diag
            .last_r1_auto_admission_reject_stage
            .clone(),
        last_r1_auto_admission_pool: if worker_diag.last_r1_auto_admission_pool_pubkey == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_auto_admission_pool_pubkey).into_string()
        },
        last_r1_auto_admission_mint_in: if worker_diag.last_r1_auto_admission_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_auto_admission_mint_in_pubkey).into_string()
        },
        last_r1_auto_admission_mint_out: if worker_diag.last_r1_auto_admission_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_auto_admission_mint_out_pubkey).into_string()
        },
        last_r1_template_executability_reject_reason: worker_diag
            .last_r1_template_executability_reject_reason
            .clone(),
        last_r1_template_executability_pool: if worker_diag
            .last_r1_template_executability_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_template_executability_pool_pubkey).into_string()
        },
        last_r1_template_executability_mint_in: if worker_diag
            .last_r1_template_executability_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_template_executability_mint_in_pubkey).into_string()
        },
        last_r1_template_executability_mint_out: if worker_diag
            .last_r1_template_executability_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_template_executability_mint_out_pubkey).into_string()
        },
        last_r1_template_executability_route: worker_diag
            .last_r1_template_executability_route
            .clone(),
        last_r1_template_executability_output_ata: if worker_diag
            .last_r1_template_executability_output_token_account_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_r1_template_executability_output_token_account_pubkey)
                .into_string()
        },
        last_admit_to_process_intent_drop_reason: worker_diag
            .last_admit_to_process_intent_drop_reason
            .clone(),
        last_admit_to_process_intent_pool: if worker_diag.last_admit_to_process_intent_pool_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_admit_to_process_intent_pool_pubkey).into_string()
        },
        last_admit_to_process_intent_mint_in: if worker_diag
            .last_admit_to_process_intent_mint_in_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_admit_to_process_intent_mint_in_pubkey).into_string()
        },
        last_admit_to_process_intent_mint_out: if worker_diag
            .last_admit_to_process_intent_mint_out_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_admit_to_process_intent_mint_out_pubkey).into_string()
        },
        last_admit_to_process_intent_route: worker_diag.last_admit_to_process_intent_route.clone(),
        last_admit_to_process_intent_source: worker_diag
            .last_admit_to_process_intent_source
            .clone(),
        last_clmm_exact_quote_response_mismatch_reason: worker_diag
            .last_clmm_exact_quote_response_mismatch_reason
            .clone(),
        last_clmm_exact_quote_response_pool_index: worker_diag
            .last_clmm_exact_quote_response_pool_index
            as u64,
        last_clmm_exact_quote_response_sequence: worker_diag.last_clmm_exact_quote_response_sequence
            as u64,
        last_clmm_exact_quote_response_request_id_expected: worker_diag
            .last_clmm_exact_quote_response_request_id_expected
            as u64,
        last_clmm_exact_quote_response_request_id_found: worker_diag
            .last_clmm_exact_quote_response_request_id_found
            as u64,
        last_clmm_exact_quote_response_pool_expected: if worker_diag
            .last_clmm_exact_quote_response_pool_expected_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_pool_expected_pubkey)
                .into_string()
        },
        last_clmm_exact_quote_response_pool_found: if worker_diag
            .last_clmm_exact_quote_response_pool_found_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_pool_found_pubkey).into_string()
        },
        last_clmm_exact_quote_response_mint_in_expected: if worker_diag
            .last_clmm_exact_quote_response_mint_in_expected_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_mint_in_expected_pubkey)
                .into_string()
        },
        last_clmm_exact_quote_response_mint_in_found: if worker_diag
            .last_clmm_exact_quote_response_mint_in_found_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_mint_in_found_pubkey)
                .into_string()
        },
        last_clmm_exact_quote_response_mint_out_expected: if worker_diag
            .last_clmm_exact_quote_response_mint_out_expected_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_mint_out_expected_pubkey)
                .into_string()
        },
        last_clmm_exact_quote_response_mint_out_found: if worker_diag
            .last_clmm_exact_quote_response_mint_out_found_pubkey
            == [0u8; 32]
        {
            String::new()
        } else {
            bs58::encode(worker_diag.last_clmm_exact_quote_response_mint_out_found_pubkey)
                .into_string()
        },
        last_clmm_exact_quote_response_amount_expected: worker_diag
            .last_clmm_exact_quote_response_amount_expected,
        last_clmm_exact_quote_response_amount_found: worker_diag
            .last_clmm_exact_quote_response_amount_found,
        last_clmm_exact_quote_response_validity_flags: worker_diag
            .last_clmm_exact_quote_response_validity_flags
            as u64,
        last_clmm_exact_quote_response_status: clmm_exact_quote_status_str(
            worker_diag.last_clmm_exact_quote_response_status,
        )
        .to_string(),
        last_clmm_exact_quote_response_status_code: worker_diag
            .last_clmm_exact_quote_response_status
            as u64,
        last_clmm_exact_quote_response_timeout_classification:
            classify_last_clmm_exact_quote_response_timeout(&worker_diag).to_string(),
        last_clmm_exact_quote_response_quote_epoch: worker_diag
            .last_clmm_exact_quote_response_quote_epoch,
        last_clmm_exact_quote_response_last_update_unix_ns: worker_diag
            .last_clmm_exact_quote_response_last_update_unix_ns,
        r1_afxdp_private_grade_route_actionable: worker_diag
            .r1_afxdp_private_grade_route_actionable,
        r1_afxdp_private_grade_process_intent_entered: worker_diag
            .r1_afxdp_private_grade_process_intent_entered,
        r1_afxdp_private_grade_auto_admission_attempt: worker_diag
            .r1_afxdp_private_grade_auto_admission_attempt,
        r1_afxdp_private_grade_auto_admission_admit: worker_diag
            .r1_afxdp_private_grade_auto_admission_admit,
        worker_signal_public_geyser: worker_diag.signal_public_geyser,
        worker_signal_external_geyser: worker_diag.signal_external_geyser,
        worker_signal_local_validator_geyser: worker_diag.signal_local_validator_geyser,
        worker_signal_jito_searcher_api: worker_diag.signal_jito_searcher_api,
        worker_signal_private_mempool: worker_diag.signal_private_mempool,
        worker_signal_harmonic_searcher_api: worker_diag.signal_harmonic_searcher_api,
        worker_signal_afxdp: worker_diag.signal_afxdp,
        worker_signal_dpdk: worker_diag.signal_dpdk,
        worker_signal_unknown: worker_diag.signal_unknown,
        dpdk_worker_records_seen: worker_diag.dpdk_worker_records_seen,
        dpdk_worker_health_drop: worker_diag.dpdk_worker_health_drops,
        dpdk_worker_stale_slot_drop: worker_diag.dpdk_worker_stale_slot_drops,
        dpdk_worker_pool_state_miss: worker_diag.dpdk_worker_pool_state_miss,
        dpdk_worker_pool_state_hit: worker_diag.dpdk_worker_pool_state_hit,
        dpdk_worker_route_actionable: worker_diag.dpdk_worker_route_actionable,
        dpdk_worker_route_dropped: worker_diag.dpdk_worker_route_dropped,
        dpdk_worker_route_unavailable: worker_diag.dpdk_worker_route_unavailable,
        dpdk_worker_route_unavailable_unknown_signal: worker_diag
            .dpdk_worker_route_unavailable_unknown_signal,
        dpdk_worker_route_unavailable_hydration_none: worker_diag
            .dpdk_worker_route_unavailable_hydration_none,
        dpdk_worker_r1_admission_drop: worker_diag.dpdk_worker_r1_admission_drop,
        dpdk_worker_candidates_entered_process_intent: worker_diag
            .dpdk_worker_candidates_entered_process_intent,
        dpdk_worker_pipeline_error: worker_diag.dpdk_worker_pipeline_error,
        dpdk_worker_rollout_shadow_published: worker_diag.dpdk_worker_rollout_shadow_published,
        dpdk_worker_final_no_send_published: worker_diag.dpdk_worker_final_no_send_published,
        worker_source_probe_afxdp_parsed_shred_drops: worker_diag
            .source_probe_afxdp_parsed_shred_drops,
        afxdp_frames_seen: afxdp_raw_diag.frames_seen,
        afxdp_udp_8000_8025_seen: afxdp_raw_diag.udp_8000_8025_seen,
        afxdp_xsk_rx_batches: afxdp_raw_diag.xsk_rx_batches,
        afxdp_shred_parse_ok: afxdp_raw_diag.shred_parse_ok,
        afxdp_shred_parse_err: afxdp_raw_diag.shred_parse_err,
        afxdp_shred_parse_data_ok: afxdp_raw_diag.shred_parse_data_ok,
        afxdp_shred_parse_coding_ok: afxdp_raw_diag.shred_parse_coding_ok,
        afxdp_feed_enqueue_ok: afxdp_raw_diag.feed_enqueue_ok,
        afxdp_feed_enqueue_full: afxdp_raw_diag.feed_enqueue_full,
        afxdp_reassembly_insert_ok: afxdp_raw_diag.reassembly_insert_ok,
        afxdp_reassembly_complete_entries: afxdp_raw_diag.reassembly_complete_entries,
        afxdp_fec_recovered_entries: afxdp_raw_diag.fec_recovered_entries,
        afxdp_data_shred_payload_ok: afxdp_raw_diag.data_shred_payload_ok,
        afxdp_tx_extract_attempt: afxdp_raw_diag.tx_extract_attempt,
        afxdp_tx_extract_ok: afxdp_raw_diag.tx_extract_ok,
        afxdp_tx_extract_err: afxdp_raw_diag.tx_extract_err,
        afxdp_tx_extract_err_empty_payload: afxdp_raw_diag.tx_extract_err_empty_payload,
        afxdp_tx_extract_err_short_vec: afxdp_raw_diag.tx_extract_err_short_vec,
        afxdp_tx_extract_err_bad_entry_header: afxdp_raw_diag.tx_extract_err_bad_entry_header,
        afxdp_tx_extract_err_bad_compact_u16: afxdp_raw_diag.tx_extract_err_bad_compact_u16,
        afxdp_tx_extract_err_sanitize_failed: afxdp_raw_diag.tx_extract_err_sanitize_failed,
        afxdp_tx_extract_err_no_transactions: afxdp_raw_diag.tx_extract_err_no_transactions,
        afxdp_tx_extract_err_unsupported_format: afxdp_raw_diag.tx_extract_err_unsupported_format,
        afxdp_tx_record_emit_attempt: afxdp_raw_diag.tx_record_emit_attempt,
        afxdp_tx_record_emit_ok: afxdp_raw_diag.tx_record_emit_ok,
        afxdp_tx_record_emit_full: afxdp_raw_diag.tx_record_emit_full,
        afxdp_pipeline_drop_unsupported_program: afxdp_raw_diag.pipeline_drop_unsupported_program,
        afxdp_pipeline_drop_allowlist_miss: afxdp_raw_diag.pipeline_drop_allowlist_miss,
        afxdp_pipeline_drop_decode_fail: afxdp_raw_diag.pipeline_drop_decode_fail,
        afxdp_pipeline_drop_route_fail: afxdp_raw_diag.pipeline_drop_route_fail,
        afxdp_pipeline_drop_mint_fail: afxdp_raw_diag.pipeline_drop_mint_fail,
        afxdp_pipeline_drop_other: afxdp_raw_diag.pipeline_drop_other,
        afxdp_unsupported_program_top_0_id: afxdp_raw_diag.unsupported_program_top_0_id,
        afxdp_unsupported_program_top_0_count: afxdp_raw_diag.unsupported_program_top_0_count,
        afxdp_unsupported_program_top_1_id: afxdp_raw_diag.unsupported_program_top_1_id,
        afxdp_unsupported_program_top_1_count: afxdp_raw_diag.unsupported_program_top_1_count,
        afxdp_unsupported_program_top_2_id: afxdp_raw_diag.unsupported_program_top_2_id,
        afxdp_unsupported_program_top_2_count: afxdp_raw_diag.unsupported_program_top_2_count,
        afxdp_unsupported_program_top_3_id: afxdp_raw_diag.unsupported_program_top_3_id,
        afxdp_unsupported_program_top_3_count: afxdp_raw_diag.unsupported_program_top_3_count,
        afxdp_unsupported_program_top_4_id: afxdp_raw_diag.unsupported_program_top_4_id,
        afxdp_unsupported_program_top_4_count: afxdp_raw_diag.unsupported_program_top_4_count,
        afxdp_unsupported_program_top_5_id: afxdp_raw_diag.unsupported_program_top_5_id,
        afxdp_unsupported_program_top_5_count: afxdp_raw_diag.unsupported_program_top_5_count,
        afxdp_unsupported_program_top_6_id: afxdp_raw_diag.unsupported_program_top_6_id,
        afxdp_unsupported_program_top_6_count: afxdp_raw_diag.unsupported_program_top_6_count,
        afxdp_unsupported_program_top_7_id: afxdp_raw_diag.unsupported_program_top_7_id,
        afxdp_unsupported_program_top_7_count: afxdp_raw_diag.unsupported_program_top_7_count,
        afxdp_unsupported_program_top_8_id: afxdp_raw_diag.unsupported_program_top_8_id,
        afxdp_unsupported_program_top_8_count: afxdp_raw_diag.unsupported_program_top_8_count,
        afxdp_unsupported_program_top_9_id: afxdp_raw_diag.unsupported_program_top_9_id,
        afxdp_unsupported_program_top_9_count: afxdp_raw_diag.unsupported_program_top_9_count,
        afxdp_unsupported_program_top_10_id: afxdp_raw_diag.unsupported_program_top_10_id,
        afxdp_unsupported_program_top_10_count: afxdp_raw_diag.unsupported_program_top_10_count,
        afxdp_unsupported_program_top_11_id: afxdp_raw_diag.unsupported_program_top_11_id,
        afxdp_unsupported_program_top_11_count: afxdp_raw_diag.unsupported_program_top_11_count,
        afxdp_unsupported_program_top_12_id: afxdp_raw_diag.unsupported_program_top_12_id,
        afxdp_unsupported_program_top_12_count: afxdp_raw_diag.unsupported_program_top_12_count,
        afxdp_unsupported_program_top_13_id: afxdp_raw_diag.unsupported_program_top_13_id,
        afxdp_unsupported_program_top_13_count: afxdp_raw_diag.unsupported_program_top_13_count,
        afxdp_unsupported_program_top_14_id: afxdp_raw_diag.unsupported_program_top_14_id,
        afxdp_unsupported_program_top_14_count: afxdp_raw_diag.unsupported_program_top_14_count,
        afxdp_unsupported_program_top_15_id: afxdp_raw_diag.unsupported_program_top_15_id,
        afxdp_unsupported_program_top_15_count: afxdp_raw_diag.unsupported_program_top_15_count,
        afxdp_source_probe_emit_attempt: afxdp_raw_diag.source_probe_emit_attempt,
        afxdp_source_probe_emit_ok: afxdp_raw_diag.source_probe_emit_ok,
        afxdp_source_probe_emit_full: afxdp_raw_diag.source_probe_emit_full,
        afxdp_source_probe_emit_err: afxdp_raw_diag.source_probe_emit_err,
        grpc_connected: grpc_diag.grpc_connected,
        grpc_subscribe_sent: grpc_diag.grpc_subscribe_sent,
        grpc_updates_total: grpc_diag.grpc_updates_total,
        grpc_updates_transaction: grpc_diag.grpc_updates_transaction,
        grpc_updates_non_transaction: grpc_diag.grpc_updates_non_transaction,
        grpc_normalize_ok: grpc_diag.grpc_normalize_ok,
        grpc_normalize_err: grpc_diag.grpc_normalize_err,
        grpc_pipeline_drop_unsupported_program: grpc_diag.grpc_pipeline_drop_unsupported_program,
        grpc_pipeline_drop_allowlist_miss: grpc_diag.grpc_pipeline_drop_allowlist_miss,
        grpc_pipeline_drop_decode_fail: grpc_diag.grpc_pipeline_drop_decode_fail,
        grpc_pipeline_drop_route_fail: grpc_diag.grpc_pipeline_drop_route_fail,
        grpc_pipeline_drop_mint_fail: grpc_diag.grpc_pipeline_drop_mint_fail,
        grpc_pipeline_drop_other: grpc_diag.grpc_pipeline_drop_other,
        grpc_unsupported_program_top_0_id: grpc_unsupported[0].0.clone(),
        grpc_unsupported_program_top_0_count: grpc_unsupported[0].1,
        grpc_unsupported_program_top_1_id: grpc_unsupported[1].0.clone(),
        grpc_unsupported_program_top_1_count: grpc_unsupported[1].1,
        grpc_unsupported_program_top_2_id: grpc_unsupported[2].0.clone(),
        grpc_unsupported_program_top_2_count: grpc_unsupported[2].1,
        grpc_unsupported_program_top_3_id: grpc_unsupported[3].0.clone(),
        grpc_unsupported_program_top_3_count: grpc_unsupported[3].1,
        grpc_unsupported_program_top_4_id: grpc_unsupported[4].0.clone(),
        grpc_unsupported_program_top_4_count: grpc_unsupported[4].1,
        grpc_unsupported_program_top_5_id: grpc_unsupported[5].0.clone(),
        grpc_unsupported_program_top_5_count: grpc_unsupported[5].1,
        grpc_unsupported_program_top_6_id: grpc_unsupported[6].0.clone(),
        grpc_unsupported_program_top_6_count: grpc_unsupported[6].1,
        grpc_unsupported_program_top_7_id: grpc_unsupported[7].0.clone(),
        grpc_unsupported_program_top_7_count: grpc_unsupported[7].1,
        grpc_unsupported_program_top_8_id: grpc_unsupported[8].0.clone(),
        grpc_unsupported_program_top_8_count: grpc_unsupported[8].1,
        grpc_unsupported_program_top_9_id: grpc_unsupported[9].0.clone(),
        grpc_unsupported_program_top_9_count: grpc_unsupported[9].1,
        grpc_unsupported_program_top_10_id: grpc_unsupported[10].0.clone(),
        grpc_unsupported_program_top_10_count: grpc_unsupported[10].1,
        grpc_unsupported_program_top_11_id: grpc_unsupported[11].0.clone(),
        grpc_unsupported_program_top_11_count: grpc_unsupported[11].1,
        grpc_unsupported_program_top_12_id: grpc_unsupported[12].0.clone(),
        grpc_unsupported_program_top_12_count: grpc_unsupported[12].1,
        grpc_unsupported_program_top_13_id: grpc_unsupported[13].0.clone(),
        grpc_unsupported_program_top_13_count: grpc_unsupported[13].1,
        grpc_unsupported_program_top_14_id: grpc_unsupported[14].0.clone(),
        grpc_unsupported_program_top_14_count: grpc_unsupported[14].1,
        grpc_unsupported_program_top_15_id: grpc_unsupported[15].0.clone(),
        grpc_unsupported_program_top_15_count: grpc_unsupported[15].1,
        grpc_feed_enqueue_ok: grpc_diag.grpc_feed_enqueue_ok,
        grpc_feed_enqueue_full: grpc_diag.grpc_feed_enqueue_full,
        grpc_feed_enqueue_err: grpc_diag.grpc_feed_enqueue_err,
        grpc_signal_source_local_validator_geyser: grpc_diag
            .grpc_signal_source_local_validator_geyser,
        worker_route_attempts: worker_diag.route_attempts,
        worker_route_actionable: worker_diag.route_actionable,
        worker_route_dropped: worker_diag.route_dropped,
        worker_route_unavailable: worker_diag.route_unavailable,
        worker_route_unavailable_unknown_signal: worker_diag.route_unavailable_unknown_signal,
        worker_route_unavailable_hydration_none: worker_diag.route_unavailable_hydration_none,
        worker_route_unavailable_hydration_none_afxdp: worker_diag
            .route_unavailable_hydration_none_afxdp,
        worker_afxdp_hydration_request_enqueued: worker_diag.afxdp_hydration_request_enqueued,
        worker_afxdp_hydration_request_deduped: worker_diag.afxdp_hydration_request_deduped,
        worker_route_reason_public_target: worker_diag.route_reason_public_target,
        worker_route_reason_stale_harmonic_no_jito: worker_diag.route_reason_stale_harmonic_no_jito,
        worker_route_reason_quarantined_harmonic_no_jito: worker_diag
            .route_reason_quarantined_harmonic_no_jito,
        worker_route_reason_tactic_mask_no_jito: worker_diag.route_reason_tactic_mask_no_jito,
        worker_route_reason_harmonic_egress_gate_no_jito: worker_diag
            .route_reason_harmonic_egress_gate_no_jito,
        worker_route_reason_over_cap: worker_diag.route_reason_over_cap,
        worker_route_reason_no_viable_route: worker_diag.route_reason_no_viable_route,
        worker_route_reason_atba_no_jito: worker_diag.route_reason_atba_no_jito,
        worker_route_reason_apex_partial_no_jito: worker_diag.route_reason_apex_partial_no_jito,
        worker_route_reason_jito_egress_gate_fallback: worker_diag
            .route_reason_jito_egress_gate_fallback,
        worker_route_reason_jito_stale_harmonic_reroute: worker_diag
            .route_reason_jito_stale_harmonic_reroute,
        worker_route_reason_jito_quarantined_fallback: worker_diag
            .route_reason_jito_quarantined_fallback,
        worker_route_reason_jito_tactic_mask_fallback: worker_diag
            .route_reason_jito_tactic_mask_fallback,
        worker_route_reason_jito_direct: worker_diag.route_reason_jito_direct,
        worker_route_reason_jito_atba: worker_diag.route_reason_jito_atba,
        worker_route_reason_jito_apex_partial: worker_diag.route_reason_jito_apex_partial,
        worker_route_reason_harmonic_fresh_plain: worker_diag.route_reason_harmonic_fresh_plain,
        worker_pipeline_attempts: worker_diag.pipeline_attempts,
        worker_pipeline_success: worker_diag.pipeline_success,
        worker_pipeline_err_routing_hydration: worker_diag.pipeline_err_routing_hydration,
        worker_pipeline_err_blockhash_missing: worker_diag.pipeline_err_blockhash_missing,
        worker_pipeline_err_lookup: worker_diag.pipeline_err_lookup,
        worker_pipeline_err_lookup_pool_tag_missing: worker_diag
            .pipeline_err_lookup_pool_tag_missing,
        worker_pipeline_err_lookup_pool_arena_missing: worker_diag
            .pipeline_err_lookup_pool_arena_missing,
        worker_pipeline_err_lookup_pool_arena_torn: worker_diag.pipeline_err_lookup_pool_arena_torn,
        worker_pipeline_err_lookup_pool_stale: worker_diag.pipeline_err_lookup_pool_stale,
        worker_pipeline_err_lookup_mint_in_tag_missing: worker_diag
            .pipeline_err_lookup_mint_in_tag_missing,
        worker_pipeline_err_lookup_mint_in_arena_missing: worker_diag
            .pipeline_err_lookup_mint_in_arena_missing,
        worker_pipeline_err_lookup_mint_in_arena_torn: worker_diag
            .pipeline_err_lookup_mint_in_arena_torn,
        worker_pipeline_err_lookup_mint_out_tag_missing: worker_diag
            .pipeline_err_lookup_mint_out_tag_missing,
        worker_pipeline_err_lookup_mint_out_arena_missing: worker_diag
            .pipeline_err_lookup_mint_out_arena_missing,
        worker_pipeline_err_lookup_mint_out_arena_torn: worker_diag
            .pipeline_err_lookup_mint_out_arena_torn,
        worker_pipeline_err_lookup_mint_stale: worker_diag.pipeline_err_lookup_mint_stale,
        worker_pipeline_err_lookup_pubkey_mismatch: worker_diag.pipeline_err_lookup_pubkey_mismatch,
        last_lookup_fail_reason: encode_lookup_failure_reason(worker_diag.last_lookup_fail_reason),
        last_lookup_fail_pool_pubkey: if worker_diag.last_lookup_fail_pool_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_lookup_fail_pool_pubkey).into_string()
        },
        last_lookup_fail_mint_in: if worker_diag.last_lookup_fail_mint_in_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_lookup_fail_mint_in_pubkey).into_string()
        },
        last_lookup_fail_mint_out: if worker_diag.last_lookup_fail_mint_out_pubkey == [0u8; 32] {
            String::new()
        } else {
            bs58::encode(worker_diag.last_lookup_fail_mint_out_pubkey).into_string()
        },
        last_lookup_fail_venue: worker_diag.last_lookup_fail_venue_id,
        last_lookup_fail_pool_index: worker_diag.last_lookup_fail_pool_index,
        last_lookup_fail_pool_last_update_unix_ns: worker_diag
            .last_lookup_fail_pool_last_update_unix_ns,
        last_lookup_fail_record_unix_ns: worker_diag.last_lookup_fail_record_unix_ns,
        last_lookup_fail_pool_age_ns: worker_diag.last_lookup_fail_pool_age_ns,
        last_lookup_fail_max_pool_staleness_ns: worker_diag.last_lookup_fail_max_pool_staleness_ns,
        last_lookup_fail_health_snapshot_slot: worker_diag.last_lookup_fail_health_snapshot_slot,
        last_lookup_fail_pool_reserve_in: worker_diag.last_lookup_fail_pool_reserve_in,
        last_lookup_fail_pool_reserve_out: worker_diag.last_lookup_fail_pool_reserve_out,
        worker_pipeline_err_quote: worker_diag.pipeline_err_quote,
        worker_pipeline_err_quote_clmm_arena_missing: worker_diag
            .pipeline_err_quote_clmm_arena_missing,
        worker_pipeline_err_quote_clmm_snapshot_missing: worker_diag
            .pipeline_err_quote_clmm_snapshot_missing,
        worker_pipeline_err_quote_clmm_pool_mismatch: worker_diag
            .pipeline_err_quote_clmm_pool_mismatch,
        worker_pipeline_err_quote_clmm_mint_in_mismatch: worker_diag
            .pipeline_err_quote_clmm_mint_in_mismatch,
        worker_pipeline_err_quote_clmm_mint_out_mismatch: worker_diag
            .pipeline_err_quote_clmm_mint_out_mismatch,
        worker_pipeline_err_quote_clmm_amount_mismatch: worker_diag
            .pipeline_err_quote_clmm_amount_mismatch,
        worker_pipeline_err_quote_clmm_direction_mismatch: worker_diag
            .pipeline_err_quote_clmm_direction_mismatch,
        worker_pipeline_err_quote_clmm_invalid_snapshot: worker_diag
            .pipeline_err_quote_clmm_invalid_snapshot,
        worker_pipeline_err_quote_clmm_stale_snapshot: worker_diag
            .pipeline_err_quote_clmm_stale_snapshot,
        worker_pipeline_err_quote_clmm_target_out_underflow: worker_diag
            .pipeline_err_quote_clmm_target_out_underflow,
        worker_pipeline_err_invalid_direction: worker_diag.pipeline_err_invalid_direction,
        worker_pipeline_err_overlay_overflow: worker_diag.pipeline_err_overlay_overflow,
        worker_pipeline_err_no_send_sink_missing: worker_diag.pipeline_err_no_send_sink_missing,
        worker_pipeline_err_policy: worker_diag.pipeline_err_policy,
        worker_pipeline_err_policy_apex_not_positive: worker_diag
            .pipeline_err_policy_apex_not_positive,
        worker_pipeline_err_policy_atba_missing_dist: worker_diag
            .pipeline_err_policy_atba_missing_dist,
        worker_pipeline_err_policy_dominance_live: worker_diag.pipeline_err_policy_dominance_live,
        worker_pipeline_err_policy_no_usable_lut: worker_diag.pipeline_err_policy_no_usable_lut,
        worker_pipeline_err_policy_net_after_fee_floor: worker_diag
            .pipeline_err_policy_net_after_fee_floor,
        worker_pipeline_err_policy_over_15_sol_live_input: worker_diag
            .pipeline_err_policy_over_15_sol_live_input,
        worker_pipeline_err_policy_fee_clamp_invalid: worker_diag
            .pipeline_err_policy_fee_clamp_invalid,
        worker_pipeline_err_policy_inventory_breach: worker_diag
            .pipeline_err_policy_inventory_breach,
        worker_pipeline_err_policy_other: worker_diag.pipeline_err_policy_other,
        worker_pipeline_err_emit_invalid_guard_pubkey: worker_diag
            .pipeline_err_emit_invalid_guard_pubkey,
        worker_pipeline_err_emit_invalid_template_for_route: worker_diag
            .pipeline_err_emit_invalid_template_for_route,
        worker_pipeline_err_emit_tactic_not_allowed_on_route: worker_diag
            .pipeline_err_emit_tactic_not_allowed_on_route,
        worker_pipeline_err_emit_oversized_payload: worker_diag.pipeline_err_emit_oversized_payload,
        worker_pipeline_err_emit_unsupported_additional_signer: worker_diag
            .pipeline_err_emit_unsupported_additional_signer,
        worker_pipeline_err_emit_sign: worker_diag.pipeline_err_emit_sign,
        worker_pipeline_err_emit_enqueue_full: worker_diag.pipeline_err_emit_enqueue_full,
        worker_pipeline_err_emit_deadline_exceeded: worker_diag.pipeline_err_emit_deadline_exceeded,
        worker_pipeline_err_emit_no_send_sink_full: worker_diag.pipeline_err_emit_no_send_sink_full,
        worker_pipeline_err_emit_build_other: worker_diag.pipeline_err_emit_build_other,
        worker_pipeline_err_emit_other: worker_diag.pipeline_err_emit_other,
        worker_pipeline_err_emit: worker_diag.pipeline_err_emit,
        public_lane_amputation_ok: !shadow_core::routing::public_execution_reachable(),
        inventory_truth_ok,
        fee_leak_ok: live_landed == 0,
        fee_leak_accounted,
        phantom_inventory_risk,
        selective_death_clamp_ok: pm_stats.death_clamp.total_classified
            == pm_stats.death_clamp.guard_breaches
                + pm_stats.death_clamp.infra_failures
                + pm_stats.death_clamp.retail_reverts
            && pm_stats.death_clamp.tactic_bits_cleared <= pm_stats.death_clamp.guard_breaches
            && pm_stats.death_clamp.quarantines_raised <= pm_stats.death_clamp.infra_failures,
        pre_sign_egress_gate_ok: !egress_gate_enabled || egress_health.is_some(),
        private_lane_admissibility_ok,
        rollout_events_observed: ledger.observed,
        rollout_would_send: ledger.would_send,
        rollout_live_send_suppressed: ledger.live_send_suppressed,
        rollout_private_lane_admissible_opportunities: ledger.private_lane_admissible_opportunities,
        rollout_private_grade_admissible_opportunities: ledger
            .private_grade_admissible_opportunities,
        rollout_geyser_shadow_opportunities: ledger.geyser_shadow_opportunities,
        rollout_afxdp_private_grade_would_send: ledger.afxdp_private_grade_would_send,
        rollout_afxdp_private_grade_live_send_suppressed: ledger
            .afxdp_private_grade_live_send_suppressed,
        rollout_public_drops: ledger.public_drops,
        rollout_stale_state_reroutes: ledger.stale_state_reroutes,
        rollout_egress_gate_reroutes: ledger.egress_gate_reroutes,
        rollout_egress_gate_drops: ledger.egress_gate_drops,
        rollout_clamped_apex_candidates: ledger.clamped_apex_candidates,
        rollout_atba_missing_dist_candidates: ledger.atba_missing_dist_candidates,
        rollout_jito_no_send_would_sends: ledger.jito_no_send_would_sends,
        rollout_harmonic_no_send_would_sends: ledger.harmonic_no_send_would_sends,
        rollout_public_rpc_no_send_would_sends: ledger.public_rpc_no_send_would_sends,
        rollout_drop_reason_none: ledger.drop_reason_counts[G12DropReason::None as usize],
        rollout_drop_reason_public_target: ledger.drop_reason_counts
            [G12DropReason::DroppedPublicTarget as usize],
        rollout_drop_reason_apex_not_positive: ledger.drop_reason_counts
            [G12DropReason::DroppedApexNotPositive as usize],
        rollout_drop_reason_harmonic_stale_state: ledger.drop_reason_counts
            [G12DropReason::DroppedHarmonicStaleState as usize],
        rollout_drop_reason_harmonic_tactic_mask: ledger.drop_reason_counts
            [G12DropReason::DroppedHarmonicTacticMask as usize],
        rollout_drop_reason_egress_gate_jito: ledger.drop_reason_counts
            [G12DropReason::DroppedEgressGateJito as usize],
        rollout_drop_reason_egress_gate_harmonic: ledger.drop_reason_counts
            [G12DropReason::DroppedEgressGateHarmonic as usize],
        rollout_drop_reason_rerouted_pre_sign_egress_gate: ledger.drop_reason_counts
            [G12DropReason::ReroutedPreSignEgressGate as usize],
        rollout_drop_reason_atba_no_dist_to_tick: ledger.drop_reason_counts
            [G12DropReason::DroppedAtbaNoDistToTick as usize],
        rollout_drop_reason_taylor_prefilter: ledger.drop_reason_counts
            [G12DropReason::DroppedTaylorPrefilter as usize],
        rollout_drop_reason_would_drop_dominance_gate: ledger.drop_reason_counts
            [G12DropReason::WouldDropDominanceGate as usize],
        rollout_drop_reason_dropped_dominance_gate: ledger.drop_reason_counts
            [G12DropReason::DroppedDominanceGate as usize],
        rollout_drop_reason_no_usable_lut_bank: ledger.drop_reason_counts
            [G12DropReason::DroppedNoUsableLutBank as usize],
        rollout_drop_reason_no_shadow_guard: ledger.drop_reason_counts
            [G12DropReason::DroppedNoShadowGuard as usize],
        rollout_drop_reason_rollout_cap: ledger.drop_reason_counts
            [G12DropReason::DroppedRolloutCap as usize],
        rollout_drop_reason_rollout_no_send: ledger.drop_reason_counts
            [G12DropReason::DroppedRolloutNoSend as usize],
        rollout_last_no_send_reason: ledger.last_no_send_reason,
        rollout_last_drop_reason: ledger.last_drop_reason,
        latency_ok: latency_proof.pass,
        worker_latency_proof_present: latency_proof.present,
        worker_latency_p99_ns: latency_proof.p99_ns,
        worker_latency_budget_ns: latency_proof.budget_ns,
        afxdp_producer_to_feed_count: tail_latency.afxdp.producer_to_feed.count,
        afxdp_producer_to_feed_p50_ns: tail_latency.afxdp.producer_to_feed.p50_ns,
        afxdp_producer_to_feed_p90_ns: tail_latency.afxdp.producer_to_feed.p90_ns,
        afxdp_producer_to_feed_p99_ns: tail_latency.afxdp.producer_to_feed.p99_ns,
        afxdp_producer_to_feed_p999_ns: tail_latency.afxdp.producer_to_feed.p999_ns,
        afxdp_producer_to_feed_max_ns: tail_latency.afxdp.producer_to_feed.max_ns,
        afxdp_feed_to_worker_count: tail_latency.afxdp.feed_to_worker.count,
        afxdp_feed_to_worker_p50_ns: tail_latency.afxdp.feed_to_worker.p50_ns,
        afxdp_feed_to_worker_p90_ns: tail_latency.afxdp.feed_to_worker.p90_ns,
        afxdp_feed_to_worker_p99_ns: tail_latency.afxdp.feed_to_worker.p99_ns,
        afxdp_feed_to_worker_p999_ns: tail_latency.afxdp.feed_to_worker.p999_ns,
        afxdp_feed_to_worker_max_ns: tail_latency.afxdp.feed_to_worker.max_ns,
        afxdp_worker_to_nosend_count: tail_latency.afxdp.worker_to_nosend.count,
        afxdp_worker_to_nosend_p50_ns: tail_latency.afxdp.worker_to_nosend.p50_ns,
        afxdp_worker_to_nosend_p90_ns: tail_latency.afxdp.worker_to_nosend.p90_ns,
        afxdp_worker_to_nosend_p99_ns: tail_latency.afxdp.worker_to_nosend.p99_ns,
        afxdp_worker_to_nosend_p999_ns: tail_latency.afxdp.worker_to_nosend.p999_ns,
        afxdp_worker_to_nosend_max_ns: tail_latency.afxdp.worker_to_nosend.max_ns,
        afxdp_end_to_end_observe_count: tail_latency.afxdp.end_to_end_observe.count,
        afxdp_end_to_end_observe_p50_ns: tail_latency.afxdp.end_to_end_observe.p50_ns,
        afxdp_end_to_end_observe_p90_ns: tail_latency.afxdp.end_to_end_observe.p90_ns,
        afxdp_end_to_end_observe_p99_ns: tail_latency.afxdp.end_to_end_observe.p99_ns,
        afxdp_end_to_end_observe_p999_ns: tail_latency.afxdp.end_to_end_observe.p999_ns,
        afxdp_end_to_end_observe_max_ns: tail_latency.afxdp.end_to_end_observe.max_ns,
        dpdk_producer_to_feed_count: tail_latency.dpdk.producer_to_feed.count,
        dpdk_producer_to_feed_p50_ns: tail_latency.dpdk.producer_to_feed.p50_ns,
        dpdk_producer_to_feed_p90_ns: tail_latency.dpdk.producer_to_feed.p90_ns,
        dpdk_producer_to_feed_p99_ns: tail_latency.dpdk.producer_to_feed.p99_ns,
        dpdk_producer_to_feed_p999_ns: tail_latency.dpdk.producer_to_feed.p999_ns,
        dpdk_producer_to_feed_max_ns: tail_latency.dpdk.producer_to_feed.max_ns,
        dpdk_feed_to_worker_count: tail_latency.dpdk.feed_to_worker.count,
        dpdk_feed_to_worker_p50_ns: tail_latency.dpdk.feed_to_worker.p50_ns,
        dpdk_feed_to_worker_p90_ns: tail_latency.dpdk.feed_to_worker.p90_ns,
        dpdk_feed_to_worker_p99_ns: tail_latency.dpdk.feed_to_worker.p99_ns,
        dpdk_feed_to_worker_p999_ns: tail_latency.dpdk.feed_to_worker.p999_ns,
        dpdk_feed_to_worker_max_ns: tail_latency.dpdk.feed_to_worker.max_ns,
        dpdk_worker_to_nosend_count: tail_latency.dpdk.worker_to_nosend.count,
        dpdk_worker_to_nosend_p50_ns: tail_latency.dpdk.worker_to_nosend.p50_ns,
        dpdk_worker_to_nosend_p90_ns: tail_latency.dpdk.worker_to_nosend.p90_ns,
        dpdk_worker_to_nosend_p99_ns: tail_latency.dpdk.worker_to_nosend.p99_ns,
        dpdk_worker_to_nosend_p999_ns: tail_latency.dpdk.worker_to_nosend.p999_ns,
        dpdk_worker_to_nosend_max_ns: tail_latency.dpdk.worker_to_nosend.max_ns,
        dpdk_end_to_end_observe_count: tail_latency.dpdk.end_to_end_observe.count,
        dpdk_end_to_end_observe_p50_ns: tail_latency.dpdk.end_to_end_observe.p50_ns,
        dpdk_end_to_end_observe_p90_ns: tail_latency.dpdk.end_to_end_observe.p90_ns,
        dpdk_end_to_end_observe_p99_ns: tail_latency.dpdk.end_to_end_observe.p99_ns,
        dpdk_end_to_end_observe_p999_ns: tail_latency.dpdk.end_to_end_observe.p999_ns,
        dpdk_end_to_end_observe_max_ns: tail_latency.dpdk.end_to_end_observe.max_ns,
        triton_grpc_producer_to_feed_count: tail_latency.triton_grpc.producer_to_feed.count,
        triton_grpc_producer_to_feed_p50_ns: tail_latency.triton_grpc.producer_to_feed.p50_ns,
        triton_grpc_producer_to_feed_p90_ns: tail_latency.triton_grpc.producer_to_feed.p90_ns,
        triton_grpc_producer_to_feed_p99_ns: tail_latency.triton_grpc.producer_to_feed.p99_ns,
        triton_grpc_producer_to_feed_p999_ns: tail_latency.triton_grpc.producer_to_feed.p999_ns,
        triton_grpc_producer_to_feed_max_ns: tail_latency.triton_grpc.producer_to_feed.max_ns,
        triton_grpc_feed_to_worker_count: tail_latency.triton_grpc.feed_to_worker.count,
        triton_grpc_feed_to_worker_p50_ns: tail_latency.triton_grpc.feed_to_worker.p50_ns,
        triton_grpc_feed_to_worker_p90_ns: tail_latency.triton_grpc.feed_to_worker.p90_ns,
        triton_grpc_feed_to_worker_p99_ns: tail_latency.triton_grpc.feed_to_worker.p99_ns,
        triton_grpc_feed_to_worker_p999_ns: tail_latency.triton_grpc.feed_to_worker.p999_ns,
        triton_grpc_feed_to_worker_max_ns: tail_latency.triton_grpc.feed_to_worker.max_ns,
        triton_grpc_worker_to_nosend_count: tail_latency.triton_grpc.worker_to_nosend.count,
        triton_grpc_worker_to_nosend_p50_ns: tail_latency.triton_grpc.worker_to_nosend.p50_ns,
        triton_grpc_worker_to_nosend_p90_ns: tail_latency.triton_grpc.worker_to_nosend.p90_ns,
        triton_grpc_worker_to_nosend_p99_ns: tail_latency.triton_grpc.worker_to_nosend.p99_ns,
        triton_grpc_worker_to_nosend_p999_ns: tail_latency.triton_grpc.worker_to_nosend.p999_ns,
        triton_grpc_worker_to_nosend_max_ns: tail_latency.triton_grpc.worker_to_nosend.max_ns,
        triton_grpc_end_to_end_observe_count: tail_latency.triton_grpc.end_to_end_observe.count,
        triton_grpc_end_to_end_observe_p50_ns: tail_latency.triton_grpc.end_to_end_observe.p50_ns,
        triton_grpc_end_to_end_observe_p90_ns: tail_latency.triton_grpc.end_to_end_observe.p90_ns,
        triton_grpc_end_to_end_observe_p99_ns: tail_latency.triton_grpc.end_to_end_observe.p99_ns,
        triton_grpc_end_to_end_observe_p999_ns: tail_latency.triton_grpc.end_to_end_observe.p999_ns,
        triton_grpc_end_to_end_observe_max_ns: tail_latency.triton_grpc.end_to_end_observe.max_ns,
        realized_pnl_ok: live_landed == 0,
        shadow_pnl_ok: observe_candidates > 0 && ledger.total_shadow_pnl_lamports >= 0,
        competition_adjusted_ok: observe_candidates > 0
            && ledger.total_competition_adjusted_delta_lamports >= 0,
        competition_adjusted_retained_ev_delta_lamports: ledger
            .total_competition_adjusted_delta_lamports,
        competition_adjusted_fee_burn_delta_lamports: if live_landed == 0 { 0 } else { 1 },
        micro_market_uplift_ok: ledger.micro_market_observations > 0
            && ledger.total_micro_market_delta_lamports >= 0,
        micro_market_observations: ledger.micro_market_observations,
        micro_market_afxdp_private_grade_observations: ledger
            .micro_market_afxdp_private_grade_observations,
        micro_market_banked_retained_ev_lamports: ledger
            .total_micro_market_banked_retained_ev_lamports,
        micro_market_flat_baseline_retained_ev_lamports: ledger
            .total_micro_market_flat_baseline_retained_ev_lamports,
        micro_market_delta_lamports: ledger.total_micro_market_delta_lamports,
        dominance_shadow_pnl_ok: if ledger.dominance_would_drops == 0
            && ledger.dominance_live_drops == 0
        {
            true
        } else {
            ledger.total_shadow_pnl_lamports >= 0
        },
        taylor_gate_ok: if ledger.taylor_prefilter_drops == 0 {
            true
        } else {
            ledger.total_shadow_pnl_lamports >= 0
        },
        shadow_guard_deployed: false,
        shadow_guard_program_id_present: shadow_guard_preflight.shadow_guard_program_id_present
            || guard_program_id_present,
        shadow_guard_mainnet_deployed: shadow_guard_preflight.shadow_guard_mainnet_deployed,
        shadow_guard_template_program_id_matches: shadow_guard_preflight
            .shadow_guard_template_program_id_matches,
        sparse_classes_fallback_safe,
        baseline_shadow_comparator_enabled,
        failback_gate_exists: rollout.require_all_gates_green
            || (rollout.mode == RolloutMode::ObserveOnly && rollout.no_send_backend),
        tactical_lane,
        additive_quant_tactics: G12AdditiveQuantTacticsEvidence {
            cu_density_metrics_visible: cfg
                .quant_tactics
                .as_ref()
                .and_then(|qt| qt.cu_density_optimization.as_ref())
                .map(|density| density.enabled && observe_candidates > 0)
                .unwrap_or(false),
            exhaust_metrics_visible: cfg
                .quant_tactics
                .as_ref()
                .and_then(|qt| qt.post_trade_exhaust_scavenging.as_ref())
                .map(|exhaust| exhaust.enabled && observe_candidates > 0)
                .unwrap_or(false),
            tick_boundary_shadow_only,
            tick_boundary_live_sizing_changes: 0,
            tick_boundary_live_routing_changes: 0,
        },
    }
}

#[derive(Debug, Clone)]
struct GateProofPaths {
    latency_proof_path: PathBuf,
    shadow_guard_preflight_path: PathBuf,
}

fn proof_paths_for_report(report_path: &Path) -> GateProofPaths {
    let run_dir = report_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("run"));
    let results_dir = run_dir.join("..").join("results");
    GateProofPaths {
        latency_proof_path: results_dir
            .join("12_g5_dequeue_egress_microbench")
            .join("dequeue_egress_latency.txt"),
        shadow_guard_preflight_path: results_dir
            .join("21_shadowguard_preflight")
            .join("summary.txt"),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LatencyProof {
    present: bool,
    pass: bool,
    p99_ns: u64,
    budget_ns: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ShadowGuardPreflightEvidence {
    shadow_guard_program_id_present: bool,
    shadow_guard_mainnet_deployed: bool,
    shadow_guard_template_program_id_matches: bool,
}

fn read_latency_proof(path: &Path) -> LatencyProof {
    let Some(map) = read_key_value_file(path) else {
        return LatencyProof::default();
    };
    LatencyProof {
        present: true,
        pass: parse_bool(map.get("pass")).unwrap_or(false),
        p99_ns: parse_u64(map.get("p99_ns")).unwrap_or(0),
        budget_ns: parse_u64(map.get("budget_ns")).unwrap_or(0),
    }
}

fn read_shadow_guard_preflight(path: &Path) -> ShadowGuardPreflightEvidence {
    let Some(map) = read_key_value_file(path) else {
        return ShadowGuardPreflightEvidence::default();
    };
    ShadowGuardPreflightEvidence {
        shadow_guard_program_id_present: parse_bool(map.get("shadow_guard_program_id_present"))
            .unwrap_or(false),
        shadow_guard_mainnet_deployed: parse_bool(map.get("shadow_guard_mainnet_deployed"))
            .unwrap_or(false),
        shadow_guard_template_program_id_matches: parse_bool(
            map.get("shadow_guard_template_program_id_matches"),
        )
        .unwrap_or(false),
    }
}

fn read_key_value_file(path: &Path) -> Option<std::collections::HashMap<String, String>> {
    let contents = std::fs::read_to_string(path).ok()?;
    let mut values = std::collections::HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let pair = trimmed
            .split_once('=')
            .or_else(|| trimmed.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()));
        if let Some((key, value)) = pair {
            values.insert(key, value);
        }
    }
    Some(values)
}

fn parse_bool(value: Option<&String>) -> Option<bool> {
    value.and_then(|raw| match raw.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    })
}

fn parse_u64(value: Option<&String>) -> Option<u64> {
    value.and_then(|raw| raw.trim().parse::<u64>().ok())
}

fn rollout_gate_report_path(cfg: &ShadowConfig) -> Option<PathBuf> {
    cfg.rollout
        .as_ref()
        .and_then(|rollout| rollout.gate_report_path.as_deref())
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn rollout_cap_rung_code(rung: RolloutCapRung) -> u8 {
    match rung {
        RolloutCapRung::R0ObserveOnly => 0,
        RolloutCapRung::R1Tiny => 1,
        RolloutCapRung::R2Small => 2,
        RolloutCapRung::R3HalfGuerrila => 3,
        RolloutCapRung::R4FullGuerrila => 4,
    }
}

fn write_latency_by_source_snapshot(path: &Path) -> Result<(), RolloutGateExportError> {
    let snapshot = crate::tail_latency::snapshot();
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_dpdk_path_diagnostics_snapshot(path: &Path) -> Result<(), RolloutGateExportError> {
    let snapshot = snapshot_dpdk_gate_diagnostics();
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_runls5b_artifacts(
    report_path: &Path,
    rejects_path: &Path,
    worker_counts_path: &Path,
) -> Result<(), RolloutGateExportError> {
    let grpc_diag = snapshot_grpc_ingest_diagnostics();
    let afxdp_raw_diag = snapshot_afxdp_gate_diagnostics();
    let autopsy = snapshot_runls5b_autopsy();

    let summary = runls5b_feed_to_process_intent_report(&autopsy, &grpc_diag, &afxdp_raw_diag);
    let rejects = runls5b_source_reject_reasons(&autopsy);
    let worker_counts = runls5b_worker_stage_counts_by_source(&autopsy);

    std::fs::write(report_path, serde_json::to_vec_pretty(&summary)?)?;
    std::fs::write(rejects_path, serde_json::to_vec_pretty(&rejects)?)?;
    std::fs::write(
        worker_counts_path,
        serde_json::to_vec_pretty(&worker_counts)?,
    )?;
    Ok(())
}

fn write_runls5c_artifacts(
    report_path: &Path,
    by_source_path: &Path,
    rows_path: &Path,
) -> Result<(), RolloutGateExportError> {
    let grpc_diag = snapshot_grpc_ingest_diagnostics();
    let afxdp_raw_diag = snapshot_afxdp_gate_diagnostics();
    let autopsy = snapshot_runls5b_autopsy();
    let rows = snapshot_runls5c_process_intent_subreason_rows();
    let summary =
        runls5c_process_intent_subreason_report(&rows, &autopsy, &grpc_diag, &afxdp_raw_diag);
    let by_source = runls5c_process_intent_subreason_by_source(&rows);

    std::fs::write(report_path, serde_json::to_vec_pretty(&summary)?)?;
    std::fs::write(by_source_path, serde_json::to_vec_pretty(&by_source)?)?;

    let mut jsonl = String::new();
    for row in &rows {
        jsonl.push_str(&serde_json::to_string(row)?);
        jsonl.push('\n');
    }
    std::fs::write(rows_path, jsonl)?;
    Ok(())
}

fn write_runls5d_artifacts(
    report_path: &Path,
    rollout_path: &Path,
    no_send_path: &Path,
    stage_counts_path: &Path,
) -> Result<(), RolloutGateExportError> {
    let grpc_diag = snapshot_grpc_ingest_diagnostics();
    let afxdp_raw_diag = snapshot_afxdp_gate_diagnostics();
    let autopsy = snapshot_runls5b_autopsy();
    let rows = snapshot_runls5c_process_intent_subreason_rows();
    let summary =
        runls5d_process_intent_subreason_report(&rows, &autopsy, &grpc_diag, &afxdp_raw_diag);
    let rollout_prefixes = ["reject_rollout_gate_"];
    let no_send_prefixes = [
        "reject_pi_no_send_",
        "reject_pi_live_disabled_but_research_allowed",
        "reject_pi_owner_mode_not_allowed_in_profile",
        "reject_pi_route_family_not_allowed_in_profile",
        "reject_pi_source_allowed_observe_only",
        "reject_pi_private_route_requires_bundle_lane",
        "reject_pi_strategy_requires_live_profile",
        "reject_pi_unknown_no_send_mode",
    ];
    let rollout =
        runls5d_process_intent_subreason_by_source_with_prefixes(&rows, &rollout_prefixes);
    let no_send =
        runls5d_process_intent_subreason_by_source_with_prefixes(&rows, &no_send_prefixes);
    let stage_counts = runls5b_worker_stage_counts_by_source(&autopsy);

    std::fs::write(report_path, serde_json::to_vec_pretty(&summary)?)?;
    std::fs::write(rollout_path, serde_json::to_vec_pretty(&rollout)?)?;
    std::fs::write(no_send_path, serde_json::to_vec_pretty(&no_send)?)?;
    std::fs::write(stage_counts_path, serde_json::to_vec_pretty(&stage_counts)?)?;
    Ok(())
}

fn write_runls5e_research_sink_artifact(report_path: &Path) -> Result<(), RolloutGateExportError> {
    let report = snapshot_research_no_send_sink_report();
    std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadow_core::pm::death_clamp::DeathClampStats;
    use shadow_core::pm::opso_ledger::PostShockEpisodeLedgerStats;
    use shadow_core::pm::pm_loop::PmLoopStats;
    use shadow_core::pm::reconciler::ReconcilerStats;
    use shadow_core::pm::release::{FenceDrift, ReleaseStats};
    use shadow_core::pm::rollout_ledger::RolloutLedgerStats;

    fn fixture_config(report_path: &Path) -> ShadowConfig {
        let report_path = report_path.display().to_string().replace('\\', "\\\\");
        ShadowConfig::from_yaml(&format!(
            r#"
deployment:
  profile: v6_4_guerrila

system:
  mode: tier0_only
  money_workers: 4
  proxies:
    ingress:
      primary: shred_capture
      shred_capture_workers: 1
      grpc_fallback: yellowstone_grpc
      grpc_workers: 1
    egress:
      quic_workers: 0
      jito_workers: 1
      harmonic_workers: 1
    health:
      publish_egress_health: true
      degraded_hysteresis_batches: 2

state:
  geyser:
    harmonic_max_state_slot_lag: 1
    require_live_transfer_fee_updates: true
  clmm:
    publish_dist_to_tick_lamports: true

risk:
  reserve_floor_sol: 10.0
  max_position_sol_per_mint: 15.0
  max_notional_sol_per_entity: 30.0

rollout:
  mode: observe_only
  no_send_backend: true
  cap_rung: r0_observe_only
  max_live_trade_input_sol: 0.0
  max_position_sol_per_mint: 0.0
  max_notional_sol_per_entity: 0.0
  require_shadow_guard_deployed: true
  shadow_guard_program_id: null
  require_all_gates_green: true
  gate_report_path: "{report_path}"
  live_bid_shading_allowed: false
  dominance_live_allowed: false
  competition_adjusted_primary_allowed: false
  min_observe_candidates: 10000
  min_live_landed: 0
  min_sample_slots: 10000

submit:
  routing:
    default_public_geyser: drop
    default_jito_searcher: jito_bundle
    private_mempool_strategy: jito_bundle_if_compatible
    enable_harmonic_lane: true
    harmonic_quarantine_enabled: true
    pre_sign_egress_admission_gate: true
    stale_harmonic_state_action: reroute_to_jito
    atba_route_mode: jito_only
    apex_partial_route_mode: jito_only
  quic:
    kamikaze_enabled: false
  harmonic:
    cu_pad_units: 150
    quarantine_mode: specific_error_only
    quarantine_scope: class_scoped_with_global_failsafe
    tactic_class_count: 8

oracle:
  guerrila:
    max_required_backrun_capital_sol: 15.0
    retail_only_mode: true
    micro_market_enabled: true
    leader_class_count: 8
    publish_mode: banked_by_leader_class
    max_live_trade_input_sol: 15.0
    allow_clamped_apex_partials: true
    allow_atba_jito_only: true
    obscurity_discount_enabled: true
    obscurity_mode: header_only_bitpacked
    obscurity_initial_floor_bps: 4500
    primary_strategy_mode: baseline
    shadow_strategy_mode: competition_adjusted
    sparse_class_action: fallback_to_baseline
    competition_selector_enabled: true
    competition_selector_mode: fixed_bank_penalty_only
    competition_selector_max_penalty_bps: 500
    competition_selector_min_live_bid_floor_bps: 100
    competition_selector_class_min_samples: 10
    competition_selector_half_life_slots: 150
    lut_source: shm_published
  opso:
    enabled: true
    mode: observe_only
    sidecar_enabled: true
    sidecar_stale_action: fallback_to_baseline
    max_sidecar_staleness_ns: 400000000
    high_alpha_override_enabled: true
    max_shade_bps: 3000
    require_shadow_pnl: true
    require_pm_episode_ledger: true
    sparse_class_action: fallback_to_baseline
    live_bid_shading_enabled: false
  tip_lut:
    jito_tax_multiplier_bps: 10638
    harmonic_tax_multiplier_bps: 10000

quant_tactics:
  taylor_compression:
    enabled: true
    mode: pre_filter_gate_only
  ghost_snipe:
    enabled: true
    mode: cache_prefetch_only
  write_lock_saturation_discount:
    enabled: false
  splinter_cell_arb:
    enabled: false
  cu_density_optimization:
    enabled: true
    mode: minimum_empirical_safe_padding
    jito_baseline_variance_pad_cus: 150
  post_trade_exhaust_scavenging:
    enabled: true
    route_mode: deterministic_jito_backrun_only
    max_capital_deployment_sol: 15.0
    min_competitor_size_sol: 50.0
    min_convexity_distortion_bps: 600
  tick_boundary_sabotage:
    enabled: true
    mode: shadow_observe_only
    proximity_threshold_lamports: 100000

telemetry:
  observe_only:
    log_drop_reasons: true
    shadow_pnl_enabled: true

shadow_control:
  rosetta:
    enabled: true
    manifest_path: "/dev/shm/shadow_parsing_manifest"
    control_flags_path: "/dev/shm/shadow_control_flags"
    raw_archive_path: "/dev/shm/shadow_raw_archive"
    golden_records_path: "crates/shadow-control/testdata/golden"
    manifest_read_interval_shreds: 64
    tripwire_window_slots: 2
    grpc_only_cooldown_slots: 8
    blindness_miss_threshold: 1
    predictive_shift_enabled: true
    predictive_shift_max_abs_delta: 16
    deep_scan_enabled: true
    executable_hash_monitor_enabled: true
    venue_quarantine_enabled: true

entry:
  pool_slot_dominance_gate:
    enabled: true
    mode: observe_only
    require_same_base_state_ts: true
    min_incremental_retained_lcb_lamports: 1000
"#
        ))
        .expect("config")
    }

    fn fixture_pm_stats() -> PmLoopStats {
        PmLoopStats {
            ticks: 1_024,
            reconciler: ReconcilerStats {
                inserted: 0,
                released: 0,
                orphan_released: 0,
                provisional: 0,
                upgraded: 0,
                already_gone: 0,
                insert_failures: 0,
                no_actions: 0,
                pending_count: 0,
                pending_reserved_lamports: 0,
            },
            release: ReleaseStats {
                total_releases: 4,
                landed: 0,
                reverted: 0,
                expired: 0,
                send_failed: 1,
                nacked: 3,
                observe_only_no_send: 0,
                pm_timeout: 0,
                total_released_lamports: 40_000_000,
                total_settled_lamports: 0,
            },
            death_clamp: DeathClampStats {
                total_classified: 3,
                guard_breaches: 1,
                infra_failures: 1,
                retail_reverts: 1,
                tactic_bits_cleared: 1,
                quarantines_raised: 1,
            },
            episode_ledger: PostShockEpisodeLedgerStats::default(),
            rollout_ledger: RolloutLedgerStats {
                observed: 12,
                observe_candidates_external_geyser: 3,
                observe_candidates_laserstream: 0,
                observe_candidates_local_validator_geyser: 2,
                observe_candidates_private_grade: 7,
                first_observed_slot: 100,
                last_observed_slot: 149,
                first_observed_unix_ns: 1_000,
                last_observed_unix_ns: 2_000,
                would_send: 5,
                live_send_suppressed: 5,
                public_drops: 3,
                private_lane_admissible_opportunities: 5,
                private_grade_admissible_opportunities: 4,
                geyser_shadow_opportunities: 5,
                afxdp_private_grade_would_send: 4,
                afxdp_private_grade_live_send_suppressed: 4,
                jito_no_send_would_sends: 5,
                harmonic_no_send_would_sends: 0,
                public_rpc_no_send_would_sends: 0,
                tpu_quic_no_send_would_sends: 0,
                stale_state_reroutes: 2,
                egress_gate_reroutes: 1,
                egress_gate_drops: 2,
                taylor_prefilter_drops: 0,
                dominance_would_drops: 0,
                dominance_live_drops: 0,
                clamped_apex_candidates: 1,
                atba_missing_dist_candidates: 1,
                cu_density_wins: 2,
                exhaust_candidates: 1,
                exhaust_wins: 1,
                total_would_reserve_lamports: 10_000,
                total_shadow_pnl_lamports: 55,
                total_opso_shadow_bid_delta_lamports: 7,
                total_competition_adjusted_delta_lamports: 11,
                micro_market_observations: 12,
                micro_market_afxdp_private_grade_observations: 4,
                total_micro_market_banked_retained_ev_lamports: 4_200,
                total_micro_market_flat_baseline_retained_ev_lamports: 4_000,
                total_micro_market_delta_lamports: 200,
                drop_reason_counts: {
                    let mut counts = [0; 16];
                    counts[G12DropReason::None as usize] = 5;
                    counts[G12DropReason::DroppedPublicTarget as usize] = 3;
                    counts[G12DropReason::DroppedEgressGateJito as usize] = 2;
                    counts[G12DropReason::DroppedNoUsableLutBank as usize] = 1;
                    counts[G12DropReason::DroppedRolloutCap as usize] = 1;
                    counts
                },
                last_route_family: 0,
                last_tactic_family: 0,
                last_no_send_reason: 0,
                last_drop_reason: 0,
                last_kill_reason: 0,
            },
            last_drift: Some(FenceDrift {
                atomic_allocation: 0,
                fence_total_reserved: 0,
                drift: 0,
            }),
            sweep_releases: 0,
        }
    }

    #[test]
    fn g12_exporter_builds_non_zero_r0_window_from_pm_stats() {
        let _grpc_lock = crate::grpc_diag::lock_grpc_ingest_diagnostics_for_test();
        let _lock = crate::money_worker::lock_worker_diagnostics_for_test();
        let _proxy_diag_lock = shadow_egress_jito::lock_proxy_diagnostics_for_test();
        crate::grpc_diag::reset_grpc_ingest_diagnostics_for_test();
        crate::money_worker::reset_worker_diagnostics_for_test();
        shadow_egress_jito::reset_proxy_diagnostics_for_test();
        #[cfg(target_os = "linux")]
        let _afxdp_lock = crate::afxdp_runtime::lock_afxdp_raw_diagnostics_for_test();
        #[cfg(target_os = "linux")]
        crate::afxdp_runtime::reset_afxdp_raw_diagnostics_for_test();
        #[cfg(target_os = "linux")]
        crate::afxdp_runtime::set_afxdp_raw_diagnostics_for_test(
            crate::afxdp_runtime::AfxdpRawDiagnosticsSnapshot {
                frames_seen: 99,
                udp_8000_8025_seen: 77,
                xsk_rx_batches: 11,
                shred_parse_ok: 55,
                shred_parse_err: 4,
                shred_parse_data_ok: 33,
                shred_parse_coding_ok: 22,
                feed_enqueue_ok: 7,
                feed_enqueue_full: 2,
                reassembly_insert_ok: 44,
                reassembly_complete_entries: 21,
                fec_recovered_entries: 6,
                data_shred_payload_ok: 19,
                tx_extract_attempt: 19,
                tx_extract_ok: 12,
                tx_extract_err: 7,
                tx_extract_err_empty_payload: 1,
                tx_extract_err_short_vec: 1,
                tx_extract_err_bad_entry_header: 1,
                tx_extract_err_bad_compact_u16: 1,
                tx_extract_err_sanitize_failed: 1,
                tx_extract_err_no_transactions: 1,
                tx_extract_err_unsupported_format: 1,
                tx_record_emit_attempt: 9,
                tx_record_emit_ok: 7,
                tx_record_emit_full: 2,
                pipeline_drop_unsupported_program: 10,
                pipeline_drop_allowlist_miss: 11,
                pipeline_drop_decode_fail: 12,
                pipeline_drop_route_fail: 13,
                pipeline_drop_mint_fail: 14,
                pipeline_drop_other: 15,
                unsupported_program_top:
                    [crate::afxdp_producer::AfxdpUnsupportedProgramTopEntry::default();
                        crate::afxdp_producer::AFXDP_UNSUPPORTED_PROGRAM_TOP_LEN],
                source_probe_emit_attempt: 5,
                source_probe_emit_ok: 3,
                source_probe_emit_full: 1,
                source_probe_emit_err: 1,
            },
        );
        crate::grpc_diag::set_grpc_ingest_diagnostics_for_test(
            crate::grpc_diag::GrpcIngestDiagnosticsSnapshot {
                grpc_connected: 1,
                grpc_subscribe_sent: 1,
                grpc_updates_total: 8,
                grpc_updates_transaction: 5,
                grpc_updates_non_transaction: 3,
                grpc_normalize_ok: 4,
                grpc_normalize_err: 1,
                grpc_pipeline_drop_unsupported_program: 2,
                grpc_pipeline_drop_allowlist_miss: 0,
                grpc_pipeline_drop_decode_fail: 0,
                grpc_pipeline_drop_route_fail: 0,
                grpc_pipeline_drop_mint_fail: 0,
                grpc_pipeline_drop_other: 1,
                grpc_feed_enqueue_ok: 3,
                grpc_feed_enqueue_full: 1,
                grpc_feed_enqueue_err: 0,
                grpc_normalize_ok_external_geyser: 0,
                grpc_normalize_ok_local_validator_geyser: 4,
                grpc_normalize_ok_laserstream: 0,
                grpc_feed_enqueue_ok_external_geyser: 0,
                grpc_feed_enqueue_ok_local_validator_geyser: 3,
                grpc_feed_enqueue_ok_laserstream: 0,
                grpc_signal_source_local_validator_geyser: 4,
                grpc_signal_source_laserstream: 0,
                grpc_unsupported_program_top: {
                    let mut top = [crate::grpc_diag::GrpcUnsupportedProgramTopEntry::default();
                        crate::grpc_diag::GRPC_UNSUPPORTED_PROGRAM_TOP_LEN];
                    top[0] = crate::grpc_diag::GrpcUnsupportedProgramTopEntry {
                        program_id: [0xAB; 32],
                        count: 7,
                    };
                    top[1] = crate::grpc_diag::GrpcUnsupportedProgramTopEntry {
                        program_id: [0xCD; 32],
                        count: 3,
                    };
                    top
                },
            },
        );
        shadow_egress_jito::set_proxy_diagnostics_for_test(
            shadow_egress_jito::JitoProxyDiagnosticsSnapshot {
                public_rpc_transport_attempt: 5,
                public_rpc_transport_failure: 5,
                public_rpc_transport_rejected: 0,
                public_rpc_nack_recorded: 5,
                first_public_rpc_attempt_signature: Some([0x43; 64]),
                last_public_rpc_rejection_code: -32002,
                last_public_rpc_rejection_message: "Transaction simulation failed".to_string(),
                last_public_rpc_rejection_body:
                    "{\"code\":-32002,\"message\":\"Transaction simulation failed\"}".to_string(),
                last_public_rpc_rejection_data_err: "{\"InstructionError\":[1,{\"Custom\":6001}]}"
                    .to_string(),
                last_public_rpc_rejection_simulation_logs: "Program log: min out too low"
                    .to_string(),
                last_public_rpc_rejection_signature: Some([0x44; 64]),
                last_public_rpc_rejection_tx_size: 321,
                last_public_rpc_rejection_blockhash: Some([0x55; 32]),
                last_public_rpc_rejection_blockhash_matches_current: false,
                last_public_rpc_rejection_blockhash_age_ns: 0,
                last_public_rpc_rejection_cu_limit: 250_000,
                last_public_rpc_rejection_cu_price_micro_lamports: 77_000,
                last_public_rpc_rejection_reason: "slippage_min_out".to_string(),
                last_public_rpc_rejection_ix1_program_id: Some([0x66; 32]),
                last_public_rpc_rejection_ix1_account_0: Some([0x67; 32]),
                last_public_rpc_rejection_ix1_account_1: Some([0x68; 32]),
                last_public_rpc_rejection_ix1_account_count: 2,
                last_public_rpc_rejection_ix2_program_id: Some([0x69; 32]),
                last_public_rpc_rejection_ix2_amount_in: 200_000_000,
                last_public_rpc_rejection_ix2_min_out: 348_264_726_421,
                last_public_rpc_rejection_ix2_account_count: 13,
                last_public_rpc_rejection_actual_output_lamports: 58_535_449_673,
                last_public_rpc_rejection_required_output_lamports: 348_264_726_421,
            },
        );
        crate::money_worker::record_signal_source_diag_for_test(
            shadow_ipc::ingress_record::SIGNAL_SOURCE_PUBLIC_GEYSER,
        );
        crate::money_worker::record_signal_source_diag_for_test(
            shadow_ipc::ingress_record::SIGNAL_SOURCE_EXTERNAL_GEYSER,
        );
        crate::money_worker::record_signal_source_diag_for_test(
            shadow_ipc::ingress_record::SIGNAL_SOURCE_LOCAL_VALIDATOR_GEYSER,
        );
        crate::money_worker::record_signal_source_diag_for_test(
            shadow_ipc::ingress_record::SIGNAL_SOURCE_PRIVATE_MEMPOOL,
        );
        crate::money_worker::record_signal_source_diag_for_test(
            shadow_ipc::ingress_record::SIGNAL_SOURCE_DPDK,
        );
        crate::money_worker::record_signal_source_diag_for_test(0xFF);
        crate::money_worker::record_worker_pool_state_miss_for_test(
            [0xAA; 32],
            6,
            shadow_ipc::ingress_record::SIGNAL_SOURCE_AFXDP,
            1,
        );
        crate::money_worker::record_route_reason_diag_for_test(
            shadow_core::routing::RouteReason::JitoDirect,
        );
        crate::money_worker::record_route_unavailable_diag_for_test(false);
        crate::money_worker::record_route_unavailable_diag_for_test(true);
        crate::money_worker::record_pipeline_success_for_test();
        crate::money_worker::record_pipeline_error_diag_for_test(
            &shadow_core::pipeline::PipelineError::LookupFailed(
                shadow_core::lookup::LookupFailure::new(
                    shadow_core::lookup::LookupFailureReason::PoolTagMissing,
                ),
            ),
        );
        crate::money_worker::set_last_lookup_fail_for_test(
            [0x11; 32],
            [0x22; 32],
            [0x33; 32],
            9,
            shadow_core::lookup::LookupFailureReason::PoolTagMissing,
            Some(shadow_core::lookup::LookupFailureDiagnostics {
                pool_index: 77,
                pool_last_update_unix_ns: 88,
                record_unix_ns: 99,
                pool_age_ns: 11,
                max_pool_staleness_ns: 22,
                health_snapshot_slot: 333,
                pool_reserve_in: 444,
                pool_reserve_out: 555,
            }),
        );
        crate::money_worker::record_r1_auto_admission_reject_for_test(
            [0x44; 32],
            [0x55; 32],
            [0x66; 32],
            "proof_filter",
        );
        crate::money_worker::record_r1_admit_to_process_intent_drop_for_test(
            [0x77; 32],
            [0x88; 32],
            [0x99; 32],
            shadow_core::routing::RouteDecision::PublicRpcScout,
            shadow_ipc::ingress_record::SIGNAL_SOURCE_AFXDP,
            "quote_wait_timeout",
        );
        crate::money_worker::record_pipeline_error_diag_for_test(
            &shadow_core::pipeline::PipelineError::EmitFailed(
                shadow_core::assembly::EmitError::SignFailed(
                    shadow_core::assembly::SignError::RingEmpty,
                ),
            ),
        );

        let dir = tempfile::tempdir().unwrap();
        let report_path = dir.path().join("run").join("g12_gate_report.json");
        std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        let latency_dir = dir
            .path()
            .join("results")
            .join("12_g5_dequeue_egress_microbench");
        std::fs::create_dir_all(&latency_dir).unwrap();
        std::fs::write(
            latency_dir.join("dequeue_egress_latency.txt"),
            "p99_ns: 4140\nbudget_ns: 150000\npass: true\n",
        )
        .unwrap();
        let preflight_dir = dir.path().join("results").join("21_shadowguard_preflight");
        std::fs::create_dir_all(&preflight_dir).unwrap();
        std::fs::write(
            preflight_dir.join("summary.txt"),
            "shadow_guard_program_id_present=true\nshadow_guard_mainnet_deployed=false\nshadow_guard_template_program_id_matches=false\n",
        )
        .unwrap();

        let cfg = fixture_config(&report_path);
        let input = build_gate_evaluator_input(
            &cfg,
            &fixture_pm_stats(),
            Some(EgressHealthPayload32B {
                jito_state: 0,
                harmonic_state: 0,
                jito_queue_age_class: 0,
                harmonic_queue_age_class: 0,
                degraded_hysteresis_batches: 0,
                _pad_u16: 0,
                last_update_unix_ns: 1,
                publish_epoch: 1,
                _reserved: [0; 8],
            }),
        );

        assert_eq!(input.rung, 0);
        assert_eq!(input.sample_slots, 50);
        assert_eq!(input.observe_candidates, 12);
        assert_eq!(input.observe_candidates_total, 12);
        assert_eq!(input.observe_candidates_external_geyser, 3);
        assert_eq!(input.observe_candidates_local_validator_geyser, 2);
        assert_eq!(input.observe_candidates_private_grade, 7);
        assert_eq!(input.live_landed, 0);
        assert!(input.inventory_truth_ok);
        assert!(input.fee_leak_accounted);
        assert!(!input.phantom_inventory_risk);
        assert!(input.pre_sign_egress_gate_ok);
        assert!(input.private_lane_admissibility_ok);
        assert!(input.worker_latency_proof_present);
        assert_eq!(input.worker_latency_p99_ns, 4_140);
        assert_eq!(input.worker_latency_budget_ns, 150_000);
        assert_eq!(input.worker_signal_public_geyser, 1);
        assert_eq!(input.worker_signal_external_geyser, 1);
        assert_eq!(input.worker_signal_local_validator_geyser, 1);
        assert_eq!(input.worker_signal_private_mempool, 1);
        assert_eq!(input.worker_signal_dpdk, 1);
        assert_eq!(input.worker_signal_unknown, 1);
        assert_eq!(input.worker_pool_state_miss, 1);
        assert_eq!(input.worker_pool_state_miss_tag_map_absent, 1);
        assert_eq!(input.worker_pool_state_miss_arena_oob, 0);
        assert_eq!(input.worker_pool_state_miss_arena_torn, 0);
        assert_eq!(input.worker_pool_state_miss_pubkey_mismatch, 0);
        assert_eq!(
            input.last_worker_pool_state_miss_pool_pubkey,
            bs58::encode([0xAA; 32]).into_string()
        );
        assert_eq!(input.last_worker_pool_state_miss_venue_id, 6);
        assert_eq!(
            input.last_worker_pool_state_miss_signal_source,
            shadow_ipc::ingress_record::SIGNAL_SOURCE_AFXDP
        );
        assert_eq!(input.last_worker_pool_state_miss_reason_code, 1);
        assert_eq!(input.worker_pool_state_miss_recent_ring.len(), 1);
        assert_eq!(
            input.worker_pool_state_miss_recent_ring[0].pool_pubkey,
            bs58::encode([0xAA; 32]).into_string()
        );
        assert_eq!(input.worker_pool_state_miss_top_recent.len(), 1);
        assert_eq!(
            input.worker_pool_state_miss_top_recent[0].pool_pubkey,
            bs58::encode([0xAA; 32]).into_string()
        );
        assert_eq!(input.worker_pool_state_miss_top_recent[0].count, 1);
        assert_eq!(input.dpdk_worker_records_seen, 1);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(input.afxdp_frames_seen, 99);
            assert_eq!(input.afxdp_udp_8000_8025_seen, 77);
            assert_eq!(input.afxdp_xsk_rx_batches, 11);
            assert_eq!(input.afxdp_shred_parse_ok, 55);
            assert_eq!(input.afxdp_shred_parse_err, 4);
            assert_eq!(input.afxdp_feed_enqueue_ok, 7);
            assert_eq!(input.afxdp_feed_enqueue_full, 2);
            assert_eq!(input.afxdp_pipeline_drop_unsupported_program, 10);
            assert_eq!(input.afxdp_pipeline_drop_allowlist_miss, 11);
            assert_eq!(input.afxdp_pipeline_drop_decode_fail, 12);
            assert_eq!(input.afxdp_pipeline_drop_route_fail, 13);
            assert_eq!(input.afxdp_pipeline_drop_mint_fail, 14);
            assert_eq!(input.afxdp_pipeline_drop_other, 15);
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(input.afxdp_frames_seen, 0);
            assert_eq!(input.afxdp_udp_8000_8025_seen, 0);
            assert_eq!(input.afxdp_xsk_rx_batches, 0);
            assert_eq!(input.afxdp_shred_parse_ok, 0);
            assert_eq!(input.afxdp_shred_parse_err, 0);
            assert_eq!(input.afxdp_feed_enqueue_ok, 0);
            assert_eq!(input.afxdp_feed_enqueue_full, 0);
        }
        assert_eq!(input.grpc_connected, 1);
        assert_eq!(input.grpc_subscribe_sent, 1);
        assert_eq!(input.grpc_updates_total, 8);
        assert_eq!(input.grpc_updates_transaction, 5);
        assert_eq!(input.grpc_updates_non_transaction, 3);
        assert_eq!(input.grpc_normalize_ok, 4);
        assert_eq!(input.grpc_normalize_err, 1);
        assert_eq!(input.grpc_pipeline_drop_unsupported_program, 2);
        assert_eq!(input.grpc_pipeline_drop_allowlist_miss, 0);
        assert_eq!(input.grpc_pipeline_drop_decode_fail, 0);
        assert_eq!(input.grpc_pipeline_drop_route_fail, 0);
        assert_eq!(input.grpc_pipeline_drop_mint_fail, 0);
        assert_eq!(input.grpc_pipeline_drop_other, 1);
        assert_eq!(
            input.grpc_unsupported_program_top_0_id,
            bs58::encode([0xAB; 32]).into_string()
        );
        assert_eq!(input.grpc_unsupported_program_top_0_count, 7);
        assert_eq!(
            input.grpc_unsupported_program_top_1_id,
            bs58::encode([0xCD; 32]).into_string()
        );
        assert_eq!(input.grpc_unsupported_program_top_1_count, 3);
        assert_eq!(input.grpc_feed_enqueue_ok, 3);
        assert_eq!(input.grpc_feed_enqueue_full, 1);
        assert_eq!(input.grpc_feed_enqueue_err, 0);
        assert_eq!(input.grpc_signal_source_local_validator_geyser, 4);
        assert_eq!(input.worker_route_attempts, 3);
        assert_eq!(input.worker_route_actionable, 1);
        assert_eq!(input.worker_route_unavailable, 2);
        assert_eq!(input.worker_route_unavailable_unknown_signal, 1);
        assert_eq!(input.worker_route_unavailable_hydration_none, 1);
        assert_eq!(input.worker_route_reason_jito_direct, 1);
        assert_eq!(input.worker_route_resolved_jito, 1);
        assert_eq!(input.public_rpc_scout_enqueued, 0);
        assert_eq!(input.public_scout_attempt_latch_set, 1);
        assert_eq!(
            input.public_scout_attempt_latch_first_signature,
            bs58::encode([0x43; 64]).into_string()
        );
        assert_eq!(input.public_rpc_transport_attempt, 5);
        assert_eq!(input.public_rpc_transport_failure, 5);
        assert_eq!(input.public_rpc_nack_recorded, 5);
        assert_eq!(input.last_public_rpc_rejection_code, -32002);
        assert_eq!(
            input.last_public_rpc_rejection_message,
            "Transaction simulation failed"
        );
        assert_eq!(input.last_public_rpc_rejection_reason, "slippage_min_out");
        assert_eq!(
            input.last_public_rpc_rejection_signature,
            bs58::encode([0x44; 64]).into_string()
        );
        assert_eq!(input.last_public_rpc_rejection_tx_size, 321);
        assert_eq!(
            input.last_public_rpc_rejection_blockhash,
            bs58::encode([0x55; 32]).into_string()
        );
        assert_eq!(input.last_public_rpc_rejection_cu_limit, 250_000);
        assert_eq!(
            input.last_public_rpc_rejection_cu_price_micro_lamports,
            77_000
        );
        assert_eq!(
            input.last_public_rpc_rejection_ix1_program_id,
            bs58::encode([0x66; 32]).into_string()
        );
        assert_eq!(
            input.last_public_rpc_rejection_ix1_account_0,
            bs58::encode([0x67; 32]).into_string()
        );
        assert_eq!(
            input.last_public_rpc_rejection_ix1_account_1,
            bs58::encode([0x68; 32]).into_string()
        );
        assert_eq!(input.last_public_rpc_rejection_ix1_account_count, 2);
        assert_eq!(input.pm_reservation_released, 4);
        assert_eq!(input.pm_release_send_failed, 1);
        assert_eq!(input.pm_release_nacked, 3);
        assert_eq!(input.worker_pipeline_attempts, 3);
        assert_eq!(input.worker_pipeline_success, 1);
        assert_eq!(input.worker_pipeline_err_lookup, 1);
        assert_eq!(input.worker_pipeline_err_blockhash_missing, 0);
        assert_eq!(input.worker_pipeline_err_lookup_pool_tag_missing, 1);
        assert_eq!(input.last_lookup_fail_reason, "pool_tag_missing");
        assert_eq!(
            input.last_lookup_fail_pool_pubkey,
            bs58::encode([0x11; 32]).into_string()
        );
        assert_eq!(
            input.last_lookup_fail_mint_in,
            bs58::encode([0x22; 32]).into_string()
        );
        assert_eq!(
            input.last_lookup_fail_mint_out,
            bs58::encode([0x33; 32]).into_string()
        );
        assert_eq!(input.last_lookup_fail_venue, 9);
        assert_eq!(input.last_lookup_fail_pool_index, 77);
        assert_eq!(input.last_lookup_fail_pool_last_update_unix_ns, 88);
        assert_eq!(input.last_lookup_fail_record_unix_ns, 99);
        assert_eq!(input.last_lookup_fail_pool_age_ns, 11);
        assert_eq!(input.last_lookup_fail_max_pool_staleness_ns, 22);
        assert_eq!(input.last_lookup_fail_health_snapshot_slot, 333);
        assert_eq!(input.last_lookup_fail_pool_reserve_in, 444);
        assert_eq!(input.last_lookup_fail_pool_reserve_out, 555);
        assert_eq!(input.r1_auto_admission_reject_proof_filter, 1);
        assert_eq!(input.r1_auto_admission_reject_template_executability, 0);
        assert_eq!(input.r1_auto_admission_admit_to_process_intent_attempt, 1);
        assert_eq!(input.r1_auto_admission_admit_to_process_intent_ok, 0);
        assert_eq!(input.r1_auto_admission_admit_to_process_intent_drop, 1);
        assert_eq!(input.r1_auto_admission_reject_proof_filter_no_quote, 0);
        assert_eq!(input.r1_auto_admission_reject_micro_market_no_uplift, 0);
        assert_eq!(input.r1_auto_admission_reject_micro_market_negative_ev, 0);
        assert_eq!(input.r1_auto_admission_reject_micro_market_below_floor, 0);
        assert_eq!(input.r1_auto_admission_reject_micro_market_other, 0);
        assert_eq!(input.last_r1_auto_admission_reject_reason, "proof_filter");
        assert_eq!(input.last_r1_auto_admission_reject_stage, "admission");
        assert_eq!(
            input.last_r1_auto_admission_pool,
            bs58::encode([0x44; 32]).into_string()
        );
        assert_eq!(
            input.last_r1_auto_admission_mint_in,
            bs58::encode([0x55; 32]).into_string()
        );
        assert_eq!(
            input.last_r1_auto_admission_mint_out,
            bs58::encode([0x66; 32]).into_string()
        );
        assert_eq!(
            input.last_admit_to_process_intent_drop_reason,
            "quote_wait_timeout"
        );
        assert_eq!(
            input.last_admit_to_process_intent_pool,
            bs58::encode([0x77; 32]).into_string()
        );
        assert_eq!(
            input.last_admit_to_process_intent_mint_in,
            bs58::encode([0x88; 32]).into_string()
        );
        assert_eq!(
            input.last_admit_to_process_intent_mint_out,
            bs58::encode([0x99; 32]).into_string()
        );
        assert_eq!(input.last_admit_to_process_intent_route, "public_rpc_scout");
        assert_eq!(input.last_admit_to_process_intent_source, "afxdp");
        assert_eq!(input.worker_pipeline_err_policy, 0);
        assert_eq!(input.worker_pipeline_err_emit_sign, 1);
        assert_eq!(input.worker_pipeline_err_emit, 1);
        assert_eq!(input.worker_pipeline_err_emit_other, 0);
        assert_eq!(input.worker_candidates_entered_process_intent, 3);
        assert_eq!(input.worker_candidates_dropped_before_process_intent, 0);
        assert_eq!(input.worker_process_intent_ok, 1);
        assert_eq!(input.worker_process_intent_err, 2);
        assert_eq!(input.last_process_intent_error, "emit_failed");
        assert_eq!(input.last_process_intent_error_stage, "emit");
        assert_eq!(input.last_process_intent_error_reason, "sign_failed");
        assert_eq!(input.worker_quote_ok, 2);
        assert_eq!(input.worker_quote_err, 0);
        assert_eq!(input.worker_template_build_ok, 2);
        assert_eq!(input.worker_template_build_err, 0);
        assert_eq!(input.worker_no_send_backend_called, 2);
        assert_eq!(input.worker_no_send_backend_ok, 1);
        assert_eq!(input.worker_no_send_backend_err, 1);
        assert_eq!(input.worker_geyser_shadow_no_send_admitted, 0);
        assert_eq!(input.worker_geyser_shadow_no_send_rejected, 0);
        assert_eq!(input.rollout_public_drops, 3);
        assert_eq!(input.rollout_private_grade_admissible_opportunities, 4);
        assert_eq!(input.rollout_geyser_shadow_opportunities, 5);
        assert_eq!(input.rollout_afxdp_private_grade_would_send, 4);
        assert_eq!(input.rollout_afxdp_private_grade_live_send_suppressed, 4);
        assert_eq!(input.rollout_stale_state_reroutes, 2);
        assert_eq!(input.rollout_egress_gate_reroutes, 1);
        assert_eq!(input.rollout_egress_gate_drops, 2);
        assert_eq!(input.rollout_clamped_apex_candidates, 1);
        assert_eq!(input.rollout_atba_missing_dist_candidates, 1);
        assert_eq!(input.rollout_drop_reason_none, 5);
        assert_eq!(input.rollout_drop_reason_public_target, 3);
        assert_eq!(input.rollout_drop_reason_egress_gate_jito, 2);
        assert_eq!(input.rollout_drop_reason_no_usable_lut_bank, 1);
        assert_eq!(input.rollout_drop_reason_rollout_cap, 1);
        assert!(input.shadow_pnl_ok);
        assert!(input.competition_adjusted_ok);
        assert_eq!(input.micro_market_observations, 12);
        assert_eq!(input.micro_market_afxdp_private_grade_observations, 4);
        assert_eq!(input.micro_market_banked_retained_ev_lamports, 4_200);
        assert_eq!(input.micro_market_flat_baseline_retained_ev_lamports, 4_000);
        assert_eq!(input.micro_market_delta_lamports, 200);
        assert!(input.additive_quant_tactics.cu_density_metrics_visible);
        assert!(input.additive_quant_tactics.exhaust_metrics_visible);
        assert!(input.additive_quant_tactics.tick_boundary_shadow_only);
        assert!(!input.shadow_guard_mainnet_deployed);
        assert!(!input.shadow_guard_template_program_id_matches);
        assert!(input.micro_market_uplift_ok);
    }

    #[test]
    fn g12_exporter_derives_input_path_from_report_path() {
        let report = Path::new("run/g12_gate_report.json");
        assert_eq!(
            rollout_gate_input_path(report),
            PathBuf::from("run").join("g12_gate_input.json")
        );
    }

    #[test]
    fn g12_exporter_derives_runls5b_paths_from_report_path() {
        let report = Path::new("run/g12_gate_report.json");
        assert_eq!(
            runls5b_feed_to_process_intent_report_path(report),
            PathBuf::from("run").join("runls5b_feed_to_process_intent_report.json")
        );
        assert_eq!(
            runls5b_source_reject_reasons_path(report),
            PathBuf::from("run").join("runls5b_source_reject_reasons.json")
        );
        assert_eq!(
            runls5b_worker_stage_counts_by_source_path(report),
            PathBuf::from("run").join("runls5b_worker_stage_counts_by_source.json")
        );
        assert_eq!(
            runls5c_process_intent_subreason_report_path(report),
            PathBuf::from("run").join("runls5c_process_intent_subreason_report.json")
        );
        assert_eq!(
            runls5c_process_intent_subreason_by_source_path(report),
            PathBuf::from("run").join("runls5c_process_intent_subreason_by_source.json")
        );
        assert_eq!(
            runls5c_process_intent_subreason_rows_path(report),
            PathBuf::from("run").join("runls5c_process_intent_subreason_rows.jsonl")
        );
        assert_eq!(
            runls5d_process_intent_subreason_report_path(report),
            PathBuf::from("run").join("runls5d_process_intent_subreason_report.json")
        );
        assert_eq!(
            runls5d_rollout_gate_subreason_report_path(report),
            PathBuf::from("run").join("runls5d_rollout_gate_subreason_report.json")
        );
        assert_eq!(
            runls5d_no_send_profile_gate_report_path(report),
            PathBuf::from("run").join("runls5d_no_send_profile_gate_report.json")
        );
        assert_eq!(
            runls5d_source_stage_counts_path(report),
            PathBuf::from("run").join("runls5d_source_stage_counts.json")
        );
        assert_eq!(
            runls5e_research_sink_report_path(report),
            PathBuf::from("run").join("runls5e_research_sink_report.json")
        );
    }

    #[cfg(all(target_os = "linux", feature = "dpdk"))]
    #[test]
    fn dpdk_diag_snapshot_serializes_explicit_pool_miss_fields() {
        let snapshot = DpdkRawGateSnapshot {
            dpdk_pool_state_miss_tag_map: 1,
            dpdk_pool_state_miss_arena_oob: 2,
            dpdk_pool_state_miss_arena_torn: 3,
            dpdk_pool_state_miss_pubkey_mismatch: 4,
            dpdk_pool_state_miss_hint_enqueued: 5,
            dpdk_pool_state_miss_hint_deduped: 6,
            dpdk_last_pool_state_miss_pool_pubkey_b58: "pool111".to_string(),
            dpdk_last_pool_state_miss_venue_id: 7,
            dpdk_last_pool_state_miss_reason: "arena_torn".to_string(),
            ..Default::default()
        };

        let value = serde_json::to_value(&snapshot).expect("serialize dpdk diag snapshot");
        assert_eq!(value["dpdk_pool_state_miss_tag_map"], 1);
        assert_eq!(value["dpdk_pool_state_miss_arena_oob"], 2);
        assert_eq!(value["dpdk_pool_state_miss_arena_torn"], 3);
        assert_eq!(value["dpdk_pool_state_miss_pubkey_mismatch"], 4);
        assert_eq!(value["dpdk_pool_state_miss_hint_enqueued"], 5);
        assert_eq!(value["dpdk_pool_state_miss_hint_deduped"], 6);
        assert_eq!(
            value["dpdk_last_pool_state_miss_pool_pubkey_b58"],
            "pool111"
        );
        assert_eq!(value["dpdk_last_pool_state_miss_venue_id"], 7);
        assert_eq!(value["dpdk_last_pool_state_miss_reason"], "arena_torn");
        assert!(value
            .get("dpdk_worker_pool_state_miss_payload_none")
            .is_none());
        assert!(value
            .get("dpdk_worker_hydration_request_enqueued")
            .is_none());
    }
}

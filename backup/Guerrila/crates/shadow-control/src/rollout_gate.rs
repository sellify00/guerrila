use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub const DEFAULT_G12_GATE_REPORT_PATH: &str = "run/g12_gate_report.json";

#[derive(Debug)]
pub enum RolloutGateError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for RolloutGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
        }
    }
}

impl std::error::Error for RolloutGateError {}

impl From<std::io::Error> for RolloutGateError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RolloutGateError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12TacticalLaneEvidence {
    #[serde(default)]
    pub harmonic_atba_routes: u64,
    #[serde(default)]
    pub harmonic_apex_partial_routes: u64,
    #[serde(default)]
    pub harmonic_exhaust_scavenge_routes: u64,
}

impl G12TacticalLaneEvidence {
    pub fn is_green(&self) -> bool {
        self.harmonic_atba_routes == 0
            && self.harmonic_apex_partial_routes == 0
            && self.harmonic_exhaust_scavenge_routes == 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12AdditiveQuantTacticsEvidence {
    #[serde(default)]
    pub cu_density_metrics_visible: bool,
    #[serde(default)]
    pub exhaust_metrics_visible: bool,
    #[serde(default)]
    pub tick_boundary_shadow_only: bool,
    #[serde(default)]
    pub tick_boundary_live_sizing_changes: u64,
    #[serde(default)]
    pub tick_boundary_live_routing_changes: u64,
}

impl G12AdditiveQuantTacticsEvidence {
    pub fn is_green(&self) -> bool {
        self.cu_density_metrics_visible
            && self.exhaust_metrics_visible
            && self.tick_boundary_shadow_only
            && self.tick_boundary_live_sizing_changes == 0
            && self.tick_boundary_live_routing_changes == 0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12WorkerPoolStateMissRecent {
    #[serde(default)]
    pub timestamp_ns: u64,
    #[serde(default)]
    pub pool_pubkey: String,
    #[serde(default)]
    pub venue_id: u8,
    #[serde(default)]
    pub signal_source: u8,
    #[serde(default)]
    pub reason_code: u8,
    #[serde(default)]
    pub record_slot: u64,
    #[serde(default)]
    pub health_snapshot_slot: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12WorkerPoolStateMissTopRecent {
    #[serde(default)]
    pub pool_pubkey: String,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub first_seen_ns: u64,
    #[serde(default)]
    pub last_seen_ns: u64,
    #[serde(default)]
    pub reason_code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12GateEvaluatorInput {
    #[serde(default)]
    pub rung: u8,
    #[serde(default)]
    pub sample_slots: u64,
    #[serde(default)]
    pub observe_candidates: u64,
    #[serde(default)]
    pub observe_candidates_total: u64,
    #[serde(default)]
    pub observe_candidates_external_geyser: u64,
    #[serde(default)]
    pub observe_candidates_local_validator_geyser: u64,
    #[serde(default)]
    pub observe_candidates_private_grade: u64,
    #[serde(default)]
    pub live_landed: u64,

    #[serde(default)]
    pub min_sample_slots: u64,
    #[serde(default)]
    pub min_observe_candidates: u64,
    #[serde(default)]
    pub min_live_landed: u64,

    #[serde(default)]
    pub worker_records_seen: u64,
    #[serde(default)]
    pub worker_health_snapshot_missing: u64,
    #[serde(default)]
    pub worker_health_drops: u64,
    #[serde(default)]
    pub worker_stale_slot_drops: u64,
    #[serde(default)]
    pub worker_pool_state_miss: u64,
    #[serde(default)]
    pub worker_pool_state_miss_tag_map_absent: u64,
    #[serde(default)]
    pub worker_pool_state_miss_arena_oob: u64,
    #[serde(default)]
    pub worker_pool_state_miss_arena_torn: u64,
    #[serde(default)]
    pub worker_pool_state_miss_pubkey_mismatch: u64,
    #[serde(default)]
    pub last_worker_pool_state_miss_pool_pubkey: String,
    #[serde(default)]
    pub last_worker_pool_state_miss_venue_id: u8,
    #[serde(default)]
    pub last_worker_pool_state_miss_signal_source: u8,
    #[serde(default)]
    pub last_worker_pool_state_miss_reason_code: u8,
    #[serde(default)]
    pub worker_pool_state_miss_recent_ring: Vec<G12WorkerPoolStateMissRecent>,
    #[serde(default)]
    pub worker_pool_state_miss_top_recent: Vec<G12WorkerPoolStateMissTopRecent>,
    #[serde(default)]
    pub worker_pool_state_hit: u64,
    #[serde(default)]
    pub worker_early_drop_mint_in_tag_missing: u64,
    #[serde(default)]
    pub worker_early_drop_mint_out_tag_missing: u64,
    #[serde(default)]
    pub worker_shadow_events_published: u64,
    #[serde(default)]
    pub worker_rollout_would_send_events_published: u64,
    #[serde(default)]
    pub worker_rollout_live_send_suppressed_events_published: u64,
    #[serde(default)]
    pub worker_candidates_entered_process_intent: u64,
    #[serde(default)]
    pub worker_candidates_dropped_before_process_intent: u64,
    #[serde(default)]
    pub worker_process_intent_ok: u64,
    #[serde(default)]
    pub worker_process_intent_err: u64,
    #[serde(default)]
    pub last_process_intent_error: String,
    #[serde(default)]
    pub last_process_intent_error_stage: String,
    #[serde(default)]
    pub last_process_intent_error_reason: String,
    #[serde(default)]
    pub worker_quote_ok: u64,
    #[serde(default)]
    pub worker_quote_err: u64,
    #[serde(default)]
    pub worker_lookup_ok: u64,
    #[serde(default)]
    pub worker_policy_ok: u64,
    #[serde(default)]
    pub worker_assembly_ready: u64,
    #[serde(default)]
    pub worker_pm_reservation_created: u64,
    #[serde(default)]
    pub worker_route_resolved_jito: u64,
    #[serde(default)]
    pub worker_route_resolved_public_rpc_scout: u64,
    #[serde(default)]
    pub worker_route_resolved_harmonic: u64,
    #[serde(default)]
    pub worker_route_resolved_drop: u64,
    #[serde(default)]
    pub worker_public_rpc_scout_emit_attempt: u64,
    #[serde(default)]
    pub worker_public_rpc_scout_emit_ok: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_route_resolved: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_r1_attempt: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_proof_filter: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_micro_market: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory_missing_input_ata: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_inventory_other: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_template_executability: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_reject_attempt_latch: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_admit: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_process_intent: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_emit_attempt: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_transport_attempt: u64,
    #[serde(default)]
    pub run38_target_public_scout_clmm_pm_created: u64,
    #[serde(default)]
    pub public_scout_attempt_latch_set: u64,
    #[serde(default)]
    pub public_scout_attempt_latch_drop_count: u64,
    #[serde(default)]
    pub public_scout_attempt_latch_first_signature: String,
    #[serde(default)]
    pub pm_reserve_attempt: u64,
    #[serde(default)]
    pub pm_reserve_ok: u64,
    #[serde(default)]
    pub pm_reserve_reject_no_live_mode: u64,
    #[serde(default)]
    pub pm_reserve_reject_no_send_mode: u64,
    #[serde(default)]
    pub pm_reserve_reject_route_not_live: u64,
    #[serde(default)]
    pub pm_reserve_reject_cap: u64,
    #[serde(default)]
    pub pm_reserve_reject_inventory: u64,
    #[serde(default)]
    pub pm_reserve_reject_missing_signature: u64,
    #[serde(default)]
    pub pm_reserve_reject_other: u64,
    #[serde(default)]
    pub last_pm_reserve_reject_reason: String,
    #[serde(default)]
    pub last_pm_reserve_route: String,
    #[serde(default)]
    pub last_pm_reserve_pool: String,
    #[serde(default)]
    pub last_pm_reserve_mint_in: String,
    #[serde(default)]
    pub last_pm_reserve_mint_out: String,
    #[serde(default)]
    pub last_pm_reserve_amount: u64,
    #[serde(default)]
    pub clmm_input_ata_missing: u64,
    #[serde(default)]
    pub clmm_output_ata_missing: u64,
    #[serde(default)]
    pub clmm_input_ata_uninitialized: u64,
    #[serde(default)]
    pub clmm_output_ata_uninitialized: u64,
    #[serde(default)]
    pub clmm_input_balance_insufficient: u64,
    #[serde(default)]
    pub clmm_token_program_mismatch: u64,
    #[serde(default)]
    pub last_clmm_input_mint: String,
    #[serde(default)]
    pub last_clmm_output_mint: String,
    #[serde(default)]
    pub last_clmm_input_token_account: String,
    #[serde(default)]
    pub last_clmm_output_token_account: String,
    #[serde(default)]
    pub template_drop_no_template: u64,
    #[serde(default)]
    pub template_drop_wrong_route_family: u64,
    #[serde(default)]
    pub template_drop_missing_clmm_accounts: u64,
    #[serde(default)]
    pub template_drop_missing_input_ata: u64,
    #[serde(default)]
    pub template_drop_missing_output_ata: u64,
    #[serde(default)]
    pub template_drop_invalid_swap_program: u64,
    #[serde(default)]
    pub template_drop_invalid_guard_template: u64,
    #[serde(default)]
    pub template_drop_unsupported_token_program: u64,
    #[serde(default)]
    pub template_drop_other: u64,
    #[serde(default)]
    pub last_template_drop_reason: String,
    #[serde(default)]
    pub last_template_drop_pool: String,
    #[serde(default)]
    pub last_template_drop_mint_in: String,
    #[serde(default)]
    pub last_template_drop_mint_out: String,
    #[serde(default)]
    pub last_template_drop_route: String,
    #[serde(default)]
    pub last_template_drop_template_program: String,
    #[serde(default)]
    pub last_template_drop_input_ata: String,
    #[serde(default)]
    pub last_template_drop_output_ata: String,
    #[serde(default)]
    pub public_rpc_scout_enqueued: u64,
    #[serde(default)]
    pub public_rpc_transport_attempt: u64,
    #[serde(default)]
    pub public_rpc_transport_failure: u64,
    #[serde(default)]
    pub public_rpc_transport_rejected: u64,
    #[serde(default)]
    pub public_rpc_nack_recorded: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_code: i64,
    #[serde(default)]
    pub last_public_rpc_rejection_message: String,
    #[serde(default)]
    pub last_public_rpc_rejection_body: String,
    #[serde(default)]
    pub last_public_rpc_rejection_data_err: String,
    #[serde(default)]
    pub last_public_rpc_rejection_simulation_logs: String,
    #[serde(default)]
    pub last_public_rpc_rejection_signature: String,
    #[serde(default)]
    pub last_public_rpc_rejection_tx_size: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_blockhash: String,
    #[serde(default)]
    pub last_public_rpc_rejection_blockhash_matches_current: bool,
    #[serde(default)]
    pub last_public_rpc_rejection_blockhash_age_ns: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_cu_limit: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_cu_price_micro_lamports: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_reason: String,
    #[serde(default)]
    pub last_public_rpc_rejection_ix1_program_id: String,
    #[serde(default)]
    pub last_public_rpc_rejection_ix1_account_0: String,
    #[serde(default)]
    pub last_public_rpc_rejection_ix1_account_1: String,
    #[serde(default)]
    pub last_public_rpc_rejection_ix1_account_count: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_ix2_program_id: String,
    #[serde(default)]
    pub last_public_rpc_rejection_ix2_amount_in: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_ix2_min_out: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_ix2_account_count: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_actual_output_lamports: u64,
    #[serde(default)]
    pub last_public_rpc_rejection_required_output_lamports: u64,
    #[serde(default)]
    pub pm_reservation_released: u64,
    #[serde(default)]
    pub pm_release_send_failed: u64,
    #[serde(default)]
    pub pm_release_nacked: u64,
    #[serde(default)]
    pub worker_presign_gate_reject: u64,
    #[serde(default)]
    pub worker_template_build_ok: u64,
    #[serde(default)]
    pub worker_template_build_err: u64,
    #[serde(default)]
    pub worker_no_send_backend_called: u64,
    #[serde(default)]
    pub worker_no_send_backend_ok: u64,
    #[serde(default)]
    pub worker_no_send_backend_err: u64,
    #[serde(default)]
    pub tpu_quic_candidate_seen: u64,
    #[serde(default)]
    pub tpu_quic_would_enqueue: u64,
    #[serde(default)]
    pub tpu_quic_ring_enqueue_ok: u64,
    #[serde(default)]
    pub tpu_quic_ring_enqueue_err: u64,
    #[serde(default)]
    pub tpu_quic_pm_reserve_simulated: u64,
    #[serde(default)]
    pub tpu_quic_last_signature: String,
    #[serde(default)]
    pub tpu_quic_last_route_family: String,
    #[serde(default)]
    pub scorecard_replay_stale_slot_bypass_seen: u64,
    #[serde(default)]
    pub scorecard_replay_stale_slot_bypass_applied: u64,
    #[serde(default)]
    pub scorecard_replay_stale_slot_bypass_signature_match: u64,
    #[serde(default)]
    pub scorecard_replay_stale_slot_bypass_pool_match: u64,
    #[serde(default)]
    pub scorecard_replay_original_slot: u64,
    #[serde(default)]
    pub scorecard_replay_current_slot: u64,
    #[serde(default)]
    pub scorecard_replay_slot_gap: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_seen: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_applied: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_signature_match: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_pool_match: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_from_public_target: u64,
    #[serde(default)]
    pub scorecard_replay_route_override_to_tpu_quic_public: u64,
    #[serde(default)]
    pub scorecard_replay_source_override_seen: u64,
    #[serde(default)]
    pub scorecard_replay_source_override_applied: u64,
    #[serde(default)]
    pub scorecard_replay_source_override_signature_match: u64,
    #[serde(default)]
    pub scorecard_replay_source_override_pool_match: u64,
    #[serde(default)]
    pub scorecard_replay_source_override_live_send_blocked: u64,
    #[serde(default)]
    pub worker_final_no_send_event_constructed: u64,
    #[serde(default)]
    pub worker_final_no_send_event_published: u64,
    #[serde(default)]
    pub worker_final_no_send_event_publish_err: u64,
    #[serde(default)]
    pub worker_final_no_send_drop_reason_last: u64,
    #[serde(default)]
    pub worker_geyser_shadow_no_send_admitted: u64,
    #[serde(default)]
    pub worker_geyser_shadow_no_send_rejected: u64,
    #[serde(default)]
    pub worker_geyser_shadow_no_send_source_external: u64,
    #[serde(default)]
    pub worker_geyser_shadow_no_send_source_local_validator: u64,
    #[serde(default)]
    pub r1_auto_admission_attempt: u64,
    #[serde(default)]
    pub r1_auto_admission_admit: u64,
    #[serde(default)]
    pub r1_auto_admission_admit_to_process_intent_attempt: u64,
    #[serde(default)]
    pub r1_auto_admission_admit_to_process_intent_ok: u64,
    #[serde(default)]
    pub r1_auto_admission_admit_to_process_intent_drop: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_publish_attempt: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_publish_ok: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_publish_full: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_overwrite_same_pool: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_suppressed_outstanding_same_pool: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_outstanding_same_pool_hit: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_outstanding_cleared_matched: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_outstanding_cleared_timeout: u64,
    #[serde(default)]
    pub clmm_exact_quote_request_outstanding_cleared_no_quote: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_matched: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_no_snapshot: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_stale: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_pool_mismatch: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_mint_mismatch: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_amount_mismatch: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_request_id_mismatch: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_flags: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_flags_zero: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_flags_no_valid_bit: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_flags_no_quote_bit: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_flags_unknown_status: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_invalid_valid_quote_shape: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_no_quote_no_current_array_upstream: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_no_quote_no_liquidity_boundary: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_no_quote_out_of_range: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_no_quote_missing_tick_array_local: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout_terminal_no_quote: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout_quote_ready_rejected: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout_stale_request_id: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout_quote_build_failed: u64,
    #[serde(default)]
    pub clmm_exact_quote_response_timeout_dependency_missing: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_source: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_non_private_grade: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_shadowguard: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_micro_market: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_egress: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_inventory: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_cap: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_stale: u64,
    #[serde(default)]
    pub r1_auto_admission_drop_public_route: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_source: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_non_private_grade: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_shadowguard: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_micro_market: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_egress: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_inventory: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_cap: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_stale: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_route: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_template_executability: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_attempt_latch: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_no_quote: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_no_positive_ev: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_no_shadow_guard: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_no_micro_market: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_public_scout_shape: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_attempt_latch: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_proof_filter_other: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_route_not_public_scout: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_wrong_source: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_wrong_venue: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_missing_template_family: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_route_template_mismatch: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_public_scout_shape_other: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_micro_market_no_uplift: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_micro_market_negative_ev: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_micro_market_below_floor: u64,
    #[serde(default)]
    pub r1_auto_admission_reject_micro_market_other: u64,
    #[serde(default)]
    pub last_public_scout_shape_reject_reason: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_pool: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_mint_in: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_mint_out: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_route: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_source: String,
    #[serde(default)]
    pub last_public_scout_shape_reject_venue: String,
    #[serde(default)]
    pub last_run38_target_pool: String,
    #[serde(default)]
    pub last_run38_target_mint_in: String,
    #[serde(default)]
    pub last_run38_target_mint_out: String,
    #[serde(default)]
    pub last_run38_target_route: String,
    #[serde(default)]
    pub last_run38_target_source: String,
    #[serde(default)]
    pub last_run38_target_venue: String,
    #[serde(default)]
    pub last_run38_target_stage: String,
    #[serde(default)]
    pub last_run38_target_reject_reason: String,
    #[serde(default)]
    pub last_run38_target_inventory_reject_reason: String,
    #[serde(default)]
    pub last_run38_target_inventory_pool: String,
    #[serde(default)]
    pub last_run38_target_inventory_mint_in: String,
    #[serde(default)]
    pub last_run38_target_inventory_mint_out: String,
    #[serde(default)]
    pub last_run38_target_inventory_input_ata: String,
    #[serde(default)]
    pub last_run38_target_inventory_input_balance: u64,
    #[serde(default)]
    pub last_run38_target_inventory_required_input: u64,
    #[serde(default)]
    pub last_run38_target_inventory_token_program: String,
    #[serde(default)]
    pub last_r1_auto_admission_reject_reason: String,
    #[serde(default)]
    pub last_r1_auto_admission_reject_stage: String,
    #[serde(default)]
    pub last_r1_auto_admission_pool: String,
    #[serde(default)]
    pub last_r1_auto_admission_mint_in: String,
    #[serde(default)]
    pub last_r1_auto_admission_mint_out: String,
    #[serde(default)]
    pub last_r1_template_executability_reject_reason: String,
    #[serde(default)]
    pub last_r1_template_executability_pool: String,
    #[serde(default)]
    pub last_r1_template_executability_mint_in: String,
    #[serde(default)]
    pub last_r1_template_executability_mint_out: String,
    #[serde(default)]
    pub last_r1_template_executability_route: String,
    #[serde(default)]
    pub last_r1_template_executability_output_ata: String,
    #[serde(default)]
    pub last_admit_to_process_intent_drop_reason: String,
    #[serde(default)]
    pub last_admit_to_process_intent_pool: String,
    #[serde(default)]
    pub last_admit_to_process_intent_mint_in: String,
    #[serde(default)]
    pub last_admit_to_process_intent_mint_out: String,
    #[serde(default)]
    pub last_admit_to_process_intent_route: String,
    #[serde(default)]
    pub last_admit_to_process_intent_source: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_mismatch_reason: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_pool_index: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_sequence: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_request_id_expected: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_request_id_found: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_pool_expected: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_pool_found: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_mint_in_expected: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_mint_in_found: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_mint_out_expected: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_mint_out_found: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_amount_expected: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_amount_found: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_validity_flags: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_status: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_status_code: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_timeout_classification: String,
    #[serde(default)]
    pub last_clmm_exact_quote_response_quote_epoch: u64,
    #[serde(default)]
    pub last_clmm_exact_quote_response_last_update_unix_ns: u64,
    #[serde(default)]
    pub r1_afxdp_private_grade_route_actionable: u64,
    #[serde(default)]
    pub r1_afxdp_private_grade_process_intent_entered: u64,
    #[serde(default)]
    pub r1_afxdp_private_grade_auto_admission_attempt: u64,
    #[serde(default)]
    pub r1_afxdp_private_grade_auto_admission_admit: u64,
    #[serde(default)]
    pub worker_signal_public_geyser: u64,
    #[serde(default)]
    pub worker_signal_external_geyser: u64,
    #[serde(default)]
    pub worker_signal_local_validator_geyser: u64,
    #[serde(default)]
    pub worker_signal_jito_searcher_api: u64,
    #[serde(default)]
    pub worker_signal_private_mempool: u64,
    #[serde(default)]
    pub worker_signal_harmonic_searcher_api: u64,
    #[serde(default)]
    pub worker_signal_afxdp: u64,
    #[serde(default)]
    pub worker_signal_dpdk: u64,
    #[serde(default)]
    pub worker_signal_unknown: u64,
    #[serde(default)]
    pub dpdk_worker_records_seen: u64,
    #[serde(default)]
    pub dpdk_worker_health_drop: u64,
    #[serde(default)]
    pub dpdk_worker_stale_slot_drop: u64,
    #[serde(default)]
    pub dpdk_worker_pool_state_miss: u64,
    #[serde(default)]
    pub dpdk_worker_pool_state_hit: u64,
    #[serde(default)]
    pub dpdk_worker_route_actionable: u64,
    #[serde(default)]
    pub dpdk_worker_route_dropped: u64,
    #[serde(default)]
    pub dpdk_worker_route_unavailable: u64,
    #[serde(default)]
    pub dpdk_worker_route_unavailable_unknown_signal: u64,
    #[serde(default)]
    pub dpdk_worker_route_unavailable_hydration_none: u64,
    #[serde(default)]
    pub dpdk_worker_r1_admission_drop: u64,
    #[serde(default)]
    pub dpdk_worker_candidates_entered_process_intent: u64,
    #[serde(default)]
    pub dpdk_worker_pipeline_error: u64,
    #[serde(default)]
    pub dpdk_worker_rollout_shadow_published: u64,
    #[serde(default)]
    pub dpdk_worker_final_no_send_published: u64,
    #[serde(default)]
    pub worker_source_probe_afxdp_parsed_shred_drops: u64,
    #[serde(default)]
    pub afxdp_frames_seen: u64,
    #[serde(default)]
    pub afxdp_udp_8000_8025_seen: u64,
    #[serde(default)]
    pub afxdp_xsk_rx_batches: u64,
    #[serde(default)]
    pub afxdp_shred_parse_ok: u64,
    #[serde(default)]
    pub afxdp_shred_parse_err: u64,
    #[serde(default)]
    pub afxdp_shred_parse_data_ok: u64,
    #[serde(default)]
    pub afxdp_shred_parse_coding_ok: u64,
    #[serde(default)]
    pub afxdp_feed_enqueue_ok: u64,
    #[serde(default)]
    pub afxdp_feed_enqueue_full: u64,
    #[serde(default)]
    pub afxdp_reassembly_insert_ok: u64,
    #[serde(default)]
    pub afxdp_reassembly_complete_entries: u64,
    #[serde(default)]
    pub afxdp_fec_recovered_entries: u64,
    #[serde(default)]
    pub afxdp_data_shred_payload_ok: u64,
    #[serde(default)]
    pub afxdp_tx_extract_attempt: u64,
    #[serde(default)]
    pub afxdp_tx_extract_ok: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_empty_payload: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_short_vec: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_bad_entry_header: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_bad_compact_u16: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_sanitize_failed: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_no_transactions: u64,
    #[serde(default)]
    pub afxdp_tx_extract_err_unsupported_format: u64,
    #[serde(default)]
    pub afxdp_tx_record_emit_attempt: u64,
    #[serde(default)]
    pub afxdp_tx_record_emit_ok: u64,
    #[serde(default)]
    pub afxdp_tx_record_emit_full: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_unsupported_program: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_allowlist_miss: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_decode_fail: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_route_fail: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_mint_fail: u64,
    #[serde(default)]
    pub afxdp_pipeline_drop_other: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_0_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_0_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_1_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_1_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_2_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_2_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_3_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_3_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_4_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_4_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_5_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_5_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_6_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_6_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_7_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_7_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_8_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_8_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_9_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_9_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_10_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_10_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_11_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_11_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_12_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_12_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_13_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_13_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_14_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_14_count: u64,
    #[serde(default)]
    pub afxdp_unsupported_program_top_15_id: String,
    #[serde(default)]
    pub afxdp_unsupported_program_top_15_count: u64,
    #[serde(default)]
    pub afxdp_source_probe_emit_attempt: u64,
    #[serde(default)]
    pub afxdp_source_probe_emit_ok: u64,
    #[serde(default)]
    pub afxdp_source_probe_emit_full: u64,
    #[serde(default)]
    pub afxdp_source_probe_emit_err: u64,
    #[serde(default)]
    pub grpc_connected: u64,
    #[serde(default)]
    pub grpc_subscribe_sent: u64,
    #[serde(default)]
    pub grpc_updates_total: u64,
    #[serde(default)]
    pub grpc_updates_transaction: u64,
    #[serde(default)]
    pub grpc_updates_non_transaction: u64,
    #[serde(default)]
    pub grpc_normalize_ok: u64,
    #[serde(default)]
    pub grpc_normalize_err: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_unsupported_program: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_allowlist_miss: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_decode_fail: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_route_fail: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_mint_fail: u64,
    #[serde(default)]
    pub grpc_pipeline_drop_other: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_0_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_0_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_1_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_1_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_2_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_2_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_3_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_3_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_4_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_4_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_5_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_5_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_6_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_6_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_7_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_7_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_8_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_8_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_9_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_9_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_10_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_10_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_11_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_11_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_12_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_12_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_13_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_13_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_14_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_14_count: u64,
    #[serde(default)]
    pub grpc_unsupported_program_top_15_id: String,
    #[serde(default)]
    pub grpc_unsupported_program_top_15_count: u64,
    #[serde(default)]
    pub grpc_feed_enqueue_ok: u64,
    #[serde(default)]
    pub grpc_feed_enqueue_full: u64,
    #[serde(default)]
    pub grpc_feed_enqueue_err: u64,
    #[serde(default)]
    pub grpc_signal_source_local_validator_geyser: u64,
    #[serde(default)]
    pub worker_route_attempts: u64,
    #[serde(default)]
    pub worker_route_actionable: u64,
    #[serde(default)]
    pub worker_route_dropped: u64,
    #[serde(default)]
    pub worker_route_unavailable: u64,
    #[serde(default)]
    pub worker_route_unavailable_unknown_signal: u64,
    #[serde(default)]
    pub worker_route_unavailable_hydration_none: u64,
    #[serde(default)]
    pub worker_route_unavailable_hydration_none_afxdp: u64,
    #[serde(default)]
    pub worker_afxdp_hydration_request_enqueued: u64,
    #[serde(default)]
    pub worker_afxdp_hydration_request_deduped: u64,
    #[serde(default)]
    pub worker_route_reason_public_target: u64,
    #[serde(default)]
    pub worker_route_reason_stale_harmonic_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_quarantined_harmonic_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_tactic_mask_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_harmonic_egress_gate_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_over_cap: u64,
    #[serde(default)]
    pub worker_route_reason_no_viable_route: u64,
    #[serde(default)]
    pub worker_route_reason_atba_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_apex_partial_no_jito: u64,
    #[serde(default)]
    pub worker_route_reason_jito_egress_gate_fallback: u64,
    #[serde(default)]
    pub worker_route_reason_jito_stale_harmonic_reroute: u64,
    #[serde(default)]
    pub worker_route_reason_jito_quarantined_fallback: u64,
    #[serde(default)]
    pub worker_route_reason_jito_tactic_mask_fallback: u64,
    #[serde(default)]
    pub worker_route_reason_jito_direct: u64,
    #[serde(default)]
    pub worker_route_reason_jito_atba: u64,
    #[serde(default)]
    pub worker_route_reason_jito_apex_partial: u64,
    #[serde(default)]
    pub worker_route_reason_harmonic_fresh_plain: u64,
    #[serde(default)]
    pub worker_pipeline_attempts: u64,
    #[serde(default)]
    pub worker_pipeline_success: u64,
    #[serde(default)]
    pub worker_pipeline_err_routing_hydration: u64,
    #[serde(default)]
    pub worker_pipeline_err_blockhash_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_pool_tag_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_pool_arena_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_pool_arena_torn: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_pool_stale: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_in_tag_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_in_arena_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_in_arena_torn: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_out_tag_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_out_arena_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_out_arena_torn: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_mint_stale: u64,
    #[serde(default)]
    pub worker_pipeline_err_lookup_pubkey_mismatch: u64,
    #[serde(default)]
    pub last_lookup_fail_reason: String,
    #[serde(default)]
    pub last_lookup_fail_pool_pubkey: String,
    #[serde(default)]
    pub last_lookup_fail_mint_in: String,
    #[serde(default)]
    pub last_lookup_fail_mint_out: String,
    #[serde(default)]
    pub last_lookup_fail_venue: u8,
    #[serde(default)]
    pub last_lookup_fail_pool_index: u32,
    #[serde(default)]
    pub last_lookup_fail_pool_last_update_unix_ns: u64,
    #[serde(default)]
    pub last_lookup_fail_record_unix_ns: u64,
    #[serde(default)]
    pub last_lookup_fail_pool_age_ns: u64,
    #[serde(default)]
    pub last_lookup_fail_max_pool_staleness_ns: u64,
    #[serde(default)]
    pub last_lookup_fail_health_snapshot_slot: u64,
    #[serde(default)]
    pub last_lookup_fail_pool_reserve_in: u64,
    #[serde(default)]
    pub last_lookup_fail_pool_reserve_out: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_arena_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_snapshot_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_pool_mismatch: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_mint_in_mismatch: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_mint_out_mismatch: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_amount_mismatch: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_direction_mismatch: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_no_quote_out_of_range: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_stale_snapshot: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote_clmm_target_out_underflow: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_pool: String,
    #[serde(default)]
    pub last_clmm_invalid_quote_mint_in: String,
    #[serde(default)]
    pub last_clmm_invalid_quote_mint_out: String,
    #[serde(default)]
    pub last_clmm_invalid_quote_amount_in: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_expected_out: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_min_out: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_projected_reserve_in: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_projected_reserve_out: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_quote_epoch: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_last_update_unix_ns: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_post_sqrt_price_x64: String,
    #[serde(default)]
    pub last_clmm_invalid_quote_post_tick: i32,
    #[serde(default)]
    pub last_clmm_invalid_quote_crossed_tick_count: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_direction: u64,
    #[serde(default)]
    pub last_clmm_invalid_quote_validity_flags: u64,
    #[serde(default)]
    pub worker_pipeline_err_invalid_direction: u64,
    #[serde(default)]
    pub worker_pipeline_err_overlay_overflow: u64,
    #[serde(default)]
    pub worker_pipeline_err_no_send_sink_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_apex_not_positive: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_atba_missing_dist: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_dominance_live: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_no_usable_lut: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_net_after_fee_floor: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_over_15_sol_live_input: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_fee_clamp_invalid: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_inventory_breach: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_other: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_invalid_guard_pubkey: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_invalid_template_for_route: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_tactic_not_allowed_on_route: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_oversized_payload: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_unsupported_additional_signer: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_sign: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_enqueue_full: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_deadline_exceeded: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_no_send_sink_full: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_build_other: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_other: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit: u64,

    #[serde(default)]
    pub public_lane_amputation_ok: bool,
    #[serde(default)]
    pub inventory_truth_ok: bool,
    #[serde(default)]
    pub fee_leak_ok: bool,
    #[serde(default)]
    pub fee_leak_accounted: bool,
    #[serde(default)]
    pub phantom_inventory_risk: bool,
    #[serde(default)]
    pub selective_death_clamp_ok: bool,
    #[serde(default)]
    pub pre_sign_egress_gate_ok: bool,
    #[serde(default)]
    pub private_lane_admissibility_ok: bool,
    #[serde(default)]
    pub rollout_events_observed: u64,
    #[serde(default)]
    pub rollout_would_send: u64,
    #[serde(default)]
    pub rollout_live_send_suppressed: u64,
    #[serde(default)]
    pub rollout_private_lane_admissible_opportunities: u64,
    #[serde(default)]
    pub rollout_private_grade_admissible_opportunities: u64,
    #[serde(default)]
    pub rollout_geyser_shadow_opportunities: u64,
    #[serde(default)]
    pub rollout_afxdp_private_grade_would_send: u64,
    #[serde(default)]
    pub rollout_afxdp_private_grade_live_send_suppressed: u64,
    #[serde(default)]
    pub rollout_public_drops: u64,
    #[serde(default)]
    pub rollout_stale_state_reroutes: u64,
    #[serde(default)]
    pub rollout_egress_gate_reroutes: u64,
    #[serde(default)]
    pub rollout_egress_gate_drops: u64,
    #[serde(default)]
    pub rollout_clamped_apex_candidates: u64,
    #[serde(default)]
    pub rollout_atba_missing_dist_candidates: u64,
    #[serde(default)]
    pub rollout_jito_no_send_would_sends: u64,
    #[serde(default)]
    pub rollout_harmonic_no_send_would_sends: u64,
    #[serde(default)]
    pub rollout_public_rpc_no_send_would_sends: u64,
    #[serde(default)]
    pub rollout_drop_reason_none: u64,
    #[serde(default)]
    pub rollout_drop_reason_public_target: u64,
    #[serde(default)]
    pub rollout_drop_reason_apex_not_positive: u64,
    #[serde(default)]
    pub rollout_drop_reason_harmonic_stale_state: u64,
    #[serde(default)]
    pub rollout_drop_reason_harmonic_tactic_mask: u64,
    #[serde(default)]
    pub rollout_drop_reason_egress_gate_jito: u64,
    #[serde(default)]
    pub rollout_drop_reason_egress_gate_harmonic: u64,
    #[serde(default)]
    pub rollout_drop_reason_rerouted_pre_sign_egress_gate: u64,
    #[serde(default)]
    pub rollout_drop_reason_atba_no_dist_to_tick: u64,
    #[serde(default)]
    pub rollout_drop_reason_taylor_prefilter: u64,
    #[serde(default)]
    pub rollout_drop_reason_would_drop_dominance_gate: u64,
    #[serde(default)]
    pub rollout_drop_reason_dropped_dominance_gate: u64,
    #[serde(default)]
    pub rollout_drop_reason_no_usable_lut_bank: u64,
    #[serde(default)]
    pub rollout_drop_reason_no_shadow_guard: u64,
    #[serde(default)]
    pub rollout_drop_reason_rollout_cap: u64,
    #[serde(default)]
    pub rollout_drop_reason_rollout_no_send: u64,
    #[serde(default)]
    pub rollout_last_no_send_reason: u16,
    #[serde(default)]
    pub rollout_last_drop_reason: u16,
    #[serde(default)]
    pub latency_ok: bool,
    #[serde(default)]
    pub worker_latency_proof_present: bool,
    #[serde(default)]
    pub worker_latency_p99_ns: u64,
    #[serde(default)]
    pub worker_latency_budget_ns: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_count: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_p50_ns: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_p90_ns: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_p99_ns: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_p999_ns: u64,
    #[serde(default)]
    pub afxdp_producer_to_feed_max_ns: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_count: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_p50_ns: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_p90_ns: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_p99_ns: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_p999_ns: u64,
    #[serde(default)]
    pub afxdp_feed_to_worker_max_ns: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_count: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_p50_ns: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_p90_ns: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_p99_ns: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_p999_ns: u64,
    #[serde(default)]
    pub afxdp_worker_to_nosend_max_ns: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_count: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_p50_ns: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_p90_ns: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_p99_ns: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_p999_ns: u64,
    #[serde(default)]
    pub afxdp_end_to_end_observe_max_ns: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_count: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_p50_ns: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_p90_ns: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_p99_ns: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_p999_ns: u64,
    #[serde(default)]
    pub dpdk_producer_to_feed_max_ns: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_count: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_p50_ns: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_p90_ns: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_p99_ns: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_p999_ns: u64,
    #[serde(default)]
    pub dpdk_feed_to_worker_max_ns: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_count: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_p50_ns: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_p90_ns: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_p99_ns: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_p999_ns: u64,
    #[serde(default)]
    pub dpdk_worker_to_nosend_max_ns: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_count: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_p50_ns: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_p90_ns: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_p99_ns: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_p999_ns: u64,
    #[serde(default)]
    pub dpdk_end_to_end_observe_max_ns: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_count: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_p50_ns: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_p90_ns: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_p99_ns: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_p999_ns: u64,
    #[serde(default)]
    pub triton_grpc_producer_to_feed_max_ns: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_count: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_p50_ns: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_p90_ns: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_p99_ns: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_p999_ns: u64,
    #[serde(default)]
    pub triton_grpc_feed_to_worker_max_ns: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_count: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_p50_ns: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_p90_ns: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_p99_ns: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_p999_ns: u64,
    #[serde(default)]
    pub triton_grpc_worker_to_nosend_max_ns: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_count: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_p50_ns: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_p90_ns: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_p99_ns: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_p999_ns: u64,
    #[serde(default)]
    pub triton_grpc_end_to_end_observe_max_ns: u64,
    #[serde(default)]
    pub realized_pnl_ok: bool,
    #[serde(default)]
    pub shadow_pnl_ok: bool,
    #[serde(default)]
    pub competition_adjusted_ok: bool,
    #[serde(default)]
    pub competition_adjusted_retained_ev_delta_lamports: i64,
    #[serde(default)]
    pub competition_adjusted_fee_burn_delta_lamports: i64,
    #[serde(default)]
    pub micro_market_uplift_ok: bool,
    #[serde(default)]
    pub micro_market_observations: u64,
    #[serde(default)]
    pub micro_market_afxdp_private_grade_observations: u64,
    #[serde(default)]
    pub micro_market_banked_retained_ev_lamports: i64,
    #[serde(default)]
    pub micro_market_flat_baseline_retained_ev_lamports: i64,
    #[serde(default)]
    pub micro_market_delta_lamports: i64,
    #[serde(default)]
    pub dominance_shadow_pnl_ok: bool,
    #[serde(default)]
    pub taylor_gate_ok: bool,
    #[serde(default)]
    pub shadow_guard_deployed: bool,
    #[serde(default)]
    pub shadow_guard_program_id_present: bool,
    #[serde(default)]
    pub shadow_guard_mainnet_deployed: bool,
    #[serde(default)]
    pub shadow_guard_template_program_id_matches: bool,
    #[serde(default)]
    pub sparse_classes_fallback_safe: bool,
    #[serde(default)]
    pub baseline_shadow_comparator_enabled: bool,
    #[serde(default)]
    pub failback_gate_exists: bool,

    #[serde(default)]
    pub tactical_lane: G12TacticalLaneEvidence,
    #[serde(default)]
    pub additive_quant_tactics: G12AdditiveQuantTacticsEvidence,
}

impl Default for G12GateEvaluatorInput {
    fn default() -> Self {
        Self {
            rung: 0,
            sample_slots: 0,
            observe_candidates: 0,
            observe_candidates_total: 0,
            observe_candidates_external_geyser: 0,
            observe_candidates_local_validator_geyser: 0,
            observe_candidates_private_grade: 0,
            live_landed: 0,
            min_sample_slots: 0,
            min_observe_candidates: 0,
            min_live_landed: 0,
            worker_records_seen: 0,
            worker_health_snapshot_missing: 0,
            worker_health_drops: 0,
            worker_stale_slot_drops: 0,
            worker_pool_state_miss: 0,
            worker_pool_state_miss_tag_map_absent: 0,
            worker_pool_state_miss_arena_oob: 0,
            worker_pool_state_miss_arena_torn: 0,
            worker_pool_state_miss_pubkey_mismatch: 0,
            last_worker_pool_state_miss_pool_pubkey: String::new(),
            last_worker_pool_state_miss_venue_id: 0,
            last_worker_pool_state_miss_signal_source: 0,
            last_worker_pool_state_miss_reason_code: 0,
            worker_pool_state_miss_recent_ring: Vec::new(),
            worker_pool_state_miss_top_recent: Vec::new(),
            worker_pool_state_hit: 0,
            worker_early_drop_mint_in_tag_missing: 0,
            worker_early_drop_mint_out_tag_missing: 0,
            worker_shadow_events_published: 0,
            worker_rollout_would_send_events_published: 0,
            worker_rollout_live_send_suppressed_events_published: 0,
            worker_candidates_entered_process_intent: 0,
            worker_candidates_dropped_before_process_intent: 0,
            worker_process_intent_ok: 0,
            worker_process_intent_err: 0,
            last_process_intent_error: String::new(),
            last_process_intent_error_stage: String::new(),
            last_process_intent_error_reason: String::new(),
            worker_quote_ok: 0,
            worker_quote_err: 0,
            worker_lookup_ok: 0,
            worker_policy_ok: 0,
            worker_assembly_ready: 0,
            worker_pm_reservation_created: 0,
            worker_route_resolved_jito: 0,
            worker_route_resolved_public_rpc_scout: 0,
            worker_route_resolved_harmonic: 0,
            worker_route_resolved_drop: 0,
            worker_public_rpc_scout_emit_attempt: 0,
            worker_public_rpc_scout_emit_ok: 0,
            run38_target_public_scout_clmm_route_resolved: 0,
            run38_target_public_scout_clmm_r1_attempt: 0,
            run38_target_public_scout_clmm_reject_proof_filter: 0,
            run38_target_public_scout_clmm_reject_micro_market: 0,
            run38_target_public_scout_clmm_reject_inventory: 0,
            run38_target_public_scout_clmm_reject_inventory_missing_input_ata: 0,
            run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: 0,
            run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: 0,
            run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: 0,
            run38_target_public_scout_clmm_reject_inventory_other: 0,
            run38_target_public_scout_clmm_reject_template_executability: 0,
            run38_target_public_scout_clmm_reject_attempt_latch: 0,
            run38_target_public_scout_clmm_admit: 0,
            run38_target_public_scout_clmm_process_intent: 0,
            run38_target_public_scout_clmm_emit_attempt: 0,
            run38_target_public_scout_clmm_transport_attempt: 0,
            run38_target_public_scout_clmm_pm_created: 0,
            public_scout_attempt_latch_set: 0,
            public_scout_attempt_latch_drop_count: 0,
            public_scout_attempt_latch_first_signature: String::new(),
            pm_reserve_attempt: 0,
            pm_reserve_ok: 0,
            pm_reserve_reject_no_live_mode: 0,
            pm_reserve_reject_no_send_mode: 0,
            pm_reserve_reject_route_not_live: 0,
            pm_reserve_reject_cap: 0,
            pm_reserve_reject_inventory: 0,
            pm_reserve_reject_missing_signature: 0,
            pm_reserve_reject_other: 0,
            last_pm_reserve_reject_reason: String::new(),
            last_pm_reserve_route: String::new(),
            last_pm_reserve_pool: String::new(),
            last_pm_reserve_mint_in: String::new(),
            last_pm_reserve_mint_out: String::new(),
            last_pm_reserve_amount: 0,
            clmm_input_ata_missing: 0,
            clmm_output_ata_missing: 0,
            clmm_input_ata_uninitialized: 0,
            clmm_output_ata_uninitialized: 0,
            clmm_input_balance_insufficient: 0,
            clmm_token_program_mismatch: 0,
            last_clmm_input_mint: String::new(),
            last_clmm_output_mint: String::new(),
            last_clmm_input_token_account: String::new(),
            last_clmm_output_token_account: String::new(),
            template_drop_no_template: 0,
            template_drop_wrong_route_family: 0,
            template_drop_missing_clmm_accounts: 0,
            template_drop_missing_input_ata: 0,
            template_drop_missing_output_ata: 0,
            template_drop_invalid_swap_program: 0,
            template_drop_invalid_guard_template: 0,
            template_drop_unsupported_token_program: 0,
            template_drop_other: 0,
            last_template_drop_reason: String::new(),
            last_template_drop_pool: String::new(),
            last_template_drop_mint_in: String::new(),
            last_template_drop_mint_out: String::new(),
            last_template_drop_route: String::new(),
            last_template_drop_template_program: String::new(),
            last_template_drop_input_ata: String::new(),
            last_template_drop_output_ata: String::new(),
            public_rpc_scout_enqueued: 0,
            public_rpc_transport_attempt: 0,
            public_rpc_transport_failure: 0,
            public_rpc_transport_rejected: 0,
            public_rpc_nack_recorded: 0,
            last_public_rpc_rejection_code: 0,
            last_public_rpc_rejection_message: String::new(),
            last_public_rpc_rejection_body: String::new(),
            last_public_rpc_rejection_data_err: String::new(),
            last_public_rpc_rejection_simulation_logs: String::new(),
            last_public_rpc_rejection_signature: String::new(),
            last_public_rpc_rejection_tx_size: 0,
            last_public_rpc_rejection_blockhash: String::new(),
            last_public_rpc_rejection_blockhash_matches_current: false,
            last_public_rpc_rejection_blockhash_age_ns: 0,
            last_public_rpc_rejection_cu_limit: 0,
            last_public_rpc_rejection_cu_price_micro_lamports: 0,
            last_public_rpc_rejection_reason: String::new(),
            last_public_rpc_rejection_ix1_program_id: String::new(),
            last_public_rpc_rejection_ix1_account_0: String::new(),
            last_public_rpc_rejection_ix1_account_1: String::new(),
            last_public_rpc_rejection_ix1_account_count: 0,
            last_public_rpc_rejection_ix2_program_id: String::new(),
            last_public_rpc_rejection_ix2_amount_in: 0,
            last_public_rpc_rejection_ix2_min_out: 0,
            last_public_rpc_rejection_ix2_account_count: 0,
            last_public_rpc_rejection_actual_output_lamports: 0,
            last_public_rpc_rejection_required_output_lamports: 0,
            pm_reservation_released: 0,
            pm_release_send_failed: 0,
            pm_release_nacked: 0,
            worker_presign_gate_reject: 0,
            worker_template_build_ok: 0,
            worker_template_build_err: 0,
            worker_no_send_backend_called: 0,
            worker_no_send_backend_ok: 0,
            worker_no_send_backend_err: 0,
            tpu_quic_candidate_seen: 0,
            tpu_quic_would_enqueue: 0,
            tpu_quic_ring_enqueue_ok: 0,
            tpu_quic_ring_enqueue_err: 0,
            tpu_quic_pm_reserve_simulated: 0,
            tpu_quic_last_signature: String::new(),
            tpu_quic_last_route_family: String::new(),
            scorecard_replay_stale_slot_bypass_seen: 0,
            scorecard_replay_stale_slot_bypass_applied: 0,
            scorecard_replay_stale_slot_bypass_signature_match: 0,
            scorecard_replay_stale_slot_bypass_pool_match: 0,
            scorecard_replay_original_slot: 0,
            scorecard_replay_current_slot: 0,
            scorecard_replay_slot_gap: 0,
            scorecard_replay_route_override_seen: 0,
            scorecard_replay_route_override_applied: 0,
            scorecard_replay_route_override_signature_match: 0,
            scorecard_replay_route_override_pool_match: 0,
            scorecard_replay_route_override_from_public_target: 0,
            scorecard_replay_route_override_to_tpu_quic_public: 0,
            scorecard_replay_source_override_seen: 0,
            scorecard_replay_source_override_applied: 0,
            scorecard_replay_source_override_signature_match: 0,
            scorecard_replay_source_override_pool_match: 0,
            scorecard_replay_source_override_live_send_blocked: 0,
            worker_final_no_send_event_constructed: 0,
            worker_final_no_send_event_published: 0,
            worker_final_no_send_event_publish_err: 0,
            worker_final_no_send_drop_reason_last: 0,
            worker_geyser_shadow_no_send_admitted: 0,
            worker_geyser_shadow_no_send_rejected: 0,
            worker_geyser_shadow_no_send_source_external: 0,
            worker_geyser_shadow_no_send_source_local_validator: 0,
            r1_auto_admission_attempt: 0,
            r1_auto_admission_admit: 0,
            r1_auto_admission_admit_to_process_intent_attempt: 0,
            r1_auto_admission_admit_to_process_intent_ok: 0,
            r1_auto_admission_admit_to_process_intent_drop: 0,
            clmm_exact_quote_request_publish_attempt: 0,
            clmm_exact_quote_request_publish_ok: 0,
            clmm_exact_quote_request_publish_full: 0,
            clmm_exact_quote_request_overwrite_same_pool: 0,
            clmm_exact_quote_request_suppressed_outstanding_same_pool: 0,
            clmm_exact_quote_request_outstanding_same_pool_hit: 0,
            clmm_exact_quote_request_outstanding_cleared_matched: 0,
            clmm_exact_quote_request_outstanding_cleared_timeout: 0,
            clmm_exact_quote_request_outstanding_cleared_no_quote: 0,
            clmm_exact_quote_response_matched: 0,
            clmm_exact_quote_response_timeout: 0,
            clmm_exact_quote_response_no_snapshot: 0,
            clmm_exact_quote_response_stale: 0,
            clmm_exact_quote_response_pool_mismatch: 0,
            clmm_exact_quote_response_mint_mismatch: 0,
            clmm_exact_quote_response_amount_mismatch: 0,
            clmm_exact_quote_response_request_id_mismatch: 0,
            clmm_exact_quote_response_invalid_flags: 0,
            clmm_exact_quote_response_invalid_flags_zero: 0,
            clmm_exact_quote_response_invalid_flags_no_valid_bit: 0,
            clmm_exact_quote_response_invalid_flags_no_quote_bit: 0,
            clmm_exact_quote_response_invalid_flags_unknown_status: 0,
            clmm_exact_quote_response_invalid_valid_quote_shape: 0,
            clmm_exact_quote_response_no_quote_no_current_array_upstream: 0,
            clmm_exact_quote_response_no_quote_no_liquidity_boundary: 0,
            clmm_exact_quote_response_no_quote_out_of_range: 0,
            clmm_exact_quote_response_no_quote_missing_tick_array_local: 0,
            clmm_exact_quote_response_timeout_terminal_no_quote: 0,
            clmm_exact_quote_response_timeout_quote_ready_rejected: 0,
            clmm_exact_quote_response_timeout_stale_request_id: 0,
            clmm_exact_quote_response_timeout_quote_build_failed: 0,
            clmm_exact_quote_response_timeout_dependency_missing: 0,
            r1_auto_admission_drop_source: 0,
            r1_auto_admission_drop_non_private_grade: 0,
            r1_auto_admission_drop_shadowguard: 0,
            r1_auto_admission_drop_micro_market: 0,
            r1_auto_admission_drop_egress: 0,
            r1_auto_admission_drop_inventory: 0,
            r1_auto_admission_drop_cap: 0,
            r1_auto_admission_drop_stale: 0,
            r1_auto_admission_drop_public_route: 0,
            r1_auto_admission_reject_source: 0,
            r1_auto_admission_reject_non_private_grade: 0,
            r1_auto_admission_reject_shadowguard: 0,
            r1_auto_admission_reject_micro_market: 0,
            r1_auto_admission_reject_egress: 0,
            r1_auto_admission_reject_inventory: 0,
            r1_auto_admission_reject_cap: 0,
            r1_auto_admission_reject_stale: 0,
            r1_auto_admission_reject_public_route: 0,
            r1_auto_admission_reject_proof_filter: 0,
            r1_auto_admission_reject_template_executability: 0,
            r1_auto_admission_reject_attempt_latch: 0,
            r1_auto_admission_reject_proof_filter_no_quote: 0,
            r1_auto_admission_reject_proof_filter_no_positive_ev: 0,
            r1_auto_admission_reject_proof_filter_no_shadow_guard: 0,
            r1_auto_admission_reject_proof_filter_no_micro_market: 0,
            r1_auto_admission_reject_proof_filter_public_scout_shape: 0,
            r1_auto_admission_reject_proof_filter_attempt_latch: 0,
            r1_auto_admission_reject_proof_filter_other: 0,
            r1_auto_admission_reject_public_scout_shape_route_not_public_scout: 0,
            r1_auto_admission_reject_public_scout_shape_wrong_source: 0,
            r1_auto_admission_reject_public_scout_shape_wrong_venue: 0,
            r1_auto_admission_reject_public_scout_shape_missing_template_family: 0,
            r1_auto_admission_reject_public_scout_shape_route_template_mismatch: 0,
            r1_auto_admission_reject_public_scout_shape_other: 0,
            r1_auto_admission_reject_micro_market_no_uplift: 0,
            r1_auto_admission_reject_micro_market_negative_ev: 0,
            r1_auto_admission_reject_micro_market_below_floor: 0,
            r1_auto_admission_reject_micro_market_other: 0,
            last_public_scout_shape_reject_reason: String::new(),
            last_public_scout_shape_reject_pool: String::new(),
            last_public_scout_shape_reject_mint_in: String::new(),
            last_public_scout_shape_reject_mint_out: String::new(),
            last_public_scout_shape_reject_route: String::new(),
            last_public_scout_shape_reject_source: String::new(),
            last_public_scout_shape_reject_venue: String::new(),
            last_run38_target_pool: String::new(),
            last_run38_target_mint_in: String::new(),
            last_run38_target_mint_out: String::new(),
            last_run38_target_route: String::new(),
            last_run38_target_source: String::new(),
            last_run38_target_venue: String::new(),
            last_run38_target_stage: String::new(),
            last_run38_target_reject_reason: String::new(),
            last_run38_target_inventory_reject_reason: String::new(),
            last_run38_target_inventory_pool: String::new(),
            last_run38_target_inventory_mint_in: String::new(),
            last_run38_target_inventory_mint_out: String::new(),
            last_run38_target_inventory_input_ata: String::new(),
            last_run38_target_inventory_input_balance: 0,
            last_run38_target_inventory_required_input: 0,
            last_run38_target_inventory_token_program: String::new(),
            last_r1_auto_admission_reject_reason: String::new(),
            last_r1_auto_admission_reject_stage: String::new(),
            last_r1_auto_admission_pool: String::new(),
            last_r1_auto_admission_mint_in: String::new(),
            last_r1_auto_admission_mint_out: String::new(),
            last_r1_template_executability_reject_reason: String::new(),
            last_r1_template_executability_pool: String::new(),
            last_r1_template_executability_mint_in: String::new(),
            last_r1_template_executability_mint_out: String::new(),
            last_r1_template_executability_route: String::new(),
            last_r1_template_executability_output_ata: String::new(),
            last_admit_to_process_intent_drop_reason: String::new(),
            last_admit_to_process_intent_pool: String::new(),
            last_admit_to_process_intent_mint_in: String::new(),
            last_admit_to_process_intent_mint_out: String::new(),
            last_admit_to_process_intent_route: String::new(),
            last_admit_to_process_intent_source: String::new(),
            last_clmm_exact_quote_response_mismatch_reason: String::new(),
            last_clmm_exact_quote_response_pool_index: 0,
            last_clmm_exact_quote_response_sequence: 0,
            last_clmm_exact_quote_response_request_id_expected: 0,
            last_clmm_exact_quote_response_request_id_found: 0,
            last_clmm_exact_quote_response_pool_expected: String::new(),
            last_clmm_exact_quote_response_pool_found: String::new(),
            last_clmm_exact_quote_response_mint_in_expected: String::new(),
            last_clmm_exact_quote_response_mint_in_found: String::new(),
            last_clmm_exact_quote_response_mint_out_expected: String::new(),
            last_clmm_exact_quote_response_mint_out_found: String::new(),
            last_clmm_exact_quote_response_amount_expected: 0,
            last_clmm_exact_quote_response_amount_found: 0,
            last_clmm_exact_quote_response_validity_flags: 0,
            last_clmm_exact_quote_response_status: String::new(),
            last_clmm_exact_quote_response_status_code: 0,
            last_clmm_exact_quote_response_timeout_classification: String::new(),
            last_clmm_exact_quote_response_quote_epoch: 0,
            last_clmm_exact_quote_response_last_update_unix_ns: 0,
            r1_afxdp_private_grade_route_actionable: 0,
            r1_afxdp_private_grade_process_intent_entered: 0,
            r1_afxdp_private_grade_auto_admission_attempt: 0,
            r1_afxdp_private_grade_auto_admission_admit: 0,
            worker_signal_public_geyser: 0,
            worker_signal_external_geyser: 0,
            worker_signal_local_validator_geyser: 0,
            worker_signal_jito_searcher_api: 0,
            worker_signal_private_mempool: 0,
            worker_signal_harmonic_searcher_api: 0,
            worker_signal_afxdp: 0,
            worker_signal_dpdk: 0,
            worker_signal_unknown: 0,
            dpdk_worker_records_seen: 0,
            dpdk_worker_health_drop: 0,
            dpdk_worker_stale_slot_drop: 0,
            dpdk_worker_pool_state_miss: 0,
            dpdk_worker_pool_state_hit: 0,
            dpdk_worker_route_actionable: 0,
            dpdk_worker_route_dropped: 0,
            dpdk_worker_route_unavailable: 0,
            dpdk_worker_route_unavailable_unknown_signal: 0,
            dpdk_worker_route_unavailable_hydration_none: 0,
            dpdk_worker_r1_admission_drop: 0,
            dpdk_worker_candidates_entered_process_intent: 0,
            dpdk_worker_pipeline_error: 0,
            dpdk_worker_rollout_shadow_published: 0,
            dpdk_worker_final_no_send_published: 0,
            worker_source_probe_afxdp_parsed_shred_drops: 0,
            afxdp_frames_seen: 0,
            afxdp_udp_8000_8025_seen: 0,
            afxdp_xsk_rx_batches: 0,
            afxdp_shred_parse_ok: 0,
            afxdp_shred_parse_err: 0,
            afxdp_shred_parse_data_ok: 0,
            afxdp_shred_parse_coding_ok: 0,
            afxdp_feed_enqueue_ok: 0,
            afxdp_feed_enqueue_full: 0,
            afxdp_reassembly_insert_ok: 0,
            afxdp_reassembly_complete_entries: 0,
            afxdp_fec_recovered_entries: 0,
            afxdp_data_shred_payload_ok: 0,
            afxdp_tx_extract_attempt: 0,
            afxdp_tx_extract_ok: 0,
            afxdp_tx_extract_err: 0,
            afxdp_tx_extract_err_empty_payload: 0,
            afxdp_tx_extract_err_short_vec: 0,
            afxdp_tx_extract_err_bad_entry_header: 0,
            afxdp_tx_extract_err_bad_compact_u16: 0,
            afxdp_tx_extract_err_sanitize_failed: 0,
            afxdp_tx_extract_err_no_transactions: 0,
            afxdp_tx_extract_err_unsupported_format: 0,
            afxdp_tx_record_emit_attempt: 0,
            afxdp_tx_record_emit_ok: 0,
            afxdp_tx_record_emit_full: 0,
            afxdp_pipeline_drop_unsupported_program: 0,
            afxdp_pipeline_drop_allowlist_miss: 0,
            afxdp_pipeline_drop_decode_fail: 0,
            afxdp_pipeline_drop_route_fail: 0,
            afxdp_pipeline_drop_mint_fail: 0,
            afxdp_pipeline_drop_other: 0,
            afxdp_unsupported_program_top_0_id: String::new(),
            afxdp_unsupported_program_top_0_count: 0,
            afxdp_unsupported_program_top_1_id: String::new(),
            afxdp_unsupported_program_top_1_count: 0,
            afxdp_unsupported_program_top_2_id: String::new(),
            afxdp_unsupported_program_top_2_count: 0,
            afxdp_unsupported_program_top_3_id: String::new(),
            afxdp_unsupported_program_top_3_count: 0,
            afxdp_unsupported_program_top_4_id: String::new(),
            afxdp_unsupported_program_top_4_count: 0,
            afxdp_unsupported_program_top_5_id: String::new(),
            afxdp_unsupported_program_top_5_count: 0,
            afxdp_unsupported_program_top_6_id: String::new(),
            afxdp_unsupported_program_top_6_count: 0,
            afxdp_unsupported_program_top_7_id: String::new(),
            afxdp_unsupported_program_top_7_count: 0,
            afxdp_unsupported_program_top_8_id: String::new(),
            afxdp_unsupported_program_top_8_count: 0,
            afxdp_unsupported_program_top_9_id: String::new(),
            afxdp_unsupported_program_top_9_count: 0,
            afxdp_unsupported_program_top_10_id: String::new(),
            afxdp_unsupported_program_top_10_count: 0,
            afxdp_unsupported_program_top_11_id: String::new(),
            afxdp_unsupported_program_top_11_count: 0,
            afxdp_unsupported_program_top_12_id: String::new(),
            afxdp_unsupported_program_top_12_count: 0,
            afxdp_unsupported_program_top_13_id: String::new(),
            afxdp_unsupported_program_top_13_count: 0,
            afxdp_unsupported_program_top_14_id: String::new(),
            afxdp_unsupported_program_top_14_count: 0,
            afxdp_unsupported_program_top_15_id: String::new(),
            afxdp_unsupported_program_top_15_count: 0,
            afxdp_source_probe_emit_attempt: 0,
            afxdp_source_probe_emit_ok: 0,
            afxdp_source_probe_emit_full: 0,
            afxdp_source_probe_emit_err: 0,
            grpc_connected: 0,
            grpc_subscribe_sent: 0,
            grpc_updates_total: 0,
            grpc_updates_transaction: 0,
            grpc_updates_non_transaction: 0,
            grpc_normalize_ok: 0,
            grpc_normalize_err: 0,
            grpc_pipeline_drop_unsupported_program: 0,
            grpc_pipeline_drop_allowlist_miss: 0,
            grpc_pipeline_drop_decode_fail: 0,
            grpc_pipeline_drop_route_fail: 0,
            grpc_pipeline_drop_mint_fail: 0,
            grpc_pipeline_drop_other: 0,
            grpc_unsupported_program_top_0_id: String::new(),
            grpc_unsupported_program_top_0_count: 0,
            grpc_unsupported_program_top_1_id: String::new(),
            grpc_unsupported_program_top_1_count: 0,
            grpc_unsupported_program_top_2_id: String::new(),
            grpc_unsupported_program_top_2_count: 0,
            grpc_unsupported_program_top_3_id: String::new(),
            grpc_unsupported_program_top_3_count: 0,
            grpc_unsupported_program_top_4_id: String::new(),
            grpc_unsupported_program_top_4_count: 0,
            grpc_unsupported_program_top_5_id: String::new(),
            grpc_unsupported_program_top_5_count: 0,
            grpc_unsupported_program_top_6_id: String::new(),
            grpc_unsupported_program_top_6_count: 0,
            grpc_unsupported_program_top_7_id: String::new(),
            grpc_unsupported_program_top_7_count: 0,
            grpc_unsupported_program_top_8_id: String::new(),
            grpc_unsupported_program_top_8_count: 0,
            grpc_unsupported_program_top_9_id: String::new(),
            grpc_unsupported_program_top_9_count: 0,
            grpc_unsupported_program_top_10_id: String::new(),
            grpc_unsupported_program_top_10_count: 0,
            grpc_unsupported_program_top_11_id: String::new(),
            grpc_unsupported_program_top_11_count: 0,
            grpc_unsupported_program_top_12_id: String::new(),
            grpc_unsupported_program_top_12_count: 0,
            grpc_unsupported_program_top_13_id: String::new(),
            grpc_unsupported_program_top_13_count: 0,
            grpc_unsupported_program_top_14_id: String::new(),
            grpc_unsupported_program_top_14_count: 0,
            grpc_unsupported_program_top_15_id: String::new(),
            grpc_unsupported_program_top_15_count: 0,
            grpc_feed_enqueue_ok: 0,
            grpc_feed_enqueue_full: 0,
            grpc_feed_enqueue_err: 0,
            grpc_signal_source_local_validator_geyser: 0,
            worker_route_attempts: 0,
            worker_route_actionable: 0,
            worker_route_dropped: 0,
            worker_route_unavailable: 0,
            worker_route_unavailable_unknown_signal: 0,
            worker_route_unavailable_hydration_none: 0,
            worker_route_unavailable_hydration_none_afxdp: 0,
            worker_afxdp_hydration_request_enqueued: 0,
            worker_afxdp_hydration_request_deduped: 0,
            worker_route_reason_public_target: 0,
            worker_route_reason_stale_harmonic_no_jito: 0,
            worker_route_reason_quarantined_harmonic_no_jito: 0,
            worker_route_reason_tactic_mask_no_jito: 0,
            worker_route_reason_harmonic_egress_gate_no_jito: 0,
            worker_route_reason_over_cap: 0,
            worker_route_reason_no_viable_route: 0,
            worker_route_reason_atba_no_jito: 0,
            worker_route_reason_apex_partial_no_jito: 0,
            worker_route_reason_jito_egress_gate_fallback: 0,
            worker_route_reason_jito_stale_harmonic_reroute: 0,
            worker_route_reason_jito_quarantined_fallback: 0,
            worker_route_reason_jito_tactic_mask_fallback: 0,
            worker_route_reason_jito_direct: 0,
            worker_route_reason_jito_atba: 0,
            worker_route_reason_jito_apex_partial: 0,
            worker_route_reason_harmonic_fresh_plain: 0,
            worker_pipeline_attempts: 0,
            worker_pipeline_success: 0,
            worker_pipeline_err_routing_hydration: 0,
            worker_pipeline_err_blockhash_missing: 0,
            worker_pipeline_err_lookup: 0,
            worker_pipeline_err_lookup_pool_tag_missing: 0,
            worker_pipeline_err_lookup_pool_arena_missing: 0,
            worker_pipeline_err_lookup_pool_arena_torn: 0,
            worker_pipeline_err_lookup_pool_stale: 0,
            worker_pipeline_err_lookup_mint_in_tag_missing: 0,
            worker_pipeline_err_lookup_mint_in_arena_missing: 0,
            worker_pipeline_err_lookup_mint_in_arena_torn: 0,
            worker_pipeline_err_lookup_mint_out_tag_missing: 0,
            worker_pipeline_err_lookup_mint_out_arena_missing: 0,
            worker_pipeline_err_lookup_mint_out_arena_torn: 0,
            worker_pipeline_err_lookup_mint_stale: 0,
            worker_pipeline_err_lookup_pubkey_mismatch: 0,
            last_lookup_fail_reason: String::new(),
            last_lookup_fail_pool_pubkey: String::new(),
            last_lookup_fail_mint_in: String::new(),
            last_lookup_fail_mint_out: String::new(),
            last_lookup_fail_venue: 0,
            last_lookup_fail_pool_index: 0,
            last_lookup_fail_pool_last_update_unix_ns: 0,
            last_lookup_fail_record_unix_ns: 0,
            last_lookup_fail_pool_age_ns: 0,
            last_lookup_fail_max_pool_staleness_ns: 0,
            last_lookup_fail_health_snapshot_slot: 0,
            last_lookup_fail_pool_reserve_in: 0,
            last_lookup_fail_pool_reserve_out: 0,
            worker_pipeline_err_quote: 0,
            worker_pipeline_err_quote_clmm_arena_missing: 0,
            worker_pipeline_err_quote_clmm_snapshot_missing: 0,
            worker_pipeline_err_quote_clmm_pool_mismatch: 0,
            worker_pipeline_err_quote_clmm_mint_in_mismatch: 0,
            worker_pipeline_err_quote_clmm_mint_out_mismatch: 0,
            worker_pipeline_err_quote_clmm_amount_mismatch: 0,
            worker_pipeline_err_quote_clmm_direction_mismatch: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: 0,
            worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: 0,
            worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: 0,
            worker_pipeline_err_quote_clmm_no_quote_out_of_range: 0,
            worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: 0,
            worker_pipeline_err_quote_clmm_stale_snapshot: 0,
            worker_pipeline_err_quote_clmm_target_out_underflow: 0,
            last_clmm_invalid_quote_pool: String::new(),
            last_clmm_invalid_quote_mint_in: String::new(),
            last_clmm_invalid_quote_mint_out: String::new(),
            last_clmm_invalid_quote_amount_in: 0,
            last_clmm_invalid_quote_expected_out: 0,
            last_clmm_invalid_quote_min_out: 0,
            last_clmm_invalid_quote_projected_reserve_in: 0,
            last_clmm_invalid_quote_projected_reserve_out: 0,
            last_clmm_invalid_quote_quote_epoch: 0,
            last_clmm_invalid_quote_last_update_unix_ns: 0,
            last_clmm_invalid_quote_post_sqrt_price_x64: String::new(),
            last_clmm_invalid_quote_post_tick: 0,
            last_clmm_invalid_quote_crossed_tick_count: 0,
            last_clmm_invalid_quote_direction: 0,
            last_clmm_invalid_quote_validity_flags: 0,
            worker_pipeline_err_invalid_direction: 0,
            worker_pipeline_err_overlay_overflow: 0,
            worker_pipeline_err_no_send_sink_missing: 0,
            worker_pipeline_err_policy: 0,
            worker_pipeline_err_policy_apex_not_positive: 0,
            worker_pipeline_err_policy_atba_missing_dist: 0,
            worker_pipeline_err_policy_dominance_live: 0,
            worker_pipeline_err_policy_no_usable_lut: 0,
            worker_pipeline_err_policy_net_after_fee_floor: 0,
            worker_pipeline_err_policy_over_15_sol_live_input: 0,
            worker_pipeline_err_policy_fee_clamp_invalid: 0,
            worker_pipeline_err_policy_inventory_breach: 0,
            worker_pipeline_err_policy_other: 0,
            worker_pipeline_err_emit_invalid_guard_pubkey: 0,
            worker_pipeline_err_emit_invalid_template_for_route: 0,
            worker_pipeline_err_emit_tactic_not_allowed_on_route: 0,
            worker_pipeline_err_emit_oversized_payload: 0,
            worker_pipeline_err_emit_unsupported_additional_signer: 0,
            worker_pipeline_err_emit_sign: 0,
            worker_pipeline_err_emit_enqueue_full: 0,
            worker_pipeline_err_emit_deadline_exceeded: 0,
            worker_pipeline_err_emit_no_send_sink_full: 0,
            worker_pipeline_err_emit_build_other: 0,
            worker_pipeline_err_emit_other: 0,
            worker_pipeline_err_emit: 0,
            public_lane_amputation_ok: false,
            inventory_truth_ok: false,
            fee_leak_ok: false,
            fee_leak_accounted: false,
            phantom_inventory_risk: true,
            selective_death_clamp_ok: false,
            pre_sign_egress_gate_ok: false,
            private_lane_admissibility_ok: false,
            rollout_events_observed: 0,
            rollout_would_send: 0,
            rollout_live_send_suppressed: 0,
            rollout_private_lane_admissible_opportunities: 0,
            rollout_private_grade_admissible_opportunities: 0,
            rollout_geyser_shadow_opportunities: 0,
            rollout_afxdp_private_grade_would_send: 0,
            rollout_afxdp_private_grade_live_send_suppressed: 0,
            rollout_public_drops: 0,
            rollout_stale_state_reroutes: 0,
            rollout_egress_gate_reroutes: 0,
            rollout_egress_gate_drops: 0,
            rollout_clamped_apex_candidates: 0,
            rollout_atba_missing_dist_candidates: 0,
            rollout_jito_no_send_would_sends: 0,
            rollout_harmonic_no_send_would_sends: 0,
            rollout_public_rpc_no_send_would_sends: 0,
            rollout_drop_reason_none: 0,
            rollout_drop_reason_public_target: 0,
            rollout_drop_reason_apex_not_positive: 0,
            rollout_drop_reason_harmonic_stale_state: 0,
            rollout_drop_reason_harmonic_tactic_mask: 0,
            rollout_drop_reason_egress_gate_jito: 0,
            rollout_drop_reason_egress_gate_harmonic: 0,
            rollout_drop_reason_rerouted_pre_sign_egress_gate: 0,
            rollout_drop_reason_atba_no_dist_to_tick: 0,
            rollout_drop_reason_taylor_prefilter: 0,
            rollout_drop_reason_would_drop_dominance_gate: 0,
            rollout_drop_reason_dropped_dominance_gate: 0,
            rollout_drop_reason_no_usable_lut_bank: 0,
            rollout_drop_reason_no_shadow_guard: 0,
            rollout_drop_reason_rollout_cap: 0,
            rollout_drop_reason_rollout_no_send: 0,
            rollout_last_no_send_reason: 0,
            rollout_last_drop_reason: 0,
            latency_ok: false,
            worker_latency_proof_present: false,
            worker_latency_p99_ns: 0,
            worker_latency_budget_ns: 0,
            afxdp_producer_to_feed_count: 0,
            afxdp_producer_to_feed_p50_ns: 0,
            afxdp_producer_to_feed_p90_ns: 0,
            afxdp_producer_to_feed_p99_ns: 0,
            afxdp_producer_to_feed_p999_ns: 0,
            afxdp_producer_to_feed_max_ns: 0,
            afxdp_feed_to_worker_count: 0,
            afxdp_feed_to_worker_p50_ns: 0,
            afxdp_feed_to_worker_p90_ns: 0,
            afxdp_feed_to_worker_p99_ns: 0,
            afxdp_feed_to_worker_p999_ns: 0,
            afxdp_feed_to_worker_max_ns: 0,
            afxdp_worker_to_nosend_count: 0,
            afxdp_worker_to_nosend_p50_ns: 0,
            afxdp_worker_to_nosend_p90_ns: 0,
            afxdp_worker_to_nosend_p99_ns: 0,
            afxdp_worker_to_nosend_p999_ns: 0,
            afxdp_worker_to_nosend_max_ns: 0,
            afxdp_end_to_end_observe_count: 0,
            afxdp_end_to_end_observe_p50_ns: 0,
            afxdp_end_to_end_observe_p90_ns: 0,
            afxdp_end_to_end_observe_p99_ns: 0,
            afxdp_end_to_end_observe_p999_ns: 0,
            afxdp_end_to_end_observe_max_ns: 0,
            dpdk_producer_to_feed_count: 0,
            dpdk_producer_to_feed_p50_ns: 0,
            dpdk_producer_to_feed_p90_ns: 0,
            dpdk_producer_to_feed_p99_ns: 0,
            dpdk_producer_to_feed_p999_ns: 0,
            dpdk_producer_to_feed_max_ns: 0,
            dpdk_feed_to_worker_count: 0,
            dpdk_feed_to_worker_p50_ns: 0,
            dpdk_feed_to_worker_p90_ns: 0,
            dpdk_feed_to_worker_p99_ns: 0,
            dpdk_feed_to_worker_p999_ns: 0,
            dpdk_feed_to_worker_max_ns: 0,
            dpdk_worker_to_nosend_count: 0,
            dpdk_worker_to_nosend_p50_ns: 0,
            dpdk_worker_to_nosend_p90_ns: 0,
            dpdk_worker_to_nosend_p99_ns: 0,
            dpdk_worker_to_nosend_p999_ns: 0,
            dpdk_worker_to_nosend_max_ns: 0,
            dpdk_end_to_end_observe_count: 0,
            dpdk_end_to_end_observe_p50_ns: 0,
            dpdk_end_to_end_observe_p90_ns: 0,
            dpdk_end_to_end_observe_p99_ns: 0,
            dpdk_end_to_end_observe_p999_ns: 0,
            dpdk_end_to_end_observe_max_ns: 0,
            triton_grpc_producer_to_feed_count: 0,
            triton_grpc_producer_to_feed_p50_ns: 0,
            triton_grpc_producer_to_feed_p90_ns: 0,
            triton_grpc_producer_to_feed_p99_ns: 0,
            triton_grpc_producer_to_feed_p999_ns: 0,
            triton_grpc_producer_to_feed_max_ns: 0,
            triton_grpc_feed_to_worker_count: 0,
            triton_grpc_feed_to_worker_p50_ns: 0,
            triton_grpc_feed_to_worker_p90_ns: 0,
            triton_grpc_feed_to_worker_p99_ns: 0,
            triton_grpc_feed_to_worker_p999_ns: 0,
            triton_grpc_feed_to_worker_max_ns: 0,
            triton_grpc_worker_to_nosend_count: 0,
            triton_grpc_worker_to_nosend_p50_ns: 0,
            triton_grpc_worker_to_nosend_p90_ns: 0,
            triton_grpc_worker_to_nosend_p99_ns: 0,
            triton_grpc_worker_to_nosend_p999_ns: 0,
            triton_grpc_worker_to_nosend_max_ns: 0,
            triton_grpc_end_to_end_observe_count: 0,
            triton_grpc_end_to_end_observe_p50_ns: 0,
            triton_grpc_end_to_end_observe_p90_ns: 0,
            triton_grpc_end_to_end_observe_p99_ns: 0,
            triton_grpc_end_to_end_observe_p999_ns: 0,
            triton_grpc_end_to_end_observe_max_ns: 0,
            realized_pnl_ok: false,
            shadow_pnl_ok: false,
            competition_adjusted_ok: false,
            competition_adjusted_retained_ev_delta_lamports: 0,
            competition_adjusted_fee_burn_delta_lamports: 0,
            micro_market_uplift_ok: false,
            micro_market_observations: 0,
            micro_market_afxdp_private_grade_observations: 0,
            micro_market_banked_retained_ev_lamports: 0,
            micro_market_flat_baseline_retained_ev_lamports: 0,
            micro_market_delta_lamports: 0,
            dominance_shadow_pnl_ok: false,
            taylor_gate_ok: false,
            shadow_guard_deployed: false,
            shadow_guard_program_id_present: false,
            shadow_guard_mainnet_deployed: false,
            shadow_guard_template_program_id_matches: false,
            sparse_classes_fallback_safe: false,
            baseline_shadow_comparator_enabled: false,
            failback_gate_exists: false,
            tactical_lane: G12TacticalLaneEvidence::default(),
            additive_quant_tactics: G12AdditiveQuantTacticsEvidence::default(),
        }
    }
}

impl G12GateEvaluatorInput {
    fn sample_floor_ok(&self) -> bool {
        self.sample_slots >= self.min_sample_slots
            && self.observe_candidates >= self.min_observe_candidates
            && self.live_landed >= self.min_live_landed
    }

    fn competition_adjusted_promotion_ok(&self) -> bool {
        self.competition_adjusted_ok
            && self.competition_adjusted_retained_ev_delta_lamports >= 0
            && self.competition_adjusted_fee_burn_delta_lamports <= 0
            && self.sparse_classes_fallback_safe
            && self.baseline_shadow_comparator_enabled
            && self.failback_gate_exists
    }

    fn shadow_guard_promotion_ok(&self) -> bool {
        self.shadow_guard_program_id_present
            && (self.shadow_guard_mainnet_deployed || self.shadow_guard_deployed)
            && self.shadow_guard_template_program_id_matches
    }

    pub fn evaluate(&self) -> G12GateReport {
        let tactical_lane_ok = self.tactical_lane.is_green();
        let additive_quant_tactics_ok = self.additive_quant_tactics.is_green();
        let competition_adjusted_promotion_ok = self.competition_adjusted_promotion_ok();
        let shadow_guard_promotion_ok = self.shadow_guard_promotion_ok();
        let afxdp_private_grade_actionability_ok = self.r1_afxdp_private_grade_route_actionable > 0
            && self.r1_afxdp_private_grade_process_intent_entered > 0
            && self.rollout_afxdp_private_grade_would_send > 0
            && self.micro_market_afxdp_private_grade_observations > 0;

        let hard_fail = !self.public_lane_amputation_ok
            || !self.inventory_truth_ok
            || !self.fee_leak_ok
            || !self.selective_death_clamp_ok
            || !tactical_lane_ok;

        let soft_fail = !self.pre_sign_egress_gate_ok
            || !self.latency_ok
            || !self.realized_pnl_ok
            || !self.shadow_pnl_ok
            || !competition_adjusted_promotion_ok
            || !self.micro_market_uplift_ok
            || !self.dominance_shadow_pnl_ok
            || !self.taylor_gate_ok
            || !afxdp_private_grade_actionability_ok
            || !additive_quant_tactics_ok;

        let evidence_window_ok = self.sample_floor_ok();
        let kill_switch_required = hard_fail;
        let failback_required =
            !kill_switch_required && (soft_fail || (!evidence_window_ok && self.live_landed > 0));
        let public_scout_gates_green = !kill_switch_required
            && shadow_guard_promotion_ok
            && self.selective_death_clamp_ok
            && self.pre_sign_egress_gate_ok
            && self.inventory_truth_ok
            && self.fee_leak_ok
            && self.fee_leak_accounted
            && !self.phantom_inventory_risk;

        let all_gates_green = !kill_switch_required
            && !failback_required
            && evidence_window_ok
            && self.public_lane_amputation_ok
            && self.inventory_truth_ok
            && self.fee_leak_ok
            && self.selective_death_clamp_ok
            && self.pre_sign_egress_gate_ok
            && self.latency_ok
            && self.realized_pnl_ok
            && self.shadow_pnl_ok
            && competition_adjusted_promotion_ok
            && self.micro_market_uplift_ok
            && self.dominance_shadow_pnl_ok
            && self.taylor_gate_ok
            && afxdp_private_grade_actionability_ok
            && tactical_lane_ok
            && additive_quant_tactics_ok;

        let promote_allowed = all_gates_green && shadow_guard_promotion_ok;

        G12GateReport {
            rung: self.rung,
            sample_slots: self.sample_slots,
            observe_candidates: self.observe_candidates,
            observe_candidates_total: self.observe_candidates_total,
            observe_candidates_external_geyser: self.observe_candidates_external_geyser,
            observe_candidates_local_validator_geyser: self
                .observe_candidates_local_validator_geyser,
            observe_candidates_private_grade: self.observe_candidates_private_grade,
            live_landed: self.live_landed,
            worker_health_snapshot_missing: self.worker_health_snapshot_missing,
            worker_health_drops: self.worker_health_drops,
            worker_early_drop_mint_in_tag_missing: self.worker_early_drop_mint_in_tag_missing,
            worker_early_drop_mint_out_tag_missing: self.worker_early_drop_mint_out_tag_missing,
            worker_rollout_would_send_events_published: self
                .worker_rollout_would_send_events_published,
            worker_rollout_live_send_suppressed_events_published: self
                .worker_rollout_live_send_suppressed_events_published,
            worker_candidates_entered_process_intent: self.worker_candidates_entered_process_intent,
            worker_candidates_dropped_before_process_intent: self
                .worker_candidates_dropped_before_process_intent,
            worker_process_intent_ok: self.worker_process_intent_ok,
            worker_process_intent_err: self.worker_process_intent_err,
            last_process_intent_error: self.last_process_intent_error.clone(),
            last_process_intent_error_stage: self.last_process_intent_error_stage.clone(),
            last_process_intent_error_reason: self.last_process_intent_error_reason.clone(),
            worker_quote_ok: self.worker_quote_ok,
            worker_quote_err: self.worker_quote_err,
            worker_lookup_ok: self.worker_lookup_ok,
            worker_policy_ok: self.worker_policy_ok,
            worker_assembly_ready: self.worker_assembly_ready,
            worker_pm_reservation_created: self.worker_pm_reservation_created,
            worker_pipeline_err_routing_hydration: self.worker_pipeline_err_routing_hydration,
            worker_pipeline_err_blockhash_missing: self.worker_pipeline_err_blockhash_missing,
            worker_pipeline_err_lookup: self.worker_pipeline_err_lookup,
            worker_pipeline_err_quote: self.worker_pipeline_err_quote,
            worker_pipeline_err_quote_clmm_arena_missing: self
                .worker_pipeline_err_quote_clmm_arena_missing,
            worker_pipeline_err_quote_clmm_snapshot_missing: self
                .worker_pipeline_err_quote_clmm_snapshot_missing,
            worker_pipeline_err_quote_clmm_pool_mismatch: self
                .worker_pipeline_err_quote_clmm_pool_mismatch,
            worker_pipeline_err_quote_clmm_mint_in_mismatch: self
                .worker_pipeline_err_quote_clmm_mint_in_mismatch,
            worker_pipeline_err_quote_clmm_mint_out_mismatch: self
                .worker_pipeline_err_quote_clmm_mint_out_mismatch,
            worker_pipeline_err_quote_clmm_amount_mismatch: self
                .worker_pipeline_err_quote_clmm_amount_mismatch,
            worker_pipeline_err_quote_clmm_direction_mismatch: self
                .worker_pipeline_err_quote_clmm_direction_mismatch,
            worker_pipeline_err_quote_clmm_invalid_snapshot: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot,
            worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out,
            worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: self
                .worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price,
            worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: self
                .worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream,
            worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: self
                .worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary,
            worker_pipeline_err_quote_clmm_no_quote_out_of_range: self
                .worker_pipeline_err_quote_clmm_no_quote_out_of_range,
            worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: self
                .worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local,
            worker_pipeline_err_quote_clmm_stale_snapshot: self
                .worker_pipeline_err_quote_clmm_stale_snapshot,
            worker_pipeline_err_quote_clmm_target_out_underflow: self
                .worker_pipeline_err_quote_clmm_target_out_underflow,
            last_clmm_invalid_quote_pool: self.last_clmm_invalid_quote_pool.clone(),
            last_clmm_invalid_quote_mint_in: self.last_clmm_invalid_quote_mint_in.clone(),
            last_clmm_invalid_quote_mint_out: self.last_clmm_invalid_quote_mint_out.clone(),
            last_clmm_invalid_quote_amount_in: self.last_clmm_invalid_quote_amount_in,
            last_clmm_invalid_quote_expected_out: self.last_clmm_invalid_quote_expected_out,
            last_clmm_invalid_quote_min_out: self.last_clmm_invalid_quote_min_out,
            last_clmm_invalid_quote_projected_reserve_in: self
                .last_clmm_invalid_quote_projected_reserve_in,
            last_clmm_invalid_quote_projected_reserve_out: self
                .last_clmm_invalid_quote_projected_reserve_out,
            last_clmm_invalid_quote_quote_epoch: self.last_clmm_invalid_quote_quote_epoch,
            last_clmm_invalid_quote_last_update_unix_ns: self
                .last_clmm_invalid_quote_last_update_unix_ns,
            last_clmm_invalid_quote_post_sqrt_price_x64: self
                .last_clmm_invalid_quote_post_sqrt_price_x64
                .clone(),
            last_clmm_invalid_quote_post_tick: self.last_clmm_invalid_quote_post_tick,
            last_clmm_invalid_quote_crossed_tick_count: self
                .last_clmm_invalid_quote_crossed_tick_count,
            last_clmm_invalid_quote_direction: self.last_clmm_invalid_quote_direction,
            last_clmm_invalid_quote_validity_flags: self.last_clmm_invalid_quote_validity_flags,
            worker_pipeline_err_invalid_direction: self.worker_pipeline_err_invalid_direction,
            worker_pipeline_err_no_send_sink_missing: self.worker_pipeline_err_no_send_sink_missing,
            worker_pipeline_err_policy: self.worker_pipeline_err_policy,
            worker_pipeline_err_emit: self.worker_pipeline_err_emit,
            worker_route_resolved_jito: self.worker_route_resolved_jito,
            worker_route_resolved_public_rpc_scout: self.worker_route_resolved_public_rpc_scout,
            worker_route_resolved_harmonic: self.worker_route_resolved_harmonic,
            worker_route_resolved_drop: self.worker_route_resolved_drop,
            worker_public_rpc_scout_emit_attempt: self.worker_public_rpc_scout_emit_attempt,
            worker_public_rpc_scout_emit_ok: self.worker_public_rpc_scout_emit_ok,
            run38_target_public_scout_clmm_route_resolved: self
                .run38_target_public_scout_clmm_route_resolved,
            run38_target_public_scout_clmm_r1_attempt: self
                .run38_target_public_scout_clmm_r1_attempt,
            run38_target_public_scout_clmm_reject_proof_filter: self
                .run38_target_public_scout_clmm_reject_proof_filter,
            run38_target_public_scout_clmm_reject_micro_market: self
                .run38_target_public_scout_clmm_reject_micro_market,
            run38_target_public_scout_clmm_reject_inventory: self
                .run38_target_public_scout_clmm_reject_inventory,
            run38_target_public_scout_clmm_reject_inventory_missing_input_ata: self
                .run38_target_public_scout_clmm_reject_inventory_missing_input_ata,
            run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: self
                .run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized,
            run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: self
                .run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient,
            run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: self
                .run38_target_public_scout_clmm_reject_inventory_token_program_mismatch,
            run38_target_public_scout_clmm_reject_inventory_other: self
                .run38_target_public_scout_clmm_reject_inventory_other,
            run38_target_public_scout_clmm_reject_template_executability: self
                .run38_target_public_scout_clmm_reject_template_executability,
            run38_target_public_scout_clmm_reject_attempt_latch: self
                .run38_target_public_scout_clmm_reject_attempt_latch,
            run38_target_public_scout_clmm_admit: self.run38_target_public_scout_clmm_admit,
            run38_target_public_scout_clmm_process_intent: self
                .run38_target_public_scout_clmm_process_intent,
            run38_target_public_scout_clmm_emit_attempt: self
                .run38_target_public_scout_clmm_emit_attempt,
            run38_target_public_scout_clmm_transport_attempt: self
                .run38_target_public_scout_clmm_transport_attempt,
            run38_target_public_scout_clmm_pm_created: self
                .run38_target_public_scout_clmm_pm_created,
            public_scout_attempt_latch_set: self.public_scout_attempt_latch_set,
            public_scout_attempt_latch_drop_count: self.public_scout_attempt_latch_drop_count,
            public_scout_attempt_latch_first_signature: self
                .public_scout_attempt_latch_first_signature
                .clone(),
            pm_reserve_attempt: self.pm_reserve_attempt,
            pm_reserve_ok: self.pm_reserve_ok,
            pm_reserve_reject_no_live_mode: self.pm_reserve_reject_no_live_mode,
            pm_reserve_reject_no_send_mode: self.pm_reserve_reject_no_send_mode,
            pm_reserve_reject_route_not_live: self.pm_reserve_reject_route_not_live,
            pm_reserve_reject_cap: self.pm_reserve_reject_cap,
            pm_reserve_reject_inventory: self.pm_reserve_reject_inventory,
            pm_reserve_reject_missing_signature: self.pm_reserve_reject_missing_signature,
            pm_reserve_reject_other: self.pm_reserve_reject_other,
            last_pm_reserve_reject_reason: self.last_pm_reserve_reject_reason.clone(),
            last_pm_reserve_route: self.last_pm_reserve_route.clone(),
            last_pm_reserve_pool: self.last_pm_reserve_pool.clone(),
            last_pm_reserve_mint_in: self.last_pm_reserve_mint_in.clone(),
            last_pm_reserve_mint_out: self.last_pm_reserve_mint_out.clone(),
            last_pm_reserve_amount: self.last_pm_reserve_amount,
            clmm_input_ata_missing: self.clmm_input_ata_missing,
            clmm_output_ata_missing: self.clmm_output_ata_missing,
            clmm_input_ata_uninitialized: self.clmm_input_ata_uninitialized,
            clmm_output_ata_uninitialized: self.clmm_output_ata_uninitialized,
            clmm_input_balance_insufficient: self.clmm_input_balance_insufficient,
            clmm_token_program_mismatch: self.clmm_token_program_mismatch,
            last_clmm_input_mint: self.last_clmm_input_mint.clone(),
            last_clmm_output_mint: self.last_clmm_output_mint.clone(),
            last_clmm_input_token_account: self.last_clmm_input_token_account.clone(),
            last_clmm_output_token_account: self.last_clmm_output_token_account.clone(),
            template_drop_no_template: self.template_drop_no_template,
            template_drop_wrong_route_family: self.template_drop_wrong_route_family,
            template_drop_missing_clmm_accounts: self.template_drop_missing_clmm_accounts,
            template_drop_missing_input_ata: self.template_drop_missing_input_ata,
            template_drop_missing_output_ata: self.template_drop_missing_output_ata,
            template_drop_invalid_swap_program: self.template_drop_invalid_swap_program,
            template_drop_invalid_guard_template: self.template_drop_invalid_guard_template,
            template_drop_unsupported_token_program: self.template_drop_unsupported_token_program,
            template_drop_other: self.template_drop_other,
            last_template_drop_reason: self.last_template_drop_reason.clone(),
            last_template_drop_pool: self.last_template_drop_pool.clone(),
            last_template_drop_mint_in: self.last_template_drop_mint_in.clone(),
            last_template_drop_mint_out: self.last_template_drop_mint_out.clone(),
            last_template_drop_route: self.last_template_drop_route.clone(),
            last_template_drop_template_program: self.last_template_drop_template_program.clone(),
            last_template_drop_input_ata: self.last_template_drop_input_ata.clone(),
            last_template_drop_output_ata: self.last_template_drop_output_ata.clone(),
            public_rpc_scout_enqueued: self.public_rpc_scout_enqueued,
            public_rpc_transport_attempt: self.public_rpc_transport_attempt,
            public_rpc_transport_failure: self.public_rpc_transport_failure,
            public_rpc_transport_rejected: self.public_rpc_transport_rejected,
            public_rpc_nack_recorded: self.public_rpc_nack_recorded,
            last_public_rpc_rejection_code: self.last_public_rpc_rejection_code,
            last_public_rpc_rejection_message: self.last_public_rpc_rejection_message.clone(),
            last_public_rpc_rejection_body: self.last_public_rpc_rejection_body.clone(),
            last_public_rpc_rejection_data_err: self.last_public_rpc_rejection_data_err.clone(),
            last_public_rpc_rejection_simulation_logs: self
                .last_public_rpc_rejection_simulation_logs
                .clone(),
            last_public_rpc_rejection_signature: self.last_public_rpc_rejection_signature.clone(),
            last_public_rpc_rejection_tx_size: self.last_public_rpc_rejection_tx_size,
            last_public_rpc_rejection_blockhash: self.last_public_rpc_rejection_blockhash.clone(),
            last_public_rpc_rejection_blockhash_matches_current: self
                .last_public_rpc_rejection_blockhash_matches_current,
            last_public_rpc_rejection_blockhash_age_ns: self
                .last_public_rpc_rejection_blockhash_age_ns,
            last_public_rpc_rejection_cu_limit: self.last_public_rpc_rejection_cu_limit,
            last_public_rpc_rejection_cu_price_micro_lamports: self
                .last_public_rpc_rejection_cu_price_micro_lamports,
            last_public_rpc_rejection_reason: self.last_public_rpc_rejection_reason.clone(),
            last_public_rpc_rejection_ix1_program_id: self
                .last_public_rpc_rejection_ix1_program_id
                .clone(),
            last_public_rpc_rejection_ix1_account_0: self
                .last_public_rpc_rejection_ix1_account_0
                .clone(),
            last_public_rpc_rejection_ix1_account_1: self
                .last_public_rpc_rejection_ix1_account_1
                .clone(),
            last_public_rpc_rejection_ix1_account_count: self
                .last_public_rpc_rejection_ix1_account_count,
            last_public_rpc_rejection_ix2_program_id: self
                .last_public_rpc_rejection_ix2_program_id
                .clone(),
            last_public_rpc_rejection_ix2_amount_in: self.last_public_rpc_rejection_ix2_amount_in,
            last_public_rpc_rejection_ix2_min_out: self.last_public_rpc_rejection_ix2_min_out,
            last_public_rpc_rejection_ix2_account_count: self
                .last_public_rpc_rejection_ix2_account_count,
            last_public_rpc_rejection_actual_output_lamports: self
                .last_public_rpc_rejection_actual_output_lamports,
            last_public_rpc_rejection_required_output_lamports: self
                .last_public_rpc_rejection_required_output_lamports,
            pm_reservation_released: self.pm_reservation_released,
            pm_release_send_failed: self.pm_release_send_failed,
            pm_release_nacked: self.pm_release_nacked,
            worker_presign_gate_reject: self.worker_presign_gate_reject,
            worker_template_build_ok: self.worker_template_build_ok,
            worker_template_build_err: self.worker_template_build_err,
            worker_no_send_backend_called: self.worker_no_send_backend_called,
            worker_no_send_backend_ok: self.worker_no_send_backend_ok,
            worker_no_send_backend_err: self.worker_no_send_backend_err,
            tpu_quic_candidate_seen: self.tpu_quic_candidate_seen,
            tpu_quic_would_enqueue: self.tpu_quic_would_enqueue,
            tpu_quic_ring_enqueue_ok: self.tpu_quic_ring_enqueue_ok,
            tpu_quic_ring_enqueue_err: self.tpu_quic_ring_enqueue_err,
            tpu_quic_pm_reserve_simulated: self.tpu_quic_pm_reserve_simulated,
            tpu_quic_last_signature: self.tpu_quic_last_signature.clone(),
            tpu_quic_last_route_family: self.tpu_quic_last_route_family.clone(),
            scorecard_replay_stale_slot_bypass_seen: self.scorecard_replay_stale_slot_bypass_seen,
            scorecard_replay_stale_slot_bypass_applied: self
                .scorecard_replay_stale_slot_bypass_applied,
            scorecard_replay_stale_slot_bypass_signature_match: self
                .scorecard_replay_stale_slot_bypass_signature_match,
            scorecard_replay_stale_slot_bypass_pool_match: self
                .scorecard_replay_stale_slot_bypass_pool_match,
            scorecard_replay_original_slot: self.scorecard_replay_original_slot,
            scorecard_replay_current_slot: self.scorecard_replay_current_slot,
            scorecard_replay_slot_gap: self.scorecard_replay_slot_gap,
            scorecard_replay_route_override_seen: self.scorecard_replay_route_override_seen,
            scorecard_replay_route_override_applied: self.scorecard_replay_route_override_applied,
            scorecard_replay_route_override_signature_match: self
                .scorecard_replay_route_override_signature_match,
            scorecard_replay_route_override_pool_match: self
                .scorecard_replay_route_override_pool_match,
            scorecard_replay_route_override_from_public_target: self
                .scorecard_replay_route_override_from_public_target,
            scorecard_replay_route_override_to_tpu_quic_public: self
                .scorecard_replay_route_override_to_tpu_quic_public,
            scorecard_replay_source_override_seen: self.scorecard_replay_source_override_seen,
            scorecard_replay_source_override_applied: self.scorecard_replay_source_override_applied,
            scorecard_replay_source_override_signature_match: self
                .scorecard_replay_source_override_signature_match,
            scorecard_replay_source_override_pool_match: self
                .scorecard_replay_source_override_pool_match,
            scorecard_replay_source_override_live_send_blocked: self
                .scorecard_replay_source_override_live_send_blocked,
            worker_final_no_send_event_constructed: self.worker_final_no_send_event_constructed,
            worker_final_no_send_event_published: self.worker_final_no_send_event_published,
            worker_final_no_send_event_publish_err: self.worker_final_no_send_event_publish_err,
            worker_final_no_send_drop_reason_last: self.worker_final_no_send_drop_reason_last,
            worker_geyser_shadow_no_send_admitted: self.worker_geyser_shadow_no_send_admitted,
            worker_geyser_shadow_no_send_rejected: self.worker_geyser_shadow_no_send_rejected,
            worker_geyser_shadow_no_send_source_external: self
                .worker_geyser_shadow_no_send_source_external,
            worker_geyser_shadow_no_send_source_local_validator: self
                .worker_geyser_shadow_no_send_source_local_validator,
            r1_auto_admission_attempt: self.r1_auto_admission_attempt,
            r1_auto_admission_admit: self.r1_auto_admission_admit,
            r1_auto_admission_admit_to_process_intent_attempt: self
                .r1_auto_admission_admit_to_process_intent_attempt,
            r1_auto_admission_admit_to_process_intent_ok: self
                .r1_auto_admission_admit_to_process_intent_ok,
            r1_auto_admission_admit_to_process_intent_drop: self
                .r1_auto_admission_admit_to_process_intent_drop,
            clmm_exact_quote_request_publish_attempt: self.clmm_exact_quote_request_publish_attempt,
            clmm_exact_quote_request_publish_ok: self.clmm_exact_quote_request_publish_ok,
            clmm_exact_quote_request_publish_full: self.clmm_exact_quote_request_publish_full,
            clmm_exact_quote_request_overwrite_same_pool: self
                .clmm_exact_quote_request_overwrite_same_pool,
            clmm_exact_quote_request_suppressed_outstanding_same_pool: self
                .clmm_exact_quote_request_suppressed_outstanding_same_pool,
            clmm_exact_quote_request_outstanding_same_pool_hit: self
                .clmm_exact_quote_request_outstanding_same_pool_hit,
            clmm_exact_quote_request_outstanding_cleared_matched: self
                .clmm_exact_quote_request_outstanding_cleared_matched,
            clmm_exact_quote_request_outstanding_cleared_timeout: self
                .clmm_exact_quote_request_outstanding_cleared_timeout,
            clmm_exact_quote_request_outstanding_cleared_no_quote: self
                .clmm_exact_quote_request_outstanding_cleared_no_quote,
            clmm_exact_quote_response_matched: self.clmm_exact_quote_response_matched,
            clmm_exact_quote_response_timeout: self.clmm_exact_quote_response_timeout,
            clmm_exact_quote_response_no_snapshot: self.clmm_exact_quote_response_no_snapshot,
            clmm_exact_quote_response_stale: self.clmm_exact_quote_response_stale,
            clmm_exact_quote_response_pool_mismatch: self.clmm_exact_quote_response_pool_mismatch,
            clmm_exact_quote_response_mint_mismatch: self.clmm_exact_quote_response_mint_mismatch,
            clmm_exact_quote_response_amount_mismatch: self
                .clmm_exact_quote_response_amount_mismatch,
            clmm_exact_quote_response_request_id_mismatch: self
                .clmm_exact_quote_response_request_id_mismatch,
            clmm_exact_quote_response_invalid_flags: self.clmm_exact_quote_response_invalid_flags,
            clmm_exact_quote_response_invalid_flags_zero: self
                .clmm_exact_quote_response_invalid_flags_zero,
            clmm_exact_quote_response_invalid_flags_no_valid_bit: self
                .clmm_exact_quote_response_invalid_flags_no_valid_bit,
            clmm_exact_quote_response_invalid_flags_no_quote_bit: self
                .clmm_exact_quote_response_invalid_flags_no_quote_bit,
            clmm_exact_quote_response_invalid_flags_unknown_status: self
                .clmm_exact_quote_response_invalid_flags_unknown_status,
            clmm_exact_quote_response_invalid_valid_quote_shape: self
                .clmm_exact_quote_response_invalid_valid_quote_shape,
            clmm_exact_quote_response_no_quote_no_current_array_upstream: self
                .clmm_exact_quote_response_no_quote_no_current_array_upstream,
            clmm_exact_quote_response_no_quote_no_liquidity_boundary: self
                .clmm_exact_quote_response_no_quote_no_liquidity_boundary,
            clmm_exact_quote_response_no_quote_out_of_range: self
                .clmm_exact_quote_response_no_quote_out_of_range,
            clmm_exact_quote_response_no_quote_missing_tick_array_local: self
                .clmm_exact_quote_response_no_quote_missing_tick_array_local,
            clmm_exact_quote_response_timeout_terminal_no_quote: self
                .clmm_exact_quote_response_timeout_terminal_no_quote,
            clmm_exact_quote_response_timeout_quote_ready_rejected: self
                .clmm_exact_quote_response_timeout_quote_ready_rejected,
            clmm_exact_quote_response_timeout_stale_request_id: self
                .clmm_exact_quote_response_timeout_stale_request_id,
            clmm_exact_quote_response_timeout_quote_build_failed: self
                .clmm_exact_quote_response_timeout_quote_build_failed,
            clmm_exact_quote_response_timeout_dependency_missing: self
                .clmm_exact_quote_response_timeout_dependency_missing,
            r1_auto_admission_drop_source: self.r1_auto_admission_drop_source,
            r1_auto_admission_drop_non_private_grade: self.r1_auto_admission_drop_non_private_grade,
            r1_auto_admission_drop_shadowguard: self.r1_auto_admission_drop_shadowguard,
            r1_auto_admission_drop_micro_market: self.r1_auto_admission_drop_micro_market,
            r1_auto_admission_drop_egress: self.r1_auto_admission_drop_egress,
            r1_auto_admission_drop_inventory: self.r1_auto_admission_drop_inventory,
            r1_auto_admission_drop_cap: self.r1_auto_admission_drop_cap,
            r1_auto_admission_drop_stale: self.r1_auto_admission_drop_stale,
            r1_auto_admission_drop_public_route: self.r1_auto_admission_drop_public_route,
            r1_auto_admission_reject_source: self.r1_auto_admission_reject_source,
            r1_auto_admission_reject_non_private_grade: self
                .r1_auto_admission_reject_non_private_grade,
            r1_auto_admission_reject_shadowguard: self.r1_auto_admission_reject_shadowguard,
            r1_auto_admission_reject_micro_market: self.r1_auto_admission_reject_micro_market,
            r1_auto_admission_reject_egress: self.r1_auto_admission_reject_egress,
            r1_auto_admission_reject_inventory: self.r1_auto_admission_reject_inventory,
            r1_auto_admission_reject_cap: self.r1_auto_admission_reject_cap,
            r1_auto_admission_reject_stale: self.r1_auto_admission_reject_stale,
            r1_auto_admission_reject_public_route: self.r1_auto_admission_reject_public_route,
            r1_auto_admission_reject_proof_filter: self.r1_auto_admission_reject_proof_filter,
            r1_auto_admission_reject_template_executability: self
                .r1_auto_admission_reject_template_executability,
            r1_auto_admission_reject_attempt_latch: self.r1_auto_admission_reject_attempt_latch,
            r1_auto_admission_reject_proof_filter_no_quote: self
                .r1_auto_admission_reject_proof_filter_no_quote,
            r1_auto_admission_reject_proof_filter_no_positive_ev: self
                .r1_auto_admission_reject_proof_filter_no_positive_ev,
            r1_auto_admission_reject_proof_filter_no_shadow_guard: self
                .r1_auto_admission_reject_proof_filter_no_shadow_guard,
            r1_auto_admission_reject_proof_filter_no_micro_market: self
                .r1_auto_admission_reject_proof_filter_no_micro_market,
            r1_auto_admission_reject_proof_filter_public_scout_shape: self
                .r1_auto_admission_reject_proof_filter_public_scout_shape,
            r1_auto_admission_reject_proof_filter_attempt_latch: self
                .r1_auto_admission_reject_proof_filter_attempt_latch,
            r1_auto_admission_reject_proof_filter_other: self
                .r1_auto_admission_reject_proof_filter_other,
            r1_auto_admission_reject_public_scout_shape_route_not_public_scout: self
                .r1_auto_admission_reject_public_scout_shape_route_not_public_scout,
            r1_auto_admission_reject_public_scout_shape_wrong_source: self
                .r1_auto_admission_reject_public_scout_shape_wrong_source,
            r1_auto_admission_reject_public_scout_shape_wrong_venue: self
                .r1_auto_admission_reject_public_scout_shape_wrong_venue,
            r1_auto_admission_reject_public_scout_shape_missing_template_family: self
                .r1_auto_admission_reject_public_scout_shape_missing_template_family,
            r1_auto_admission_reject_public_scout_shape_route_template_mismatch: self
                .r1_auto_admission_reject_public_scout_shape_route_template_mismatch,
            r1_auto_admission_reject_public_scout_shape_other: self
                .r1_auto_admission_reject_public_scout_shape_other,
            r1_auto_admission_reject_micro_market_no_uplift: self
                .r1_auto_admission_reject_micro_market_no_uplift,
            r1_auto_admission_reject_micro_market_negative_ev: self
                .r1_auto_admission_reject_micro_market_negative_ev,
            r1_auto_admission_reject_micro_market_below_floor: self
                .r1_auto_admission_reject_micro_market_below_floor,
            r1_auto_admission_reject_micro_market_other: self
                .r1_auto_admission_reject_micro_market_other,
            last_public_scout_shape_reject_reason: self
                .last_public_scout_shape_reject_reason
                .clone(),
            last_public_scout_shape_reject_pool: self.last_public_scout_shape_reject_pool.clone(),
            last_public_scout_shape_reject_mint_in: self
                .last_public_scout_shape_reject_mint_in
                .clone(),
            last_public_scout_shape_reject_mint_out: self
                .last_public_scout_shape_reject_mint_out
                .clone(),
            last_public_scout_shape_reject_route: self.last_public_scout_shape_reject_route.clone(),
            last_public_scout_shape_reject_source: self
                .last_public_scout_shape_reject_source
                .clone(),
            last_public_scout_shape_reject_venue: self.last_public_scout_shape_reject_venue.clone(),
            last_run38_target_pool: self.last_run38_target_pool.clone(),
            last_run38_target_mint_in: self.last_run38_target_mint_in.clone(),
            last_run38_target_mint_out: self.last_run38_target_mint_out.clone(),
            last_run38_target_route: self.last_run38_target_route.clone(),
            last_run38_target_source: self.last_run38_target_source.clone(),
            last_run38_target_venue: self.last_run38_target_venue.clone(),
            last_run38_target_stage: self.last_run38_target_stage.clone(),
            last_run38_target_reject_reason: self.last_run38_target_reject_reason.clone(),
            last_run38_target_inventory_reject_reason: self
                .last_run38_target_inventory_reject_reason
                .clone(),
            last_run38_target_inventory_pool: self.last_run38_target_inventory_pool.clone(),
            last_run38_target_inventory_mint_in: self.last_run38_target_inventory_mint_in.clone(),
            last_run38_target_inventory_mint_out: self.last_run38_target_inventory_mint_out.clone(),
            last_run38_target_inventory_input_ata: self
                .last_run38_target_inventory_input_ata
                .clone(),
            last_run38_target_inventory_input_balance: self
                .last_run38_target_inventory_input_balance,
            last_run38_target_inventory_required_input: self
                .last_run38_target_inventory_required_input,
            last_run38_target_inventory_token_program: self
                .last_run38_target_inventory_token_program
                .clone(),
            last_r1_auto_admission_reject_reason: self.last_r1_auto_admission_reject_reason.clone(),
            last_r1_auto_admission_reject_stage: self.last_r1_auto_admission_reject_stage.clone(),
            last_r1_auto_admission_pool: self.last_r1_auto_admission_pool.clone(),
            last_r1_auto_admission_mint_in: self.last_r1_auto_admission_mint_in.clone(),
            last_r1_auto_admission_mint_out: self.last_r1_auto_admission_mint_out.clone(),
            last_r1_template_executability_reject_reason: self
                .last_r1_template_executability_reject_reason
                .clone(),
            last_r1_template_executability_pool: self.last_r1_template_executability_pool.clone(),
            last_r1_template_executability_mint_in: self
                .last_r1_template_executability_mint_in
                .clone(),
            last_r1_template_executability_mint_out: self
                .last_r1_template_executability_mint_out
                .clone(),
            last_r1_template_executability_route: self.last_r1_template_executability_route.clone(),
            last_r1_template_executability_output_ata: self
                .last_r1_template_executability_output_ata
                .clone(),
            last_admit_to_process_intent_drop_reason: self
                .last_admit_to_process_intent_drop_reason
                .clone(),
            last_admit_to_process_intent_pool: self.last_admit_to_process_intent_pool.clone(),
            last_admit_to_process_intent_mint_in: self.last_admit_to_process_intent_mint_in.clone(),
            last_admit_to_process_intent_mint_out: self
                .last_admit_to_process_intent_mint_out
                .clone(),
            last_admit_to_process_intent_route: self.last_admit_to_process_intent_route.clone(),
            last_admit_to_process_intent_source: self.last_admit_to_process_intent_source.clone(),
            last_clmm_exact_quote_response_mismatch_reason: self
                .last_clmm_exact_quote_response_mismatch_reason
                .clone(),
            last_clmm_exact_quote_response_pool_index: self
                .last_clmm_exact_quote_response_pool_index,
            last_clmm_exact_quote_response_sequence: self.last_clmm_exact_quote_response_sequence,
            last_clmm_exact_quote_response_request_id_expected: self
                .last_clmm_exact_quote_response_request_id_expected,
            last_clmm_exact_quote_response_request_id_found: self
                .last_clmm_exact_quote_response_request_id_found,
            last_clmm_exact_quote_response_pool_expected: self
                .last_clmm_exact_quote_response_pool_expected
                .clone(),
            last_clmm_exact_quote_response_pool_found: self
                .last_clmm_exact_quote_response_pool_found
                .clone(),
            last_clmm_exact_quote_response_mint_in_expected: self
                .last_clmm_exact_quote_response_mint_in_expected
                .clone(),
            last_clmm_exact_quote_response_mint_in_found: self
                .last_clmm_exact_quote_response_mint_in_found
                .clone(),
            last_clmm_exact_quote_response_mint_out_expected: self
                .last_clmm_exact_quote_response_mint_out_expected
                .clone(),
            last_clmm_exact_quote_response_mint_out_found: self
                .last_clmm_exact_quote_response_mint_out_found
                .clone(),
            last_clmm_exact_quote_response_amount_expected: self
                .last_clmm_exact_quote_response_amount_expected,
            last_clmm_exact_quote_response_amount_found: self
                .last_clmm_exact_quote_response_amount_found,
            last_clmm_exact_quote_response_validity_flags: self
                .last_clmm_exact_quote_response_validity_flags,
            last_clmm_exact_quote_response_status: self
                .last_clmm_exact_quote_response_status
                .clone(),
            last_clmm_exact_quote_response_status_code: self
                .last_clmm_exact_quote_response_status_code,
            last_clmm_exact_quote_response_timeout_classification: self
                .last_clmm_exact_quote_response_timeout_classification
                .clone(),
            last_clmm_exact_quote_response_quote_epoch: self
                .last_clmm_exact_quote_response_quote_epoch,
            last_clmm_exact_quote_response_last_update_unix_ns: self
                .last_clmm_exact_quote_response_last_update_unix_ns,
            r1_afxdp_private_grade_route_actionable: self.r1_afxdp_private_grade_route_actionable,
            r1_afxdp_private_grade_process_intent_entered: self
                .r1_afxdp_private_grade_process_intent_entered,
            r1_afxdp_private_grade_auto_admission_attempt: self
                .r1_afxdp_private_grade_auto_admission_attempt,
            r1_afxdp_private_grade_auto_admission_admit: self
                .r1_afxdp_private_grade_auto_admission_admit,
            worker_signal_external_geyser: self.worker_signal_external_geyser,
            worker_signal_local_validator_geyser: self.worker_signal_local_validator_geyser,
            worker_signal_afxdp: self.worker_signal_afxdp,
            worker_signal_dpdk: self.worker_signal_dpdk,
            worker_signal_unknown: self.worker_signal_unknown,
            dpdk_worker_records_seen: self.dpdk_worker_records_seen,
            dpdk_worker_health_drop: self.dpdk_worker_health_drop,
            dpdk_worker_stale_slot_drop: self.dpdk_worker_stale_slot_drop,
            dpdk_worker_pool_state_miss: self.dpdk_worker_pool_state_miss,
            dpdk_worker_pool_state_hit: self.dpdk_worker_pool_state_hit,
            dpdk_worker_route_actionable: self.dpdk_worker_route_actionable,
            dpdk_worker_route_dropped: self.dpdk_worker_route_dropped,
            dpdk_worker_route_unavailable: self.dpdk_worker_route_unavailable,
            dpdk_worker_route_unavailable_unknown_signal: self
                .dpdk_worker_route_unavailable_unknown_signal,
            dpdk_worker_route_unavailable_hydration_none: self
                .dpdk_worker_route_unavailable_hydration_none,
            dpdk_worker_r1_admission_drop: self.dpdk_worker_r1_admission_drop,
            dpdk_worker_candidates_entered_process_intent: self
                .dpdk_worker_candidates_entered_process_intent,
            dpdk_worker_pipeline_error: self.dpdk_worker_pipeline_error,
            dpdk_worker_rollout_shadow_published: self.dpdk_worker_rollout_shadow_published,
            dpdk_worker_final_no_send_published: self.dpdk_worker_final_no_send_published,
            worker_route_unavailable_hydration_none_afxdp: self
                .worker_route_unavailable_hydration_none_afxdp,
            worker_afxdp_hydration_request_enqueued: self.worker_afxdp_hydration_request_enqueued,
            worker_afxdp_hydration_request_deduped: self.worker_afxdp_hydration_request_deduped,
            worker_source_probe_afxdp_parsed_shred_drops: self
                .worker_source_probe_afxdp_parsed_shred_drops,
            afxdp_frames_seen: self.afxdp_frames_seen,
            afxdp_udp_8000_8025_seen: self.afxdp_udp_8000_8025_seen,
            afxdp_xsk_rx_batches: self.afxdp_xsk_rx_batches,
            afxdp_shred_parse_ok: self.afxdp_shred_parse_ok,
            afxdp_shred_parse_err: self.afxdp_shred_parse_err,
            afxdp_shred_parse_data_ok: self.afxdp_shred_parse_data_ok,
            afxdp_shred_parse_coding_ok: self.afxdp_shred_parse_coding_ok,
            afxdp_feed_enqueue_ok: self.afxdp_feed_enqueue_ok,
            afxdp_feed_enqueue_full: self.afxdp_feed_enqueue_full,
            afxdp_reassembly_insert_ok: self.afxdp_reassembly_insert_ok,
            afxdp_reassembly_complete_entries: self.afxdp_reassembly_complete_entries,
            afxdp_fec_recovered_entries: self.afxdp_fec_recovered_entries,
            afxdp_data_shred_payload_ok: self.afxdp_data_shred_payload_ok,
            afxdp_tx_extract_attempt: self.afxdp_tx_extract_attempt,
            afxdp_tx_extract_ok: self.afxdp_tx_extract_ok,
            afxdp_tx_extract_err: self.afxdp_tx_extract_err,
            afxdp_tx_extract_err_empty_payload: self.afxdp_tx_extract_err_empty_payload,
            afxdp_tx_extract_err_short_vec: self.afxdp_tx_extract_err_short_vec,
            afxdp_tx_extract_err_bad_entry_header: self.afxdp_tx_extract_err_bad_entry_header,
            afxdp_tx_extract_err_bad_compact_u16: self.afxdp_tx_extract_err_bad_compact_u16,
            afxdp_tx_extract_err_sanitize_failed: self.afxdp_tx_extract_err_sanitize_failed,
            afxdp_tx_extract_err_no_transactions: self.afxdp_tx_extract_err_no_transactions,
            afxdp_tx_extract_err_unsupported_format: self.afxdp_tx_extract_err_unsupported_format,
            afxdp_tx_record_emit_attempt: self.afxdp_tx_record_emit_attempt,
            afxdp_tx_record_emit_ok: self.afxdp_tx_record_emit_ok,
            afxdp_tx_record_emit_full: self.afxdp_tx_record_emit_full,
            afxdp_pipeline_drop_unsupported_program: self.afxdp_pipeline_drop_unsupported_program,
            afxdp_pipeline_drop_allowlist_miss: self.afxdp_pipeline_drop_allowlist_miss,
            afxdp_pipeline_drop_decode_fail: self.afxdp_pipeline_drop_decode_fail,
            afxdp_pipeline_drop_route_fail: self.afxdp_pipeline_drop_route_fail,
            afxdp_pipeline_drop_mint_fail: self.afxdp_pipeline_drop_mint_fail,
            afxdp_pipeline_drop_other: self.afxdp_pipeline_drop_other,
            afxdp_unsupported_program_top_0_id: self.afxdp_unsupported_program_top_0_id.clone(),
            afxdp_unsupported_program_top_0_count: self.afxdp_unsupported_program_top_0_count,
            afxdp_unsupported_program_top_1_id: self.afxdp_unsupported_program_top_1_id.clone(),
            afxdp_unsupported_program_top_1_count: self.afxdp_unsupported_program_top_1_count,
            afxdp_unsupported_program_top_2_id: self.afxdp_unsupported_program_top_2_id.clone(),
            afxdp_unsupported_program_top_2_count: self.afxdp_unsupported_program_top_2_count,
            afxdp_unsupported_program_top_3_id: self.afxdp_unsupported_program_top_3_id.clone(),
            afxdp_unsupported_program_top_3_count: self.afxdp_unsupported_program_top_3_count,
            afxdp_unsupported_program_top_4_id: self.afxdp_unsupported_program_top_4_id.clone(),
            afxdp_unsupported_program_top_4_count: self.afxdp_unsupported_program_top_4_count,
            afxdp_unsupported_program_top_5_id: self.afxdp_unsupported_program_top_5_id.clone(),
            afxdp_unsupported_program_top_5_count: self.afxdp_unsupported_program_top_5_count,
            afxdp_unsupported_program_top_6_id: self.afxdp_unsupported_program_top_6_id.clone(),
            afxdp_unsupported_program_top_6_count: self.afxdp_unsupported_program_top_6_count,
            afxdp_unsupported_program_top_7_id: self.afxdp_unsupported_program_top_7_id.clone(),
            afxdp_unsupported_program_top_7_count: self.afxdp_unsupported_program_top_7_count,
            afxdp_unsupported_program_top_8_id: self.afxdp_unsupported_program_top_8_id.clone(),
            afxdp_unsupported_program_top_8_count: self.afxdp_unsupported_program_top_8_count,
            afxdp_unsupported_program_top_9_id: self.afxdp_unsupported_program_top_9_id.clone(),
            afxdp_unsupported_program_top_9_count: self.afxdp_unsupported_program_top_9_count,
            afxdp_unsupported_program_top_10_id: self.afxdp_unsupported_program_top_10_id.clone(),
            afxdp_unsupported_program_top_10_count: self.afxdp_unsupported_program_top_10_count,
            afxdp_unsupported_program_top_11_id: self.afxdp_unsupported_program_top_11_id.clone(),
            afxdp_unsupported_program_top_11_count: self.afxdp_unsupported_program_top_11_count,
            afxdp_unsupported_program_top_12_id: self.afxdp_unsupported_program_top_12_id.clone(),
            afxdp_unsupported_program_top_12_count: self.afxdp_unsupported_program_top_12_count,
            afxdp_unsupported_program_top_13_id: self.afxdp_unsupported_program_top_13_id.clone(),
            afxdp_unsupported_program_top_13_count: self.afxdp_unsupported_program_top_13_count,
            afxdp_unsupported_program_top_14_id: self.afxdp_unsupported_program_top_14_id.clone(),
            afxdp_unsupported_program_top_14_count: self.afxdp_unsupported_program_top_14_count,
            afxdp_unsupported_program_top_15_id: self.afxdp_unsupported_program_top_15_id.clone(),
            afxdp_unsupported_program_top_15_count: self.afxdp_unsupported_program_top_15_count,
            afxdp_source_probe_emit_attempt: self.afxdp_source_probe_emit_attempt,
            afxdp_source_probe_emit_ok: self.afxdp_source_probe_emit_ok,
            afxdp_source_probe_emit_full: self.afxdp_source_probe_emit_full,
            afxdp_source_probe_emit_err: self.afxdp_source_probe_emit_err,
            grpc_connected: self.grpc_connected,
            grpc_subscribe_sent: self.grpc_subscribe_sent,
            grpc_updates_total: self.grpc_updates_total,
            grpc_updates_transaction: self.grpc_updates_transaction,
            grpc_updates_non_transaction: self.grpc_updates_non_transaction,
            grpc_normalize_ok: self.grpc_normalize_ok,
            grpc_normalize_err: self.grpc_normalize_err,
            grpc_pipeline_drop_unsupported_program: self.grpc_pipeline_drop_unsupported_program,
            grpc_pipeline_drop_allowlist_miss: self.grpc_pipeline_drop_allowlist_miss,
            grpc_pipeline_drop_decode_fail: self.grpc_pipeline_drop_decode_fail,
            grpc_pipeline_drop_route_fail: self.grpc_pipeline_drop_route_fail,
            grpc_pipeline_drop_mint_fail: self.grpc_pipeline_drop_mint_fail,
            grpc_pipeline_drop_other: self.grpc_pipeline_drop_other,
            grpc_unsupported_program_top_0_id: self.grpc_unsupported_program_top_0_id.clone(),
            grpc_unsupported_program_top_0_count: self.grpc_unsupported_program_top_0_count,
            grpc_unsupported_program_top_1_id: self.grpc_unsupported_program_top_1_id.clone(),
            grpc_unsupported_program_top_1_count: self.grpc_unsupported_program_top_1_count,
            grpc_unsupported_program_top_2_id: self.grpc_unsupported_program_top_2_id.clone(),
            grpc_unsupported_program_top_2_count: self.grpc_unsupported_program_top_2_count,
            grpc_unsupported_program_top_3_id: self.grpc_unsupported_program_top_3_id.clone(),
            grpc_unsupported_program_top_3_count: self.grpc_unsupported_program_top_3_count,
            grpc_unsupported_program_top_4_id: self.grpc_unsupported_program_top_4_id.clone(),
            grpc_unsupported_program_top_4_count: self.grpc_unsupported_program_top_4_count,
            grpc_unsupported_program_top_5_id: self.grpc_unsupported_program_top_5_id.clone(),
            grpc_unsupported_program_top_5_count: self.grpc_unsupported_program_top_5_count,
            grpc_unsupported_program_top_6_id: self.grpc_unsupported_program_top_6_id.clone(),
            grpc_unsupported_program_top_6_count: self.grpc_unsupported_program_top_6_count,
            grpc_unsupported_program_top_7_id: self.grpc_unsupported_program_top_7_id.clone(),
            grpc_unsupported_program_top_7_count: self.grpc_unsupported_program_top_7_count,
            grpc_unsupported_program_top_8_id: self.grpc_unsupported_program_top_8_id.clone(),
            grpc_unsupported_program_top_8_count: self.grpc_unsupported_program_top_8_count,
            grpc_unsupported_program_top_9_id: self.grpc_unsupported_program_top_9_id.clone(),
            grpc_unsupported_program_top_9_count: self.grpc_unsupported_program_top_9_count,
            grpc_unsupported_program_top_10_id: self.grpc_unsupported_program_top_10_id.clone(),
            grpc_unsupported_program_top_10_count: self.grpc_unsupported_program_top_10_count,
            grpc_unsupported_program_top_11_id: self.grpc_unsupported_program_top_11_id.clone(),
            grpc_unsupported_program_top_11_count: self.grpc_unsupported_program_top_11_count,
            grpc_unsupported_program_top_12_id: self.grpc_unsupported_program_top_12_id.clone(),
            grpc_unsupported_program_top_12_count: self.grpc_unsupported_program_top_12_count,
            grpc_unsupported_program_top_13_id: self.grpc_unsupported_program_top_13_id.clone(),
            grpc_unsupported_program_top_13_count: self.grpc_unsupported_program_top_13_count,
            grpc_unsupported_program_top_14_id: self.grpc_unsupported_program_top_14_id.clone(),
            grpc_unsupported_program_top_14_count: self.grpc_unsupported_program_top_14_count,
            grpc_unsupported_program_top_15_id: self.grpc_unsupported_program_top_15_id.clone(),
            grpc_unsupported_program_top_15_count: self.grpc_unsupported_program_top_15_count,
            grpc_feed_enqueue_ok: self.grpc_feed_enqueue_ok,
            grpc_feed_enqueue_full: self.grpc_feed_enqueue_full,
            grpc_feed_enqueue_err: self.grpc_feed_enqueue_err,
            grpc_signal_source_local_validator_geyser: self
                .grpc_signal_source_local_validator_geyser,
            public_lane_amputation_ok: self.public_lane_amputation_ok,
            inventory_truth_ok: self.inventory_truth_ok,
            fee_leak_ok: self.fee_leak_ok,
            fee_leak_accounted: self.fee_leak_accounted,
            phantom_inventory_risk: self.phantom_inventory_risk,
            selective_death_clamp_ok: self.selective_death_clamp_ok,
            pre_sign_egress_gate_ok: self.pre_sign_egress_gate_ok,
            private_lane_admissibility_ok: self.private_lane_admissibility_ok,
            rollout_events_observed: self.rollout_events_observed,
            rollout_would_send: self.rollout_would_send,
            rollout_live_send_suppressed: self.rollout_live_send_suppressed,
            rollout_private_lane_admissible_opportunities: self
                .rollout_private_lane_admissible_opportunities,
            rollout_private_grade_admissible_opportunities: self
                .rollout_private_grade_admissible_opportunities,
            rollout_geyser_shadow_opportunities: self.rollout_geyser_shadow_opportunities,
            rollout_afxdp_private_grade_would_send: self.rollout_afxdp_private_grade_would_send,
            rollout_afxdp_private_grade_live_send_suppressed: self
                .rollout_afxdp_private_grade_live_send_suppressed,
            rollout_jito_no_send_would_sends: self.rollout_jito_no_send_would_sends,
            rollout_harmonic_no_send_would_sends: self.rollout_harmonic_no_send_would_sends,
            rollout_public_rpc_no_send_would_sends: self.rollout_public_rpc_no_send_would_sends,
            rollout_last_no_send_reason: self.rollout_last_no_send_reason,
            rollout_last_drop_reason: self.rollout_last_drop_reason,
            latency_ok: self.latency_ok,
            worker_latency_proof_present: self.worker_latency_proof_present,
            worker_latency_p99_ns: self.worker_latency_p99_ns,
            worker_latency_budget_ns: self.worker_latency_budget_ns,
            dpdk_producer_to_feed_count: self.dpdk_producer_to_feed_count,
            dpdk_producer_to_feed_p50_ns: self.dpdk_producer_to_feed_p50_ns,
            dpdk_producer_to_feed_p90_ns: self.dpdk_producer_to_feed_p90_ns,
            dpdk_producer_to_feed_p99_ns: self.dpdk_producer_to_feed_p99_ns,
            dpdk_producer_to_feed_p999_ns: self.dpdk_producer_to_feed_p999_ns,
            dpdk_producer_to_feed_max_ns: self.dpdk_producer_to_feed_max_ns,
            dpdk_feed_to_worker_count: self.dpdk_feed_to_worker_count,
            dpdk_feed_to_worker_p50_ns: self.dpdk_feed_to_worker_p50_ns,
            dpdk_feed_to_worker_p90_ns: self.dpdk_feed_to_worker_p90_ns,
            dpdk_feed_to_worker_p99_ns: self.dpdk_feed_to_worker_p99_ns,
            dpdk_feed_to_worker_p999_ns: self.dpdk_feed_to_worker_p999_ns,
            dpdk_feed_to_worker_max_ns: self.dpdk_feed_to_worker_max_ns,
            dpdk_worker_to_nosend_count: self.dpdk_worker_to_nosend_count,
            dpdk_worker_to_nosend_p50_ns: self.dpdk_worker_to_nosend_p50_ns,
            dpdk_worker_to_nosend_p90_ns: self.dpdk_worker_to_nosend_p90_ns,
            dpdk_worker_to_nosend_p99_ns: self.dpdk_worker_to_nosend_p99_ns,
            dpdk_worker_to_nosend_p999_ns: self.dpdk_worker_to_nosend_p999_ns,
            dpdk_worker_to_nosend_max_ns: self.dpdk_worker_to_nosend_max_ns,
            dpdk_end_to_end_observe_count: self.dpdk_end_to_end_observe_count,
            dpdk_end_to_end_observe_p50_ns: self.dpdk_end_to_end_observe_p50_ns,
            dpdk_end_to_end_observe_p90_ns: self.dpdk_end_to_end_observe_p90_ns,
            dpdk_end_to_end_observe_p99_ns: self.dpdk_end_to_end_observe_p99_ns,
            dpdk_end_to_end_observe_p999_ns: self.dpdk_end_to_end_observe_p999_ns,
            dpdk_end_to_end_observe_max_ns: self.dpdk_end_to_end_observe_max_ns,
            realized_pnl_ok: self.realized_pnl_ok,
            shadow_pnl_ok: self.shadow_pnl_ok,
            competition_adjusted_ok: competition_adjusted_promotion_ok,
            competition_adjusted_retained_ev_delta_lamports: self
                .competition_adjusted_retained_ev_delta_lamports,
            competition_adjusted_fee_burn_delta_lamports: self
                .competition_adjusted_fee_burn_delta_lamports,
            micro_market_uplift_ok: self.micro_market_uplift_ok,
            micro_market_observations: self.micro_market_observations,
            micro_market_afxdp_private_grade_observations: self
                .micro_market_afxdp_private_grade_observations,
            micro_market_banked_retained_ev_lamports: self.micro_market_banked_retained_ev_lamports,
            micro_market_flat_baseline_retained_ev_lamports: self
                .micro_market_flat_baseline_retained_ev_lamports,
            micro_market_delta_lamports: self.micro_market_delta_lamports,
            dominance_shadow_pnl_ok: self.dominance_shadow_pnl_ok,
            taylor_gate_ok: self.taylor_gate_ok,
            sparse_classes_fallback_safe: self.sparse_classes_fallback_safe,
            baseline_shadow_comparator_enabled: self.baseline_shadow_comparator_enabled,
            failback_gate_exists: self.failback_gate_exists,
            tactical_lane_ok,
            additive_quant_tactics_ok,
            promote_allowed,
            failback_required,
            kill_switch_required,
            public_scout_gates_green,
            all_gates_green,
            shadow_guard_deployed: self.shadow_guard_mainnet_deployed || self.shadow_guard_deployed,
            shadow_guard_program_id_present: self.shadow_guard_program_id_present,
            shadow_guard_mainnet_deployed: self.shadow_guard_mainnet_deployed
                || self.shadow_guard_deployed,
            shadow_guard_template_program_id_matches: self.shadow_guard_template_program_id_matches,
            dominance_live_allowed: promote_allowed && self.dominance_shadow_pnl_ok,
            competition_adjusted_primary_allowed: promote_allowed
                && competition_adjusted_promotion_ok
                && self.micro_market_uplift_ok,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct G12GateReport {
    pub rung: u8,
    pub sample_slots: u64,
    pub observe_candidates: u64,
    pub observe_candidates_total: u64,
    pub observe_candidates_external_geyser: u64,
    pub observe_candidates_local_validator_geyser: u64,
    pub observe_candidates_private_grade: u64,
    pub live_landed: u64,
    pub worker_health_snapshot_missing: u64,
    pub worker_health_drops: u64,
    pub worker_early_drop_mint_in_tag_missing: u64,
    pub worker_early_drop_mint_out_tag_missing: u64,
    pub worker_rollout_would_send_events_published: u64,
    pub worker_rollout_live_send_suppressed_events_published: u64,
    pub worker_candidates_entered_process_intent: u64,
    pub worker_candidates_dropped_before_process_intent: u64,
    pub worker_process_intent_ok: u64,
    pub worker_process_intent_err: u64,
    pub last_process_intent_error: String,
    pub last_process_intent_error_stage: String,
    pub last_process_intent_error_reason: String,
    pub worker_quote_ok: u64,
    pub worker_quote_err: u64,
    pub worker_lookup_ok: u64,
    pub worker_policy_ok: u64,
    pub worker_assembly_ready: u64,
    pub worker_pm_reservation_created: u64,
    pub worker_pipeline_err_routing_hydration: u64,
    pub worker_pipeline_err_blockhash_missing: u64,
    pub worker_pipeline_err_lookup: u64,
    pub worker_pipeline_err_quote: u64,
    pub worker_pipeline_err_quote_clmm_arena_missing: u64,
    pub worker_pipeline_err_quote_clmm_snapshot_missing: u64,
    pub worker_pipeline_err_quote_clmm_pool_mismatch: u64,
    pub worker_pipeline_err_quote_clmm_mint_in_mismatch: u64,
    pub worker_pipeline_err_quote_clmm_mint_out_mismatch: u64,
    pub worker_pipeline_err_quote_clmm_amount_mismatch: u64,
    pub worker_pipeline_err_quote_clmm_direction_mismatch: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: u64,
    pub worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: u64,
    pub worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: u64,
    pub worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: u64,
    pub worker_pipeline_err_quote_clmm_no_quote_out_of_range: u64,
    pub worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: u64,
    pub worker_pipeline_err_quote_clmm_stale_snapshot: u64,
    pub worker_pipeline_err_quote_clmm_target_out_underflow: u64,
    pub last_clmm_invalid_quote_pool: String,
    pub last_clmm_invalid_quote_mint_in: String,
    pub last_clmm_invalid_quote_mint_out: String,
    pub last_clmm_invalid_quote_amount_in: u64,
    pub last_clmm_invalid_quote_expected_out: u64,
    pub last_clmm_invalid_quote_min_out: u64,
    pub last_clmm_invalid_quote_projected_reserve_in: u64,
    pub last_clmm_invalid_quote_projected_reserve_out: u64,
    pub last_clmm_invalid_quote_quote_epoch: u64,
    pub last_clmm_invalid_quote_last_update_unix_ns: u64,
    pub last_clmm_invalid_quote_post_sqrt_price_x64: String,
    pub last_clmm_invalid_quote_post_tick: i32,
    pub last_clmm_invalid_quote_crossed_tick_count: u64,
    pub last_clmm_invalid_quote_direction: u64,
    pub last_clmm_invalid_quote_validity_flags: u64,
    pub worker_pipeline_err_invalid_direction: u64,
    pub worker_pipeline_err_no_send_sink_missing: u64,
    pub worker_pipeline_err_policy: u64,
    pub worker_pipeline_err_emit: u64,
    pub worker_route_resolved_jito: u64,
    pub worker_route_resolved_public_rpc_scout: u64,
    pub worker_route_resolved_harmonic: u64,
    pub worker_route_resolved_drop: u64,
    pub worker_public_rpc_scout_emit_attempt: u64,
    pub worker_public_rpc_scout_emit_ok: u64,
    pub run38_target_public_scout_clmm_route_resolved: u64,
    pub run38_target_public_scout_clmm_r1_attempt: u64,
    pub run38_target_public_scout_clmm_reject_proof_filter: u64,
    pub run38_target_public_scout_clmm_reject_micro_market: u64,
    pub run38_target_public_scout_clmm_reject_inventory: u64,
    pub run38_target_public_scout_clmm_reject_inventory_missing_input_ata: u64,
    pub run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: u64,
    pub run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: u64,
    pub run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: u64,
    pub run38_target_public_scout_clmm_reject_inventory_other: u64,
    pub run38_target_public_scout_clmm_reject_template_executability: u64,
    pub run38_target_public_scout_clmm_reject_attempt_latch: u64,
    pub run38_target_public_scout_clmm_admit: u64,
    pub run38_target_public_scout_clmm_process_intent: u64,
    pub run38_target_public_scout_clmm_emit_attempt: u64,
    pub run38_target_public_scout_clmm_transport_attempt: u64,
    pub run38_target_public_scout_clmm_pm_created: u64,
    pub public_scout_attempt_latch_set: u64,
    pub public_scout_attempt_latch_drop_count: u64,
    pub public_scout_attempt_latch_first_signature: String,
    pub pm_reserve_attempt: u64,
    pub pm_reserve_ok: u64,
    pub pm_reserve_reject_no_live_mode: u64,
    pub pm_reserve_reject_no_send_mode: u64,
    pub pm_reserve_reject_route_not_live: u64,
    pub pm_reserve_reject_cap: u64,
    pub pm_reserve_reject_inventory: u64,
    pub pm_reserve_reject_missing_signature: u64,
    pub pm_reserve_reject_other: u64,
    pub last_pm_reserve_reject_reason: String,
    pub last_pm_reserve_route: String,
    pub last_pm_reserve_pool: String,
    pub last_pm_reserve_mint_in: String,
    pub last_pm_reserve_mint_out: String,
    pub last_pm_reserve_amount: u64,
    pub clmm_input_ata_missing: u64,
    pub clmm_output_ata_missing: u64,
    pub clmm_input_ata_uninitialized: u64,
    pub clmm_output_ata_uninitialized: u64,
    pub clmm_input_balance_insufficient: u64,
    pub clmm_token_program_mismatch: u64,
    pub last_clmm_input_mint: String,
    pub last_clmm_output_mint: String,
    pub last_clmm_input_token_account: String,
    pub last_clmm_output_token_account: String,
    pub template_drop_no_template: u64,
    pub template_drop_wrong_route_family: u64,
    pub template_drop_missing_clmm_accounts: u64,
    pub template_drop_missing_input_ata: u64,
    pub template_drop_missing_output_ata: u64,
    pub template_drop_invalid_swap_program: u64,
    pub template_drop_invalid_guard_template: u64,
    pub template_drop_unsupported_token_program: u64,
    pub template_drop_other: u64,
    pub last_template_drop_reason: String,
    pub last_template_drop_pool: String,
    pub last_template_drop_mint_in: String,
    pub last_template_drop_mint_out: String,
    pub last_template_drop_route: String,
    pub last_template_drop_template_program: String,
    pub last_template_drop_input_ata: String,
    pub last_template_drop_output_ata: String,
    pub public_rpc_scout_enqueued: u64,
    pub public_rpc_transport_attempt: u64,
    pub public_rpc_transport_failure: u64,
    pub public_rpc_transport_rejected: u64,
    pub public_rpc_nack_recorded: u64,
    pub last_public_rpc_rejection_code: i64,
    pub last_public_rpc_rejection_message: String,
    pub last_public_rpc_rejection_body: String,
    pub last_public_rpc_rejection_data_err: String,
    pub last_public_rpc_rejection_simulation_logs: String,
    pub last_public_rpc_rejection_signature: String,
    pub last_public_rpc_rejection_tx_size: u64,
    pub last_public_rpc_rejection_blockhash: String,
    pub last_public_rpc_rejection_blockhash_matches_current: bool,
    pub last_public_rpc_rejection_blockhash_age_ns: u64,
    pub last_public_rpc_rejection_cu_limit: u64,
    pub last_public_rpc_rejection_cu_price_micro_lamports: u64,
    pub last_public_rpc_rejection_reason: String,
    pub last_public_rpc_rejection_ix1_program_id: String,
    pub last_public_rpc_rejection_ix1_account_0: String,
    pub last_public_rpc_rejection_ix1_account_1: String,
    pub last_public_rpc_rejection_ix1_account_count: u64,
    pub last_public_rpc_rejection_ix2_program_id: String,
    pub last_public_rpc_rejection_ix2_amount_in: u64,
    pub last_public_rpc_rejection_ix2_min_out: u64,
    pub last_public_rpc_rejection_ix2_account_count: u64,
    pub last_public_rpc_rejection_actual_output_lamports: u64,
    pub last_public_rpc_rejection_required_output_lamports: u64,
    pub pm_reservation_released: u64,
    pub pm_release_send_failed: u64,
    pub pm_release_nacked: u64,
    pub worker_presign_gate_reject: u64,
    pub worker_template_build_ok: u64,
    pub worker_template_build_err: u64,
    pub worker_no_send_backend_called: u64,
    pub worker_no_send_backend_ok: u64,
    pub worker_no_send_backend_err: u64,
    pub tpu_quic_candidate_seen: u64,
    pub tpu_quic_would_enqueue: u64,
    pub tpu_quic_ring_enqueue_ok: u64,
    pub tpu_quic_ring_enqueue_err: u64,
    pub tpu_quic_pm_reserve_simulated: u64,
    pub tpu_quic_last_signature: String,
    pub tpu_quic_last_route_family: String,
    pub scorecard_replay_stale_slot_bypass_seen: u64,
    pub scorecard_replay_stale_slot_bypass_applied: u64,
    pub scorecard_replay_stale_slot_bypass_signature_match: u64,
    pub scorecard_replay_stale_slot_bypass_pool_match: u64,
    pub scorecard_replay_original_slot: u64,
    pub scorecard_replay_current_slot: u64,
    pub scorecard_replay_slot_gap: u64,
    pub scorecard_replay_route_override_seen: u64,
    pub scorecard_replay_route_override_applied: u64,
    pub scorecard_replay_route_override_signature_match: u64,
    pub scorecard_replay_route_override_pool_match: u64,
    pub scorecard_replay_route_override_from_public_target: u64,
    pub scorecard_replay_route_override_to_tpu_quic_public: u64,
    pub scorecard_replay_source_override_seen: u64,
    pub scorecard_replay_source_override_applied: u64,
    pub scorecard_replay_source_override_signature_match: u64,
    pub scorecard_replay_source_override_pool_match: u64,
    pub scorecard_replay_source_override_live_send_blocked: u64,
    pub worker_final_no_send_event_constructed: u64,
    pub worker_final_no_send_event_published: u64,
    pub worker_final_no_send_event_publish_err: u64,
    pub worker_final_no_send_drop_reason_last: u64,
    pub worker_geyser_shadow_no_send_admitted: u64,
    pub worker_geyser_shadow_no_send_rejected: u64,
    pub worker_geyser_shadow_no_send_source_external: u64,
    pub worker_geyser_shadow_no_send_source_local_validator: u64,
    pub r1_auto_admission_attempt: u64,
    pub r1_auto_admission_admit: u64,
    pub r1_auto_admission_admit_to_process_intent_attempt: u64,
    pub r1_auto_admission_admit_to_process_intent_ok: u64,
    pub r1_auto_admission_admit_to_process_intent_drop: u64,
    pub clmm_exact_quote_request_publish_attempt: u64,
    pub clmm_exact_quote_request_publish_ok: u64,
    pub clmm_exact_quote_request_publish_full: u64,
    pub clmm_exact_quote_request_overwrite_same_pool: u64,
    pub clmm_exact_quote_request_suppressed_outstanding_same_pool: u64,
    pub clmm_exact_quote_request_outstanding_same_pool_hit: u64,
    pub clmm_exact_quote_request_outstanding_cleared_matched: u64,
    pub clmm_exact_quote_request_outstanding_cleared_timeout: u64,
    pub clmm_exact_quote_request_outstanding_cleared_no_quote: u64,
    pub clmm_exact_quote_response_matched: u64,
    pub clmm_exact_quote_response_timeout: u64,
    pub clmm_exact_quote_response_no_snapshot: u64,
    pub clmm_exact_quote_response_stale: u64,
    pub clmm_exact_quote_response_pool_mismatch: u64,
    pub clmm_exact_quote_response_mint_mismatch: u64,
    pub clmm_exact_quote_response_amount_mismatch: u64,
    pub clmm_exact_quote_response_request_id_mismatch: u64,
    pub clmm_exact_quote_response_invalid_flags: u64,
    pub clmm_exact_quote_response_invalid_flags_zero: u64,
    pub clmm_exact_quote_response_invalid_flags_no_valid_bit: u64,
    pub clmm_exact_quote_response_invalid_flags_no_quote_bit: u64,
    pub clmm_exact_quote_response_invalid_flags_unknown_status: u64,
    pub clmm_exact_quote_response_invalid_valid_quote_shape: u64,
    pub clmm_exact_quote_response_no_quote_no_current_array_upstream: u64,
    pub clmm_exact_quote_response_no_quote_no_liquidity_boundary: u64,
    pub clmm_exact_quote_response_no_quote_out_of_range: u64,
    pub clmm_exact_quote_response_no_quote_missing_tick_array_local: u64,
    pub clmm_exact_quote_response_timeout_terminal_no_quote: u64,
    pub clmm_exact_quote_response_timeout_quote_ready_rejected: u64,
    pub clmm_exact_quote_response_timeout_stale_request_id: u64,
    pub clmm_exact_quote_response_timeout_quote_build_failed: u64,
    pub clmm_exact_quote_response_timeout_dependency_missing: u64,
    pub r1_auto_admission_drop_source: u64,
    pub r1_auto_admission_drop_non_private_grade: u64,
    pub r1_auto_admission_drop_shadowguard: u64,
    pub r1_auto_admission_drop_micro_market: u64,
    pub r1_auto_admission_drop_egress: u64,
    pub r1_auto_admission_drop_inventory: u64,
    pub r1_auto_admission_drop_cap: u64,
    pub r1_auto_admission_drop_stale: u64,
    pub r1_auto_admission_drop_public_route: u64,
    pub r1_auto_admission_reject_source: u64,
    pub r1_auto_admission_reject_non_private_grade: u64,
    pub r1_auto_admission_reject_shadowguard: u64,
    pub r1_auto_admission_reject_micro_market: u64,
    pub r1_auto_admission_reject_egress: u64,
    pub r1_auto_admission_reject_inventory: u64,
    pub r1_auto_admission_reject_cap: u64,
    pub r1_auto_admission_reject_stale: u64,
    pub r1_auto_admission_reject_public_route: u64,
    pub r1_auto_admission_reject_proof_filter: u64,
    pub r1_auto_admission_reject_template_executability: u64,
    pub r1_auto_admission_reject_attempt_latch: u64,
    pub r1_auto_admission_reject_proof_filter_no_quote: u64,
    pub r1_auto_admission_reject_proof_filter_no_positive_ev: u64,
    pub r1_auto_admission_reject_proof_filter_no_shadow_guard: u64,
    pub r1_auto_admission_reject_proof_filter_no_micro_market: u64,
    pub r1_auto_admission_reject_proof_filter_public_scout_shape: u64,
    pub r1_auto_admission_reject_proof_filter_attempt_latch: u64,
    pub r1_auto_admission_reject_proof_filter_other: u64,
    pub r1_auto_admission_reject_public_scout_shape_route_not_public_scout: u64,
    pub r1_auto_admission_reject_public_scout_shape_wrong_source: u64,
    pub r1_auto_admission_reject_public_scout_shape_wrong_venue: u64,
    pub r1_auto_admission_reject_public_scout_shape_missing_template_family: u64,
    pub r1_auto_admission_reject_public_scout_shape_route_template_mismatch: u64,
    pub r1_auto_admission_reject_public_scout_shape_other: u64,
    pub r1_auto_admission_reject_micro_market_no_uplift: u64,
    pub r1_auto_admission_reject_micro_market_negative_ev: u64,
    pub r1_auto_admission_reject_micro_market_below_floor: u64,
    pub r1_auto_admission_reject_micro_market_other: u64,
    pub last_public_scout_shape_reject_reason: String,
    pub last_public_scout_shape_reject_pool: String,
    pub last_public_scout_shape_reject_mint_in: String,
    pub last_public_scout_shape_reject_mint_out: String,
    pub last_public_scout_shape_reject_route: String,
    pub last_public_scout_shape_reject_source: String,
    pub last_public_scout_shape_reject_venue: String,
    pub last_run38_target_pool: String,
    pub last_run38_target_mint_in: String,
    pub last_run38_target_mint_out: String,
    pub last_run38_target_route: String,
    pub last_run38_target_source: String,
    pub last_run38_target_venue: String,
    pub last_run38_target_stage: String,
    pub last_run38_target_reject_reason: String,
    pub last_run38_target_inventory_reject_reason: String,
    pub last_run38_target_inventory_pool: String,
    pub last_run38_target_inventory_mint_in: String,
    pub last_run38_target_inventory_mint_out: String,
    pub last_run38_target_inventory_input_ata: String,
    pub last_run38_target_inventory_input_balance: u64,
    pub last_run38_target_inventory_required_input: u64,
    pub last_run38_target_inventory_token_program: String,
    pub last_r1_auto_admission_reject_reason: String,
    pub last_r1_auto_admission_reject_stage: String,
    pub last_r1_auto_admission_pool: String,
    pub last_r1_auto_admission_mint_in: String,
    pub last_r1_auto_admission_mint_out: String,
    pub last_r1_template_executability_reject_reason: String,
    pub last_r1_template_executability_pool: String,
    pub last_r1_template_executability_mint_in: String,
    pub last_r1_template_executability_mint_out: String,
    pub last_r1_template_executability_route: String,
    pub last_r1_template_executability_output_ata: String,
    pub last_admit_to_process_intent_drop_reason: String,
    pub last_admit_to_process_intent_pool: String,
    pub last_admit_to_process_intent_mint_in: String,
    pub last_admit_to_process_intent_mint_out: String,
    pub last_admit_to_process_intent_route: String,
    pub last_admit_to_process_intent_source: String,
    pub last_clmm_exact_quote_response_mismatch_reason: String,
    pub last_clmm_exact_quote_response_pool_index: u64,
    pub last_clmm_exact_quote_response_sequence: u64,
    pub last_clmm_exact_quote_response_request_id_expected: u64,
    pub last_clmm_exact_quote_response_request_id_found: u64,
    pub last_clmm_exact_quote_response_pool_expected: String,
    pub last_clmm_exact_quote_response_pool_found: String,
    pub last_clmm_exact_quote_response_mint_in_expected: String,
    pub last_clmm_exact_quote_response_mint_in_found: String,
    pub last_clmm_exact_quote_response_mint_out_expected: String,
    pub last_clmm_exact_quote_response_mint_out_found: String,
    pub last_clmm_exact_quote_response_amount_expected: u64,
    pub last_clmm_exact_quote_response_amount_found: u64,
    pub last_clmm_exact_quote_response_validity_flags: u64,
    pub last_clmm_exact_quote_response_status: String,
    pub last_clmm_exact_quote_response_status_code: u64,
    pub last_clmm_exact_quote_response_timeout_classification: String,
    pub last_clmm_exact_quote_response_quote_epoch: u64,
    pub last_clmm_exact_quote_response_last_update_unix_ns: u64,
    pub r1_afxdp_private_grade_route_actionable: u64,
    pub r1_afxdp_private_grade_process_intent_entered: u64,
    pub r1_afxdp_private_grade_auto_admission_attempt: u64,
    pub r1_afxdp_private_grade_auto_admission_admit: u64,
    pub worker_signal_external_geyser: u64,
    pub worker_signal_local_validator_geyser: u64,
    pub worker_signal_afxdp: u64,
    pub worker_signal_dpdk: u64,
    pub worker_signal_unknown: u64,
    pub dpdk_worker_records_seen: u64,
    pub dpdk_worker_health_drop: u64,
    pub dpdk_worker_stale_slot_drop: u64,
    pub dpdk_worker_pool_state_miss: u64,
    pub dpdk_worker_pool_state_hit: u64,
    pub dpdk_worker_route_actionable: u64,
    pub dpdk_worker_route_dropped: u64,
    pub dpdk_worker_route_unavailable: u64,
    pub dpdk_worker_route_unavailable_unknown_signal: u64,
    pub dpdk_worker_route_unavailable_hydration_none: u64,
    pub dpdk_worker_r1_admission_drop: u64,
    pub dpdk_worker_candidates_entered_process_intent: u64,
    pub dpdk_worker_pipeline_error: u64,
    pub dpdk_worker_rollout_shadow_published: u64,
    pub dpdk_worker_final_no_send_published: u64,
    pub worker_route_unavailable_hydration_none_afxdp: u64,
    pub worker_afxdp_hydration_request_enqueued: u64,
    pub worker_afxdp_hydration_request_deduped: u64,
    pub worker_source_probe_afxdp_parsed_shred_drops: u64,
    pub afxdp_frames_seen: u64,
    pub afxdp_udp_8000_8025_seen: u64,
    pub afxdp_xsk_rx_batches: u64,
    pub afxdp_shred_parse_ok: u64,
    pub afxdp_shred_parse_err: u64,
    pub afxdp_shred_parse_data_ok: u64,
    pub afxdp_shred_parse_coding_ok: u64,
    pub afxdp_feed_enqueue_ok: u64,
    pub afxdp_feed_enqueue_full: u64,
    pub afxdp_reassembly_insert_ok: u64,
    pub afxdp_reassembly_complete_entries: u64,
    pub afxdp_fec_recovered_entries: u64,
    pub afxdp_data_shred_payload_ok: u64,
    pub afxdp_tx_extract_attempt: u64,
    pub afxdp_tx_extract_ok: u64,
    pub afxdp_tx_extract_err: u64,
    pub afxdp_tx_extract_err_empty_payload: u64,
    pub afxdp_tx_extract_err_short_vec: u64,
    pub afxdp_tx_extract_err_bad_entry_header: u64,
    pub afxdp_tx_extract_err_bad_compact_u16: u64,
    pub afxdp_tx_extract_err_sanitize_failed: u64,
    pub afxdp_tx_extract_err_no_transactions: u64,
    pub afxdp_tx_extract_err_unsupported_format: u64,
    pub afxdp_tx_record_emit_attempt: u64,
    pub afxdp_tx_record_emit_ok: u64,
    pub afxdp_tx_record_emit_full: u64,
    pub afxdp_pipeline_drop_unsupported_program: u64,
    pub afxdp_pipeline_drop_allowlist_miss: u64,
    pub afxdp_pipeline_drop_decode_fail: u64,
    pub afxdp_pipeline_drop_route_fail: u64,
    pub afxdp_pipeline_drop_mint_fail: u64,
    pub afxdp_pipeline_drop_other: u64,
    pub afxdp_unsupported_program_top_0_id: String,
    pub afxdp_unsupported_program_top_0_count: u64,
    pub afxdp_unsupported_program_top_1_id: String,
    pub afxdp_unsupported_program_top_1_count: u64,
    pub afxdp_unsupported_program_top_2_id: String,
    pub afxdp_unsupported_program_top_2_count: u64,
    pub afxdp_unsupported_program_top_3_id: String,
    pub afxdp_unsupported_program_top_3_count: u64,
    pub afxdp_unsupported_program_top_4_id: String,
    pub afxdp_unsupported_program_top_4_count: u64,
    pub afxdp_unsupported_program_top_5_id: String,
    pub afxdp_unsupported_program_top_5_count: u64,
    pub afxdp_unsupported_program_top_6_id: String,
    pub afxdp_unsupported_program_top_6_count: u64,
    pub afxdp_unsupported_program_top_7_id: String,
    pub afxdp_unsupported_program_top_7_count: u64,
    pub afxdp_unsupported_program_top_8_id: String,
    pub afxdp_unsupported_program_top_8_count: u64,
    pub afxdp_unsupported_program_top_9_id: String,
    pub afxdp_unsupported_program_top_9_count: u64,
    pub afxdp_unsupported_program_top_10_id: String,
    pub afxdp_unsupported_program_top_10_count: u64,
    pub afxdp_unsupported_program_top_11_id: String,
    pub afxdp_unsupported_program_top_11_count: u64,
    pub afxdp_unsupported_program_top_12_id: String,
    pub afxdp_unsupported_program_top_12_count: u64,
    pub afxdp_unsupported_program_top_13_id: String,
    pub afxdp_unsupported_program_top_13_count: u64,
    pub afxdp_unsupported_program_top_14_id: String,
    pub afxdp_unsupported_program_top_14_count: u64,
    pub afxdp_unsupported_program_top_15_id: String,
    pub afxdp_unsupported_program_top_15_count: u64,
    pub afxdp_source_probe_emit_attempt: u64,
    pub afxdp_source_probe_emit_ok: u64,
    pub afxdp_source_probe_emit_full: u64,
    pub afxdp_source_probe_emit_err: u64,
    pub grpc_connected: u64,
    pub grpc_subscribe_sent: u64,
    pub grpc_updates_total: u64,
    pub grpc_updates_transaction: u64,
    pub grpc_updates_non_transaction: u64,
    pub grpc_normalize_ok: u64,
    pub grpc_normalize_err: u64,
    pub grpc_pipeline_drop_unsupported_program: u64,
    pub grpc_pipeline_drop_allowlist_miss: u64,
    pub grpc_pipeline_drop_decode_fail: u64,
    pub grpc_pipeline_drop_route_fail: u64,
    pub grpc_pipeline_drop_mint_fail: u64,
    pub grpc_pipeline_drop_other: u64,
    pub grpc_unsupported_program_top_0_id: String,
    pub grpc_unsupported_program_top_0_count: u64,
    pub grpc_unsupported_program_top_1_id: String,
    pub grpc_unsupported_program_top_1_count: u64,
    pub grpc_unsupported_program_top_2_id: String,
    pub grpc_unsupported_program_top_2_count: u64,
    pub grpc_unsupported_program_top_3_id: String,
    pub grpc_unsupported_program_top_3_count: u64,
    pub grpc_unsupported_program_top_4_id: String,
    pub grpc_unsupported_program_top_4_count: u64,
    pub grpc_unsupported_program_top_5_id: String,
    pub grpc_unsupported_program_top_5_count: u64,
    pub grpc_unsupported_program_top_6_id: String,
    pub grpc_unsupported_program_top_6_count: u64,
    pub grpc_unsupported_program_top_7_id: String,
    pub grpc_unsupported_program_top_7_count: u64,
    pub grpc_unsupported_program_top_8_id: String,
    pub grpc_unsupported_program_top_8_count: u64,
    pub grpc_unsupported_program_top_9_id: String,
    pub grpc_unsupported_program_top_9_count: u64,
    pub grpc_unsupported_program_top_10_id: String,
    pub grpc_unsupported_program_top_10_count: u64,
    pub grpc_unsupported_program_top_11_id: String,
    pub grpc_unsupported_program_top_11_count: u64,
    pub grpc_unsupported_program_top_12_id: String,
    pub grpc_unsupported_program_top_12_count: u64,
    pub grpc_unsupported_program_top_13_id: String,
    pub grpc_unsupported_program_top_13_count: u64,
    pub grpc_unsupported_program_top_14_id: String,
    pub grpc_unsupported_program_top_14_count: u64,
    pub grpc_unsupported_program_top_15_id: String,
    pub grpc_unsupported_program_top_15_count: u64,
    pub grpc_feed_enqueue_ok: u64,
    pub grpc_feed_enqueue_full: u64,
    pub grpc_feed_enqueue_err: u64,
    pub grpc_signal_source_local_validator_geyser: u64,

    pub public_lane_amputation_ok: bool,
    pub inventory_truth_ok: bool,
    pub fee_leak_ok: bool,
    pub fee_leak_accounted: bool,
    pub phantom_inventory_risk: bool,
    pub selective_death_clamp_ok: bool,
    pub pre_sign_egress_gate_ok: bool,
    pub private_lane_admissibility_ok: bool,
    pub rollout_events_observed: u64,
    pub rollout_would_send: u64,
    pub rollout_live_send_suppressed: u64,
    pub rollout_private_lane_admissible_opportunities: u64,
    pub rollout_private_grade_admissible_opportunities: u64,
    pub rollout_geyser_shadow_opportunities: u64,
    pub rollout_afxdp_private_grade_would_send: u64,
    pub rollout_afxdp_private_grade_live_send_suppressed: u64,
    pub rollout_jito_no_send_would_sends: u64,
    pub rollout_harmonic_no_send_would_sends: u64,
    pub rollout_public_rpc_no_send_would_sends: u64,
    pub rollout_last_no_send_reason: u16,
    pub rollout_last_drop_reason: u16,
    pub latency_ok: bool,
    pub worker_latency_proof_present: bool,
    pub worker_latency_p99_ns: u64,
    pub worker_latency_budget_ns: u64,
    pub dpdk_producer_to_feed_count: u64,
    pub dpdk_producer_to_feed_p50_ns: u64,
    pub dpdk_producer_to_feed_p90_ns: u64,
    pub dpdk_producer_to_feed_p99_ns: u64,
    pub dpdk_producer_to_feed_p999_ns: u64,
    pub dpdk_producer_to_feed_max_ns: u64,
    pub dpdk_feed_to_worker_count: u64,
    pub dpdk_feed_to_worker_p50_ns: u64,
    pub dpdk_feed_to_worker_p90_ns: u64,
    pub dpdk_feed_to_worker_p99_ns: u64,
    pub dpdk_feed_to_worker_p999_ns: u64,
    pub dpdk_feed_to_worker_max_ns: u64,
    pub dpdk_worker_to_nosend_count: u64,
    pub dpdk_worker_to_nosend_p50_ns: u64,
    pub dpdk_worker_to_nosend_p90_ns: u64,
    pub dpdk_worker_to_nosend_p99_ns: u64,
    pub dpdk_worker_to_nosend_p999_ns: u64,
    pub dpdk_worker_to_nosend_max_ns: u64,
    pub dpdk_end_to_end_observe_count: u64,
    pub dpdk_end_to_end_observe_p50_ns: u64,
    pub dpdk_end_to_end_observe_p90_ns: u64,
    pub dpdk_end_to_end_observe_p99_ns: u64,
    pub dpdk_end_to_end_observe_p999_ns: u64,
    pub dpdk_end_to_end_observe_max_ns: u64,
    pub realized_pnl_ok: bool,
    pub shadow_pnl_ok: bool,
    pub competition_adjusted_ok: bool,
    pub competition_adjusted_retained_ev_delta_lamports: i64,
    pub competition_adjusted_fee_burn_delta_lamports: i64,
    pub micro_market_uplift_ok: bool,
    pub micro_market_observations: u64,
    pub micro_market_afxdp_private_grade_observations: u64,
    pub micro_market_banked_retained_ev_lamports: i64,
    pub micro_market_flat_baseline_retained_ev_lamports: i64,
    pub micro_market_delta_lamports: i64,
    pub dominance_shadow_pnl_ok: bool,
    pub taylor_gate_ok: bool,
    pub sparse_classes_fallback_safe: bool,
    pub baseline_shadow_comparator_enabled: bool,
    pub failback_gate_exists: bool,
    pub tactical_lane_ok: bool,
    pub additive_quant_tactics_ok: bool,

    pub promote_allowed: bool,
    pub failback_required: bool,
    pub kill_switch_required: bool,

    pub public_scout_gates_green: bool,
    pub all_gates_green: bool,
    pub shadow_guard_deployed: bool,
    pub shadow_guard_program_id_present: bool,
    pub shadow_guard_mainnet_deployed: bool,
    pub shadow_guard_template_program_id_matches: bool,
    pub dominance_live_allowed: bool,
    pub competition_adjusted_primary_allowed: bool,
}

pub fn default_gate_report_path() -> PathBuf {
    PathBuf::from(DEFAULT_G12_GATE_REPORT_PATH)
}

pub fn read_gate_evaluator_input(path: &Path) -> Result<G12GateEvaluatorInput, RolloutGateError> {
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

pub fn write_gate_evaluator_input(
    path: &Path,
    input: &G12GateEvaluatorInput,
) -> Result<(), RolloutGateError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_vec_pretty(input)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn write_gate_report(path: &Path, report: &G12GateReport) -> Result<(), RolloutGateError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_vec_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn evaluate_gate_report_from_input(
    input: &G12GateEvaluatorInput,
    output_path: &Path,
) -> Result<G12GateReport, RolloutGateError> {
    let report = input.evaluate();
    write_gate_report(output_path, &report)?;
    Ok(report)
}

pub fn evaluate_gate_report_from_json(
    input_path: &Path,
    output_path: Option<&Path>,
) -> Result<G12GateReport, RolloutGateError> {
    let input = read_gate_evaluator_input(input_path)?;
    evaluate_gate_report_from_input(
        &input,
        output_path.unwrap_or(Path::new(DEFAULT_G12_GATE_REPORT_PATH)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn green_input() -> G12GateEvaluatorInput {
        G12GateEvaluatorInput {
            rung: 1,
            sample_slots: 10_000,
            observe_candidates: 10_000,
            observe_candidates_total: 10_000,
            observe_candidates_external_geyser: 0,
            observe_candidates_local_validator_geyser: 0,
            observe_candidates_private_grade: 10_000,
            live_landed: 0,
            min_sample_slots: 10_000,
            min_observe_candidates: 10_000,
            min_live_landed: 0,
            worker_records_seen: 10_000,
            worker_health_snapshot_missing: 0,
            worker_health_drops: 0,
            worker_stale_slot_drops: 0,
            worker_pool_state_miss: 0,
            worker_pool_state_miss_tag_map_absent: 0,
            worker_pool_state_miss_arena_oob: 0,
            worker_pool_state_miss_arena_torn: 0,
            worker_pool_state_miss_pubkey_mismatch: 0,
            last_worker_pool_state_miss_pool_pubkey: String::new(),
            last_worker_pool_state_miss_venue_id: 0,
            last_worker_pool_state_miss_signal_source: 0,
            last_worker_pool_state_miss_reason_code: 0,
            worker_pool_state_miss_recent_ring: Vec::new(),
            worker_pool_state_miss_top_recent: Vec::new(),
            worker_pool_state_hit: 10_000,
            worker_early_drop_mint_in_tag_missing: 0,
            worker_early_drop_mint_out_tag_missing: 0,
            worker_shadow_events_published: 10_000,
            worker_rollout_would_send_events_published: 128,
            worker_rollout_live_send_suppressed_events_published: 128,
            worker_candidates_entered_process_intent: 128,
            worker_candidates_dropped_before_process_intent: 0,
            worker_process_intent_ok: 128,
            worker_process_intent_err: 0,
            last_process_intent_error: String::new(),
            last_process_intent_error_stage: String::new(),
            last_process_intent_error_reason: String::new(),
            worker_quote_ok: 128,
            worker_quote_err: 0,
            worker_lookup_ok: 128,
            worker_policy_ok: 128,
            worker_assembly_ready: 128,
            worker_pm_reservation_created: 128,
            worker_route_resolved_jito: 10_000,
            worker_route_resolved_public_rpc_scout: 0,
            worker_route_resolved_harmonic: 0,
            worker_route_resolved_drop: 0,
            worker_public_rpc_scout_emit_attempt: 0,
            worker_public_rpc_scout_emit_ok: 0,
            run38_target_public_scout_clmm_route_resolved: 0,
            run38_target_public_scout_clmm_r1_attempt: 0,
            run38_target_public_scout_clmm_reject_proof_filter: 0,
            run38_target_public_scout_clmm_reject_micro_market: 0,
            run38_target_public_scout_clmm_reject_inventory: 0,
            run38_target_public_scout_clmm_reject_inventory_missing_input_ata: 0,
            run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized: 0,
            run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient: 0,
            run38_target_public_scout_clmm_reject_inventory_token_program_mismatch: 0,
            run38_target_public_scout_clmm_reject_inventory_other: 0,
            run38_target_public_scout_clmm_reject_template_executability: 0,
            run38_target_public_scout_clmm_reject_attempt_latch: 0,
            run38_target_public_scout_clmm_admit: 0,
            run38_target_public_scout_clmm_process_intent: 0,
            run38_target_public_scout_clmm_emit_attempt: 0,
            run38_target_public_scout_clmm_transport_attempt: 0,
            run38_target_public_scout_clmm_pm_created: 0,
            public_scout_attempt_latch_set: 0,
            public_scout_attempt_latch_drop_count: 0,
            public_scout_attempt_latch_first_signature: String::new(),
            clmm_input_ata_missing: 0,
            clmm_output_ata_missing: 0,
            clmm_input_ata_uninitialized: 0,
            clmm_output_ata_uninitialized: 0,
            clmm_input_balance_insufficient: 0,
            clmm_token_program_mismatch: 0,
            last_clmm_input_mint: String::new(),
            last_clmm_output_mint: String::new(),
            last_clmm_input_token_account: String::new(),
            last_clmm_output_token_account: String::new(),
            template_drop_no_template: 0,
            template_drop_wrong_route_family: 0,
            template_drop_missing_clmm_accounts: 0,
            template_drop_missing_input_ata: 0,
            template_drop_missing_output_ata: 0,
            template_drop_invalid_swap_program: 0,
            template_drop_invalid_guard_template: 0,
            template_drop_unsupported_token_program: 0,
            template_drop_other: 0,
            last_template_drop_reason: String::new(),
            last_template_drop_pool: String::new(),
            last_template_drop_mint_in: String::new(),
            last_template_drop_mint_out: String::new(),
            last_template_drop_route: String::new(),
            last_template_drop_template_program: String::new(),
            last_template_drop_input_ata: String::new(),
            last_template_drop_output_ata: String::new(),
            public_rpc_scout_enqueued: 0,
            public_rpc_transport_attempt: 0,
            public_rpc_transport_failure: 0,
            public_rpc_transport_rejected: 0,
            public_rpc_nack_recorded: 0,
            last_public_rpc_rejection_code: 0,
            last_public_rpc_rejection_message: String::new(),
            last_public_rpc_rejection_body: String::new(),
            last_public_rpc_rejection_data_err: String::new(),
            last_public_rpc_rejection_simulation_logs: String::new(),
            last_public_rpc_rejection_signature: String::new(),
            last_public_rpc_rejection_tx_size: 0,
            last_public_rpc_rejection_blockhash: String::new(),
            last_public_rpc_rejection_blockhash_matches_current: false,
            last_public_rpc_rejection_blockhash_age_ns: 0,
            last_public_rpc_rejection_cu_limit: 0,
            last_public_rpc_rejection_cu_price_micro_lamports: 0,
            last_public_rpc_rejection_reason: String::new(),
            last_public_rpc_rejection_ix1_program_id: String::new(),
            last_public_rpc_rejection_ix1_account_0: String::new(),
            last_public_rpc_rejection_ix1_account_1: String::new(),
            last_public_rpc_rejection_ix1_account_count: 0,
            last_public_rpc_rejection_ix2_program_id: String::new(),
            last_public_rpc_rejection_ix2_amount_in: 0,
            last_public_rpc_rejection_ix2_min_out: 0,
            last_public_rpc_rejection_ix2_account_count: 0,
            last_public_rpc_rejection_actual_output_lamports: 0,
            last_public_rpc_rejection_required_output_lamports: 0,
            pm_reservation_released: 128,
            pm_release_send_failed: 0,
            pm_release_nacked: 0,
            worker_presign_gate_reject: 0,
            worker_template_build_ok: 128,
            worker_template_build_err: 0,
            worker_no_send_backend_called: 128,
            worker_no_send_backend_ok: 128,
            worker_no_send_backend_err: 0,
            tpu_quic_candidate_seen: 0,
            tpu_quic_would_enqueue: 0,
            tpu_quic_ring_enqueue_ok: 0,
            tpu_quic_ring_enqueue_err: 0,
            tpu_quic_pm_reserve_simulated: 0,
            tpu_quic_last_signature: String::new(),
            tpu_quic_last_route_family: String::new(),
            scorecard_replay_stale_slot_bypass_seen: 0,
            scorecard_replay_stale_slot_bypass_applied: 0,
            scorecard_replay_stale_slot_bypass_signature_match: 0,
            scorecard_replay_stale_slot_bypass_pool_match: 0,
            scorecard_replay_original_slot: 0,
            scorecard_replay_current_slot: 0,
            scorecard_replay_slot_gap: 0,
            scorecard_replay_route_override_seen: 0,
            scorecard_replay_route_override_applied: 0,
            scorecard_replay_route_override_signature_match: 0,
            scorecard_replay_route_override_pool_match: 0,
            scorecard_replay_route_override_from_public_target: 0,
            scorecard_replay_route_override_to_tpu_quic_public: 0,
            scorecard_replay_source_override_seen: 0,
            scorecard_replay_source_override_applied: 0,
            scorecard_replay_source_override_signature_match: 0,
            scorecard_replay_source_override_pool_match: 0,
            scorecard_replay_source_override_live_send_blocked: 0,
            worker_final_no_send_event_constructed: 128,
            worker_final_no_send_event_published: 128,
            worker_final_no_send_event_publish_err: 0,
            worker_final_no_send_drop_reason_last: 12,
            worker_geyser_shadow_no_send_admitted: 0,
            worker_geyser_shadow_no_send_rejected: 0,
            worker_geyser_shadow_no_send_source_external: 0,
            worker_geyser_shadow_no_send_source_local_validator: 0,
            r1_auto_admission_attempt: 128,
            r1_auto_admission_admit: 128,
            r1_auto_admission_admit_to_process_intent_attempt: 128,
            r1_auto_admission_admit_to_process_intent_ok: 128,
            r1_auto_admission_admit_to_process_intent_drop: 0,
            clmm_exact_quote_request_publish_attempt: 0,
            clmm_exact_quote_request_publish_ok: 0,
            clmm_exact_quote_request_publish_full: 0,
            clmm_exact_quote_request_overwrite_same_pool: 0,
            clmm_exact_quote_request_suppressed_outstanding_same_pool: 0,
            clmm_exact_quote_request_outstanding_same_pool_hit: 0,
            clmm_exact_quote_request_outstanding_cleared_matched: 0,
            clmm_exact_quote_request_outstanding_cleared_timeout: 0,
            clmm_exact_quote_request_outstanding_cleared_no_quote: 0,
            clmm_exact_quote_response_matched: 0,
            clmm_exact_quote_response_timeout: 0,
            clmm_exact_quote_response_no_snapshot: 0,
            clmm_exact_quote_response_stale: 0,
            clmm_exact_quote_response_pool_mismatch: 0,
            clmm_exact_quote_response_mint_mismatch: 0,
            clmm_exact_quote_response_amount_mismatch: 0,
            clmm_exact_quote_response_request_id_mismatch: 0,
            clmm_exact_quote_response_invalid_flags: 0,
            clmm_exact_quote_response_invalid_flags_zero: 0,
            clmm_exact_quote_response_invalid_flags_no_valid_bit: 0,
            clmm_exact_quote_response_invalid_flags_no_quote_bit: 0,
            clmm_exact_quote_response_invalid_flags_unknown_status: 0,
            clmm_exact_quote_response_invalid_valid_quote_shape: 0,
            clmm_exact_quote_response_no_quote_no_current_array_upstream: 0,
            clmm_exact_quote_response_no_quote_no_liquidity_boundary: 0,
            clmm_exact_quote_response_no_quote_out_of_range: 0,
            clmm_exact_quote_response_no_quote_missing_tick_array_local: 0,
            clmm_exact_quote_response_timeout_terminal_no_quote: 0,
            clmm_exact_quote_response_timeout_quote_ready_rejected: 0,
            clmm_exact_quote_response_timeout_stale_request_id: 0,
            clmm_exact_quote_response_timeout_quote_build_failed: 0,
            clmm_exact_quote_response_timeout_dependency_missing: 0,
            r1_auto_admission_drop_source: 0,
            r1_auto_admission_drop_non_private_grade: 0,
            r1_auto_admission_drop_shadowguard: 0,
            r1_auto_admission_drop_micro_market: 0,
            r1_auto_admission_drop_egress: 0,
            r1_auto_admission_drop_inventory: 0,
            r1_auto_admission_drop_cap: 0,
            r1_auto_admission_drop_stale: 0,
            r1_auto_admission_drop_public_route: 0,
            r1_auto_admission_reject_source: 0,
            r1_auto_admission_reject_non_private_grade: 0,
            r1_auto_admission_reject_shadowguard: 0,
            r1_auto_admission_reject_micro_market: 0,
            r1_auto_admission_reject_egress: 0,
            r1_auto_admission_reject_inventory: 0,
            r1_auto_admission_reject_cap: 0,
            r1_auto_admission_reject_stale: 0,
            r1_auto_admission_reject_public_route: 0,
            r1_auto_admission_reject_proof_filter: 0,
            r1_auto_admission_reject_template_executability: 0,
            r1_auto_admission_reject_attempt_latch: 0,
            r1_auto_admission_reject_proof_filter_no_quote: 0,
            r1_auto_admission_reject_proof_filter_no_positive_ev: 0,
            r1_auto_admission_reject_proof_filter_no_shadow_guard: 0,
            r1_auto_admission_reject_proof_filter_no_micro_market: 0,
            r1_auto_admission_reject_proof_filter_public_scout_shape: 0,
            r1_auto_admission_reject_proof_filter_attempt_latch: 0,
            r1_auto_admission_reject_proof_filter_other: 0,
            r1_auto_admission_reject_public_scout_shape_route_not_public_scout: 0,
            r1_auto_admission_reject_public_scout_shape_wrong_source: 0,
            r1_auto_admission_reject_public_scout_shape_wrong_venue: 0,
            r1_auto_admission_reject_public_scout_shape_missing_template_family: 0,
            r1_auto_admission_reject_public_scout_shape_route_template_mismatch: 0,
            r1_auto_admission_reject_public_scout_shape_other: 0,
            r1_auto_admission_reject_micro_market_no_uplift: 0,
            r1_auto_admission_reject_micro_market_negative_ev: 0,
            r1_auto_admission_reject_micro_market_below_floor: 0,
            r1_auto_admission_reject_micro_market_other: 0,
            last_public_scout_shape_reject_reason: String::new(),
            last_public_scout_shape_reject_pool: String::new(),
            last_public_scout_shape_reject_mint_in: String::new(),
            last_public_scout_shape_reject_mint_out: String::new(),
            last_public_scout_shape_reject_route: String::new(),
            last_public_scout_shape_reject_source: String::new(),
            last_public_scout_shape_reject_venue: String::new(),
            last_run38_target_pool: String::new(),
            last_run38_target_mint_in: String::new(),
            last_run38_target_mint_out: String::new(),
            last_run38_target_route: String::new(),
            last_run38_target_source: String::new(),
            last_run38_target_venue: String::new(),
            last_run38_target_stage: String::new(),
            last_run38_target_reject_reason: String::new(),
            last_run38_target_inventory_reject_reason: String::new(),
            last_run38_target_inventory_pool: String::new(),
            last_run38_target_inventory_mint_in: String::new(),
            last_run38_target_inventory_mint_out: String::new(),
            last_run38_target_inventory_input_ata: String::new(),
            last_run38_target_inventory_input_balance: 0,
            last_run38_target_inventory_required_input: 0,
            last_run38_target_inventory_token_program: String::new(),
            last_r1_auto_admission_reject_reason: String::new(),
            last_r1_auto_admission_reject_stage: String::new(),
            last_r1_auto_admission_pool: String::new(),
            last_r1_auto_admission_mint_in: String::new(),
            last_r1_auto_admission_mint_out: String::new(),
            last_r1_template_executability_reject_reason: String::new(),
            last_r1_template_executability_pool: String::new(),
            last_r1_template_executability_mint_in: String::new(),
            last_r1_template_executability_mint_out: String::new(),
            last_r1_template_executability_route: String::new(),
            last_r1_template_executability_output_ata: String::new(),
            last_admit_to_process_intent_drop_reason: String::new(),
            last_admit_to_process_intent_pool: String::new(),
            last_admit_to_process_intent_mint_in: String::new(),
            last_admit_to_process_intent_mint_out: String::new(),
            last_admit_to_process_intent_route: String::new(),
            last_admit_to_process_intent_source: String::new(),
            last_clmm_exact_quote_response_mismatch_reason: String::new(),
            last_clmm_exact_quote_response_pool_index: 0,
            last_clmm_exact_quote_response_sequence: 0,
            last_clmm_exact_quote_response_request_id_expected: 0,
            last_clmm_exact_quote_response_request_id_found: 0,
            last_clmm_exact_quote_response_pool_expected: String::new(),
            last_clmm_exact_quote_response_pool_found: String::new(),
            last_clmm_exact_quote_response_mint_in_expected: String::new(),
            last_clmm_exact_quote_response_mint_in_found: String::new(),
            last_clmm_exact_quote_response_mint_out_expected: String::new(),
            last_clmm_exact_quote_response_mint_out_found: String::new(),
            last_clmm_exact_quote_response_amount_expected: 0,
            last_clmm_exact_quote_response_amount_found: 0,
            last_clmm_exact_quote_response_validity_flags: 0,
            last_clmm_exact_quote_response_status: String::new(),
            last_clmm_exact_quote_response_status_code: 0,
            last_clmm_exact_quote_response_timeout_classification: String::new(),
            last_clmm_exact_quote_response_quote_epoch: 0,
            last_clmm_exact_quote_response_last_update_unix_ns: 0,
            r1_afxdp_private_grade_route_actionable: 128,
            r1_afxdp_private_grade_process_intent_entered: 128,
            r1_afxdp_private_grade_auto_admission_attempt: 128,
            r1_afxdp_private_grade_auto_admission_admit: 128,
            worker_signal_public_geyser: 0,
            worker_signal_external_geyser: 0,
            worker_signal_local_validator_geyser: 0,
            worker_signal_jito_searcher_api: 0,
            worker_signal_private_mempool: 10_000,
            worker_signal_harmonic_searcher_api: 0,
            worker_signal_afxdp: 0,
            worker_signal_unknown: 0,
            worker_source_probe_afxdp_parsed_shred_drops: 0,
            afxdp_frames_seen: 0,
            afxdp_udp_8000_8025_seen: 0,
            afxdp_xsk_rx_batches: 0,
            afxdp_shred_parse_ok: 0,
            afxdp_shred_parse_err: 0,
            afxdp_shred_parse_data_ok: 0,
            afxdp_shred_parse_coding_ok: 0,
            afxdp_feed_enqueue_ok: 0,
            afxdp_feed_enqueue_full: 0,
            afxdp_reassembly_insert_ok: 0,
            afxdp_reassembly_complete_entries: 0,
            afxdp_fec_recovered_entries: 0,
            afxdp_data_shred_payload_ok: 0,
            afxdp_tx_extract_attempt: 0,
            afxdp_tx_extract_ok: 0,
            afxdp_tx_extract_err: 0,
            afxdp_tx_extract_err_empty_payload: 0,
            afxdp_tx_extract_err_short_vec: 0,
            afxdp_tx_extract_err_bad_entry_header: 0,
            afxdp_tx_extract_err_bad_compact_u16: 0,
            afxdp_tx_extract_err_sanitize_failed: 0,
            afxdp_tx_extract_err_no_transactions: 0,
            afxdp_tx_extract_err_unsupported_format: 0,
            afxdp_tx_record_emit_attempt: 0,
            afxdp_tx_record_emit_ok: 0,
            afxdp_tx_record_emit_full: 0,
            afxdp_pipeline_drop_unsupported_program: 0,
            afxdp_pipeline_drop_allowlist_miss: 0,
            afxdp_pipeline_drop_decode_fail: 0,
            afxdp_pipeline_drop_route_fail: 0,
            afxdp_pipeline_drop_mint_fail: 0,
            afxdp_pipeline_drop_other: 0,
            afxdp_source_probe_emit_attempt: 0,
            afxdp_source_probe_emit_ok: 0,
            afxdp_source_probe_emit_full: 0,
            afxdp_source_probe_emit_err: 0,
            grpc_connected: 0,
            grpc_subscribe_sent: 0,
            grpc_updates_total: 0,
            grpc_updates_transaction: 0,
            grpc_updates_non_transaction: 0,
            grpc_normalize_ok: 0,
            grpc_normalize_err: 0,
            grpc_pipeline_drop_unsupported_program: 0,
            grpc_pipeline_drop_allowlist_miss: 0,
            grpc_pipeline_drop_decode_fail: 0,
            grpc_pipeline_drop_route_fail: 0,
            grpc_pipeline_drop_mint_fail: 0,
            grpc_pipeline_drop_other: 0,
            grpc_unsupported_program_top_0_id: String::new(),
            grpc_unsupported_program_top_0_count: 0,
            grpc_unsupported_program_top_1_id: String::new(),
            grpc_unsupported_program_top_1_count: 0,
            grpc_unsupported_program_top_2_id: String::new(),
            grpc_unsupported_program_top_2_count: 0,
            grpc_unsupported_program_top_3_id: String::new(),
            grpc_unsupported_program_top_3_count: 0,
            grpc_unsupported_program_top_4_id: String::new(),
            grpc_unsupported_program_top_4_count: 0,
            grpc_unsupported_program_top_5_id: String::new(),
            grpc_unsupported_program_top_5_count: 0,
            grpc_unsupported_program_top_6_id: String::new(),
            grpc_unsupported_program_top_6_count: 0,
            grpc_unsupported_program_top_7_id: String::new(),
            grpc_unsupported_program_top_7_count: 0,
            grpc_unsupported_program_top_8_id: String::new(),
            grpc_unsupported_program_top_8_count: 0,
            grpc_unsupported_program_top_9_id: String::new(),
            grpc_unsupported_program_top_9_count: 0,
            grpc_unsupported_program_top_10_id: String::new(),
            grpc_unsupported_program_top_10_count: 0,
            grpc_unsupported_program_top_11_id: String::new(),
            grpc_unsupported_program_top_11_count: 0,
            grpc_unsupported_program_top_12_id: String::new(),
            grpc_unsupported_program_top_12_count: 0,
            grpc_unsupported_program_top_13_id: String::new(),
            grpc_unsupported_program_top_13_count: 0,
            grpc_unsupported_program_top_14_id: String::new(),
            grpc_unsupported_program_top_14_count: 0,
            grpc_unsupported_program_top_15_id: String::new(),
            grpc_unsupported_program_top_15_count: 0,
            grpc_feed_enqueue_ok: 0,
            grpc_feed_enqueue_full: 0,
            grpc_feed_enqueue_err: 0,
            grpc_signal_source_local_validator_geyser: 0,
            worker_route_attempts: 10_000,
            worker_route_actionable: 10_000,
            worker_route_dropped: 0,
            worker_route_unavailable: 0,
            worker_route_unavailable_unknown_signal: 0,
            worker_route_unavailable_hydration_none: 0,
            worker_route_unavailable_hydration_none_afxdp: 0,
            worker_afxdp_hydration_request_enqueued: 0,
            worker_afxdp_hydration_request_deduped: 0,
            worker_route_reason_public_target: 0,
            worker_route_reason_stale_harmonic_no_jito: 0,
            worker_route_reason_quarantined_harmonic_no_jito: 0,
            worker_route_reason_tactic_mask_no_jito: 0,
            worker_route_reason_harmonic_egress_gate_no_jito: 0,
            worker_route_reason_over_cap: 0,
            worker_route_reason_no_viable_route: 0,
            worker_route_reason_atba_no_jito: 0,
            worker_route_reason_apex_partial_no_jito: 0,
            worker_route_reason_jito_egress_gate_fallback: 0,
            worker_route_reason_jito_stale_harmonic_reroute: 0,
            worker_route_reason_jito_quarantined_fallback: 0,
            worker_route_reason_jito_tactic_mask_fallback: 0,
            worker_route_reason_jito_direct: 10_000,
            worker_route_reason_jito_atba: 0,
            worker_route_reason_jito_apex_partial: 0,
            worker_route_reason_harmonic_fresh_plain: 0,
            worker_pipeline_attempts: 128,
            worker_pipeline_success: 128,
            worker_pipeline_err_routing_hydration: 0,
            worker_pipeline_err_blockhash_missing: 0,
            worker_pipeline_err_lookup: 0,
            worker_pipeline_err_lookup_pool_tag_missing: 0,
            worker_pipeline_err_lookup_pool_arena_missing: 0,
            worker_pipeline_err_lookup_pool_arena_torn: 0,
            worker_pipeline_err_lookup_pool_stale: 0,
            worker_pipeline_err_lookup_mint_in_tag_missing: 0,
            worker_pipeline_err_lookup_mint_in_arena_missing: 0,
            worker_pipeline_err_lookup_mint_in_arena_torn: 0,
            worker_pipeline_err_lookup_mint_out_tag_missing: 0,
            worker_pipeline_err_lookup_mint_out_arena_missing: 0,
            worker_pipeline_err_lookup_mint_out_arena_torn: 0,
            worker_pipeline_err_lookup_mint_stale: 0,
            worker_pipeline_err_lookup_pubkey_mismatch: 0,
            last_lookup_fail_reason: String::new(),
            last_lookup_fail_pool_pubkey: String::new(),
            last_lookup_fail_mint_in: String::new(),
            last_lookup_fail_mint_out: String::new(),
            last_lookup_fail_venue: 0,
            last_lookup_fail_pool_index: 0,
            last_lookup_fail_pool_last_update_unix_ns: 0,
            last_lookup_fail_record_unix_ns: 0,
            last_lookup_fail_pool_age_ns: 0,
            last_lookup_fail_max_pool_staleness_ns: 0,
            last_lookup_fail_health_snapshot_slot: 0,
            last_lookup_fail_pool_reserve_in: 0,
            last_lookup_fail_pool_reserve_out: 0,
            worker_pipeline_err_quote: 0,
            worker_pipeline_err_quote_clmm_arena_missing: 0,
            worker_pipeline_err_quote_clmm_snapshot_missing: 0,
            worker_pipeline_err_quote_clmm_pool_mismatch: 0,
            worker_pipeline_err_quote_clmm_mint_in_mismatch: 0,
            worker_pipeline_err_quote_clmm_mint_out_mismatch: 0,
            worker_pipeline_err_quote_clmm_amount_mismatch: 0,
            worker_pipeline_err_quote_clmm_direction_mismatch: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_valid_flag_missing: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_expected_out: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_min_out: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_min_out_exceeds_expected: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_last_update_ts: 0,
            worker_pipeline_err_quote_clmm_invalid_snapshot_zero_sqrt_price: 0,
            worker_pipeline_err_quote_clmm_no_quote_no_current_array_upstream: 0,
            worker_pipeline_err_quote_clmm_no_quote_no_liquidity_boundary: 0,
            worker_pipeline_err_quote_clmm_no_quote_out_of_range: 0,
            worker_pipeline_err_quote_clmm_no_quote_missing_tick_array_local: 0,
            worker_pipeline_err_quote_clmm_stale_snapshot: 0,
            worker_pipeline_err_quote_clmm_target_out_underflow: 0,
            last_clmm_invalid_quote_pool: String::new(),
            last_clmm_invalid_quote_mint_in: String::new(),
            last_clmm_invalid_quote_mint_out: String::new(),
            last_clmm_invalid_quote_amount_in: 0,
            last_clmm_invalid_quote_expected_out: 0,
            last_clmm_invalid_quote_min_out: 0,
            last_clmm_invalid_quote_projected_reserve_in: 0,
            last_clmm_invalid_quote_projected_reserve_out: 0,
            last_clmm_invalid_quote_quote_epoch: 0,
            last_clmm_invalid_quote_last_update_unix_ns: 0,
            last_clmm_invalid_quote_post_sqrt_price_x64: String::new(),
            last_clmm_invalid_quote_post_tick: 0,
            last_clmm_invalid_quote_crossed_tick_count: 0,
            last_clmm_invalid_quote_direction: 0,
            last_clmm_invalid_quote_validity_flags: 0,
            worker_pipeline_err_invalid_direction: 0,
            worker_pipeline_err_overlay_overflow: 0,
            worker_pipeline_err_no_send_sink_missing: 0,
            worker_pipeline_err_policy: 0,
            worker_pipeline_err_policy_apex_not_positive: 0,
            worker_pipeline_err_policy_atba_missing_dist: 0,
            worker_pipeline_err_policy_dominance_live: 0,
            worker_pipeline_err_policy_no_usable_lut: 0,
            worker_pipeline_err_policy_net_after_fee_floor: 0,
            worker_pipeline_err_policy_over_15_sol_live_input: 0,
            worker_pipeline_err_policy_fee_clamp_invalid: 0,
            worker_pipeline_err_policy_inventory_breach: 0,
            worker_pipeline_err_policy_other: 0,
            worker_pipeline_err_emit_invalid_guard_pubkey: 0,
            worker_pipeline_err_emit_invalid_template_for_route: 0,
            worker_pipeline_err_emit_tactic_not_allowed_on_route: 0,
            worker_pipeline_err_emit_oversized_payload: 0,
            worker_pipeline_err_emit_unsupported_additional_signer: 0,
            worker_pipeline_err_emit_sign: 0,
            worker_pipeline_err_emit_enqueue_full: 0,
            worker_pipeline_err_emit_deadline_exceeded: 0,
            worker_pipeline_err_emit_no_send_sink_full: 0,
            worker_pipeline_err_emit_build_other: 0,
            worker_pipeline_err_emit_other: 0,
            worker_pipeline_err_emit: 0,
            public_lane_amputation_ok: true,
            inventory_truth_ok: true,
            fee_leak_ok: true,
            fee_leak_accounted: true,
            phantom_inventory_risk: false,
            selective_death_clamp_ok: true,
            pre_sign_egress_gate_ok: true,
            private_lane_admissibility_ok: true,
            rollout_events_observed: 10_000,
            rollout_would_send: 128,
            rollout_live_send_suppressed: 128,
            rollout_private_lane_admissible_opportunities: 128,
            rollout_private_grade_admissible_opportunities: 128,
            rollout_geyser_shadow_opportunities: 0,
            rollout_afxdp_private_grade_would_send: 128,
            rollout_afxdp_private_grade_live_send_suppressed: 128,
            rollout_public_drops: 0,
            rollout_stale_state_reroutes: 0,
            rollout_egress_gate_reroutes: 0,
            rollout_egress_gate_drops: 0,
            rollout_clamped_apex_candidates: 0,
            rollout_atba_missing_dist_candidates: 0,
            rollout_jito_no_send_would_sends: 128,
            rollout_harmonic_no_send_would_sends: 0,
            rollout_public_rpc_no_send_would_sends: 0,
            rollout_drop_reason_none: 128,
            rollout_drop_reason_public_target: 0,
            rollout_drop_reason_apex_not_positive: 0,
            rollout_drop_reason_harmonic_stale_state: 0,
            rollout_drop_reason_harmonic_tactic_mask: 0,
            rollout_drop_reason_egress_gate_jito: 0,
            rollout_drop_reason_egress_gate_harmonic: 0,
            rollout_drop_reason_rerouted_pre_sign_egress_gate: 0,
            rollout_drop_reason_atba_no_dist_to_tick: 0,
            rollout_drop_reason_taylor_prefilter: 0,
            rollout_drop_reason_would_drop_dominance_gate: 0,
            rollout_drop_reason_dropped_dominance_gate: 0,
            rollout_drop_reason_no_usable_lut_bank: 0,
            rollout_drop_reason_no_shadow_guard: 0,
            rollout_drop_reason_rollout_cap: 0,
            rollout_drop_reason_rollout_no_send: 0,
            rollout_last_no_send_reason: 1,
            rollout_last_drop_reason: 0,
            latency_ok: true,
            worker_latency_proof_present: true,
            worker_latency_p99_ns: 4_140,
            worker_latency_budget_ns: 150_000,
            realized_pnl_ok: true,
            shadow_pnl_ok: true,
            competition_adjusted_ok: true,
            competition_adjusted_retained_ev_delta_lamports: 5_000,
            competition_adjusted_fee_burn_delta_lamports: -500,
            micro_market_uplift_ok: true,
            micro_market_observations: 128,
            micro_market_afxdp_private_grade_observations: 128,
            micro_market_banked_retained_ev_lamports: 25_000,
            micro_market_flat_baseline_retained_ev_lamports: 24_900,
            micro_market_delta_lamports: 100,
            dominance_shadow_pnl_ok: true,
            taylor_gate_ok: true,
            shadow_guard_deployed: true,
            shadow_guard_program_id_present: true,
            shadow_guard_mainnet_deployed: true,
            shadow_guard_template_program_id_matches: true,
            sparse_classes_fallback_safe: true,
            baseline_shadow_comparator_enabled: true,
            failback_gate_exists: true,
            tactical_lane: G12TacticalLaneEvidence::default(),
            additive_quant_tactics: G12AdditiveQuantTacticsEvidence {
                cu_density_metrics_visible: true,
                exhaust_metrics_visible: true,
                tick_boundary_shadow_only: true,
                tick_boundary_live_sizing_changes: 0,
                tick_boundary_live_routing_changes: 0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn green_inputs_enable_promotion_without_auto_switching() {
        let report = green_input().evaluate();

        assert!(report.public_scout_gates_green);
        assert!(report.all_gates_green);
        assert!(report.promote_allowed);
        assert!(!report.failback_required);
        assert!(!report.kill_switch_required);
        assert!(report.dominance_live_allowed);
        assert!(report.competition_adjusted_primary_allowed);
    }

    #[test]
    fn g12_readiness_is_source_coupled_to_afxdp_private_grade_actionability() {
        let mut input = green_input();
        input.r1_afxdp_private_grade_route_actionable = 0;
        input.r1_afxdp_private_grade_process_intent_entered = 0;
        input.rollout_afxdp_private_grade_would_send = 0;
        input.micro_market_afxdp_private_grade_observations = 0;
        input.worker_route_actionable = 10_000;
        input.worker_candidates_entered_process_intent = 128;
        input.rollout_would_send = 128;
        input.micro_market_observations = 128;

        let report = input.evaluate();

        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
        assert_eq!(report.rollout_would_send, 128);
        assert_eq!(report.micro_market_observations, 128);
        assert_eq!(report.r1_afxdp_private_grade_route_actionable, 0);
    }

    #[test]
    fn afxdp_raw_counters_are_carried_into_report() {
        let mut input = green_input();
        input.worker_source_probe_afxdp_parsed_shred_drops = 6;
        input.afxdp_frames_seen = 21;
        input.afxdp_udp_8000_8025_seen = 18;
        input.afxdp_xsk_rx_batches = 3;
        input.afxdp_shred_parse_ok = 17;
        input.afxdp_shred_parse_err = 1;
        input.afxdp_shred_parse_data_ok = 12;
        input.afxdp_shred_parse_coding_ok = 5;
        input.afxdp_feed_enqueue_ok = 9;
        input.afxdp_feed_enqueue_full = 2;
        input.afxdp_reassembly_insert_ok = 16;
        input.afxdp_reassembly_complete_entries = 11;
        input.afxdp_fec_recovered_entries = 3;
        input.afxdp_data_shred_payload_ok = 10;
        input.afxdp_tx_extract_attempt = 10;
        input.afxdp_tx_extract_ok = 6;
        input.afxdp_tx_extract_err = 4;
        input.afxdp_tx_extract_err_empty_payload = 1;
        input.afxdp_tx_extract_err_short_vec = 2;
        input.afxdp_tx_extract_err_bad_entry_header = 3;
        input.afxdp_tx_extract_err_bad_compact_u16 = 4;
        input.afxdp_tx_extract_err_sanitize_failed = 5;
        input.afxdp_tx_extract_err_no_transactions = 6;
        input.afxdp_tx_extract_err_unsupported_format = 7;
        input.afxdp_tx_record_emit_attempt = 5;
        input.afxdp_tx_record_emit_ok = 4;
        input.afxdp_tx_record_emit_full = 1;
        input.afxdp_pipeline_drop_unsupported_program = 11;
        input.afxdp_pipeline_drop_allowlist_miss = 12;
        input.afxdp_pipeline_drop_decode_fail = 13;
        input.afxdp_pipeline_drop_route_fail = 14;
        input.afxdp_pipeline_drop_mint_fail = 15;
        input.afxdp_pipeline_drop_other = 16;
        input.afxdp_source_probe_emit_attempt = 5;
        input.afxdp_source_probe_emit_ok = 4;
        input.afxdp_source_probe_emit_full = 1;
        input.afxdp_source_probe_emit_err = 0;

        let report = input.evaluate();

        assert_eq!(report.worker_source_probe_afxdp_parsed_shred_drops, 6);
        assert_eq!(report.afxdp_frames_seen, 21);
        assert_eq!(report.afxdp_udp_8000_8025_seen, 18);
        assert_eq!(report.afxdp_xsk_rx_batches, 3);
        assert_eq!(report.afxdp_shred_parse_ok, 17);
        assert_eq!(report.afxdp_shred_parse_err, 1);
        assert_eq!(report.afxdp_shred_parse_data_ok, 12);
        assert_eq!(report.afxdp_shred_parse_coding_ok, 5);
        assert_eq!(report.afxdp_feed_enqueue_ok, 9);
        assert_eq!(report.afxdp_feed_enqueue_full, 2);
        assert_eq!(report.afxdp_reassembly_insert_ok, 16);
        assert_eq!(report.afxdp_reassembly_complete_entries, 11);
        assert_eq!(report.afxdp_fec_recovered_entries, 3);
        assert_eq!(report.afxdp_data_shred_payload_ok, 10);
        assert_eq!(report.afxdp_tx_extract_attempt, 10);
        assert_eq!(report.afxdp_tx_extract_ok, 6);
        assert_eq!(report.afxdp_tx_extract_err, 4);
        assert_eq!(report.afxdp_tx_extract_err_empty_payload, 1);
        assert_eq!(report.afxdp_tx_extract_err_short_vec, 2);
        assert_eq!(report.afxdp_tx_extract_err_bad_entry_header, 3);
        assert_eq!(report.afxdp_tx_extract_err_bad_compact_u16, 4);
        assert_eq!(report.afxdp_tx_extract_err_sanitize_failed, 5);
        assert_eq!(report.afxdp_tx_extract_err_no_transactions, 6);
        assert_eq!(report.afxdp_tx_extract_err_unsupported_format, 7);
        assert_eq!(report.afxdp_tx_record_emit_attempt, 5);
        assert_eq!(report.afxdp_tx_record_emit_ok, 4);
        assert_eq!(report.afxdp_tx_record_emit_full, 1);
        assert_eq!(report.afxdp_pipeline_drop_unsupported_program, 11);
        assert_eq!(report.afxdp_pipeline_drop_allowlist_miss, 12);
        assert_eq!(report.afxdp_pipeline_drop_decode_fail, 13);
        assert_eq!(report.afxdp_pipeline_drop_route_fail, 14);
        assert_eq!(report.afxdp_pipeline_drop_mint_fail, 15);
        assert_eq!(report.afxdp_pipeline_drop_other, 16);
        assert_eq!(report.afxdp_source_probe_emit_attempt, 5);
        assert_eq!(report.afxdp_source_probe_emit_ok, 4);
        assert_eq!(report.afxdp_source_probe_emit_full, 1);
        assert_eq!(report.afxdp_source_probe_emit_err, 0);
    }

    #[test]
    fn missing_shadow_guard_preflight_blocks_promotion_but_not_gate_green() {
        let mut input = green_input();
        input.shadow_guard_mainnet_deployed = false;
        input.shadow_guard_deployed = false;

        let report = input.evaluate();

        assert!(report.all_gates_green);
        assert!(!report.promote_allowed);
        assert!(!report.shadow_guard_mainnet_deployed);
        assert!(!report.competition_adjusted_primary_allowed);
    }

    #[test]
    fn competition_adjusted_promotion_requires_non_negative_uplift_and_non_worse_fee_burn() {
        let mut input = green_input();
        input.competition_adjusted_retained_ev_delta_lamports = -1;
        input.competition_adjusted_fee_burn_delta_lamports = 1;

        let report = input.evaluate();

        assert!(!report.competition_adjusted_ok);
        assert!(report.failback_required);
        assert!(!report.promote_allowed);
        assert!(!report.competition_adjusted_primary_allowed);
    }

    #[test]
    fn competition_adjusted_promotion_requires_baseline_shadow_and_failback_path() {
        let mut input = green_input();
        input.baseline_shadow_comparator_enabled = false;
        input.failback_gate_exists = false;

        let report = input.evaluate();

        assert!(!report.competition_adjusted_ok);
        assert!(!report.baseline_shadow_comparator_enabled);
        assert!(!report.failback_gate_exists);
        assert!(report.failback_required);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn tactical_lane_violation_requires_kill_switch() {
        let mut input = green_input();
        input.tactical_lane.harmonic_exhaust_scavenge_routes = 1;

        let report = input.evaluate();

        assert!(!report.tactical_lane_ok);
        assert!(report.kill_switch_required);
        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn additive_quant_violation_requires_failback_not_kill() {
        let mut input = green_input();
        input.live_landed = 5;
        input.min_live_landed = 1;
        input
            .additive_quant_tactics
            .tick_boundary_live_routing_changes = 1;

        let report = input.evaluate();

        assert!(!report.additive_quant_tactics_ok);
        assert!(report.failback_required);
        assert!(!report.kill_switch_required);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn insufficient_evidence_prevents_promotion_but_not_failback_in_observe_only() {
        let mut input = green_input();
        input.sample_slots = 9_999;

        let report = input.evaluate();

        assert!(report.public_scout_gates_green);
        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
        assert!(!report.failback_required);
        assert!(!report.kill_switch_required);
    }

    #[test]
    fn public_scout_green_blocks_on_inventory_or_fee_truth() {
        let mut input = green_input();
        input.inventory_truth_ok = false;
        input.fee_leak_accounted = false;

        let report = input.evaluate();

        assert!(!report.public_scout_gates_green);
        assert!(!report.all_gates_green);
    }

    #[test]
    fn json_round_trip_writes_startup_readiness_fields() {
        let dir = tempfile::tempdir().unwrap();
        let input_path = dir.path().join("gate_input.json");
        let output_path = dir.path().join("run").join("g12_gate_report.json");
        write_gate_evaluator_input(&input_path, &green_input()).unwrap();

        let report = evaluate_gate_report_from_json(&input_path, Some(&output_path)).unwrap();
        let written = std::fs::read_to_string(output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();

        assert!(report.all_gates_green);
        assert_eq!(
            parsed["shadow_guard_deployed"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["shadow_guard_program_id_present"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["shadow_guard_mainnet_deployed"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["shadow_guard_template_program_id_matches"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["dominance_live_allowed"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["competition_adjusted_primary_allowed"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["baseline_shadow_comparator_enabled"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parsed["failback_gate_exists"],
            serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn g12_gate_input_writer_round_trips_json() {
        let dir = tempfile::tempdir().unwrap();
        let input_path = dir.path().join("run").join("g12_gate_input.json");
        let input = green_input();

        write_gate_evaluator_input(&input_path, &input).unwrap();

        let parsed = read_gate_evaluator_input(&input_path).unwrap();
        assert_eq!(parsed, input);
    }

    #[test]
    fn g12_gate_report_blocks_promotion_on_public_send() {
        let mut input = green_input();
        input.public_lane_amputation_ok = false;

        let report = input.evaluate();

        assert!(!report.public_lane_amputation_ok);
        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_blocks_promotion_on_inventory_mismatch() {
        let mut input = green_input();
        input.inventory_truth_ok = false;

        let report = input.evaluate();

        assert!(!report.inventory_truth_ok);
        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_blocks_promotion_on_fee_leak() {
        let mut input = green_input();
        input.fee_leak_ok = false;

        let report = input.evaluate();

        assert!(!report.fee_leak_ok);
        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_blocks_promotion_on_latency_regression() {
        let mut input = green_input();
        input.latency_ok = false;

        let report = input.evaluate();

        assert!(!report.latency_ok);
        assert!(report.failback_required);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_blocks_competition_primary_negative_delta() {
        let mut input = green_input();
        input.competition_adjusted_retained_ev_delta_lamports = -1;

        let report = input.evaluate();

        assert!(!report.competition_adjusted_ok);
        assert!(!report.competition_adjusted_primary_allowed);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_blocks_dominance_live_negative_shadow_pnl() {
        let mut input = green_input();
        input.dominance_shadow_pnl_ok = false;

        let report = input.evaluate();

        assert!(!report.dominance_shadow_pnl_ok);
        assert!(!report.dominance_live_allowed);
        assert!(!report.promote_allowed);
    }

    #[test]
    fn g12_gate_report_all_green_allows_next_rung() {
        let report = green_input().evaluate();

        assert!(report.all_gates_green);
        assert!(report.promote_allowed);
        assert!(!report.failback_required);
        assert!(!report.kill_switch_required);
    }
}

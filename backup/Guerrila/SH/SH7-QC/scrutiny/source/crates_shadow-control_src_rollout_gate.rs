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
    pub worker_health_drops: u64,
    #[serde(default)]
    pub worker_stale_slot_drops: u64,
    #[serde(default)]
    pub worker_pool_state_miss: u64,
    #[serde(default)]
    pub worker_pool_state_hit: u64,
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
    pub worker_quote_ok: u64,
    #[serde(default)]
    pub worker_quote_err: u64,
    #[serde(default)]
    pub worker_route_resolved_jito: u64,
    #[serde(default)]
    pub worker_route_resolved_harmonic: u64,
    #[serde(default)]
    pub worker_route_resolved_drop: u64,
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
    pub worker_signal_unknown: u64,
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
    pub worker_pipeline_err_lookup: u64,
    #[serde(default)]
    pub worker_pipeline_err_quote: u64,
    #[serde(default)]
    pub worker_pipeline_err_invalid_direction: u64,
    #[serde(default)]
    pub worker_pipeline_err_overlay_overflow: u64,
    #[serde(default)]
    pub worker_pipeline_err_no_send_sink_missing: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_apex_not_positive: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_atba_missing_dist: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_dominance_live: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_no_usable_lut: u64,
    #[serde(default)]
    pub worker_pipeline_err_policy_other: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_invalid_guard_pubkey: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_build_other: u64,
    #[serde(default)]
    pub worker_pipeline_err_emit_other: u64,

    #[serde(default)]
    pub public_lane_amputation_ok: bool,
    #[serde(default)]
    pub inventory_truth_ok: bool,
    #[serde(default)]
    pub fee_leak_ok: bool,
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
            worker_health_drops: 0,
            worker_stale_slot_drops: 0,
            worker_pool_state_miss: 0,
            worker_pool_state_hit: 0,
            worker_shadow_events_published: 0,
            worker_rollout_would_send_events_published: 0,
            worker_rollout_live_send_suppressed_events_published: 0,
            worker_candidates_entered_process_intent: 0,
            worker_candidates_dropped_before_process_intent: 0,
            worker_process_intent_ok: 0,
            worker_process_intent_err: 0,
            worker_quote_ok: 0,
            worker_quote_err: 0,
            worker_route_resolved_jito: 0,
            worker_route_resolved_harmonic: 0,
            worker_route_resolved_drop: 0,
            worker_presign_gate_reject: 0,
            worker_template_build_ok: 0,
            worker_template_build_err: 0,
            worker_no_send_backend_called: 0,
            worker_no_send_backend_ok: 0,
            worker_no_send_backend_err: 0,
            worker_final_no_send_event_constructed: 0,
            worker_final_no_send_event_published: 0,
            worker_final_no_send_event_publish_err: 0,
            worker_final_no_send_drop_reason_last: 0,
            worker_geyser_shadow_no_send_admitted: 0,
            worker_geyser_shadow_no_send_rejected: 0,
            worker_geyser_shadow_no_send_source_external: 0,
            worker_geyser_shadow_no_send_source_local_validator: 0,
            worker_signal_public_geyser: 0,
            worker_signal_external_geyser: 0,
            worker_signal_local_validator_geyser: 0,
            worker_signal_jito_searcher_api: 0,
            worker_signal_private_mempool: 0,
            worker_signal_harmonic_searcher_api: 0,
            worker_signal_afxdp: 0,
            worker_signal_unknown: 0,
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
            worker_pipeline_err_lookup: 0,
            worker_pipeline_err_quote: 0,
            worker_pipeline_err_invalid_direction: 0,
            worker_pipeline_err_overlay_overflow: 0,
            worker_pipeline_err_no_send_sink_missing: 0,
            worker_pipeline_err_policy_apex_not_positive: 0,
            worker_pipeline_err_policy_atba_missing_dist: 0,
            worker_pipeline_err_policy_dominance_live: 0,
            worker_pipeline_err_policy_no_usable_lut: 0,
            worker_pipeline_err_policy_other: 0,
            worker_pipeline_err_emit_invalid_guard_pubkey: 0,
            worker_pipeline_err_emit_build_other: 0,
            worker_pipeline_err_emit_other: 0,
            public_lane_amputation_ok: false,
            inventory_truth_ok: false,
            fee_leak_ok: false,
            selective_death_clamp_ok: false,
            pre_sign_egress_gate_ok: false,
            private_lane_admissibility_ok: false,
            rollout_events_observed: 0,
            rollout_would_send: 0,
            rollout_live_send_suppressed: 0,
            rollout_private_lane_admissible_opportunities: 0,
            rollout_private_grade_admissible_opportunities: 0,
            rollout_geyser_shadow_opportunities: 0,
            rollout_public_drops: 0,
            rollout_stale_state_reroutes: 0,
            rollout_egress_gate_reroutes: 0,
            rollout_egress_gate_drops: 0,
            rollout_clamped_apex_candidates: 0,
            rollout_atba_missing_dist_candidates: 0,
            rollout_jito_no_send_would_sends: 0,
            rollout_harmonic_no_send_would_sends: 0,
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
            realized_pnl_ok: false,
            shadow_pnl_ok: false,
            competition_adjusted_ok: false,
            competition_adjusted_retained_ev_delta_lamports: 0,
            competition_adjusted_fee_burn_delta_lamports: 0,
            micro_market_uplift_ok: false,
            micro_market_observations: 0,
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
            || !additive_quant_tactics_ok;

        let evidence_window_ok = self.sample_floor_ok();
        let kill_switch_required = hard_fail;
        let failback_required =
            !kill_switch_required && (soft_fail || (!evidence_window_ok && self.live_landed > 0));

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
            worker_rollout_would_send_events_published: self
                .worker_rollout_would_send_events_published,
            worker_rollout_live_send_suppressed_events_published: self
                .worker_rollout_live_send_suppressed_events_published,
            worker_candidates_entered_process_intent: self.worker_candidates_entered_process_intent,
            worker_candidates_dropped_before_process_intent: self
                .worker_candidates_dropped_before_process_intent,
            worker_process_intent_ok: self.worker_process_intent_ok,
            worker_process_intent_err: self.worker_process_intent_err,
            worker_quote_ok: self.worker_quote_ok,
            worker_quote_err: self.worker_quote_err,
            worker_route_resolved_jito: self.worker_route_resolved_jito,
            worker_route_resolved_harmonic: self.worker_route_resolved_harmonic,
            worker_route_resolved_drop: self.worker_route_resolved_drop,
            worker_presign_gate_reject: self.worker_presign_gate_reject,
            worker_template_build_ok: self.worker_template_build_ok,
            worker_template_build_err: self.worker_template_build_err,
            worker_no_send_backend_called: self.worker_no_send_backend_called,
            worker_no_send_backend_ok: self.worker_no_send_backend_ok,
            worker_no_send_backend_err: self.worker_no_send_backend_err,
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
            worker_signal_external_geyser: self.worker_signal_external_geyser,
            worker_signal_local_validator_geyser: self.worker_signal_local_validator_geyser,
            worker_signal_afxdp: self.worker_signal_afxdp,
            worker_signal_unknown: self.worker_signal_unknown,
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
            grpc_feed_enqueue_ok: self.grpc_feed_enqueue_ok,
            grpc_feed_enqueue_full: self.grpc_feed_enqueue_full,
            grpc_feed_enqueue_err: self.grpc_feed_enqueue_err,
            grpc_signal_source_local_validator_geyser: self
                .grpc_signal_source_local_validator_geyser,
            public_lane_amputation_ok: self.public_lane_amputation_ok,
            inventory_truth_ok: self.inventory_truth_ok,
            fee_leak_ok: self.fee_leak_ok,
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
            rollout_jito_no_send_would_sends: self.rollout_jito_no_send_would_sends,
            rollout_harmonic_no_send_would_sends: self.rollout_harmonic_no_send_would_sends,
            rollout_last_no_send_reason: self.rollout_last_no_send_reason,
            rollout_last_drop_reason: self.rollout_last_drop_reason,
            latency_ok: self.latency_ok,
            worker_latency_proof_present: self.worker_latency_proof_present,
            worker_latency_p99_ns: self.worker_latency_p99_ns,
            worker_latency_budget_ns: self.worker_latency_budget_ns,
            realized_pnl_ok: self.realized_pnl_ok,
            shadow_pnl_ok: self.shadow_pnl_ok,
            competition_adjusted_ok: competition_adjusted_promotion_ok,
            competition_adjusted_retained_ev_delta_lamports: self
                .competition_adjusted_retained_ev_delta_lamports,
            competition_adjusted_fee_burn_delta_lamports: self
                .competition_adjusted_fee_burn_delta_lamports,
            micro_market_uplift_ok: self.micro_market_uplift_ok,
            micro_market_observations: self.micro_market_observations,
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
    pub worker_rollout_would_send_events_published: u64,
    pub worker_rollout_live_send_suppressed_events_published: u64,
    pub worker_candidates_entered_process_intent: u64,
    pub worker_candidates_dropped_before_process_intent: u64,
    pub worker_process_intent_ok: u64,
    pub worker_process_intent_err: u64,
    pub worker_quote_ok: u64,
    pub worker_quote_err: u64,
    pub worker_route_resolved_jito: u64,
    pub worker_route_resolved_harmonic: u64,
    pub worker_route_resolved_drop: u64,
    pub worker_presign_gate_reject: u64,
    pub worker_template_build_ok: u64,
    pub worker_template_build_err: u64,
    pub worker_no_send_backend_called: u64,
    pub worker_no_send_backend_ok: u64,
    pub worker_no_send_backend_err: u64,
    pub worker_final_no_send_event_constructed: u64,
    pub worker_final_no_send_event_published: u64,
    pub worker_final_no_send_event_publish_err: u64,
    pub worker_final_no_send_drop_reason_last: u64,
    pub worker_geyser_shadow_no_send_admitted: u64,
    pub worker_geyser_shadow_no_send_rejected: u64,
    pub worker_geyser_shadow_no_send_source_external: u64,
    pub worker_geyser_shadow_no_send_source_local_validator: u64,
    pub worker_signal_external_geyser: u64,
    pub worker_signal_local_validator_geyser: u64,
    pub worker_signal_afxdp: u64,
    pub worker_signal_unknown: u64,
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
    pub grpc_feed_enqueue_ok: u64,
    pub grpc_feed_enqueue_full: u64,
    pub grpc_feed_enqueue_err: u64,
    pub grpc_signal_source_local_validator_geyser: u64,

    pub public_lane_amputation_ok: bool,
    pub inventory_truth_ok: bool,
    pub fee_leak_ok: bool,
    pub selective_death_clamp_ok: bool,
    pub pre_sign_egress_gate_ok: bool,
    pub private_lane_admissibility_ok: bool,
    pub rollout_events_observed: u64,
    pub rollout_would_send: u64,
    pub rollout_live_send_suppressed: u64,
    pub rollout_private_lane_admissible_opportunities: u64,
    pub rollout_private_grade_admissible_opportunities: u64,
    pub rollout_geyser_shadow_opportunities: u64,
    pub rollout_jito_no_send_would_sends: u64,
    pub rollout_harmonic_no_send_would_sends: u64,
    pub rollout_last_no_send_reason: u16,
    pub rollout_last_drop_reason: u16,
    pub latency_ok: bool,
    pub worker_latency_proof_present: bool,
    pub worker_latency_p99_ns: u64,
    pub worker_latency_budget_ns: u64,
    pub realized_pnl_ok: bool,
    pub shadow_pnl_ok: bool,
    pub competition_adjusted_ok: bool,
    pub competition_adjusted_retained_ev_delta_lamports: i64,
    pub competition_adjusted_fee_burn_delta_lamports: i64,
    pub micro_market_uplift_ok: bool,
    pub micro_market_observations: u64,
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
            worker_health_drops: 0,
            worker_stale_slot_drops: 0,
            worker_pool_state_miss: 0,
            worker_pool_state_hit: 10_000,
            worker_shadow_events_published: 10_000,
            worker_rollout_would_send_events_published: 128,
            worker_rollout_live_send_suppressed_events_published: 128,
            worker_candidates_entered_process_intent: 128,
            worker_candidates_dropped_before_process_intent: 0,
            worker_process_intent_ok: 128,
            worker_process_intent_err: 0,
            worker_quote_ok: 128,
            worker_quote_err: 0,
            worker_route_resolved_jito: 10_000,
            worker_route_resolved_harmonic: 0,
            worker_route_resolved_drop: 0,
            worker_presign_gate_reject: 0,
            worker_template_build_ok: 128,
            worker_template_build_err: 0,
            worker_no_send_backend_called: 128,
            worker_no_send_backend_ok: 128,
            worker_no_send_backend_err: 0,
            worker_final_no_send_event_constructed: 128,
            worker_final_no_send_event_published: 128,
            worker_final_no_send_event_publish_err: 0,
            worker_final_no_send_drop_reason_last: 12,
            worker_geyser_shadow_no_send_admitted: 0,
            worker_geyser_shadow_no_send_rejected: 0,
            worker_geyser_shadow_no_send_source_external: 0,
            worker_geyser_shadow_no_send_source_local_validator: 0,
            worker_signal_public_geyser: 0,
            worker_signal_external_geyser: 0,
            worker_signal_local_validator_geyser: 0,
            worker_signal_jito_searcher_api: 0,
            worker_signal_private_mempool: 10_000,
            worker_signal_harmonic_searcher_api: 0,
            worker_signal_afxdp: 0,
            worker_signal_unknown: 0,
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
            worker_pipeline_err_lookup: 0,
            worker_pipeline_err_quote: 0,
            worker_pipeline_err_invalid_direction: 0,
            worker_pipeline_err_overlay_overflow: 0,
            worker_pipeline_err_no_send_sink_missing: 0,
            worker_pipeline_err_policy_apex_not_positive: 0,
            worker_pipeline_err_policy_atba_missing_dist: 0,
            worker_pipeline_err_policy_dominance_live: 0,
            worker_pipeline_err_policy_no_usable_lut: 0,
            worker_pipeline_err_policy_other: 0,
            worker_pipeline_err_emit_invalid_guard_pubkey: 0,
            worker_pipeline_err_emit_build_other: 0,
            worker_pipeline_err_emit_other: 0,
            public_lane_amputation_ok: true,
            inventory_truth_ok: true,
            fee_leak_ok: true,
            selective_death_clamp_ok: true,
            pre_sign_egress_gate_ok: true,
            private_lane_admissibility_ok: true,
            rollout_events_observed: 10_000,
            rollout_would_send: 128,
            rollout_live_send_suppressed: 128,
            rollout_private_lane_admissible_opportunities: 128,
            rollout_private_grade_admissible_opportunities: 128,
            rollout_geyser_shadow_opportunities: 0,
            rollout_public_drops: 0,
            rollout_stale_state_reroutes: 0,
            rollout_egress_gate_reroutes: 0,
            rollout_egress_gate_drops: 0,
            rollout_clamped_apex_candidates: 0,
            rollout_atba_missing_dist_candidates: 0,
            rollout_jito_no_send_would_sends: 128,
            rollout_harmonic_no_send_would_sends: 0,
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
        }
    }

    #[test]
    fn green_inputs_enable_promotion_without_auto_switching() {
        let report = green_input().evaluate();

        assert!(report.all_gates_green);
        assert!(report.promote_allowed);
        assert!(!report.failback_required);
        assert!(!report.kill_switch_required);
        assert!(report.dominance_live_allowed);
        assert!(report.competition_adjusted_primary_allowed);
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

        assert!(!report.all_gates_green);
        assert!(!report.promote_allowed);
        assert!(!report.failback_required);
        assert!(!report.kill_switch_required);
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

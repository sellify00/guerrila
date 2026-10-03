use std::fmt;
use std::path::{Path, PathBuf};

use shadow_control::rollout_gate::{
    evaluate_gate_report_from_input, write_gate_evaluator_input, G12AdditiveQuantTacticsEvidence,
    G12GateEvaluatorInput, G12TacticalLaneEvidence, RolloutGateError,
};
use shadow_core::config::{
    ApexPartialRouteMode, AtbaRouteMode, ExhaustScavengingRouteMode, RolloutCapRung, RolloutMode,
    ShadowConfig, ShadowStrategyMode, SparseClassAction, StrategyMode, TickBoundarySabotageMode,
};
use shadow_core::pm::pm_loop::PmLoopStats;
use shadow_ipc::egress_health::EgressHealthPayload32B;
use shadow_ipc::rollout_shadow::G12DropReason;

use crate::money_worker::snapshot_worker_diagnostics;

pub const ROLLOUT_GATE_EXPORT_INTERVAL_NS: u64 = 1_000_000_000;

const DEFAULT_PLACEHOLDER_PROGRAM_ID: &str = "11111111111111111111111111111111";

#[derive(Debug)]
pub enum RolloutGateExportError {
    Gate(RolloutGateError),
}

impl fmt::Display for RolloutGateExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gate(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RolloutGateExportError {}

impl From<RolloutGateError> for RolloutGateExportError {
    fn from(value: RolloutGateError) -> Self {
        Self::Gate(value)
    }
}

pub fn rollout_gate_input_path(report_path: &Path) -> PathBuf {
    report_path.with_file_name("g12_gate_input.json")
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
    let input = build_gate_evaluator_input(cfg, pm_stats, egress_health);
    write_gate_evaluator_input(&input_path, &input)?;
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
    let shadow_guard_preflight =
        read_shadow_guard_preflight(&proof_paths.shadow_guard_preflight_path);
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
        worker_health_drops: worker_diag.health_drops,
        worker_stale_slot_drops: worker_diag.stale_slot_drops,
        worker_pool_state_miss: worker_diag.pool_state_miss,
        worker_pool_state_hit: worker_diag.pool_state_hit,
        worker_shadow_events_published: worker_diag.shadow_events_published,
        worker_signal_public_geyser: worker_diag.signal_public_geyser,
        worker_signal_external_geyser: worker_diag.signal_external_geyser,
        worker_signal_local_validator_geyser: worker_diag.signal_local_validator_geyser,
        worker_signal_jito_searcher_api: worker_diag.signal_jito_searcher_api,
        worker_signal_private_mempool: worker_diag.signal_private_mempool,
        worker_signal_harmonic_searcher_api: worker_diag.signal_harmonic_searcher_api,
        worker_signal_afxdp: worker_diag.signal_afxdp,
        worker_signal_unknown: worker_diag.signal_unknown,
        worker_route_attempts: worker_diag.route_attempts,
        worker_route_actionable: worker_diag.route_actionable,
        worker_route_dropped: worker_diag.route_dropped,
        worker_route_unavailable: worker_diag.route_unavailable,
        worker_route_unavailable_unknown_signal: worker_diag.route_unavailable_unknown_signal,
        worker_route_unavailable_hydration_none: worker_diag.route_unavailable_hydration_none,
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
        worker_pipeline_err_lookup: worker_diag.pipeline_err_lookup,
        worker_pipeline_err_quote: worker_diag.pipeline_err_quote,
        worker_pipeline_err_invalid_direction: worker_diag.pipeline_err_invalid_direction,
        worker_pipeline_err_overlay_overflow: worker_diag.pipeline_err_overlay_overflow,
        worker_pipeline_err_no_send_sink_missing: worker_diag.pipeline_err_no_send_sink_missing,
        worker_pipeline_err_policy_apex_not_positive: worker_diag
            .pipeline_err_policy_apex_not_positive,
        worker_pipeline_err_policy_atba_missing_dist: worker_diag
            .pipeline_err_policy_atba_missing_dist,
        worker_pipeline_err_policy_dominance_live: worker_diag.pipeline_err_policy_dominance_live,
        worker_pipeline_err_policy_no_usable_lut: worker_diag.pipeline_err_policy_no_usable_lut,
        worker_pipeline_err_policy_other: worker_diag.pipeline_err_policy_other,
        worker_pipeline_err_emit_invalid_guard_pubkey: worker_diag
            .pipeline_err_emit_invalid_guard_pubkey,
        worker_pipeline_err_emit_build_other: worker_diag.pipeline_err_emit_build_other,
        worker_pipeline_err_emit_other: worker_diag.pipeline_err_emit_other,
        public_lane_amputation_ok: !shadow_core::routing::public_execution_reachable(),
        inventory_truth_ok: pm_stats
            .last_drift
            .map(|drift| drift.is_zero())
            .unwrap_or(false),
        fee_leak_ok: live_landed == 0,
        selective_death_clamp_ok: pm_stats.death_clamp.total_classified
            == pm_stats.death_clamp.guard_breaches
                + pm_stats.death_clamp.infra_failures
                + pm_stats.death_clamp.retail_reverts
            && pm_stats.death_clamp.tactic_bits_cleared <= pm_stats.death_clamp.guard_breaches
            && pm_stats.death_clamp.quarantines_raised <= pm_stats.death_clamp.infra_failures,
        pre_sign_egress_gate_ok: !egress_gate_enabled || egress_health.is_some(),
        private_lane_admissibility_ok,
        rollout_would_send: ledger.would_send,
        rollout_live_send_suppressed: ledger.live_send_suppressed,
        rollout_private_lane_admissible_opportunities: ledger.private_lane_admissible_opportunities,
        rollout_private_grade_admissible_opportunities: ledger
            .private_grade_admissible_opportunities,
        rollout_geyser_shadow_opportunities: ledger.geyser_shadow_opportunities,
        rollout_public_drops: ledger.public_drops,
        rollout_stale_state_reroutes: ledger.stale_state_reroutes,
        rollout_egress_gate_reroutes: ledger.egress_gate_reroutes,
        rollout_egress_gate_drops: ledger.egress_gate_drops,
        rollout_clamped_apex_candidates: ledger.clamped_apex_candidates,
        rollout_atba_missing_dist_candidates: ledger.atba_missing_dist_candidates,
        rollout_jito_no_send_would_sends: ledger.jito_no_send_would_sends,
        rollout_harmonic_no_send_would_sends: ledger.harmonic_no_send_would_sends,
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
        latency_ok: latency_proof.pass,
        worker_latency_proof_present: latency_proof.present,
        worker_latency_p99_ns: latency_proof.p99_ns,
        worker_latency_budget_ns: latency_proof.budget_ns,
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
                total_releases: 0,
                landed: 0,
                reverted: 0,
                expired: 0,
                send_failed: 0,
                nacked: 0,
                pm_timeout: 0,
                total_released_lamports: 0,
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
                jito_no_send_would_sends: 5,
                harmonic_no_send_would_sends: 0,
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
        let _lock = crate::money_worker::lock_worker_diagnostics_for_test();
        crate::money_worker::reset_worker_diagnostics_for_test();
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
        crate::money_worker::record_signal_source_diag_for_test(0xFF);
        crate::money_worker::record_route_reason_diag_for_test(
            shadow_core::routing::RouteReason::JitoDirect,
        );
        crate::money_worker::record_route_unavailable_diag_for_test(false);
        crate::money_worker::record_route_unavailable_diag_for_test(true);
        crate::money_worker::record_pipeline_success_for_test();
        crate::money_worker::record_pipeline_error_diag_for_test(
            &shadow_core::pipeline::PipelineError::LookupFailed,
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
        assert!(input.pre_sign_egress_gate_ok);
        assert!(input.private_lane_admissibility_ok);
        assert!(input.latency_ok);
        assert!(input.worker_latency_proof_present);
        assert_eq!(input.worker_latency_p99_ns, 4_140);
        assert_eq!(input.worker_latency_budget_ns, 150_000);
        assert_eq!(input.worker_signal_public_geyser, 1);
        assert_eq!(input.worker_signal_external_geyser, 1);
        assert_eq!(input.worker_signal_local_validator_geyser, 1);
        assert_eq!(input.worker_signal_private_mempool, 1);
        assert_eq!(input.worker_signal_unknown, 1);
        assert_eq!(input.worker_route_attempts, 3);
        assert_eq!(input.worker_route_actionable, 1);
        assert_eq!(input.worker_route_unavailable, 2);
        assert_eq!(input.worker_route_unavailable_unknown_signal, 1);
        assert_eq!(input.worker_route_unavailable_hydration_none, 1);
        assert_eq!(input.worker_route_reason_jito_direct, 1);
        assert_eq!(input.worker_pipeline_attempts, 3);
        assert_eq!(input.worker_pipeline_success, 1);
        assert_eq!(input.worker_pipeline_err_lookup, 1);
        assert_eq!(input.worker_pipeline_err_emit_other, 1);
        assert_eq!(input.rollout_public_drops, 3);
        assert_eq!(input.rollout_private_grade_admissible_opportunities, 4);
        assert_eq!(input.rollout_geyser_shadow_opportunities, 5);
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
}

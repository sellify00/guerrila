#!/usr/bin/env bash
# 66ctf_fresh_continuation_replay_window.sh - fresh same-window Continuation structural chain + selected replay

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTF - Fresh Continuation Replay Window"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66CTF_STEP:-66ctf_fresh_continuation_replay_window}"
WINDOW_SECONDS="${SHADOW_RUN66CTF_WINDOW_SECONDS:-300}"
FRESH_PREFIX="${SHADOW_RUN66CTF_FRESH_PREFIX:-${STEP}_fresh}"
RUN65_STEP="${SHADOW_RUN66CTF_RUN65_STEP:-${FRESH_PREFIX}_run65}"
RUN65_WINDOW_SECONDS="${SHADOW_RUN66CTF_RUN65_WINDOW_SECONDS:-$WINDOW_SECONDS}"
SKIP_RUN65="${SHADOW_RUN66CTF_SKIP_RUN65:-false}"
WATCHLIST_JSON="${SHADOW_RUN66CTF_WATCHLIST_JSON:-$REPO_ROOT/continuation/watchlist/latest_watchlist.json}"
ACTIVE_DISCOVERY_JSON="${SHADOW_RUN66CTF_ACTIVE_DISCOVERY_JSON:-$REPO_ROOT/continuation/active_discovery/latest_active_continuation_discovery.json}"
RUN80D_ROWS="${SHADOW_RUN66CTF_RUN80D_ROWS:-$REPO_ROOT/continuation/run80d/rows.jsonl}"
PERSISTENCE_JSON="${SHADOW_RUN66CTF_PERSISTENCE_JSON:-$REPO_ROOT/continuation/studies/hourly_persistence/study_summary.json}"
VALIDATION_MODE="${SHADOW_RUN66CTF_VALIDATION_MODE:-fresh_thesis_revalidation}"
RUN_STRICT_DIAGNOSTICS="${SHADOW_RUN66CTF_RUN_STRICT_DIAGNOSTICS:-true}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
SUMMARY_JSON="$RESULT_DIR/run66ctf_fresh_continuation_replay_window.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

bool_true() {
    case "${1:-}" in
        1|true|TRUE|True|yes|YES|Yes|on|ON|On) return 0 ;;
        *) return 1 ;;
    esac
}

run_step() {
    info "66CTF stage: $*"
    "$@"
}

if ! bool_true "$SKIP_RUN65"; then
    run_step env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN65_STEP="$RUN65_STEP" \
        SHADOW_RUN65_WINDOW_SECONDS="$RUN65_WINDOW_SECONDS" \
        SHADOW_DISABLE_LIVE_SEND=true \
        SHADOW_DISABLE_SENDBUNDLE=true \
        "$PACK_DIR/scripts/65_full_potential_continuous_shard_simulation.sh"
else
    info "Skipping fresh Run65; using existing $RESULTS_ROOT/$RUN65_STEP"
fi

RUN66Q_STEP="${FRESH_PREFIX}_66q"
RUN66Q2_STEP="${FRESH_PREFIX}_66q2"
RUN66R_RULE_STEP="${FRESH_PREFIX}_66_rule"
RUN66RE_STEP="${FRESH_PREFIX}_66re"
RUN66A_STEP="${FRESH_PREFIX}_66a"
RUN66R_OVERLAY_STEP="${FRESH_PREFIX}_66r_overlay"
RUN66R_ATA_STEP="${FRESH_PREFIX}_66r_ata"
RUN66T_STEP="${FRESH_PREFIX}_66t"
RUN66U_STEP="${FRESH_PREFIX}_66u"
RUN66V_STEP="${FRESH_PREFIX}_66v"
RUN66RI_STEP="${FRESH_PREFIX}_66ri"
RUN66RP_STEP="${FRESH_PREFIX}_66rp"
RUN66RT_STEP="${FRESH_PREFIX}_66rt"
RUN66RG2_STEP="${FRESH_PREFIX}_66rg2"
RUN66RG3_STEP="${FRESH_PREFIX}_66rg3"
RUN66RG4_STEP="${FRESH_PREFIX}_66rg4"
RUN66RG5_STEP="${FRESH_PREFIX}_66rg5"
RUN66RG6_STEP="${FRESH_PREFIX}_66rg6"
RUN66CT0_STEP="${FRESH_PREFIX}_66ct0"
RUN66CT1_STEP="${FRESH_PREFIX}_66ct1"
RUN66CTPM_STEP="${FRESH_PREFIX}_66ctpm2"
RUN66CTB_STEP="${FRESH_PREFIX}_66ctb"
RUN66CTA_STEP="${FRESH_PREFIX}_66cta"
RUN66CT2_STEP="${FRESH_PREFIX}_66ct2"
RUN66CT3_STEP="${FRESH_PREFIX}_66ct3"
RUN66CTH_STEP="${FRESH_PREFIX}_66cth"
RUN66CTV2_STEP="${FRESH_PREFIX}_66ctv2"
RUN66CTV_STEP="${FRESH_PREFIX}_66ctv"
RUN66CTP_STEP="${FRESH_PREFIX}_66ctp"
RUN66CTQ_STEP="${FRESH_PREFIX}_66ctq"
RUN66CTL_STEP="${FRESH_PREFIX}_66ctl"

run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66Q_STEP="$RUN66Q_STEP" SHADOW_RUN66Q_RUN65_STEP="$RUN65_STEP" "$PACK_DIR/scripts/66q_rebalancing_quote_minout_preservation.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66Q2_STEP="$RUN66Q2_STEP" SHADOW_RUN66Q2_RUN66Q_STEP="$RUN66Q_STEP" "$PACK_DIR/scripts/66q2_rebalancing_exact_quote_route_ev.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66R_STEP="$RUN66R_RULE_STEP" SHADOW_RUN66R_RUN65_STEP="$RUN65_STEP" SHADOW_RUN66R_RUN66Q_STEP="$RUN66Q_STEP" "$PACK_DIR/scripts/66_rebalancing_rule_proof.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RE_STEP="$RUN66RE_STEP" SHADOW_RUN66RE_SOURCE_ROWS="$RESULTS_ROOT/$RUN66R_RULE_STEP/run66_rebalancing_rule_rows.jsonl" "$PACK_DIR/scripts/66re_rebalancing_entry_exit_rule_proof.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66A_STEP="$RUN66A_STEP" SHADOW_RUN66A_RUN65_STEP="$RUN65_STEP" SHADOW_RUN66A_RUN66Q_STEP="$RUN66Q_STEP" SHADOW_RUN66A_RUN66Q2_STEP="$RUN66Q2_STEP" "$PACK_DIR/scripts/66a_rebalancing_account_ata_unlock_planner.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66R_OVERLAY_STEP="$RUN66R_OVERLAY_STEP" SHADOW_RUN66R_OVERLAY_Q2_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl" SHADOW_RUN66R_OVERLAY_RULE_ROWS="$RESULTS_ROOT/$RUN66R_RULE_STEP/run66_rebalancing_rule_rows.jsonl" SHADOW_RUN66R_OVERLAY_ATA_ROWS="$RESULTS_ROOT/$RUN66A_STEP/run66a_rebalancing_account_ata_unlock_rows.jsonl" "$PACK_DIR/scripts/66r_rebalancing_exact_positive_readiness_overlay.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66R_ATA_STEP="$RUN66R_ATA_STEP" SHADOW_RUN66R_ATA_SOURCE_ROWS="$RESULTS_ROOT/$RUN66R_OVERLAY_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl" "$PACK_DIR/scripts/66r_ata_only_convertible_planner.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66T_STEP="$RUN66T_STEP" SHADOW_RUN66T_SOURCE_ROWS="$RESULTS_ROOT/$RUN66R_ATA_STEP/run66r_ata_only_convertible_planner_rows.jsonl" "$PACK_DIR/scripts/66t_token2022_explicit_allow_research.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66U_STEP="$RUN66U_STEP" SHADOW_RUN66U_SOURCE_ROWS="$RESULTS_ROOT/$RUN66R_ATA_STEP/run66r_ata_only_convertible_planner_rows.jsonl" bash "$PACK_DIR/scripts/66u_ata_repeatability_aggregate.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66V_STEP="$RUN66V_STEP" SHADOW_RUN66V_RUN66R_OVERLAY_STEP="$RUN66R_OVERLAY_STEP" SHADOW_RUN66V_RUN66R_ATA_STEP="$RUN66R_ATA_STEP" SHADOW_RUN66V_RUN66T_STEP="$RUN66T_STEP" SHADOW_RUN66V_RUN66U_STEP="$RUN66U_STEP" "$PACK_DIR/scripts/66v_inline_ata_lifecycle_model.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RI_STEP="$RUN66RI_STEP" SHADOW_RUN66RI_OVERLAY_ROWS="$RESULTS_ROOT/$RUN66R_OVERLAY_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl" SHADOW_RUN66RI_INLINE_ROWS="$RESULTS_ROOT/$RUN66V_STEP/run66v_inline_ata_lifecycle_rows.jsonl" "$PACK_DIR/scripts/66ri_rebalancing_inline_ata_lifecycle.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RP_STEP="$RUN66RP_STEP" SHADOW_RUN66RP_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RI_STEP/run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl" SHADOW_RUN66RP_ATA_ROWS="$RESULTS_ROOT/$RUN66A_STEP/run66a_rebalancing_account_ata_unlock_rows.jsonl" "$PACK_DIR/scripts/66rp_rebalancing_policy_liquidity_gate.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RT_STEP="$RUN66RT_STEP" SHADOW_RUN66RT_INLINE_ROWS="$RESULTS_ROOT/$RUN66RI_STEP/run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl" SHADOW_RUN66RT_POLICY_ROWS="$RESULTS_ROOT/$RUN66RP_STEP/run66rp_rebalancing_policy_liquidity_gate_rows.jsonl" SHADOW_RUN66RT_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" "$PACK_DIR/scripts/66rt_rebalancing_template_nosend.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RG2_STEP="$RUN66RG2_STEP" SHADOW_RUN66RG2_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66RG2_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" SHADOW_RUN66RG2_Q2_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl" SHADOW_RUN66RG2_OVERLAY_ROWS="$RESULTS_ROOT/$RUN66R_OVERLAY_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl" SHADOW_RUN66RG2_TEMPLATE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" "$PACK_DIR/scripts/66rg2_rebalancing_owner_selector_instrumentation_repair.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RG3_STEP="$RUN66RG3_STEP" SHADOW_RUN66RG3_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66RG3_OWNER_ROWS="$RESULTS_ROOT/$RUN66RG2_STEP/owner_selector_instrumentation_rows.jsonl" SHADOW_RUN66RG3_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" SHADOW_RUN66RG3_Q2_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl" "$PACK_DIR/scripts/66rg3_rebalancing_pm_owner_lock_surface.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RG4_STEP="$RUN66RG4_STEP" SHADOW_RUN66RG4_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66RG4_OWNER_ROWS="$RESULTS_ROOT/$RUN66RG2_STEP/owner_selector_instrumentation_rows.jsonl" SHADOW_RUN66RG4_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" SHADOW_RUN66RG4_Q2_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl" SHADOW_RUN66RG4_OVERLAY_ROWS="$RESULTS_ROOT/$RUN66R_OVERLAY_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl" "$PACK_DIR/scripts/66rg4_rebalancing_restoration_pressure_measurement.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RG5_STEP="$RUN66RG5_STEP" SHADOW_RUN66RG5_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66RG5_OWNER_ROWS="$RESULTS_ROOT/$RUN66RG2_STEP/owner_selector_instrumentation_rows.jsonl" SHADOW_RUN66RG5_RESTORATION_ROWS="$RESULTS_ROOT/$RUN66RG4_STEP/run66rg4_rebalancing_restoration_pressure_measurement_rows.jsonl" "$PACK_DIR/scripts/66rg5_false_restoration_gate.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66RG6_STEP="$RUN66RG6_STEP" SHADOW_RUN66RG6_SOURCE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66RG6_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" SHADOW_RUN66RG6_FALSE_RESTORATION_ROWS="$RESULTS_ROOT/$RUN66RG5_STEP/run66rg5_false_restoration_gate_rows.jsonl" SHADOW_RUN66RG6_CANDIDATE_AUTOPSY_ROWS="$RESULTS_ROOT/${RUN65_STEP}_run55_run39/candidate_autopsy.jsonl" "$PACK_DIR/scripts/66rg6_rebalancing_false_restoration_threshold_audit.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CT0_STEP="$RUN66CT0_STEP" SHADOW_RUN66CT0_WATCHLIST_JSON="$WATCHLIST_JSON" SHADOW_RUN66CT0_ACTIVE_DISCOVERY_JSON="$ACTIVE_DISCOVERY_JSON" SHADOW_RUN66CT0_RUN80D_ROWS="$RUN80D_ROWS" SHADOW_RUN66CT0_PERSISTENCE_JSON="$PERSISTENCE_JSON" SHADOW_RUN66CT0_ROUTE_ROWS="$RESULTS_ROOT/$RUN66RG6_STEP/route_to_continuation_rows.jsonl" SHADOW_RUN66CT0_CANDIDATE_AUTOPSY_ROWS="$RESULTS_ROOT/${RUN65_STEP}_run55_run39/candidate_autopsy.jsonl" SHADOW_RUN66CT0_TEMPLATE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" SHADOW_RUN66CT0_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" "$PACK_DIR/scripts/66ct0_continuation_candidate_merger.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CT1_STEP="$RUN66CT1_STEP" SHADOW_RUN66CT1_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CT0_STEP/continuation_candidate_manifest.jsonl" "$PACK_DIR/scripts/66ct1_continuation_owner_eligibility.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTPM2_STEP="$RUN66CTPM_STEP" SHADOW_RUN66CTPM2_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CT1_STEP/run66ct1_continuation_owner_eligibility_rows.jsonl" SHADOW_RUN66CTPM2_CANDIDATE_AUTOPSY_ROWS="$RESULTS_ROOT/${RUN65_STEP}_run55_run39/candidate_autopsy.jsonl" "$PACK_DIR/scripts/66ctpm2_direct_continuation_pm_owner_lock_repair.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTB_STEP="$RUN66CTB_STEP" SHADOW_RUN66CTB_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CTPM_STEP/run66ctpm2_direct_continuation_pm_owner_lock_repair_rows.jsonl" SHADOW_RUN66CTB_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" "$PACK_DIR/scripts/66ctb_continuation_entry_exit_rule_carry_forward_repair.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTA_STEP="$RUN66CTA_STEP" SHADOW_RUN66CTA_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CTB_STEP/run66ctb_continuation_entry_exit_rule_carry_forward_repair_rows.jsonl" SHADOW_RUN66CTA_RULE_ROWS="$RESULTS_ROOT/$RUN66RE_STEP/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl" SHADOW_RUN66CTA_INLINE_ROWS="$RESULTS_ROOT/$RUN66RI_STEP/run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl" SHADOW_RUN66CTA_POLICY_ROWS="$RESULTS_ROOT/$RUN66RP_STEP/run66rp_rebalancing_policy_liquidity_gate_rows.jsonl" SHADOW_RUN66CTA_TEMPLATE_ROWS="$RESULTS_ROOT/$RUN66RT_STEP/run66rt_rebalancing_template_nosend_rows.jsonl" "$PACK_DIR/scripts/66cta_continuation_account_template_policy_autopsy.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CT2_STEP="$RUN66CT2_STEP" SHADOW_RUN66CT2_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CTA_STEP/run66cta_continuation_account_template_policy_autopsy_rows.jsonl" "$PACK_DIR/scripts/66ct2_continuation_execution_rule_proof.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CT3_STEP="$RUN66CT3_STEP" SHADOW_RUN66CT3_SOURCE_ROWS="$RESULTS_ROOT/$RUN66CT2_STEP/run66ct2_continuation_execution_rule_rows.jsonl" "$PACK_DIR/scripts/66ct3_continuation_markout_risk_validation.sh"
run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTH_STEP="$RUN66CTH_STEP" SHADOW_RUN66CTH_MERGER_ROWS="$RESULTS_ROOT/$RUN66CT0_STEP/continuation_candidate_manifest.jsonl" SHADOW_RUN66CTH_OWNER_ROWS="$RESULTS_ROOT/$RUN66CTPM_STEP/run66ctpm2_direct_continuation_pm_owner_lock_repair_rows.jsonl" SHADOW_RUN66CTH_EXECUTION_ROWS="$RESULTS_ROOT/$RUN66CT2_STEP/run66ct2_continuation_execution_rule_rows.jsonl" SHADOW_RUN66CTH_RISK_ROWS="$RESULTS_ROOT/$RUN66CT3_STEP/run66ct3_continuation_markout_risk_rows.jsonl" "$PACK_DIR/scripts/66cth_continuation_structural_hunt_readiness.sh"

PROPOSAL_ROWS="$RESULTS_ROOT/$RUN66CTH_STEP/proposal_eligible_continuation_rows.jsonl"
if [[ -s "$PROPOSAL_ROWS" ]]; then
    if [[ "$VALIDATION_MODE" == "fresh_thesis_revalidation" ]]; then
        run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTV2_STEP="$RUN66CTV2_STEP" SHADOW_RUN66CTV2_SOURCE_ROWS="$PROPOSAL_ROWS" "$PACK_DIR/scripts/66ctv2_continuation_fresh_runtime_pressure_recompute.sh"
        run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTV_STEP="$RUN66CTV_STEP" SHADOW_RUN66CTV_SOURCE_ROWS="$PROPOSAL_ROWS" SHADOW_RUN66CTV_RUNTIME_ROWS="$RESULTS_ROOT/$RUN66CTV2_STEP/continuation_fresh_runtime_pressure_rows.jsonl" SHADOW_RUN66CTV_ENRICH_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl:$RESULTS_ROOT/$RUN66Q_STEP/run66q_rebalancing_quote_minout_rows.jsonl" "$PACK_DIR/scripts/66ctv_continuation_fresh_thesis_revalidation.sh"
    fi
    if [[ "$VALIDATION_MODE" == "strict_identity_replay" ]] || bool_true "$RUN_STRICT_DIAGNOSTICS"; then
        run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTP_STEP="$RUN66CTP_STEP" SHADOW_RUN66CTP_SOURCE_ROWS="$PROPOSAL_ROWS" SHADOW_RUN66CTP_ENRICH_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl:$RESULTS_ROOT/$RUN66Q_STEP/run66q_rebalancing_quote_minout_rows.jsonl" SHADOW_RUN66CTP_MODE="strict_identity_replay" "$PACK_DIR/scripts/66ctp_continuation_selected_private_replay_nosend.sh"
        run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTQ_STEP="$RUN66CTQ_STEP" SHADOW_RUN66CTQ_REPLAY_ROWS="$RESULTS_ROOT/$RUN66CTP_STEP/selected_replay_rows.jsonl" SHADOW_RUN66CTQ_SOURCE_ROWS="$PROPOSAL_ROWS" "$PACK_DIR/scripts/66ctq_continuation_quote_drift_autopsy.sh"
        run_step env SHADOW_RESULTS_DIR="$RESULTS_ROOT" SHADOW_RUN66CTL_STEP="$RUN66CTL_STEP" SHADOW_RUN66CTL_AUTOPSY_ROWS="$RESULTS_ROOT/$RUN66CTQ_STEP/continuation_quote_drift_autopsy_rows.jsonl" SHADOW_RUN66CTL_SOURCE_ROWS="$PROPOSAL_ROWS" SHADOW_RUN66CTL_SURVIVAL_MODE="$VALIDATION_MODE" "$PACK_DIR/scripts/66ctl_continuation_decay_window_report.sh"
    fi
fi

python3 - "$RESULTS_ROOT" "$RESULT_DIR" "$INPUT_MANIFEST" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" \
    "$RUN65_STEP" "$RUN66RG6_STEP" "$RUN66CTH_STEP" "$RUN66CTPM_STEP" "$RUN66CTB_STEP" "$RUN66CTA_STEP" "$RUN66CTV2_STEP" "$RUN66CTV_STEP" "$RUN66CTP_STEP" "$RUN66CTQ_STEP" "$RUN66CTL_STEP" "$PROPOSAL_ROWS" "$VALIDATION_MODE" "$RUN_STRICT_DIAGNOSTICS" <<'PY'
import json
import sys
from pathlib import Path

(
    results_root,
    result_dir_arg,
    manifest_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    run65_step,
    rg6_step,
    cth_step,
    ctpm_step,
    ctb_step,
    cta_step,
    ctv2_step,
    ctv_step,
    ctp_step,
    ctq_step,
    ctl_step,
    proposal_rows_arg,
    validation_mode,
    strict_diagnostics_arg,
) = sys.argv[1:21]
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
results = Path(results_root)
proposal_rows = Path(proposal_rows_arg)
strict_diagnostics_enabled = strict_diagnostics_arg.lower() in {"1", "true", "yes", "on"}

def load_json(path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))

def count_jsonl(path):
    if not path.exists():
        return 0
    return sum(1 for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip())

cth = load_json(results / cth_step / "final_verdict.json")
ctpm = load_json(results / ctpm_step / "final_verdict.json")
ctb = load_json(results / ctb_step / "final_verdict.json")
cta = load_json(results / cta_step / "final_verdict.json")
ctv2 = load_json(results / ctv2_step / "final_verdict.json")
ctv = load_json(results / ctv_step / "final_verdict.json")
ctp = load_json(results / ctp_step / "final_verdict.json")
ctq = load_json(results / ctq_step / "final_verdict.json")
ctl = load_json(results / ctl_step / "final_verdict.json")
rg6 = load_json(results / rg6_step / "final_verdict.json")
proposal_count = count_jsonl(proposal_rows)
cth_rows_seen = int(cth.get("rows_seen") or 0)
fresh_runtime_ready = int(ctv2.get("fresh_runtime_ready_rows") or 0)
fresh_ready = int(ctv.get("fresh_thesis_ready_rows") or ctv.get("selected_private_replay_ready_rows") or 0)
replay_ready = int(ctp.get("selected_private_replay_ready_rows") or 0)
thesis_signal_source = str(ctv.get("thesis_signal_source") or ("carried_proposal_fields" if validation_mode == "fresh_thesis_revalidation" else "not_used_in_strict_identity_replay"))
proposal_grade_fresh_ready = int(ctv.get("proposal_grade_fresh_thesis_ready_rows") or 0)

def strict_parked_blocker(ctp_verdict, ctq_verdict):
    if replay_ready > 0:
        return "none"
    ctp_value = str(ctp_verdict or "")
    ctq_value = str(ctq_verdict or "")
    if ctp_value == "PARK_SELECTED_REPLAY_FRESH_QUOTE_DRIFTED" or ctq_value in {"ENTRY_IDENTITY_DRIFT", "ROUNDTRIP_TURNED_NEGATIVE", "DRIFTED_BEFORE_REPLAY"}:
        return "fresh_quote_identity_drift / negative fresh roundtrip"
    if ctp_value == "PARK_SELECTED_REPLAY_MARKOUT_NOT_PERSISTENT":
        return "markout_not_persistent"
    if ctp_value == "PARK_SELECTED_REPLAY_ADVERSE_EXCURSION":
        return "adverse_excursion_too_high"
    if ctp_value == "PARK_SELECTED_REPLAY_PM":
        return "pm_owner_lock_failed"
    if ctp_value == "PARK_SELECTED_REPLAY_EXIT_RULE":
        return "exit_rule_failed"
    if ctp_value == "PARK_SELECTED_REPLAY_EGRESS":
        return "egress_no_send_failed"
    if ctp_value == "PARK_SELECTED_REPLAY_ACCOUNT_TEMPLATE":
        return "account_or_template_drift"
    if proposal_count <= 0:
        return "no_proposal_eligible_rows"
    return "fresh_quote_identity_drift / negative fresh roundtrip"

def fresh_parked_blocker(ctv_verdict, ctv2_verdict, signal_source):
    runtime_value = str(ctv2_verdict or "")
    if signal_source == "fresh_runtime_recomputed":
        if runtime_value == "PARK_FRESH_RUNTIME_API_KEY_MISSING":
            return "fresh_runtime_api_key_missing"
        if runtime_value == "PARK_FRESH_RUNTIME_SIGNAL_FETCH_FAILED":
            return "fresh_runtime_signal_fetch_failed"
        if runtime_value in {"PARK_FRESH_RUNTIME_SIGNAL_INCOMPLETE", "PARK_FRESH_RUNTIME_SIGNAL_MISSING"}:
            return "fresh_runtime_signal_incomplete"
    value = str(ctv_verdict or "")
    if fresh_ready > 0:
        return "none"
    if value == "PARK_THESIS_DECAYED":
        return "thesis_decayed"
    if value == "PARK_ENTRY_QUOTE_NEGATIVE":
        return "fresh_entry_quote_negative"
    if value == "PARK_FRESH_NET_NEGATIVE_AFTER_COST":
        return "fresh_net_negative_after_cost"
    if value == "PARK_EXIT_NOT_READY":
        return "exit_not_ready"
    if value == "PARK_ADVERSE_EXCURSION":
        return "adverse_excursion_too_high"
    if value == "PARK_PM":
        return "pm_owner_lock_failed"
    if value == "PARK_ACCOUNT_OR_TEMPLATE":
        return "account_or_template_drift"
    if proposal_count <= 0:
        return "no_proposal_eligible_rows"
    return "fresh_entry_quote_negative"


def structural_cth_blocker(cth_verdict):
    value = str(cth_verdict or "")
    if value == "PARK_CONTINUATION_PM":
        return "pm_owner_lock_failed"
    if value in {"PARK_CONTINUATION_ACCOUNT_OR_TEMPLATE", "PARK_CONTINUATION_ACCOUNT_OR_POLICY"}:
        return "account_or_template_drift"
    if value == "PARK_CONTINUATION_ENTRY_RULE":
        return "entry_rule_failed"
    if value == "PARK_CONTINUATION_EXIT_RULE":
        return "exit_rule_failed"
    if value == "PARK_CONTINUATION_RISK":
        return "adverse_excursion_too_high"
    if value == "PARK_CONTINUATION_HABITAT_ONLY":
        return "no_structural_route_to_continuation_rows"
    if cth_rows_seen > 0:
        return "proposal_eligibility_blocked"
    return "no_proposal_eligible_rows"

if validation_mode == "fresh_thesis_revalidation":
    if fresh_ready > 0:
        status = (
            "CONTINUATION_FRESH_THESIS_READY_NO_SEND"
            if thesis_signal_source == "fresh_runtime_recomputed"
            else "CONTINUATION_FRESH_THESIS_READY_NO_SEND_CARRIED_SIGNAL"
        )
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = (
            "CURRENT_SELECTED_FRESH_THESIS_READY_NO_SEND"
            if thesis_signal_source == "fresh_runtime_recomputed"
            else "CURRENT_SELECTED_FRESH_THESIS_READY_NO_SEND_CARRIED_SIGNAL"
        )
    elif proposal_count <= 0 and cth_rows_seen > 0:
        status = "CONTINUATION_STRUCTURAL_HUNT_READY_PROPOSAL_BLOCKED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_FRESH_THESIS_NOT_REACHED"
    elif proposal_count <= 0:
        status = "PARK_FRESH_CONTINUATION_NO_PROPOSAL_ELIGIBLE_ROWS"
        continuation_state = "STRUCTURAL_HUNT_NOT_READY"
        current_live_track_state = "CURRENT_SELECTED_FRESH_THESIS_NOT_REACHED"
    elif ctv:
        status = "CONTINUATION_CURRENT_FRESH_THESIS_PARKED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_FRESH_THESIS_PARKED"
    else:
        status = "PARK_FRESH_CONTINUATION_REVALIDATION_NOT_REACHED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_FRESH_THESIS_NOT_REACHED"
    current_live_track_blocker = (
        fresh_parked_blocker(ctv.get("final_verdict"), ctv2.get("final_verdict"), thesis_signal_source)
        if ctv
        else structural_cth_blocker(cth.get("final_verdict"))
        if proposal_count <= 0 and cth_rows_seen > 0
        else fresh_parked_blocker(ctv.get("final_verdict"), ctv2.get("final_verdict"), thesis_signal_source)
    )
    current_selected_replay_state = (
        "CURRENT_SELECTED_REPLAY_READY_NO_SEND" if replay_ready > 0 else
        "CURRENT_SELECTED_REPLAY_PARKED" if ctp else
        "CURRENT_SELECTED_REPLAY_NOT_RUN" if not strict_diagnostics_enabled else
        "CURRENT_SELECTED_REPLAY_NOT_REACHED"
    )
    current_selected_replay_blocker = strict_parked_blocker(ctp.get("final_verdict"), ctq.get("final_verdict")) if strict_diagnostics_enabled else "strict_identity_replay_not_run"
else:
    if replay_ready > 0:
        status = "CONTINUATION_SELECTED_REPLAY_READY_NO_SEND"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_REPLAY_READY_NO_SEND"
    elif proposal_count <= 0 and cth_rows_seen > 0:
        status = "CONTINUATION_STRUCTURAL_HUNT_READY_REPLAY_BLOCKED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_REPLAY_NOT_REACHED"
    elif proposal_count <= 0:
        status = "PARK_FRESH_CONTINUATION_NO_PROPOSAL_ELIGIBLE_ROWS"
        continuation_state = "STRUCTURAL_HUNT_NOT_READY"
        current_live_track_state = "CURRENT_SELECTED_REPLAY_NOT_REACHED"
    elif ctp:
        status = "CONTINUATION_CURRENT_SELECTED_REPLAY_PARKED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_REPLAY_PARKED"
    else:
        status = "PARK_FRESH_CONTINUATION_REPLAY_NOT_REACHED"
        continuation_state = "STRUCTURAL_HUNT_READY"
        current_live_track_state = "CURRENT_SELECTED_REPLAY_NOT_REACHED"
    current_live_track_blocker = (
        strict_parked_blocker(ctp.get("final_verdict"), ctq.get("final_verdict"))
        if ctp
        else structural_cth_blocker(cth.get("final_verdict"))
        if proposal_count <= 0 and cth_rows_seen > 0
        else strict_parked_blocker(ctp.get("final_verdict"), ctq.get("final_verdict"))
    )
    current_selected_replay_state = current_live_track_state
    current_selected_replay_blocker = current_live_track_blocker

summary = {
    "status": status,
    "validation_mode": validation_mode,
    "strict_diagnostics_enabled": strict_diagnostics_enabled,
    "thesis_signal_source": thesis_signal_source,
    "continuation_state": continuation_state,
    "current_live_track_state": current_live_track_state,
    "current_live_track_blocker": current_live_track_blocker,
    "current_selected_fresh_thesis_state": current_live_track_state if validation_mode == "fresh_thesis_revalidation" else "CURRENT_SELECTED_FRESH_THESIS_NOT_USED",
    "current_selected_fresh_thesis_blocker": current_live_track_blocker if validation_mode == "fresh_thesis_revalidation" else "strict_identity_replay_mode",
    "proposal_grade_fresh_thesis_state": (
        "PROPOSAL_GRADE_FRESH_THESIS_READY_NO_SEND"
        if validation_mode == "fresh_thesis_revalidation" and proposal_grade_fresh_ready > 0 and thesis_signal_source == "fresh_runtime_recomputed"
        else "PROPOSAL_GRADE_NOT_PROVEN_CARRIED_SIGNAL"
        if validation_mode == "fresh_thesis_revalidation" and thesis_signal_source != "fresh_runtime_recomputed"
        else "PROPOSAL_GRADE_NOT_EVALUATED"
        if validation_mode != "fresh_thesis_revalidation"
        else "PROPOSAL_GRADE_NOT_READY"
    ),
    "proposal_grade_fresh_thesis_blocker": (
        "none"
        if validation_mode == "fresh_thesis_revalidation" and proposal_grade_fresh_ready > 0 and thesis_signal_source == "fresh_runtime_recomputed"
        else "fresh_runtime_pressure_recompute_missing"
        if validation_mode == "fresh_thesis_revalidation" and thesis_signal_source != "fresh_runtime_recomputed"
        else current_live_track_blocker
    ),
    "current_selected_replay_state": current_selected_replay_state,
    "current_selected_replay_blocker": current_selected_replay_blocker,
    "run65_step": run65_step,
    "rg6_step": rg6_step,
    "cth_step": cth_step,
    "ctpm_step": ctpm_step,
    "ctb_step": ctb_step,
    "cta_step": cta_step,
    "ctv2_step": ctv2_step,
    "ctv_step": ctv_step,
    "ctp_step": ctp_step,
    "ctq_step": ctq_step,
    "ctl_step": ctl_step,
    "proposal_eligible_continuation_rows": proposal_count,
    "structural_continuation_rows_seen": cth_rows_seen,
    "pm_owner_lock_ready_rows": int(ctpm.get("pm_owner_lock_ready_rows") or 0),
    "ctb_entry_rule_recovered_rows": int(ctb.get("entry_rule_recovered_rows") or 0),
    "ctb_exit_rule_recovered_rows": int(ctb.get("exit_rule_recovered_rows") or 0),
    "cta_execution_plumbing_ready_rows": int(cta.get("continuation_execution_plumbing_ready_rows") or 0),
    "fresh_runtime_ready_rows": fresh_runtime_ready,
    "fresh_thesis_ready_rows": fresh_ready,
    "proposal_grade_fresh_thesis_ready_rows": proposal_grade_fresh_ready,
    "selected_private_replay_ready_rows": replay_ready,
    "cth_final_verdict": cth.get("final_verdict"),
    "ctpm_final_verdict": ctpm.get("final_verdict"),
    "ctb_final_verdict": ctb.get("final_verdict"),
    "cta_final_verdict": cta.get("final_verdict"),
    "ctv2_final_verdict": ctv2.get("final_verdict"),
    "runtime_recompute_verdict": ctv2.get("final_verdict"),
    "ctv_final_verdict": ctv.get("final_verdict"),
    "ctp_final_verdict": ctp.get("final_verdict"),
    "ctq_final_verdict": ctq.get("final_verdict"),
    "ctl_final_verdict": ctl.get("final_verdict"),
    "rg6_final_verdict": rg6.get("final_verdict"),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": status,
    "validation_mode": validation_mode,
    "thesis_signal_source": thesis_signal_source,
    "continuation_state": continuation_state,
    "current_live_track_state": current_live_track_state,
    "current_live_track_blocker": current_live_track_blocker,
    "proposal_grade_fresh_thesis_state": summary["proposal_grade_fresh_thesis_state"],
    "proposal_grade_fresh_thesis_blocker": summary["proposal_grade_fresh_thesis_blocker"],
    "current_selected_replay_state": current_selected_replay_state,
    "current_selected_replay_blocker": current_selected_replay_blocker,
    "fresh_runtime_recompute_verdict": ctv2.get("final_verdict"),
    "runtime_recompute_verdict": ctv2.get("final_verdict"),
    "current_fresh_thesis_verdict": ctv.get("final_verdict"),
    "current_replay_verdict": ctp.get("final_verdict"),
    "proposal_eligible_continuation_rows": proposal_count,
    "structural_continuation_rows_seen": cth_rows_seen,
    "pm_owner_lock_ready_rows": int(ctpm.get("pm_owner_lock_ready_rows") or 0),
    "ctb_entry_rule_recovered_rows": int(ctb.get("entry_rule_recovered_rows") or 0),
    "ctb_exit_rule_recovered_rows": int(ctb.get("exit_rule_recovered_rows") or 0),
    "cta_execution_plumbing_ready_rows": int(cta.get("continuation_execution_plumbing_ready_rows") or 0),
    "fresh_runtime_ready_rows": fresh_runtime_ready,
    "fresh_thesis_ready_rows": fresh_ready,
    "proposal_grade_fresh_thesis_ready_rows": proposal_grade_fresh_ready,
    "selected_private_replay_ready_rows": replay_ready,
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "result_dir": str(result_dir),
    "validation_mode": validation_mode,
    "strict_diagnostics_enabled": strict_diagnostics_enabled,
    "thesis_signal_source": thesis_signal_source,
    "run65_step": run65_step,
    "proposal_rows": str(proposal_rows),
    "ctv2_rows": str(results / ctv2_step / "continuation_fresh_runtime_pressure_rows.jsonl"),
    "ctv_rows": str(results / ctv_step / "continuation_fresh_thesis_revalidation_rows.jsonl"),
    "ctq_rows": str(results / ctq_step / "continuation_quote_drift_autopsy_rows.jsonl"),
    "live_send": False,
    "sendBundle": False,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
PY

pass "Run66CTF fresh continuation replay window written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

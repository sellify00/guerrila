#!/usr/bin/env bash
# 66rg2_rebalancing_owner_selector_instrumentation_repair.sh - owner-gated Rebalancing instrumentation audit

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66RG2 - Rebalancing Owner-Selector Instrumentation Repair"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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
STEP="${SHADOW_RUN66RG2_STEP:-66rg2_rebalancing_owner_selector_instrumentation_repair_20260527}"
SOURCE_DIR="${SHADOW_RUN66RG2_SOURCE_DIR:-}"
SOURCE_ROWS="${SHADOW_RUN66RG2_SOURCE_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66RG2_RULE_ROWS:-}"
Q2_ROWS="${SHADOW_RUN66RG2_Q2_ROWS:-}"
OVERLAY_ROWS="${SHADOW_RUN66RG2_OVERLAY_ROWS:-}"
TEMPLATE_ROWS="${SHADOW_RUN66RG2_TEMPLATE_ROWS:-}"
PM_ROWS="${SHADOW_RUN66RG2_PM_ROWS:-}"
RESTORATION_ROWS="${SHADOW_RUN66RG2_RESTORATION_ROWS:-}"
FALSE_RESTORATION_ROWS="${SHADOW_RUN66RG2_FALSE_RESTORATION_ROWS:-}"

latest_66rg_dir() {
    find "$RESULTS_ROOT" -maxdepth 1 -type d -name '66rg_rebalancing_certificate_owner_reconciliation*' -printf '%T@ %p\n' 2>/dev/null \
        | sort -nr \
        | head -n 1 \
        | cut -d' ' -f2-
}

[[ -n "$SOURCE_DIR" ]] || SOURCE_DIR="$(latest_66rg_dir)"
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$SOURCE_DIR/all_21_structural_ready_rows_frozen.jsonl"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(find "$RESULTS_ROOT" -type f -name run66re_rebalancing_entry_exit_rule_proof_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$Q2_ROWS" ]] || Q2_ROWS="$(find "$RESULTS_ROOT" -type f -name run66q2_rebalancing_exact_quote_route_ev_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$OVERLAY_ROWS" ]] || OVERLAY_ROWS="$(find "$RESULTS_ROOT" -type f -name run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$TEMPLATE_ROWS" ]] || TEMPLATE_ROWS="$(find "$RESULTS_ROOT" -type f -name run66rt_rebalancing_template_nosend_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$PM_ROWS" ]] || PM_ROWS="$(find "$RESULTS_ROOT" -type f -name run66rg3_rebalancing_pm_owner_lock_surface_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$RESTORATION_ROWS" ]] || RESTORATION_ROWS="$(find "$RESULTS_ROOT" -type f -name run66rg4_rebalancing_restoration_pressure_measurement_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
[[ -n "$FALSE_RESTORATION_ROWS" ]] || FALSE_RESTORATION_ROWS="$(find "$RESULTS_ROOT" -type f -name run66rg5_false_restoration_gate_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/owner_selector_instrumentation_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/owner_selector_instrumentation_summary.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_DIR" "$SOURCE_ROWS" "$RULE_ROWS" "$Q2_ROWS" "$OVERLAY_ROWS" "$TEMPLATE_ROWS" "$PM_ROWS" "$RESTORATION_ROWS" "$FALSE_RESTORATION_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_dir_arg, source_rows_arg, rule_rows_arg, q2_rows_arg, overlay_rows_arg, template_rows_arg, pm_rows_arg, restoration_rows_arg, false_restoration_rows_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:16]
source_dir = Path(source_dir_arg) if source_dir_arg else None
source_rows = Path(source_rows_arg) if source_rows_arg else None
rule_rows_path = Path(rule_rows_arg) if rule_rows_arg else None
q2_rows_path = Path(q2_rows_arg) if q2_rows_arg else None
overlay_rows_path = Path(overlay_rows_arg) if overlay_rows_arg else None
template_rows_path = Path(template_rows_arg) if template_rows_arg else None
pm_rows_path = Path(pm_rows_arg) if pm_rows_arg else None
restoration_rows_path = Path(restoration_rows_arg) if restoration_rows_arg else None
false_restoration_rows_path = Path(false_restoration_rows_arg) if false_restoration_rows_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def text(value):
    if value is None:
        return ""
    return str(value).strip()

def lower(value):
    return text(value).lower()

def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}

def first_text(row, *keys):
    for key in keys:
        value = row.get(key)
        if text(value):
            return text(value)
    return ""

def first_number(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is None or value == "":
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return None

def first_number_with_source(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is None or value == "":
            continue
        try:
            return float(value), key
        except (TypeError, ValueError):
            continue
    return None, ""

def first_bool(row, *keys):
    for key in keys:
        if key in row:
            return boolish(row.get(key))
    return False

def positive_number(row, *keys):
    value = first_number(row, *keys)
    return value is not None and value > 0

def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out

def overlay(row, *others):
    out = dict(row)
    for other in others:
        if other:
            for key, value in other.items():
                if key not in out or out.get(key) in (None, ""):
                    out[key] = value
    return out

def overlay_authoritative(row, *others):
    out = dict(row)
    for other in others:
        if other:
            for key, value in other.items():
                if value not in (None, ""):
                    out[key] = value
    return out

def execution_plumbing_ready(row):
    structural_ready = boolish(row.get("previous_structural_ready")) or boolish(row.get("template_ready"))
    quote_identity_ready = (
        boolish(row.get("exact_quote_or_ingress_identity_match"))
        or boolish(row.get("quote_identity_exact_or_unique_safe"))
        or boolish(row.get("exact_quote_match"))
    )
    min_out_ready = (
        boolish(row.get("min_out_raw_positive"))
        or boolish(row.get("entry_min_out_positive"))
        or positive_number(row, "min_out_raw", "entry_min_out")
    )
    ev_ready = (
        boolish(row.get("route_specific_net_ev_positive"))
        or boolish(row.get("roundtrip_positive_after_all_costs"))
        or boolish(row.get("positive_after_cost"))
        or boolish(row.get("net_ev_after_fee_tip_positive"))
        or positive_number(row, "route_specific_net_ev_raw", "route_cost_model_net_ev_after_fee_tip_raw", "net_ev_after_fee_tip_raw")
    )
    pm_ready = (
        boolish(row.get("pm_execution_ready"))
        or (
            boolish(row.get("pm_reserve_input_ok"))
            and boolish(row.get("pm_release_on_no_send_ok"))
            and first_number(row, "pm_pending_after") in (0, 0.0, None)
        )
        or boolish(row.get("pm_reserve_sim_ok"))
    )
    account_template_ready = (
        boolish(row.get("account_inventory_template_ready"))
        or (boolish(row.get("template_ready")) and not boolish(row.get("ata_created")))
        or (boolish(row.get("account_ready")) and boolish(row.get("template_ready")))
    )
    egress_ready = (
        boolish(row.get("egress_ready_no_send_or_no_submit"))
        or boolish(row.get("helius_sender_no_send_accept"))
        or boolish(row.get("simulate_bundle_pass"))
        or text(row.get("decision")) in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"}
    )
    return all([
        structural_ready,
        quote_identity_ready,
        min_out_ready,
        ev_ready,
        pm_ready,
        account_template_ready,
        egress_ready,
        not boolish(row.get("wire_send")),
        not boolish(row.get("live_landed")),
    ])

def first_blocker(item, row):
    if not item["execution_plumbing_ready"]:
        return "execution_plumbing_regressed"
    if item["owner_score_source"] != "fresh_runtime_owner_scores":
        return item.get("owner_match_failure_reason") or "REBALANCING_SCORE_PLACEHOLDER_OR_ZERO"
    if not item["owner_mode_match_rebalancing"]:
        return item["owner_match_failure_reason"] or first_text(row, "first_owner_blocker") or "owner_mode_match_rebalancing_false"
    if not item["pm_owner_lock_ready"]:
        return "pm_owner_lock_missing"
    if not item["restoration_pressure_measured"]:
        return "restoration_pressure_missing"
    if not item["displacement_threshold_pass"]:
        return "displacement_threshold_missing_or_failed"
    if item["recovery_fraction_target"] is None:
        return "recovery_fraction_target_missing"
    if not item["entry_rule_rebalancing_specific"]:
        return "entry_rule_not_rebalancing_specific"
    if not item["exit_rule_rebalancing_specific"]:
        return "exit_rule_not_rebalancing_specific"
    if not item["false_restoration_gate_input_ready"]:
        return "false_restoration_gate_inputs_missing"
    if not item["false_restoration_gate_pass"]:
        return "false_restoration_gate_failed"
    return "none"

def owner_match_failure_reason(item):
    if item["owner_score_source"] != "fresh_runtime_owner_scores":
        return "REBALANCING_SCORE_PLACEHOLDER_OR_ZERO"
    if item["owner_mode_selected"] != "rebalancing":
        return "REBALANCING_SCORE_PRESENT_BUT_LOST_OWNER_RACE"
    if item["winner_score"] is not None and item["rebalancing_score"] is not None and item["winner_score"] > item["rebalancing_score"]:
        return "REBALANCING_SCORE_PRESENT_BUT_LOST_OWNER_RACE"
    if not item["restoration_pressure_measured"]:
        return "RESTORATION_PRESSURE_MISSING"
    if not item["pm_owner_lock_ready"]:
        return "PM_OWNER_LOCK_MISSING"
    if not item["false_restoration_gate_input_ready"]:
        return "FALSE_RESTORATION_GATE_NOT_READY"
    if not item.get("false_restoration_gate_pass", False):
        return "FALSE_RESTORATION_GATE_FAILED"
    return "OWNER_MODE_MATCH_FALSE"

source_rows_loaded = load_jsonl(source_rows)
rule_by_candidate = by_candidate(load_jsonl(rule_rows_path))
q2_by_candidate = by_candidate(load_jsonl(q2_rows_path))
overlay_by_candidate = by_candidate(load_jsonl(overlay_rows_path))
template_by_candidate = by_candidate(load_jsonl(template_rows_path))
pm_by_candidate = by_candidate(load_jsonl(pm_rows_path))
restoration_by_candidate = by_candidate(load_jsonl(restoration_rows_path))
false_restoration_by_candidate = by_candidate(load_jsonl(false_restoration_rows_path))
instrumented_rows = []
for index, row in enumerate(source_rows_loaded, start=1):
    cid = first_text(row, "candidate_id", "row_id")
    row = overlay(row, rule_by_candidate.get(cid), q2_by_candidate.get(cid), overlay_by_candidate.get(cid), template_by_candidate.get(cid))
    row = overlay_authoritative(row, pm_by_candidate.get(cid), restoration_by_candidate.get(cid), false_restoration_by_candidate.get(cid))
    selected_owner = first_text(row, "owner_mode_selected", "selected_owner", "owner_mode")
    pm_lock_ready = first_bool(row, "pm_owner_lock_ready", "pm_owner_lock_present")
    restoration_measured = first_bool(row, "restoration_pressure_measured", "restoration_pressure_fields_present", "reserve_displacement_measured")
    false_restoration_input_ready = first_bool(row, "false_restoration_gate_input_ready", "false_restoration_gate_inputs_present")
    if "false_restoration_gate_pass" in row:
        false_restoration_pass = first_bool(row, "false_restoration_gate_pass")
    else:
        false_restoration_pass = false_restoration_input_ready
    entry_specific = first_bool(row, "entry_rule_rebalancing_specific") or (first_bool(row, "entry_rule_present") and text(row.get("entry_trigger")).startswith("next_same_pool_event"))
    exit_specific = first_bool(row, "exit_rule_rebalancing_specific") or (first_bool(row, "exit_rule_present") and text(row.get("exit_trigger")).startswith("next_same_pool_event"))
    displacement_value = first_number(row, "reserve_displacement_bps", "price_displacement_bps", "observed_displacement_bps", "displacement_threshold")
    displacement_pass = first_bool(row, "displacement_threshold_pass") or (displacement_value is not None and abs(displacement_value) > 0)
    recovery_target = first_number(row, "recovery_fraction_target", "target_recovery_fraction")
    owner_score, owner_score_key = first_number_with_source(row, "owner_mode_score_rebalancing", "rebalancing_owner_score", "rebalancing_score", "selected_owner_score")
    owner_score_source = first_text(row, "owner_mode_score_rebalancing_source", "owner_score_source")
    if not owner_score_source and owner_score_key:
        owner_score_source = f"artifact:{owner_score_key}"
    if owner_score is None:
        owner_score = 0.0
        owner_score_source = "missing_default_zero_fail_closed"
    continuation_score = first_number(row, "owner_mode_score_continuation", "continuation_score")
    backrun_score = first_number(row, "owner_mode_score_backrun", "backrun_score")
    drop_score = first_number(row, "owner_mode_score_drop", "drop_score")
    score_by_owner = {
        "rebalancing": owner_score,
        "continuation": continuation_score,
        "backrun": backrun_score,
        "drop": drop_score,
    }
    present_scores = {owner: score for owner, score in score_by_owner.items() if score is not None}
    if present_scores:
        winner_owner, winner_score = max(present_scores.items(), key=lambda item: item[1])
    else:
        winner_owner, winner_score = selected_owner, owner_score
    score_delta_vs_winner = None if winner_score is None or owner_score is None else owner_score - winner_score
    owner_selector_match = (
        selected_owner == "rebalancing"
        and owner_score_source == "fresh_runtime_owner_scores"
        and winner_owner == "rebalancing"
    )
    matched_rollout_owner_match = first_bool(row, "owner_mode_match_rebalancing", "owner_mode_match")
    owner_match = matched_rollout_owner_match or owner_selector_match
    item = {
        "candidate_id": cid or f"row_{index}",
        "row_class_id": first_text(row, "row_class_id"),
        "pool": first_text(row, "pool"),
        "venue": first_text(row, "venue"),
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "owner_mode_selected": selected_owner,
        "owner_mode_match_rebalancing": owner_match,
        "owner_mode_score_rebalancing": owner_score,
        "owner_mode_score_rebalancing_value": owner_score,
        "owner_mode_score_rebalancing_source": owner_score_source,
        "owner_score_source": owner_score_source,
        "rebalancing_score": owner_score,
        "owner_mode_score_continuation": continuation_score,
        "owner_mode_score_backrun": backrun_score,
        "owner_mode_score_drop": drop_score,
        "winner_owner": winner_owner,
        "winner_score": winner_score,
        "score_delta_vs_winner": score_delta_vs_winner,
        "matched_rollout_owner_mode": first_text(row, "matched_rollout_owner_mode"),
        "matched_rollout_owner_match_rebalancing": matched_rollout_owner_match,
        "owner_selector_match_rebalancing": owner_selector_match,
        "quote_identity_source": first_text(row, "quote_identity_source"),
        "quote_identity_exact_or_unique_safe": first_bool(row, "quote_identity_exact_or_unique_safe", "exact_quote_match"),
        "route_specific_net_ev_raw": first_number(row, "route_specific_net_ev_raw", "route_cost_model_net_ev_after_fee_tip_raw", "net_ev_after_fee_tip_raw"),
        "entry_trigger": first_text(row, "entry_trigger"),
        "exit_trigger": first_text(row, "exit_trigger"),
        "max_hold_horizon": first_text(row, "max_hold_horizon"),
        "max_adverse_excursion": first_number(row, "max_adverse_excursion", "max_adverse_excursion_bps"),
        "displacement_observed": displacement_value,
        "runner_up_owner": first_text(row, "runner_up_owner"),
        "fallback_owner": first_text(row, "fallback_owner"),
        "restoration_pressure_measured": restoration_measured,
        "displacement_threshold_pass": displacement_pass,
        "recovery_fraction_target": recovery_target,
        "entry_rule_rebalancing_specific": entry_specific,
        "exit_rule_rebalancing_specific": exit_specific,
        "false_restoration_gate_input_ready": false_restoration_input_ready,
        "false_restoration_gate_pass": false_restoration_pass,
        "false_restoration_reason": first_text(row, "false_restoration_reason") or ("inputs_missing" if not false_restoration_input_ready else ("none" if false_restoration_pass else "gate_failed")),
        "pm_owner_lock_ready": pm_lock_ready,
        "pm_owner_lock_owner_mode": first_text(row, "pm_owner_lock_owner_mode"),
        "pm_owner_lock_candidate_id": first_text(row, "pm_owner_lock_candidate_id"),
        "pm_owner_lock_conflict_domain": first_text(row, "pm_owner_lock_conflict_domain"),
        "execution_plumbing_ready": execution_plumbing_ready(row),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    item["proposal_eligible"] = all([
        item["execution_plumbing_ready"],
        item["owner_score_source"] == "fresh_runtime_owner_scores",
        item["owner_mode_match_rebalancing"],
        item["pm_owner_lock_ready"],
        item["restoration_pressure_measured"],
        item["displacement_threshold_pass"],
        item["recovery_fraction_target"] is not None,
        item["entry_rule_rebalancing_specific"],
        item["exit_rule_rebalancing_specific"],
        item["false_restoration_gate_input_ready"],
        item["false_restoration_gate_pass"],
    ])
    item["owner_match_failure_reason"] = owner_match_failure_reason(item) if not item["owner_mode_match_rebalancing"] else "none"
    item["first_owner_eligibility_blocker"] = first_blocker(item, row)
    instrumented_rows.append(item)

counts = {
    "rows_seen": len(instrumented_rows),
    "execution_plumbing_ready_rows": sum(1 for row in instrumented_rows if row["execution_plumbing_ready"]),
    "owner_mode_match_rebalancing_rows": sum(1 for row in instrumented_rows if row["owner_mode_match_rebalancing"]),
    "owner_mode_score_rebalancing_rows": sum(1 for row in instrumented_rows if row["owner_mode_score_rebalancing"] is not None),
    "owner_mode_score_rebalancing_fresh_runtime_rows": sum(1 for row in instrumented_rows if row["owner_score_source"] == "fresh_runtime_owner_scores"),
    "pm_owner_lock_ready_rows": sum(1 for row in instrumented_rows if row["pm_owner_lock_ready"]),
    "restoration_pressure_measured_rows": sum(1 for row in instrumented_rows if row["restoration_pressure_measured"]),
    "displacement_threshold_pass_rows": sum(1 for row in instrumented_rows if row["displacement_threshold_pass"]),
    "recovery_fraction_target_rows": sum(1 for row in instrumented_rows if row["recovery_fraction_target"] is not None),
    "entry_rule_rebalancing_specific_rows": sum(1 for row in instrumented_rows if row["entry_rule_rebalancing_specific"]),
    "exit_rule_rebalancing_specific_rows": sum(1 for row in instrumented_rows if row["exit_rule_rebalancing_specific"]),
    "false_restoration_gate_input_ready_rows": sum(1 for row in instrumented_rows if row["false_restoration_gate_input_ready"]),
    "false_restoration_gate_pass_rows": sum(1 for row in instrumented_rows if row["false_restoration_gate_pass"]),
    "proposal_eligible_rebalancing_rows": sum(1 for row in instrumented_rows if row["proposal_eligible"]),
}
blocker_counts = Counter(row["first_owner_eligibility_blocker"] for row in instrumented_rows)
execution_ready_blocker_counts = Counter(row["first_owner_eligibility_blocker"] for row in instrumented_rows if row["execution_plumbing_ready"])
owner_counts = Counter(row["owner_mode_selected"] or "missing" for row in instrumented_rows)
owner_score_source_counts = Counter(row["owner_mode_score_rebalancing_source"] for row in instrumented_rows)
owner_match_failure_reason_counts = Counter(row["owner_match_failure_reason"] for row in instrumented_rows)
runner_up_counts = Counter(row["runner_up_owner"] or "missing" for row in instrumented_rows)
fallback_counts = Counter(row["fallback_owner"] or "missing" for row in instrumented_rows)

if not source_rows or not source_rows.exists() or not instrumented_rows:
    final_verdict = "PARK_SELECTOR_INPUTS_INCOMPLETE"
elif counts["proposal_eligible_rebalancing_rows"] > 0:
    final_verdict = "OWNER_SELECTOR_INSTRUMENTATION_READY"
elif counts["owner_mode_score_rebalancing_fresh_runtime_rows"] == 0 or counts["owner_mode_match_rebalancing_rows"] == 0 or counts["owner_mode_score_rebalancing_rows"] == 0:
    final_verdict = "PARK_OWNER_SELECTOR_INSTRUMENTATION"
elif counts["pm_owner_lock_ready_rows"] == 0:
    final_verdict = "PARK_PM_OWNER_LOCK_SURFACE"
elif counts["restoration_pressure_measured_rows"] == 0:
    final_verdict = "PARK_RESTORATION_DATA_ABSENT"
elif counts["false_restoration_gate_input_ready_rows"] == 0:
    final_verdict = "PARK_FALSE_RESTORATION_DATA_ABSENT"
elif counts["false_restoration_gate_pass_rows"] == 0:
    final_verdict = "PARK_FALSE_RESTORATION"
else:
    final_verdict = "PARK_OWNER_SELECTOR_INSTRUMENTATION"

next_action_by_verdict = {
    "OWNER_SELECTOR_INSTRUMENTATION_READY": "run owner-gated one-class proposal package",
    "PARK_FALSE_RESTORATION": "audit false-restoration continuation/absorption scoring thresholds on execution-ready rows",
    "PARK_FALSE_RESTORATION_DATA_ABSENT": "export missing false-restoration input fields",
    "PARK_PM_OWNER_LOCK_SURFACE": "repair Rebalancing PM owner-lock export",
    "PARK_RESTORATION_DATA_ABSENT": "repair restoration-pressure measurement export",
    "PARK_OWNER_SELECTOR_INSTRUMENTATION": "repair owner-selector score/match instrumentation",
    "PARK_SELECTOR_INPUTS_INCOMPLETE": "repair selector input artifact paths",
}

manifest = {
    "source_dir": str(source_dir or ""),
    "source_rows": str(source_rows or ""),
    "rule_rows": str(rule_rows_path or ""),
    "q2_rows": str(q2_rows_path or ""),
    "overlay_rows": str(overlay_rows_path or ""),
    "template_rows": str(template_rows_path or ""),
    "pm_rows": str(pm_rows_path or ""),
    "restoration_rows": str(restoration_rows_path or ""),
    "false_restoration_rows": str(false_restoration_rows_path or ""),
    "result_dir": str(result_dir),
    "mode": "frozen_66rg_rows_only",
    "enrichment_mode": "optional_66re_66q2_66r_66rt_66rg3_66rg4_66rg5_rows_fail_closed",
    "broad_hunt": False,
    "live_send": False,
    "sendBundle": False,
    "ata_creation": False,
    "acquisition": False,
    "token2022_explicit_allow": False,
}
summary = {
    "status": final_verdict,
    "certificate_scope": "owner_selector_instrumentation_only",
    "source_rows": str(source_rows or ""),
    "counts": counts,
    "owner_mode_selected_counts": dict(owner_counts),
    "owner_mode_score_rebalancing_source_counts": dict(owner_score_source_counts),
    "runner_up_owner_counts": dict(runner_up_counts),
    "fallback_owner_counts": dict(fallback_counts),
    "first_owner_eligibility_blocker_counts": dict(blocker_counts),
    "execution_ready_first_owner_eligibility_blocker_counts": dict(execution_ready_blocker_counts),
    "owner_match_failure_reason_counts": dict(owner_match_failure_reason_counts),
    "acceptance": {
        "owner_mode_score_rebalancing_fresh_runtime_rows_gt_0": counts["owner_mode_score_rebalancing_fresh_runtime_rows"] > 0,
        "owner_mode_match_rebalancing_rows_gt_0": counts["owner_mode_match_rebalancing_rows"] > 0,
        "pm_owner_lock_ready_rows_gt_0": counts["pm_owner_lock_ready_rows"] > 0,
        "restoration_pressure_measured_rows_gt_0": counts["restoration_pressure_measured_rows"] > 0,
        "false_restoration_gate_input_ready_rows_gt_0": counts["false_restoration_gate_input_ready_rows"] > 0,
        "false_restoration_gate_pass_rows_gt_0": counts["false_restoration_gate_pass_rows"] > 0,
        "proposal_eligible_rebalancing_rows_gt_0": counts["proposal_eligible_rebalancing_rows"] > 0,
    },
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "live_send": False,
        "ata_creation": False,
        "acquisition": False,
        "token2022_explicit_allow": False,
    },
}
verdict = {
    "final_verdict": final_verdict,
    **counts,
    "primary_blocker": (
        execution_ready_blocker_counts.most_common(1)[0][0]
        if execution_ready_blocker_counts
        else (blocker_counts.most_common(1)[0][0] if blocker_counts else "inputs_missing")
    ),
    "all_rows_first_owner_eligibility_blocker_counts": dict(blocker_counts),
    "execution_ready_first_owner_eligibility_blocker_counts": dict(execution_ready_blocker_counts),
    "proposal_path_open": counts["proposal_eligible_rebalancing_rows"] > 0,
    "live_approval": False,
    "next_required_action": next_action_by_verdict.get(final_verdict, "repair owner-selector score/match instrumentation"),
    "hard_stops_preserved": summary["hard_stops_preserved"],
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in instrumented_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"final_verdict={final_verdict}",
    f"source_rows={source_rows or ''}",
]
for key in (
    "rows_seen",
    "execution_plumbing_ready_rows",
    "owner_mode_match_rebalancing_rows",
    "owner_mode_score_rebalancing_rows",
    "owner_mode_score_rebalancing_fresh_runtime_rows",
    "pm_owner_lock_ready_rows",
    "restoration_pressure_measured_rows",
    "displacement_threshold_pass_rows",
    "recovery_fraction_target_rows",
    "entry_rule_rebalancing_specific_rows",
    "exit_rule_rebalancing_specific_rows",
    "false_restoration_gate_input_ready_rows",
    "false_restoration_gate_pass_rows",
    "proposal_eligible_rebalancing_rows",
):
    lines.append(f"{key}={counts[key]}")
lines.extend([
    f"first_owner_eligibility_blocker_counts={json.dumps(dict(blocker_counts), sort_keys=True)}",
    f"owner_match_failure_reason_counts={json.dumps(dict(owner_match_failure_reason_counts), sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
])
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RG2 owner-selector instrumentation repair written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

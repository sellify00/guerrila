#!/usr/bin/env bash
# 21_lsprebrh_backrun_preprocessed_structural_hunt_readiness.sh — certificate for preprocessed Backrun selection readiness

source "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-H — Backrun Preprocessed Structural Hunt Readiness"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_LSPREBRH_RESULTS_ROOT:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRH_STEP:-21_lsprebrh_backrun_preprocessed_structural_hunt_readiness}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/21_lsprebrh_backrun_preprocessed_structural_hunt_readiness.json"

LS13A_SUMMARY_PATH="${SHADOW_LSPREBRH_LS13A_SUMMARY_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_summary.json}"
LS13B_SUMMARY_PATH="${SHADOW_LSPREBRH_LS13B_SUMMARY_PATH:-$RESULTS_ROOT/21_runls13b_preprocessed_backrun_selector/runls13b_preprocessed_backrun_selector_summary.json}"
PREBR0_SUMMARY_PATH="${SHADOW_LSPREBRH_PREBR0_SUMMARY_PATH:-$RESULTS_ROOT/21_runlsprebr0_preprocessed_projection_quote/runlsprebr0_preprocessed_projection_quote_summary.json}"
PREBR2_SUMMARY_PATH="${SHADOW_LSPREBRH_PREBR2_SUMMARY_PATH:-$RESULTS_ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/runlsprebr2_preprocessed_backrun_executable_shape_filter_summary.json}"
PREBR1_SUMMARY_PATH="${SHADOW_LSPREBRH_PREBR1_SUMMARY_PATH:-$RESULTS_ROOT/21_runlsprebr1_preprocessed_bundle_nosubmit/runlsprebr1_preprocessed_bundle_nosubmit_summary.json}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON"

python3 - "$LS13A_SUMMARY_PATH" "$LS13B_SUMMARY_PATH" "$PREBR0_SUMMARY_PATH" "$PREBR2_SUMMARY_PATH" "$PREBR1_SUMMARY_PATH" "$SUMMARY_FILE" "$REPORT_JSON" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}

ls13a = load_json(Path(sys.argv[1]))
ls13b = load_json(Path(sys.argv[2]))
prebr0 = load_json(Path(sys.argv[3]))
prebr2 = load_json(Path(sys.argv[4]))
prebr1 = load_json(Path(sys.argv[5]))
summary_path = Path(sys.argv[6])
report_path = Path(sys.argv[7])

preprocessed_connect_ok = bool(ls13a.get("preprocessed_connect_ok"))
preprocessed_raw_tx_seen_count = int(ls13a.get("preprocessed_raw_tx_seen_count") or 0)
preprocessed_tx_bytes_available_count = int(ls13a.get("preprocessed_tx_bytes_available_count") or 0)
target_not_landed_at_first_check_count = int(ls13a.get("target_not_landed_at_first_check_count") or 0)
target_entity_ready_count = int(ls13b.get("target_entity_ready_count") or 0)
pre_target_pool_state_available_count = int(prebr0.get("pre_target_pool_state_available_count") or 0)
projected_post_target_state_count = int(prebr0.get("projected_post_target_state_count") or 0)
projected_expected_out_positive_count = int(prebr0.get("projected_expected_out_positive_count") or 0)
projected_min_out_positive_count = int(prebr0.get("projected_min_out_positive_count") or 0)
projected_ev_after_tip_positive_count = int(prebr0.get("projected_ev_after_tip_positive_count") or 0)
projection_blocker_counts = prebr0.get("projection_blocker_counts") if isinstance(prebr0.get("projection_blocker_counts"), dict) else {}
executable_shape_ready_count = int(prebr2.get("backrun_executable_shape_ready_count") or 0)
held_input_projected_ev_positive_count = int(prebr2.get("held_input_projected_ev_positive_count") or 0)
core_input_projected_ev_positive_count = int(prebr2.get("core_input_projected_ev_positive_count") or 0)
projected_ev_positive_unheld_token_rows = int(prebr2.get("projected_ev_positive_unheld_token_rows") or 0)
executable_shape_decision_counts = prebr2.get("decision_counts") if isinstance(prebr2.get("decision_counts"), dict) else {}
bundle_envelope_built_count = int(prebr1.get("bundle_envelope_built_count") or 0)
pm_reserve_ok_count = int(prebr1.get("pm_reserve_ok_count") or 0)
fresh_quote_positive_count = int(prebr1.get("fresh_quote_positive_count") or 0)
input_account_ready_count = int(prebr1.get("input_account_ready_count") or 0)
output_account_ready_count = int(prebr1.get("output_account_ready_count") or 0)
simulate_bundle_called_count = int(prebr1.get("simulate_bundle_called_count") or 0)
simulate_bundle_pass_count = int(prebr1.get("simulate_bundle_pass_count") or 0)
bundle_ready_no_submit_rows = int(prebr1.get("bundle_ready_no_submit_rows") or 0)
bundle_decision_counts = prebr1.get("decision_counts") if isinstance(prebr1.get("decision_counts"), dict) else {}

def top_blocker(blockers: dict[str, object]) -> str:
    best_key = ""
    best_value = -1
    for key, value in blockers.items():
        try:
            numeric = int(value)
        except Exception:
            numeric = 0
        if numeric > best_value:
            best_value = numeric
            best_key = str(key)
    return best_key

if not preprocessed_connect_ok or preprocessed_raw_tx_seen_count <= 0:
    status = "PARK_NO_PREPROCESSED_TARGET"
    first_blocker = "preprocessed_feed_missing"
elif preprocessed_tx_bytes_available_count <= 0 or target_not_landed_at_first_check_count <= 0:
    status = "PARK_TARGET_ALREADY_PROCESSED"
    first_blocker = "fresh_target_bytes_missing"
elif target_entity_ready_count <= 0:
    status = "PARK_SELECTOR_TOO_NARROW"
    first_blocker = "selector_target_entity_ready_zero"
elif not prebr0:
    status = "PARK_PROJECTION_MISSING"
    first_blocker = "projection_quote_stage_not_integrated"
elif projected_post_target_state_count <= 0:
    status = "PARK_PROJECTION_MISSING"
    first_blocker = top_blocker(projection_blocker_counts) or "projected_post_target_state_zero"
elif projected_expected_out_positive_count <= 0 or projected_min_out_positive_count <= 0:
    status = "PARK_QUOTE_NEGATIVE"
    first_blocker = top_blocker(projection_blocker_counts) or "projected_quote_non_positive"
elif projected_ev_after_tip_positive_count <= 0:
    status = "PARK_QUOTE_NEGATIVE"
    first_blocker = top_blocker(projection_blocker_counts) or "projected_ev_after_tip_non_positive"
elif not prebr2:
    status = "PARK_HELD_INPUT_FILTER_ZERO"
    first_blocker = "executable_shape_filter_not_integrated"
elif executable_shape_ready_count <= 0:
    status = top_blocker(executable_shape_decision_counts) or "PARK_HELD_INPUT_FILTER_ZERO"
    first_blocker = status
elif not prebr1:
    status = "PARK_BUNDLE_NOSUBMIT_MISSING"
    first_blocker = "bundle_nosubmit_stage_not_integrated"
elif bundle_ready_no_submit_rows > 0 and simulate_bundle_pass_count > 0:
    status = "BACKRUN_PREPROCESSED_STRUCTURAL_HUNT_READY"
    first_blocker = "none"
elif input_account_ready_count <= 0 or output_account_ready_count <= 0 or pm_reserve_ok_count <= 0:
    status = "PARK_PM"
    first_blocker = top_blocker(bundle_decision_counts) or "pm_or_account_gate_zero"
elif fresh_quote_positive_count <= 0:
    status = "PARK_QUOTE_NEGATIVE"
    first_blocker = top_blocker(bundle_decision_counts) or "fresh_quote_non_positive"
elif bundle_envelope_built_count <= 0:
    status = "PARK_BUNDLE_NOSUBMIT_MISSING"
    first_blocker = top_blocker(bundle_decision_counts) or "bundle_envelope_built_zero"
elif simulate_bundle_called_count <= 0:
    status = "PARK_BUNDLE_NOSUBMIT_MISSING"
    first_blocker = top_blocker(bundle_decision_counts) or "simulate_bundle_not_called"
else:
    status = "PARK_BUNDLE_SIM_FAILED"
    first_blocker = top_blocker(bundle_decision_counts) or "simulate_bundle_pass_zero"

report = {
    "status": status,
    "first_blocker": first_blocker,
    "preprocessed_connect_ok": preprocessed_connect_ok,
    "preprocessed_raw_tx_seen_count": preprocessed_raw_tx_seen_count,
    "preprocessed_tx_bytes_available_count": preprocessed_tx_bytes_available_count,
    "target_not_landed_at_first_check_count": target_not_landed_at_first_check_count,
    "target_entity_ready_count": target_entity_ready_count,
    "pre_target_pool_state_available_count": pre_target_pool_state_available_count,
    "projected_post_target_state_count": projected_post_target_state_count,
    "projected_expected_out_positive_count": projected_expected_out_positive_count,
    "projected_min_out_positive_count": projected_min_out_positive_count,
    "projected_ev_after_tip_positive_count": projected_ev_after_tip_positive_count,
    "projection_blocker_counts": projection_blocker_counts,
    "executable_shape_ready_count": executable_shape_ready_count,
    "held_input_projected_ev_positive_count": held_input_projected_ev_positive_count,
    "core_input_projected_ev_positive_count": core_input_projected_ev_positive_count,
    "projected_ev_positive_unheld_token_rows": projected_ev_positive_unheld_token_rows,
    "executable_shape_decision_counts": executable_shape_decision_counts,
    "fresh_quote_positive_count": fresh_quote_positive_count,
    "input_account_ready_count": input_account_ready_count,
    "output_account_ready_count": output_account_ready_count,
    "bundle_envelope_built_count": bundle_envelope_built_count,
    "pm_reserve_ok_count": pm_reserve_ok_count,
    "simulate_bundle_called_count": simulate_bundle_called_count,
    "simulate_bundle_pass_count": simulate_bundle_pass_count,
    "bundle_ready_no_submit_rows": bundle_ready_no_submit_rows,
    "bundle_decision_counts": bundle_decision_counts,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
    "ls13a_summary_path": str(Path(sys.argv[1])),
    "ls13b_summary_path": str(Path(sys.argv[2])),
    "prebr0_summary_path": str(Path(sys.argv[3])) if prebr0 else None,
    "prebr2_summary_path": str(Path(sys.argv[4])) if prebr2 else None,
    "prebr1_summary_path": str(Path(sys.argv[5])) if prebr1 else None,
}

report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={status}",
    f"first_blocker={first_blocker}",
    f"preprocessed_connect_ok={str(preprocessed_connect_ok).lower()}",
    f"preprocessed_raw_tx_seen_count={preprocessed_raw_tx_seen_count}",
    f"preprocessed_tx_bytes_available_count={preprocessed_tx_bytes_available_count}",
    f"target_not_landed_at_first_check_count={target_not_landed_at_first_check_count}",
    f"target_entity_ready_count={target_entity_ready_count}",
    f"pre_target_pool_state_available_count={pre_target_pool_state_available_count}",
    f"projected_post_target_state_count={projected_post_target_state_count}",
    f"projected_expected_out_positive_count={projected_expected_out_positive_count}",
    f"projected_min_out_positive_count={projected_min_out_positive_count}",
    f"projected_ev_after_tip_positive_count={projected_ev_after_tip_positive_count}",
    "projection_blocker_counts=" + json.dumps(projection_blocker_counts, sort_keys=True),
    f"executable_shape_ready_count={executable_shape_ready_count}",
    f"held_input_projected_ev_positive_count={held_input_projected_ev_positive_count}",
    f"core_input_projected_ev_positive_count={core_input_projected_ev_positive_count}",
    f"projected_ev_positive_unheld_token_rows={projected_ev_positive_unheld_token_rows}",
    "executable_shape_decision_counts=" + json.dumps(executable_shape_decision_counts, sort_keys=True),
    f"fresh_quote_positive_count={fresh_quote_positive_count}",
    f"input_account_ready_count={input_account_ready_count}",
    f"output_account_ready_count={output_account_ready_count}",
    f"bundle_envelope_built_count={bundle_envelope_built_count}",
    f"pm_reserve_ok_count={pm_reserve_ok_count}",
    f"simulate_bundle_called_count={simulate_bundle_called_count}",
    f"simulate_bundle_pass_count={simulate_bundle_pass_count}",
    f"bundle_ready_no_submit_rows={bundle_ready_no_submit_rows}",
    "bundle_decision_counts=" + json.dumps(bundle_decision_counts, sort_keys=True),
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "LS-PRE-BR-H summary written to $REPORT_JSON"
finish

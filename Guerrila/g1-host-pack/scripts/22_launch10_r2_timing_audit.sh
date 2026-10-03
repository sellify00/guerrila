#!/usr/bin/env bash
# 22_launch10_r2_timing_audit.sh — summarize Launch R2 hot-path timing from a completed run root

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch10 — R2 Timing Audit"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-$(find "$RESULTS_ROOT" -maxdepth 1 -type d -name 'run_launch_r2_press_window_1h_*' | sort | tail -n 1)}"
[[ -d "$RUN_ROOT" ]] || die "run root not found: $RUN_ROOT"

STEP_DIR="$RUN_ROOT/22_launch5_one_mint_live_execute_r2"
REPORT_JSON="$STEP_DIR/launch5_one_mint_live_execute.json"
SUMMARY_TXT="$RUN_ROOT/22_launch10_r2_timing_audit_summary.txt"
SUMMARY_JSON="$RUN_ROOT/22_launch10_r2_timing_audit_summary.json"
[[ -f "$REPORT_JSON" ]] || die "launch5 report missing: $REPORT_JSON"

python3 - "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

report_path = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
summary_txt_path = Path(sys.argv[3])
report = json.loads(report_path.read_text(encoding="utf-8", errors="replace"))

def safe_int(value, default=0):
    try:
        if value in (None, ""):
            return default
        return int(value)
    except Exception:
        try:
            return int(float(value))
        except Exception:
            return default

target_seen_preprocessed_ns = safe_int(report.get("target_seen_preprocessed_ns"))
target_first_status_check_ns = safe_int(report.get("target_first_status_check_ns"))
execute_start_ns = safe_int(report.get("execute_start_ns"))
bundle_submit_ns = safe_int(report.get("bundle_submit_ns"))
entry_send_ns = safe_int(report.get("entry_send_ns"))
target_seen_to_bundle_ready_us = max(0, (bundle_submit_ns - target_seen_preprocessed_ns) // 1000) if bundle_submit_ns and target_seen_preprocessed_ns else 0
target_seen_to_execute_start_us = max(0, (execute_start_ns - target_seen_preprocessed_ns) // 1000) if execute_start_ns and target_seen_preprocessed_ns else 0
target_landed_before_selector = False
target_landed_before_bundle = False
target_landed_before_execute = "target_already_landed" in str(report.get("first_failure") or "")

decision = "HOT_READY"
if target_landed_before_execute and target_seen_to_execute_start_us > 500_000:
    decision = "PIPELINE_LATE"
elif target_landed_before_execute:
    decision = "SOURCE_LATE"
elif bundle_submit_ns <= 0:
    decision = "ARTIFACT_LATE"
elif target_seen_to_bundle_ready_us > 500_000:
    decision = "BUNDLE_SIM_LATE"

summary = {
    "status": decision,
    "target_seen_preprocessed_ns": target_seen_preprocessed_ns,
    "target_first_status_check_ns": target_first_status_check_ns,
    "selector_ready_ns": 0,
    "bundle_ready_ns": bundle_submit_ns,
    "execute_start_ns": execute_start_ns,
    "presend_status_check_ns": target_first_status_check_ns,
    "target_landed_before_selector": target_landed_before_selector,
    "target_landed_before_bundle": target_landed_before_bundle,
    "target_landed_before_execute": target_landed_before_execute,
    "target_seen_to_bundle_ready_us": target_seen_to_bundle_ready_us,
    "target_seen_to_execute_start_us": target_seen_to_execute_start_us,
    "target_seen_to_our_buy_built_ms": max(0, target_seen_to_bundle_ready_us // 1000),
    "bundle_submit_to_result_ms": max(0, (safe_int(report.get("bundle_result_ns")) - bundle_submit_ns) // 1_000_000) if safe_int(report.get("bundle_result_ns")) and bundle_submit_ns else 0,
    "entry_mode_selected": report.get("entry_mode_selected"),
    "first_failure": report.get("first_failure"),
}

summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(f"{k}={str(v).lower() if isinstance(v, bool) else v}" for k, v in summary.items()) + "\n",
    encoding="utf-8",
)
PY

pass "Launch10 timing audit written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 22_launch0u_active_bonding_curve_r3_candidate_gate.sh — no-wire active Pump bonding-curve R3 candidate gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U — Active Bonding-Curve R3 Candidate Gate"

assert_cmd_exists python3

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    WARMUP_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0t_warmup_calibration_replay' || true)"
    if [[ -z "$WARMUP_SUMMARY" ]]; then
        WARMUP_SUMMARY="$(latest_file launch0q_realtime_launch_quote_state_summary.json)"
    fi
    [[ -f "$WARMUP_SUMMARY" ]] || fail "Launch0U source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$WARMUP_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U_REFRESH_GATES:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0q_realtime_launch_quote_state_hydrator.sh" "$RUN_ROOT"
    bash "$SCRIPT_DIR/22_launch24_entry_quality_gate.sh" "$RUN_ROOT"
    bash "$SCRIPT_DIR/22_launch35_absorption_no_exit_fragility_gate.sh" "$RUN_ROOT"
    SHADOW_LAUNCH0T_REFRESH_GATES=false bash "$SCRIPT_DIR/22_launch0t_warmup_calibration_replay.sh" "$RUN_ROOT"
fi

if [[ -n "${SHADOW_LAUNCH0U_WARMUP_ROWS:-}" ]]; then
    WARMUP_ROWS="$SHADOW_LAUNCH0U_WARMUP_ROWS"
elif [[ -f "$RUN_ROOT/22_launch0u5_active_curve_account_identity_autopsy/rows.jsonl" ]]; then
    WARMUP_ROWS="$RUN_ROOT/22_launch0u5_active_curve_account_identity_autopsy/rows.jsonl"
elif [[ -f "$RUN_ROOT/22_launch0u4_active_curve_state_hydration_repair/rows.jsonl" ]]; then
    WARMUP_ROWS="$RUN_ROOT/22_launch0u4_active_curve_state_hydration_repair/rows.jsonl"
else
    WARMUP_ROWS="$RUN_ROOT/22_launch0t_warmup_calibration_replay/bucket_rows.jsonl"
fi
WARMUP_SUMMARY="${SHADOW_LAUNCH0U_WARMUP_SUMMARY:-$RUN_ROOT/22_launch0t_warmup_calibration_replay/summary.json}"
OUT_DIR="${SHADOW_LAUNCH0U_OUT_DIR:-$RUN_ROOT/22_launch0u_active_bonding_curve_r3_candidate_gate}"

[[ -f "$WARMUP_ROWS" ]] || fail "Launch0U warmup rows missing: $WARMUP_ROWS"
[[ -f "$WARMUP_SUMMARY" ]] || fail "Launch0U warmup summary missing: $WARMUP_SUMMARY"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_active_bonding_curve_r3_candidate_gate.py" \
    --run-root "$RUN_ROOT" \
    --warmup-rows "$WARMUP_ROWS" \
    --warmup-summary "$WARMUP_SUMMARY" \
    --out-dir "$OUT_DIR" \
    --normal-warmup-min-age-ms "${SHADOW_LAUNCH0U_NORMAL_WARMUP_MIN_AGE_MS:-10000}" \
    --normal-warmup-max-age-ms "${SHADOW_LAUNCH0U_NORMAL_WARMUP_MAX_AGE_MS:-30000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}" \
    --hunter-mode "${SHADOW_LAUNCH_HUNTER_MODE:-false}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U active bonding-curve R3 candidate gate written to $OUT_DIR/summary.txt"
finish

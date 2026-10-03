#!/usr/bin/env bash
# 22_launch64_completion_probability_model.sh — R5E no-wire completion probability model
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch64 — R5 Completion Probability Model"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "/etc/shadow-engine/env.g1.local.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH64_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60C_SUMMARY="$(latest_file_under_results '/22_launch60c_completion_pressure_velocity/summary.json')"
    [[ -f "$L60C_SUMMARY" ]] || fail "Launch64 Launch60C summary missing"
    RUN_ROOT="$(dirname "$(dirname "$L60C_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch64 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

STATE_JSON="${SHADOW_LAUNCH64_STATE_JSON:-$RESULTS_ROOT/r5_pre_migration_persistent_watchlist.json}"
VELOCITY_ROWS="${SHADOW_LAUNCH64_VELOCITY_ROWS:-$RUN_ROOT/22_launch60c_completion_pressure_velocity/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH64_OUT_DIR:-$RUN_ROOT/22_launch64_completion_probability_model}"

[[ -f "$STATE_JSON" ]] || fail "Launch64 state json missing: $STATE_JSON"
if [[ -n "$VELOCITY_ROWS" && ! -f "$VELOCITY_ROWS" ]]; then
    warn "Launch64 velocity rows missing: $VELOCITY_ROWS"
    VELOCITY_ROWS=""
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

VELOCITY_ARG=()
[[ -n "$VELOCITY_ROWS" ]] && VELOCITY_ARG=(--velocity-rows "$VELOCITY_ROWS")

python3 "$PACK_DIR/scripts/launch_completion_probability_model.py" \
    --state-json "$STATE_JSON" \
    "${VELOCITY_ARG[@]}" \
    --out-dir "$OUT_DIR" \
    --ready-score "${SHADOW_LAUNCH64_READY_SCORE:-70}" \
    --ready-estimated-sol-to-completion "${SHADOW_LAUNCH64_READY_ESTIMATED_SOL_TO_COMPLETION:-2}" \
    --max-estimated-sol-to-completion "${SHADOW_LAUNCH64_MAX_ESTIMATED_SOL_TO_COMPLETION:-30}" \
    --stall-minutes "${SHADOW_LAUNCH64_STALL_MINUTES:-60}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch64 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch64 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch64 completion probability written to $OUT_DIR/summary.txt"
finish

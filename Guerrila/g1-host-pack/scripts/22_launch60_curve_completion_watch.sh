#!/usr/bin/env bash
# 22_launch60_curve_completion_watch.sh — no-wire Pump curve completion pressure watch
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch60 — Curve Completion Watch"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH60_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    U8_ROWS="$(latest_file_under_results '/22_launch0u8_active_curve_fresh_state_filter/rows.jsonl')"
    [[ -f "$U8_ROWS" ]] || fail "Launch60 Launch0U8 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$U8_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch60 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH60_REFRESH_U8:-false}" == "true" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch0u8_active_curve_fresh_state_filter.sh" "$RUN_ROOT"
fi

INPUT_ROWS="${SHADOW_LAUNCH60_INPUT_ROWS:-$RUN_ROOT/22_launch0u8_active_curve_fresh_state_filter/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH60_OUT_DIR:-$RUN_ROOT/22_launch60_curve_completion_watch}"

[[ -f "$INPUT_ROWS" ]] || fail "Launch60 input rows missing: $INPUT_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_curve_completion_watch.py" \
    --input-rows "$INPUT_ROWS" \
    --out-dir "$OUT_DIR" \
    --min-progress-pct "${SHADOW_LAUNCH60_MIN_PROGRESS_PCT:-90}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch60 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch60 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch60 curve completion watch written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 22_launch0u8a_enhanced_create_join.sh — join active Pump curve states to enhanced/create provenance
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U8A — Enhanced Create Join"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U8A_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0D_SUMMARY="$(latest_file launch0d_realtime_birth_decoder_summary.json)"
    [[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch0U8A Launch0D summary missing"
    RUN_ROOT="$(dirname "$LAUNCH0D_SUMMARY")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U8A run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U8A_REFRESH_ENHANCED:-false}" == "true" ]]; then
    SHADOW_LAUNCH0P_RUN_ROOT="$RUN_ROOT/22_launch0u8a_enhanced_source" bash "$SCRIPT_DIR/22_launch0p_helius_processed_enhanced_birth_source.sh"
fi

LAUNCH_ROWS="${SHADOW_LAUNCH0U8A_LAUNCH_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_rows.jsonl}"
ENHANCED_ROWS="${SHADOW_LAUNCH0U8A_ENHANCED_ROWS:-}"
if [[ -z "$ENHANCED_ROWS" ]]; then
    if [[ -f "$RUN_ROOT/22_launch0u8a_enhanced_source/launch0p_helius_processed_enhanced_birth_source_rows.jsonl" ]]; then
        ENHANCED_ROWS="$RUN_ROOT/22_launch0u8a_enhanced_source/launch0p_helius_processed_enhanced_birth_source_rows.jsonl"
    else
        ENHANCED_ROWS="$(latest_file launch0p_helius_processed_enhanced_birth_source_rows.jsonl || true)"
    fi
fi
OUT_DIR="${SHADOW_LAUNCH0U8A_OUT_DIR:-$RUN_ROOT/22_launch0u8a_enhanced_create_join}"

[[ -f "$LAUNCH_ROWS" ]] || fail "Launch0U8A launch rows missing: $LAUNCH_ROWS"
[[ -z "$ENHANCED_ROWS" || -f "$ENHANCED_ROWS" ]] || fail "Launch0U8A enhanced rows missing: $ENHANCED_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=(
    --run-root "$RUN_ROOT"
    --launch-state-rows "$LAUNCH_ROWS"
    --out-dir "$OUT_DIR"
)
if [[ -n "$ENHANCED_ROWS" ]]; then
    ARGS+=(--enhanced-rows "$ENHANCED_ROWS")
fi

python3 "$PACK_DIR/scripts/launch_enhanced_create_join.py" "${ARGS[@]}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U8A no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U8A no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U8A enhanced create join written to $OUT_DIR/summary.txt"
finish

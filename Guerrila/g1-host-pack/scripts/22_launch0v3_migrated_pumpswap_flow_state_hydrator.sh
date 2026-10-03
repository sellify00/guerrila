#!/usr/bin/env bash
# 22_launch0v3_migrated_pumpswap_flow_state_hydrator.sh — no-wire migrated PumpSwap flow state hydrator
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0V3 — Migrated PumpSwap Flow State Hydrator"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0V3_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0M_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0m_migration_pumpswap_route_join' || true)"
    [[ -f "$LAUNCH0M_SUMMARY" ]] || fail "Launch0V3 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0M_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0V3 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0V3_REFRESH_M_GATE:-false}" == "true" ]]; then
    SHADOW_LAUNCH0M_RUN_DOWNSTREAM_GATES=false bash "$SCRIPT_DIR/22_launch0m_migration_pumpswap_route_join.sh" "$RUN_ROOT"
fi

LAUNCH0M_ROWS="${SHADOW_LAUNCH0V3_LAUNCH0M_ROWS:-$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl}"
FLOW_ACTIVITY_ROWS="${SHADOW_LAUNCH0V3_FLOW_ACTIVITY_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_activity_rows.jsonl}"
FLOW_STATE_ROWS="${SHADOW_LAUNCH0V3_FLOW_STATE_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH0V3_OUT_DIR:-$RUN_ROOT/22_launch0v3_migrated_pumpswap_flow_state_hydrator}"

[[ -f "$LAUNCH0M_ROWS" ]] || fail "Launch0V3 Launch0M rows missing: $LAUNCH0M_ROWS"
[[ -f "$FLOW_ACTIVITY_ROWS" ]] || fail "Launch0V3 flow activity rows missing: $FLOW_ACTIVITY_ROWS"
[[ -f "$FLOW_STATE_ROWS" ]] || fail "Launch0V3 flow state rows missing: $FLOW_STATE_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_migrated_pumpswap_flow_state_hydrator.py" \
    --run-root "$RUN_ROOT" \
    --launch0m-rows "$LAUNCH0M_ROWS" \
    --flow-rows "$FLOW_ACTIVITY_ROWS" \
    --flow-rows "$FLOW_STATE_ROWS" \
    --out-dir "$OUT_DIR"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0V3 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0V3 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0V3 migrated PumpSwap flow state hydrator written to $OUT_DIR/summary.txt"
finish

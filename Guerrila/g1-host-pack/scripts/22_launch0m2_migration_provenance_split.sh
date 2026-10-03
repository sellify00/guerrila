#!/usr/bin/env bash
# 22_launch0m2_migration_provenance_split.sh — no-wire completed-curve vs migration provenance split
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0M2 — Migration Provenance Split"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0M2_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    QUOTE_STATE_SUMMARY="$(latest_file launch0q_realtime_launch_quote_state_summary.json)"
    [[ -f "$QUOTE_STATE_SUMMARY" ]] || fail "Launch0Q quote-state summary missing"
    RUN_ROOT="$(dirname "$QUOTE_STATE_SUMMARY")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0M2 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0M2_REFRESH_QUOTE_STATE:-false}" == "true" ]]; then
    bash "$SCRIPT_DIR/22_launch0q_realtime_launch_quote_state_hydrator.sh" "$RUN_ROOT"
fi

QUOTE_STATE_ROWS="${SHADOW_LAUNCH0M2_QUOTE_STATE_ROWS:-$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl}"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
LAUNCH0M_PATH="$PACK_DIR/scripts/launch_migration_pumpswap_route_join.py"
OUT_DIR="${SHADOW_LAUNCH0M2_OUT_DIR:-$RUN_ROOT/22_launch0m2_migration_provenance_split}"
RPC_URL="${SHADOW_LAUNCH0M2_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$QUOTE_STATE_ROWS" ]] || fail "Launch0M2 quote-state rows missing: $QUOTE_STATE_ROWS"
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -f "$LAUNCH0M_PATH" ]] || fail "Launch0M module missing: $LAUNCH0M_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0M2 RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_migration_provenance_split.py" \
    --run-root "$RUN_ROOT" \
    --quote-state-rows "$QUOTE_STATE_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --launch0m-path "$LAUNCH0M_PATH" \
    --rpc-url "$RPC_URL" \
    --out-dir "$OUT_DIR" \
    --signature-scan-limit "${SHADOW_LAUNCH0M2_SIGNATURE_SCAN_LIMIT:-0}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0M2 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0M2 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0M2 migration provenance split written to $OUT_DIR/summary.txt"
finish

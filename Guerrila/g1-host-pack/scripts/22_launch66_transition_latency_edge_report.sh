#!/usr/bin/env bash
# 22_launch66_transition_latency_edge_report.sh — R5G no-wire migration transition latency edge report
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch66 — R5 Transition Latency Edge Report"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH66_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    M3_ROWS="$(latest_file_under_results '/22_launch0m3_realtime_migration_event_watcher/rows.jsonl')"
    [[ -f "$M3_ROWS" ]] || fail "Launch66 migration watcher rows missing"
    RUN_ROOT="$(dirname "$(dirname "$M3_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch66 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MIGRATION_ROWS="${SHADOW_LAUNCH66_MIGRATION_ROWS:-$RUN_ROOT/22_launch0m3_realtime_migration_event_watcher/rows.jsonl}"
PUMPSWAP_ROUTE_ROWS="${SHADOW_LAUNCH66_PUMPSWAP_ROUTE_ROWS:-$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl}"
BRIDGE_ROWS="${SHADOW_LAUNCH66_BRIDGE_ROWS:-$RUN_ROOT/22_launch62b_pre_migration_bridge_followthrough/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH66_OUT_DIR:-$RUN_ROOT/22_launch66_transition_latency_edge_report}"

if [[ -n "$MIGRATION_ROWS" && ! -f "$MIGRATION_ROWS" ]]; then
    warn "Launch66 migration rows missing: $MIGRATION_ROWS"
    MIGRATION_ROWS=""
fi
if [[ -n "$PUMPSWAP_ROUTE_ROWS" && ! -f "$PUMPSWAP_ROUTE_ROWS" ]]; then
    warn "Launch66 PumpSwap route rows missing: $PUMPSWAP_ROUTE_ROWS"
    PUMPSWAP_ROUTE_ROWS=""
fi
if [[ -n "$BRIDGE_ROWS" && ! -f "$BRIDGE_ROWS" ]]; then
    warn "Launch66 bridge rows missing: $BRIDGE_ROWS"
    BRIDGE_ROWS=""
fi

[[ -n "$MIGRATION_ROWS$PUMPSWAP_ROUTE_ROWS$BRIDGE_ROWS" ]] || fail "Launch66 no input rows found"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=()
[[ -n "$MIGRATION_ROWS" ]] && ARGS+=(--migration-rows "$MIGRATION_ROWS")
[[ -n "$PUMPSWAP_ROUTE_ROWS" ]] && ARGS+=(--pumpswap-route-rows "$PUMPSWAP_ROUTE_ROWS")
[[ -n "$BRIDGE_ROWS" ]] && ARGS+=(--bridge-rows "$BRIDGE_ROWS")

python3 "$PACK_DIR/scripts/launch_transition_latency_edge_report.py" \
    "${ARGS[@]}" \
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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch66 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch66 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch66 transition latency edge written to $OUT_DIR/summary.txt"
finish

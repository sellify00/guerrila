#!/usr/bin/env bash
# 22_launch61a_pre_migration_markout_input_builder.sh — no-wire pre-migration markout input builder
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch61A — Pre-Migration Markout Input Builder"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH61A_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch61A Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch61A run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

COMPLETION_ROWS="${SHADOW_LAUNCH61A_COMPLETION_ROWS:-$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl}"
MIGRATION_ROWS="${SHADOW_LAUNCH61A_MIGRATION_ROWS:-$RUN_ROOT/22_launch0m3_realtime_migration_event_watcher/rows.jsonl}"
PUMPSWAP_ROUTE_ROWS="${SHADOW_LAUNCH61A_PUMPSWAP_ROUTE_ROWS:-$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl}"
POST_MIGRATION_MARKOUT_ROWS="${SHADOW_LAUNCH61A_POST_MIGRATION_MARKOUT_ROWS:-}"
OUT_DIR="${SHADOW_LAUNCH61A_OUT_DIR:-$RUN_ROOT/22_launch61a_pre_migration_markout_input_builder}"

[[ -f "$COMPLETION_ROWS" ]] || fail "Launch61A completion rows missing: $COMPLETION_ROWS"
if [[ -n "$MIGRATION_ROWS" && ! -f "$MIGRATION_ROWS" ]]; then
    warn "Launch61A migration rows missing; rows will park as migration missing: $MIGRATION_ROWS"
    MIGRATION_ROWS=""
fi
if [[ -n "$PUMPSWAP_ROUTE_ROWS" && ! -f "$PUMPSWAP_ROUTE_ROWS" ]]; then
    warn "Launch61A PumpSwap route rows missing; rows will park as post-migration quote missing: $PUMPSWAP_ROUTE_ROWS"
    PUMPSWAP_ROUTE_ROWS=""
fi
if [[ -n "$POST_MIGRATION_MARKOUT_ROWS" && ! -f "$POST_MIGRATION_MARKOUT_ROWS" ]]; then
    fail "Launch61A post-migration markout rows configured but missing: $POST_MIGRATION_MARKOUT_ROWS"
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MIGRATION_ARG=()
[[ -n "$MIGRATION_ROWS" ]] && MIGRATION_ARG=(--migration-rows "$MIGRATION_ROWS")
ROUTE_ARG=()
[[ -n "$PUMPSWAP_ROUTE_ROWS" ]] && ROUTE_ARG=(--pumpswap-route-rows "$PUMPSWAP_ROUTE_ROWS")
MARKOUT_ARG=()
[[ -n "$POST_MIGRATION_MARKOUT_ROWS" ]] && MARKOUT_ARG=(--post-migration-markout-rows "$POST_MIGRATION_MARKOUT_ROWS")

python3 "$PACK_DIR/scripts/launch_pre_migration_markout_input_builder.py" \
    --completion-rows "$COMPLETION_ROWS" \
    "${MIGRATION_ARG[@]}" \
    "${ROUTE_ARG[@]}" \
    "${MARKOUT_ARG[@]}" \
    --out-dir "$OUT_DIR" \
    --min-progress-pct "${SHADOW_LAUNCH61A_MIN_PROGRESS_PCT:-90}" \
    --max-spend-lamports "${SHADOW_LAUNCH61A_MAX_SPEND_LAMPORTS:-10000000}" \
    --fee-tip-slippage-lamports "${SHADOW_LAUNCH61A_FEE_TIP_SLIPPAGE_LAMPORTS:-1500000}" \
    --rent-recovery-lamports "${SHADOW_LAUNCH61A_RENT_RECOVERY_LAMPORTS:-2039280}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch61A no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch61A no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch61A markout input rows written to $OUT_DIR/summary.txt"
finish

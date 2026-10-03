#!/usr/bin/env bash
# 22_launch0m_migration_pumpswap_route_join.sh — no-wire completed bonding-curve to PumpSwap route join
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0M — Migration PumpSwap Route Join"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0M_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    QUOTE_STATE_SUMMARY="$(latest_file launch0q_realtime_launch_quote_state_summary.json)"
    [[ -f "$QUOTE_STATE_SUMMARY" ]] || fail "Launch0Q quote-state summary missing"
    RUN_ROOT="$(dirname "$QUOTE_STATE_SUMMARY")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0M run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0M_REFRESH_QUOTE_STATE:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0q_realtime_launch_quote_state_hydrator.sh" "$RUN_ROOT"
fi

if [[ "${SHADOW_LAUNCH0M_RUN_PROVENANCE_SPLIT:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0m2_migration_provenance_split.sh" "$RUN_ROOT"
fi

if [[ "${SHADOW_LAUNCH0M_RUN_MIGRATION_WATCHER:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0m3_realtime_migration_event_watcher.sh" "$RUN_ROOT"
fi

QUOTE_STATE_ROWS="${SHADOW_LAUNCH0M_QUOTE_STATE_ROWS:-$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl}"
MIGRATION_PROVENANCE_ROWS="${SHADOW_LAUNCH0M_MIGRATION_PROVENANCE_ROWS:-$RUN_ROOT/22_launch0m2_migration_provenance_split/rows.jsonl}"
MIGRATION_WATCHER_ROWS="${SHADOW_LAUNCH0M_MIGRATION_WATCHER_ROWS:-$RUN_ROOT/22_launch0m3_realtime_migration_event_watcher/rows.jsonl}"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
OUT_DIR="${SHADOW_LAUNCH0M_OUT_DIR:-$RUN_ROOT/22_launch0m_migration_pumpswap_route_join}"
RPC_URL="${SHADOW_LAUNCH0M_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$QUOTE_STATE_ROWS" ]] || fail "Launch0M quote-state rows missing: $QUOTE_STATE_ROWS"
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0M RPC URL missing"
if [[ "${SHADOW_LAUNCH0M_REQUIRE_PROVENANCE_SPLIT:-true}" != "false" ]]; then
    [[ -f "$MIGRATION_PROVENANCE_ROWS" ]] || fail "Launch0M provenance rows missing: $MIGRATION_PROVENANCE_ROWS"
fi
if [[ "${SHADOW_LAUNCH0M_REQUIRE_MIGRATION_WATCHER:-false}" == "true" ]]; then
    [[ -f "$MIGRATION_WATCHER_ROWS" ]] || fail "Launch0M migration watcher rows missing: $MIGRATION_WATCHER_ROWS"
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-${SHADOW_LAUNCH0M_MAX_LOSS_SOL:-0.015}}"
MAX_LOSS_LAMPORTS="$(python3 - <<PY
print(int(round(float("$MAX_LOSS_SOL") * 1_000_000_000)))
PY
)"

HISTORY_SCAN_ARGS=()
if [[ "${SHADOW_LAUNCH0M_ENABLE_HISTORY_SCAN:-false}" == "true" ]]; then
    HISTORY_SCAN_ARGS+=(--enable-history-scan)
fi

python3 "$PACK_DIR/scripts/launch_migration_pumpswap_route_join.py" \
    --run-root "$RUN_ROOT" \
    --quote-state-rows "$QUOTE_STATE_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --rpc-url "$RPC_URL" \
    --out-dir "$OUT_DIR" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-${SHADOW_LAUNCH0M_MAX_SPEND_SOL:-0.01}}" \
    --max-loss-lamports "$MAX_LOSS_LAMPORTS" \
    --slippage-bps "${SHADOW_LAUNCH0M_SLIPPAGE_BPS:-${SHADOW_LAUNCH0Q_SLIPPAGE_BPS:-${SHADOW_LAUNCH5_SLIPPAGE_BPS:-100}}}" \
    --fee-budget-lamports "${SHADOW_LAUNCH0M_FEE_BUDGET_LAMPORTS:-410000}" \
    --exit-safety-bps "${SHADOW_LAUNCH0M_EXIT_SAFETY_BPS:-10000}" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}" \
    --signature-search-limit "${SHADOW_LAUNCH0M_SIGNATURE_SEARCH_LIMIT:-100}" \
    --tx-scan-limit "${SHADOW_LAUNCH0M_TX_SCAN_LIMIT:-100}" \
    --migration-provenance-rows "$MIGRATION_PROVENANCE_ROWS" \
    --migration-watcher-rows "$MIGRATION_WATCHER_ROWS" \
    "${HISTORY_SCAN_ARGS[@]}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0M no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0M no-wire invariant failed: live_landed=$LIVE_LANDED"

if [[ "${SHADOW_LAUNCH0M_RUN_DOWNSTREAM_GATES:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch24_entry_quality_gate.sh" "$RUN_ROOT"
    bash "$SCRIPT_DIR/22_launch35_absorption_no_exit_fragility_gate.sh" "$RUN_ROOT"
fi

pass "Launch0M migration PumpSwap route join written to $OUT_DIR/summary.txt"
finish

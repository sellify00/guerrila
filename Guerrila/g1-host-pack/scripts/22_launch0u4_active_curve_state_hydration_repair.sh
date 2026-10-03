#!/usr/bin/env bash
# 22_launch0u4_active_curve_state_hydration_repair.sh — no-wire active curve state hydration repair
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U4 — Active Curve State Hydration Repair"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U4_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0U_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0u_active_bonding_curve_r3_candidate_gate' || true)"
    [[ -f "$LAUNCH0U_SUMMARY" ]] || fail "Launch0U4 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0U_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U4 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U4_REFRESH_U_GATE:-true}" != "false" ]]; then
    SHADOW_LAUNCH0U_REFRESH_GATES=false bash "$SCRIPT_DIR/22_launch0u_active_bonding_curve_r3_candidate_gate.sh" "$RUN_ROOT"
fi

LAUNCH0U_ROWS="${SHADOW_LAUNCH0U4_LAUNCH0U_ROWS:-$RUN_ROOT/22_launch0u_active_bonding_curve_r3_candidate_gate/rows.jsonl}"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
QUOTE_HYDRATOR_PATH="$PACK_DIR/scripts/launch_realtime_quote_state_hydrator.py"
OUT_DIR="${SHADOW_LAUNCH0U4_OUT_DIR:-$RUN_ROOT/22_launch0u4_active_curve_state_hydration_repair}"
RPC_URL="${SHADOW_LAUNCH0U4_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$LAUNCH0U_ROWS" ]] || fail "Launch0U4 Launch0U rows missing: $LAUNCH0U_ROWS"
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -f "$QUOTE_HYDRATOR_PATH" ]] || fail "Launch quote hydrator missing: $QUOTE_HYDRATOR_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0U4 RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_active_curve_state_hydration_repair.py" \
    --run-root "$RUN_ROOT" \
    --launch0u-rows "$LAUNCH0U_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --quote-hydrator-path "$QUOTE_HYDRATOR_PATH" \
    --rpc-url "$RPC_URL" \
    --out-dir "$OUT_DIR" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-0.01}" \
    --max-loss-sol "${SHADOW_R2_MAX_LOSS_SOL:-0.015}" \
    --slippage-bps "${SHADOW_LAUNCH0U4_SLIPPAGE_BPS:-${SHADOW_LAUNCH0Q_SLIPPAGE_BPS:-100}}" \
    --fee-budget-lamports "${SHADOW_LAUNCH0U4_FEE_BUDGET_LAMPORTS:-410000}" \
    --exit-safety-bps "${SHADOW_LAUNCH0U4_EXIT_SAFETY_BPS:-10000}" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}" \
    --min-price-change-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS:-100}" \
    --max-sell-count-2s "${SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S:-0}" \
    --panic-drop-2s-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_PANIC_DROP_2S_BPS:-700}" \
    --edge-min-price-change-bps "${SHADOW_LAUNCH_ENTRY_EDGE_MIN_PRICE_CHANGE_BPS:-300}" \
    --edge-max-price-change-bps "${SHADOW_LAUNCH_ENTRY_EDGE_MAX_PRICE_CHANGE_BPS:-1500}" \
    --edge-max-signal-age-ms "${SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS:-8000}" \
    --edge-min-net-buy-pressure-5s "${SHADOW_LAUNCH_ENTRY_EDGE_MIN_NET_BUY_PRESSURE_5S:-1}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U4 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U4 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U4 active curve state hydration repair written to $OUT_DIR/summary.txt"
finish

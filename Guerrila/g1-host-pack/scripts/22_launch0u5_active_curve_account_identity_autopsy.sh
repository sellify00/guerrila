#!/usr/bin/env bash
# 22_launch0u5_active_curve_account_identity_autopsy.sh — no-wire active curve account identity autopsy
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U5 — Active Curve Account Identity Autopsy"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U5_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0U_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0u_active_bonding_curve_r3_candidate_gate' || true)"
    [[ -f "$LAUNCH0U_SUMMARY" ]] || fail "Launch0U5 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0U_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U5 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U5_REFRESH_U4:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0u4_active_curve_state_hydration_repair.sh" "$RUN_ROOT"
fi

if [[ -n "${SHADOW_LAUNCH0U5_LAUNCH0U_ROWS:-}" ]]; then
    LAUNCH0U_ROWS="$SHADOW_LAUNCH0U5_LAUNCH0U_ROWS"
elif [[ -f "$RUN_ROOT/22_launch0u4_active_curve_state_hydration_repair/rows.jsonl" ]]; then
    LAUNCH0U_ROWS="$RUN_ROOT/22_launch0u4_active_curve_state_hydration_repair/rows.jsonl"
else
    LAUNCH0U_ROWS="$RUN_ROOT/22_launch0u_active_bonding_curve_r3_candidate_gate/rows.jsonl"
fi

ACTIVITY_ROWS="${SHADOW_LAUNCH0U5_ACTIVITY_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_activity_rows.jsonl}"
STATE_ROWS="${SHADOW_LAUNCH0U5_STATE_ROWS:-$RUN_ROOT/launch0d_realtime_birth_decoder_rows.jsonl}"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
QUOTE_HYDRATOR_PATH="$PACK_DIR/scripts/launch_realtime_quote_state_hydrator.py"
OUT_DIR="${SHADOW_LAUNCH0U5_OUT_DIR:-$RUN_ROOT/22_launch0u5_active_curve_account_identity_autopsy}"
RPC_URL="${SHADOW_LAUNCH0U5_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$LAUNCH0U_ROWS" ]] || fail "Launch0U5 Launch0U rows missing: $LAUNCH0U_ROWS"
[[ -f "$ACTIVITY_ROWS" ]] || fail "Launch0U5 activity rows missing: $ACTIVITY_ROWS"
[[ -f "$STATE_ROWS" ]] || fail "Launch0U5 state rows missing: $STATE_ROWS"
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -f "$QUOTE_HYDRATOR_PATH" ]] || fail "Launch quote hydrator missing: $QUOTE_HYDRATOR_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0U5 RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_active_curve_account_identity_autopsy.py" \
    --run-root "$RUN_ROOT" \
    --launch0u-rows "$LAUNCH0U_ROWS" \
    --activity-rows "$ACTIVITY_ROWS" \
    --activity-rows "$STATE_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --quote-hydrator-path "$QUOTE_HYDRATOR_PATH" \
    --rpc-url "$RPC_URL" \
    --out-dir "$OUT_DIR" \
    --max-tx-fetches "${SHADOW_LAUNCH0U5_MAX_TX_FETCHES:-300}" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-0.01}" \
    --max-loss-sol "${SHADOW_R2_MAX_LOSS_SOL:-0.015}" \
    --slippage-bps "${SHADOW_LAUNCH0U5_SLIPPAGE_BPS:-${SHADOW_LAUNCH0Q_SLIPPAGE_BPS:-100}}" \
    --fee-budget-lamports "${SHADOW_LAUNCH0U5_FEE_BUDGET_LAMPORTS:-410000}" \
    --exit-safety-bps "${SHADOW_LAUNCH0U5_EXIT_SAFETY_BPS:-10000}" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U5 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U5 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U5 active curve account identity autopsy written to $OUT_DIR/summary.txt"
finish

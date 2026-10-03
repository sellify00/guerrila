#!/usr/bin/env bash
# 22_launch43_liquidity_cliff_sentinel.sh — replay/no-wire validation for liquidity-cliff exits
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch43 — Liquidity Cliff Sentinel Replay"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH43_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch43 run root missing"

OUT_DIR="${SHADOW_LAUNCH43_OUT_DIR:-$RUN_ROOT/22_launch43_liquidity_cliff_sentinel}"
python3 "$PACK_DIR/scripts/launch_liquidity_cliff_sentinel.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --drop-1s-bps "${SHADOW_LAUNCH_LIQUIDITY_CLIFF_DROP_1S_BPS:-1500}" \
    --drop-2s-bps "${SHADOW_LAUNCH_LIQUIDITY_CLIFF_DROP_2S_BPS:-2500}" \
    --drop-5s-bps "${SHADOW_LAUNCH_LIQUIDITY_CLIFF_DROP_5S_BPS:-4000}" \
    --exit-value-collapse-bps "${SHADOW_LAUNCH_EXIT_VALUE_COLLAPSE_BPS:-2000}" \
    --quote-weakening-checks "${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS:-2}" \
    --pre-arm-timeout-seconds "${SHADOW_LAUNCH_PRE_ARM_WEAK_TIMEOUT_SECONDS:-30}" \
    --pre-arm-min-gain-bps "${SHADOW_LAUNCH_PRE_ARM_MIN_GAIN_BPS:-500}"

pass "Launch43 liquidity cliff sentinel replay complete"
finish

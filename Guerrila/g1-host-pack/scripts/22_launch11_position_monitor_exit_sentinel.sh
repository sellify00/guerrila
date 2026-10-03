#!/usr/bin/env bash
# 22_launch11_position_monitor_exit_sentinel.sh — Launch R2-HOLD position exit sentinel

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch11 — Position Monitor Exit Sentinel"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH11_STEP:-22_launch11_position_monitor_exit_sentinel}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SNAPSHOTS_JSONL="${SHADOW_LAUNCH11_SNAPSHOTS_JSONL:-$RESULTS_ROOT/launch_position_snapshots.jsonl}"
SUMMARY_JSON="$RESULT_DIR/launch11_position_monitor_exit_sentinel_summary.json"
ROWS_JSONL="$RESULT_DIR/launch11_position_monitor_exit_sentinel_rows.jsonl"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"

python3 "$PACK_DIR/scripts/launch_position_exit_sentinel.py" \
    --snapshots-jsonl "$SNAPSHOTS_JSONL" \
    --summary-json "$SUMMARY_JSON" \
    --rows-jsonl "$ROWS_JSONL" \
    --entry-guard-seconds "${SHADOW_LAUNCH_EXIT_ENTRY_GUARD_SECONDS:-120}" \
    --hard-stop-bps "${SHADOW_LAUNCH_EXIT_HARD_STOP_BPS:-1000}" \
    --trailing-stop-bps "${SHADOW_LAUNCH_EXIT_TRAILING_STOP_BPS:-1500}" \
    --trailing-arm-gain-bps "${SHADOW_LAUNCH_EXIT_TRAILING_ARM_GAIN_BPS:-0}" \
    --panic-drop-10s-bps "${SHADOW_LAUNCH_EXIT_DROP_10S_BPS:-800}" \
    --panic-drop-60s-bps "${SHADOW_LAUNCH_EXIT_DROP_60S_BPS:-1500}" \
    --adaptive-floor-bps "${SHADOW_LAUNCH_EXIT_ADAPTIVE_FLOOR_BPS:-800}" \
    --adaptive-noise-multiplier "${SHADOW_LAUNCH_EXIT_ADAPTIVE_NOISE_MULTIPLIER:-3}" \
    --sell-pressure-ratio "${SHADOW_LAUNCH_EXIT_SELL_PRESSURE_RATIO:-1.5}" \
    --net-sell-pressure-threshold "${SHADOW_LAUNCH_EXIT_NET_SELL_PRESSURE_THRESHOLD:-2}" \
    --large-sell-vs-median-buy-ratio "${SHADOW_LAUNCH_EXIT_LARGE_SELL_VS_MEDIAN_BUY_RATIO:-2}" \
    --max-hold-seconds "${SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS:-3600}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_MAX_PRICE_IMPACT_BPS:-1500}" \
    --max-exit-slippage-bps "${SHADOW_LAUNCH_EXIT_MAX_SLIPPAGE_BPS:-2000}" \
    --liquidity-drop-bps "${SHADOW_LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS:-2000}" \
    | tee "$SUMMARY_TXT"

pass "Launch11 sentinel summary written to $SUMMARY_TXT"
finish

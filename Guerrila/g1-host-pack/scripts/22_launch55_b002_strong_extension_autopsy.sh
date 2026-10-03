#!/usr/bin/env bash
# 22_launch55_b002_strong_extension_autopsy.sh — compare B002 strong winners vs weak-extension losers.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch55 — B002 Strong Extension Autopsy"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
ROOTS="${SHADOW_LAUNCH55_ROOTS:-}"
if [[ -z "$ROOTS" ]]; then
    ROOTS="$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260601T150013Z"
    [[ -d "$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260603T071242Z" ]] && ROOTS="$ROOTS,$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260603T071242Z"
    [[ -d "$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260603T082112Z" ]] && ROOTS="$ROOTS,$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260603T082112Z"
fi
OUT_DIR="${SHADOW_LAUNCH55_OUT_DIR:-$RESULTS_ROOT/22_launch55_b002_strong_extension_autopsy}"

python3 "$PACK_DIR/scripts/launch_b002_plus_tools.py" autopsy \
    --roots "$ROOTS" \
    --out-dir "$OUT_DIR" \
    --strong-peak-bps "${SHADOW_LAUNCH55_STRONG_PEAK_BPS:-2200}" \
    --min-price-change-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_PRICE_CHANGE_BPS:-500}" \
    --min-exit-value-slope-2s-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_EXIT_VALUE_SLOPE_2S_BPS:-1}" \
    --min-net-buy-pressure-5s "${SHADOW_LAUNCH_B002_PLUS_MIN_NET_BUY_PRESSURE_5S:-1}" \
    --max-sell-count-2s "${SHADOW_LAUNCH_B002_PLUS_MAX_SELL_COUNT_2S:-0}" \
    --min-base-reserve-velocity-bps "${SHADOW_LAUNCH_B002_PLUS_MIN_BASE_RESERVE_VELOCITY_BPS:-0}"

pass "Launch55 B002 strong-extension autopsy complete: $OUT_DIR/summary.json"
finish

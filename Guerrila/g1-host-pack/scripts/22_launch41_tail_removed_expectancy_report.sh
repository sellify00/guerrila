#!/usr/bin/env bash
# 22_launch41_tail_removed_expectancy_report.sh — compare Launch expectancy before/after no-exit tail filter
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch41 — Tail-Removed Expectancy Report"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
OUT_DIR="${SHADOW_LAUNCH41_OUT_DIR:-$RESULTS_ROOT/22_launch41_tail_removed_expectancy_report}"

ARGS=(
    "$PACK_DIR/scripts/launch_tail_removed_expectancy_report.py"
    --results-root "$RESULTS_ROOT"
    --out-dir "$OUT_DIR"
    --max-roots "${SHADOW_LAUNCH41_MAX_ROOTS:-20}"
    --only-landed "${SHADOW_LAUNCH41_ONLY_LANDED:-true}"
    --max-spend-lamports "${SHADOW_LAUNCH41_MAX_SPEND_LAMPORTS:-10000000}"
    --min-base-reserve-position-ratio "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_POSITION_RATIO:-50}"
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-25000000}"
    --exit-quote-stability-checks "${SHADOW_LAUNCH_EXIT_QUOTE_STABILITY_CHECKS:-5}"
    --max-quote-zero-recent-count "${SHADOW_LAUNCH_EXIT_MAX_QUOTE_ZERO_RECENT_COUNT:-0}"
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1000}"
    --max-base-reserve-decline-bps "${SHADOW_LAUNCH_EXIT_MAX_BASE_RESERVE_DECLINE_BPS:-0}"
    --sol-usd "${SHADOW_R2_DAILY_SOL_USD:-${SHADOW_R1_DAILY_SOL_USD:-${SHADOW_RUN60_SOL_USD_EQUIVALENT:-0}}}"
    --daily-target-usd "${SHADOW_LAUNCH41_DAILY_TARGET_USD:-60}"
)

if [[ $# -gt 0 ]]; then
    for root in "$@"; do
        ARGS+=(--run-root "$root")
    done
fi

python3 "${ARGS[@]}"

pass "Launch41 tail-removed expectancy report complete"
finish

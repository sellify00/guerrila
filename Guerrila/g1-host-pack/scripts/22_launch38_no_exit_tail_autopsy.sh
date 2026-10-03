#!/usr/bin/env bash
# 22_launch38_no_exit_tail_autopsy.sh — autopsy R4 no-exit burn/close tails
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch38 — No-Exit Tail Autopsy"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH38_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch38 run root missing"
OUT_DIR="${SHADOW_LAUNCH38_OUT_DIR:-$RUN_ROOT/22_launch38_no_exit_tail_autopsy}"

ARGS=(
    "$PACK_DIR/scripts/launch_no_exit_tail_autopsy.py"
    --run-root "$RUN_ROOT"
    --out-dir "$OUT_DIR"
    --max-spend-lamports "${SHADOW_LAUNCH38_MAX_SPEND_LAMPORTS:-10000000}"
    --min-base-reserve-position-ratio "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_POSITION_RATIO:-50}"
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-25000000}"
    --max-quote-zero-recent-count "${SHADOW_LAUNCH_EXIT_MAX_QUOTE_ZERO_RECENT_COUNT:-0}"
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1000}"
    --max-base-reserve-decline-bps "${SHADOW_LAUNCH_EXIT_MAX_BASE_RESERVE_DECLINE_BPS:-0}"
    --exit-quote-stability-checks "${SHADOW_LAUNCH_EXIT_QUOTE_STABILITY_CHECKS:-5}"
)

if value_is_true "${SHADOW_LAUNCH38_INCLUDE_ALL_ATTEMPTS:-false}"; then
    ARGS+=(--include-all-attempts)
fi

python3 "${ARGS[@]}"

pass "Launch38 no-exit tail autopsy complete"
finish

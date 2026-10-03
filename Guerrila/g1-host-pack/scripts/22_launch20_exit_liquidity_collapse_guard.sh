#!/usr/bin/env bash
# 22_launch20_exit_liquidity_collapse_guard.sh — Launch R2-HOLD exit liquidity guard

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch20 — Exit Liquidity Collapse Guard"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
[[ -f "$PACK_DIR/env.g1.sh" ]] && source_crlf_safe "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RUN_ROOT="${1:-${SHADOW_LAUNCH20_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH20_OUT_DIR:-$RUN_ROOT/22_launch20_exit_liquidity_collapse_guard}"

python3 "$PACK_DIR/scripts/launch_exit_liquidity_guard.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}" \
    --quote-stability-max-degrade-bps "${SHADOW_LAUNCH_EXIT_QUOTE_STABILITY_MAX_DEGRADE_BPS:-1500}" \
    --require-no-exit-loss-floor "${SHADOW_LAUNCH_NO_EXIT_LOSS_FLOOR_REQUIRED:-true}"

finish

#!/usr/bin/env bash
# 22_launch48_b002_hold_policy_replay.sh — replay B002 clone HOLD policy over captured attempts
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch48 — B002 HOLD Policy Replay"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH48_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
RUN_ROOT="$(printf '%s' "$RUN_ROOT" | tr -d '\r')"
[[ -n "$RUN_ROOT" ]] || fail "Launch48 run root missing"
OUT_DIR="${SHADOW_LAUNCH48_OUT_DIR:-$RUN_ROOT/22_launch48_b002_hold_policy_replay}"
OUT_DIR="$(printf '%s' "$OUT_DIR" | tr -d '\r')"

python3 "$PACK_DIR/scripts/launch_b002_hold_policy_replay.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --quote-weakening-confirm-checks "${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS:-3}" \
    --quote-weakening-confirm-exit-count "${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT:-6}" \
    --executable-exit-value-drop-from-high-bps "${SHADOW_LAUNCH_EXIT_VALUE_COLLAPSE_BPS:-2000}" \
    --base-reserve-drop-5s-bps "${SHADOW_LAUNCH_LIQUIDITY_CLIFF_DROP_5S_BPS:-3000}" \
    --exit-price-impact-bps-max "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}"

pass "Launch48 B002 HOLD policy replay complete"
finish

#!/usr/bin/env bash
# 22_launch50_no_exit_stress_preflight.sh — replay reserve-stress exit survivability before entry
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch50 — No-Exit Stress Preflight"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH50_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch50 run root missing"

OUT_DIR="${SHADOW_LAUNCH50_OUT_DIR:-$RUN_ROOT/22_launch50_no_exit_stress_preflight}"
ARGS=(
    "$PACK_DIR/scripts/launch_no_exit_stress_preflight.py"
    --run-root "$RUN_ROOT"
    --out-dir "$OUT_DIR"
    --max-spend-lamports "${SHADOW_LAUNCH50_MAX_SPEND_LAMPORTS:-10000000}"
    --stress-drop-bps "${SHADOW_LAUNCH50_STRESS_DROP_BPS:-2500,5000,7500,9000,9500}"
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH50_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-25000000}"
    --max-stress-price-impact-bps "${SHADOW_LAUNCH50_MAX_STRESS_PRICE_IMPACT_BPS:-3000}"
    --max-impact-under-50pct-bps "${SHADOW_LAUNCH50_MAX_IMPACT_UNDER_50PCT_BPS:-3000}"
)

if [[ -n "${SHADOW_LAUNCH50_ATTEMPT:-}" ]]; then
    ARGS+=(--attempt "$SHADOW_LAUNCH50_ATTEMPT")
elif [[ $# -gt 1 ]]; then
    ARGS+=(--attempt "$2")
fi

python3 "${ARGS[@]}"

pass "Launch50 no-exit stress preflight complete"
finish

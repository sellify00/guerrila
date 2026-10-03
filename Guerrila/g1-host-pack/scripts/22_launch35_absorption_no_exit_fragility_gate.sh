#!/usr/bin/env bash
# 22_launch35_absorption_no_exit_fragility_gate.sh — audit absorption and no-exit fragility gates
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch35 — Absorption / No-Exit Fragility Gate"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH35_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch35 run root missing"
OUT_DIR="${SHADOW_LAUNCH35_OUT_DIR:-$RUN_ROOT/22_launch35_absorption_no_exit_fragility_gate}"
MAX_SPEND_LAMPORTS="${SHADOW_LAUNCH35_MAX_SPEND_LAMPORTS:-10000000}"

python3 "$PACK_DIR/scripts/launch_absorption_no_exit_fragility_gate.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --max-spend-lamports "$MAX_SPEND_LAMPORTS" \
    --min-base-reserve-position-ratio "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_POSITION_RATIO:-20}" \
    --min-reserve-after-exit-position-ratio "${SHADOW_LAUNCH_EXIT_MIN_RESERVE_AFTER_EXIT_POSITION_RATIO:-10}" \
    --max-quote-zero-recent-count "${SHADOW_LAUNCH_EXIT_MAX_QUOTE_ZERO_RECENT_COUNT:-0}" \
    --min-absorption-score-after-sell "${SHADOW_LAUNCH_EXIT_MIN_ABSORPTION_SCORE_AFTER_SELL:-50}"

pass "Launch35 absorption / no-exit fragility gate audit complete"
finish

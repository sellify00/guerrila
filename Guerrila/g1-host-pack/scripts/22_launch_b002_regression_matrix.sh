#!/usr/bin/env bash
# 22_launch_b002_regression_matrix.sh — no-wire regression suite for B002 money-lane promotion.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch B002 — Regression Matrix"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_LAUNCH_B002_REGRESSION_OUT_DIR:-$RESULTS_ROOT/22_launch_b002_regression_matrix_$RUN_ID}"
mkdir -p "$OUT_DIR"

B002_ROOT="${SHADOW_LAUNCH_B002_ROOT:-$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260601T150013Z}"
QW_ROOT="${SHADOW_LAUNCH_B002_QW_TAIL_ROOT:-$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260603T051614Z}"
QW_ATTEMPT="${SHADOW_LAUNCH_B002_QW_TAIL_ATTEMPT:-attempt_0001_bb0baa780066}"

run_step() {
    local name="$1"
    shift
    set +e
    "$@" >"$OUT_DIR/$name.log" 2>&1
    local rc=$?
    set -e
    printf '%s:%s\n' "$name" "$rc" >>"$OUT_DIR/steps.rc"
    if [[ $rc -eq 0 ]]; then
        pass "$name"
    else
        fail "$name rc=$rc (see $OUT_DIR/$name.log)"
    fi
}

: >"$OUT_DIR/steps.rc"

run_step b002_replay "$SCRIPT_DIR/22_launch_b002_replay_frozen.sh" "$B002_ROOT"
run_step monitor_exception_rescue "$SCRIPT_DIR/22_launch30_monitor_exception_replay.sh"
run_step no_exit_quarantine "$SCRIPT_DIR/22_launch32_no_exit_auto_quarantine_replay.sh"

if [[ -d "$QW_ROOT" ]]; then
    run_step quote_weakening_order env \
        SHADOW_LAUNCH52_OUT_DIR="$OUT_DIR/quote_weakening_order_out" \
        "$SCRIPT_DIR/22_launch52_quote_weakening_confirmation_order_replay.sh" "$QW_ROOT" "$QW_ATTEMPT"
else
    warn "Skipping quote_weakening_order; missing root $QW_ROOT"
    printf '%s:%s\n' "quote_weakening_order_skipped_missing_root" "1" >>"$OUT_DIR/steps.rc"
fi

STEP_ARGS=()
while IFS= read -r line; do
    STEP_ARGS+=(--step "$line")
done <"$OUT_DIR/steps.rc"

python3 "$PACK_DIR/scripts/launch_b002_frozen_tools.py" regression-summary \
    --out-dir "$OUT_DIR" \
    "${STEP_ARGS[@]}"

pass "B002 regression matrix complete: $OUT_DIR/summary.json"
finish

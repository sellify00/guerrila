#!/usr/bin/env bash
# 22_launch18_r2_hold_cost_floor_gate.sh — Launch R2-HOLD true cost-floor gate

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch18 — R2-HOLD Cost Floor Gate"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
[[ -f "$PACK_DIR/env.g1.sh" ]] && source_crlf_safe "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RUN_ROOT="${1:-${SHADOW_LAUNCH18_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH18_OUT_DIR:-$RUN_ROOT/22_launch18_r2_hold_cost_floor_gate}"
PANIC_BPS="${SHADOW_LAUNCH_COST_FLOOR_PANIC_EXIT_BUFFER_BPS:-${SHADOW_LAUNCH_EXIT_DROP_10S_BPS:-800}}"

python3 "$PACK_DIR/scripts/launch_r2_hold_cost_floor_gate.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --panic-exit-buffer-bps "$PANIC_BPS"

finish

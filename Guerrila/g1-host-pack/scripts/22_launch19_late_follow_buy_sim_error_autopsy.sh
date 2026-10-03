#!/usr/bin/env bash
# 22_launch19_late_follow_buy_sim_error_autopsy.sh — split late-follow buy simulation failures

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch19 — Late-Follow Buy Sim Error Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
[[ -f "$PACK_DIR/env.g1.sh" ]] && source_crlf_safe "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RUN_ROOT="${1:-${SHADOW_LAUNCH19_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH19_OUT_DIR:-$RUN_ROOT/22_launch19_late_follow_buy_sim_error_autopsy}"

python3 "$PACK_DIR/scripts/launch_late_follow_buy_sim_error_autopsy.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR"

finish

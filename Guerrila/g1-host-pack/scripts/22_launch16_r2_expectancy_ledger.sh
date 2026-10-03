#!/usr/bin/env bash
# 22_launch16_r2_expectancy_ledger.sh — aggregate Launch R2-HOLD expectancy

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch16 — R2-HOLD Expectancy Ledger"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
[[ -f "$PACK_DIR/env.g1.sh" ]] && source_crlf_safe "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${1:-${SHADOW_LAUNCH16_EXPECTANCY_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH16_EXPECTANCY_OUT_DIR:-$RESULTS_ROOT/launch_r2_expectancy_ledger}"

python3 "$PACK_DIR/scripts/launch_r2_expectancy_ledger.py" \
    --results-root "$RESULTS_ROOT" \
    --out-dir "$OUT_DIR"

finish

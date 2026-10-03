#!/usr/bin/env bash
# 90_deriv5_money_gate.sh — unified no-wire derivative money gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv5 — Derivatives Money Gate"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
POSTSHOCK_ROWS="${SHADOW_DERIV5_POSTSHOCK_ROWS:-$RESULTS_ROOT/90_deriv3_postshock_perp_paper_runner/rows.jsonl}"
BASIS_ROWS="${SHADOW_DERIV5_BASIS_ROWS:-$RESULTS_ROOT/90_deriv4_basis_fair_value_scanner/rows.jsonl}"
IMPULSE_ROWS="${SHADOW_DERIV5_IMPULSE_ROWS:-$RESULTS_ROOT/90_deriv6_perp_impulse_watch/rows.jsonl}"
ROLLING_ROWS="${SHADOW_DERIV5_ROLLING_ROWS:-$RESULTS_ROOT/90_deriv7_rolling_impulse_trigger_replay/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV5_OUT_DIR:-$RESULTS_ROOT/90_deriv5_money_gate}"

ARGS=()
[[ -f "$POSTSHOCK_ROWS" ]] && ARGS+=(--postshock-rows "$POSTSHOCK_ROWS")
[[ -f "$BASIS_ROWS" ]] && ARGS+=(--basis-rows "$BASIS_ROWS")
[[ -f "$IMPULSE_ROWS" ]] && ARGS+=(--impulse-rows "$IMPULSE_ROWS")
[[ -f "$ROLLING_ROWS" ]] && ARGS+=(--rolling-rows "$ROLLING_ROWS")
[[ ${#ARGS[@]} -gt 0 ]] || fail "Deriv5 input rows missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" money-gate \
    "${ARGS[@]}" \
    --out-dir "$OUT_DIR" \
    --max-loss-usd "${SHADOW_DERIV_MAX_LOSS_USD:-10}"

pass "Deriv5 money gate written to $OUT_DIR/summary.txt"
finish

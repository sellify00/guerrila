#!/usr/bin/env bash
# 90_deriv4_basis_fair_value_scanner.sh — no-wire cross-venue perp basis/funding scanner
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv4 — Basis/Fair-Value Scanner"

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
COST_ROWS="${SHADOW_DERIV4_COST_ROWS:-$RESULTS_ROOT/90_deriv1_fee_funding_cost_model/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV4_OUT_DIR:-$RESULTS_ROOT/90_deriv4_basis_fair_value_scanner}"

[[ -f "$COST_ROWS" ]] || fail "Deriv4 cost rows missing: $COST_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" basis-scan \
    --cost-rows "$COST_ROWS" \
    --out-dir "$OUT_DIR" \
    --min-basis-net-bps "${SHADOW_DERIV_MIN_BASIS_NET_BPS:-10}" \
    --min-funding-spread-bps "${SHADOW_DERIV_MIN_FUNDING_SPREAD_BPS:-5}"

pass "Deriv4 basis scanner written to $OUT_DIR/summary.txt"
finish

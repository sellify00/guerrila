#!/usr/bin/env bash
# 90_deriv1_fee_funding_cost_model.sh — no-wire derivative fee/funding cost model
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv1 — Fee/Funding Cost Model"

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
UNIVERSE_ROWS="${SHADOW_DERIV1_UNIVERSE_ROWS:-$RESULTS_ROOT/90_deriv0_market_universe_probe/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV1_OUT_DIR:-$RESULTS_ROOT/90_deriv1_fee_funding_cost_model}"

[[ -f "$UNIVERSE_ROWS" ]] || fail "Deriv1 universe rows missing: $UNIVERSE_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" cost-model \
    --universe-rows "$UNIVERSE_ROWS" \
    --out-dir "$OUT_DIR" \
    --notional-usd "${SHADOW_DERIV_NOTIONAL_USD:-100}" \
    --hold-hours "${SHADOW_DERIV_HOLD_HOURS:-1}" \
    --default-spread-bps "${SHADOW_DERIV_DEFAULT_SPREAD_BPS:-2}" \
    --min-profit-bps "${SHADOW_DERIV_MIN_PROFIT_BPS:-25}"

pass "Deriv1 cost model written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 90_deriv3_postshock_perp_paper_runner.sh — no-wire mapped-signal perp paper gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv3 — Post-Shock Perp Paper Runner"

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
MAPPED_ROWS="${SHADOW_DERIV3_MAPPED_ROWS:-$RESULTS_ROOT/90_deriv2_onchain_signal_to_perp_mapper/rows.jsonl}"
COST_ROWS="${SHADOW_DERIV3_COST_ROWS:-$RESULTS_ROOT/90_deriv1_fee_funding_cost_model/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV3_OUT_DIR:-$RESULTS_ROOT/90_deriv3_postshock_perp_paper_runner}"

[[ -f "$MAPPED_ROWS" ]] || fail "Deriv3 mapped rows missing: $MAPPED_ROWS"
[[ -f "$COST_ROWS" ]] || fail "Deriv3 cost rows missing: $COST_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" postshock-paper \
    --mapped-signal-rows "$MAPPED_ROWS" \
    --cost-rows "$COST_ROWS" \
    --out-dir "$OUT_DIR" \
    --min-net-edge-bps "${SHADOW_DERIV_MIN_NET_EDGE_BPS:-10}"

pass "Deriv3 post-shock paper output written to $OUT_DIR/summary.txt"
finish

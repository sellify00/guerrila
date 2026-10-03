#!/usr/bin/env bash
# 90_deriv6_perp_impulse_watch.sh — no-wire listed-perp impulse watcher
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv6 — Listed Perp Impulse Watch"

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
COST_ROWS="${SHADOW_DERIV6_COST_ROWS:-$RESULTS_ROOT/90_deriv1_fee_funding_cost_model/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV6_OUT_DIR:-$RESULTS_ROOT/90_deriv6_perp_impulse_watch}"

ARGS=()
[[ -f "$COST_ROWS" ]] && ARGS+=(--cost-rows "$COST_ROWS")

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" perp-impulse-watch \
    --out-dir "$OUT_DIR" \
    "${ARGS[@]}" \
    --duration-seconds "${SHADOW_DERIV_IMPULSE_DURATION_SECONDS:-60}" \
    --interval-seconds "${SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS:-5}" \
    --min-impulse-bps "${SHADOW_DERIV_MIN_IMPULSE_BPS:-20}" \
    --min-net-edge-bps "${SHADOW_DERIV_MIN_NET_EDGE_BPS:-10}" \
    --min-day-notional-volume "${SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME:-1000000}"

pass "Deriv6 impulse watch written to $OUT_DIR/summary.txt"
finish

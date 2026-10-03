#!/usr/bin/env bash
# 90_deriv7_rolling_impulse_trigger_replay.sh — no-wire live-detectable impulse replay
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv7 — Rolling Impulse Trigger Replay"

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
SAMPLES_ROWS="${SHADOW_DERIV7_SAMPLES_ROWS:-$RESULTS_ROOT/90_deriv6_perp_impulse_watch/samples.jsonl}"
COST_ROWS="${SHADOW_DERIV7_COST_ROWS:-$RESULTS_ROOT/90_deriv1_fee_funding_cost_model/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV7_OUT_DIR:-$RESULTS_ROOT/90_deriv7_rolling_impulse_trigger_replay}"

[[ -f "$SAMPLES_ROWS" ]] || fail "Deriv7 sample rows missing: $SAMPLES_ROWS"
ARGS=(--samples-rows "$SAMPLES_ROWS")
[[ -f "$COST_ROWS" ]] && ARGS+=(--cost-rows "$COST_ROWS")
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" rolling-impulse-replay \
    "${ARGS[@]}" \
    --out-dir "$OUT_DIR" \
    --trigger-window-seconds "${SHADOW_DERIV_TRIGGER_WINDOW_SECONDS:-60}" \
    --min-trigger-bps "${SHADOW_DERIV_MIN_TRIGGER_BPS:-35}" \
    --min-net-edge-bps "${SHADOW_DERIV_MIN_NET_EDGE_BPS:-10}" \
    --min-day-notional-volume "${SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME:-1000000}" \
    --adverse-stop-bps "${SHADOW_DERIV_ADVERSE_STOP_BPS:-25}" \
    --decay-exit-fraction "${SHADOW_DERIV_DECAY_EXIT_FRACTION:-0.30}" \
    --max-hold-seconds "${SHADOW_DERIV_MAX_HOLD_SECONDS:-900}"

pass "Deriv7 rolling trigger replay written to $OUT_DIR/summary.txt"
finish

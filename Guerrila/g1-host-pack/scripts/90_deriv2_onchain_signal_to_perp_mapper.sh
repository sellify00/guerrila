#!/usr/bin/env bash
# 90_deriv2_onchain_signal_to_perp_mapper.sh — map Shadow signal rows to listed perps
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv2 — On-Chain Signal to Perp Mapper"

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
UNIVERSE_ROWS="${SHADOW_DERIV2_UNIVERSE_ROWS:-$RESULTS_ROOT/90_deriv0_market_universe_probe/rows.jsonl}"
OUT_DIR="${SHADOW_DERIV2_OUT_DIR:-$RESULTS_ROOT/90_deriv2_onchain_signal_to_perp_mapper}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$UNIVERSE_ROWS" ]] || fail "Deriv2 universe rows missing: $UNIVERSE_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=()
if [[ -n "${SHADOW_DERIV_SIGNAL_ROWS:-}" ]]; then
    IFS=',' read -r -a SIGNAL_ROWS <<<"$SHADOW_DERIV_SIGNAL_ROWS"
    for path in "${SIGNAL_ROWS[@]}"; do
        [[ -f "$path" ]] && ARGS+=(--signal-rows "$path")
    done
else
    for suffix in \
        '/22_launch70_r5_money_gate/rows.jsonl' \
        '/22_launch57_b002_plus_active_curve_no_wire/rows.jsonl' \
        '/22_launch0u8_active_curve_fresh_state_filter/rows.jsonl' \
        '/66ctv2_continuation_fresh_runtime_pressure_recompute/continuation_fresh_runtime_pressure_rows.jsonl' \
        '/66ctv_continuation_fresh_thesis_revalidation/continuation_fresh_thesis_revalidation_rows.jsonl' \
        '/66ctv_continuation_fresh_thesis_revalidation/selected_replay_rows.jsonl'
    do
        candidate="$(latest_file_under_results "$suffix")"
        [[ -s "$candidate" ]] && ARGS+=(--signal-rows "$candidate")
    done
    while IFS= read -r candidate; do
        [[ -s "$candidate" ]] && ARGS+=(--signal-rows "$candidate")
    done < <(
        find "$RESULTS_ROOT" -type f \( \
            -name 'proposal_rows.jsonl' -o \
            -name 'launch10_hot_candidate_events.jsonl' -o \
            -name 'launch10_hot_decisions.jsonl' -o \
            -name 'continuation_fresh_runtime_pressure_rows.jsonl' -o \
            -name 'continuation_fresh_thesis_revalidation_rows.jsonl' -o \
            -name 'selected_replay_rows.jsonl' \
        \) -size +0c -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 50 | cut -d' ' -f2-
    )
fi

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" signal-map \
    --universe-rows "$UNIVERSE_ROWS" \
    --out-dir "$OUT_DIR" \
    --max-signal-rows "${SHADOW_DERIV_MAX_SIGNAL_ROWS:-50000}" \
    "${ARGS[@]}"

pass "Deriv2 signal map written to $OUT_DIR/summary.txt"
finish

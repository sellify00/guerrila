#!/usr/bin/env bash
# 22_launch0r_route_support_and_warmup_reconcile.sh — split Launch0Q route coverage and entry-quality warmup blockers
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0R — Route Support / Warmup Reconcile"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0R_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    QUOTE_STATE_SUMMARY="${SHADOW_LAUNCH0R_QUOTE_STATE_SUMMARY:-$(latest_file launch0q_realtime_launch_quote_state_summary.json)}"
    [[ -f "$QUOTE_STATE_SUMMARY" ]] || fail "Launch0Q quote-state summary missing"
    RUN_ROOT="$(dirname "$QUOTE_STATE_SUMMARY")"
else
    QUOTE_STATE_SUMMARY="${SHADOW_LAUNCH0R_QUOTE_STATE_SUMMARY:-$RUN_ROOT/launch0q_realtime_launch_quote_state_summary.json}"
fi

QUOTE_STATE_ROWS="${SHADOW_LAUNCH0R_QUOTE_STATE_ROWS:-$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl}"
ENTRY_QUALITY_SUMMARY="${SHADOW_LAUNCH0R_ENTRY_QUALITY_SUMMARY:-$RUN_ROOT/22_launch24_entry_quality_gate/summary.json}"
OUT_DIR="${SHADOW_LAUNCH0R_OUT_DIR:-$RUN_ROOT/22_launch0r_route_support_and_warmup_reconcile}"

[[ -d "$RUN_ROOT" ]] || fail "Launch0R run root missing: $RUN_ROOT"
[[ -f "$QUOTE_STATE_SUMMARY" ]] || fail "Launch0Q summary missing: $QUOTE_STATE_SUMMARY"
[[ -f "$QUOTE_STATE_ROWS" ]] || fail "Launch0Q rows missing: $QUOTE_STATE_ROWS"
[[ -f "$ENTRY_QUALITY_SUMMARY" ]] || fail "Launch24 summary missing: $ENTRY_QUALITY_SUMMARY"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_route_support_warmup_reconcile.py" \
    --run-root "$RUN_ROOT" \
    --quote-state-rows "$QUOTE_STATE_ROWS" \
    --quote-state-summary "$QUOTE_STATE_SUMMARY" \
    --entry-quality-summary "$ENTRY_QUALITY_SUMMARY" \
    --out-dir "$OUT_DIR" \
    --min-price-change-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS:-100}" \
    --max-sell-count-2s "${SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S:-0}" \
    --panic-drop-2s-bps "${SHADOW_LAUNCH_EXIT_DROP_2S_BPS:-700}"

pass "Launch0R route/warmup reconcile written to $OUT_DIR/summary.txt"
finish

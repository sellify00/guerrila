#!/usr/bin/env bash
# 22_launch0h_launch_source_coverage_reconcile.sh — reconcile global launch census vs hot executor coverage

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0H — Source Coverage Reconcile"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_path_match() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -path "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

LATEST_REALTIME_DECODER_SUMMARY="$(latest_file launch0d_realtime_birth_decoder_summary.json)"
LATEST_BACKFILL_CENSUS_SUMMARY="$(latest_file launch0g_global_launch_birth_census_summary.json)"
CENSUS_SUMMARY="${SHADOW_LAUNCH0H_CENSUS_SUMMARY:-${LATEST_REALTIME_DECODER_SUMMARY:-$LATEST_BACKFILL_CENSUS_SUMMARY}}"
GAP_SUMMARY="${SHADOW_LAUNCH0H_GAP_SUMMARY:-$(latest_file launch0i_launch_decode_gap_report_summary.json)}"
QUOTE_STATE_SUMMARY="${SHADOW_LAUNCH0H_QUOTE_STATE_SUMMARY:-}"
ENTRY_QUALITY_SUMMARY="${SHADOW_LAUNCH0H_ENTRY_QUALITY_SUMMARY:-}"

[[ -f "$CENSUS_SUMMARY" ]] || fail "Launch0G census summary missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

RUN_ROOT="$(dirname "$CENSUS_SUMMARY")"
if [[ -z "$QUOTE_STATE_SUMMARY" ]]; then
    SAME_ROOT_QUOTE_STATE_SUMMARY="$RUN_ROOT/launch0q_realtime_launch_quote_state_summary.json"
    if [[ -f "$SAME_ROOT_QUOTE_STATE_SUMMARY" ]]; then
        QUOTE_STATE_SUMMARY="$SAME_ROOT_QUOTE_STATE_SUMMARY"
    else
        QUOTE_STATE_SUMMARY="$(latest_file launch0q_realtime_launch_quote_state_summary.json)"
    fi
fi
if [[ -z "$ENTRY_QUALITY_SUMMARY" ]]; then
    SAME_ROOT_ENTRY_QUALITY_SUMMARY="$RUN_ROOT/22_launch24_entry_quality_gate/summary.json"
    if [[ -f "$SAME_ROOT_ENTRY_QUALITY_SUMMARY" ]]; then
        ENTRY_QUALITY_SUMMARY="$SAME_ROOT_ENTRY_QUALITY_SUMMARY"
    else
        ENTRY_QUALITY_SUMMARY="$(latest_path_match '*/22_launch24_entry_quality_gate/summary.json')"
    fi
fi
HOT_SUMMARY="${SHADOW_LAUNCH0H_HOT_SUMMARY:-}"
if [[ -z "$HOT_SUMMARY" ]]; then
    SAME_ROOT_HOT_SUMMARY="$RUN_ROOT/launch10_hot_stream_executor_summary.json"
    if [[ -f "$SAME_ROOT_HOT_SUMMARY" ]]; then
        HOT_SUMMARY="$SAME_ROOT_HOT_SUMMARY"
    elif [[ "$(basename "$CENSUS_SUMMARY")" != "launch0d_realtime_birth_decoder_summary.json" ]]; then
        HOT_SUMMARY="$(latest_file launch10_hot_stream_executor_summary.json)"
    fi
fi
SUMMARY_JSON="$RUN_ROOT/launch0h_launch_source_coverage_reconcile_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0h_coverage_reconcile_summary.txt"

ARGS=(
    "$PACK_DIR/scripts/launch_coverage_tools.py" reconcile
    --census-summary "$CENSUS_SUMMARY"
    --min-coverage-pct "${SHADOW_LAUNCH0H_MIN_COVERAGE_PCT:-20}"
    --summary-json "$SUMMARY_JSON"
    --summary-txt "$SUMMARY_TXT"
)
[[ -f "$HOT_SUMMARY" ]] && ARGS+=(--hot-summary "$HOT_SUMMARY")
[[ -f "$GAP_SUMMARY" ]] && ARGS+=(--gap-summary "$GAP_SUMMARY")
[[ -f "$QUOTE_STATE_SUMMARY" ]] && ARGS+=(--quote-state-summary "$QUOTE_STATE_SUMMARY")
[[ -f "$ENTRY_QUALITY_SUMMARY" ]] && ARGS+=(--entry-quality-summary "$ENTRY_QUALITY_SUMMARY")

python3 "${ARGS[@]}"

info "Launch0H reconcile summary written to $SUMMARY_JSON"

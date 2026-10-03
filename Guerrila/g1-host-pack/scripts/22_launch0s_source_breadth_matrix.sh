#!/usr/bin/env bash
# 22_launch0s_source_breadth_matrix.sh — compare launch source breadth across available inputs

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0S — Source Breadth Matrix"

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

REALTIME_SUMMARY="${SHADOW_LAUNCH0S_REALTIME_SUMMARY:-$(latest_file launch0rt_realtime_birth_rate_probe_summary.json)}"
REALTIME_DECODER_SUMMARY="${SHADOW_LAUNCH0S_REALTIME_DECODER_SUMMARY:-$(latest_file launch0d_realtime_birth_decoder_summary.json)}"
CENSUS_SUMMARY="${SHADOW_LAUNCH0S_CENSUS_SUMMARY:-$(latest_file launch0g_global_launch_birth_census_summary.json)}"
HOT_SUMMARY="${SHADOW_LAUNCH0S_HOT_SUMMARY:-$(latest_file launch10_hot_stream_executor_summary.json)}"
PROCESSED_SUMMARY="${SHADOW_LAUNCH0S_PROCESSED_SUMMARY:-}"
ENHANCED_SUMMARY="${SHADOW_LAUNCH0S_ENHANCED_SUMMARY:-$(latest_file launch0p_helius_processed_enhanced_birth_source_summary.json)}"
RUN_ROOT="${SHADOW_LAUNCH0S_RUN_ROOT:-$(dirname "${REALTIME_SUMMARY:-$CENSUS_SUMMARY}")}"
SUMMARY_JSON="$RUN_ROOT/launch0s_source_breadth_matrix_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0s_source_breadth_matrix_summary.txt"

ARGS=(
    "$PACK_DIR/scripts/launch_coverage_tools.py" source-matrix
    --summary-json "$SUMMARY_JSON"
    --summary-txt "$SUMMARY_TXT"
)
[[ -f "$REALTIME_SUMMARY" ]] && ARGS+=(--realtime-summary "$REALTIME_SUMMARY")
[[ -f "$REALTIME_DECODER_SUMMARY" ]] && ARGS+=(--realtime-decoder-summary "$REALTIME_DECODER_SUMMARY")
[[ -f "$CENSUS_SUMMARY" ]] && ARGS+=(--census-summary "$CENSUS_SUMMARY")
[[ -f "$HOT_SUMMARY" ]] && ARGS+=(--hot-summary "$HOT_SUMMARY")
[[ -f "$PROCESSED_SUMMARY" ]] && ARGS+=(--processed-summary "$PROCESSED_SUMMARY")
[[ -f "$ENHANCED_SUMMARY" ]] && ARGS+=(--enhanced-summary "$ENHANCED_SUMMARY")

python3 "${ARGS[@]}"

info "Launch0S source matrix written to $SUMMARY_JSON"

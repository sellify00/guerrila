#!/usr/bin/env bash
# 22_launch0i_launch_decode_gap_report.sh — explain Launch source/decode/admission loss buckets

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0I — Decode Gap Report"

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

CENSUS_SUMMARY="${SHADOW_LAUNCH0I_CENSUS_SUMMARY:-$(latest_file launch0g_global_launch_birth_census_summary.json)}"
RUN_ROOT="$(dirname "$CENSUS_SUMMARY")"
LS13A_SUMMARY="${SHADOW_LAUNCH0I_LS13A_SUMMARY:-$RUN_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_summary.json}"
LAUNCH0_SUMMARY="${SHADOW_LAUNCH0I_LAUNCH0_SUMMARY:-$RUN_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_summary.json}"
LAUNCH0_ROWS="${SHADOW_LAUNCH0I_LAUNCH0_ROWS:-$RUN_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_rows.jsonl}"
HOT_SUMMARY="${SHADOW_LAUNCH0I_HOT_SUMMARY:-$(latest_file launch10_hot_stream_executor_summary.json)}"
HOT_DECISIONS="${SHADOW_LAUNCH0I_HOT_DECISIONS:-$(latest_file launch10_hot_decisions.jsonl)}"
SUMMARY_JSON="$RUN_ROOT/launch0i_launch_decode_gap_report_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0i_decode_gap_summary.txt"

[[ -f "$CENSUS_SUMMARY" ]] || fail "Launch0G census summary missing"
[[ -f "$LS13A_SUMMARY" ]] || fail "LS13A summary missing"
[[ -f "$LAUNCH0_SUMMARY" ]] || fail "Launch0 summary missing"
[[ -f "$LAUNCH0_ROWS" ]] || fail "Launch0 rows missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=(
    "$PACK_DIR/scripts/launch_coverage_tools.py" gap
    --run-root "$RUN_ROOT"
    --census-summary "$CENSUS_SUMMARY"
    --ls13a-summary "$LS13A_SUMMARY"
    --launch0-summary "$LAUNCH0_SUMMARY"
    --launch0-rows "$LAUNCH0_ROWS"
    --summary-json "$SUMMARY_JSON"
    --summary-txt "$SUMMARY_TXT"
)
[[ -f "$HOT_SUMMARY" ]] && ARGS+=(--hot-summary "$HOT_SUMMARY")
[[ -f "$HOT_DECISIONS" ]] && ARGS+=(--hot-decision-rows "$HOT_DECISIONS")

python3 "${ARGS[@]}"

info "Launch0I gap summary written to $SUMMARY_JSON"

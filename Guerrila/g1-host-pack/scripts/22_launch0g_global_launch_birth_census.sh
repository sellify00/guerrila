#!/usr/bin/env bash
# 22_launch0g_global_launch_birth_census.sh — no-wire Launch source coverage census

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0G — Global Launch Birth Census"

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

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH0G_WINDOW_SECONDS:-3600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_ROOT="${SHADOW_LAUNCH0G_RUN_ROOT:-$RESULTS_ROOT/run_launch0g_global_launch_birth_census_${WINDOW_SECONDS}s_${RUN_STAMP}}"
CHAIN_ESTIMATE_PER_HOUR="${SHADOW_LAUNCH0G_CHAIN_ESTIMATE_PER_HOUR:-1000}"

mkdir -p "$RUN_ROOT"

export SHADOW_RESULTS_DIR="$RUN_ROOT"
export SHADOW_LAUNCH0_FILTER_MODE="${SHADOW_LAUNCH0_FILTER_MODE:-nofilter}"
export SHADOW_RUNLS13A_FILTER_MODE="${SHADOW_RUNLS13A_FILTER_MODE:-nofilter}"

info "Running no-wire broad Launch0 census for ${WINDOW_SECONDS}s (filter_mode=${SHADOW_LAUNCH0_FILTER_MODE})"
bash "$PACK_DIR/scripts/22_launch0_new_mint_pool_census.sh" "$WINDOW_SECONDS"

LS13A_SUMMARY="$RUN_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_summary.json"
LS13A_ROWS="$RUN_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl"
LAUNCH0_SUMMARY="$RUN_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_summary.json"
LAUNCH0_ROWS="$RUN_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_rows.jsonl"
SUMMARY_JSON="$RUN_ROOT/launch0g_global_launch_birth_census_summary.json"
SUMMARY_TXT="$RUN_ROOT/summary.txt"
GAP_JSON="$RUN_ROOT/launch0i_launch_decode_gap_report_summary.json"
GAP_TXT="$RUN_ROOT/launch0i_decode_gap_summary.txt"

python3 "$PACK_DIR/scripts/launch_coverage_tools.py" census \
    --run-root "$RUN_ROOT" \
    --window-seconds "$WINDOW_SECONDS" \
    --chain-estimate-per-hour "$CHAIN_ESTIMATE_PER_HOUR" \
    --ls13a-summary "$LS13A_SUMMARY" \
    --ls13a-rows "$LS13A_ROWS" \
    --launch0-summary "$LAUNCH0_SUMMARY" \
    --launch0-rows "$LAUNCH0_ROWS" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

python3 "$PACK_DIR/scripts/launch_coverage_tools.py" gap \
    --run-root "$RUN_ROOT" \
    --census-summary "$SUMMARY_JSON" \
    --ls13a-summary "$LS13A_SUMMARY" \
    --launch0-summary "$LAUNCH0_SUMMARY" \
    --launch0-rows "$LAUNCH0_ROWS" \
    --summary-json "$GAP_JSON" \
    --summary-txt "$GAP_TXT"

info "Launch0G summary written to $SUMMARY_JSON"
info "Launch0I gap summary written to $GAP_JSON"

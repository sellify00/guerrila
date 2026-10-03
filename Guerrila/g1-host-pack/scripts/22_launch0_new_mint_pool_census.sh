#!/usr/bin/env bash
# 22_launch0_new_mint_pool_census.sh — launch-follow census over new mint/pool flow from preprocessed LaserStream

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0 — New Mint / Pool Census"

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

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH0_WINDOW_SECONDS:-300}}"
FILTER_MODE="${SHADOW_LAUNCH0_FILTER_MODE:-nofilter}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH0_STEP:-22_launch0_new_mint_pool_census}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch0_new_mint_pool_census_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch0_new_mint_pool_census_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PROCESSED_ROWS_PATH="${SHADOW_LAUNCH0_PROCESSED_ROWS_PATH:-}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

RUNLS13A_DIR="$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census"
RUNLS13A_ROWS="$RUNLS13A_DIR/runls13a_preprocessed_backrun_target_census_rows.jsonl"
RUNLS13ALT_ROWS="$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl"
RUNLS13B_ROWS="$RESULTS_ROOT/21_runls13b_preprocessed_backrun_selector/runls13b_preprocessed_backrun_selector_rows.jsonl"

export SHADOW_RESULTS_DIR="$RESULTS_ROOT"
export SHADOW_RUNLS13A_RESULT_DIR="$RUNLS13A_DIR"
export SHADOW_RUNLS13ALT_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLS13ALT_INPUT_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLS13B_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLS13B_INPUT_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLS13B_ALT_AUDIT_ROWS_PATH="$RUNLS13ALT_ROWS"
export SHADOW_RUNLS13A_FILTER_MODE="$FILTER_MODE"
bash "$PACK_DIR/scripts/21_runls13a_preprocessed_backrun_target_census.sh" "$WINDOW_SECONDS" || true
bash "$PACK_DIR/scripts/21_runls13alt_preprocessed_alt_resolution_audit.sh"
bash "$PACK_DIR/scripts/21_runls13b_preprocessed_backrun_selector.sh"

SELECTOR_ROWS_PATH="${SHADOW_LAUNCH0_SELECTOR_ROWS_PATH:-$RESULTS_ROOT/21_runls13b_preprocessed_backrun_selector/runls13b_preprocessed_backrun_selector_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LAUNCH0_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
[[ -f "$SELECTOR_ROWS_PATH" ]] || SELECTOR_ROWS_PATH="$(latest_rows_for_step runls13b_preprocessed_backrun_selector_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_rows_for_step runls13a_preprocessed_backrun_target_census_rows.jsonl)"

PY_ARGS=(
    "$PACK_DIR/scripts/launch_follow_lane.py" launch0
    --selector-rows "$SELECTOR_ROWS_PATH"
    --ls13a-rows "$LS13A_ROWS_PATH"
    --rows-out "$ROWS_OUT"
    --summary-json "$SUMMARY_JSON"
    --summary-txt "$SUMMARY_TXT"
    --window-seconds "$WINDOW_SECONDS"
    --filter-mode "$FILTER_MODE"
)
[[ -n "$PROCESSED_ROWS_PATH" ]] && PY_ARGS+=(--processed-rows "$PROCESSED_ROWS_PATH")

python3 "${PY_ARGS[@]}"

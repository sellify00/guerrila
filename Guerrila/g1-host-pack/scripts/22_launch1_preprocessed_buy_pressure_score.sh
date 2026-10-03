#!/usr/bin/env bash
# 22_launch1_preprocessed_buy_pressure_score.sh — score buy pressure on launch-follow census rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch1 — Preprocessed Buy Pressure Score"

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
STEP="${SHADOW_LAUNCH1_STEP:-22_launch1_preprocessed_buy_pressure_score}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch1_preprocessed_buy_pressure_score_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch1_preprocessed_buy_pressure_score_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
INPUT_ROWS_PATH="${SHADOW_LAUNCH1_INPUT_ROWS_PATH:-$RESULTS_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_rows.jsonl}"
MIN_BUY_COUNT_1S="${SHADOW_LAUNCH1_MIN_BUY_COUNT_1S:-1}"
MIN_BUY_COUNT_2S="${SHADOW_LAUNCH1_MIN_BUY_COUNT_2S:-2}"
MIN_BUY_COUNT_5S="${SHADOW_LAUNCH1_MIN_BUY_COUNT_5S:-2}"
MIN_UNIQUE_BUYERS_5S="${SHADOW_LAUNCH1_MIN_UNIQUE_BUYERS_5S:-2}"
MIN_FOLLOWTHROUGH_SCORE="${SHADOW_LAUNCH1_MIN_FOLLOWTHROUGH_SCORE:-0.5}"
MAX_AGE_MS="${SHADOW_LAUNCH1_MAX_AGE_MS:-30000}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$INPUT_ROWS_PATH" ]] || INPUT_ROWS_PATH="$(latest_rows_for_step launch0_new_mint_pool_census_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 "$PACK_DIR/scripts/launch_follow_lane.py" launch1 \
    --input-rows "$INPUT_ROWS_PATH" \
    --rows-out "$ROWS_OUT" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT" \
    --min-buy-count-1s "$MIN_BUY_COUNT_1S" \
    --min-buy-count-2s "$MIN_BUY_COUNT_2S" \
    --min-buy-count-5s "$MIN_BUY_COUNT_5S" \
    --min-unique-buyers-5s "$MIN_UNIQUE_BUYERS_5S" \
    --min-followthrough-score "$MIN_FOLLOWTHROUGH_SCORE" \
    --max-age-ms "$MAX_AGE_MS"

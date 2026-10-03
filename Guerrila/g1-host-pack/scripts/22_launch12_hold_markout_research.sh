#!/usr/bin/env bash
# 22_launch12_hold_markout_research.sh — Launch R2-HOLD long horizon markout study

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch12 — Hold Markout Research"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH12_STEP:-22_launch12_hold_markout_research}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_JSONL="${SHADOW_LAUNCH12_INPUT_JSONL:-${SHADOW_LAUNCH11_SNAPSHOTS_JSONL:-$RESULTS_ROOT/launch_position_snapshots.jsonl}}"
SUMMARY_JSON="$RESULT_DIR/launch12_hold_markout_research_summary.json"
ROWS_JSONL="$RESULT_DIR/launch12_hold_markout_research_rows.jsonl"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"

python3 "$PACK_DIR/scripts/launch_hold_markout_research.py" \
    --input-jsonl "$INPUT_JSONL" \
    --summary-json "$SUMMARY_JSON" \
    --rows-jsonl "$ROWS_JSONL" \
    | tee "$SUMMARY_TXT"

pass "Launch12 hold markout summary written to $SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 22_launch14_chaos_catastrophe_detector.sh — Launch R2-HOLD chaos / sell-quality detector

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch14 — Chaos Catastrophe Detector"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LAUNCH14_STEP:-22_launch14_chaos_catastrophe_detector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SNAPSHOTS_JSONL="${SHADOW_LAUNCH14_SNAPSHOTS_JSONL:-${SHADOW_LAUNCH11_SNAPSHOTS_JSONL:-$RESULTS_ROOT/launch_position_snapshots.jsonl}}"
SUMMARY_JSON="$RESULT_DIR/launch14_chaos_catastrophe_detector_summary.json"
ROWS_JSONL="$RESULT_DIR/launch14_chaos_catastrophe_detector_rows.jsonl"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"

python3 "$PACK_DIR/scripts/launch_chaos_detector.py" \
    --snapshots-jsonl "$SNAPSHOTS_JSONL" \
    --summary-json "$SUMMARY_JSON" \
    --rows-jsonl "$ROWS_JSONL" \
    | tee "$SUMMARY_TXT"

pass "Launch14 chaos detector summary written to $SUMMARY_TXT"
finish

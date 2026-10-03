#!/usr/bin/env bash
# 22_launch36_batch002_vs_batch007_outcome_diff.sh — compare positive Batch002 vs later weak/no-exit batch
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch36 — Batch002 vs Batch007 Outcome Diff"

assert_cmd_exists python3

BATCH002_ROOT="${1:-${SHADOW_LAUNCH36_BATCH002_ROOT:-/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260601T150013Z}}"
BATCH007_ROOT="${2:-${SHADOW_LAUNCH36_BATCH007_ROOT:-/home/ubuntu/Guerrila/g1-host-pack/results/run_launch10_hot_r2_press_window_1h_20260602T075036Z}}"
OUT_DIR="${SHADOW_LAUNCH36_OUT_DIR:-$BATCH007_ROOT/22_launch36_batch002_vs_batch007_outcome_diff}"

python3 "$PACK_DIR/scripts/launch_batch_outcome_diff.py" \
    --batch-a-root "$BATCH002_ROOT" \
    --batch-b-root "$BATCH007_ROOT" \
    --batch-a-label "batch002_positive_reference" \
    --batch-b-label "batch007_hold_v1_tail" \
    --out-dir "$OUT_DIR"

pass "Launch36 Batch002 vs Batch007 outcome diff complete"
finish

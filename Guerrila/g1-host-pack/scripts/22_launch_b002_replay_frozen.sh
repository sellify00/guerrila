#!/usr/bin/env bash
# 22_launch_b002_replay_frozen.sh — replay/assert the Batch002 frozen phenotype.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch B002 — Frozen Replay"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
B002_ROOT="${1:-${SHADOW_LAUNCH_B002_ROOT:-$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260601T150013Z}}"
B002_ROOT="$(printf '%s' "$B002_ROOT" | tr -d '\r')"
OUT_DIR="${SHADOW_LAUNCH_B002_REPLAY_OUT_DIR:-$RESULTS_ROOT/22_launch_b002_replay_frozen}"
OUT_DIR="$(printf '%s' "$OUT_DIR" | tr -d '\r')"

python3 "$PACK_DIR/scripts/launch_b002_frozen_tools.py" replay \
    --run-root "$B002_ROOT" \
    --out-dir "$OUT_DIR"

pass "B002 frozen replay passed: $OUT_DIR/summary.json"
finish

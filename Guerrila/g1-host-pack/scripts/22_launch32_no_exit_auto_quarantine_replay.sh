#!/usr/bin/env bash
# 22_launch32_no_exit_auto_quarantine_replay.sh — replay quote-zero held-position auto quarantine
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch32 — No-Exit Auto Quarantine Replay"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_LAUNCH32_OUT_DIR:-$PACK_DIR/results/run_launch32_no_exit_auto_quarantine_replay_$RUN_ID}"

python3 "$PACK_DIR/scripts/launch_no_exit_auto_quarantine_replay.py" \
    --executor-path "$PACK_DIR/scripts/launch_follow_live_execute.py" \
    --out-dir "$OUT_DIR"

pass "Launch32 replay written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 22_launch61b_pre_migration_threshold_replay.sh — no-wire live-detectable threshold replay
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch61B — Pre-Migration Threshold Replay"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH61B_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L61A_ROWS="$(latest_file_under_results '/22_launch61a_pre_migration_markout_input_builder/rows.jsonl')"
    [[ -f "$L61A_ROWS" ]] || fail "Launch61B Launch61A rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L61A_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch61B run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH61B_REFRESH_MARKOUT:-false}" == "true" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch61a_pre_migration_markout_input_builder.sh" "$RUN_ROOT"
fi

MARKOUT_ROWS="${SHADOW_LAUNCH61B_MARKOUT_ROWS:-$RUN_ROOT/22_launch61a_pre_migration_markout_input_builder/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH61B_OUT_DIR:-$RUN_ROOT/22_launch61b_pre_migration_threshold_replay}"

[[ -f "$MARKOUT_ROWS" ]] || fail "Launch61B markout rows missing: $MARKOUT_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_pre_migration_threshold_replay.py" \
    --markout-rows "$MARKOUT_ROWS" \
    --out-dir "$OUT_DIR" \
    --progress-thresholds "${SHADOW_LAUNCH61B_PROGRESS_THRESHOLDS:-90,95,98,99}" \
    --sol-thresholds "${SHADOW_LAUNCH61B_SOL_THRESHOLDS:-5,2,1,0.5}" \
    --no-migration-loss-lamports "${SHADOW_LAUNCH61B_NO_MIGRATION_LOSS_LAMPORTS:-0}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch61B no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch61B no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch61B threshold replay written to $OUT_DIR/summary.txt"
finish

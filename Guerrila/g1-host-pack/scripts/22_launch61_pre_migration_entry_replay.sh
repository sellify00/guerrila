#!/usr/bin/env bash
# 22_launch61_pre_migration_entry_replay.sh — no-wire pre-migration entry replay
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch61 — Pre-Migration Entry Replay"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH61_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch61 Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch61 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

COMPLETION_ROWS="${SHADOW_LAUNCH61_COMPLETION_ROWS:-$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl}"
MARKOUT_ROWS="${SHADOW_LAUNCH61_MARKOUT_ROWS:-}"
OUT_DIR="${SHADOW_LAUNCH61_OUT_DIR:-$RUN_ROOT/22_launch61_pre_migration_entry_replay}"

[[ -f "$COMPLETION_ROWS" ]] || fail "Launch61 completion rows missing: $COMPLETION_ROWS"
if [[ -n "$MARKOUT_ROWS" && ! -f "$MARKOUT_ROWS" ]]; then
    fail "Launch61 markout rows configured but missing: $MARKOUT_ROWS"
fi
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MARKOUT_ARG=()
if [[ -n "$MARKOUT_ROWS" ]]; then
    MARKOUT_ARG=(--markout-rows "$MARKOUT_ROWS")
fi

python3 "$PACK_DIR/scripts/launch_pre_migration_entry_replay.py" \
    --completion-rows "$COMPLETION_ROWS" \
    "${MARKOUT_ARG[@]}" \
    --out-dir "$OUT_DIR" \
    --thresholds "${SHADOW_LAUNCH61_THRESHOLDS:-90,95,98,99}" \
    --max-spend-lamports "${SHADOW_LAUNCH61_MAX_SPEND_LAMPORTS:-10000000}" \
    --fee-tip-slippage-lamports "${SHADOW_LAUNCH61_FEE_TIP_SLIPPAGE_LAMPORTS:-1500000}" \
    --rent-recovery-lamports "${SHADOW_LAUNCH61_RENT_RECOVERY_LAMPORTS:-2039280}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch61 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch61 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch61 pre-migration replay written to $OUT_DIR/summary.txt"
finish

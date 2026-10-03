#!/usr/bin/env bash
# 22_launch57a_b002_plus_active_curve_blocker_audit.sh — no-wire attribution audit for Launch57
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch57a — B002_PLUS Active-Curve Blocker Audit"

assert_cmd_exists python3

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
RUN_ROOT="${1:-${SHADOW_LAUNCH57A_RUN_ROOT:-}}"

latest_named_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_path_file() {
    local path_pattern="$1"
    find "$RESULTS_ROOT" -type f -path "$path_pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    U8_SUMMARY="$(latest_path_file '*/22_launch0u8_active_curve_fresh_state_filter/summary.json')"
    if [[ -n "$U8_SUMMARY" ]]; then
        RUN_ROOT="$(dirname "$(dirname "$U8_SUMMARY")")"
    else
        LAUNCH0D_SUMMARY="$(latest_named_file launch0d_realtime_birth_decoder_summary.json)"
        [[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch57a run root missing"
        RUN_ROOT="$(dirname "$LAUNCH0D_SUMMARY")"
    fi
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch57a run root missing: $RUN_ROOT"
[[ -f "$PACK_DIR/scripts/launch_b002_plus_active_curve_blocker_audit.py" ]] || fail "Launch57a Python audit missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH57A_REFRESH_GATE:-true}" != "false" ]]; then
    sudo -E bash "$SCRIPT_DIR/22_launch57_b002_plus_active_curve_no_wire.sh" "$RUN_ROOT"
fi

U8_ROWS="${SHADOW_LAUNCH57A_ACTIVE_ROWS:-$RUN_ROOT/22_launch0u8_active_curve_fresh_state_filter/rows.jsonl}"
GATE_ROWS="${SHADOW_LAUNCH57A_GATE_ROWS:-$RESULTS_ROOT/22_launch57_b002_plus_active_curve_no_wire/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH57A_OUT_DIR:-$RUN_ROOT/22_launch57a_b002_plus_active_curve_blocker_audit}"

[[ -f "$U8_ROWS" ]] || fail "Launch57a U8 rows missing: $U8_ROWS"
[[ -f "$GATE_ROWS" ]] || fail "Launch57a Launch57 evaluated rows missing: $GATE_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_b002_plus_active_curve_blocker_audit.py" \
    --run-root "$RUN_ROOT" \
    --active-curve-rows "$U8_ROWS" \
    --gate-rows "$GATE_ROWS" \
    --out-dir "$OUT_DIR"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch57a no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch57a no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch57a B002_PLUS active-curve blocker audit written to $OUT_DIR/summary.txt"
finish

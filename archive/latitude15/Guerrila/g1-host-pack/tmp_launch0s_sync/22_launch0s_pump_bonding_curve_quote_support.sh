#!/usr/bin/env bash
# 22_launch0s_pump_bonding_curve_quote_support.sh — no-wire Pump bonding-curve quote support gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0S — Pump Bonding-Curve Quote Support"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0S_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0D_SUMMARY="$(latest_file launch0d_realtime_birth_decoder_summary.json)"
    [[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch0D realtime launch-state summary missing"
    RUN_ROOT="$(dirname "$LAUNCH0D_SUMMARY")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0S run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

bash "$SCRIPT_DIR/22_launch0q_realtime_launch_quote_state_hydrator.sh" "$RUN_ROOT"
bash "$SCRIPT_DIR/22_launch24_entry_quality_gate.sh" "$RUN_ROOT"
bash "$SCRIPT_DIR/22_launch35_absorption_no_exit_fragility_gate.sh" "$RUN_ROOT"
bash "$SCRIPT_DIR/22_launch0r_route_support_and_warmup_reconcile.sh" "$RUN_ROOT"

QUOTE_STATE_ROWS="${SHADOW_LAUNCH0S_QUOTE_STATE_ROWS:-$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl}"
QUOTE_STATE_SUMMARY="${SHADOW_LAUNCH0S_QUOTE_STATE_SUMMARY:-$RUN_ROOT/launch0q_realtime_launch_quote_state_summary.json}"
LAUNCH0R_SUMMARY="${SHADOW_LAUNCH0S_LAUNCH0R_SUMMARY:-$RUN_ROOT/22_launch0r_route_support_and_warmup_reconcile/summary.json}"
OUT_DIR="${SHADOW_LAUNCH0S_OUT_DIR:-$RUN_ROOT/22_launch0s_pump_bonding_curve_quote_support}"

[[ -f "$QUOTE_STATE_ROWS" ]] || fail "Launch0S quote-state rows missing: $QUOTE_STATE_ROWS"
[[ -f "$QUOTE_STATE_SUMMARY" ]] || fail "Launch0S quote-state summary missing: $QUOTE_STATE_SUMMARY"
[[ -f "$LAUNCH0R_SUMMARY" ]] || fail "Launch0S Launch0R summary missing: $LAUNCH0R_SUMMARY"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_pump_bonding_curve_quote_support.py" \
    --run-root "$RUN_ROOT" \
    --quote-state-rows "$QUOTE_STATE_ROWS" \
    --quote-state-summary "$QUOTE_STATE_SUMMARY" \
    --launch0r-summary "$LAUNCH0R_SUMMARY" \
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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0S no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0S no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0S Pump bonding-curve quote support summary written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 22_launch54_birth_ramp_capture_sim.sh — simulate capture of Pump birth-ramp after real costs
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch54 — Birth-Ramp Capture Sim"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
OUT_DIR="${SHADOW_LAUNCH54_OUT_DIR:-$RESULTS_ROOT/22_launch54_birth_ramp_capture_sim}"
mkdir -p "$OUT_DIR"

INPUT_PATH="${1:-${SHADOW_LAUNCH54_INPUT_PATH:-}}"
if [[ -z "$INPUT_PATH" ]]; then
    if [[ -f "$PACK_DIR/../tmp/bonding_curve_3h_probe_result.json" ]]; then
        INPUT_PATH="$PACK_DIR/../tmp/bonding_curve_3h_probe_result.json"
    elif [[ -f "$PACK_DIR/results/launch_position_snapshots.jsonl" ]]; then
        INPUT_PATH="$PACK_DIR/results/launch_position_snapshots.jsonl"
    else
        INPUT_PATH="$(find "$RESULTS_ROOT" -type f \( -name '*markout*rows*.jsonl' -o -name '*position*snapshots*.jsonl' -o -name 'bonding_curve_3h_probe_result.json' \) -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
    fi
fi

[[ -f "$INPUT_PATH" ]] || fail "Launch54 input path missing: $INPUT_PATH"
[[ -f "$PACK_DIR/scripts/launch_birth_ramp_capture_sim.py" ]] || fail "Launch54 Python source missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MAX_SPEND_LAMPORTS="$(python3 - <<PY
print(int(round(float("${SHADOW_R2_MAX_SPEND_SOL:-0.01}") * 1_000_000_000)))
PY
)"

python3 "$PACK_DIR/scripts/launch_birth_ramp_capture_sim.py" \
    --input-path "$INPUT_PATH" \
    --summary-json "$OUT_DIR/summary.json" \
    --summary-txt "$OUT_DIR/summary.txt" \
    --rows-jsonl "$OUT_DIR/rows.jsonl" \
    --max-spend-lamports "${SHADOW_LAUNCH54_MAX_SPEND_LAMPORTS:-$MAX_SPEND_LAMPORTS}" \
    --true-cost-floor-lamports "${SHADOW_LAUNCH54_TRUE_COST_FLOOR_LAMPORTS:-1640000}" \
    --profit-buffer-bps "${SHADOW_LAUNCH54_PROFIT_BUFFER_BPS:-500}" \
    ${SHADOW_LAUNCH54_TREAT_COMPLETION_AS_NO_EXIT:+--treat-completion-as-no-exit}

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch54 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch54 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch54 birth-ramp capture sim written to $OUT_DIR/summary.txt"
finish

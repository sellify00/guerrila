#!/usr/bin/env bash
# 22_launch72_b002_size_capacity_ladder.sh — no-wire Batch002 size-capacity replay ladder
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch72 — B002 Size Capacity Ladder"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "/etc/shadow-engine/env.g1.local.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
B002_ROOT="${1:-${SHADOW_LAUNCH_B002_ROOT:-$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260601T150013Z}}"
OUT_DIR="${SHADOW_LAUNCH72_OUT_DIR:-$RESULTS_ROOT/22_launch72_b002_size_capacity_ladder}"

[[ -d "$B002_ROOT" ]] || fail "Launch72 B002 root missing: $B002_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_b002_size_capacity_ladder.py" \
    --run-root "$B002_ROOT" \
    --out-dir "$OUT_DIR" \
    --sizes-sol "${SHADOW_LAUNCH72_SIZES_SOL:-0.01,0.025,0.05,0.1,0.25,0.5,1.0}" \
    --max-loss-sol "${SHADOW_LAUNCH72_MAX_LOSS_SOL:-${SHADOW_R2_MAX_LOSS_SOL:-0.015}}" \
    --max-entry-price-impact-bps "${SHADOW_LAUNCH72_MAX_ENTRY_PRICE_IMPACT_BPS:-1500}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH72_MAX_EXIT_PRICE_IMPACT_BPS:-1500}" \
    --min-base-reserve-to-position-ratio "${SHADOW_LAUNCH72_MIN_BASE_RESERVE_TO_POSITION_RATIO:-50}" \
    --min-reserve-after-exit-to-position-ratio "${SHADOW_LAUNCH72_MIN_RESERVE_AFTER_EXIT_TO_POSITION_RATIO:-25}" \
    --cost-protector-arm-bps "${SHADOW_LAUNCH72_COST_PROTECTOR_ARM_BPS:-2000}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch72 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch72 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch72 B002 size capacity written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 22_launch0v2_migrated_pumpswap_momentum_recheck.sh — no-wire migrated PumpSwap momentum / absorption recheck
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0V2 — Migrated PumpSwap Momentum Recheck"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0V2_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0V_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0v_migrated_pumpswap_r3_candidate_gate' || true)"
    [[ -f "$LAUNCH0V_SUMMARY" ]] || fail "Launch0V2 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0V_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0V2 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0V2_REFRESH_V_GATE:-true}" != "false" ]]; then
    SHADOW_LAUNCH0V_REFRESH_GATES=false bash "$SCRIPT_DIR/22_launch0v_migrated_pumpswap_r3_candidate_gate.sh" "$RUN_ROOT"
fi

ROWS="${SHADOW_LAUNCH0V2_ROWS:-$RUN_ROOT/22_launch0v_migrated_pumpswap_r3_candidate_gate/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH0V2_OUT_DIR:-$RUN_ROOT/22_launch0v2_migrated_pumpswap_momentum_recheck}"

[[ -f "$ROWS" ]] || fail "Launch0V2 rows missing: $ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_migrated_pumpswap_momentum_recheck.py" \
    --run-root "$RUN_ROOT" \
    --rows "$ROWS" \
    --out-dir "$OUT_DIR" \
    --min-net-buy-pressure-5s "${SHADOW_LAUNCH0V2_MIN_NET_BUY_PRESSURE_5S:-1}" \
    --max-sell-count-2s "${SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S:-0}" \
    --min-absorbed-sell-age-ms "${SHADOW_LAUNCH0V2_MIN_ABSORBED_SELL_AGE_MS:-3000}" \
    --min-absorption-score "${SHADOW_LAUNCH0V2_MIN_ABSORPTION_SCORE:-60}" \
    --min-price-change-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS:-100}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0V2 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0V2 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0V2 migrated PumpSwap momentum recheck written to $OUT_DIR/summary.txt"
finish

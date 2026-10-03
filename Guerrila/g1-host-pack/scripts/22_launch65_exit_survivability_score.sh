#!/usr/bin/env bash
# 22_launch65_exit_survivability_score.sh — R5F no-wire exit survivability stress score
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch65 — R5 Exit Survivability Score"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH65_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch65 Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch65 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

OUT_DIR="${SHADOW_LAUNCH65_OUT_DIR:-$RUN_ROOT/22_launch65_exit_survivability_score}"
INPUTS=()
if [[ -n "${SHADOW_LAUNCH65_INPUT_ROWS:-}" ]]; then
    IFS=':' read -r -a INPUTS <<<"$SHADOW_LAUNCH65_INPUT_ROWS"
else
    for candidate in \
        "$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl" \
        "$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl" \
        "$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl"
    do
        [[ -f "$candidate" ]] && INPUTS+=("$candidate")
    done
fi

[[ ${#INPUTS[@]} -gt 0 ]] || fail "Launch65 no input rows found"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=()
for path in "${INPUTS[@]}"; do
    [[ -f "$path" ]] || fail "Launch65 input rows missing: $path"
    ARGS+=(--rows "$path")
done
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_exit_survivability_score.py" \
    "${ARGS[@]}" \
    --out-dir "$OUT_DIR" \
    --stress-reserve-fraction "${SHADOW_LAUNCH65_STRESS_RESERVE_FRACTION:-0.50}" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH65_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH65_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --stress-reserve-after-floor-lamports "${SHADOW_LAUNCH65_STRESS_RESERVE_AFTER_FLOOR_LAMPORTS:-10000000}" \
    --max-price-impact-bps "${SHADOW_LAUNCH65_MAX_PRICE_IMPACT_BPS:-1500}" \
    --stress-max-price-impact-bps "${SHADOW_LAUNCH65_STRESS_MAX_PRICE_IMPACT_BPS:-3000}" \
    --max-decline-velocity-bps "${SHADOW_LAUNCH65_MAX_DECLINE_VELOCITY_BPS:-1}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch65 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch65 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch65 exit survivability written to $OUT_DIR/summary.txt"
finish

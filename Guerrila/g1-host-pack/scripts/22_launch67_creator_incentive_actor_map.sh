#!/usr/bin/env bash
# 22_launch67_creator_incentive_actor_map.sh — R5H no-wire creator/actor quality map
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch67 — R5 Creator Incentive Actor Map"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH67_RUN_ROOT:-}}"

latest_file_under_results() {
    local suffix="$1"
    find "$RESULTS_ROOT" -type f -path "*$suffix" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    L60_ROWS="$(latest_file_under_results '/22_launch60_curve_completion_watch/rows.jsonl')"
    [[ -f "$L60_ROWS" ]] || fail "Launch67 Launch60 rows missing"
    RUN_ROOT="$(dirname "$(dirname "$L60_ROWS")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch67 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

OUT_DIR="${SHADOW_LAUNCH67_OUT_DIR:-$RUN_ROOT/22_launch67_creator_incentive_actor_map}"
INPUTS=()
if [[ -n "${SHADOW_LAUNCH67_INPUT_ROWS:-}" ]]; then
    IFS=':' read -r -a INPUTS <<<"$SHADOW_LAUNCH67_INPUT_ROWS"
else
    for candidate in \
        "$RUN_ROOT/22_launch60_curve_completion_watch/rows.jsonl" \
        "$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl" \
        "$RUN_ROOT/22_launch62d_pre_migration_persistent_watchlist/rows.jsonl"
    do
        [[ -f "$candidate" ]] && INPUTS+=("$candidate")
    done
fi

[[ ${#INPUTS[@]} -gt 0 ]] || fail "Launch67 no input rows found"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=()
for path in "${INPUTS[@]}"; do
    [[ -f "$path" ]] || fail "Launch67 input rows missing: $path"
    ARGS+=(--rows "$path")
done
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_creator_incentive_actor_map.py" \
    "${ARGS[@]}" \
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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch67 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch67 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch67 creator/actor map written to $OUT_DIR/summary.txt"
finish

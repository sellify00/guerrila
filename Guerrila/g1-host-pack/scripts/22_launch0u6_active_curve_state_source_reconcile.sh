#!/usr/bin/env bash
# 22_launch0u6_active_curve_state_source_reconcile.sh — no-wire active curve state/source reconcile
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U6 — Active Curve State Source Reconcile"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U6_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    U5_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0u5_active_curve_account_identity_autopsy' || true)"
    [[ -f "$U5_SUMMARY" ]] || fail "Launch0U6 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$U5_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U6 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U6_REFRESH_U5:-true}" != "false" ]]; then
    bash "$SCRIPT_DIR/22_launch0u5_active_curve_account_identity_autopsy.sh" "$RUN_ROOT"
fi

U5_ROWS="${SHADOW_LAUNCH0U6_U5_ROWS:-$RUN_ROOT/22_launch0u5_active_curve_account_identity_autopsy/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH0U6_OUT_DIR:-$RUN_ROOT/22_launch0u6_active_curve_state_source_reconcile}"

[[ -f "$U5_ROWS" ]] || fail "Launch0U6 U5 rows missing: $U5_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_active_curve_state_source_reconcile.py" \
    --run-root "$RUN_ROOT" \
    --launch0u5-rows "$U5_ROWS" \
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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U6 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U6 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U6 active curve state/source reconcile written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 22_launch28_late_follow_slippage_ladder.sh — summarize late-follow entry slippage ladder diagnostics.

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch28 — Late-Follow Slippage Ladder"

RUN_ROOT="${1:-${SHADOW_LAUNCH28_RUN_ROOT:-}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RESULTS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/results"
PY="$SCRIPT_DIR/launch_late_follow_slippage_ladder.py"

if [[ ! -f "$PY" ]]; then
    fail "Launch28 Python helper missing: $PY"
    finish
    exit 1
fi

ARGS=(python3 "$PY" --results-dir "$RESULTS_DIR")
if [[ -n "$RUN_ROOT" ]]; then
    ARGS+=(--run-root "$RUN_ROOT")
fi

"${ARGS[@]}"
status=$?
if [[ $status -eq 0 ]]; then
    pass "Launch28 slippage ladder summary complete"
else
    fail "Launch28 slippage ladder summary failed: $status"
fi
finish
exit "$status"

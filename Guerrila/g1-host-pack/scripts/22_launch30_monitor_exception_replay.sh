#!/usr/bin/env bash
# 22_launch30_monitor_exception_replay.sh — inject a monitor exception and prove automatic rescue guard behavior.

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch30 — Monitor Exception Replay"

RUN_ROOT="${1:-${SHADOW_LAUNCH30_RUN_ROOT:-}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RESULTS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/results"
PY="$SCRIPT_DIR/launch_monitor_exception_replay.py"

if [[ ! -f "$PY" ]]; then
    fail "Launch30 Python helper missing: $PY"
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
    pass "Launch30 monitor exception replay passed"
else
    fail "Launch30 monitor exception replay failed: $status"
fi
finish
exit "$status"

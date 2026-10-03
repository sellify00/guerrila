#!/usr/bin/env bash
# 22_launch31_monitor_metrics_repair_validation.sh — no-wire replay for repaired Launch monitor metrics path.

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch31 — Monitor Metrics Repair Validation"

RUN_ROOT="${1:-${SHADOW_LAUNCH31_RUN_ROOT:-}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RESULTS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/results"
PY="$SCRIPT_DIR/launch_monitor_metrics_repair_validation.py"

if [[ ! -f "$PY" ]]; then
    fail "Launch31 Python helper missing: $PY"
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
    pass "Launch31 monitor metrics repair validation passed"
else
    fail "Launch31 monitor metrics repair validation failed: $status"
fi
finish
exit "$status"

#!/usr/bin/env bash
# 22_launch49_no_exit_tail_filter_path_autopsy.sh — verify no-exit filter path wiring for landed tails
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch49 — No-Exit Tail Filter-Path Autopsy"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH49_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch49 run root missing"

OUT_DIR="${SHADOW_LAUNCH49_OUT_DIR:-$RUN_ROOT/22_launch49_no_exit_tail_filter_path_autopsy}"
ARGS=(
    "$PACK_DIR/scripts/launch_no_exit_tail_filter_path_autopsy.py"
    --run-root "$RUN_ROOT"
    --out-dir "$OUT_DIR"
)

if [[ -n "${SHADOW_LAUNCH49_ATTEMPT:-}" ]]; then
    ARGS+=(--attempt "$SHADOW_LAUNCH49_ATTEMPT")
elif [[ $# -gt 1 ]]; then
    ARGS+=(--attempt "$2")
fi

python3 "${ARGS[@]}"

pass "Launch49 no-exit tail filter-path autopsy complete"
finish

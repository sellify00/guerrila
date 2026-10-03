#!/usr/bin/env bash
# 22_launch47_b002_clone_exit_policy_autopsy.sh — inspect B002 clone quote-weakening exits
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch47 — B002 Clone Exit Policy Autopsy"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH47_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
RUN_ROOT="$(printf '%s' "$RUN_ROOT" | tr -d '\r')"
[[ -n "$RUN_ROOT" ]] || fail "Launch47 run root missing"
OUT_DIR="${SHADOW_LAUNCH47_OUT_DIR:-$RUN_ROOT/22_launch47_b002_clone_exit_policy_autopsy}"
OUT_DIR="$(printf '%s' "$OUT_DIR" | tr -d '\r')"

python3 "$PACK_DIR/scripts/launch_b002_clone_exit_policy_autopsy.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR"

pass "Launch47 B002 clone exit-policy autopsy complete"
finish

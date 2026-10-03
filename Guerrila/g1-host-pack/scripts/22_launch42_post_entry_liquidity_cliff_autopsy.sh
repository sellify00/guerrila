#!/usr/bin/env bash
# 22_launch42_post_entry_liquidity_cliff_autopsy.sh — inspect no-exit tails for post-entry reserve cliffs
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch42 — Post-Entry Liquidity Cliff Autopsy"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH42_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch42 run root missing"

OUT_DIR="${SHADOW_LAUNCH42_OUT_DIR:-$RUN_ROOT/22_launch42_post_entry_liquidity_cliff_autopsy}"
python3 "$PACK_DIR/scripts/launch_post_entry_liquidity_cliff_autopsy.py" \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR"

pass "Launch42 post-entry liquidity cliff autopsy complete"
finish

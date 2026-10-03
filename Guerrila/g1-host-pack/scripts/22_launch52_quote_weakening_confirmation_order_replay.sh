#!/usr/bin/env bash
# 22_launch52_quote_weakening_confirmation_order_replay.sh — verify quote weakening confirmation wins before quote zero
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch52 — Quote Weakening Confirmation Order Replay"

assert_cmd_exists python3

RUN_ROOT="${1:-${SHADOW_LAUNCH52_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$PACK_DIR"/results/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
RUN_ROOT="$(printf '%s' "$RUN_ROOT" | tr -d '\r')"
[[ -n "$RUN_ROOT" ]] || fail "Launch52 run root missing"

OUT_DIR="${SHADOW_LAUNCH52_OUT_DIR:-$RUN_ROOT/22_launch52_quote_weakening_confirmation_order_replay}"
OUT_DIR="$(printf '%s' "$OUT_DIR" | tr -d '\r')"

ARGS=(
    "$PACK_DIR/scripts/launch_quote_weakening_confirmation_order_replay.py"
    --run-root "$RUN_ROOT"
    --out-dir "$OUT_DIR"
    --confirm-exit-count "${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT:-3}"
)

if [[ -n "${SHADOW_LAUNCH52_ATTEMPT:-}" ]]; then
    ARGS+=(--attempt "$SHADOW_LAUNCH52_ATTEMPT")
elif [[ $# -gt 1 ]]; then
    ARGS+=(--attempt "$2")
fi

python3 "${ARGS[@]}"

pass "Launch52 quote weakening confirmation order replay complete"
finish

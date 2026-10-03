#!/usr/bin/env bash
# 22_launch23_cost_aware_profit_protector.sh — inspect and tune Launch cost-aware trailing protection
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch23 — Cost-Aware Profit Protector"

RUN_ROOT="${1:-${SHADOW_LAUNCH23_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH23_OUT_DIR:-$RUN_ROOT/22_launch23_cost_aware_profit_protector}"
FLOOR_SOURCE="${SHADOW_LAUNCH_COST_PROFIT_FLOOR_SOURCE:-true_loss_floor}"
PROFIT_BUFFER_BPS="${SHADOW_LAUNCH_COST_PROFIT_BUFFER_BPS:-500}"
TIGHT_TRAILING_BPS="${SHADOW_LAUNCH_COST_PROFIT_TIGHT_TRAILING_BPS:-500}"

sudo -E python3 "$PACK_DIR/scripts/launch_cost_aware_profit_protector.py" \
    "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --floor-source "$FLOOR_SOURCE" \
    --profit-buffer-bps "$PROFIT_BUFFER_BPS" \
    --tight-trailing-stop-bps "$TIGHT_TRAILING_BPS"

pass "Launch23 cost-aware profit protector written to $OUT_DIR/summary.txt"
finish

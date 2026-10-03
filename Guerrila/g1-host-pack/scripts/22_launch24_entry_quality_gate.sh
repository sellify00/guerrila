#!/usr/bin/env bash
# 22_launch24_entry_quality_gate.sh — inspect Launch R3-HOLD entry-quality gating
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch24 — Entry Quality Gate"

RUN_ROOT="${1:-${SHADOW_LAUNCH24_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH24_OUT_DIR:-$RUN_ROOT/22_launch24_entry_quality_gate}"
MIN_PRICE_CHANGE_BPS="${SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS:-100}"
MAX_SELL_COUNT_2S="${SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S:-0}"
PANIC_DROP_2S_BPS="${SHADOW_LAUNCH_EXIT_DROP_2S_BPS:-700}"
EDGE_ENABLED="${SHADOW_LAUNCH_ENTRY_EDGE_ENABLED:-true}"
EDGE_MIN_PRICE_CHANGE_BPS="${SHADOW_LAUNCH_ENTRY_EDGE_MIN_PRICE_CHANGE_BPS:-300}"
EDGE_MAX_PRICE_CHANGE_BPS="${SHADOW_LAUNCH_ENTRY_EDGE_MAX_PRICE_CHANGE_BPS:-1500}"
EDGE_MAX_SIGNAL_AGE_MS="${SHADOW_LAUNCH_ENTRY_EDGE_MAX_SIGNAL_AGE_MS:-1000}"
EDGE_MIN_NET_BUY_PRESSURE_5S="${SHADOW_LAUNCH_ENTRY_EDGE_MIN_NET_BUY_PRESSURE_5S:-1}"
SELF_TEST="${SHADOW_LAUNCH24_SELF_TEST:-false}"

ARGS=(
    "$PACK_DIR/scripts/launch_entry_quality_gate.py"
    "$RUN_ROOT"
    --out-dir "$OUT_DIR"
    --min-price-change-bps "$MIN_PRICE_CHANGE_BPS"
    --max-sell-count-2s "$MAX_SELL_COUNT_2S"
    --panic-drop-2s-bps "$PANIC_DROP_2S_BPS"
    --edge-enabled "$EDGE_ENABLED"
    --edge-min-price-change-bps "$EDGE_MIN_PRICE_CHANGE_BPS"
    --edge-max-price-change-bps "$EDGE_MAX_PRICE_CHANGE_BPS"
    --edge-max-signal-age-ms "$EDGE_MAX_SIGNAL_AGE_MS"
    --edge-min-net-buy-pressure-5s "$EDGE_MIN_NET_BUY_PRESSURE_5S"
)

if [[ "${SELF_TEST,,}" == "true" || "${SELF_TEST,,}" == "1" || "${SELF_TEST,,}" == "yes" ]]; then
    ARGS+=(--self-test)
fi

sudo -E python3 "${ARGS[@]}"

pass "Launch24 entry-quality gate written to $OUT_DIR/summary.txt"
finish

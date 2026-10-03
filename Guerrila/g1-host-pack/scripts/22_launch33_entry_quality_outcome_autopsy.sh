#!/usr/bin/env bash
# 22_launch33_entry_quality_outcome_autopsy.sh — compare Launch entry-quality features against realized outcomes
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch33 — Entry Quality Outcome Autopsy"

POSITIVE_ROOT="${1:-${SHADOW_LAUNCH33_POSITIVE_ROOT:-}}"
NEGATIVE_ROOT="${2:-${SHADOW_LAUNCH33_NEGATIVE_ROOT:-}}"
OUT_DIR="${SHADOW_LAUNCH33_OUT_DIR:-}"
EDGE_MIN_PRICE_CHANGE_BPS="${SHADOW_LAUNCH_ENTRY_EDGE_MIN_PRICE_CHANGE_BPS:-300}"
EDGE_MAX_PRICE_CHANGE_BPS="${SHADOW_LAUNCH_ENTRY_EDGE_MAX_PRICE_CHANGE_BPS:-1500}"
EDGE_MAX_SIGNAL_AGE_MS="${SHADOW_LAUNCH_ENTRY_EDGE_MAX_SIGNAL_AGE_MS:-1000}"
EDGE_MIN_NET_BUY_PRESSURE_5S="${SHADOW_LAUNCH_ENTRY_EDGE_MIN_NET_BUY_PRESSURE_5S:-1}"

ARGS=(
    "$PACK_DIR/scripts/launch_entry_quality_outcome_autopsy.py"
    --edge-min-price-change-bps "$EDGE_MIN_PRICE_CHANGE_BPS"
    --edge-max-price-change-bps "$EDGE_MAX_PRICE_CHANGE_BPS"
    --edge-max-signal-age-ms "$EDGE_MAX_SIGNAL_AGE_MS"
    --edge-min-net-buy-pressure-5s "$EDGE_MIN_NET_BUY_PRESSURE_5S"
)

if [[ -n "$POSITIVE_ROOT" ]]; then
    ARGS+=(--positive-root "$POSITIVE_ROOT")
fi
if [[ -n "$NEGATIVE_ROOT" ]]; then
    ARGS+=(--negative-root "$NEGATIVE_ROOT")
fi
if [[ -n "$OUT_DIR" ]]; then
    ARGS+=(--out-dir "$OUT_DIR")
fi
if [[ -n "${SHADOW_LAUNCH33_EXTRA_RUN_ROOTS:-}" ]]; then
    IFS=',' read -r -a EXTRA_ROOTS <<<"$SHADOW_LAUNCH33_EXTRA_RUN_ROOTS"
    for item in "${EXTRA_ROOTS[@]}"; do
        [[ -n "$item" ]] && ARGS+=(--run-root "$item")
    done
fi

sudo -E python3 "${ARGS[@]}"

pass "Launch33 entry-quality outcome autopsy complete"
finish

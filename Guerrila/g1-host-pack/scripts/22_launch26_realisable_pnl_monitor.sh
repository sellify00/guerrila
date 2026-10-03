#!/usr/bin/env bash
# 22_launch26_realisable_pnl_monitor.sh - Launch R3-HOLD realisable exit PnL monitor

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch26 - Realisable PnL Monitor"

RUN_ROOT="${1:-${SHADOW_LAUNCH26_RUN_ROOT:-${SHADOW_RESULTS_DIR:-g1-host-pack/results}}}"
OUT_DIR="${SHADOW_LAUNCH26_OUT_DIR:-}"

ARGS=(--run-root "$RUN_ROOT")
if [[ -n "$OUT_DIR" ]]; then
    ARGS+=(--out-dir "$OUT_DIR")
fi

python3 "$SCRIPT_DIR/launch_realisable_pnl_monitor.py" "${ARGS[@]}"

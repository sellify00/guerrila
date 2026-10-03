#!/usr/bin/env bash
# 22_launch22_post_flatten_account_close_recovery.sh — recover closeable signer-owned post-flatten token rent
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch22 — Post-Flatten Account Close Recovery"

RUN_ROOT="${1:-${SHADOW_LAUNCH22_RUN_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}}"
OUT_DIR="${SHADOW_LAUNCH22_OUT_DIR:-$RUN_ROOT/22_launch22_post_flatten_account_close_recovery}"
DISABLE_CLOSE="${SHADOW_LAUNCH22_DISABLE_CLOSE:-false}"

ARGS=(
    "$PACK_DIR/scripts/launch_post_flatten_account_close_recovery.py"
    "$RUN_ROOT"
    --out-dir "$OUT_DIR"
)

if [[ "${DISABLE_CLOSE,,}" == "true" || "${DISABLE_CLOSE,,}" == "1" || "${DISABLE_CLOSE,,}" == "yes" ]]; then
    ARGS+=(--disable-close)
fi

sudo -E python3 "${ARGS[@]}"

pass "Launch22 post-flatten close recovery written to $OUT_DIR/summary.txt"
finish

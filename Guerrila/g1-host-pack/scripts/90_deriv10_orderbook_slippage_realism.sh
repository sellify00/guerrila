#!/usr/bin/env bash
# 90_deriv10_orderbook_slippage_realism.sh — no-wire orderbook/slippage realism gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv10 — Orderbook Slippage Realism"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_DERIV10_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    ROOT_PTR="$RESULTS_ROOT/90_deriv8_rolling_impulse_paper_watch_latest_root.txt"
    [[ -f "$ROOT_PTR" ]] || fail "Deriv10 run root missing and latest root pointer missing: $ROOT_PTR"
    RUN_ROOT="$(tr -d '\r' < "$ROOT_PTR")"
fi
if [[ "$RUN_ROOT" == [A-Za-z]:\\* ]]; then
    if command -v wslpath >/dev/null 2>&1; then
        RUN_ROOT="$(wslpath -u "$RUN_ROOT")"
    else
        DRIVE="$(printf '%s' "${RUN_ROOT:0:1}" | tr '[:upper:]' '[:lower:]')"
        REST="${RUN_ROOT:2}"
        REST="${REST//\\//}"
        RUN_ROOT="/mnt/$DRIVE$REST"
    fi
fi

[[ -d "$RUN_ROOT" ]] || fail "Deriv10 run root missing: $RUN_ROOT"
READY_ROWS="${SHADOW_DERIV10_READY_ROWS:-$RUN_ROOT/90_deriv9_rolling_impulse_edge_validation/ready_rows.jsonl}"
if [[ ! -f "$READY_ROWS" ]]; then
    warn "Deriv10 Deriv9 ready rows missing; running Deriv9 first"
    bash "$SCRIPT_DIR/90_deriv9_rolling_impulse_edge_validation.sh" "$RUN_ROOT"
fi
[[ -f "$READY_ROWS" ]] || fail "Deriv10 ready rows missing: $READY_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

OUT_DIR="${SHADOW_DERIV10_OUT_DIR:-$RUN_ROOT/90_deriv10_orderbook_slippage_realism}"

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" orderbook-slippage-realism \
    --ready-rows "$READY_ROWS" \
    --out-dir "$OUT_DIR" \
    --notional-usd "${SHADOW_DERIV10_NOTIONAL_USD:-25}" \
    --already-modeled-spread-bps "${SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS:-2}" \
    --min-net-after-orderbook-bps "${SHADOW_DERIV10_MIN_NET_AFTER_ORDERBOOK_BPS:-5}" \
    --max-spread-bps "${SHADOW_DERIV10_MAX_SPREAD_BPS:-20}" \
    --max-roundtrip-book-drag-bps "${SHADOW_DERIV10_MAX_ROUNDTRIP_BOOK_DRAG_BPS:-25}" \
    --min-depth-multiple "${SHADOW_DERIV10_MIN_DEPTH_MULTIPLE:-3}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
print(int((json.loads(Path("$OUT_DIR/summary.json").read_text())).get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
print(int((json.loads(Path("$OUT_DIR/summary.json").read_text())).get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Deriv10 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Deriv10 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Deriv10 orderbook realism written to $OUT_DIR/summary.txt"
finish

#!/usr/bin/env bash
# 90_deriv9_rolling_impulse_edge_validation.sh — no-wire repeatability/edge validation
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv9 — Rolling Impulse Edge Validation"

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
RUN_ROOT="${1:-${SHADOW_DERIV9_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    ROOT_PTR="$RESULTS_ROOT/90_deriv8_rolling_impulse_paper_watch_latest_root.txt"
    [[ -f "$ROOT_PTR" ]] || fail "Deriv9 run root missing and latest root pointer missing: $ROOT_PTR"
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

[[ -d "$RUN_ROOT" ]] || fail "Deriv9 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

OUT_DIR="${SHADOW_DERIV9_OUT_DIR:-$RUN_ROOT/90_deriv9_rolling_impulse_edge_validation}"

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" rolling-edge-validation \
    --run-root "$RUN_ROOT" \
    --out-dir "$OUT_DIR" \
    --thresholds "${SHADOW_DERIV9_THRESHOLDS:-0,10,20,30,50,75,100}" \
    --min-completed-windows "${SHADOW_DERIV9_MIN_COMPLETED_WINDOWS:-8}" \
    --min-ready-windows "${SHADOW_DERIV9_MIN_READY_WINDOWS:-4}" \
    --min-ready-rows "${SHADOW_DERIV9_MIN_READY_ROWS:-6}" \
    --min-avg-net-bps "${SHADOW_DERIV9_MIN_AVG_NET_BPS:-20}" \
    --min-p25-net-bps "${SHADOW_DERIV9_MIN_P25_NET_BPS:-5}" \
    --max-top-symbol-share "${SHADOW_DERIV9_MAX_TOP_SYMBOL_SHARE:-0.50}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Deriv9 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Deriv9 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Deriv9 edge validation written to $OUT_DIR/summary.txt"
finish

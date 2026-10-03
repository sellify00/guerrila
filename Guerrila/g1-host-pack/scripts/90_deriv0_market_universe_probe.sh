#!/usr/bin/env bash
# 90_deriv0_market_universe_probe.sh — no-wire derivative market universe probe
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv0 — Market Universe Probe"

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
OUT_DIR="${SHADOW_DERIV0_OUT_DIR:-$RESULTS_ROOT/90_deriv0_market_universe_probe}"

VENUES_RAW="${SHADOW_DERIV_VENUES:-hyperliquid,backpack,jupiter_static}"
IFS=',' read -r -a VENUES <<<"$VENUES_RAW"

python3 "$PACK_DIR/scripts/derivatives_research_tools.py" market-universe \
    --out-dir "$OUT_DIR" \
    --venues "${VENUES[@]}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Deriv0 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Deriv0 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Deriv0 market universe written to $OUT_DIR/summary.txt"
finish

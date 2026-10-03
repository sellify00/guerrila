#!/usr/bin/env bash
# 22_launch45_batch002_similarity_gate.sh — no-wire Batch002 similarity gate over Launch10 hot rows
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch45 — Batch002 Similarity Gate"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-${SHADOW_LAUNCH45_RUN_ROOT:-}}"
if [[ -z "$RUN_ROOT" ]]; then
    RUN_ROOT="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_* 2>/dev/null | head -1 || true)"
fi
[[ -n "$RUN_ROOT" ]] || fail "Launch45 run root missing"

FINGERPRINT_PATH="${SHADOW_LAUNCH_BATCH002_FINGERPRINT_PATH:-${SHADOW_LAUNCH45_FINGERPRINT_PATH:-$RESULTS_ROOT/22_launch44_batch002_fingerprint/fingerprint.json}}"
[[ -f "$FINGERPRINT_PATH" ]] || fail "Batch002 fingerprint missing: $FINGERPRINT_PATH"

OUT_DIR="${SHADOW_LAUNCH45_OUT_DIR:-$RUN_ROOT/22_launch45_batch002_similarity_gate}"

python3 "$PACK_DIR/scripts/launch_batch002_similarity_gate.py" \
    --run-root "$RUN_ROOT" \
    --fingerprint-path "$FINGERPRINT_PATH" \
    --out-dir "$OUT_DIR" \
    --require-no-wire "${SHADOW_LAUNCH45_REQUIRE_NO_WIRE:-false}"

pass "Launch45 Batch002 similarity gate complete: $OUT_DIR/summary.json"
finish

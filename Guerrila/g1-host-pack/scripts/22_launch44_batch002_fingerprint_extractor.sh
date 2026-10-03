#!/usr/bin/env bash
# 22_launch44_batch002_fingerprint_extractor.sh — extract the pre-entry Batch002 winning fingerprint
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch44 — Batch002 Fingerprint Extractor"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
OUT_DIR="${SHADOW_LAUNCH44_OUT_DIR:-$RESULTS_ROOT/22_launch44_batch002_fingerprint}"

POSITIVE_ROOTS="${SHADOW_LAUNCH44_POSITIVE_ROOTS:-}"
BAD_ROOTS="${SHADOW_LAUNCH44_BAD_ROOTS:-}"

if [[ -z "$POSITIVE_ROOTS" ]]; then
    POSITIVE_ROOTS="$(ls -dt "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_1h_20260601T150013Z 2>/dev/null | paste -sd, - || true)"
fi
if [[ -z "$BAD_ROOTS" ]]; then
    BAD_ROOTS="$(ls -dt \
        "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_1h_20260602T173557Z \
        "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_1h_20260603T012032Z \
        "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_1h_20260602T084031Z \
        "$RESULTS_ROOT"/run_launch10_hot_r2_press_window_1h_20260602T075036Z \
        2>/dev/null | paste -sd, - || true)"
fi

[[ -n "$POSITIVE_ROOTS" ]] || fail "Launch44 positive roots missing; set SHADOW_LAUNCH44_POSITIVE_ROOTS"

python3 "$PACK_DIR/scripts/launch_batch002_fingerprint_extractor.py" \
    --positive-roots "$POSITIVE_ROOTS" \
    --bad-roots "$BAD_ROOTS" \
    --out-dir "$OUT_DIR"

pass "Launch44 Batch002 fingerprint extracted: $OUT_DIR/fingerprint.json"
finish

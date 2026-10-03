#!/usr/bin/env bash
# 22_launch57c_b002_plus_active_curve_outcome_autopsy.sh — compare B002-like vs weak-extension active-curve outcomes.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch57c — B002_PLUS Active-Curve Outcome Autopsy"

assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
OUT_DIR="${SHADOW_LAUNCH57C_OUT_DIR:-$RESULTS_ROOT/22_launch57c_b002_plus_active_curve_outcome_autopsy}"
B002_ROOT="${SHADOW_LAUNCH_B002_ROOT:-$RESULTS_ROOT/run_launch10_hot_r2_press_window_1h_20260601T150013Z}"

latest_run_with_summary() {
    python3 - "$RESULTS_ROOT" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
paths = sorted(
    root.glob("run_launch10_hot_r2_press_window_*/launch10_hot_stream_executor_summary.json"),
    key=lambda p: p.stat().st_mtime,
    reverse=True,
)
for path in paths:
    try:
        data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        continue
    latest = data.get("latest_attempt_summary") or {}
    if latest.get("risk_tier") in {
        "B002_PLUS_ACTIVE_CURVE_LIVE_WINDOW",
        "B002_PLUS_ACTIVE_CURVE_EXTENSION",
    }:
        print(path.parent)
        raise SystemExit(0)
raise SystemExit(1)
PY
}

NEGATIVE_ROOTS_CSV="${SHADOW_LAUNCH57C_NEGATIVE_ROOTS:-$(latest_run_with_summary || true)}"

ARGS=(--positive-root "$B002_ROOT" --out-dir "$OUT_DIR")
IFS=',' read -r -a NEGATIVE_ROOTS <<<"$NEGATIVE_ROOTS_CSV"
for root in "${NEGATIVE_ROOTS[@]}"; do
    root="$(printf '%s' "$root" | xargs)"
    [[ -n "$root" ]] && ARGS+=(--negative-root "$root")
done

python3 "$PACK_DIR/scripts/launch_b002_plus_active_curve_outcome_autopsy.py" "${ARGS[@]}"

pass "Launch57c outcome autopsy complete: $OUT_DIR/summary.json"

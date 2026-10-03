#!/usr/bin/env bash
# 22_launch58_b002_plus_active_curve_live_runner.sh — B002_PLUS active-curve live runner.
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch58 — B002_PLUS Active-Curve Live Runner"

assert_cmd_exists python3

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH58_WINDOW_SECONDS:-3600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
PROOF_PATH="${SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_PROOF_PATH:-$RESULTS_ROOT/22_launch57_b002_plus_active_curve_no_wire/summary.json}"
REQUIRED_DECISION="${SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_REQUIRED_DECISION:-B002_PLUS_ACTIVE_CURVE_READY_PRICE_CHANGE_UNKNOWN}"

[[ -f "$PROOF_PATH" ]] || fail "B002_PLUS active-curve no-wire proof missing: $PROOF_PATH"
python3 - "$PROOF_PATH" "$REQUIRED_DECISION" <<'PY'
import json
import sys
from pathlib import Path

proof_path = Path(sys.argv[1])
required_decision = sys.argv[2]
summary = json.loads(proof_path.read_text(encoding="utf-8"))
rows_path = proof_path.with_name("rows.jsonl")

status = summary.get("status")
ready = int(summary.get("b002_plus_active_curve_ready_rows") or 0)
wire = int(summary.get("wire_send") or 0)
landed = int(summary.get("live_landed") or 0)
replay = summary.get("b002_frozen_replay_status")
regression = summary.get("b002_regression_matrix_status")
if status != "B002_PLUS_ACTIVE_CURVE_NO_WIRE_PASS":
    raise SystemExit(f"invalid Launch57 proof status: {status!r}")
if wire != 0 or landed != 0:
    raise SystemExit(f"Launch57 proof is not no-wire: wire={wire} landed={landed}")
if replay != "B002_FROZEN_REPLAY_PASS":
    raise SystemExit(f"B002 replay did not pass in proof: {replay!r}")
if regression != "B002_REGRESSION_MATRIX_PASS":
    raise SystemExit(f"B002 regression matrix did not pass in proof: {regression!r}")
if ready <= 0:
    raise SystemExit(f"Launch57 proof has no ready rows: ready={ready}")
if not rows_path.exists():
    raise SystemExit(f"Launch57 rows missing: {rows_path}")

row_ready = 0
for line in rows_path.read_text(encoding="utf-8", errors="replace").splitlines():
    if not line.strip():
        continue
    row = json.loads(line)
    if row.get("b002_plus_active_curve_ready") and row.get("b002_plus_active_curve_decision") == required_decision:
        row_ready += 1
if row_ready <= 0:
    raise SystemExit(f"Launch57 rows have no {required_decision} rows")
print(
    "launch58_proof_ok "
    f"status={status} ready={ready} row_ready={row_ready} "
    f"wire={wire} landed={landed} decision={required_decision}"
)
PY

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-B002_PLUS_ACTIVE_CURVE}"
export SHADOW_LAUNCH_HOLD_V2_ENABLED="${SHADOW_LAUNCH_HOLD_V2_ENABLED:-true}"
export SHADOW_LAUNCH_STOP_AFTER_SUCCESS="${SHADOW_LAUNCH_STOP_AFTER_SUCCESS:-false}"
export SHADOW_R2_MAX_SPEND_SOL="${SHADOW_R2_MAX_SPEND_SOL:-0.01}"
export SHADOW_R2_MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-0.015}"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.025}"
export SHADOW_R2_MAX_ATTEMPTS="${SHADOW_R2_MAX_ATTEMPTS:-3}"

export SHADOW_LAUNCH_HUNTER_MODE="true"
export SHADOW_LAUNCH_BATCH002_SIMILARITY_REQUIRED="false"
export SHADOW_LAUNCH_B002_PLUS_REQUIRED="false"
export SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_REQUIRED="true"
export SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_PROOF_PATH="$PROOF_PATH"
export SHADOW_LAUNCH_B002_PLUS_ACTIVE_CURVE_REQUIRED_DECISION="$REQUIRED_DECISION"

export SHADOW_LAUNCH_ENTRY_QUALITY_ENABLED="${SHADOW_LAUNCH_ENTRY_QUALITY_ENABLED:-false}"
export SHADOW_LAUNCH_ENTRY_EDGE_ENABLED="${SHADOW_LAUNCH_ENTRY_EDGE_ENABLED:-false}"
export SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_SIGNAL_AGED_QUALITY_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_AGED_QUALITY_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_SIGNAL_SOFT_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_SOFT_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN="${SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN:-true}"
export SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS="${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CHECKS:-3}"
export SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT="${SHADOW_LAUNCH_EXIT_QUOTE_WEAKENING_CONFIRM_EXIT_COUNT:-3}"

export SHADOW_LAUNCH_EXECUTE_CONFIRM="${SHADOW_LAUNCH_EXECUTE_CONFIRM:-EXECUTE_B002_PLUS_ACTIVE_CURVE_001}"
export SHADOW_R2_EXECUTE_CONFIRM="${SHADOW_R2_EXECUTE_CONFIRM:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"

exec "$SCRIPT_DIR/22_launch13_r2_hold_live_execute.sh" "$WINDOW_SECONDS"

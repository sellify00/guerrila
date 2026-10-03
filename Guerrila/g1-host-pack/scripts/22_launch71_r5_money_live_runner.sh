#!/usr/bin/env bash
# 22_launch71_r5_money_live_runner.sh — guarded R5 money runner
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch71 — R5 Money Live Runner"

assert_cmd_exists python3

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}
source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "/etc/shadow-engine/env.g1.local.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

WINDOW_SECONDS="${1:-${SHADOW_LAUNCH71_WINDOW_SECONDS:-3600}}"
RUN_ROOT="${SHADOW_LAUNCH71_RUN_ROOT:-${2:-}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"

if [[ -z "${SHADOW_LAUNCH_R5_MONEY_PROOF_PATH:-}" ]]; then
    [[ -n "$RUN_ROOT" ]] || fail "Launch71 requires RUN_ROOT or SHADOW_LAUNCH_R5_MONEY_PROOF_PATH"
    [[ -d "$RUN_ROOT" ]] || fail "Launch71 run root missing: $RUN_ROOT"
    if [[ $FAILURES -ne 0 ]]; then
        finish
        exit 1
    fi
    if [[ "${SHADOW_LAUNCH71_RUN_GATE:-true}" == "true" ]]; then
        sudo -E bash "$SCRIPT_DIR/22_launch70_r5_money_gate.sh" "$RUN_ROOT"
    fi
    export SHADOW_LAUNCH_R5_MONEY_PROOF_PATH="$RUN_ROOT/22_launch70_r5_money_gate/summary.json"
fi

PROOF_PATH="$SHADOW_LAUNCH_R5_MONEY_PROOF_PATH"
[[ -f "$PROOF_PATH" ]] || fail "Launch71 R5 money proof missing: $PROOF_PATH"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

READY_ROWS="$(python3 - <<PY
import json
from pathlib import Path
p = Path("$PROOF_PATH")
data = json.loads(p.read_text(encoding="utf-8", errors="replace"))
print(int(data.get("r5_candidate_ready_rows") or 0))
PY
)"
if [[ "$READY_ROWS" -le 0 ]]; then
    fail "Launch71 refusing live path: r5_candidate_ready_rows=$READY_ROWS"
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH71_RUN_B002_REGRESSION:-true}" == "true" ]]; then
    RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
    SHADOW_LAUNCH_B002_REPLAY_OUT_DIR="$RESULTS_ROOT/22_launch71_b002_replay_$RUN_ID" \
        bash "$SCRIPT_DIR/22_launch_b002_replay_frozen.sh"
    SHADOW_LAUNCH_B002_REGRESSION_OUT_DIR="$RESULTS_ROOT/22_launch71_b002_regression_$RUN_ID" \
        bash "$SCRIPT_DIR/22_launch_b002_regression_matrix.sh"
fi

export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-R5_PHASE_TRANSITION_MONEY}"
export SHADOW_LAUNCH_HOLD_V2_ENABLED="${SHADOW_LAUNCH_HOLD_V2_ENABLED:-true}"
export SHADOW_LAUNCH_STOP_AFTER_SUCCESS="${SHADOW_LAUNCH_STOP_AFTER_SUCCESS:-false}"
export SHADOW_LAUNCH_HUNTER_MODE="${SHADOW_LAUNCH_HUNTER_MODE:-true}"
export SHADOW_R2_MAX_SPEND_SOL="${SHADOW_R2_MAX_SPEND_SOL:-0.01}"
export SHADOW_R2_MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-0.015}"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.025}"
export SHADOW_R2_MAX_ATTEMPTS="${SHADOW_R2_MAX_ATTEMPTS:-1}"
export SHADOW_LAUNCH5_DAILY_MAX_TRADES="${SHADOW_LAUNCH5_DAILY_MAX_TRADES:-$SHADOW_R2_MAX_ATTEMPTS}"

# R5 money proof is a hard whitelist for this runner. If the hot stream does not
# see one of these mints again, no trade starts.
export SHADOW_LAUNCH_R5_MONEY_REQUIRED="true"
export SHADOW_LAUNCH_R5_MONEY_READY_MAX_AGE_MS="${SHADOW_LAUNCH_R5_MONEY_READY_MAX_AGE_MS:-300000}"
export SHADOW_LAUNCH_ENTRY_QUALITY_ENABLED="${SHADOW_LAUNCH_ENTRY_QUALITY_ENABLED:-false}"
export SHADOW_LAUNCH_ENTRY_EDGE_ENABLED="${SHADOW_LAUNCH_ENTRY_EDGE_ENABLED:-false}"
export SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_SIGNAL_AGED_QUALITY_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_AGED_QUALITY_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_SIGNAL_SOFT_MAX_AGE_MS="${SHADOW_LAUNCH_SIGNAL_SOFT_MAX_AGE_MS:-60000}"
export SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN="${SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN:-true}"

export SHADOW_LAUNCH_EXECUTE_CONFIRM="${SHADOW_LAUNCH_EXECUTE_CONFIRM:-MISMATCH_R5_MONEY_NO_WIRE}"
export SHADOW_R2_EXECUTE_CONFIRM="${SHADOW_R2_EXECUTE_CONFIRM:-$SHADOW_LAUNCH_EXECUTE_CONFIRM}"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_R5_MONEY_LIVE_001}"

info "Launch71 R5 proof=$PROOF_PATH ready_rows=$READY_ROWS"
info "Launch71 execute_confirm=$SHADOW_R2_EXECUTE_CONFIRM required=$SHADOW_R2_EXECUTE_CONFIRM_REQUIRED"

exec "$SCRIPT_DIR/22_launch13_r2_hold_live_execute.sh" "$WINDOW_SECONDS"

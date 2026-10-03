#!/usr/bin/env bash
# 22_launch0u2_active_curve_warmup_blocker_audit.sh — no-wire active curve blocker split
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0U2 — Active Curve Warmup Blocker Audit"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0U2_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0U_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0u_active_bonding_curve_r3_candidate_gate' || true)"
    [[ -f "$LAUNCH0U_SUMMARY" ]] || fail "Launch0U2 source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0U_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0U2 run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0U2_REFRESH_U_GATE:-true}" != "false" ]]; then
    SHADOW_LAUNCH0U_REFRESH_GATES=false bash "$SCRIPT_DIR/22_launch0u_active_bonding_curve_r3_candidate_gate.sh" "$RUN_ROOT"
fi

LAUNCH0U_ROWS="${SHADOW_LAUNCH0U2_LAUNCH0U_ROWS:-$RUN_ROOT/22_launch0u_active_bonding_curve_r3_candidate_gate/rows.jsonl}"
OUT_DIR="${SHADOW_LAUNCH0U2_OUT_DIR:-$RUN_ROOT/22_launch0u2_active_curve_warmup_blocker_audit}"

[[ -f "$LAUNCH0U_ROWS" ]] || fail "Launch0U2 Launch0U rows missing: $LAUNCH0U_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_active_curve_warmup_blocker_audit.py" \
    --run-root "$RUN_ROOT" \
    --launch0u-rows "$LAUNCH0U_ROWS" \
    --out-dir "$OUT_DIR" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-0.01}" \
    --max-loss-sol "${SHADOW_R2_MAX_LOSS_SOL:-0.015}" \
    --max-cash-at-risk-sol "${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.025}" \
    --entry-tip-lamports "${SHADOW_LAUNCH_ENTRY_TIP_LAMPORTS:-200000}" \
    --exit-tip-lamports "${SHADOW_LAUNCH_EXIT_TIP_LAMPORTS:-200000}" \
    --entry-base-fee-lamports "${SHADOW_LAUNCH_ENTRY_BASE_FEE_LAMPORTS:-5000}" \
    --exit-base-fee-lamports "${SHADOW_LAUNCH_EXIT_BASE_FEE_LAMPORTS:-5000}" \
    --unrecoverable-inline-account-lamports "${SHADOW_LAUNCH_UNRECOVERABLE_INLINE_ACCOUNT_LAMPORTS:-0}" \
    --slippage-buffer-lamports "${SHADOW_LAUNCH_SLIPPAGE_BUFFER_LAMPORTS:-1000000}" \
    --panic-exit-buffer-lamports "${SHADOW_LAUNCH_PANIC_EXIT_BUFFER_LAMPORTS:-1000000}" \
    --inline-account-recovery-proven "${SHADOW_LAUNCH_INLINE_ACCOUNT_RECOVERY_PROVEN:-true}" \
    --assume-pm-ready "${SHADOW_LAUNCH0U2_ASSUME_PM_READY:-true}"

WIRE_SEND="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("wire_send") or 0))
PY
)"
LIVE_LANDED="$(python3 - <<PY
import json
from pathlib import Path
data=json.loads(Path("$OUT_DIR/summary.json").read_text())
print(int(data.get("live_landed") or 0))
PY
)"
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0U2 no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0U2 no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0U2 active curve blocker audit written to $OUT_DIR/summary.txt"
finish

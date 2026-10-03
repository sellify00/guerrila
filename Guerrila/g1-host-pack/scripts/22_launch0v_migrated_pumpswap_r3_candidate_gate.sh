#!/usr/bin/env bash
# 22_launch0v_migrated_pumpswap_r3_candidate_gate.sh — no-wire migrated PumpSwap R3 candidate gate
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "22 Launch0V — Migrated PumpSwap R3 Candidate Gate"

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
RUN_ROOT="${1:-${SHADOW_LAUNCH0V_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0M_SUMMARY="$(latest_file summary.json | grep -m1 '22_launch0m_migration_pumpswap_route_join' || true)"
    if [[ -z "$LAUNCH0M_SUMMARY" ]]; then
        LAUNCH0M_SUMMARY="$(latest_file launch0q_realtime_launch_quote_state_summary.json)"
    fi
    [[ -f "$LAUNCH0M_SUMMARY" ]] || fail "Launch0V source summary missing"
    RUN_ROOT="$(dirname "$(dirname "$LAUNCH0M_SUMMARY")")"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0V run root missing: $RUN_ROOT"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

if [[ "${SHADOW_LAUNCH0V_REFRESH_GATES:-true}" != "false" ]]; then
    SHADOW_LAUNCH0M_RUN_DOWNSTREAM_GATES=false bash "$SCRIPT_DIR/22_launch0m_migration_pumpswap_route_join.sh" "$RUN_ROOT"
    bash "$SCRIPT_DIR/22_launch24_entry_quality_gate.sh" "$RUN_ROOT"
    bash "$SCRIPT_DIR/22_launch35_absorption_no_exit_fragility_gate.sh" "$RUN_ROOT"
fi

if [[ -n "${SHADOW_LAUNCH0V_LAUNCH0M_ROWS:-}" ]]; then
    LAUNCH0M_ROWS="$SHADOW_LAUNCH0V_LAUNCH0M_ROWS"
elif [[ -f "$RUN_ROOT/22_launch0v4_migrated_pumpswap_flow_join_repair/rows.jsonl" ]]; then
    LAUNCH0M_ROWS="$RUN_ROOT/22_launch0v4_migrated_pumpswap_flow_join_repair/rows.jsonl"
elif [[ -f "$RUN_ROOT/22_launch0v3_migrated_pumpswap_flow_state_hydrator/rows.jsonl" ]]; then
    LAUNCH0M_ROWS="$RUN_ROOT/22_launch0v3_migrated_pumpswap_flow_state_hydrator/rows.jsonl"
else
    LAUNCH0M_ROWS="$RUN_ROOT/22_launch0m_migration_pumpswap_route_join/rows.jsonl"
fi
OUT_DIR="${SHADOW_LAUNCH0V_OUT_DIR:-$RUN_ROOT/22_launch0v_migrated_pumpswap_r3_candidate_gate}"

[[ -f "$LAUNCH0M_ROWS" ]] || fail "Launch0V Launch0M rows missing: $LAUNCH0M_ROWS"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

python3 "$PACK_DIR/scripts/launch_migrated_pumpswap_r3_candidate_gate.py" \
    --run-root "$RUN_ROOT" \
    --launch0m-rows "$LAUNCH0M_ROWS" \
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
    --assume-pm-ready "${SHADOW_LAUNCH0V_ASSUME_PM_READY:-true}" \
    --hold-v2-allowed "${SHADOW_LAUNCH_HOLD_V2_ENABLED:-true}" \
    --hunter-mode "${SHADOW_LAUNCH_HUNTER_MODE:-false}" \
    --min-price-change-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_MIN_PRICE_CHANGE_BPS:-100}" \
    --max-sell-count-2s "${SHADOW_LAUNCH_ENTRY_QUALITY_MAX_SELL_COUNT_2S:-0}" \
    --panic-drop-2s-bps "${SHADOW_LAUNCH_ENTRY_QUALITY_PANIC_DROP_2S_BPS:-700}" \
    --edge-min-price-change-bps "${SHADOW_LAUNCH_ENTRY_EDGE_MIN_PRICE_CHANGE_BPS:-300}" \
    --edge-max-price-change-bps "${SHADOW_LAUNCH_ENTRY_EDGE_MAX_PRICE_CHANGE_BPS:-1500}" \
    --edge-max-signal-age-ms "${SHADOW_LAUNCH_SIGNAL_MAX_AGE_MS:-8000}" \
    --edge-min-net-buy-pressure-5s "${SHADOW_LAUNCH_ENTRY_EDGE_MIN_NET_BUY_PRESSURE_5S:-1}"

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
[[ "$WIRE_SEND" == "0" ]] || fail "Launch0V no-wire invariant failed: wire_send=$WIRE_SEND"
[[ "$LIVE_LANDED" == "0" ]] || fail "Launch0V no-wire invariant failed: live_landed=$LIVE_LANDED"

pass "Launch0V migrated PumpSwap R3 candidate gate written to $OUT_DIR/summary.txt"
finish

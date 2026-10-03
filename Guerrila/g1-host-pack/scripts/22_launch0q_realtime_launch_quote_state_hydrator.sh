#!/usr/bin/env bash
# 22_launch0q_realtime_launch_quote_state_hydrator.sh — hydrate realtime launch states with executable quote/liquidity state

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0Q — Realtime Launch Quote-State Hydrator"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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
RUN_ROOT="${1:-${SHADOW_LAUNCH0Q_RUN_ROOT:-}}"

latest_file() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

if [[ -z "$RUN_ROOT" ]]; then
    LAUNCH0D_SUMMARY="${SHADOW_LAUNCH0Q_LAUNCH0D_SUMMARY:-$(latest_file launch0d_realtime_birth_decoder_summary.json)}"
    [[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch0D realtime launch-state summary missing"
    RUN_ROOT="$(dirname "$LAUNCH0D_SUMMARY")"
else
    LAUNCH0D_SUMMARY="${SHADOW_LAUNCH0Q_LAUNCH0D_SUMMARY:-$RUN_ROOT/launch0d_realtime_birth_decoder_summary.json}"
fi

[[ -d "$RUN_ROOT" ]] || fail "Launch0Q run root missing: $RUN_ROOT"
[[ -f "$LAUNCH0D_SUMMARY" ]] || fail "Launch0D summary missing: $LAUNCH0D_SUMMARY"

INPUT_ROWS="${SHADOW_LAUNCH0Q_INPUT_ROWS:-}"
if [[ -z "$INPUT_ROWS" ]]; then
    INPUT_ROWS="$(python3 - <<PY
import json
from pathlib import Path
p = Path("$LAUNCH0D_SUMMARY")
try:
    data = json.loads(p.read_text())
except Exception:
    data = {}
print(((data.get("source_paths") or {}).get("birth_rows") or ""))
PY
)"
fi
if [[ -z "$INPUT_ROWS" || ! -f "$INPUT_ROWS" ]]; then
    INPUT_ROWS="$RUN_ROOT/launch0d_realtime_birth_decoder_rows.jsonl"
fi

HYDRATOR_PATH="$PACK_DIR/scripts/launch_realtime_quote_state_hydrator.py"
LANE_PATH="$PACK_DIR/scripts/launch_follow_lane.py"
ROWS_OUT="$RUN_ROOT/launch0q_realtime_launch_quote_state_rows.jsonl"
SUMMARY_JSON="$RUN_ROOT/launch0q_realtime_launch_quote_state_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0q_realtime_launch_quote_state_summary.txt"
BASELINE_CACHE="${SHADOW_LAUNCH0Q_BASELINE_CACHE:-$RUN_ROOT/launch0q_quote_baseline_cache.json}"
RPC_URL="${SHADOW_LAUNCH0Q_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

[[ -f "$INPUT_ROWS" ]] || fail "Launch0Q input launch-state rows missing: $INPUT_ROWS"
[[ -f "$HYDRATOR_PATH" ]] || fail "Launch0Q hydrator missing: $HYDRATOR_PATH"
[[ -f "$LANE_PATH" ]] || fail "Launch lane module missing: $LANE_PATH"
[[ -n "$RPC_URL" ]] || fail "Launch0Q RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-${SHADOW_LAUNCH0Q_MAX_LOSS_SOL:-0.015}}"
MAX_LOSS_LAMPORTS="$(python3 - <<PY
print(int(round(float("$MAX_LOSS_SOL") * 1_000_000_000)))
PY
)"

python3 "$HYDRATOR_PATH" \
    --run-root "$RUN_ROOT" \
    --input-rows "$INPUT_ROWS" \
    --launch-lane-path "$LANE_PATH" \
    --rpc-url "$RPC_URL" \
    --max-spend-sol "${SHADOW_R2_MAX_SPEND_SOL:-${SHADOW_LAUNCH0Q_MAX_SPEND_SOL:-0.01}}" \
    --max-loss-lamports "$MAX_LOSS_LAMPORTS" \
    --slippage-bps "${SHADOW_LAUNCH0Q_SLIPPAGE_BPS:-${SHADOW_LAUNCH5_SLIPPAGE_BPS:-100}}" \
    --fee-budget-lamports "${SHADOW_LAUNCH0Q_FEE_BUDGET_LAMPORTS:-410000}" \
    --exit-safety-bps "${SHADOW_LAUNCH0Q_EXIT_SAFETY_BPS:-10000}" \
    --min-base-reserve-lamports "${SHADOW_LAUNCH_EXIT_MIN_BASE_RESERVE_LAMPORTS:-25000000}" \
    --reserve-after-exit-floor-lamports "${SHADOW_LAUNCH_EXIT_RESERVE_AFTER_EXIT_FLOOR_LAMPORTS:-10000000}" \
    --max-exit-price-impact-bps "${SHADOW_LAUNCH_EXIT_GUARD_MAX_PRICE_IMPACT_BPS:-1500}" \
    --baseline-cache "$BASELINE_CACHE" \
    --rows-out "$ROWS_OUT" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

info "Launch0Q quote-state summary written to $SUMMARY_JSON"

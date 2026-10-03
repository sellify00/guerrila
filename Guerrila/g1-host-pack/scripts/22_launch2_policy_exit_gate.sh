#!/usr/bin/env bash
# 22_launch2_policy_exit_gate.sh — launch-follow policy and exit-size gate before any ATA or acquisition

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch2 — Policy / Exit Gate"

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
STEP="${SHADOW_LAUNCH2_STEP:-22_launch2_policy_exit_gate}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch2_policy_exit_gate_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch2_policy_exit_gate_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
INPUT_ROWS_PATH="${SHADOW_LAUNCH2_INPUT_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
RPC_URL="${SHADOW_LAUNCH2_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
SIZE_LIST="${SHADOW_LAUNCH2_SIZE_LIST:-0.001,0.002,0.003,0.005}"
MAX_SPEND_SOL="${SHADOW_LAUNCH2_MAX_SPEND_SOL:-0.003}"
MAX_LOSS_SOL="${SHADOW_LAUNCH2_MAX_LOSS_SOL:-0.0015}"
SLIPPAGE_BPS="${SHADOW_LAUNCH2_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCH2_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCH2_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LAUNCH2_TIP_LAMPORTS:-0}"
EXIT_SAFETY_BPS="${SHADOW_LAUNCH2_EXIT_SAFETY_BPS:-9970}"
ALLOW_TOKEN2022="${SHADOW_LAUNCH2_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_LAUNCH2_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_LAUNCH2_ALLOW_MINT_AUTHORITY:-false}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$INPUT_ROWS_PATH" ]] || INPUT_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

PY_ARGS=(
    "$PACK_DIR/scripts/launch_follow_lane.py" launch2
    --input-rows "$INPUT_ROWS_PATH"
    --rows-out "$ROWS_OUT"
    --summary-json "$SUMMARY_JSON"
    --summary-txt "$SUMMARY_TXT"
    --rpc-url "$RPC_URL"
    --size-list "$SIZE_LIST"
    --max-spend-sol "$MAX_SPEND_SOL"
    --max-loss-sol "$MAX_LOSS_SOL"
    --slippage-bps "$SLIPPAGE_BPS"
    --base-tx-fee-lamports "$BASE_TX_FEE_LAMPORTS"
    --priority-fee-lamports "$PRIORITY_FEE_LAMPORTS"
    --tip-lamports "$TIP_LAMPORTS"
    --exit-safety-bps "$EXIT_SAFETY_BPS"
)

value_is_true "$ALLOW_TOKEN2022" && PY_ARGS+=(--allow-token2022)
value_is_true "$ALLOW_FREEZE_AUTHORITY" && PY_ARGS+=(--allow-freeze-authority)
value_is_true "$ALLOW_MINT_AUTHORITY" && PY_ARGS+=(--allow-mint-authority)

python3 "${PY_ARGS[@]}"

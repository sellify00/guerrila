#!/usr/bin/env bash
# 22_launch3_tiny_inventory_sim.sh — no-send tiny inventory sim for launch-follow candidates

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch3 — Tiny Inventory Sim"

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
STEP="${SHADOW_LAUNCH3_STEP:-22_launch3_tiny_inventory_sim}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch3_tiny_inventory_sim_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch3_tiny_inventory_sim_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
INPUT_ROWS_PATH="${SHADOW_LAUNCH3_INPUT_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
RPC_URL="${SHADOW_LAUNCH3_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCH3_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
MAX_SPEND_SOL="${SHADOW_LAUNCH3_MAX_SPEND_SOL:-0.003}"
MAX_LOSS_SOL="${SHADOW_LAUNCH3_MAX_LOSS_SOL:-0.0015}"
TTL_SECONDS="${SHADOW_LAUNCH3_TTL_SECONDS:-30}"
STOP_LOSS_BPS="${SHADOW_LAUNCH3_STOP_LOSS_BPS:-300}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCH3_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCH3_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LAUNCH3_TIP_LAMPORTS:-0}"
MAX_CASH_AT_RISK_SOL="${SHADOW_LAUNCH3_MAX_CASH_AT_RISK_SOL:-0.006}"
ALLOW_INLINE_ACCOUNT_CREATE="${SHADOW_LAUNCH3_ALLOW_INLINE_ACCOUNT_CREATE:-true}"

if [[ ! -r "$SIGNER_KEYPAIR_PATH" ]] && [[ "$(id -u)" != "0" ]] && command -v sudo >/dev/null 2>&1; then
    exec sudo -E bash "$0" "$@"
fi

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$INPUT_ROWS_PATH" ]] || INPUT_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

extra_args=()
if [[ "${ALLOW_INLINE_ACCOUNT_CREATE,,}" =~ ^(1|true|yes|y|on)$ ]]; then
    extra_args+=(--allow-inline-account-create)
fi

python3 "$PACK_DIR/scripts/launch_follow_lane.py" launch3 \
    --input-rows "$INPUT_ROWS_PATH" \
    --rows-out "$ROWS_OUT" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT" \
    --rpc-url "$RPC_URL" \
    --signer-keypair "$SIGNER_KEYPAIR_PATH" \
    --max-spend-sol "$MAX_SPEND_SOL" \
    --max-loss-sol "$MAX_LOSS_SOL" \
    --ttl-seconds "$TTL_SECONDS" \
    --stop-loss-bps "$STOP_LOSS_BPS" \
    --base-tx-fee-lamports "$BASE_TX_FEE_LAMPORTS" \
    --priority-fee-lamports "$PRIORITY_FEE_LAMPORTS" \
    --tip-lamports "$TIP_LAMPORTS" \
    --max-cash-at-risk-sol "$MAX_CASH_AT_RISK_SOL" \
    "${extra_args[@]}"

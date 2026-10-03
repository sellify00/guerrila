#!/usr/bin/env bash
# 22_launch5_one_mint_live_execute.sh — launch-follow one-mint live executor with ordered bundle entry and Sender exit

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch5 — One-Mint Live Execute"

assert_root
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
STEP="${SHADOW_LAUNCH5_STEP:-22_launch5_one_mint_live_execute}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
REPORT_JSON="$RESULT_DIR/launch5_one_mint_live_execute.json"
SUMMARY_JSON="$RESULT_DIR/launch5_one_mint_live_execute_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PROPOSAL_JSON_PATH="${SHADOW_LAUNCH5_PROPOSAL_JSON_PATH:-$RESULTS_ROOT/22_launch4_one_mint_live_proposal/launch4_one_mint_live_proposal.json}"
PROPOSAL_ROWS_PATH="${SHADOW_LAUNCH5_PROPOSAL_ROWS_PATH:-$RESULTS_ROOT/22_launch4_one_mint_live_proposal/launch4_one_mint_live_proposal_rows.jsonl}"
BUNDLE_ROWS_PATH="${SHADOW_LAUNCH5_BUNDLE_ROWS_PATH:-$RESULTS_ROOT/22_launch4_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LAUNCH5_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LAUNCH5_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
REFRESH_INVENTORY_SCRIPT="${SHADOW_LAUNCH5_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"

SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCH5_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_LAUNCH5_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-${HELIUS_SENDER_URL:-}}"
SENDER_API_KEY="${SHADOW_HELIUS_SENDER_API_KEY:-}"
BUNDLE_RPC_URL="${SHADOW_LAUNCH5_BUNDLE_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-}}}}}"
BUNDLE_API_KEY="${SHADOW_LAUNCH5_BUNDLE_API_KEY:-${SHADOW_HELIUS_BUNDLE_API_KEY:-}}"

TARGET_MINT="${SHADOW_LAUNCH5_MINT:-${SHADOW_R2_MINT:-${SHADOW_R1_MINT:-}}}"
TARGET_POOL="${SHADOW_LAUNCH5_POOL:-${SHADOW_R2_POOL:-${SHADOW_R1_POOL:-}}}"
RISK_TIER="${SHADOW_LAUNCH5_RISK_TIER:-${SHADOW_R1_RISK_TIER:-R1_LAUNCH_CONTINUATION_TINY}}"
MAX_SPEND_SOL="${SHADOW_LAUNCH5_MAX_SPEND_SOL:-${SHADOW_R1_MAX_SPEND_SOL:-0.003}}"
MAX_LOSS_SOL="${SHADOW_LAUNCH5_MAX_LOSS_SOL:-${SHADOW_R1_MAX_LOSS_SOL:-0.0015}}"
TTL_SECONDS="${SHADOW_LAUNCH_EXIT_MAX_HOLD_SECONDS:-${SHADOW_LAUNCH5_TTL_SECONDS:-${SHADOW_R1_TTL_SECONDS:-30}}}"
STOP_LOSS_BPS="${SHADOW_LAUNCH5_STOP_LOSS_BPS:-${SHADOW_R1_STOP_LOSS_BPS:-300}}"
TAKE_PROFIT_BPS="${SHADOW_LAUNCH5_TAKE_PROFIT_BPS:-${SHADOW_R1_TAKE_PROFIT_BPS:-1000}}"
PRESEND_WAIT_SECONDS="${SHADOW_LAUNCH5_PRESEND_WAIT_SECONDS:-${SHADOW_R1_PRESEND_WAIT_SECONDS:-30}}"
PRESEND_QUOTE_POLL_MS="${SHADOW_LAUNCH5_PRESEND_QUOTE_POLL_MS:-${SHADOW_R1_PRESEND_QUOTE_POLL_MS:-2000}}"
REQUIRE_DIRECT_PUMPFUN_EXIT="${SHADOW_LAUNCH5_REQUIRE_DIRECT_PUMPFUN_EXIT:-${SHADOW_R1_REQUIRE_DIRECT_PUMPFUN_EXIT:-true}}"
EXECUTE_CONFIRM="${SHADOW_LAUNCH5_EXECUTE_CONFIRM:-${SHADOW_R2_EXECUTE_CONFIRM:-${SHADOW_R1_EXECUTE_CONFIRM:-}}}"
EXECUTE_CONFIRM_REQUIRED="${SHADOW_LAUNCH5_EXECUTE_CONFIRM_REQUIRED:-${SHADOW_R2_EXECUTE_CONFIRM_REQUIRED:-${SHADOW_R1_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_LAUNCH_ONE_MINT_0003SOL}}}"
SLIPPAGE_BPS="${SHADOW_LAUNCH5_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCH5_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCH5_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LAUNCH5_TIP_LAMPORTS:-200000}"
SENDER_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LAUNCH5_SENDER_TIP_LAMPORTS:-200000}}"
SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-${SHADOW_LAUNCH5_SENDER_TIP_ACCOUNT:-}}"
MONITOR_POLL_SECONDS="${SHADOW_LAUNCH5_MONITOR_POLL_SECONDS:-1}"
FLATTEN_RETRY_LIMIT="${SHADOW_LAUNCH5_FLATTEN_RETRY_LIMIT:-1}"
EXPECTED_PROPOSAL_VERDICT="${SHADOW_LAUNCH5_EXPECTED_PROPOSAL_VERDICT:-LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE}"
HOT_MODE="${SHADOW_LAUNCH_HOT_MODE:-false}"
ORDERED_BUNDLE_IMMEDIATE="${SHADOW_LAUNCH_ORDERED_BUNDLE_IMMEDIATE:-false}"
SKIP_TARGET_STATUS_PREFLIGHT="${SHADOW_LAUNCH_SKIP_TARGET_STATUS_PREFLIGHT:-false}"
ALLOW_LATE_FOLLOW="${SHADOW_LAUNCH_ALLOW_LATE_FOLLOW:-false}"
LATE_FOLLOW_MAX_AGE_MS="${SHADOW_LAUNCH_LATE_FOLLOW_MAX_AGE_MS:-1000}"
SKIP_TARGET_BLOCKHASH_PREFLIGHT="${SHADOW_LAUNCH_SKIP_TARGET_BLOCKHASH_PREFLIGHT:-false}"
SKIP_ENTRY_BUNDLE_SIM_PREFLIGHT="${SHADOW_LAUNCH_SKIP_ENTRY_BUNDLE_SIM_PREFLIGHT:-false}"
SKIP_LATE_FOLLOW_SIM_PREFLIGHT="${SHADOW_LAUNCH_SKIP_LATE_FOLLOW_SIM_PREFLIGHT:-false}"
LAUNCH_EXIT_ENABLED="${SHADOW_LAUNCH_EXIT_ENABLED:-true}"
LAUNCH_EXIT_HARD_STOP_BPS="${SHADOW_LAUNCH_EXIT_HARD_STOP_BPS:-2500}"
LAUNCH_EXIT_TRAILING_ENABLED="${SHADOW_LAUNCH_EXIT_TRAILING_ENABLED:-true}"
LAUNCH_EXIT_TRAILING_ARM_GAIN_BPS="${SHADOW_LAUNCH_EXIT_TRAILING_ARM_GAIN_BPS:-2000}"
LAUNCH_EXIT_TRAILING_STOP_BPS="${SHADOW_LAUNCH_EXIT_TRAILING_STOP_BPS:-1000}"
LAUNCH_EXIT_VELOCITY_ENABLED="${SHADOW_LAUNCH_EXIT_VELOCITY_ENABLED:-true}"
LAUNCH_EXIT_DROP_1S_BPS="${SHADOW_LAUNCH_EXIT_DROP_1S_BPS:-500}"
LAUNCH_EXIT_DROP_2S_BPS="${SHADOW_LAUNCH_EXIT_DROP_2S_BPS:-700}"
LAUNCH_EXIT_DROP_5S_BPS="${SHADOW_LAUNCH_EXIT_DROP_5S_BPS:-1200}"
LAUNCH_EXIT_DROP_10S_BPS="${SHADOW_LAUNCH_EXIT_DROP_10S_BPS:-800}"
LAUNCH_EXIT_LIQUIDITY_ENABLED="${SHADOW_LAUNCH_EXIT_LIQUIDITY_ENABLED:-true}"
LAUNCH_EXIT_MAX_PRICE_IMPACT_BPS="${SHADOW_LAUNCH_EXIT_MAX_PRICE_IMPACT_BPS:-1500}"
LAUNCH_EXIT_MAX_SLIPPAGE_BPS="${SHADOW_LAUNCH_EXIT_MAX_SLIPPAGE_BPS:-2000}"
LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS="${SHADOW_LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS:-3000}"
LAUNCH_EXIT_TAKE_PROFIT_BPS="${SHADOW_LAUNCH_EXIT_TAKE_PROFIT_BPS:-0}"
LAUNCH_EXIT_RESCUE_ON_SIM_FAIL="${SHADOW_LAUNCH_EXIT_RESCUE_ON_SIM_FAIL:-true}"
LAUNCH_EXIT_RESCUE_ON_SEND_FAIL="${SHADOW_LAUNCH_EXIT_RESCUE_ON_SEND_FAIL:-true}"
LAUNCH_EXIT_MAX_EXIT_ATTEMPTS="${SHADOW_LAUNCH_EXIT_MAX_EXIT_ATTEMPTS:-3}"
LAUNCH_EXIT_MANUAL_KILL_FILE="${SHADOW_LAUNCH_EXIT_MANUAL_KILL_FILE:-}"

DAILY_GUARD_ROOT="${SHADOW_R1_DAILY_GUARD_ROOT:-$PACK_DIR/results/runtime_daily_risk}"
mkdir -p "$DAILY_GUARD_ROOT"
TODAY_UTC="$(date -u +%F)"
export SHADOW_R1_DAILY_LEDGER_PATH="${SHADOW_R1_DAILY_LEDGER_PATH:-$DAILY_GUARD_ROOT/${STEP}_${TODAY_UTC}.json}"
export SHADOW_R1_DAILY_PROFIT_TARGET_USD="${SHADOW_LAUNCH5_DAILY_PROFIT_TARGET_USD:-${SHADOW_R1_DAILY_PROFIT_TARGET_USD:-60}}"
export SHADOW_R1_DAILY_MAX_LOSS_USD="${SHADOW_LAUNCH5_DAILY_MAX_LOSS_USD:-${SHADOW_R1_DAILY_MAX_LOSS_USD:-25}}"
export SHADOW_R1_DAILY_MAX_TRADES="${SHADOW_LAUNCH5_DAILY_MAX_TRADES:-${SHADOW_R1_DAILY_MAX_TRADES:-12}}"
export SHADOW_R1_DAILY_SOL_USD="${SHADOW_LAUNCH5_DAILY_SOL_USD:-${SHADOW_R1_DAILY_SOL_USD:-${SHADOW_RUN60_SOL_USD_EQUIVALENT:-0}}}"

latest_file_for_pattern() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PROPOSAL_JSON_PATH" ]] || PROPOSAL_JSON_PATH="$(latest_file_for_pattern launch4_one_mint_live_proposal.json)"
[[ -f "$PROPOSAL_ROWS_PATH" ]] || PROPOSAL_ROWS_PATH="$(latest_file_for_pattern launch4_one_mint_live_proposal_rows.jsonl)"
[[ -f "$BUNDLE_ROWS_PATH" ]] || BUNDLE_ROWS_PATH="$(latest_file_for_pattern launch4_entry_bundle_nosubmit_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_file_for_pattern runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_file_for_pattern runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT"

# Re-run the launch live proposal before any live entry.
bash "$PACK_DIR/scripts/22_launch4_one_mint_live_proposal.sh"

if [[ -f "$PROPOSAL_JSON_PATH" ]]; then
    PROPOSAL_DERIVED="$(
        python3 - "$PROPOSAL_JSON_PATH" <<'PY'
import json, sys
from pathlib import Path

path = Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8", errors="replace"))
top = doc.get("top_candidate") or {}
launch_mint = str(top.get("launch_mint") or "").strip()
pool = str(top.get("pool") or "").strip()
signature = str(top.get("latest_signature") or top.get("target_signature") or "").strip()
print(launch_mint)
print(pool)
print(signature)
PY
    )"
    DERIVED_TARGET_MINT="$(printf '%s\n' "$PROPOSAL_DERIVED" | sed -n '1p')"
    DERIVED_TARGET_POOL="$(printf '%s\n' "$PROPOSAL_DERIVED" | sed -n '2p')"
    if [[ -z "$TARGET_MINT" ]]; then
        TARGET_MINT="$DERIVED_TARGET_MINT"
    fi
    if [[ -z "$TARGET_POOL" ]]; then
        TARGET_POOL="$DERIVED_TARGET_POOL"
    fi
fi

python3 "$PACK_DIR/scripts/launch_follow_live_execute.py" \
    --step "$STEP" \
    --proposal-json "$PROPOSAL_JSON_PATH" \
    --proposal-rows "$PROPOSAL_ROWS_PATH" \
    --bundle-rows "$BUNDLE_ROWS_PATH" \
    --ls13a-rows "$LS13A_ROWS_PATH" \
    --alt-rows "$ALT_AUDIT_ROWS_PATH" \
    --utils-path "$UTILS_PATH" \
    --input-manifest "$INPUT_MANIFEST" \
    --report-json "$REPORT_JSON" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT" \
    --signer-keypair "$SIGNER_KEYPAIR_PATH" \
    --rpc-url "$RPC_URL" \
    --sender-url "$SENDER_URL" \
    --sender-api-key "$SENDER_API_KEY" \
    --bundle-rpc-url "$BUNDLE_RPC_URL" \
    --bundle-api-key "$BUNDLE_API_KEY" \
    --refresh-inventory-script "$REFRESH_INVENTORY_SCRIPT" \
    --target-mint "$TARGET_MINT" \
    --target-pool "$TARGET_POOL" \
    --risk-tier "$RISK_TIER" \
    --max-spend-sol "$MAX_SPEND_SOL" \
    --max-loss-sol "$MAX_LOSS_SOL" \
    --ttl-seconds "$TTL_SECONDS" \
    --stop-loss-bps "$STOP_LOSS_BPS" \
    --take-profit-bps "$TAKE_PROFIT_BPS" \
    --presend-wait-seconds "$PRESEND_WAIT_SECONDS" \
    --presend-quote-poll-ms "$PRESEND_QUOTE_POLL_MS" \
    --require-direct-pumpfun-exit "$REQUIRE_DIRECT_PUMPFUN_EXIT" \
    --execute-confirm "$EXECUTE_CONFIRM" \
    --execute-confirm-required "$EXECUTE_CONFIRM_REQUIRED" \
    --slippage-bps "$SLIPPAGE_BPS" \
    --base-tx-fee-lamports "$BASE_TX_FEE_LAMPORTS" \
    --priority-fee-lamports "$PRIORITY_FEE_LAMPORTS" \
    --tip-lamports "$TIP_LAMPORTS" \
    --sender-tip-lamports "$SENDER_TIP_LAMPORTS" \
    --sender-tip-account "$SENDER_TIP_ACCOUNT" \
    --monitor-poll-seconds "$MONITOR_POLL_SECONDS" \
    --flatten-retry-limit "$FLATTEN_RETRY_LIMIT" \
    --expected-proposal-verdict "$EXPECTED_PROPOSAL_VERDICT" \
    --hot-mode "$HOT_MODE" \
    --ordered-bundle-immediate "$ORDERED_BUNDLE_IMMEDIATE" \
    --skip-target-status-preflight "$SKIP_TARGET_STATUS_PREFLIGHT" \
    --allow-late-follow "$ALLOW_LATE_FOLLOW" \
    --late-follow-max-age-ms "$LATE_FOLLOW_MAX_AGE_MS" \
    --skip-target-blockhash-preflight "$SKIP_TARGET_BLOCKHASH_PREFLIGHT" \
    --skip-entry-bundle-sim-preflight "$SKIP_ENTRY_BUNDLE_SIM_PREFLIGHT" \
    --skip-late-follow-sim-preflight "$SKIP_LATE_FOLLOW_SIM_PREFLIGHT" \
    --daily-ledger-path "$SHADOW_R1_DAILY_LEDGER_PATH" \
    --daily-profit-target-usd "$SHADOW_R1_DAILY_PROFIT_TARGET_USD" \
    --daily-max-loss-usd "$SHADOW_R1_DAILY_MAX_LOSS_USD" \
    --daily-max-trades "$SHADOW_R1_DAILY_MAX_TRADES" \
    --daily-sol-usd "$SHADOW_R1_DAILY_SOL_USD" \
    --launch-exit-enabled "$LAUNCH_EXIT_ENABLED" \
    --launch-exit-hard-stop-bps "$LAUNCH_EXIT_HARD_STOP_BPS" \
    --launch-exit-trailing-enabled "$LAUNCH_EXIT_TRAILING_ENABLED" \
    --launch-exit-trailing-arm-gain-bps "$LAUNCH_EXIT_TRAILING_ARM_GAIN_BPS" \
    --launch-exit-trailing-stop-bps "$LAUNCH_EXIT_TRAILING_STOP_BPS" \
    --launch-exit-velocity-enabled "$LAUNCH_EXIT_VELOCITY_ENABLED" \
    --launch-exit-drop-1s-bps "$LAUNCH_EXIT_DROP_1S_BPS" \
    --launch-exit-drop-2s-bps "$LAUNCH_EXIT_DROP_2S_BPS" \
    --launch-exit-drop-5s-bps "$LAUNCH_EXIT_DROP_5S_BPS" \
    --launch-exit-drop-10s-bps "$LAUNCH_EXIT_DROP_10S_BPS" \
    --launch-exit-liquidity-enabled "$LAUNCH_EXIT_LIQUIDITY_ENABLED" \
    --launch-exit-max-price-impact-bps "$LAUNCH_EXIT_MAX_PRICE_IMPACT_BPS" \
    --launch-exit-max-slippage-bps "$LAUNCH_EXIT_MAX_SLIPPAGE_BPS" \
    --launch-exit-pool-liquidity-drop-bps "$LAUNCH_EXIT_POOL_LIQUIDITY_DROP_BPS" \
    --launch-exit-take-profit-bps "$LAUNCH_EXIT_TAKE_PROFIT_BPS" \
    --launch-exit-rescue-on-sim-fail "$LAUNCH_EXIT_RESCUE_ON_SIM_FAIL" \
    --launch-exit-rescue-on-send-fail "$LAUNCH_EXIT_RESCUE_ON_SEND_FAIL" \
    --launch-exit-max-exit-attempts "$LAUNCH_EXIT_MAX_EXIT_ATTEMPTS" \
    --launch-exit-manual-kill-file "$LAUNCH_EXIT_MANUAL_KILL_FILE"

pass "Launch5 live execute written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

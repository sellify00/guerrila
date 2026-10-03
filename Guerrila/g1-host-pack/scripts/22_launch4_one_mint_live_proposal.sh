#!/usr/bin/env bash
# 22_launch4_one_mint_live_proposal.sh — build a launch-follow one-mint live proposal package without send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch4 — One-Mint Live Proposal"

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
STEP="${SHADOW_LAUNCH4_STEP:-22_launch4_one_mint_live_proposal}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch4_one_mint_live_proposal_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch4_one_mint_live_proposal_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PROPOSAL_JSON="$RESULT_DIR/launch4_one_mint_live_proposal.json"
PROPOSAL_SUMMARY="$RESULT_DIR/proposal_summary.txt"
GATE_ROWS_PATH="${SHADOW_LAUNCH4_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCH4_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
BUNDLE_ROWS_PATH="${SHADOW_LAUNCH4_BUNDLE_ROWS_PATH:-$RESULTS_ROOT/22_launch4_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl}"
RISK_TIER="${SHADOW_LAUNCH4_RISK_TIER:-R1_LAUNCH_CONTINUATION_TINY}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"
[[ -f "$BUNDLE_ROWS_PATH" ]] || BUNDLE_ROWS_PATH="$(latest_rows_for_step launch4_entry_bundle_nosubmit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$PROPOSAL_JSON" "$PROPOSAL_SUMMARY"

python3 "$PACK_DIR/scripts/launch_follow_lane.py" launch4 \
    --gate-rows "$GATE_ROWS_PATH" \
    --sim-rows "$SIM_ROWS_PATH" \
    --bundle-rows "$BUNDLE_ROWS_PATH" \
    --rows-out "$ROWS_OUT" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT" \
    --proposal-json "$PROPOSAL_JSON" \
    --proposal-summary "$PROPOSAL_SUMMARY" \
    --risk-tier "$RISK_TIER" \
    --require-entry-bundle-proof

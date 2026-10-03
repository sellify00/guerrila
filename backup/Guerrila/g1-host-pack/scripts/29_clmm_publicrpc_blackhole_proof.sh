#!/usr/bin/env bash
# 29_clmm_publicrpc_blackhole_proof.sh - corrected R1-PUBLIC-A blackhole proof
#
# Runs a tiny-live-shaped public RPC scout proof against a blackholed public
# RPC endpoint so the engine can progress through public-scout routing, build,
# sign, and PM lifecycle logic without any real chain send.

source "$(dirname "$0")/lib.sh"
script_header "29 - CLMM Public-RPC Blackhole Proof"

assert_root
require_env SHADOW_NIC_IFACE

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT_PATH="$PACK_DIR/scripts/$(basename "$0")"
ARTIFACT_DIR="$PACK_DIR/artifacts/release-guerrila"
TIER0_BIN="${SHADOW_RUN29C_TIER0_BIN:-$ARTIFACT_DIR/shadow-tier0}"
ORACLE_BIN="${SHADOW_RUN29C_ORACLE_BIN:-$ARTIFACT_DIR/shadow-oracle}"
TEMPLATE_PROOF_BIN="${SHADOW_RUN29C_TEMPLATE_PROOF_BIN:-$ARTIFACT_DIR/shadowguard-template-proof}"
CONFIG_TEMPLATE="${SHADOW_RUN29C_CONFIG_TEMPLATE:-$PACK_DIR/config.r1-public-a.yaml}"
STEP="${SHADOW_RUN29C_STEP:-29_clmm_publicrpc_blackhole_proof}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
CONFIG_TMP="${SHADOW_RUN29C_CONFIG_TMP:-$RESULT_DIR/config.guerrila.yaml}"
WATCHER_SUMMARY_FILE="${SHADOW_RUN29C_WATCHER_SUMMARY_FILE:-$RESULT_DIR/watcher_summary.txt}"
RUN_DIR="$PACK_DIR/run"
GATE_INPUT="$RUN_DIR/g12_gate_input.json"
GATE_REPORT="$RUN_DIR/g12_gate_report.json"
RESULT_GATE_INPUT="$RESULT_DIR/g12_gate_input.json"
RESULT_GATE_REPORT="$RESULT_DIR/g12_gate_report.json"
SHOCK_RESEARCH_PATH="${SHADOW_RUN29C_SHOCK_RESEARCH_PATH:-$RESULT_DIR/shock_research.jsonl}"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
ORACLE_LOG="$RESULT_DIR/shadow-oracle.log"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
CONFIG_CHECK_FILE="$RESULT_DIR/config_check.txt"
TIER0_TAIL_FILE="$RESULT_DIR/tier0_tail.txt"
WINDOW_SECONDS_RAW="${1:-${SHADOW_RUN29C_WINDOW_SECONDS:-1800}}"
WINDOW_SECONDS="$(printf '%s\n' "$WINDOW_SECONDS_RAW" | head -n 1 | tr -d '\r')"
if [[ ! "$WINDOW_SECONDS" =~ ^[0-9]+$ ]]; then
    fail "WINDOW_SECONDS must be an integer (got '$WINDOW_SECONDS_RAW')"
    exit 1
fi
WINDOW_SECONDS_INT=$((10#$WINDOW_SECONDS))
READINESS_TIMEOUT="${SHADOW_RUN29C_READINESS_TIMEOUT_SECONDS:-60}"
POLL_SECONDS="${SHADOW_RUN29C_POLL_SECONDS:-10}"
RUN_MODE="${SHADOW_RUN29C_MODE:-strict-proof}"
WATCH_LOG_PATH="${SHADOW_RUN29C_WATCH_LOG_PATH:-}"
WATCH_TIMEOUT_SECONDS="${SHADOW_RUN29C_WATCH_TIMEOUT_SECONDS:-0}"
WATCH_TRIGGER_ON="${SHADOW_RUN29C_WATCH_TRIGGER_ON:-quote_matched}"
WATCH_AUTO_ARM_STRICT_PROOF="${SHADOW_RUN29C_WATCH_AUTO_ARM_STRICT_PROOF:-true}"
ARMED_WINDOW_SECONDS="${SHADOW_RUN29C_ARMED_WINDOW_SECONDS:-900}"
AUTO_ARM_STEP_SUFFIX="${SHADOW_RUN29C_AUTO_ARM_STEP_SUFFIX:-_armed}"
TRIGGERED_FROM_WATCHER="${SHADOW_RUN29C_TRIGGERED_FROM_WATCHER:-false}"
BLACKHOLE_PUBLIC_RPC_ENDPOINT="${SHADOW_RUN29C_BLACKHOLE_PUBLIC_RPC_ENDPOINT:-https://127.0.0.1:1}"
MAINNET_URL="${SHADOW_RUN29C_MAINNET_URL:-${SHADOW_SOLANA_RPC_URL:-$(default_http_rpc_url)}}"
PUBLIC_RPC_ENDPOINT="${SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT:-$BLACKHOLE_PUBLIC_RPC_ENDPOINT}"
MAX_LIVE_TRADE_INPUT_SOL="${SHADOW_RUN29C_MAX_LIVE_TRADE_INPUT_SOL:-0.05}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN29C_MAX_POSITION_SOL_PER_MINT:-0.1}"
MAX_NOTIONAL_SOL_PER_ENTITY="${SHADOW_RUN29C_MAX_NOTIONAL_SOL_PER_ENTITY:-0.1}"
ALLOW_ONLY_VENUE_ID="${SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID:-${SHADOW_RUN42_ALLOW_ONLY_VENUE_ID:-}}"
ALLOW_ONLY_POOL="${SHADOW_RUN29C_ALLOW_ONLY_POOL:-${SHADOW_RUN42_ALLOW_ONLY_POOL:-}}"
ALLOW_ONLY_MINT_IN="${SHADOW_RUN29C_ALLOW_ONLY_MINT_IN:-${SHADOW_RUN42_ALLOW_ONLY_MINT_IN:-}}"
ALLOW_ONLY_MINT_OUT="${SHADOW_RUN29C_ALLOW_ONLY_MINT_OUT:-${SHADOW_RUN42_ALLOW_ONLY_MINT_OUT:-}}"
SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
DEFAULT_APPROVED_HELD_INPUT_MINTS="So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"
APPROVED_HELD_INPUT_MINTS="${SHADOW_RUN29C_APPROVED_HELD_INPUT_MINTS:-${SHADOW_RUN42_APPROVED_HELD_INPUT_MINTS:-$DEFAULT_APPROVED_HELD_INPUT_MINTS}}"
HELD_INPUT_MINTS_ENV_SET="false"
if [[ -n "${SHADOW_RUN29C_HELD_INPUT_MINTS+x}" ]] || [[ -n "${SHADOW_RUN42_HELD_INPUT_MINTS+x}" ]]; then
    HELD_INPUT_MINTS_ENV_SET="true"
fi
HELD_INPUT_MINTS="${SHADOW_RUN29C_HELD_INPUT_MINTS:-${SHADOW_RUN42_HELD_INPUT_MINTS:-}}"
EXPECT_BLACKHOLE_ENDPOINT="${SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT:-true}"
ONE_SHOT_MODE="${SHADOW_RUN29C_ONE_SHOT_MODE:-false}"
RESULT_GRACE_SECONDS="${SHADOW_RUN29C_RESULT_GRACE_SECONDS:-45}"
FINAL_SETTLE_DELAY_SECONDS="${SHADOW_RUN29C_FINAL_SETTLE_DELAY_SECONDS:-3}"
VALIDATE_PUBLIC_SCOUT_PROFILE="${SHADOW_RUN29C_VALIDATE_PUBLIC_SCOUT_PROFILE:-false}"
MUTATE_ROLLOUT_CAPS="${SHADOW_RUN29C_MUTATE_ROLLOUT_CAPS:-true}"
REQUIRE_RUNTIME_ENDPOINT_SEEN="${SHADOW_RUN29C_REQUIRE_RUNTIME_ENDPOINT_SEEN:-true}"
XDP_DIR="$PACK_DIR/xdp"
XDP_OBJ="$XDP_DIR/shadow_rx_meta.bpf.o"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-xdpgeneric}"
XDP_QUEUE_COUNT=1
SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-true}"
ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-false}"
REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"
PRIVATE_PACKET_BACKEND="${SHADOW_RUN29C_PRIVATE_PACKET_BACKEND:-afxdp}"
ALLOW_MANAGEMENT_NIC_DPDK="${SHADOW_ALLOW_MANAGEMENT_NIC_DPDK:-false}"
DPDK_PORT_ID="${SHADOW_RUN29C_DPDK_PORT_ID:-0}"
DPDK_PCI_ADDR=""
DPDK_CURRENT_DRIVER=""
DPDK_IPV4_CIDR=""
DPDK_DEFAULT_ROUTE=""
DPDK_BOUND_TO_VFIO="false"
TIER0_CPU_AFFINITY="${SHADOW_RUN29C_TIER0_CPU_AFFINITY:-}"
ORACLE_CPU_AFFINITY="${SHADOW_RUN29C_ORACLE_CPU_AFFINITY:-}"
SHADOW_CONTROL_BIN="${SHADOW_RUN29C_SHADOW_CONTROL_BIN:-$ARTIFACT_DIR/shadow-control}"
FINAL_GATE_SNAPSHOT_RETRIES="${SHADOW_RUN29C_FINAL_GATE_SNAPSHOT_RETRIES:-10}"
FINAL_GATE_SNAPSHOT_SLEEP_SECONDS="${SHADOW_RUN29C_FINAL_GATE_SNAPSHOT_SLEEP_SECONDS:-1}"
FINAL_DIAGNOSTICS_CAPTURED="false"
WATCHER_ARM_TIMEOUT_SECONDS="${SHADOW_RUN29C_WATCHER_ARM_TIMEOUT_SECONDS:-600}"
WATCHER_ARM_POLL_SECONDS="${SHADOW_RUN29C_WATCHER_ARM_POLL_SECONDS:-1}"
WATCHER_ARM_REASON=""
WATCHER_ARM_WAIT_SECONDS=0
WATCHER_ARM_BASELINE_CAPTURED="false"

REQUIRE_TARGET_PUBLIC_SCOUT="false"
if [[ -n "${SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT:-}" ]]; then
    REQUIRE_TARGET_PUBLIC_SCOUT="$SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT"
fi
if [[ -z "${SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT:-}" ]] && [[ -n "$ALLOW_ONLY_POOL" ]] && [[ -n "$ALLOW_ONLY_MINT_IN" ]] && [[ -n "$ALLOW_ONLY_MINT_OUT" ]]; then
    REQUIRE_TARGET_PUBLIC_SCOUT="true"
fi

derive_run38_held_input_mints() {
    local inventory_path="${1:-}"
    local approved_mints="${2:-}"
    python3 - "$inventory_path" "$approved_mints" <<'PY'
import json
import sys

inventory_path = sys.argv[1]
approved = [item.strip() for item in sys.argv[2].split(",") if item.strip()]
if not approved:
    print("")
    raise SystemExit(0)

try:
    with open(inventory_path, "r", encoding="utf-8") as fh:
        payload = json.load(fh)
except Exception:
    print("")
    raise SystemExit(0)

held = set()
for entry in payload.get("accounts", []):
    mint_pubkey = str(entry.get("mint_pubkey", "")).strip()
    if mint_pubkey not in approved:
        continue
    if not bool(entry.get("initialized", False)):
        continue
    try:
        amount_raw = int(entry.get("amount_raw", 0))
    except Exception:
        amount_raw = 0
    if amount_raw > 0:
        held.add(mint_pubkey)

print(",".join(mint for mint in approved if mint in held))
PY
}

if [[ "$HELD_INPUT_MINTS_ENV_SET" != "true" ]]; then
    HELD_INPUT_MINTS="$(derive_run38_held_input_mints "$SIGNER_TOKEN_INVENTORY_PATH" "$APPROVED_HELD_INPUT_MINTS")"
fi

mkdir -p "$RUN_DIR" "$RESULT_DIR" "$(dirname "$SHOCK_RESEARCH_PATH")"
rm -f \
    "$RESULT_GATE_INPUT" \
    "$RESULT_GATE_REPORT" \
    "$LOCAL_AGAVE_PREFLIGHT_FILE" \
    "$SUMMARY_FILE"

if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi

write_best_effort_summary_if_missing() {
    [[ -f "$SUMMARY_FILE" ]] && return 0
    capture_final_live_diagnostics
    [[ -f "$RESULT_GATE_INPUT" || -f "$RESULT_GATE_REPORT" || -f "$GATE_INPUT" || -f "$GATE_REPORT" ]] || return 0

    local worker_records_seen worker_pool_state_hit worker_route_actionable
    local entered_process_intent worker_emit_attempt worker_emit_ok pm_created enqueued
    local transport_attempt transport_failure transport_rejected nack_recorded pm_released
    local live_landed route_public_rpc_scout
    local public_scout_attempt_latch_set public_scout_attempt_latch_drop_count
    local public_scout_attempt_latch_first_signature fee_leak_accounted inventory_truth_ok
    local phantom_inventory_risk kill_switch_required public_scout_gates_green
    local r1_auto_admission_attempt r1_auto_admission_admit
    local r1_auto_admission_reject_proof_filter r1_auto_admission_reject_template_executability r1_auto_admission_reject_attempt_latch
    local r1_auto_admission_reject_proof_filter_no_quote r1_auto_admission_reject_proof_filter_no_positive_ev
    local r1_auto_admission_reject_proof_filter_no_shadow_guard r1_auto_admission_reject_proof_filter_public_scout_shape
    local r1_auto_admission_reject_proof_filter_other
    local r1_auto_admission_reject_public_scout_shape_route_not_public_scout
    local r1_auto_admission_reject_public_scout_shape_wrong_source
    local r1_auto_admission_reject_public_scout_shape_wrong_venue
    local r1_auto_admission_reject_public_scout_shape_missing_template_family
    local r1_auto_admission_reject_public_scout_shape_route_template_mismatch
    local r1_auto_admission_reject_public_scout_shape_other
    local r1_auto_admission_reject_micro_market r1_auto_admission_reject_micro_market_no_uplift
    local r1_auto_admission_reject_micro_market_negative_ev r1_auto_admission_reject_micro_market_below_floor
    local r1_auto_admission_reject_micro_market_other
    local last_public_scout_shape_reject_reason last_public_scout_shape_reject_pool
    local last_public_scout_shape_reject_mint_in last_public_scout_shape_reject_mint_out
    local last_public_scout_shape_reject_route last_public_scout_shape_reject_source
    local last_public_scout_shape_reject_venue
    local admit_to_process_intent_attempt admit_to_process_intent_ok
    local admit_to_process_intent_drop last_admit_to_process_intent_drop_reason
    local last_admit_to_process_intent_pool last_admit_to_process_intent_mint_in
    local last_admit_to_process_intent_mint_out last_admit_to_process_intent_route
    local last_admit_to_process_intent_source

    worker_records_seen="$(gate_metric "worker_records_seen")"
    worker_pool_state_hit="$(gate_metric "worker_pool_state_hit")"
    worker_route_actionable="$(gate_metric "worker_route_actionable")"
    entered_process_intent="$(gate_metric "worker_candidates_entered_process_intent")"
    route_public_rpc_scout="$(gate_metric "worker_route_resolved_public_rpc_scout")"
    worker_emit_attempt="$(gate_metric "worker_public_rpc_scout_emit_attempt")"
    worker_emit_ok="$(gate_metric "worker_public_rpc_scout_emit_ok")"
    pm_created="$(gate_metric "worker_pm_reservation_created")"
    enqueued="$(gate_metric "public_rpc_scout_enqueued")"
    transport_attempt="$(gate_metric "public_rpc_transport_attempt")"
    transport_failure="$(gate_metric "public_rpc_transport_failure")"
    transport_rejected="$(gate_metric "public_rpc_transport_rejected")"
    nack_recorded="$(gate_metric "public_rpc_nack_recorded")"
    pm_released="$(gate_metric "pm_reservation_released")"
    live_landed="$(gate_metric "live_landed")"
    public_scout_attempt_latch_set="$(gate_metric "public_scout_attempt_latch_set")"
    public_scout_attempt_latch_drop_count="$(gate_metric "public_scout_attempt_latch_drop_count")"
    public_scout_attempt_latch_first_signature="$(gate_metric "public_scout_attempt_latch_first_signature")"
    fee_leak_accounted="$(gate_metric "fee_leak_accounted")"
    inventory_truth_ok="$(gate_metric "inventory_truth_ok")"
    phantom_inventory_risk="$(gate_metric "phantom_inventory_risk")"
    kill_switch_required="$(gate_metric "kill_switch_required")"
    public_scout_gates_green="$(gate_metric "public_scout_gates_green")"
    r1_auto_admission_attempt="$(gate_metric "r1_auto_admission_attempt")"
    r1_auto_admission_admit="$(gate_metric "r1_auto_admission_admit")"
    r1_auto_admission_reject_micro_market="$(gate_metric "r1_auto_admission_reject_micro_market")"
    r1_auto_admission_reject_proof_filter="$(gate_metric "r1_auto_admission_reject_proof_filter")"
    r1_auto_admission_reject_template_executability="$(gate_metric "r1_auto_admission_reject_template_executability")"
    r1_auto_admission_reject_attempt_latch="$(gate_metric "r1_auto_admission_reject_attempt_latch")"
    r1_auto_admission_reject_proof_filter_no_quote="$(gate_metric "r1_auto_admission_reject_proof_filter_no_quote")"
    r1_auto_admission_reject_proof_filter_no_positive_ev="$(gate_metric "r1_auto_admission_reject_proof_filter_no_positive_ev")"
    r1_auto_admission_reject_proof_filter_no_shadow_guard="$(gate_metric "r1_auto_admission_reject_proof_filter_no_shadow_guard")"
    r1_auto_admission_reject_proof_filter_public_scout_shape="$(gate_metric "r1_auto_admission_reject_proof_filter_public_scout_shape")"
    r1_auto_admission_reject_proof_filter_other="$(gate_metric "r1_auto_admission_reject_proof_filter_other")"
    r1_auto_admission_reject_public_scout_shape_route_not_public_scout="$(gate_metric "r1_auto_admission_reject_public_scout_shape_route_not_public_scout")"
    r1_auto_admission_reject_public_scout_shape_wrong_source="$(gate_metric "r1_auto_admission_reject_public_scout_shape_wrong_source")"
    r1_auto_admission_reject_public_scout_shape_wrong_venue="$(gate_metric "r1_auto_admission_reject_public_scout_shape_wrong_venue")"
    r1_auto_admission_reject_public_scout_shape_missing_template_family="$(gate_metric "r1_auto_admission_reject_public_scout_shape_missing_template_family")"
    r1_auto_admission_reject_public_scout_shape_route_template_mismatch="$(gate_metric "r1_auto_admission_reject_public_scout_shape_route_template_mismatch")"
    r1_auto_admission_reject_public_scout_shape_other="$(gate_metric "r1_auto_admission_reject_public_scout_shape_other")"
    run38_target_public_scout_clmm_route_resolved="$(gate_metric "run38_target_public_scout_clmm_route_resolved")"
    run38_target_public_scout_clmm_r1_attempt="$(gate_metric "run38_target_public_scout_clmm_r1_attempt")"
    run38_target_public_scout_clmm_reject_proof_filter="$(gate_metric "run38_target_public_scout_clmm_reject_proof_filter")"
    run38_target_public_scout_clmm_reject_micro_market="$(gate_metric "run38_target_public_scout_clmm_reject_micro_market")"
    run38_target_public_scout_clmm_reject_inventory="$(gate_metric "run38_target_public_scout_clmm_reject_inventory")"
    run38_target_public_scout_clmm_reject_inventory_missing_input_ata="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_missing_input_ata")"
    run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized")"
    run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient")"
    run38_target_public_scout_clmm_reject_inventory_token_program_mismatch="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch")"
    run38_target_public_scout_clmm_reject_inventory_other="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_other")"
    run38_target_public_scout_clmm_reject_template_executability="$(gate_metric "run38_target_public_scout_clmm_reject_template_executability")"
    run38_target_public_scout_clmm_reject_attempt_latch="$(gate_metric "run38_target_public_scout_clmm_reject_attempt_latch")"
    run38_target_public_scout_clmm_admit="$(gate_metric "run38_target_public_scout_clmm_admit")"
    run38_target_public_scout_clmm_process_intent="$(gate_metric "run38_target_public_scout_clmm_process_intent")"
    run38_target_public_scout_clmm_emit_attempt="$(gate_metric "run38_target_public_scout_clmm_emit_attempt")"
    run38_target_public_scout_clmm_transport_attempt="$(gate_metric "run38_target_public_scout_clmm_transport_attempt")"
    run38_target_public_scout_clmm_pm_created="$(gate_metric "run38_target_public_scout_clmm_pm_created")"
    r1_auto_admission_reject_micro_market_no_uplift="$(gate_metric "r1_auto_admission_reject_micro_market_no_uplift")"
    r1_auto_admission_reject_micro_market_negative_ev="$(gate_metric "r1_auto_admission_reject_micro_market_negative_ev")"
    r1_auto_admission_reject_micro_market_below_floor="$(gate_metric "r1_auto_admission_reject_micro_market_below_floor")"
    r1_auto_admission_reject_micro_market_other="$(gate_metric "r1_auto_admission_reject_micro_market_other")"
    last_public_scout_shape_reject_reason="$(gate_metric "last_public_scout_shape_reject_reason")"
    last_public_scout_shape_reject_pool="$(gate_metric "last_public_scout_shape_reject_pool")"
    last_public_scout_shape_reject_mint_in="$(gate_metric "last_public_scout_shape_reject_mint_in")"
    last_public_scout_shape_reject_mint_out="$(gate_metric "last_public_scout_shape_reject_mint_out")"
    last_public_scout_shape_reject_route="$(gate_metric "last_public_scout_shape_reject_route")"
    last_public_scout_shape_reject_source="$(gate_metric "last_public_scout_shape_reject_source")"
    last_public_scout_shape_reject_venue="$(gate_metric "last_public_scout_shape_reject_venue")"
    last_run38_target_pool="$(gate_metric "last_run38_target_pool")"
    last_run38_target_mint_in="$(gate_metric "last_run38_target_mint_in")"
    last_run38_target_mint_out="$(gate_metric "last_run38_target_mint_out")"
    last_run38_target_route="$(gate_metric "last_run38_target_route")"
    last_run38_target_source="$(gate_metric "last_run38_target_source")"
    last_run38_target_venue="$(gate_metric "last_run38_target_venue")"
    last_run38_target_stage="$(gate_metric "last_run38_target_stage")"
    last_run38_target_reject_reason="$(gate_metric "last_run38_target_reject_reason")"
    last_run38_target_inventory_reject_reason="$(gate_metric "last_run38_target_inventory_reject_reason")"
    last_run38_target_inventory_pool="$(gate_metric "last_run38_target_inventory_pool")"
    last_run38_target_inventory_mint_in="$(gate_metric "last_run38_target_inventory_mint_in")"
    last_run38_target_inventory_mint_out="$(gate_metric "last_run38_target_inventory_mint_out")"
    last_run38_target_inventory_input_ata="$(gate_metric "last_run38_target_inventory_input_ata")"
    last_run38_target_inventory_input_balance="$(gate_metric "last_run38_target_inventory_input_balance")"
    last_run38_target_inventory_required_input="$(gate_metric "last_run38_target_inventory_required_input")"
    last_run38_target_inventory_token_program="$(gate_metric "last_run38_target_inventory_token_program")"
    admit_to_process_intent_attempt="$(gate_metric "r1_auto_admission_admit_to_process_intent_attempt")"
    admit_to_process_intent_ok="$(gate_metric "r1_auto_admission_admit_to_process_intent_ok")"
    admit_to_process_intent_drop="$(gate_metric "r1_auto_admission_admit_to_process_intent_drop")"
    last_admit_to_process_intent_drop_reason="$(gate_metric "last_admit_to_process_intent_drop_reason")"
    last_admit_to_process_intent_pool="$(gate_metric "last_admit_to_process_intent_pool")"
    last_admit_to_process_intent_mint_in="$(gate_metric "last_admit_to_process_intent_mint_in")"
    last_admit_to_process_intent_mint_out="$(gate_metric "last_admit_to_process_intent_mint_out")"
    last_admit_to_process_intent_route="$(gate_metric "last_admit_to_process_intent_route")"
    last_admit_to_process_intent_source="$(gate_metric "last_admit_to_process_intent_source")"

    {
        echo "mode=$RUN_MODE"
        echo "window_seconds=$WINDOW_SECONDS"
        echo "generated_config=$CONFIG_TMP"
        echo "private_packet_backend=$PRIVATE_PACKET_BACKEND"
        echo "public_rpc_endpoint=$PUBLIC_RPC_ENDPOINT"
        echo "approved_held_input_mints=$APPROVED_HELD_INPUT_MINTS"
        echo "held_input_mints=$HELD_INPUT_MINTS"
        echo "held_input_filter_active=true"
        echo "one_shot_mode=$ONE_SHOT_MODE"
        echo "raw_outcome=${RUN37_OUTCOME:-none}"
        echo "outcome=${RUN37_OUTCOME:-none}"
        echo "worker_records_seen=$worker_records_seen"
        echo "worker_pool_state_hit=$worker_pool_state_hit"
        echo "worker_route_actionable=$worker_route_actionable"
        echo "worker_candidates_entered_process_intent=$entered_process_intent"
        echo "worker_route_resolved_public_rpc_scout=$route_public_rpc_scout"
        echo "run38_target_public_scout_clmm_route_resolved=$run38_target_public_scout_clmm_route_resolved"
        echo "run38_target_public_scout_clmm_r1_attempt=$run38_target_public_scout_clmm_r1_attempt"
        echo "run38_target_public_scout_clmm_reject_proof_filter=$run38_target_public_scout_clmm_reject_proof_filter"
        echo "run38_target_public_scout_clmm_reject_micro_market=$run38_target_public_scout_clmm_reject_micro_market"
        echo "run38_target_public_scout_clmm_reject_inventory=$run38_target_public_scout_clmm_reject_inventory"
        echo "run38_target_public_scout_clmm_reject_inventory_missing_input_ata=$run38_target_public_scout_clmm_reject_inventory_missing_input_ata"
        echo "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized=$run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized"
        echo "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient=$run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient"
        echo "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch=$run38_target_public_scout_clmm_reject_inventory_token_program_mismatch"
        echo "run38_target_public_scout_clmm_reject_inventory_other=$run38_target_public_scout_clmm_reject_inventory_other"
        echo "run38_target_public_scout_clmm_reject_template_executability=$run38_target_public_scout_clmm_reject_template_executability"
        echo "run38_target_public_scout_clmm_reject_attempt_latch=$run38_target_public_scout_clmm_reject_attempt_latch"
        echo "run38_target_public_scout_clmm_admit=$run38_target_public_scout_clmm_admit"
        echo "run38_target_public_scout_clmm_process_intent=$run38_target_public_scout_clmm_process_intent"
        echo "run38_target_public_scout_clmm_emit_attempt=$run38_target_public_scout_clmm_emit_attempt"
        echo "run38_target_public_scout_clmm_transport_attempt=$run38_target_public_scout_clmm_transport_attempt"
        echo "run38_target_public_scout_clmm_pm_created=$run38_target_public_scout_clmm_pm_created"
        echo "worker_public_rpc_scout_emit_attempt=$worker_emit_attempt"
        echo "worker_public_rpc_scout_emit_ok=$worker_emit_ok"
        echo "worker_pm_reservation_created=$pm_created"
        echo "public_rpc_scout_enqueued=$enqueued"
        echo "public_rpc_transport_attempt=$transport_attempt"
        echo "public_rpc_transport_failure=$transport_failure"
        echo "public_rpc_transport_rejected=$transport_rejected"
        echo "public_rpc_nack_recorded=$nack_recorded"
        echo "pm_reservation_released=$pm_released"
        echo "live_landed=$live_landed"
        echo "public_scout_attempt_latch_set=$public_scout_attempt_latch_set"
        echo "public_scout_attempt_latch_drop_count=$public_scout_attempt_latch_drop_count"
        echo "public_scout_attempt_latch_first_signature=$public_scout_attempt_latch_first_signature"
        echo "fee_leak_accounted=$fee_leak_accounted"
        echo "inventory_truth_ok=$inventory_truth_ok"
        echo "phantom_inventory_risk=$phantom_inventory_risk"
        echo "kill_switch_required=$kill_switch_required"
        echo "public_scout_gates_green=$public_scout_gates_green"
        echo "r1_auto_admission_attempt=$r1_auto_admission_attempt"
        echo "r1_auto_admission_admit=$r1_auto_admission_admit"
        echo "r1_auto_admission_reject_micro_market=$r1_auto_admission_reject_micro_market"
        echo "r1_auto_admission_reject_proof_filter=$r1_auto_admission_reject_proof_filter"
        echo "r1_auto_admission_reject_template_executability=$r1_auto_admission_reject_template_executability"
        echo "r1_auto_admission_reject_attempt_latch=$r1_auto_admission_reject_attempt_latch"
        echo "r1_auto_admission_reject_proof_filter_no_quote=$r1_auto_admission_reject_proof_filter_no_quote"
        echo "r1_auto_admission_reject_proof_filter_no_positive_ev=$r1_auto_admission_reject_proof_filter_no_positive_ev"
        echo "r1_auto_admission_reject_proof_filter_no_shadow_guard=$r1_auto_admission_reject_proof_filter_no_shadow_guard"
        echo "r1_auto_admission_reject_proof_filter_public_scout_shape=$r1_auto_admission_reject_proof_filter_public_scout_shape"
        echo "r1_auto_admission_reject_proof_filter_other=$r1_auto_admission_reject_proof_filter_other"
        echo "r1_auto_admission_reject_public_scout_shape_route_not_public_scout=$r1_auto_admission_reject_public_scout_shape_route_not_public_scout"
        echo "r1_auto_admission_reject_public_scout_shape_wrong_source=$r1_auto_admission_reject_public_scout_shape_wrong_source"
        echo "r1_auto_admission_reject_public_scout_shape_wrong_venue=$r1_auto_admission_reject_public_scout_shape_wrong_venue"
        echo "r1_auto_admission_reject_public_scout_shape_missing_template_family=$r1_auto_admission_reject_public_scout_shape_missing_template_family"
        echo "r1_auto_admission_reject_public_scout_shape_route_template_mismatch=$r1_auto_admission_reject_public_scout_shape_route_template_mismatch"
        echo "r1_auto_admission_reject_public_scout_shape_other=$r1_auto_admission_reject_public_scout_shape_other"
        echo "r1_auto_admission_reject_micro_market_no_uplift=$r1_auto_admission_reject_micro_market_no_uplift"
        echo "r1_auto_admission_reject_micro_market_negative_ev=$r1_auto_admission_reject_micro_market_negative_ev"
        echo "r1_auto_admission_reject_micro_market_below_floor=$r1_auto_admission_reject_micro_market_below_floor"
        echo "r1_auto_admission_reject_micro_market_other=$r1_auto_admission_reject_micro_market_other"
        echo "last_public_scout_shape_reject_reason=$last_public_scout_shape_reject_reason"
        echo "last_public_scout_shape_reject_pool=$last_public_scout_shape_reject_pool"
        echo "last_public_scout_shape_reject_mint_in=$last_public_scout_shape_reject_mint_in"
        echo "last_public_scout_shape_reject_mint_out=$last_public_scout_shape_reject_mint_out"
        echo "last_public_scout_shape_reject_route=$last_public_scout_shape_reject_route"
        echo "last_public_scout_shape_reject_source=$last_public_scout_shape_reject_source"
        echo "last_public_scout_shape_reject_venue=$last_public_scout_shape_reject_venue"
        echo "last_run38_target_pool=$last_run38_target_pool"
        echo "last_run38_target_mint_in=$last_run38_target_mint_in"
        echo "last_run38_target_mint_out=$last_run38_target_mint_out"
        echo "last_run38_target_route=$last_run38_target_route"
        echo "last_run38_target_source=$last_run38_target_source"
        echo "last_run38_target_venue=$last_run38_target_venue"
        echo "last_run38_target_stage=$last_run38_target_stage"
        echo "last_run38_target_reject_reason=$last_run38_target_reject_reason"
        echo "last_run38_target_inventory_reject_reason=$last_run38_target_inventory_reject_reason"
        echo "last_run38_target_inventory_pool=$last_run38_target_inventory_pool"
        echo "last_run38_target_inventory_mint_in=$last_run38_target_inventory_mint_in"
        echo "last_run38_target_inventory_mint_out=$last_run38_target_inventory_mint_out"
        echo "last_run38_target_inventory_input_ata=$last_run38_target_inventory_input_ata"
        echo "last_run38_target_inventory_input_balance=$last_run38_target_inventory_input_balance"
        echo "last_run38_target_inventory_required_input=$last_run38_target_inventory_required_input"
        echo "last_run38_target_inventory_token_program=$last_run38_target_inventory_token_program"
        echo "r1_auto_admission_admit_to_process_intent_attempt=$admit_to_process_intent_attempt"
        echo "r1_auto_admission_admit_to_process_intent_ok=$admit_to_process_intent_ok"
        echo "r1_auto_admission_admit_to_process_intent_drop=$admit_to_process_intent_drop"
        echo "last_admit_to_process_intent_drop_reason=$last_admit_to_process_intent_drop_reason"
        echo "last_admit_to_process_intent_pool=$last_admit_to_process_intent_pool"
        echo "last_admit_to_process_intent_mint_in=$last_admit_to_process_intent_mint_in"
        echo "last_admit_to_process_intent_mint_out=$last_admit_to_process_intent_mint_out"
        echo "last_admit_to_process_intent_route=$last_admit_to_process_intent_route"
        echo "last_admit_to_process_intent_source=$last_admit_to_process_intent_source"
    } >"$SUMMARY_FILE"
}

cleanup() {
    capture_final_live_diagnostics
    if [[ -n "${TIER0_PID:-}" ]] && kill -0 "$TIER0_PID" 2>/dev/null; then
        kill "$TIER0_PID" 2>/dev/null || true
        wait "$TIER0_PID" 2>/dev/null || true
    fi
    if [[ -n "${ORACLE_PID:-}" ]] && kill -0 "$ORACLE_PID" 2>/dev/null; then
        kill "$ORACLE_PID" 2>/dev/null || true
        wait "$ORACLE_PID" 2>/dev/null || true
    fi
    if is_true "$DPDK_BOUND_TO_VFIO"; then
        command -v dpdk-devbind.py >/dev/null 2>&1 && [[ -n "$DPDK_PCI_ADDR" ]] && [[ -n "$DPDK_CURRENT_DRIVER" ]] && \
            dpdk-devbind.py --bind="$DPDK_CURRENT_DRIVER" "$DPDK_PCI_ADDR" >/dev/null 2>&1 || true
        ip link set dev "$SHADOW_NIC_IFACE" up >/dev/null 2>&1 || true
        if [[ -n "$DPDK_IPV4_CIDR" ]]; then
            ip addr replace "$DPDK_IPV4_CIDR" dev "$SHADOW_NIC_IFACE" >/dev/null 2>&1 || true
        fi
        if [[ -n "$DPDK_DEFAULT_ROUTE" ]]; then
            ip route replace default via "$DPDK_DEFAULT_ROUTE" dev "$SHADOW_NIC_IFACE" proto static >/dev/null 2>&1 || true
        fi
    fi
    write_best_effort_summary_if_missing
}
trap cleanup EXIT

require_file() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        fail "Missing file: $path"
        exit 1
    fi
}

require_executable() {
    local path="$1"
    if [[ ! -x "$path" ]]; then
        fail "Missing executable: $path"
        exit 1
    fi
}

pin_process_if_configured() {
    local label="$1"
    local target_pid="$2"
    local cpu_affinity="$3"
    if [[ -z "$target_pid" || -z "$cpu_affinity" ]]; then
        return 0
    fi
    assert_cmd_exists taskset
    info "Pinning $label pid=$target_pid to CPU(s) $cpu_affinity"
    taskset -pc "$cpu_affinity" "$target_pid" >/dev/null
}

is_true() {
    case "${1,,}" in
        1|true|yes|on)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

default_route_iface() {
    ip route show default 2>/dev/null | awk '
        {
            for (i = 1; i <= NF; i++) {
                if ($i == "dev" && (i + 1) <= NF) {
                    print $(i + 1)
                    exit
                }
            }
        }
    '
}

resolve_afxdp_bind_mode() {
    local bind_mode
    bind_mode="$(awk '
        /^[[:space:]]*afxdp_bind_mode:[[:space:]]*[[:alnum:]_]+[[:space:]]*$/ {
            print $2
            exit
        }
    ' "$CONFIG_TMP" | tr -d '\r' | tr -d '[:space:]')"
    echo "${bind_mode:-zero_copy}"
}

guard_management_nic_xdp() {
    local default_iface
    default_iface="$(default_route_iface)"
    if [[ -n "$default_iface" && "$default_iface" == "$SHADOW_NIC_IFACE" ]] && ! is_true "$ALLOW_MANAGEMENT_NIC_XDP"; then
        fail "Refusing XDP on default-route interface $SHADOW_NIC_IFACE unless SHADOW_ALLOW_MANAGEMENT_NIC_XDP=true"
        exit 1
    fi
}

guard_management_nic_dpdk() {
    local default_iface
    default_iface="$(default_route_iface)"
    if [[ -n "$default_iface" && "$default_iface" == "$SHADOW_NIC_IFACE" ]] && ! is_true "$ALLOW_MANAGEMENT_NIC_DPDK"; then
        fail "Refusing DPDK on default-route interface $SHADOW_NIC_IFACE unless SHADOW_ALLOW_MANAGEMENT_NIC_DPDK=true"
        exit 1
    fi
}

validate_safe_profile() {
    local bind_mode="$1"
    if [[ "$XDP_ATTACH_MODE" != "xdpgeneric" ]]; then
        fail "Run29-C requires SHADOW_XDP_ATTACH_MODE=xdpgeneric (got $XDP_ATTACH_MODE)"
        exit 1
    fi
    if [[ "$bind_mode" != "copy" ]]; then
        fail "Run29-C requires afxdp_bind_mode=copy (got $bind_mode)"
        exit 1
    fi
    if [[ "$XDP_QUEUE_COUNT" -ne 1 ]]; then
        fail "Run29-C requires exactly one AF_XDP queue"
        exit 1
    fi
}

apply_safe_afxdp_posture() {
    local feature

    info "Applying R1-AFXDP-ENO1-SAFE host posture on $SHADOW_NIC_IFACE"
    ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
        warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s)"

    for feature in gro lro tso gso rx-gro-hw tx-gso-partial; do
        ethtool -K "$SHADOW_NIC_IFACE" "$feature" off >/dev/null 2>&1 || \
            warn "Could not disable $feature on $SHADOW_NIC_IFACE"
    done
    ethtool -K "$SHADOW_NIC_IFACE" ntuple off >/dev/null 2>&1 || \
        warn "Could not disable ntuple filters on $SHADOW_NIC_IFACE"
}

wait_for_path() {
    local path="$1"
    local timeout_s="$2"
    local waited=0
    while (( waited < timeout_s )); do
        if [[ -e "$path" ]]; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

tip_lut_bank_published() {
    local path="$1"
    python3 - "$path" <<'PY'
import struct
import sys

path = sys.argv[1]
try:
    with open(path, "rb") as fh:
        header = fh.read(64)
        if len(header) < 64:
            raise ValueError("short header")
        payload_offset = struct.unpack_from("<I", header, 48)[0]
        fh.seek(payload_offset)
        meta = fh.read(32)
        if len(meta) < 25:
            raise ValueError("short meta")
        active_idx = meta[0]
        last_update_unix_ns = struct.unpack_from("<Q", meta, 16)[0]
        if active_idx <= 1 and last_update_unix_ns != 0:
            sys.exit(0)
except Exception:
    pass
sys.exit(1)
PY
}

wait_for_tip_lut_publish() {
    local path="$1"
    local timeout_s="$2"
    local waited=0
    while (( waited < timeout_s )); do
        if tip_lut_bank_published "$path"; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

resolve_shred_capture_workers() {
    local workers
    workers="$(awk '
        /^[[:space:]]*shred_capture_workers:[[:space:]]*[0-9]+[[:space:]]*$/ {
            print $2
            exit
        }
    ' "$CONFIG_TMP" | tr -d '\r' | tr -d '[:space:]')"
    if [[ "$workers" =~ ^[0-9]+$ ]] && (( workers > 0 )); then
        XDP_QUEUE_COUNT="$workers"
    fi
}

attached_xdp_mode() {
    local first_line
    first_line="$(ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | head -n 1 || true)"
    if [[ "$first_line" == *"xdpgeneric"* ]]; then
        echo "xdpgeneric"
    elif [[ "$first_line" == *" xdp "* ]]; then
        echo "native"
    else
        echo "none"
    fi
}

attached_xdp_prog_name() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | sed -n 's/.*prog\/xdp id [0-9]\+ name \([^ ]\+\).*/\1/p' | head -n 1
}

ensure_afxdp_xdp_path() {
    assert_cmd_exists clang
    assert_cmd_exists llvm-strip
    assert_cmd_exists make
    assert_cmd_exists bpftool
    assert_cmd_exists ethtool

    if [[ ! -d /sys/fs/bpf ]]; then
        mkdir -p /sys/fs/bpf
    fi
    if ! mountpoint -q /sys/fs/bpf; then
        info "Mounting bpffs at /sys/fs/bpf"
        mount -t bpf bpf /sys/fs/bpf
    fi

    if [[ ! -f "$XDP_OBJ" ]]; then
        info "Building XDP program at $XDP_OBJ"
        (
            cd "$XDP_DIR"
            make
        )
    fi

    if is_true "$SAFE_MODE"; then
        apply_safe_afxdp_posture
    else
        info "Clamping $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s) for AF_XDP"
        ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
            warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s)"
    fi

    local attached_mode attached_prog reload_xdp
    attached_mode="$(attached_xdp_mode)"
    attached_prog="$(attached_xdp_prog_name)"
    reload_xdp="true"
    if is_true "$REUSE_ATTACHED_XDP" && [[ "$attached_mode" == "$XDP_ATTACH_MODE" ]] && [[ "$attached_prog" == shadow_rx_meta_* ]]; then
        info "Reusing existing XDP program on $SHADOW_NIC_IFACE (mode=$attached_mode prog=$attached_prog)"
        reload_xdp="false"
    fi

    if [[ "$reload_xdp" == "true" ]]; then
        info "Loading XDP program on $SHADOW_NIC_IFACE (mode=$XDP_ATTACH_MODE)"
        ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
        case "$XDP_ATTACH_MODE" in
            native)
                ip link set dev "$SHADOW_NIC_IFACE" xdp obj "$XDP_OBJ" sec xdp
                ;;
            xdpgeneric)
                ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric obj "$XDP_OBJ" sec xdp
                ;;
            *)
                fail "Unsupported SHADOW_XDP_ATTACH_MODE=$XDP_ATTACH_MODE"
                exit 1
                ;;
        esac
    fi

    info "Pinning XSKMAP at $XSKMAP_PIN"
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    local prog_id map_ids map_id
    prog_id="$(ip -details link show dev "$SHADOW_NIC_IFACE" | sed -n 's/.*prog\/xdp id \([0-9]\+\).*/\1/p' | head -n 1)"
    if [[ -z "$prog_id" ]]; then
        fail "Could not resolve attached XDP program id on $SHADOW_NIC_IFACE"
        exit 1
    fi
    map_ids="$(bpftool prog show id "$prog_id" | sed -n 's/.*map_ids \(.*\)$/\1/p' | tr ',' ' ')"
    for candidate in $map_ids; do
        if bpftool map show id "$candidate" 2>/dev/null | grep -q 'name xsks_map'; then
            map_id="$candidate"
            break
        fi
    done
    if [[ -z "${map_id:-}" ]]; then
        fail "Could not resolve xsks_map id from attached XDP program $prog_id"
        exit 1
    fi
    bpftool map pin id "$map_id" "$XSKMAP_PIN"

    if ! ip link show "$SHADOW_NIC_IFACE" | grep -qi "xdp"; then
        fail "XDP program did not attach to $SHADOW_NIC_IFACE"
        exit 1
    fi
    if [[ ! -e "$XSKMAP_PIN" ]]; then
        fail "Pinned XSKMAP missing at $XSKMAP_PIN"
        exit 1
    fi

    export SHADOW_XSKMAP_PIN="$XSKMAP_PIN"
    pass "XDP/XSKMAP ready on $SHADOW_NIC_IFACE"
}

ensure_dpdk_path() {
    assert_cmd_exists ethtool
    assert_cmd_exists dpdk-devbind.py
    assert_cmd_exists ip

    DPDK_PCI_ADDR="$(ethtool -i "$SHADOW_NIC_IFACE" | awk '/bus-info:/ { print $2; exit }')"
    DPDK_CURRENT_DRIVER="$(ethtool -i "$SHADOW_NIC_IFACE" | awk '/driver:/ { print $2; exit }')"
    DPDK_IPV4_CIDR="$(ip -o -4 addr show dev "$SHADOW_NIC_IFACE" | awk '{ print $4; exit }')"
    DPDK_DEFAULT_ROUTE="$(ip route show default | awk -v iface="$SHADOW_NIC_IFACE" '$0 ~ ("dev " iface) { print $3; exit }')"

    if [[ -z "$DPDK_PCI_ADDR" || -z "$DPDK_CURRENT_DRIVER" ]]; then
        fail "Could not resolve PCI/driver for $SHADOW_NIC_IFACE"
        exit 1
    fi

    modprobe vfio-pci
    sysctl -w vm.nr_hugepages=4096 >/dev/null 2>&1 || true
    mkdir -p /dev/hugepages
    mountpoint -q /dev/hugepages || mount -t hugetlbfs nodev /dev/hugepages

    info "Binding $SHADOW_NIC_IFACE ($DPDK_PCI_ADDR) to vfio-pci"
    ip link set dev "$SHADOW_NIC_IFACE" down
    dpdk-devbind.py --bind=vfio-pci "$DPDK_PCI_ADDR"
    DPDK_BOUND_TO_VFIO="true"
}

ensure_private_packet_path() {
    case "$PRIVATE_PACKET_BACKEND" in
        afxdp)
            ensure_afxdp_xdp_path
            ;;
        dpdk)
            ensure_dpdk_path
            ;;
        *)
            fail "Unsupported SHADOW_RUN29C_PRIVATE_PACKET_BACKEND=$PRIVATE_PACKET_BACKEND"
            exit 1
            ;;
    esac
}

json_field() {
    local file="$1"
    local field="$2"
    python3 - "$file" "$field" <<'PY'
import json
import sys
import time

path, field = sys.argv[1], sys.argv[2]
data = None
for _ in range(10):
    try:
        with open(path, "r", encoding="utf-8") as fh:
            raw = fh.read()
        if not raw.strip():
            raise ValueError("empty JSON file")
        data = json.loads(raw)
        break
    except (FileNotFoundError, json.JSONDecodeError, ValueError):
        time.sleep(0.1)

if data is None:
    print("null")
    raise SystemExit(0)

value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if isinstance(value, bool):
    print("true" if value else "false")
elif value is None:
    print("null")
else:
    print(value)
PY
}

json_file_valid() {
    local file="$1"
    python3 - "$file" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as fh:
        raw = fh.read()
    if not raw.strip():
        raise ValueError("empty JSON file")
    json.loads(raw)
except (FileNotFoundError, json.JSONDecodeError, ValueError):
    raise SystemExit(1)
PY
}

snapshot_json_if_valid() {
    local src="$1"
    local dst="$2"
    python3 - "$src" "$dst" <<'PY'
import json
import os
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])

raw = src.read_text(encoding="utf-8")
if not raw.strip():
    raise ValueError("empty JSON file")
payload = json.loads(raw)

dst.parent.mkdir(parents=True, exist_ok=True)
tmp = dst.with_suffix(dst.suffix + ".tmp")
tmp.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
os.replace(tmp, dst)
PY
}

summary_gate_input_path() {
    if json_file_valid "$RESULT_GATE_INPUT" >/dev/null 2>&1; then
        echo "$RESULT_GATE_INPUT"
        return 0
    fi
    if json_file_valid "$GATE_INPUT" >/dev/null 2>&1; then
        echo "$GATE_INPUT"
        return 0
    fi
    return 1
}

summary_gate_report_path() {
    if json_file_valid "$RESULT_GATE_REPORT" >/dev/null 2>&1; then
        echo "$RESULT_GATE_REPORT"
        return 0
    fi
    if json_file_valid "$GATE_REPORT" >/dev/null 2>&1; then
        echo "$GATE_REPORT"
        return 0
    fi
    return 1
}

gate_metric() {
    local field="$1"
    local value path

    path="$(summary_gate_report_path || true)"
    if [[ -n "$path" ]]; then
        value="$(json_field "$path" "$field")"
        if [[ "$value" != "null" ]]; then
            echo "$value"
            return 0
        fi
    fi

    path="$(summary_gate_input_path || true)"
    if [[ -n "$path" ]]; then
        value="$(json_field "$path" "$field")"
        if [[ "$value" != "null" ]]; then
            echo "$value"
            return 0
        fi
    fi

    echo "null"
}

live_gate_input_path() {
    if json_file_valid "$GATE_INPUT" >/dev/null 2>&1; then
        echo "$GATE_INPUT"
        return 0
    fi
    return 1
}

live_gate_report_path() {
    if json_file_valid "$GATE_REPORT" >/dev/null 2>&1; then
        echo "$GATE_REPORT"
        return 0
    fi
    return 1
}

live_gate_metric() {
    local field="$1"
    local value path

    path="$(live_gate_report_path || true)"
    if [[ -n "$path" ]]; then
        value="$(json_field "$path" "$field")"
        if [[ "$value" != "null" ]]; then
            echo "$value"
            return 0
        fi
    fi

    path="$(live_gate_input_path || true)"
    if [[ -n "$path" ]]; then
        value="$(json_field "$path" "$field")"
        if [[ "$value" != "null" ]]; then
            echo "$value"
            return 0
        fi
    fi

    echo "null"
}

metric_int() {
    local value="${1:-0}"
    if [[ -z "$value" || "$value" == "null" ]]; then
        echo 0
        return 0
    fi
    echo "$value"
}

live_gate_metric_int() {
    metric_int "$(live_gate_metric "$1")"
}

metric_delta() {
    local final_value
    local baseline_value
    final_value="$(metric_int "${1:-0}")"
    baseline_value="$(metric_int "${2:-0}")"
    echo $((final_value - baseline_value))
}

capture_watcher_arm_baseline() {
    baseline_worker_records_seen="$(live_gate_metric_int "worker_records_seen")"
    baseline_worker_pool_state_hit="$(live_gate_metric_int "worker_pool_state_hit")"
    baseline_worker_route_actionable="$(live_gate_metric_int "worker_route_actionable")"
    baseline_worker_route_resolved_public_rpc_scout="$(live_gate_metric_int "worker_route_resolved_public_rpc_scout")"
    baseline_run38_target_public_scout_clmm_route_resolved="$(live_gate_metric_int "run38_target_public_scout_clmm_route_resolved")"
    baseline_run38_target_public_scout_clmm_r1_attempt="$(live_gate_metric_int "run38_target_public_scout_clmm_r1_attempt")"
    baseline_public_scout_attempt_latch_set="$(live_gate_metric_int "public_scout_attempt_latch_set")"
    baseline_public_scout_attempt_latch_drop_count="$(live_gate_metric_int "public_scout_attempt_latch_drop_count")"
    baseline_worker_candidates_entered_process_intent="$(live_gate_metric_int "worker_candidates_entered_process_intent")"
    baseline_worker_public_rpc_scout_emit_attempt="$(live_gate_metric_int "worker_public_rpc_scout_emit_attempt")"
    baseline_public_rpc_scout_enqueued="$(live_gate_metric_int "public_rpc_scout_enqueued")"
    baseline_public_rpc_transport_attempt="$(live_gate_metric_int "public_rpc_transport_attempt")"
    baseline_public_rpc_transport_failure="$(live_gate_metric_int "public_rpc_transport_failure")"
    baseline_public_rpc_transport_rejected="$(live_gate_metric_int "public_rpc_transport_rejected")"
    baseline_public_rpc_nack_recorded="$(live_gate_metric_int "public_rpc_nack_recorded")"
    baseline_worker_pm_reservation_created="$(live_gate_metric_int "worker_pm_reservation_created")"
    baseline_pm_reservation_released="$(live_gate_metric_int "pm_reservation_released")"
    baseline_live_landed="$(live_gate_metric_int "live_landed")"
    baseline_r1_auto_admission_attempt="$(live_gate_metric_int "r1_auto_admission_attempt")"
    baseline_r1_auto_admission_admit="$(live_gate_metric_int "r1_auto_admission_admit")"
    baseline_r1_auto_admission_reject_micro_market="$(live_gate_metric_int "r1_auto_admission_reject_micro_market")"
    baseline_r1_auto_admission_reject_proof_filter="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter")"
    baseline_r1_auto_admission_reject_template_executability="$(live_gate_metric_int "r1_auto_admission_reject_template_executability")"
    baseline_r1_auto_admission_reject_attempt_latch="$(live_gate_metric_int "r1_auto_admission_reject_attempt_latch")"
    baseline_run38_target_public_scout_clmm_reject_proof_filter="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_proof_filter")"
    baseline_run38_target_public_scout_clmm_reject_micro_market="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_micro_market")"
    baseline_run38_target_public_scout_clmm_reject_inventory="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory")"
    baseline_run38_target_public_scout_clmm_reject_inventory_missing_input_ata="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory_missing_input_ata")"
    baseline_run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized")"
    baseline_run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient")"
    baseline_run38_target_public_scout_clmm_reject_inventory_token_program_mismatch="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch")"
    baseline_run38_target_public_scout_clmm_reject_inventory_other="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_inventory_other")"
    baseline_run38_target_public_scout_clmm_reject_template_executability="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_template_executability")"
    baseline_run38_target_public_scout_clmm_reject_attempt_latch="$(live_gate_metric_int "run38_target_public_scout_clmm_reject_attempt_latch")"
    baseline_run38_target_public_scout_clmm_admit="$(live_gate_metric_int "run38_target_public_scout_clmm_admit")"
    baseline_run38_target_public_scout_clmm_process_intent="$(live_gate_metric_int "run38_target_public_scout_clmm_process_intent")"
    baseline_run38_target_public_scout_clmm_emit_attempt="$(live_gate_metric_int "run38_target_public_scout_clmm_emit_attempt")"
    baseline_run38_target_public_scout_clmm_transport_attempt="$(live_gate_metric_int "run38_target_public_scout_clmm_transport_attempt")"
    baseline_run38_target_public_scout_clmm_pm_created="$(live_gate_metric_int "run38_target_public_scout_clmm_pm_created")"
    baseline_r1_auto_admission_reject_proof_filter_no_quote="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter_no_quote")"
    baseline_r1_auto_admission_reject_proof_filter_no_positive_ev="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter_no_positive_ev")"
    baseline_r1_auto_admission_reject_proof_filter_no_shadow_guard="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter_no_shadow_guard")"
    baseline_r1_auto_admission_reject_proof_filter_public_scout_shape="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter_public_scout_shape")"
    baseline_r1_auto_admission_reject_proof_filter_other="$(live_gate_metric_int "r1_auto_admission_reject_proof_filter_other")"
    baseline_r1_auto_admission_reject_micro_market_no_uplift="$(live_gate_metric_int "r1_auto_admission_reject_micro_market_no_uplift")"
    baseline_r1_auto_admission_reject_micro_market_negative_ev="$(live_gate_metric_int "r1_auto_admission_reject_micro_market_negative_ev")"
    baseline_r1_auto_admission_reject_micro_market_below_floor="$(live_gate_metric_int "r1_auto_admission_reject_micro_market_below_floor")"
    baseline_r1_auto_admission_reject_micro_market_other="$(live_gate_metric_int "r1_auto_admission_reject_micro_market_other")"
    baseline_r1_auto_admission_admit_to_process_intent_attempt="$(live_gate_metric_int "r1_auto_admission_admit_to_process_intent_attempt")"
    baseline_r1_auto_admission_admit_to_process_intent_ok="$(live_gate_metric_int "r1_auto_admission_admit_to_process_intent_ok")"
    baseline_r1_auto_admission_admit_to_process_intent_drop="$(live_gate_metric_int "r1_auto_admission_admit_to_process_intent_drop")"
    WATCHER_ARM_BASELINE_CAPTURED="true"
}

refresh_preserved_gate_report() {
    if ! json_file_valid "$RESULT_GATE_INPUT" >/dev/null 2>&1; then
        return 0
    fi
    if json_file_valid "$RESULT_GATE_REPORT" >/dev/null 2>&1 && [[ ! "$RESULT_GATE_REPORT" -ot "$RESULT_GATE_INPUT" ]]; then
        return 0
    fi
    if [[ ! -x "$SHADOW_CONTROL_BIN" ]]; then
        return 0
    fi
    "$SHADOW_CONTROL_BIN" \
        --rollout-gate-input-path "$RESULT_GATE_INPUT" \
        --rollout-gate-output-path "$RESULT_GATE_REPORT" \
        >/dev/null 2>&1 || true
}

capture_final_live_diagnostics() {
    [[ "$FINAL_DIAGNOSTICS_CAPTURED" == "true" ]] && return 0

    local attempt=0
    while (( attempt < FINAL_GATE_SNAPSHOT_RETRIES )); do
        snapshot_json_if_valid "$GATE_INPUT" "$RESULT_GATE_INPUT" >/dev/null 2>&1 || true
        snapshot_json_if_valid "$GATE_REPORT" "$RESULT_GATE_REPORT" >/dev/null 2>&1 || true
        refresh_preserved_gate_report

        if json_file_valid "$RESULT_GATE_INPUT" >/dev/null 2>&1 && \
            json_file_valid "$RESULT_GATE_REPORT" >/dev/null 2>&1; then
            break
        fi

        attempt=$((attempt + 1))
        if (( attempt < FINAL_GATE_SNAPSHOT_RETRIES )); then
            sleep "$FINAL_GATE_SNAPSHOT_SLEEP_SECONDS"
        fi
    done

    FINAL_DIAGNOSTICS_CAPTURED="true"
}

yaml_string() {
    local key="$1"
    sed -n "s/^[[:space:]]*$key:[[:space:]]*\"\([^\"]*\)\"[[:space:]]*$/\1/p" "$CONFIG_TMP" | head -1
}

write_public_proof_config() {
    assert_cmd_exists envsubst
    python3 - "$CONFIG_TEMPLATE" "$CONFIG_TMP" "$PUBLIC_RPC_ENDPOINT" "$SHOCK_RESEARCH_PATH" "$GATE_REPORT" "$MAX_LIVE_TRADE_INPUT_SOL" "$MAX_POSITION_SOL_PER_MINT" "$MAX_NOTIONAL_SOL_PER_ENTITY" "$SAFE_MODE" "$PRIVATE_PACKET_BACKEND" "$DPDK_PORT_ID" "$MUTATE_ROLLOUT_CAPS" <<'PY'
import re
import subprocess
import sys
from pathlib import Path

template_path = Path(sys.argv[1])
dst_path = Path(sys.argv[2])
public_rpc_endpoint = sys.argv[3]
shock_path = sys.argv[4]
gate_report_path = sys.argv[5]
max_live_trade_input_sol = sys.argv[6]
max_position_sol_per_mint = sys.argv[7]
max_notional_sol_per_entity = sys.argv[8]
safe_mode = sys.argv[9].lower() in {"1", "true", "yes", "on"}
private_packet_backend = sys.argv[10]
dpdk_port_id = sys.argv[11]
mutate_rollout_caps = sys.argv[12].lower() in {"1", "true", "yes", "on"}
desired_bind_mode = "copy" if safe_mode else "zero_copy"

expanded = subprocess.run(
    ["envsubst"],
    input=template_path.read_text(),
    text=True,
    capture_output=True,
    check=True,
).stdout

expanded = re.sub(
    r'(^public_rpc:\s*$\n(?:^[ \t].*$\n)+?^[ \t]*endpoint:\s*).+$',
    rf'\1"{public_rpc_endpoint}"',
    expanded,
    count=1,
    flags=re.M,
)
expanded = re.sub(
    r'(^\s*gate_report_path:\s*).+$',
    rf'\1"{gate_report_path}"',
    expanded,
    count=1,
    flags=re.M,
)
if re.search(r'^\s*private_packet_backend:\s*', expanded, flags=re.M):
    expanded = re.sub(
        r'(^\s*private_packet_backend:\s*).+$',
        rf'\1{private_packet_backend}',
        expanded,
        count=1,
        flags=re.M,
    )
else:
    expanded = re.sub(
        r'(^\s*shred_capture_workers:\s*.+$)',
        rf'\1\n      private_packet_backend: {private_packet_backend}',
        expanded,
        count=1,
        flags=re.M,
    )
if re.search(r'^\s*afxdp_bind_mode:\s*', expanded, flags=re.M):
    expanded = re.sub(
        r'(^\s*afxdp_bind_mode:\s*).+$',
        rf'\1{desired_bind_mode}',
        expanded,
        count=1,
        flags=re.M,
    )
else:
    expanded = re.sub(
        r'(^\s*shred_capture_workers:\s*.+$)',
        rf'\1\n      afxdp_bind_mode: {desired_bind_mode}',
        expanded,
        count=1,
        flags=re.M,
    )
if private_packet_backend == "dpdk":
    if re.search(r'^\s*dpdk:\s*$', expanded, flags=re.M):
        expanded = re.sub(
            r'(^\s*port_id:\s*).+$',
            rf'\1{dpdk_port_id}',
            expanded,
            count=1,
            flags=re.M,
        )
    else:
        expanded = re.sub(
            r'(^\s*afxdp_bind_mode:\s*.+$)',
            rf'\1\n      dpdk:\n        port_id: {dpdk_port_id}',
            expanded,
            count=1,
            flags=re.M,
        )
grpc_fallback_match = re.search(
    r'^\s*grpc_fallback:\s*"?(.*?)"?\s*$',
    expanded,
    flags=re.M,
)
grpc_fallback = grpc_fallback_match.group(1).strip() if grpc_fallback_match else ""
if not grpc_fallback:
    if re.search(r'^\s*grpc_workers:\s*', expanded, flags=re.M):
        expanded = re.sub(
            r'(^[ \t]*grpc_workers:).*$',
            r'\1 0',
            expanded,
            count=1,
            flags=re.M,
        )
    else:
        expanded = re.sub(
            r'(^\s*grpc_fallback_token:\s*.+$)',
            r'\1\n      grpc_workers: 0',
            expanded,
            count=1,
            flags=re.M,
        )
if re.search(r'^\s*sh8_afxdp_source_probe_mode:\s*', expanded, flags=re.M):
    expanded = re.sub(
        r'(^\s*sh8_afxdp_source_probe_mode:\s*).+$',
        r'\1disabled',
        expanded,
        count=1,
        flags=re.M,
    )
else:
    expanded = re.sub(
        rf'(^\s*afxdp_bind_mode:\s*{re.escape(desired_bind_mode)}\s*$)',
        r'\1\n      sh8_afxdp_source_probe_mode: disabled',
        expanded,
        count=1,
        flags=re.M,
    )
if re.search(r'^\s*shock_research_export_path:\s*', expanded, flags=re.M):
    expanded = re.sub(
        r'(^\s*shock_research_export_path:\s*).+$',
        rf'\1"{shock_path}"',
        expanded,
        count=1,
        flags=re.M,
    )
else:
    expanded = re.sub(
        r'(^\s*rpc_timeout_ms:\s*.+$)',
        rf'\1\n  shock_research_export_path: "{shock_path}"',
        expanded,
        count=1,
        flags=re.M,
    )

def replace_in_block(text: str, block_name: str, child_indent: int, replacements: dict[str, str]) -> str:
    lines = text.splitlines()
    inside = False
    block_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not inside:
            if stripped == f"{block_name}:":
                inside = True
                block_indent = indent
            continue
        if stripped and indent <= block_indent:
            inside = False
            if stripped == f"{block_name}:":
                inside = True
                block_indent = indent
            continue
        for key, value in replacements.items():
            expected_prefix = " " * child_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"

def replace_in_nested_block(
    text: str,
    parent: str,
    child: str,
    child_key_indent: int,
    replacements: dict[str, str],
) -> str:
    lines = text.splitlines()
    in_parent = False
    in_child = False
    parent_indent = 0
    child_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not in_parent:
            if stripped == f"{parent}:":
                in_parent = True
                parent_indent = indent
            continue
        if stripped and indent <= parent_indent:
            in_parent = False
            in_child = False
            if stripped == f"{parent}:":
                in_parent = True
                parent_indent = indent
            continue
        if not in_child:
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        if stripped and indent <= child_indent:
            in_child = False
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        for key, value in replacements.items():
            expected_prefix = " " * child_key_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"

if mutate_rollout_caps:
    expanded = replace_in_block(
        expanded,
        "rollout",
        2,
        {
            "max_live_trade_input_sol": max_live_trade_input_sol,
            "max_position_sol_per_mint": max_position_sol_per_mint,
            "max_notional_sol_per_entity": max_notional_sol_per_entity,
        },
    )
expanded = replace_in_nested_block(
    expanded,
    "r1",
    "live_admission",
    4,
    {
        "max_live_trade_input_sol": max_live_trade_input_sol,
        "max_position_sol_per_mint": max_position_sol_per_mint,
        "max_notional_sol_per_entity": max_notional_sol_per_entity,
    },
)
expanded = replace_in_nested_block(
    expanded,
    "r1",
    "public_scout",
    4,
    {
        "max_live_trade_input_sol": max_live_trade_input_sol,
        "max_notional_sol_per_entity": max_notional_sol_per_entity,
    },
)

dst_path.write_text(expanded)
PY
}

seed_shadowguard_preflight() {
    local preflight_dir="$PACK_DIR/results/21_shadowguard_preflight"
    local program_id template_matches mainnet_deployed
    mkdir -p "$preflight_dir"
    program_id="$(yaml_string shadow_guard_program_id)"
    if [[ -z "$program_id" ]]; then
        fail "shadow_guard_program_id missing from generated config"
        exit 1
    fi
    if [[ ! -x "$TEMPLATE_PROOF_BIN" ]]; then
        fail "Missing template proof helper: $TEMPLATE_PROOF_BIN"
        exit 1
    fi
    "$TEMPLATE_PROOF_BIN" --config "$CONFIG_TMP" >"$preflight_dir/template_proof.txt" 2>"$preflight_dir/template_proof.stderr"
    template_matches="$(sed -n 's/^shadow_guard_template_program_id_matches=//p' "$preflight_dir/template_proof.txt" | head -1)"
    if [[ "$template_matches" != "true" ]]; then
        fail "ShadowGuard template proof did not confirm the configured program id"
        exit 1
    fi
    mainnet_deployed="$(python3 - "$MAINNET_URL" "$program_id" <<'PY'
import json
import sys
import urllib.request

url = sys.argv[1]
program_id = sys.argv[2]
payload = json.dumps({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [program_id, {"encoding": "base64"}],
}).encode("utf-8")
req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"})
try:
    with urllib.request.urlopen(req, timeout=10) as resp:
        data = json.load(resp)
    value = (data.get("result") or {}).get("value") or {}
    print("true" if value.get("executable") else "false")
except Exception:
    print("false")
PY
)"
    {
        echo "shadow_guard_program_id_present=true"
        echo "shadow_guard_mainnet_deployed=$mainnet_deployed"
        echo "shadow_guard_template_program_id_matches=$template_matches"
        echo "program_id=$program_id"
    } >"$preflight_dir/summary.txt"
}

seed_startup_gate_report() {
    cat >"$GATE_REPORT" <<JSON
{
  "all_gates_green": false,
  "public_scout_gates_green": true,
  "shadow_guard_deployed": true,
  "shadow_guard_program_id_present": true,
  "shadow_guard_mainnet_deployed": true,
  "shadow_guard_template_program_id_matches": true,
  "dominance_live_allowed": false,
  "competition_adjusted_primary_allowed": false
}
JSON
}

validate_public_scout_profile() {
    python3 - "$CONFIG_TMP" "$MAX_LIVE_TRADE_INPUT_SOL" "$MAX_POSITION_SOL_PER_MINT" "$MAX_NOTIONAL_SOL_PER_ENTITY" <<'PY'
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
max_live_trade_input_sol = sys.argv[2]
max_position_sol_per_mint = sys.argv[3]
max_notional_sol_per_entity = sys.argv[4]
checks = {
    "rollout.mode": "mode: tiny_live" in text,
    "rollout.cap_rung": "cap_rung: r1_tiny" in text,
    "rollout.max_live_trade_input_sol": text.count(f"max_live_trade_input_sol: {max_live_trade_input_sol}") >= 3,
    "rollout.max_position_sol_per_mint": text.count(f"max_position_sol_per_mint: {max_position_sol_per_mint}") >= 2,
    "rollout.max_notional_sol_per_entity": text.count(f"max_notional_sol_per_entity: {max_notional_sol_per_entity}") >= 3,
    "rollout.require_all_gates_green": "require_all_gates_green: false" in text,
    "rollout.require_public_scout_gates_green": "require_public_scout_gates_green: true" in text,
    "r1.live_admission.max_new_positions_per_window": "max_new_positions_per_window: 1" in text,
    "r1.live_admission.kill_on_health_drop": text.count("kill_on_health_drop: true") >= 2,
    "r1.live_admission.kill_on_fee_leak": text.count("kill_on_fee_leak: true") >= 2,
    "r1.live_admission.kill_on_inventory_mismatch": text.count("kill_on_inventory_mismatch: true") >= 2,
    "r1.public_scout.max_scout_attempts_per_window": "max_scout_attempts_per_window: 1" in text,
    "r1.public_scout.require_shadow_guard": text.count("require_shadow_guard: true") >= 2,
    "submit.routing.pre_sign_egress_admission_gate": "pre_sign_egress_admission_gate: true" in text,
    "public_rpc.max_pending_tx": "max_pending_tx: 1" in text,
}

missing = [name for name, ok in checks.items() if not ok]
if missing:
    print("missing=" + ",".join(missing))
    raise SystemExit(1)
PY
}

collect_target_trace_metrics() {
    local log_path="${1:-$TIER0_LOG}"
    local shock_path="${2:-$SHOCK_RESEARCH_PATH}"
    python3 - "$log_path" "$shock_path" "$ALLOW_ONLY_POOL" "$ALLOW_ONLY_MINT_IN" "$ALLOW_ONLY_MINT_OUT" "$REQUIRE_TARGET_PUBLIC_SCOUT" <<'PY'
import shlex
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
shock_path = Path(sys.argv[2])
pool = sys.argv[3]
mint_in = sys.argv[4]
mint_out = sys.argv[5]
require_target = sys.argv[6].strip().lower() in {"1", "true", "yes", "on"}

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
ALPHABET_INDEX = {ch: idx for idx, ch in enumerate(ALPHABET)}

metrics = {
    "TARGET_EXACT_DIRECTION_HITS": 0,
    "TARGET_ROUTE_SEEN_HITS": 0,
    "TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS": 0,
    "TARGET_NON_PUBLIC_ROUTE_HITS": 0,
    "TARGET_QUOTE_REQUEST_SEEN_HITS": 0,
    "TARGET_QUOTE_MATCHED_HITS": 0,
    "TARGET_QUOTE_WAIT_TIMEOUT_HITS": 0,
    "TARGET_PUBLIC_SCOUT_ROUTE_OVERRIDE_HITS": 0,
    "TARGET_PUBLIC_SCOUT_BLOCKED_HITS": 0,
    "TARGET_PUBLIC_SCOUT_BLOCKED_REASON": "",
    "TARGET_PUBLIC_SCOUT_BLOCKED_STAGE": "",
    "TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL": "",
    "TARGET_FIRST_ROUTE_DECISION": "",
    "TARGET_FIRST_NON_PUBLIC_DECISION": "",
    "TARGET_FIRST_SIGNATURE": "",
    "TARGET_FIRST_SLOT": "",
    "TARGET_ROLLOUT_SHADOW_HITS": 0,
    "TARGET_WOULD_BUILD_HITS": 0,
    "TARGET_WOULD_SIGN_HITS": 0,
    "TARGET_WOULD_SEND_HITS": 0,
    "TARGET_FIRST_ROLLOUT_SIGNATURE": "",
    "TARGET_ASSERTION_RESULT": "not_configured",
}

def parse_fields(raw_line: str):
    fields = {}
    for token in raw_line.strip().split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        fields[key] = value
    return fields

def fields_match(fields):
    return (
        fields.get("pool") == pool
        and fields.get("mint_in") == mint_in
        and fields.get("mint_out") == mint_out
    )

def b58decode_to_hex(value: str):
    if not value:
        return None
    number = 0
    for ch in value:
        digit = ALPHABET_INDEX.get(ch)
        if digit is None:
            return None
        number = number * 58 + digit
    decoded = bytearray()
    while number:
        number, rem = divmod(number, 256)
        decoded.append(rem)
    decoded.reverse()
    leading_zeroes = 0
    for ch in value:
        if ch != "1":
            break
        leading_zeroes += 1
    return (b"\x00" * leading_zeroes + bytes(decoded)).hex()

target_pool_hex = b58decode_to_hex(pool)
target_mint_in_hex = b58decode_to_hex(mint_in)
target_mint_out_hex = b58decode_to_hex(mint_out)

if pool and mint_in and mint_out and log_path.exists():
    with log_path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            fields = parse_fields(raw_line)

            if raw_line.startswith("TRACE target_public_scout_route_override "):
                if not fields_match(fields):
                    continue
                metrics["TARGET_PUBLIC_SCOUT_ROUTE_OVERRIDE_HITS"] += 1
                if not metrics["TARGET_FIRST_SIGNATURE"]:
                    metrics["TARGET_FIRST_SIGNATURE"] = fields.get("signature", "")
                if not metrics["TARGET_FIRST_SLOT"]:
                    metrics["TARGET_FIRST_SLOT"] = fields.get("slot", "")
                continue
            if raw_line.startswith("TRACE target_public_scout_blocked "):
                if not fields_match(fields):
                    continue
                metrics["TARGET_PUBLIC_SCOUT_BLOCKED_HITS"] += 1
                if not metrics["TARGET_PUBLIC_SCOUT_BLOCKED_REASON"]:
                    metrics["TARGET_PUBLIC_SCOUT_BLOCKED_REASON"] = fields.get("reason", "")
                if not metrics["TARGET_PUBLIC_SCOUT_BLOCKED_STAGE"]:
                    metrics["TARGET_PUBLIC_SCOUT_BLOCKED_STAGE"] = fields.get("stage", "")
                if not metrics["TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL"]:
                    metrics["TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL"] = fields.get("detail", "")
                if not metrics["TARGET_FIRST_SIGNATURE"]:
                    metrics["TARGET_FIRST_SIGNATURE"] = fields.get("signature", "")
                if not metrics["TARGET_FIRST_SLOT"]:
                    metrics["TARGET_FIRST_SLOT"] = fields.get("slot", "")
                continue

            if pool not in raw_line or mint_in not in raw_line or mint_out not in raw_line:
                continue
            if not fields_match(fields):
                continue

            metrics["TARGET_EXACT_DIRECTION_HITS"] += 1
            if not metrics["TARGET_FIRST_SIGNATURE"]:
                metrics["TARGET_FIRST_SIGNATURE"] = fields.get("signature", "")
            if not metrics["TARGET_FIRST_SLOT"]:
                metrics["TARGET_FIRST_SLOT"] = fields.get("slot", "")

            stage = fields.get("stage", "")
            if stage == "route":
                metrics["TARGET_ROUTE_SEEN_HITS"] += 1
                decision = fields.get("decision", "")
                if not metrics["TARGET_FIRST_ROUTE_DECISION"] and decision:
                    metrics["TARGET_FIRST_ROUTE_DECISION"] = decision
                if decision == "public_rpc_scout":
                    metrics["TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS"] += 1
                elif decision:
                    metrics["TARGET_NON_PUBLIC_ROUTE_HITS"] += 1
                    if not metrics["TARGET_FIRST_NON_PUBLIC_DECISION"]:
                        metrics["TARGET_FIRST_NON_PUBLIC_DECISION"] = decision
            elif stage == "quote_request":
                metrics["TARGET_QUOTE_REQUEST_SEEN_HITS"] += 1
                status = fields.get("status", "")
                if status == "quote_matched":
                    metrics["TARGET_QUOTE_MATCHED_HITS"] += 1
                elif status == "quote_wait_timeout":
                    metrics["TARGET_QUOTE_WAIT_TIMEOUT_HITS"] += 1

if (
    pool
    and mint_in
    and mint_out
    and shock_path.exists()
    and target_pool_hex
    and target_mint_in_hex
    and target_mint_out_hex
):
    with shock_path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            raw_line = raw_line.strip()
            if not raw_line:
                continue
            try:
                record = json.loads(raw_line)
            except json.JSONDecodeError:
                continue
            if record.get("record_type") != "rollout_shadow":
                continue
            if (
                str(record.get("pool_pubkey_hex", "")).lower() != target_pool_hex
                or str(record.get("mint_in_hex", "")).lower() != target_mint_in_hex
                or str(record.get("mint_out_hex", "")).lower() != target_mint_out_hex
            ):
                continue
            metrics["TARGET_ROLLOUT_SHADOW_HITS"] += 1
            if record.get("would_build") is True:
                metrics["TARGET_WOULD_BUILD_HITS"] += 1
            if record.get("would_sign") is True:
                metrics["TARGET_WOULD_SIGN_HITS"] += 1
            if record.get("would_send") is True:
                metrics["TARGET_WOULD_SEND_HITS"] += 1
            if not metrics["TARGET_FIRST_ROLLOUT_SIGNATURE"]:
                metrics["TARGET_FIRST_ROLLOUT_SIGNATURE"] = str(record.get("signature_hex", ""))

    if metrics["TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS"] > 0:
        metrics["TARGET_ASSERTION_RESULT"] = "passed"
    elif require_target:
        metrics["TARGET_ASSERTION_RESULT"] = "NO_TARGET_HIT"
    else:
        metrics["TARGET_ASSERTION_RESULT"] = "not_required"
elif pool and mint_in and mint_out:
    metrics["TARGET_ASSERTION_RESULT"] = "NO_TARGET_HIT" if require_target else "not_required"

for key, value in metrics.items():
    if isinstance(value, str):
        print(f"{key}={shlex.quote(value)}")
    else:
        print(f"{key}={value}")
PY
}

run_window_loop() {
    local deadline
    deadline=$((SECONDS + WINDOW_SECONDS_INT))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited before the proof window completed"
            tail -n 80 "$TIER0_LOG" | tee "$TIER0_TAIL_FILE"
            exit 1
        fi
        if [[ -f "$GATE_REPORT" ]]; then
            sample_slots="$(json_field "$GATE_REPORT" "sample_slots")"
            observe_candidates="$(json_field "$GATE_REPORT" "observe_candidates")"
            entered="$(json_field "$GATE_INPUT" "worker_candidates_entered_process_intent")"
            would_send="$(json_field "$GATE_INPUT" "worker_rollout_would_send_events_published")"
            public_no_send="$(json_field "$GATE_REPORT" "rollout_public_rpc_no_send_would_sends")"
            info "gate_report sample_slots=$sample_slots observe_candidates=$observe_candidates process_intent=$entered public_rpc_no_send=$public_no_send would_send=$would_send"
        else
            info "Waiting for $GATE_REPORT"
        fi
        sleep "$POLL_SECONDS"
    done
}

watcher_trigger_reached() {
    case "$WATCH_TRIGGER_ON" in
        exact_direction)
            (( TARGET_EXACT_DIRECTION_HITS > 0 ))
            ;;
        route_seen)
            (( TARGET_ROUTE_SEEN_HITS > 0 ))
            ;;
        quote_matched)
            (( TARGET_QUOTE_MATCHED_HITS > 0 ))
            ;;
        *)
            fail "Unsupported watcher trigger mode: $WATCH_TRIGGER_ON"
            exit 1
            ;;
    esac
}

write_watcher_summary() {
    local outcome="$1"
    local elapsed_seconds="$2"
    {
        echo "mode=watcher"
        echo "watch_log_path=$WATCH_LOG_PATH"
        echo "watch_timeout_seconds=$WATCH_TIMEOUT_SECONDS"
        echo "watch_trigger_on=$WATCH_TRIGGER_ON"
        echo "watch_auto_arm_strict_proof=$WATCH_AUTO_ARM_STRICT_PROOF"
        echo "armed_window_seconds=$ARMED_WINDOW_SECONDS"
        echo "elapsed_seconds=$elapsed_seconds"
        echo "outcome=$outcome"
        echo "target_assertion_result=$TARGET_ASSERTION_RESULT"
        echo "target_exact_direction_hits=$TARGET_EXACT_DIRECTION_HITS"
        echo "target_route_seen_hits=$TARGET_ROUTE_SEEN_HITS"
        echo "target_quote_request_seen_hits=$TARGET_QUOTE_REQUEST_SEEN_HITS"
        echo "target_quote_matched_hits=$TARGET_QUOTE_MATCHED_HITS"
        echo "target_quote_wait_timeout_hits=$TARGET_QUOTE_WAIT_TIMEOUT_HITS"
        echo "target_public_rpc_scout_route_hits=$TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS"
        echo "target_non_public_route_hits=$TARGET_NON_PUBLIC_ROUTE_HITS"
        echo "target_public_scout_route_override_hits=$TARGET_PUBLIC_SCOUT_ROUTE_OVERRIDE_HITS"
        echo "target_public_scout_blocked_hits=$TARGET_PUBLIC_SCOUT_BLOCKED_HITS"
        echo "target_public_scout_blocked_reason=$TARGET_PUBLIC_SCOUT_BLOCKED_REASON"
        echo "target_public_scout_blocked_stage=$TARGET_PUBLIC_SCOUT_BLOCKED_STAGE"
        echo "target_public_scout_blocked_detail=$TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL"
        echo "target_first_route_decision=$TARGET_FIRST_ROUTE_DECISION"
        echo "target_first_non_public_decision=$TARGET_FIRST_NON_PUBLIC_DECISION"
        echo "target_first_signature=$TARGET_FIRST_SIGNATURE"
        echo "target_first_slot=$TARGET_FIRST_SLOT"
    } | tee "$WATCHER_SUMMARY_FILE"
}

run_watcher_mode() {
    local watcher_started_at elapsed_seconds=0
    watcher_started_at=$SECONDS

    if [[ -z "$WATCH_LOG_PATH" ]]; then
        fail "Watcher mode requires SHADOW_RUN29C_WATCH_LOG_PATH"
        exit 1
    fi

    mkdir -p "$RESULT_DIR"
    rm -f "$WATCHER_SUMMARY_FILE"

    info "Watcher mode armed on $WATCH_LOG_PATH (trigger_on=$WATCH_TRIGGER_ON timeout=${WATCH_TIMEOUT_SECONDS}s auto_arm=$WATCH_AUTO_ARM_STRICT_PROOF armed_window=${ARMED_WINDOW_SECONDS}s)"

    while true; do
        if [[ -f "$WATCH_LOG_PATH" ]]; then
            eval "$(collect_target_trace_metrics "$WATCH_LOG_PATH" "/dev/null")"
            info "watcher target_hits=$TARGET_EXACT_DIRECTION_HITS route_seen=$TARGET_ROUTE_SEEN_HITS quote_seen=$TARGET_QUOTE_REQUEST_SEEN_HITS quote_matched=$TARGET_QUOTE_MATCHED_HITS public_route=$TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS non_public_route=$TARGET_NON_PUBLIC_ROUTE_HITS blocked=$TARGET_PUBLIC_SCOUT_BLOCKED_HITS"
            if watcher_trigger_reached; then
                elapsed_seconds=$((SECONDS - watcher_started_at))
                write_watcher_summary "TRIGGERED" "$elapsed_seconds"
                if is_true "$WATCH_AUTO_ARM_STRICT_PROOF"; then
                    info "Watcher triggered; launching strict proof for ${ARMED_WINDOW_SECONDS}s"
                    export SHADOW_RUN29C_MODE="strict-proof"
                    export SHADOW_RUN29C_TRIGGERED_FROM_WATCHER="true"
                    export SHADOW_RUN29C_STEP="${STEP}${AUTO_ARM_STEP_SUFFIX}"
                    exec "$SCRIPT_PATH" "$ARMED_WINDOW_SECONDS"
                fi
                return 0
            fi
        else
            info "Waiting for watcher log $WATCH_LOG_PATH"
        fi

        if (( WATCH_TIMEOUT_SECONDS > 0 )) && (( SECONDS - watcher_started_at >= WATCH_TIMEOUT_SECONDS )); then
            elapsed_seconds=$((SECONDS - watcher_started_at))
            eval "$(collect_target_trace_metrics "$WATCH_LOG_PATH" "/dev/null")"
            write_watcher_summary "NO_TARGET_HIT" "$elapsed_seconds"
            fail "Watcher timed out without target recurrence"
            exit 1
        fi

        sleep "$POLL_SECONDS"
    done
}

run_watcher_armed_loop() {
    local watcher_started_at elapsed_seconds=0
    local worker_records_seen_current worker_pool_state_hit_current
    local worker_route_actionable_current worker_route_resolved_public_rpc_scout_current
    local run38_target_public_scout_clmm_route_resolved_current
    local run38_target_public_scout_clmm_r1_attempt_current
    local worker_records_seen_delta worker_pool_state_hit_delta
    local worker_route_actionable_delta worker_route_resolved_public_rpc_scout_delta
    local run38_target_public_scout_clmm_route_resolved_delta
    local run38_target_public_scout_clmm_r1_attempt_delta
    watcher_started_at=$SECONDS

    capture_watcher_arm_baseline

    info "Watcher-armed Run38 waiting up to ${WATCHER_ARM_TIMEOUT_SECONDS}s for target-class route or R1 deltas"

    while true; do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited before watcher-armed proof could start"
            tail -n 80 "$TIER0_LOG" | tee "$TIER0_TAIL_FILE"
            exit 1
        fi

        worker_records_seen_current="$(live_gate_metric_int "worker_records_seen")"
        worker_pool_state_hit_current="$(live_gate_metric_int "worker_pool_state_hit")"
        worker_route_actionable_current="$(live_gate_metric_int "worker_route_actionable")"
        worker_route_resolved_public_rpc_scout_current="$(live_gate_metric_int "worker_route_resolved_public_rpc_scout")"
        run38_target_public_scout_clmm_route_resolved_current="$(live_gate_metric_int "run38_target_public_scout_clmm_route_resolved")"
        run38_target_public_scout_clmm_r1_attempt_current="$(live_gate_metric_int "run38_target_public_scout_clmm_r1_attempt")"

        worker_records_seen_delta=$((worker_records_seen_current - baseline_worker_records_seen))
        worker_pool_state_hit_delta=$((worker_pool_state_hit_current - baseline_worker_pool_state_hit))
        worker_route_actionable_delta=$((worker_route_actionable_current - baseline_worker_route_actionable))
        worker_route_resolved_public_rpc_scout_delta=$((worker_route_resolved_public_rpc_scout_current - baseline_worker_route_resolved_public_rpc_scout))
        run38_target_public_scout_clmm_route_resolved_delta=$((run38_target_public_scout_clmm_route_resolved_current - baseline_run38_target_public_scout_clmm_route_resolved))
        run38_target_public_scout_clmm_r1_attempt_delta=$((run38_target_public_scout_clmm_r1_attempt_current - baseline_run38_target_public_scout_clmm_r1_attempt))

        elapsed_seconds=$((SECONDS - watcher_started_at))
        info "watcher_armed wait=${elapsed_seconds}s records_delta=$worker_records_seen_delta pool_hit_delta=$worker_pool_state_hit_delta route_actionable_delta=$worker_route_actionable_delta public_route_delta=$worker_route_resolved_public_rpc_scout_delta target_route_delta=$run38_target_public_scout_clmm_route_resolved_delta target_r1_attempt_delta=$run38_target_public_scout_clmm_r1_attempt_delta"

        if (( run38_target_public_scout_clmm_route_resolved_delta > 0 )); then
            WATCHER_ARM_REASON="target_held_input_public_scout_clmm_route_resolved"
            WATCHER_ARM_WAIT_SECONDS=$elapsed_seconds
            break
        fi

        if (( run38_target_public_scout_clmm_r1_attempt_delta > 0 )); then
            WATCHER_ARM_REASON="target_held_input_public_scout_clmm_r1_attempt"
            WATCHER_ARM_WAIT_SECONDS=$elapsed_seconds
            break
        fi

        if (( elapsed_seconds >= WATCHER_ARM_TIMEOUT_SECONDS )); then
            WATCHER_ARM_REASON="watch_timeout_no_target"
            WATCHER_ARM_WAIT_SECONDS=$elapsed_seconds
            RUN37_OUTCOME="none"
            info "Watcher-armed Run38 timed out before any target-class arm condition appeared"
            return 0
        fi

        sleep "$WATCHER_ARM_POLL_SECONDS"
    done

    info "Watcher-armed Run38 armed reason=$WATCHER_ARM_REASON wait=${WATCHER_ARM_WAIT_SECONDS}s; starting ${WINDOW_SECONDS}s proof window"
    run_one_shot_loop
}

classify_strict_proof_outcome() {
    if ! is_true "$REQUIRE_TARGET_PUBLIC_SCOUT"; then
        if is_true "$ONE_SHOT_MODE"; then
            if (( worker_route_resolved_public_rpc_scout > 0 )) \
                && [[ "$public_scout_attempt_latch_set" == "1" ]] \
                && (( public_rpc_transport_attempt == 1 )) \
                && (( public_rpc_transport_failure >= 1 || public_rpc_transport_rejected >= 1 || public_rpc_nack_recorded >= 1 )) \
                && (( worker_pm_reservation_created >= 1 )) \
                && (( pm_reservation_released >= worker_pm_reservation_created )) \
                && [[ "$fee_leak_accounted" == "true" ]] \
                && [[ "$inventory_truth_ok" == "true" ]] \
                && [[ "$phantom_inventory_risk" != "true" ]] \
                && [[ "$live_landed" == "0" ]]; then
                printf 'PASS\n'
            elif (( public_rpc_transport_attempt == 0 )) \
                && (( worker_candidates_entered_process_intent == 0 )) \
                && [[ "$public_scout_attempt_latch_set" != "1" ]]; then
                printf 'NO_QUALIFYING_HUNT\n'
            else
                printf 'FAIL\n'
            fi
            return 0
        fi
        printf '%s\n' "$RUN37_OUTCOME"
        return 0
    fi

    if (( TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS > 0 )); then
        if (( TARGET_WOULD_BUILD_HITS > 0 )) \
            && (( TARGET_WOULD_SIGN_HITS > 0 )) \
            && (( TARGET_WOULD_SEND_HITS > 0 || worker_rollout_would_send_events_published > 0 )) \
            && (( public_rpc_transport_attempt > 0 || public_rpc_transport_failure > 0 || public_rpc_nack_recorded > 0 || rollout_public_rpc_no_send_would_sends > 0 || worker_public_rpc_scout_emit_ok > 0 )) \
            && (( pm_reservation_released >= worker_pm_reservation_created )) \
            && [[ "$fee_leak_accounted" == "true" ]] \
            && [[ "$inventory_truth_ok" == "true" ]] \
            && [[ "$phantom_inventory_risk" != "true" ]] \
            && [[ "$live_landed" == "0" ]]; then
            printf 'PASS\n'
        else
            printf 'FAIL\n'
        fi
        return 0
    fi

    if (( TARGET_EXACT_DIRECTION_HITS == 0 )); then
        if is_true "$TRIGGERED_FROM_WATCHER"; then
            printf 'NO_SECOND_HIT\n'
        else
            printf 'NO_TARGET_HIT\n'
        fi
        return 0
    fi

    if (( TARGET_QUOTE_MATCHED_HITS > 0 )) && (( TARGET_NON_PUBLIC_ROUTE_HITS > 0 )); then
        printf 'TARGET_ROUTED_NON_PUBLIC\n'
        return 0
    fi

    printf 'FAIL\n'
}

classify_watcher_armed_outcome() {
    local target_reject_total=0
    target_reject_total=$((run38_target_public_scout_clmm_reject_proof_filter_delta \
        + run38_target_public_scout_clmm_reject_micro_market_delta \
        + run38_target_public_scout_clmm_reject_inventory_delta \
        + run38_target_public_scout_clmm_reject_template_executability_delta \
        + run38_target_public_scout_clmm_reject_attempt_latch_delta))

    if (( worker_records_seen_delta > 0 )) \
        && (( run38_target_public_scout_clmm_route_resolved_delta == 0 )) \
        && (( run38_target_public_scout_clmm_r1_attempt_delta == 0 )); then
        printf 'TARGET_NO_HELD_INPUT_RECURRENCE\n'
        return 0
    fi

    if ((( run38_target_public_scout_clmm_route_resolved_delta > 0 )) \
            || (( run38_target_public_scout_clmm_r1_attempt_delta > 0 ))) \
        && (( run38_target_public_scout_clmm_admit_delta == 0 )) \
        && ((( target_reject_total > 0 )) || [[ -n "$last_run38_target_reject_reason" ]]); then
        printf 'TARGET_HELD_INPUT_FILTERED\n'
        return 0
    fi

    if (( run38_target_public_scout_clmm_admit_delta > 0 )) \
        && (( run38_target_public_scout_clmm_process_intent_delta == 0 )); then
        printf 'TARGET_ADMITTED_BUT_NOT_PROCESSING\n'
        return 0
    fi

    if (( run38_target_public_scout_clmm_process_intent_delta > 0 )) \
        && (( run38_target_public_scout_clmm_transport_attempt_delta == 0 )); then
        printf 'TARGET_PROCESSING_BUT_NO_TRANSPORT\n'
        return 0
    fi

    if (( run38_target_public_scout_clmm_process_intent_delta > 0 )) \
        && (( run38_target_public_scout_clmm_emit_attempt_delta > 0 )) \
        && (( run38_target_public_scout_clmm_transport_attempt_delta > 0 )) \
        && ((( public_rpc_transport_failure_delta > 0 )) || (( public_rpc_transport_rejected_delta > 0 )) || (( public_rpc_nack_recorded_delta > 0 ))) \
        && (( pm_reservation_released_delta > 0 )) \
        && (( live_landed_delta == 0 )); then
        printf 'RUN38_PASS_BLACKHOLE\n'
        return 0
    fi

    printf 'UNCLASSIFIED\n'
}

run_one_shot_loop() {
    refresh_one_shot_report_fields() {
        live_landed="$(json_field "$GATE_REPORT" "live_landed")"
        worker_health_drops="$(json_field "$GATE_REPORT" "worker_health_drops")"
        entered_process_intent="$(json_field "$GATE_REPORT" "worker_candidates_entered_process_intent")"
        worker_emit_ok="$(json_field "$GATE_REPORT" "worker_public_rpc_scout_emit_ok")"
        pm_created="$(json_field "$GATE_REPORT" "worker_pm_reservation_created")"
        enqueued="$(json_field "$GATE_REPORT" "public_rpc_scout_enqueued")"
        transport_attempt="$(json_field "$GATE_REPORT" "public_rpc_transport_attempt")"
        transport_failure="$(json_field "$GATE_REPORT" "public_rpc_transport_failure")"
        transport_rejected="$(json_field "$GATE_REPORT" "public_rpc_transport_rejected")"
        nack_recorded="$(json_field "$GATE_REPORT" "public_rpc_nack_recorded")"
        pm_released="$(json_field "$GATE_REPORT" "pm_reservation_released")"
        fee_leak_accounted="$(json_field "$GATE_REPORT" "fee_leak_accounted")"
        inventory_truth_ok="$(json_field "$GATE_REPORT" "inventory_truth_ok")"
        phantom_inventory_risk="$(json_field "$GATE_REPORT" "phantom_inventory_risk")"
        kill_switch_required="$(json_field "$GATE_REPORT" "kill_switch_required")"
        public_scout_gates_green="$(json_field "$GATE_REPORT" "public_scout_gates_green")"
        public_scout_attempt_latch_set="$(json_field "$GATE_REPORT" "public_scout_attempt_latch_set")"
        public_scout_attempt_latch_drop_count="$(json_field "$GATE_REPORT" "public_scout_attempt_latch_drop_count")"
        public_scout_attempt_latch_first_signature="$(json_field "$GATE_REPORT" "public_scout_attempt_latch_first_signature")"
    }

    validate_one_shot_final_report() {
        local outcome="$1"
        sleep "$FINAL_SETTLE_DELAY_SECONDS"
        refresh_one_shot_report_fields
        info "one_shot_final outcome=$outcome entered=$entered_process_intent landed=$live_landed emit_ok=$worker_emit_ok enqueued=$enqueued attempts=$transport_attempt failure=$transport_failure rejected=$transport_rejected nack=$nack_recorded pm_created=$pm_created pm_released=$pm_released latch_set=$public_scout_attempt_latch_set latch_drop=$public_scout_attempt_latch_drop_count"

        if [[ "$inventory_truth_ok" != "true" || "$fee_leak_accounted" != "true" || "$phantom_inventory_risk" == "true" ]]; then
            fail "one-shot final report failed safety truth (inventory_truth_ok=$inventory_truth_ok fee_leak_accounted=$fee_leak_accounted phantom_inventory_risk=$phantom_inventory_risk)"
            exit 1
        fi
        if [[ "$kill_switch_required" == "true" ]]; then
            fail "one-shot final report requires kill switch"
            exit 1
        fi
        if [[ "$public_scout_gates_green" != "true" ]]; then
            fail "one-shot final report lost public-scout gate green status"
            exit 1
        fi
        if [[ "$public_scout_attempt_latch_set" != "1" ]]; then
            fail "one-shot final report missing public-scout attempt latch set"
            exit 1
        fi
        if (( worker_emit_ok > 1 )); then
            fail "one-shot final report exceeded the single-attempt emit envelope"
            exit 1
        fi
        if [[ "$outcome" != "landed" ]] && (( pm_released < pm_created )); then
            fail "one-shot final report reached $outcome without PM release convergence"
            exit 1
        fi
    }

    local deadline attempt_started=false attempt_started_at=0 outcome="none"
    deadline=$((SECONDS + WINDOW_SECONDS_INT))

    while (( SECONDS < deadline )); do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited before the one-shot proof completed"
            tail -n 80 "$TIER0_LOG" | tee "$TIER0_TAIL_FILE"
            exit 1
        fi
        if [[ ! -f "$GATE_REPORT" ]]; then
            info "Waiting for $GATE_REPORT"
            sleep "$POLL_SECONDS"
            continue
        fi

        refresh_one_shot_report_fields

        info "one_shot entered=$entered_process_intent landed=$live_landed emit_ok=$worker_emit_ok enqueued=$enqueued attempts=$transport_attempt failure=$transport_failure rejected=$transport_rejected nack=$nack_recorded pm_created=$pm_created pm_released=$pm_released latch_set=$public_scout_attempt_latch_set latch_drop=$public_scout_attempt_latch_drop_count"

        if (( worker_emit_ok > 1 || pm_created > 1 || enqueued > 1 || transport_attempt > 1 )); then
            RUN37_OUTCOME="failure"
            fail "one-shot live proof exceeded the single-attempt envelope"
            exit 1
        fi

        if (( worker_emit_ok > 0 || pm_created > 0 || enqueued > 0 || transport_attempt > 0 || live_landed > 0 )); then
            if [[ "$attempt_started" != "true" ]]; then
                attempt_started=true
                attempt_started_at=$SECONDS
            fi
        fi

        if (( live_landed > 0 )); then
            outcome="landed"
        elif (( transport_failure > 0 || transport_rejected > 0 || nack_recorded > 0 || pm_released > 0 )); then
            outcome="failure"
        fi

        if [[ "$outcome" != "none" ]]; then
            RUN37_OUTCOME="$outcome"
            validate_one_shot_final_report "$outcome"
            return 0
        fi

        if [[ "$attempt_started" == "true" ]] && (( SECONDS - attempt_started_at >= RESULT_GRACE_SECONDS )); then
            RUN37_OUTCOME="failure"
            fail "one-shot live proof saw a scout attempt without a terminal landed/NACK outcome"
            exit 1
        fi

        sleep "$POLL_SECONDS"
    done

    RUN37_OUTCOME="timeout"
    if [[ "$RUN_MODE" == "watcher_armed" ]]; then
        info "one-shot live proof timed out without any scout attempt; deferring final classification to watcher-armed summary"
        return 0
    fi
    fail "one-shot live proof timed out without any scout attempt"
    exit 1
}

if [[ "$RUN_MODE" == "watcher" ]]; then
    assert_cmd_exists python3
    run_watcher_mode
    exit 0
fi

stop_existing_tier0() {
    local pids
    pids="$(pgrep -f 'shadow-tier0 .*config' || true)"
    if [[ -n "$pids" ]]; then
        info "Stopping existing shadow-tier0: $pids"
        while read -r pid; do
            [[ -n "$pid" ]] || continue
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        done <<<"$pids"
    fi
}

require_file "$CONFIG_TEMPLATE"
require_file "$SIGNER_TOKEN_INVENTORY_PATH"
require_executable "$TIER0_BIN"
require_executable "$ORACLE_BIN"
assert_cmd_exists python3
case "$PRIVATE_PACKET_BACKEND" in
    afxdp)
        guard_management_nic_xdp
        ;;
    dpdk)
        guard_management_nic_dpdk
        ;;
    *)
        fail "Unsupported SHADOW_RUN29C_PRIVATE_PACKET_BACKEND=$PRIVATE_PACKET_BACKEND"
        exit 1
        ;;
esac
stop_existing_tier0
rm -f "$GATE_INPUT" "$GATE_REPORT" "$TIER0_LOG" "$SUMMARY_FILE" "$TIER0_TAIL_FILE"
find /dev/shm -maxdepth 1 -name 'shadow_rollout_shadow*' -delete 2>/dev/null || true
rm -f /dev/shm/shadow_control_flags
: >"$SHOCK_RESEARCH_PATH"

write_public_proof_config
resolve_shred_capture_workers
AFXDP_BIND_MODE="$(resolve_afxdp_bind_mode)"
if [[ "$PRIVATE_PACKET_BACKEND" == "afxdp" ]] && is_true "$SAFE_MODE"; then
    validate_safe_profile "$AFXDP_BIND_MODE"
fi

if is_true "$EXPECT_BLACKHOLE_ENDPOINT"; then
    if ! awk '
        /^[[:space:]]*public_rpc:[[:space:]]*$/ { in_public_rpc=1; next }
        in_public_rpc && /^[^[:space:]]/ { in_public_rpc=0 }
        in_public_rpc && /endpoint:[[:space:]]*"https:\/\/127\.0\.0\.1:1"/ { found=1 }
        END { exit(found ? 0 : 1) }
    ' "$CONFIG_TMP"; then
        fail "Generated config does not blackhole public_rpc.endpoint"
        exit 1
    fi
else
    if awk '
        /^[[:space:]]*public_rpc:[[:space:]]*$/ { in_public_rpc=1; next }
        in_public_rpc && /^[^[:space:]]/ { in_public_rpc=0 }
        in_public_rpc && /endpoint:[[:space:]]*"https:\/\/127\.0\.0\.1:1"/ { found=1 }
        END { exit(found ? 0 : 1) }
    ' "$CONFIG_TMP"; then
        fail "Generated config still points public_rpc.endpoint at the blackhole"
        exit 1
    fi
fi
if ! grep -q 'CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK' "$CONFIG_TMP"; then
    fail "Generated config is missing Raydium CLMM pool_program"
    exit 1
fi
if ! grep -q 'pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA' "$CONFIG_TMP"; then
    warn "Generated config is missing PumpSwap pool_program"
fi
if is_true "$VALIDATE_PUBLIC_SCOUT_PROFILE"; then
    validate_public_scout_profile
fi

seed_shadowguard_preflight
seed_startup_gate_report

info "Running static config validation"
"$TIER0_BIN" --check-config "$CONFIG_TMP" | tee "$CONFIG_CHECK_FILE"

info "Ensuring $PRIVATE_PACKET_BACKEND private packet path is live"
ensure_private_packet_path

if ! pgrep -x shadow-oracle >/dev/null 2>&1; then
    info "shadow-oracle is not running; starting publisher for tip LUT shm segments"
    (
        cd "$PACK_DIR"
        if [[ -n "$ORACLE_CPU_AFFINITY" ]]; then
            assert_cmd_exists taskset
            info "Pinning shadow-oracle to CPU(s) $ORACLE_CPU_AFFINITY"
            exec taskset -c "$ORACLE_CPU_AFFINITY" "$ORACLE_BIN" "$CONFIG_TMP" >"$ORACLE_LOG" 2>&1
        fi
        exec "$ORACLE_BIN" "$CONFIG_TMP" >"$ORACLE_LOG" 2>&1
    ) &
    ORACLE_PID=$!
else
    info "shadow-oracle already running; reusing existing tip LUT publisher"
    pin_process_if_configured "shadow-oracle" "$(pgrep -xo shadow-oracle || true)" "$ORACLE_CPU_AFFINITY"
fi

if ! wait_for_path /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_base did not appear within ${READINESS_TIMEOUT}s"
    exit 1
fi
if ! wait_for_path /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_jito did not appear within ${READINESS_TIMEOUT}s"
    exit 1
fi
if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_base did not publish an active bank"
    exit 1
fi
if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT"; then
    fail "shadow_tip_lut_jito did not publish an active bank"
    exit 1
fi
if ! wait_for_path /dev/shm/shadow_leader_schedule "$READINESS_TIMEOUT"; then
    fail "shadow_leader_schedule is missing"
    exit 1
fi

info "Starting shadow-tier0 for ${WINDOW_SECONDS}s"
(
    cd "$PACK_DIR"
    export SHADOW_SIGNER_TOKEN_INVENTORY_PATH="$SIGNER_TOKEN_INVENTORY_PATH"
    if [[ -n "$ALLOW_ONLY_VENUE_ID" ]]; then
        export SHADOW_RUN42_ALLOW_ONLY_VENUE_ID="$ALLOW_ONLY_VENUE_ID"
    fi
    if [[ -n "$ALLOW_ONLY_POOL" ]]; then
        export SHADOW_RUN42_ALLOW_ONLY_POOL="$ALLOW_ONLY_POOL"
    fi
    if [[ -n "$ALLOW_ONLY_MINT_IN" ]]; then
        export SHADOW_RUN42_ALLOW_ONLY_MINT_IN="$ALLOW_ONLY_MINT_IN"
    fi
    if [[ -n "$ALLOW_ONLY_MINT_OUT" ]]; then
        export SHADOW_RUN42_ALLOW_ONLY_MINT_OUT="$ALLOW_ONLY_MINT_OUT"
    fi
    export SHADOW_RUN42_HELD_INPUT_MINTS="$HELD_INPUT_MINTS"
    export SHADOW_RUN42_REQUIRE_TARGET_PUBLIC_SCOUT="$REQUIRE_TARGET_PUBLIC_SCOUT"
    if [[ -n "$TIER0_CPU_AFFINITY" ]]; then
        assert_cmd_exists taskset
        info "Pinning shadow-tier0 to CPU(s) $TIER0_CPU_AFFINITY"
        exec taskset -c "$TIER0_CPU_AFFINITY" "$TIER0_BIN" --config "$CONFIG_TMP" >"$TIER0_LOG" 2>&1
    fi
    exec "$TIER0_BIN" --config "$CONFIG_TMP" >"$TIER0_LOG" 2>&1
) &
TIER0_PID=$!

endpoint_seen=false
endpoint_deadline=$((SECONDS + READINESS_TIMEOUT))
if is_true "$REQUIRE_RUNTIME_ENDPOINT_SEEN"; then
    while (( SECONDS < endpoint_deadline )); do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited before public RPC scout startup completed"
            tail -n 80 "$TIER0_LOG" | tee "$TIER0_TAIL_FILE"
            exit 1
        fi
        if grep -q "$PUBLIC_RPC_ENDPOINT" "$TIER0_LOG" 2>/dev/null; then
            endpoint_seen=true
            break
        fi
        sleep 1
    done
    if [[ "$endpoint_seen" != "true" ]]; then
        fail "Runtime log never showed the configured public RPC endpoint"
        tail -n 80 "$TIER0_LOG" | tee "$TIER0_TAIL_FILE"
        exit 1
    fi
else
    endpoint_seen="skipped"
fi

RUN37_OUTCOME="none"
if is_true "$ONE_SHOT_MODE"; then
    if [[ "$RUN_MODE" == "watcher_armed" ]]; then
        run_watcher_armed_loop
    else
        run_one_shot_loop
    fi
else
    run_window_loop
fi

capture_final_live_diagnostics

kill "$TIER0_PID" 2>/dev/null || true
wait "$TIER0_PID" 2>/dev/null || true

sample_slots="$(gate_metric "sample_slots")"
observe_candidates="$(gate_metric "observe_candidates")"
live_landed="$(gate_metric "live_landed")"
worker_records_seen="$(gate_metric "worker_records_seen")"
worker_pool_state_hit="$(gate_metric "worker_pool_state_hit")"
worker_route_actionable="$(gate_metric "worker_route_actionable")"
worker_route_resolved_drop="$(gate_metric "worker_route_resolved_drop")"
worker_candidates_entered_process_intent="$(gate_metric "worker_candidates_entered_process_intent")"
worker_route_resolved_public_rpc_scout="$(gate_metric "worker_route_resolved_public_rpc_scout")"
run38_target_public_scout_clmm_route_resolved="$(gate_metric "run38_target_public_scout_clmm_route_resolved")"
run38_target_public_scout_clmm_r1_attempt="$(gate_metric "run38_target_public_scout_clmm_r1_attempt")"
run38_target_public_scout_clmm_reject_proof_filter="$(gate_metric "run38_target_public_scout_clmm_reject_proof_filter")"
run38_target_public_scout_clmm_reject_micro_market="$(gate_metric "run38_target_public_scout_clmm_reject_micro_market")"
run38_target_public_scout_clmm_reject_inventory="$(gate_metric "run38_target_public_scout_clmm_reject_inventory")"
run38_target_public_scout_clmm_reject_inventory_missing_input_ata="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_missing_input_ata")"
run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized")"
run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient")"
run38_target_public_scout_clmm_reject_inventory_token_program_mismatch="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch")"
run38_target_public_scout_clmm_reject_inventory_other="$(gate_metric "run38_target_public_scout_clmm_reject_inventory_other")"
run38_target_public_scout_clmm_reject_template_executability="$(gate_metric "run38_target_public_scout_clmm_reject_template_executability")"
run38_target_public_scout_clmm_reject_attempt_latch="$(gate_metric "run38_target_public_scout_clmm_reject_attempt_latch")"
run38_target_public_scout_clmm_admit="$(gate_metric "run38_target_public_scout_clmm_admit")"
run38_target_public_scout_clmm_process_intent="$(gate_metric "run38_target_public_scout_clmm_process_intent")"
run38_target_public_scout_clmm_emit_attempt="$(gate_metric "run38_target_public_scout_clmm_emit_attempt")"
run38_target_public_scout_clmm_transport_attempt="$(gate_metric "run38_target_public_scout_clmm_transport_attempt")"
run38_target_public_scout_clmm_pm_created="$(gate_metric "run38_target_public_scout_clmm_pm_created")"
worker_rollout_would_send_events_published="$(gate_metric "worker_rollout_would_send_events_published")"
worker_rollout_live_send_suppressed_events_published="$(gate_metric "worker_rollout_live_send_suppressed_events_published")"
worker_pm_reservation_created="$(gate_metric "worker_pm_reservation_created")"
worker_public_rpc_scout_emit_attempt="$(gate_metric "worker_public_rpc_scout_emit_attempt")"
worker_public_rpc_scout_emit_ok="$(gate_metric "worker_public_rpc_scout_emit_ok")"
public_rpc_scout_enqueued="$(gate_metric "public_rpc_scout_enqueued")"
public_rpc_transport_attempt="$(gate_metric "public_rpc_transport_attempt")"
public_rpc_transport_failure="$(gate_metric "public_rpc_transport_failure")"
public_rpc_transport_rejected="$(gate_metric "public_rpc_transport_rejected")"
public_rpc_nack_recorded="$(gate_metric "public_rpc_nack_recorded")"
pm_reservation_released="$(gate_metric "pm_reservation_released")"
public_scout_attempt_latch_set="$(gate_metric "public_scout_attempt_latch_set")"
public_scout_attempt_latch_drop_count="$(gate_metric "public_scout_attempt_latch_drop_count")"
public_scout_attempt_latch_first_signature="$(gate_metric "public_scout_attempt_latch_first_signature")"
fee_leak_accounted="$(gate_metric "fee_leak_accounted")"
inventory_truth_ok="$(gate_metric "inventory_truth_ok")"
phantom_inventory_risk="$(gate_metric "phantom_inventory_risk")"
kill_switch_required="$(gate_metric "kill_switch_required")"
public_scout_gates_green="$(gate_metric "public_scout_gates_green")"
r1_auto_admission_attempt="$(gate_metric "r1_auto_admission_attempt")"
r1_auto_admission_admit="$(gate_metric "r1_auto_admission_admit")"
r1_auto_admission_reject_micro_market="$(gate_metric "r1_auto_admission_reject_micro_market")"
r1_auto_admission_reject_proof_filter="$(gate_metric "r1_auto_admission_reject_proof_filter")"
r1_auto_admission_reject_template_executability="$(gate_metric "r1_auto_admission_reject_template_executability")"
r1_auto_admission_reject_attempt_latch="$(gate_metric "r1_auto_admission_reject_attempt_latch")"
r1_auto_admission_reject_proof_filter_no_quote="$(gate_metric "r1_auto_admission_reject_proof_filter_no_quote")"
r1_auto_admission_reject_proof_filter_no_positive_ev="$(gate_metric "r1_auto_admission_reject_proof_filter_no_positive_ev")"
r1_auto_admission_reject_proof_filter_no_shadow_guard="$(gate_metric "r1_auto_admission_reject_proof_filter_no_shadow_guard")"
r1_auto_admission_reject_proof_filter_public_scout_shape="$(gate_metric "r1_auto_admission_reject_proof_filter_public_scout_shape")"
r1_auto_admission_reject_proof_filter_other="$(gate_metric "r1_auto_admission_reject_proof_filter_other")"
r1_auto_admission_reject_public_scout_shape_route_not_public_scout="$(gate_metric "r1_auto_admission_reject_public_scout_shape_route_not_public_scout")"
r1_auto_admission_reject_public_scout_shape_wrong_source="$(gate_metric "r1_auto_admission_reject_public_scout_shape_wrong_source")"
r1_auto_admission_reject_public_scout_shape_wrong_venue="$(gate_metric "r1_auto_admission_reject_public_scout_shape_wrong_venue")"
r1_auto_admission_reject_public_scout_shape_missing_template_family="$(gate_metric "r1_auto_admission_reject_public_scout_shape_missing_template_family")"
r1_auto_admission_reject_public_scout_shape_route_template_mismatch="$(gate_metric "r1_auto_admission_reject_public_scout_shape_route_template_mismatch")"
r1_auto_admission_reject_public_scout_shape_other="$(gate_metric "r1_auto_admission_reject_public_scout_shape_other")"
r1_auto_admission_reject_micro_market_no_uplift="$(gate_metric "r1_auto_admission_reject_micro_market_no_uplift")"
r1_auto_admission_reject_micro_market_negative_ev="$(gate_metric "r1_auto_admission_reject_micro_market_negative_ev")"
r1_auto_admission_reject_micro_market_below_floor="$(gate_metric "r1_auto_admission_reject_micro_market_below_floor")"
r1_auto_admission_reject_micro_market_other="$(gate_metric "r1_auto_admission_reject_micro_market_other")"
last_public_scout_shape_reject_reason="$(gate_metric "last_public_scout_shape_reject_reason")"
last_public_scout_shape_reject_pool="$(gate_metric "last_public_scout_shape_reject_pool")"
last_public_scout_shape_reject_mint_in="$(gate_metric "last_public_scout_shape_reject_mint_in")"
last_public_scout_shape_reject_mint_out="$(gate_metric "last_public_scout_shape_reject_mint_out")"
last_public_scout_shape_reject_route="$(gate_metric "last_public_scout_shape_reject_route")"
last_public_scout_shape_reject_source="$(gate_metric "last_public_scout_shape_reject_source")"
last_public_scout_shape_reject_venue="$(gate_metric "last_public_scout_shape_reject_venue")"
last_run38_target_pool="$(gate_metric "last_run38_target_pool")"
last_run38_target_mint_in="$(gate_metric "last_run38_target_mint_in")"
last_run38_target_mint_out="$(gate_metric "last_run38_target_mint_out")"
last_run38_target_route="$(gate_metric "last_run38_target_route")"
last_run38_target_source="$(gate_metric "last_run38_target_source")"
last_run38_target_venue="$(gate_metric "last_run38_target_venue")"
last_run38_target_stage="$(gate_metric "last_run38_target_stage")"
last_run38_target_reject_reason="$(gate_metric "last_run38_target_reject_reason")"
last_run38_target_inventory_reject_reason="$(gate_metric "last_run38_target_inventory_reject_reason")"
last_run38_target_inventory_pool="$(gate_metric "last_run38_target_inventory_pool")"
last_run38_target_inventory_mint_in="$(gate_metric "last_run38_target_inventory_mint_in")"
last_run38_target_inventory_mint_out="$(gate_metric "last_run38_target_inventory_mint_out")"
last_run38_target_inventory_input_ata="$(gate_metric "last_run38_target_inventory_input_ata")"
last_run38_target_inventory_input_balance="$(gate_metric "last_run38_target_inventory_input_balance")"
last_run38_target_inventory_required_input="$(gate_metric "last_run38_target_inventory_required_input")"
last_run38_target_inventory_token_program="$(gate_metric "last_run38_target_inventory_token_program")"
r1_auto_admission_admit_to_process_intent_attempt="$(gate_metric "r1_auto_admission_admit_to_process_intent_attempt")"
r1_auto_admission_admit_to_process_intent_ok="$(gate_metric "r1_auto_admission_admit_to_process_intent_ok")"
r1_auto_admission_admit_to_process_intent_drop="$(gate_metric "r1_auto_admission_admit_to_process_intent_drop")"
last_admit_to_process_intent_drop_reason="$(gate_metric "last_admit_to_process_intent_drop_reason")"
last_admit_to_process_intent_pool="$(gate_metric "last_admit_to_process_intent_pool")"
last_admit_to_process_intent_mint_in="$(gate_metric "last_admit_to_process_intent_mint_in")"
last_admit_to_process_intent_mint_out="$(gate_metric "last_admit_to_process_intent_mint_out")"
last_admit_to_process_intent_route="$(gate_metric "last_admit_to_process_intent_route")"
last_admit_to_process_intent_source="$(gate_metric "last_admit_to_process_intent_source")"
rollout_events_observed="$(gate_metric "rollout_events_observed")"
rollout_public_rpc_no_send_would_sends="$(gate_metric "rollout_public_rpc_no_send_would_sends")"
shadow_guard_mainnet_deployed="$(gate_metric "shadow_guard_mainnet_deployed")"
shadow_guard_template_program_id_matches="$(gate_metric "shadow_guard_template_program_id_matches")"

baseline_worker_records_seen="${baseline_worker_records_seen:-0}"
final_worker_records_seen="$worker_records_seen"
worker_records_seen_delta=0
worker_pool_state_hit_delta=0
worker_route_actionable_delta=0
worker_route_resolved_public_rpc_scout_delta=0
run38_target_public_scout_clmm_route_resolved_delta=0
run38_target_public_scout_clmm_r1_attempt_delta=0
public_scout_attempt_latch_set_delta=0
public_scout_attempt_latch_drop_count_delta=0
worker_candidates_entered_process_intent_delta=0
worker_public_rpc_scout_emit_attempt_delta=0
run38_target_public_scout_clmm_reject_proof_filter_delta=0
run38_target_public_scout_clmm_reject_micro_market_delta=0
run38_target_public_scout_clmm_reject_inventory_delta=0
run38_target_public_scout_clmm_reject_inventory_missing_input_ata_delta=0
run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized_delta=0
run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient_delta=0
run38_target_public_scout_clmm_reject_inventory_token_program_mismatch_delta=0
run38_target_public_scout_clmm_reject_inventory_other_delta=0
run38_target_public_scout_clmm_reject_template_executability_delta=0
run38_target_public_scout_clmm_reject_attempt_latch_delta=0
run38_target_public_scout_clmm_admit_delta=0
run38_target_public_scout_clmm_process_intent_delta=0
run38_target_public_scout_clmm_emit_attempt_delta=0
run38_target_public_scout_clmm_transport_attempt_delta=0
run38_target_public_scout_clmm_pm_created_delta=0
public_rpc_scout_enqueued_delta=0
public_rpc_transport_attempt_delta=0
public_rpc_transport_failure_delta=0
public_rpc_transport_rejected_delta=0
public_rpc_nack_recorded_delta=0
worker_pm_reservation_created_delta=0
pm_reservation_released_delta=0
live_landed_delta=0
r1_auto_admission_attempt_delta=0
r1_auto_admission_admit_delta=0
r1_auto_admission_reject_micro_market_delta=0
r1_auto_admission_reject_proof_filter_delta=0
r1_auto_admission_reject_template_executability_delta=0
r1_auto_admission_reject_attempt_latch_delta=0
r1_auto_admission_reject_proof_filter_no_quote_delta=0
r1_auto_admission_reject_proof_filter_no_positive_ev_delta=0
r1_auto_admission_reject_proof_filter_no_shadow_guard_delta=0
r1_auto_admission_reject_proof_filter_public_scout_shape_delta=0
r1_auto_admission_reject_proof_filter_other_delta=0
r1_auto_admission_reject_micro_market_no_uplift_delta=0
r1_auto_admission_reject_micro_market_negative_ev_delta=0
r1_auto_admission_reject_micro_market_below_floor_delta=0
r1_auto_admission_reject_micro_market_other_delta=0
r1_auto_admission_admit_to_process_intent_attempt_delta=0
r1_auto_admission_admit_to_process_intent_ok_delta=0
r1_auto_admission_admit_to_process_intent_drop_delta=0

if [[ "$RUN_MODE" == "watcher_armed" ]] && [[ "$WATCHER_ARM_BASELINE_CAPTURED" == "true" ]]; then
    worker_records_seen_delta="$(metric_delta "$worker_records_seen" "$baseline_worker_records_seen")"
    worker_pool_state_hit_delta="$(metric_delta "$worker_pool_state_hit" "${baseline_worker_pool_state_hit:-0}")"
    worker_route_actionable_delta="$(metric_delta "$worker_route_actionable" "${baseline_worker_route_actionable:-0}")"
    worker_route_resolved_public_rpc_scout_delta="$(metric_delta "$worker_route_resolved_public_rpc_scout" "${baseline_worker_route_resolved_public_rpc_scout:-0}")"
    run38_target_public_scout_clmm_route_resolved_delta="$(metric_delta "$run38_target_public_scout_clmm_route_resolved" "${baseline_run38_target_public_scout_clmm_route_resolved:-0}")"
    run38_target_public_scout_clmm_r1_attempt_delta="$(metric_delta "$run38_target_public_scout_clmm_r1_attempt" "${baseline_run38_target_public_scout_clmm_r1_attempt:-0}")"
    public_scout_attempt_latch_set_delta="$(metric_delta "$public_scout_attempt_latch_set" "${baseline_public_scout_attempt_latch_set:-0}")"
    public_scout_attempt_latch_drop_count_delta="$(metric_delta "$public_scout_attempt_latch_drop_count" "${baseline_public_scout_attempt_latch_drop_count:-0}")"
    worker_candidates_entered_process_intent_delta="$(metric_delta "$worker_candidates_entered_process_intent" "${baseline_worker_candidates_entered_process_intent:-0}")"
    worker_public_rpc_scout_emit_attempt_delta="$(metric_delta "$worker_public_rpc_scout_emit_attempt" "${baseline_worker_public_rpc_scout_emit_attempt:-0}")"
    run38_target_public_scout_clmm_reject_proof_filter_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_proof_filter" "${baseline_run38_target_public_scout_clmm_reject_proof_filter:-0}")"
    run38_target_public_scout_clmm_reject_micro_market_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_micro_market" "${baseline_run38_target_public_scout_clmm_reject_micro_market:-0}")"
    run38_target_public_scout_clmm_reject_inventory_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory" "${baseline_run38_target_public_scout_clmm_reject_inventory:-0}")"
    run38_target_public_scout_clmm_reject_inventory_missing_input_ata_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory_missing_input_ata" "${baseline_run38_target_public_scout_clmm_reject_inventory_missing_input_ata:-0}")"
    run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized" "${baseline_run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized:-0}")"
    run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient" "${baseline_run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient:-0}")"
    run38_target_public_scout_clmm_reject_inventory_token_program_mismatch_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory_token_program_mismatch" "${baseline_run38_target_public_scout_clmm_reject_inventory_token_program_mismatch:-0}")"
    run38_target_public_scout_clmm_reject_inventory_other_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_inventory_other" "${baseline_run38_target_public_scout_clmm_reject_inventory_other:-0}")"
    run38_target_public_scout_clmm_reject_template_executability_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_template_executability" "${baseline_run38_target_public_scout_clmm_reject_template_executability:-0}")"
    run38_target_public_scout_clmm_reject_attempt_latch_delta="$(metric_delta "$run38_target_public_scout_clmm_reject_attempt_latch" "${baseline_run38_target_public_scout_clmm_reject_attempt_latch:-0}")"
    run38_target_public_scout_clmm_admit_delta="$(metric_delta "$run38_target_public_scout_clmm_admit" "${baseline_run38_target_public_scout_clmm_admit:-0}")"
    run38_target_public_scout_clmm_process_intent_delta="$(metric_delta "$run38_target_public_scout_clmm_process_intent" "${baseline_run38_target_public_scout_clmm_process_intent:-0}")"
    run38_target_public_scout_clmm_emit_attempt_delta="$(metric_delta "$run38_target_public_scout_clmm_emit_attempt" "${baseline_run38_target_public_scout_clmm_emit_attempt:-0}")"
    run38_target_public_scout_clmm_transport_attempt_delta="$(metric_delta "$run38_target_public_scout_clmm_transport_attempt" "${baseline_run38_target_public_scout_clmm_transport_attempt:-0}")"
    run38_target_public_scout_clmm_pm_created_delta="$(metric_delta "$run38_target_public_scout_clmm_pm_created" "${baseline_run38_target_public_scout_clmm_pm_created:-0}")"
    public_rpc_scout_enqueued_delta="$(metric_delta "$public_rpc_scout_enqueued" "${baseline_public_rpc_scout_enqueued:-0}")"
    public_rpc_transport_attempt_delta="$(metric_delta "$public_rpc_transport_attempt" "${baseline_public_rpc_transport_attempt:-0}")"
    public_rpc_transport_failure_delta="$(metric_delta "$public_rpc_transport_failure" "${baseline_public_rpc_transport_failure:-0}")"
    public_rpc_transport_rejected_delta="$(metric_delta "$public_rpc_transport_rejected" "${baseline_public_rpc_transport_rejected:-0}")"
    public_rpc_nack_recorded_delta="$(metric_delta "$public_rpc_nack_recorded" "${baseline_public_rpc_nack_recorded:-0}")"
    worker_pm_reservation_created_delta="$(metric_delta "$worker_pm_reservation_created" "${baseline_worker_pm_reservation_created:-0}")"
    pm_reservation_released_delta="$(metric_delta "$pm_reservation_released" "${baseline_pm_reservation_released:-0}")"
    live_landed_delta="$(metric_delta "$live_landed" "${baseline_live_landed:-0}")"
    r1_auto_admission_attempt_delta="$(metric_delta "$r1_auto_admission_attempt" "${baseline_r1_auto_admission_attempt:-0}")"
    r1_auto_admission_admit_delta="$(metric_delta "$r1_auto_admission_admit" "${baseline_r1_auto_admission_admit:-0}")"
    r1_auto_admission_reject_micro_market_delta="$(metric_delta "$r1_auto_admission_reject_micro_market" "${baseline_r1_auto_admission_reject_micro_market:-0}")"
    r1_auto_admission_reject_proof_filter_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter" "${baseline_r1_auto_admission_reject_proof_filter:-0}")"
    r1_auto_admission_reject_template_executability_delta="$(metric_delta "$r1_auto_admission_reject_template_executability" "${baseline_r1_auto_admission_reject_template_executability:-0}")"
    r1_auto_admission_reject_attempt_latch_delta="$(metric_delta "$r1_auto_admission_reject_attempt_latch" "${baseline_r1_auto_admission_reject_attempt_latch:-0}")"
    r1_auto_admission_reject_proof_filter_no_quote_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter_no_quote" "${baseline_r1_auto_admission_reject_proof_filter_no_quote:-0}")"
    r1_auto_admission_reject_proof_filter_no_positive_ev_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter_no_positive_ev" "${baseline_r1_auto_admission_reject_proof_filter_no_positive_ev:-0}")"
    r1_auto_admission_reject_proof_filter_no_shadow_guard_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter_no_shadow_guard" "${baseline_r1_auto_admission_reject_proof_filter_no_shadow_guard:-0}")"
    r1_auto_admission_reject_proof_filter_public_scout_shape_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter_public_scout_shape" "${baseline_r1_auto_admission_reject_proof_filter_public_scout_shape:-0}")"
    r1_auto_admission_reject_proof_filter_other_delta="$(metric_delta "$r1_auto_admission_reject_proof_filter_other" "${baseline_r1_auto_admission_reject_proof_filter_other:-0}")"
    r1_auto_admission_reject_micro_market_no_uplift_delta="$(metric_delta "$r1_auto_admission_reject_micro_market_no_uplift" "${baseline_r1_auto_admission_reject_micro_market_no_uplift:-0}")"
    r1_auto_admission_reject_micro_market_negative_ev_delta="$(metric_delta "$r1_auto_admission_reject_micro_market_negative_ev" "${baseline_r1_auto_admission_reject_micro_market_negative_ev:-0}")"
    r1_auto_admission_reject_micro_market_below_floor_delta="$(metric_delta "$r1_auto_admission_reject_micro_market_below_floor" "${baseline_r1_auto_admission_reject_micro_market_below_floor:-0}")"
    r1_auto_admission_reject_micro_market_other_delta="$(metric_delta "$r1_auto_admission_reject_micro_market_other" "${baseline_r1_auto_admission_reject_micro_market_other:-0}")"
    r1_auto_admission_admit_to_process_intent_attempt_delta="$(metric_delta "$r1_auto_admission_admit_to_process_intent_attempt" "${baseline_r1_auto_admission_admit_to_process_intent_attempt:-0}")"
    r1_auto_admission_admit_to_process_intent_ok_delta="$(metric_delta "$r1_auto_admission_admit_to_process_intent_ok" "${baseline_r1_auto_admission_admit_to_process_intent_ok:-0}")"
    r1_auto_admission_admit_to_process_intent_drop_delta="$(metric_delta "$r1_auto_admission_admit_to_process_intent_drop" "${baseline_r1_auto_admission_admit_to_process_intent_drop:-0}")"
fi

shock_size="$(stat -c '%s' "$SHOCK_RESEARCH_PATH" 2>/dev/null || echo 0)"
TARGET_EXACT_DIRECTION_HITS=0
TARGET_ROUTE_SEEN_HITS=0
TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS=0
TARGET_NON_PUBLIC_ROUTE_HITS=0
TARGET_QUOTE_REQUEST_SEEN_HITS=0
TARGET_QUOTE_MATCHED_HITS=0
TARGET_QUOTE_WAIT_TIMEOUT_HITS=0
TARGET_PUBLIC_SCOUT_ROUTE_OVERRIDE_HITS=0
TARGET_PUBLIC_SCOUT_BLOCKED_HITS=0
TARGET_PUBLIC_SCOUT_BLOCKED_REASON=""
TARGET_PUBLIC_SCOUT_BLOCKED_STAGE=""
TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL=""
TARGET_FIRST_ROUTE_DECISION=""
TARGET_FIRST_NON_PUBLIC_DECISION=""
TARGET_FIRST_SIGNATURE=""
TARGET_FIRST_SLOT=""
TARGET_ROLLOUT_SHADOW_HITS=0
TARGET_WOULD_BUILD_HITS=0
TARGET_WOULD_SIGN_HITS=0
TARGET_WOULD_SEND_HITS=0
TARGET_FIRST_ROLLOUT_SIGNATURE=""
TARGET_ASSERTION_RESULT="not_configured"
eval "$(collect_target_trace_metrics "$TIER0_LOG" "$SHOCK_RESEARCH_PATH")"
if [[ "$RUN_MODE" == "watcher_armed" ]]; then
    FINAL_PROOF_OUTCOME="$(classify_watcher_armed_outcome)"
else
    FINAL_PROOF_OUTCOME="$(classify_strict_proof_outcome)"
fi

{
    echo "mode=$RUN_MODE"
    echo "window_seconds=$WINDOW_SECONDS"
    echo "generated_config=$CONFIG_TMP"
    echo "private_packet_backend=$PRIVATE_PACKET_BACKEND"
    echo "public_rpc_endpoint=$PUBLIC_RPC_ENDPOINT"
    echo "max_live_trade_input_sol=$MAX_LIVE_TRADE_INPUT_SOL"
    echo "max_position_sol_per_mint=$MAX_POSITION_SOL_PER_MINT"
    echo "max_notional_sol_per_entity=$MAX_NOTIONAL_SOL_PER_ENTITY"
    echo "allow_only_venue_id=$ALLOW_ONLY_VENUE_ID"
    echo "allow_only_pool=$ALLOW_ONLY_POOL"
    echo "allow_only_mint_in=$ALLOW_ONLY_MINT_IN"
    echo "allow_only_mint_out=$ALLOW_ONLY_MINT_OUT"
    echo "approved_held_input_mints=$APPROVED_HELD_INPUT_MINTS"
    echo "held_input_mints=$HELD_INPUT_MINTS"
    echo "held_input_filter_active=true"
    echo "require_target_public_scout=$REQUIRE_TARGET_PUBLIC_SCOUT"
    echo "triggered_from_watcher=$TRIGGERED_FROM_WATCHER"
    echo "target_assertion_result=$TARGET_ASSERTION_RESULT"
    echo "target_exact_direction_hits=$TARGET_EXACT_DIRECTION_HITS"
    echo "target_route_seen_hits=$TARGET_ROUTE_SEEN_HITS"
    echo "target_public_rpc_scout_route_hits=$TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS"
    echo "target_non_public_route_hits=$TARGET_NON_PUBLIC_ROUTE_HITS"
    echo "target_quote_request_seen_hits=$TARGET_QUOTE_REQUEST_SEEN_HITS"
    echo "target_quote_matched_hits=$TARGET_QUOTE_MATCHED_HITS"
    echo "target_quote_wait_timeout_hits=$TARGET_QUOTE_WAIT_TIMEOUT_HITS"
    echo "target_public_scout_route_override_hits=$TARGET_PUBLIC_SCOUT_ROUTE_OVERRIDE_HITS"
    echo "target_public_scout_blocked_hits=$TARGET_PUBLIC_SCOUT_BLOCKED_HITS"
    echo "target_public_scout_blocked_reason=$TARGET_PUBLIC_SCOUT_BLOCKED_REASON"
    echo "target_public_scout_blocked_stage=$TARGET_PUBLIC_SCOUT_BLOCKED_STAGE"
    echo "target_public_scout_blocked_detail=$TARGET_PUBLIC_SCOUT_BLOCKED_DETAIL"
    echo "target_first_route_decision=$TARGET_FIRST_ROUTE_DECISION"
    echo "target_first_non_public_decision=$TARGET_FIRST_NON_PUBLIC_DECISION"
    echo "target_first_signature=$TARGET_FIRST_SIGNATURE"
    echo "target_first_slot=$TARGET_FIRST_SLOT"
    echo "target_rollout_shadow_hits=$TARGET_ROLLOUT_SHADOW_HITS"
    echo "target_would_build_hits=$TARGET_WOULD_BUILD_HITS"
    echo "target_would_sign_hits=$TARGET_WOULD_SIGN_HITS"
    echo "target_would_send_hits=$TARGET_WOULD_SEND_HITS"
    echo "target_first_rollout_signature=$TARGET_FIRST_ROLLOUT_SIGNATURE"
    echo "runtime_log_endpoint_seen=$endpoint_seen"
    echo "one_shot_mode=$ONE_SHOT_MODE"
    echo "raw_outcome=$RUN37_OUTCOME"
    echo "outcome=$FINAL_PROOF_OUTCOME"
    echo "arm_reason=$WATCHER_ARM_REASON"
    echo "arm_wait_seconds=$WATCHER_ARM_WAIT_SECONDS"
    echo "baseline_worker_records_seen=$baseline_worker_records_seen"
    echo "final_worker_records_seen=$final_worker_records_seen"
    echo "worker_records_seen_delta=$worker_records_seen_delta"
    echo "sample_slots=$sample_slots"
    echo "observe_candidates=$observe_candidates"
    echo "live_landed=$live_landed"
    echo "worker_records_seen=$worker_records_seen"
    echo "worker_pool_state_hit=$worker_pool_state_hit"
    echo "worker_pool_state_hit_delta=$worker_pool_state_hit_delta"
    echo "worker_route_actionable=$worker_route_actionable"
    echo "worker_route_actionable_delta=$worker_route_actionable_delta"
    echo "worker_route_resolved_drop=$worker_route_resolved_drop"
    echo "worker_candidates_entered_process_intent=$worker_candidates_entered_process_intent"
    echo "worker_candidates_entered_process_intent_delta=$worker_candidates_entered_process_intent_delta"
    echo "worker_route_resolved_public_rpc_scout=$worker_route_resolved_public_rpc_scout"
    echo "worker_route_resolved_public_rpc_scout_delta=$worker_route_resolved_public_rpc_scout_delta"
    echo "run38_target_public_scout_clmm_route_resolved=$run38_target_public_scout_clmm_route_resolved"
    echo "run38_target_public_scout_clmm_route_resolved_delta=$run38_target_public_scout_clmm_route_resolved_delta"
    echo "run38_target_public_scout_clmm_r1_attempt=$run38_target_public_scout_clmm_r1_attempt"
    echo "run38_target_public_scout_clmm_r1_attempt_delta=$run38_target_public_scout_clmm_r1_attempt_delta"
    echo "run38_target_public_scout_clmm_reject_proof_filter=$run38_target_public_scout_clmm_reject_proof_filter"
    echo "run38_target_public_scout_clmm_reject_proof_filter_delta=$run38_target_public_scout_clmm_reject_proof_filter_delta"
    echo "run38_target_public_scout_clmm_reject_micro_market=$run38_target_public_scout_clmm_reject_micro_market"
    echo "run38_target_public_scout_clmm_reject_micro_market_delta=$run38_target_public_scout_clmm_reject_micro_market_delta"
    echo "run38_target_public_scout_clmm_reject_inventory=$run38_target_public_scout_clmm_reject_inventory"
    echo "run38_target_public_scout_clmm_reject_inventory_delta=$run38_target_public_scout_clmm_reject_inventory_delta"
    echo "run38_target_public_scout_clmm_reject_inventory_missing_input_ata=$run38_target_public_scout_clmm_reject_inventory_missing_input_ata"
    echo "run38_target_public_scout_clmm_reject_inventory_missing_input_ata_delta=$run38_target_public_scout_clmm_reject_inventory_missing_input_ata_delta"
    echo "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized=$run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized"
    echo "run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized_delta=$run38_target_public_scout_clmm_reject_inventory_input_ata_uninitialized_delta"
    echo "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient=$run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient"
    echo "run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient_delta=$run38_target_public_scout_clmm_reject_inventory_input_balance_insufficient_delta"
    echo "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch=$run38_target_public_scout_clmm_reject_inventory_token_program_mismatch"
    echo "run38_target_public_scout_clmm_reject_inventory_token_program_mismatch_delta=$run38_target_public_scout_clmm_reject_inventory_token_program_mismatch_delta"
    echo "run38_target_public_scout_clmm_reject_inventory_other=$run38_target_public_scout_clmm_reject_inventory_other"
    echo "run38_target_public_scout_clmm_reject_inventory_other_delta=$run38_target_public_scout_clmm_reject_inventory_other_delta"
    echo "run38_target_public_scout_clmm_reject_template_executability=$run38_target_public_scout_clmm_reject_template_executability"
    echo "run38_target_public_scout_clmm_reject_template_executability_delta=$run38_target_public_scout_clmm_reject_template_executability_delta"
    echo "run38_target_public_scout_clmm_reject_attempt_latch=$run38_target_public_scout_clmm_reject_attempt_latch"
    echo "run38_target_public_scout_clmm_reject_attempt_latch_delta=$run38_target_public_scout_clmm_reject_attempt_latch_delta"
    echo "run38_target_public_scout_clmm_admit=$run38_target_public_scout_clmm_admit"
    echo "run38_target_public_scout_clmm_admit_delta=$run38_target_public_scout_clmm_admit_delta"
    echo "run38_target_public_scout_clmm_process_intent=$run38_target_public_scout_clmm_process_intent"
    echo "run38_target_public_scout_clmm_process_intent_delta=$run38_target_public_scout_clmm_process_intent_delta"
    echo "run38_target_public_scout_clmm_emit_attempt=$run38_target_public_scout_clmm_emit_attempt"
    echo "run38_target_public_scout_clmm_emit_attempt_delta=$run38_target_public_scout_clmm_emit_attempt_delta"
    echo "run38_target_public_scout_clmm_transport_attempt=$run38_target_public_scout_clmm_transport_attempt"
    echo "run38_target_public_scout_clmm_transport_attempt_delta=$run38_target_public_scout_clmm_transport_attempt_delta"
    echo "run38_target_public_scout_clmm_pm_created=$run38_target_public_scout_clmm_pm_created"
    echo "run38_target_public_scout_clmm_pm_created_delta=$run38_target_public_scout_clmm_pm_created_delta"
    echo "worker_rollout_would_send_events_published=$worker_rollout_would_send_events_published"
    echo "worker_rollout_live_send_suppressed_events_published=$worker_rollout_live_send_suppressed_events_published"
    echo "worker_pm_reservation_created=$worker_pm_reservation_created"
    echo "worker_pm_reservation_created_delta=$worker_pm_reservation_created_delta"
    echo "worker_public_rpc_scout_emit_attempt=$worker_public_rpc_scout_emit_attempt"
    echo "worker_public_rpc_scout_emit_attempt_delta=$worker_public_rpc_scout_emit_attempt_delta"
    echo "worker_public_rpc_scout_emit_ok=$worker_public_rpc_scout_emit_ok"
    echo "public_rpc_scout_enqueued=$public_rpc_scout_enqueued"
    echo "public_rpc_scout_enqueued_delta=$public_rpc_scout_enqueued_delta"
    echo "public_rpc_transport_attempt=$public_rpc_transport_attempt"
    echo "public_rpc_transport_attempt_delta=$public_rpc_transport_attempt_delta"
    echo "public_rpc_transport_failure=$public_rpc_transport_failure"
    echo "public_rpc_transport_rejected=$public_rpc_transport_rejected"
    echo "public_rpc_nack_recorded=$public_rpc_nack_recorded"
    echo "pm_reservation_released=$pm_reservation_released"
    echo "pm_reservation_released_delta=$pm_reservation_released_delta"
    echo "public_scout_attempt_latch_set=$public_scout_attempt_latch_set"
    echo "public_scout_attempt_latch_set_delta=$public_scout_attempt_latch_set_delta"
    echo "public_scout_attempt_latch_drop_count=$public_scout_attempt_latch_drop_count"
    echo "public_scout_attempt_latch_drop_count_delta=$public_scout_attempt_latch_drop_count_delta"
    echo "public_scout_attempt_latch_first_signature=$public_scout_attempt_latch_first_signature"
    echo "fee_leak_accounted=$fee_leak_accounted"
    echo "inventory_truth_ok=$inventory_truth_ok"
    echo "phantom_inventory_risk=$phantom_inventory_risk"
    echo "kill_switch_required=$kill_switch_required"
    echo "public_scout_gates_green=$public_scout_gates_green"
    echo "r1_auto_admission_attempt=$r1_auto_admission_attempt"
    echo "r1_auto_admission_attempt_delta=$r1_auto_admission_attempt_delta"
    echo "r1_auto_admission_admit=$r1_auto_admission_admit"
    echo "r1_auto_admission_admit_delta=$r1_auto_admission_admit_delta"
    echo "r1_auto_admission_reject_micro_market=$r1_auto_admission_reject_micro_market"
    echo "r1_auto_admission_reject_micro_market_delta=$r1_auto_admission_reject_micro_market_delta"
    echo "r1_auto_admission_reject_proof_filter=$r1_auto_admission_reject_proof_filter"
    echo "r1_auto_admission_reject_proof_filter_delta=$r1_auto_admission_reject_proof_filter_delta"
    echo "r1_auto_admission_reject_template_executability=$r1_auto_admission_reject_template_executability"
    echo "r1_auto_admission_reject_template_executability_delta=$r1_auto_admission_reject_template_executability_delta"
    echo "r1_auto_admission_reject_attempt_latch=$r1_auto_admission_reject_attempt_latch"
    echo "r1_auto_admission_reject_attempt_latch_delta=$r1_auto_admission_reject_attempt_latch_delta"
    echo "r1_auto_admission_reject_proof_filter_no_quote=$r1_auto_admission_reject_proof_filter_no_quote"
    echo "r1_auto_admission_reject_proof_filter_no_quote_delta=$r1_auto_admission_reject_proof_filter_no_quote_delta"
    echo "r1_auto_admission_reject_proof_filter_no_positive_ev=$r1_auto_admission_reject_proof_filter_no_positive_ev"
    echo "r1_auto_admission_reject_proof_filter_no_positive_ev_delta=$r1_auto_admission_reject_proof_filter_no_positive_ev_delta"
    echo "r1_auto_admission_reject_proof_filter_no_shadow_guard=$r1_auto_admission_reject_proof_filter_no_shadow_guard"
    echo "r1_auto_admission_reject_proof_filter_no_shadow_guard_delta=$r1_auto_admission_reject_proof_filter_no_shadow_guard_delta"
    echo "r1_auto_admission_reject_proof_filter_public_scout_shape=$r1_auto_admission_reject_proof_filter_public_scout_shape"
    echo "r1_auto_admission_reject_proof_filter_public_scout_shape_delta=$r1_auto_admission_reject_proof_filter_public_scout_shape_delta"
    echo "r1_auto_admission_reject_proof_filter_other=$r1_auto_admission_reject_proof_filter_other"
    echo "r1_auto_admission_reject_proof_filter_other_delta=$r1_auto_admission_reject_proof_filter_other_delta"
    echo "r1_auto_admission_reject_public_scout_shape_route_not_public_scout=$r1_auto_admission_reject_public_scout_shape_route_not_public_scout"
    echo "r1_auto_admission_reject_public_scout_shape_wrong_source=$r1_auto_admission_reject_public_scout_shape_wrong_source"
    echo "r1_auto_admission_reject_public_scout_shape_wrong_venue=$r1_auto_admission_reject_public_scout_shape_wrong_venue"
    echo "r1_auto_admission_reject_public_scout_shape_missing_template_family=$r1_auto_admission_reject_public_scout_shape_missing_template_family"
    echo "r1_auto_admission_reject_public_scout_shape_route_template_mismatch=$r1_auto_admission_reject_public_scout_shape_route_template_mismatch"
    echo "r1_auto_admission_reject_public_scout_shape_other=$r1_auto_admission_reject_public_scout_shape_other"
    echo "r1_auto_admission_reject_micro_market_no_uplift=$r1_auto_admission_reject_micro_market_no_uplift"
    echo "r1_auto_admission_reject_micro_market_no_uplift_delta=$r1_auto_admission_reject_micro_market_no_uplift_delta"
    echo "r1_auto_admission_reject_micro_market_negative_ev=$r1_auto_admission_reject_micro_market_negative_ev"
    echo "r1_auto_admission_reject_micro_market_negative_ev_delta=$r1_auto_admission_reject_micro_market_negative_ev_delta"
    echo "r1_auto_admission_reject_micro_market_below_floor=$r1_auto_admission_reject_micro_market_below_floor"
    echo "r1_auto_admission_reject_micro_market_below_floor_delta=$r1_auto_admission_reject_micro_market_below_floor_delta"
    echo "r1_auto_admission_reject_micro_market_other=$r1_auto_admission_reject_micro_market_other"
    echo "r1_auto_admission_reject_micro_market_other_delta=$r1_auto_admission_reject_micro_market_other_delta"
    echo "last_public_scout_shape_reject_reason=$last_public_scout_shape_reject_reason"
    echo "last_public_scout_shape_reject_pool=$last_public_scout_shape_reject_pool"
    echo "last_public_scout_shape_reject_mint_in=$last_public_scout_shape_reject_mint_in"
    echo "last_public_scout_shape_reject_mint_out=$last_public_scout_shape_reject_mint_out"
    echo "last_public_scout_shape_reject_route=$last_public_scout_shape_reject_route"
    echo "last_public_scout_shape_reject_source=$last_public_scout_shape_reject_source"
    echo "last_public_scout_shape_reject_venue=$last_public_scout_shape_reject_venue"
    echo "last_run38_target_pool=$last_run38_target_pool"
    echo "last_run38_target_mint_in=$last_run38_target_mint_in"
    echo "last_run38_target_mint_out=$last_run38_target_mint_out"
    echo "last_run38_target_route=$last_run38_target_route"
    echo "last_run38_target_source=$last_run38_target_source"
    echo "last_run38_target_venue=$last_run38_target_venue"
    echo "last_run38_target_stage=$last_run38_target_stage"
    echo "last_run38_target_reject_reason=$last_run38_target_reject_reason"
    echo "last_run38_target_inventory_reject_reason=$last_run38_target_inventory_reject_reason"
    echo "last_run38_target_inventory_pool=$last_run38_target_inventory_pool"
    echo "last_run38_target_inventory_mint_in=$last_run38_target_inventory_mint_in"
    echo "last_run38_target_inventory_mint_out=$last_run38_target_inventory_mint_out"
    echo "last_run38_target_inventory_input_ata=$last_run38_target_inventory_input_ata"
    echo "last_run38_target_inventory_input_balance=$last_run38_target_inventory_input_balance"
    echo "last_run38_target_inventory_required_input=$last_run38_target_inventory_required_input"
    echo "last_run38_target_inventory_token_program=$last_run38_target_inventory_token_program"
    echo "r1_auto_admission_admit_to_process_intent_attempt=$r1_auto_admission_admit_to_process_intent_attempt"
    echo "r1_auto_admission_admit_to_process_intent_attempt_delta=$r1_auto_admission_admit_to_process_intent_attempt_delta"
    echo "r1_auto_admission_admit_to_process_intent_ok=$r1_auto_admission_admit_to_process_intent_ok"
    echo "r1_auto_admission_admit_to_process_intent_ok_delta=$r1_auto_admission_admit_to_process_intent_ok_delta"
    echo "r1_auto_admission_admit_to_process_intent_drop=$r1_auto_admission_admit_to_process_intent_drop"
    echo "r1_auto_admission_admit_to_process_intent_drop_delta=$r1_auto_admission_admit_to_process_intent_drop_delta"
    echo "last_admit_to_process_intent_drop_reason=$last_admit_to_process_intent_drop_reason"
    echo "last_admit_to_process_intent_pool=$last_admit_to_process_intent_pool"
    echo "last_admit_to_process_intent_mint_in=$last_admit_to_process_intent_mint_in"
    echo "last_admit_to_process_intent_mint_out=$last_admit_to_process_intent_mint_out"
    echo "last_admit_to_process_intent_route=$last_admit_to_process_intent_route"
    echo "last_admit_to_process_intent_source=$last_admit_to_process_intent_source"
    echo "live_landed_delta=$live_landed_delta"
    echo "rollout_events_observed=$rollout_events_observed"
    echo "rollout_public_rpc_no_send_would_sends=$rollout_public_rpc_no_send_would_sends"
    echo "shadow_guard_mainnet_deployed=$shadow_guard_mainnet_deployed"
    echo "shadow_guard_template_program_id_matches=$shadow_guard_template_program_id_matches"
    echo "shock_research_size=$shock_size"
} | tee "$SUMMARY_FILE"

if [[ "$FINAL_PROOF_OUTCOME" != "PASS" && "$FINAL_PROOF_OUTCOME" != "RUN38_PASS_BLACKHOLE" ]]; then
    fail "$FINAL_PROOF_OUTCOME: pool=$ALLOW_ONLY_POOL mint_in=$ALLOW_ONLY_MINT_IN mint_out=$ALLOW_ONLY_MINT_OUT exact_hits=$TARGET_EXACT_DIRECTION_HITS route_seen_hits=$TARGET_ROUTE_SEEN_HITS public_route_hits=$TARGET_PUBLIC_RPC_SCOUT_ROUTE_HITS non_public_route_hits=$TARGET_NON_PUBLIC_ROUTE_HITS quote_matched_hits=$TARGET_QUOTE_MATCHED_HITS quote_wait_timeout_hits=$TARGET_QUOTE_WAIT_TIMEOUT_HITS would_build_hits=$TARGET_WOULD_BUILD_HITS would_sign_hits=$TARGET_WOULD_SIGN_HITS would_send_hits=$TARGET_WOULD_SEND_HITS first_route_decision=$TARGET_FIRST_ROUTE_DECISION blocked_reason=$TARGET_PUBLIC_SCOUT_BLOCKED_REASON blocked_stage=$TARGET_PUBLIC_SCOUT_BLOCKED_STAGE"
    exit 1
fi

if is_true "$EXPECT_BLACKHOLE_ENDPOINT"; then
    if [[ "$live_landed" != "0" ]]; then
        fail "Run29-C blackhole proof observed a landed live send"
        exit 1
    fi
elif is_true "$ONE_SHOT_MODE"; then
    if [[ "$RUN37_OUTCOME" == "none" ]]; then
        fail "Run37 one-shot proof ended without a landed or controlled failure outcome"
        exit 1
    fi
fi

finish

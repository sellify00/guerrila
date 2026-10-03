#!/usr/bin/env bash
# 52a_tpu_route_conversion_9i1gx.sh - exact-target or watcher-armed TPU route conversion proof

source "$(dirname "$0")/lib.sh"
script_header "52A - TPU Route Conversion 9i1Gx"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_MODE="${SHADOW_RUN52_TARGET_MODE:-${SHADOW_RUN52A_TARGET_MODE:-exact_allow_only}}"
TARGET_VENUE_ID="${SHADOW_RUN52A_TARGET_VENUE_ID:-meteora_damm_v2}"
TARGET_POOL="${SHADOW_RUN52A_TARGET_POOL:-9i1GxRjhZRyjPK5M4JTxAKB3HPNmiAQKVQLaBbGKCtRs}"
TARGET_MINT_IN="${SHADOW_RUN52A_TARGET_MINT_IN:-EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v}"
TARGET_MINT_OUT="${SHADOW_RUN52A_TARGET_MINT_OUT:-MomhYZFWByMgz5vnLLLHZTr4a5dNUWXAcR7bfFPXHbF}"
RUN40_SCORECARD_PATH="${SHADOW_RUN52A_SCORECARD_PATH:-${SHADOW_RUN52_SCORECARD_PATH:-}}"
RUN40_SCORECARD_SEARCH_ROOT="${SHADOW_RUN52A_SCORECARD_SEARCH_ROOT:-${SHADOW_RUN52_SCORECARD_SEARCH_ROOT:-}}"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL-}"
CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL-}"
CALLER_SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL-}"
CALLER_SHADOW_RUN40_RPC_URL="${SHADOW_RUN40_RPC_URL-}"

WINDOW_SECONDS="${SHADOW_RUN52A_WINDOW_SECONDS:-600}"
MAX_INPUT_SOL="${SHADOW_RUN52A_MAX_INPUT_SOL:-1.0}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN52A_MAX_POSITION_SOL_PER_MINT:-1.0}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN52A_MAX_DAILY_LOSS_SOL:-0.5}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

require_env SHADOW_NIC_IFACE

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
DEFAULT_SCORECARD_SEARCH_ROOT="$RESULTS_ROOT"
if [[ -d "/home/ubuntu/shadow-host-audit/dpdk-runs" ]]; then
    DEFAULT_SCORECARD_SEARCH_ROOT="/home/ubuntu/shadow-host-audit/dpdk-runs"
fi
if [[ -z "$RUN40_SCORECARD_SEARCH_ROOT" ]]; then
    RUN40_SCORECARD_SEARCH_ROOT="$DEFAULT_SCORECARD_SEARCH_ROOT"
fi
STEP="${SHADOW_RUN52A_STEP:-52a_tpu_route_conversion_9i1gx}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run52a_tpu_route_conversion_summary.json"
INNER_STEP="${SHADOW_RUN52A_INNER_STEP:-${STEP}_run40}"
INNER_RESULT_DIR="$RESULTS_ROOT/$INNER_STEP"
INNER_SUMMARY_FILE="$INNER_RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

summary_value() {
    local path="$1"
    local key="$2"
    [[ -f "$path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { print substr($0, length($1) + 2) }' "$path" | tail -n 1
}

if [[ "$TARGET_MODE" == "exact_allow_only" ]]; then
    info "Running exact-target TPU no-send conversion proof"
    info "target_pool=$TARGET_POOL target_mint_in=$TARGET_MINT_IN target_mint_out=$TARGET_MINT_OUT"
else
    info "Running watcher-armed TPU no-send conversion proof"
    info "target_mode=$TARGET_MODE target_venue=$TARGET_VENUE_ID scorecard_search_root=$RUN40_SCORECARD_SEARCH_ROOT"
fi

set +e
(
    export SHADOW_RUN40_ENABLE_TPU_QUIC=true
    export SHADOW_RUN40_STEP="$INNER_STEP"
    export SHADOW_RUN40_TARGET_MODE="$TARGET_MODE"
    export SHADOW_RUN40_WINDOW_SECONDS="$WINDOW_SECONDS"
    export SHADOW_RUN40_MODE="no_send"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="false"
    export SHADOW_RUN40_MAX_ATTEMPTS="1"
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED="true"
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
    export SHADOW_RUN40_SCORECARD_SEARCH_ROOT="$RUN40_SCORECARD_SEARCH_ROOT"
    if [[ -n "$RUN40_SCORECARD_PATH" ]]; then
        export SHADOW_RUN40_SCORECARD_PATH="$RUN40_SCORECARD_PATH"
    fi
    if [[ "$TARGET_MODE" == "exact_allow_only" ]]; then
        export SHADOW_RUN40_ALLOW_ONLY_VENUE_ID="$TARGET_VENUE_ID"
        export SHADOW_RUN40_ALLOW_ONLY_POOL="$TARGET_POOL"
        export SHADOW_RUN40_ALLOW_ONLY_MINT_IN="$TARGET_MINT_IN"
        export SHADOW_RUN40_ALLOW_ONLY_MINT_OUT="$TARGET_MINT_OUT"
    fi
    exec "$PACK_DIR/scripts/40_tpu_quic_tiny_live_scout.sh" no_send
)
INNER_RC=$?
set -e

python3 - "$INNER_SUMMARY_FILE" "$SUMMARY_FILE" "$SUMMARY_JSON" "$INNER_RC" \
    "$TARGET_MODE" "$TARGET_VENUE_ID" "$TARGET_POOL" "$TARGET_MINT_IN" "$TARGET_MINT_OUT" <<'PY'
import json
import sys
from pathlib import Path

inner_summary_path = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
inner_rc = int(sys.argv[4])
target_mode = sys.argv[5]
target_venue = sys.argv[6]
target_pool = sys.argv[7]
target_mint_in = sys.argv[8]
target_mint_out = sys.argv[9]

def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data

def as_int(mapping, key):
    value = mapping.get(key, 0)
    try:
        return int(value)
    except Exception:
        return 0

inner = parse_kv(inner_summary_path)
candidate_seen = as_int(inner, "tpu_quic_candidate_seen")
would_enqueue = as_int(inner, "tpu_quic_would_enqueue")
ring_enqueue_ok = as_int(inner, "tpu_quic_ring_enqueue_ok")
sender_dequeue_count = as_int(inner, "tpu_quic_sender_dequeue_count")
no_send_accept_count = as_int(inner, "tpu_quic_no_send_accept_count")
tpu_wire_send_count = as_int(inner, "tpu_wire_send_count")
live_landed = as_int(inner, "live_landed")
last_route_family = inner.get("tpu_quic_last_route_family", "")
scorecard_replay_stale_slot_bypass_seen = as_int(inner, "scorecard_replay_stale_slot_bypass_seen")
scorecard_replay_stale_slot_bypass_applied = as_int(inner, "scorecard_replay_stale_slot_bypass_applied")
scorecard_replay_stale_slot_bypass_signature_match = as_int(inner, "scorecard_replay_stale_slot_bypass_signature_match")
scorecard_replay_stale_slot_bypass_pool_match = as_int(inner, "scorecard_replay_stale_slot_bypass_pool_match")
scorecard_replay_original_slot = as_int(inner, "scorecard_replay_original_slot")
scorecard_replay_current_slot = as_int(inner, "scorecard_replay_current_slot")
scorecard_replay_slot_gap = as_int(inner, "scorecard_replay_slot_gap")
scorecard_replay_route_override_seen = as_int(inner, "scorecard_replay_route_override_seen")
scorecard_replay_route_override_applied = as_int(inner, "scorecard_replay_route_override_applied")
scorecard_replay_route_override_signature_match = as_int(inner, "scorecard_replay_route_override_signature_match")
scorecard_replay_route_override_pool_match = as_int(inner, "scorecard_replay_route_override_pool_match")
scorecard_replay_route_override_from_public_target = as_int(inner, "scorecard_replay_route_override_from_public_target")
scorecard_replay_route_override_to_tpu_quic_public = as_int(inner, "scorecard_replay_route_override_to_tpu_quic_public")
scorecard_replay_source_override_seen = as_int(inner, "scorecard_replay_source_override_seen")
scorecard_replay_source_override_applied = as_int(inner, "scorecard_replay_source_override_applied")
scorecard_replay_source_override_signature_match = as_int(inner, "scorecard_replay_source_override_signature_match")
scorecard_replay_source_override_pool_match = as_int(inner, "scorecard_replay_source_override_pool_match")
scorecard_replay_source_override_live_send_blocked = as_int(inner, "scorecard_replay_source_override_live_send_blocked")
selection_status = inner.get("selection_status", "")
current_scorecard_loaded = inner.get("current_scorecard_loaded", "")
stale_scorecard_used = inner.get("stale_scorecard_used", "")
selection_attempted = inner.get("selection_attempted", "")
gate_autopsy_scope = inner.get("gate_autopsy_scope", "")
gate_autopsy_observed = as_int(inner, "gate_autopsy_observed")
gate_autopsy_held_input = as_int(inner, "gate_autopsy_held_input")
gate_autopsy_positive_ev = as_int(inner, "gate_autopsy_positive_ev")
gate_autopsy_input_balance_ok = as_int(inner, "gate_autopsy_input_balance_ok")
gate_autopsy_output_account_ok = as_int(inner, "gate_autopsy_output_account_ok")
gate_autopsy_template_ready = as_int(inner, "gate_autopsy_template_ready")
gate_autopsy_tiny_cap_safe = as_int(inner, "gate_autopsy_tiny_cap_safe")
gate_autopsy_tpu_convertible = as_int(inner, "gate_autopsy_tpu_convertible")
gate_autopsy_eligible = as_int(inner, "gate_autopsy_eligible")
selected_venue = inner.get("selected_venue", "")
selected_pool = inner.get("selected_pool", "")
selected_mint_in = inner.get("selected_mint_in", "")
selected_mint_out = inner.get("selected_mint_out", "")
selected_signature = inner.get("selected_signature", "")
selected_source = inner.get("selected_source", "")
selected_target_route_family = inner.get("selected_target_route_family", "")
canonical_private_grade = inner.get("canonical_private_grade", "false")
source_relaxed_dev_proof = inner.get("source_relaxed_dev_proof", "false")
scorecard_replay_dev_proof = inner.get("scorecard_replay_dev_proof", "false")
live_market_recurrence_required = inner.get("live_market_recurrence_required", "true")
scorecard_replay_spec = inner.get("scorecard_replay_spec", "")
replay_hydration_mode = inner.get("replay_hydration_mode", "")
replay_hydration_before_xdp = inner.get("replay_hydration_before_xdp", "")
replay_hydration_rpc_during_lease = inner.get("replay_hydration_rpc_during_lease", "")
replay_tx_artifact_exists = inner.get("replay_tx_artifact_exists", "")
prehydrated_replay_tx_path = inner.get("prehydrated_replay_tx_path", "")
prehydrated_replay_tx_loaded = inner.get("prehydrated_replay_tx_loaded", "")

route_resolved_tpu_quic_public = candidate_seen if last_route_family in {"tpu_quic_public", ""} else 0
acceptance = {
    "route_resolved_tpu_quic_public": route_resolved_tpu_quic_public > 0,
    "tpu_quic_candidate_seen": candidate_seen > 0,
    "tpu_quic_would_enqueue": would_enqueue > 0,
    "tpu_quic_ring_enqueue_ok": ring_enqueue_ok > 0,
    "tpu_quic_sender_dequeue_count": sender_dequeue_count > 0,
    "tpu_quic_no_send_accept_count": no_send_accept_count > 0,
    "tpu_wire_send_count_zero": tpu_wire_send_count == 0,
    "live_landed_zero": live_landed == 0,
    "canonical_private_grade_false": canonical_private_grade == "false",
    "source_relaxed_dev_proof_true": (
        source_relaxed_dev_proof == "true"
        if target_mode in {
            "any_exec_ready_source_relaxed_tpu_convertible",
            "scorecard_selected_replay_tpu_nosend",
        }
        else True
    ),
    "scorecard_replay_dev_proof_true": (
        scorecard_replay_dev_proof == "true"
        if target_mode == "scorecard_selected_replay_tpu_nosend"
        else True
    ),
    "live_market_recurrence_required_false": (
        live_market_recurrence_required == "false"
        if target_mode == "scorecard_selected_replay_tpu_nosend"
        else True
    ),
}
acceptance_pass = all(acceptance.values())

status = inner.get("status", "")
if not inner_summary_path.exists():
    status = "blocked_missing_run40_summary"
elif acceptance_pass:
    status = "run52a_tpu_conversion_ready"
elif not status:
    status = "blocked_tpu_route_conversion"

payload = {
    "status": status,
    "inner_rc": inner_rc,
    "target": {
        "target_mode": target_mode,
        "venue": target_venue,
        "pool": target_pool,
        "mint_in": target_mint_in,
        "mint_out": target_mint_out,
        "desired_route": "tpu_quic_public",
        "mode": "no_send",
    },
    "selection": {
        "selection_status": selection_status,
        "current_scorecard_loaded": current_scorecard_loaded == "true",
        "stale_scorecard_used": stale_scorecard_used == "true",
        "selection_attempted": selection_attempted == "true",
        "gate_autopsy": {
            "scope": gate_autopsy_scope,
            "observed": gate_autopsy_observed,
            "held_input": gate_autopsy_held_input,
            "positive_ev": gate_autopsy_positive_ev,
            "input_balance_ok": gate_autopsy_input_balance_ok,
            "output_account_ok": gate_autopsy_output_account_ok,
            "template_ready": gate_autopsy_template_ready,
            "tiny_cap_safe": gate_autopsy_tiny_cap_safe,
            "tpu_convertible": gate_autopsy_tpu_convertible,
            "eligible": gate_autopsy_eligible,
        },
        "selected_venue": selected_venue,
        "selected_pool": selected_pool,
        "selected_mint_in": selected_mint_in,
        "selected_mint_out": selected_mint_out,
        "selected_signature": selected_signature,
        "selected_source": selected_source,
        "selected_target_route_family": selected_target_route_family,
        "canonical_private_grade": canonical_private_grade == "true",
        "source_relaxed_dev_proof": source_relaxed_dev_proof == "true",
        "scorecard_replay_dev_proof": scorecard_replay_dev_proof == "true",
        "live_market_recurrence_required": live_market_recurrence_required == "true",
        "scorecard_replay_spec": scorecard_replay_spec,
        "replay_hydration_mode": replay_hydration_mode,
        "replay_hydration_before_xdp": replay_hydration_before_xdp == "true",
        "replay_hydration_rpc_during_lease": replay_hydration_rpc_during_lease,
        "replay_tx_artifact_exists": replay_tx_artifact_exists == "true",
        "prehydrated_replay_tx_path": prehydrated_replay_tx_path,
        "prehydrated_replay_tx_loaded": as_int(inner, "prehydrated_replay_tx_loaded"),
    },
    "acceptance": acceptance,
    "acceptance_pass": acceptance_pass,
    "counters": {
        "route_resolved_tpu_quic_public": route_resolved_tpu_quic_public,
        "tpu_quic_candidate_seen": candidate_seen,
        "tpu_quic_would_enqueue": would_enqueue,
        "tpu_quic_ring_enqueue_ok": ring_enqueue_ok,
        "tpu_quic_sender_dequeue_count": sender_dequeue_count,
        "tpu_quic_no_send_accept_count": no_send_accept_count,
        "scorecard_replay_stale_slot_bypass_seen": scorecard_replay_stale_slot_bypass_seen,
        "scorecard_replay_stale_slot_bypass_applied": scorecard_replay_stale_slot_bypass_applied,
        "scorecard_replay_stale_slot_bypass_signature_match": scorecard_replay_stale_slot_bypass_signature_match,
        "scorecard_replay_stale_slot_bypass_pool_match": scorecard_replay_stale_slot_bypass_pool_match,
        "scorecard_replay_original_slot": scorecard_replay_original_slot,
        "scorecard_replay_current_slot": scorecard_replay_current_slot,
        "scorecard_replay_slot_gap": scorecard_replay_slot_gap,
        "scorecard_replay_route_override_seen": scorecard_replay_route_override_seen,
        "scorecard_replay_route_override_applied": scorecard_replay_route_override_applied,
        "scorecard_replay_route_override_signature_match": scorecard_replay_route_override_signature_match,
        "scorecard_replay_route_override_pool_match": scorecard_replay_route_override_pool_match,
        "scorecard_replay_route_override_from_public_target": scorecard_replay_route_override_from_public_target,
        "scorecard_replay_route_override_to_tpu_quic_public": scorecard_replay_route_override_to_tpu_quic_public,
        "scorecard_replay_source_override_seen": scorecard_replay_source_override_seen,
        "scorecard_replay_source_override_applied": scorecard_replay_source_override_applied,
        "scorecard_replay_source_override_signature_match": scorecard_replay_source_override_signature_match,
        "scorecard_replay_source_override_pool_match": scorecard_replay_source_override_pool_match,
        "scorecard_replay_source_override_live_send_blocked": scorecard_replay_source_override_live_send_blocked,
        "tpu_wire_send_count": tpu_wire_send_count,
        "live_landed": live_landed,
    },
    "inner_summary_path": str(inner_summary_path),
    "inner_status": inner.get("status"),
    "inner_probe_status": inner.get("probe_status"),
    "last_route_family": last_route_family,
}

summary_lines = [
    f"status={payload['status']}",
    f"inner_rc={inner_rc}",
    f"target_mode={target_mode}",
    f"target_venue={target_venue}",
    f"target_pool={target_pool}",
    f"target_mint_in={target_mint_in}",
    f"target_mint_out={target_mint_out}",
    "desired_route=tpu_quic_public",
    "mode=no_send",
    f"selection_status={selection_status}",
    f"current_scorecard_loaded={current_scorecard_loaded}",
    f"stale_scorecard_used={stale_scorecard_used}",
    f"selection_attempted={selection_attempted}",
    f"gate_autopsy_scope={gate_autopsy_scope}",
    f"gate_autopsy_observed={gate_autopsy_observed}",
    f"gate_autopsy_held_input={gate_autopsy_held_input}",
    f"gate_autopsy_positive_ev={gate_autopsy_positive_ev}",
    f"gate_autopsy_input_balance_ok={gate_autopsy_input_balance_ok}",
    f"gate_autopsy_output_account_ok={gate_autopsy_output_account_ok}",
    f"gate_autopsy_template_ready={gate_autopsy_template_ready}",
    f"gate_autopsy_tiny_cap_safe={gate_autopsy_tiny_cap_safe}",
    f"gate_autopsy_tpu_convertible={gate_autopsy_tpu_convertible}",
    f"gate_autopsy_eligible={gate_autopsy_eligible}",
    f"selected_venue={selected_venue}",
    f"selected_pool={selected_pool}",
    f"selected_mint_in={selected_mint_in}",
    f"selected_mint_out={selected_mint_out}",
    f"selected_signature={selected_signature}",
    f"selected_source={selected_source}",
    f"selected_target_route_family={selected_target_route_family}",
    f"canonical_private_grade={canonical_private_grade}",
    f"source_relaxed_dev_proof={source_relaxed_dev_proof}",
    f"scorecard_replay_dev_proof={scorecard_replay_dev_proof}",
    f"live_market_recurrence_required={live_market_recurrence_required}",
    f"scorecard_replay_spec={scorecard_replay_spec}",
    f"replay_hydration_mode={replay_hydration_mode}",
    f"replay_hydration_before_xdp={replay_hydration_before_xdp}",
    f"replay_hydration_rpc_during_lease={replay_hydration_rpc_during_lease}",
    f"replay_tx_artifact_exists={replay_tx_artifact_exists}",
    f"prehydrated_replay_tx_path={prehydrated_replay_tx_path}",
    f"prehydrated_replay_tx_loaded={prehydrated_replay_tx_loaded}",
    f"route_resolved_tpu_quic_public={route_resolved_tpu_quic_public}",
    f"tpu_quic_candidate_seen={candidate_seen}",
    f"tpu_quic_would_enqueue={would_enqueue}",
    f"tpu_quic_ring_enqueue_ok={ring_enqueue_ok}",
    f"tpu_quic_sender_dequeue_count={sender_dequeue_count}",
    f"tpu_quic_no_send_accept_count={no_send_accept_count}",
    f"scorecard_replay_stale_slot_bypass_seen={scorecard_replay_stale_slot_bypass_seen}",
    f"scorecard_replay_stale_slot_bypass_applied={scorecard_replay_stale_slot_bypass_applied}",
    f"scorecard_replay_stale_slot_bypass_signature_match={scorecard_replay_stale_slot_bypass_signature_match}",
    f"scorecard_replay_stale_slot_bypass_pool_match={scorecard_replay_stale_slot_bypass_pool_match}",
    f"scorecard_replay_original_slot={scorecard_replay_original_slot}",
    f"scorecard_replay_current_slot={scorecard_replay_current_slot}",
    f"scorecard_replay_slot_gap={scorecard_replay_slot_gap}",
    f"scorecard_replay_route_override_seen={scorecard_replay_route_override_seen}",
    f"scorecard_replay_route_override_applied={scorecard_replay_route_override_applied}",
    f"scorecard_replay_route_override_signature_match={scorecard_replay_route_override_signature_match}",
    f"scorecard_replay_route_override_pool_match={scorecard_replay_route_override_pool_match}",
    f"scorecard_replay_route_override_from_public_target={scorecard_replay_route_override_from_public_target}",
    f"scorecard_replay_route_override_to_tpu_quic_public={scorecard_replay_route_override_to_tpu_quic_public}",
    f"scorecard_replay_source_override_seen={scorecard_replay_source_override_seen}",
    f"scorecard_replay_source_override_applied={scorecard_replay_source_override_applied}",
    f"scorecard_replay_source_override_signature_match={scorecard_replay_source_override_signature_match}",
    f"scorecard_replay_source_override_pool_match={scorecard_replay_source_override_pool_match}",
    f"scorecard_replay_source_override_live_send_blocked={scorecard_replay_source_override_live_send_blocked}",
    f"tpu_wire_send_count={tpu_wire_send_count}",
    f"live_landed={live_landed}",
    f"inner_status={inner.get('status', '')}",
    f"inner_probe_status={inner.get('probe_status', '')}",
    f"last_route_family={last_route_family}",
    f"acceptance_pass={'true' if acceptance_pass else 'false'}",
]

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

ACCEPTANCE_PASS="$(summary_value "$SUMMARY_FILE" "acceptance_pass")"
if [[ "$ACCEPTANCE_PASS" == "true" ]]; then
    pass "Run52A TPU no-send acceptance passed"
    finish
    exit 0
fi

fail "Run52A TPU no-send acceptance failed"
finish
exit 1

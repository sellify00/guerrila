#!/usr/bin/env bash
# 40_tpu_quic_tiny_live_scout.sh - fail-closed TPU/QUIC readiness + scout entrypoint

source "$(dirname "$0")/lib.sh"
script_header "40 - TPU/QUIC Tiny Live Scout"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ARTIFACT_DIR="$PACK_DIR/artifacts/release-guerrila"
TIER0_BIN="${SHADOW_RUN40_TIER0_BIN:-$ARTIFACT_DIR/shadow-tier0}"
TPU_BIN="${SHADOW_RUN40_TPU_BIN:-$ARTIFACT_DIR/shadow-egress-tpu}"
ORACLE_BIN="${SHADOW_RUN40_ORACLE_BIN:-$ARTIFACT_DIR/shadow-oracle}"
CONFIG="${SHADOW_RUN40_CONFIG:-/etc/shadow-engine/config.guerrila.yaml}"
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

assert_root
require_env SHADOW_NIC_IFACE

STEP="${SHADOW_RUN40_STEP:-40_tpu_quic_tiny_live_scout}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
CONFIG_CHECK_FILE="$RESULT_DIR/config_check.txt"
TPU_SUMMARY_FILE="$RESULT_DIR/tpu_summary.txt"
TPU_LOG="$RESULT_DIR/shadow-egress-tpu.log"
TIER0_LOG="$RESULT_DIR/shadow-tier0.log"
ORACLE_LOG="$RESULT_DIR/shadow-oracle.log"
TARGET_SELECTION_ENV="$RESULT_DIR/run40_target_selection.env"
TARGET_SELECTION_JSON="$RESULT_DIR/run40_target_selection.json"
TARGET_SCORECARD_COPY="$RESULT_DIR/surface_scorecard.json"
TARGET_REPLAY_SPEC="$RESULT_DIR/run40_scorecard_replay_spec.json"
RUN_DIR="$PACK_DIR/run"
GATE_INPUT="$RUN_DIR/g12_gate_input.json"
GATE_REPORT="$RUN_DIR/g12_gate_report.json"
RESULT_GATE_INPUT="$RESULT_DIR/g12_gate_input.json"
RESULT_GATE_REPORT="$RESULT_DIR/g12_gate_report.json"
MODE="${1:-${SHADOW_RUN40_MODE:-dry_run}}"
TARGET_MODE="${SHADOW_RUN40_TARGET_MODE:-exact_allow_only}"
RUN40_SCORECARD_PATH="${SHADOW_RUN40_SCORECARD_PATH:-}"
RUN40_SCORECARD_SEARCH_ROOT="${SHADOW_RUN40_SCORECARD_SEARCH_ROOT:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}"
RUN40_SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RPC_URL="${SHADOW_RUN40_RPC_URL:-http://127.0.0.1:8899}"
WINDOW_SECONDS="${SHADOW_RUN40_WINDOW_SECONDS:-600}"
READINESS_TIMEOUT="${SHADOW_RUN40_READINESS_TIMEOUT:-60}"
NO_SEND_POLL_MS="${SHADOW_RUN40_NO_SEND_POLL_MS:-50}"
TPU_RESULT_RING_PATH="/dev/shm/shadow_egress_result_tpu"
NACK_RING_PATH="/dev/shm/shadow_nack_ring"

MAX_INPUT_SOL="${SHADOW_RUN40_MAX_INPUT_SOL:-0.005}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT:-0.01}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN40_MAX_DAILY_LOSS_SOL:-0.05}"
MAX_ATTEMPTS="${SHADOW_RUN40_MAX_ATTEMPTS:-3}"
STOP_AFTER_FIRST_LANDED="${SHADOW_RUN40_STOP_AFTER_FIRST_LANDED:-true}"
ENABLE_LIVE_SEND="${SHADOW_RUN40_ENABLE_LIVE_SEND:-false}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE"
rm -f "$TPU_SUMMARY_FILE" "$TPU_LOG" "$TIER0_LOG" "$RESULT_GATE_INPUT" "$RESULT_GATE_REPORT" \
    "$TARGET_SELECTION_ENV" "$TARGET_SELECTION_JSON" "$TARGET_SCORECARD_COPY" "$TARGET_REPLAY_SPEC"

if [[ ! -f "$CONFIG" ]]; then
    fail "Missing config: $CONFIG"
    finish
    exit 1
fi
MONEY_WORKERS="$(awk '/^[[:space:]]*money_workers:/ { print $2; exit }' "$CONFIG")"
if [[ ! "$MONEY_WORKERS" =~ ^[0-9]+$ ]] || [[ "$MONEY_WORKERS" -le 0 ]]; then
    fail "Unable to resolve money_workers from config: $CONFIG"
    finish
    exit 1
fi
if [[ ! -x "$TIER0_BIN" ]]; then
    fail "Missing executable: $TIER0_BIN"
    finish
    exit 1
fi
if [[ ! -x "$TPU_BIN" ]]; then
    printf '%s\n' \
        "status=blocked_missing_tpu_quic_sender_binary" \
        "mode=$MODE" \
        "route=direct_tpu_quic_public_lane" \
        "tpu_quic_sender_binary_exists=false" \
        "sender_binary=$TPU_BIN" | tee "$SUMMARY_FILE"
    fail "Missing executable: $TPU_BIN"
    finish
    exit 1
fi

case "$MODE" in
    disabled|dry_run|no_send|tiny_live) ;;
    *)
        fail "Unsupported Run40 mode: $MODE"
        finish
        exit 1
        ;;
esac

info "Loss-budgeted live scout configuration"
info "mode=$MODE rpc_url=$RPC_URL max_input_sol=$MAX_INPUT_SOL max_position_sol_per_mint=$MAX_POSITION_SOL_PER_MINT max_daily_loss_sol=$MAX_DAILY_LOSS_SOL max_attempts=$MAX_ATTEMPTS stop_after_first_landed=$STOP_AFTER_FIRST_LANDED enable_live_send=$ENABLE_LIVE_SEND"

"$TIER0_BIN" --check-config "$CONFIG" | tee "$CONFIG_CHECK_FILE"
"$TIER0_BIN" --bootstrap-tpu-dry-run "$CONFIG" >/dev/null

TIER0_PID=""
ORACLE_PID=""
cleanup() {
    if [[ -n "${TIER0_PID:-}" ]] && kill -0 "$TIER0_PID" 2>/dev/null; then
        kill "$TIER0_PID" 2>/dev/null || true
        wait "$TIER0_PID" 2>/dev/null || true
    fi
    if [[ -n "${ORACLE_PID:-}" ]] && kill -0 "$ORACLE_PID" 2>/dev/null; then
        kill "$ORACLE_PID" 2>/dev/null || true
        wait "$ORACLE_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

copy_gate_artifacts() {
    [[ -f "$GATE_INPUT" ]] && cp "$GATE_INPUT" "$RESULT_GATE_INPUT"
    [[ -f "$GATE_REPORT" ]] && cp "$GATE_REPORT" "$RESULT_GATE_REPORT"
}

gate_file_mtime() {
    local path="$1"
    if [[ -f "$path" ]]; then
        stat -c '%Y' "$path" 2>/dev/null || echo 0
    else
        echo 0
    fi
}

summary_status() {
    local path="$1"
    awk -F= '$1 == "status" { print $2; exit }' "$path" 2>/dev/null
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

ensure_oracle_ready() {
    if [[ ! -x "$ORACLE_BIN" ]]; then
        fail "Missing executable: $ORACLE_BIN"
        finish
        exit 1
    fi

    if ! pgrep -x shadow-oracle >/dev/null 2>&1; then
        info "shadow-oracle is not running; starting publisher for tip LUT shm segments"
        (
            cd "$PACK_DIR"
            exec "$ORACLE_BIN" "$CONFIG" >"$ORACLE_LOG" 2>&1
        ) &
        ORACLE_PID=$!
    else
        info "shadow-oracle already running; reusing existing tip LUT publisher"
    fi

    if ! wait_for_path /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT"; then
        fail "shadow_tip_lut_base did not appear within ${READINESS_TIMEOUT}s"
        finish
        exit 1
    fi
    if ! wait_for_path /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT"; then
        fail "shadow_tip_lut_jito did not appear within ${READINESS_TIMEOUT}s"
        finish
        exit 1
    fi
    if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_base "$READINESS_TIMEOUT"; then
        fail "shadow_tip_lut_base was not published within ${READINESS_TIMEOUT}s"
        finish
        exit 1
    fi
    if ! wait_for_tip_lut_publish /dev/shm/shadow_tip_lut_jito "$READINESS_TIMEOUT"; then
        fail "shadow_tip_lut_jito was not published within ${READINESS_TIMEOUT}s"
        finish
        exit 1
    fi
}

wait_for_tiny_live_pm_reconcile() {
    local baseline_mtime="${1:-0}"
    local tier0_pid="${2:-}"
    local timeout="${SHADOW_RUN40_PM_RECONCILE_TIMEOUT:-15}"
    if [[ ! "$timeout" =~ ^[0-9]+$ ]] || (( timeout < 1 )); then
        timeout=15
    fi
    local deadline=$(( $(date +%s) + timeout ))
    while (( $(date +%s) < deadline )); do
        local gate_state="no_progress"
        if [[ -f "$GATE_INPUT" ]]; then
            gate_state="$(
                python3 - "$GATE_INPUT" "$baseline_mtime" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
baseline_mtime = int(sys.argv[2] or "0")
try:
    payload = json.loads(path.read_text(encoding="utf-8"))
except Exception:
    print("no_progress")
    sys.exit(0)

def as_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0

pm_released = as_int(payload.get("pm_reservation_released"))
live_landed = as_int(payload.get("live_landed"))
current_mtime = int(path.stat().st_mtime) if path.exists() else 0
if pm_released > 0 or live_landed > 0:
    print("pm_evidence")
elif current_mtime > baseline_mtime:
    print("post_probe_export")
else:
    print("no_progress")
PY
            )"
        fi
        case "$gate_state" in
            pm_evidence)
                return 0
                ;;
            post_probe_export)
                return 1
                ;;
        esac
        if [[ -n "$tier0_pid" ]] && ! kill -0 "$tier0_pid" 2>/dev/null; then
            break
        fi
        sleep 1
    done
    return 1
}

ring_file_ready() {
    local path="$1"
    local min_mtime="$2"
    [[ -f "$path" ]] || return 1
    local size
    size="$(stat -c '%s' "$path" 2>/dev/null || echo 0)"
    local mtime
    mtime="$(stat -c '%Y' "$path" 2>/dev/null || echo 0)"
    [[ "$size" -gt 0 && "$mtime" -ge "$min_mtime" ]]
}

tpu_probe_rings_ready() {
    local min_mtime="$1"
    local worker_id
    for (( worker_id=0; worker_id<MONEY_WORKERS; worker_id++ )); do
        ring_file_ready "/dev/shm/shadow_egress_ring_tpu_w${worker_id}" "$min_mtime" || return 1
    done
    ring_file_ready "$TPU_RESULT_RING_PATH" "$min_mtime" || return 1
    ring_file_ready "$NACK_RING_PATH" "$min_mtime" || return 1
}

write_selection_summary() {
    local status="$1"
    cat >"$SUMMARY_FILE" <<EOF
status=$status
mode=$MODE
target_mode=$TARGET_MODE
canonical_private_grade=${SHADOW_RUN40_CANONICAL_PRIVATE_GRADE:-false}
source_relaxed_dev_proof=${SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF:-false}
scorecard_replay_dev_proof=${SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF:-false}
live_market_recurrence_required=${SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED:-true}
selection_status=${SHADOW_RUN40_SELECTION_STATUS:-}
surface_scorecard=${SHADOW_RUN40_SELECTED_SCORECARD:-}
scorecard_replay_spec=${SHADOW_RUN40_SCORECARD_REPLAY_SPEC:-}
current_scorecard_loaded=${SHADOW_RUN40_CURRENT_SCORECARD_LOADED:-false}
stale_scorecard_used=${SHADOW_RUN40_STALE_SCORECARD_USED:-false}
selection_attempted=${SHADOW_RUN40_SELECTION_ATTEMPTED:-false}
gate_autopsy_scope=${SHADOW_RUN40_GATE_AUTOPSY_SCOPE:-}
gate_autopsy_observed=${SHADOW_RUN40_GATE_AUTOPSY_OBSERVED:-0}
gate_autopsy_held_input=${SHADOW_RUN40_GATE_AUTOPSY_HELD_INPUT:-0}
gate_autopsy_positive_ev=${SHADOW_RUN40_GATE_AUTOPSY_POSITIVE_EV:-0}
gate_autopsy_input_balance_ok=${SHADOW_RUN40_GATE_AUTOPSY_INPUT_BALANCE_OK:-0}
gate_autopsy_output_account_ok=${SHADOW_RUN40_GATE_AUTOPSY_OUTPUT_ACCOUNT_OK:-0}
gate_autopsy_template_ready=${SHADOW_RUN40_GATE_AUTOPSY_TEMPLATE_READY:-0}
gate_autopsy_tiny_cap_safe=${SHADOW_RUN40_GATE_AUTOPSY_TINY_CAP_SAFE:-0}
gate_autopsy_tpu_convertible=${SHADOW_RUN40_GATE_AUTOPSY_TPU_CONVERTIBLE:-0}
gate_autopsy_eligible=${SHADOW_RUN40_GATE_AUTOPSY_ELIGIBLE:-0}
selected_venue=${SHADOW_RUN40_SELECTED_VENUE:-}
selected_pool=${SHADOW_RUN40_SELECTED_POOL:-}
selected_mint_in=${SHADOW_RUN40_SELECTED_MINT_IN:-}
selected_mint_out=${SHADOW_RUN40_SELECTED_MINT_OUT:-}
selected_signature=${SHADOW_RUN40_SELECTED_SIGNATURE:-}
selected_route=${SHADOW_RUN40_SELECTED_ROUTE:-}
selected_source=${SHADOW_RUN40_SELECTED_SOURCE:-}
selected_target_route_family=${SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY:-}
selected_held_input=${SHADOW_RUN40_SELECTED_HELD_INPUT:-false}
selected_template_ready=${SHADOW_RUN40_SELECTED_TEMPLATE_READY:-false}
selected_quote_ready_or_classified=${SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED:-false}
selected_tiny_cap_safe=${SHADOW_RUN40_SELECTED_TINY_CAP_SAFE:-false}
selected_output_account_ready=${SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY:-false}
EOF
}

select_run40_target_from_scorecard() {
    python3 - "$TARGET_MODE" "$RUN40_SCORECARD_PATH" "$RUN40_SCORECARD_SEARCH_ROOT" \
        "$RESULT_DIR" "$TARGET_SELECTION_ENV" "$TARGET_SELECTION_JSON" "$TARGET_SCORECARD_COPY" "$TARGET_REPLAY_SPEC" \
        "$RUN40_SIGNER_TOKEN_INVENTORY_PATH" "$MAX_INPUT_SOL" <<'PY'
import json
import shlex
import sys
from pathlib import Path

target_mode = sys.argv[1]
explicit_scorecard = sys.argv[2].strip()
search_root = Path(sys.argv[3])
result_dir = Path(sys.argv[4])
env_path = Path(sys.argv[5])
json_path = Path(sys.argv[6])
scorecard_copy = Path(sys.argv[7])
replay_spec_path = Path(sys.argv[8])
inventory_path = Path(sys.argv[9]) if sys.argv[9].strip() else None
max_input_sol = float(sys.argv[10] or "0.005")
max_tiny_lamports = int(round(max_input_sol * 1_000_000_000))

WSOL_MINT = "So11111111111111111111111111111111111111112"
DEFAULT_VENUE_PRIORITY = [
    "raydium_cpmm",
    "meteora_damm_v2",
    "pumpfun_amm",
    "raydium_v4",
    "raydium_clmm",
]
TPU_CONVERTIBLE_ROUTE_FAMILIES = {"jito_bundle", "harmonic_bundle", "tpu_quic_public"}

def shell_line(key: str, value: str) -> str:
    return f"export {key}={shlex.quote(value)}"

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def text_value(value) -> str:
    return (value or "").strip()

def classify_tiny_cap_blocker(row: dict, max_tiny_lamports: int) -> str:
    proof_filter_result = text_value(row.get("proof_filter_result"))
    if bool(row.get("would_admit")) or proof_filter_result == "no_quote":
        return ""
    trade_size_lamports = safe_int(row.get("trade_size_lamports"))
    if max_tiny_lamports > 0 and trade_size_lamports > max_tiny_lamports:
        return "trade_size_exceeds_max_tiny"
    if proof_filter_result and proof_filter_result != "not_applicable":
        return f"proof_filter:{proof_filter_result}"
    micro_market_result = text_value(row.get("micro_market_result"))
    if micro_market_result and micro_market_result not in {"not_applicable", "not_evaluated"}:
        return f"micro_market:{micro_market_result}"
    execution_blocker = text_value(row.get("execution_blocker"))
    if execution_blocker and execution_blocker != "none":
        return execution_blocker
    r1_outcome = text_value(row.get("r1_outcome"))
    if r1_outcome:
        return f"r1_outcome:{r1_outcome}"
    drop_reason = text_value(row.get("drop_reason"))
    if drop_reason:
        return f"drop_reason:{drop_reason}"
    return "would_admit_false"

def load_inventory(path: Path | None):
    if not path or not path.exists():
        return {}
    raw = load_json(path)
    inventory = {}
    for entry in raw.get("accounts", []):
        mint = entry.get("mint_pubkey")
        if mint:
            inventory[mint] = entry
    return inventory

def overlay_inventory_truth(row: dict, inventory: dict) -> dict:
    enriched = dict(row)
    mint_in = enriched.get("mint_in") or ""
    mint_out = enriched.get("mint_out") or ""
    input_entry = inventory.get(mint_in)
    output_entry = inventory.get(mint_out)
    required_input_raw = safe_int(enriched.get("trade_size_lamports"))
    native_sol_balance_raw = safe_int(enriched.get("native_sol_balance_raw"))
    native_sol_wrap_possible = bool(
        mint_in == WSOL_MINT and native_sol_balance_raw >= required_input_raw and required_input_raw > 0
    )

    if input_entry:
        input_amount_raw = safe_int(input_entry.get("amount_raw"))
        input_initialized = bool(input_entry.get("initialized"))
        enriched["has_input_ata"] = True
        enriched["input_ata_initialized"] = input_initialized
        enriched["input_balance_raw"] = input_amount_raw
        enriched["input_balance_sufficient"] = bool(input_initialized and input_amount_raw >= required_input_raw)
        enriched["spl_input_balance_sufficient"] = enriched["input_balance_sufficient"]
        enriched["inventory_input_token_program"] = input_entry.get("token_program_id")

    if output_entry:
        output_amount_raw = safe_int(output_entry.get("amount_raw"))
        output_initialized = bool(output_entry.get("initialized"))
        enriched["has_output_ata"] = True
        enriched["output_ata_initialized"] = output_initialized
        enriched["output_balance_raw"] = output_amount_raw
        enriched["inventory_output_token_program"] = output_entry.get("token_program_id")

    if enriched.get("would_admit"):
        output_ready_current_template = bool(
            mint_out == WSOL_MINT or (enriched.get("has_output_ata") and enriched.get("output_ata_initialized"))
        )
        spl_input_balance_sufficient = bool(enriched.get("input_balance_sufficient"))
        if not output_ready_current_template:
            enriched["execution_blocker"] = (
                "missing_output_ata" if not enriched.get("has_output_ata") else "output_ata_uninitialized"
            )
        elif not spl_input_balance_sufficient:
            if mint_in == WSOL_MINT and native_sol_wrap_possible:
                enriched["execution_blocker"] = "wsol_ata_balance_insufficient"
            elif mint_in == WSOL_MINT:
                enriched["execution_blocker"] = "native_sol_wrap_insufficient"
            else:
                enriched["execution_blocker"] = "spl_input_balance_insufficient"
        else:
            enriched["execution_blocker"] = "none"

    return enriched

def discover_scorecard():
    if explicit_scorecard:
        return Path(explicit_scorecard)
    if not search_root.exists():
        return None
    candidates = sorted(
        (
            path
            for path in search_root.rglob("surface_scorecard.json")
            if path.is_file() and result_dir not in path.parents
        ),
        key=lambda path: path.stat().st_mtime_ns,
        reverse=True,
    )
    return candidates[0] if candidates else None

def classify_run40_target_route_family(row: dict) -> str:
    route = (row.get("route") or "").strip().lower()
    route_reason = (row.get("route_reason") or "").strip().lower()
    route_family_label = (row.get("route_family_label") or "").strip().lower()
    route_tokens = [token for token in (route, route_reason, route_family_label) if token]
    joined = " ".join(route_tokens)
    if "tpu" in joined or "public" in joined:
        return "tpu_quic_public"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    for token in route_tokens:
        if token:
            return token
    return "unknown"

def derive_candidate_from_rows(scorecard: dict):
    scorecard_replay_mode = target_mode == "scorecard_selected_replay_tpu_nosend"
    watcher_mode = target_mode in {
        "any_meteora_hunt_tpu_convertible",
        "any_hunt_tpu_convertible",
        "any_exec_ready_source_relaxed_tpu_convertible",
    }
    meteora_watcher_mode = target_mode == "any_meteora_hunt_tpu_convertible"
    any_hunt_watcher_mode = target_mode == "any_hunt_tpu_convertible"
    source_relaxed_exec_ready_mode = target_mode == "any_exec_ready_source_relaxed_tpu_convertible"
    selection = scorecard.get("run40_candidate_selection") or {}
    selected_candidate = selection.get("selected_candidate")
    if (
        not watcher_mode
        and (
        isinstance(selected_candidate, dict)
        and selected_candidate.get("pool")
        )
    ):
        return (
            selected_candidate,
            selection.get("status") or "selected_from_scorecard",
            {
                "scope": "preselected_candidate",
                "observed": 1,
                "held_input": 1 if selected_candidate.get("held_input") else 0,
                "positive_ev": 1 if int(selected_candidate.get("net_after_fee_lamports") or 0) > 0 else 0,
                "input_balance_ok": 1 if selected_candidate.get("input_balance_ok") else 0,
                "output_account_ok": 1 if selected_candidate.get("output_account_ready") else 0,
                "template_ready": 1 if selected_candidate.get("template_ready") else 0,
                "tiny_cap_safe": 1 if selected_candidate.get("tiny_cap_safe") else 0,
                "tpu_convertible": 1 if selected_candidate.get("tpu_convertible_route") else 0,
                "eligible": 1,
                "first_blocker_counts": {},
            },
        )

    inventory = load_inventory(inventory_path)
    rows = [overlay_inventory_truth(row, inventory) for row in (scorecard.get("r1_surface_observations") or [])]
    venue_priority = selection.get("venue_priority") or DEFAULT_VENUE_PRIORITY
    venue_rank = {venue: index for index, venue in enumerate(venue_priority)}
    candidates = []
    for row in rows:
        proof_filter_result = text_value(row.get("proof_filter_result"))
        template_result = text_value(row.get("template_executability_result"))
        micro_market_result = text_value(row.get("micro_market_result"))
        r1_outcome = text_value(row.get("r1_outcome"))
        execution_blocker = text_value(row.get("execution_blocker"))
        source_value = text_value(row.get("source")).lower()
        source_afxdp = source_value == "afxdp"
        venue = text_value(row.get("venue")).lower()
        mint_out = row.get("mint_out") or ""
        approved_held_input = bool(row.get("is_approved_held_input"))
        input_account_ready = bool(row.get("has_input_ata") and row.get("input_ata_initialized"))
        input_balance_ok = bool(input_account_ready and row.get("input_balance_sufficient"))
        held_input = bool(approved_held_input and input_balance_ok)
        output_account_ready = bool(
            mint_out == WSOL_MINT
            or (row.get("has_output_ata") and row.get("output_ata_initialized"))
        )
        positive_ev = int(row.get("net_after_fee_lamports") or 0) > 0
        template_ready = template_result in {"passed", "not_applicable"}
        target_route_family = classify_run40_target_route_family(row)
        route_tpu_ready = target_route_family == "tpu_quic_public"
        quote_ready_or_classified = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
        tiny_cap_safe = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
        tiny_cap_blocker = classify_tiny_cap_blocker(row, max_tiny_lamports)
        source_relaxed_tiny_cap_safe = bool(
            tiny_cap_safe
            or (tiny_cap_blocker == "r1_not_admitted" and r1_outcome == "source")
        )
        effective_tiny_cap_safe = (
            source_relaxed_tiny_cap_safe
            if (source_relaxed_exec_ready_mode or scorecard_replay_mode)
            else tiny_cap_safe
        )
        tpu_convertible_route = target_route_family in TPU_CONVERTIBLE_ROUTE_FAMILIES
        hunt_like = bool(
            (row.get("classification") or "").strip().upper() == "HUNT"
            or (
                positive_ev
                and (bool(row.get("would_send")) or bool(row.get("live_send_suppressed")))
            )
        )
        strict_eligible = bool(
            source_afxdp
            and approved_held_input
            and positive_ev
            and input_balance_ok
            and output_account_ready
            and template_ready
            and route_tpu_ready
            and row.get("would_admit")
            and quote_ready_or_classified
            and tiny_cap_safe
        )
        fallback_no_quote_eligible = bool(
            source_afxdp
            and approved_held_input
            and positive_ev
            and input_balance_ok
            and output_account_ready
            and template_ready
            and route_tpu_ready
            and proof_filter_result == "no_quote"
        )
        meteora_hunt_tpu_convertible = bool(
            venue == "meteora_damm_v2"
            and hunt_like
            and held_input
            and positive_ev
            and input_balance_ok
            and output_account_ready
            and template_ready
            and quote_ready_or_classified
            and tiny_cap_safe
            and tpu_convertible_route
        )
        any_hunt_tpu_convertible = bool(
            hunt_like
            and held_input
            and positive_ev
            and input_balance_ok
            and output_account_ready
            and template_ready
            and quote_ready_or_classified
            and tiny_cap_safe
            and tpu_convertible_route
        )
        any_exec_ready_source_relaxed_tpu_convertible = bool(
            held_input
            and positive_ev
            and input_balance_ok
            and output_account_ready
            and template_ready
            and effective_tiny_cap_safe
            and tpu_convertible_route
        )
        candidate = dict(row)
        candidate.update(
            {
                "approved_held_input": approved_held_input,
                "source_afxdp": source_afxdp,
                "source_value": source_value,
                "held_input": held_input,
                "input_account_ready": input_account_ready,
                "input_balance_ok": input_balance_ok,
                "positive_ev": positive_ev,
                "template_ready": template_ready,
                "quote_ready_or_classified": quote_ready_or_classified,
                "tiny_cap_safe": effective_tiny_cap_safe,
                "raw_tiny_cap_safe": tiny_cap_safe,
                "tiny_cap_blocker": tiny_cap_blocker,
                "micro_market_result": micro_market_result,
                "r1_outcome": r1_outcome,
                "execution_blocker": execution_blocker,
                "output_account_ready": output_account_ready,
                "route_tpu_ready": route_tpu_ready,
                "tpu_convertible_route": tpu_convertible_route,
                "hunt_like": hunt_like,
                "selected_target_route_family": (
                    "tpu_quic_public" if (watcher_mode or scorecard_replay_mode) else target_route_family
                ),
                "target_route_family": target_route_family,
                "strict_eligible": strict_eligible,
                "fallback_no_quote_eligible": fallback_no_quote_eligible,
                "meteora_hunt_tpu_convertible": meteora_hunt_tpu_convertible,
                "any_hunt_tpu_convertible": any_hunt_tpu_convertible,
                "any_exec_ready_source_relaxed_tpu_convertible": any_exec_ready_source_relaxed_tpu_convertible,
                "scorecard_selected_replay_tpu_nosend": bool(
                    scorecard_replay_mode
                    and any_exec_ready_source_relaxed_tpu_convertible
                    and text_value(row.get("signature"))
                ),
                "source_relaxed_dev_proof": bool(
                    source_relaxed_exec_ready_mode or scorecard_replay_mode
                ),
                "scorecard_replay_dev_proof": scorecard_replay_mode,
                "live_market_recurrence_required": not scorecard_replay_mode,
                "canonical_private_grade": bool(
                    source_afxdp and not (source_relaxed_exec_ready_mode or scorecard_replay_mode)
                ),
                "venue_priority_rank": venue_rank.get(
                    row.get("venue") or "", len(venue_priority)
                ),
                "source_rank": 0 if source_afxdp else 1,
            }
        )
        candidates.append(candidate)

    if meteora_watcher_mode:
        autopsy_scope_rows = [
            item for item in candidates if (item.get("venue") or "").strip().lower() == "meteora_damm_v2"
        ]
        autopsy_scope = "meteora_observed_rows"
    elif any_hunt_watcher_mode:
        autopsy_scope_rows = [item for item in candidates if item.get("hunt_like")]
        autopsy_scope = "hunt_rows_all_venues"
    elif source_relaxed_exec_ready_mode:
        autopsy_scope_rows = list(candidates)
        autopsy_scope = "exec_ready_rows_all_sources"
    elif scorecard_replay_mode:
        autopsy_scope_rows = list(candidates)
        autopsy_scope = "scorecard_replay_rows_all_sources"
    else:
        autopsy_scope_rows = list(candidates)
        autopsy_scope = "all_rows"

    autopsy_stage_counts = {"observed": len(autopsy_scope_rows)}
    autopsy_rows = list(autopsy_scope_rows)
    autopsy_filters = [
        ("held_input", lambda item: item.get("held_input")),
        ("positive_ev", lambda item: item.get("positive_ev")),
        ("input_balance_ok", lambda item: item.get("input_balance_ok")),
        ("output_account_ok", lambda item: item.get("output_account_ready")),
        ("template_ready", lambda item: item.get("template_ready")),
        ("tiny_cap_safe", lambda item: item.get("tiny_cap_safe")),
        ("tpu_convertible", lambda item: item.get("tpu_convertible_route")),
    ]
    for stage_name, predicate in autopsy_filters:
        autopsy_rows = [item for item in autopsy_rows if predicate(item)]
        autopsy_stage_counts[stage_name] = len(autopsy_rows)

    autopsy_first_blocker_counts = {}
    for item in autopsy_scope_rows:
        blocker = None
        if not item.get("held_input"):
            blocker = "held_input"
        elif not item.get("positive_ev"):
            blocker = "positive_ev"
        elif not item.get("input_balance_ok"):
            blocker = "input_balance_ok"
        elif not item.get("output_account_ready"):
            blocker = "output_account_ok"
        elif not item.get("template_ready"):
            blocker = "template_ready"
        elif not item.get("tiny_cap_safe"):
            blocker = "tiny_cap_safe"
        elif not item.get("tpu_convertible_route"):
            blocker = "tpu_convertible"
        if blocker:
            autopsy_first_blocker_counts[blocker] = autopsy_first_blocker_counts.get(blocker, 0) + 1

    autopsy_summary = {
        "scope": autopsy_scope,
        "observed": autopsy_stage_counts.get("observed", 0),
        "held_input": autopsy_stage_counts.get("held_input", 0),
        "positive_ev": autopsy_stage_counts.get("positive_ev", 0),
        "input_balance_ok": autopsy_stage_counts.get("input_balance_ok", 0),
        "output_account_ok": autopsy_stage_counts.get("output_account_ok", 0),
        "template_ready": autopsy_stage_counts.get("template_ready", 0),
        "tiny_cap_safe": autopsy_stage_counts.get("tiny_cap_safe", 0),
        "tpu_convertible": autopsy_stage_counts.get("tpu_convertible", 0),
        "eligible": len(autopsy_rows),
        "first_blocker_counts": autopsy_first_blocker_counts,
    }

    candidates.sort(
        key=lambda item: (
            0
            if item.get("meteora_hunt_tpu_convertible") and meteora_watcher_mode
            else 1
            if item.get("any_hunt_tpu_convertible") and any_hunt_watcher_mode
            else 2
            if item.get("any_exec_ready_source_relaxed_tpu_convertible")
            and (source_relaxed_exec_ready_mode or scorecard_replay_mode)
            else 3
            if item.get("strict_eligible")
            else 4,
            int(item.get("source_rank", 1)),
            -int(item.get("net_after_fee_lamports") or 0),
            -int(item.get("retained_ev_lamports") or 0),
            -int(item.get("slot") or 0),
            int(item.get("venue_priority_rank", len(venue_priority))),
        )
    )
    if meteora_watcher_mode:
        selected_candidate = next(
            (item for item in candidates if item.get("meteora_hunt_tpu_convertible")), None
        )
        if selected_candidate is not None:
            return selected_candidate, "meteora_hunt_tpu_convertible_selected", autopsy_summary
        return None, "no_meteora_hunt_tpu_convertible_candidate", autopsy_summary
    if any_hunt_watcher_mode:
        selected_candidate = next(
            (item for item in candidates if item.get("any_hunt_tpu_convertible")), None
        )
        if selected_candidate is not None:
            return selected_candidate, "any_hunt_tpu_convertible_selected", autopsy_summary
        return None, "no_any_hunt_tpu_convertible_candidate", autopsy_summary
    if source_relaxed_exec_ready_mode:
        selected_candidate = next(
            (item for item in candidates if item.get("any_exec_ready_source_relaxed_tpu_convertible")), None
        )
        if selected_candidate is not None:
            return selected_candidate, "any_exec_ready_source_relaxed_tpu_convertible_selected", autopsy_summary
        return None, "no_any_exec_ready_source_relaxed_tpu_convertible_candidate", autopsy_summary
    if scorecard_replay_mode:
        selected_candidate = next(
            (item for item in candidates if item.get("scorecard_selected_replay_tpu_nosend")), None
        )
        if selected_candidate is not None:
            return selected_candidate, "scorecard_selected_replay_selected", autopsy_summary
        return None, "no_scorecard_selected_replay_candidate", autopsy_summary
    selected_candidate = next((item for item in candidates if item.get("strict_eligible")), None)
    if selected_candidate is not None:
        return selected_candidate, "strict_quote_ready_selected", autopsy_summary
    selected_candidate = next(
        (item for item in candidates if item.get("fallback_no_quote_eligible")), None
    )
    if selected_candidate is not None:
        return selected_candidate, "fallback_no_quote_classified_selected", autopsy_summary
    return None, "no_eligible_candidate", autopsy_summary

scorecard_path = discover_scorecard()
scorecard_path_str = str(scorecard_path) if scorecard_path else ""
explicit_scorecard_path = str(Path(explicit_scorecard).resolve()) if explicit_scorecard else ""
resolved_scorecard_path = str(scorecard_path.resolve()) if scorecard_path and scorecard_path.exists() else ""
current_scorecard_loaded = bool(
    explicit_scorecard_path
    and resolved_scorecard_path
    and explicit_scorecard_path == resolved_scorecard_path
)
stale_scorecard_used = bool(
    scorecard_path_str
    and not current_scorecard_loaded
    and "current" not in scorecard_path_str.lower()
)
payload = {
    "target_mode": target_mode,
    "scorecard_path": scorecard_path_str,
    "current_scorecard_loaded": current_scorecard_loaded,
    "stale_scorecard_used": stale_scorecard_used,
    "status": "missing_scorecard",
    "selected_candidate": None,
    "gate_autopsy": None,
}
env_lines = [
    shell_line("SHADOW_RUN40_SELECTION_STATUS", "missing_scorecard"),
    shell_line("SHADOW_RUN40_SELECTED_SCORECARD", ""),
    shell_line("SHADOW_RUN40_SCORECARD_REPLAY_SPEC", ""),
    shell_line("SHADOW_RUN40_CURRENT_SCORECARD_LOADED", "false"),
    shell_line("SHADOW_RUN40_STALE_SCORECARD_USED", "false"),
    shell_line("SHADOW_RUN40_SELECTION_ATTEMPTED", "false"),
    shell_line(
        "SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF",
        "true"
        if target_mode in {"any_exec_ready_source_relaxed_tpu_convertible", "scorecard_selected_replay_tpu_nosend"}
        else "false",
    ),
    shell_line(
        "SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF",
        "true" if target_mode == "scorecard_selected_replay_tpu_nosend" else "false",
    ),
    shell_line(
        "SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED",
        "false" if target_mode == "scorecard_selected_replay_tpu_nosend" else "true",
    ),
    shell_line("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "false"),
]

if scorecard_path is not None and scorecard_path.exists():
    scorecard = load_json(scorecard_path)
    scorecard_copy.write_text(scorecard_path.read_text(encoding="utf-8"), encoding="utf-8")
    selected_candidate, selection_status, gate_autopsy = derive_candidate_from_rows(scorecard)
    payload["status"] = selection_status
    payload["selected_candidate"] = selected_candidate
    payload["gate_autopsy"] = gate_autopsy
    env_lines = [
        shell_line("SHADOW_RUN40_SELECTION_STATUS", selection_status),
        shell_line("SHADOW_RUN40_SELECTED_SCORECARD", str(scorecard_path)),
        shell_line("SHADOW_RUN40_SCORECARD_REPLAY_SPEC", ""),
        shell_line("SHADOW_RUN40_CURRENT_SCORECARD_LOADED", "true" if current_scorecard_loaded else "false"),
        shell_line("SHADOW_RUN40_STALE_SCORECARD_USED", "true" if stale_scorecard_used else "false"),
        shell_line("SHADOW_RUN40_SELECTION_ATTEMPTED", "true"),
        shell_line(
            "SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF",
            "true"
            if target_mode in {"any_exec_ready_source_relaxed_tpu_convertible", "scorecard_selected_replay_tpu_nosend"}
            else "false",
        ),
        shell_line(
            "SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF",
            "true" if target_mode == "scorecard_selected_replay_tpu_nosend" else "false",
        ),
        shell_line(
            "SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED",
            "false" if target_mode == "scorecard_selected_replay_tpu_nosend" else "true",
        ),
        shell_line("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "false"),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_SCOPE", gate_autopsy.get("scope") or ""),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_OBSERVED", str(gate_autopsy.get("observed", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_HELD_INPUT", str(gate_autopsy.get("held_input", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_POSITIVE_EV", str(gate_autopsy.get("positive_ev", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_INPUT_BALANCE_OK", str(gate_autopsy.get("input_balance_ok", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_OUTPUT_ACCOUNT_OK", str(gate_autopsy.get("output_account_ok", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_TEMPLATE_READY", str(gate_autopsy.get("template_ready", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_TINY_CAP_SAFE", str(gate_autopsy.get("tiny_cap_safe", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_TPU_CONVERTIBLE", str(gate_autopsy.get("tpu_convertible", 0))),
        shell_line("SHADOW_RUN40_GATE_AUTOPSY_ELIGIBLE", str(gate_autopsy.get("eligible", 0))),
    ]
    if selected_candidate is not None:
        env_lines.extend(
            [
                shell_line("SHADOW_RUN40_ALLOW_ONLY_VENUE_ID", selected_candidate.get("venue") or ""),
                shell_line("SHADOW_RUN40_ALLOW_ONLY_POOL", selected_candidate.get("pool") or ""),
                shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_IN", selected_candidate.get("mint_in") or ""),
                shell_line("SHADOW_RUN40_ALLOW_ONLY_MINT_OUT", selected_candidate.get("mint_out") or ""),
                shell_line("SHADOW_RUN40_SELECTED_VENUE", selected_candidate.get("venue") or ""),
                shell_line("SHADOW_RUN40_SELECTED_POOL", selected_candidate.get("pool") or ""),
                shell_line("SHADOW_RUN40_SELECTED_MINT_IN", selected_candidate.get("mint_in") or ""),
                shell_line("SHADOW_RUN40_SELECTED_MINT_OUT", selected_candidate.get("mint_out") or ""),
                shell_line("SHADOW_RUN40_SELECTED_SIGNATURE", selected_candidate.get("signature") or ""),
                shell_line("SHADOW_RUN40_SELECTED_ROUTE", selected_candidate.get("route") or ""),
                shell_line("SHADOW_RUN40_SELECTED_SOURCE", selected_candidate.get("source") or ""),
                shell_line(
                    "SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY",
                    selected_candidate.get("selected_target_route_family")
                    or selected_candidate.get("target_route_family")
                    or "tpu_quic_public",
                ),
                shell_line(
                    "SHADOW_RUN40_SELECTED_HELD_INPUT",
                    "true" if selected_candidate.get("held_input") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SELECTED_TEMPLATE_READY",
                    "true" if selected_candidate.get("template_ready") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED",
                    "true" if selected_candidate.get("quote_ready_or_classified") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SELECTED_TINY_CAP_SAFE",
                    "true" if selected_candidate.get("tiny_cap_safe") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY",
                    "true" if selected_candidate.get("output_account_ready") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF",
                    "true" if selected_candidate.get("source_relaxed_dev_proof") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF",
                    "true" if selected_candidate.get("scorecard_replay_dev_proof") else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED",
                    "true" if selected_candidate.get("live_market_recurrence_required", True) else "false",
                ),
                shell_line(
                    "SHADOW_RUN40_CANONICAL_PRIVATE_GRADE",
                    "true" if selected_candidate.get("canonical_private_grade") else "false",
                ),
            ]
        )
        if target_mode == "scorecard_selected_replay_tpu_nosend":
            replay_spec = {
                "signature": selected_candidate.get("signature") or "",
                "signal_source": selected_candidate.get("source") or "",
                "slot": safe_int(selected_candidate.get("slot")),
                "unix_ns": safe_int(selected_candidate.get("unix_ns")),
                "pool": selected_candidate.get("pool") or "",
                "mint_in": selected_candidate.get("mint_in") or "",
                "mint_out": selected_candidate.get("mint_out") or "",
            }
            replay_spec_path.write_text(json.dumps(replay_spec, indent=2) + "\n", encoding="utf-8")
            env_lines.append(shell_line("SHADOW_RUN40_SCORECARD_REPLAY_SPEC", str(replay_spec_path)))

env_path.write_text("\n".join(env_lines) + "\n", encoding="utf-8")
json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

if payload["status"] == "missing_scorecard":
    sys.exit(2)
if payload["selected_candidate"] is None:
    sys.exit(3)
sys.exit(0)
PY
}

run_probe() {
    local mode="$1"
    set +e
    if [[ "$mode" == "tiny_live" && "$TARGET_MODE" == "scorecard_selected_replay_tpu_nosend" ]]; then
        env -u SHADOW_RUN40_SELECTED_SIGNATURE \
            "$TPU_BIN" \
            --config "$CONFIG" \
            --mode "$mode" \
            --rpc-url "$RPC_URL" \
            --summary-file "$TPU_SUMMARY_FILE" \
            >"$TPU_LOG" 2>&1
    else
        "$TPU_BIN" \
            --config "$CONFIG" \
            --mode "$mode" \
            --rpc-url "$RPC_URL" \
            --summary-file "$TPU_SUMMARY_FILE" \
            >"$TPU_LOG" 2>&1
    fi
    rc=$?
    set -e
    return "$rc"
}

render_no_send_summary() {
    python3 - "$TPU_SUMMARY_FILE" "$RESULT_GATE_INPUT" "$SUMMARY_FILE" "$1" <<'PY'
import json
import os
import pathlib
import sys

tpu_path = pathlib.Path(sys.argv[1])
gate_path = pathlib.Path(sys.argv[2])
out_path = pathlib.Path(sys.argv[3])

def parse_kv(path: pathlib.Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data

def parse_json(path: pathlib.Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

tpu = parse_kv(tpu_path)
gate = parse_json(gate_path)
target_mode = (sys.argv[4:] + [""])[0] if len(sys.argv) > 4 else ""

def as_int(mapping, key):
    value = mapping.get(key, 0)
    if isinstance(value, bool):
        return int(value)
    try:
        return int(value)
    except Exception:
        return 0

status = tpu.get("status", "")
candidate_seen = as_int(gate, "tpu_quic_candidate_seen")
would_enqueue = as_int(gate, "tpu_quic_would_enqueue")
ring_enqueue_ok = as_int(gate, "tpu_quic_ring_enqueue_ok")
ring_enqueue_err = as_int(gate, "tpu_quic_ring_enqueue_err")
dequeue_count = as_int(tpu, "tpu_quic_sender_dequeue_count")
accept_count = as_int(tpu, "tpu_quic_no_send_accept_count")
pm_reserve_sim = max(
    as_int(gate, "tpu_quic_pm_reserve_simulated"),
    as_int(tpu, "tpu_quic_pm_reserve_simulated"),
)
pm_release_sim = as_int(tpu, "tpu_quic_pm_release_simulated")
wire_send_count = as_int(tpu, "tpu_wire_send_count")
live_landed = as_int(gate, "live_landed")

if (
    candidate_seen > 0
    and would_enqueue > 0
    and ring_enqueue_ok > 0
    and dequeue_count > 0
    and accept_count > 0
    and pm_reserve_sim > 0
    and pm_release_sim > 0
    and wire_send_count == 0
    and live_landed == 0
):
    merged_status = "run40_no_send_ready"
elif (
    candidate_seen == 0
    and would_enqueue == 0
    and ring_enqueue_ok == 0
    and ring_enqueue_err == 0
    and dequeue_count == 0
    and accept_count == 0
    and wire_send_count == 0
    and live_landed == 0
    and tpu.get("tpu_ring_ready") == "true"
    and tpu.get("tpu_result_ring_ready") == "true"
    and tpu.get("nack_ring_ready") == "true"
    and tpu.get("local_rpc_status_ok") == "true"
):
    merged_status = "RUN40_NO_SEND_NO_CANDIDATE"
elif status:
    merged_status = status
else:
    merged_status = "run40_no_send_incomplete"

lines = [
    f"status={merged_status}",
    "mode=no_send",
    f"target_mode={target_mode}",
    f"canonical_private_grade={os.environ.get('SHADOW_RUN40_CANONICAL_PRIVATE_GRADE', 'false')}",
    f"source_relaxed_dev_proof={os.environ.get('SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF', 'false')}",
    f"scorecard_replay_dev_proof={os.environ.get('SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF', 'false')}",
    f"live_market_recurrence_required={os.environ.get('SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED', 'true')}",
    f"selection_status={os.environ.get('SHADOW_RUN40_SELECTION_STATUS', '')}",
    f"surface_scorecard={os.environ.get('SHADOW_RUN40_SELECTED_SCORECARD', '')}",
    f"scorecard_replay_spec={os.environ.get('SHADOW_RUN40_SCORECARD_REPLAY_SPEC', '')}",
    f"current_scorecard_loaded={os.environ.get('SHADOW_RUN40_CURRENT_SCORECARD_LOADED', 'false')}",
    f"stale_scorecard_used={os.environ.get('SHADOW_RUN40_STALE_SCORECARD_USED', 'false')}",
    f"selection_attempted={os.environ.get('SHADOW_RUN40_SELECTION_ATTEMPTED', 'false')}",
    f"gate_autopsy_scope={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_SCOPE', '')}",
    f"gate_autopsy_observed={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_OBSERVED', '0')}",
    f"gate_autopsy_held_input={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_HELD_INPUT', '0')}",
    f"gate_autopsy_positive_ev={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_POSITIVE_EV', '0')}",
    f"gate_autopsy_input_balance_ok={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_INPUT_BALANCE_OK', '0')}",
    f"gate_autopsy_output_account_ok={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_OUTPUT_ACCOUNT_OK', '0')}",
    f"gate_autopsy_template_ready={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_TEMPLATE_READY', '0')}",
    f"gate_autopsy_tiny_cap_safe={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_TINY_CAP_SAFE', '0')}",
    f"gate_autopsy_tpu_convertible={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_TPU_CONVERTIBLE', '0')}",
    f"gate_autopsy_eligible={os.environ.get('SHADOW_RUN40_GATE_AUTOPSY_ELIGIBLE', '0')}",
    f"selected_venue={os.environ.get('SHADOW_RUN40_SELECTED_VENUE', '')}",
    f"selected_pool={os.environ.get('SHADOW_RUN40_SELECTED_POOL', '')}",
    f"selected_mint_in={os.environ.get('SHADOW_RUN40_SELECTED_MINT_IN', '')}",
    f"selected_mint_out={os.environ.get('SHADOW_RUN40_SELECTED_MINT_OUT', '')}",
    f"selected_signature={os.environ.get('SHADOW_RUN40_SELECTED_SIGNATURE', '')}",
    f"selected_route={os.environ.get('SHADOW_RUN40_SELECTED_ROUTE', '')}",
    f"selected_source={os.environ.get('SHADOW_RUN40_SELECTED_SOURCE', '')}",
    f"selected_target_route_family={os.environ.get('SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY', '')}",
    f"selected_held_input={os.environ.get('SHADOW_RUN40_SELECTED_HELD_INPUT', 'false')}",
    f"selected_template_ready={os.environ.get('SHADOW_RUN40_SELECTED_TEMPLATE_READY', 'false')}",
    f"selected_quote_ready_or_classified={os.environ.get('SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED', 'false')}",
    f"selected_tiny_cap_safe={os.environ.get('SHADOW_RUN40_SELECTED_TINY_CAP_SAFE', 'false')}",
    f"selected_output_account_ready={os.environ.get('SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY', 'false')}",
    f"probe_status={status}",
    f"tpu_quic_sender_binary_exists={tpu.get('tpu_quic_sender_binary_exists', 'false')}",
    f"tpu_ring_ready={tpu.get('tpu_ring_ready', 'false')}",
    f"tpu_result_ring_ready={tpu.get('tpu_result_ring_ready', 'false')}",
    f"nack_ring_ready={tpu.get('nack_ring_ready', 'false')}",
    f"local_rpc_status_ok={tpu.get('local_rpc_status_ok', 'false')}",
    f"getSlot_ok={tpu.get('getSlot_ok', 'false')}",
    f"getSlotLeaders_ok={tpu.get('getSlotLeaders_ok', 'false')}",
    f"getClusterNodes_ok={tpu.get('getClusterNodes_ok', 'false')}",
    f"getLatestBlockhash_ok={tpu.get('getLatestBlockhash_ok', 'false')}",
    f"leader_contact_count={tpu.get('leader_contact_count', '0')}",
    f"tpu_quic_candidate_seen={candidate_seen}",
    f"tpu_quic_would_enqueue={would_enqueue}",
    f"tpu_quic_ring_enqueue_ok={ring_enqueue_ok}",
    f"tpu_quic_ring_enqueue_err={ring_enqueue_err}",
    f"tpu_quic_sender_dequeue_count={dequeue_count}",
    f"tpu_quic_no_send_accept_count={accept_count}",
    f"tpu_quic_pm_reserve_simulated={pm_reserve_sim}",
    f"tpu_quic_pm_release_simulated={pm_release_sim}",
    f"tpu_quic_last_signature={gate.get('tpu_quic_last_signature', tpu.get('tpu_quic_last_signature', ''))}",
    f"tpu_quic_last_route_family={gate.get('tpu_quic_last_route_family', tpu.get('tpu_quic_last_route_family', ''))}",
    f"scorecard_replay_stale_slot_bypass_seen={gate.get('scorecard_replay_stale_slot_bypass_seen', 0)}",
    f"scorecard_replay_stale_slot_bypass_applied={gate.get('scorecard_replay_stale_slot_bypass_applied', 0)}",
    f"scorecard_replay_stale_slot_bypass_signature_match={gate.get('scorecard_replay_stale_slot_bypass_signature_match', 0)}",
    f"scorecard_replay_stale_slot_bypass_pool_match={gate.get('scorecard_replay_stale_slot_bypass_pool_match', 0)}",
    f"scorecard_replay_original_slot={gate.get('scorecard_replay_original_slot', 0)}",
    f"scorecard_replay_current_slot={gate.get('scorecard_replay_current_slot', 0)}",
    f"scorecard_replay_slot_gap={gate.get('scorecard_replay_slot_gap', 0)}",
    f"scorecard_replay_route_override_seen={gate.get('scorecard_replay_route_override_seen', 0)}",
    f"scorecard_replay_route_override_applied={gate.get('scorecard_replay_route_override_applied', 0)}",
    f"scorecard_replay_route_override_signature_match={gate.get('scorecard_replay_route_override_signature_match', 0)}",
    f"scorecard_replay_route_override_pool_match={gate.get('scorecard_replay_route_override_pool_match', 0)}",
    f"scorecard_replay_route_override_from_public_target={gate.get('scorecard_replay_route_override_from_public_target', 0)}",
    f"scorecard_replay_route_override_to_tpu_quic_public={gate.get('scorecard_replay_route_override_to_tpu_quic_public', 0)}",
    f"scorecard_replay_source_override_seen={gate.get('scorecard_replay_source_override_seen', 0)}",
    f"scorecard_replay_source_override_applied={gate.get('scorecard_replay_source_override_applied', 0)}",
    f"scorecard_replay_source_override_signature_match={gate.get('scorecard_replay_source_override_signature_match', 0)}",
    f"scorecard_replay_source_override_pool_match={gate.get('scorecard_replay_source_override_pool_match', 0)}",
    f"scorecard_replay_source_override_live_send_blocked={gate.get('scorecard_replay_source_override_live_send_blocked', 0)}",
    f"worker_template_build_ok={gate.get('worker_template_build_ok', 0)}",
    f"worker_no_send_backend_ok={gate.get('worker_no_send_backend_ok', 0)}",
    f"worker_pm_reservation_created={gate.get('worker_pm_reservation_created', 0)}",
    f"pm_reservation_released={gate.get('pm_reservation_released', 0)}",
    f"tpu_wire_send_count={wire_send_count}",
    f"live_landed={live_landed}",
]

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
}

render_tiny_live_summary() {
    python3 - "$TPU_SUMMARY_FILE" "$RESULT_GATE_INPUT" "$SUMMARY_FILE" "$1" <<'PY'
import json
import pathlib
import os
import sys

tpu_path = pathlib.Path(sys.argv[1])
gate_path = pathlib.Path(sys.argv[2])
out_path = pathlib.Path(sys.argv[3])
target_mode = (sys.argv[4:] + [""])[0] if len(sys.argv) > 4 else ""

def parse_kv(path: pathlib.Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data

def parse_json(path: pathlib.Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

def parse_int(value, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default

tpu = parse_kv(tpu_path)
gate = parse_json(gate_path)
status = tpu.get("status") or "run40_tiny_live_incomplete"
pm_release_or_landed_reconcile = 1 if (
    parse_int(gate.get("pm_reservation_released"), 0) > 0
    or parse_int(gate.get("live_landed", tpu.get("live_landed")), 0) > 0
) else 0
if status == "blocked_pm_live_reconcile_unimplemented" and pm_release_or_landed_reconcile:
    if parse_int(gate.get("live_landed", tpu.get("live_landed")), 0) > 0:
        status = "tpu_quic_tiny_live_landed_reconciled"
    else:
        status = "tpu_quic_tiny_live_terminal_reconciled"

lines = []
seen = set()

def push(key: str, value):
    if key in seen:
        return
    seen.add(key)
    lines.append(f"{key}={value}")

push("status", status)
push("mode", "tiny_live")
push("target_mode", target_mode)
push("canonical_private_grade", os.environ.get("SHADOW_RUN40_CANONICAL_PRIVATE_GRADE", "false"))
push("source_relaxed_dev_proof", os.environ.get("SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF", "false"))
push("scorecard_replay_dev_proof", os.environ.get("SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF", "false"))
push("live_market_recurrence_required", os.environ.get("SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED", "true"))
push("selection_status", os.environ.get("SHADOW_RUN40_SELECTION_STATUS", ""))
push("surface_scorecard", os.environ.get("SHADOW_RUN40_SELECTED_SCORECARD", ""))
push("scorecard_replay_spec", os.environ.get("SHADOW_RUN40_SCORECARD_REPLAY_SPEC", ""))
push("current_scorecard_loaded", os.environ.get("SHADOW_RUN40_CURRENT_SCORECARD_LOADED", "false"))
push("stale_scorecard_used", os.environ.get("SHADOW_RUN40_STALE_SCORECARD_USED", "false"))
push("selection_attempted", os.environ.get("SHADOW_RUN40_SELECTION_ATTEMPTED", "false"))
push("gate_autopsy_scope", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_SCOPE", ""))
push("gate_autopsy_observed", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_OBSERVED", "0"))
push("gate_autopsy_held_input", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_HELD_INPUT", "0"))
push("gate_autopsy_positive_ev", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_POSITIVE_EV", "0"))
push("gate_autopsy_input_balance_ok", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_INPUT_BALANCE_OK", "0"))
push("gate_autopsy_output_account_ok", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_OUTPUT_ACCOUNT_OK", "0"))
push("gate_autopsy_template_ready", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_TEMPLATE_READY", "0"))
push("gate_autopsy_tiny_cap_safe", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_TINY_CAP_SAFE", "0"))
push("gate_autopsy_tpu_convertible", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_TPU_CONVERTIBLE", "0"))
push("gate_autopsy_eligible", os.environ.get("SHADOW_RUN40_GATE_AUTOPSY_ELIGIBLE", "0"))
push("selected_venue", os.environ.get("SHADOW_RUN40_SELECTED_VENUE", ""))
push("selected_pool", os.environ.get("SHADOW_RUN40_SELECTED_POOL", ""))
push("selected_mint_in", os.environ.get("SHADOW_RUN40_SELECTED_MINT_IN", ""))
push("selected_mint_out", os.environ.get("SHADOW_RUN40_SELECTED_MINT_OUT", ""))
push("selected_signature", os.environ.get("SHADOW_RUN40_SELECTED_SIGNATURE", ""))
push("selected_route", os.environ.get("SHADOW_RUN40_SELECTED_ROUTE", ""))
push("selected_source", os.environ.get("SHADOW_RUN40_SELECTED_SOURCE", ""))
push("selected_target_route_family", os.environ.get("SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY", ""))
push("selected_held_input", os.environ.get("SHADOW_RUN40_SELECTED_HELD_INPUT", "false"))
push("selected_template_ready", os.environ.get("SHADOW_RUN40_SELECTED_TEMPLATE_READY", "false"))
push("selected_quote_ready_or_classified", os.environ.get("SHADOW_RUN40_SELECTED_QUOTE_READY_OR_CLASSIFIED", "false"))
push("selected_tiny_cap_safe", os.environ.get("SHADOW_RUN40_SELECTED_TINY_CAP_SAFE", "false"))
push("selected_output_account_ready", os.environ.get("SHADOW_RUN40_SELECTED_OUTPUT_ACCOUNT_READY", "false"))
push("pm_release_or_landed_reconcile", pm_release_or_landed_reconcile)
push("probe_status", status)

for key, value in tpu.items():
    push(key, value)

for key in (
    "tpu_quic_candidate_seen",
    "tpu_quic_would_enqueue",
    "tpu_quic_ring_enqueue_ok",
    "tpu_quic_ring_enqueue_err",
    "worker_template_build_ok",
    "worker_pm_reservation_created",
    "pm_reservation_released",
    "live_landed",
    "scorecard_replay_stale_slot_bypass_seen",
    "scorecard_replay_stale_slot_bypass_applied",
    "scorecard_replay_stale_slot_bypass_signature_match",
    "scorecard_replay_stale_slot_bypass_pool_match",
    "scorecard_replay_original_slot",
    "scorecard_replay_current_slot",
    "scorecard_replay_slot_gap",
    "scorecard_replay_route_override_seen",
    "scorecard_replay_route_override_applied",
    "scorecard_replay_route_override_signature_match",
    "scorecard_replay_route_override_pool_match",
    "scorecard_replay_route_override_from_public_target",
    "scorecard_replay_route_override_to_tpu_quic_public",
    "scorecard_replay_source_override_seen",
    "scorecard_replay_source_override_applied",
    "scorecard_replay_source_override_signature_match",
    "scorecard_replay_source_override_pool_match",
    "scorecard_replay_source_override_live_send_blocked",
):
    if key in gate:
        push(key, gate.get(key))

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
}

prepare_tpu_probe_runtime() {
    if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
        finish
        exit 1
    fi
    case "$TARGET_MODE" in
        any_held_input_tpu_template_ready|any_meteora_hunt_tpu_convertible|any_hunt_tpu_convertible|any_exec_ready_source_relaxed_tpu_convertible|scorecard_selected_replay_tpu_nosend)
        set +e
        select_run40_target_from_scorecard
        selection_rc=$?
        set -e
        if [[ -f "$TARGET_SELECTION_ENV" ]]; then
            # shellcheck disable=SC1090
            source "$TARGET_SELECTION_ENV"
        fi
        case "$selection_rc" in
            0)
                ;;
            2)
                write_selection_summary "blocked_missing_run39_scorecard"
                fail "Run40 target selection requires a Run39 surface_scorecard.json"
                finish
                exit 1
                ;;
            3)
                if [[ "$MODE" == "tiny_live" ]]; then
                    write_selection_summary "RUN40_TINY_LIVE_NO_CANDIDATE"
                else
                    write_selection_summary "RUN40_NO_SEND_NO_CANDIDATE"
                fi
                pass "Run40 $MODE: no eligible multi-venue candidate selected from current Run39 scorecard"
                finish
                exit 0
                ;;
            *)
                write_selection_summary "blocked_target_selection_failed"
                fail "Run40 target selection failed"
                finish
                exit 1
                ;;
        esac
        ;;
    esac
    export SHADOW_RUN40_ENABLE_TPU_QUIC=true
    export SHADOW_RUN40_MODE="$MODE"
    export SHADOW_RUN40_MAX_INPUT_SOL="$MAX_INPUT_SOL"
    export SHADOW_RUN40_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
    export SHADOW_RUN40_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"
    export SHADOW_RUN40_MAX_ATTEMPTS="$MAX_ATTEMPTS"
    export SHADOW_RUN40_STOP_AFTER_FIRST_LANDED="$STOP_AFTER_FIRST_LANDED"
    export SHADOW_RUN40_ENABLE_LIVE_SEND="$ENABLE_LIVE_SEND"
    ensure_oracle_ready
    if [[ "$MODE" == "no_send" ]] && [[ -z "${SHADOW_RUN40_NO_SEND_IDLE_POLLS:-}" ]]; then
        export SHADOW_RUN40_NO_SEND_IDLE_POLLS="$(( (WINDOW_SECONDS * 1000 + NO_SEND_POLL_MS - 1) / NO_SEND_POLL_MS ))"
    fi

    tier0_start_epoch="$(date +%s)"
    (
        cd "$PACK_DIR"
        exec "$TIER0_BIN" --config "$CONFIG" >"$TIER0_LOG" 2>&1
    ) &
    TIER0_PID=$!

    deadline=$((SECONDS + READINESS_TIMEOUT))
    probe_ready=false
    while (( SECONDS < deadline )); do
        if ! kill -0 "$TIER0_PID" 2>/dev/null; then
            fail "shadow-tier0 exited before Run40 no-send started"
            tail -n 80 "$TIER0_LOG" || true
            finish
            exit 1
        fi
        if tpu_probe_rings_ready "$tier0_start_epoch"; then
            probe_ready=true
            break
        fi
        sleep 1
    done

    if [[ "$probe_ready" != true ]]; then
        fail "shadow-tier0 did not recreate TPU probe rings before readiness timeout"
        tail -n 80 "$TIER0_LOG" || true
        finish
        exit 1
    fi
}

if [[ "$MODE" == "no_send" || "$MODE" == "tiny_live" ]]; then
    prepare_tpu_probe_runtime
    if run_probe "$MODE"; then
        rc=0
    else
        rc=$?
    fi
    if [[ "$MODE" == "tiny_live" ]]; then
        gate_mtime_before_wait="$(gate_file_mtime "$GATE_INPUT")"
        wait_for_tiny_live_pm_reconcile "$gate_mtime_before_wait" "$TIER0_PID" || true
    fi
    copy_gate_artifacts
    if [[ "$MODE" == "no_send" ]]; then
        render_no_send_summary "$TARGET_MODE"
    else
        render_tiny_live_summary "$TARGET_MODE"
        final_status="$(summary_status "$SUMMARY_FILE")"
        if [[ "$final_status" == "tpu_quic_tiny_live_terminal_reconciled" || "$final_status" == "tpu_quic_tiny_live_landed_reconciled" ]]; then
            rc=0
        fi
    fi
else
    if run_probe "$MODE"; then
        rc=0
    else
        rc=$?
    fi
    [[ -f "$TPU_SUMMARY_FILE" ]] && cp "$TPU_SUMMARY_FILE" "$SUMMARY_FILE"
fi

if [[ $rc -ne 0 ]]; then
    fail "Run40 $MODE probe failed"
    finish
    exit "$rc"
fi

pass "Run40 $MODE probe completed"
finish
exit 0

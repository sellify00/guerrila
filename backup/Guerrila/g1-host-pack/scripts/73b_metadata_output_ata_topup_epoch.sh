#!/usr/bin/env bash
# 73b_metadata_output_ata_topup_epoch.sh - narrow metadata-only Token-2022 output ATA precreation + minimal WSOL top-up + fresh epoch rerun

source "$(dirname "$0")/lib.sh"
script_header "73B - Metadata Output ATA + WSOL Top-Up Epoch"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT_PATH="$PACK_DIR/scripts/$(basename "$0")"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"

MODE="${SHADOW_RUN73B_MODE:-full}"
RPC_URL="${SHADOW_RUN73B_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
SIGNER_PATH="${SHADOW_RUN73B_SIGNER_PATH:-${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}}"
INVENTORY_PATH="${SHADOW_RUN73B_INVENTORY_PATH:-${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}}"

if [[ "$MODE" == "create-single-ata" ]]; then
    SINGLE_MINT="${SHADOW_RUN73B_SINGLE_MINT:-}"
    SINGLE_TOKEN_PROGRAM="${SHADOW_RUN73B_SINGLE_TOKEN_PROGRAM:-}"
    SINGLE_RPC_URL="${SHADOW_RUN73B_SINGLE_RPC_URL:-$RPC_URL}"
    SINGLE_SIGNER_PATH="${SHADOW_RUN73B_SINGLE_SIGNER_PATH:-$SIGNER_PATH}"
    SINGLE_RESULT_JSON="${SHADOW_RUN73B_SINGLE_RESULT_JSON:-}"

    if [[ -z "$SINGLE_MINT" || -z "$SINGLE_TOKEN_PROGRAM" ]]; then
        fail "Set SHADOW_RUN73B_SINGLE_MINT and SHADOW_RUN73B_SINGLE_TOKEN_PROGRAM for create-single-ata mode"
        finish
        exit 1
    fi

    python3 - \
        "$SINGLE_MINT" \
        "$SINGLE_TOKEN_PROGRAM" \
        "$SINGLE_RPC_URL" \
        "$SINGLE_SIGNER_PATH" \
        "$SINGLE_RESULT_JSON" <<'PY'
import base64
import json
import ssl
import sys
import time
import urllib.request
from pathlib import Path

from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from spl.token.instructions import create_idempotent_associated_token_account, get_associated_token_address

mint_pubkey = sys.argv[1]
token_program_id = sys.argv[2]
rpc_url = sys.argv[3]
signer_path = Path(sys.argv[4])
result_json_path = Path(sys.argv[5]) if sys.argv[5] else None


def rpc(method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if rpc_url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def latest_blockhash():
    response = rpc("getLatestBlockhash")
    value = (response.get("result") or {}).get("value") or {}
    return Hash.from_string(value["blockhash"])


def account_info(pubkey):
    response = rpc("getAccountInfo", [pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    return ((response.get("result") or {}).get("value")) or None


def send_raw_tx(tx_bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
        "sendTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": False,
                "preflightCommitment": "confirmed",
                "maxRetries": 1,
            },
        ],
    )


def confirm_signature(signature, attempts=15, delay_seconds=2.0):
    last_response = None
    for attempt in range(attempts):
        last_response = rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        values = (((last_response or {}).get("result") or {}).get("value")) or []
        if values and values[0] is not None:
            return last_response
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return last_response


def rpc_result_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    if not isinstance(result, dict):
        return False, None
    values = result.get("value") or []
    if not values:
        return False, None
    entry = values[0]
    if not isinstance(entry, dict):
        return False, entry
    return entry.get("err") is None, entry


raw = json.loads(signer_path.read_text(encoding="utf-8"))
if not isinstance(raw, list) or len(raw) < 64:
    raise RuntimeError(f"unexpected signer keypair shape in {signer_path}")
owner_keypair = Keypair.from_bytes(bytes(int(value) for value in raw[:64]))
owner = owner_keypair.pubkey()
mint = Pubkey.from_string(mint_pubkey)
program_id = Pubkey.from_string(token_program_id)
ata = get_associated_token_address(owner, mint, program_id)
ata_before = account_info(str(ata))
ata_exists_before = ata_before is not None

payload = {
    "mint": mint_pubkey,
    "token_program": token_program_id,
    "owner_pubkey": str(owner),
    "derived_ata": str(ata),
    "ata_exists_before": ata_exists_before,
    "action": "already_exists" if ata_exists_before else "created",
    "create_signature": None,
    "confirm_response": None,
    "ata_exists_after": ata_exists_before,
}

if not ata_exists_before:
    instruction = create_idempotent_associated_token_account(owner, owner, mint, program_id)
    message = Message([instruction], owner)
    tx = Transaction([owner_keypair], message, latest_blockhash())
    send_response = send_raw_tx(bytes(tx))
    if "error" in send_response:
        raise RuntimeError(f"sendTransaction failed for {mint_pubkey}: {send_response['error']}")
    signature = send_response.get("result")
    confirm_response = confirm_signature(signature)
    ok, status_entry = rpc_result_ok(confirm_response)
    payload["create_signature"] = signature
    payload["confirm_response"] = confirm_response
    payload["status_entry"] = status_entry
    if not ok:
        raise RuntimeError(f"ATA confirmation failed for {mint_pubkey}: {confirm_response}")
    payload["ata_exists_after"] = account_info(str(ata)) is not None
else:
    payload["status_entry"] = None

if result_json_path:
    result_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

print(f"mint={payload['mint']}")
print(f"owner_pubkey={payload['owner_pubkey']}")
print(f"derived_ata={payload['derived_ata']}")
print(f"ata_exists_before={str(payload['ata_exists_before']).lower()}")
print(f"action={payload['action']}")
print(f"create_signature={payload['create_signature'] or 'none'}")
print(f"ata_exists_after={str(payload['ata_exists_after']).lower()}")
PY

    finish
    exit 0
fi

if [[ "$MODE" == "top-up-wsol" ]]; then
    TARGET_TOTAL_SOL="${SHADOW_RUN73B_WSOL_TARGET_TOTAL_SOL:-}"
    WSOL_RPC_URL="${SHADOW_RUN73B_WSOL_RPC_URL:-$RPC_URL}"
    WSOL_SIGNER_PATH="${SHADOW_RUN73B_WSOL_SIGNER_PATH:-$SIGNER_PATH}"
    WSOL_INVENTORY_PATH="${SHADOW_RUN73B_WSOL_INVENTORY_PATH:-$INVENTORY_PATH}"
    WSOL_RESULT_JSON="${SHADOW_RUN73B_WSOL_RESULT_JSON:-}"

    if [[ -z "$TARGET_TOTAL_SOL" ]]; then
        fail "Set SHADOW_RUN73B_WSOL_TARGET_TOTAL_SOL for top-up-wsol mode"
        finish
        exit 1
    fi

    python3 - \
        "$TARGET_TOTAL_SOL" \
        "$WSOL_RPC_URL" \
        "$WSOL_SIGNER_PATH" \
        "$WSOL_INVENTORY_PATH" \
        "$WSOL_RESULT_JSON" <<'PY'
import base64
import json
import math
import ssl
import sys
import time
import urllib.request
from pathlib import Path

from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer
from solders.transaction import Transaction
from spl.token.instructions import SyncNativeParams, sync_native

WSOL_MINT = "So11111111111111111111111111111111111111112"

target_total_sol = float(sys.argv[1])
rpc_url = sys.argv[2]
signer_path = Path(sys.argv[3])
inventory_path = Path(sys.argv[4])
result_json_path = Path(sys.argv[5]) if sys.argv[5] else None


def rpc(method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if rpc_url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def latest_blockhash():
    response = rpc("getLatestBlockhash")
    value = (response.get("result") or {}).get("value") or {}
    return Hash.from_string(value["blockhash"])


def send_raw_tx(tx_bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
        "sendTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": False,
                "preflightCommitment": "confirmed",
                "maxRetries": 1,
            },
        ],
    )


def confirm_signature(signature, attempts=15, delay_seconds=2.0):
    last_response = None
    for attempt in range(attempts):
        last_response = rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        values = (((last_response or {}).get("result") or {}).get("value")) or []
        if values and values[0] is not None:
            return last_response
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return last_response


def rpc_result_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    if not isinstance(result, dict):
        return False, None
    values = result.get("value") or []
    if not values:
        return False, None
    entry = values[0]
    if not isinstance(entry, dict):
        return False, entry
    return entry.get("err") is None, entry


def token_account_raw_amount(pubkey):
    response = rpc("getTokenAccountBalance", [pubkey, {"commitment": "confirmed"}])
    value = (response.get("result") or {}).get("value") or {}
    return int(value.get("amount") or 0)


inventory_payload = json.loads(inventory_path.read_text(encoding="utf-8"))
wsol_entry = None
for entry in inventory_payload.get("accounts") or []:
    if str(entry.get("mint_pubkey") or "").strip() == WSOL_MINT:
        wsol_entry = entry
        break

if not wsol_entry:
    raise RuntimeError(f"WSOL entry is missing from {inventory_path}; refusing to auto-create a new path in narrow Run73B mode")

raw = json.loads(signer_path.read_text(encoding="utf-8"))
if not isinstance(raw, list) or len(raw) < 64:
    raise RuntimeError(f"unexpected signer keypair shape in {signer_path}")
owner_keypair = Keypair.from_bytes(bytes(int(value) for value in raw[:64]))
owner = owner_keypair.pubkey()
wsol_account = Pubkey.from_string(str(wsol_entry["token_account_pubkey"]))
token_program_id = Pubkey.from_string(str(wsol_entry["token_program_id"]))

current_raw_before = int(wsol_entry.get("amount_raw") or 0)
target_total_lamports = max(0, math.ceil(target_total_sol * 1_000_000_000))
top_up_lamports = max(target_total_lamports - current_raw_before, 0)

payload = {
    "owner_pubkey": str(owner),
    "wsol_account_pubkey": str(wsol_account),
    "token_program_id": str(token_program_id),
    "current_wsol_raw_before": current_raw_before,
    "current_wsol_balance_sol_before": current_raw_before / 1_000_000_000.0,
    "target_total_wsol_sol": round(target_total_lamports / 1_000_000_000.0, 9),
    "top_up_lamports": top_up_lamports,
    "top_up_sol": round(top_up_lamports / 1_000_000_000.0, 9),
    "action": "no_top_up_needed" if top_up_lamports <= 0 else "top_up_existing_wsol_account",
    "create_signature": None,
    "confirm_response": None,
    "current_wsol_raw_after": current_raw_before,
    "current_wsol_balance_sol_after": current_raw_before / 1_000_000_000.0,
}

if top_up_lamports > 0:
    instructions = [
        transfer(
            TransferParams(
                from_pubkey=owner,
                to_pubkey=wsol_account,
                lamports=top_up_lamports,
            )
        ),
        sync_native(
            SyncNativeParams(
                program_id=token_program_id,
                account=wsol_account,
            )
        ),
    ]
    message = Message(instructions, owner)
    tx = Transaction([owner_keypair], message, latest_blockhash())
    send_response = send_raw_tx(bytes(tx))
    if "error" in send_response:
        raise RuntimeError(f"sendTransaction failed for WSOL top-up: {send_response['error']}")
    signature = send_response.get("result")
    confirm_response = confirm_signature(signature)
    ok, status_entry = rpc_result_ok(confirm_response)
    payload["create_signature"] = signature
    payload["confirm_response"] = confirm_response
    payload["status_entry"] = status_entry
    if not ok:
        raise RuntimeError(f"WSOL top-up confirmation failed: {confirm_response}")
    current_raw_after = token_account_raw_amount(str(wsol_account))
    payload["current_wsol_raw_after"] = current_raw_after
    payload["current_wsol_balance_sol_after"] = round(current_raw_after / 1_000_000_000.0, 9)
else:
    payload["status_entry"] = None

if result_json_path:
    result_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

print(f"wsol_account_pubkey={payload['wsol_account_pubkey']}")
print(f"current_wsol_balance_sol_before={payload['current_wsol_balance_sol_before']}")
print(f"target_total_wsol_sol={payload['target_total_wsol_sol']}")
print(f"top_up_sol={payload['top_up_sol']}")
print(f"action={payload['action']}")
print(f"create_signature={payload['create_signature'] or 'none'}")
print(f"current_wsol_balance_sol_after={payload['current_wsol_balance_sol_after']}")
PY

    finish
    exit 0
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN73B_STEP:-73b_metadata_output_ata_topup_epoch}"
STATUS_PREFIX="${SHADOW_RUN73B_STATUS_PREFIX:-run73b_metadata_output_ata_topup_epoch}"
REPORT_BASENAME="${SHADOW_RUN73B_REPORT_BASENAME:-run73b_metadata_output_ata_topup_epoch}"

TARGET_OUTPUT_MINTS="${SHADOW_RUN73B_TARGET_OUTPUT_MINTS:-VpsXGqBL9MDKvbvM3EMKAeCmDJ8NZtm2yKP31FeCba5,8HmofH2u9A5EFoMhcHfXubLD2bPi32YzVdKLqZAUm65n,Ct27RqiKyV7EUPfeystWDMxHXqq8Qy1xRKXBgyUHXeJT,6A63uzxVoPkW8HCb6divQRtB5SdPvngAyuwPXuStW8cW,BD35dqMdcmy5cfU9iTXjFuc569dyTPiNHKqWDXZx98wn,BkK43JimdWEHnkBB7miFTUXre4h23n3qseQaqBp3AZAF,456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o}"
BASELINE_RUN61_STEP="${SHADOW_RUN73B_BASELINE_RUN61_STEP:-epoch6_6jus_approval_propagation_test_canonical_monetization}"
BASELINE_RUN39_STEP="${SHADOW_RUN73B_BASELINE_RUN39_STEP:-${BASELINE_RUN61_STEP}_run55_run39}"
BASELINE_RUN62_STEP="${SHADOW_RUN73B_BASELINE_RUN62_STEP:-${BASELINE_RUN61_STEP}_run62}"
BASELINE_PLAN_STEP="${SHADOW_RUN73B_BASELINE_PLAN_STEP:-73b_epoch6_metadata_output_ata_plan}"
BASELINE_RUN66_STEP="${SHADOW_RUN73B_BASELINE_RUN66_STEP:-73b_epoch6_post_run66_forensic}"
BASELINE_RUN69_STEP="${SHADOW_RUN73B_BASELINE_RUN69_STEP:-73b_epoch6_post_run69_strict_surface}"

EPOCH_NAME_RAW="${SHADOW_RUN73B_EPOCH_NAME:-epoch7_run73b_metadata_output_ata}"
EPOCH_WINDOW_SECONDS="${SHADOW_RUN73B_EPOCH_WINDOW_SECONDS:-900}"
TOP_UP_BUFFER_SOL="${SHADOW_RUN73B_TOP_UP_BUFFER_SOL:-0.00025}"

BOOTSTRAP_RPC_URL="${SHADOW_RUN73B_BOOTSTRAP_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-https://api.mainnet-beta.solana.com}}"
LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_RUN73B_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-$BOOTSTRAP_RPC_URL}}"

EPOCH_SLUG="$(printf '%s' "$EPOCH_NAME_RAW" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9._-' '_')"
EPOCH_SLUG="${EPOCH_SLUG##_}"
EPOCH_SLUG="${EPOCH_SLUG%%_}"
if [[ -z "$EPOCH_SLUG" ]]; then
    fail "Epoch name resolved to an empty slug"
    finish
    exit 2
fi

FRESH_RUN61_STEP="${SHADOW_RUN73B_FRESH_RUN61_STEP:-${EPOCH_SLUG}_canonical_monetization}"
FRESH_RUN39_STEP="${SHADOW_RUN73B_FRESH_RUN39_STEP:-${FRESH_RUN61_STEP}_run55_run39}"
FRESH_RUN62_STEP="${SHADOW_RUN73B_FRESH_RUN62_STEP:-${FRESH_RUN61_STEP}_run62}"
FRESH_PLAN_STEP="${SHADOW_RUN73B_FRESH_PLAN_STEP:-${EPOCH_SLUG}_run73b_metadata_output_ata_plan}"
FRESH_RUN66_STEP="${SHADOW_RUN73B_FRESH_RUN66_STEP:-${EPOCH_SLUG}_post_run66_forensic}"
FRESH_RUN69_STEP="${SHADOW_RUN73B_FRESH_RUN69_STEP:-${EPOCH_SLUG}_post_run69_strict_surface}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
PLAN_JSON="$RESULT_DIR/run73b_execution_plan.json"
TASKS_FILE="$RESULT_DIR/run73b_operator_tasks.sh"
ATA_EXECUTION_JSON="$RESULT_DIR/run73b_ata_execution.json"
WSOL_TOP_UP_JSON="$RESULT_DIR/run73b_wsol_top_up.json"
COMPARE_JSON="$RESULT_DIR/run73b_comparison.json"
INVENTORY_REFRESH_LOG="$RESULT_DIR/run73b_inventory_refresh.log"

BASELINE_PLAN_RESULT_DIR="$RESULTS_ROOT/$BASELINE_PLAN_STEP"
BASELINE_TARGET_MINTS_JSON="$BASELINE_PLAN_RESULT_DIR/run73_target_output_mints.json"
BASELINE_WSOL_JSON="$BASELINE_PLAN_RESULT_DIR/run73_wsol_capacity.json"
BASELINE_RUN66_RESULT_DIR="$RESULTS_ROOT/$BASELINE_RUN66_STEP"
BASELINE_RUN66_SUMMARY_JSON="$BASELINE_RUN66_RESULT_DIR/${BASELINE_RUN66_STEP}.json"
BASELINE_RUN66_ROWS_JSONL="$BASELINE_RUN66_RESULT_DIR/epoch1_would_fire_rows.jsonl"
BASELINE_RUN69_RESULT_DIR="$RESULTS_ROOT/$BASELINE_RUN69_STEP"
BASELINE_RUN69_SUMMARY_JSON="$BASELINE_RUN69_RESULT_DIR/${BASELINE_RUN69_STEP}.json"
BASELINE_RUN69_DIRECTARB_ROWS_JSONL="$BASELINE_RUN69_RESULT_DIR/directarb_strict_rows.jsonl"
BASELINE_RUN39_AUTOPSY_JSONL="$RESULTS_ROOT/$BASELINE_RUN39_STEP/candidate_autopsy.jsonl"

FRESH_PLAN_RESULT_DIR="$RESULTS_ROOT/$FRESH_PLAN_STEP"
FRESH_TARGET_MINTS_JSON="$FRESH_PLAN_RESULT_DIR/run73_target_output_mints.json"
FRESH_WSOL_JSON="$FRESH_PLAN_RESULT_DIR/run73_wsol_capacity.json"
FRESH_RUN66_RESULT_DIR="$RESULTS_ROOT/$FRESH_RUN66_STEP"
FRESH_RUN66_SUMMARY_JSON="$FRESH_RUN66_RESULT_DIR/${FRESH_RUN66_STEP}.json"
FRESH_RUN66_ROWS_JSONL="$FRESH_RUN66_RESULT_DIR/epoch1_would_fire_rows.jsonl"
FRESH_RUN69_RESULT_DIR="$RESULTS_ROOT/$FRESH_RUN69_STEP"
FRESH_RUN69_SUMMARY_JSON="$FRESH_RUN69_RESULT_DIR/${FRESH_RUN69_STEP}.json"
FRESH_RUN69_DIRECTARB_ROWS_JSONL="$FRESH_RUN69_RESULT_DIR/directarb_strict_rows.jsonl"
FRESH_RUN39_AUTOPSY_JSONL="$RESULTS_ROOT/$FRESH_RUN39_STEP/candidate_autopsy.jsonl"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$PLAN_JSON" \
    "$TASKS_FILE" \
    "$ATA_EXECUTION_JSON" \
    "$WSOL_TOP_UP_JSON" \
    "$COMPARE_JSON" \
    "$INVENTORY_REFRESH_LOG"

for required_path in \
    "$RESULTS_ROOT/$BASELINE_RUN61_STEP/run61_continuous_shadow_live_simulation.json" \
    "$BASELINE_RUN39_AUTOPSY_JSONL"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required baseline artifact is missing: $required_path"
        finish
        exit 1
    fi
done

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN73_STEP="$BASELINE_PLAN_STEP" \
    SHADOW_RUN73_STATUS_PREFIX="$BASELINE_PLAN_STEP" \
    SHADOW_RUN73_REPORT_BASENAME="$BASELINE_PLAN_STEP" \
    SHADOW_RUN73_RUN61_STEP="$BASELINE_RUN61_STEP" \
    SHADOW_RUN73_RUN39_STEP="$BASELINE_RUN39_STEP" \
    SHADOW_RUN73_RPC_URL="$RPC_URL" \
    SHADOW_RUN73_QUOTE_API_URL="${SHADOW_RUN73B_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}" \
    SHADOW_RUN73_TARGET_OUTPUT_MINTS="$TARGET_OUTPUT_MINTS" \
    SHADOW_RUN73_ALLOW_METADATA_ONLY_TOKEN2022=true \
    "$PACK_DIR/scripts/73_wsol_entry_meteora_directarb_ata_input_capacity_planner.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN62_STEP="$BASELINE_RUN62_STEP" \
    SHADOW_RUN62_STATUS_PREFIX="$BASELINE_RUN62_STEP" \
    SHADOW_RUN62_REPORT_BASENAME="$BASELINE_RUN62_STEP" \
    SHADOW_RUN62_RUN61_STEP="$BASELINE_RUN61_STEP" \
    SHADOW_RUN62_RUN39_STEP="$BASELINE_RUN39_STEP" \
    "$PACK_DIR/scripts/62_conversion_autopsy.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN66_STEP="$BASELINE_RUN66_STEP" \
    SHADOW_RUN66_STATUS_PREFIX="$BASELINE_RUN66_STEP" \
    SHADOW_RUN66_REPORT_BASENAME="$BASELINE_RUN66_STEP" \
    SHADOW_RUN66_RUN61_STEP="$BASELINE_RUN61_STEP" \
    SHADOW_RUN66_RUN62_STEP="$BASELINE_RUN62_STEP" \
    SHADOW_RUN66_RUN39_STEP="$BASELINE_RUN39_STEP" \
    SHADOW_RUN66_ALLOW_RECONSTRUCTION_MISMATCH=true \
    "$PACK_DIR/scripts/66_epoch1_monetization_forensic_audit.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN69_STEP="$BASELINE_RUN69_STEP" \
    SHADOW_RUN69_STATUS_PREFIX="$BASELINE_RUN69_STEP" \
    SHADOW_RUN69_REPORT_BASENAME="$BASELINE_RUN69_STEP" \
    SHADOW_RUN69_FORENSIC_STEP="$BASELINE_RUN66_STEP" \
    SHADOW_RUN69_RUN39_STEP="$BASELINE_RUN39_STEP" \
    "$PACK_DIR/scripts/69_strict_surface_forensic_audit.sh"

python3 - \
    "$BASELINE_TARGET_MINTS_JSON" \
    "$BASELINE_WSOL_JSON" \
    "$INVENTORY_PATH" \
    "$PLAN_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$RPC_URL" \
    "$SIGNER_PATH" \
    "$SCRIPT_PATH" \
    "$TOP_UP_BUFFER_SOL" <<'PY'
import json
import math
import shlex
import stat
import sys
from pathlib import Path

from solders.pubkey import Pubkey
from spl.token.instructions import get_associated_token_address

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ALLOWED_METADATA_ONLY_EXTENSIONS = {"metadataPointer", "tokenMetadata"}

target_mints_path = Path(sys.argv[1])
wsol_path = Path(sys.argv[2])
inventory_path = Path(sys.argv[3])
plan_json_path = Path(sys.argv[4])
tasks_file_path = Path(sys.argv[5])
step = sys.argv[6]
rpc_url = sys.argv[7]
signer_path = Path(sys.argv[8])
script_path = Path(sys.argv[9])
top_up_buffer_sol = float(sys.argv[10] or "0.00025")

mint_rows = json.loads(target_mints_path.read_text(encoding="utf-8"))
wsol_payload = json.loads(wsol_path.read_text(encoding="utf-8"))
inventory_payload = json.loads(inventory_path.read_text(encoding="utf-8"))
owner_pubkey = str(inventory_payload.get("owner_pubkey") or "").strip()

if not owner_pubkey:
    raise RuntimeError(f"inventory owner_pubkey missing in {inventory_path}")

inventory_lookup = {
    str(entry.get("mint_pubkey") or "").strip(): entry
    for entry in inventory_payload.get("accounts") or []
    if str(entry.get("mint_pubkey") or "").strip()
}

required_wsol_band_bounded_sol = float(wsol_payload.get("required_wsol_band_bounded_sol") or 0.0)
current_wsol_balance_sol = float(wsol_payload.get("current_wsol_balance_sol") or 0.0)
top_up_recommendation_sol = float(wsol_payload.get("bounded_top_up_recommendation_sol") or 0.0)
target_total_wsol_sol = round(max(required_wsol_band_bounded_sol, current_wsol_balance_sol + top_up_recommendation_sol) + top_up_buffer_sol, 9)
top_up_needed_sol = max(target_total_wsol_sol - current_wsol_balance_sol, 0.0)
top_up_needed_lamports = max(0, math.ceil(top_up_needed_sol * 1_000_000_000.0))
wsol_entry = inventory_lookup.get(WSOL_MINT) or {}

plan_items = []
plan_errors = []
for item in mint_rows:
    mint = str(item.get("mint") or "").strip()
    token_program = str(item.get("token_program") or "").strip()
    if not mint or not token_program:
        plan_errors.append(f"missing mint/token_program in {item}")
        continue
    extensions = item.get("token_2022_extensions") or []
    policy_checks = item.get("policy_checks") or {}
    derived_ata = str(
        get_associated_token_address(
            Pubkey.from_string(owner_pubkey),
            Pubkey.from_string(mint),
            Pubkey.from_string(token_program),
        )
    )
    rule_pass = bool(
        token_program == TOKEN2022_PROGRAM
        and set(extensions).issubset(ALLOWED_METADATA_ONLY_EXTENSIONS)
        and policy_checks.get("metadata_only_extension_subset_ok")
        and policy_checks.get("metadata_only_token2022_eligible")
        and policy_checks.get("rpc_policy_snapshot_stable")
        and policy_checks.get("decimals_sane")
        and not policy_checks.get("transfer_hook_present")
        and not policy_checks.get("permanent_delegate_present")
        and not policy_checks.get("non_transferable_present")
        and not policy_checks.get("default_account_state_frozen")
        and not policy_checks.get("denylisted")
        and not bool(item.get("existing_signer_ata"))
        and bool(item.get("candidate_sized_exit_quote_pass"))
        and bool(item.get("precreate_allowed"))
    )
    command = " ".join(
        [
            "sudo -n env",
            f"SHADOW_RUN73B_MODE={shlex.quote('create-single-ata')}",
            f"SHADOW_RUN73B_SINGLE_MINT={shlex.quote(mint)}",
            f"SHADOW_RUN73B_SINGLE_TOKEN_PROGRAM={shlex.quote(token_program)}",
            f"SHADOW_RUN73B_SINGLE_RPC_URL={shlex.quote(rpc_url)}",
            f"SHADOW_RUN73B_SINGLE_SIGNER_PATH={shlex.quote(str(signer_path))}",
            shlex.quote(str(script_path)),
        ]
    )
    plan_items.append(
        {
            "mint": mint,
            "token_program": token_program,
            "decimals": item.get("decimals"),
            "extensions": extensions,
            "mint_authority": item.get("mint_authority"),
            "freeze_authority": item.get("freeze_authority"),
            "denylist_status": item.get("denylist_status"),
            "rpc_policy_snapshot_stable": bool(policy_checks.get("rpc_policy_snapshot_stable")),
            "candidate_sized_exit_quote_pass": bool(item.get("candidate_sized_exit_quote_pass")),
            "candidate_sized_exit_quote": item.get("exit_quote") or {},
            "derived_ata": derived_ata,
            "existing_ata": bool(item.get("existing_signer_ata")),
            "existing_signer_token_account_pubkey": item.get("existing_signer_token_account_pubkey"),
            "expected_unlock_rows": int(item.get("expected_unlock_row_count") or 0),
            "expected_unlock_ev_usd": item.get("expected_unlock_ev_usd"),
            "policy_checks": policy_checks,
            "rule_pass": rule_pass,
            "create_idempotent_ata_command": command,
        }
    )

plan_clean = (
    len(plan_items) == 7
    and not plan_errors
    and bool(wsol_entry.get("token_account_pubkey"))
    and all(item.get("rule_pass") for item in plan_items)
)

top_up_command = " ".join(
    [
        "sudo -n env",
        f"SHADOW_RUN73B_MODE={shlex.quote('top-up-wsol')}",
        f"SHADOW_RUN73B_WSOL_TARGET_TOTAL_SOL={shlex.quote(str(target_total_wsol_sol))}",
        f"SHADOW_RUN73B_WSOL_RPC_URL={shlex.quote(rpc_url)}",
        f"SHADOW_RUN73B_WSOL_SIGNER_PATH={shlex.quote(str(signer_path))}",
        f"SHADOW_RUN73B_WSOL_INVENTORY_PATH={shlex.quote(str(inventory_path))}",
        shlex.quote(str(script_path)),
    ]
)

payload = {
    "status": "run73b_plan_ready" if plan_clean else "run73b_plan_blocked",
    "step": step,
    "plan_clean": plan_clean,
    "plan_errors": plan_errors,
    "target_mint_count": len(plan_items),
    "projected_unlock_rows": sum(int(item.get("expected_unlock_rows") or 0) for item in plan_items),
    "projected_unlock_ev_usd": round(sum(float(item.get("expected_unlock_ev_usd") or 0.0) for item in plan_items), 6),
    "wsol_top_up": {
        "current_wsol_balance_sol": round(current_wsol_balance_sol, 9),
        "required_wsol_band_bounded_sol": round(required_wsol_band_bounded_sol, 9),
        "top_up_recommendation_sol": round(top_up_recommendation_sol, 9),
        "top_up_buffer_sol": round(top_up_buffer_sol, 9),
        "target_total_wsol_sol": round(target_total_wsol_sol, 9),
        "top_up_needed_sol": round(top_up_needed_sol, 9),
        "top_up_needed_lamports": top_up_needed_lamports,
        "existing_wsol_token_account_pubkey": wsol_entry.get("token_account_pubkey"),
        "command": top_up_command,
    },
    "plan_items": plan_items,
}

task_lines = [
    "#!/usr/bin/env bash",
    "# Run73B operator tasks (generated)",
    "set -euo pipefail",
    "",
    "# Narrow metadata-only Token-2022 output-ATA rule for Run73B only:",
    "#   token_program = Token-2022",
    "#   extensions subset = {metadataPointer, tokenMetadata}",
    "#   mint_authority = null",
    "#   freeze_authority = null",
    "#   no transferHook",
    "#   no permanentDelegate",
    "#   no nonTransferable",
    "#   no defaultAccountState frozen",
    "#   not denylisted",
    "#   decimals sane",
    "#   parsed mint info stable across two RPC reads",
    "#   candidate-sized exit quote passes",
    "",
    f"# plan_clean={str(plan_clean).lower()}",
    f"# projected_unlock_rows={payload['projected_unlock_rows']}",
    f"# projected_unlock_ev_usd={payload['projected_unlock_ev_usd']}",
    f"# target_total_wsol_sol={payload['wsol_top_up']['target_total_wsol_sol']}",
    f"# top_up_needed_sol={payload['wsol_top_up']['top_up_needed_sol']}",
    "",
    "# 1. Precreate only the seven approved output ATAs below.",
]
for item in plan_items:
    task_lines.extend(
        [
            (
                "#    mint={mint} token_program={token_program} decimals={decimals} "
                "extensions={extensions} mint_authority={mint_authority} freeze_authority={freeze_authority} "
                "denylist_status={denylist_status} rpc_policy_snapshot_stable={rpc_policy_snapshot_stable} "
                "candidate_sized_exit_quote_pass={candidate_sized_exit_quote_pass} exit_value_usdc={exit_value_usdc} "
                "derived_ata={derived_ata} existing_ata={existing_ata} expected_unlock_rows={expected_unlock_rows} "
                "expected_unlock_ev_usd={expected_unlock_ev_usd}"
            ).format(
                mint=item["mint"],
                token_program=item["token_program"],
                decimals=item["decimals"],
                extensions=",".join(item["extensions"]),
                mint_authority=item["mint_authority"],
                freeze_authority=item["freeze_authority"],
                denylist_status=item["denylist_status"],
                rpc_policy_snapshot_stable=str(bool(item["rpc_policy_snapshot_stable"])).lower(),
                candidate_sized_exit_quote_pass=str(bool(item["candidate_sized_exit_quote_pass"])).lower(),
                exit_value_usdc=(item["candidate_sized_exit_quote"] or {}).get("exit_value_usdc"),
                derived_ata=item["derived_ata"],
                existing_ata=str(bool(item["existing_ata"])).lower(),
                expected_unlock_rows=item["expected_unlock_rows"],
                expected_unlock_ev_usd=item["expected_unlock_ev_usd"],
            ),
            item["create_idempotent_ata_command"],
            "",
        ]
    )
task_lines.extend(
    [
        "# 2. Top up WSOL only to the bounded 1.0 WSOL working band plus the tiny buffer above.",
        payload["wsol_top_up"]["command"],
        "",
        "# 3. Refresh signer inventory.",
        f"sudo -n env SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL={shlex.quote(rpc_url)} bash {shlex.quote(str(script_path.parent / 'refresh_signer_token_inventory.sh'))}",
        "",
    ]
)

plan_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file_path.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file_path.chmod(tasks_file_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
print(f"plan_clean={str(plan_clean).lower()}")
print(f"selected_target_count={len(plan_items)}")
print(f"projected_unlock_rows={payload['projected_unlock_rows']}")
print(f"projected_unlock_ev_usd={payload['projected_unlock_ev_usd']}")
print(f"top_up_needed_sol={payload['wsol_top_up']['top_up_needed_sol']}")
PY

plan_clean="$(python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
print('true' if payload.get('plan_clean') else 'false')
PY
)"

if [[ "$plan_clean" != "true" ]]; then
    fail "Run73B plan is not clean; see $PLAN_JSON"
    finish
    exit 1
fi

while IFS=$'\t' read -r mint token_program; do
    result_path="$RESULT_DIR/run73b_create_${mint}.json"
    env \
        SHADOW_RUN73B_MODE=create-single-ata \
        SHADOW_RUN73B_SINGLE_MINT="$mint" \
        SHADOW_RUN73B_SINGLE_TOKEN_PROGRAM="$token_program" \
        SHADOW_RUN73B_SINGLE_RPC_URL="$RPC_URL" \
        SHADOW_RUN73B_SINGLE_SIGNER_PATH="$SIGNER_PATH" \
        SHADOW_RUN73B_SINGLE_RESULT_JSON="$result_path" \
        "$SCRIPT_PATH"
done < <(
    python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
for item in payload.get('plan_items') or []:
    print(f"{item['mint']}\t{item['token_program']}")
PY
)

python3 - "$PLAN_JSON" "$ATA_EXECUTION_JSON" "$RESULT_DIR" <<'PY'
import json
import sys
from pathlib import Path

plan = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
execution_json_path = Path(sys.argv[2])
result_dir = Path(sys.argv[3])
rows = []
for item in plan.get("plan_items") or []:
    path = result_dir / f"run73b_create_{item['mint']}.json"
    if path.exists():
        rows.append(json.loads(path.read_text(encoding="utf-8")))
payload = {
    "status": "run73b_ata_execution_complete",
    "execution_count": len(rows),
    "rows": rows,
}
execution_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(f"execution_count={len(rows)}")
PY

TARGET_TOTAL_WSOL_SOL="$(python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
print(payload.get('wsol_top_up', {}).get('target_total_wsol_sol') or 0)
PY
)"

env \
    SHADOW_RUN73B_MODE=top-up-wsol \
    SHADOW_RUN73B_WSOL_TARGET_TOTAL_SOL="$TARGET_TOTAL_WSOL_SOL" \
    SHADOW_RUN73B_WSOL_RPC_URL="$RPC_URL" \
    SHADOW_RUN73B_WSOL_SIGNER_PATH="$SIGNER_PATH" \
    SHADOW_RUN73B_WSOL_INVENTORY_PATH="$INVENTORY_PATH" \
    SHADOW_RUN73B_WSOL_RESULT_JSON="$WSOL_TOP_UP_JSON" \
    "$SCRIPT_PATH"

env \
    SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$RPC_URL" \
    bash "$PACK_DIR/scripts/refresh_signer_token_inventory.sh" | tee "$INVENTORY_REFRESH_LOG"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_BOOTSTRAP_RPC_URL="$BOOTSTRAP_RPC_URL" \
    SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$LOCAL_AGAVE_REFERENCE_RPC_URL" \
    SHADOW_EPOCH_NAME="$EPOCH_NAME_RAW" \
    SHADOW_EPOCH_WINDOW_SECONDS="$EPOCH_WINDOW_SECONDS" \
    "$PACK_DIR/scripts/epoch_canonical_monetization.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN62_STEP="$FRESH_RUN62_STEP" \
    SHADOW_RUN62_STATUS_PREFIX="$FRESH_RUN62_STEP" \
    SHADOW_RUN62_REPORT_BASENAME="$FRESH_RUN62_STEP" \
    SHADOW_RUN62_RUN61_STEP="$FRESH_RUN61_STEP" \
    SHADOW_RUN62_RUN39_STEP="$FRESH_RUN39_STEP" \
    "$PACK_DIR/scripts/62_conversion_autopsy.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN73_STEP="$FRESH_PLAN_STEP" \
    SHADOW_RUN73_STATUS_PREFIX="$FRESH_PLAN_STEP" \
    SHADOW_RUN73_REPORT_BASENAME="$FRESH_PLAN_STEP" \
    SHADOW_RUN73_RUN61_STEP="$FRESH_RUN61_STEP" \
    SHADOW_RUN73_RUN39_STEP="$FRESH_RUN39_STEP" \
    SHADOW_RUN73_RPC_URL="$RPC_URL" \
    SHADOW_RUN73_QUOTE_API_URL="${SHADOW_RUN73B_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}" \
    SHADOW_RUN73_TARGET_OUTPUT_MINTS="$TARGET_OUTPUT_MINTS" \
    SHADOW_RUN73_ALLOW_METADATA_ONLY_TOKEN2022=true \
    "$PACK_DIR/scripts/73_wsol_entry_meteora_directarb_ata_input_capacity_planner.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN66_STEP="$FRESH_RUN66_STEP" \
    SHADOW_RUN66_STATUS_PREFIX="$FRESH_RUN66_STEP" \
    SHADOW_RUN66_REPORT_BASENAME="$FRESH_RUN66_STEP" \
    SHADOW_RUN66_RUN61_STEP="$FRESH_RUN61_STEP" \
    SHADOW_RUN66_RUN62_STEP="$FRESH_RUN62_STEP" \
    SHADOW_RUN66_RUN39_STEP="$FRESH_RUN39_STEP" \
    SHADOW_RUN66_ALLOW_RECONSTRUCTION_MISMATCH=true \
    "$PACK_DIR/scripts/66_epoch1_monetization_forensic_audit.sh"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN69_STEP="$FRESH_RUN69_STEP" \
    SHADOW_RUN69_STATUS_PREFIX="$FRESH_RUN69_STEP" \
    SHADOW_RUN69_REPORT_BASENAME="$FRESH_RUN69_STEP" \
    SHADOW_RUN69_FORENSIC_STEP="$FRESH_RUN66_STEP" \
    SHADOW_RUN69_RUN39_STEP="$FRESH_RUN39_STEP" \
    "$PACK_DIR/scripts/69_strict_surface_forensic_audit.sh"

python3 - \
    "$BASELINE_RUN66_SUMMARY_JSON" \
    "$BASELINE_RUN66_ROWS_JSONL" \
    "$FRESH_RUN66_SUMMARY_JSON" \
    "$FRESH_RUN66_ROWS_JSONL" \
    "$BASELINE_RUN69_SUMMARY_JSON" \
    "$BASELINE_RUN69_DIRECTARB_ROWS_JSONL" \
    "$FRESH_RUN69_SUMMARY_JSON" \
    "$FRESH_RUN69_DIRECTARB_ROWS_JSONL" \
    "$BASELINE_RUN39_AUTOPSY_JSONL" \
    "$FRESH_RUN39_AUTOPSY_JSONL" \
    "$PLAN_JSON" \
    "$ATA_EXECUTION_JSON" \
    "$WSOL_TOP_UP_JSON" \
    "$BASELINE_TARGET_MINTS_JSON" \
    "$FRESH_TARGET_MINTS_JSON" \
    "$COMPARE_JSON" \
    "$SUMMARY_JSON" \
    "$SUMMARY_FILE" \
    "$INVENTORY_REFRESH_LOG" \
    "$STATUS_PREFIX" \
    "$STEP" <<'PY'
import json
import sys
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
ATA_BLOCKERS = {"missing_output_ata", "output_ata_uninitialized"}

baseline_run66_summary_path = Path(sys.argv[1])
baseline_run66_rows_path = Path(sys.argv[2])
fresh_run66_summary_path = Path(sys.argv[3])
fresh_run66_rows_path = Path(sys.argv[4])
baseline_run69_summary_path = Path(sys.argv[5])
baseline_run69_directarb_rows_path = Path(sys.argv[6])
fresh_run69_summary_path = Path(sys.argv[7])
fresh_run69_directarb_rows_path = Path(sys.argv[8])
baseline_autopsy_path = Path(sys.argv[9])
fresh_autopsy_path = Path(sys.argv[10])
plan_path = Path(sys.argv[11])
execution_path = Path(sys.argv[12])
wsol_top_up_path = Path(sys.argv[13])
baseline_target_mints_path = Path(sys.argv[14])
fresh_target_mints_path = Path(sys.argv[15])
compare_json_path = Path(sys.argv[16])
summary_json_path = Path(sys.argv[17])
summary_file_path = Path(sys.argv[18])
inventory_refresh_log_path = Path(sys.argv[19])
status_prefix = sys.argv[20]
step = sys.argv[21]


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text_value(value):
    return str(value or "").strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def round_or_none(value, digits=6):
    if value is None:
        return None
    return round(value, digits)


def bool_field(row, *keys):
    for key in keys:
        if key in row:
            value = row.get(key)
            if isinstance(value, bool):
                return value
            if isinstance(value, str):
                lowered = value.strip().lower()
                if lowered in {"true", "1", "yes", "on"}:
                    return True
                if lowered in {"false", "0", "no", "off", ""}:
                    return False
            if value is not None:
                return bool(value)
    return False


def route_family(row):
    return text_value(row.get("target_route_family") or row.get("route")).lower()


def source_afxdp(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def input_balance_ok(row):
    return bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")


def output_account_ready(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def target_scope_autopsy_rows(rows, target_mints):
    scoped = []
    for row in rows:
        if text_value(row.get("selected_owner")).lower() != "directarb":
            continue
        if text_value(row.get("venue")).lower() != "meteora_damm_v2":
            continue
        if text_value(row.get("mint_in")) != WSOL_MINT:
            continue
        if text_value(row.get("mint_out")) not in target_mints:
            continue
        if not source_afxdp(row):
            continue
        if route_family(row) not in USABLE_ROUTE_FAMILIES:
            continue
        scoped.append(row)
    return scoped


def missing_output_ata(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    return blocker in ATA_BLOCKERS or not output_account_ready(row)


def target_scope_strict_rows(rows, target_mints):
    scoped = []
    for row in rows:
        if text_value(row.get("owner_mode")).lower() != "directarb":
            continue
        if text_value(row.get("venue")).lower() != "meteora_damm_v2":
            continue
        if text_value(row.get("mint_in")) != WSOL_MINT:
            continue
        if text_value(row.get("mint_out")) not in target_mints:
            continue
        scoped.append(row)
    return scoped


def target_scope_would_fire_rows(rows, target_mints):
    scoped = []
    for row in rows:
        if text_value(row.get("owner_mode")).lower() != "directarb":
            continue
        if text_value(row.get("venue")).lower() != "meteora_damm_v2":
            continue
        if text_value(row.get("mint_in")) != WSOL_MINT:
            continue
        if text_value(row.get("mint_out")) not in target_mints:
            continue
        scoped.append(row)
    return scoped


baseline_run66_summary = load_json(baseline_run66_summary_path)
baseline_run66_rows = load_jsonl(baseline_run66_rows_path)
fresh_run66_summary = load_json(fresh_run66_summary_path)
fresh_run66_rows = load_jsonl(fresh_run66_rows_path)
baseline_run69_summary = load_json(baseline_run69_summary_path)
baseline_run69_directarb_rows = load_jsonl(baseline_run69_directarb_rows_path)
fresh_run69_summary = load_json(fresh_run69_summary_path)
fresh_run69_directarb_rows = load_jsonl(fresh_run69_directarb_rows_path)
baseline_autopsy_rows = load_jsonl(baseline_autopsy_path)
fresh_autopsy_rows = load_jsonl(fresh_autopsy_path)
plan_payload = load_json(plan_path)
execution_payload = load_json(execution_path)
wsol_top_up_payload = load_json(wsol_top_up_path)
baseline_target_rows = load_json(baseline_target_mints_path)
fresh_target_rows = load_json(fresh_target_mints_path)
inventory_refresh_log = inventory_refresh_log_path.read_text(encoding="utf-8") if inventory_refresh_log_path.exists() else ""

target_mints = [item["mint"] for item in plan_payload.get("plan_items") or []]

baseline_strict_vs_fixup = baseline_run66_summary.get("strict_vs_fixup") or {}
fresh_strict_vs_fixup = fresh_run66_summary.get("strict_vs_fixup") or {}
baseline_route_discipline = baseline_run66_summary.get("route_discipline") or {}
fresh_route_discipline = fresh_run66_summary.get("route_discipline") or {}
baseline_run69_recon = baseline_run69_summary.get("baseline_reconciliation") or {}
fresh_run69_recon = fresh_run69_summary.get("baseline_reconciliation") or {}

baseline_scope_autopsy = target_scope_autopsy_rows(baseline_autopsy_rows, set(target_mints))
fresh_scope_autopsy = target_scope_autopsy_rows(fresh_autopsy_rows, set(target_mints))
baseline_scope_strict = target_scope_strict_rows(baseline_run69_directarb_rows, set(target_mints))
fresh_scope_strict = target_scope_strict_rows(fresh_run69_directarb_rows, set(target_mints))
baseline_scope_would_fire = target_scope_would_fire_rows(baseline_run66_rows, set(target_mints))
fresh_scope_would_fire = target_scope_would_fire_rows(fresh_run66_rows, set(target_mints))

target_mint_comparison = []
for item in plan_payload.get("plan_items") or []:
    mint = item["mint"]
    baseline_autopsy_for_mint = [row for row in baseline_scope_autopsy if text_value(row.get("mint_out")) == mint]
    fresh_autopsy_for_mint = [row for row in fresh_scope_autopsy if text_value(row.get("mint_out")) == mint]
    baseline_would_fire_for_mint = [row for row in baseline_scope_would_fire if text_value(row.get("mint_out")) == mint]
    fresh_would_fire_for_mint = [row for row in fresh_scope_would_fire if text_value(row.get("mint_out")) == mint]
    baseline_strict_for_mint = [row for row in baseline_scope_strict if text_value(row.get("mint_out")) == mint]
    fresh_strict_for_mint = [row for row in fresh_scope_strict if text_value(row.get("mint_out")) == mint]

    target_mint_comparison.append(
        {
            "mint": mint,
            "derived_ata": item.get("derived_ata"),
            "expected_unlock_rows": item.get("expected_unlock_rows"),
            "expected_unlock_ev_usd": item.get("expected_unlock_ev_usd"),
            "baseline_autopsy_scope_rows": len(baseline_autopsy_for_mint),
            "fresh_autopsy_scope_rows": len(fresh_autopsy_for_mint),
            "baseline_missing_output_ata_rows": sum(1 for row in baseline_autopsy_for_mint if missing_output_ata(row)),
            "fresh_missing_output_ata_rows": sum(1 for row in fresh_autopsy_for_mint if missing_output_ata(row)),
            "baseline_input_balance_not_ready_rows": sum(1 for row in baseline_autopsy_for_mint if not input_balance_ok(row)),
            "fresh_input_balance_not_ready_rows": sum(1 for row in fresh_autopsy_for_mint if not input_balance_ok(row)),
            "baseline_fixup_would_fire_rows": sum(1 for row in baseline_would_fire_for_mint if not bool(row.get("strict_current_state"))),
            "fresh_fixup_would_fire_rows": sum(1 for row in fresh_would_fire_for_mint if not bool(row.get("strict_current_state"))),
            "baseline_strict_rows": len(baseline_strict_for_mint),
            "fresh_strict_rows": len(fresh_strict_for_mint),
            "baseline_strict_ev_usd": round_or_none(sum(safe_float(row.get("retained_ev_usd")) for row in baseline_strict_for_mint), 6),
            "fresh_strict_ev_usd": round_or_none(sum(safe_float(row.get("retained_ev_usd")) for row in fresh_strict_for_mint), 6),
        }
    )

for item in target_mint_comparison:
    item["missing_output_ata_rows_delta"] = item["fresh_missing_output_ata_rows"] - item["baseline_missing_output_ata_rows"]
    item["input_balance_not_ready_rows_delta"] = item["fresh_input_balance_not_ready_rows"] - item["baseline_input_balance_not_ready_rows"]
    item["fixup_would_fire_rows_delta"] = item["fresh_fixup_would_fire_rows"] - item["baseline_fixup_would_fire_rows"]
    item["strict_rows_delta"] = item["fresh_strict_rows"] - item["baseline_strict_rows"]
    item["strict_ev_usd_delta"] = round_or_none((item["fresh_strict_ev_usd"] or 0.0) - (item["baseline_strict_ev_usd"] or 0.0), 6)

comparison = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "live_send_executed": False,
    "targets": target_mints,
    "execution": execution_payload,
    "wsol_top_up": wsol_top_up_payload,
    "baseline": {
        "strict_current_state_would_fire": safe_int(baseline_strict_vs_fixup.get("strict_current_state_would_fire")),
        "strict_current_state_ev_usd": safe_float(baseline_strict_vs_fixup.get("strict_current_state_ev_usd")),
        "ata_fixup_would_fire": safe_int(baseline_strict_vs_fixup.get("ata_fixup_would_fire")),
        "directarb_strict_would_fire": safe_int(baseline_run69_recon.get("directarb_strict_would_fire")),
        "route_leak_count": safe_int(baseline_route_discipline.get("route_leak_count")),
        "jito_bundle_executable_count": safe_int(baseline_route_discipline.get("jito_bundle_executable_count")),
        "target_scope_missing_output_ata_rows": sum(1 for row in baseline_scope_autopsy if missing_output_ata(row)),
        "target_scope_input_balance_not_ready_rows": sum(1 for row in baseline_scope_autopsy if not input_balance_ok(row)),
        "target_scope_strict_rows": len(baseline_scope_strict),
        "target_scope_strict_ev_usd": round_or_none(sum(safe_float(row.get("retained_ev_usd")) for row in baseline_scope_strict), 6),
        "retained_wsol_entry_directarb_row_count": safe_int(load_json(baseline_target_mints_path.parent / "run73_wsol_capacity.json").get("retained_wsol_entry_directarb_row_count")),
    },
    "fresh": {
        "strict_current_state_would_fire": safe_int(fresh_strict_vs_fixup.get("strict_current_state_would_fire")),
        "strict_current_state_ev_usd": safe_float(fresh_strict_vs_fixup.get("strict_current_state_ev_usd")),
        "ata_fixup_would_fire": safe_int(fresh_strict_vs_fixup.get("ata_fixup_would_fire")),
        "directarb_strict_would_fire": safe_int(fresh_run69_recon.get("directarb_strict_would_fire")),
        "route_leak_count": safe_int(fresh_route_discipline.get("route_leak_count")),
        "jito_bundle_executable_count": safe_int(fresh_route_discipline.get("jito_bundle_executable_count")),
        "target_scope_missing_output_ata_rows": sum(1 for row in fresh_scope_autopsy if missing_output_ata(row)),
        "target_scope_input_balance_not_ready_rows": sum(1 for row in fresh_scope_autopsy if not input_balance_ok(row)),
        "target_scope_strict_rows": len(fresh_scope_strict),
        "target_scope_strict_ev_usd": round_or_none(sum(safe_float(row.get("retained_ev_usd")) for row in fresh_scope_strict), 6),
        "retained_wsol_entry_directarb_row_count": safe_int(load_json(fresh_target_mints_path.parent / "run73_wsol_capacity.json").get("retained_wsol_entry_directarb_row_count")),
    },
    "delta": {
        "strict_current_state_would_fire_delta": safe_int(fresh_strict_vs_fixup.get("strict_current_state_would_fire")) - safe_int(baseline_strict_vs_fixup.get("strict_current_state_would_fire")),
        "strict_current_state_ev_usd_delta": round_or_none(safe_float(fresh_strict_vs_fixup.get("strict_current_state_ev_usd")) - safe_float(baseline_strict_vs_fixup.get("strict_current_state_ev_usd")), 6),
        "ata_fixup_would_fire_delta": safe_int(fresh_strict_vs_fixup.get("ata_fixup_would_fire")) - safe_int(baseline_strict_vs_fixup.get("ata_fixup_would_fire")),
        "directarb_strict_would_fire_delta": safe_int(fresh_run69_recon.get("directarb_strict_would_fire")) - safe_int(baseline_run69_recon.get("directarb_strict_would_fire")),
        "target_scope_missing_output_ata_rows_delta": sum(1 for row in fresh_scope_autopsy if missing_output_ata(row)) - sum(1 for row in baseline_scope_autopsy if missing_output_ata(row)),
        "target_scope_input_balance_not_ready_rows_delta": sum(1 for row in fresh_scope_autopsy if not input_balance_ok(row)) - sum(1 for row in baseline_scope_autopsy if not input_balance_ok(row)),
        "target_scope_strict_rows_delta": len(fresh_scope_strict) - len(baseline_scope_strict),
        "target_scope_strict_ev_usd_delta": round_or_none(sum(safe_float(row.get("retained_ev_usd")) for row in fresh_scope_strict) - sum(safe_float(row.get("retained_ev_usd")) for row in baseline_scope_strict), 6),
    },
    "target_mints": target_mint_comparison,
    "checks": {
        "strict_directarb_increased": safe_int(fresh_run69_recon.get("directarb_strict_would_fire")) > safe_int(baseline_run69_recon.get("directarb_strict_would_fire")),
        "strict_surface_increased": safe_int(fresh_strict_vs_fixup.get("strict_current_state_would_fire")) > safe_int(baseline_strict_vs_fixup.get("strict_current_state_would_fire")),
        "target_missing_output_ata_decreased": sum(1 for row in fresh_scope_autopsy if missing_output_ata(row)) < sum(1 for row in baseline_scope_autopsy if missing_output_ata(row)),
        "target_input_balance_not_ready_decreased": sum(1 for row in fresh_scope_autopsy if not input_balance_ok(row)) < sum(1 for row in baseline_scope_autopsy if not input_balance_ok(row)),
        "target_scope_strict_rows_increased": len(fresh_scope_strict) > len(baseline_scope_strict),
        "jito_bundle_executable_count_zero": safe_int(fresh_route_discipline.get("jito_bundle_executable_count")) == 0,
        "route_leak_count_zero": safe_int(fresh_route_discipline.get("route_leak_count")) == 0,
        "no_live_send": True,
    },
    "baseline_run73": {
        "target_mint_count": len(baseline_target_rows),
    },
    "fresh_run73": {
        "target_mint_count": len(fresh_target_rows),
    },
    "inventory_refresh_log": str(inventory_refresh_log_path),
}

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "plan": plan_payload,
    "comparison": comparison,
    "execution": execution_payload,
    "wsol_top_up": wsol_top_up_payload,
    "inventory_refresh_log_text": inventory_refresh_log,
}

summary_lines = [
    f"status={summary_payload['status']}",
    f"selected_target_count={len(plan_payload.get('plan_items') or [])}",
    f"ata_created_count={execution_payload.get('execution_count')}",
    f"wsol_top_up_sol={wsol_top_up_payload.get('top_up_sol')}",
    f"strict_current_state_would_fire_baseline={comparison['baseline']['strict_current_state_would_fire']}",
    f"strict_current_state_would_fire_fresh={comparison['fresh']['strict_current_state_would_fire']}",
    f"strict_current_state_would_fire_delta={comparison['delta']['strict_current_state_would_fire_delta']}",
    f"strict_current_state_ev_usd_baseline={comparison['baseline']['strict_current_state_ev_usd']}",
    f"strict_current_state_ev_usd_fresh={comparison['fresh']['strict_current_state_ev_usd']}",
    f"strict_current_state_ev_usd_delta={comparison['delta']['strict_current_state_ev_usd_delta']}",
    f"ata_fixup_would_fire_baseline={comparison['baseline']['ata_fixup_would_fire']}",
    f"ata_fixup_would_fire_fresh={comparison['fresh']['ata_fixup_would_fire']}",
    f"ata_fixup_would_fire_delta={comparison['delta']['ata_fixup_would_fire_delta']}",
    f"directarb_strict_would_fire_baseline={comparison['baseline']['directarb_strict_would_fire']}",
    f"directarb_strict_would_fire_fresh={comparison['fresh']['directarb_strict_would_fire']}",
    f"directarb_strict_would_fire_delta={comparison['delta']['directarb_strict_would_fire_delta']}",
    f"target_scope_missing_output_ata_rows_baseline={comparison['baseline']['target_scope_missing_output_ata_rows']}",
    f"target_scope_missing_output_ata_rows_fresh={comparison['fresh']['target_scope_missing_output_ata_rows']}",
    f"target_scope_missing_output_ata_rows_delta={comparison['delta']['target_scope_missing_output_ata_rows_delta']}",
    f"target_scope_input_balance_not_ready_rows_baseline={comparison['baseline']['target_scope_input_balance_not_ready_rows']}",
    f"target_scope_input_balance_not_ready_rows_fresh={comparison['fresh']['target_scope_input_balance_not_ready_rows']}",
    f"target_scope_input_balance_not_ready_rows_delta={comparison['delta']['target_scope_input_balance_not_ready_rows_delta']}",
    f"target_scope_strict_rows_baseline={comparison['baseline']['target_scope_strict_rows']}",
    f"target_scope_strict_rows_fresh={comparison['fresh']['target_scope_strict_rows']}",
    f"target_scope_strict_rows_delta={comparison['delta']['target_scope_strict_rows_delta']}",
    f"route_leak_count_fresh={comparison['fresh']['route_leak_count']}",
    f"jito_bundle_executable_count_fresh={comparison['fresh']['jito_bundle_executable_count']}",
    f"strict_directarb_increased={str(comparison['checks']['strict_directarb_increased']).lower()}",
    f"strict_surface_increased={str(comparison['checks']['strict_surface_increased']).lower()}",
    f"target_missing_output_ata_decreased={str(comparison['checks']['target_missing_output_ata_decreased']).lower()}",
    f"target_input_balance_not_ready_decreased={str(comparison['checks']['target_input_balance_not_ready_decreased']).lower()}",
    f"target_scope_strict_rows_increased={str(comparison['checks']['target_scope_strict_rows_increased']).lower()}",
    f"route_leak_count_zero={str(comparison['checks']['route_leak_count_zero']).lower()}",
    f"jito_bundle_executable_count_zero={str(comparison['checks']['jito_bundle_executable_count_zero']).lower()}",
    "live_send_executed=false",
]

for index, item in enumerate(target_mint_comparison, start=1):
    summary_lines.extend(
        [
            f"target_{index}_mint={item['mint']}",
            f"target_{index}_baseline_missing_output_ata_rows={item['baseline_missing_output_ata_rows']}",
            f"target_{index}_fresh_missing_output_ata_rows={item['fresh_missing_output_ata_rows']}",
            f"target_{index}_missing_output_ata_rows_delta={item['missing_output_ata_rows_delta']}",
            f"target_{index}_baseline_input_balance_not_ready_rows={item['baseline_input_balance_not_ready_rows']}",
            f"target_{index}_fresh_input_balance_not_ready_rows={item['fresh_input_balance_not_ready_rows']}",
            f"target_{index}_input_balance_not_ready_rows_delta={item['input_balance_not_ready_rows_delta']}",
            f"target_{index}_baseline_strict_rows={item['baseline_strict_rows']}",
            f"target_{index}_fresh_strict_rows={item['fresh_strict_rows']}",
            f"target_{index}_strict_rows_delta={item['strict_rows_delta']}",
            f"target_{index}_strict_ev_usd_delta={item['strict_ev_usd_delta']}",
        ]
    )

compare_json_path.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")
summary_file_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run73B completed; summary written to $SUMMARY_FILE"
finish

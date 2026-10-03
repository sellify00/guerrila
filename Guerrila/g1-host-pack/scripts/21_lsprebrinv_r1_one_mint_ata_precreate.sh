#!/usr/bin/env bash
# 21_lsprebrinv_r1_one_mint_ata_precreate.sh — one-mint ATA precreate only for R1 Backrun inventory-band prep

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1A — One-Mint ATA Precreate"

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
STEP="${SHADOW_LSPREBRINV_R1A_STEP:-21_lsprebrinv_r1_one_mint_ata_precreate}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
RESULT_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_ata_precreate.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_ata_precreate_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
REFRESH_JSON="$RESULT_DIR/refreshed_owner_mint_accounts.json"

PROPOSAL_JSON_PATH="${SHADOW_LSPREBRINV_R1A_PROPOSAL_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_proposal/r1_one_mint_proposal.json}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1A_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_R1_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}}}}"
RPC_URL="${SHADOW_LSPREBRINV_R1A_RPC_URL:-${SHADOW_LSPREBRINV_R1_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}}"
TARGET_MINT="${SHADOW_LSPREBRINV_R1A_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_LSPREBRINV_R1A_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"

latest_proposal_json() {
    find "$RESULTS_ROOT" -type f -name r1_one_mint_proposal.json -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PROPOSAL_JSON_PATH" ]] || PROPOSAL_JSON_PATH="$(latest_proposal_json)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$RESULT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$REFRESH_JSON"

python3 - "$STEP" "$PROPOSAL_JSON_PATH" "$INPUT_MANIFEST" "$RESULT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$REFRESH_JSON" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from spl.token.instructions import create_idempotent_associated_token_account, get_associated_token_address

(
    step_name_arg,
    proposal_json_arg,
    manifest_arg,
    result_json_arg,
    summary_json_arg,
    summary_txt_arg,
    refresh_json_arg,
    signer_path_arg,
    rpc_url,
    target_mint_arg,
    target_pool_arg,
) = sys.argv[1:12]

step_name = str(step_name_arg or "21_lsprebrinv_r1_one_mint_ata_precreate").strip() or "21_lsprebrinv_r1_one_mint_ata_precreate"
proposal_json_path = Path(proposal_json_arg) if proposal_json_arg else Path("")
manifest_path = Path(manifest_arg)
result_json_path = Path(result_json_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
refresh_json_path = Path(refresh_json_arg)
signer_path = Path(signer_path_arg) if signer_path_arg else None
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()

EXPECTED_PROPOSAL_VERDICT = "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def is_placeholder(value: Any) -> bool:
    raw = text(value)
    return not raw or "CHANGE_ME" in raw or "${" in raw


def http_post_json(url: str, payload: dict[str, Any]):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def rpc(method: str, params=None):
    if is_placeholder(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def load_signer_keypair(path: Path) -> Keypair:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list) or len(raw) < 64:
        raise RuntimeError(f"unexpected signer keypair shape in {path}")
    return Keypair.from_bytes(bytes(int(value) for value in raw[:64]))


def latest_blockhash() -> str:
    response, error = rpc("getLatestBlockhash")
    if error is not None:
        raise RuntimeError(f"getLatestBlockhash failed: {error}")
    value = (response or {}).get("result", response) or {}
    if isinstance(value, dict):
        value = value.get("value", value)
    blockhash = text((value or {}).get("blockhash"))
    if not blockhash:
        raise RuntimeError("latest_blockhash_missing")
    return blockhash


def account_info(pubkey: str):
    response, error = rpc("getAccountInfo", [pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    if error is not None:
        return None, error if isinstance(error, dict) else {"error": str(error)}
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        value = value.get("value")
    return value, None


def send_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    response, error = rpc(
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
    if error is not None:
        raise RuntimeError(f"sendTransaction failed: {error}")
    result = (response or {}).get("result", response)
    signature = text(result)
    if not signature:
        raise RuntimeError("sendTransaction missing signature")
    return signature


def signature_status(signature: str):
    response, error = rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
    if error is not None:
        return None, error if isinstance(error, dict) else {"error": str(error)}
    values = (((response or {}).get("result") or {}).get("value")) or []
    if not values:
        return None, None
    return values[0], None


def wait_for_finalized(signature: str, attempts: int = 30, delay_seconds: float = 2.0):
    last_status = None
    for attempt in range(attempts):
        status, error = signature_status(signature)
        if error is not None:
            last_status = {"error": error}
        elif status is not None:
            last_status = status
            if isinstance(status, dict) and status.get("err") is None and text(status.get("confirmationStatus")) == "finalized":
                return True, status
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return False, last_status


def get_token_accounts_by_owner(owner: str, mint: str):
    response, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return [], error if isinstance(error, dict) else {"error": str(error)}
    result = (response or {}).get("result", response) or {}
    return result.get("value") or [], None


proposal = {}
if proposal_json_path.exists():
    try:
        proposal = json.loads(proposal_json_path.read_text(encoding="utf-8", errors="replace"))
    except Exception as exc:  # noqa: BLE001
        proposal = {"parse_error": str(exc)}

proposal_verdict = text(proposal.get("final_verdict"))
proposal_mint = text(proposal.get("mint"))
proposal_pool = text(proposal.get("pool"))
proposal_policy_pass = bool(proposal.get("mint_policy_pass"))
proposal_ata_required = bool(proposal.get("ata_precreate_required"))
proposal_policy_clean = bool(proposal.get("ata_precreate_policy_clean"))

owner_keypair = load_signer_keypair(signer_path)
owner_pubkey = str(owner_keypair.pubkey())

if target_mint and proposal_mint and proposal_mint != target_mint:
    raise RuntimeError(f"proposal mint mismatch: expected {target_mint}, got {proposal_mint}")
if target_pool and proposal_pool and proposal_pool != target_pool:
    raise RuntimeError(f"proposal pool mismatch: expected {target_pool}, got {proposal_pool}")

mint = proposal_mint or target_mint
pool = proposal_pool or target_pool
token_program = text(proposal.get("token_program")) or "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
if not mint:
    raise RuntimeError("mint_missing")

derived_ata = str(get_associated_token_address(owner_keypair.pubkey(), Pubkey.from_string(mint), Pubkey.from_string(token_program)))
ata_before, ata_before_error = account_info(derived_ata)
ata_exists_before = ata_before is not None

status = "R1_ATA_PRECREATE_COMPLETED"
first_blocker = "none"
setup_signature = ""
setup_tx_landed = False
ata_precreate_tx_finalized = False
status_entry: dict[str, Any] | None = None

if proposal_verdict != EXPECTED_PROPOSAL_VERDICT:
    status = "PARK_PROPOSAL_NOT_READY"
    first_blocker = f"proposal_verdict={proposal_verdict or 'missing'}"
elif not proposal_policy_pass or not proposal_policy_clean:
    status = "PARK_POLICY"
    first_blocker = "proposal_policy_not_clean"
elif not proposal_ata_required:
    status = "R1_ATA_ALREADY_EXISTS"
    first_blocker = "ata_precreate_not_required"
elif ata_exists_before:
    status = "R1_ATA_ALREADY_EXISTS"
    first_blocker = "ata_already_exists"
else:
    instruction = create_idempotent_associated_token_account(
        owner_keypair.pubkey(),
        owner_keypair.pubkey(),
        Pubkey.from_string(mint),
        Pubkey.from_string(token_program),
    )
    message = Message([instruction], owner_keypair.pubkey())
    tx = Transaction([owner_keypair], message, Hash.from_string(latest_blockhash()))
    setup_signature = send_raw_tx(bytes(tx))
    ata_precreate_tx_finalized, status_entry = wait_for_finalized(setup_signature)
    setup_tx_landed = ata_precreate_tx_finalized
    if not ata_precreate_tx_finalized:
        status = "PARK_ATA_FINALIZATION_FAILED"
        first_blocker = "ata_precreate_not_finalized"

ata_after, ata_after_error = account_info(derived_ata)
entry_output_account_exists = ata_after is not None

token_accounts, refresh_error = get_token_accounts_by_owner(owner_pubkey, mint)
matched_accounts = []
for row in token_accounts:
    pubkey = text((row or {}).get("pubkey"))
    parsed = ((((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {})
    info = parsed.get("info") or {}
    token_amount = info.get("tokenAmount") or {}
    matched_accounts.append(
        {
            "pubkey": pubkey,
            "amount_raw": safe_int(token_amount.get("amount")),
            "decimals": safe_int(token_amount.get("decimals")),
            "state": text(info.get("state")),
        }
    )

inventory_refresh_ok = refresh_error is None and any(text(item.get("pubkey")) == derived_ata for item in matched_accounts)
pm_sees_output_account = inventory_refresh_ok and entry_output_account_exists

if status == "R1_ATA_PRECREATE_COMPLETED" and not inventory_refresh_ok:
    status = "PARK_INVENTORY_REFRESH_FAILED"
    first_blocker = "inventory_refresh_missing_new_ata"

payload = {
    "package_name": step_name,
    "proposal_json_path": str(proposal_json_path),
    "proposal_verdict_expected": EXPECTED_PROPOSAL_VERDICT,
    "proposal_verdict_seen": proposal_verdict,
    "mint": mint,
    "pool": pool,
    "token_program": token_program,
    "owner_pubkey": owner_pubkey,
    "derived_ata": derived_ata,
    "ata_exists_before": ata_exists_before,
    "ata_exists_after": entry_output_account_exists,
    "ata_precreate_tx_signature": setup_signature or None,
    "ata_precreate_tx_finalized": ata_precreate_tx_finalized,
    "status_entry": status_entry,
    "setup_tx_landed": setup_tx_landed,
    "strategy_live_send": False,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "acquisition_executed": False,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_sees_output_account": pm_sees_output_account,
    "refreshed_account_count_for_mint": len(matched_accounts),
    "refreshed_accounts": matched_accounts,
    "refresh_error": refresh_error,
    "status": status,
    "first_blocker": first_blocker,
}

refresh_payload = {
    "owner_pubkey": owner_pubkey,
    "mint": mint,
    "derived_ata": derived_ata,
    "entry_output_account_exists": entry_output_account_exists,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_sees_output_account": pm_sees_output_account,
    "accounts": matched_accounts,
    "refresh_error": refresh_error,
}

summary = {
    "status": status,
    "mint": mint,
    "pool": pool,
    "ata_precreate_tx_finalized": ata_precreate_tx_finalized,
    "entry_output_account_exists": entry_output_account_exists,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_sees_output_account": pm_sees_output_account,
    "setup_tx_landed": setup_tx_landed,
    "strategy_live_send": False,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "acquisition_executed": False,
    "first_blocker": first_blocker,
}

manifest = {
    "proposal_json_path": str(proposal_json_path),
    "signer_path": str(signer_path or ""),
    "rpc_url": rpc_url,
    "target_mint": mint,
    "target_pool": pool,
    "live_send": False,
    "sendBundle": False,
    "acquisition": False,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
result_json_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
refresh_json_path.write_text(json.dumps(refresh_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={status}",
            f"mint={mint}",
            f"pool={pool}",
            f"ata_precreate_tx_finalized={str(ata_precreate_tx_finalized).lower()}",
            f"entry_output_account_exists={str(entry_output_account_exists).lower()}",
            f"inventory_refresh_ok={str(inventory_refresh_ok).lower()}",
            f"pm_sees_output_account={str(pm_sees_output_account).lower()}",
            f"setup_tx_landed={str(setup_tx_landed).lower()}",
            "strategy_live_send=false",
            "wire_send=0",
            "live_landed=0",
            "send_transaction_called=false",
            "sendBundle_called=false",
            "acquisition_executed=false",
            f"first_blocker={first_blocker}",
            f"ata_precreate_tx_signature={setup_signature or 'none'}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 ATA precreate written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

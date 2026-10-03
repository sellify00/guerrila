#!/usr/bin/env bash
# 21_lsprebrinv_r1_one_mint_close_empty_ata.sh — close an empty one-mint ATA after R1 path is parked

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1C — Close Empty ATA"

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
STEP="${SHADOW_LSPREBRINV_R1C_STEP:-21_lsprebrinv_r1_one_mint_close_empty_ata}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
RESULT_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_close_empty_ata.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_close_empty_ata_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
REFRESH_JSON="$RESULT_DIR/refreshed_owner_mint_accounts.json"

PROPOSAL_JSON_PATH="${SHADOW_LSPREBRINV_R1C_PROPOSAL_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_proposal/r1_one_mint_proposal.json}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1C_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_R1A_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_R1_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}}}}}"
RPC_URL="${SHADOW_LSPREBRINV_R1C_RPC_URL:-${SHADOW_LSPREBRINV_R1A_RPC_URL:-${SHADOW_LSPREBRINV_R1_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}}}"
INVENTORY_PATH="${SHADOW_LSPREBRINV_R1C_INVENTORY_PATH:-${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}}"
TARGET_MINT="${SHADOW_LSPREBRINV_R1C_MINT:-${SHADOW_LSPREBRINV_R1A_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}}"
TARGET_POOL="${SHADOW_LSPREBRINV_R1C_POOL:-${SHADOW_LSPREBRINV_R1A_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}}"

latest_proposal_json() {
    find "$RESULTS_ROOT" -type f -name r1_one_mint_proposal.json -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PROPOSAL_JSON_PATH" ]] || PROPOSAL_JSON_PATH="$(latest_proposal_json)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$RESULT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$REFRESH_JSON"

python3 - "$STEP" "$PROPOSAL_JSON_PATH" "$INPUT_MANIFEST" "$RESULT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$REFRESH_JSON" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$INVENTORY_PATH" "$TARGET_MINT" "$TARGET_POOL" <<'PY'
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
from spl.token.instructions import CloseAccountParams, close_account, get_associated_token_address

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
    inventory_path_arg,
    target_mint_arg,
    target_pool_arg,
) = sys.argv[1:13]

step_name = str(step_name_arg or "21_lsprebrinv_r1_one_mint_close_empty_ata").strip() or "21_lsprebrinv_r1_one_mint_close_empty_ata"
proposal_json_path = Path(proposal_json_arg) if proposal_json_arg else Path("")
manifest_path = Path(manifest_arg)
result_json_path = Path(result_json_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
refresh_json_path = Path(refresh_json_arg)
signer_path = Path(signer_path_arg) if signer_path_arg else None
inventory_path = Path(inventory_path_arg) if inventory_path_arg else None
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()

TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
TOKEN_PROGRAMS = [TOKEN_PROGRAM, TOKEN2022_PROGRAM]


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


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


def get_balance(pubkey: str) -> tuple[int, dict[str, Any] | None]:
    response, error = rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error if isinstance(error, dict) else {"error": str(error)}
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        value = value.get("value", value)
    return safe_int(value), None


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


def normalize_account(program_id: str, owner_pubkey: str, row: dict[str, Any]) -> dict[str, Any] | None:
    pubkey = text(row.get("pubkey"))
    parsed = ((((row.get("account") or {}).get("data") or {}).get("parsed") or {}))
    info = parsed.get("info") or {}
    mint_pubkey = text(info.get("mint"))
    if not pubkey or not mint_pubkey:
        return None
    token_amount = info.get("tokenAmount") or {}
    state = text(info.get("state")).lower()
    return {
        "mint_pubkey": mint_pubkey,
        "token_account_pubkey": pubkey,
        "token_program_id": program_id,
        "initialized": state in {"initialized", "frozen"},
        "amount_raw": safe_int(token_amount.get("amount")),
        "owner": owner_pubkey,
        "delegate": text(info.get("delegate")) or None,
        "close_authority": text(info.get("closeAuthority")) or None,
        "state": state or None,
    }


def score_entry(entry: dict[str, Any]):
    return (
        0 if entry.get("initialized") else 1,
        -safe_int(entry.get("amount_raw")),
        text(entry.get("mint_pubkey")),
        text(entry.get("token_account_pubkey")),
    )


def refresh_inventory_file(owner_pubkey: str) -> tuple[bool, list[dict[str, Any]], int | None, str]:
    accounts_by_pubkey: dict[str, dict[str, Any]] = {}
    slot: int | None = None
    for program_id in TOKEN_PROGRAMS:
        response, error = rpc(
            "getTokenAccountsByOwner",
            [
                owner_pubkey,
                {"programId": program_id},
                {"encoding": "jsonParsed", "commitment": "confirmed"},
            ],
        )
        if error is not None:
            return False, [], None, f"refresh_error:{error}"
        result = (response or {}).get("result") or {}
        slot = (result.get("context") or {}).get("slot") or slot
        for row in result.get("value") or []:
            entry = normalize_account(program_id, owner_pubkey, row)
            if not entry:
                continue
            pubkey = text(entry.get("token_account_pubkey"))
            current = accounts_by_pubkey.get(pubkey)
            if current is None or score_entry(entry) < score_entry(current):
                accounts_by_pubkey[pubkey] = entry
    accounts = sorted(accounts_by_pubkey.values(), key=score_entry)
    payload = {
        "owner_pubkey": owner_pubkey,
        "rpc_url": rpc_url,
        "slot": slot,
        "accounts": accounts,
    }
    if inventory_path:
        inventory_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = inventory_path.with_suffix(inventory_path.suffix + ".tmp")
        tmp_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        tmp_path.replace(inventory_path)
    return True, accounts, slot, ""


proposal = {}
if proposal_json_path.exists():
    try:
        proposal = json.loads(proposal_json_path.read_text(encoding="utf-8", errors="replace"))
    except Exception as exc:  # noqa: BLE001
        proposal = {"parse_error": str(exc)}

proposal_mint = text(proposal.get("mint"))
proposal_pool = text(proposal.get("pool"))
proposal_token_program = text(proposal.get("token_program"))
derived_from_proposal = text(proposal.get("entry_output_derived_pubkey"))

owner_keypair = load_signer_keypair(signer_path)
owner_pubkey = str(owner_keypair.pubkey())

if target_mint and proposal_mint and proposal_mint != target_mint:
    raise RuntimeError(f"proposal mint mismatch: expected {target_mint}, got {proposal_mint}")
if target_pool and proposal_pool and proposal_pool != target_pool:
    raise RuntimeError(f"proposal pool mismatch: expected {target_pool}, got {proposal_pool}")

mint = proposal_mint or target_mint
pool = proposal_pool or target_pool
if not mint:
    raise RuntimeError("mint_missing")

token_accounts, refresh_before_error = get_token_accounts_by_owner(owner_pubkey, mint)
matched_before = []
for row in token_accounts:
    parsed = ((((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {})
    info = parsed.get("info") or {}
    token_amount = info.get("tokenAmount") or {}
    matched_before.append(
        {
            "pubkey": text(row.get("pubkey")),
            "amount_raw": safe_int(token_amount.get("amount")),
            "decimals": safe_int(token_amount.get("decimals")),
            "state": text(info.get("state")),
        }
    )

derived_ata = derived_from_proposal
if not derived_ata:
    derived_token_program = proposal_token_program or TOKEN_PROGRAM
    derived_ata = str(
        get_associated_token_address(
            owner_keypair.pubkey(),
            Pubkey.from_string(mint),
            Pubkey.from_string(derived_token_program),
        )
    )

owner_balance_before, owner_balance_before_error = get_balance(owner_pubkey)
ata_before, ata_before_error = account_info(derived_ata)
ata_exists_before = ata_before is not None
ata_lamports_before = safe_int((ata_before or {}).get("lamports"))
ata_program_owner = text((ata_before or {}).get("owner")) or proposal_token_program or TOKEN_PROGRAM
ata_info = ((((ata_before or {}).get("data") or {}).get("parsed") or {}).get("info") or {})
token_balance_raw = safe_int(((ata_info.get("tokenAmount") or {}).get("amount")))
token_decimals = safe_int(((ata_info.get("tokenAmount") or {}).get("decimals")))
token_account_owner = text(ata_info.get("owner"))
token_account_state = text(ata_info.get("state")).lower()
close_authority = text(ata_info.get("closeAuthority"))

close_account_possible = bool(
    ata_exists_before
    and ata_program_owner in {TOKEN_PROGRAM, TOKEN2022_PROGRAM}
    and token_balance_raw == 0
    and token_account_owner == owner_pubkey
    and (not close_authority or close_authority == owner_pubkey)
)

status = "R1_EMPTY_ATA_CLOSE_COMPLETED"
first_blocker = "none"
close_tx_signature = ""
close_tx_finalized = False
status_entry: dict[str, Any] | None = None
setup_tx_landed = False
rent_recovered_lamports = 0
owner_balance_delta_lamports = 0

if not ata_exists_before:
    status = "R1_EMPTY_ATA_ALREADY_CLOSED"
    first_blocker = "ata_missing_before_close"
elif token_balance_raw != 0:
    status = "PARK_TOKEN_BALANCE_NONZERO"
    first_blocker = f"token_balance_raw={token_balance_raw}"
elif not close_account_possible:
    status = "PARK_CLOSE_ACCOUNT_NOT_PROVEN"
    first_blocker = "close_account_not_proven_for_owner_zero_balance"
else:
    instruction = close_account(
        CloseAccountParams(
            program_id=Pubkey.from_string(ata_program_owner),
            account=Pubkey.from_string(derived_ata),
            dest=owner_keypair.pubkey(),
            owner=owner_keypair.pubkey(),
            signers=[],
        )
    )
    message = Message([instruction], owner_keypair.pubkey())
    tx = Transaction([owner_keypair], message, Hash.from_string(latest_blockhash()))
    close_tx_signature = send_raw_tx(bytes(tx))
    close_tx_finalized, status_entry = wait_for_finalized(close_tx_signature)
    setup_tx_landed = close_tx_finalized
    if not close_tx_finalized:
        status = "PARK_CLOSE_TX_FINALIZATION_FAILED"
        first_blocker = "close_tx_not_finalized"
    else:
        rent_recovered_lamports = ata_lamports_before

ata_after, ata_after_error = account_info(derived_ata)
entry_output_account_exists = ata_after is not None
owner_balance_after, owner_balance_after_error = get_balance(owner_pubkey)
if owner_balance_before_error is None and owner_balance_after_error is None:
    owner_balance_delta_lamports = owner_balance_after - owner_balance_before

inventory_write_ok, refreshed_inventory_accounts, refresh_slot, refresh_error = refresh_inventory_file(owner_pubkey)
refreshed_accounts_for_mint = [row for row in refreshed_inventory_accounts if text(row.get("mint_pubkey")) == mint]
refreshed_derived_present = any(text(row.get("token_account_pubkey")) == derived_ata for row in refreshed_accounts_for_mint)
refreshed_total_amount_raw = sum(safe_int(row.get("amount_raw")) for row in refreshed_accounts_for_mint)
inventory_refresh_ok = inventory_write_ok and not refreshed_derived_present and refreshed_total_amount_raw == 0
pm_sees_output_account_removed = inventory_refresh_ok and not entry_output_account_exists
pm_pending_after = 0

if status == "R1_EMPTY_ATA_CLOSE_COMPLETED" and entry_output_account_exists:
    status = "PARK_CLOSE_ACCOUNT_STILL_EXISTS"
    first_blocker = "ata_still_exists_after_close"
elif status in {"R1_EMPTY_ATA_CLOSE_COMPLETED", "R1_EMPTY_ATA_ALREADY_CLOSED"} and not inventory_refresh_ok:
    status = "PARK_INVENTORY_REFRESH_FAILED"
    first_blocker = refresh_error or "inventory_refresh_still_sees_target_account"

payload = {
    "package_name": step_name,
    "proposal_json_path": str(proposal_json_path),
    "mint": mint,
    "pool": pool,
    "owner_pubkey": owner_pubkey,
    "token_program": ata_program_owner,
    "derived_ata": derived_ata,
    "ata_exists_before": ata_exists_before,
    "entry_output_account_exists": entry_output_account_exists,
    "token_balance_raw": token_balance_raw,
    "token_decimals": token_decimals,
    "token_account_state": token_account_state or None,
    "close_authority": close_authority or None,
    "close_account_possible": close_account_possible,
    "close_tx_signature": close_tx_signature or None,
    "close_tx_finalized": close_tx_finalized,
    "status_entry": status_entry,
    "setup_tx_landed": setup_tx_landed,
    "setup_tx_signature": close_tx_signature or None,
    "strategy_live_send": False,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "acquisition_executed": False,
    "rent_recovered_lamports": rent_recovered_lamports,
    "owner_balance_delta_lamports": owner_balance_delta_lamports,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_sees_output_account_removed": pm_sees_output_account_removed,
    "pm_pending_after": pm_pending_after,
    "refreshed_account_count_for_mint": len(refreshed_accounts_for_mint),
    "refreshed_accounts_for_mint": refreshed_accounts_for_mint,
    "refresh_error": refresh_error or None,
    "status": status,
    "first_blocker": first_blocker,
}

refresh_payload = {
    "owner_pubkey": owner_pubkey,
    "mint": mint,
    "pool": pool,
    "derived_ata": derived_ata,
    "entry_output_account_exists": entry_output_account_exists,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_sees_output_account_removed": pm_sees_output_account_removed,
    "pm_pending_after": pm_pending_after,
    "accounts_for_mint": refreshed_accounts_for_mint,
    "refresh_error": refresh_error or None,
    "refresh_slot": refresh_slot,
}

summary = {
    "status": status,
    "target_mint": mint,
    "pool": pool,
    "token_balance_raw": token_balance_raw,
    "close_account_possible": close_account_possible,
    "close_tx_finalized": close_tx_finalized,
    "rent_recovered_lamports": rent_recovered_lamports,
    "inventory_refresh_ok": inventory_refresh_ok,
    "pm_pending_after": pm_pending_after,
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
    "inventory_path": str(inventory_path or ""),
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
            f"target_mint={mint}",
            f"pool={pool}",
            f"token_balance_raw={token_balance_raw}",
            f"close_account_possible={str(close_account_possible).lower()}",
            f"close_tx_finalized={str(close_tx_finalized).lower()}",
            f"rent_recovered_lamports={rent_recovered_lamports}",
            f"inventory_refresh_ok={str(inventory_refresh_ok).lower()}",
            f"pm_sees_output_account_removed={str(pm_sees_output_account_removed).lower()}",
            f"pm_pending_after={pm_pending_after}",
            "strategy_live_send=false",
            "wire_send=0",
            "live_landed=0",
            "send_transaction_called=false",
            "sendBundle_called=false",
            "acquisition_executed=false",
            f"first_blocker={first_blocker}",
            f"close_tx_signature={close_tx_signature or 'none'}",
            f"owner_balance_delta_lamports={owner_balance_delta_lamports}",
            f"derived_ata={derived_ata}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 empty ATA close written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

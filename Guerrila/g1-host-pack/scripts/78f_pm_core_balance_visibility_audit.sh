#!/usr/bin/env bash
# 78F - PM/core-balance visibility audit for live-gate reserve checks

set -euo pipefail

source "$(dirname "$0")/lib.sh"
script_header "78F - PM Core Balance Visibility Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-/home/ubuntu/shadow-host-audit/dpdk-runs}"
STEP="${SHADOW_RUN78F_STEP:-epoch8_auto_strict_directarb_quote_identity_run78f_pm_core_balance_visibility_audit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78f_pm_core_balance_visibility_report.json"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_PATH="${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL_OVERRIDE="${SHADOW_RUN78F_RPC_URL:-}"
WSOL_RESERVE_TEST_RAW="${SHADOW_RUN78F_WSOL_RESERVE_TEST_RAW:-165922}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON"

if [[ ! -f "$INVENTORY_PATH" ]]; then
    fail "Missing signer inventory snapshot: $INVENTORY_PATH"
    {
        echo "status=blocked_missing_signer_inventory"
        echo "inventory_path=$INVENTORY_PATH"
        echo "pm_reserve_test_165922_WSOL=false"
    } > "$SUMMARY_FILE"
    finish
    exit 1
fi

if [[ ! -f "$SIGNER_PATH" ]]; then
    fail "Missing signer keypair: $SIGNER_PATH"
    {
        echo "status=blocked_missing_signer_keypair"
        echo "signer_path=$SIGNER_PATH"
        echo "pm_reserve_test_165922_WSOL=false"
    } > "$SUMMARY_FILE"
    finish
    exit 1
fi

export INVENTORY_PATH SIGNER_PATH SUMMARY_FILE REPORT_JSON RPC_URL_OVERRIDE WSOL_RESERVE_TEST_RAW

python3 - <<'PY'
import json
import os
import ssl
import subprocess
import urllib.request
from pathlib import Path

try:
    from solders.pubkey import Pubkey
except Exception as exc:  # pragma: no cover - host precondition
    raise SystemExit(f"missing solders.pubkey: {exc}")

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN_2022_PROGRAM_ID = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ATA_PROGRAM_ID = "ATokenGPvbdGVxr1b2hvZbsiW5xWH25efTNsLJA8knL"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"


def b58encode(raw: bytes) -> str:
    if not raw:
        return ""
    zeros = 0
    for value in raw:
        if value == 0:
            zeros += 1
        else:
            break
    number = int.from_bytes(raw, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def load_worker_signer_pubkey_text(raw_text: str, path: Path) -> str:
    raw = json.loads(raw_text)
    if not isinstance(raw, list) or len(raw) < 64:
        raise ValueError(f"unexpected signer keypair shape in {path}")
    return b58encode(bytes(int(value) for value in raw[-32:]))


def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
    )
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def normalize_account(program_id: str, owner_pubkey: str, row: dict) -> dict | None:
    pubkey = str(row.get("pubkey") or "").strip()
    parsed = (((row.get("account") or {}).get("data") or {}).get("parsed") or {})
    info = parsed.get("info") or {}
    mint_pubkey = str(info.get("mint") or "").strip()
    if not pubkey or not mint_pubkey:
        return None
    token_amount = info.get("tokenAmount") or {}
    state = str(info.get("state") or "").strip().lower()
    amount_raw = safe_int(token_amount.get("amount"))
    initialized = state in {"initialized", "frozen"}
    return {
        "mint_pubkey": mint_pubkey,
        "token_account_pubkey": pubkey,
        "token_program_id": program_id,
        "initialized": bool(initialized),
        "amount_raw": amount_raw,
        "owner": owner_pubkey,
    }


def score_entry(entry: dict):
    return (
        0 if entry.get("initialized") else 1,
        -safe_int(entry.get("amount_raw")),
        str(entry.get("token_account_pubkey") or ""),
    )


def best_entry(entries: list[dict]) -> dict | None:
    if not entries:
        return None
    return sorted(entries, key=score_entry)[0]


def truth(value) -> str:
    return "true" if value else "false"


inventory_path = Path(os.environ["INVENTORY_PATH"])
signer_path = Path(os.environ["SIGNER_PATH"])
summary_path = Path(os.environ["SUMMARY_FILE"])
report_path = Path(os.environ["REPORT_JSON"])
reserve_test_raw = safe_int(os.environ.get("WSOL_RESERVE_TEST_RAW")) or 165_922


def read_text_via_sudo_or_open(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        return subprocess.check_output(["sudo", "cat", str(path)], text=True)


inventory = json.loads(read_text_via_sudo_or_open(inventory_path))
owner_pubkey = str(inventory.get("owner_pubkey") or "").strip() or load_worker_signer_pubkey_text(
    read_text_via_sudo_or_open(signer_path),
    signer_path,
)
inventory_rpc_url = str(inventory.get("rpc_url") or "").strip()
rpc_url = str(os.environ.get("RPC_URL_OVERRIDE") or "").strip() or inventory_rpc_url
if not rpc_url:
    raise SystemExit("missing RPC URL override and inventory rpc_url")

inventory_accounts = inventory.get("accounts") or []
inventory_entries_by_mint: dict[str, list[dict]] = {}
inventory_entries_by_token_account: dict[str, dict] = {}
for entry in inventory_accounts:
    mint = str(entry.get("mint_pubkey") or "").strip()
    token_account = str(entry.get("token_account_pubkey") or "").strip()
    if not mint or not token_account:
        continue
    inventory_entries_by_mint.setdefault(mint, []).append(entry)
    inventory_entries_by_token_account[token_account] = entry

chain_entries_by_mint: dict[str, list[dict]] = {}
chain_entries_by_token_account: dict[str, dict] = {}
chain_slot = None
for program_id in (TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID):
    response = rpc(
        rpc_url,
        "getTokenAccountsByOwner",
        [
            owner_pubkey,
            {"programId": program_id},
            {"encoding": "jsonParsed", "commitment": "confirmed"},
        ],
    )
    if "error" in response:
        raise RuntimeError(f"rpc error for {program_id}: {response['error']}")
    result = response.get("result") or {}
    chain_slot = (result.get("context") or {}).get("slot") or chain_slot
    for row in result.get("value") or []:
        entry = normalize_account(program_id, owner_pubkey, row)
        if not entry:
            continue
        chain_entries_by_mint.setdefault(entry["mint_pubkey"], []).append(entry)
        chain_entries_by_token_account[entry["token_account_pubkey"]] = entry


def mint_owner_program(mint_pubkey: str) -> str:
    response = rpc(
        rpc_url,
        "getAccountInfo",
        [mint_pubkey, {"encoding": "base64", "commitment": "confirmed"}],
    )
    value = response.get("result") if isinstance(response, dict) else response
    value = value.get("value") if isinstance(value, dict) else value
    return str((value or {}).get("owner") or "").strip()


def derive_ata(owner_pubkey: str, token_program_id: str, mint_pubkey: str) -> str:
    ata, _ = Pubkey.find_program_address(
        [
            bytes(Pubkey.from_string(owner_pubkey)),
            bytes(Pubkey.from_string(token_program_id)),
            bytes(Pubkey.from_string(mint_pubkey)),
        ],
        Pubkey.from_string(ATA_PROGRAM_ID),
    )
    return str(ata)


def resolve_pm_view_entry(mint_pubkey: str, derived_ata: str) -> dict | None:
    exact = inventory_entries_by_token_account.get(derived_ata)
    if exact is not None:
        return exact
    entries = inventory_entries_by_mint.get(mint_pubkey) or []
    if len(entries) == 1:
        return entries[0]
    return None


def core_row(mint_pubkey: str) -> dict:
    owner_program = TOKEN_PROGRAM_ID if mint_pubkey in {WSOL_MINT, USDC_MINT} else mint_owner_program(mint_pubkey)
    derived = derive_ata(owner_pubkey, owner_program, mint_pubkey)
    chain_entries = chain_entries_by_mint.get(mint_pubkey) or []
    chain_best = best_entry(chain_entries)
    chain_ata_entry = chain_entries_by_token_account.get(derived)
    inventory_entries = inventory_entries_by_mint.get(mint_pubkey) or []
    inventory_best = best_entry(inventory_entries)
    pm_view_entry = resolve_pm_view_entry(mint_pubkey, derived)
    return {
        "mint_pubkey": mint_pubkey,
        "token_program_id": owner_program,
        "derived_ata": derived,
        "chain_selected_token_account_pubkey": str((chain_ata_entry or chain_best or {}).get("token_account_pubkey") or ""),
        "chain_balance_raw": safe_int((chain_ata_entry or chain_best or {}).get("amount_raw")),
        "inventory_selected_token_account_pubkey": str((inventory_best or {}).get("token_account_pubkey") or ""),
        "inventory_file_balance_raw": safe_int((inventory_best or {}).get("amount_raw")),
        "pm_view_token_account_pubkey": str((pm_view_entry or {}).get("token_account_pubkey") or ""),
        "pm_view_balance_raw": safe_int((pm_view_entry or {}).get("amount_raw")),
        "pm_view_initialized": bool((pm_view_entry or {}).get("initialized")),
        "chain_ata_balance_raw": safe_int((chain_ata_entry or {}).get("amount_raw")),
        "inventory_entry_count": len(inventory_entries),
        "chain_entry_count": len(chain_entries),
    }


wsol = core_row(WSOL_MINT)
usdc = core_row(USDC_MINT)
pm_reserve_test_wsol = (
    wsol["pm_view_initialized"]
    and wsol["pm_view_balance_raw"] >= reserve_test_raw
)

report = {
    "status": "ok",
    "rpc_url": rpc_url,
    "signer_pubkey": owner_pubkey,
    "inventory_path": str(inventory_path),
    "inventory_slot": safe_int(inventory.get("slot")),
    "chain_slot": safe_int(chain_slot),
    "wsol": wsol,
    "usdc": usdc,
    "pm_reserve_test_165922_WSOL": pm_reserve_test_wsol,
}

lines = [
    ("status", "ok"),
    ("signer_pubkey", owner_pubkey),
    ("inventory_path", str(inventory_path)),
    ("inventory_slot", safe_int(inventory.get("slot"))),
    ("chain_slot", safe_int(chain_slot)),
    ("WSOL_ATA", wsol["derived_ata"]),
    ("WSOL_chain_balance_raw", wsol["chain_balance_raw"]),
    ("WSOL_inventory_file_balance_raw", wsol["inventory_file_balance_raw"]),
    ("WSOL_PM_view_balance_raw", wsol["pm_view_balance_raw"]),
    ("WSOL_chain_selected_token_account_pubkey", wsol["chain_selected_token_account_pubkey"]),
    ("WSOL_inventory_selected_token_account_pubkey", wsol["inventory_selected_token_account_pubkey"]),
    ("WSOL_PM_view_token_account_pubkey", wsol["pm_view_token_account_pubkey"]),
    ("USDC_ATA", usdc["derived_ata"]),
    ("USDC_chain_balance_raw", usdc["chain_balance_raw"]),
    ("USDC_inventory_file_balance_raw", usdc["inventory_file_balance_raw"]),
    ("USDC_PM_view_balance_raw", usdc["pm_view_balance_raw"]),
    ("USDC_chain_selected_token_account_pubkey", usdc["chain_selected_token_account_pubkey"]),
    ("USDC_inventory_selected_token_account_pubkey", usdc["inventory_selected_token_account_pubkey"]),
    ("USDC_PM_view_token_account_pubkey", usdc["pm_view_token_account_pubkey"]),
    ("PM_reserve_test_165922_WSOL", truth(pm_reserve_test_wsol)),
]

report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
with summary_path.open("w", encoding="utf-8", newline="\n") as handle:
    for key, value in lines:
        handle.write(f"{key}={value}\n")
PY

pass "PM/core balance visibility audit complete: $SUMMARY_FILE"
finish

#!/usr/bin/env bash
# refresh_signer_token_inventory.sh - export signer token accounts into signer-token-inventory.json

source "$(dirname "$0")/lib.sh"
script_header "Refresh Signer Token Inventory"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

RPC_URL="${SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-}}}"
SIGNER_PATH="${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"

if [[ -z "$RPC_URL" || "$RPC_URL" == *CHANGE_ME* || "$RPC_URL" == *'${'* ]]; then
    fail "Set SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL or SHADOW_SOLANA_RPC_URL to a real RPC endpoint"
    finish
    exit 1
fi

if [[ ! -f "$SIGNER_PATH" ]]; then
    fail "Signer keypair not found: $SIGNER_PATH"
    finish
    exit 1
fi

mkdir -p "$(dirname "$INVENTORY_PATH")"

if python3 - "$RPC_URL" "$SIGNER_PATH" "$INVENTORY_PATH" <<'PY'
import json
import ssl
import sys
import urllib.request
from pathlib import Path

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
TOKEN_PROGRAMS = [
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
]


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


def load_worker_signer_pubkey(path: Path) -> str:
    raw = json.loads(path.read_text(encoding="utf-8"))
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
    delegate = str(info.get("delegate") or "").strip()
    close_authority = str(info.get("closeAuthority") or "").strip()
    return {
        "mint_pubkey": mint_pubkey,
        "token_account_pubkey": pubkey,
        "token_program_id": program_id,
        "initialized": bool(initialized),
        "amount_raw": amount_raw,
        "owner": owner_pubkey,
        "delegate": delegate or None,
        "close_authority": close_authority or None,
    }


def score_entry(entry: dict):
    return (
        0 if entry.get("initialized") else 1,
        -safe_int(entry.get("amount_raw")),
        str(entry.get("token_account_pubkey") or ""),
    )


rpc_url = sys.argv[1]
signer_path = Path(sys.argv[2])
inventory_path = Path(sys.argv[3])

owner_pubkey = load_worker_signer_pubkey(signer_path)
accounts_by_mint: dict[str, dict] = {}
slot = None

for program_id in TOKEN_PROGRAMS:
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
    slot = (result.get("context") or {}).get("slot") or slot
    for row in result.get("value") or []:
        entry = normalize_account(program_id, owner_pubkey, row)
        if not entry:
            continue
        mint = entry["mint_pubkey"]
        current = accounts_by_mint.get(mint)
        if current is None or score_entry(entry) < score_entry(current):
            accounts_by_mint[mint] = entry

payload = {
    "owner_pubkey": owner_pubkey,
    "rpc_url": rpc_url,
    "slot": slot,
    "accounts": [
        accounts_by_mint[mint]
        for mint in sorted(accounts_by_mint)
    ],
}

tmp_path = inventory_path.with_suffix(inventory_path.suffix + ".tmp")
tmp_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tmp_path.replace(inventory_path)

print(f"owner_pubkey={owner_pubkey}")
print(f"rpc_url={rpc_url}")
print(f"slot={slot}")
print(f"accounts={len(payload['accounts'])}")
print(f"inventory_path={inventory_path}")
PY
then
    pass "Signer token inventory refreshed: $INVENTORY_PATH"
else
    fail "Failed to refresh signer token inventory"
    finish
    exit 1
fi

finish

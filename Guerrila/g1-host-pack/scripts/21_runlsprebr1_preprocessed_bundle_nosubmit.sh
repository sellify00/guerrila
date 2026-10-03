#!/usr/bin/env bash
# 21_runlsprebr1_preprocessed_bundle_nosubmit.sh — build one preprocessed Backrun bundle [target, our_tx, tip] and simulate it without submitting

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR1 — Preprocessed Bundle No-Submit"

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
STEP="${SHADOW_RUNLSPREBR1_STEP:-21_runlsprebr1_preprocessed_bundle_nosubmit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebr1_preprocessed_bundle_nosubmit_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebr1_preprocessed_bundle_nosubmit_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PREBR2_ROWS_PATH="${SHADOW_RUNLSPREBR1_PREBR2_ROWS_PATH:-$RESULTS_ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl}"
PREBR0_ROWS_PATH="${SHADOW_RUNLSPREBR1_PREBR0_ROWS_PATH:-$RESULTS_ROOT/21_runlsprebr0_preprocessed_projection_quote/runlsprebr0_preprocessed_projection_quote_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_RUNLSPREBR1_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
UTILS_PATH="${SHADOW_RUNLSPREBR1_UTILS_PATH:-$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py}"
MAX_ROWS="${SHADOW_RUNLSPREBR1_MAX_ROWS:-5}"
QUOTE_API_URL="${SHADOW_RUNLSPREBR1_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUNLSPREBR1_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"
SLIPPAGE_BPS="${SHADOW_RUNLSPREBR1_SLIPPAGE_BPS:-100}"
DEFAULT_BASE_TX_FEE_LAMPORTS="${SHADOW_RUNLSPREBR1_DEFAULT_BASE_TX_FEE_LAMPORTS:-5000}"
DEFAULT_TIP_LAMPORTS="${SHADOW_RUNLSPREBR1_DEFAULT_TIP_LAMPORTS:-200000}"
SIGNER_KEYPAIR_PATH="${SHADOW_RUNLSPREBR1_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_RUNLSPREBR1_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
BUNDLE_RPC_URL="${SHADOW_RUNLSPREBR1_BUNDLE_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-}}}}}"
BUNDLE_API_KEY="${SHADOW_RUNLSPREBR1_BUNDLE_API_KEY:-${SHADOW_HELIUS_BUNDLE_API_KEY:-}}"
ALLOW_TOKEN2022="${SHADOW_RUNLSPREBR1_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUNLSPREBR1_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUNLSPREBR1_ALLOW_MINT_AUTHORITY:-false}"

latest_prebr0_rows() {
    find "$RESULTS_ROOT" -type f -name runlsprebr0_preprocessed_projection_quote_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

SOURCE_ROWS_PATH="$PREBR2_ROWS_PATH"
if [[ ! -f "$SOURCE_ROWS_PATH" ]]; then
    [[ -f "$PREBR0_ROWS_PATH" ]] || PREBR0_ROWS_PATH="$(latest_prebr0_rows)"
    SOURCE_ROWS_PATH="$PREBR0_ROWS_PATH"
fi

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS_PATH" "$LS13A_ROWS_PATH" "$UTILS_PATH" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$MAX_ROWS" "$QUOTE_API_URL" "$SWAP_API_URL" "$SLIPPAGE_BPS" "$DEFAULT_BASE_TX_FEE_LAMPORTS" "$DEFAULT_TIP_LAMPORTS" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$BUNDLE_RPC_URL" "$BUNDLE_API_KEY" "$ALLOW_TOKEN2022" "$ALLOW_FREEZE_AUTHORITY" "$ALLOW_MINT_AUTHORITY" <<'PY'
from __future__ import annotations

import base64
import importlib.util
import json
import os
import re
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    source_rows_arg,
    ls13a_rows_arg,
    utils_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    max_rows_arg,
    quote_api_url,
    swap_api_url,
    slippage_bps_arg,
    default_base_tx_fee_arg,
    default_tip_arg,
    signer_path_arg,
    rpc_url,
    bundle_rpc_url,
    bundle_api_key,
    allow_token2022_arg,
    allow_freeze_authority_arg,
    allow_mint_authority_arg,
) = sys.argv[1:22]

source_rows_path = Path(source_rows_arg) if source_rows_arg else None
ls13a_rows_path = Path(ls13a_rows_arg)
utils_path = Path(utils_arg)
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
max_rows = max(0, int(max_rows_arg or "5"))
slippage_bps = max(0, int(float(slippage_bps_arg or 100)))
default_base_tx_fee_lamports = int(float(default_base_tx_fee_arg or 5000))
default_tip_lamports = int(float(default_tip_arg or 200000))
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
allow_token2022 = str(allow_token2022_arg).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(allow_freeze_authority_arg).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(allow_mint_authority_arg).lower() in {"1", "true", "yes", "on"}

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKENKEG_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ASSOCIATED_TOKEN_PROGRAM = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
TIP_ACCOUNT_URLS = [
    "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
]
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


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


def http_json(url: str):
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "shadow-lsprebr1/1.0"})
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=25, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def http_post_json(url: str, payload: dict[str, Any], headers=None):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json", **(headers or {})},
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


def add_api_key(url: str, api_key: str):
    if is_placeholder(url) or not api_key:
        return text(url)
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment))


def redacted_url(url: str):
    if is_placeholder(url):
        return text(url)
    parsed = urllib.parse.urlsplit(url)
    if not parsed.query:
        return url
    parts = []
    for key, value in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True):
        parts.append((key, "REDACTED" if key.lower() in {"api-key", "apikey", "key", "token"} and value else value))
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(parts), parsed.fragment))


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int):
    if not input_mint or not output_mint or amount_raw <= 0:
        return None, {"error": "missing_quote_input"}
    params = urllib.parse.urlencode(
        {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": str(int(amount_raw)),
            "slippageBps": str(int(slippage_bps)),
            "swapMode": "ExactIn",
            "restrictIntermediateTokens": "true",
        }
    )
    return http_json(f"{quote_api_url}?{params}")


def build_swap_transaction(quote_response: dict[str, Any], owner_pubkey: str):
    body = {
        "userPublicKey": owner_pubkey,
        "quoteResponse": quote_response,
        "wrapAndUnwrapSol": False,
        "dynamicComputeUnitLimit": True,
    }
    return http_post_json(swap_api_url, body)


def latest_blockhash():
    result, error = rpc("getLatestBlockhash")
    if error is not None:
        return "", error
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        maybe = value.get("value", value)
    else:
        maybe = {}
    blockhash = text((maybe or {}).get("blockhash"))
    if not blockhash:
        return "", {"error": "latest_blockhash_missing"}
    return blockhash, None


def get_balance(pubkey: str) -> tuple[int, dict[str, Any] | None]:
    result, error = rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        maybe = value.get("value", value)
    else:
        maybe = 0
    return safe_int(maybe), None


def get_token_accounts_by_owner(owner: str, mint: str) -> list[dict[str, Any]]:
    result, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        rows = value.get("value") or []
    else:
        rows = []
    out = []
    for row in rows:
        parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
        info = parsed.get("info") if isinstance(parsed, dict) else {}
        token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
        out.append(
            {
                "pubkey": text((row or {}).get("pubkey")),
                "amount_raw": safe_int((token_amount or {}).get("amount")),
                "decimals": safe_int((token_amount or {}).get("decimals")),
            }
        )
    return out


def mint_metadata(mint: str) -> dict[str, Any]:
    result, error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    out = {
        "mint": mint,
        "token_program": "",
        "decimals": None,
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "rpc_error": error or {},
    }
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        return out
    out["token_program"] = text(value.get("owner"))
    parsed = value.get("data", {}).get("parsed", {}) if isinstance(value.get("data"), dict) else {}
    info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
    out["decimals"] = info.get("decimals")
    out["freeze_authority"] = text(info.get("freezeAuthority"))
    out["mint_authority"] = text(info.get("mintAuthority"))
    extensions = []
    raw_extensions = info.get("extensions") if isinstance(info.get("extensions"), list) else []
    for item in raw_extensions:
        if isinstance(item, dict):
            ext_name = text(item.get("extension"))
            if ext_name:
                extensions.append(ext_name)
        elif isinstance(item, str):
            extensions.append(item)
    out["extensions"] = extensions
    return out


def policy_for_mint(meta: dict[str, Any]) -> tuple[bool, str]:
    token_program = text(meta.get("token_program"))
    if token_program == TOKEN2022_PROGRAM and not allow_token2022:
        return False, "token2022_not_allowed"
    if text(meta.get("freeze_authority")) and not allow_freeze_authority:
        return False, "freeze_authority_present"
    if text(meta.get("mint_authority")) and not allow_mint_authority:
        return False, "mint_authority_present"
    normalized_exts = {text(item).replace("_", "").replace("-", "").lower() for item in meta.get("extensions", [])}
    dangerous = sorted(ext for ext in normalized_exts if ext in DANGEROUS_TOKEN2022_EXTENSIONS)
    if dangerous:
        return False, f"token2022_extension_{dangerous[0]}"
    return True, ""


def parse_dotenv_tip_accounts() -> list[str]:
    dotenv_path = Path.cwd() / ".env"
    if not dotenv_path.exists():
        return []
    blob = dotenv_path.read_text(encoding="utf-8", errors="replace")
    match = re.search(r'(?ms)^tip_accounts\s*=\s*"([^"]*)"', blob)
    raw_accounts = ""
    if match:
        raw_accounts = match.group(1)
    else:
        match = re.search(r"(?mi)^(?:HELIUS_SENDER_TIP_ACCOUNTS|JITO_TIP_ACCOUNTS)\s*=\s*(.+)$", blob)
        if match:
            raw_accounts = match.group(1).strip().strip('"').strip("'")
    accounts = [item.strip().strip(",") for item in re.split(r"[\s,]+", raw_accounts) if item.strip().strip(",")]
    return list(dict.fromkeys(accounts))


def parse_dotenv_provider_urls():
    dotenv_path = Path.cwd() / ".env"
    out = {}
    if not dotenv_path.exists():
        return out
    for raw in dotenv_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        out[key.strip()] = value.strip().strip('"').strip("'")
    return out


def fetch_tip_accounts():
    payload = {"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}
    for url in TIP_ACCOUNT_URLS:
        result, error = http_post_json(url, payload)
        if error is not None:
            continue
        if isinstance(result, dict) and isinstance(result.get("result"), list):
            accounts = [text(item) for item in result["result"] if text(item)]
            if accounts:
                return list(dict.fromkeys(accounts))
    return []


def simulate_bundle(endpoint: str, txs_base64: list[str]):
    if is_placeholder(endpoint):
        return {
            "available": False,
            "pass": False,
            "reachable": False,
            "method_available": False,
            "failure_reason": "provider_unconfigured",
            "http_status": 0,
            "result_summary": "",
        }
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateBundle",
        "params": [{"encodedTransactions": txs_base64}],
    }
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    context = ssl.create_default_context() if endpoint.startswith("https://") else None
    try:
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            status = response.status
            body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        status = exc.code
        body = exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return {
            "available": False,
            "pass": False,
            "reachable": False,
            "method_available": False,
            "failure_reason": str(exc),
            "http_status": 0,
            "result_summary": "",
        }
    try:
        decoded = json.loads(body) if body else {}
    except Exception:
        decoded = {"raw": body[:240]}
    error = decoded.get("error") if isinstance(decoded, dict) else None
    error_message = str(error.get("message", "")) if isinstance(error, dict) else ""
    method_missing = isinstance(error, dict) and (error.get("code") == -32601 or "method not found" in error_message.lower())
    auth_failed = status in {401, 403} or "api key" in error_message.lower() or "unauthorized" in error_message.lower()
    result = decoded.get("result") if isinstance(decoded, dict) else None
    passed = status < 500 and not method_missing and not auth_failed and error is None
    return {
        "available": status < 500 and not auth_failed,
        "pass": passed,
        "reachable": status < 500,
        "method_available": not method_missing and not auth_failed,
        "failure_reason": "" if passed else (error_message or f"http_{status}"),
        "http_status": status,
        "result_summary": json.dumps(result, sort_keys=True)[:240] if result is not None else "",
    }


def classify_sim_failure(reason: str) -> str:
    raw = lower(reason)
    if not raw:
        return "SIM_FAIL_UNKNOWN"
    if "blockhash" in raw:
        return "SIM_FAIL_BLOCKHASH"
    if "already processed" in raw or "already landed" in raw or "already finalized" in raw:
        return "SIM_FAIL_TARGET_ALREADY_PROCESSED"
    if "insufficient" in raw or "account" in raw or "owner" in raw or "token" in raw:
        return "SIM_FAIL_PM_OR_ACCOUNT"
    if "provider" in raw or "unauthorized" in raw or "api key" in raw:
        return "SIM_FAIL_PROVIDER"
    if "slippage" in raw or "min out" in raw or "quote" in raw:
        return "SIM_FAIL_PROJECTION_MISMATCH"
    return "SIM_FAIL_UNKNOWN"


dotenv_provider_urls = parse_dotenv_provider_urls()
if is_placeholder(bundle_rpc_url):
    bundle_rpc_url = (
        text(dotenv_provider_urls.get("HELIUS_BUNDLE_RPC_URL"))
        or text(dotenv_provider_urls.get("HELIUS_RPC_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER"))
        or bundle_rpc_url
    )
if is_placeholder(rpc_url):
    rpc_url = (
        text(dotenv_provider_urls.get("HELIUS_RPC_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER_URL"))
        or text(dotenv_provider_urls.get("HELIUS_GATEKEEPER"))
        or rpc_url
    )
provider_endpoint = add_api_key(bundle_rpc_url, bundle_api_key)
if is_placeholder(provider_endpoint):
    provider_endpoint = add_api_key(rpc_url, bundle_api_key)

env_tip_accounts = [
    item.strip().strip(",")
    for item in re.split(
        r"[\s,]+",
        os.environ.get("HELIUS_SENDER_TIP_ACCOUNTS")
        or os.environ.get("JITO_TIP_ACCOUNTS")
        or os.environ.get("tip_accounts")
        or "",
    )
    if item.strip().strip(",")
]
tip_accounts = list(dict.fromkeys([text(v) for v in env_tip_accounts if text(v)] + parse_dotenv_tip_accounts()))
if not tip_accounts:
    tip_accounts = fetch_tip_accounts()

signer_staged = bool(signer_keypair_path and signer_keypair_path.exists())
solders_ready = False
solders_error = ""
owner_pubkey = ""
owner_keypair = None
VersionedTransaction = None
Transaction = None
Message = None
Hash = None
Pubkey = None
TransferParams = None
transfer = None
if signer_staged:
    try:
        from solders.hash import Hash as SoldersHash
        from solders.keypair import Keypair
        from solders.message import Message as SoldersMessage
        from solders.pubkey import Pubkey as SoldersPubkey
        from solders.system_program import TransferParams as SoldersTransferParams, transfer as solders_transfer
        from solders.transaction import Transaction as SoldersTransaction
        from solders.transaction import VersionedTransaction as SoldersVersionedTransaction

        Hash = SoldersHash
        Message = SoldersMessage
        Pubkey = SoldersPubkey
        Transaction = SoldersTransaction
        TransferParams = SoldersTransferParams
        transfer = solders_transfer
        VersionedTransaction = SoldersVersionedTransaction
        owner_keypair = Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8"))
        owner_pubkey = str(owner_keypair.pubkey())
        solders_ready = True
    except Exception as exc:
        solders_error = str(exc)


def sign_swap_transaction(quote_response: dict[str, Any]):
    if not solders_ready or owner_keypair is None or VersionedTransaction is None or not owner_pubkey:
        return "", {"error": solders_error or "signer_not_ready"}, {}
    prepared, prepare_error = build_swap_transaction(quote_response, owner_pubkey)
    if prepare_error is not None:
        return "", prepare_error, prepared or {}
    swap_tx_b64 = text((prepared or {}).get("swapTransaction"))
    if not swap_tx_b64:
        return "", {"error": "swap_transaction_missing"}, prepared or {}
    try:
        raw = base64.b64decode(swap_tx_b64)
        unsigned = VersionedTransaction.from_bytes(raw)
        signed = VersionedTransaction(unsigned.message, [owner_keypair])
        return base64.b64encode(bytes(signed)).decode("ascii"), None, prepared or {}
    except Exception as exc:
        return "", {"error": str(exc)}, prepared or {}


def build_tip_transaction(tip_account: str, lamports: int):
    if not solders_ready or owner_keypair is None or Message is None or Pubkey is None or Transaction is None or TransferParams is None or transfer is None:
        return "", {"error": solders_error or "tip_builder_unavailable"}
    if lamports <= 0:
        return "", {"error": "tip_lamports_non_positive"}
    blockhash, blockhash_error = latest_blockhash()
    if blockhash_error is not None:
        return "", blockhash_error
    try:
        instruction = transfer(
            TransferParams(
                from_pubkey=owner_keypair.pubkey(),
                to_pubkey=Pubkey.from_string(text(tip_account)),
                lamports=lamports,
            )
        )
        message = Message([instruction], owner_keypair.pubkey())
        tx = Transaction([owner_keypair], message, Hash.from_string(blockhash))
        return base64.b64encode(bytes(tx)).decode("ascii"), None
    except Exception as exc:
        return "", {"error": str(exc)}


ls13a_by_signature = {
    text(row.get("target_signature")): row
    for row in load_jsonl(ls13a_rows_path)
    if text(row.get("target_signature"))
}

source_rows = load_jsonl(source_rows_path)
has_executable_shape_decisions = any("decision" in row for row in source_rows)
rows = []
for row in source_rows:
    if has_executable_shape_decisions:
        if text(row.get("decision")) == "BACKRUN_EXECUTABLE_SHAPE_READY":
            rows.append(row)
        continue
    if text(row.get("projection_blocker")) == "PROJECTION_READY" and boolish(row.get("net_ev_after_tip_positive")):
        rows.append(row)
if max_rows > 0:
    rows = rows[:max_rows]

out = []
for index, row in enumerate(rows, start=1):
    target_signature = text(row.get("target_signature"))
    candidate_id = text(row.get("target_entity_id")) or target_signature or f"preprocessed_backrun_{index}"
    ls13a_row = ls13a_by_signature.get(target_signature, {})
    target_tx_base64 = text(ls13a_row.get("target_tx_base64"))
    target_tx_bytes_available = bool(target_tx_base64)
    target_tx_parse, target_tx_parse_error = utils.parse_wire_tx(target_tx_base64) if target_tx_bytes_available else (None, "target_tx_missing")
    target_signature_matches = bool(target_tx_parse and target_signature in (target_tx_parse.get("signatures") or []))
    target_recent_blockhash_valid = boolish(row.get("target_blockhash_valid"))
    target_already_landed = boolish(row.get("target_already_landed_at_first_check"))

    our_direction = text(row.get("our_backrun_direction"))
    if "->" in our_direction:
        backrun_input_mint, backrun_output_mint = [part.strip() for part in our_direction.split("->", 1)]
    else:
        backrun_input_mint, backrun_output_mint = "", ""
    input_amount_raw = safe_int(row.get("our_input_raw"))
    tip_lamports = max(default_tip_lamports, safe_int(row.get("tip_lamports")))
    projected_expected_out_raw = safe_int(row.get("projected_expected_out_raw"))
    projected_min_out_raw = safe_int(row.get("projected_min_out_raw"))
    projected_net_ev_after_tip_raw = safe_int(row.get("net_ev_after_tip_raw"))

    input_meta = mint_metadata(backrun_input_mint) if backrun_input_mint else {}
    output_meta = mint_metadata(backrun_output_mint) if backrun_output_mint else {}
    input_policy_pass, input_policy_blocker = policy_for_mint(input_meta) if input_meta else (False, "input_mint_missing")
    output_policy_pass, output_policy_blocker = policy_for_mint(output_meta) if output_meta else (False, "output_mint_missing")
    token_policy_pass = input_policy_pass and output_policy_pass

    owner_native_balance_raw, owner_native_balance_error = get_balance(owner_pubkey) if owner_pubkey else (0, {"error": "owner_pubkey_missing"})
    input_accounts = []
    input_account_ready = False
    input_account_pubkey = ""
    input_account_balance_raw = 0
    if backrun_input_mint == WSOL_MINT:
        input_account_ready = owner_native_balance_raw >= input_amount_raw > 0
    elif backrun_input_mint:
        input_accounts = get_token_accounts_by_owner(owner_pubkey, backrun_input_mint) if owner_pubkey else []
        richest_input = max(input_accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
        input_account_pubkey = text(richest_input.get("pubkey"))
        input_account_balance_raw = safe_int(richest_input.get("amount_raw"))
        input_account_ready = bool(input_account_pubkey) and input_account_balance_raw >= input_amount_raw > 0

    output_accounts = get_token_accounts_by_owner(owner_pubkey, backrun_output_mint) if backrun_output_mint and owner_pubkey else []
    output_account_pubkey = text((output_accounts[0] if output_accounts else {}).get("pubkey"))
    output_account_ready = bool(output_account_pubkey)

    fresh_quote, fresh_quote_error = quote_exact_in(backrun_input_mint, backrun_output_mint, input_amount_raw) if input_amount_raw > 0 else (None, {"error": "input_amount_non_positive"})
    fresh_quote_out_raw = safe_int((fresh_quote or {}).get("outAmount"))
    fresh_min_out_raw = safe_int((fresh_quote or {}).get("otherAmountThreshold"))
    fresh_quote_positive = fresh_quote_out_raw > 0 and fresh_min_out_raw > 0

    our_tx_b64 = ""
    our_swap_prepare = {}
    our_swap_prepare_error = {}
    our_tx_parse = {"decode_ok": False, "decode_error": "our_tx_not_built", "account_keys": [], "program_ids": [], "instructions": [], "signature_count": 0}
    our_tx_built = False
    our_tx_signed = False
    if fresh_quote_positive and input_account_ready and solders_ready and token_policy_pass:
        our_tx_b64, our_swap_prepare_error, our_swap_prepare = sign_swap_transaction(fresh_quote or {})
        our_tx_built = bool(our_tx_b64)
        our_tx_signed = our_tx_built
        if our_tx_built:
            our_tx_parse = utils.parse_wire_tx(our_tx_b64)[0] or our_tx_parse

    our_tx_creates_ata = ASSOCIATED_TOKEN_PROGRAM in (our_tx_parse.get("program_ids") or [])
    output_account_path_clean = output_account_ready and not our_tx_creates_ata

    selected_tip_account = text(tip_accounts[0]) if tip_accounts else ""
    tip_tx_b64, tip_build_error = build_tip_transaction(selected_tip_account, tip_lamports) if selected_tip_account and tip_lamports > 0 else ("", {"error": "tip_account_missing"})
    tip_tx_built = bool(tip_tx_b64)
    tip_tx_signed = tip_tx_built
    tip_tx_parse = utils.parse_wire_tx(tip_tx_b64)[0] if tip_tx_built else None
    bundle_tip_present = bool(tip_tx_built and tip_tx_signed and selected_tip_account and tip_lamports > 0)

    our_tx_priority_fee_lamports = safe_int((our_swap_prepare or {}).get("prioritizationFeeLamports"))
    our_tx_fee_lamports = default_base_tx_fee_lamports if our_tx_built else 0
    tip_tx_fee_lamports = default_base_tx_fee_lamports if tip_tx_built else 0
    total_fee_and_tip_lamports = tip_lamports + our_tx_priority_fee_lamports + our_tx_fee_lamports + tip_tx_fee_lamports

    if backrun_input_mint == WSOL_MINT:
        pm_reserve_ok = owner_native_balance_raw >= (input_amount_raw + total_fee_and_tip_lamports) and input_amount_raw > 0
    else:
        pm_reserve_ok = input_account_ready and owner_native_balance_raw >= (tip_lamports + our_tx_priority_fee_lamports + our_tx_fee_lamports + tip_tx_fee_lamports)
    pm_release_on_no_send_ok = True
    pm_pending_after = 0

    txs_base64 = []
    if target_tx_bytes_available:
        txs_base64.append(target_tx_base64)
    if our_tx_built:
        txs_base64.append(our_tx_b64)
    if tip_tx_built:
        txs_base64.append(tip_tx_b64)
    bundle_tx_count = len(txs_base64)
    target_tx_first = bool(bundle_tx_count >= 1 and txs_base64[0] == target_tx_base64)
    our_backrun_tx_second = bool(bundle_tx_count >= 2 and txs_base64[1] == our_tx_b64)
    bundle_envelope_built = (
        target_tx_bytes_available
        and target_signature_matches
        and target_recent_blockhash_valid
        and not target_already_landed
        and our_tx_built
        and our_tx_signed
        and bundle_tip_present
        and bundle_tx_count == 3
        and target_tx_first
        and our_backrun_tx_second
    )

    simulate_bundle_called = bool(bundle_envelope_built and pm_reserve_ok and output_account_path_clean and token_policy_pass and not our_tx_creates_ata and not target_already_landed and not is_placeholder(provider_endpoint))
    simulate_result = simulate_bundle(provider_endpoint, txs_base64) if simulate_bundle_called else {
        "available": False,
        "pass": False,
        "reachable": False,
        "method_available": False,
        "failure_reason": "bundle_not_complete" if not bundle_envelope_built else "pre_bundle_gates_failed",
        "http_status": 0,
        "result_summary": "",
    }
    simulate_failure_class = classify_sim_failure(simulate_result["failure_reason"])

    if not target_tx_bytes_available:
        decision = "PARK_TARGET_TX_MISSING"
    elif not target_signature_matches:
        decision = "PARK_TARGET_SIGNATURE_MISMATCH"
    elif not target_recent_blockhash_valid or target_already_landed:
        decision = "PARK_TARGET_ALREADY_PROCESSED"
    elif not token_policy_pass:
        decision = "PARK_POLICY"
    elif input_amount_raw <= 0 or not backrun_input_mint or not backrun_output_mint:
        decision = "PARK_QUOTE_NEGATIVE"
    elif not input_account_ready:
        decision = "PARK_INPUT_ACCOUNT_MISSING"
    elif not fresh_quote_positive:
        decision = "PARK_QUOTE_NEGATIVE"
    elif not our_tx_built or not our_tx_signed:
        decision = "PARK_BUNDLE_ENVELOPE_NOT_BUILDABLE"
    elif not output_account_ready:
        decision = "PARK_OUTPUT_ACCOUNT_MISSING"
    elif our_tx_creates_ata:
        decision = "PARK_OUTPUT_ACCOUNT_REQUIRES_ATA_CREATE"
    elif not bundle_tip_present:
        decision = "PARK_BUNDLE_TIP_MISSING"
    elif not pm_reserve_ok or not pm_release_on_no_send_ok or pm_pending_after != 0:
        decision = "PARK_PM"
    elif not simulate_bundle_called:
        decision = "PARK_BUNDLE_SIM_UNAVAILABLE"
    elif not simulate_result["pass"]:
        decision = simulate_failure_class
    else:
        decision = "BACKRUN_PREPROCESSED_BUNDLE_READY_NO_SUBMIT"

    out.append(
        {
            **row,
            "candidate_id": candidate_id,
            "target_tx_bytes_available": target_tx_bytes_available,
            "target_signature_matches": target_signature_matches,
            "target_tx_first": target_tx_first,
            "our_backrun_tx_second": our_backrun_tx_second,
            "target_recent_blockhash_valid": target_recent_blockhash_valid,
            "target_already_landed": target_already_landed,
            "backrun_input_mint": backrun_input_mint,
            "backrun_output_mint": backrun_output_mint,
            "fresh_quote": fresh_quote or {"ok": False, "error": fresh_quote_error or {"error": "fresh_quote_missing"}},
            "fresh_quote_positive": fresh_quote_positive,
            "fresh_quote_out_raw": fresh_quote_out_raw,
            "fresh_min_out_raw": fresh_min_out_raw,
            "projected_expected_out_raw": projected_expected_out_raw,
            "projected_min_out_raw": projected_min_out_raw,
            "projected_net_ev_after_tip_raw": projected_net_ev_after_tip_raw,
            "input_token_program": text(input_meta.get("token_program")),
            "output_token_program": text(output_meta.get("token_program")),
            "input_token_policy_pass": input_policy_pass,
            "output_token_policy_pass": output_policy_pass,
            "input_policy_blocker": input_policy_blocker,
            "output_policy_blocker": output_policy_blocker,
            "token_policy_pass": token_policy_pass,
            "owner_pubkey": owner_pubkey,
            "owner_native_balance_raw": owner_native_balance_raw,
            "owner_native_balance_error": owner_native_balance_error or {},
            "input_account_ready": input_account_ready,
            "input_account_pubkey": input_account_pubkey,
            "input_account_balance_raw": input_account_balance_raw,
            "input_account_count": len(input_accounts),
            "output_account_ready": output_account_ready,
            "output_account_pubkey": output_account_pubkey,
            "output_account_count": len(output_accounts),
            "output_account_path_clean": output_account_path_clean,
            "our_tx_built": our_tx_built,
            "our_tx_signed": our_tx_signed,
            "our_tx_priority_fee_lamports": our_tx_priority_fee_lamports,
            "our_tx_fee_lamports": our_tx_fee_lamports,
            "our_tx_creates_ata": our_tx_creates_ata,
            "our_tx_parse": our_tx_parse,
            "our_swap_prepare_error": our_swap_prepare_error or {},
            "bundle_tip_present": bundle_tip_present,
            "bundle_tip_lamports": tip_lamports,
            "selected_tip_account": selected_tip_account,
            "tip_tx_built": tip_tx_built,
            "tip_tx_signed": tip_tx_signed,
            "tip_tx_fee_lamports": tip_tx_fee_lamports,
            "tip_build_error": tip_build_error or {},
            "bundle_tx_count": bundle_tx_count,
            "bundle_envelope_built": bundle_envelope_built,
            "pm_reserve_ok": pm_reserve_ok,
            "pm_release_on_no_send_ok": pm_release_on_no_send_ok,
            "pm_pending_after": pm_pending_after,
            "simulate_bundle_called": simulate_bundle_called,
            "simulate_bundle_pass": bool(simulate_result["pass"]),
            "simulate_bundle_available": bool(simulate_result["available"]),
            "simulate_bundle_failure_reason": simulate_result["failure_reason"],
            "simulate_bundle_result_summary": simulate_result["result_summary"],
            "simulate_bundle_failure_class": simulate_failure_class,
            "provider_endpoint": redacted_url(provider_endpoint),
            "provider_route_name": "jito_via_helius_bundle",
            "signer_staged": signer_staged,
            "solders_ready": solders_ready,
            "solders_error": solders_error,
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": "BACKRUN_PREPROCESSED_BUNDLE_READY_NO_SUBMIT" if decision_counts.get("BACKRUN_PREPROCESSED_BUNDLE_READY_NO_SUBMIT", 0) > 0 else "PARK_PREPROCESSED_BUNDLE_NOSUBMIT",
    "source_rows": str(source_rows_path or ""),
    "rows_seen": len(out),
    "target_tx_bytes_available_count": sum(1 for row in out if row["target_tx_bytes_available"]),
    "input_account_ready_count": sum(1 for row in out if row["input_account_ready"]),
    "output_account_ready_count": sum(1 for row in out if row["output_account_ready"]),
    "fresh_quote_positive_count": sum(1 for row in out if row["fresh_quote_positive"]),
    "bundle_envelope_built_count": sum(1 for row in out if row["bundle_envelope_built"]),
    "pm_reserve_ok_count": sum(1 for row in out if row["pm_reserve_ok"]),
    "simulate_bundle_called_count": sum(1 for row in out if row["simulate_bundle_called"]),
    "simulate_bundle_pass_count": sum(1 for row in out if row["simulate_bundle_pass"]),
    "bundle_ready_no_submit_rows": decision_counts.get("BACKRUN_PREPROCESSED_BUNDLE_READY_NO_SUBMIT", 0),
    "decision_counts": dict(decision_counts),
    "provider_endpoint": redacted_url(provider_endpoint),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(source_rows_path or ""),
    "result_dir": str(result_dir),
    "provider_endpoint": redacted_url(provider_endpoint),
    "tip_accounts_count": len(tip_accounts),
    "signer_staged": signer_staged,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v}"
        for k, v in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBR1 preprocessed bundle no-submit written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

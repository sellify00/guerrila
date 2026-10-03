#!/usr/bin/env bash
# 22_launch4_entry_bundle_nosubmit.sh — build one ordered Launch Continuation entry bundle [target buy, our buy, tip] and simulate it without submitting

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch4 — Entry Bundle No-Submit"

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
STEP="${SHADOW_LAUNCH4B_STEP:-22_launch4_entry_bundle_nosubmit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/launch4_entry_bundle_nosubmit_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch4_entry_bundle_nosubmit_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

SIM_ROWS_PATH="${SHADOW_LAUNCH4B_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
CENSUS_ROWS_PATH="${SHADOW_LAUNCH4B_CENSUS_ROWS_PATH:-$RESULTS_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LAUNCH4B_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LAUNCH4B_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
UTILS_PATH="${SHADOW_LAUNCH4B_UTILS_PATH:-$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py}"

SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCH4B_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_LAUNCH4B_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
BUNDLE_RPC_URL="${SHADOW_LAUNCH4B_BUNDLE_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-}}}}}"
BUNDLE_API_KEY="${SHADOW_LAUNCH4B_BUNDLE_API_KEY:-${SHADOW_HELIUS_BUNDLE_API_KEY:-}}"

DEFAULT_BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCH4B_DEFAULT_BASE_TX_FEE_LAMPORTS:-5000}"
DEFAULT_TIP_LAMPORTS="${SHADOW_LAUNCH4B_DEFAULT_TIP_LAMPORTS:-200000}"
MAX_ROWS="${SHADOW_LAUNCH4B_MAX_ROWS:-5}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"
[[ -f "$CENSUS_ROWS_PATH" ]] || CENSUS_ROWS_PATH="$(latest_rows_for_step launch0_new_mint_pool_census_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_rows_for_step runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_rows_for_step runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$SIM_ROWS_PATH" "$CENSUS_ROWS_PATH" "$LS13A_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$UTILS_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$BUNDLE_RPC_URL" "$BUNDLE_API_KEY" "$DEFAULT_BASE_TX_FEE_LAMPORTS" "$DEFAULT_TIP_LAMPORTS" "$MAX_ROWS" <<'PY'
from __future__ import annotations

import base64
import importlib.util
import json
import os
import re
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
import sys
from collections import Counter
from pathlib import Path
from typing import Any

(
    sim_rows_arg,
    census_rows_arg,
    ls13a_rows_arg,
    alt_rows_arg,
    utils_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    signer_path_arg,
    rpc_url,
    bundle_rpc_url,
    bundle_api_key,
    default_base_tx_fee_arg,
    default_tip_arg,
    max_rows_arg,
) = sys.argv[1:17]

sim_rows_path = Path(sim_rows_arg)
census_rows_path = Path(census_rows_arg)
ls13a_rows_path = Path(ls13a_rows_arg)
alt_rows_path = Path(alt_rows_arg)
utils_path = Path(utils_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
default_base_tx_fee_lamports = int(float(default_base_tx_fee_arg or 5000))
default_tip_lamports = int(float(default_tip_arg or 200000))
max_rows = max(0, int(max_rows_arg or "5"))

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

from solders.hash import Hash
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer
from solders.transaction import Transaction
from spl.token.instructions import create_idempotent_associated_token_account, get_associated_token_address

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
CREATE_POOL_DISCRIMINATOR = bytes([233, 146, 209, 142, 207, 104, 64, 188])
TIP_ACCOUNT_URLS = [
    "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
]


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


def sol_to_lamports(value: Any) -> int:
    try:
        return max(0, int(round(float(value) * 1_000_000_000.0)))
    except Exception:
        return 0


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


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


def latest_blockhash() -> tuple[str, dict[str, Any] | None]:
    result, error = rpc("getLatestBlockhash")
    if error is not None:
        return "", error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value", value)
    blockhash = text((value or {}).get("blockhash"))
    if not blockhash:
        return "", {"error": "latest_blockhash_missing"}
    return blockhash, None


def get_token_accounts_by_owner(owner: str, mint: str) -> list[dict[str, Any]]:
    result, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    value = (result or {}).get("result", result)
    rows = value.get("value") if isinstance(value, dict) else []
    out = []
    for row in rows or []:
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


def first_or_derived_account(owner_pubkey: str, mint: str, token_program: str) -> dict[str, Any]:
    accounts = get_token_accounts_by_owner(owner_pubkey, mint) if owner_pubkey and mint else []
    richest = max(accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
    actual_pubkey = text(richest.get("pubkey"))
    actual_balance_raw = safe_int(richest.get("amount_raw"))
    derived_pubkey = ""
    derived_error = ""
    try:
        derived_pubkey = str(
            get_associated_token_address(
                Pubkey.from_string(owner_pubkey),
                Pubkey.from_string(mint),
                Pubkey.from_string(token_program),
            )
        )
    except Exception as exc:
        derived_error = str(exc)
    selected_pubkey = actual_pubkey or derived_pubkey
    return {
        "actual_pubkey": actual_pubkey,
        "actual_balance_raw": actual_balance_raw,
        "actual_count": len(accounts),
        "derived_pubkey": derived_pubkey,
        "derived_error": derived_error,
        "selected_pubkey": selected_pubkey,
    }


def permission_map(parsed: dict[str, Any], resolved_account_keys: list[str], alt_row: dict[str, Any]) -> dict[int, tuple[bool, bool]]:
    header = parsed.get("header") or {}
    num_required_signatures = safe_int(header.get("num_required_signatures"))
    num_readonly_signed_accounts = safe_int(header.get("num_readonly_signed_accounts"))
    num_readonly_unsigned_accounts = safe_int(header.get("num_readonly_unsigned_accounts"))
    static_account_keys = list(parsed.get("static_account_keys") or [])
    static_count = len(static_account_keys)
    loaded_writable_count = safe_int(alt_row.get("resolved_writable_count"))
    permissions: dict[int, tuple[bool, bool]] = {}
    for idx in range(len(resolved_account_keys)):
        if idx < static_count:
            is_signer = idx < num_required_signatures
            if is_signer:
                is_writable = idx < max(0, num_required_signatures - num_readonly_signed_accounts)
            else:
                is_writable = idx < max(num_required_signatures, static_count - num_readonly_unsigned_accounts)
        else:
            loaded_index = idx - static_count
            is_signer = False
            is_writable = loaded_index < loaded_writable_count
        permissions[idx] = (is_signer, is_writable)
    return permissions


def encode_direct_ix_data(route_mode: str, amount_in_raw: int, min_out_raw: int) -> bytes:
    if route_mode == "sell":
        return SELL_DISCRIMINATOR + int(amount_in_raw).to_bytes(8, "little") + int(min_out_raw).to_bytes(8, "little")
    if route_mode == "buy_exact_quote_in":
        return BUY_EXACT_QUOTE_IN_DISCRIMINATOR + int(amount_in_raw).to_bytes(8, "little") + int(min_out_raw).to_bytes(8, "little") + bytes([1])
    return b""


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


def build_tip_transaction(owner_keypair: Keypair, tip_account: str, lamports: int):
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
    if "blockhash" in raw:
        return "SIM_FAIL_BLOCKHASH"
    if "processed" in raw or "already" in raw or "landed" in raw:
        return "SIM_FAIL_TARGET_ALREADY_PROCESSED"
    if "provider" in raw or "unauthorized" in raw or "api key" in raw:
        return "SIM_FAIL_PROVIDER"
    return "SIM_FAIL_UNKNOWN"


def le_u64(data: bytes, start: int) -> int:
    if start + 8 > len(data):
        return 0
    return int.from_bytes(data[start:start + 8], "little")


def pump_raw_kind(data: bytes) -> str:
    prefix = data[:8]
    if prefix == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
        return "buy_exact_quote_in"
    if prefix == SELL_DISCRIMINATOR:
        return "sell"
    if prefix == CREATE_POOL_DISCRIMINATOR:
        return "create_pool"
    return "other"


def pump_template_for_route(route_mode: str) -> dict[str, int]:
    if route_mode == "sell":
        return {
            "core_mint_pos": 3,
            "launch_mint_pos": 4,
            "user_input_pos": 5,
            "user_output_pos": 6,
            "min_accounts": 7,
        }
    if route_mode == "buy_exact_quote_in":
        return {
            "core_mint_pos": 4,
            "launch_mint_pos": 3,
            "user_input_pos": 6,
            "user_output_pos": 5,
            "min_accounts": 7,
        }
    return {}


def classify_launch_target_shape(
    parsed: dict[str, Any],
    resolved_account_keys: list[str],
    target_instruction_index: int,
    target_pool: str,
    core_mint: str,
    launch_mint: str,
) -> dict[str, Any]:
    pump_views = [
        ix
        for ix in utils.instruction_views(parsed, resolved_account_keys)
        if text(ix.get("program_id")) == PUMPFUN_PROGRAM_ID
    ]
    if not pump_views:
        return {
            "target_kind": "TARGET_DECODE_FAILED",
            "first_shape_blocker": "target_pumpfun_instruction_missing",
            "selected_ix": None,
            "selected_route_mode": "",
            "selected_template": {},
            "selector_target_kind": "TARGET_DECODE_FAILED",
            "selected_target_instruction_index": -1,
            "target_instruction_kinds": [],
            "has_create_ix": False,
            "has_buy_ix": False,
            "has_sell_ix": False,
            "economic_buy_ix_count": 0,
            "economic_sell_ix_count": 0,
            "buy_amount_sol_lamports": 0,
            "buy_token_out_raw": 0,
        }

    typed_views: list[dict[str, Any]] = []
    selector_view: dict[str, Any] | None = None
    for ix in pump_views:
        accounts = [text(item) for item in (ix.get("accounts") or [])]
        data = ix.get("data") or b""
        raw_kind = pump_raw_kind(data)
        pool = text(accounts[0]) if accounts else ""
        mint_in = ""
        mint_out = ""
        if raw_kind == "buy_exact_quote_in" and len(accounts) >= 23:
            mint_in = text(accounts[4])
            mint_out = text(accounts[3])
        elif raw_kind == "sell" and len(accounts) >= 21:
            mint_in = text(accounts[3])
            mint_out = text(accounts[4])
        typed = {
            "instruction_index": safe_int(ix.get("instruction_index")),
            "ix": ix,
            "accounts": accounts,
            "data": data,
            "raw_kind": raw_kind,
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "pool_matches": bool(target_pool) and pool == target_pool,
            "launch_buy": bool(target_pool) and pool == target_pool and mint_in == core_mint and mint_out == launch_mint,
            "launch_sell": bool(target_pool) and pool == target_pool and mint_in == launch_mint and mint_out == core_mint,
            "amount_in_raw": le_u64(data, 8),
            "min_out_raw": le_u64(data, 16),
        }
        typed_views.append(typed)
        if typed["instruction_index"] == target_instruction_index:
            selector_view = typed

    selector_target_kind = "TARGET_DECODE_FAILED"
    if selector_view is not None:
        if selector_view["launch_buy"]:
            selector_target_kind = "TARGET_PUMP_BUY_PREFIX"
        elif selector_view["raw_kind"] == "create_pool":
            selector_target_kind = "TARGET_PUMP_CREATE_ONLY"
        elif selector_view["launch_sell"] or selector_view["raw_kind"] in {"buy_exact_quote_in", "sell"}:
            selector_target_kind = "TARGET_SELL_OR_NON_BUY"
        else:
            selector_target_kind = "TARGET_DECODE_FAILED"

    target_instruction_kinds = [text(item.get("raw_kind")) for item in typed_views if text(item.get("raw_kind"))]
    has_create_ix = any(item["raw_kind"] == "create_pool" for item in typed_views)
    has_buy_ix = any(item["raw_kind"] == "buy_exact_quote_in" for item in typed_views)
    has_sell_ix = any(item["raw_kind"] == "sell" for item in typed_views)
    matching_pool_views = [item for item in typed_views if item["pool_matches"]]
    buy_views = [item for item in matching_pool_views if item["launch_buy"]]
    sell_views = [item for item in matching_pool_views if item["launch_sell"]]

    if not matching_pool_views:
        return {
            "target_kind": "TARGET_MINT_MISMATCH",
            "first_shape_blocker": "target_mint_mismatch",
            "selected_ix": None,
            "selected_route_mode": "",
            "selected_template": {},
            "selector_target_kind": selector_target_kind,
            "selected_target_instruction_index": -1,
            "target_instruction_kinds": target_instruction_kinds,
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_views),
            "economic_sell_ix_count": len(sell_views),
            "buy_amount_sol_lamports": 0,
            "buy_token_out_raw": 0,
        }
    if has_create_ix and not buy_views:
        return {
            "target_kind": "TARGET_PUMP_CREATE_ONLY",
            "first_shape_blocker": "target_create_only",
            "selected_ix": None,
            "selected_route_mode": "",
            "selected_template": {},
            "selector_target_kind": selector_target_kind,
            "selected_target_instruction_index": -1,
            "target_instruction_kinds": target_instruction_kinds,
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_views),
            "economic_sell_ix_count": len(sell_views),
            "buy_amount_sol_lamports": 0,
            "buy_token_out_raw": 0,
        }
    if not buy_views or sell_views:
        return {
            "target_kind": "TARGET_SELL_OR_NON_BUY",
            "first_shape_blocker": "target_sell_or_non_buy",
            "selected_ix": None,
            "selected_route_mode": "",
            "selected_template": {},
            "selector_target_kind": selector_target_kind,
            "selected_target_instruction_index": -1,
            "target_instruction_kinds": target_instruction_kinds,
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": len(buy_views),
            "economic_sell_ix_count": len(sell_views),
            "buy_amount_sol_lamports": 0,
            "buy_token_out_raw": 0,
        }

    selected_view = selector_view if selector_view and selector_view["launch_buy"] else sorted(buy_views, key=lambda item: item["instruction_index"])[0]
    if has_create_ix:
        target_kind = "TARGET_PUMP_CREATE_AND_BUY"
    elif selector_view and selected_view["instruction_index"] == selector_view["instruction_index"]:
        target_kind = "TARGET_PUMP_BUY_PREFIX"
    else:
        target_kind = "TARGET_PUMP_BUY_AFTER_CREATE"

    return {
        "target_kind": target_kind,
        "first_shape_blocker": "none",
        "selected_ix": selected_view,
        "selected_route_mode": text(selected_view.get("raw_kind")),
        "selected_template": pump_template_for_route(text(selected_view.get("raw_kind"))),
        "selector_target_kind": selector_target_kind,
        "selected_target_instruction_index": safe_int(selected_view.get("instruction_index"), -1),
        "target_instruction_kinds": target_instruction_kinds,
        "has_create_ix": has_create_ix,
        "has_buy_ix": has_buy_ix,
        "has_sell_ix": has_sell_ix,
        "economic_buy_ix_count": len(buy_views),
        "economic_sell_ix_count": len(sell_views),
        "buy_amount_sol_lamports": safe_int(selected_view.get("amount_in_raw")),
        "buy_token_out_raw": safe_int(selected_view.get("min_out_raw")),
    }


sim_rows = load_jsonl(sim_rows_path)
census_rows = load_jsonl(census_rows_path)
ls13a_by_signature = {text(row.get("target_signature")): row for row in load_jsonl(ls13a_rows_path) if text(row.get("target_signature"))}
alt_by_signature = {text(row.get("target_signature")): row for row in load_jsonl(alt_rows_path) if text(row.get("target_signature"))}
latest_census_by_group: dict[str, dict[str, Any]] = {}
for row in census_rows:
    group_id = text(row.get("launch_group_id"))
    if not group_id:
        continue
    prev = latest_census_by_group.get(group_id)
    if prev is None or safe_int(row.get("target_seen_preprocessed_ns")) >= safe_int(prev.get("target_seen_preprocessed_ns")):
        latest_census_by_group[group_id] = row

owner_keypair = Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8"))
owner_pubkey = str(owner_keypair.pubkey())
provider_endpoint = add_api_key(bundle_rpc_url, bundle_api_key)
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
selected_tip_account = text(tip_accounts[0]) if tip_accounts else ""

rows_out: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

if max_rows > 0:
    sim_rows = sim_rows[:max_rows]

for index, sim_row in enumerate(sim_rows, start=1):
    group_id = text(sim_row.get("launch_group_id"))
    census_row = latest_census_by_group.get(group_id, {})
    target_signature = text(sim_row.get("latest_signature")) or text(census_row.get("target_signature"))
    target_instruction_index = safe_int(sim_row.get("target_instruction_index") or census_row.get("target_instruction_index"))
    ls13a_row = ls13a_by_signature.get(target_signature, {})
    alt_row = alt_by_signature.get(target_signature, {})

    venue = text(sim_row.get("venue"))
    core_mint = text(sim_row.get("core_mint"))
    launch_mint = text(sim_row.get("launch_mint"))
    input_amount_raw = sol_to_lamports(sim_row.get("max_spend_sol"))
    entry_min_out_raw = safe_int(sim_row.get("entry_min_out_raw"))
    target_tx_base64 = text(ls13a_row.get("target_tx_base64"))
    target_tx_bytes_available = bool(target_tx_base64)
    target_blockhash_valid = boolish(sim_row.get("target_blockhash_valid") if "target_blockhash_valid" in sim_row else census_row.get("target_blockhash_valid"))
    target_blockhash = text(sim_row.get("target_blockhash")) or text(census_row.get("target_blockhash"))
    target_already_landed = boolish(sim_row.get("target_already_landed_at_first_check") if "target_already_landed_at_first_check" in sim_row else census_row.get("target_already_landed_at_first_check"))
    target_not_landed = boolish(sim_row.get("target_not_landed_at_first_check") if "target_not_landed_at_first_check" in sim_row else census_row.get("target_not_landed_at_first_check"))
    target_seen_preprocessed_ns = safe_int(sim_row.get("target_seen_preprocessed_ns") if "target_seen_preprocessed_ns" in sim_row else census_row.get("target_seen_preprocessed_ns"))
    latest_seen_preprocessed_ns = safe_int(sim_row.get("latest_seen_preprocessed_ns") if "latest_seen_preprocessed_ns" in sim_row else census_row.get("latest_seen_preprocessed_ns"))
    first_seen_ns = safe_int(sim_row.get("first_seen_ns") if "first_seen_ns" in sim_row else census_row.get("first_seen_ns"))
    target_seen_host_mono_ns = safe_int(sim_row.get("target_seen_host_mono_ns") if "target_seen_host_mono_ns" in sim_row else census_row.get("target_seen_host_mono_ns"))
    target_seen_unix_ns = safe_int(sim_row.get("target_seen_unix_ns") if "target_seen_unix_ns" in sim_row else census_row.get("target_seen_unix_ns"))
    target_source_slice_id = text(sim_row.get("target_source_slice_id")) or text(census_row.get("target_source_slice_id"))
    target_source_row_id = text(sim_row.get("target_source_row_id")) or text(census_row.get("target_source_row_id"))
    selected_row_source = text(sim_row.get("selected_row_source")) or text(census_row.get("selected_row_source")) or "live_current_window"
    bundle_ready_ns = 0
    target_status_at_bundle_build = "unknown"

    decision = "PARK_ENTRY_NOT_READY"
    first_blocker = "sim_not_ready"
    target_kind = ""
    selector_target_kind = ""
    target_route_mode = ""
    direct_route_mode = "buy_exact_quote_in"
    selected_target_instruction_index = -1
    target_instruction_kinds: list[str] = []
    has_create_ix = False
    has_buy_ix = False
    has_sell_ix = False
    economic_buy_ix_count = 0
    economic_sell_ix_count = 0
    buy_amount_sol_lamports = 0
    buy_token_out_raw = 0
    our_buy_tx_built = False
    our_buy_tx_signed = False
    tip_tx_built = False
    bundle_built = False
    simulate_bundle_called = False
    simulate_bundle_pass = False
    simulate_bundle_failure_reason = ""
    simulate_bundle_failure_class = ""
    target_tx_first = False
    our_buy_second = False
    tip_present = False
    fresh_entry_quote = boolish(sim_row.get("direct_entry_quote_ok"))

    selected_input = first_or_derived_account(owner_pubkey, core_mint, TOKEN_PROGRAM) if core_mint else {}
    selected_output = first_or_derived_account(owner_pubkey, launch_mint, TOKEN_PROGRAM) if launch_mint else {}
    input_account_ready = bool(text(selected_input.get("actual_pubkey"))) and safe_int(selected_input.get("actual_balance_raw")) >= input_amount_raw > 0
    output_account_ready = bool(text(selected_output.get("actual_pubkey")))
    inline_account_required = boolish(sim_row.get("inline_account_required")) or not output_account_ready
    inline_account_creation_allowed = boolish(sim_row.get("inline_account_creation_allowed")) and inline_account_required and bool(text(selected_output.get("selected_pubkey")))
    output_account_surface_ok = output_account_ready or inline_account_creation_allowed
    build_error = ""

    if text(sim_row.get("sim_decision")) == "PARK_ATA_REQUIRED":
        decision = "PARK_ATA_REQUIRED"
        first_blocker = text(sim_row.get("first_sim_blocker")) or "entry_output_account_missing"
    elif text(sim_row.get("sim_decision")) != "LAUNCH_TINY_SIM_READY":
        decision = "PARK_ENTRY_NOT_READY"
        first_blocker = text(sim_row.get("first_sim_blocker")) or "sim_not_ready"
    elif venue != "pumpfun_amm":
        decision = "PARK_ENTRY_ROUTE_UNSUPPORTED"
        first_blocker = "venue_not_pumpfun_amm"
    elif core_mint != WSOL_MINT:
        decision = "PARK_CORE_INPUT_UNSUPPORTED"
        first_blocker = "core_input_not_wsol"
    elif not boolish(sim_row.get("mint_policy_pass")):
        decision = "PARK_POLICY"
        first_blocker = "mint_policy_failed"
    elif not target_tx_bytes_available:
        decision = "PARK_TARGET_TX_MISSING"
        first_blocker = "target_tx_missing"
    elif target_seen_preprocessed_ns <= 0:
        decision = "PARK_MISSING_FRESHNESS_LINEAGE"
        first_blocker = "missing_freshness_lineage"
    elif not target_not_landed or target_already_landed:
        decision = "PARK_TARGET_ALREADY_LANDED"
        first_blocker = "target_already_landed"
    elif not target_blockhash_valid:
        decision = "PARK_TARGET_BLOCKHASH_INVALID"
        first_blocker = "target_blockhash_invalid"
    elif not fresh_entry_quote or entry_min_out_raw <= 0 or input_amount_raw <= 0:
        decision = "PARK_ENTRY_QUOTE_NOT_READY"
        first_blocker = "entry_quote_not_ready"
    elif not input_account_ready:
        decision = "PARK_INPUT_ACCOUNT_MISSING"
        first_blocker = "core_input_account_missing_or_unfunded"
    elif not output_account_surface_ok:
        decision = "PARK_OUTPUT_ACCOUNT_MISSING"
        first_blocker = "launch_output_account_missing"
    else:
        parsed, parse_error = utils.parse_wire_tx(target_tx_base64)
        resolved_account_keys = [text(item) for item in (alt_row.get("resolved_account_keys") or [])]
        if parsed is None:
            decision = "PARK_TARGET_PARSE_FAILED"
            first_blocker = parse_error or "target_tx_parse_failed"
        else:
            if not resolved_account_keys:
                resolved_account_keys = list(parsed.get("static_account_keys") or [])
            target_blockhash = target_blockhash or text(parsed.get("recent_blockhash"))
            target_shape = classify_launch_target_shape(
                parsed,
                resolved_account_keys,
                target_instruction_index,
                text(sim_row.get("pool")) or text(census_row.get("pool")),
                core_mint,
                launch_mint,
            )
            target_kind = text(target_shape.get("target_kind"))
            selector_target_kind = text(target_shape.get("selector_target_kind"))
            selected_target_instruction_index = safe_int(target_shape.get("selected_target_instruction_index"), -1)
            target_instruction_kinds = list(target_shape.get("target_instruction_kinds") or [])
            has_create_ix = boolish(target_shape.get("has_create_ix"))
            has_buy_ix = boolish(target_shape.get("has_buy_ix"))
            has_sell_ix = boolish(target_shape.get("has_sell_ix"))
            economic_buy_ix_count = safe_int(target_shape.get("economic_buy_ix_count"))
            economic_sell_ix_count = safe_int(target_shape.get("economic_sell_ix_count"))
            buy_amount_sol_lamports = safe_int(target_shape.get("buy_amount_sol_lamports"))
            buy_token_out_raw = safe_int(target_shape.get("buy_token_out_raw"))
            target_ix = target_shape.get("selected_ix")
            target_route_mode = text(target_shape.get("selected_route_mode"))
            direct_route_mode = target_route_mode or direct_route_mode
            template = target_shape.get("selected_template") or {}
            if target_ix is None:
                decision = "PARK_TARGET_NOT_BUY_PREFIX"
                first_blocker = text(target_shape.get("first_shape_blocker")) or "target_not_buy_prefix"
            else:
                target_accounts = [text(item) for item in (target_ix.get("accounts") or [])]
                required_len = safe_int(template.get("min_accounts"))
                core_mint_pos = safe_int(template.get("core_mint_pos"), -1)
                launch_mint_pos = safe_int(template.get("launch_mint_pos"), -1)
                user_input_pos = safe_int(template.get("user_input_pos"), -1)
                user_output_pos = safe_int(template.get("user_output_pos"), -1)
                if not template or required_len <= 0 or core_mint_pos < 0 or launch_mint_pos < 0 or user_input_pos < 0 or user_output_pos < 0:
                    decision = "PARK_TARGET_NOT_BUY_PREFIX"
                    first_blocker = "target_route_template_missing"
                elif len(target_accounts) < required_len:
                    decision = "PARK_ENTRY_TEMPLATE_MISSING"
                    first_blocker = "target_accounts_missing"
                elif core_mint != text(target_accounts[core_mint_pos]) or launch_mint != text(target_accounts[launch_mint_pos]):
                    decision = "PARK_ENTRY_TEMPLATE_MISSING"
                    first_blocker = "target_accounts_not_core_to_launch"
                else:
                    try:
                        replacements = {1: owner_pubkey}
                        if route_mode := target_route_mode:
                            if route_mode == "sell":
                                replacements[user_input_pos] = text(selected_input.get("selected_pubkey"))
                                replacements[user_output_pos] = text(selected_output.get("selected_pubkey"))
                            else:
                                replacements[user_output_pos] = text(selected_output.get("selected_pubkey"))
                                replacements[user_input_pos] = text(selected_input.get("selected_pubkey"))
                        if not replacements.get(user_output_pos) or not replacements.get(user_input_pos):
                            raise RuntimeError("derived_user_token_account_missing")
                        permissions = permission_map(parsed, resolved_account_keys, alt_row)
                        metas = []
                        for pos, acct_index in enumerate(target_ix.get("account_indices") or []):
                            if pos >= len(target_accounts):
                                raise RuntimeError("target_account_position_out_of_bounds")
                            pubkey_str = text(replacements.get(pos, target_accounts[pos]))
                            is_signer, is_writable = permissions.get(safe_int(acct_index), (False, False))
                            metas.append(AccountMeta(Pubkey.from_string(pubkey_str), is_signer, is_writable))
                        direct_ix_data = encode_direct_ix_data(direct_route_mode, input_amount_raw, entry_min_out_raw)
                        if not direct_ix_data:
                            raise RuntimeError("direct_ix_encode_failed")
                        blockhash, blockhash_error = latest_blockhash()
                        if blockhash_error is not None or not blockhash:
                            decision = "PARK_ENTRY_BLOCKHASH_UNAVAILABLE"
                            first_blocker = text((blockhash_error or {}).get("error")) or "latest_blockhash_missing"
                        else:
                            instruction = Instruction(Pubkey.from_string(PUMPFUN_PROGRAM_ID), direct_ix_data, metas)
                            instructions = []
                            if not output_account_ready and inline_account_creation_allowed:
                                instructions.append(
                                    create_idempotent_associated_token_account(
                                        owner_keypair.pubkey(),
                                        owner_keypair.pubkey(),
                                        Pubkey.from_string(launch_mint),
                                        Pubkey.from_string(TOKEN_PROGRAM),
                                    )
                                )
                            instructions.append(instruction)
                            message = Message(instructions, owner_keypair.pubkey())
                            our_buy_tx = Transaction([owner_keypair], message, Hash.from_string(blockhash))
                            our_buy_tx_b64 = base64.b64encode(bytes(our_buy_tx)).decode("ascii")
                            our_buy_tx_built = True
                            our_buy_tx_signed = True
                            tip_tx_b64, tip_error = build_tip_transaction(owner_keypair, selected_tip_account, default_tip_lamports) if selected_tip_account else ("", {"error": "tip_account_missing"})
                            tip_tx_built = bool(tip_tx_b64)
                            tip_present = tip_tx_built and bool(selected_tip_account)
                            if not tip_tx_built:
                                decision = "PARK_TIP_MISSING"
                                first_blocker = text((tip_error or {}).get("error")) or "tip_account_missing"
                            else:
                                txs_base64 = [target_tx_base64, our_buy_tx_b64, tip_tx_b64]
                                target_tx_first = txs_base64[0] == target_tx_base64
                                our_buy_second = txs_base64[1] == our_buy_tx_b64
                                bundle_built = target_tx_first and our_buy_second and tip_present
                                bundle_ready_ns = time.time_ns()
                                target_status_at_bundle_build = "landed" if target_already_landed else ("not_landed" if target_tx_bytes_available else "unknown")
                                simulate_bundle_called = bundle_built and boolish(sim_row.get("pm_reserve_inventory_ok")) and boolish(sim_row.get("pm_reserve_fee_ok")) and boolish(sim_row.get("pm_reserve_rent_ok"))
                                if not simulate_bundle_called:
                                    decision = "PARK_PM"
                                    first_blocker = "pm_reserve_not_ready"
                                else:
                                    simulate_result = simulate_bundle(provider_endpoint, txs_base64)
                                    simulate_bundle_pass = bool(simulate_result["pass"])
                                    simulate_bundle_failure_reason = text(simulate_result["failure_reason"])
                                    simulate_bundle_failure_class = classify_sim_failure(simulate_bundle_failure_reason)
                                    if simulate_bundle_pass:
                                        decision = "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT"
                                        first_blocker = "none"
                                    else:
                                        decision = simulate_bundle_failure_class or "PARK_BUNDLE_SIM_FAILED"
                                        first_blocker = simulate_bundle_failure_reason or "bundle_sim_failed"
                    except Exception as exc:
                        build_error = str(exc)
                        decision = "PARK_ENTRY_BUNDLE_NOT_BUILDABLE"
                        first_blocker = build_error or "entry_bundle_not_buildable"

    rows_out.append(
        {
            **sim_row,
            "record_type": "launch4_entry_bundle_nosubmit_row",
            "target_signature": target_signature,
            "target_instruction_index": target_instruction_index,
            "selected_target_instruction_index": selected_target_instruction_index,
            "target_kind": target_kind,
            "selector_target_kind": selector_target_kind,
            "target_instruction_kinds": target_instruction_kinds,
            "has_create_ix": has_create_ix,
            "has_buy_ix": has_buy_ix,
            "has_sell_ix": has_sell_ix,
            "economic_buy_ix_count": economic_buy_ix_count,
            "economic_sell_ix_count": economic_sell_ix_count,
            "buy_amount_sol_lamports": buy_amount_sol_lamports,
            "buy_token_out_raw": buy_token_out_raw,
            "target_route_mode": target_route_mode,
            "direct_route_mode": direct_route_mode,
            "target_tx_bytes_available": target_tx_bytes_available,
            "target_blockhash": target_blockhash,
            "target_not_landed": target_not_landed,
            "target_blockhash_valid": target_blockhash_valid,
            "target_blockhash_valid_at_capture": boolish(sim_row.get("target_blockhash_valid_at_capture")) if "target_blockhash_valid_at_capture" in sim_row else target_blockhash_valid,
            "target_status_at_capture": text(sim_row.get("target_status_at_capture")) or ("landed" if target_already_landed else ("not_landed" if target_tx_bytes_available else "unknown")),
            "target_blockhash_valid_at_selector": boolish(sim_row.get("target_blockhash_valid_at_selector")) if "target_blockhash_valid_at_selector" in sim_row else target_blockhash_valid,
            "target_status_at_selector": text(sim_row.get("target_status_at_selector")) or ("landed" if target_already_landed else ("not_landed" if target_tx_bytes_available else "unknown")),
            "target_blockhash_valid_at_bundle_build": target_blockhash_valid,
            "target_status_at_bundle_build": target_status_at_bundle_build,
            "target_seen_preprocessed_ns": target_seen_preprocessed_ns or None,
            "latest_seen_preprocessed_ns": latest_seen_preprocessed_ns or None,
            "first_seen_ns": first_seen_ns or None,
            "target_seen_host_mono_ns": target_seen_host_mono_ns or None,
            "target_seen_unix_ns": target_seen_unix_ns or None,
            "target_source_slice_id": target_source_slice_id,
            "target_source_row_id": target_source_row_id,
            "selected_row_source": selected_row_source,
            "bundle_ready_ns": bundle_ready_ns or None,
            "input_account_ready": input_account_ready,
            "output_account_ready": output_account_ready,
            "inline_account_required": inline_account_required,
            "inline_account_creation_allowed": inline_account_creation_allowed,
            "output_account_surface_ok": output_account_surface_ok,
            "fresh_entry_quote": fresh_entry_quote,
            "our_buy_tx_built": our_buy_tx_built,
            "our_buy_tx_signed": our_buy_tx_signed,
            "tip_present": tip_present,
            "bundle_built": bundle_built,
            "target_tx_first": target_tx_first,
            "our_buy_second": our_buy_second,
            "pm_reserve_ok": boolish(sim_row.get("pm_reserve_inventory_ok")) and boolish(sim_row.get("pm_reserve_fee_ok")) and boolish(sim_row.get("pm_reserve_rent_ok")),
            "simulate_bundle_called": simulate_bundle_called,
            "simulate_bundle_pass": simulate_bundle_pass,
            "simulate_bundle_failure_reason": simulate_bundle_failure_reason,
            "simulate_bundle_failure_class": simulate_bundle_failure_class,
            "selected_tip_account": selected_tip_account,
            "provider_endpoint": redacted_url(provider_endpoint),
            "decision": decision,
            "first_entry_bundle_blocker": first_blocker,
            "build_error": build_error,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )
    decision_counts[decision] += 1

summary = {
    "status": "LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT" if decision_counts.get("LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT", 0) > 0 else "PARK_LAUNCH_ENTRY_BUNDLE_NOSUBMIT",
    "rows_seen": len(rows_out),
    "entry_bundle_ready_no_submit_rows": decision_counts.get("LAUNCH_ENTRY_BUNDLE_READY_NO_SUBMIT", 0),
    "ata_required_rows": decision_counts.get("PARK_ATA_REQUIRED", 0),
    "target_tx_bytes_available_count": sum(1 for row in rows_out if boolish(row.get("target_tx_bytes_available"))),
    "target_not_landed_count": sum(1 for row in rows_out if boolish(row.get("target_not_landed"))),
    "bundle_built_count": sum(1 for row in rows_out if boolish(row.get("bundle_built"))),
    "simulate_bundle_called_count": sum(1 for row in rows_out if boolish(row.get("simulate_bundle_called"))),
    "simulate_bundle_pass_count": sum(1 for row in rows_out if boolish(row.get("simulate_bundle_pass"))),
    "rows_with_target_seen_preprocessed_ns": sum(1 for row in rows_out if safe_int(row.get("target_seen_preprocessed_ns")) > 0),
    "decision_counts": dict(decision_counts),
    "provider_endpoint": redacted_url(provider_endpoint),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
manifest = {
    "sim_rows": str(sim_rows_path),
    "census_rows": str(census_rows_path),
    "ls13a_rows": str(ls13a_rows_path),
    "alt_rows": str(alt_rows_path),
    "signer_path": str(signer_keypair_path or ""),
    "rpc_url": rpc_url,
    "provider_endpoint": redacted_url(provider_endpoint),
    "tip_accounts_count": len(tip_accounts),
    "live_send": False,
    "sendBundle": False,
}

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows_out), encoding="utf-8")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Launch entry bundle no-submit written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

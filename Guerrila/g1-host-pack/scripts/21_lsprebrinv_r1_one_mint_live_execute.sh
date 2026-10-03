#!/usr/bin/env bash
# 21_lsprebrinv_r1_one_mint_live_execute.sh — one-mint R1 Backrun inventory-band live executor

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1L — One-Mint Live Execute"

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
STEP="${SHADOW_LSPREBRINV_R1L_STEP:-21_lsprebrinv_r1_one_mint_live_execute}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
REPORT_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_live_execute.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_live_execute_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PROPOSAL_JSON_PATH="${SHADOW_LSPREBRINV_R1L_PROPOSAL_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_one_mint_proposal/r1_one_mint_proposal.json}"
PROPOSAL_SCRIPT="${SHADOW_LSPREBRINV_R1L_PROPOSAL_SCRIPT:-$PACK_DIR/scripts/21_lsprebrinv_r1_one_mint_proposal.sh}"
DIRECT_ROWS_PATH="${SHADOW_LSPREBRINV_R1L_DIRECT_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LSPREBRINV_R1L_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LSPREBRINV_R1L_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
REFRESH_INVENTORY_SCRIPT="${SHADOW_LSPREBRINV_R1L_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"

SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1L_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_LSPREBRINV_R1L_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-${HELIUS_SENDER_URL:-}}"
SENDER_API_KEY="${SHADOW_HELIUS_SENDER_API_KEY:-}"

TARGET_MINT="${SHADOW_R1_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_R1_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"
RISK_TIER="${SHADOW_R1_RISK_TIER:-R1_BACKRUN_TINY_INVENTORY_BAND}"
MAX_SPEND_SOL="${SHADOW_R1_MAX_SPEND_SOL:-0.005}"
MAX_LOSS_SOL="${SHADOW_R1_MAX_LOSS_SOL:-0.0025}"
TTL_SECONDS="${SHADOW_R1_TTL_SECONDS:-300}"
STOP_LOSS_BPS="${SHADOW_R1_STOP_LOSS_BPS:-300}"
PRESEND_WAIT_SECONDS="${SHADOW_R1_PRESEND_WAIT_SECONDS:-300}"
PRESEND_QUOTE_POLL_MS="${SHADOW_R1_PRESEND_QUOTE_POLL_MS:-2000}"
REQUIRE_DIRECT_PUMPFUN_EXIT="${SHADOW_R1_REQUIRE_DIRECT_PUMPFUN_EXIT:-true}"
EXECUTE_CONFIRM="${SHADOW_R1_EXECUTE_CONFIRM:-}"
EXECUTE_CONFIRM_REQUIRED="${SHADOW_R1_EXECUTE_CONFIRM_REQUIRED:-EXECUTE_R1_HXN_ONE_MINT_0005SOL}"

SLIPPAGE_BPS="${SHADOW_LSPREBRINV_R1L_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1L_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1L_PRIORITY_FEE_LAMPORTS:-0}"
SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_R1L_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_R1L_TIP_LAMPORTS:-0}}}"
SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-${SHADOW_LSPREBRINV_R1L_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}}"
TIP_LAMPORTS="$SENDER_MIN_TIP_LAMPORTS"
MONITOR_POLL_SECONDS="${SHADOW_LSPREBRINV_R1L_MONITOR_POLL_SECONDS:-2}"
FLATTEN_RETRY_LIMIT="${SHADOW_LSPREBRINV_R1L_FLATTEN_RETRY_LIMIT:-1}"
EXPECTED_PROPOSAL_VERDICT="${SHADOW_LSPREBRINV_R1L_EXPECTED_PROPOSAL_VERDICT:-R1_PROPOSAL_READY_ONE_MINT_LIVE_WITH_SENDER_TIP}"
DAILY_GUARD_ROOT="${SHADOW_R1_DAILY_GUARD_ROOT:-$PACK_DIR/results/runtime_daily_risk}"
mkdir -p "$DAILY_GUARD_ROOT"
TODAY_UTC="$(date -u +%F)"
export SHADOW_R1_DAILY_LEDGER_PATH="${SHADOW_R1_DAILY_LEDGER_PATH:-$DAILY_GUARD_ROOT/${STEP}_${TODAY_UTC}.json}"
export SHADOW_R1_DAILY_PROFIT_TARGET_USD="${SHADOW_R1_DAILY_PROFIT_TARGET_USD:-60}"
export SHADOW_R1_DAILY_MAX_LOSS_USD="${SHADOW_R1_DAILY_MAX_LOSS_USD:-25}"
export SHADOW_R1_DAILY_MAX_TRADES="${SHADOW_R1_DAILY_MAX_TRADES:-12}"
export SHADOW_R1_DAILY_SOL_USD="${SHADOW_R1_DAILY_SOL_USD:-${SHADOW_RUN60_SOL_USD_EQUIVALENT:-0}}"

latest_file_for_pattern() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PROPOSAL_JSON_PATH" ]] || PROPOSAL_JSON_PATH="$(latest_file_for_pattern r1_one_mint_proposal.json)"
[[ -f "$DIRECT_ROWS_PATH" ]] || DIRECT_ROWS_PATH="$(latest_file_for_pattern runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_file_for_pattern runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_file_for_pattern runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT"

# 1. Re-run the live gate proposal before any send.
bash "$PROPOSAL_SCRIPT"

python3 - "$STEP" "$PROPOSAL_JSON_PATH" "$DIRECT_ROWS_PATH" "$LS13A_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$UTILS_PATH" "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$SENDER_URL" "$SENDER_API_KEY" "$REFRESH_INVENTORY_SCRIPT" "$TARGET_MINT" "$TARGET_POOL" "$RISK_TIER" "$MAX_SPEND_SOL" "$MAX_LOSS_SOL" "$TTL_SECONDS" "$STOP_LOSS_BPS" "$PRESEND_WAIT_SECONDS" "$PRESEND_QUOTE_POLL_MS" "$REQUIRE_DIRECT_PUMPFUN_EXIT" "$EXECUTE_CONFIRM" "$EXECUTE_CONFIRM_REQUIRED" "$SLIPPAGE_BPS" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" "$TIP_LAMPORTS" "$SENDER_TIP_ACCOUNT" "$MONITOR_POLL_SECONDS" "$FLATTEN_RETRY_LIMIT" "$EXPECTED_PROPOSAL_VERDICT" <<'PY'
from __future__ import annotations

import atexit
import base64
import importlib.util
import json
import os
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from solders.hash import Hash
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer
from solders.transaction import Transaction
from spl.token.instructions import get_associated_token_address

(
    step_name_arg,
    proposal_json_arg,
    direct_rows_arg,
    ls13a_rows_arg,
    alt_rows_arg,
    utils_path_arg,
    manifest_arg,
    report_json_arg,
    summary_json_arg,
    summary_txt_arg,
    signer_path_arg,
    rpc_url,
    sender_url_arg,
    sender_api_key_arg,
    refresh_inventory_script_arg,
    target_mint_arg,
    target_pool_arg,
    risk_tier_arg,
    max_spend_sol_arg,
    max_loss_sol_arg,
    ttl_seconds_arg,
    stop_loss_bps_arg,
    presend_wait_seconds_arg,
    presend_quote_poll_ms_arg,
    require_direct_pumpfun_exit_arg,
    execute_confirm_arg,
    execute_confirm_required_arg,
    slippage_bps_arg,
    base_tx_fee_lamports_arg,
    priority_fee_lamports_arg,
    tip_lamports_arg,
    sender_tip_account_arg,
    monitor_poll_seconds_arg,
    flatten_retry_limit_arg,
    expected_proposal_verdict_arg,
) = sys.argv[1:36]

step_name = str(step_name_arg or "21_lsprebrinv_r1_one_mint_live_execute").strip()
proposal_json_path = Path(proposal_json_arg)
direct_rows_path = Path(direct_rows_arg)
ls13a_rows_path = Path(ls13a_rows_arg)
alt_rows_path = Path(alt_rows_arg)
utils_path = Path(utils_path_arg)
manifest_path = Path(manifest_arg)
report_json_path = Path(report_json_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
signer_path = Path(signer_path_arg)
refresh_inventory_script = Path(refresh_inventory_script_arg)
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()
risk_tier = str(risk_tier_arg or "").strip()
max_spend_sol = float(max_spend_sol_arg or "0.005")
max_loss_sol = float(max_loss_sol_arg or "0.0025")
ttl_seconds = max(1, int(float(ttl_seconds_arg or "300")))
stop_loss_bps = max(1, int(float(stop_loss_bps_arg or "300")))
presend_wait_seconds = max(0, int(float(presend_wait_seconds_arg or "300")))
presend_quote_poll_seconds = max(0.2, float(presend_quote_poll_ms_arg or "2000") / 1000.0)
require_direct_pumpfun_exit_raw = require_direct_pumpfun_exit_arg
execute_confirm = str(execute_confirm_arg or "").strip()
execute_confirm_required = str(execute_confirm_required_arg or "").strip()
slippage_bps = max(0, int(float(slippage_bps_arg or "100")))
base_tx_fee_lamports = max(0, int(float(base_tx_fee_lamports_arg or "5000")))
priority_fee_lamports = max(0, int(float(priority_fee_lamports_arg or "0")))
tip_lamports = max(0, int(float(tip_lamports_arg or "0")))
sender_tip_account = str(sender_tip_account_arg or "").strip()
monitor_poll_seconds = max(1.0, float(monitor_poll_seconds_arg or "2"))
flatten_retry_limit = max(0, int(float(flatten_retry_limit_arg or "1")))
sender_url = str(sender_url_arg or "").strip()
sender_api_key = str(sender_api_key_arg or "").strip()
rescue_flatten_only = str(os.environ.get("SHADOW_R1_RESCUE_FLATTEN_ONLY", "")).strip().lower() in {"1", "true", "yes", "on"}

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
GLOBAL_VOLUME_ACCUMULATOR_SEED = b"global_volume_accumulator"
USER_VOLUME_ACCUMULATOR_SEED = b"user_volume_accumulator"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
BUY_EXACT_QUOTE_IN_META_FLAGS = {
    0: (False, True),
    1: (True, True),
    2: (False, False),
    3: (False, False),
    4: (False, False),
    5: (False, True),
    6: (False, True),
    7: (False, True),
    8: (False, True),
    9: (False, False),
    10: (False, True),
    11: (False, False),
    12: (False, False),
    13: (False, False),
    14: (False, False),
    15: (False, False),
    16: (False, False),
    17: (False, True),
    18: (False, False),
    19: (False, False),
    20: (False, True),
    21: (False, False),
    22: (False, False),
}
SELL_META_FLAGS = {
    0: (False, True),
    1: (True, True),
    2: (False, False),
    3: (False, False),
    4: (False, False),
    5: (False, True),
    6: (False, True),
    7: (False, True),
    8: (False, True),
    9: (False, False),
    10: (False, True),
    11: (False, False),
    12: (False, False),
    13: (False, False),
    14: (False, False),
    15: (False, False),
    16: (False, False),
    17: (False, True),
    18: (False, False),
    19: (False, False),
    20: (False, False),
}
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64
EXPECTED_PROPOSAL_VERDICT = "R1_PROPOSAL_READY_ONE_MINT_LIVE"
EXPECTED_PROPOSAL_VERDICT = str(expected_proposal_verdict_arg or "R1_PROPOSAL_READY_ONE_MINT_LIVE").strip() or "R1_PROPOSAL_READY_ONE_MINT_LIVE"
HELIUS_ALLOWED_TIP_ACCOUNTS = {
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
}

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)


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


require_direct_pumpfun_exit = boolish(require_direct_pumpfun_exit_raw if require_direct_pumpfun_exit_raw not in (None, "") else True)


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


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


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


def normalize_sender_endpoint(endpoint: str, api_key: str) -> str:
    trimmed = endpoint.strip().rstrip("/")
    if not api_key.strip():
        return trimmed
    if "?" in trimmed:
        return f"{trimmed}&api-key={urllib.parse.quote(api_key.strip())}"
    return f"{trimmed}?api-key={urllib.parse.quote(api_key.strip())}"


def sender_send_tx(tx_bytes: bytes):
    endpoint = normalize_sender_endpoint(sender_url, sender_api_key)
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            base64.b64encode(tx_bytes).decode("ascii"),
            {"encoding": "base64", "skipPreflight": True, "maxRetries": 0},
        ],
    }
    return http_post_json(endpoint, payload)


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


def get_balance(pubkey: str) -> tuple[int, dict[str, Any] | None]:
    response, error = rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error if isinstance(error, dict) else {"error": str(error)}
    result = (response or {}).get("result", response)
    if isinstance(result, dict):
        result = result.get("value", result)
    return safe_int(result), None


def account_info(pubkey: str):
    response, error = rpc("getAccountInfo", [pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    if error is not None:
        return None, error if isinstance(error, dict) else {"error": str(error)}
    result = (response or {}).get("result", response)
    if isinstance(result, dict):
        result = result.get("value")
    return result, None


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    if error is not None:
        return {}, 0, "getMultipleAccounts_failed"
    result = (response or {}).get("result", response) or {}
    slot = safe_int(((result.get("context") or {}).get("slot")))
    values = result.get("value") or []
    out: dict[str, dict[str, Any]] = {}
    for pubkey, value in zip(pubkeys, values):
        if not value:
            out[pubkey] = {"exists": False, "data_raw": b"", "lamports": 0, "owner": ""}
            continue
        data_field = value.get("data") if isinstance(value, dict) else None
        raw = b""
        if isinstance(data_field, list) and data_field:
            try:
                raw = base64.b64decode(str(data_field[0]), validate=True)
            except Exception:
                raw = b""
        out[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return out, slot, None


def get_token_accounts_by_owner(owner: str, mint: str) -> list[dict[str, Any]]:
    response, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    result = (response or {}).get("result", response)
    rows = result.get("value") if isinstance(result, dict) else []
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


def refresh_inventory() -> tuple[bool, str]:
    if not refresh_inventory_script.exists():
        return False, "refresh_script_missing"
    env = os.environ.copy()
    proc = subprocess.run(
        ["bash", str(refresh_inventory_script)],
        text=True,
        capture_output=True,
        env=env,
        timeout=120,
        check=False,
    )
    ok = proc.returncode == 0
    log = "\n".join(part for part in [proc.stdout.strip(), proc.stderr.strip()] if part)
    return ok, log[:2000]


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) != PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pool_len_mismatch"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pool_discriminator_mismatch"
    return {
        "base_mint": utils.b58(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": utils.b58(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": utils.b58(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": utils.b58(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
        return 0
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return (amount_in * reserve_out) // denominator


def min_out_from_expected(expected_out: int, slippage_bps_value: int) -> int:
    if expected_out <= 0:
        return 0
    return max(0, (expected_out * max(0, 10_000 - slippage_bps_value)) // 10_000)


def mint_metadata(mint: str) -> dict[str, Any]:
    response, error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    out = {"mint": mint, "token_program": "", "freeze_authority": "", "mint_authority": "", "extensions": [], "rpc_error": error or {}}
    result = (response or {}).get("result", response)
    if isinstance(result, dict):
        result = result.get("value")
    if not isinstance(result, dict):
        if mint == WSOL_MINT:
            out["token_program"] = TOKEN_PROGRAM
        return out
    out["token_program"] = text(result.get("owner"))
    parsed = result.get("data", {}).get("parsed", {}) if isinstance(result.get("data"), dict) else {}
    info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
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
    if token_program == TOKEN2022_PROGRAM:
        return False, "token2022_not_allowed"
    if text(meta.get("freeze_authority")):
        return False, "freeze_authority_present"
    if text(meta.get("mint_authority")):
        return False, "mint_authority_present"
    normalized_exts = {text(item).replace("_", "").replace("-", "").lower() for item in meta.get("extensions", [])}
    dangerous = sorted(
        ext
        for ext in normalized_exts
        if ext in {
            "transferhook",
            "permanentdelegate",
            "nontransferable",
            "defaultaccountstate",
            "confidentialtransfermint",
            "confidentialtransferfeeamountextension",
        }
    )
    if dangerous:
        return False, f"token2022_extension_{dangerous[0]}"
    return True, ""


def richest_account(owner: str, mint: str) -> dict[str, Any]:
    accounts = get_token_accounts_by_owner(owner, mint) if owner and mint else []
    return max(accounts, key=lambda item: safe_int(item.get("amount_raw")), default={"pubkey": "", "amount_raw": 0, "decimals": 0})


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


def target_trailing_accounts() -> list[str]:
    if target_route_mode == "buy_exact_quote_in":
        core_len = 23
    elif target_route_mode == "sell":
        core_len = 21
    else:
        return []
    return [text(item) for item in target_accounts[core_len:] if text(item)]


def build_route_remaining_accounts(
    route_mode: str,
    quote_mint: str,
    quote_token_program: str,
    user_volume_accumulator_pubkey: str,
) -> list[tuple[str, bool, bool]]:
    trailing = target_trailing_accounts()
    if len(trailing) < 2:
        return []
    fee_pair = trailing[-2:]
    leading = trailing[:-2]
    derived_cashback_quote_ata = ""
    try:
        derived_cashback_quote_ata = str(
            get_associated_token_address(
                Pubkey.from_string(user_volume_accumulator_pubkey),
                Pubkey.from_string(quote_mint),
                Pubkey.from_string(quote_token_program),
            )
        )
    except Exception:
        derived_cashback_quote_ata = ""

    has_cashback_accounts = bool(
        derived_cashback_quote_ata
        and leading
        and text(leading[0]) == derived_cashback_quote_ata
    )
    maybe_pool_v2 = ""
    if target_route_mode == "sell":
        if has_cashback_accounts:
            leading = leading[2:] if len(leading) >= 2 else []
        if leading:
            maybe_pool_v2 = text(leading[0])
    elif target_route_mode == "buy_exact_quote_in":
        if has_cashback_accounts:
            leading = leading[1:] if len(leading) >= 1 else []
        if leading:
            maybe_pool_v2 = text(leading[0])

    remaining: list[tuple[str, bool, bool]] = []
    if route_mode == "buy_exact_quote_in":
        if has_cashback_accounts and derived_cashback_quote_ata:
            remaining.append((derived_cashback_quote_ata, False, True))
        if maybe_pool_v2:
            remaining.append((maybe_pool_v2, False, False))
        remaining.append((fee_pair[0], False, False))
        remaining.append((fee_pair[1], False, True))
        return remaining

    if has_cashback_accounts and derived_cashback_quote_ata:
        remaining.append((derived_cashback_quote_ata, False, True))
        remaining.append((user_volume_accumulator_pubkey, False, True))
    if maybe_pool_v2:
        remaining.append((maybe_pool_v2, False, False))
    remaining.append((fee_pair[0], False, False))
    remaining.append((fee_pair[1], False, True))
    return remaining


proposal: dict[str, Any] = {}
entry_reason = "profit_trigger"
first_failure = "none"
wire_send = 0
live_landed = 0
acquisition_tx_signature = ""
acquisition_landed = False
flatten_tx_signature = ""
flatten_landed = False
acquired_amount_raw = 0
inventory_refresh_ok = False
max_loss_breached = False
pm_pending_after = 0
final_inventory_raw = 0
pnl_lamports = 0
final_status = ""
flatten_reason = "not_started"
refresh_log_after_acq = ""
refresh_log_after_flatten = ""
presend_last_snapshot: dict[str, Any] = {}
presend_poll_count = 0
owner_pubkey = ""
pre_native_balance_raw = 0
pre_wsol_balance_raw = 0
pre_token_balance_raw = 0
planned_fee_budget_lamports = 0
daily_guard_snapshot: dict[str, Any] = {}
daily_guard_gate_blocker = ""


def current_utc_day() -> str:
    return time.strftime("%Y-%m-%d", time.gmtime())


daily_ledger_path = Path(os.environ.get("SHADOW_R1_DAILY_LEDGER_PATH", "").strip()) if os.environ.get("SHADOW_R1_DAILY_LEDGER_PATH", "").strip() else None
daily_profit_target_usd = float(os.environ.get("SHADOW_R1_DAILY_PROFIT_TARGET_USD", "60") or "60")
daily_max_loss_usd = float(os.environ.get("SHADOW_R1_DAILY_MAX_LOSS_USD", "25") or "25")
daily_max_trades = max(0, int(float(os.environ.get("SHADOW_R1_DAILY_MAX_TRADES", "12") or "12")))
daily_sol_usd = float(os.environ.get("SHADOW_R1_DAILY_SOL_USD", "0") or "0")


def load_daily_guard(path: Path | None) -> dict[str, Any]:
    base = {
        "date_utc": current_utc_day(),
        "risk_tier": risk_tier,
        "sol_usd_hint": daily_sol_usd,
        "daily_profit_target_usd": daily_profit_target_usd,
        "daily_max_loss_usd": daily_max_loss_usd,
        "daily_max_trades": daily_max_trades,
        "executed_trade_count": 0,
        "landed_trade_count": 0,
        "flat_trade_count": 0,
        "open_exposure_count": 0,
        "max_loss_breach_count": 0,
        "realized_pnl_lamports": 0,
        "realized_pnl_sol": 0.0,
        "realized_pnl_usd_estimate": 0.0,
        "last_status": "none",
        "last_failure": "none",
        "last_trade_pnl_lamports": 0,
        "last_updated_utc": "",
    }
    if path is None or not path.exists():
        return base
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return base
    if text(loaded.get("date_utc")) != current_utc_day():
        return base
    merged = {**base, **loaded}
    merged["risk_tier"] = risk_tier
    merged["sol_usd_hint"] = daily_sol_usd
    merged["daily_profit_target_usd"] = daily_profit_target_usd
    merged["daily_max_loss_usd"] = daily_max_loss_usd
    merged["daily_max_trades"] = daily_max_trades
    return merged


def realized_pnl_usd_estimate(lamports_value: int) -> float:
    if daily_sol_usd <= 0:
        return 0.0
    return lamports_to_sol(lamports_value) * daily_sol_usd


def evaluate_daily_guard(ledger: dict[str, Any]) -> tuple[bool, str]:
    if safe_int(ledger.get("open_exposure_count")) > 0:
        return False, "daily_open_exposure_present"
    if daily_max_trades > 0 and safe_int(ledger.get("executed_trade_count")) >= daily_max_trades:
        return False, "daily_max_trades_reached"
    realized_lamports = safe_int(ledger.get("realized_pnl_lamports"))
    if daily_sol_usd > 0 and daily_profit_target_usd > 0 and realized_pnl_usd_estimate(realized_lamports) >= daily_profit_target_usd:
        return False, "daily_profit_target_reached"
    if daily_sol_usd > 0 and daily_max_loss_usd > 0 and realized_pnl_usd_estimate(realized_lamports) <= -daily_max_loss_usd:
        return False, "daily_max_loss_reached"
    return True, "none"


def persist_daily_guard(path: Path | None, ledger: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    ledger["realized_pnl_sol"] = lamports_to_sol(safe_int(ledger.get("realized_pnl_lamports")))
    ledger["realized_pnl_usd_estimate"] = realized_pnl_usd_estimate(safe_int(ledger.get("realized_pnl_lamports")))
    ledger["last_updated_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    path.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n", encoding="utf-8")


daily_guard_snapshot = load_daily_guard(daily_ledger_path)


def infer_final_status() -> str:
    if final_status:
        return final_status
    if first_failure == "daily_guard:daily_risk_sol_usd_missing":
        return "ABORT_DAILY_RISK_SOL_USD_MISSING"
    if first_failure == "daily_guard:daily_open_exposure_present":
        return "ABORT_DAILY_OPEN_EXPOSURE_PRESENT"
    if first_failure == "daily_guard:daily_max_trades_reached":
        return "ABORT_DAILY_MAX_TRADES_REACHED"
    if first_failure == "daily_guard:daily_profit_target_reached":
        return "ABORT_DAILY_PROFIT_TARGET_REACHED"
    if first_failure == "daily_guard:daily_max_loss_reached":
        return "ABORT_DAILY_MAX_LOSS_REACHED"
    if rescue_flatten_only and acquisition_landed and flatten_landed:
        return "R1_ONE_MINT_RESCUE_FLATTENED"
    if rescue_flatten_only and acquisition_landed and not flatten_landed:
        return "R1_ONE_MINT_RESCUE_FLATTEN_FAILED"
    if acquisition_landed and flatten_landed:
        return "R1_ONE_MINT_EXECUTED_AND_FLATTENED"
    if acquisition_landed and not flatten_landed:
        return "R1_ONE_MINT_ACQUIRED_FLATTEN_FAILED"
    if first_failure.startswith("presend_timeout:exit_quote"):
        return "ABORT_PRE_SEND_EXIT_QUOTE_MISSING_TIMEOUT"
    if first_failure.startswith("presend_timeout:entry_quote"):
        return "ABORT_PRE_SEND_ENTRY_QUOTE_MISSING_TIMEOUT"
    if first_failure.startswith("presend_timeout:auto_flatten"):
        return "ABORT_PRE_SEND_AUTO_FLATTEN_TIMEOUT"
    if first_failure.startswith("presend_timeout:pm_reserve"):
        return "ABORT_PRE_SEND_PM_TIMEOUT"
    if first_failure.startswith("presend_timeout:entry_output_account"):
        return "ABORT_PRE_SEND_ENTRY_OUTPUT_ACCOUNT_TIMEOUT"
    if first_failure.startswith("presend_timeout:mint_policy"):
        return "ABORT_PRE_SEND_POLICY_TIMEOUT"
    if first_failure.startswith("exit_quote_"):
        return "ABORT_PRE_SEND_EXIT_QUOTE_MISSING"
    if first_failure.startswith("entry_quote_"):
        return "ABORT_PRE_SEND_ENTRY_QUOTE_MISSING"
    if first_failure.startswith("mint_policy"):
        return "ABORT_PRE_SEND_POLICY_FAILED"
    if first_failure.startswith("pm_reserve"):
        return "ABORT_PRE_SEND_PM_FAILED"
    if first_failure.startswith("entry_output_account"):
        return "ABORT_PRE_SEND_ENTRY_OUTPUT_ACCOUNT_MISSING"
    if first_failure == "execute_confirm_mismatch":
        return "ABORT_EXECUTE_CONFIRM_MISMATCH"
    if first_failure.startswith("proposal_not_ready"):
        return "ABORT_PROPOSAL_NOT_READY"
    if wire_send == 0:
        return "ABORT_PRE_SEND_GATE_FAILED"
    return "R1_ONE_MINT_ACQUISITION_FAILED"


def write_outputs() -> None:
    status = infer_final_status()
    realized_trade = bool(acquisition_tx_signature)
    realized_flat = bool((not rescue_flatten_only) and acquisition_landed and flatten_landed and final_inventory_raw <= 0)
    open_exposure = bool(acquisition_landed and (not flatten_landed or final_inventory_raw > 0))
    daily_guard_effective = dict(daily_guard_snapshot) if daily_guard_snapshot else load_daily_guard(daily_ledger_path)
    if realized_trade:
        daily_guard_effective["executed_trade_count"] = safe_int(daily_guard_effective.get("executed_trade_count")) + 1
    if acquisition_landed:
        daily_guard_effective["landed_trade_count"] = safe_int(daily_guard_effective.get("landed_trade_count")) + 1
    if realized_flat:
        daily_guard_effective["flat_trade_count"] = safe_int(daily_guard_effective.get("flat_trade_count")) + 1
        daily_guard_effective["realized_pnl_lamports"] = safe_int(daily_guard_effective.get("realized_pnl_lamports")) + pnl_lamports
    if max_loss_breached:
        daily_guard_effective["max_loss_breach_count"] = safe_int(daily_guard_effective.get("max_loss_breach_count")) + 1
    daily_guard_effective["open_exposure_count"] = safe_int(daily_guard_effective.get("open_exposure_count"))
    if rescue_flatten_only and flatten_landed:
        daily_guard_effective["open_exposure_count"] = max(0, daily_guard_effective["open_exposure_count"] - 1)
        daily_guard_effective["realized_pnl_lamports"] = safe_int(daily_guard_effective.get("realized_pnl_lamports")) + pnl_lamports
        daily_guard_effective["flat_trade_count"] = safe_int(daily_guard_effective.get("flat_trade_count")) + 1
    elif open_exposure:
        daily_guard_effective["open_exposure_count"] += 1
    daily_guard_effective["last_status"] = status
    daily_guard_effective["last_failure"] = first_failure
    daily_guard_effective["last_trade_pnl_lamports"] = pnl_lamports if realized_trade else 0
    persist_daily_guard(daily_ledger_path, daily_guard_effective)
    presend_entry_quote = presend_last_snapshot.get("entry_quote") if isinstance(presend_last_snapshot, dict) else {}
    presend_exit_quote = presend_last_snapshot.get("exit_quote") if isinstance(presend_last_snapshot, dict) else {}
    presend_entry_expected_out_raw = safe_int((presend_entry_quote or {}).get("expected_out_raw"))
    presend_entry_min_out_raw = safe_int((presend_entry_quote or {}).get("min_out_raw"))
    presend_exit_quote_input_raw = safe_int((presend_last_snapshot or {}).get("presend_exit_quote_input_raw"))
    presend_exit_quote_input_source = text((presend_last_snapshot or {}).get("presend_exit_quote_input_source"))
    current_inventory_raw = safe_int((presend_last_snapshot or {}).get("current_inventory_raw"))
    exit_quote_expected_out_raw = safe_int((presend_exit_quote or {}).get("expected_out_raw"))
    report = {
        "status": status,
        "risk_tier": risk_tier,
        "target_mint": target_mint,
        "target_pool": target_pool,
        "proposal_json_path": str(proposal_json_path),
        "direct_rows_path": str(direct_rows_path),
        "ls13a_rows_path": str(ls13a_rows_path),
        "alt_rows_path": str(alt_rows_path),
        "sender_url": normalize_sender_endpoint(sender_url, sender_api_key),
        "sender_tip_lamports": tip_lamports,
        "sender_tip_account": sender_tip_account,
        "sender_tip_instruction_present": bool(tip_lamports > 0 and sender_tip_account in HELIUS_ALLOWED_TIP_ACCOUNTS),
        "acquisition_tx_signature": acquisition_tx_signature or None,
        "acquisition_landed": acquisition_landed,
        "acquired_amount_raw": acquired_amount_raw,
        "inventory_refresh_ok": inventory_refresh_ok,
        "flatten_tx_signature": flatten_tx_signature or None,
        "flatten_landed": flatten_landed,
        "final_inventory_raw": final_inventory_raw,
        "pnl_lamports": pnl_lamports,
        "max_loss_breached": max_loss_breached,
        "pm_pending_after": pm_pending_after,
        "wire_send": wire_send,
        "live_landed": live_landed,
        "presend_entry_expected_out_raw": presend_entry_expected_out_raw,
        "presend_entry_min_out_raw": presend_entry_min_out_raw,
        "presend_exit_quote_input_raw": presend_exit_quote_input_raw,
        "presend_exit_quote_input_source": presend_exit_quote_input_source,
        "current_inventory_raw": current_inventory_raw,
        "exit_quote_expected_out_raw": exit_quote_expected_out_raw,
        "first_failure": first_failure,
        "flatten_reason": flatten_reason,
        "presend_poll_count": presend_poll_count,
        "presend_last_snapshot": presend_last_snapshot,
        "daily_guard_path": str(daily_ledger_path) if daily_ledger_path else "",
        "daily_guard_blocker": daily_guard_gate_blocker,
        "daily_guard_state": daily_guard_effective,
        "refresh_log_after_acquisition": refresh_log_after_acq,
        "refresh_log_after_flatten": refresh_log_after_flatten,
        "sendBundle_called": False,
    }
    manifest = {
        "proposal_json_path": str(proposal_json_path),
        "direct_rows_path": str(direct_rows_path),
        "ls13a_rows_path": str(ls13a_rows_path),
        "alt_rows_path": str(alt_rows_path),
        "signer_path": str(signer_path),
        "rpc_url": rpc_url,
        "sender_url": normalize_sender_endpoint(sender_url, sender_api_key),
        "target_mint": target_mint,
        "target_pool": target_pool,
        "risk_tier": risk_tier,
        "max_spend_sol": max_spend_sol,
        "max_loss_sol": max_loss_sol,
        "ttl_seconds": ttl_seconds,
        "stop_loss_bps": stop_loss_bps,
        "presend_wait_seconds": presend_wait_seconds,
        "presend_quote_poll_seconds": presend_quote_poll_seconds,
        "require_direct_pumpfun_exit": require_direct_pumpfun_exit,
        "daily_guard_path": str(daily_ledger_path) if daily_ledger_path else "",
        "daily_profit_target_usd": daily_profit_target_usd,
        "daily_max_loss_usd": daily_max_loss_usd,
        "daily_max_trades": daily_max_trades,
        "daily_sol_usd": daily_sol_usd,
    }
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    report_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary_txt_path.write_text(
        "\n".join(
            [
                f"final_status={status}",
                f"acquisition_tx_signature={acquisition_tx_signature or 'none'}",
                f"acquisition_landed={str(acquisition_landed).lower()}",
                f"acquired_amount_raw={acquired_amount_raw}",
                f"inventory_refresh_ok={str(inventory_refresh_ok).lower()}",
                f"flatten_tx_signature={flatten_tx_signature or 'none'}",
                f"flatten_landed={str(flatten_landed).lower()}",
                f"final_inventory_raw={final_inventory_raw}",
                f"pnl_lamports={pnl_lamports}",
                f"max_loss_breached={str(max_loss_breached).lower()}",
                f"pm_pending_after={pm_pending_after}",
                f"wire_send={wire_send}",
                f"live_landed={live_landed}",
                f"presend_entry_expected_out_raw={presend_entry_expected_out_raw}",
                f"presend_entry_min_out_raw={presend_entry_min_out_raw}",
                f"presend_exit_quote_input_raw={presend_exit_quote_input_raw}",
                f"presend_exit_quote_input_source={presend_exit_quote_input_source or 'unknown'}",
                f"current_inventory_raw={current_inventory_raw}",
                f"exit_quote_expected_out_raw={exit_quote_expected_out_raw}",
                f"daily_guard_blocker={daily_guard_gate_blocker or 'none'}",
                f"sender_tip_lamports={tip_lamports}",
                f"sender_tip_account={sender_tip_account or 'none'}",
                f"sender_tip_instruction_present={str(bool(tip_lamports > 0 and sender_tip_account in HELIUS_ALLOWED_TIP_ACCOUNTS)).lower()}",
                f"daily_executed_trade_count={safe_int(daily_guard_effective.get('executed_trade_count'))}",
                f"daily_realized_pnl_lamports={safe_int(daily_guard_effective.get('realized_pnl_lamports'))}",
                f"daily_realized_pnl_usd_estimate={daily_guard_effective.get('realized_pnl_usd_estimate', 0.0)}",
                f"first_failure={first_failure}",
            ]
        )
        + "\n",
        encoding="utf-8",
    )


atexit.register(write_outputs)


proposal = load_json(proposal_json_path)
if execute_confirm != execute_confirm_required:
    first_failure = "execute_confirm_mismatch"
    raise RuntimeError(first_failure)
if not rescue_flatten_only and text(proposal.get("final_verdict")) != EXPECTED_PROPOSAL_VERDICT:
    first_failure = f"proposal_not_ready:{text(proposal.get('final_verdict'))}"
    raise RuntimeError(first_failure)
if text(proposal.get("mint")) != target_mint or text(proposal.get("pool")) != target_pool:
    first_failure = "proposal_target_mismatch"
    raise RuntimeError(first_failure)
if daily_sol_usd <= 0:
    daily_guard_gate_blocker = "daily_risk_sol_usd_missing"
    first_failure = f"daily_guard:{daily_guard_gate_blocker}"
    raise RuntimeError(first_failure)
if not rescue_flatten_only:
    daily_guard_allowed, daily_guard_gate_blocker = evaluate_daily_guard(daily_guard_snapshot)
    if not daily_guard_allowed:
        first_failure = f"daily_guard:{daily_guard_gate_blocker}"
        raise RuntimeError(first_failure)

if not sender_url:
    first_failure = "helius_sender_url_missing"
    raise RuntimeError(first_failure)
if not utils_path.exists():
    first_failure = "utils_missing"
    raise RuntimeError(first_failure)

owner_keypair = Keypair.from_json(signer_path.read_text(encoding="utf-8"))
owner_pubkey = str(owner_keypair.pubkey())

max_spend_lamports = sol_to_lamports(max_spend_sol)
max_loss_lamports = sol_to_lamports(max_loss_sol)

direct_rows = [
    row
    for row in load_jsonl(direct_rows_path)
    if text(row.get("pool")) == target_pool
    and target_mint in {text(row.get("our_input_mint")), text(row.get("our_output_mint")), text(row.get("candidate_mint"))}
]
if not direct_rows:
    raise RuntimeError("direct_route_row_missing")
direct_row = direct_rows[0]
target_signature = text(direct_row.get("target_signature"))

ls13a_row = next((row for row in load_jsonl(ls13a_rows_path) if text(row.get("target_signature")) == target_signature), {})
alt_row = next((row for row in load_jsonl(alt_rows_path) if text(row.get("target_signature")) == target_signature), {})
target_tx_base64 = text(ls13a_row.get("target_tx_base64"))
if not target_tx_base64:
    raise RuntimeError("target_tx_missing")

parsed, parse_error = utils.parse_wire_tx(target_tx_base64)
if parsed is None:
    raise RuntimeError(f"target_tx_parse_failed:{parse_error}")
resolved_account_keys = list(alt_row.get("resolved_account_keys") or []) or list(parsed.get("static_account_keys") or [])
target_ix = None
for ix in utils.instruction_views(parsed, resolved_account_keys):
    if text(ix.get("program_id")) != PUMPFUN_PROGRAM_ID:
        continue
    if safe_int(ix.get("instruction_index")) != safe_int(direct_row.get("target_instruction_index")):
        continue
    target_ix = ix
    break
if target_ix is None:
    raise RuntimeError("target_instruction_missing")
target_accounts = [text(item) for item in (target_ix.get("accounts") or [])]
target_data = target_ix.get("data") or b""
target_route_mode = ""
if target_data[:8] == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
    target_route_mode = "buy_exact_quote_in"
elif target_data[:8] == SELL_DISCRIMINATOR:
    target_route_mode = "sell"
if len(target_accounts) < 9:
    raise RuntimeError("target_accounts_missing")
permission_by_index = permission_map(parsed, resolved_account_keys, alt_row)

policy_meta = mint_metadata(target_mint)
mint_policy_pass, mint_policy_blocker = policy_for_mint(policy_meta)
if not mint_policy_pass:
    first_failure = f"mint_policy_failed:{mint_policy_blocker}"
    raise RuntimeError(first_failure)

pre_native_balance_raw, _ = get_balance(owner_pubkey)
pre_wsol_entry = richest_account(owner_pubkey, WSOL_MINT)
pre_wsol_balance_raw = safe_int(pre_wsol_entry.get("amount_raw"))
pre_token_entry = richest_account(owner_pubkey, target_mint)
pre_token_balance_raw = safe_int(pre_token_entry.get("amount_raw"))
if rescue_flatten_only and pre_token_balance_raw <= 0:
    first_failure = "rescue_inventory_missing"
    raise RuntimeError(first_failure)


def current_pool_quote_buy(spend_lamports: int) -> dict[str, Any]:
    accounts, slot, error = get_multiple_accounts([target_pool])
    if error:
        return {"ok": False, "error": error}
    pool_account = accounts.get(target_pool, {})
    pool_doc, pool_error = decode_pumpfun_pool(pool_account.get("data_raw", b""))
    if pool_error or not pool_doc:
        return {"ok": False, "error": pool_error or "pool_decode_failed"}
    if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {target_mint, WSOL_MINT}:
        return {"ok": False, "error": "pool_mint_set_mismatch"}
    vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
    if vault_error:
        return {"ok": False, "error": vault_error}
    current_base_reserve_raw = decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b""))
    current_quote_reserve_raw = decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b""))
    if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
        reserve_in = current_base_reserve_raw
        reserve_out = current_quote_reserve_raw
    else:
        reserve_in = current_quote_reserve_raw
        reserve_out = current_base_reserve_raw
    expected_out = constant_product_out(reserve_in, reserve_out, spend_lamports)
    min_out = min_out_from_expected(expected_out, slippage_bps)
    quote_error = ""
    if expected_out <= 0:
        quote_error = "entry_quote_expected_out_zero"
    elif min_out <= 0:
        quote_error = "entry_quote_min_out_zero"
    return {
        "ok": expected_out > 0 and min_out > 0,
        "slot": slot,
        "pool_doc": pool_doc,
        "expected_out_raw": expected_out,
        "min_out_raw": min_out,
        "current_base_reserve_raw": current_base_reserve_raw,
        "current_quote_reserve_raw": current_quote_reserve_raw,
        "error": quote_error,
        "quote_source_used": "pumpfun_direct_pool_math",
        "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
        "direct_pool_math_quote_available": expected_out > 0,
        "route_source_mismatch": False,
    }


def current_pool_quote_sell(amount_in_raw: int) -> dict[str, Any]:
    accounts, slot, error = get_multiple_accounts([target_pool])
    if error:
        return {"ok": False, "error": error}
    pool_account = accounts.get(target_pool, {})
    pool_doc, pool_error = decode_pumpfun_pool(pool_account.get("data_raw", b""))
    if pool_error or not pool_doc:
        return {"ok": False, "error": pool_error or "pool_decode_failed"}
    if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {target_mint, WSOL_MINT}:
        return {"ok": False, "error": "pool_mint_set_mismatch"}
    vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
    if vault_error:
        return {"ok": False, "error": vault_error}
    current_base_reserve_raw = decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b""))
    current_quote_reserve_raw = decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b""))
    if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
        reserve_in = current_quote_reserve_raw
        reserve_out = current_base_reserve_raw
    else:
        reserve_in = current_base_reserve_raw
        reserve_out = current_quote_reserve_raw
    expected_out = constant_product_out(reserve_in, reserve_out, amount_in_raw)
    min_out = min_out_from_expected(expected_out, slippage_bps)
    quote_error = ""
    if expected_out <= 0:
        quote_error = "exit_quote_expected_out_zero"
    elif min_out <= 0:
        quote_error = "exit_quote_min_out_zero"
    return {
        "ok": expected_out > 0 and min_out > 0,
        "slot": slot,
        "pool_doc": pool_doc,
        "expected_out_raw": expected_out,
        "min_out_raw": min_out,
        "current_base_reserve_raw": current_base_reserve_raw,
        "current_quote_reserve_raw": current_quote_reserve_raw,
        "error": quote_error,
        "quote_source_used": "pumpfun_direct_pool_math",
        "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
        "direct_pool_math_quote_available": expected_out > 0,
        "route_source_mismatch": False,
    }


def build_direct_tx(input_mint: str, output_mint: str, amount_in_raw: int, min_out_raw: int) -> tuple[bytes, dict[str, Any]]:
    input_meta = mint_metadata(input_mint)
    output_meta = mint_metadata(output_mint)
    input_policy_pass, input_policy_blocker = policy_for_mint(input_meta) if input_mint != WSOL_MINT else (True, "")
    output_policy_pass, output_policy_blocker = policy_for_mint(output_meta) if output_mint != WSOL_MINT else (True, "")
    if not input_policy_pass:
        raise RuntimeError(f"input_policy_failed:{input_policy_blocker}")
    if not output_policy_pass:
        raise RuntimeError(f"output_policy_failed:{output_policy_blocker}")
    input_token_program = text(input_meta.get("token_program")) or (TOKEN_PROGRAM if input_mint == WSOL_MINT else "")
    output_token_program = text(output_meta.get("token_program")) or (TOKEN_PROGRAM if output_mint == WSOL_MINT else "")
    input_account_doc = first_or_derived_account(owner_pubkey, input_mint, input_token_program)
    output_account_doc = first_or_derived_account(owner_pubkey, output_mint, output_token_program)
    if input_mint == target_accounts[3] and output_mint == target_accounts[4]:
        route_mode = "sell"
    elif input_mint == target_accounts[4] and output_mint == target_accounts[3]:
        route_mode = "buy_exact_quote_in"
    else:
        raise RuntimeError("route_direction_not_mappable")
    direct_ix_data = encode_direct_ix_data(route_mode, amount_in_raw, min_out_raw)
    if not direct_ix_data:
        raise RuntimeError("direct_ix_encode_failed")
    replacements = {1: owner_pubkey}
    if route_mode == "sell":
        replacements[5] = text(input_account_doc.get("selected_pubkey"))
        replacements[6] = text(output_account_doc.get("selected_pubkey"))
    else:
        replacements[5] = text(output_account_doc.get("selected_pubkey"))
        replacements[6] = text(input_account_doc.get("selected_pubkey"))
    if not replacements.get(5) or not replacements.get(6):
        raise RuntimeError("selected_user_token_account_missing")
    global_volume_accumulator = str(
        Pubkey.find_program_address(
            [GLOBAL_VOLUME_ACCUMULATOR_SEED],
            Pubkey.from_string(PUMPFUN_PROGRAM_ID),
        )[0]
    )
    user_volume_accumulator = str(
        Pubkey.find_program_address(
            [USER_VOLUME_ACCUMULATOR_SEED, bytes(Pubkey.from_string(owner_pubkey))],
            Pubkey.from_string(PUMPFUN_PROGRAM_ID),
        )[0]
    )
    if route_mode == "buy_exact_quote_in":
        fee_config = text(target_accounts[21] if target_route_mode == "buy_exact_quote_in" and len(target_accounts) >= 23 else (target_accounts[19] if len(target_accounts) >= 20 else ""))
        fee_program = text(target_accounts[22] if target_route_mode == "buy_exact_quote_in" and len(target_accounts) >= 23 else (target_accounts[20] if len(target_accounts) >= 21 else ""))
        target_accounts_for_route = list(target_accounts[:19]) + [
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
            fee_program,
        ]
        explicit_meta_flags = BUY_EXACT_QUOTE_IN_META_FLAGS
    else:
        fee_config = text(target_accounts[21] if target_route_mode == "buy_exact_quote_in" and len(target_accounts) >= 23 else (target_accounts[19] if len(target_accounts) >= 20 else ""))
        fee_program = text(target_accounts[22] if target_route_mode == "buy_exact_quote_in" and len(target_accounts) >= 23 else (target_accounts[20] if len(target_accounts) >= 21 else ""))
        target_accounts_for_route = list(target_accounts[:19]) + [fee_config, fee_program]
        explicit_meta_flags = SELL_META_FLAGS
    if len(target_accounts_for_route) != len(explicit_meta_flags):
        raise RuntimeError("pumpfun_route_account_shape_mismatch")
    route_remaining_accounts = build_route_remaining_accounts(
        route_mode,
        text(target_accounts[4]),
        text(target_accounts[12]),
        user_volume_accumulator,
    )
    metas = []
    for pos in range(len(target_accounts_for_route)):
        if pos >= len(target_accounts_for_route):
            raise RuntimeError("target_account_position_out_of_bounds")
        pubkey_str = text(replacements.get(pos, target_accounts_for_route[pos]))
        is_signer, is_writable = explicit_meta_flags.get(pos, (False, False))
        metas.append(AccountMeta(Pubkey.from_string(pubkey_str), is_signer, is_writable))
    for pubkey_str, is_signer, is_writable in route_remaining_accounts:
        if not text(pubkey_str):
            continue
        metas.append(AccountMeta(Pubkey.from_string(pubkey_str), is_signer, is_writable))
    instruction = Instruction(Pubkey.from_string(PUMPFUN_PROGRAM_ID), direct_ix_data, metas)
    instructions = [instruction]
    sender_tip_instruction_present = False
    if tip_lamports > 0:
        if sender_tip_account not in HELIUS_ALLOWED_TIP_ACCOUNTS:
            raise RuntimeError("sender_tip_account_not_allowed")
        tip_ix = transfer(
            TransferParams(
                from_pubkey=owner_keypair.pubkey(),
                to_pubkey=Pubkey.from_string(sender_tip_account),
                lamports=tip_lamports,
            )
        )
        instructions.append(tip_ix)
        sender_tip_instruction_present = True
    tx = Transaction([owner_keypair], Message(instructions, owner_keypair.pubkey()), Hash.from_string(latest_blockhash()))
    meta = {
        "route_mode": route_mode,
        "input_account_doc": input_account_doc,
        "output_account_doc": output_account_doc,
        "instruction_len": len(direct_ix_data),
        "remaining_accounts_count": len(route_remaining_accounts),
        "remaining_accounts": [item[0] for item in route_remaining_accounts],
        "sender_tip_lamports": tip_lamports,
        "sender_tip_account": sender_tip_account,
        "sender_tip_instruction_present": sender_tip_instruction_present,
    }
    return bytes(tx), meta


def simulate_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    response, error = rpc(
        "simulateTransaction",
        [encoded, {"encoding": "base64", "commitment": "confirmed", "sigVerify": True}],
    )
    return (response or {}), error


def simulation_ok(sim_response: dict[str, Any], sim_error: dict[str, Any] | None) -> tuple[bool, str]:
    if sim_error is not None:
        return False, text(sim_error.get("error") or sim_error)
    result = (sim_response or {}).get("result") or {}
    value = result.get("value") if isinstance(result, dict) else {}
    if not isinstance(value, dict):
        return False, "simulation_value_missing"
    if value.get("err") is not None:
        return False, json.dumps(value.get("err"), sort_keys=True)
    return True, ""


def confirm_signature(signature: str, attempts: int = 30, delay_seconds: float = 2.0) -> tuple[bool, dict[str, Any] | None]:
    last_status = None
    for attempt in range(attempts):
        response, error = rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        if error is not None:
            last_status = {"error": error}
        else:
            values = (((response or {}).get("result") or {}).get("value")) or []
            if values and values[0] is not None:
                last_status = values[0]
                if isinstance(values[0], dict) and values[0].get("err") is None and text(values[0].get("confirmationStatus")) in {"confirmed", "finalized"}:
                    return True, values[0]
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return False, last_status


planned_fee_budget_lamports = (2 * base_tx_fee_lamports) + (2 * priority_fee_lamports) + (2 * tip_lamports)


def presend_snapshot() -> dict[str, Any]:
    snapshot: dict[str, Any] = {
        "quote_source_used": "pumpfun_direct_pool_math",
        "pumpfun_direct_exit_quote_called": True,
        "jupiter_quote_called": False,
        "pool": target_pool,
        "mint": target_mint,
        "amount_in_raw": 0,
        "entry_quote_ok": False,
        "exit_quote_ok": False,
        "exit_quote_error": "",
        "exit_quote_dust": False,
        "pool_reserves_available": False,
        "direct_pool_math_quote_available": False,
        "route_source_mismatch": False,
        "auto_flatten_available": False,
        "pm_reserve_inventory_ok": False,
        "pm_reserve_fee_ok": False,
        "entry_output_account_exists": False,
        "mint_policy_pass": False,
        "first_blocker": "",
        "ready": False,
    }
    target_meta = mint_metadata(target_mint)
    current_policy_pass, current_policy_blocker = policy_for_mint(target_meta)
    snapshot["mint_policy_pass"] = current_policy_pass
    if not current_policy_pass:
        snapshot["first_blocker"] = f"mint_policy_changed:{current_policy_blocker}"
        return snapshot
    entry_quote_local = current_pool_quote_buy(max_spend_lamports)
    snapshot["entry_quote"] = entry_quote_local
    snapshot["entry_quote_ok"] = boolish(entry_quote_local.get("ok"))
    snapshot["pool_reserves_available"] = boolish(entry_quote_local.get("pool_reserves_available"))
    snapshot["direct_pool_math_quote_available"] = boolish(entry_quote_local.get("direct_pool_math_quote_available"))
    if not snapshot["entry_quote_ok"]:
        snapshot["first_blocker"] = f"entry_quote_missing:{text(entry_quote_local.get('error'))}"
        return snapshot
    expected_entry_out_raw_local = safe_int(entry_quote_local.get("expected_out_raw"))
    entry_min_out_raw_local = safe_int(entry_quote_local.get("min_out_raw"))
    snapshot["amount_in_raw"] = expected_entry_out_raw_local
    snapshot["presend_entry_expected_out_raw"] = expected_entry_out_raw_local
    snapshot["presend_entry_min_out_raw"] = entry_min_out_raw_local
    if expected_entry_out_raw_local <= 0 or entry_min_out_raw_local <= 0:
        snapshot["first_blocker"] = "entry_quote_non_positive"
        return snapshot
    exit_quote_input_raw_local = entry_min_out_raw_local
    snapshot["presend_exit_quote_input_raw"] = exit_quote_input_raw_local
    snapshot["presend_exit_quote_input_source"] = "entry_min_out_raw"
    exit_quote_local = current_pool_quote_sell(exit_quote_input_raw_local)
    snapshot["exit_quote"] = exit_quote_local
    snapshot["exit_quote_ok"] = boolish(exit_quote_local.get("ok"))
    snapshot["exit_quote_error"] = text(exit_quote_local.get("error"))
    snapshot["exit_quote_dust"] = safe_int(exit_quote_local.get("expected_out_raw")) <= 1 or safe_int(exit_quote_local.get("min_out_raw")) <= 0
    snapshot["pool_reserves_available"] = snapshot["pool_reserves_available"] and boolish(exit_quote_local.get("pool_reserves_available"))
    snapshot["direct_pool_math_quote_available"] = snapshot["direct_pool_math_quote_available"] and boolish(exit_quote_local.get("direct_pool_math_quote_available"))
    if require_direct_pumpfun_exit and not snapshot["exit_quote_ok"]:
        snapshot["first_blocker"] = f"exit_quote_missing:{snapshot['exit_quote_error']}"
        return snapshot
    if require_direct_pumpfun_exit and snapshot["exit_quote_dust"]:
        snapshot["first_blocker"] = "exit_quote_dust"
        return snapshot
    wsol_entry_local = richest_account(owner_pubkey, WSOL_MINT)
    wsol_balance_raw_local = safe_int(wsol_entry_local.get("amount_raw"))
    token_entry_local = richest_account(owner_pubkey, target_mint)
    snapshot["current_inventory_raw"] = safe_int(token_entry_local.get("amount_raw"))
    token_account_pubkey_local = text(token_entry_local.get("pubkey"))
    owner_native_balance_raw_local, _ = get_balance(owner_pubkey)
    snapshot["entry_output_account_exists"] = bool(token_account_pubkey_local)
    snapshot["pm_reserve_inventory_ok"] = wsol_balance_raw_local >= max_spend_lamports
    snapshot["pm_reserve_fee_ok"] = owner_native_balance_raw_local >= planned_fee_budget_lamports
    if not snapshot["entry_output_account_exists"]:
        snapshot["first_blocker"] = "entry_output_account_missing"
        return snapshot
    if not snapshot["pm_reserve_inventory_ok"]:
        snapshot["first_blocker"] = "pm_reserve_inventory_failed"
        return snapshot
    if not snapshot["pm_reserve_fee_ok"]:
        snapshot["first_blocker"] = "pm_reserve_fee_failed"
        return snapshot
    if require_direct_pumpfun_exit:
        try:
            build_direct_tx(target_mint, WSOL_MINT, expected_entry_out_raw_local, safe_int(exit_quote_local.get("min_out_raw")))
            snapshot["auto_flatten_available"] = True
        except Exception as exc:  # noqa: BLE001
            snapshot["first_blocker"] = f"auto_flatten_unavailable:{text(exc)}"
            return snapshot
    else:
        snapshot["auto_flatten_available"] = True
    snapshot["ready"] = True
    return snapshot


deadline = time.time() + presend_wait_seconds
entry_quote = {}
pre_exit_quote = {}
entry_min_out_raw = 0
expected_entry_out_raw = 0
if rescue_flatten_only:
    acquisition_landed = True
    acquired_amount_raw = pre_token_balance_raw
    inventory_refresh_ok, refresh_log_after_acq = refresh_inventory()
    if not inventory_refresh_ok:
        first_failure = "inventory_refresh_failed_before_rescue"
    flatten_reason = "rescue"
    current_exit_quote = {}
else:
    while True:
        presend_poll_count += 1
        presend_last_snapshot = presend_snapshot()
        if boolish(presend_last_snapshot.get("ready")):
            entry_quote = presend_last_snapshot.get("entry_quote") or {}
            pre_exit_quote = presend_last_snapshot.get("exit_quote") or {}
            expected_entry_out_raw = safe_int(entry_quote.get("expected_out_raw"))
            entry_min_out_raw = safe_int(entry_quote.get("min_out_raw"))
            break
        if time.time() >= deadline:
            first_failure = f"presend_timeout:{text(presend_last_snapshot.get('first_blocker')) or 'gate_not_green'}"
            raise RuntimeError(first_failure)
        time.sleep(presend_quote_poll_seconds)

    wsol_entry = richest_account(owner_pubkey, WSOL_MINT)
    wsol_balance_raw = safe_int(wsol_entry.get("amount_raw"))
    wsol_account_pubkey = text(wsol_entry.get("pubkey"))
    token_entry = richest_account(owner_pubkey, target_mint)
    token_account_pubkey = text(token_entry.get("pubkey"))
    owner_native_balance_raw, _ = get_balance(owner_pubkey)
    entry_output_account_exists = bool(token_account_pubkey)

    acq_tx_bytes, acq_meta = build_direct_tx(WSOL_MINT, target_mint, max_spend_lamports, entry_min_out_raw)
    acq_sim_response, acq_sim_error = simulate_raw_tx(acq_tx_bytes)
    acq_sim_ok, acq_sim_blocker = simulation_ok(acq_sim_response, acq_sim_error)
    if not acq_sim_ok:
        raise RuntimeError(f"acquisition_sim_failed:{acq_sim_blocker}")

    sender_response, sender_error = sender_send_tx(acq_tx_bytes)
    wire_send += 1
    if sender_error is not None:
        first_failure = f"acquisition_send_transport:{text(sender_error.get('error') or sender_error)}"
        raise RuntimeError(first_failure)
    if sender_response.get("error") is not None:
        first_failure = f"acquisition_send_rejected:{sender_response.get('error')}"
        raise RuntimeError(first_failure)
    acquisition_tx_signature = text(sender_response.get("result"))
    if not acquisition_tx_signature:
        first_failure = "acquisition_signature_missing"
        raise RuntimeError(first_failure)

    acquisition_landed, acq_status = confirm_signature(acquisition_tx_signature)
    post_acq_token_entry = richest_account(owner_pubkey, target_mint)
    post_acq_token_balance_raw = safe_int(post_acq_token_entry.get("amount_raw"))
    if not acquisition_landed and post_acq_token_balance_raw <= pre_token_balance_raw:
        first_failure = "acquisition_not_confirmed"
        raise RuntimeError(first_failure)
    if acquisition_landed:
        live_landed += 1
    acquired_amount_raw = max(0, post_acq_token_balance_raw - pre_token_balance_raw)
    if acquired_amount_raw <= 0:
        acquired_amount_raw = post_acq_token_balance_raw
    if acquired_amount_raw <= 0:
        first_failure = "acquired_amount_zero"
        raise RuntimeError(first_failure)

    inventory_refresh_ok, refresh_log_after_acq = refresh_inventory()
    if not inventory_refresh_ok:
        first_failure = "inventory_refresh_failed_after_acquisition"

    start_ts = time.time()
    flatten_reason = "ttl"
    current_exit_quote = {}
    while True:
        current_token_entry = richest_account(owner_pubkey, target_mint)
        current_token_balance_raw = safe_int(current_token_entry.get("amount_raw"))
        if current_token_balance_raw <= 0:
            flatten_reason = "inventory_already_zero"
            break
        current_exit_quote = current_pool_quote_sell(current_token_balance_raw)
        if not current_exit_quote.get("ok"):
            if time.time() - start_ts >= ttl_seconds:
                flatten_reason = "ttl"
                break
            time.sleep(monitor_poll_seconds)
            continue
        projected_exit_out_raw = safe_int(current_exit_quote.get("expected_out_raw"))
        quote_net_after_costs = projected_exit_out_raw - max_spend_lamports - planned_fee_budget_lamports
        if quote_net_after_costs >= 0:
            flatten_reason = "profit_trigger"
            break
        if quote_net_after_costs <= -max_loss_lamports:
            flatten_reason = "stop_loss"
            max_loss_breached = True
            break
        if time.time() - start_ts >= ttl_seconds:
            flatten_reason = "ttl"
            break
        time.sleep(monitor_poll_seconds)

if flatten_reason != "inventory_already_zero":
    flatten_attempt = 0
    flatten_confirm_status = None
    while True:
        current_token_entry = richest_account(owner_pubkey, target_mint)
        current_token_balance_raw = safe_int(current_token_entry.get("amount_raw"))
        if current_token_balance_raw <= 0:
            flatten_landed = True
            break
        exit_quote = current_pool_quote_sell(current_token_balance_raw)
        if not exit_quote.get("ok"):
            first_failure = f"flatten_quote_missing:{exit_quote.get('error')}"
            break
        exit_min_out_raw = safe_int(exit_quote.get("min_out_raw"))
        flatten_tx_bytes, flatten_meta = build_direct_tx(target_mint, WSOL_MINT, current_token_balance_raw, exit_min_out_raw)
        flatten_sim_response, flatten_sim_error = simulate_raw_tx(flatten_tx_bytes)
        flatten_sim_ok, flatten_sim_blocker = simulation_ok(flatten_sim_response, flatten_sim_error)
        if not flatten_sim_ok:
            first_failure = f"flatten_sim_failed:{flatten_sim_blocker}"
            break
        flatten_response, flatten_error = sender_send_tx(flatten_tx_bytes)
        wire_send += 1
        if flatten_error is not None:
            if flatten_attempt < flatten_retry_limit:
                flatten_attempt += 1
                time.sleep(1.0)
                continue
            first_failure = f"flatten_send_transport:{text(flatten_error.get('error') or flatten_error)}"
            break
        if flatten_response.get("error") is not None:
            if flatten_attempt < flatten_retry_limit:
                flatten_attempt += 1
                time.sleep(1.0)
                continue
            first_failure = f"flatten_send_rejected:{flatten_response.get('error')}"
            break
        flatten_tx_signature = text(flatten_response.get("result"))
        if not flatten_tx_signature:
            if flatten_attempt < flatten_retry_limit:
                flatten_attempt += 1
                time.sleep(1.0)
                continue
            first_failure = "flatten_signature_missing"
            break
        flatten_landed, flatten_confirm_status = confirm_signature(flatten_tx_signature)
        post_flatten_token_entry = richest_account(owner_pubkey, target_mint)
        post_flatten_token_balance_raw = safe_int(post_flatten_token_entry.get("amount_raw"))
        if flatten_landed or post_flatten_token_balance_raw <= 0:
            if flatten_landed:
                live_landed += 1
            else:
                flatten_landed = True
            break
        first_failure = "flatten_not_confirmed"
        break

post_native_balance_raw, _ = get_balance(owner_pubkey)
post_wsol_entry = richest_account(owner_pubkey, WSOL_MINT)
post_wsol_balance_raw = safe_int(post_wsol_entry.get("amount_raw"))
post_token_entry = richest_account(owner_pubkey, target_mint)
final_inventory_raw = safe_int(post_token_entry.get("amount_raw"))
inventory_refresh_ok_final, refresh_log_after_flatten = refresh_inventory()
inventory_refresh_ok = inventory_refresh_ok and inventory_refresh_ok_final
pnl_lamports = (post_native_balance_raw - pre_native_balance_raw) + (post_wsol_balance_raw - pre_wsol_balance_raw)
if pnl_lamports < -max_loss_lamports:
    max_loss_breached = True
pm_pending_after = 0 if flatten_landed or not acquisition_landed else 1

if acquisition_landed and flatten_landed:
    final_status = "R1_ONE_MINT_EXECUTED_AND_FLATTENED"
elif acquisition_landed and not flatten_landed:
    final_status = "R1_ONE_MINT_ACQUIRED_FLATTEN_FAILED"
else:
    final_status = "R1_ONE_MINT_ACQUISITION_FAILED"
PY

pass "RunLSPREBRINV R1 live execute written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

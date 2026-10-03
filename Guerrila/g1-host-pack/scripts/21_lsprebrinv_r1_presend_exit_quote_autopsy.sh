#!/usr/bin/env bash
# 21_lsprebrinv_r1_presend_exit_quote_autopsy.sh — direct Pump.fun pre-send quote autopsy

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1Q — Presend Exit Quote Autopsy"

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
STEP="${SHADOW_LSPREBRINV_R1Q_STEP:-21_lsprebrinv_r1_presend_exit_quote_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
REPORT_JSON="$RESULT_DIR/runlsprebrinv_r1_presend_exit_quote_autopsy.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

TARGET_MINT="${SHADOW_R1_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_R1_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"
MAX_SPEND_SOL="${SHADOW_R1_MAX_SPEND_SOL:-0.005}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_R1Q_SLIPPAGE_BPS:-100}"
RPC_URL="${SHADOW_LSPREBRINV_R1Q_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

mkdir -p "$RESULT_DIR"
rm -f "$REPORT_JSON" "$SUMMARY_TXT"

python3 - "$REPORT_JSON" "$SUMMARY_TXT" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$MAX_SPEND_SOL" "$SLIPPAGE_BPS" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.request
from pathlib import Path
from typing import Any

report_json_path = Path(sys.argv[1])
summary_txt_path = Path(sys.argv[2])
rpc_url = str(sys.argv[3])
target_mint = str(sys.argv[4])
target_pool = str(sys.argv[5])
max_spend_sol = float(sys.argv[6])
slippage_bps = max(0, int(float(sys.argv[7])))

WSOL_MINT = "So11111111111111111111111111111111111111112"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def b58encode(raw: bytes) -> str:
    zeros = 0
    for byte in raw:
        if byte == 0:
            zeros += 1
        else:
            break
    num = int.from_bytes(raw, "big")
    chars: list[str] = []
    while num > 0:
        num, rem = divmod(num, 58)
        chars.append(ALPHABET[rem])
    return ("1" * zeros) + "".join(reversed(chars or ["1"]))


def http_post_json(url: str, payload: dict[str, Any]):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        if url.startswith("https://"):
            with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                return json.loads(response.read().decode("utf-8")), None
        with urllib.request.urlopen(request, timeout=20) as response:
            return json.loads(response.read().decode("utf-8")), None
    except Exception as exc:  # noqa: BLE001
        return None, {"error": str(exc)}


def rpc(method: str, params: list[Any] | None = None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    response, error = http_post_json(rpc_url, payload)
    if error is not None:
        return None, error
    if isinstance(response, dict) and response.get("error") is not None:
        return None, response["error"]
    return response, None


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int | None, str]:
    response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return {}, None, text(error.get("message") if isinstance(error, dict) else error)
    result = (response or {}).get("result") or {}
    values = result.get("value") or []
    slot = safe_int((result.get("context") or {}).get("slot"))
    accounts: dict[str, dict[str, Any]] = {}
    for pubkey, value in zip(pubkeys, values):
        data_raw = b""
        if isinstance(value, dict):
            data = value.get("data")
            if isinstance(data, list) and data:
                data_raw = base64.b64decode(text(data[0]).encode("ascii"), validate=False)
        accounts[pubkey] = {"data_raw": data_raw}
    return accounts, slot, ""


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str]:
    if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pumpfun_pool_account_too_short"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pumpfun_pool_discriminator_mismatch"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, ""


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if reserve_in <= 0 or reserve_out <= 0 or amount_in <= 0:
        return 0
    numerator = reserve_out * amount_in
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return numerator // denominator


def min_out_from_expected(expected_out: int, slippage: int) -> int:
    if expected_out <= 0:
        return 0
    bounded_slippage = min(max(int(slippage), 0), 10_000)
    return max(0, expected_out * (10_000 - bounded_slippage) // 10_000)


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
    error_text = ""
    if expected_out <= 0:
        error_text = "entry_quote_expected_out_zero"
    elif min_out <= 0:
        error_text = "entry_quote_min_out_zero"
    return {
        "ok": expected_out > 0 and min_out > 0,
        "slot": slot,
        "expected_out_raw": expected_out,
        "min_out_raw": min_out,
        "error": error_text,
        "current_base_reserve_raw": current_base_reserve_raw,
        "current_quote_reserve_raw": current_quote_reserve_raw,
        "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
        "direct_pool_math_quote_available": expected_out > 0,
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
    error_text = ""
    if expected_out <= 0:
        error_text = "exit_quote_expected_out_zero"
    elif min_out <= 0:
        error_text = "exit_quote_min_out_zero"
    return {
        "ok": expected_out > 0 and min_out > 0,
        "slot": slot,
        "expected_out_raw": expected_out,
        "min_out_raw": min_out,
        "error": error_text,
        "current_base_reserve_raw": current_base_reserve_raw,
        "current_quote_reserve_raw": current_quote_reserve_raw,
        "pool_reserves_available": current_base_reserve_raw > 0 and current_quote_reserve_raw > 0,
        "direct_pool_math_quote_available": expected_out > 0,
    }


max_spend_lamports = sol_to_lamports(max_spend_sol)
entry_quote = current_pool_quote_buy(max_spend_lamports)
expected_entry_out_raw = safe_int(entry_quote.get("expected_out_raw"))
exit_quote = current_pool_quote_sell(expected_entry_out_raw) if entry_quote.get("ok") else {}

report = {
    "quote_source_used": "pumpfun_direct_pool_math",
    "pumpfun_direct_exit_quote_called": True,
    "jupiter_quote_called": False,
    "pool": target_pool,
    "mint": target_mint,
    "amount_in_raw": expected_entry_out_raw,
    "entry_quote_ok": bool(entry_quote.get("ok")),
    "exit_quote_ok": bool(exit_quote.get("ok")),
    "exit_quote_error": text(exit_quote.get("error")),
    "exit_quote_dust": safe_int(exit_quote.get("expected_out_raw")) <= 1 or safe_int(exit_quote.get("min_out_raw")) <= 0,
    "pool_reserves_available": bool(entry_quote.get("pool_reserves_available")) and bool(exit_quote.get("pool_reserves_available", True)),
    "direct_pool_math_quote_available": bool(entry_quote.get("direct_pool_math_quote_available")) and bool(exit_quote.get("direct_pool_math_quote_available", True)),
    "route_source_mismatch": False,
}

report_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={str(value).lower() if isinstance(value, bool) else value}"
        for key, value in report.items()
    ) + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 presend exit quote autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

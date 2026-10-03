#!/usr/bin/env bash
# 21_lsprebrinv_r1_exit_quote_autopsy.sh — explain zero/nonzero direct Pump.fun exit at R1 size

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1A — Exit Quote Autopsy"

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
STEP="${SHADOW_LSPREBRINV_R1A_STEP:-21_lsprebrinv_r1_exit_quote_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
REPORT_JSON="$RESULT_DIR/runlsprebrinv_r1_exit_quote_autopsy.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

TARGET_MINT="${SHADOW_R1_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_R1_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"
MAX_SPEND_SOL="${SHADOW_R1_MAX_SPEND_SOL:-0.005}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_R1A_SLIPPAGE_BPS:-100}"
RPC_URL="${SHADOW_LSPREBRINV_R1A_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

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
MINT_DECIMALS_OFFSET = 44
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


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def to_native(raw: int, decimals: int) -> float:
    if decimals < 0:
        decimals = 0
    return safe_int(raw) / float(10 ** decimals)


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


def decode_mint_decimals(raw: bytes) -> int:
    if len(raw) <= MINT_DECIMALS_OFFSET:
        return 0
    return int(raw[MINT_DECIMALS_OFFSET])


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


accounts, _, error = get_multiple_accounts([target_pool, target_mint])
if error:
    raise RuntimeError(error)
pool_account = accounts.get(target_pool, {})
mint_account = accounts.get(target_mint, {})
pool_doc, pool_error = decode_pumpfun_pool(pool_account.get("data_raw", b""))
if pool_error or not pool_doc:
    raise RuntimeError(pool_error or "pool_decode_failed")
if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {target_mint, WSOL_MINT}:
    raise RuntimeError("pool_mint_set_mismatch")

vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
if vault_error:
    raise RuntimeError(vault_error)

current_base_reserve_raw = decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b""))
current_quote_reserve_raw = decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b""))
mint_decimals = decode_mint_decimals(mint_account.get("data_raw", b""))
spend_lamports = sol_to_lamports(max_spend_sol)

if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
    entry_reserve_in_raw = current_base_reserve_raw
    entry_reserve_out_raw = current_quote_reserve_raw
    exit_reserve_in_raw = current_quote_reserve_raw
    exit_reserve_out_raw = current_base_reserve_raw
    pool_reserve_token_raw = current_quote_reserve_raw
    pool_reserve_wsol_raw = current_base_reserve_raw
else:
    entry_reserve_in_raw = current_quote_reserve_raw
    entry_reserve_out_raw = current_base_reserve_raw
    exit_reserve_in_raw = current_base_reserve_raw
    exit_reserve_out_raw = current_quote_reserve_raw
    pool_reserve_token_raw = current_base_reserve_raw
    pool_reserve_wsol_raw = current_quote_reserve_raw

entry_expected_out_raw = constant_product_out(entry_reserve_in_raw, entry_reserve_out_raw, spend_lamports)
entry_min_out_raw = min_out_from_expected(entry_expected_out_raw, slippage_bps)
exit_quote_input_raw = entry_min_out_raw
direct_math_expected_out = constant_product_out(exit_reserve_in_raw, exit_reserve_out_raw, exit_quote_input_raw)
quote_path_expected_out = direct_math_expected_out
exit_min_out_raw = min_out_from_expected(quote_path_expected_out, slippage_bps)

if entry_expected_out_raw <= 0:
    reason_expected_out_zero = "entry_quote_expected_out_zero"
elif entry_min_out_raw <= 0:
    reason_expected_out_zero = "entry_quote_min_out_zero"
elif exit_quote_input_raw <= 0:
    reason_expected_out_zero = "exit_quote_input_zero"
elif pool_reserve_token_raw <= 0 or pool_reserve_wsol_raw <= 0:
    reason_expected_out_zero = "pool_reserve_zero"
elif quote_path_expected_out <= 0:
    reason_expected_out_zero = "integer_rounding_dust_under_zero_fee_model"
else:
    reason_expected_out_zero = "none"

report = {
    "mint": target_mint,
    "pool": target_pool,
    "mint_decimals": mint_decimals,
    "entry_output_native": to_native(entry_expected_out_raw, mint_decimals),
    "exit_quote_input_native": to_native(exit_quote_input_raw, mint_decimals),
    "pool_reserve_token": pool_reserve_token_raw,
    "pool_reserve_wsol": pool_reserve_wsol_raw,
    "pumpfun_fee_bps": 0,
    "fee_model_source": "constant_product_zero_fee_placeholder",
    "direct_math_expected_out": direct_math_expected_out,
    "quote_path_expected_out": quote_path_expected_out,
    "reason_expected_out_zero": reason_expected_out_zero,
    "entry_expected_out_raw": entry_expected_out_raw,
    "entry_min_out_raw": entry_min_out_raw,
    "exit_quote_input_raw": exit_quote_input_raw,
    "exit_min_out_raw": exit_min_out_raw,
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

pass "RunLSPREBRINV R1 exit quote autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

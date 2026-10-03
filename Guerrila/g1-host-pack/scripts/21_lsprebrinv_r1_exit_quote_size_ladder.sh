#!/usr/bin/env bash
# 21_lsprebrinv_r1_exit_quote_size_ladder.sh — direct Pump.fun exit-quote size ladder for one R1 mint

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1S — Exit Quote Size Ladder"

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
STEP="${SHADOW_LSPREBRINV_R1S_STEP:-21_lsprebrinv_r1_exit_quote_size_ladder}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_exit_quote_size_ladder_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

TARGET_MINT="${SHADOW_R1_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_R1_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"
SIZE_LIST="${SHADOW_LSPREBRINV_R1S_SIZE_LIST:-0.005,0.01,0.025,0.05,0.1}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_R1S_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1S_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1S_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LSPREBRINV_R1S_TIP_LAMPORTS:-0}"
MAX_LOSS_SOL="${SHADOW_LSPREBRINV_R1S_MAX_LOSS_SOL:-0.0025}"
RPC_URL="${SHADOW_LSPREBRINV_R1S_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$SIZE_LIST" "$SLIPPAGE_BPS" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" "$TIP_LAMPORTS" "$MAX_LOSS_SOL" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    rpc_url,
    target_mint,
    target_pool,
    size_list_arg,
    slippage_bps_arg,
    base_tx_fee_lamports_arg,
    priority_fee_lamports_arg,
    tip_lamports_arg,
    max_loss_sol_arg,
) = sys.argv[1:14]

manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)

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

slippage_bps = max(0, int(float(slippage_bps_arg or "100")))
base_tx_fee_lamports = max(0, int(float(base_tx_fee_lamports_arg or "5000")))
priority_fee_lamports = max(0, int(float(priority_fee_lamports_arg or "0")))
tip_lamports = max(0, int(float(tip_lamports_arg or "0")))
max_loss_lamports = max(0, int(round(float(max_loss_sol_arg or "0.0025") * 1_000_000_000.0)))
planned_fee_budget_lamports = (2 * base_tx_fee_lamports) + (2 * priority_fee_lamports) + (2 * tip_lamports)


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


accounts, slot, error = get_multiple_accounts([target_pool])
if error:
    raise RuntimeError(error)
pool_account = accounts.get(target_pool, {})
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

if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
    entry_reserve_in_raw = current_base_reserve_raw
    entry_reserve_out_raw = current_quote_reserve_raw
    exit_reserve_in_raw = current_quote_reserve_raw
    exit_reserve_out_raw = current_base_reserve_raw
else:
    entry_reserve_in_raw = current_quote_reserve_raw
    entry_reserve_out_raw = current_base_reserve_raw
    exit_reserve_in_raw = current_base_reserve_raw
    exit_reserve_out_raw = current_quote_reserve_raw

sizes = [piece.strip() for piece in text(size_list_arg).split(",") if piece.strip()]
rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

size_exit_ready_rows = 0
size_exit_zero_rows = 0
size_exit_dust_rows = 0
size_negative_too_large_rows = 0
size_policy_fail_rows = 0
best_ready_size_sol = ""

for size_text in sizes:
    spend_sol = float(size_text)
    spend_lamports = sol_to_lamports(spend_sol)
    entry_expected_out_raw = constant_product_out(entry_reserve_in_raw, entry_reserve_out_raw, spend_lamports)
    entry_min_out_raw = min_out_from_expected(entry_expected_out_raw, slippage_bps)
    exit_quote_input_raw = entry_min_out_raw
    exit_expected_out_raw = constant_product_out(exit_reserve_in_raw, exit_reserve_out_raw, exit_quote_input_raw)
    exit_min_out_raw = min_out_from_expected(exit_expected_out_raw, slippage_bps)
    immediate_roundtrip_loss_lamports = (spend_lamports + planned_fee_budget_lamports) - exit_expected_out_raw
    immediate_roundtrip_loss_sol = lamports_to_sol(immediate_roundtrip_loss_lamports)
    max_loss_required_sol = lamports_to_sol(max(0, immediate_roundtrip_loss_lamports))

    if entry_expected_out_raw <= 0 or entry_min_out_raw <= 0:
        decision = "SIZE_EXIT_ZERO"
        first_blocker = "entry_quote_non_positive"
        size_exit_zero_rows += 1
    elif exit_expected_out_raw <= 0:
        decision = "SIZE_EXIT_ZERO"
        first_blocker = "exit_quote_expected_out_zero"
        size_exit_zero_rows += 1
    elif exit_min_out_raw <= 0 or exit_expected_out_raw <= 1:
        decision = "SIZE_EXIT_DUST"
        first_blocker = "exit_quote_dust"
        size_exit_dust_rows += 1
    elif immediate_roundtrip_loss_lamports > max_loss_lamports:
        decision = "SIZE_NEGATIVE_TOO_LARGE"
        first_blocker = "max_loss_required_exceeds_r1"
        size_negative_too_large_rows += 1
    else:
        decision = "SIZE_EXIT_READY"
        first_blocker = "none"
        size_exit_ready_rows += 1
        if not best_ready_size_sol:
            best_ready_size_sol = size_text

    decision_counts[decision] += 1
    rows.append(
        {
            "record_type": "runlsprebrinv_r1_exit_quote_size_ladder_row",
            "mint": target_mint,
            "pool": target_pool,
            "size_sol": spend_sol,
            "size_lamports": spend_lamports,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "exit_quote_input_raw": exit_quote_input_raw,
            "exit_expected_out_raw": exit_expected_out_raw,
            "exit_min_out_raw": exit_min_out_raw,
            "immediate_roundtrip_loss_sol": immediate_roundtrip_loss_sol,
            "max_loss_required_sol": max_loss_required_sol,
            "decision": decision,
            "first_blocker": first_blocker,
        }
    )

if size_exit_ready_rows > 0:
    final_status = "R1_EXIT_READY_AT_SIZE"
elif (size_exit_zero_rows + size_exit_dust_rows) == len(rows):
    final_status = "PARK_NO_EXIT_AT_R1_SIZE_BAND"
else:
    final_status = "PARK_ONLY_LARGER_SIZE_NEGATIVE"

summary = {
    "status": final_status,
    "mint": target_mint,
    "pool": target_pool,
    "sizes_tested": [float(item) for item in sizes],
    "size_exit_ready_rows": size_exit_ready_rows,
    "size_exit_zero_rows": size_exit_zero_rows,
    "size_exit_dust_rows": size_exit_dust_rows,
    "size_negative_too_large_rows": size_negative_too_large_rows,
    "size_policy_fail_rows": size_policy_fail_rows,
    "decision_counts": dict(decision_counts),
    "best_ready_size_sol": best_ready_size_sol or "none",
    "pool_reserve_token_raw": exit_reserve_in_raw,
    "pool_reserve_wsol_raw": exit_reserve_out_raw,
    "planned_fee_budget_lamports": planned_fee_budget_lamports,
}

manifest = {
    "rpc_url": rpc_url,
    "target_mint": target_mint,
    "target_pool": target_pool,
    "size_list": sizes,
    "slippage_bps": slippage_bps,
    "base_tx_fee_lamports": base_tx_fee_lamports,
    "priority_fee_lamports": priority_fee_lamports,
    "tip_lamports": tip_lamports,
    "max_loss_sol": lamports_to_sol(max_loss_lamports),
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as handle:
    for row in rows:
        handle.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={final_status}",
            f"mint={target_mint}",
            f"pool={target_pool}",
            f"sizes_tested={','.join(sizes)}",
            f"size_exit_ready_rows={size_exit_ready_rows}",
            f"size_exit_zero_rows={size_exit_zero_rows}",
            f"size_exit_dust_rows={size_exit_dust_rows}",
            f"size_negative_too_large_rows={size_negative_too_large_rows}",
            f"decision_counts={json.dumps(dict(decision_counts), sort_keys=True)}",
            f"best_ready_size_sol={best_ready_size_sol or 'none'}",
        ]
    ) + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 exit quote size ladder written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

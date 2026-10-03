#!/usr/bin/env bash
# 21_lsprebrinv_r1_cost_model_v2.sh — compare inventory-band transport/setup cost architectures

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1CM2 — Cost Model V2"

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
STEP="${SHADOW_LSPREBRINV_R1CM2_STEP:-21_lsprebrinv_r1_cost_model_v2}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_cost_model_v2_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_cost_model_v2_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

TARGET_MINT="${SHADOW_R1_MINT:-}"
TARGET_POOL="${SHADOW_R1_POOL:-}"
SIZE_LIST="${SHADOW_LSPREBRINV_R1CM2_SIZE_LIST:-0.005,0.01,0.025,0.05,0.1}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_R1CM2_SLIPPAGE_BPS:-100}"
RPC_URL="${SHADOW_LSPREBRINV_R1CM2_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
LIVE_AUTOPSY_JSON_PATH="${SHADOW_LSPREBRINV_R1CM2_LIVE_AUTOPSY_JSON_PATH:-}"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$RESULTS_ROOT" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$SIZE_LIST" "$SLIPPAGE_BPS" "$LIVE_AUTOPSY_JSON_PATH" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.request
from pathlib import Path
from typing import Any

(
    results_root_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    rpc_url,
    target_mint,
    target_pool,
    size_list_arg,
    slippage_bps_arg,
    autopsy_json_arg,
) = sys.argv[1:12]

results_root = Path(results_root_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
autopsy_json_path = Path(str(autopsy_json_arg).strip()).expanduser() if str(autopsy_json_arg).strip() else None

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM_IDS = {
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
}
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


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
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
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    context = ssl.create_default_context() if url.startswith("https://") else None
    with urllib.request.urlopen(req, timeout=20, context=context) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(method: str, params: list[Any] | None = None) -> dict[str, Any]:
    payload: dict[str, Any] = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    response = http_post_json(rpc_url, payload)
    if isinstance(response, dict) and response.get("error") is not None:
        raise RuntimeError(f"{method}_failed:{response['error']}")
    return response


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def find_latest_autopsy_json(root: Path, mint: str, pool: str) -> Path:
    matches: list[Path] = []
    for path in root.rglob("runlsprebrinv_r1_live_pnl_autopsy.json"):
        try:
            obj = load_json(path)
        except Exception:
            continue
        if mint and text(obj.get("mint")) != mint:
            continue
        if pool and text(obj.get("pool")) != pool:
            continue
        matches.append(path)
    if not matches:
        raise RuntimeError("live_autopsy_json_missing")
    return max(matches, key=lambda item: item.stat().st_mtime)


def get_multiple_accounts(pubkeys: list[str], encoding: str = "base64") -> list[Any]:
    response = rpc("getMultipleAccounts", [pubkeys, {"encoding": encoding, "commitment": "confirmed"}]).get("result") or {}
    return response.get("value") or []


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


def min_out_from_expected(expected_out: int, slippage_bps: int) -> int:
    if expected_out <= 0:
        return 0
    bounded = min(max(slippage_bps, 0), 10_000)
    return max(0, expected_out * (10_000 - bounded) // 10_000)


autopsy_path = autopsy_json_path if autopsy_json_path is not None and autopsy_json_path.is_file() else find_latest_autopsy_json(results_root, target_mint, target_pool)
autopsy = load_json(autopsy_path)
if not target_mint:
    target_mint = text(autopsy.get("mint"))
if not target_pool:
    target_pool = text(autopsy.get("pool"))

slippage_bps = max(0, int(float(slippage_bps_arg or "100")))

buy_sender_tip = safe_int(autopsy.get("buy_sender_tip_lamports"))
sell_sender_tip = safe_int(autopsy.get("sell_sender_tip_lamports"))
buy_base_fee = safe_int(autopsy.get("buy_base_fee_lamports"))
sell_base_fee = safe_int(autopsy.get("sell_base_fee_lamports"))
priority_fee = safe_int(autopsy.get("priority_fee_lamports"))
entry_pumpfun_fee = safe_int(autopsy.get("entry_pumpfun_fee_lamports"))
exit_pumpfun_fee = safe_int(autopsy.get("exit_pumpfun_fee_lamports"))
entry_shortfall = safe_int(autopsy.get("entry_execution_shortfall_lamports"))
exit_shortfall = safe_int(autopsy.get("exit_execution_shortfall_lamports"))
observed_rounding = safe_int(autopsy.get("rounding_loss_lamports"))
account_creation_rent = safe_int(autopsy.get("account_creation_rent_lamports"))

created_account_rows = list(autopsy.get("acquisition_created_accounts") or []) + list(autopsy.get("flatten_created_accounts") or [])
created_account_pubkeys = [text(row.get("account")) for row in created_account_rows if text(row.get("account"))]
created_account_infos: list[dict[str, Any]] = []
recoverable_rent_lamports = 0
created_account_closeable = False
created_account_close_blocker = "no_created_account"
if created_account_pubkeys:
    account_values = get_multiple_accounts(created_account_pubkeys, encoding="jsonParsed")
    blockers: list[str] = []
    for pubkey, value, row in zip(created_account_pubkeys, account_values, created_account_rows):
        info = (value or {}).get("data", {}).get("parsed", {}).get("info", {}) if isinstance((value or {}).get("data"), dict) else {}
        token_amount = safe_int(((info.get("tokenAmount") or {}).get("amount")))
        owner = text((value or {}).get("owner"))
        lamports = safe_int((value or {}).get("lamports"))
        closeable = owner in TOKEN_PROGRAM_IDS and token_amount == 0
        created_account_infos.append(
            {
                "account": pubkey,
                "owner_program": owner,
                "lamports": lamports,
                "space": safe_int((value or {}).get("space")),
                "token_amount_raw": token_amount,
                "closeable": closeable,
            }
        )
        if closeable:
            recoverable_rent_lamports += lamports
            created_account_closeable = True
        else:
            blockers.append(f"{pubkey}:{owner or 'unknown_owner'}")
    created_account_close_blocker = "closeable_token_account" if created_account_closeable else (";".join(blockers) if blockers else "created_account_not_closeable")

pool_raw_value = get_multiple_accounts([target_pool])[0]
pool_raw = b""
if isinstance(pool_raw_value, dict):
    data = pool_raw_value.get("data")
    if isinstance(data, list) and data:
        pool_raw = base64.b64decode(text(data[0]).encode("ascii"), validate=False)
pool_doc, pool_error = decode_pumpfun_pool(pool_raw)
if pool_error or not pool_doc:
    raise RuntimeError(pool_error or "pool_decode_failed")
if {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} != {target_mint, WSOL_MINT}:
    raise RuntimeError("pool_mint_set_mismatch")

vault_values = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
vault_raws: dict[str, bytes] = {}
for key, value in zip([pool_doc["base_vault"], pool_doc["quote_vault"]], vault_values):
    raw = b""
    if isinstance(value, dict):
        data = value.get("data")
        if isinstance(data, list) and data:
            raw = base64.b64decode(text(data[0]).encode("ascii"), validate=False)
    vault_raws[key] = raw

current_base_reserve_raw = decode_vault_balance(vault_raws.get(pool_doc["base_vault"], b""))
current_quote_reserve_raw = decode_vault_balance(vault_raws.get(pool_doc["quote_vault"], b""))
if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
    entry_reserve_in_raw = current_base_reserve_raw
    entry_reserve_out_raw = current_quote_reserve_raw
else:
    entry_reserve_in_raw = current_quote_reserve_raw
    entry_reserve_out_raw = current_base_reserve_raw

modes = [
    {
        "mode": "A_sender_buy_sender_sell",
        "description": "Sender buy + Sender sell + created account this cycle",
        "buy_sender_tip_lamports": buy_sender_tip,
        "sell_sender_tip_lamports": sell_sender_tip,
        "buy_base_fee_lamports": buy_base_fee,
        "sell_base_fee_lamports": sell_base_fee,
        "one_time_setup_cost_lamports": account_creation_rent,
        "recoverable_rent_lamports": recoverable_rent_lamports,
        "entry_variable_cost_model_lamports": entry_pumpfun_fee + entry_shortfall,
        "exit_variable_cost_model_lamports": exit_pumpfun_fee + exit_shortfall,
        "projected_gross_mode": "roundtrip_after_buy",
    },
    {
        "mode": "B_cheap_buy_sender_sell",
        "description": "Cheap/RPC buy + Sender sell + created account this cycle",
        "buy_sender_tip_lamports": 0,
        "sell_sender_tip_lamports": sell_sender_tip,
        "buy_base_fee_lamports": buy_base_fee,
        "sell_base_fee_lamports": sell_base_fee,
        "one_time_setup_cost_lamports": account_creation_rent,
        "recoverable_rent_lamports": recoverable_rent_lamports,
        "entry_variable_cost_model_lamports": entry_pumpfun_fee + entry_shortfall,
        "exit_variable_cost_model_lamports": exit_pumpfun_fee + exit_shortfall,
        "projected_gross_mode": "roundtrip_after_buy",
    },
    {
        "mode": "C_precreated_account_cheap_buy_sender_sell",
        "description": "Precreated account + cheap/RPC buy + Sender sell",
        "buy_sender_tip_lamports": 0,
        "sell_sender_tip_lamports": sell_sender_tip,
        "buy_base_fee_lamports": buy_base_fee,
        "sell_base_fee_lamports": sell_base_fee,
        "one_time_setup_cost_lamports": 0,
        "recoverable_rent_lamports": 0,
        "entry_variable_cost_model_lamports": entry_pumpfun_fee + entry_shortfall,
        "exit_variable_cost_model_lamports": exit_pumpfun_fee + exit_shortfall,
        "projected_gross_mode": "roundtrip_after_buy",
    },
    {
        "mode": "D_precreated_account_held_inventory_sender_sell",
        "description": "Precreated account + already-held inventory + Sender sell",
        "buy_sender_tip_lamports": 0,
        "sell_sender_tip_lamports": sell_sender_tip,
        "buy_base_fee_lamports": 0,
        "sell_base_fee_lamports": sell_base_fee,
        "one_time_setup_cost_lamports": 0,
        "recoverable_rent_lamports": 0,
        "entry_variable_cost_model_lamports": 0,
        "exit_variable_cost_model_lamports": exit_pumpfun_fee + exit_shortfall,
        "projected_gross_mode": "held_inventory_markout",
    },
]

rows: list[dict[str, Any]] = []
best_by_mode: dict[str, dict[str, Any]] = {}
for piece in [item.strip() for item in text(size_list_arg).split(",") if item.strip()]:
    size_sol = safe_float(piece)
    spend_lamports = sol_to_lamports(size_sol)
    entry_expected_out_raw = constant_product_out(entry_reserve_in_raw, entry_reserve_out_raw, spend_lamports)
    entry_min_out_raw = min_out_from_expected(entry_expected_out_raw, slippage_bps)
    if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == target_mint:
        post_base_reserve_raw = current_base_reserve_raw + spend_lamports
        post_quote_reserve_raw = max(0, current_quote_reserve_raw - entry_expected_out_raw)
        exit_expected_out_after_buy_raw = constant_product_out(post_quote_reserve_raw, post_base_reserve_raw, entry_expected_out_raw)
        held_inventory_exit_expected_out_raw = constant_product_out(current_quote_reserve_raw, current_base_reserve_raw, entry_expected_out_raw)
    else:
        post_quote_reserve_raw = current_quote_reserve_raw + spend_lamports
        post_base_reserve_raw = max(0, current_base_reserve_raw - entry_expected_out_raw)
        exit_expected_out_after_buy_raw = constant_product_out(post_base_reserve_raw, post_quote_reserve_raw, entry_expected_out_raw)
        held_inventory_exit_expected_out_raw = constant_product_out(current_base_reserve_raw, current_quote_reserve_raw, entry_expected_out_raw)
    exit_min_out_after_buy_raw = min_out_from_expected(exit_expected_out_after_buy_raw, slippage_bps)
    held_inventory_exit_min_out_raw = min_out_from_expected(held_inventory_exit_expected_out_raw, slippage_bps)

    for mode in modes:
        fixed_cost = (
            safe_int(mode["buy_sender_tip_lamports"])
            + safe_int(mode["sell_sender_tip_lamports"])
            + safe_int(mode["buy_base_fee_lamports"])
            + safe_int(mode["sell_base_fee_lamports"])
            + priority_fee
        )
        one_time_setup_cost = safe_int(mode["one_time_setup_cost_lamports"])
        recoverable_rent = min(safe_int(mode["recoverable_rent_lamports"]), one_time_setup_cost)
        one_time_setup_at_risk = max(0, one_time_setup_cost - recoverable_rent)
        variable_cost = safe_int(mode["entry_variable_cost_model_lamports"]) + safe_int(mode["exit_variable_cost_model_lamports"]) + observed_rounding
        if mode["projected_gross_mode"] == "held_inventory_markout":
            exit_expected_out_raw = held_inventory_exit_expected_out_raw
            exit_min_out_raw = held_inventory_exit_min_out_raw
            projected_gross_pnl_lamports = exit_expected_out_raw - spend_lamports
        else:
            exit_expected_out_raw = exit_expected_out_after_buy_raw
            exit_min_out_raw = exit_min_out_after_buy_raw
            projected_gross_pnl_lamports = exit_expected_out_raw - spend_lamports
        per_cycle_cost = fixed_cost + variable_cost
        total_cycle_cost_with_setup = per_cycle_cost + one_time_setup_at_risk
        expected_net_after_all_costs = projected_gross_pnl_lamports - total_cycle_cost_with_setup
        expected_net_after_setup_recovery = projected_gross_pnl_lamports - per_cycle_cost
        break_even_move_bps = int(round((max(0, total_cycle_cost_with_setup - projected_gross_pnl_lamports) / max(spend_lamports, 1)) * 10_000))
        row = {
            "mode": mode["mode"],
            "description": mode["description"],
            "size_sol": size_sol,
            "spend_lamports": spend_lamports,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "exit_expected_out_raw": exit_expected_out_raw,
            "exit_min_out_raw": exit_min_out_raw,
            "projected_gross_pnl_lamports": projected_gross_pnl_lamports,
            "fixed_cost_lamports": fixed_cost,
            "one_time_setup_cost_lamports": one_time_setup_cost,
            "recoverable_rent_lamports": recoverable_rent,
            "one_time_setup_at_risk_lamports": one_time_setup_at_risk,
            "per_cycle_cost_lamports": per_cycle_cost,
            "entry_variable_cost_model_lamports": safe_int(mode["entry_variable_cost_model_lamports"]),
            "exit_variable_cost_model_lamports": safe_int(mode["exit_variable_cost_model_lamports"]),
            "expected_net_after_all_costs_lamports": expected_net_after_all_costs,
            "expected_net_after_setup_recovery_lamports": expected_net_after_setup_recovery,
            "break_even_move_bps": break_even_move_bps,
            "projected_gross_mode": mode["projected_gross_mode"],
            "decision": "MODE_NET_POSITIVE" if expected_net_after_all_costs > 0 else "MODE_NET_NEGATIVE",
        }
        rows.append(row)
        best = best_by_mode.get(mode["mode"])
        if best is None or safe_int(row["expected_net_after_all_costs_lamports"]) > safe_int(best["expected_net_after_all_costs_lamports"]):
            best_by_mode[mode["mode"]] = row

summary = {
    "status": "R1_COST_MODEL_V2_READY",
    "mint": target_mint,
    "pool": target_pool,
    "sizes_tested": [safe_float(item) for item in [s.strip() for s in text(size_list_arg).split(",") if s.strip()]],
    "created_account_closeable": created_account_closeable,
    "created_account_close_blocker": created_account_close_blocker,
    "created_account_infos": created_account_infos,
    "best_by_mode": best_by_mode,
    "positive_mode_rows": sum(1 for row in rows if safe_int(row.get("expected_net_after_all_costs_lamports")) > 0),
}

manifest = {
    "rpc_url": rpc_url,
    "mint": target_mint,
    "pool": target_pool,
    "size_list": [s.strip() for s in text(size_list_arg).split(",") if s.strip()],
    "slippage_bps": slippage_bps,
    "live_autopsy_json_path": str(autopsy_path),
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as handle:
    for row in rows:
        handle.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    "status=R1_COST_MODEL_V2_READY",
    f"mint={target_mint}",
    f"pool={target_pool}",
    f"sizes_tested={','.join([s.strip() for s in text(size_list_arg).split(',') if s.strip()])}",
    f"created_account_closeable={str(created_account_closeable).lower()}",
    f"created_account_close_blocker={created_account_close_blocker or 'none'}",
    f"positive_mode_rows={summary['positive_mode_rows']}",
]
for mode_name in [
    "A_sender_buy_sender_sell",
    "B_cheap_buy_sender_sell",
    "C_precreated_account_cheap_buy_sender_sell",
    "D_precreated_account_held_inventory_sender_sell",
]:
    best = best_by_mode.get(mode_name) or {}
    summary_lines.extend(
        [
            f"{mode_name}.best_size_sol={best.get('size_sol', 'none')}",
            f"{mode_name}.fixed_cost_lamports={safe_int(best.get('fixed_cost_lamports'))}",
            f"{mode_name}.one_time_setup_cost_lamports={safe_int(best.get('one_time_setup_cost_lamports'))}",
            f"{mode_name}.recoverable_rent_lamports={safe_int(best.get('recoverable_rent_lamports'))}",
            f"{mode_name}.per_cycle_cost_lamports={safe_int(best.get('per_cycle_cost_lamports'))}",
            f"{mode_name}.expected_net_after_all_costs_lamports={safe_int(best.get('expected_net_after_all_costs_lamports'))}",
            f"{mode_name}.break_even_move_bps={safe_int(best.get('break_even_move_bps'))}",
        ]
    )

summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "RunLSPREBRINV R1 cost model v2 written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote.sh — project post-target Meteora DAMM v2 state from preprocessed target rows and quote a no-send backrun/postrun

source "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR0M — Meteora DAMM v2 Projection / Quote"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
RESULTS_ROOT="${SHADOW_RUNLSPREBR0M_RESULTS_ROOT:-$PACK_DIR/results}"
STEP="${SHADOW_RUNLSPREBR0M_STEP:-21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
ROWS_PATH="$RESULT_DIR/runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_summary.json"
SELECTOR_ROWS_PATH="${SHADOW_RUNLSPREBR0M_SELECTOR_ROWS_PATH:-$RESULTS_ROOT/21_runls13b_preprocessed_backrun_selector/runls13b_preprocessed_backrun_selector_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_RUNLSPREBR0M_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_RUNLSPREBR0M_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
MAX_ROWS="${SHADOW_RUNLSPREBR0M_MAX_ROWS:-20}"
SLIPPAGE_BPS="${SHADOW_RUNLSPREBR0M_SLIPPAGE_BPS:-100}"
DEFAULT_BASE_TX_FEE_LAMPORTS="${SHADOW_RUNLSPREBR0M_DEFAULT_BASE_TX_FEE_LAMPORTS:-5000}"
DEFAULT_TIP_LAMPORTS="${SHADOW_RUNLSPREBR0M_DEFAULT_TIP_LAMPORTS:-200000}"

is_placeholder_value() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == *CHANGE_ME* || "$value" == *'${'* ]]
}

append_api_key_if_needed() {
    local url="$1"
    local api_key="$2"
    if is_placeholder_value "$url" || is_placeholder_value "$api_key"; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"api-key="* || "$url" == *"apikey="* ]]; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"?"* ]]; then
        printf '%s&api-key=%s\n' "$url" "$api_key"
    else
        printf '%s?api-key=%s\n' "$url" "$api_key"
    fi
}

RPC_URL="${SHADOW_RUNLSPREBR0M_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}"
HELIUS_RPC_API_KEY="${SHADOW_RUNLSPREBR0M_RPC_API_KEY:-${HELIUS_API_KEY:-${HELIUS_BUNDLE_API_KEY:-}}}"
RPC_URL="$(append_api_key_if_needed "$RPC_URL" "$HELIUS_RPC_API_KEY")"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$ROWS_PATH" "$SUMMARY_JSON_PATH"

if [[ ! -f "$SELECTOR_ROWS_PATH" ]]; then
    fail "RunLSPREBR0M selector rows missing: $SELECTOR_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$LS13A_ROWS_PATH" ]]; then
    fail "RunLSPREBR0M LS13A rows missing: $LS13A_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$ALT_AUDIT_ROWS_PATH" ]]; then
    fail "RunLSPREBR0M ALT audit rows missing: $ALT_AUDIT_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$UTILS_PATH" ]]; then
    fail "RunLSPREBR0M utils missing: $UTILS_PATH"
    finish
    exit 1
fi
if is_placeholder_value "$RPC_URL"; then
    fail "RunLSPREBR0M RPC URL missing"
    finish
    exit 1
fi

python3 - "$UTILS_PATH" "$SELECTOR_ROWS_PATH" "$LS13A_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE" "$RPC_URL" "$MAX_ROWS" "$SLIPPAGE_BPS" "$DEFAULT_BASE_TX_FEE_LAMPORTS" "$DEFAULT_TIP_LAMPORTS" <<'PY'
from __future__ import annotations

import base64
import importlib.util
import json
import ssl
import sys
import time
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

utils_path = Path(sys.argv[1])
selector_rows_path = Path(sys.argv[2])
ls13a_rows_path = Path(sys.argv[3])
alt_audit_rows_path = Path(sys.argv[4])
rows_path = Path(sys.argv[5])
summary_json_path = Path(sys.argv[6])
summary_file = Path(sys.argv[7])
rpc_url = sys.argv[8]
max_rows = int(sys.argv[9])
slippage_bps = int(sys.argv[10])
default_base_tx_fee_lamports = int(sys.argv[11])
default_tip_lamports = int(sys.argv[12])

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

WSOL_MINT = "So11111111111111111111111111111111111111112"
METEORA_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"
METEORA_DAMM_V2_ACCOUNT_LEN = 1112
METEORA_TOKEN_A_MINT_OFFSET = 168
METEORA_TOKEN_B_MINT_OFFSET = 200
METEORA_TOKEN_A_VAULT_OFFSET = 232
METEORA_TOKEN_B_VAULT_OFFSET = 264
METEORA_POOL_STATUS_OFFSET = 481
METEORA_ENABLED_STATUS = 0
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64

METEORA_SWAP = bytes([248, 198, 158, 145, 225, 117, 135, 200])
METEORA_SWAP2 = bytes([65, 75, 63, 76, 235, 91, 91, 136])
SWAP_MODE_EXACT_OUT = 2


def rpc(method: str, params: list[Any]) -> dict[str, Any]:
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    request = urllib.request.Request(
        rpc_url,
        data=payload,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    context = ssl.create_default_context() if rpc_url.startswith("https://") else None
    with urllib.request.urlopen(request, timeout=30, context=context) as response:
        return json.loads(response.read().decode("utf-8", errors="replace"))


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    try:
        response = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    except urllib.error.URLError:
        return {}, 0, "PRE_TARGET_POOL_STATE_MISSING"
    except Exception:
        return {}, 0, "PRE_TARGET_POOL_STATE_MISSING"

    if response.get("error"):
        return {}, 0, "PRE_TARGET_POOL_STATE_MISSING"

    result = response.get("result") or {}
    context = result.get("context") or {}
    slot = utils.as_int(context.get("slot"))
    values = result.get("value") or []
    accounts: dict[str, dict[str, Any]] = {}
    for pubkey, value in zip(pubkeys, values):
        if not value:
            accounts[pubkey] = {"exists": False, "data_raw": b""}
            continue
        data_field = value.get("data") if isinstance(value, dict) else None
        raw = b""
        if isinstance(data_field, list) and data_field:
            try:
                raw = base64.b64decode(str(data_field[0]), validate=True)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": utils.as_int(value.get("lamports")),
            "owner": utils.text(value.get("owner")),
        }
    return accounts, slot, None


def decode_meteora_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) != METEORA_DAMM_V2_ACCOUNT_LEN:
        return None, "PRE_TARGET_POOL_STATE_MISSING"
    status = raw[METEORA_POOL_STATUS_OFFSET]
    return {
        "status_enabled": status == METEORA_ENABLED_STATUS,
        "token_a_mint": utils.b58(raw[METEORA_TOKEN_A_MINT_OFFSET:METEORA_TOKEN_A_MINT_OFFSET + 32]),
        "token_b_mint": utils.b58(raw[METEORA_TOKEN_B_MINT_OFFSET:METEORA_TOKEN_B_MINT_OFFSET + 32]),
        "token_a_vault": utils.b58(raw[METEORA_TOKEN_A_VAULT_OFFSET:METEORA_TOKEN_A_VAULT_OFFSET + 32]),
        "token_b_vault": utils.b58(raw[METEORA_TOKEN_B_VAULT_OFFSET:METEORA_TOKEN_B_VAULT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int | None:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return None
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def min_out_from_expected(expected_out: int, slippage_bps_value: int) -> int:
    if expected_out <= 0:
        return 0
    adjusted = (expected_out * max(0, 10_000 - slippage_bps_value)) // 10_000
    return max(0, adjusted)


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int, trade_fee_num: int, trade_fee_denom: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0 or trade_fee_denom <= 0:
        return 0
    fee = ((amount_in * trade_fee_num) + trade_fee_denom - 1) // trade_fee_denom
    amount_in_less_fee = max(0, amount_in - fee)
    denominator = reserve_in + amount_in_less_fee
    if denominator <= 0:
        return 0
    return (amount_in_less_fee * reserve_out) // denominator


def decode_meteora_target(parsed: dict[str, Any], resolved_account_keys: list[str]) -> tuple[dict[str, Any] | None, str | None]:
    for ix in utils.instruction_views(parsed, resolved_account_keys):
        if ix.get("program_id") != METEORA_PROGRAM_ID:
            continue
        accounts = ix.get("accounts") or []
        data = ix.get("data") or b""
        if data[:8] == METEORA_SWAP and len(accounts) >= 11 and len(data) >= 24:
            amount_in_raw = utils.le_u64(data, 8)
            min_out_raw = utils.le_u64(data, 16)
            return {
                "program_id": METEORA_PROGRAM_ID,
                "instruction_index": utils.as_int(ix.get("instruction_index")),
                "pool": accounts[1],
                "mint_in": accounts[6],
                "mint_out": accounts[7],
                "direction": f"{accounts[6]}->{accounts[7]}",
                "amount_in_raw": amount_in_raw,
                "minimum_amount_out_raw": min_out_raw,
                "pattern": "swap",
            }, None
        if data[:8] == METEORA_SWAP2 and len(accounts) >= 11 and len(data) >= 25:
            amount_0 = utils.le_u64(data, 8)
            amount_1 = utils.le_u64(data, 16)
            swap_mode = data[24]
            if swap_mode == SWAP_MODE_EXACT_OUT:
                amount_in_raw = amount_1
                min_out_raw = amount_0
            else:
                amount_in_raw = amount_0
                min_out_raw = amount_1
            return {
                "program_id": METEORA_PROGRAM_ID,
                "instruction_index": utils.as_int(ix.get("instruction_index")),
                "pool": accounts[1],
                "mint_in": accounts[6],
                "mint_out": accounts[7],
                "direction": f"{accounts[6]}->{accounts[7]}",
                "amount_in_raw": amount_in_raw,
                "minimum_amount_out_raw": min_out_raw,
                "pattern": "swap2",
            }, None
    return None, "METEORA_DAMM_V2_DECODER_MISSING"


selector_rows = [
    row
    for row in utils.load_jsonl(selector_rows_path)
    if utils.text(row.get("selector_decision")) == "TARGET_ENTITY_READY"
    and utils.text(row.get("target_venue")) == "meteora_damm_v2"
][:max_rows]
ls13a_by_signature = {
    utils.text(row.get("target_signature")): row
    for row in utils.load_jsonl(ls13a_rows_path)
    if utils.text(row.get("target_signature"))
}
alt_by_signature = {
    utils.text(row.get("target_signature")): row
    for row in utils.load_jsonl(alt_audit_rows_path)
    if utils.text(row.get("target_signature"))
}

output_rows: list[dict[str, Any]] = []
projection_blocker_counts: Counter[str] = Counter()

pre_target_pool_state_available_count = 0
projected_post_target_state_count = 0
projected_expected_out_positive_count = 0
projected_min_out_positive_count = 0
projected_ev_after_tip_positive_count = 0

top_net_ev_after_tip_raw = -10**30
top_pool = ""
top_venue = ""

for selector_row in selector_rows:
    started_ns = time.monotonic_ns()
    signature = utils.text(selector_row.get("target_signature"))
    ls13a_row = ls13a_by_signature.get(signature, {})
    alt_row = alt_by_signature.get(signature, {})
    target_tx_base64 = utils.text(ls13a_row.get("target_tx_base64"))
    target_seen_preprocessed_ns = utils.as_int(
        ls13a_row.get("target_receive_ns")
        or ls13a_row.get("provider_created_at_ns")
        or ls13a_row.get("target_seen_preprocessed_ns")
    )
    target_blockhash_valid = utils.as_bool(selector_row.get("target_blockhash_valid"))
    target_already_landed = utils.as_bool(selector_row.get("target_already_landed_at_first_check"))
    venue = utils.text(selector_row.get("target_venue"))
    pool = utils.text(selector_row.get("target_pool"))
    mint_in = utils.text(selector_row.get("target_mint_in"))
    mint_out = utils.text(selector_row.get("target_mint_out"))
    target_direction = utils.text(selector_row.get("target_direction"))
    target_amount_in_raw = utils.as_int(selector_row.get("target_amount_in_raw"))
    target_program_id = utils.text(selector_row.get("target_program_id"))
    target_instruction_index = selector_row.get("target_instruction_index")

    blocker = "PROJECTION_READY"
    parsed = None
    parse_error = None
    decoded_target = None
    target_min_out_raw_if_decodable = 0
    target_delta_reserve_in_raw = 0
    target_delta_reserve_out_raw = 0
    pre_target_pool_state_available = False
    projected_post_target_state_available = False
    pre_target_state_source = "rpc_processed_getMultipleAccounts_research"
    pre_target_state_slot = 0
    pre_target_reserve_in_raw = 0
    pre_target_reserve_out_raw = 0
    projected_reserve_in_raw = 0
    projected_reserve_out_raw = 0
    projection_source = "laserstream_preprocessed_local_projection"
    our_backrun_direction = ""
    our_input_raw = 0
    our_input_source = ""
    our_counterfactual_pre_target_out_raw = 0
    projected_expected_out_raw = 0
    projected_min_out_raw = 0
    tip_lamports = default_tip_lamports
    base_fee_lamports = default_base_tx_fee_lamports
    net_ev_after_tip_raw = 0
    fee_model_source = "meteora_damm_v2_pool_placeholder_zero_fee"
    trade_fee_numerator = 0
    trade_fee_denominator = 1_000_000_000

    if venue != "meteora_damm_v2" or target_program_id != METEORA_PROGRAM_ID:
        blocker = "METEORA_DAMM_V2_DECODER_MISSING"
    elif not target_tx_base64:
        blocker = "METEORA_DAMM_V2_DECODER_MISSING"
    else:
        parsed, parse_error = utils.parse_wire_tx(target_tx_base64)
        resolved_account_keys = list(alt_row.get("resolved_account_keys") or [])
        if not resolved_account_keys and parsed is not None:
            resolved_account_keys = list(parsed.get("static_account_keys") or [])
        if parsed is None or parse_error:
            blocker = "METEORA_DAMM_V2_DECODER_MISSING"
        else:
            decoded_target, decode_error = decode_meteora_target(parsed, resolved_account_keys)
            if decode_error or not decoded_target:
                blocker = "METEORA_DAMM_V2_DECODER_MISSING"
            else:
                target_min_out_raw_if_decodable = utils.as_int(decoded_target.get("minimum_amount_out_raw"))
                target_amount_in_raw = utils.as_int(decoded_target.get("amount_in_raw")) or target_amount_in_raw
                target_direction = utils.text(decoded_target.get("direction")) or target_direction
                target_instruction_index = decoded_target.get("instruction_index", target_instruction_index)
                accounts, pre_target_state_slot, state_error = get_multiple_accounts([pool])
                if state_error:
                    blocker = state_error
                else:
                    pool_account = accounts.get(pool, {})
                    pool_doc, pool_error = decode_meteora_pool(pool_account.get("data_raw", b""))
                    if pool_error or not pool_doc:
                        blocker = pool_error or "METEORA_DAMM_V2_DECODER_MISSING"
                    elif not pool_doc.get("status_enabled"):
                        blocker = "PROJECTED_STATE_INVALID"
                    else:
                        vault_accounts, _, vault_error = get_multiple_accounts([pool_doc["token_a_vault"], pool_doc["token_b_vault"]])
                        if vault_error:
                            blocker = vault_error
                        else:
                            token_a_reserve = decode_vault_balance(vault_accounts.get(pool_doc["token_a_vault"], {}).get("data_raw", b""))
                            token_b_reserve = decode_vault_balance(vault_accounts.get(pool_doc["token_b_vault"], {}).get("data_raw", b""))
                            if token_a_reserve is None or token_b_reserve is None:
                                blocker = "PRE_TARGET_POOL_STATE_MISSING"
                            else:
                                if pool_doc["token_a_mint"] == mint_in and pool_doc["token_b_mint"] == mint_out:
                                    reserve_in = token_a_reserve
                                    reserve_out = token_b_reserve
                                elif pool_doc["token_a_mint"] == mint_out and pool_doc["token_b_mint"] == mint_in:
                                    reserve_in = token_b_reserve
                                    reserve_out = token_a_reserve
                                else:
                                    blocker = "TARGET_DIRECTION_MISSING"
                                    reserve_in = 0
                                    reserve_out = 0

                                if blocker == "PROJECTION_READY":
                                    pre_target_pool_state_available = reserve_in > 0 and reserve_out > 0
                                    pre_target_reserve_in_raw = reserve_in
                                    pre_target_reserve_out_raw = reserve_out
                                    if not pre_target_pool_state_available:
                                        blocker = "PRE_TARGET_POOL_STATE_MISSING"

                                if blocker == "PROJECTION_READY":
                                    target_out_raw = constant_product_out(
                                        reserve_in,
                                        reserve_out,
                                        target_amount_in_raw,
                                        trade_fee_numerator,
                                        trade_fee_denominator,
                                    )
                                    target_delta_reserve_in_raw = target_amount_in_raw
                                    target_delta_reserve_out_raw = -target_out_raw
                                    projected_reserve_in_raw = reserve_in + target_amount_in_raw
                                    projected_reserve_out_raw = reserve_out - target_out_raw
                                    projected_post_target_state_available = (
                                        target_out_raw > 0
                                        and projected_reserve_in_raw > 0
                                        and projected_reserve_out_raw > 0
                                    )
                                    if not projected_post_target_state_available:
                                        blocker = "TARGET_DELTA_COMPUTE_FAILED"

                                if blocker == "PROJECTION_READY":
                                    if mint_in == WSOL_MINT:
                                        our_backrun_direction = f"{mint_out}->{mint_in}"
                                        our_input_raw = target_min_out_raw_if_decodable or target_out_raw
                                        our_input_source = (
                                            "target_min_out_raw_if_decodable"
                                            if target_min_out_raw_if_decodable > 0
                                            else "target_projected_out_raw"
                                        )
                                        baseline_our_reserve_in = reserve_out
                                        baseline_our_reserve_out = reserve_in
                                        projected_our_reserve_in = projected_reserve_out_raw
                                        projected_our_reserve_out = projected_reserve_in_raw
                                    elif mint_out == WSOL_MINT:
                                        our_backrun_direction = f"{mint_out}->{mint_in}"
                                        our_input_raw = target_min_out_raw_if_decodable or target_out_raw
                                        our_input_source = (
                                            "target_min_out_raw_if_decodable"
                                            if target_min_out_raw_if_decodable > 0
                                            else "target_projected_out_raw"
                                        )
                                        baseline_our_reserve_in = reserve_out
                                        baseline_our_reserve_out = reserve_in
                                        projected_our_reserve_in = projected_reserve_out_raw
                                        projected_our_reserve_out = projected_reserve_in_raw
                                    else:
                                        blocker = "QUOTE_MISSING"
                                        baseline_our_reserve_in = 0
                                        baseline_our_reserve_out = 0
                                        projected_our_reserve_in = 0
                                        projected_our_reserve_out = 0

                                if blocker == "PROJECTION_READY":
                                    if our_input_raw <= 0:
                                        blocker = "TARGET_AMOUNT_IN_MISSING"
                                    elif not our_backrun_direction:
                                        blocker = "TARGET_DIRECTION_MISSING"
                                    else:
                                        our_counterfactual_pre_target_out_raw = constant_product_out(
                                            baseline_our_reserve_in,
                                            baseline_our_reserve_out,
                                            our_input_raw,
                                            trade_fee_numerator,
                                            trade_fee_denominator,
                                        )
                                        projected_expected_out_raw = constant_product_out(
                                            projected_our_reserve_in,
                                            projected_our_reserve_out,
                                            our_input_raw,
                                            trade_fee_numerator,
                                            trade_fee_denominator,
                                        )
                                        if projected_expected_out_raw <= 0:
                                            blocker = "QUOTE_MISSING"
                                        else:
                                            projected_min_out_raw = min_out_from_expected(projected_expected_out_raw, slippage_bps)
                                            if projected_min_out_raw <= 0:
                                                blocker = "MIN_OUT_MISSING"
                                            else:
                                                net_ev_after_tip_raw = (
                                                    projected_expected_out_raw
                                                    - our_counterfactual_pre_target_out_raw
                                                    - tip_lamports
                                                    - base_fee_lamports
                                                )
                                                if net_ev_after_tip_raw <= 0:
                                                    blocker = "EV_NEGATIVE_AFTER_TIP"

    projection_latency_us = (time.monotonic_ns() - started_ns) // 1_000

    if pre_target_pool_state_available:
        pre_target_pool_state_available_count += 1
    if projected_post_target_state_available:
        projected_post_target_state_count += 1
    if projected_expected_out_raw > 0:
        projected_expected_out_positive_count += 1
    if projected_min_out_raw > 0:
        projected_min_out_positive_count += 1
    if net_ev_after_tip_raw > 0:
        projected_ev_after_tip_positive_count += 1
    if net_ev_after_tip_raw > top_net_ev_after_tip_raw:
        top_net_ev_after_tip_raw = net_ev_after_tip_raw
        top_pool = pool
        top_venue = venue

    projection_blocker_counts[blocker] += 1
    output_rows.append(
        {
            "record_type": "runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_row",
            "target_signature": signature,
            "target_entity_id": utils.text(selector_row.get("target_entity_id")),
            "target_seen_preprocessed_ns": target_seen_preprocessed_ns,
            "target_blockhash_valid": target_blockhash_valid,
            "target_already_landed_at_first_check": target_already_landed,
            "venue": venue,
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "target_direction": target_direction,
            "target_amount_in_raw": target_amount_in_raw,
            "target_min_out_raw_if_decodable": target_min_out_raw_if_decodable,
            "target_instruction_index": target_instruction_index,
            "target_program_id": target_program_id,
            "pre_target_pool_state_available": pre_target_pool_state_available,
            "pre_target_state_source": pre_target_state_source,
            "pre_target_state_slot": pre_target_state_slot,
            "pre_target_reserve_in_raw": pre_target_reserve_in_raw,
            "pre_target_reserve_out_raw": pre_target_reserve_out_raw,
            "target_delta_reserve_in_raw": target_delta_reserve_in_raw,
            "target_delta_reserve_out_raw": target_delta_reserve_out_raw,
            "projected_post_target_state_available": projected_post_target_state_available,
            "projected_reserve_in_raw": projected_reserve_in_raw,
            "projected_reserve_out_raw": projected_reserve_out_raw,
            "projection_source": projection_source,
            "projection_latency_us": projection_latency_us,
            "projection_blocker": blocker,
            "our_backrun_direction": our_backrun_direction,
            "our_input_raw": our_input_raw,
            "our_input_source": our_input_source,
            "counterfactual_pre_target_out_raw": our_counterfactual_pre_target_out_raw,
            "projected_expected_out_raw": projected_expected_out_raw,
            "projected_min_out_raw": projected_min_out_raw,
            "projected_min_out_positive": projected_min_out_raw > 0,
            "tip_lamports": tip_lamports,
            "base_fee_lamports": base_fee_lamports,
            "trade_fee_numerator": trade_fee_numerator,
            "trade_fee_denominator": trade_fee_denominator,
            "fee_model_source": fee_model_source,
            "net_ev_after_tip_raw": net_ev_after_tip_raw,
            "net_ev_after_tip_positive": net_ev_after_tip_raw > 0,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

rows_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in output_rows),
    encoding="utf-8",
)

summary = {
    "status": "runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_complete",
    "rows_seen": len(output_rows),
    "pre_target_pool_state_available_count": pre_target_pool_state_available_count,
    "projected_post_target_state_count": projected_post_target_state_count,
    "projected_expected_out_positive_count": projected_expected_out_positive_count,
    "projected_min_out_positive_count": projected_min_out_positive_count,
    "projected_ev_after_tip_positive_count": projected_ev_after_tip_positive_count,
    "projection_blocker_counts": dict(projection_blocker_counts),
    "top_venue": top_venue,
    "top_pool": top_pool,
    "top_net_ev_after_tip_raw": top_net_ev_after_tip_raw if top_net_ev_after_tip_raw > -(10**30) else 0,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"rows_seen={summary['rows_seen']}",
    f"pre_target_pool_state_available_count={summary['pre_target_pool_state_available_count']}",
    f"projected_post_target_state_count={summary['projected_post_target_state_count']}",
    f"projected_expected_out_positive_count={summary['projected_expected_out_positive_count']}",
    f"projected_min_out_positive_count={summary['projected_min_out_positive_count']}",
    f"projected_ev_after_tip_positive_count={summary['projected_ev_after_tip_positive_count']}",
    "projection_blocker_counts=" + json.dumps(summary["projection_blocker_counts"], sort_keys=True),
    f"top_venue={summary['top_venue']}",
    f"top_pool={summary['top_pool']}",
    f"top_net_ev_after_tip_raw={summary['top_net_ev_after_tip_raw']}",
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "RunLSPREBR0M rows written to $ROWS_PATH"
cat "$SUMMARY_FILE"
finish

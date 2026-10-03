#!/usr/bin/env bash
# 21_lsprebrinv_r1_mode_c_precreate_review.sh — unit/size audit for one mode-C precreate-review candidate

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1MCR — Mode C Precreate Review"

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
STEP="${SHADOW_LSPREBRINV_R1MCR_STEP:-21_lsprebrinv_r1_mode_c_precreate_review}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
REVIEW_JSON="$RESULT_DIR/runlsprebrinv_r1_mode_c_precreate_review.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_mode_c_precreate_review_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

MODE_C_ROWS_PATH="${SHADOW_LSPREBRINV_R1MCR_MODE_C_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_mode_c_restage/runlsprebrinv_r1_mode_c_restage_rows.jsonl}"
LIVE_AUTOPSY_JSON_PATH="${SHADOW_LSPREBRINV_R1MCR_LIVE_AUTOPSY_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_live_pnl_autopsy/runlsprebrinv_r1_live_pnl_autopsy.json}"
RPC_URL="${SHADOW_LSPREBRINV_R1MCR_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

TARGET_MINT="${SHADOW_R1_MINT:-${SHADOW_LSPREBRINV_R1_MINT:-8kZ1T1WjKXkVxTHRGdErCtYAYu7wBBYSPGkZf1MtXnKj}}"
TARGET_POOL="${SHADOW_R1_POOL:-${SHADOW_LSPREBRINV_R1_POOL:-CAJ6m6ZtRCqEzJZA6SHTZ22BJeD65GafqMtDXkbdzq6h}}"
REVIEW_DECISION="${SHADOW_LSPREBRINV_R1MCR_EXPECT_DECISION:-MODE_C_READY_PRECREATE_REVIEW}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_file_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$MODE_C_ROWS_PATH" ]] || MODE_C_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_mode_c_restage_rows.jsonl)"
[[ -f "$LIVE_AUTOPSY_JSON_PATH" ]] || LIVE_AUTOPSY_JSON_PATH="$(latest_file_for_step runlsprebrinv_r1_live_pnl_autopsy.json)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$REVIEW_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$MODE_C_ROWS_PATH" "$LIVE_AUTOPSY_JSON_PATH" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$REVIEW_DECISION" "$INPUT_MANIFEST" "$REVIEW_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import base64
import json
import math
import ssl
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

(
    mode_c_rows_arg,
    live_autopsy_arg,
    rpc_url,
    target_mint_arg,
    target_pool_arg,
    review_decision_arg,
    manifest_arg,
    review_json_arg,
    summary_json_arg,
    summary_txt_arg,
) = sys.argv[1:11]

mode_c_rows_path = Path(mode_c_rows_arg) if mode_c_rows_arg else Path("")
live_autopsy_path = Path(live_autopsy_arg) if live_autopsy_arg else Path("")
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()
review_decision = str(review_decision_arg or "MODE_C_READY_PRECREATE_REVIEW").strip() or "MODE_C_READY_PRECREATE_REVIEW"
manifest_path = Path(manifest_arg)
review_json_path = Path(review_json_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}
TOKEN2022_EXTENSION_NAMES = {
    1: "transferFeeConfig",
    2: "transferFeeAmount",
    3: "mintCloseAuthority",
    4: "confidentialTransferMint",
    5: "confidentialTransferAccount",
    6: "defaultAccountState",
    7: "immutableOwner",
    8: "memoTransfer",
    9: "nonTransferable",
    10: "interestBearingConfig",
    11: "cpiGuard",
    12: "permanentDelegate",
    13: "nonTransferableAccount",
    14: "transferHook",
    15: "transferHookAccount",
    16: "confidentialTransferFeeConfig",
    17: "confidentialTransferFeeAmount",
    18: "metadataPointer",
    19: "tokenMetadata",
    20: "groupPointer",
    21: "tokenGroup",
    22: "groupMemberPointer",
    23: "tokenGroupMember",
    24: "scaledUiAmountConfig",
    25: "pausableConfig",
    26: "pausableAccount",
}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def sol_to_lamports(value: Any) -> int:
    return max(0, int(round(safe_float(value) * 1_000_000_000.0)))


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists() or not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists() or not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def first_row(path: Path) -> dict[str, Any]:
    rows = load_jsonl(path)
    return rows[0] if rows else {}


def pick_mode_c_row(rows: list[dict[str, Any]]) -> dict[str, Any]:
    target_match = None
    first_review = None
    for row in rows:
        if text(row.get("decision")) != review_decision:
            continue
        if first_review is None:
            first_review = row
        if target_mint and text(row.get("mint")) != target_mint:
            continue
        if target_pool and text(row.get("pool")) != target_pool:
            continue
        target_match = row
        break
    return target_match or first_review or {}


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
    except Exception as exc:  # noqa: BLE001
        return None, {"error": str(exc)}


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def b58(data: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    num = int.from_bytes(data, "big")
    out = ""
    while num:
        num, rem = divmod(num, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    if pad or out:
        return alphabet[0] * pad + out
    return alphabet[0]


def fetch_mint_account(mint: str) -> tuple[dict[str, Any], dict[str, Any] | None]:
    result, error = rpc("getAccountInfo", [mint, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return {"mint": mint, "metadata_available": False, "rpc_error": error}, error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        return {"mint": mint, "metadata_available": False, "rpc_error": {"error": "mint_value_missing"}}, {"error": "mint_value_missing"}
    owner = text(value.get("owner"))
    data_field = value.get("data")
    raw = b""
    if isinstance(data_field, list) and data_field:
        try:
            raw = base64.b64decode(str(data_field[0]), validate=True)
        except Exception:
            raw = b""
    mint_authority = ""
    freeze_authority = ""
    decimals = None
    if len(raw) >= 45:
        decimals = raw[44]
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_authority = b58(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_authority = b58(raw[50:82])
    extensions: list[str] = []
    cursor = 82
    if owner == TOKEN2022_PROGRAM:
        while cursor + 4 <= len(raw):
            ext_type = int.from_bytes(raw[cursor:cursor + 2], "little")
            ext_len = int.from_bytes(raw[cursor + 2:cursor + 4], "little")
            cursor += 4
            if ext_type == 0 and ext_len == 0:
                break
            extensions.append(TOKEN2022_EXTENSION_NAMES.get(ext_type, f"extension_{ext_type}"))
            cursor += max(0, ext_len)
    return (
        {
            "mint": mint,
            "metadata_available": True,
            "token_program": owner,
            "mint_authority": mint_authority,
            "freeze_authority": freeze_authority,
            "decimals": decimals,
            "extensions": sorted(set(extensions)),
        },
        None,
    )


def policy_for_mint(metadata: dict[str, Any]) -> tuple[str, bool]:
    if not metadata.get("metadata_available"):
        return "metadata_unavailable", False
    freeze_authority = text(metadata.get("freeze_authority"))
    mint_authority = text(metadata.get("mint_authority"))
    if freeze_authority:
        return "freeze_authority_present", False
    if mint_authority:
        return "mint_authority_present", False
    token_program = text(metadata.get("token_program"))
    extensions = {lower(item) for item in metadata.get("extensions") or []}
    dangerous_extensions = sorted(item for item in extensions if item in DANGEROUS_TOKEN2022_EXTENSIONS)
    if token_program == TOKEN_PROGRAM:
        return "pass", True
    if token_program == TOKEN2022_PROGRAM:
        if dangerous_extensions:
            return "unsupported_token2022_extensions", False
        return "token2022_requires_explicit_allow", False
    return "unsupported_token_program", False


def get_balance(owner: str) -> tuple[int, str]:
    result, error = rpc("getBalance", [owner, {"commitment": "processed"}])
    if error is not None:
        return 0, text((error or {}).get("error") or "getBalance_failed")
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value", value)
    return safe_int(value), ""


def get_token_accounts(owner: str, mint: str) -> list[dict[str, Any]]:
    result, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "processed"}],
    )
    if error is not None:
        return []
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        values = value.get("value") or []
    else:
        values = []
    out: list[dict[str, Any]] = []
    for row in values:
        parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
        info = parsed.get("info") if isinstance(parsed, dict) else {}
        token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
        out.append(
            {
                "pubkey": text((row or {}).get("pubkey")),
                "amount_raw": safe_int((token_amount or {}).get("amount")),
            }
        )
    return out


def richest_token_account(owner: str, mint: str) -> dict[str, Any]:
    best = {"pubkey": "", "amount_raw": 0}
    for row in get_token_accounts(owner, mint):
        if safe_int(row.get("amount_raw")) > safe_int(best.get("amount_raw")):
            best = row
    return best


def min_out_from_expected(expected_out: int, slippage_bps: int) -> int:
    if expected_out <= 0:
        return 0
    bounded = min(max(slippage_bps, 0), 10_000)
    return max(0, (expected_out * (10_000 - bounded)) // 10_000)


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int, fee_num: int, fee_den: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0 or fee_den <= 0:
        return 0
    fee = ((amount_in * fee_num) + fee_den - 1) // fee_den
    amount_less_fee = max(0, amount_in - fee)
    denominator = reserve_in + amount_less_fee
    if denominator <= 0:
        return 0
    return (amount_less_fee * reserve_out) // denominator


mode_c_rows = load_jsonl(mode_c_rows_path)
mode_c_row = pick_mode_c_row(mode_c_rows)
candidate_dir = Path(text(mode_c_row.get("candidate_dir")))
direct_row = first_row(candidate_dir / "21_lsprebrpf_pumpfun_amm_direct_route_probe" / "runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl")
size_summary = load_json(candidate_dir / "21_lsprebrinv_r1_exit_quote_size_ladder" / "runlsprebrinv_r1_exit_quote_size_ladder_summary.json")
size_rows = load_jsonl(candidate_dir / "21_lsprebrinv_r1_exit_quote_size_ladder" / "runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl")
sim_row = first_row(candidate_dir / "21_lsprebrinv_tiny_inventory_band_sim" / "runlsprebrinv_tiny_inventory_band_sim_rows.jsonl")
live_autopsy = load_json(live_autopsy_path)

selected_mint = text(mode_c_row.get("mint")) or target_mint
selected_pool = text(mode_c_row.get("pool")) or target_pool
repeat_count_6h = safe_int(mode_c_row.get("repeat_count_6h"))
best_ready_size_sol_text = text(mode_c_row.get("best_ready_size_sol")) or text(size_summary.get("best_ready_size_sol")) or "none"
best_ready_size_sol = safe_float(best_ready_size_sol_text, 0.0)
planned_inventory_input_lamports = sol_to_lamports(best_ready_size_sol)

exact_size_row = {}
for row in size_rows:
    if abs(safe_float(row.get("size_sol")) - best_ready_size_sol) < 1e-12:
        exact_size_row = row
        break
if not exact_size_row and size_rows:
    exact_size_row = size_rows[0]

cheap_buy_expected_token_raw = safe_int(exact_size_row.get("entry_expected_out_raw"))
cheap_buy_min_token_raw = safe_int(exact_size_row.get("entry_min_out_raw"))
planned_inventory_token_out_raw = cheap_buy_min_token_raw or cheap_buy_expected_token_raw
projected_unwind_input_raw = safe_int(direct_row.get("our_input_raw"))
projected_unwind_input_lte_planned_inventory = projected_unwind_input_raw > 0 and planned_inventory_token_out_raw > 0 and projected_unwind_input_raw <= planned_inventory_token_out_raw

mint_in = text(direct_row.get("mint_in"))
mint_out = text(direct_row.get("mint_out"))
our_output_mint = text(direct_row.get("our_output_mint"))
trade_fee_numerator = safe_int(direct_row.get("trade_fee_numerator"))
trade_fee_denominator = max(1, safe_int(direct_row.get("trade_fee_denominator"), 1_000_000_000))

pre_target_reserve_in_raw = safe_int(direct_row.get("pre_target_reserve_in_raw"))
pre_target_reserve_out_raw = safe_int(direct_row.get("pre_target_reserve_out_raw"))
projected_reserve_in_raw = safe_int(direct_row.get("projected_reserve_in_raw"))
projected_reserve_out_raw = safe_int(direct_row.get("projected_reserve_out_raw"))

if mint_in == WSOL_MINT:
    baseline_our_reserve_in = pre_target_reserve_out_raw
    baseline_our_reserve_out = pre_target_reserve_in_raw
    projected_our_reserve_in = projected_reserve_out_raw
    projected_our_reserve_out = projected_reserve_in_raw
elif mint_out == WSOL_MINT:
    baseline_our_reserve_in = pre_target_reserve_in_raw
    baseline_our_reserve_out = pre_target_reserve_out_raw
    projected_our_reserve_in = projected_reserve_in_raw
    projected_our_reserve_out = projected_reserve_out_raw
else:
    baseline_our_reserve_in = 0
    baseline_our_reserve_out = 0
    projected_our_reserve_in = 0
    projected_our_reserve_out = 0

reported_counterfactual_pre_target_wsol_raw = safe_int(direct_row.get("counterfactual_pre_target_out_raw"))
reported_projected_exit_expected_wsol_raw = safe_int(direct_row.get("projected_expected_out_raw"))
reported_projected_exit_min_wsol_raw = safe_int(direct_row.get("projected_min_out_raw"))
projected_unwind_ev_lamports = safe_int(mode_c_row.get("projected_unwind_ev_lamports"))
projected_unwind_ev_is_wsol_lamports = our_output_mint == WSOL_MINT and reported_projected_exit_expected_wsol_raw > 0

recomputed_reported_counterfactual_wsol_raw = constant_product_out(
    baseline_our_reserve_in,
    baseline_our_reserve_out,
    projected_unwind_input_raw,
    trade_fee_numerator,
    trade_fee_denominator,
)
recomputed_reported_projected_exit_wsol_raw = constant_product_out(
    projected_our_reserve_in,
    projected_our_reserve_out,
    projected_unwind_input_raw,
    trade_fee_numerator,
    trade_fee_denominator,
)
hot_sender_tip_lamports = safe_int(mode_c_row.get("hot_sender_tip_lamports"), safe_int(live_autopsy.get("sell_sender_tip_lamports")))
hot_sender_base_fee_lamports = safe_int(live_autopsy.get("sell_base_fee_lamports"), 5000)
recomputed_reported_unwind_ev_lamports = (
    recomputed_reported_projected_exit_wsol_raw
    - recomputed_reported_counterfactual_wsol_raw
    - hot_sender_tip_lamports
    - hot_sender_base_fee_lamports
)
projected_unwind_ev_recomputed_matches_reported = abs(recomputed_reported_unwind_ev_lamports - projected_unwind_ev_lamports) <= 10
projected_unwind_ev_size_consistent = projected_unwind_input_lte_planned_inventory and projected_unwind_ev_recomputed_matches_reported

size_consistent_counterfactual_pre_target_wsol_raw = constant_product_out(
    baseline_our_reserve_in,
    baseline_our_reserve_out,
    planned_inventory_token_out_raw,
    trade_fee_numerator,
    trade_fee_denominator,
)
hot_sender_exit_expected_wsol_raw = constant_product_out(
    projected_our_reserve_in,
    projected_our_reserve_out,
    planned_inventory_token_out_raw,
    trade_fee_numerator,
    trade_fee_denominator,
)
slippage_bps = 100
hot_sender_exit_min_wsol_raw = min_out_from_expected(hot_sender_exit_expected_wsol_raw, slippage_bps)
size_consistent_projected_unwind_ev_lamports = (
    hot_sender_exit_expected_wsol_raw
    - size_consistent_counterfactual_pre_target_wsol_raw
    - hot_sender_tip_lamports
    - hot_sender_base_fee_lamports
)

cheap_buy_quote_ok = cheap_buy_expected_token_raw > 0 and cheap_buy_min_token_raw > 0
hot_sender_exit_quote_ok = (
    boolish(direct_row.get("pumpfun_direct_route_exists"))
    and boolish(direct_row.get("pumpfun_direct_quote_available"))
    and boolish(direct_row.get("pumpfun_direct_template_ready"))
    and boolish(direct_row.get("pumpfun_direct_tx_buildable"))
    and hot_sender_exit_expected_wsol_raw > 0
    and hot_sender_exit_min_wsol_raw > 0
)

mint_metadata, _ = fetch_mint_account(selected_mint) if selected_mint else ({"metadata_available": False}, {"error": "mint_missing"})
policy_blocker, policy_pass = policy_for_mint(mint_metadata) if selected_mint else ("mint_missing", False)
freeze_authority = text(mint_metadata.get("freeze_authority")) or "none"
mint_authority = text(mint_metadata.get("mint_authority")) or "none"
dangerous_extensions = sorted(
    item
    for item in (lower(extension) for extension in (mint_metadata.get("extensions") or []))
    if item in DANGEROUS_TOKEN2022_EXTENSIONS
)
dangerous_token2022_extensions = bool(dangerous_extensions)

owner_pubkey = text(sim_row.get("owner_pubkey"))
native_sol_balance_lamports, native_balance_error = get_balance(owner_pubkey) if owner_pubkey else (0, "owner_pubkey_missing")
wsol_entry = richest_token_account(owner_pubkey, WSOL_MINT) if owner_pubkey else {"pubkey": "", "amount_raw": 0}
wsol_balance_lamports = safe_int(wsol_entry.get("amount_raw"))
pm_available_inventory_lamports = max(native_sol_balance_lamports, wsol_balance_lamports)
pm_available_fee_lamports = native_sol_balance_lamports
pm_available_setup_lamports = native_sol_balance_lamports

pump_account_setup_lamports = safe_int(mode_c_row.get("account_setup_lamports"), safe_int(live_autopsy.get("pump_account_setup_lamports"), 1_844_400))
pump_account_setup_recoverable = boolish(mode_c_row.get("account_setup_recoverable"))
cheap_buy_cost_lamports = safe_int(mode_c_row.get("cheap_buy_cost_lamports"))
per_cycle_cost_lamports = safe_int(mode_c_row.get("per_cycle_cost_lamports"))
setup_amortized_ev = safe_int(mode_c_row.get("setup_amortized_ev"))
size_consistent_setup_amortized_ev = (
    size_consistent_projected_unwind_ev_lamports
    - per_cycle_cost_lamports
    - safe_int(mode_c_row.get("setup_amortization_lamports"))
)
setup_tx_fee_lamports = safe_int(live_autopsy.get("buy_base_fee_lamports"), 5000)
pm_reserve_setup_ok = pm_available_setup_lamports >= (pump_account_setup_lamports + setup_tx_fee_lamports)
pm_reserve_inventory_ok = planned_inventory_input_lamports > 0 and pm_available_inventory_lamports >= planned_inventory_input_lamports
pm_reserve_fee_ok = per_cycle_cost_lamports > 0 and pm_available_fee_lamports >= per_cycle_cost_lamports
pm_release_ok = boolish(sim_row.get("pm_release_ok"))

if not mode_c_row:
    status = "PARK_MODE_C_ROW_MISSING"
    first_blocker = "mode_c_precreate_review_row_missing"
elif not projected_unwind_ev_is_wsol_lamports:
    status = "PARK_EV_UNIT_AMBIGUOUS"
    first_blocker = "projected_unwind_ev_not_wsol_lamports"
elif not projected_unwind_ev_size_consistent:
    status = "PARK_PROJECTED_EV_SIZE_INCONSISTENT"
    first_blocker = (
        "projected_unwind_input_exceeds_planned_inventory"
        if not projected_unwind_input_lte_planned_inventory
        else "reported_projected_unwind_ev_mismatch"
    )
elif not cheap_buy_quote_ok:
    status = "PARK_NO_CHEAP_BUY"
    first_blocker = "cheap_buy_quote_missing"
elif not hot_sender_exit_quote_ok:
    status = "PARK_NO_HOT_EXIT"
    first_blocker = "hot_sender_exit_quote_missing"
elif not policy_pass:
    status = "PARK_POLICY"
    first_blocker = policy_blocker or "policy_fail"
elif not pm_reserve_setup_ok:
    status = "PARK_PM_SETUP"
    first_blocker = "pm_reserve_setup_failed"
elif not pm_reserve_inventory_ok:
    status = "PARK_PM_INVENTORY"
    first_blocker = "pm_reserve_inventory_failed"
elif not pm_reserve_fee_ok:
    status = "PARK_PM_FEE"
    first_blocker = "pm_reserve_fee_failed"
elif not pm_release_ok:
    status = "PARK_PM_RELEASE"
    first_blocker = "pm_release_failed"
elif setup_amortized_ev <= 0:
    status = "PARK_SETUP_NOT_AMORTIZABLE"
    first_blocker = "setup_amortized_ev_nonpositive"
else:
    status = "MODE_C_PRECREATE_REVIEW_READY"
    first_blocker = "none"

review = {
    "record_type": "runlsprebrinv_r1_mode_c_precreate_review",
    "status": status,
    "first_blocker": first_blocker,
    "mint": selected_mint,
    "pool": selected_pool,
    "candidate_dir": str(candidate_dir),
    "repeat_count_6h": repeat_count_6h,
    "best_ready_size_sol": best_ready_size_sol_text,
    "projected_unwind_ev_lamports": projected_unwind_ev_lamports,
    "projected_unwind_ev_is_wsol_lamports": projected_unwind_ev_is_wsol_lamports,
    "projected_unwind_ev_size_consistent": projected_unwind_ev_size_consistent,
    "projected_unwind_ev_recomputed_matches_reported": projected_unwind_ev_recomputed_matches_reported,
    "planned_inventory_input_lamports": planned_inventory_input_lamports,
    "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
    "projected_unwind_input_raw": projected_unwind_input_raw,
    "projected_unwind_input_lte_planned_inventory": projected_unwind_input_lte_planned_inventory,
    "cheap_buy_quote_ok": cheap_buy_quote_ok,
    "cheap_buy_expected_token_raw": cheap_buy_expected_token_raw,
    "cheap_buy_min_token_raw": cheap_buy_min_token_raw,
    "hot_sender_exit_quote_ok": hot_sender_exit_quote_ok,
    "hot_sender_exit_expected_wsol_raw": hot_sender_exit_expected_wsol_raw,
    "hot_sender_exit_min_wsol_raw": hot_sender_exit_min_wsol_raw,
    "pump_account_setup_lamports": pump_account_setup_lamports,
    "pump_account_setup_recoverable": pump_account_setup_recoverable,
    "per_cycle_cost_lamports": per_cycle_cost_lamports,
    "setup_amortized_ev": setup_amortized_ev,
    "size_consistent_projected_unwind_ev_lamports": size_consistent_projected_unwind_ev_lamports,
    "size_consistent_setup_amortized_ev": size_consistent_setup_amortized_ev,
    "policy_pass": policy_pass,
    "freeze_authority": freeze_authority,
    "mint_authority": mint_authority,
    "dangerous_extensions": dangerous_extensions,
    "dangerous_extensions_present": dangerous_token2022_extensions,
    "pm_reserve_setup_ok": pm_reserve_setup_ok,
    "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
    "pm_reserve_fee_ok": pm_reserve_fee_ok,
    "pm_release_ok": pm_release_ok,
    "owner_pubkey": owner_pubkey,
    "native_sol_balance_lamports": native_sol_balance_lamports,
    "wsol_balance_lamports": wsol_balance_lamports,
    "reported_counterfactual_pre_target_wsol_raw": reported_counterfactual_pre_target_wsol_raw,
    "reported_projected_exit_expected_wsol_raw": reported_projected_exit_expected_wsol_raw,
    "reported_projected_exit_min_wsol_raw": reported_projected_exit_min_wsol_raw,
    "recomputed_reported_counterfactual_wsol_raw": recomputed_reported_counterfactual_wsol_raw,
    "recomputed_reported_projected_exit_wsol_raw": recomputed_reported_projected_exit_wsol_raw,
    "recomputed_reported_unwind_ev_lamports": recomputed_reported_unwind_ev_lamports,
    "size_consistent_counterfactual_pre_target_wsol_raw": size_consistent_counterfactual_pre_target_wsol_raw,
    "policy_blocker": policy_blocker,
    "native_balance_error": native_balance_error,
}

manifest = {
    "mode_c_rows_path": str(mode_c_rows_path),
    "live_autopsy_json_path": str(live_autopsy_path),
    "rpc_url": rpc_url,
    "target_mint": target_mint,
    "target_pool": target_pool,
    "selected_candidate_dir": str(candidate_dir),
}

summary = {
    "status": status,
    "first_blocker": first_blocker,
    "mint": selected_mint,
    "pool": selected_pool,
    "repeat_count_6h": repeat_count_6h,
    "best_ready_size_sol": best_ready_size_sol_text,
    "projected_unwind_ev_lamports": projected_unwind_ev_lamports,
    "projected_unwind_ev_is_wsol_lamports": projected_unwind_ev_is_wsol_lamports,
    "projected_unwind_ev_size_consistent": projected_unwind_ev_size_consistent,
    "planned_inventory_input_lamports": planned_inventory_input_lamports,
    "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
    "projected_unwind_input_raw": projected_unwind_input_raw,
    "projected_unwind_input_lte_planned_inventory": projected_unwind_input_lte_planned_inventory,
    "cheap_buy_quote_ok": cheap_buy_quote_ok,
    "hot_sender_exit_quote_ok": hot_sender_exit_quote_ok,
    "hot_sender_exit_expected_wsol_raw": hot_sender_exit_expected_wsol_raw,
    "hot_sender_exit_min_wsol_raw": hot_sender_exit_min_wsol_raw,
    "pump_account_setup_lamports": pump_account_setup_lamports,
    "pump_account_setup_recoverable": pump_account_setup_recoverable,
    "per_cycle_cost_lamports": per_cycle_cost_lamports,
    "setup_amortized_ev": setup_amortized_ev,
    "size_consistent_projected_unwind_ev_lamports": size_consistent_projected_unwind_ev_lamports,
    "size_consistent_setup_amortized_ev": size_consistent_setup_amortized_ev,
    "policy_pass": policy_pass,
    "freeze_authority": freeze_authority,
    "mint_authority": mint_authority,
    "dangerous_extensions": dangerous_extensions,
    "pm_reserve_setup_ok": pm_reserve_setup_ok,
    "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
    "pm_reserve_fee_ok": pm_reserve_fee_ok,
    "pm_release_ok": pm_release_ok,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
review_json_path.write_text(json.dumps(review, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={status}",
            f"first_blocker={first_blocker}",
            f"mint={selected_mint}",
            f"pool={selected_pool}",
            f"repeat_count_6h={repeat_count_6h}",
            f"best_ready_size_sol={best_ready_size_sol_text}",
            f"projected_unwind_ev_lamports={projected_unwind_ev_lamports}",
            f"projected_unwind_ev_is_wsol_lamports={str(projected_unwind_ev_is_wsol_lamports).lower()}",
            f"projected_unwind_ev_size_consistent={str(projected_unwind_ev_size_consistent).lower()}",
            f"planned_inventory_input_lamports={planned_inventory_input_lamports}",
            f"planned_inventory_token_out_raw={planned_inventory_token_out_raw}",
            f"projected_unwind_input_raw={projected_unwind_input_raw}",
            f"projected_unwind_input_lte_planned_inventory={str(projected_unwind_input_lte_planned_inventory).lower()}",
            f"cheap_buy_quote_ok={str(cheap_buy_quote_ok).lower()}",
            f"cheap_buy_expected_token_raw={cheap_buy_expected_token_raw}",
            f"cheap_buy_min_token_raw={cheap_buy_min_token_raw}",
            f"hot_sender_exit_quote_ok={str(hot_sender_exit_quote_ok).lower()}",
            f"hot_sender_exit_expected_wsol_raw={hot_sender_exit_expected_wsol_raw}",
            f"hot_sender_exit_min_wsol_raw={hot_sender_exit_min_wsol_raw}",
            f"pump_account_setup_lamports={pump_account_setup_lamports}",
            f"pump_account_setup_recoverable={str(pump_account_setup_recoverable).lower()}",
            f"per_cycle_cost_lamports={per_cycle_cost_lamports}",
            f"setup_amortized_ev={setup_amortized_ev}",
            f"size_consistent_projected_unwind_ev_lamports={size_consistent_projected_unwind_ev_lamports}",
            f"size_consistent_setup_amortized_ev={size_consistent_setup_amortized_ev}",
            f"policy_pass={str(policy_pass).lower()}",
            f"freeze_authority={freeze_authority}",
            f"mint_authority={mint_authority}",
            f"dangerous_extensions={','.join(dangerous_extensions) if dangerous_extensions else 'false'}",
            f"pm_reserve_setup_ok={str(pm_reserve_setup_ok).lower()}",
            f"pm_reserve_inventory_ok={str(pm_reserve_inventory_ok).lower()}",
            f"pm_reserve_fee_ok={str(pm_reserve_fee_ok).lower()}",
            f"pm_release_ok={str(pm_release_ok).lower()}",
        ]
    ) + "\n",
    encoding="utf-8",
)
PY

pass "Mode C precreate review written: $SUMMARY_TXT"
finish

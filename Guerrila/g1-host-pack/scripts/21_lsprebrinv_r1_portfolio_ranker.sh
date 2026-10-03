#!/usr/bin/env bash
# 21_lsprebrinv_r1_portfolio_ranker.sh — rank Backrun R1 inventory-band proposal rows and select a top mint

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1R — Portfolio Ranker"

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
STEP="${SHADOW_LSPREBRINV_R1R_STEP:-21_lsprebrinv_r1_portfolio_ranker}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_portfolio_ranker_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_portfolio_ranker_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

FACTORY_ROWS_PATH="${SHADOW_LSPREBRINV_R1R_FACTORY_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_candidate_factory/runlsprebrinv_r1_candidate_factory_rows.jsonl}"
DIRECT_ROWS_PATH="${SHADOW_LSPREBRINV_R1R_DIRECT_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LSPREBRINV_R1R_SIM_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_tiny_inventory_band_sim/runlsprebrinv_tiny_inventory_band_sim_rows.jsonl}"
EXEC_SHAPE_ROWS_PATH="${SHADOW_LSPREBRINV_R1R_EXEC_SHAPE_ROWS_PATH:-$RESULTS_ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl}"
TAXONOMY_ROWS_PATH="${SHADOW_LSPREBRINV_R1R_TAXONOMY_ROWS_PATH:-$RESULTS_ROOT/21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy/runlsprebr3_preprocessed_backrun_direction_inventory_taxonomy_rows.jsonl}"
RPC_URL="${SHADOW_LSPREBRINV_R1R_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
LIVE_AUTOPSY_JSON_PATH="${SHADOW_LSPREBRINV_R1R_LIVE_AUTOPSY_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_live_pnl_autopsy/runlsprebrinv_r1_live_pnl_autopsy.json}"
TOP_N="${SHADOW_LSPREBRINV_R1R_TOP_N:-5}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$FACTORY_ROWS_PATH" ]] || FACTORY_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_candidate_factory_rows.jsonl)"
[[ -f "$DIRECT_ROWS_PATH" ]] || DIRECT_ROWS_PATH="$(latest_rows_for_step runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_tiny_inventory_band_sim_rows.jsonl)"
[[ -f "$EXEC_SHAPE_ROWS_PATH" ]] || EXEC_SHAPE_ROWS_PATH="$(latest_rows_for_step runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl)"
[[ -f "$TAXONOMY_ROWS_PATH" ]] || TAXONOMY_ROWS_PATH="$(latest_rows_for_step runlsprebr3_preprocessed_backrun_direction_inventory_taxonomy_rows.jsonl)"
[[ -f "$LIVE_AUTOPSY_JSON_PATH" ]] || LIVE_AUTOPSY_JSON_PATH="$(find "$RESULTS_ROOT" -type f -name 'runlsprebrinv_r1_live_pnl_autopsy.json' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$FACTORY_ROWS_PATH" "$DIRECT_ROWS_PATH" "$SIM_ROWS_PATH" "$EXEC_SHAPE_ROWS_PATH" "$TAXONOMY_ROWS_PATH" "$LIVE_AUTOPSY_JSON_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$RPC_URL" "$TOP_N" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    factory_rows_arg,
    direct_rows_arg,
    sim_rows_arg,
    exec_shape_rows_arg,
    taxonomy_rows_arg,
    live_autopsy_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    rpc_url_arg,
    top_n_arg,
) = sys.argv[1:13]

factory_rows_path = Path(factory_rows_arg) if factory_rows_arg else Path("")
direct_rows_path = Path(direct_rows_arg) if direct_rows_arg else Path("")
sim_rows_path = Path(sim_rows_arg) if sim_rows_arg else Path("")
exec_shape_rows_path = Path(exec_shape_rows_arg) if exec_shape_rows_arg else Path("")
taxonomy_rows_path = Path(taxonomy_rows_arg) if taxonomy_rows_arg else Path("")
live_autopsy_path = Path(live_autopsy_arg) if live_autopsy_arg else Path("")
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
rpc_url = str(rpc_url_arg or "").strip()
top_n = max(1, int(float(top_n_arg or "5")))

WSOL_MINT = "So11111111111111111111111111111111111111112"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64


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


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists() or not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


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
    if not rpc_url or "CHANGE_ME" in rpc_url or "${" in rpc_url:
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], str | None]:
    response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    if error is not None:
        return {}, text(error.get("error") if isinstance(error, dict) else error) or "rpc_error"
    result = (response or {}).get("result") or {}
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
                raw = base64.b64decode(str(data_field[0]), validate=False)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return accounts, None


def b58encode(raw: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
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
        chars.append(alphabet[rem])
    return ("1" * zeros) + "".join(reversed(chars or ["1"]))


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pool_state_missing"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pumpfun_pool_decode_failed"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def load_pool_depth(pool: str, mint: str) -> dict[str, Any]:
    if not pool or not mint:
        return {"pool_state_available": False, "pool_state_blocker": "pool_or_mint_missing"}
    accounts, error = get_multiple_accounts([pool])
    if error is not None:
        return {"pool_state_available": False, "pool_state_blocker": error}
    pool_doc, pool_error = decode_pumpfun_pool((accounts.get(pool) or {}).get("data_raw", b""))
    if pool_error or not pool_doc:
        return {"pool_state_available": False, "pool_state_blocker": pool_error or "pool_decode_failed"}
    if WSOL_MINT not in {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} or mint not in {pool_doc.get("base_mint"), pool_doc.get("quote_mint")}:
        return {"pool_state_available": False, "pool_state_blocker": "pool_mint_set_mismatch"}
    vaults, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
    if vault_error is not None:
        return {"pool_state_available": False, "pool_state_blocker": vault_error}
    base_reserve_raw = decode_vault_balance((vaults.get(pool_doc["base_vault"]) or {}).get("data_raw", b""))
    quote_reserve_raw = decode_vault_balance((vaults.get(pool_doc["quote_vault"]) or {}).get("data_raw", b""))
    if pool_doc.get("base_mint") == WSOL_MINT:
        core_reserve_raw = base_reserve_raw
        launch_reserve_raw = quote_reserve_raw
    else:
        core_reserve_raw = quote_reserve_raw
        launch_reserve_raw = base_reserve_raw
    return {
        "pool_state_available": True,
        "pool_state_blocker": "none",
        "pool_core_reserve_raw": core_reserve_raw,
        "pool_launch_reserve_raw": launch_reserve_raw,
        "entry_reserve_in_raw": base_reserve_raw if pool_doc.get("base_mint") == WSOL_MINT else quote_reserve_raw,
        "entry_reserve_out_raw": quote_reserve_raw if pool_doc.get("base_mint") == WSOL_MINT else base_reserve_raw,
    }


def min_out_from_expected(expected_out: int, slippage_bps: int) -> int:
    if expected_out <= 0:
        return 0
    return max(0, (expected_out * max(0, 10_000 - slippage_bps)) // 10_000)


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
        return 0
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return (amount_in * reserve_out) // denominator


def constant_product_out_with_fee(reserve_in: int, reserve_out: int, amount_in: int, fee_num: int, fee_den: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0 or fee_den <= 0:
        return 0
    fee = ((amount_in * fee_num) + fee_den - 1) // fee_den
    amount_less_fee = max(0, amount_in - fee)
    denominator = reserve_in + amount_less_fee
    if denominator <= 0:
        return 0
    return (amount_less_fee * reserve_out) // denominator


factory_rows = load_jsonl(factory_rows_path)
direct_rows = load_jsonl(direct_rows_path)
sim_rows = load_jsonl(sim_rows_path)
exec_shape_rows = load_jsonl(exec_shape_rows_path)
taxonomy_rows = load_jsonl(taxonomy_rows_path)
live_autopsy = load_json(live_autopsy_path)

hot_sender_tip_lamports = safe_int(live_autopsy.get("sell_sender_tip_lamports"), 200_000)
hot_sender_base_fee_lamports = safe_int(live_autopsy.get("sell_base_fee_lamports"), 5_000)
exit_variable_cost_lamports = safe_int(live_autopsy.get("exit_pumpfun_fee_lamports")) + safe_int(live_autopsy.get("exit_execution_shortfall_lamports"))
cheap_buy_base_fee_lamports = safe_int(live_autopsy.get("buy_base_fee_lamports"), 5_000)
cheap_buy_variable_cost_lamports = safe_int(live_autopsy.get("entry_pumpfun_fee_lamports")) + safe_int(live_autopsy.get("entry_execution_shortfall_lamports"))
pump_account_setup_lamports = safe_int(live_autopsy.get("pump_account_setup_lamports"), 1_844_400)

manifest = {
    "factory_rows_path": str(factory_rows_path),
    "direct_rows_path": str(direct_rows_path),
    "sim_rows_path": str(sim_rows_path),
    "exec_shape_rows_path": str(exec_shape_rows_path),
    "taxonomy_rows_path": str(taxonomy_rows_path),
    "live_autopsy_json_path": str(live_autopsy_path),
    "rpc_url": rpc_url,
    "top_n": top_n,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

direct_by_candidate = {text(row.get("candidate_id")): row for row in direct_rows if text(row.get("candidate_id"))}
sim_by_candidate = {text(row.get("candidate_id")): row for row in sim_rows if text(row.get("candidate_id"))}
exec_shape_by_candidate = {text(row.get("candidate_id")): row for row in exec_shape_rows if text(row.get("candidate_id"))}
taxonomy_by_candidate = {text(row.get("candidate_id")): row for row in taxonomy_rows if text(row.get("candidate_id"))}
repeat_counter = Counter((text(row.get("our_input_mint")), text(row.get("pool"))) for row in direct_rows if text(row.get("our_input_mint")) and text(row.get("pool")))

proposal_rows = [row for row in factory_rows if text(row.get("decision")).startswith("R1_PROPOSAL_READY")]
source_decision_counts = Counter(text(row.get("decision")) or "UNKNOWN" for row in proposal_rows)

ranked_rows: list[dict[str, Any]] = []
for row in proposal_rows:
    candidate_id = text(row.get("candidate_id"))
    mint = text(row.get("mint"))
    pool = text(row.get("pool"))
    direct_row = direct_by_candidate.get(candidate_id, {})
    sim_row = sim_by_candidate.get(candidate_id, {})
    exec_shape_row = exec_shape_by_candidate.get(candidate_id, {})
    taxonomy_row = taxonomy_by_candidate.get(candidate_id, {})
    pool_depth = load_pool_depth(pool, mint)

    projected_ev_raw = safe_int(direct_row.get("projected_net_ev_after_tip_raw"))
    immediate_loss_lamports = safe_int(sim_row.get("immediate_roundtrip_loss_lamports"), safe_int(row.get("immediate_roundtrip_loss_lamports")))
    immediate_loss_sol = safe_float(sim_row.get("immediate_roundtrip_loss_sol"), immediate_loss_lamports / 1_000_000_000.0)
    repeat_count_6h = repeat_counter[(text(direct_row.get("our_input_mint")) or mint, pool)]
    fresh_route_stability_score = sum(
        [
            1 if boolish(row.get("direct_entry_quote_ok")) else 0,
            1 if boolish(row.get("direct_exit_quote_ok")) else 0,
            1 if boolish(row.get("exit_non_dust")) else 0,
            1 if boolish(row.get("auto_flatten_available")) else 0,
            1 if boolish(direct_row.get("pumpfun_direct_template_ready")) else 0,
            1 if boolish(direct_row.get("pumpfun_direct_tx_buildable")) else 0,
        ]
    )
    size_ready = boolish(row.get("size_ladder_has_ready_size"))
    direct_exit_nonzero = safe_int(row.get("exit_expected_out_raw")) > 0 and safe_int(row.get("exit_min_out_raw")) > 0
    policy_pass = boolish(row.get("policy_pass"))
    top_decision = text(row.get("decision"))
    best_ready_size_sol = text(row.get("best_ready_size_sol")) or "none"
    planned_inventory_input_lamports = sol_to_lamports(safe_float(best_ready_size_sol, 0.0))
    planned_inventory_token_out_raw = 0
    if boolish(pool_depth.get("pool_state_available")) and planned_inventory_input_lamports > 0:
        planned_inventory_token_out_raw = min_out_from_expected(
            constant_product_out(safe_int(pool_depth.get("entry_reserve_in_raw")), safe_int(pool_depth.get("entry_reserve_out_raw")), planned_inventory_input_lamports),
            100,
        )
    held_inventory_raw = max(
        safe_int(direct_row.get("pumpfun_direct_input_actual_balance_raw")),
        safe_int(sim_row.get("exit_input_actual_balance_raw")),
        safe_int(sim_row.get("exit_input_balance_raw")),
    )
    projected_unwind_input_raw = safe_int(direct_row.get("our_input_raw"))
    projected_unwind_input_lte_planned_inventory = projected_unwind_input_raw > 0 and planned_inventory_token_out_raw > 0 and projected_unwind_input_raw <= planned_inventory_token_out_raw
    projected_unwind_input_lte_held_inventory = projected_unwind_input_raw > 0 and held_inventory_raw > 0 and projected_unwind_input_raw <= held_inventory_raw
    trade_fee_numerator = safe_int(direct_row.get("trade_fee_numerator"))
    trade_fee_denominator = max(1, safe_int(direct_row.get("trade_fee_denominator"), 1_000_000_000))
    mint_in = text(direct_row.get("mint_in"))
    mint_out = text(direct_row.get("mint_out"))
    if mint_in == WSOL_MINT:
        baseline_our_reserve_in = safe_int(direct_row.get("pre_target_reserve_out_raw"))
        baseline_our_reserve_out = safe_int(direct_row.get("pre_target_reserve_in_raw"))
        projected_our_reserve_in = safe_int(direct_row.get("projected_reserve_out_raw"))
        projected_our_reserve_out = safe_int(direct_row.get("projected_reserve_in_raw"))
    elif mint_out == WSOL_MINT:
        baseline_our_reserve_in = safe_int(direct_row.get("pre_target_reserve_in_raw"))
        baseline_our_reserve_out = safe_int(direct_row.get("pre_target_reserve_out_raw"))
        projected_our_reserve_in = safe_int(direct_row.get("projected_reserve_in_raw"))
        projected_our_reserve_out = safe_int(direct_row.get("projected_reserve_out_raw"))
    else:
        baseline_our_reserve_in = 0
        baseline_our_reserve_out = 0
        projected_our_reserve_in = 0
        projected_our_reserve_out = 0
    target_sized_unwind_ev_lamports = safe_int(direct_row.get("net_ev_after_tip_raw")) + hot_sender_tip_lamports + hot_sender_base_fee_lamports
    hot_sender_exit_expected_wsol_raw = constant_product_out_with_fee(
        projected_our_reserve_in,
        projected_our_reserve_out,
        planned_inventory_token_out_raw,
        trade_fee_numerator,
        trade_fee_denominator,
    )
    hot_sender_exit_min_wsol_raw = min_out_from_expected(hot_sender_exit_expected_wsol_raw, 100)
    inventory_counterfactual_pre_target_wsol_raw = constant_product_out_with_fee(
        baseline_our_reserve_in,
        baseline_our_reserve_out,
        planned_inventory_token_out_raw,
        trade_fee_numerator,
        trade_fee_denominator,
    )
    inventory_sized_unwind_ev_lamports = (
        hot_sender_exit_expected_wsol_raw
        - inventory_counterfactual_pre_target_wsol_raw
        - hot_sender_tip_lamports
        - hot_sender_base_fee_lamports
    )
    per_cycle_cost_lamports = cheap_buy_base_fee_lamports + cheap_buy_variable_cost_lamports + hot_sender_tip_lamports + hot_sender_base_fee_lamports + exit_variable_cost_lamports
    setup_cost_lamports = 0 if boolish(row.get("entry_output_account_exists")) else pump_account_setup_lamports
    size_consistent_net_after_mode_c_costs = inventory_sized_unwind_ev_lamports - per_cycle_cost_lamports - setup_cost_lamports - 10_000
    size_consistency_pass = projected_unwind_input_lte_planned_inventory or projected_unwind_input_lte_held_inventory
    size_consistent_candidate = size_consistency_pass and hot_sender_exit_expected_wsol_raw > 0 and size_consistent_net_after_mode_c_costs > 0
    portfolio_score = (
        (2_000_000 if top_decision == "R1_PROPOSAL_READY_ONE_MINT_LIVE" else 1_000_000)
        + (250_000 if policy_pass else 0)
        + (100_000 if size_ready else 0)
        + (100_000 if direct_exit_nonzero else 0)
        + (fresh_route_stability_score * 10_000)
        + min(max(size_consistent_net_after_mode_c_costs, 0), 500_000)
        + (repeat_count_6h * 2_500)
        + min(safe_int(pool_depth.get("pool_core_reserve_raw")) // 1_000_000, 50_000)
        - min(immediate_loss_lamports, 500_000)
    )
    rank_decision = top_decision
    if not size_consistency_pass:
        rank_decision = "PARK_PROJECTED_EV_SIZE_INCONSISTENT"
    elif hot_sender_exit_expected_wsol_raw <= 0 or hot_sender_exit_min_wsol_raw <= 0:
        rank_decision = "PARK_INVENTORY_SIZED_EXIT_ZERO"
    elif size_consistent_net_after_mode_c_costs <= 0:
        rank_decision = "PARK_MODE_C_NEGATIVE"

    ranked_rows.append(
        {
            "record_type": "runlsprebrinv_r1_portfolio_ranker_row",
            "candidate_id": candidate_id,
            "mint": mint,
            "pool": pool,
            "venue": text(row.get("venue")),
            "decision": rank_decision,
            "source_factory_decision": top_decision,
            "policy_pass": policy_pass,
            "exit_size_ladder_pass": size_ready,
            "direct_exit_nonzero": direct_exit_nonzero,
            "best_ready_size_sol": best_ready_size_sol,
            "immediate_roundtrip_loss_lamports": immediate_loss_lamports,
            "immediate_roundtrip_loss_sol": immediate_loss_sol,
            "expected_projected_ev_after_tip_raw": projected_ev_raw,
            "target_sized_unwind_ev_lamports": target_sized_unwind_ev_lamports,
            "inventory_sized_unwind_ev_lamports": inventory_sized_unwind_ev_lamports,
            "projected_unwind_input_raw": projected_unwind_input_raw,
            "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
            "projected_unwind_input_lte_planned_inventory": projected_unwind_input_lte_planned_inventory,
            "projected_unwind_input_lte_held_inventory": projected_unwind_input_lte_held_inventory,
            "hot_sender_exit_expected_wsol_raw": hot_sender_exit_expected_wsol_raw,
            "hot_sender_exit_min_wsol_raw": hot_sender_exit_min_wsol_raw,
            "size_consistent_net_after_mode_c_costs": size_consistent_net_after_mode_c_costs,
            "size_consistent_candidate": size_consistent_candidate,
            "pool_state_available": boolish(pool_depth.get("pool_state_available")),
            "pool_state_blocker": text(pool_depth.get("pool_state_blocker")) or "none",
            "pool_core_reserve_raw": safe_int(pool_depth.get("pool_core_reserve_raw")),
            "pool_launch_reserve_raw": safe_int(pool_depth.get("pool_launch_reserve_raw")),
            "repeat_count_6h": repeat_count_6h,
            "fresh_route_stability_score": fresh_route_stability_score,
            "pm_reserve_ok": boolish(row.get("pm_reserve_ok")),
            "pm_release_ok": boolish(row.get("pm_release_ok")),
            "entry_output_account_exists": boolish(row.get("entry_output_account_exists")),
            "projected_ev_positive_unheld_token": boolish(exec_shape_row.get("projected_ev_positive_unheld_token_rows")) or boolish(taxonomy_row.get("decision") == "BACKRUN_INVENTORY_UNWIND_INPUT_NOT_HELD"),
            "taxonomy_decision": text(taxonomy_row.get("decision")),
            "fresh_route_taxonomy_blocker": text(taxonomy_row.get("decision")) if text(taxonomy_row.get("decision")).startswith("BACKRUN_") else "",
            "portfolio_score": portfolio_score if size_consistent_candidate else min(portfolio_score, 0),
            "first_candidate_blocker": text(row.get("first_candidate_blocker")),
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

deduped_rows: dict[str, dict[str, Any]] = {}
for row in ranked_rows:
    dedupe_key = f"{text(row.get('venue'))}|{text(row.get('mint'))}|{text(row.get('pool'))}"
    current = deduped_rows.get(dedupe_key)
    if current is None:
        row["row_variant_count"] = 1
        deduped_rows[dedupe_key] = row
        continue
    current["row_variant_count"] = safe_int(current.get("row_variant_count"), 1) + 1
    current_score = safe_int(current.get("portfolio_score"))
    row_score = safe_int(row.get("portfolio_score"))
    if row_score > current_score:
        row["row_variant_count"] = safe_int(current.get("row_variant_count"), 1)
        deduped_rows[dedupe_key] = row

ranked_rows = list(deduped_rows.values())
ranked_rows.sort(
    key=lambda item: (
        0 if boolish(item.get("size_consistent_candidate")) else 1,
        -safe_int(item.get("portfolio_score")),
        -safe_int(item.get("size_consistent_net_after_mode_c_costs")),
        safe_int(item.get("immediate_roundtrip_loss_lamports")),
        text(item.get("mint")),
    )
)

for index, row in enumerate(ranked_rows, start=1):
    row["portfolio_rank"] = index
    row["selected_for_portfolio"] = index <= top_n
    row["selected_for_first_live"] = index == 1

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in ranked_rows), encoding="utf-8")

top = ranked_rows[0] if ranked_rows else {}
summary = {
    "status": "R1_PORTFOLIO_READY" if any(boolish(row.get("size_consistent_candidate")) for row in ranked_rows) else ("PARK_NO_PROPOSAL_ROWS" if not ranked_rows else "PARK_SIZE_CONSISTENT_FILTER_ZERO"),
    "rows_seen": len(factory_rows),
    "source_proposal_ready_rows": len(ranked_rows),
    "source_ata_precreate_only_ready_rows": source_decision_counts.get("R1_PROPOSAL_READY_ATA_PRECREATE_ONLY", 0),
    "source_one_mint_live_ready_rows": source_decision_counts.get("R1_PROPOSAL_READY_ONE_MINT_LIVE", 0),
    "size_consistent_candidate_rows": sum(1 for row in ranked_rows if boolish(row.get("size_consistent_candidate"))),
    "top_n": top_n,
    "source_decision_counts": dict(source_decision_counts),
    "rank_decision_counts": dict(Counter(text(row.get("decision")) or "UNKNOWN" for row in ranked_rows)),
    "top_candidate": top,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary['status']}",
    f"rows_seen={summary['rows_seen']}",
    f"source_proposal_ready_rows={summary['source_proposal_ready_rows']}",
    f"source_ata_precreate_only_ready_rows={summary['source_ata_precreate_only_ready_rows']}",
    f"source_one_mint_live_ready_rows={summary['source_one_mint_live_ready_rows']}",
    f"size_consistent_candidate_rows={summary['size_consistent_candidate_rows']}",
    f"top_n={summary['top_n']}",
    "source_decision_counts=" + json.dumps(summary["source_decision_counts"], sort_keys=True),
    "rank_decision_counts=" + json.dumps(summary["rank_decision_counts"], sort_keys=True),
    "top_candidate=" + json.dumps(top, sort_keys=True),
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "Ranked Backrun R1 portfolio candidates into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

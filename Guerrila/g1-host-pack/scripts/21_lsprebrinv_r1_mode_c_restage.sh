#!/usr/bin/env bash
# 21_lsprebrinv_r1_mode_c_restage.sh — restage Backrun inventory-band candidates under mode C only

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1MC — Mode C Restage"

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
STEP="${SHADOW_LSPREBRINV_R1MC_STEP:-21_lsprebrinv_r1_mode_c_restage}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_mode_c_restage_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_mode_c_restage_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

RESTAGE_ROWS_PATH="${SHADOW_LSPREBRINV_R1MC_RESTAGE_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_portfolio_restage_batch/runlsprebrinv_r1_portfolio_restage_batch_rows.jsonl}"
RANKER_ROWS_PATH="${SHADOW_LSPREBRINV_R1MC_RANKER_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_portfolio_ranker/runlsprebrinv_r1_portfolio_ranker_rows.jsonl}"
LIVE_AUTOPSY_JSON_PATH="${SHADOW_LSPREBRINV_R1MC_LIVE_AUTOPSY_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_live_pnl_autopsy/runlsprebrinv_r1_live_pnl_autopsy.json}"
TOP_N="${SHADOW_LSPREBRINV_R1MC_TOP_N:-10}"
SETUP_REPEAT_THRESHOLD="${SHADOW_LSPREBRINV_R1MC_SETUP_REPEAT_THRESHOLD:-2}"
MODE_C_SLIPPAGE_BUFFER_LAMPORTS="${SHADOW_LSPREBRINV_R1MC_SLIPPAGE_BUFFER_LAMPORTS:-10000}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_file_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$RESTAGE_ROWS_PATH" ]] || RESTAGE_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_portfolio_restage_batch_rows.jsonl)"
[[ -f "$RANKER_ROWS_PATH" ]] || RANKER_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_portfolio_ranker_rows.jsonl)"
[[ -f "$LIVE_AUTOPSY_JSON_PATH" ]] || LIVE_AUTOPSY_JSON_PATH="$(latest_file_for_step runlsprebrinv_r1_live_pnl_autopsy.json)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$RESTAGE_ROWS_PATH" "$RANKER_ROWS_PATH" "$LIVE_AUTOPSY_JSON_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$TOP_N" "$SETUP_REPEAT_THRESHOLD" "$MODE_C_SLIPPAGE_BUFFER_LAMPORTS" <<'PY'
from __future__ import annotations

import json
import math
import sys
from collections import Counter
from pathlib import Path
from typing import Any

(
    restage_rows_arg,
    ranker_rows_arg,
    autopsy_json_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    top_n_arg,
    setup_repeat_threshold_arg,
    slippage_buffer_arg,
) = sys.argv[1:11]

restage_rows_path = Path(restage_rows_arg) if restage_rows_arg else Path("")
ranker_rows_path = Path(ranker_rows_arg) if ranker_rows_arg else Path("")
autopsy_json_path = Path(autopsy_json_arg) if autopsy_json_arg else Path("")
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
top_n = max(1, int(float(top_n_arg or "10")))
setup_repeat_threshold = max(1, int(float(setup_repeat_threshold_arg or "2")))
slippage_buffer_lamports = max(0, int(float(slippage_buffer_arg or "10000")))
default_hot_sender_tip_account = "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn"


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


def min_out_from_expected(expected_out: int, slippage_bps: int) -> int:
    if expected_out <= 0:
        return 0
    bounded = min(max(slippage_bps, 0), 10_000)
    return max(0, (expected_out * (10_000 - bounded)) // 10_000)


def constant_product_out_with_fee(reserve_in: int, reserve_out: int, amount_in: int, fee_num: int, fee_den: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0 or fee_den <= 0:
        return 0
    fee = ((amount_in * fee_num) + fee_den - 1) // fee_den
    amount_less_fee = max(0, amount_in - fee)
    denominator = reserve_in + amount_less_fee
    if denominator <= 0:
        return 0
    return (amount_less_fee * reserve_out) // denominator


autopsy = load_json(autopsy_json_path)
hot_sender_tip_lamports = safe_int(autopsy.get("sell_sender_tip_lamports"))
hot_sender_base_fee_lamports = safe_int(autopsy.get("sell_base_fee_lamports"))
exit_variable_cost_lamports = safe_int(autopsy.get("exit_pumpfun_fee_lamports")) + safe_int(autopsy.get("exit_execution_shortfall_lamports"))
cheap_buy_base_fee_lamports = safe_int(autopsy.get("buy_base_fee_lamports"))
cheap_buy_variable_cost_lamports = safe_int(autopsy.get("entry_pumpfun_fee_lamports")) + safe_int(autopsy.get("entry_execution_shortfall_lamports"))
pump_account_setup_lamports = safe_int(autopsy.get("pump_account_setup_lamports"), safe_int(autopsy.get("account_creation_rent_lamports")))
pump_account_setup_recoverable = boolish(autopsy.get("pump_account_setup_recoverable"))

restage_rows = load_jsonl(restage_rows_path)[:top_n]
ranker_rows = load_jsonl(ranker_rows_path)
ranker_by_key: dict[tuple[str, str], dict[str, Any]] = {
    (text(row.get("mint")), text(row.get("pool"))): row for row in ranker_rows
}

manifest = {
    "restage_rows_path": str(restage_rows_path),
    "ranker_rows_path": str(ranker_rows_path),
    "live_autopsy_json_path": str(autopsy_json_path),
    "top_n": top_n,
    "setup_repeat_threshold": setup_repeat_threshold,
    "slippage_buffer_lamports": slippage_buffer_lamports,
    "mode_c_cost_constants": {
        "hot_sender_tip_lamports": hot_sender_tip_lamports,
        "hot_sender_base_fee_lamports": hot_sender_base_fee_lamports,
        "exit_variable_cost_lamports": exit_variable_cost_lamports,
        "cheap_buy_base_fee_lamports": cheap_buy_base_fee_lamports,
        "cheap_buy_variable_cost_lamports": cheap_buy_variable_cost_lamports,
        "pump_account_setup_lamports": pump_account_setup_lamports,
        "pump_account_setup_recoverable": pump_account_setup_recoverable,
    },
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

decision_counts: Counter[str] = Counter()
rows: list[dict[str, Any]] = []

for selected in restage_rows:
    candidate_dir = Path(text(selected.get("candidate_dir")))
    mint = text(selected.get("mint"))
    pool = text(selected.get("pool"))
    ranker_row = ranker_by_key.get((mint, pool), {})
    proposal_doc = load_json(candidate_dir / "21_lsprebrinv_r1_one_mint_proposal" / "r1_one_mint_proposal.json")
    top_candidate = proposal_doc.get("top_candidate") if isinstance(proposal_doc.get("top_candidate"), dict) else {}
    direct_row = first_row(candidate_dir / "21_lsprebrpf_pumpfun_amm_direct_route_probe" / "runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl")
    sim_row = first_row(candidate_dir / "21_lsprebrinv_tiny_inventory_band_sim" / "runlsprebrinv_tiny_inventory_band_sim_rows.jsonl")
    size_summary = load_json(candidate_dir / "21_lsprebrinv_r1_exit_quote_size_ladder" / "runlsprebrinv_r1_exit_quote_size_ladder_summary.json")
    size_rows = load_jsonl(candidate_dir / "21_lsprebrinv_r1_exit_quote_size_ladder" / "runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl")

    account_precreated = boolish(top_candidate.get("entry_output_account_exists"))
    account_setup_lamports = 0 if account_precreated else pump_account_setup_lamports
    account_setup_recoverable = False if account_setup_lamports > 0 else pump_account_setup_recoverable
    account_setup_amortization_lamports = 0 if account_precreated else math.ceil(account_setup_lamports / max(safe_int(ranker_row.get("repeat_count_6h")), 1))

    cheap_buy_quote_ok = boolish(sim_row.get("direct_entry_quote_ok")) or boolish(top_candidate.get("pumpfun_direct_entry_quote_ok")) or (boolish(direct_row.get("pumpfun_direct_route_exists")) and boolish(direct_row.get("pumpfun_direct_quote_available")) and boolish(direct_row.get("pumpfun_direct_min_out_positive")))
    cheap_buy_expected_out = safe_int(sim_row.get("entry_expected_out_raw"), safe_int(direct_row.get("projected_expected_out_raw")))
    hot_sender_exit_quote_ok = (
        boolish(direct_row.get("pumpfun_direct_route_exists"))
        and boolish(direct_row.get("pumpfun_direct_quote_available"))
        and boolish(direct_row.get("pumpfun_direct_min_out_positive"))
        and boolish(direct_row.get("pumpfun_direct_template_ready"))
        and boolish(direct_row.get("pumpfun_direct_tx_buildable"))
    )
    hot_sender_tip_account = text(top_candidate.get("sender_tip_account")) or text(proposal_doc.get("sender_tip_account")) or default_hot_sender_tip_account
    hot_sender_tip_account_allowed = bool(hot_sender_tip_account)
    best_ready_size_sol = text(top_candidate.get("size_ladder_best_ready_size_sol")) or text(size_summary.get("best_ready_size_sol")) or text(ranker_row.get("best_ready_size_sol")) or "none"
    best_ready_size = safe_float(best_ready_size_sol, 0.0)
    planned_inventory_input_lamports = max(0, int(round(best_ready_size * 1_000_000_000.0)))
    exact_size_row = {}
    for size_row in size_rows:
        if abs(safe_float(size_row.get("size_sol")) - best_ready_size) < 1e-12:
            exact_size_row = size_row
            break
    if not exact_size_row and size_rows:
        exact_size_row = size_rows[0]
    planned_inventory_token_out_raw = safe_int(exact_size_row.get("entry_min_out_raw"), safe_int(exact_size_row.get("entry_expected_out_raw")))
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
    if mint_in == "So11111111111111111111111111111111111111112":
        baseline_our_reserve_in = safe_int(direct_row.get("pre_target_reserve_out_raw"))
        baseline_our_reserve_out = safe_int(direct_row.get("pre_target_reserve_in_raw"))
        projected_our_reserve_in = safe_int(direct_row.get("projected_reserve_out_raw"))
        projected_our_reserve_out = safe_int(direct_row.get("projected_reserve_in_raw"))
    elif mint_out == "So11111111111111111111111111111111111111112":
        baseline_our_reserve_in = safe_int(direct_row.get("pre_target_reserve_in_raw"))
        baseline_our_reserve_out = safe_int(direct_row.get("pre_target_reserve_out_raw"))
        projected_our_reserve_in = safe_int(direct_row.get("projected_reserve_in_raw"))
        projected_our_reserve_out = safe_int(direct_row.get("projected_reserve_out_raw"))
    else:
        baseline_our_reserve_in = 0
        baseline_our_reserve_out = 0
        projected_our_reserve_in = 0
        projected_our_reserve_out = 0

    projected_net_after_hot_fixed_lamports = safe_int(ranker_row.get("expected_projected_ev_after_tip_raw"))
    if projected_net_after_hot_fixed_lamports <= 0:
        projected_net_after_hot_fixed_lamports = safe_int(direct_row.get("net_ev_after_tip_raw"))
    target_sized_unwind_ev_lamports = projected_net_after_hot_fixed_lamports + hot_sender_tip_lamports + hot_sender_base_fee_lamports
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
    hot_sender_exit_cost_lamports = hot_sender_tip_lamports + hot_sender_base_fee_lamports + exit_variable_cost_lamports
    cheap_buy_cost_lamports = cheap_buy_base_fee_lamports + cheap_buy_variable_cost_lamports
    per_cycle_cost_lamports = cheap_buy_cost_lamports + hot_sender_exit_cost_lamports
    size_consistent_net_after_mode_c_costs = inventory_sized_unwind_ev_lamports - per_cycle_cost_lamports - account_setup_lamports - slippage_buffer_lamports
    setup_amortized_ev_lamports = inventory_sized_unwind_ev_lamports - per_cycle_cost_lamports - account_setup_amortization_lamports - slippage_buffer_lamports
    repeat_count_6h = safe_int(ranker_row.get("repeat_count_6h"))
    policy_pass = boolish(top_candidate.get("mint_policy_pass", top_candidate.get("policy_pass", ranker_row.get("policy_pass"))))
    size_consistency_pass = projected_unwind_input_lte_planned_inventory or projected_unwind_input_lte_held_inventory

    if not policy_pass:
        decision = "PARK_POLICY"
        first_blocker = text(top_candidate.get("mint_policy_blocker")) or "policy_fail"
    elif not cheap_buy_quote_ok:
        decision = "PARK_NO_CHEAP_BUY"
        first_blocker = "cheap_buy_quote_missing"
    elif not hot_sender_exit_quote_ok:
        decision = "PARK_NO_HOT_EXIT"
        first_blocker = "hot_sender_exit_quote_missing"
    elif not hot_sender_tip_account_allowed:
        decision = "PARK_NO_HOT_EXIT"
        first_blocker = "hot_sender_tip_account_missing_or_disallowed"
    elif not size_consistency_pass:
        decision = "PARK_PROJECTED_EV_SIZE_INCONSISTENT"
        first_blocker = "projected_unwind_input_exceeds_available_inventory"
    elif hot_sender_exit_expected_wsol_raw <= 0 or hot_sender_exit_min_wsol_raw <= 0:
        decision = "PARK_INVENTORY_SIZED_EXIT_ZERO"
        first_blocker = "inventory_sized_exit_quote_zero"
    elif account_precreated and size_consistent_net_after_mode_c_costs > 0:
        decision = "MODE_C_READY_EXISTING_ACCOUNT"
        first_blocker = "none"
    elif (not account_precreated) and repeat_count_6h >= setup_repeat_threshold and setup_amortized_ev_lamports > 0:
        decision = "MODE_C_READY_PRECREATE_REVIEW"
        first_blocker = "precreate_review_required"
    elif (not account_precreated) and (repeat_count_6h < setup_repeat_threshold or setup_amortized_ev_lamports <= 0):
        decision = "PARK_SETUP_NOT_AMORTIZABLE"
        first_blocker = "setup_not_amortized"
    else:
        decision = "PARK_MODE_C_NEGATIVE"
        first_blocker = "size_consistent_net_after_mode_c_nonpositive"

    row = {
        "record_type": "runlsprebrinv_r1_mode_c_restage_row",
        "portfolio_rank": safe_int(selected.get("portfolio_rank")),
        "candidate_id": text(selected.get("candidate_id")) or text(ranker_row.get("candidate_id")) or text(direct_row.get("candidate_id")),
        "mint": mint,
        "pool": pool,
        "account_precreated": account_precreated,
        "account_setup_lamports": account_setup_lamports,
        "account_setup_recoverable": account_setup_recoverable,
        "cheap_buy_quote_ok": cheap_buy_quote_ok,
        "cheap_buy_expected_out": cheap_buy_expected_out,
        "hot_sender_exit_quote_ok": hot_sender_exit_quote_ok,
        "hot_sender_tip_lamports": hot_sender_tip_lamports,
        "hot_sender_tip_account": hot_sender_tip_account or None,
        "per_cycle_cost_lamports": per_cycle_cost_lamports,
        "cheap_buy_cost_lamports": cheap_buy_cost_lamports,
        "hot_sender_exit_cost_lamports": hot_sender_exit_cost_lamports,
        "target_sized_unwind_ev_lamports": target_sized_unwind_ev_lamports,
        "inventory_sized_unwind_ev_lamports": inventory_sized_unwind_ev_lamports,
        "projected_unwind_input_raw": projected_unwind_input_raw,
        "planned_inventory_input_lamports": planned_inventory_input_lamports,
        "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
        "projected_unwind_input_lte_planned_inventory": projected_unwind_input_lte_planned_inventory,
        "projected_unwind_input_lte_held_inventory": projected_unwind_input_lte_held_inventory,
        "hot_sender_exit_expected_wsol_raw": hot_sender_exit_expected_wsol_raw,
        "hot_sender_exit_min_wsol_raw": hot_sender_exit_min_wsol_raw,
        "projected_net_after_hot_fixed_lamports": projected_net_after_hot_fixed_lamports,
        "size_consistent_net_after_mode_c_costs": size_consistent_net_after_mode_c_costs,
        "repeat_count_6h": repeat_count_6h,
        "setup_amortization_lamports": account_setup_amortization_lamports,
        "setup_amortized_ev": setup_amortized_ev_lamports,
        "best_ready_size_sol": best_ready_size_sol,
        "policy_pass": policy_pass,
        "decision": decision,
        "first_blocker": first_blocker,
        "candidate_dir": str(candidate_dir),
    }
    rows.append(row)
    decision_counts[decision] += 1

rows.sort(
    key=lambda item: (
        0 if text(item.get("decision")) == "MODE_C_READY_EXISTING_ACCOUNT" else 1 if text(item.get("decision")) == "MODE_C_READY_PRECREATE_REVIEW" else 2,
        -safe_int(item.get("size_consistent_net_after_mode_c_costs")),
        -safe_int(item.get("setup_amortized_ev")),
        safe_int(item.get("portfolio_rank")),
    )
)
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")

best_existing = next((row for row in rows if text(row.get("decision")) == "MODE_C_READY_EXISTING_ACCOUNT"), {})
best_precreate = next((row for row in rows if text(row.get("decision")) == "MODE_C_READY_PRECREATE_REVIEW"), {})
ready_existing_count = sum(1 for row in rows if text(row.get("decision")) == "MODE_C_READY_EXISTING_ACCOUNT")
ready_precreate_count = sum(1 for row in rows if text(row.get("decision")) == "MODE_C_READY_PRECREATE_REVIEW")
summary = {
    "status": (
        "MODE_C_ROWS_READY"
        if (ready_existing_count + ready_precreate_count) > 0
        else ("PARK_SIZE_CONSISTENT_FILTER_ZERO" if rows else "PARK_NO_ROWS")
    ),
    "portfolio_rows_seen": len(restage_rows),
    "mode_c_ready_existing_account_rows": ready_existing_count,
    "mode_c_ready_precreate_review_rows": ready_precreate_count,
    "decision_counts": dict(decision_counts),
    "best_existing_account_mint": text(best_existing.get("mint")),
    "best_precreate_review_mint": text(best_precreate.get("mint")),
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"portfolio_rows_seen={summary['portfolio_rows_seen']}",
            f"mode_c_ready_existing_account_rows={summary['mode_c_ready_existing_account_rows']}",
            f"mode_c_ready_precreate_review_rows={summary['mode_c_ready_precreate_review_rows']}",
            "decision_counts=" + json.dumps(summary["decision_counts"], sort_keys=True),
            f"best_existing_account_mint={summary['best_existing_account_mint']}",
            f"best_precreate_review_mint={summary['best_precreate_review_mint']}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Completed Backrun mode C restage in $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

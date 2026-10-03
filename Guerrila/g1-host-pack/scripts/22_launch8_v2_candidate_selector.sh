#!/usr/bin/env bash
# 22_launch8_v2_candidate_selector.sh — Launch Continuation V2 selector using the looser trigger profile over existing launch pressure/gate/sim rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch8 — V2 Candidate Selector"

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
STEP="${SHADOW_LAUNCH8_STEP:-22_launch8_v2_candidate_selector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch8_v2_candidate_selector_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch8_v2_candidate_selector_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PRESSURE_ROWS_PATH="${SHADOW_LAUNCH8_PRESSURE_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
GATE_ROWS_PATH="${SHADOW_LAUNCH8_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCH8_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
PM_ROWS_PATH="${SHADOW_LAUNCH8_PM_ROWS_PATH:-$RESULTS_ROOT/22_launchpm_v2_pm_readiness_autopsy/launchpm_v2_pm_readiness_autopsy_rows.jsonl}"

TRIGGER_MODE="${SHADOW_LAUNCH8_TRIGGER_MODE:-B}"
REQUIRE_TARGET_UNLANDED="${SHADOW_LAUNCH8_REQUIRE_TARGET_UNLANDED:-true}"
REQUIRE_TARGET_BYTES="${SHADOW_LAUNCH8_REQUIRE_TARGET_BYTES:-true}"
REQUIRE_TARGET_BLOCKHASH_VALID="${SHADOW_LAUNCH8_REQUIRE_TARGET_BLOCKHASH_VALID:-true}"
MAX_CASH_AT_RISK_SOL="${SHADOW_LAUNCH8_MAX_CASH_AT_RISK_SOL:-0.006}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PRESSURE_ROWS_PATH" ]] || PRESSURE_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"
[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"
[[ -f "$PM_ROWS_PATH" ]] || PM_ROWS_PATH="$(latest_rows_for_step launchpm_v2_pm_readiness_autopsy_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PRESSURE_ROWS_PATH" "$GATE_ROWS_PATH" "$SIM_ROWS_PATH" "$PM_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$TRIGGER_MODE" "$REQUIRE_TARGET_UNLANDED" "$REQUIRE_TARGET_BYTES" "$REQUIRE_TARGET_BLOCKHASH_VALID" "$MAX_CASH_AT_RISK_SOL" <<'PY'
from __future__ import annotations

import json
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Any

pressure_rows_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
gate_rows_path = Path(sys.argv[2]) if sys.argv[2] else Path("")
sim_rows_path = Path(sys.argv[3]) if sys.argv[3] else Path("")
pm_rows_path = Path(sys.argv[4]) if sys.argv[4] else Path("")
rows_out_path = Path(sys.argv[5])
summary_json_path = Path(sys.argv[6])
summary_txt_path = Path(sys.argv[7])
trigger_mode = str(sys.argv[8] or "B").strip().upper()
require_target_unlanded = str(sys.argv[9]).strip().lower() in {"1", "true", "yes", "y", "on"}
require_target_bytes = str(sys.argv[10]).strip().lower() in {"1", "true", "yes", "y", "on"}
require_target_blockhash_valid = str(sys.argv[11]).strip().lower() in {"1", "true", "yes", "y", "on"}
max_cash_at_risk_sol = float(sys.argv[12] or 0.006)


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


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def sol_to_lamports(value: Any) -> int:
    try:
        if value in (None, ""):
            return 0
        return max(0, int(round(float(value) * 1_000_000_000.0)))
    except (TypeError, ValueError):
        return 0


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if str(path) in {"", "."} or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def first_nonempty(*values: Any) -> str:
    for value in values:
        if text(value):
            return text(value)
    return ""


def first_positive_int(*values: Any) -> int | None:
    for value in values:
        parsed = safe_int(value)
        if parsed > 0:
            return parsed
    return None


def derive_target_status(tx_bytes_available: bool, already_landed: bool) -> str:
    if already_landed:
        return "landed"
    if tx_bytes_available:
        return "not_landed"
    return "unknown"


pressure_rows = load_jsonl(pressure_rows_path)
gate_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(gate_rows_path) if text(row.get("launch_group_id"))}
sim_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(sim_rows_path) if text(row.get("launch_group_id"))}
pm_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(pm_rows_path) if text(row.get("launch_group_id"))}


def trigger_passes(row: dict[str, Any]) -> tuple[bool, str]:
    pair_class = text(row.get("pair_class"))
    if pair_class != "core_launch_pair":
        return False, "unsupported_pair_class"
    if boolish(row.get("creator_sell_detected")):
        return False, "creator_sell_detected"
    if boolish(row.get("insider_cluster_sell_seen")):
        return False, "insider_cluster_sell_seen"
    if safe_int(row.get("buy_count_1s")) < 1:
        return False, "buy_count_1s_below_threshold"
    if safe_int(row.get("unique_buyers_2s")) < 1:
        return False, "unique_buyers_2s_below_threshold"
    if safe_float(row.get("net_buy_pressure")) <= 0:
        return False, "sell_pressure_dominant"
    if boolish(row.get("first_sell_seen")):
        return False, "sell_seen_before_entry"

    mode = trigger_mode or "B"
    if mode == "A":
        return True, "none"
    if mode == "B":
        return True, "none"
    if mode == "C" and safe_int(row.get("buy_count_2s")) >= 2:
        return True, "none"
    if mode == "D" and safe_int(row.get("buy_count_5s")) >= 3:
        return True, "none"
    if mode == "E" and safe_int(row.get("unique_buyers_5s")) >= 2:
        return True, "none"
    if mode in {"C", "D", "E"}:
        mapping = {
            "C": "buy_count_2s_below_threshold",
            "D": "buy_count_5s_below_threshold",
            "E": "unique_buyers_5s_below_threshold",
        }
        return False, mapping.get(mode, "trigger_mode_not_satisfied")
    return False, "unsupported_trigger_mode"


out_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()
blocker_counts: Counter[str] = Counter()

for pressure_row in pressure_rows:
    group_id = text(pressure_row.get("launch_group_id"))
    gate_row = gate_by_group.get(group_id, {})
    sim_row = sim_by_group.get(group_id, {})
    pm_row = pm_by_group.get(group_id, {})

    trigger_ready, trigger_blocker = trigger_passes(pressure_row)
    decision = "LAUNCH_V2_CANDIDATE_READY"
    blocker = "none"

    target_not_landed = boolish(pressure_row.get("target_not_landed_at_first_check"))
    target_already_landed = boolish(pressure_row.get("target_already_landed_at_first_check"))
    target_tx_bytes_available = boolish(pressure_row.get("target_tx_bytes_available"))
    target_blockhash_valid = boolish(pressure_row.get("target_blockhash_valid"))
    target_seen_preprocessed_ns = first_positive_int(
        pressure_row.get("target_seen_preprocessed_ns"),
        pressure_row.get("latest_seen_preprocessed_ns"),
        gate_row.get("target_seen_preprocessed_ns"),
        gate_row.get("latest_seen_preprocessed_ns"),
        sim_row.get("target_seen_preprocessed_ns"),
        sim_row.get("latest_seen_preprocessed_ns"),
        pm_row.get("target_seen_preprocessed_ns"),
        pm_row.get("latest_seen_preprocessed_ns"),
    )
    latest_seen_preprocessed_ns = first_positive_int(
        pressure_row.get("latest_seen_preprocessed_ns"),
        pressure_row.get("target_seen_preprocessed_ns"),
        gate_row.get("latest_seen_preprocessed_ns"),
        sim_row.get("latest_seen_preprocessed_ns"),
        pm_row.get("latest_seen_preprocessed_ns"),
    )
    first_seen_ns = first_positive_int(
        pressure_row.get("first_seen_ns"),
        gate_row.get("first_seen_ns"),
        sim_row.get("first_seen_ns"),
        pm_row.get("first_seen_ns"),
    )
    target_seen_host_mono_ns = first_positive_int(
        pressure_row.get("target_seen_host_mono_ns"),
        gate_row.get("target_seen_host_mono_ns"),
        sim_row.get("target_seen_host_mono_ns"),
        pm_row.get("target_seen_host_mono_ns"),
    )
    target_seen_unix_ns = first_positive_int(
        pressure_row.get("target_seen_unix_ns"),
        gate_row.get("target_seen_unix_ns"),
        sim_row.get("target_seen_unix_ns"),
        pm_row.get("target_seen_unix_ns"),
        latest_seen_preprocessed_ns,
    )
    target_source_slice_id = first_nonempty(
        pressure_row.get("target_source_slice_id"),
        gate_row.get("target_source_slice_id"),
        sim_row.get("target_source_slice_id"),
        pm_row.get("target_source_slice_id"),
    )
    target_source_row_id = first_nonempty(
        pressure_row.get("target_source_row_id"),
        gate_row.get("target_source_row_id"),
        sim_row.get("target_source_row_id"),
        pm_row.get("target_source_row_id"),
    )
    selected_row_source = first_nonempty(
        pressure_row.get("selected_row_source"),
        gate_row.get("selected_row_source"),
        sim_row.get("selected_row_source"),
        pm_row.get("selected_row_source"),
        "live_current_window",
    )
    target_blockhash = first_nonempty(
        pressure_row.get("target_blockhash"),
        gate_row.get("target_blockhash"),
        sim_row.get("target_blockhash"),
        pm_row.get("target_blockhash"),
    )
    inline_account_required = boolish(pm_row.get("inline_account_required"))
    inline_account_creation_allowed = boolish(pm_row.get("inline_account_creation_allowed"))
    inline_account_rent_lamports = safe_int(sim_row.get("ata_rent_lamports"))
    inline_account_closeable = inline_account_required and inline_account_creation_allowed and text(pressure_row.get("pair_class")) == "core_launch_pair"
    close_after_flatten_required = inline_account_closeable
    recoverable_rent_lamports = inline_account_rent_lamports if inline_account_closeable else 0
    unrecoverable_rent_lamports = inline_account_rent_lamports if inline_account_required and not inline_account_closeable else 0
    max_spend_sol = safe_float(pm_row.get("max_spend_sol") or sim_row.get("max_spend_sol"))
    max_loss_sol = safe_float(pm_row.get("max_loss_sol") or sim_row.get("max_loss_sol"))
    attempt_loss_lamports = max(0, safe_int(sim_row.get("immediate_roundtrip_loss_lamports"))) + unrecoverable_rent_lamports
    max_loss_budget_valid_v2 = attempt_loss_lamports <= sol_to_lamports(max_loss_sol)
    max_cash_at_risk_lamports = (
        safe_int(pm_row.get("required_inventory_lamports"))
        + safe_int(pm_row.get("required_fee_lamports"))
        + safe_int(pm_row.get("required_tip_lamports"))
        + safe_int(pm_row.get("required_rent_lamports"))
    )
    max_cash_at_risk_valid = max_cash_at_risk_lamports <= sol_to_lamports(max_cash_at_risk_sol)

    if not trigger_ready:
        decision = "PARK_V2_PRESSURE"
        blocker = trigger_blocker
    elif not target_seen_preprocessed_ns:
        decision = "PARK_V2_MISSING_FRESHNESS_LINEAGE"
        blocker = "missing_freshness_lineage"
    elif require_target_bytes and not target_tx_bytes_available:
        decision = "PARK_V2_TARGET_BYTES_MISSING"
        blocker = "target_tx_bytes_missing"
    elif require_target_unlanded and (not target_not_landed or target_already_landed):
        decision = "PARK_V2_TARGET_LANDED"
        blocker = "target_already_landed"
    elif require_target_blockhash_valid and not target_blockhash_valid:
        decision = "PARK_V2_BLOCKHASH_INVALID"
        blocker = "target_blockhash_invalid"
    elif not boolish(gate_row.get("mint_policy_pass")):
        decision = "PARK_V2_POLICY"
        blocker = text(gate_row.get("mint_policy_blocker")) or "mint_policy_failed"
    elif not boolish(gate_row.get("r1_exit_nonzero_size_band_ready")):
        decision = "PARK_V2_EXIT_ZERO"
        blocker = text(gate_row.get("first_gate_blocker")) or "exit_quote_expected_out_zero"
    elif safe_int(gate_row.get("exit_expected_out_raw")) <= 0 or safe_int(gate_row.get("exit_min_out_raw")) <= 0:
        decision = "PARK_V2_EXIT_ZERO"
        blocker = "exit_quote_expected_out_zero"
    elif not boolish(gate_row.get("exit_non_dust")):
        decision = "PARK_V2_EXIT_DUST"
        blocker = "exit_quote_dust"
    elif not boolish(gate_row.get("direct_entry_quote_ok")):
        decision = "PARK_V2_ENTRY_QUOTE"
        blocker = "entry_quote_not_ready"
    elif not boolish(gate_row.get("direct_exit_quote_ok")) or not boolish(gate_row.get("auto_flatten_available")):
        decision = "PARK_V2_AUTO_FLATTEN"
        blocker = "auto_flatten_unavailable"
    elif not (
        boolish(pm_row.get("pm_reserve_input_ok"))
        and boolish(pm_row.get("pm_reserve_fee_ok"))
        and boolish(pm_row.get("pm_reserve_tip_ok"))
        and boolish(pm_row.get("pm_reserve_rent_ok"))
        and boolish(pm_row.get("pm_release_on_no_send_ok"))
        and boolish(pm_row.get("pm_account_surface_ok"))
    ):
        decision = "PARK_V2_PM"
        blocker = text(pm_row.get("first_pm_blocker")) or "pm_reserve_failed"
    elif not max_cash_at_risk_valid:
        decision = "PARK_V2_CASH_AT_RISK_EXCEEDS_CAP"
        blocker = "max_cash_at_risk_exceeds_cap"
    elif inline_account_required and not inline_account_closeable:
        decision = "PARK_V2_RENT_NOT_RECOVERABLE"
        blocker = "inline_account_rent_not_recoverable"
    elif not max_loss_budget_valid_v2:
        decision = "PARK_V2_COST_TRUE_LOSS"
        blocker = "attempt_loss_exceeds_cap"
    elif inline_account_required and inline_account_creation_allowed:
        decision = "LAUNCH_V2_READY_INLINE_ACCOUNT_RECOVERABLE"
        blocker = "entry_output_account_missing"
    else:
        decision = "LAUNCH_V2_CANDIDATE_READY"
        blocker = "none"

    out_row = {
        "record_type": "launch8_v2_candidate_selector_row",
        "launch_group_id": group_id,
        "candidate_id": text(pressure_row.get("candidate_id")) or text(gate_row.get("candidate_id")) or text(sim_row.get("candidate_id")) or text(pm_row.get("candidate_id")),
        "trigger_mode": trigger_mode,
        "venue": text(pressure_row.get("venue")) or text(gate_row.get("venue")) or text(sim_row.get("venue")),
        "pool": text(pressure_row.get("pool")) or text(gate_row.get("pool")) or text(sim_row.get("pool")),
        "core_mint": text(pressure_row.get("core_mint")) or text(gate_row.get("core_mint")) or text(sim_row.get("core_mint")),
        "launch_mint": text(pressure_row.get("launch_mint")) or text(gate_row.get("launch_mint")) or text(sim_row.get("launch_mint")),
        "latest_signature": text(pressure_row.get("latest_signature")) or text(gate_row.get("latest_signature")) or text(sim_row.get("latest_signature")),
        "target_instruction_index": safe_int(pressure_row.get("target_instruction_index") or gate_row.get("target_instruction_index") or sim_row.get("target_instruction_index")),
        "target_tx_bytes_available": target_tx_bytes_available,
        "target_blockhash": target_blockhash,
        "target_blockhash_valid": target_blockhash_valid,
        "target_already_landed_at_first_check": target_already_landed,
        "target_not_landed_at_first_check": target_not_landed,
        "target_blockhash_valid_at_capture": boolish(pressure_row.get("target_blockhash_valid_at_capture")) or target_blockhash_valid,
        "target_status_at_capture": first_nonempty(
            pressure_row.get("target_status_at_capture"),
            derive_target_status(target_tx_bytes_available, target_already_landed),
        ),
        "target_blockhash_valid_at_selector": target_blockhash_valid,
        "target_status_at_selector": derive_target_status(target_tx_bytes_available, target_already_landed),
        "target_seen_preprocessed_ns": target_seen_preprocessed_ns,
        "latest_seen_preprocessed_ns": latest_seen_preprocessed_ns,
        "first_seen_ns": first_seen_ns,
        "target_seen_host_mono_ns": target_seen_host_mono_ns,
        "target_seen_unix_ns": target_seen_unix_ns,
        "target_source_slice_id": target_source_slice_id,
        "target_source_row_id": target_source_row_id,
        "selected_row_source": selected_row_source,
        "pair_class": text(pressure_row.get("pair_class")),
        "buy_count_1s": safe_int(pressure_row.get("buy_count_1s")),
        "buy_count_2s": safe_int(pressure_row.get("buy_count_2s")),
        "buy_count_5s": safe_int(pressure_row.get("buy_count_5s")),
        "unique_buyers_2s": safe_int(pressure_row.get("unique_buyers_2s")),
        "unique_buyers_5s": safe_int(pressure_row.get("unique_buyers_5s")),
        "net_buy_pressure": safe_float(pressure_row.get("net_buy_pressure")),
        "buy_pressure_score": safe_float(pressure_row.get("buy_pressure_score")),
        "same_direction_followthrough": safe_float(pressure_row.get("same_direction_followthrough")),
        "first_sell_seen": boolish(pressure_row.get("first_sell_seen")),
        "creator_sell_detected": boolish(pressure_row.get("creator_sell_detected")),
        "insider_cluster_sell_seen": boolish(pressure_row.get("insider_cluster_sell_seen")),
        "mint_policy_pass": boolish(gate_row.get("mint_policy_pass")),
        "mint_policy_blocker": text(gate_row.get("mint_policy_blocker")),
        "direct_entry_quote_ok": boolish(gate_row.get("direct_entry_quote_ok")),
        "direct_exit_quote_ok": boolish(gate_row.get("direct_exit_quote_ok")),
        "entry_expected_out_raw": safe_int(gate_row.get("entry_expected_out_raw")),
        "entry_min_out_raw": safe_int(gate_row.get("entry_min_out_raw")),
        "exit_expected_out_raw": safe_int(gate_row.get("exit_expected_out_raw")),
        "exit_min_out_raw": safe_int(gate_row.get("exit_min_out_raw")),
        "exit_non_dust": boolish(gate_row.get("exit_non_dust")),
        "size_ladder_exact_decision": text(gate_row.get("size_ladder_exact_decision")),
        "size_ladder_best_ready_size_sol": text(gate_row.get("size_ladder_best_ready_size_sol")) or "none",
        "r1_exit_nonzero_size_band_ready": boolish(gate_row.get("r1_exit_nonzero_size_band_ready")),
        "auto_flatten_available": boolish(gate_row.get("auto_flatten_available")),
        "max_spend_sol": max_spend_sol,
        "max_loss_sol": max_loss_sol,
        "entry_output_account_exists": boolish(pm_row.get("output_account_ready")) or boolish(sim_row.get("entry_output_account_exists")),
        "entry_output_account_ready": boolish(pm_row.get("output_account_ready")) or boolish(sim_row.get("entry_output_account_ready")),
        "ata_precreate_required": boolish(pm_row.get("inline_account_required")) and not boolish(pm_row.get("inline_account_creation_allowed")),
        "inline_account_required": boolish(pm_row.get("inline_account_required")),
        "inline_account_creation_allowed": boolish(pm_row.get("inline_account_creation_allowed")),
        "inline_account_rent_lamports": inline_account_rent_lamports,
        "inline_account_closeable": inline_account_closeable,
        "close_after_flatten_required": close_after_flatten_required,
        "recoverable_rent_lamports": recoverable_rent_lamports,
        "unrecoverable_rent_lamports": unrecoverable_rent_lamports,
        "max_cash_at_risk_lamports": max_cash_at_risk_lamports,
        "max_cash_at_risk_valid": max_cash_at_risk_valid,
        "attempt_loss_lamports": attempt_loss_lamports,
        "max_loss_budget_valid_v2": max_loss_budget_valid_v2,
        "pm_reserve_inventory_ok": boolish(pm_row.get("pm_reserve_input_ok")),
        "pm_reserve_fee_ok": boolish(pm_row.get("pm_reserve_fee_ok")),
        "pm_reserve_tip_ok": boolish(pm_row.get("pm_reserve_tip_ok")),
        "pm_reserve_rent_ok": boolish(pm_row.get("pm_reserve_rent_ok")),
        "pm_release_ok": boolish(pm_row.get("pm_release_on_no_send_ok")),
        "pm_account_surface_ok": boolish(pm_row.get("pm_account_surface_ok")),
        "pm_first_blocker": text(pm_row.get("first_pm_blocker")),
        "sim_decision": text(sim_row.get("sim_decision")),
        "selector_ready_ns": time.time_ns(),
        "selector_decision": decision,
        "first_v2_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    out_rows.append(out_row)
    decision_counts[decision] += 1
    blocker_counts[blocker] += 1

out_rows.sort(
    key=lambda row: (
        0 if text(row.get("selector_decision")) in {"LAUNCH_V2_CANDIDATE_READY", "LAUNCH_V2_READY_INLINE_ACCOUNT_RECOVERABLE"} else 1 if text(row.get("selector_decision")) == "LAUNCH_V2_CANDIDATE_READY_ATA_REQUIRED" else 2,
        -safe_float(row.get("buy_pressure_score")),
        safe_int(row.get("target_instruction_index")),
        text(row.get("launch_group_id")),
    )
)
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")

ready_rows = sum(1 for row in out_rows if text(row.get("selector_decision")) in {"LAUNCH_V2_CANDIDATE_READY", "LAUNCH_V2_READY_INLINE_ACCOUNT_RECOVERABLE"})
ata_rows = sum(1 for row in out_rows if text(row.get("selector_decision")) == "LAUNCH_V2_CANDIDATE_READY_ATA_REQUIRED")
trigger_pass_count = sum(1 for row in out_rows if text(row.get("first_v2_blocker")) != "unsupported_pair_class" and text(row.get("selector_decision")) not in {"PARK_V2_PRESSURE"})
best_candidate = next((row for row in out_rows if text(row.get("selector_decision")).startswith("LAUNCH_V2_CANDIDATE_READY")), {})
if not best_candidate:
    best_candidate = next((row for row in out_rows if text(row.get("selector_decision")) == "LAUNCH_V2_READY_INLINE_ACCOUNT_RECOVERABLE"), {})
summary = {
    "status": "LAUNCH_V2_CANDIDATES_READY" if (ready_rows + ata_rows) > 0 else ("PARK_NO_LAUNCH_V2_READY_ROWS" if out_rows else "launch8_v2_candidate_selector_no_input_rows"),
    "rows_seen": len(out_rows),
    "trigger_mode": trigger_mode,
    "buy_prefix_pass_count": sum(
        1
        for row in out_rows
        if safe_int(row.get("buy_count_1s")) >= 1
        and safe_int(row.get("unique_buyers_2s")) >= 1
        and safe_float(row.get("net_buy_pressure")) > 0
        and not boolish(row.get("first_sell_seen"))
        and not boolish(row.get("creator_sell_detected"))
        and not boolish(row.get("insider_cluster_sell_seen"))
    ),
    "exit_ladder_pass_count": sum(1 for row in out_rows if boolish(row.get("r1_exit_nonzero_size_band_ready"))),
    "policy_pass_count": sum(1 for row in out_rows if boolish(row.get("mint_policy_pass"))),
    "pm_ready_count": sum(
        1
        for row in out_rows
        if boolish(row.get("pm_reserve_inventory_ok"))
        and boolish(row.get("pm_reserve_fee_ok"))
        and boolish(row.get("pm_reserve_tip_ok"))
        and boolish(row.get("pm_reserve_rent_ok"))
        and boolish(row.get("pm_release_ok"))
        and boolish(row.get("pm_account_surface_ok"))
    ),
    "rows_with_target_seen_preprocessed_ns": sum(1 for row in out_rows if safe_int(row.get("target_seen_preprocessed_ns")) > 0),
    "live_current_window_rows": sum(1 for row in out_rows if text(row.get("selected_row_source")) == "live_current_window"),
    "missing_lineage_rows": sum(1 for row in out_rows if safe_int(row.get("target_seen_preprocessed_ns")) <= 0),
    "max_cash_at_risk_valid_count": sum(1 for row in out_rows if boolish(row.get("max_cash_at_risk_valid"))),
    "max_loss_budget_valid_v2_count": sum(1 for row in out_rows if boolish(row.get("max_loss_budget_valid_v2"))),
    "auto_flatten_ready_count": sum(1 for row in out_rows if boolish(row.get("auto_flatten_available"))),
    "v2_ready_rows": ready_rows,
    "v2_ata_required_rows": ata_rows,
    "best_candidate": best_candidate,
    "decision_counts": dict(decision_counts),
    "first_blocker_counts": dict(blocker_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Computed Launch V2 candidate selector into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 22_launch7_trigger_markout_history.sh — compare Launch Continuation trigger profiles against available conversion data and optional markout snapshots

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch7 — Trigger Markout History"

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
STEP="${SHADOW_LAUNCH7_STEP:-22_launch7_trigger_markout_history}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch7_trigger_markout_history_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch7_trigger_markout_history_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PRESSURE_ROWS_PATH="${SHADOW_LAUNCH7_PRESSURE_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
GATE_ROWS_PATH="${SHADOW_LAUNCH7_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCH7_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
PROPOSAL_ROWS_PATH="${SHADOW_LAUNCH7_PROPOSAL_ROWS_PATH:-$RESULTS_ROOT/22_launch4_one_mint_live_proposal/launch4_one_mint_live_proposal_rows.jsonl}"
MARKOUT_ROWS_PATH="${SHADOW_LAUNCH7_MARKOUT_ROWS_PATH:-$RESULTS_ROOT/22_launch7_markout_snapshots/launch7_markout_snapshots_rows.jsonl}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PRESSURE_ROWS_PATH" ]] || PRESSURE_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"
[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"
[[ -f "$PROPOSAL_ROWS_PATH" ]] || PROPOSAL_ROWS_PATH="$(latest_rows_for_step launch4_one_mint_live_proposal_rows.jsonl)"
[[ -f "$MARKOUT_ROWS_PATH" ]] || MARKOUT_ROWS_PATH="$(latest_rows_for_step launch7_markout_snapshots_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PRESSURE_ROWS_PATH" "$GATE_ROWS_PATH" "$SIM_ROWS_PATH" "$PROPOSAL_ROWS_PATH" "$MARKOUT_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

pressure_rows_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
gate_rows_path = Path(sys.argv[2]) if sys.argv[2] else Path("")
sim_rows_path = Path(sys.argv[3]) if sys.argv[3] else Path("")
proposal_rows_path = Path(sys.argv[4]) if sys.argv[4] else Path("")
markout_rows_path = Path(sys.argv[5]) if sys.argv[5] else Path("")
rows_out_path = Path(sys.argv[6])
summary_json_path = Path(sys.argv[7])
summary_txt_path = Path(sys.argv[8])


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


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if str(path) in {"", "."} or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


pressure_rows = load_jsonl(pressure_rows_path)
pressure_by_group = {text(row.get("launch_group_id")): row for row in pressure_rows if text(row.get("launch_group_id"))}
gate_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(gate_rows_path) if text(row.get("launch_group_id"))}
sim_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(sim_rows_path) if text(row.get("launch_group_id"))}
proposal_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(proposal_rows_path) if text(row.get("launch_group_id"))}
markout_rows = load_jsonl(markout_rows_path)
markout_by_group: dict[str, list[dict[str, Any]]] = defaultdict(list)
for row in markout_rows:
    key = text(row.get("launch_group_id"))
    if key:
        markout_by_group[key].append(row)


def base_eligible(row: dict[str, Any]) -> bool:
    return (
        text(row.get("pair_class")) == "core_launch_pair"
        and not boolish(row.get("creator_sell_detected"))
        and not boolish(row.get("insider_cluster_sell_seen"))
    )


profiles = [
    ("A_first_buy_no_sell_exit_ready", "first buy + no sell + exit ladder pass", lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_1s")) >= 1 and not boolish(row.get("first_sell_seen")) and boolish((gate or {}).get("r1_exit_nonzero_size_band_ready"))),
    ("B_first_buy_unique1_positive_pressure", "first buy + unique buyer >= 1 + net buy pressure positive", lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_1s")) >= 1 and safe_int(row.get("unique_buyers_2s")) >= 1 and safe_float(row.get("net_buy_pressure")) > 0),
    ("C_buy_count_2s_ge_2", "buy_count_2s >= 2", lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_2s")) >= 2),
    ("D_buy_count_5s_ge_3", "buy_count_5s >= 3", lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_5s")) >= 3),
    ("E_unique_buyers_5s_ge_2", "unique_buyers_5s >= 2", lambda row, gate: base_eligible(row) and safe_int(row.get("unique_buyers_5s")) >= 2),
]


def summarize_returns(rows: list[dict[str, Any]], horizon_s: int) -> tuple[float | None, float | None]:
    values = [safe_float(row.get("return_bps")) for row in rows if safe_int(row.get("horizon_s")) == horizon_s and row.get("return_bps") is not None]
    if not values:
        return None, None
    sorted_values = sorted(values)
    p75_index = min(len(sorted_values) - 1, max(0, int(round(0.75 * (len(sorted_values) - 1)))))
    return statistics.median(values), sorted_values[p75_index]


out_rows: list[dict[str, Any]] = []

for profile_name, description, predicate in profiles:
    selected_groups: list[str] = []
    first_blocker_counts: Counter[str] = Counter()
    mae_samples: list[float] = []

    for pressure_row in pressure_rows:
        group_id = text(pressure_row.get("launch_group_id"))
        gate_row = gate_by_group.get(group_id, {})
        sim_row = sim_by_group.get(group_id, {})
        proposal_row = proposal_by_group.get(group_id, {})
        if not predicate(pressure_row, gate_row):
            continue
        selected_groups.append(group_id)
        blocker = (
            text(proposal_row.get("first_blocker_if_rejected"))
            or text(sim_row.get("first_sim_blocker"))
            or text(gate_row.get("first_gate_blocker"))
            or "none"
        )
        first_blocker_counts[blocker] += 1
        mae = proposal_row.get("max_adverse_excursion_bps")
        if mae is not None:
            mae_samples.append(safe_float(mae))

    selected_set = set(selected_groups)
    selected_markouts = [row for gid in selected_set for row in markout_by_group.get(gid, [])]
    median_5s, p75_5s = summarize_returns(selected_markouts, 5)
    median_15s, p75_15s = summarize_returns(selected_markouts, 15)
    median_30s, _ = summarize_returns(selected_markouts, 30)
    out_rows.append(
        {
            "record_type": "launch7_trigger_markout_history_row",
            "profile": profile_name,
            "description": description,
            "candidate_count": len(selected_groups),
            "exit_ladder_pass_count": sum(1 for gid in selected_set if boolish((gate_by_group.get(gid) or {}).get("r1_exit_nonzero_size_band_ready"))),
            "policy_pass_count": sum(1 for gid in selected_set if boolish((gate_by_group.get(gid) or {}).get("mint_policy_pass"))),
            "proposal_possible_count": sum(1 for gid in selected_set if text((proposal_by_group.get(gid) or {}).get("final_verdict")).startswith("LAUNCH_PROPOSAL_READY")),
            "first_sell_before_entry_count": sum(1 for gid in selected_set if boolish((pressure_by_group.get(gid) or {}).get("first_sell_seen"))),
            "median_return_5s": median_5s,
            "median_return_15s": median_15s,
            "median_return_30s": median_30s,
            "p75_return_5s": p75_5s,
            "p75_return_15s": p75_15s,
            "max_adverse_excursion": max(mae_samples) if mae_samples else None,
            "markout_histories_available": bool(selected_markouts),
            "markout_status": "markout_history_available" if selected_markouts else "markout_history_unavailable",
            "first_blocker_counts": dict(first_blocker_counts),
        }
    )

out_rows.sort(key=lambda row: (-safe_int(row.get("proposal_possible_count")), -safe_int(row.get("exit_ladder_pass_count")), -safe_int(row.get("candidate_count")), text(row.get("profile"))))
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")

summary = {
    "status": "launch7_trigger_markout_history_complete" if out_rows else "launch7_trigger_markout_history_no_input_rows",
    "rows_seen": len(pressure_rows),
    "profiles_tested": len(out_rows),
    "best_profile": out_rows[0] if out_rows else {},
    "markout_histories_available": any(boolish(row.get("markout_histories_available")) for row in out_rows),
    "profile_summaries": out_rows,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"rows_seen={summary['rows_seen']}",
            f"profiles_tested={summary['profiles_tested']}",
            f"markout_histories_available={str(summary['markout_histories_available']).lower()}",
            "best_profile=" + json.dumps(summary["best_profile"], sort_keys=True),
            "profile_summaries=" + json.dumps(summary["profile_summaries"], sort_keys=True),
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Computed Launch markout-history sweep into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

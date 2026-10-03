#!/usr/bin/env bash
# 22_launch6_threshold_sweep.sh — compare earlier Launch Continuation trigger profiles against current exit/policy/PM conversion

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch6 — Threshold Sweep"

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
STEP="${SHADOW_LAUNCH6_STEP:-22_launch6_threshold_sweep}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch6_threshold_sweep_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch6_threshold_sweep_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PRESSURE_ROWS_PATH="${SHADOW_LAUNCH6_PRESSURE_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
GATE_ROWS_PATH="${SHADOW_LAUNCH6_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCH6_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
PROPOSAL_ROWS_PATH="${SHADOW_LAUNCH6_PROPOSAL_ROWS_PATH:-$RESULTS_ROOT/22_launch4_one_mint_live_proposal/launch4_one_mint_live_proposal_rows.jsonl}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PRESSURE_ROWS_PATH" ]] || PRESSURE_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"
[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"
[[ -f "$PROPOSAL_ROWS_PATH" ]] || PROPOSAL_ROWS_PATH="$(latest_rows_for_step launch4_one_mint_live_proposal_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PRESSURE_ROWS_PATH" "$GATE_ROWS_PATH" "$SIM_ROWS_PATH" "$PROPOSAL_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Any

pressure_rows_path = Path(sys.argv[1]) if sys.argv[1] else Path("")
gate_rows_path = Path(sys.argv[2]) if sys.argv[2] else Path("")
sim_rows_path = Path(sys.argv[3]) if sys.argv[3] else Path("")
proposal_rows_path = Path(sys.argv[4]) if sys.argv[4] else Path("")
rows_out_path = Path(sys.argv[5])
summary_json_path = Path(sys.argv[6])
summary_txt_path = Path(sys.argv[7])


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
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


pressure_rows = load_jsonl(pressure_rows_path)
gate_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(gate_rows_path) if text(row.get("launch_group_id"))}
sim_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(sim_rows_path) if text(row.get("launch_group_id"))}
proposal_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(proposal_rows_path) if text(row.get("launch_group_id"))}


def base_eligible(row: dict[str, Any]) -> bool:
    return (
        text(row.get("pair_class")) == "core_launch_pair"
        and not boolish(row.get("creator_sell_detected"))
        and not boolish(row.get("insider_cluster_sell_seen"))
    )


profiles = [
    {
        "profile": "A_first_buy_no_sell_exit_ready",
        "description": "first buy + no sell yet + exit ladder pass",
        "trigger": lambda row, gate: base_eligible(row)
        and safe_int(row.get("buy_count_1s")) >= 1
        and not boolish(row.get("first_sell_seen"))
        and boolish((gate or {}).get("r1_exit_nonzero_size_band_ready")),
    },
    {
        "profile": "B_first_buy_unique1_positive_pressure",
        "description": "first buy + unique buyer >=1 + net buy pressure positive",
        "trigger": lambda row, gate: base_eligible(row)
        and safe_int(row.get("buy_count_1s")) >= 1
        and safe_int(row.get("unique_buyers_2s")) >= 1
        and safe_float(row.get("net_buy_pressure")) > 0,
    },
    {
        "profile": "C_buy_count_2s_ge_2",
        "description": "buy_count_2s >= 2",
        "trigger": lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_2s")) >= 2,
    },
    {
        "profile": "D_buy_count_5s_ge_3",
        "description": "buy_count_5s >= 3",
        "trigger": lambda row, gate: base_eligible(row) and safe_int(row.get("buy_count_5s")) >= 3,
    },
    {
        "profile": "E_unique_buyers_5s_ge_2",
        "description": "unique_buyers_5s >= 2",
        "trigger": lambda row, gate: base_eligible(row) and safe_int(row.get("unique_buyers_5s")) >= 2,
    },
]

profile_rows: list[dict[str, Any]] = []
best_profile: dict[str, Any] = {}

for profile in profiles:
    candidate_rows: list[dict[str, Any]] = []
    first_blocker_counts: Counter[str] = Counter()
    immediate_loss_samples: list[float] = []
    buy_pressure_samples: list[float] = []

    for row in pressure_rows:
        group_id = text(row.get("launch_group_id"))
        gate = gate_by_group.get(group_id, {})
        sim = sim_by_group.get(group_id, {})
        proposal = proposal_by_group.get(group_id, {})
        passed = bool(profile["trigger"](row, gate))
        if not passed:
            continue

        exit_ladder_pass = boolish(gate.get("r1_exit_nonzero_size_band_ready"))
        policy_pass = boolish(gate.get("mint_policy_pass"))
        proposal_possible = (
            exit_ladder_pass
            and policy_pass
            and text(sim.get("sim_decision")) in {"LAUNCH_TINY_SIM_READY", "PARK_ATA_REQUIRED"}
        )
        one_mint_live_ready = text(proposal.get("final_verdict")) == "LAUNCH_PROPOSAL_READY_ONE_MINT_LIVE"
        candidate_rows.append(
            {
                "launch_group_id": group_id,
                "buy_pressure_score": safe_float(row.get("buy_pressure_score")),
                "net_buy_pressure": safe_float(row.get("net_buy_pressure")),
                "time_since_launch_ms": safe_int(row.get("time_since_launch_ms")),
                "first_sell_seen": boolish(row.get("first_sell_seen")),
                "exit_ladder_pass": exit_ladder_pass,
                "policy_pass": policy_pass,
                "proposal_possible": proposal_possible,
                "one_mint_live_ready": one_mint_live_ready,
                "first_gate_blocker": text(gate.get("first_gate_blocker")),
                "first_sim_blocker": text(sim.get("first_sim_blocker")),
                "first_proposal_blocker": text(proposal.get("first_blocker_if_rejected")),
                "immediate_roundtrip_loss_sol": safe_float(sim.get("immediate_roundtrip_loss_sol")),
            }
        )
        blocker = (
            text(proposal.get("first_blocker_if_rejected"))
            or text(sim.get("first_sim_blocker"))
            or text(gate.get("first_gate_blocker"))
            or "none"
        )
        first_blocker_counts[blocker] += 1
        if safe_float(sim.get("immediate_roundtrip_loss_sol")) != 0:
            immediate_loss_samples.append(safe_float(sim.get("immediate_roundtrip_loss_sol")))
        buy_pressure_samples.append(safe_float(row.get("buy_pressure_score")))

    summary_row = {
        "record_type": "launch6_threshold_sweep_row",
        "profile": profile["profile"],
        "description": profile["description"],
        "candidate_count": len(candidate_rows),
        "median_return_5s": None,
        "median_return_15s": None,
        "median_return_30s": None,
        "p75_return": None,
        "max_adverse_excursion": None,
        "markout_histories_available": False,
        "markout_status": "markout_history_unavailable",
        "exit_ladder_pass_count": sum(1 for item in candidate_rows if item["exit_ladder_pass"]),
        "policy_pass_count": sum(1 for item in candidate_rows if item["policy_pass"]),
        "proposal_possible_count": sum(1 for item in candidate_rows if item["proposal_possible"]),
        "one_mint_live_ready_rows": sum(1 for item in candidate_rows if item["one_mint_live_ready"]),
        "first_sell_before_entry_count": sum(1 for item in candidate_rows if item["first_sell_seen"]),
        "median_immediate_roundtrip_loss_sol": statistics.median(immediate_loss_samples) if immediate_loss_samples else None,
        "median_buy_pressure_score": statistics.median(buy_pressure_samples) if buy_pressure_samples else None,
        "first_blocker_counts": dict(first_blocker_counts),
    }
    profile_rows.append(summary_row)

profile_rows.sort(
    key=lambda row: (
        -safe_int(row.get("proposal_possible_count")),
        -safe_int(row.get("exit_ladder_pass_count")),
        -safe_int(row.get("candidate_count")),
        safe_int(row.get("first_sell_before_entry_count")),
        text(row.get("profile")),
    )
)
best_profile = profile_rows[0] if profile_rows else {}

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in profile_rows), encoding="utf-8")

summary = {
    "status": "launch6_threshold_sweep_complete" if profile_rows else "launch6_threshold_sweep_no_input_rows",
    "rows_seen": len(pressure_rows),
    "profiles_tested": len(profile_rows),
    "best_profile": best_profile,
    "profile_summaries": profile_rows,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"rows_seen={summary['rows_seen']}",
            f"profiles_tested={summary['profiles_tested']}",
            "best_profile=" + json.dumps(best_profile, sort_keys=True),
            "profile_summaries=" + json.dumps(profile_rows, sort_keys=True),
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Computed Launch Continuation threshold sweep into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

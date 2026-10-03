#!/usr/bin/env bash
# 22_launch1c_buy_prefix_threshold_sweep.sh — compare launch buy-prefix readiness under a few threshold profiles

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch1C — Buy-Prefix Threshold Sweep"

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
STEP="${SHADOW_LAUNCH1C_STEP:-22_launch1c_buy_prefix_threshold_sweep}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch1c_buy_prefix_threshold_sweep_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch1c_buy_prefix_threshold_sweep_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
INPUT_ROWS_PATH="${SHADOW_LAUNCH1C_INPUT_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$INPUT_ROWS_PATH" ]] || INPUT_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

if [[ -z "$INPUT_ROWS_PATH" || ! -f "$INPUT_ROWS_PATH" ]]; then
    printf '%s\n' \
        "status=launch1c_buy_prefix_threshold_sweep_no_input_rows" \
        "rows_seen=0" \
        "profiles_tested=0" \
        "profile_summaries=[]" > "$SUMMARY_TXT"
    python3 - "$SUMMARY_JSON" <<'PY'
from __future__ import annotations
import json
import sys
from pathlib import Path
summary_path = Path(sys.argv[1])
summary = {
    "status": "launch1c_buy_prefix_threshold_sweep_no_input_rows",
    "rows_seen": 0,
    "profiles_tested": 0,
    "profile_summaries": [],
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
    pass "Launch1C found no input rows; wrote empty summary to $SUMMARY_TXT"
    cat "$SUMMARY_TXT"
    finish
fi

python3 - "$INPUT_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

input_rows_path = Path(sys.argv[1])
rows_out_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
summary_txt_path = Path(sys.argv[4])


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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on"}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


rows = load_jsonl(input_rows_path)
profiles = [
    {
        "profile": "r1_current",
        "min_buy_count_1s": 1,
        "min_buy_count_2s": 2,
        "min_buy_count_5s": 2,
        "min_unique_buyers_5s": 2,
        "min_followthrough_score": 0.5,
        "max_age_ms": 30000,
    },
    {
        "profile": "r1_5s3",
        "min_buy_count_1s": 1,
        "min_buy_count_2s": 2,
        "min_buy_count_5s": 3,
        "min_unique_buyers_5s": 2,
        "min_followthrough_score": 0.5,
        "max_age_ms": 30000,
    },
    {
        "profile": "r1_looser",
        "min_buy_count_1s": 1,
        "min_buy_count_2s": 1,
        "min_buy_count_5s": 2,
        "min_unique_buyers_5s": 1,
        "min_followthrough_score": 0.4,
        "max_age_ms": 30000,
    },
]


def classify(row: dict[str, Any], profile: dict[str, Any]) -> str:
    if text(row.get("pair_class")) != "core_launch_pair":
        return "PARK_UNSUPPORTED_PAIR_CLASS"
    if safe_int(row.get("time_since_launch_ms")) > safe_int(profile["max_age_ms"]):
        return "PARK_LAUNCH_STALE"
    if boolish(row.get("creator_sell_detected")):
        return "PARK_CREATOR_SELL"
    if boolish(row.get("insider_cluster_sell_seen")):
        return "PARK_CREATOR_SELL"
    if boolish(row.get("first_sell_seen")) and safe_int(row.get("buy_count_2s")) < max(2, safe_int(profile["min_buy_count_2s"])):
        return "PARK_SELL_PRESSURE_DOMINANT"
    if safe_int(row.get("buy_count_2s")) < safe_int(profile["min_buy_count_2s"]):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_int(row.get("unique_buyers_2s")) < min(1, safe_int(profile["min_unique_buyers_5s"])):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_int(row.get("buy_count_5s")) < safe_int(profile["min_buy_count_5s"]):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_int(row.get("unique_buyers_5s")) < safe_int(profile["min_unique_buyers_5s"]):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_int(row.get("buy_count_1s")) < safe_int(profile["min_buy_count_1s"]):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_float(row.get("same_direction_followthrough")) < safe_float(profile["min_followthrough_score"]):
        return "PARK_INSUFFICIENT_BUY_PRESSURE"
    if safe_float(row.get("net_buy_pressure")) <= 0:
        return "PARK_SELL_PRESSURE_DOMINANT"
    return "BUY_PREFIX_READY"


out_rows: list[dict[str, Any]] = []
profile_summaries: list[dict[str, Any]] = []
for profile in profiles:
    decision_counts: Counter[str] = Counter()
    ready_rows = 0
    for row in rows:
        decision = classify(row, profile)
        decision_counts[decision] += 1
        if decision == "BUY_PREFIX_READY":
            ready_rows += 1
    profile_summary = {
        **profile,
        "rows_seen": len(rows),
        "ready_rows": ready_rows,
        "buy_count_1s_ge_1_rows": sum(1 for row in rows if safe_int(row.get("buy_count_1s")) >= 1),
        "buy_count_2s_ge_2_rows": sum(1 for row in rows if safe_int(row.get("buy_count_2s")) >= 2),
        "buy_count_5s_ge_3_rows": sum(1 for row in rows if safe_int(row.get("buy_count_5s")) >= 3),
        "unique_buyers_2s_ge_1_rows": sum(1 for row in rows if safe_int(row.get("unique_buyers_2s")) >= 1),
        "unique_buyers_5s_ge_2_rows": sum(1 for row in rows if safe_int(row.get("unique_buyers_5s")) >= 2),
        "positive_net_buy_pressure_rows": sum(1 for row in rows if safe_float(row.get("net_buy_pressure")) > 0),
        "first_sell_seen_before_entry_rows": sum(
            1 for row in rows if boolish(row.get("first_sell_seen")) and safe_int(row.get("buy_count_2s")) < max(2, safe_int(profile["min_buy_count_2s"]))
        ),
        "decision_counts": dict(decision_counts),
    }
    profile_summaries.append(profile_summary)
    out_rows.append(profile_summary)

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")
summary = {
    "status": "launch1c_buy_prefix_threshold_sweep_complete",
    "rows_seen": len(rows),
    "profiles_tested": len(profile_summaries),
    "profile_summaries": profile_summaries,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"rows_seen={summary['rows_seen']}",
            f"profiles_tested={summary['profiles_tested']}",
            "profile_summaries=" + json.dumps(profile_summaries, sort_keys=True),
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

#!/usr/bin/env bash
# 66ct3_continuation_markout_risk_validation.sh - markout/adverse validation for Continuation candidates

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CT3 - Continuation Markout Risk Validation"

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
STEP="${SHADOW_RUN66CT3_STEP:-66ct3_continuation_markout_risk_validation}"
SOURCE_ROWS="${SHADOW_RUN66CT3_SOURCE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name run66ct2_continuation_execution_rule_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ct3_continuation_markout_risk_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ct3_continuation_markout_risk_validation.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, summary_txt_arg = sys.argv[1:7]
source_path = Path(source_arg) if source_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()]

def text(value):
    return "" if value is None else str(value).strip()

def lower(value):
    return text(value).lower()

def num(value):
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None

def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}

def first_num(row, *keys):
    for key in keys:
        value = num(row.get(key))
        if value is not None:
            return value
    return None

def any_positive(*values):
    return any(value is not None and value > 0 for value in values)

def first_blocker(row):
    checks = [
        ("BLOCKED_NEGATIVE_AFTER_COST", row["best_later_net_positive"]),
        ("BLOCKED_ADVERSE_EXCURSION", row["max_adverse_excursion_bounded"]),
        ("BLOCKED_CONTINUATION_SIGNAL", row["same_direction_followthrough_positive"]),
        ("BLOCKED_ABSORPTION_TOXIC", row["opposite_direction_absorption_not_toxic"]),
    ]
    for blocker, ok in checks:
        if not ok:
            return blocker
    return "none"

out = []
for row in load_jsonl(source_path):
    event1 = first_num(row, "next_same_pool_event_1_markout")
    event2 = first_num(row, "next_same_pool_event_2_markout")
    event3 = first_num(row, "next_same_pool_event_3_markout")
    time50 = first_num(row, "actual_markout_at_50ms")
    time100 = first_num(row, "actual_markout_at_100ms")
    time250 = first_num(row, "actual_markout_at_250ms")
    time500 = first_num(row, "actual_markout_at_500ms")
    time1s = first_num(row, "actual_markout_at_1s")
    time2s = first_num(row, "actual_markout_at_2s")
    time5s = first_num(row, "actual_markout_at_5s")
    time10s = first_num(row, "actual_markout_at_10s")
    time30s = first_num(row, "actual_markout_at_30s")
    pressure = first_num(row, "same_direction_continuation_pressure_score", "same_direction_followthrough_score")
    absorption = first_num(row, "opposite_direction_absorption_score")
    item = {
        **row,
        "immediate_net_ev": first_num(row, "route_specific_ev_after_cost_raw"),
        "later_horizon_ev_by_horizon": {
            "50ms": time50,
            "100ms": time100,
            "250ms": time250,
            "500ms": time500,
            "1s": time1s,
            "2s": time2s,
            "5s": time5s,
            "10s": time10s,
            "30s": time30s,
            "next_same_pool_event_1": event1,
            "next_same_pool_event_2": event2,
            "next_same_pool_event_3": event3,
        },
        "best_later_horizon": text(row.get("best_horizon")),
        "best_later_net_positive": any_positive(time50, time100, time250, time500, time1s, time2s, time5s, time10s, time30s, event1, event2, event3),
        "max_adverse_excursion": first_num(row, "max_adverse_excursion"),
        "max_adverse_excursion_bounded": boolish(row.get("max_adverse_excursion_bounded")),
        "same_direction_followthrough": pressure,
        "same_direction_followthrough_positive": pressure is not None and pressure >= 0.5,
        "opposite_direction_absorption": absorption,
        "opposite_direction_absorption_not_toxic": absorption is not None and absorption <= 0.5,
        "time_to_decay": text(row.get("best_horizon")),
        "saved_loss_if_drop": None,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    blocker = first_blocker(item)
    item["first_markout_risk_blocker"] = blocker
    item["markout_risk_ready"] = blocker == "none" and text(row.get("decision")) == "CONTINUATION_EXECUTION_PLUMBING_READY"
    item["decision"] = "CONTINUATION_MARKOUT_RISK_READY" if item["markout_risk_ready"] else blocker
    out.append(item)

counts = Counter(row["decision"] for row in out)
summary = {
    "status": "run66ct3_continuation_markout_risk_validation_complete",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "markout_risk_ready_rows": counts.get("CONTINUATION_MARKOUT_RISK_READY", 0),
    "best_later_net_positive_rows": sum(1 for row in out if row["best_later_net_positive"]),
    "max_adverse_excursion_bounded_rows": sum(1 for row in out if row["max_adverse_excursion_bounded"]),
    "same_direction_followthrough_positive_rows": sum(1 for row in out if row["same_direction_followthrough_positive"]),
    "opposite_direction_absorption_not_toxic_rows": sum(1 for row in out if row["opposite_direction_absorption_not_toxic"]),
    "decision_counts": dict(counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {"source_rows": str(source_path or ""), "result_dir": str(result_dir), "live_send": False, "sendBundle": False}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()) + "\n", encoding="utf-8")
PY

pass "Run66CT3 continuation markout risk validation written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

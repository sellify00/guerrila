#!/usr/bin/env bash
# 66ct1_continuation_owner_eligibility.sh - owner/PM eligibility for Continuation reroute candidates

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CT1 - Continuation Owner Eligibility"

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
STEP="${SHADOW_RUN66CT1_STEP:-66ct1_continuation_owner_eligibility}"
SOURCE_ROWS="${SHADOW_RUN66CT1_SOURCE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name continuation_candidate_manifest.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ct1_continuation_owner_eligibility_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ct1_continuation_owner_eligibility.json"
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

def conflict_domain(row):
    return "|".join([text(row.get("venue")), text(row.get("pool")), text(row.get("mint_in")), text(row.get("mint_out"))])

rows = load_jsonl(source_path)
out = []
for row in rows:
    pressure = first_num(row, "same_direction_continuation_pressure_score", "same_direction_followthrough_score")
    from_reroute = boolish(row.get("from_false_restoration_reject")) and text(row.get("recommended_route")) == "continuation_structural_certificate"
    direct_owner = (
        text(row.get("source")) == "fresh_runtime_owner_continuation"
        or lower(row.get("owner_mode_selected")) == "continuation"
        or lower(row.get("selected_owner")) == "continuation"
        or boolish(row.get("owner_mode_match_continuation"))
    )
    execution_ready = boolish(row.get("execution_ready_from_66rg6"))
    habitat_only = text(row.get("source")) == "continuation_habitat_context"
    owner_match = (from_reroute or direct_owner) and pressure is not None and pressure > 0
    pm_ready = owner_match and execution_ready
    runner_up = text(row.get("runner_up_owner"))
    item = {
        **row,
        "owner_mode_selected": "continuation" if owner_match else ("habitat_watch" if habitat_only else "unresolved"),
        "owner_mode_match_continuation": owner_match,
        "owner_mode_score_continuation": pressure,
        "owner_mode_score_source": text(row.get("owner_mode_score_source")) or ("fresh_runtime" if direct_owner else "reroute_pressure"),
        "runner_up_owner": runner_up or text(row.get("source_owner_mode_before_reroute")) or ("rebalancing" if from_reroute else "unknown"),
        "fallback_owner": "drop_flatten" if boolish(row.get("actual_toxic")) else "park",
        "same_direction_followthrough_score": first_num(row, "same_direction_followthrough_score", "same_direction_continuation_pressure_score"),
        "continuation_pressure_measured": pressure is not None,
        "directarb_preempted": False,
        "backrun_preempted": False,
        "rebalancing_false_restoration_source": from_reroute,
        "fresh_runtime_owner_source": direct_owner,
        "conflict_domain": conflict_domain(row),
        "pm_owner_lock_ready": pm_ready,
        "pm_owner_lock_owner_mode": "continuation" if pm_ready else "",
        "pm_owner_lock_candidate_id": text(row.get("candidate_id")) if pm_ready else "",
        "pm_owner_lock_conflict_domain": conflict_domain(row) if pm_ready else "",
        "pm_owner_lock_status": "structural_no_send_reroute_lock" if pm_ready else "not_ready",
        "double_reserve_possible": False if pm_ready else True,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    if owner_match and pm_ready:
        decision = "CONTINUATION_OWNER_ELIGIBLE"
    elif owner_match and direct_owner:
        decision = "BLOCKED_PM_OWNER_LOCK"
    elif habitat_only:
        decision = "CONTINUATION_HABITAT_WATCH_OWNER_UNPROVEN"
    elif owner_match:
        decision = "BLOCKED_PM_OWNER_LOCK"
    else:
        decision = "BLOCKED_OWNER_MISMATCH"
    item["decision"] = decision
    out.append(item)

counts = Counter(row["decision"] for row in out)
summary = {
    "status": "run66ct1_continuation_owner_eligibility_complete",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "owner_mode_match_continuation_rows": sum(1 for row in out if row["owner_mode_match_continuation"]),
    "continuation_pressure_measured_rows": sum(1 for row in out if row["continuation_pressure_measured"]),
    "pm_owner_lock_ready_rows": sum(1 for row in out if row["pm_owner_lock_ready"]),
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

pass "Run66CT1 continuation owner eligibility written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

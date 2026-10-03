#!/usr/bin/env bash
# 66rg5_false_restoration_gate.sh - false-restoration guard supplement for Rebalancing

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66RG5 - Rebalancing False-Restoration Gate"

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
STEP="${SHADOW_RUN66RG5_STEP:-66rg5_false_restoration_gate_20260527}"
SOURCE_ROWS="${SHADOW_RUN66RG5_SOURCE_ROWS:-}"
OWNER_ROWS="${SHADOW_RUN66RG5_OWNER_ROWS:-}"
RESTORATION_ROWS="${SHADOW_RUN66RG5_RESTORATION_ROWS:-}"
CONTINUATION_THRESHOLD="${SHADOW_RUN66RG5_CONTINUATION_THRESHOLD:-0.50}"
ABSORPTION_THRESHOLD="${SHADOW_RUN66RG5_ABSORPTION_THRESHOLD:-0.50}"
FOLLOWTHROUGH_THRESHOLD="${SHADOW_RUN66RG5_FOLLOWTHROUGH_THRESHOLD:-0.50}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$OWNER_ROWS" ]] || OWNER_ROWS="$(latest_file owner_selector_instrumentation_rows.jsonl)"
[[ -n "$RESTORATION_ROWS" ]] || RESTORATION_ROWS="$(latest_file run66rg4_rebalancing_restoration_pressure_measurement_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66rg5_false_restoration_gate_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66rg5_false_restoration_gate.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$OWNER_ROWS" "$RESTORATION_ROWS" "$CONTINUATION_THRESHOLD" "$ABSORPTION_THRESHOLD" "$FOLLOWTHROUGH_THRESHOLD" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, owner_arg, restoration_arg, continuation_threshold_arg, absorption_threshold_arg, followthrough_threshold_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:13]
source_path = Path(source_arg) if source_arg else None
owner_path = Path(owner_arg) if owner_arg else None
restoration_path = Path(restoration_arg) if restoration_arg else None
continuation_threshold = float(continuation_threshold_arg or 0.50)
absorption_threshold = float(absorption_threshold_arg or 0.50)
followthrough_threshold = float(followthrough_threshold_arg or 0.50)
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def text(value):
    return "" if value is None else str(value).strip()

def lower(value):
    return text(value).lower()

def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready", "toxic"}

def first_text(row, *keys):
    for key in keys:
        value = row.get(key)
        if text(value):
            return text(value)
    return ""

def first_number(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is None or value == "":
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return None

def first_bool(row, *keys):
    for key in keys:
        if key in row:
            return boolish(row.get(key))
    return False

def first_optional_bool(row, *keys):
    for key in keys:
        if key in row:
            return boolish(row.get(key))
    return None

def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out

def merge(*rows):
    out = {}
    for row in rows:
        if row:
            for key, value in row.items():
                if value not in (None, ""):
                    out[key] = value
    return out

def first_reason(item):
    if item["owner_mode_selected"] != "rebalancing":
        return "owner_mode_not_rebalancing"
    if not item["restoration_pressure_measured"]:
        return "restoration_pressure_not_measured"
    if item["continuation_pressure_score"] is None:
        return "continuation_pressure_score_missing"
    if item["absorption_score"] is None:
        return "absorption_score_missing"
    if item["same_direction_followthrough_score"] is None:
        return "same_direction_followthrough_score_missing"
    if item["route_lag_conflict"] is None:
        return "route_lag_conflict_missing"
    if item["liquidity_migration_signal"] is None:
        return "liquidity_migration_signal_missing"
    if item["toxic_flow_signal"] is None:
        return "toxic_flow_signal_missing"
    if item["continuation_pressure_score"] > continuation_threshold:
        return "same_direction_continuation_pressure_too_high"
    if item["absorption_score"] > absorption_threshold:
        return "opposite_direction_absorption_toxic"
    if item["same_direction_followthrough_score"] > followthrough_threshold:
        return "same_direction_followthrough_dominant"
    if item["route_lag_conflict"]:
        return "route_gap_decay_or_lag_conflict"
    if item["liquidity_migration_signal"]:
        return "liquidity_migration_signal"
    if item["toxic_flow_signal"]:
        return "toxic_flow_signal"
    return "none"

def inputs_ready(item):
    return all([
        item["owner_mode_selected"] == "rebalancing",
        item["restoration_pressure_measured"],
        item["continuation_pressure_score"] is not None,
        item["absorption_score"] is not None,
        item["same_direction_followthrough_score"] is not None,
        item["route_lag_conflict"] is not None,
        item["liquidity_migration_signal"] is not None,
        item["toxic_flow_signal"] is not None,
    ])

source_rows = load_jsonl(source_path)
owner_by_candidate = by_candidate(load_jsonl(owner_path))
restoration_by_candidate = by_candidate(load_jsonl(restoration_path))

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    row = merge(owner_by_candidate.get(cid), base, restoration_by_candidate.get(cid))
    route_lag_raw = first_optional_bool(row, "route_lag_conflict")
    if route_lag_raw is None and "route_gap_decay_not_stale" in row:
        route_lag_raw = not first_bool(row, "route_gap_decay_not_stale")
    migration_raw = first_optional_bool(row, "liquidity_migration_signal")
    toxic_raw = first_optional_bool(row, "toxic_flow_signal")
    item = {
        "candidate_id": cid,
        "pool": first_text(row, "pool"),
        "venue": first_text(row, "venue"),
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "owner_mode_selected": lower(first_text(row, "owner_mode_selected", "selected_owner", "owner_mode")),
        "restoration_pressure_measured": first_bool(row, "restoration_pressure_measured"),
        "continuation_pressure_score": first_number(row, "continuation_pressure_score", "continuation_score"),
        "absorption_score": first_number(row, "absorption_score", "opposite_direction_absorption_score"),
        "same_direction_followthrough_score": first_number(row, "same_direction_followthrough_score", "followthrough_score"),
        "route_lag_conflict": route_lag_raw,
        "liquidity_migration_signal": migration_raw,
        "toxic_flow_signal": toxic_raw,
        "continuation_pressure_threshold": continuation_threshold,
        "absorption_threshold": absorption_threshold,
        "same_direction_followthrough_threshold": followthrough_threshold,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    reason = first_reason(item)
    item["false_restoration_gate_input_ready"] = inputs_ready(item)
    item["false_restoration_gate_pass"] = reason == "none"
    item["false_restoration_reason"] = reason
    item["same_direction_followthrough_not_dominant"] = (
        item["same_direction_followthrough_score"] is not None
        and item["same_direction_followthrough_score"] <= followthrough_threshold
    )
    item["opposite_direction_absorption_not_toxic"] = (
        item["absorption_score"] is not None
        and item["absorption_score"] <= absorption_threshold
    )
    item["route_gap_decay_not_stale"] = item["route_lag_conflict"] is False
    item["crowding_or_copy_wave_not_dominant"] = (
        item["continuation_pressure_score"] is not None
        and item["continuation_pressure_score"] <= continuation_threshold
        and item["same_direction_followthrough_not_dominant"]
    )
    out.append(item)

reasons = Counter(row["false_restoration_reason"] for row in out)
counts = {
    "rows_seen": len(out),
    "owner_mode_rebalancing_rows": sum(1 for row in out if row["owner_mode_selected"] == "rebalancing"),
    "restoration_pressure_measured_rows": sum(1 for row in out if row["restoration_pressure_measured"]),
    "continuation_pressure_score_rows": sum(1 for row in out if row["continuation_pressure_score"] is not None),
    "absorption_score_rows": sum(1 for row in out if row["absorption_score"] is not None),
    "same_direction_followthrough_score_rows": sum(1 for row in out if row["same_direction_followthrough_score"] is not None),
    "false_restoration_gate_input_ready_rows": sum(1 for row in out if row["false_restoration_gate_input_ready"]),
    "false_restoration_gate_pass_rows": sum(1 for row in out if row["false_restoration_gate_pass"]),
}
if counts["false_restoration_gate_pass_rows"] > 0:
    status = "FALSE_RESTORATION_GATE_READY"
elif counts["false_restoration_gate_input_ready_rows"] > 0:
    status = "PARK_FALSE_RESTORATION"
else:
    status = "PARK_FALSE_RESTORATION_DATA_ABSENT"

manifest = {
    "source_rows": str(source_path or ""),
    "owner_rows": str(owner_path or ""),
    "restoration_rows": str(restoration_path or ""),
    "continuation_threshold": continuation_threshold,
    "absorption_threshold": absorption_threshold,
    "followthrough_threshold": followthrough_threshold,
    "result_dir": str(result_dir),
    "certificate_scope": "false_restoration_gate_only",
    "live_send": False,
    "sendBundle": False,
}
summary = {
    "status": status,
    "counts": counts,
    "false_restoration_reason_counts": dict(reasons),
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
verdict = {
    "final_verdict": status,
    **counts,
    "primary_blocker": reasons.most_common(1)[0][0] if reasons else "inputs_missing",
    "live_approval": False,
    "proposal_path_open": False,
    "hard_stops_preserved": summary["hard_stops_preserved"],
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [f"final_verdict={status}"]
for key, value in counts.items():
    lines.append(f"{key}={value}")
lines.append(f"false_restoration_reason_counts={json.dumps(dict(reasons), sort_keys=True)}")
lines.extend(["wire_send=0", "live_landed=0", "send_transaction_called=false", "sendBundle_called=false"])
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RG5 false-restoration gate written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

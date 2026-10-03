#!/usr/bin/env bash
# 66rg4_rebalancing_restoration_pressure_measurement.sh - restoration-pressure supplement for Rebalancing

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66RG4 - Rebalancing Restoration Pressure Measurement"

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
STEP="${SHADOW_RUN66RG4_STEP:-66rg4_rebalancing_restoration_pressure_measurement_20260527}"
SOURCE_ROWS="${SHADOW_RUN66RG4_SOURCE_ROWS:-}"
OWNER_ROWS="${SHADOW_RUN66RG4_OWNER_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66RG4_RULE_ROWS:-}"
Q2_ROWS="${SHADOW_RUN66RG4_Q2_ROWS:-}"
OVERLAY_ROWS="${SHADOW_RUN66RG4_OVERLAY_ROWS:-}"
MAX_ADVERSE_EXCURSION_BPS="${SHADOW_RUN66RG4_MAX_ADVERSE_EXCURSION_BPS:-500}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$OWNER_ROWS" ]] || OWNER_ROWS="$(latest_file owner_selector_instrumentation_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"
[[ -n "$Q2_ROWS" ]] || Q2_ROWS="$(latest_file run66q2_rebalancing_exact_quote_route_ev_rows.jsonl)"
[[ -n "$OVERLAY_ROWS" ]] || OVERLAY_ROWS="$(latest_file run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66rg4_rebalancing_restoration_pressure_measurement_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66rg4_rebalancing_restoration_pressure_measurement.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$OWNER_ROWS" "$RULE_ROWS" "$Q2_ROWS" "$OVERLAY_ROWS" "$MAX_ADVERSE_EXCURSION_BPS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import re
import sys
from collections import Counter
from pathlib import Path

source_arg, owner_arg, rule_arg, q2_arg, overlay_arg, max_adverse_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:13]
source_path = Path(source_arg) if source_arg else None
owner_path = Path(owner_arg) if owner_arg else None
rule_path = Path(rule_arg) if rule_arg else None
q2_path = Path(q2_arg) if q2_arg else None
overlay_path = Path(overlay_arg) if overlay_arg else None
max_adverse_limit = float(max_adverse_arg or 500)
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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}

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

def horizon_ms(row):
    numeric = first_number(row, "max_horizon_ms", "max_hold_horizon_ms")
    if numeric is not None:
        return numeric
    raw = first_text(row, "max_hold_horizon", "hold_horizon")
    match = re.search(r"(\d+(?:\.\d+)?)\s*ms", raw)
    if match:
        return float(match.group(1))
    return None

def first_blocker(item):
    if item["owner_mode_selected"] != "rebalancing":
        return "owner_mode_not_rebalancing"
    if item["reserve_displacement_raw"] is None and item["displacement_bps"] is None:
        return "reserve_displacement_missing"
    if item["restoration_pressure_score"] is None:
        return "restoration_pressure_score_missing"
    if item["recovery_fraction_target"] is None:
        return "recovery_fraction_target_missing"
    if not item["pre_impulse_state"] or not item["post_impulse_state"]:
        return "pre_post_impulse_state_missing"
    if not item["entry_side"] or not item["exit_side"] or not item["expected_restoration_path"]:
        return "entry_exit_restoration_path_missing"
    if item["max_horizon_ms"] is None:
        return "max_horizon_missing"
    if item["max_adverse_excursion"] is None:
        return "max_adverse_excursion_missing"
    if not item["max_adverse_excursion_bounded"]:
        return "max_adverse_excursion_unbounded"
    return "none"

source_rows = load_jsonl(source_path)
owner_by_candidate = by_candidate(load_jsonl(owner_path))
rule_by_candidate = by_candidate(load_jsonl(rule_path))
q2_by_candidate = by_candidate(load_jsonl(q2_path))
overlay_by_candidate = by_candidate(load_jsonl(overlay_path))

out = []
for index, base in enumerate(source_rows, start=1):
    cid = first_text(base, "candidate_id", "row_id") or f"row_{index}"
    row = merge(owner_by_candidate.get(cid), q2_by_candidate.get(cid), rule_by_candidate.get(cid), overlay_by_candidate.get(cid), base)
    max_adverse = first_number(row, "max_adverse_excursion_bps", "max_adverse_excursion")
    item = {
        "candidate_id": cid,
        "impulse_id": first_text(row, "impulse_id", "signature_or_impulse_id", "signature", "candidate_id") or cid,
        "pool": first_text(row, "pool"),
        "venue": first_text(row, "venue"),
        "mint_pair": f"{first_text(row, 'mint_in')}->{first_text(row, 'mint_out')}",
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "owner_mode_selected": lower(first_text(row, "owner_mode_selected", "selected_owner", "owner_mode")),
        "displacement_bps": first_number(row, "reserve_displacement_bps", "price_displacement_bps", "observed_displacement_bps", "displacement_bps", "displacement_threshold"),
        "reserve_displacement_raw": first_number(row, "reserve_displacement_raw", "reserve_delta_raw", "post_event_reserve_delta_raw"),
        "restoration_pressure_score": first_number(row, "restoration_pressure_score", "restoration_score"),
        "recovery_fraction_target": first_number(row, "recovery_fraction_target", "target_recovery_fraction"),
        "pre_impulse_state": first_text(row, "pre_impulse_state", "pre_event_pool_state"),
        "post_impulse_state": first_text(row, "post_impulse_state", "post_event_pool_state"),
        "entry_side": first_text(row, "entry_side"),
        "exit_side": first_text(row, "exit_side"),
        "expected_restoration_path": first_text(row, "expected_restoration_path", "expected_restoration_direction"),
        "max_horizon_ms": horizon_ms(row),
        "max_adverse_excursion": max_adverse,
        "max_adverse_excursion_limit_bps": max_adverse_limit,
        "max_adverse_excursion_bounded": max_adverse is not None and abs(max_adverse) <= max_adverse_limit,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    item["first_restoration_blocker"] = first_blocker(item)
    explicit_measured = first_bool(row, "restoration_pressure_measured", "reserve_displacement_measured")
    item["restoration_pressure_measured"] = explicit_measured and item["first_restoration_blocker"] == "none"
    out.append(item)

blockers = Counter(row["first_restoration_blocker"] for row in out)
counts = {
    "rows_seen": len(out),
    "owner_mode_rebalancing_rows": sum(1 for row in out if row["owner_mode_selected"] == "rebalancing"),
    "reserve_displacement_input_rows": sum(1 for row in out if row["reserve_displacement_raw"] is not None or row["displacement_bps"] is not None),
    "restoration_pressure_score_rows": sum(1 for row in out if row["restoration_pressure_score"] is not None),
    "recovery_fraction_target_rows": sum(1 for row in out if row["recovery_fraction_target"] is not None),
    "max_adverse_excursion_bounded_rows": sum(1 for row in out if row["max_adverse_excursion_bounded"]),
    "restoration_pressure_measured_rows": sum(1 for row in out if row["restoration_pressure_measured"]),
}
status = "RESTORATION_PRESSURE_MEASURED" if counts["restoration_pressure_measured_rows"] > 0 else "PARK_RESTORATION_DATA_ABSENT"

manifest = {
    "source_rows": str(source_path or ""),
    "owner_rows": str(owner_path or ""),
    "rule_rows": str(rule_path or ""),
    "q2_rows": str(q2_path or ""),
    "overlay_rows": str(overlay_path or ""),
    "max_adverse_excursion_limit_bps": max_adverse_limit,
    "result_dir": str(result_dir),
    "certificate_scope": "restoration_pressure_measurement_only",
    "live_send": False,
    "sendBundle": False,
}
summary = {
    "status": status,
    "counts": counts,
    "first_restoration_blocker_counts": dict(blockers),
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
    "primary_blocker": blockers.most_common(1)[0][0] if blockers else "inputs_missing",
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
lines.append(f"first_restoration_blocker_counts={json.dumps(dict(blockers), sort_keys=True)}")
lines.extend(["wire_send=0", "live_landed=0", "send_transaction_called=false", "sendBundle_called=false"])
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RG4 restoration-pressure measurement written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

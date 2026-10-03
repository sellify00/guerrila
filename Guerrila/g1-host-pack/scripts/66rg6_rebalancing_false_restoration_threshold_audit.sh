#!/usr/bin/env bash
# 66rg6_rebalancing_false_restoration_threshold_audit.sh - audit false-restoration thresholds for Rebalancing

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66RG6 - Rebalancing False-Restoration Threshold Audit"

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
STEP="${SHADOW_RUN66RG6_STEP:-66rg6_rebalancing_false_restoration_threshold_audit_20260527}"
SOURCE_ROWS="${SHADOW_RUN66RG6_SOURCE_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66RG6_RULE_ROWS:-}"
FALSE_RESTORATION_ROWS="${SHADOW_RUN66RG6_FALSE_RESTORATION_ROWS:-}"
CANDIDATE_AUTOPSY_ROWS="${SHADOW_RUN66RG6_CANDIDATE_AUTOPSY_ROWS:-}"
CURRENT_THRESHOLD="${SHADOW_RUN66RG6_CURRENT_THRESHOLD:-0.50}"
LOOSER_THRESHOLD_1="${SHADOW_RUN66RG6_LOOSER_THRESHOLD_1:-0.67}"
LOOSER_THRESHOLD_2="${SHADOW_RUN66RG6_LOOSER_THRESHOLD_2:-0.80}"
STRICTER_THRESHOLD_1="${SHADOW_RUN66RG6_STRICTER_THRESHOLD_1:-0.33}"
MAX_ADVERSE_BPS="${SHADOW_RUN66RG6_MAX_ADVERSE_BPS:-50}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66_rebalancing_rule_rows.jsonl)"
[[ -n "$FALSE_RESTORATION_ROWS" ]] || FALSE_RESTORATION_ROWS="$(latest_file run66rg5_false_restoration_gate_rows.jsonl)"
[[ -n "$CANDIDATE_AUTOPSY_ROWS" ]] || CANDIDATE_AUTOPSY_ROWS="$(latest_file candidate_autopsy.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66rg6_false_restoration_threshold_audit_rows.jsonl"
EXECUTION_READY_ROWS="$RESULT_DIR/execution_ready_threshold_audit_rows.jsonl"
CONTINUATION_ROUTE_ROWS="$RESULT_DIR/route_to_continuation_rows.jsonl"
DROP_FLATTEN_ROUTE_ROWS="$RESULT_DIR/route_to_drop_flatten_saved_loss_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66rg6_false_restoration_threshold_audit.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$EXECUTION_READY_ROWS" "$CONTINUATION_ROUTE_ROWS" "$DROP_FLATTEN_ROUTE_ROWS" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RULE_ROWS" "$FALSE_RESTORATION_ROWS" "$CANDIDATE_AUTOPSY_ROWS" "$CURRENT_THRESHOLD" "$LOOSER_THRESHOLD_1" "$LOOSER_THRESHOLD_2" "$STRICTER_THRESHOLD_1" "$MAX_ADVERSE_BPS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$EXECUTION_READY_ROWS" "$CONTINUATION_ROUTE_ROWS" "$DROP_FLATTEN_ROUTE_ROWS" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

(
    source_arg,
    rule_arg,
    false_restoration_arg,
    autopsy_arg,
    current_threshold_arg,
    looser_threshold_1_arg,
    looser_threshold_2_arg,
    stricter_threshold_1_arg,
    max_adverse_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    execution_ready_arg,
    continuation_route_arg,
    drop_flatten_route_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
) = sys.argv[1:19]

source_path = Path(source_arg) if source_arg else None
rule_path = Path(rule_arg) if rule_arg else None
false_restoration_path = Path(false_restoration_arg) if false_restoration_arg else None
autopsy_path = Path(autopsy_arg) if autopsy_arg else None
current_threshold = float(current_threshold_arg or 0.50)
looser_threshold_1 = float(looser_threshold_1_arg or 0.67)
looser_threshold_2 = float(looser_threshold_2_arg or 0.80)
stricter_threshold_1 = float(stricter_threshold_1_arg or 0.33)
max_adverse_bps = float(max_adverse_arg or 50)
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
execution_ready_path = Path(execution_ready_arg)
continuation_route_path = Path(continuation_route_arg)
drop_flatten_route_path = Path(drop_flatten_route_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

SENDER_READY = "REBALANCING_SENDER_READY_NO_SEND"
BUNDLE_READY = "REBALANCING_BUNDLE_READY_NO_SUBMIT"

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

def number(value):
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None

def first_text(row, *keys):
    for key in keys:
        value = row.get(key)
        if text(value):
            return text(value)
    return ""

def first_number(row, *keys):
    for key in keys:
        value = number(row.get(key))
        if value is not None:
            return value
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

def nested_markout(markouts, group, key, field):
    value = ((markouts.get(group) or {}).get(key) or {}).get(field)
    return number(value)

def nested_bool(markouts, group, key, field):
    snapshot = (markouts.get(group) or {}).get(key) or {}
    if field not in snapshot:
        return None
    return boolish(snapshot.get(field))

def gate_pass(row, threshold):
    cont = first_number(row, "continuation_pressure_score")
    absorption = first_number(row, "absorption_score", "opposite_direction_absorption_score")
    follow = first_number(row, "same_direction_followthrough_score", "followthrough_score")
    route_lag = first_bool(row, "route_lag_conflict")
    migration = first_bool(row, "liquidity_migration_signal")
    toxic = first_bool(row, "toxic_flow_signal")
    if cont is None or absorption is None or follow is None:
        return False
    return all([
        cont <= threshold,
        absorption <= threshold,
        follow <= threshold,
        not route_lag,
        not migration,
        not toxic,
    ])

def positive(value):
    return value is not None and value > 0

def adverse_bounded(value):
    if value is None:
        return False
    return abs(min(0.0, value)) <= max_adverse_bps

source_rows = load_jsonl(source_path)
rule_by_candidate = by_candidate(load_jsonl(rule_path))
false_by_candidate = by_candidate(load_jsonl(false_restoration_path))
autopsy_by_candidate = by_candidate(load_jsonl(autopsy_path))

out = []
for idx, source in enumerate(source_rows, start=1):
    cid = first_text(source, "candidate_id", "row_id") or f"row_{idx}"
    rule = rule_by_candidate.get(cid, {})
    false_row = false_by_candidate.get(cid, {})
    autopsy = autopsy_by_candidate.get(cid, {})
    row = merge(autopsy, rule, source, false_row)
    markouts = autopsy.get("markouts") if isinstance(autopsy.get("markouts"), dict) else {}
    event_1 = nested_markout(markouts, "event_horizons", "next_same_pool_event_1", "markout_lamports")
    event_2 = nested_markout(markouts, "event_horizons", "next_same_pool_event_2", "markout_lamports")
    event_3 = nested_markout(markouts, "event_horizons", "next_same_pool_event_3", "markout_lamports")
    best_horizon = first_text(row, "best_horizon", "restoration_target") or text(markouts.get("best_horizon"))
    best_same_direction = None
    if best_horizon.startswith("next_same_pool_event"):
        best_same_direction = nested_bool(markouts, "event_horizons", best_horizon, "best_same_direction")
    time_50 = nested_markout(markouts, "time_horizons", "50ms", "markout_lamports")
    time_100 = nested_markout(markouts, "time_horizons", "100ms", "markout_lamports")
    time_250 = nested_markout(markouts, "time_horizons", "250ms", "markout_lamports")
    time_500 = nested_markout(markouts, "time_horizons", "500ms", "markout_lamports")
    time_1s = nested_markout(markouts, "time_horizons", "1s", "markout_lamports")
    time_2s = nested_markout(markouts, "time_horizons", "2s", "markout_lamports")
    time_5s = nested_markout(markouts, "time_horizons", "5s", "markout_lamports")
    time_10s = nested_markout(markouts, "time_horizons", "10s", "markout_lamports")
    time_30s = nested_markout(markouts, "time_horizons", "30s", "markout_lamports")
    max_adverse = first_number(row, "max_adverse_excursion_bps") or number(markouts.get("max_adverse_excursion_bps"))
    cont = first_number(row, "continuation_pressure_score")
    absorption = first_number(row, "absorption_score", "opposite_direction_absorption_score")
    follow = first_number(row, "same_direction_followthrough_score", "followthrough_score")
    restoration_score = first_number(row, "restoration_pressure_score")
    false_reason = first_text(row, "false_restoration_reason") or "missing"
    execution_ready = first_text(source, "decision") in {SENDER_READY, BUNDLE_READY}
    any_positive_markout = any(positive(v) for v in [time_50, time_100, time_250, time_500, event_1, event_2, event_3])
    event_reversal_positive = any([
        positive(event_1) and nested_bool(markouts, "event_horizons", "next_same_pool_event_1", "best_same_direction") is False,
        positive(event_2) and nested_bool(markouts, "event_horizons", "next_same_pool_event_2", "best_same_direction") is False,
        positive(event_3) and nested_bool(markouts, "event_horizons", "next_same_pool_event_3", "best_same_direction") is False,
    ])
    continuation_positive = any([
        first_bool(row, "follow_through"),
        (best_same_direction is True and any_positive_markout),
        cont is not None and cont > current_threshold,
    ])
    toxic = any([
        absorption is not None and absorption > current_threshold,
        first_bool(row, "toxic_flow_signal"),
        not adverse_bounded(max_adverse),
    ])
    restoration_positive = any_positive_markout and event_reversal_positive and adverse_bounded(max_adverse)
    looser_1_pass = gate_pass(row, looser_threshold_1)
    looser_2_pass = gate_pass(row, looser_threshold_2)
    item = {
        "candidate_id": cid,
        "original_impulse_id": first_text(row, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature") or cid,
        "slot": first_number(row, "slot"),
        "candidate_ingress_unix_ns": first_number(row, "candidate_ingress_unix_ns", "unix_ns"),
        "pool": first_text(row, "pool"),
        "venue": first_text(row, "venue"),
        "mint_pair": f"{first_text(row, 'mint_in')}->{first_text(row, 'mint_out')}",
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "owner_mode_selected": lower(first_text(row, "owner_mode_selected", "selected_owner", "owner_mode")),
        "execution_plumbing_ready": execution_ready,
        "execution_decision": first_text(source, "decision"),
        "restoration_pressure_score": restoration_score,
        "same_direction_continuation_pressure_score": cont,
        "opposite_direction_absorption_score": absorption,
        "toxic_flow_score": 1.0 if first_bool(row, "toxic_flow_signal") else 0.0,
        "route_lag_conflict_score": 1.0 if first_bool(row, "route_lag_conflict") else 0.0,
        "liquidity_migration_signal": first_bool(row, "liquidity_migration_signal"),
        "same_direction_followthrough_score": follow,
        "false_restoration_reason": false_reason,
        "current_gate_decision": "PASS" if gate_pass(row, current_threshold) else "FAIL",
        "actual_markout_at_50ms": time_50,
        "actual_markout_at_100ms": time_100,
        "actual_markout_at_250ms": time_250,
        "actual_markout_at_500ms": time_500,
        "actual_markout_at_1s": time_1s,
        "actual_markout_at_2s": time_2s,
        "actual_markout_at_5s": time_5s,
        "actual_markout_at_10s": time_10s,
        "actual_markout_at_30s": time_30s,
        "next_same_pool_event_1_markout": event_1,
        "next_same_pool_event_2_markout": event_2,
        "next_same_pool_event_3_markout": event_3,
        "best_horizon": best_horizon,
        "best_horizon_best_same_direction": best_same_direction,
        "max_adverse_excursion": max_adverse,
        "max_adverse_excursion_bounded": adverse_bounded(max_adverse),
        "best_recovery_fraction": first_number(row, "recovery_fraction_target", "target_recovery_fraction"),
        "current_threshold": current_threshold,
        "looser_threshold_1": looser_threshold_1,
        "looser_threshold_2": looser_threshold_2,
        "stricter_threshold_1": stricter_threshold_1,
        "pass_current_threshold": gate_pass(row, current_threshold),
        "pass_looser_threshold_1": looser_1_pass,
        "pass_looser_threshold_2": looser_2_pass,
        "pass_stricter_threshold_1": gate_pass(row, stricter_threshold_1),
        "would_pass_under_looser_threshold": looser_1_pass or looser_2_pass,
        "actual_restoration_positive": restoration_positive,
        "actual_continuation_positive": continuation_positive,
        "actual_toxic": toxic,
        "reopen_rebalancing_candidate": (
            execution_ready
            and looser_1_pass
            and restoration_positive
            and not continuation_positive
            and not toxic
        ),
        "recommended_route": "continuation_structural_certificate" if false_reason == "same_direction_continuation_pressure_too_high" else (
            "drop_flatten_saved_loss_analysis" if false_reason == "opposite_direction_absorption_toxic" else "park_rebalancing"
        ),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

counts = {
    "rows_seen": len(out),
    "execution_ready_rows": sum(1 for row in out if row["execution_plumbing_ready"]),
    "current_gate_pass_rows": sum(1 for row in out if row["pass_current_threshold"]),
    "looser_threshold_1_pass_rows": sum(1 for row in out if row["pass_looser_threshold_1"]),
    "looser_threshold_2_pass_rows": sum(1 for row in out if row["pass_looser_threshold_2"]),
    "stricter_threshold_1_pass_rows": sum(1 for row in out if row["pass_stricter_threshold_1"]),
    "actual_restoration_positive_rows": sum(1 for row in out if row["actual_restoration_positive"]),
    "actual_continuation_positive_rows": sum(1 for row in out if row["actual_continuation_positive"]),
    "actual_toxic_rows": sum(1 for row in out if row["actual_toxic"]),
    "execution_ready_reopen_rebalancing_candidates": sum(1 for row in out if row["reopen_rebalancing_candidate"]),
}
execution_rows = [row for row in out if row["execution_plumbing_ready"]]
execution_counts = {
    "execution_ready_rows": len(execution_rows),
    "execution_ready_current_gate_pass_rows": sum(1 for row in execution_rows if row["pass_current_threshold"]),
    "execution_ready_looser_threshold_1_pass_rows": sum(1 for row in execution_rows if row["pass_looser_threshold_1"]),
    "execution_ready_looser_threshold_2_pass_rows": sum(1 for row in execution_rows if row["pass_looser_threshold_2"]),
    "execution_ready_actual_restoration_positive_rows": sum(1 for row in execution_rows if row["actual_restoration_positive"]),
    "execution_ready_actual_continuation_positive_rows": sum(1 for row in execution_rows if row["actual_continuation_positive"]),
    "execution_ready_actual_toxic_rows": sum(1 for row in execution_rows if row["actual_toxic"]),
}
reason_counts = Counter(row["false_restoration_reason"] for row in out)
execution_reason_counts = Counter(row["false_restoration_reason"] for row in execution_rows)
route_counts = Counter(row["recommended_route"] for row in out)

if not out:
    final_verdict = "PARK_INPUTS_INCOMPLETE"
elif counts["execution_ready_reopen_rebalancing_candidates"] > 0:
    final_verdict = "FALSE_RESTORATION_THRESHOLD_REVIEW_READY"
else:
    final_verdict = "PARK_FALSE_RESTORATION_CONFIRMED"

manifest = {
    "source_rows": str(source_path or ""),
    "rule_rows": str(rule_path or ""),
    "false_restoration_rows": str(false_restoration_path or ""),
    "candidate_autopsy_rows": str(autopsy_path or ""),
    "current_threshold": current_threshold,
    "looser_threshold_1": looser_threshold_1,
    "looser_threshold_2": looser_threshold_2,
    "stricter_threshold_1": stricter_threshold_1,
    "max_adverse_bps": max_adverse_bps,
    "broad_hunt": False,
    "live_send": False,
    "sendBundle": False,
}
summary = {
    "status": final_verdict,
    "counts": counts,
    "execution_ready_counts": execution_counts,
    "false_restoration_reason_counts": dict(reason_counts),
    "execution_ready_false_restoration_reason_counts": dict(execution_reason_counts),
    "recommended_route_counts": dict(route_counts),
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
verdict = {
    "final_verdict": final_verdict,
    **counts,
    **execution_counts,
    "primary_blocker": "false_restoration_confirmed" if final_verdict == "PARK_FALSE_RESTORATION_CONFIRMED" else "inputs_or_threshold_review",
    "false_restoration_reason_counts": dict(reason_counts),
    "execution_ready_false_restoration_reason_counts": dict(execution_reason_counts),
    "recommended_route_counts": dict(route_counts),
    "proposal_path_open": False,
    "live_approval": False,
    "next_required_action": "route continuation-pressure rows to Continuation certificate and absorption-toxic rows to Drop/Flatten saved-loss audit",
    "hard_stops_preserved": summary["hard_stops_preserved"],
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
with execution_ready_path.open("w", encoding="utf-8") as fh:
    for row in execution_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
with continuation_route_path.open("w", encoding="utf-8") as fh:
    for row in out:
        if row["recommended_route"] == "continuation_structural_certificate":
            fh.write(json.dumps(row, sort_keys=True) + "\n")
with drop_flatten_route_path.open("w", encoding="utf-8") as fh:
    for row in out:
        if row["recommended_route"] == "drop_flatten_saved_loss_analysis":
            fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [f"final_verdict={final_verdict}"]
for key, value in counts.items():
    lines.append(f"{key}={value}")
for key, value in execution_counts.items():
    lines.append(f"{key}={value}")
lines.append(f"false_restoration_reason_counts={json.dumps(dict(reason_counts), sort_keys=True)}")
lines.append(f"execution_ready_false_restoration_reason_counts={json.dumps(dict(execution_reason_counts), sort_keys=True)}")
lines.append(f"recommended_route_counts={json.dumps(dict(route_counts), sort_keys=True)}")
lines.extend(["wire_send=0", "live_landed=0", "send_transaction_called=false", "sendBundle_called=false"])
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RG6 false-restoration threshold audit written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 66re_rebalancing_entry_exit_rule_proof.sh - normalize rebalancing entry/exit rule proof rows

source "$(dirname "$0")/lib.sh"
script_header "66RE - Rebalancing Entry/Exit Rule Proof"

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
STEP="${SHADOW_RUN66RE_STEP:-66re_rebalancing_entry_exit_rule_proof}"
STATUS_PREFIX="${SHADOW_RUN66RE_STATUS_PREFIX:-run66re_rebalancing_entry_exit_rule_proof}"
REPORT_BASENAME="${SHADOW_RUN66RE_REPORT_BASENAME:-run66re_rebalancing_entry_exit_rule_proof}"
SOURCE_ROWS="${SHADOW_RUN66RE_SOURCE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66_rebalancing_rule_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66re_rebalancing_entry_exit_rule_proof_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$SOURCE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, summary_file, summary_json, rows_jsonl, step, status_prefix = sys.argv[1:7]
source_rows = Path(source_arg) if source_arg else None
summary_file = Path(summary_file)
summary_json = Path(summary_json)
rows_jsonl = Path(rows_jsonl)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out

def write_jsonl(path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

def text(value):
    return str(value or "").strip()

def boolish(value):
    return bool(value)

def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def safe_float(value):
    try:
        return float(value or 0)
    except Exception:
        return 0.0

def decision_for(row):
    if not boolish(row.get("entry_rule_present")):
        return "BLOCKED_ENTRY_RULE_MISSING"
    if not boolish(row.get("exit_rule_present")):
        return "BLOCKED_EXIT_RULE_MISSING"
    if not boolish(row.get("adverse_excursion_ok")):
        return "BLOCKED_ADVERSE_EXCURSION"
    if not boolish(row.get("quote_ready")) or not boolish(row.get("numeric_quote_min_out")):
        return "BLOCKED_QUOTE_MISSING"
    if not boolish(row.get("route_usable")):
        return "BLOCKED_EXIT_RULE_MISSING"
    future_events = safe_int(row.get("future_same_pool_event_count"))
    best_horizon = text(row.get("best_horizon"))
    if future_events <= 0 and "same_pool" not in best_horizon:
        return "BLOCKED_NO_RESTORATION_SIGNAL"
    return "ENTRY_EXIT_RULE_READY"

rows = load_jsonl(source_rows)
out = []
for row in rows:
    decision = decision_for(row)
    candidate_id = text(row.get("candidate_id"))
    best_horizon = text(row.get("best_horizon")) or text(row.get("max_hold_horizon")) or "unknown"
    route_family = text(row.get("route_family") or row.get("target_route_family") or row.get("route") or "unknown")
    min_out_raw = safe_int(row.get("min_out_raw") or row.get("min_out_lamports"))
    expected_out_raw = safe_int(row.get("expected_out_raw") or row.get("quote_expected_out_lamports"))
    item = {
        "candidate_id": candidate_id,
        "selected_owner": row.get("selected_owner"),
        "selected_owner_score": row.get("selected_owner_score"),
        "owner_mode_score_rebalancing": row.get("owner_mode_score_rebalancing") or row.get("rebalancing_score"),
        "owner_mode_score_rebalancing_source": row.get("owner_mode_score_rebalancing_source") or "missing",
        "owner_mode_score_continuation": row.get("owner_mode_score_continuation") or row.get("continuation_score"),
        "owner_mode_score_backrun": row.get("owner_mode_score_backrun") or row.get("backrun_score"),
        "owner_mode_score_drop": row.get("owner_mode_score_drop") or row.get("drop_score"),
        "owner_mode_score_flatten": row.get("owner_mode_score_flatten") or row.get("flatten_score"),
        "owner_mode_score_housekeeping": row.get("owner_mode_score_housekeeping") or row.get("housekeeping_score"),
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "impulse_id": row.get("impulse_id") or candidate_id,
        "displacement_threshold": row.get("min_displacement_bps") or row.get("best_horizon_markout_bps"),
        "entry_side": row.get("entry_side") or "from_mint_in",
        "entry_trigger": row.get("entry_trigger") or best_horizon,
        "entry_quote_expected_out": expected_out_raw,
        "entry_min_out": min_out_raw,
        "exit_trigger": row.get("exit_trigger") or best_horizon,
        "exit_route": route_family,
        "exit_quote_expected_out": safe_int(row.get("exit_quote_expected_out") or row.get("exit_expected_out_raw")),
        "exit_min_out": safe_int(row.get("exit_min_out") or row.get("exit_min_out_raw")),
        "target_recovery_fraction": row.get("target_recovery_fraction") or row.get("best_horizon_recovery_fraction"),
        "recovery_fraction_target": row.get("recovery_fraction_target") or row.get("target_recovery_fraction") or row.get("best_horizon_recovery_fraction"),
        "max_hold_horizon": best_horizon,
        "max_horizon_ms": row.get("max_horizon_ms"),
        "stop_loss": row.get("stop_loss") or row.get("max_adverse_excursion_lamports"),
        "max_adverse_excursion": row.get("max_adverse_excursion_bps"),
        "reserve_displacement_raw": row.get("reserve_displacement_raw"),
        "reserve_displacement_bps": row.get("reserve_displacement_bps") or row.get("observed_displacement_bps"),
        "reserve_displacement_source": row.get("reserve_displacement_source"),
        "pre_impulse_state": row.get("pre_impulse_state"),
        "post_impulse_state": row.get("post_impulse_state"),
        "restoration_pressure_score": row.get("restoration_pressure_score"),
        "restoration_pressure_measured": boolish(row.get("restoration_pressure_measured")),
        "restoration_pressure_source": row.get("restoration_pressure_source"),
        "expected_restoration_path": row.get("expected_restoration_path"),
        "exit_side": row.get("exit_side"),
        "continuation_pressure_score": row.get("continuation_pressure_score"),
        "same_direction_followthrough_score": row.get("same_direction_followthrough_score"),
        "absorption_score": row.get("absorption_score"),
        "route_lag_conflict": boolish(row.get("route_lag_conflict")),
        "liquidity_migration_signal": boolish(row.get("liquidity_migration_signal")),
        "toxic_flow_signal": boolish(row.get("toxic_flow_signal")),
        "false_restoration_gate_input_ready": boolish(row.get("false_restoration_gate_input_ready")),
        "false_restoration_reason": row.get("false_restoration_reason"),
        "pm_owner_lock_ready": boolish(row.get("pm_owner_lock_ready")),
        "pm_owner_lock_owner_mode": row.get("pm_owner_lock_owner_mode"),
        "pm_owner_lock_candidate_id": row.get("pm_owner_lock_candidate_id"),
        "pm_owner_lock_conflict_domain": row.get("pm_owner_lock_conflict_domain"),
        "pm_owner_lock_status": row.get("pm_owner_lock_status"),
        "pm_owner_lock_source": row.get("pm_owner_lock_source"),
        "pm_owner_lock_notional_raw": row.get("pm_owner_lock_notional_raw"),
        "pm_reserved_notional_raw": row.get("pm_reserved_notional_raw"),
        "pm_reserved_fee_raw": row.get("pm_reserved_fee_raw"),
        "pm_reserved_tip_raw": row.get("pm_reserved_tip_raw"),
        "pm_release_on_no_send_ok": boolish(row.get("pm_release_on_no_send_ok")),
        "pm_pending_after": row.get("pm_pending_after"),
        "entry_rule_present": boolish(row.get("entry_rule_present")),
        "exit_rule_present": boolish(row.get("exit_rule_present")),
        "quote_ready": boolish(row.get("quote_ready")),
        "numeric_quote_min_out": boolish(row.get("numeric_quote_min_out")),
        "adverse_excursion_ok": boolish(row.get("adverse_excursion_ok")),
        "route_usable": boolish(row.get("route_usable")),
        "pm_reserve_sim_ok": boolish(row.get("pm_reserve_sim_ok")),
        "template_ready": boolish(row.get("template_ready")),
        "account_ready": boolish(row.get("account_ready")),
        "inventory_ready": boolish(row.get("inventory_ready")),
        "positive_after_cost": boolish(row.get("positive_after_cost")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

write_jsonl(rows_jsonl, out)
decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": f"{status_prefix}_complete" if source_rows and source_rows.exists() else f"{status_prefix}_missing_source",
    "step": step,
    "source_rows": str(source_rows) if source_rows else "",
    "rebalancing_rows_seen": len(out),
    "entry_exit_rule_ready_count": decision_counts.get("ENTRY_EXIT_RULE_READY", 0),
    "entry_rule_ready_rows": sum(1 for row in out if row["entry_rule_present"]),
    "exit_rule_ready_rows": sum(1 for row in out if row["exit_rule_present"]),
    "quote_min_out_ready_rows": sum(1 for row in out if row["quote_ready"] and row["numeric_quote_min_out"]),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_rows={summary['source_rows']}",
    f"rebalancing_rows_seen={summary['rebalancing_rows_seen']}",
    f"entry_exit_rule_ready_count={summary['entry_exit_rule_ready_count']}",
    f"entry_rule_ready_rows={summary['entry_rule_ready_rows']}",
    f"exit_rule_ready_rows={summary['exit_rule_ready_rows']}",
    f"quote_min_out_ready_rows={summary['quote_min_out_ready_rows']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RE entry/exit rule proof written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

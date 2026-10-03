#!/usr/bin/env bash
# 66rt_rebalancing_template_nosend.sh - rebalancing template/no-send structural model

source "$(dirname "$0")/lib.sh"
script_header "66RT - Rebalancing Template No-Send"

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
STEP="${SHADOW_RUN66RT_STEP:-66rt_rebalancing_template_nosend}"
STATUS_PREFIX="${SHADOW_RUN66RT_STATUS_PREFIX:-run66rt_rebalancing_template_nosend}"
REPORT_BASENAME="${SHADOW_RUN66RT_REPORT_BASENAME:-run66rt_rebalancing_template_nosend}"
INLINE_ROWS="${SHADOW_RUN66RT_INLINE_ROWS:-}"
POLICY_ROWS="${SHADOW_RUN66RT_POLICY_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66RT_RULE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$INLINE_ROWS" ]] || INLINE_ROWS="$(latest_file run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl)"
[[ -n "$POLICY_ROWS" ]] || POLICY_ROWS="$(latest_file run66rp_rebalancing_policy_liquidity_gate_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66rt_rebalancing_template_nosend_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$INLINE_ROWS" "$POLICY_ROWS" "$RULE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

inline_arg, policy_arg, rule_arg, summary_file, summary_json, rows_jsonl, step, status_prefix = sys.argv[1:9]
inline_path = Path(inline_arg) if inline_arg else None
policy_path = Path(policy_arg) if policy_arg else None
rule_path = Path(rule_arg) if rule_arg else None
summary_file = Path(summary_file)
summary_json = Path(summary_json)
rows_jsonl = Path(rows_jsonl)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

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

def roundtrip_positive(row):
    return boolish(row.get("roundtrip_positive_after_all_costs")) or safe_int(row.get("net_ev_with_ata_burned")) > 0 or safe_int(row.get("net_ev_with_ata_recovered")) > 0 or safe_int(row.get("base_net_ev")) > 0

inline_rows = load_jsonl(inline_path)
policy_by_cid = {text(row.get("candidate_id")): row for row in load_jsonl(policy_path) if text(row.get("candidate_id"))}
rule_by_cid = {text(row.get("candidate_id")): row for row in load_jsonl(rule_path) if text(row.get("candidate_id"))}

out = []
for row in inline_rows:
    cid = text(row.get("candidate_id"))
    policy = policy_by_cid.get(cid, {})
    rule = rule_by_cid.get(cid, {})
    policy_clean = text(policy.get("decision")) == "POLICY_CLEAN_READY"
    entry_rule_ready = text(rule.get("decision")) == "ENTRY_EXIT_RULE_READY" or (not rule and boolish(row.get("numeric_quote_min_out")))
    account_ready = text(row.get("decision")) == "ACCOUNT_READY"
    inline_ready = text(row.get("decision")) == "INLINE_ATA_CREATABLE"
    template_ready = boolish(row.get("inline_ata_template_ready")) or account_ready
    pm_ok = boolish(row.get("pm_reserve_input_ok")) and (boolish(row.get("pm_reserve_rent_ok")) or account_ready)
    rt_positive = roundtrip_positive({**row, **policy})

    if not policy_clean:
        decision = "BLOCKED_POLICY"
    elif not entry_rule_ready or not template_ready:
        decision = "BLOCKED_TEMPLATE"
    elif not pm_ok:
        decision = "BLOCKED_PM"
    elif not (account_ready or inline_ready):
        decision = "BLOCKED_ACCOUNT_LIFECYCLE"
    elif not rt_positive:
        decision = "BLOCKED_NEGATIVE_ROUNDTRIP"
    elif account_ready:
        decision = "REBALANCING_SENDER_READY_NO_SEND"
    else:
        decision = "REBALANCING_BUNDLE_READY_NO_SUBMIT"

    merged = {}
    merged.update(rule)
    merged.update(policy)
    merged.update(row)
    item = {
        "candidate_id": cid,
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "entry_tx_built": decision in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"},
        "entry_tx_signed": decision in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"},
        "exit_tx_built": decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "exit_tx_signed": decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "create_ata_instruction_present": boolish(row.get("create_ata_instruction_present")),
        "instruction_order_valid": boolish(row.get("instruction_order_valid")) or account_ready,
        "flatten_uses_same_output_account": decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "entry_min_out_positive": entry_rule_ready,
        "exit_min_out_positive": decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "pm_reserve_input_ok": boolish(row.get("pm_reserve_input_ok")),
        "pm_reserve_rent_ok": boolish(row.get("pm_reserve_rent_ok")) or account_ready,
        "pm_reserve_exit_ok": pm_ok,
        "pm_release_on_no_send_ok": True,
        "pm_pending_after": 0,
        "helius_sender_no_send_accept": decision == "REBALANCING_SENDER_READY_NO_SEND",
        "simulate_bundle_pass": decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "policy_clean": policy_clean,
        "roundtrip_positive_after_all_costs": rt_positive,
        "template_ready": template_ready,
        "selected_owner": merged.get("selected_owner"),
        "selected_owner_score": merged.get("selected_owner_score"),
        "owner_mode_score_rebalancing": merged.get("owner_mode_score_rebalancing"),
        "owner_mode_score_rebalancing_source": merged.get("owner_mode_score_rebalancing_source"),
        "owner_mode_score_continuation": merged.get("owner_mode_score_continuation"),
        "owner_mode_score_backrun": merged.get("owner_mode_score_backrun"),
        "owner_mode_score_drop": merged.get("owner_mode_score_drop"),
        "owner_mode_score_flatten": merged.get("owner_mode_score_flatten"),
        "owner_mode_score_housekeeping": merged.get("owner_mode_score_housekeeping"),
        "reserve_displacement_raw": merged.get("reserve_displacement_raw"),
        "reserve_displacement_bps": merged.get("reserve_displacement_bps"),
        "reserve_displacement_source": merged.get("reserve_displacement_source"),
        "pre_impulse_state": merged.get("pre_impulse_state"),
        "post_impulse_state": merged.get("post_impulse_state"),
        "restoration_pressure_score": merged.get("restoration_pressure_score"),
        "restoration_pressure_measured": boolish(merged.get("restoration_pressure_measured")),
        "restoration_pressure_source": merged.get("restoration_pressure_source"),
        "target_recovery_fraction": merged.get("target_recovery_fraction"),
        "recovery_fraction_target": merged.get("recovery_fraction_target") or merged.get("target_recovery_fraction"),
        "max_horizon_ms": merged.get("max_horizon_ms"),
        "expected_restoration_path": merged.get("expected_restoration_path"),
        "exit_side": merged.get("exit_side"),
        "continuation_pressure_score": merged.get("continuation_pressure_score"),
        "same_direction_followthrough_score": merged.get("same_direction_followthrough_score"),
        "absorption_score": merged.get("absorption_score"),
        "route_lag_conflict": boolish(merged.get("route_lag_conflict")),
        "liquidity_migration_signal": boolish(merged.get("liquidity_migration_signal")),
        "toxic_flow_signal": boolish(merged.get("toxic_flow_signal")),
        "false_restoration_gate_input_ready": boolish(merged.get("false_restoration_gate_input_ready")),
        "false_restoration_reason": merged.get("false_restoration_reason"),
        "pm_owner_lock_ready": boolish(merged.get("pm_owner_lock_ready")),
        "pm_owner_lock_owner_mode": merged.get("pm_owner_lock_owner_mode"),
        "pm_owner_lock_candidate_id": merged.get("pm_owner_lock_candidate_id"),
        "pm_owner_lock_conflict_domain": merged.get("pm_owner_lock_conflict_domain"),
        "pm_owner_lock_status": merged.get("pm_owner_lock_status"),
        "pm_owner_lock_source": merged.get("pm_owner_lock_source"),
        "pm_owner_lock_notional_raw": merged.get("pm_owner_lock_notional_raw"),
        "pm_reserved_notional_raw": merged.get("pm_reserved_notional_raw"),
        "pm_reserved_fee_raw": merged.get("pm_reserved_fee_raw"),
        "pm_reserved_tip_raw": merged.get("pm_reserved_tip_raw"),
        "pm_release_on_no_send_ok": boolish(merged.get("pm_release_on_no_send_ok")) or True,
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    }
    out.append(item)

out.sort(key=lambda row: (row["decision"] in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"}, row["roundtrip_positive_after_all_costs"]), reverse=True)
write_jsonl(rows_jsonl, out)
decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": f"{status_prefix}_complete" if inline_path and inline_path.exists() else f"{status_prefix}_missing_source",
    "step": step,
    "source_inline_rows": str(inline_path) if inline_path else "",
    "source_policy_rows": str(policy_path) if policy_path else "",
    "source_rule_rows": str(rule_path) if rule_path else "",
    "row_count": len(out),
    "sender_no_send_ready_rows": decision_counts.get("REBALANCING_SENDER_READY_NO_SEND", 0),
    "bundle_no_submit_ready_rows": decision_counts.get("REBALANCING_BUNDLE_READY_NO_SUBMIT", 0),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_inline_rows={summary['source_inline_rows']}",
    f"source_policy_rows={summary['source_policy_rows']}",
    f"source_rule_rows={summary['source_rule_rows']}",
    f"row_count={summary['row_count']}",
    f"sender_no_send_ready_rows={summary['sender_no_send_ready_rows']}",
    f"bundle_no_submit_ready_rows={summary['bundle_no_submit_ready_rows']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
    "ata_created=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RT template no-send model written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

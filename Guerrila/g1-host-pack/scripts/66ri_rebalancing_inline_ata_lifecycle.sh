#!/usr/bin/env bash
# 66ri_rebalancing_inline_ata_lifecycle.sh - normalize inline ATA lifecycle readiness without repeatability gating

source "$(dirname "$0")/lib.sh"
script_header "66RI - Rebalancing Inline ATA Lifecycle"

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
STEP="${SHADOW_RUN66RI_STEP:-66ri_rebalancing_inline_ata_lifecycle}"
STATUS_PREFIX="${SHADOW_RUN66RI_STATUS_PREFIX:-run66ri_rebalancing_inline_ata_lifecycle}"
REPORT_BASENAME="${SHADOW_RUN66RI_REPORT_BASENAME:-run66ri_rebalancing_inline_ata_lifecycle}"
OVERLAY_ROWS="${SHADOW_RUN66RI_OVERLAY_ROWS:-}"
INLINE_ROWS="${SHADOW_RUN66RI_INLINE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$OVERLAY_ROWS" ]] || OVERLAY_ROWS="$(latest_file run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl)"
[[ -n "$INLINE_ROWS" ]] || INLINE_ROWS="$(latest_file run66v_inline_ata_lifecycle_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$OVERLAY_ROWS" "$INLINE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

overlay_arg, inline_arg, summary_file, summary_json, rows_jsonl, step, status_prefix = sys.argv[1:8]
overlay_path = Path(overlay_arg) if overlay_arg else None
inline_path = Path(inline_arg) if inline_arg else None
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

def decision_for(row):
    if boolish(row.get("account_ready")):
        return "ACCOUNT_READY"
    if boolish(row.get("hard_policy_risk")) or (boolish(row.get("explicit_allow_required")) and not boolish(row.get("policy_actionable_now"))):
        return "INLINE_ATA_POLICY_BLOCKED"
    if boolish(row.get("exit_dust")):
        return "INLINE_ATA_EXIT_DUST"
    if not boolish(row.get("inline_ata_creatable")):
        return "INLINE_ATA_POLICY_BLOCKED"
    if not boolish(row.get("pm_reserve_rent_ok")):
        return "INLINE_ATA_RENT_NEGATIVE"
    if not boolish(row.get("inline_ata_template_ready")):
        return "INLINE_ATA_TEMPLATE_BLOCKED"
    if not boolish(row.get("inline_ata_ev_positive")):
        return "INLINE_ATA_RENT_NEGATIVE"
    return "INLINE_ATA_CREATABLE"

overlay_rows = load_jsonl(overlay_path)
inline_rows = load_jsonl(inline_path)
inline_by_cid = {text(row.get("candidate_id")): row for row in inline_rows if text(row.get("candidate_id"))}

out = []
seen = set()
for base in overlay_rows:
    cid = text(base.get("candidate_id"))
    if not cid:
        continue
    seen.add(cid)
    inline = inline_by_cid.get(cid, {})
    merged = dict(base)
    merged.update({k: v for k, v in inline.items() if v not in (None, "")})
    account_ready = boolish(base.get("account_ready")) or boolish(base.get("baseline_would_fire"))
    merged["account_ready"] = account_ready
    decision = decision_for(merged)
    item = {
        "candidate_id": cid,
        "pool": merged.get("pool"),
        "venue": merged.get("venue"),
        "mint_in": merged.get("mint_in"),
        "mint_out": merged.get("mint_out"),
        "base_net_ev": safe_int(merged.get("matched_rollout_net_ev_after_fee_tip_raw") or merged.get("route_cost_model_net_ev_after_fee_tip_raw")),
        "account_ready": account_ready,
        "inline_ata_creatable": boolish(merged.get("inline_ata_creatable")) or decision == "ACCOUNT_READY",
        "inline_ata_template_ready": boolish(merged.get("inline_ata_template_ready")) or account_ready,
        "create_ata_instruction_present": boolish(merged.get("create_ata_instruction_present")),
        "instruction_order_valid": boolish(merged.get("instruction_order_valid")) or boolish(merged.get("inline_ata_template_ready")) or account_ready,
        "ata_create_rent_lamports": safe_int(merged.get("ata_create_rent_lamports")),
        "ata_create_fee_lamports": safe_int(merged.get("ata_create_fee_estimate_lamports") or merged.get("ata_create_fee_lamports")),
        "extra_cu_cost": safe_int(merged.get("extra_cu_cost")),
        "extra_tip_cost": safe_int(merged.get("extra_tip_cost")),
        "rent_recovered": False,
        "exit_quote_value": merged.get("full_band_exit_probe_value_usdc") or merged.get("exit_quote_value"),
        "dust_after_exit": boolish(merged.get("exit_dust")),
        "net_ev_with_ata_burned": safe_int(merged.get("net_ev_with_ata_burned")),
        "net_ev_with_ata_recovered": safe_int(merged.get("net_ev_with_ata_recovered")),
        "inline_ata_ev_positive": boolish(merged.get("inline_ata_ev_positive")) or account_ready,
        "pm_reserve_input_ok": boolish(merged.get("pm_reserve_input_ok") or merged.get("pm_reserve_sim_ok")),
        "pm_reserve_rent_ok": boolish(merged.get("pm_reserve_rent_ok")) or account_ready,
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
        "pm_release_on_no_send_ok": boolish(merged.get("pm_release_on_no_send_ok")),
        "pm_pending_after": merged.get("pm_pending_after"),
        "policy_verdict": merged.get("policy_verdict"),
        "policy_actionable_now": boolish(merged.get("policy_actionable_now")),
        "explicit_allow_required": boolish(merged.get("explicit_allow_required")),
        "hard_policy_risk": boolish(merged.get("hard_policy_risk")),
        "token_program": merged.get("token_program"),
        "extensions": merged.get("extensions") or [],
        "exit_dust": boolish(merged.get("exit_dust")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    }
    out.append(item)

for inline in inline_rows:
    cid = text(inline.get("candidate_id"))
    if not cid or cid in seen:
        continue
    decision = decision_for(inline)
    item = dict(inline)
    item.update({
        "account_ready": False,
        "base_net_ev": safe_int(inline.get("matched_rollout_net_ev_after_fee_tip_raw")),
        "ata_create_fee_lamports": safe_int(inline.get("ata_create_fee_estimate_lamports") or inline.get("ata_create_fee_lamports")),
        "rent_recovered": False,
        "dust_after_exit": boolish(inline.get("exit_dust")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    })
    out.append(item)

out.sort(key=lambda row: (row["decision"] in {"ACCOUNT_READY", "INLINE_ATA_CREATABLE"}, safe_int(row.get("base_net_ev"))), reverse=True)
write_jsonl(rows_jsonl, out)
decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": f"{status_prefix}_complete" if (overlay_path and overlay_path.exists()) or (inline_path and inline_path.exists()) else f"{status_prefix}_missing_source",
    "step": step,
    "source_overlay_rows": str(overlay_path) if overlay_path else "",
    "source_inline_rows": str(inline_path) if inline_path else "",
    "row_count": len(out),
    "account_ready_count": decision_counts.get("ACCOUNT_READY", 0),
    "inline_ata_creatable_count": decision_counts.get("INLINE_ATA_CREATABLE", 0),
    "inline_ata_ev_positive_count": sum(1 for row in out if boolish(row.get("inline_ata_ev_positive"))),
    "pm_reserve_rent_ok_count": sum(1 for row in out if boolish(row.get("pm_reserve_rent_ok"))),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_overlay_rows={summary['source_overlay_rows']}",
    f"source_inline_rows={summary['source_inline_rows']}",
    f"row_count={summary['row_count']}",
    f"account_ready_count={summary['account_ready_count']}",
    f"inline_ata_creatable_count={summary['inline_ata_creatable_count']}",
    f"inline_ata_ev_positive_count={summary['inline_ata_ev_positive_count']}",
    f"pm_reserve_rent_ok_count={summary['pm_reserve_rent_ok_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RI inline ATA lifecycle written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

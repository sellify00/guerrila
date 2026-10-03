#!/usr/bin/env bash
# 66ct2_continuation_execution_rule_proof.sh - executable no-send rule proof for Continuation candidates

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CT2 - Continuation Execution Rule Proof"

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
STEP="${SHADOW_RUN66CT2_STEP:-66ct2_continuation_execution_rule_proof}"
SOURCE_ROWS="${SHADOW_RUN66CT2_SOURCE_ROWS:-}"
MAX_ADVERSE_BPS="${SHADOW_RUN66CT2_MAX_ADVERSE_BPS:-50}"

latest_file() {
    local cta
    cta="$(find "$RESULTS_ROOT" -type f -name run66cta_continuation_account_template_policy_autopsy_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"
    if [[ -n "$cta" ]]; then
        printf '%s\n' "$cta"
        return 0
    fi
    find "$RESULTS_ROOT" -type f -name run66ct1_continuation_owner_eligibility_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}
[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ct2_continuation_execution_rule_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ct2_continuation_execution_rule_proof.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$MAX_ADVERSE_BPS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, max_adverse_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, summary_txt_arg = sys.argv[1:8]
source_path = Path(source_arg) if source_arg else None
max_adverse_bps = float(max_adverse_arg or 50)
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

def blocker_from_account_policy_template(row):
    blocker = text(row.get("first_account_policy_template_blocker"))
    if blocker in {"", "none"}:
        return ""
    if blocker == "BLOCKED_ENTRY_RULE_CARRY_FORWARD":
        return "BLOCKED_ENTRY_RULE"
    if blocker == "BLOCKED_EXIT_RULE_CARRY_FORWARD":
        return "BLOCKED_EXIT_RULE"
    if blocker in {"BLOCKED_INPUT_ACCOUNT", "BLOCKED_OUTPUT_ACCOUNT", "BLOCKED_INLINE_ATA_POLICY", "BLOCKED_INLINE_ATA_RENT"}:
        return "BLOCKED_ACCOUNT_OR_ATA"
    if blocker in {
        "BLOCKED_TOKEN_POLICY",
        "BLOCKED_FREEZE_AUTHORITY",
        "BLOCKED_TOKEN2022_EXPLICIT_ALLOW",
        "BLOCKED_TEMPLATE_FAMILY_UNSUPPORTED",
        "BLOCKED_TEMPLATE_INPUTS_MISSING",
        "BLOCKED_ROUTE_FAMILY",
    }:
        return "BLOCKED_TEMPLATE"
    return "BLOCKED_TEMPLATE"

def first_blocker(row):
    mapped = blocker_from_account_policy_template(row)
    if mapped == "BLOCKED_ENTRY_RULE" and not row["entry_trigger_ready"]:
        return mapped
    if mapped == "BLOCKED_EXIT_RULE" and (not row["hold_horizon_defined"] or not row["exit_trigger_ready"]):
        return mapped
    checks = [
        ("BLOCKED_ENTRY_RULE", row["entry_trigger_ready"]),
        ("BLOCKED_CONFIRMATION", row["confirmation_condition_ready"]),
        ("BLOCKED_ENTRY_RULE", row["entry_delay_or_event_gate_ready"]),
        ("BLOCKED_ENTRY_RULE", row["size_rule_ready"]),
        ("BLOCKED_EXIT_RULE", row["hold_horizon_defined"]),
        ("BLOCKED_EXIT_RULE", row["exit_trigger_ready"]),
        ("BLOCKED_ADVERSE_EXCURSION", row["stop_loss_defined"]),
        ("BLOCKED_ADVERSE_EXCURSION", row["max_adverse_excursion_bounded"]),
        ("BLOCKED_TEMPLATE", row["quote_min_out_ready"]),
        ("BLOCKED_PM", row["pm_owner_lock_ready"]),
        ("BLOCKED_ACCOUNT_OR_ATA", row["account_or_inline_ata_ready"]),
        ("BLOCKED_TEMPLATE", row["template_ready"]),
        ("BLOCKED_TEMPLATE", row["egress_no_send_ready"]),
        ("BLOCKED_NEGATIVE_AFTER_COST", row["net_ev_after_all_costs_positive"]),
    ]
    for blocker, ok in checks:
        if not ok:
            if blocker in {"BLOCKED_ACCOUNT_OR_ATA", "BLOCKED_TEMPLATE"} and mapped:
                return mapped
            return blocker
    if mapped:
        return mapped
    return "none"

out = []
for row in load_jsonl(source_path):
    pressure = first_num(row, "same_direction_continuation_pressure_score", "same_direction_followthrough_score")
    max_adv = first_num(row, "max_adverse_excursion")
    min_out = first_num(row, "min_out_raw")
    expected_out = first_num(row, "expected_out_raw")
    ev = first_num(
        row,
        "route_specific_ev_after_cost_raw",
        "base_net_ev",
        "net_ev_with_ata_burned",
        "net_ev_with_ata_recovered",
        "matched_rollout_net_ev_after_fee_tip_raw",
        "route_cost_model_net_ev_after_fee_tip_raw",
    )
    execution_decision = text(row.get("execution_decision"))
    item = {
        **row,
        "entry_trigger_ready": boolish(row.get("entry_rule_ready")) if "entry_rule_ready" in row else boolish(row.get("owner_mode_match_continuation")) and pressure is not None and pressure > 0,
        "confirmation_condition_ready": boolish(row.get("confirmation_condition_ready")) if "confirmation_condition_ready" in row else pressure is not None and pressure >= 0.5,
        "entry_delay_or_event_gate_ready": boolish(row.get("hold_horizon_defined")) if "hold_horizon_defined" in row else bool(text(row.get("best_horizon"))) or boolish(row.get("actual_continuation_positive")),
        "size_rule_ready": boolish(row.get("pm_owner_lock_ready")),
        "hold_horizon_defined": boolish(row.get("hold_horizon_defined")) if "hold_horizon_defined" in row else bool(text(row.get("best_horizon"))),
        "exit_trigger_ready": boolish(row.get("exit_rule_ready")) if "exit_rule_ready" in row else bool(text(row.get("best_horizon"))),
        "stop_loss_defined": boolish(row.get("stop_loss_defined")) if "stop_loss_defined" in row else max_adv is not None,
        "max_adverse_excursion_bounded": boolish(row.get("max_adverse_excursion_bounded")) if "max_adverse_excursion_bounded" in row else max_adv is not None and abs(min(0.0, max_adv)) <= max_adverse_bps,
        "quote_min_out_ready": boolish(row.get("entry_rule_ready")) or (min_out is not None and min_out > 0) or (expected_out is not None and expected_out > 0),
        "pm_owner_lock_ready": boolish(row.get("pm_owner_lock_ready")),
        "account_or_inline_ata_ready": boolish(row.get("account_or_inline_ata_ready")) if "account_or_inline_ata_ready" in row else boolish(row.get("execution_ready_from_66rg6")),
        "template_ready": boolish(row.get("template_ready")) if "template_ready" in row else boolish(row.get("execution_ready_from_66rg6")),
        "helius_sender_no_send_ready": boolish(row.get("sender_possible")) or boolish(row.get("helius_sender_no_send_accept")) or execution_decision == "REBALANCING_SENDER_READY_NO_SEND",
        "bundle_no_submit_ready": boolish(row.get("jito_bundle_possible")) or boolish(row.get("simulate_bundle_pass")) or execution_decision == "REBALANCING_BUNDLE_READY_NO_SUBMIT",
        "net_ev_after_all_costs_positive": boolish(row.get("net_ev_after_all_costs_positive")) or boolish(row.get("roundtrip_positive_after_all_costs")) or (ev is not None and ev > 0),
        "first_account_policy_template_blocker": text(row.get("first_account_policy_template_blocker")) or "none",
        "max_adverse_limit_bps": max_adverse_bps,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    item["egress_no_send_ready"] = item["helius_sender_no_send_ready"] or item["bundle_no_submit_ready"]
    blocker = first_blocker(item)
    item["first_execution_blocker"] = blocker
    item["decision"] = "CONTINUATION_EXECUTION_PLUMBING_READY" if blocker == "none" else blocker
    out.append(item)

counts = Counter(row["decision"] for row in out)
summary = {
    "status": "run66ct2_continuation_execution_rule_proof_complete",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "continuation_execution_plumbing_ready_rows": counts.get("CONTINUATION_EXECUTION_PLUMBING_READY", 0),
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

pass "Run66CT2 continuation execution rule proof written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

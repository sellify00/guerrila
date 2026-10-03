#!/usr/bin/env bash
# 66rb_rebalancing_structural_readiness.sh - aggregate rebalancing execution-plumbing readiness certificate

source "$(dirname "$0")/lib.sh"
script_header "66RB - Rebalancing Execution-Plumbing Readiness"

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
STEP="${SHADOW_RUN66RB_STEP:-66rb_rebalancing_structural_readiness}"
STATUS_PREFIX="${SHADOW_RUN66RB_STATUS_PREFIX:-run66rb_rebalancing_structural_readiness}"
REPORT_BASENAME="${SHADOW_RUN66RB_REPORT_BASENAME:-run66rb_rebalancing_structural_readiness}"
RULE_ROWS="${SHADOW_RUN66RB_RULE_ROWS:-}"
INLINE_ROWS="${SHADOW_RUN66RB_INLINE_ROWS:-}"
POLICY_ROWS="${SHADOW_RUN66RB_POLICY_ROWS:-}"
TEMPLATE_ROWS="${SHADOW_RUN66RB_TEMPLATE_ROWS:-}"
OVERLAY_ROWS="${SHADOW_RUN66RB_OVERLAY_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"
[[ -n "$INLINE_ROWS" ]] || INLINE_ROWS="$(latest_file run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl)"
[[ -n "$POLICY_ROWS" ]] || POLICY_ROWS="$(latest_file run66rp_rebalancing_policy_liquidity_gate_rows.jsonl)"
[[ -n "$TEMPLATE_ROWS" ]] || TEMPLATE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$OVERLAY_ROWS" ]] || OVERLAY_ROWS="$(latest_file run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

python3 - "$RULE_ROWS" "$INLINE_ROWS" "$POLICY_ROWS" "$TEMPLATE_ROWS" "$OVERLAY_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

rule_arg, inline_arg, policy_arg, template_arg, overlay_arg, summary_file, summary_json, step, status_prefix = sys.argv[1:10]
paths = {
    "rule": Path(rule_arg) if rule_arg else None,
    "inline": Path(inline_arg) if inline_arg else None,
    "policy": Path(policy_arg) if policy_arg else None,
    "template": Path(template_arg) if template_arg else None,
    "overlay": Path(overlay_arg) if overlay_arg else None,
}
summary_file = Path(summary_file)
summary_json = Path(summary_json)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def boolish(value):
    return bool(value)

def text(value):
    return str(value or "").strip()

rule_rows = load_jsonl(paths["rule"])
inline_rows = load_jsonl(paths["inline"])
policy_rows = load_jsonl(paths["policy"])
template_rows = load_jsonl(paths["template"])
overlay_rows = load_jsonl(paths["overlay"])

ready_decisions = {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"}
template_ready_rows = [row for row in template_rows if row.get("decision") in ready_decisions]
template_decisions = Counter(row.get("decision") for row in template_rows)
inline_decisions = Counter(row.get("decision") for row in inline_rows)
policy_decisions = Counter(row.get("decision") for row in policy_rows)
rule_decisions = Counter(row.get("decision") for row in rule_rows)

architecture_blockers = Counter()
rejected_row_blockers = Counter()
for decision, count in rule_decisions.items():
    if decision not in {"ENTRY_EXIT_RULE_READY"}:
        rejected_row_blockers[decision] += count
for decision, count in inline_decisions.items():
    if decision not in {"ACCOUNT_READY", "INLINE_ATA_CREATABLE"}:
        rejected_row_blockers[decision] += count
for decision, count in policy_decisions.items():
    if decision not in {"POLICY_CLEAN_READY", "REVIEW_METADATA_ONLY_TOKEN2022"}:
        rejected_row_blockers[decision] += count
for decision, count in template_decisions.items():
    if decision not in ready_decisions:
        rejected_row_blockers[decision] += count

market_blockers = Counter()
if not rule_rows:
    architecture_blockers["NO_REBALANCING_RULE_ROWS"] += 1
if rule_rows and rule_decisions.get("ENTRY_EXIT_RULE_READY", 0) == 0:
    market_blockers["NO_ENTRY_EXIT_READY_ROW"] += 1
if overlay_rows and not any(boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive")) for row in overlay_rows):
    market_blockers["MARKET_EV_NEGATIVE"] += 1

structural_ready_rows = len(template_ready_rows)
if structural_ready_rows <= 0:
    architecture_blockers.update(rejected_row_blockers)
status = "REBALANCING_EXECUTION_PLUMBING_READY" if structural_ready_rows > 0 and not architecture_blockers else "BLOCKED_ARCHITECTURE"

summary = {
    "status": status,
    "status_meaning": "execution_plumbing_structural_ready",
    "certificate_scope": "execution_plumbing_only",
    "owner_mode_eligible": False,
    "proposal_eligible": False,
    "proposal_eligible_rebalancing_rows": 0,
    "live_approval": False,
    "superseded_structural_hunt_ready_wording": True,
    "step": step,
    "source_rule_rows": str(paths["rule"]) if paths["rule"] else "",
    "source_inline_rows": str(paths["inline"]) if paths["inline"] else "",
    "source_policy_rows": str(paths["policy"]) if paths["policy"] else "",
    "source_template_rows": str(paths["template"]) if paths["template"] else "",
    "source_overlay_rows": str(paths["overlay"]) if paths["overlay"] else "",
    "rebalancing_rows_seen": len(rule_rows),
    "exact_positive_rows": len(overlay_rows),
    "quote_min_out_ready_rows": sum(1 for row in rule_rows if boolish(row.get("quote_ready")) and boolish(row.get("numeric_quote_min_out"))),
    "entry_rule_ready_rows": sum(1 for row in rule_rows if boolish(row.get("entry_rule_present"))),
    "exit_rule_ready_rows": sum(1 for row in rule_rows if boolish(row.get("exit_rule_present"))),
    "pm_ready_rows": sum(1 for row in inline_rows if boolish(row.get("pm_reserve_input_ok")) and (boolish(row.get("pm_reserve_rent_ok")) or row.get("decision") == "ACCOUNT_READY")),
    "account_ready_rows": inline_decisions.get("ACCOUNT_READY", 0),
    "inline_ata_possible_rows": inline_decisions.get("INLINE_ATA_CREATABLE", 0),
    "template_ready_rows": sum(1 for row in template_rows if boolish(row.get("template_ready"))),
    "sender_no_send_ready_rows": template_decisions.get("REBALANCING_SENDER_READY_NO_SEND", 0),
    "bundle_no_submit_ready_rows": template_decisions.get("REBALANCING_BUNDLE_READY_NO_SUBMIT", 0),
    "roundtrip_positive_rows": sum(1 for row in template_rows if boolish(row.get("roundtrip_positive_after_all_costs"))),
    "structural_ready_rows": structural_ready_rows,
    "architecture_blocker_counts": dict(architecture_blockers),
    "rejected_row_blocker_counts": dict(rejected_row_blockers),
    "market_blocker_counts": dict(market_blockers),
    "rule_decision_counts": dict(rule_decisions),
    "inline_decision_counts": dict(inline_decisions),
    "policy_decision_counts": dict(policy_decisions),
    "template_decision_counts": dict(template_decisions),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"status_meaning={summary['status_meaning']}",
    f"certificate_scope={summary['certificate_scope']}",
    "owner_mode_eligible=false",
    "proposal_eligible=false",
    "superseded_structural_hunt_ready_wording=true",
    f"source_rule_rows={summary['source_rule_rows']}",
    f"source_inline_rows={summary['source_inline_rows']}",
    f"source_policy_rows={summary['source_policy_rows']}",
    f"source_template_rows={summary['source_template_rows']}",
    f"rebalancing_rows_seen={summary['rebalancing_rows_seen']}",
    f"exact_positive_rows={summary['exact_positive_rows']}",
    f"quote_min_out_ready_rows={summary['quote_min_out_ready_rows']}",
    f"entry_rule_ready_rows={summary['entry_rule_ready_rows']}",
    f"exit_rule_ready_rows={summary['exit_rule_ready_rows']}",
    f"pm_ready_rows={summary['pm_ready_rows']}",
    f"account_ready_rows={summary['account_ready_rows']}",
    f"inline_ata_possible_rows={summary['inline_ata_possible_rows']}",
    f"template_ready_rows={summary['template_ready_rows']}",
    f"sender_no_send_ready_rows={summary['sender_no_send_ready_rows']}",
    f"bundle_no_submit_ready_rows={summary['bundle_no_submit_ready_rows']}",
    f"roundtrip_positive_rows={summary['roundtrip_positive_rows']}",
    f"structural_ready_rows={summary['structural_ready_rows']}",
    f"proposal_eligible_rebalancing_rows={summary['proposal_eligible_rebalancing_rows']}",
    "live_approval=false",
    f"architecture_blocker_counts={json.dumps(summary['architecture_blocker_counts'], sort_keys=True)}",
    f"rejected_row_blocker_counts={json.dumps(summary['rejected_row_blocker_counts'], sort_keys=True)}",
    f"market_blocker_counts={json.dumps(summary['market_blocker_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RB rebalancing structural readiness written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

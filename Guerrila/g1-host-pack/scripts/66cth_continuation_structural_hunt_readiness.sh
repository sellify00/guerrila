#!/usr/bin/env bash
# 66cth_continuation_structural_hunt_readiness.sh - final structural readiness certificate for Continuation

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTH - Continuation Structural Hunt Readiness"

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
STEP="${SHADOW_RUN66CTH_STEP:-66cth_continuation_structural_hunt_readiness}"
MERGER_ROWS="${SHADOW_RUN66CTH_MERGER_ROWS:-}"
OWNER_ROWS="${SHADOW_RUN66CTH_OWNER_ROWS:-}"
EXECUTION_ROWS="${SHADOW_RUN66CTH_EXECUTION_ROWS:-}"
RISK_ROWS="${SHADOW_RUN66CTH_RISK_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}
[[ -n "$MERGER_ROWS" ]] || MERGER_ROWS="$(latest_named continuation_candidate_manifest.jsonl)"
[[ -n "$OWNER_ROWS" ]] || OWNER_ROWS="$(latest_named run66ct1_continuation_owner_eligibility_rows.jsonl)"
[[ -n "$EXECUTION_ROWS" ]] || EXECUTION_ROWS="$(latest_named run66ct2_continuation_execution_rule_rows.jsonl)"
[[ -n "$RISK_ROWS" ]] || RISK_ROWS="$(latest_named run66ct3_continuation_markout_risk_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/proposal_eligible_continuation_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66cth_continuation_structural_hunt_readiness.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$MERGER_ROWS" "$OWNER_ROWS" "$EXECUTION_ROWS" "$RISK_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

merger_arg, owner_arg, execution_arg, risk_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:11]
paths = [Path(p) if p else None for p in (merger_arg, owner_arg, execution_arg, risk_arg)]
merger_path, owner_path, execution_path, risk_path = paths
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()]

def text(value):
    return "" if value is None else str(value).strip()

def by_candidate(rows):
    return {text(row.get("candidate_id")): row for row in rows if text(row.get("candidate_id"))}

def merge(*rows):
    out = {}
    for row in rows:
        if row:
            out.update(row)
    return out

merger = by_candidate(load_jsonl(merger_path))
owner = by_candidate(load_jsonl(owner_path))
execution = by_candidate(load_jsonl(execution_path))
risk = by_candidate(load_jsonl(risk_path))
all_ids = sorted(set(merger) | set(owner) | set(execution) | set(risk))
eligible = []
all_rows = []
for cid in all_ids:
    row = merge(merger.get(cid), owner.get(cid), execution.get(cid), risk.get(cid))
    ready = (
        row.get("owner_mode_match_continuation") is True
        and row.get("pm_owner_lock_ready") is True
        and row.get("decision") == "CONTINUATION_MARKOUT_RISK_READY"
        and row.get("markout_risk_ready") is True
    )
    row["proposal_eligible_continuation"] = ready
    if ready:
        eligible.append(row)
    all_rows.append(row)

architecture_blockers = Counter()
for row in all_rows:
    if row.get("proposal_eligible_continuation"):
        continue
    specific_blocker = text(row.get("first_account_policy_template_blocker"))
    if not row.get("owner_mode_match_continuation"):
        architecture_blockers["PARK_CONTINUATION_OWNER_MISMATCH"] += 1
    elif not row.get("pm_owner_lock_ready"):
        architecture_blockers["PARK_CONTINUATION_PM"] += 1
    elif specific_blocker not in {"", "none"}:
        architecture_blockers["PARK_CONTINUATION_ACCOUNT_OR_POLICY"] += 1
    elif text(row.get("first_execution_blocker")) in {"BLOCKED_ACCOUNT_OR_ATA", "BLOCKED_TEMPLATE"}:
        architecture_blockers["PARK_CONTINUATION_ACCOUNT_OR_POLICY"] += 1
    elif text(row.get("first_execution_blocker")) == "BLOCKED_NEGATIVE_AFTER_COST":
        architecture_blockers["PARK_CONTINUATION_NEGATIVE_AFTER_COST"] += 1
    elif text(row.get("first_markout_risk_blocker")) == "BLOCKED_NEGATIVE_AFTER_COST":
        architecture_blockers["PARK_CONTINUATION_NEGATIVE_AFTER_COST"] += 1
    elif text(row.get("first_markout_risk_blocker")) in {"BLOCKED_ADVERSE_EXCURSION", "BLOCKED_ABSORPTION_TOXIC"}:
        architecture_blockers["PARK_CONTINUATION_ADVERSE_EXCURSION"] += 1
    else:
        architecture_blockers["PARK_CONTINUATION_HABITAT_ONLY"] += 1

if eligible:
    status = "CONTINUATION_STRUCTURAL_HUNT_READY"
elif architecture_blockers:
    status = architecture_blockers.most_common(1)[0][0]
else:
    status = "PARK_CONTINUATION_HABITAT_ONLY"

summary = {
    "status": status,
    "merger_rows": str(merger_path or ""),
    "owner_rows": str(owner_path or ""),
    "execution_rows": str(execution_path or ""),
    "risk_rows": str(risk_path or ""),
    "rows_seen": len(all_rows),
    "proposal_eligible_continuation_rows": len(eligible),
    "architecture_blocker_counts": dict(architecture_blockers),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": status,
    "rows_seen": len(all_rows),
    "proposal_eligible_continuation_rows": len(eligible),
    "architecture_blocker_counts": dict(architecture_blockers),
    "proposal_path_open": False,
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "merger_rows": str(merger_path or ""),
    "owner_rows": str(owner_path or ""),
    "execution_rows": str(execution_path or ""),
    "risk_rows": str(risk_path or ""),
    "result_dir": str(result_dir),
    "live_send": False,
    "sendBundle": False,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in eligible:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()) + "\n", encoding="utf-8")
PY

pass "Run66CTH continuation structural readiness written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

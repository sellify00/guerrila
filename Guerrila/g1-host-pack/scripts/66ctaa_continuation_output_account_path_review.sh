#!/usr/bin/env bash
# 66ctaa_continuation_output_account_path_review.sh - classify Continuation output-account blockers without mutating accounts

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTAA - Continuation Output Account Path Review"

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
STEP="${SHADOW_RUN66CTAA_STEP:-66ctaa_continuation_output_account_path_review}"
SOURCE_ROWS="${SHADOW_RUN66CTAA_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66cta_continuation_account_template_policy_autopsy_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctaa_continuation_output_account_path_review_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctaa_continuation_output_account_path_review.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, verdict_arg, summary_txt_arg = sys.argv[1:8]
source_path = Path(source_arg) if source_arg else None
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


def safe_int(value):
    try:
        return int(float(value or 0))
    except (TypeError, ValueError):
        return 0


rows = [
    row for row in load_jsonl(source_path)
    if text(row.get("first_account_policy_template_blocker")) == "BLOCKED_OUTPUT_ACCOUNT"
]
out = []
for row in rows:
    output_account_ready = boolish(row.get("output_account_ready"))
    inline_ata_possible = boolish(row.get("inline_ata_possible"))
    inline_ata_policy_pass = boolish(row.get("inline_ata_policy_pass"))
    inline_ata_rent_positive = boolish(row.get("inline_ata_rent_positive"))
    net_after_ata_cost_positive = boolish(row.get("net_ev_after_all_costs_positive")) or safe_int(row.get("base_net_ev")) > 0
    pm_rent_reserve_ok = boolish(row.get("pm_reserve_rent_ok"))

    if output_account_ready:
        decision = "ACCOUNT_ALREADY_EXISTS"
    elif inline_ata_possible and inline_ata_policy_pass and inline_ata_rent_positive and net_after_ata_cost_positive and pm_rent_reserve_ok:
        decision = "INLINE_ATA_POLICY_CLEAN"
    elif inline_ata_possible and not inline_ata_policy_pass:
        decision = "INLINE_ATA_POLICY_BLOCKED"
    elif inline_ata_possible and not inline_ata_rent_positive:
        decision = "INLINE_ATA_RENT_NEGATIVE"
    else:
        decision = "OUTPUT_ACCOUNT_UNSUPPORTED"

    item = {
        **row,
        "output_account_path_review_decision": decision,
        "net_after_ata_cost_positive": net_after_ata_cost_positive,
        "pm_rent_reserve_ok": pm_rent_reserve_ok,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    }
    out.append(item)

decision_counts = Counter(row["output_account_path_review_decision"] for row in out)
summary = {
    "status": "CONTINUATION_OUTPUT_ACCOUNT_PATH_REVIEW_COMPLETE",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "decision_counts": dict(decision_counts),
    "inline_ata_policy_clean_rows": decision_counts.get("INLINE_ATA_POLICY_CLEAN", 0),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "ata_created": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "decision_counts": dict(decision_counts),
    "inline_ata_policy_clean_rows": decision_counts.get("INLINE_ATA_POLICY_CLEAN", 0),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
        "ata_created": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "result_dir": str(result_dir),
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text("\n".join(
    f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}"
    for k, v in summary.items()
) + "\n", encoding="utf-8")
PY

pass "Run66CTAA continuation output-account path review written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 22_launch10_hot_gate_autopsy.sh — summarize hot Launch R2 gate blockers

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch10 — Hot Gate Autopsy"

assert_cmd_exists python3

RUN_ROOT="${1:-}"
if [[ -z "$RUN_ROOT" ]]; then
    fail "Usage: $0 /abs/run_root"
    finish
    exit 1
fi

SUMMARY_JSON="$RUN_ROOT/launch10_hot_stream_executor_summary.json"
DECISIONS_JSONL="$RUN_ROOT/launch10_hot_decisions.jsonl"
SUMMARY_TXT="$RUN_ROOT/launch10_hot_gate_autopsy_summary.txt"
SUMMARY_JSON_OUT="$RUN_ROOT/launch10_hot_gate_autopsy_summary.json"

if [[ ! -f "$SUMMARY_JSON" ]]; then
    fail "Missing hot summary: $SUMMARY_JSON"
    finish
    exit 1
fi

python3 - "$SUMMARY_JSON" "$DECISIONS_JSONL" "$SUMMARY_JSON_OUT" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

summary_path = Path(sys.argv[1])
decisions_path = Path(sys.argv[2])
json_out = Path(sys.argv[3])
txt_out = Path(sys.argv[4])

summary = json.loads(summary_path.read_text(encoding="utf-8"))
rows = []
if decisions_path.exists():
    for raw in decisions_path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            row = json.loads(raw)
        except Exception:
            continue
        if isinstance(row, dict):
            rows.append(row)

hot_rows = [row for row in rows if str(row.get("decision", "")).startswith("HOT_")]
block_rows = [row for row in hot_rows if str(row.get("decision", "")).startswith("HOT_BLOCK_")]
ready_ordered_rows = [row for row in hot_rows if row.get("decision") == "HOT_READY_ORDERED_BUNDLE"]
ready_late_rows = [row for row in hot_rows if row.get("decision") == "HOT_READY_LATE_FOLLOW"]

first_blocker_counts = Counter(str(row.get("first_hot_gate_blocker") or "unknown") for row in block_rows)
decision_counts = Counter(str(row.get("decision") or "unknown") for row in hot_rows)

payload = {
    "status": summary.get("status"),
    "stream_rows_seen": int(summary.get("stream_rows_seen") or 0),
    "eligible_pump_rows": int(summary.get("eligible_pump_rows") or 0),
    "trigger_b_pass_rows": int(summary.get("trigger_b_pass_rows") or 0),
    "hot_gate_rows_seen": len(hot_rows),
    "hot_ready_ordered_bundle_rows": len(ready_ordered_rows),
    "hot_ready_late_follow_rows": len(ready_late_rows),
    "attempt_count": int(summary.get("attempt_count") or 0),
    "wire_send": int(summary.get("wire_send") or 0),
    "live_landed": int(summary.get("live_landed") or 0),
    "decision_counts": dict(decision_counts),
    "first_hot_gate_blocker_counts": dict(first_blocker_counts),
}

json_out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = []
for key, value in payload.items():
    if isinstance(value, (dict, list)):
        rendered = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        rendered = str(value).lower()
    else:
        rendered = str(value)
    lines.append(f"{key}={rendered}")
txt_out.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(txt_out.read_text(encoding="utf-8"), end="")
PY

pass "Hot gate autopsy written to $SUMMARY_TXT"
finish

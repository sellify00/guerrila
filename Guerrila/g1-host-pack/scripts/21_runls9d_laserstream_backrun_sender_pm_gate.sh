#!/usr/bin/env bash
# 21_runls9d_laserstream_backrun_sender_pm_gate.sh — Sender PM/accounting repair audit

source "$(dirname "$0")/lib.sh"
script_header "21D — RunLS9D LaserStream Backrun Sender PM Gate Audit"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS9_INNER_DIR:-$PACK_DIR/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only}}"
ROWS_PATH="$INNER_STEP_DIR/runls9_laserstream_backrun_nosend_rows.jsonl"
OUT_ROWS_PATH="$INNER_STEP_DIR/runls9d_laserstream_backrun_sender_pm_rows.jsonl"
OUT_SUMMARY_PATH="$INNER_STEP_DIR/runls9d_laserstream_backrun_sender_pm_summary.json"
OUT_BLOCKER_PATH="$INNER_STEP_DIR/runls9d_laserstream_backrun_sender_pm_blockers.json"

if [[ ! -f "$ROWS_PATH" ]]; then
    fail "RunLS9 rows missing: $ROWS_PATH"
    finish
    exit 1
fi

python3 - "$ROWS_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" "$OUT_BLOCKER_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

rows_path = Path(sys.argv[1])
out_rows_path = Path(sys.argv[2])
out_summary_path = Path(sys.argv[3])
out_blocker_path = Path(sys.argv[4])

sender_rows = []
classifications = Counter()

for raw in rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    if row.get("route_family") != "helius_sender_dual":
        continue

    execution_ready = bool(row.get("pm_reserve_ok"))
    research_only = bool(row.get("no_send_result")) and not execution_ready
    classification = (
        "sender_execution_no_send_accept"
        if execution_ready
        else "blocked_sender_pm_reserve_missing"
    )
    if research_only and not execution_ready:
        classifications["sender_research_accept_only"] += 1
    classifications[classification] += 1

    enriched = dict(row)
    enriched["sender_pm_gate_classification"] = classification
    enriched["research_accept_only"] = research_only and not execution_ready
    sender_rows.append(enriched)

sender_subset_total = len(sender_rows)
sender_pm_reserve_ok_count = sum(1 for row in sender_rows if row.get("pm_reserve_ok"))
sender_execution_no_send_accept_count = classifications["sender_execution_no_send_accept"]
sender_research_only_accept_count = classifications["sender_research_accept_only"]
sender_blocked_pm_reserve_missing_count = classifications[
    "blocked_sender_pm_reserve_missing"
]

out_rows_path.write_text(
    "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in sender_rows),
    encoding="utf-8",
)
summary = {
    "sender_subset_total": sender_subset_total,
    "sender_pm_reserve_ok_count": sender_pm_reserve_ok_count,
    "sender_blocked_pm_reserve_missing_count": sender_blocked_pm_reserve_missing_count,
    "sender_execution_no_send_accept_count": sender_execution_no_send_accept_count,
    "sender_research_only_accept_count": sender_research_only_accept_count,
    "result": (
        "pass_pm_gate_honest"
        if sender_execution_no_send_accept_count == sender_pm_reserve_ok_count
        and sender_blocked_pm_reserve_missing_count + sender_execution_no_send_accept_count
        == sender_subset_total
        else "failed_pm_gate_honest_classification"
    ),
}
out_summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
blockers = {
    "top_blocker": (
        "sender_pm_reserve_missing"
        if sender_blocked_pm_reserve_missing_count > 0
        else "none"
    ),
    "sender_subset_total": sender_subset_total,
    "sender_pm_reserve_ok_count": sender_pm_reserve_ok_count,
    "sender_blocked_pm_reserve_missing_count": sender_blocked_pm_reserve_missing_count,
    "sender_execution_no_send_accept_count": sender_execution_no_send_accept_count,
    "sender_research_only_accept_count": sender_research_only_accept_count,
}
out_blocker_path.write_text(json.dumps(blockers, indent=2) + "\n", encoding="utf-8")
PY

{
    echo "inner_result_dir=$INNER_STEP_DIR"
    echo "sender_rows_path=$OUT_ROWS_PATH"
    echo "sender_summary_json=$OUT_SUMMARY_PATH"
    echo "sender_blocker_json=$OUT_BLOCKER_PATH"
} > "$INNER_STEP_DIR/runls9d_summary.txt"

finish

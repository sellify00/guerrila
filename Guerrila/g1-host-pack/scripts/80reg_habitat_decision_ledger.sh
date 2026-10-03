#!/usr/bin/env bash
# 80REG - append current habitat decisions into an explicit decision ledger

source "$(dirname "$0")/lib.sh"
script_header "80REG - Habitat Decision Ledger"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80REG_LEDGER_STEP:-80_habitat_registry_latest}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/decision_ledger_summary.txt"
SUMMARY_JSON="$RESULT_DIR/decision_ledger_summary.json"
ROWS_JSONL="$RESULT_DIR/decision_ledger_rows.jsonl"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
DECISION_LEDGER_PATH="${SHADOW_HABITAT_DECISION_LEDGER_PATH:-/home/ubuntu/shadow-host-audit/dpdk-runs/habitat_registry/shadow_habitat_decision_ledger.jsonl}"
OWNER_LABEL="${SHADOW_HABITAT_DECISION_OWNER:-supervisor}"

mkdir -p "$RESULT_DIR"
mkdir -p "$(dirname "$DECISION_LEDGER_PATH")"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$RUNTIME_REGISTRY_PATH" "$DECISION_LEDGER_PATH" "$OWNER_LABEL" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" <<'PY'
from __future__ import annotations

import json
import sys
from datetime import datetime, timezone
from pathlib import Path

runtime_registry_path = Path(sys.argv[1])
decision_ledger_path = Path(sys.argv[2])
owner_label = sys.argv[3]
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])

now_iso = datetime.now(timezone.utc).isoformat()


def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            continue
    return rows


runtime_doc = load_json(runtime_registry_path)
habitats = runtime_doc.get("habitats") if isinstance(runtime_doc.get("habitats"), list) else []
existing_rows = load_jsonl(decision_ledger_path)
existing_ids = {str(row.get("decision_id") or "") for row in existing_rows}

new_rows = []
for row in habitats:
    decision_id = str(row.get("decision_id") or "")
    if not decision_id or decision_id in existing_ids:
        continue
    question = f"Should {row.get('pool')} {row.get('pair')} become an active Shadow habitat?"
    decision = str((row.get("decision") or {}).get("status") or row.get("label") or "WATCH")
    evidence = ["Run80A", "Run80B"]
    if ((row.get("evidence") or {}).get("run80c_rows_path")):
        evidence.append("Run80C")
    ledger_row = {
        "record_type": "shadow_habitat_decision_ledger",
        "decision_id": decision_id,
        "habitat_id": row.get("habitat_id"),
        "question": question,
        "decision": decision,
        "strategy_habitat_label": row.get("strategy_habitat_label"),
        "evidence": evidence,
        "reason": (row.get("decision") or {}).get("reason"),
        "next_action": (row.get("decision") or {}).get("next_action"),
        "reversal_condition": (row.get("decision") or {}).get("promotion_condition"),
        "stop_condition": (row.get("decision") or {}).get("kill_condition"),
        "owner": owner_label,
        "timestamp": now_iso,
    }
    new_rows.append(ledger_row)

if new_rows:
    with decision_ledger_path.open("a", encoding="utf-8") as handle:
        for row in new_rows:
            handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in new_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

summary = {
    "status": "run80reg_ledger_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "decision_ledger_path": str(decision_ledger_path),
    "new_decision_count": len(new_rows),
    "rows_jsonl": str(rows_jsonl_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"runtime_registry_path={summary['runtime_registry_path']}",
    f"decision_ledger_path={summary['decision_ledger_path']}",
    f"new_decision_count={summary['new_decision_count']}",
    f"rows_jsonl={summary['rows_jsonl']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Habitat decision ledger updated"
finish

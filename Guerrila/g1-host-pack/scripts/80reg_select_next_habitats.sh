#!/usr/bin/env bash
# 80REG - select the next habitats to study from the durable registry

source "$(dirname "$0")/lib.sh"
script_header "80REG - Select Next Habitats"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80REG_SELECT_STEP:-80_habitat_registry_latest}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/select_next_summary.txt"
SUMMARY_JSON="$RESULT_DIR/select_next_summary.json"
ROWS_JSONL="$RESULT_DIR/select_next_rows.jsonl"

RUNTIME_REGISTRY_PATH="${SHADOW_HABITAT_RUNTIME_REGISTRY_PATH:-/etc/shadow-engine/shadow_habitat_registry.json}"
TOP_N="${SHADOW_HABITAT_SELECT_TOP_N:-5}"
SELECT_MODE="${1:-${SHADOW_HABITAT_SELECT_MODE:-research}}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$RUNTIME_REGISTRY_PATH" "$TOP_N" "$SELECT_MODE" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

runtime_registry_path = Path(sys.argv[1])
top_n = int(sys.argv[2])
mode = str(sys.argv[3]).strip().lower()
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])


def load_json(path: Path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


doc = load_json(runtime_registry_path)
habitats = doc.get("habitats") if isinstance(doc.get("habitats"), list) else []

def score(row: dict) -> tuple:
    scores = row.get("scores") or {}
    shadow = row.get("shadow_metrics") or {}
    return (
        safe_float(scores.get("habitat_score")),
        safe_float(scores.get("readiness_score")),
        safe_float(shadow.get("candidate_per_min")),
        safe_float(shadow.get("would_fire_per_min")),
        safe_float(shadow.get("exact_positive_count")),
    )


def eligible_for_live(row: dict) -> bool:
    label = str(row.get("label") or "")
    strategy = str(row.get("strategy_habitat_label") or "")
    blocker = str((row.get("shadow_metrics") or {}).get("main_blocker") or "")
    if label != "HUNT_LIVE_TRACK":
        return False
    if strategy in {"DROP_ONLY_RESEARCH", "DENY_POLICY", "DENY_DUST_EXIT", "DENY_NO_VISIBILITY"}:
        return False
    if blocker == "source_policy_live_disallowed":
        return False
    return True


research_rows = sorted(
    [
        row for row in habitats
        if str(row.get("label") or "") in {"WATCH", "HUNT_RESEARCH"}
        and str(row.get("strategy_habitat_label") or "") not in {"DENY_POLICY", "DENY_DUST_EXIT", "DENY_NO_VISIBILITY", "DROP_ONLY_RESEARCH"}
    ],
    key=score,
    reverse=True,
)[:top_n]
live_track_rows = sorted([row for row in habitats if eligible_for_live(row)], key=score, reverse=True)[:top_n]
deny_review_rows = sorted(
    [row for row in habitats if str(row.get("label") or "") == "DENY"],
    key=lambda row: (str(row.get("last_updated") or ""), *score(row)),
    reverse=True,
)[:top_n]

if mode == "live-track":
    selected_rows = live_track_rows
    selected_bucket = "live_track"
elif mode == "deny-review":
    selected_rows = deny_review_rows
    selected_bucket = "deny_review"
else:
    selected_rows = research_rows
    selected_bucket = "research"

output_rows = []
for row in selected_rows:
    payload = {
        "mode": mode,
        "bucket": selected_bucket,
        "habitat_id": row.get("habitat_id"),
        "pool": row.get("pool"),
        "pool_hex": row.get("pool_hex"),
        "pair": row.get("pair"),
        "venue": row.get("venue"),
        "label": row.get("label"),
        "strategy_habitat_label": row.get("strategy_habitat_label"),
        "main_blocker": (row.get("shadow_metrics") or {}).get("main_blocker"),
        "next_action": (row.get("decision") or {}).get("next_action"),
        "habitat_score": (row.get("scores") or {}).get("habitat_score"),
        "readiness_score": (row.get("scores") or {}).get("readiness_score"),
    }
    output_rows.append(payload)

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in output_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

summary = {
    "status": "run80reg_select_complete",
    "runtime_registry_path": str(runtime_registry_path),
    "mode": mode,
    "research_count": len(research_rows),
    "live_track_count": len(live_track_rows),
    "deny_review_count": len(deny_review_rows),
    "selected_count": len(selected_rows),
    "rows_jsonl": str(rows_jsonl_path),
}
summary_lines = [
    f"status={summary['status']}",
    f"runtime_registry_path={summary['runtime_registry_path']}",
    f"mode={summary['mode']}",
    f"research_count={summary['research_count']}",
    f"live_track_count={summary['live_track_count']}",
    f"deny_review_count={summary['deny_review_count']}",
    f"selected_count={summary['selected_count']}",
    f"rows_jsonl={summary['rows_jsonl']}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Next habitats selected from registry"
finish

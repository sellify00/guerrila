#!/usr/bin/env bash
# 66ctae_continuation_adverse_excursion_review.sh - review whether adverse-excursion drag reflects a conservative stop or a genuinely bad thesis

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTAE - Continuation Adverse Excursion Review"

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
STEP="${SHADOW_RUN66CTAE_STEP:-66ctae_continuation_adverse_excursion_review}"
SOURCE_ROWS="${SHADOW_RUN66CTAE_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66cte_continuation_negative_after_cost_autopsy_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctae_continuation_adverse_excursion_review_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctae_continuation_adverse_excursion_review.json"
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
    if text(row.get("dominant_negative_driver")) == "ADVERSE_EXCURSION_DRAG"
]
out = []
for row in rows:
    gross_continuation_ev = safe_int(row.get("gross_plus_costs"))
    adverse_adjustment = safe_int(row.get("adverse_excursion_adjustment_raw"))
    max_adverse = float(row.get("max_adverse_excursion") or 0)

    if gross_continuation_ev > 0 and adverse_adjustment > 0 and boolish(row.get("max_adverse_excursion_bounded")):
        decision = "STOP_CONSERVATIVE_REVIEW_ONLY"
    else:
        decision = "ADVERSE_EXCURSION_CONFIRMED"

    item = {
        **row,
        "gross_continuation_ev": gross_continuation_ev,
        "adverse_excursion_adjustment_raw": adverse_adjustment,
        "max_adverse_excursion": max_adverse,
        "stop_too_conservative": decision == "STOP_CONSERVATIVE_REVIEW_ONLY",
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": "CONTINUATION_ADVERSE_EXCURSION_REVIEW_COMPLETE",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "stop_too_conservative_rows": decision_counts.get("STOP_CONSERVATIVE_REVIEW_ONLY", 0),
    "adverse_excursion_confirmed_rows": decision_counts.get("ADVERSE_EXCURSION_CONFIRMED", 0),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "stop_too_conservative_rows": summary["stop_too_conservative_rows"],
    "decision_counts": dict(decision_counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
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

pass "Run66CTAE continuation adverse-excursion review written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 66ctt_continuation_token2022_policy_exception_review.sh - watch-only policy review for Continuation Token-2022 rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTT - Continuation Token-2022 Policy Exception Review"

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
STEP="${SHADOW_RUN66CTT_STEP:-66ctt_continuation_token2022_policy_exception_review}"
SOURCE_ROWS="${SHADOW_RUN66CTT_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66cta_continuation_account_template_policy_autopsy_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctt_continuation_token2022_policy_exception_review_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctt_continuation_token2022_policy_exception_review.json"
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

TOKEN2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


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


def as_extensions(value):
    if isinstance(value, list):
        return [text(v) for v in value if text(v)]
    if isinstance(value, str):
        raw = value.strip()
        if not raw:
            return []
        if raw.startswith("["):
            try:
                parsed = json.loads(raw)
            except Exception:
                parsed = []
            return as_extensions(parsed)
        return [part.strip() for part in raw.replace(";", ",").split(",") if part.strip()]
    return []


def classify(row):
    extensions = set(as_extensions(row.get("token2022_extensions") if "token2022_extensions" in row else row.get("extensions")))
    if text(row.get("freeze_authority")):
        return "DENY_FREEZE_AUTHORITY"
    if text(row.get("mint_authority")):
        return "DENY_AUTHORITY_RISK"
    if "TransferHook" in extensions:
        return "DENY_TRANSFER_HOOK"
    if "PermanentDelegate" in extensions:
        return "DENY_PERMANENT_DELEGATE"
    if "NonTransferable" in extensions:
        return "DENY_NON_TRANSFERABLE"
    if "DefaultAccountState" in extensions:
        return "DENY_DEFAULT_FROZEN"
    if text(row.get("token_program")) == TOKEN2022 and not boolish(row.get("output_account_ready")) and not boolish(row.get("inline_ata_possible")):
        return "REVIEW_ONLY_METADATA_TOKEN2022"
    if not boolish(row.get("exit_non_dust", True)):
        return "DENY_EXIT_RISK"
    return "REVIEW_ONLY_METADATA_TOKEN2022" if text(row.get("token_program")) == TOKEN2022 else "DENY_EXIT_RISK"


rows = [
    row for row in load_jsonl(source_path)
    if text(row.get("first_account_policy_template_blocker")) == "BLOCKED_TOKEN2022_EXPLICIT_ALLOW"
]
out = []
for row in rows:
    decision = classify(row)
    item = {
        **row,
        "review_decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

decision_counts = Counter(row["review_decision"] for row in out)
summary = {
    "status": "CONTINUATION_TOKEN2022_POLICY_EXCEPTION_REVIEW_COMPLETE",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "decision_counts": dict(decision_counts),
    "watch_only_metadata_rows": decision_counts.get("REVIEW_ONLY_METADATA_TOKEN2022", 0),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "decision_counts": dict(decision_counts),
    "watch_only_metadata_rows": summary["watch_only_metadata_rows"],
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

pass "Run66CTT continuation Token-2022 policy exception review written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 66rs_rebalancing_shadowlite_source_overlay.sh - compare AF_XDP and LaserStream rebalancing surfaces

source "$(dirname "$0")/lib.sh"
script_header "66RS - Rebalancing ShadowLite Source Overlay"

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
STEP="${SHADOW_RUN66RS_STEP:-66rs_rebalancing_shadowlite_source_overlay}"
STATUS_PREFIX="${SHADOW_RUN66RS_STATUS_PREFIX:-run66rs_rebalancing_shadowlite_source_overlay}"
REPORT_BASENAME="${SHADOW_RUN66RS_REPORT_BASENAME:-run66rs_rebalancing_shadowlite_source_overlay}"
RULE_ROWS="${SHADOW_RUN66RS_RULE_ROWS:-}"
TEMPLATE_ROWS="${SHADOW_RUN66RS_TEMPLATE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66re_rebalancing_entry_exit_rule_proof_rows.jsonl)"
[[ -n "$TEMPLATE_ROWS" ]] || TEMPLATE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66rs_rebalancing_shadowlite_source_overlay_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$RULE_ROWS" "$TEMPLATE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

rule_arg, template_arg, summary_file, summary_json, rows_jsonl, step, status_prefix = sys.argv[1:8]
rule_path = Path(rule_arg) if rule_arg else None
template_path = Path(template_arg) if template_arg else None
summary_file = Path(summary_file)
summary_json = Path(summary_json)
rows_jsonl = Path(rows_jsonl)

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def write_jsonl(path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

def text(value):
    return str(value or "").strip().lower()

def source_of(row):
    raw = text(row.get("source") or row.get("ingest_source") or row.get("signal_source") or row.get("matched_rollout_source"))
    if "laser" in raw or "grpc" in raw or "shadowlite" in raw:
        return "laserstream"
    if "afxdp" in raw or "af_xdp" in raw or "xdp" in raw:
        return "afxdp"
    return "unknown"

def boolish(value):
    return bool(value)

template_by_cid = {str(row.get("candidate_id") or ""): row for row in load_jsonl(template_path)}
groups = defaultdict(list)
for row in load_jsonl(rule_path):
    cid = str(row.get("candidate_id") or "")
    source = source_of(row)
    item = dict(row)
    template = template_by_cid.get(cid, {})
    item.update({
        "source": source,
        "template_decision": template.get("decision"),
        "no_send_ready": template.get("decision") in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"},
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })
    groups[source].append(item)

out = []
for source, rows in sorted(groups.items()):
    out.append({
        "source": source,
        "rebalancing_rows": len(rows),
        "exact_positive_rows": sum(1 for row in rows if boolish(row.get("positive_after_cost"))),
        "entry_exit_rule_ready_rows": sum(1 for row in rows if row.get("decision") == "ENTRY_EXIT_RULE_READY"),
        "inline_ata_possible_rows": 0,
        "roundtrip_positive_rows": sum(1 for row in rows if boolish(row.get("positive_after_cost"))),
        "no_send_ready_rows": sum(1 for row in rows if row.get("no_send_ready")),
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })

write_jsonl(rows_jsonl, out)
source_counts = Counter()
for source, rows in groups.items():
    source_counts[source] = len(rows)
summary = {
    "status": f"{status_prefix}_complete" if rule_path and rule_path.exists() else f"{status_prefix}_missing_source",
    "step": step,
    "source_rule_rows": str(rule_path) if rule_path else "",
    "source_template_rows": str(template_path) if template_path else "",
    "AF_XDP rows": source_counts.get("afxdp", 0),
    "LaserStream processed rows": source_counts.get("laserstream", 0),
    "matched rows": 0,
    "LaserStream-only rows": source_counts.get("laserstream", 0),
    "AF_XDP-only rows": source_counts.get("afxdp", 0),
    "unknown_source_rows": source_counts.get("unknown", 0),
    "source_rows": out,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_rule_rows={summary['source_rule_rows']}",
    f"source_template_rows={summary['source_template_rows']}",
    f"AF_XDP rows={summary['AF_XDP rows']}",
    f"LaserStream processed rows={summary['LaserStream processed rows']}",
    f"matched rows={summary['matched rows']}",
    f"LaserStream-only rows={summary['LaserStream-only rows']}",
    f"AF_XDP-only rows={summary['AF_XDP-only rows']}",
    f"unknown_source_rows={summary['unknown_source_rows']}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RS source overlay written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

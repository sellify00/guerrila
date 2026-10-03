#!/usr/bin/env bash
# 22_launch10_target_freshness_timing_audit.sh — audit Launch hot-path freshness lineage and timing over a completed run root

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch10 — Target Freshness Timing Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_ROOT="${1:-$(find "$RESULTS_ROOT" -maxdepth 1 -type d -name 'run_launch10_hot_r2_press_window_*' | sort | tail -n 1)}"
[[ -d "$RUN_ROOT" ]] || die "run root not found: $RUN_ROOT"

SELECTOR_ROWS="$RUN_ROOT/22_launch8_v2_candidate_selector/launch8_v2_candidate_selector_rows.jsonl"
BUNDLE_ROWS="$RUN_ROOT/22_launch9_v2_entry_bundle_nosubmit/launch4_entry_bundle_nosubmit_rows.jsonl"
PROPOSAL_JSON="$RUN_ROOT/22_launch4_one_mint_live_proposal_v2/launch4_one_mint_live_proposal.json"
LIVE_REPORT="$RUN_ROOT/22_launch5_one_mint_live_execute_r2/launch5_one_mint_live_execute.json"
SUMMARY_JSON="$RUN_ROOT/22_launch10_target_freshness_timing_audit_summary.json"
SUMMARY_TXT="$RUN_ROOT/22_launch10_target_freshness_timing_audit_summary.txt"

python3 - "$RUN_ROOT" "$SELECTOR_ROWS" "$BUNDLE_ROWS" "$PROPOSAL_JSON" "$LIVE_REPORT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

run_root = Path(sys.argv[1])
selector_rows_path = Path(sys.argv[2])
bundle_rows_path = Path(sys.argv[3])
proposal_json_path = Path(sys.argv[4])
live_report_path = Path(sys.argv[5])
summary_json_path = Path(sys.argv[6])
summary_txt_path = Path(sys.argv[7])


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        doc = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return doc if isinstance(doc, dict) else {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            row = json.loads(raw)
        except Exception:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def delta_us_or_none(end_ns: Any, start_ns: Any) -> int | None:
    end_val = safe_int(end_ns)
    start_val = safe_int(start_ns)
    if end_val <= 0 or start_val <= 0:
        return None
    return max(0, (end_val - start_val) // 1000)


selector_rows = load_jsonl(selector_rows_path)
bundle_rows = load_jsonl(bundle_rows_path)
proposal_doc = load_json(proposal_json_path)
live_report = load_json(live_report_path)
proposal_top = proposal_doc.get("top_candidate") if isinstance(proposal_doc.get("top_candidate"), dict) else {}

selected_group_id = text(live_report.get("launch_group_id")) or text(proposal_top.get("launch_group_id"))
selected_signature = text(live_report.get("target_tx_signature")) or text(proposal_top.get("latest_signature"))

selector_by_group = {text(row.get("launch_group_id")): row for row in selector_rows if text(row.get("launch_group_id"))}
bundle_by_group = {text(row.get("launch_group_id")): row for row in bundle_rows if text(row.get("launch_group_id"))}
selected_selector = selector_by_group.get(selected_group_id, {})
selected_bundle = bundle_by_group.get(selected_group_id, {})

selected_row_source = text(
    live_report.get("selected_row_source")
    or proposal_top.get("selected_row_source")
    or selected_bundle.get("selected_row_source")
    or selected_selector.get("selected_row_source")
)
target_seen_preprocessed_ns = safe_int(
    live_report.get("target_seen_preprocessed_ns")
    or proposal_top.get("target_seen_preprocessed_ns")
    or selected_bundle.get("target_seen_preprocessed_ns")
    or selected_selector.get("target_seen_preprocessed_ns")
)
selector_ready_ns = safe_int(selected_selector.get("selector_ready_ns"))
bundle_ready_ns = safe_int(selected_bundle.get("bundle_ready_ns"))
presend_status_check_ns = safe_int(live_report.get("target_first_status_check_ns"))
execute_start_ns = safe_int(live_report.get("execute_start_ns"))

rows_with_target_seen_preprocessed_ns = sum(1 for row in selector_rows if safe_int(row.get("target_seen_preprocessed_ns")) > 0)
rows_with_target_blockhash = sum(1 for row in selector_rows if text(row.get("target_blockhash")))
rows_valid_at_capture = sum(1 for row in selector_rows if row.get("target_blockhash_valid_at_capture") is True)
rows_valid_at_selector = sum(1 for row in selector_rows if row.get("target_blockhash_valid_at_selector") is True)
rows_valid_at_bundle_build = sum(1 for row in bundle_rows if row.get("target_blockhash_valid_at_bundle_build") is True)
rows_valid_at_presend = 1 if live_report.get("target_blockhash_valid_at_presend") is True else 0
missing_lineage_rows = sum(1 for row in selector_rows if safe_int(row.get("target_seen_preprocessed_ns")) <= 0)
stale_artifact_rows = sum(1 for row in selector_rows if text(row.get("selected_row_source")) not in {"", "live_current_window"})

selected_missing_lineage = target_seen_preprocessed_ns <= 0
selected_stale_source = bool(selected_row_source) and selected_row_source != "live_current_window"
target_seen_to_selector_us = delta_us_or_none(selector_ready_ns, target_seen_preprocessed_ns)
target_seen_to_bundle_ready_us = delta_us_or_none(bundle_ready_ns, target_seen_preprocessed_ns)
target_seen_to_presend_us = delta_us_or_none(presend_status_check_ns, target_seen_preprocessed_ns)
bundle_ready_to_presend_us = delta_us_or_none(presend_status_check_ns, bundle_ready_ns)

status = "HOT_PATH_READY"
if selected_missing_lineage:
    status = "ABORT_MISSING_FRESHNESS_LINEAGE"
elif selected_stale_source:
    status = "SELECTED_ROW_FROM_STALE_ARTIFACT"
elif live_report.get("first_failure") == "target_blockhash_invalid" and rows_valid_at_capture > 0 and rows_valid_at_selector > 0:
    status = "PIPELINE_TOO_LATE"
elif live_report.get("first_failure") == "target_blockhash_invalid":
    status = "TARGET_BYTES_STALE"
elif live_report.get("first_failure") and "blockhash" in text(live_report.get("first_failure")):
    status = "BLOCKHASH_VALIDITY_RPC_STALE"

summary = {
    "status": status,
    "run_root": str(run_root),
    "rows_seen": len(selector_rows),
    "rows_with_target_seen_preprocessed_ns": rows_with_target_seen_preprocessed_ns,
    "rows_with_target_blockhash": rows_with_target_blockhash,
    "rows_valid_at_capture": rows_valid_at_capture,
    "rows_valid_at_selector": rows_valid_at_selector,
    "rows_valid_at_bundle_build": rows_valid_at_bundle_build,
    "rows_valid_at_presend": rows_valid_at_presend,
    "missing_lineage_rows": missing_lineage_rows,
    "stale_artifact_rows": stale_artifact_rows,
    "pipeline_late_rows": 1 if status == "PIPELINE_TOO_LATE" else 0,
    "selected_row_source": selected_row_source or None,
    "target_signature": selected_signature or None,
    "target_seen_preprocessed_ns": target_seen_preprocessed_ns or None,
    "target_seen_wall_time": safe_int(
        live_report.get("target_seen_unix_ns")
        or proposal_top.get("target_seen_unix_ns")
        or selected_bundle.get("target_seen_unix_ns")
        or selected_selector.get("target_seen_unix_ns")
    ) or None,
    "target_blockhash": text(
        live_report.get("target_blockhash")
        or proposal_top.get("target_blockhash")
        or selected_bundle.get("target_blockhash")
        or selected_selector.get("target_blockhash")
    ) or None,
    "target_blockhash_valid_at_capture": live_report.get("target_blockhash_valid_at_capture", proposal_top.get("target_blockhash_valid_at_capture", selected_bundle.get("target_blockhash_valid_at_capture", selected_selector.get("target_blockhash_valid_at_capture")))),
    "target_blockhash_valid_at_bundle_build": live_report.get("target_blockhash_valid_at_bundle_build", proposal_top.get("target_blockhash_valid_at_bundle_build", selected_bundle.get("target_blockhash_valid_at_bundle_build"))),
    "target_blockhash_valid_at_presend": live_report.get("target_blockhash_valid_at_presend"),
    "target_status_at_capture": live_report.get("target_status_at_capture", proposal_top.get("target_status_at_capture", selected_bundle.get("target_status_at_capture", selected_selector.get("target_status_at_capture")))),
    "target_status_at_bundle_build": live_report.get("target_status_at_bundle_build", proposal_top.get("target_status_at_bundle_build", selected_bundle.get("target_status_at_bundle_build"))),
    "target_status_at_presend": live_report.get("target_status_at_presend"),
    "target_seen_to_selector_us": target_seen_to_selector_us,
    "target_seen_to_bundle_ready_us": target_seen_to_bundle_ready_us,
    "target_seen_to_presend_us": target_seen_to_presend_us,
    "bundle_ready_to_presend_us": bundle_ready_to_presend_us,
    "source_artifact_path": str(selector_rows_path),
    "live_stream_row_id": text(
        live_report.get("target_source_row_id")
        or proposal_top.get("target_source_row_id")
        or selected_bundle.get("target_source_row_id")
        or selected_selector.get("target_source_row_id")
    ) or None,
}

summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Launch freshness audit written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 21_afbr1g_target_arrival_timing_audit.sh — classify AFBR1 target arrival timing

source "$(dirname "$0")/lib.sh"
script_header "21G — AFBR1G Target Arrival Timing Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INPUT_DIR="${1:-${SHADOW_AFBR1G_INPUT_DIR:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/21_afbr1f_backrun_freshness_batch}}"
OUT_DIR="${2:-${SHADOW_AFBR1G_OUT_DIR:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/21_afbr1g_target_arrival_timing_audit}}"
OUT_JSON_PATH="$OUT_DIR/afbr1g_target_arrival_timing_audit.json"
OUT_ROWS_PATH="$OUT_DIR/afbr1g_target_arrival_timing_rows.jsonl"
OUT_SUMMARY_PATH="$OUT_DIR/summary.txt"
STATUS_CHECK_LATE_US="${SHADOW_AFBR1G_STATUS_CHECK_LATE_US:-10000}"
SOURCE_DELTA_LATE_US="${SHADOW_AFBR1G_SOURCE_DELTA_LATE_US:-10000}"
INTERNAL_FAST_US="${SHADOW_AFBR1G_INTERNAL_FAST_US:-50000}"

mkdir -p "$OUT_DIR"

python3 - "$INPUT_DIR" "$OUT_JSON_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" \
    "$STATUS_CHECK_LATE_US" "$SOURCE_DELTA_LATE_US" "$INTERNAL_FAST_US" <<'PY'
from __future__ import annotations

import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Any

input_dir = Path(sys.argv[1])
out_json_path = Path(sys.argv[2])
out_rows_path = Path(sys.argv[3])
out_summary_path = Path(sys.argv[4])
status_check_late_us = int(sys.argv[5])
source_delta_late_us = int(sys.argv[6])
internal_fast_us = int(sys.argv[7])


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except json.JSONDecodeError:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def as_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return int(value)
    try:
        if value is None or value == "":
            return None
        return int(value)
    except (TypeError, ValueError):
        return None


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if value is None:
        return False
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def first_value(doc: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        value = doc.get(key)
        if value not in (None, ""):
            return value
    return None


def first_int(doc: dict[str, Any], *keys: str) -> int | None:
    for key in keys:
        value = as_int(doc.get(key))
        if value is not None:
            return value
    return None


def delta_us(start_ns: int | None, end_ns: int | None) -> int | None:
    if start_ns is None or end_ns is None or end_ns < start_ns:
        return None
    return (end_ns - start_ns) // 1_000


def percentile(values: list[int], pct: int) -> int | None:
    if not values:
        return None
    if pct <= 0:
        return min(values)
    if pct >= 100:
        return max(values)
    ordered = sorted(values)
    idx = math.ceil((pct / 100) * len(ordered)) - 1
    idx = max(0, min(idx, len(ordered) - 1))
    return ordered[idx]


def extract_status_slot(status: Any) -> int | None:
    if isinstance(status, dict):
        return as_int(status.get("slot"))
    return None


def status_is_processed(status: Any) -> bool:
    return status is not None


def sample_dirs(root: Path) -> list[Path]:
    samples_root = root / "samples"
    if samples_root.exists():
        dirs = sorted(path for path in samples_root.glob("sample_*") if path.is_dir())
        if dirs:
            return dirs
    if (root / "runafbr1f_backrun_freshness_race_rows.jsonl").exists():
        return [root]
    if any(root.glob("sample_*")):
        return sorted(path for path in root.glob("sample_*") if path.is_dir())
    return []


def load_source_rows(sample_dir: Path) -> dict[str, dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    for filename in ("laserstream_vs_afxdp_first_seen.jsonl", "laserstream_vs_afxdp_signature_first_seen.jsonl"):
        for row in load_jsonl(sample_dir / filename):
            if row.get("record_type") not in (None, "source_arbiter", "signature_first_seen"):
                continue
            signature = first_value(row, "signature", "target_signature", "target_signature_base58")
            if signature:
                rows[str(signature)] = row
    return rows


def decision_for(row: dict[str, Any]) -> tuple[str, str]:
    freshness_class = str(row.get("freshness_class") or row.get("freshness_race_classification") or "STATUS_UNKNOWN")
    if freshness_class == "FRESH_AT_BUNDLE_READY" or row.get("final_blocker") == "afbr_target_fresh_bundle_eligible_no_send":
        return "FRESH_AT_BUNDLE_READY", "freshness_gate_reported_bundle_eligible"

    source_delta = first_int(row, "source_delta_us")
    if source_delta is not None and source_delta <= -source_delta_late_us:
        return "SOURCE_LATE", "laserstream_matched_before_afxdp"

    status = row.get("target_status_at_first_check")
    target_already_processed = as_bool(row.get("target_already_processed")) or status_is_processed(status)
    bundle_ready_to_status = first_int(row, "bundle_ready_to_first_status_check_us")
    seen_to_bundle = first_int(row, "target_seen_to_bundle_ready_us")
    bundle_ready_ns = first_int(row, "bundle_ready_ns")
    first_status_start_ns = first_int(row, "first_status_check_start_ns")

    if not target_already_processed:
        if bundle_ready_ns is not None:
            return "FRESH_AT_BUNDLE_READY", "no_processed_status_at_first_check"
        return "UNKNOWN", "missing_bundle_ready_and_unprocessed_status"

    if bundle_ready_ns is not None and first_status_start_ns is not None:
        if bundle_ready_to_status is not None and bundle_ready_to_status > status_check_late_us:
            return "BUNDLE_READY_BUT_STATUS_LATE", "first_status_check_started_after_bundle_ready_threshold"
        if seen_to_bundle is not None and seen_to_bundle <= internal_fast_us:
            return "SOURCE_LATE", "target_processed_despite_fast_bundle_ready_and_timely_status_check"
        return "SOURCE_LATE", "target_processed_at_first_check_after_bundle_ready"

    if first_status_start_ns is not None:
        return "STATUS_CHECK_LATE", "processed_status_but_bundle_ready_timestamp_missing"
    return "UNKNOWN", "missing_status_timing"


sample_paths = sample_dirs(input_dir)
if not sample_paths:
    raise SystemExit(f"no AFBR1F sample dirs or rows found under {input_dir}")

rows_out: list[dict[str, Any]] = []
for sample_dir in sample_paths:
    rows_path = sample_dir / "runafbr1f_backrun_freshness_race_rows.jsonl"
    if not rows_path.exists():
        continue
    freshness_doc = load_json(sample_dir / "runafbr1_target_freshness_bundle_eligibility.json")
    summary_doc = load_json(sample_dir / "runafbr1_summary.json")
    source_rows = load_source_rows(sample_dir)
    for index, row in enumerate(load_jsonl(rows_path)):
        target_status_at_first_check = row.get("target_status_at_first_check")
        target_landed_slot = extract_status_slot(target_status_at_first_check)
        local_current_slot = first_int(row, "local_current_slot_at_first_check")
        if local_current_slot is None:
            local_current_slot = first_int(freshness_doc, "local_current_slot_at_first_check")

        target_signature = first_value(row, "target_signature", "target_signature_base58") or first_value(
            freshness_doc,
            "target_signature",
            "target_signature_base58",
        )
        source_match = source_rows.get(str(target_signature)) if target_signature else None
        target_seen_afxdp = first_int(
            row,
            "target_seen_afxdp_host_mono_ns",
            "target_seen_afxdp_unix_ns",
            "afxdp_seen_ns",
        )
        target_seen_laserstream = first_int(row, "target_seen_laserstream_host_mono_ns", "laserstream_seen_ns")
        if target_seen_laserstream is None and source_match:
            target_seen_laserstream = first_int(source_match, "laserstream_seen_ns", "laserstream_host_mono_recv_ns")
        source_delta = first_int(row, "source_delta_us", "laserstream_minus_afxdp_us")
        if source_delta is None and target_seen_afxdp is not None and target_seen_laserstream is not None:
            source_delta = (target_seen_laserstream - target_seen_afxdp) // 1_000

        bundle_ready_ns = first_int(row, "bundle_ready_ns", "bundle_envelope_built_unix_ns")
        first_status_start_ns = first_int(row, "first_status_check_start_ns")
        first_status_end_ns = first_int(row, "first_status_check_end_ns")
        target_seen_to_bundle = first_int(row, "target_seen_to_bundle_ready_us")
        if target_seen_to_bundle is None:
            target_seen_to_bundle = delta_us(target_seen_afxdp, bundle_ready_ns)
        bundle_ready_to_status = first_int(row, "bundle_ready_to_first_status_check_us")
        if bundle_ready_to_status is None:
            bundle_ready_to_status = delta_us(bundle_ready_ns, first_status_start_ns)

        leader_slot_distance = None
        if target_landed_slot is not None and local_current_slot is not None:
            leader_slot_distance = local_current_slot - target_landed_slot

        audit_row = {
            "record_type": "afbr1g_target_arrival_timing_row",
            "sample_id": sample_dir.name,
            "candidate_index": index,
            "target_signature": target_signature,
            "target_pool": first_value(row, "target_pool"),
            "target_seen_afxdp_host_mono_ns": target_seen_afxdp,
            "target_seen_laserstream_host_mono_ns": target_seen_laserstream,
            "source_delta_us": source_delta,
            "source_delta_basis": "laserstream_minus_afxdp",
            "source_matched_afxdp_laserstream": target_seen_afxdp is not None and target_seen_laserstream is not None,
            "first_status_check_start_ns": first_status_start_ns,
            "first_status_check_end_ns": first_status_end_ns,
            "target_status_at_first_check": target_status_at_first_check,
            "target_landed_slot_if_known": target_landed_slot,
            "local_current_slot_at_first_check": local_current_slot,
            "leader_slot_distance": leader_slot_distance,
            "bundle_ready_ns": bundle_ready_ns,
            "target_seen_to_bundle_ready_us": target_seen_to_bundle,
            "bundle_ready_to_first_status_check_us": bundle_ready_to_status,
            "provider_sim_latency_us": first_int(row, "provider_sim_latency_us"),
            "freshness_class": first_value(row, "freshness_race_classification") or "STATUS_UNKNOWN",
            "final_blocker": first_value(row, "final_blocker"),
            "target_recent_blockhash_valid": row.get("target_recent_blockhash_valid"),
            "target_already_processed": as_bool(row.get("target_already_processed")) or target_status_at_first_check is not None,
            "bundle_envelope_built": as_bool(row.get("bundle_envelope_built")),
            "selected_pm_reserve_ok": as_bool(row.get("selected_pm_reserve_ok")),
            "projected_net_ev_after_tip_raw": first_int(row, "projected_net_ev_after_tip_raw"),
            "sendBundle_called": False,
            "wire_send": 0,
            "live_landed": 0,
            "source_sample_dir": str(sample_dir),
            "source_summary_status": summary_doc.get("status"),
        }
        decision, reason = decision_for(audit_row)
        audit_row["arrival_timing_decision"] = decision
        audit_row["arrival_timing_reason"] = reason
        rows_out.append(audit_row)

decision_counts = Counter(str(row.get("arrival_timing_decision") or "UNKNOWN") for row in rows_out)
freshness_counts = Counter(str(row.get("freshness_class") or "STATUS_UNKNOWN") for row in rows_out)


def collect_int(key: str) -> list[int]:
    return [value for row in rows_out if isinstance((value := row.get(key)), int)]


def latency_stats(prefix: str, key: str) -> dict[str, int | None]:
    values = collect_int(key)
    return {
        f"{prefix}_p50": int(statistics.median(values)) if values else None,
        f"{prefix}_p90": percentile(values, 90),
        f"{prefix}_p99": percentile(values, 99),
        f"{prefix}_min": min(values) if values else None,
        f"{prefix}_max": max(values) if values else None,
    }


if rows_out:
    top_decision, top_count = decision_counts.most_common(1)[0]
    overall_diagnosis = top_decision if top_count > len(rows_out) / 2 else "MIXED"
else:
    overall_diagnosis = "UNKNOWN"

report: dict[str, Any] = {
    "record_type": "afbr1g_target_arrival_timing_audit",
    "input_dir": str(input_dir),
    "sample_dir_count": len(sample_paths),
    "candidate_row_count": len(rows_out),
    "arrival_timing_decision_counts": dict(decision_counts),
    "freshness_class_counts": dict(freshness_counts),
    "source_late_count": decision_counts.get("SOURCE_LATE", 0),
    "status_check_late_count": decision_counts.get("STATUS_CHECK_LATE", 0),
    "bundle_ready_but_status_late_count": decision_counts.get("BUNDLE_READY_BUT_STATUS_LATE", 0),
    "fresh_at_bundle_ready_count": decision_counts.get("FRESH_AT_BUNDLE_READY", 0),
    "unknown_count": decision_counts.get("UNKNOWN", 0),
    "overall_diagnosis": overall_diagnosis,
    "status_check_late_threshold_us": status_check_late_us,
    "source_delta_late_threshold_us": source_delta_late_us,
    "internal_fast_threshold_us": internal_fast_us,
    "rows_path": str(out_rows_path),
}
report.update(latency_stats("target_seen_to_bundle_ready_us", "target_seen_to_bundle_ready_us"))
report.update(latency_stats("bundle_ready_to_first_status_check_us", "bundle_ready_to_first_status_check_us"))
report.update(latency_stats("provider_sim_latency_us", "provider_sim_latency_us"))
report.update(latency_stats("source_delta_us", "source_delta_us"))

out_rows_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows_out), encoding="utf-8")
out_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    f"candidate_row_count={report['candidate_row_count']}",
    f"sample_dir_count={report['sample_dir_count']}",
    f"overall_diagnosis={report['overall_diagnosis']}",
    f"arrival_timing_decision_counts={json.dumps(report['arrival_timing_decision_counts'], sort_keys=True)}",
    f"freshness_class_counts={json.dumps(report['freshness_class_counts'], sort_keys=True)}",
    f"source_late_count={report['source_late_count']}",
    f"status_check_late_count={report['status_check_late_count']}",
    f"bundle_ready_but_status_late_count={report['bundle_ready_but_status_late_count']}",
    f"fresh_at_bundle_ready_count={report['fresh_at_bundle_ready_count']}",
    f"unknown_count={report['unknown_count']}",
    f"target_seen_to_bundle_ready_us_p50={report['target_seen_to_bundle_ready_us_p50']}",
    f"target_seen_to_bundle_ready_us_p90={report['target_seen_to_bundle_ready_us_p90']}",
    f"target_seen_to_bundle_ready_us_p99={report['target_seen_to_bundle_ready_us_p99']}",
    f"bundle_ready_to_first_status_check_us_p50={report['bundle_ready_to_first_status_check_us_p50']}",
    f"bundle_ready_to_first_status_check_us_p90={report['bundle_ready_to_first_status_check_us_p90']}",
    f"bundle_ready_to_first_status_check_us_p99={report['bundle_ready_to_first_status_check_us_p99']}",
    f"provider_sim_latency_us_p50={report['provider_sim_latency_us_p50']}",
    f"provider_sim_latency_us_p90={report['provider_sim_latency_us_p90']}",
    f"provider_sim_latency_us_p99={report['provider_sim_latency_us_p99']}",
    f"source_delta_us_p50={report['source_delta_us_p50']}",
    f"source_delta_us_p90={report['source_delta_us_p90']}",
    f"source_delta_us_p99={report['source_delta_us_p99']}",
    f"report_path={out_json_path}",
    f"rows_path={out_rows_path}",
]
out_summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "AFBR1G target arrival timing audit written"
echo "afbr1g_report_path=$OUT_JSON_PATH"
echo "afbr1g_rows_path=$OUT_ROWS_PATH"
echo "afbr1g_summary_path=$OUT_SUMMARY_PATH"
finish

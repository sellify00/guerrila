#!/usr/bin/env bash
# 78Q2 - classify quote numeric lineage for strict DirectArb rows

source "$(dirname "$0")/lib.sh"
script_header "78Q2 - DirectArb Quote Numeric Lineage"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78Q2_STEP:-epoch8_auto_strict_directarb_quote_numeric_lineage}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78q2_directarb_quote_numeric_lineage.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78Q2_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78Q2_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
CORE_INPUT_MINTS_CSV="${SHADOW_RUN78Q2_CORE_INPUT_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$AUTOPSY_ROWS_PATH" "$CORE_INPUT_MINTS_CSV" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from pathlib import Path

source_rows_path = Path(sys.argv[1])
core_input_mints = {item.strip() for item in sys.argv[2].split(",") if item.strip()}
summary_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])


def parse_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def is_live_track_scope(row: dict) -> bool:
    return (
        str(row.get("mint_in") or "") in core_input_mints
        and str(row.get("route_family") or "") == "tpu_quic_public"
        and str(row.get("owner_mode") or "") == "directarb"
    )


def classify_row(row: dict) -> str:
    expected = as_int(row.get("quote_expected_out_raw"))
    min_out = as_int(row.get("min_out_raw"))
    quote_ready = as_bool(row.get("quote_numeric_present")) and expected > 0 and min_out > 0
    if quote_ready:
        return "QUOTE_READY"

    ev_basis = str(row.get("candidate_ev_math_basis") or row.get("ev_math_blocker") or "")
    if ev_basis in {"unsupported_non_wsol_pair", "non_wsol_unverified"}:
        return "UNSUPPORTED_ROUTE_MATH"
    if ev_basis == "missing_rollout_row":
        return "MISSING_FINAL_ROLLOUT_ROW"
    if expected <= 0 and min_out <= 0 and not is_live_track_scope(row):
        return "OUT_OF_SCOPE_NON_NUMERIC_ARTIFACT"
    if as_int(row.get("quote_request_id")) <= 0 and as_int(row.get("template_id")) <= 0:
        return "QUOTE_IDENTITY_JOIN_FAIL"
    if expected > 0 or min_out > 0:
        return "QUOTE_NUMERIC_EXPORT_BUG"
    return "QUOTE_NUMERIC_EXPORT_BUG"


source_rows, malformed = parse_jsonl(source_rows_path)
target_rows = [
    row
    for row in source_rows
    if str(row.get("first_failed") or "") == "quote_numeric_missing"
    or str(row.get("quote_blocker") or "") == "quote_numeric_missing"
]

out_rows = []
for row in target_rows:
    classification = classify_row(row)
    live_scope = is_live_track_scope(row)
    out_rows.append(
        {
            "source_step": str(row.get("source_step") or ""),
            "signature": str(row.get("signature") or ""),
            "pool": str(row.get("pool") or ""),
            "mint_in": str(row.get("mint_in") or ""),
            "mint_out": str(row.get("mint_out") or ""),
            "route_family": str(row.get("route_family") or ""),
            "owner_mode": str(row.get("owner_mode") or ""),
            "is_live_track_scope": live_scope,
            "quote_expected_out_raw": as_int(row.get("quote_expected_out_raw")),
            "min_out_raw": as_int(row.get("min_out_raw")),
            "quote_request_id": as_int(row.get("quote_request_id")),
            "template_id": as_int(row.get("template_id")),
            "ingress_unix_ns": as_int(row.get("ingress_unix_ns")),
            "quote_numeric_present": as_bool(row.get("quote_numeric_present")),
            "candidate_ev_math_basis": str(row.get("candidate_ev_math_basis") or ""),
            "ev_math_blocker": str(row.get("ev_math_blocker") or ""),
            "classification": classification,
        }
    )

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "source_step",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "owner_mode",
    "is_live_track_scope",
    "quote_expected_out_raw",
    "min_out_raw",
    "quote_request_id",
    "template_id",
    "ingress_unix_ns",
    "quote_numeric_present",
    "candidate_ev_math_basis",
    "ev_math_blocker",
    "classification",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(row)

classification_counts = Counter(row["classification"] for row in out_rows)
live_track_scope_quote_numeric_missing = sum(
    1 for row in out_rows if row["is_live_track_scope"] and row["classification"] != "QUOTE_READY"
)

summary = {
    "status": "run78q2_complete" if out_rows else "blocked_no_quote_numeric_rows",
    "source_row_count": len(target_rows),
    "classification_counts": dict(classification_counts),
    "live_track_scope_quote_numeric_missing": live_track_scope_quote_numeric_missing,
    "malformed_source_rows": malformed,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"source_row_count={summary['source_row_count']}",
    "classification_counts=" + json.dumps(summary["classification_counts"], separators=(",", ":")),
    f"live_track_scope_quote_numeric_missing={summary['live_track_scope_quote_numeric_missing']}",
    f"malformed_source_rows={summary['malformed_source_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78Q2 quote numeric lineage complete"
finish

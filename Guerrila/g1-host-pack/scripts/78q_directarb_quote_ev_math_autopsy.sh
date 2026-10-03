#!/usr/bin/env bash
# 78Q - classify quote / EV math holes for strict DirectArb Run78 near-miss rows

source "$(dirname "$0")/lib.sh"
script_header "78Q - DirectArb Quote / EV Math Autopsy"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78Q_STEP:-epoch8_auto_strict_directarb_private_egress_run78q_quote_ev_math_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78q_directarb_quote_ev_math_autopsy.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78Q_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78Q_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$AUTOPSY_ROWS_PATH" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from pathlib import Path

source_rows_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
rows_jsonl_path = Path(sys.argv[4])
rows_csv_path = Path(sys.argv[5])


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


def classify_quote_missing_reason(row):
    blocker = str(row.get("quote_blocker") or "")
    if blocker != "quote_numeric_missing":
        return "none"
    expected = as_int(row.get("quote_expected_out_raw"))
    min_out = as_int(row.get("min_out_raw"))
    if expected <= 0 and min_out <= 0:
        return "non_numeric_artifact"
    if not as_bool(row.get("template_ready")):
        return "missing_quote_identity"
    return "quote_numeric_missing"


def classify_ev_math_missing_reason(row):
    if as_bool(row.get("candidate_ev_math_available")):
        return "none"
    basis = str(row.get("candidate_ev_math_basis") or row.get("ev_math_blocker") or "")
    if basis in {"unsupported_non_wsol_pair", "non_wsol_unverified"}:
        return "unsupported_route_math"
    if basis in {"missing_rollout_row", "missing_quote_identity", "non_numeric_artifact"}:
        return basis
    return basis or "ev_math_unavailable"


def classify_decision(row, quote_missing_reason: str, ev_math_missing_reason: str):
    if quote_missing_reason != "none":
        return quote_missing_reason.upper()
    if ev_math_missing_reason != "none":
        return ev_math_missing_reason.upper()
    first_failed = str(row.get("first_failed") or "")
    return first_failed.upper() if first_failed else "NONE"


source_rows, malformed = parse_jsonl(source_rows_path)
target_rows = [
    row
    for row in source_rows
    if str(row.get("first_failed") or "") in {"ev_math_unavailable", "quote_numeric_missing"}
]

out_rows = []
for row in target_rows:
    quote_missing_reason = classify_quote_missing_reason(row)
    ev_math_missing_reason = classify_ev_math_missing_reason(row)
    route_family = str(row.get("route_family") or "")
    ev_basis = str(row.get("candidate_ev_math_basis") or "")
    route_cost_model = str(row.get("route_cost_model") or "")
    if not route_cost_model:
        if ev_basis:
            route_cost_model = f"{route_family}:{ev_basis}"
        else:
            route_cost_model = route_family or "unknown"

    quote_identity_match = row.get("quote_identity_match")
    if quote_identity_match is None:
        quote_identity_match = as_bool(row.get("quote_numeric_present")) and as_int(
            row.get("quote_expected_out_raw")
        ) > 0

    out_rows.append(
        {
            "source_step": row.get("source_step"),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "route_family": route_family,
            "quote_expected_out_raw": as_int(row.get("quote_expected_out_raw")),
            "min_out_raw": as_int(row.get("min_out_raw")),
            "quote_request_id": row.get("quote_request_id"),
            "template_id": row.get("template_id"),
            "ingress_unix_ns": row.get("ingress_unix_ns"),
            "quote_identity_match": bool(quote_identity_match),
            "route_cost_model": route_cost_model,
            "base_fee_raw": as_int(row.get("base_fee")),
            "tip_lamports": as_int(row.get("tip_lamports")),
            "net_ev_before_fee_tip": as_int(row.get("net_ev_before_fee_tip")),
            "net_ev_after_fee_tip": as_int(row.get("net_ev_after_fee_tip")),
            "quote_missing_reason": quote_missing_reason,
            "ev_math_missing_reason": ev_math_missing_reason,
            "first_failed": row.get("first_failed"),
            "decision": classify_decision(row, quote_missing_reason, ev_math_missing_reason),
        }
    )

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "source_step",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "quote_expected_out_raw",
    "min_out_raw",
    "quote_request_id",
    "template_id",
    "ingress_unix_ns",
    "quote_identity_match",
    "route_cost_model",
    "base_fee_raw",
    "tip_lamports",
    "net_ev_before_fee_tip",
    "net_ev_after_fee_tip",
    "quote_missing_reason",
    "ev_math_missing_reason",
    "first_failed",
    "decision",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(row)

decision_counts = Counter(row["decision"] for row in out_rows)
quote_reason_counts = Counter(row["quote_missing_reason"] for row in out_rows)
ev_reason_counts = Counter(row["ev_math_missing_reason"] for row in out_rows)

summary = {
    "status": "run78q_complete" if out_rows else "blocked_no_quote_or_ev_math_holes",
    "source_row_count": len(target_rows),
    "ev_math_unavailable_source_count": sum(
        1 for row in target_rows if str(row.get("first_failed") or "") == "ev_math_unavailable"
    ),
    "quote_numeric_missing_source_count": sum(
        1 for row in target_rows if str(row.get("first_failed") or "") == "quote_numeric_missing"
    ),
    "ev_math_unavailable_residual_count": sum(
        1 for row in out_rows if row["ev_math_missing_reason"] == "ev_math_unavailable"
    ),
    "quote_numeric_missing_residual_count": sum(
        1 for row in out_rows if row["quote_missing_reason"] == "quote_numeric_missing"
    ),
    "permanent_classification_counts": dict(decision_counts),
    "quote_missing_reason_counts": dict(quote_reason_counts),
    "ev_math_missing_reason_counts": dict(ev_reason_counts),
    "malformed_source_rows": malformed,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"source_row_count={summary['source_row_count']}",
    f"ev_math_unavailable_source_count={summary['ev_math_unavailable_source_count']}",
    f"quote_numeric_missing_source_count={summary['quote_numeric_missing_source_count']}",
    f"ev_math_unavailable_residual_count={summary['ev_math_unavailable_residual_count']}",
    f"quote_numeric_missing_residual_count={summary['quote_numeric_missing_residual_count']}",
    "permanent_classification_counts="
    + json.dumps(summary["permanent_classification_counts"], separators=(",", ":")),
    "quote_missing_reason_counts="
    + json.dumps(summary["quote_missing_reason_counts"], separators=(",", ":")),
    "ev_math_missing_reason_counts="
    + json.dumps(summary["ev_math_missing_reason_counts"], separators=(",", ":")),
    f"malformed_source_rows={summary['malformed_source_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78Q quote / EV math autopsy complete"
finish

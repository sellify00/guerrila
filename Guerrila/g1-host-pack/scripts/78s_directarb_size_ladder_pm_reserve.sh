#!/usr/bin/env bash
# 78S - fail-closed PM reserve size ladder for Run78 DirectArb near-miss rows

source "$(dirname "$0")/lib.sh"
script_header "78S - DirectArb Size Ladder PM Reserve"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78S_STEP:-epoch8_auto_strict_directarb_private_egress_run78s_pm_reserve_size_ladder}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78s_directarb_size_ladder_pm_reserve.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78S_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78S_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
AUDIT_STEP="${SHADOW_RUN78S_AUDIT_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
AUDIT_ROWS_PATH="${SHADOW_RUN78S_AUDIT_ROWS_PATH:-$RESULTS_ROOT/$AUDIT_STEP/rows.jsonl}"
SIZE_LADDER_SOL_CSV="${SHADOW_RUN78S_SIZE_LADDER_SOL_CSV:-1,0.5,0.1,0.05,0.01,0.005}"
WSOL_MINT="So11111111111111111111111111111111111111112"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$AUTOPSY_ROWS_PATH" "$AUDIT_ROWS_PATH" "$SIZE_LADDER_SOL_CSV" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$WSOL_MINT" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter, defaultdict
from decimal import Decimal
from pathlib import Path

source_rows_path = Path(sys.argv[1])
audit_rows_path = Path(sys.argv[2])
ladder_sizes = [Decimal(item.strip()) for item in sys.argv[3].split(",") if item.strip()]
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
rows_csv_path = Path(sys.argv[7])
wsol_mint = sys.argv[8].strip()


def parse_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows


def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def sol_to_lamports(value: Decimal) -> int:
    return int((value * Decimal("1000000000")).to_integral_value())


autopsy_rows = parse_jsonl(source_rows_path)
audit_rows = parse_jsonl(audit_rows_path)
pm_rows = [row for row in autopsy_rows if not as_bool(row.get("pm_reserve_can_lock_exact_input"))]
audit_rows_by_key = {
    (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or "")): row
    for row in audit_rows
}

out_rows = []
size_pass_counts = defaultdict(int)
size_pass_counts_audit_visible_view = defaultdict(int)
size_supported_counts = defaultdict(int)
size_rows_considered = defaultdict(int)

for row in pm_rows:
    row_key = (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or ""))
    audit_row = audit_rows_by_key.get(row_key) or {}
    mint_in = str(row.get("mint_in") or "")
    available_raw = as_int(row.get("pm_available_input_raw"))
    audit_visible_available_raw = as_int(audit_row.get("pm_available_balance_raw"))
    original_required_raw = as_int(row.get("pm_required_input_raw"))
    template_ready = as_bool(row.get("template_ready"))
    quote_numeric_present = as_bool(row.get("quote_numeric_present"))
    min_out_positive = as_bool(row.get("min_out_positive"))
    candidate_ev_positive = as_bool(row.get("candidate_ev_positive"))

    for size_sol in ladder_sizes:
        size_raw = sol_to_lamports(size_sol) if mint_in == wsol_mint else None
        supported = mint_in == wsol_mint
        if supported:
                size_supported_counts[str(size_sol)] += 1
        size_rows_considered[str(size_sol)] += 1

        if not supported:
            pm_reserve_ok = None
            audit_visible_reserve_ok = None
            quote_ok = None
            min_out_positive_ladder = None
            net_ev_after_fee_tip_positive = None
            private_egress_no_send_possible = None
            proof_status = "unsupported_non_wsol_input"
        else:
            pm_reserve_ok = available_raw >= size_raw > 0
            audit_visible_reserve_ok = audit_visible_available_raw >= size_raw > 0
            if pm_reserve_ok:
                size_pass_counts[str(size_sol)] += 1
            if audit_visible_reserve_ok:
                size_pass_counts_audit_visible_view[str(size_sol)] += 1
            # Fail-closed: current artifacts do not re-quote or re-run EV at smaller sizes.
            if size_raw == original_required_raw:
                quote_ok = quote_numeric_present
                min_out_positive_ladder = min_out_positive
                net_ev_after_fee_tip_positive = candidate_ev_positive
                private_egress_no_send_possible = None
                proof_status = "exact_size_match_from_original_row"
            else:
                quote_ok = None
                min_out_positive_ladder = None
                net_ev_after_fee_tip_positive = None
                private_egress_no_send_possible = None
                proof_status = "pm_only_no_requote"

        out_rows.append(
            {
                "source_step": row.get("source_step"),
                "signature": row.get("signature"),
                "pool": row.get("pool"),
                "mint_in": mint_in,
                "mint_out": row.get("mint_out"),
                "first_failed": row.get("first_failed"),
                "size_sol": str(size_sol),
                "size_raw": size_raw,
                "original_required_input_raw": original_required_raw,
                "current_available_input_raw": available_raw,
                "audit_visible_available_input_raw": audit_visible_available_raw,
                "same_or_smaller_than_original": bool(size_raw is not None and original_required_raw > 0 and size_raw <= original_required_raw),
                "quote_ok": quote_ok,
                "min_out_positive": min_out_positive_ladder,
                "net_ev_after_fee_tip_positive": net_ev_after_fee_tip_positive,
                "pm_reserve_ok": pm_reserve_ok,
                "audit_visible_reserve_ok": audit_visible_reserve_ok,
                "template_ready": template_ready,
                "private_egress_no_send_possible": private_egress_no_send_possible,
                "audit_visibility_classification": audit_row.get("pm_reserve_blocker"),
                "proof_status": proof_status,
            }
        )

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "source_step",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "first_failed",
    "size_sol",
    "size_raw",
    "original_required_input_raw",
    "current_available_input_raw",
    "audit_visible_available_input_raw",
    "same_or_smaller_than_original",
    "quote_ok",
    "min_out_positive",
    "net_ev_after_fee_tip_positive",
    "pm_reserve_ok",
    "audit_visible_reserve_ok",
    "template_ready",
    "private_egress_no_send_possible",
    "audit_visibility_classification",
    "proof_status",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(row)

proof_status_counts = Counter(row["proof_status"] for row in out_rows)
summary = {
    "status": "run78s_complete" if out_rows else "blocked_no_pm_failed_rows",
    "pm_failed_source_row_count": len(pm_rows),
    "ladder_row_count": len(out_rows),
    "size_pass_counts": dict(size_pass_counts),
    "size_pass_counts_pm_snapshot_view": dict(size_pass_counts),
    "size_pass_counts_audit_visible_view": dict(size_pass_counts_audit_visible_view),
    "size_supported_counts": dict(size_supported_counts),
    "size_rows_considered": dict(size_rows_considered),
    "proof_status_counts": dict(proof_status_counts),
    "audit_rows_path": str(audit_rows_path),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"pm_failed_source_row_count={summary['pm_failed_source_row_count']}",
    f"ladder_row_count={summary['ladder_row_count']}",
    "size_pass_counts=" + json.dumps(summary["size_pass_counts"], separators=(",", ":")),
    "size_pass_counts_pm_snapshot_view=" + json.dumps(summary["size_pass_counts_pm_snapshot_view"], separators=(",", ":")),
    "size_pass_counts_audit_visible_view=" + json.dumps(summary["size_pass_counts_audit_visible_view"], separators=(",", ":")),
    "size_supported_counts=" + json.dumps(summary["size_supported_counts"], separators=(",", ":")),
    "size_rows_considered=" + json.dumps(summary["size_rows_considered"], separators=(",", ":")),
    "proof_status_counts=" + json.dumps(summary["proof_status_counts"], separators=(",", ":")),
    f"audit_rows_path={summary['audit_rows_path']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78S size ladder PM reserve analysis complete"
finish

#!/usr/bin/env bash
# 78P - fail-closed private-route EV rescore over Run78 DirectArb EV near-miss rows

source "$(dirname "$0")/lib.sh"
script_header "78P - DirectArb Private-Route EV Rescore"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78P_STEP:-epoch8_auto_strict_directarb_private_egress_run78p_private_route_ev_rescore}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78p_directarb_private_route_ev_rescore.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78P_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78P_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"

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


def classify(row):
    if row.get("private_route_ev_after_fee_tip_proven"):
        return "PRIVATE_ROUTE_PROVEN"
    proof_status = str(row.get("private_route_ev_proof_status") or "")
    if proof_status == "owner_mode_mismatch_shadow_only":
        return "PROOF_UNAVAILABLE_OWNER_MODE_MISMATCH"
    if proof_status == "no_private_route_shadow":
        return "NO_PRIVATE_ROUTE_CONTEXT"
    return "FAIL_CLOSED"


autopsy_rows = parse_jsonl(source_rows_path)
ev_rows = [row for row in autopsy_rows if str(row.get("first_failed") or "") == "ev_not_positive"]

out_rows = []
for row in ev_rows:
    private_proven = bool(row.get("private_route_ev_after_fee_tip_proven"))
    private_ev = row.get("private_route_ev_after_fee_tip")
    out_rows.append(
        {
            "source_step": row.get("source_step"),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "tpu_quic_public_ev_after_fee": as_int(row.get("tpu_route_ev_after_fee_tip")),
            "helius_sender_dual_ev_after_fee_tip": private_ev if private_proven else None,
            "jito_via_helius_bundle_ev_after_tip": private_ev if private_proven else None,
            "jito_bundle_shadow_markout_after_fee_lamports": row.get("best_jito_bundle_shadow_markout_after_fee_lamports"),
            "private_route_shadow_owner_modes": row.get("private_shadow_owner_modes"),
            "private_route_ev_proven": private_proven,
            "private_route_ev_proof_status": row.get("private_route_ev_proof_status"),
            "ev_positive_under_private_route": bool(private_proven and as_int(private_ev) > 0),
            "decision": classify(row),
        }
    )

fieldnames = list(out_rows[0].keys()) if out_rows else [
    "source_step",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "tpu_quic_public_ev_after_fee",
    "helius_sender_dual_ev_after_fee_tip",
    "jito_via_helius_bundle_ev_after_tip",
    "jito_bundle_shadow_markout_after_fee_lamports",
    "private_route_shadow_owner_modes",
    "private_route_ev_proven",
    "private_route_ev_proof_status",
    "ev_positive_under_private_route",
    "decision",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        csv_row = dict(row)
        if isinstance(csv_row.get("private_route_shadow_owner_modes"), list):
            csv_row["private_route_shadow_owner_modes"] = ",".join(csv_row["private_route_shadow_owner_modes"])
        writer.writerow(csv_row)

decision_counts = Counter(row["decision"] for row in out_rows)
summary = {
    "status": "run78p_complete" if out_rows else "blocked_no_ev_not_positive_rows",
    "ev_not_positive_source_row_count": len(ev_rows),
    "private_route_ev_proven_count": sum(1 for row in out_rows if row["private_route_ev_proven"]),
    "ev_positive_under_private_route_count": sum(1 for row in out_rows if row["ev_positive_under_private_route"]),
    "decision_counts": dict(decision_counts),
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"ev_not_positive_source_row_count={summary['ev_not_positive_source_row_count']}",
    f"private_route_ev_proven_count={summary['private_route_ev_proven_count']}",
    f"ev_positive_under_private_route_count={summary['ev_positive_under_private_route_count']}",
    "decision_counts=" + json.dumps(summary["decision_counts"], separators=(",", ":")),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78P private-route EV rescore complete"
finish

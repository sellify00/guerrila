#!/usr/bin/env bash
# 78F2 - search certified exit rows for default-policy clean flatten readiness.

source "$(dirname "$0")/lib.sh"
script_header "78F2 - DirectArb Policy-Clean Flatten Search"

assert_cmd_exists python3

EPOCH_DIR=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            EPOCH_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$EPOCH_DIR" ]]; then
    EPOCH_DIR="${SHADOW_RUN78F2_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
fi
if [[ ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found: $EPOCH_DIR"
    exit 1
fi
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78f2_directarb_policy_clean_flatten_search"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78f2_directarb_policy_clean_flatten_search.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

DEFAULT_ROWS="${SHADOW_RUN78F2_DEFAULT_ROWS:-$EPOCH_DIR/78f_directarb_exit_flatten_route_certificate/rows.jsonl}"
WHATIF_ROWS="${SHADOW_RUN78F2_WHATIF_ROWS:-$EPOCH_DIR/78f_directarb_exit_flatten_route_certificate_policy_whatif/rows.jsonl}"

python3 - "$EPOCH_DIR" "$OUTPUT_DIR" "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$DEFAULT_ROWS" "$WHATIF_ROWS" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from pathlib import Path

epoch_dir = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])
default_rows_path = Path(sys.argv[7])
whatif_rows_path = Path(sys.argv[8])


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
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


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def row_key(row: dict) -> tuple[str, str, str, str]:
    return (
        str(row.get("candidate_id") or ""),
        str(row.get("signature") or ""),
        str(row.get("pool") or ""),
        str(row.get("mint_out") or ""),
    )


default_rows, default_bad = parse_jsonl(default_rows_path)
whatif_rows, whatif_bad = parse_jsonl(whatif_rows_path)
whatif_by_key = {row_key(row): row for row in whatif_rows}

out_rows: list[dict] = []
for row in default_rows:
    whatif = whatif_by_key.get(row_key(row), {})
    policy_clean = truth(row.get("output_token_policy_pass"))
    exit_route_ready = policy_clean and truth(row.get("exit_route_exists")) and not truth(row.get("exit_dust"))
    roundtrip_net = safe_int(row.get("roundtrip_net_after_all_costs"))
    roundtrip_positive = exit_route_ready and roundtrip_net > 0
    flatten_ready = policy_clean and truth(row.get("flatten_template_ready"))
    no_send_ready = row.get("decision") in {"DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND"}
    whatif_no_send_ready = whatif.get("decision") in {"DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND"}

    if no_send_ready:
        decision = "POLICY_CLEAN_FLATTEN_READY_NO_SEND"
    elif not policy_clean and whatif_no_send_ready:
        decision = "POLICY_EXCEPTION_DEPENDENT"
    elif not policy_clean:
        decision = "PARK_POLICY"
    elif not truth(row.get("exit_route_exists")):
        decision = "PARK_MISSING_EXIT_ROUTE"
    elif truth(row.get("exit_dust")):
        decision = "PARK_DUST_EXIT"
    elif not roundtrip_positive:
        decision = "POLICY_CLEAN_EXIT_NEGATIVE"
    elif not flatten_ready:
        decision = "PARK_TEMPLATE_UNSUPPORTED"
    else:
        decision = "POLICY_CLEAN_UNCLASSIFIED"

    out_rows.append(
        {
            "candidate_id": row.get("candidate_id"),
            "signature": row.get("signature"),
            "slot": row.get("slot"),
            "source": row.get("source"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "entry_amount_in_raw": safe_int(row.get("entry_amount_in_raw")),
            "entry_min_out_raw": safe_int(row.get("entry_min_out_raw")),
            "entry_net_ev_after_fee_tip": safe_int(row.get("entry_net_ev_after_fee_tip")),
            "output_token_policy_pass": policy_clean,
            "output_token_policy_blocker": row.get("output_token_policy_blocker"),
            "exit_route_exists": truth(row.get("exit_route_exists")),
            "exit_route_family": row.get("exit_route_family"),
            "exit_dust": truth(row.get("exit_dust")),
            "roundtrip_net_after_all_costs": row.get("roundtrip_net_after_all_costs"),
            "roundtrip_positive": roundtrip_positive,
            "flatten_template_ready": flatten_ready,
            "auto_flatten_possible": policy_clean and truth(row.get("auto_flatten_possible")),
            "default_decision": row.get("decision"),
            "whatif_decision": whatif.get("decision", ""),
            "whatif_policy_blocker": whatif.get("output_token_policy_blocker", ""),
            "policy_exception_dependent": (not policy_clean) and whatif_no_send_ready,
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
fieldnames = list(out_rows[0].keys()) if out_rows else ["candidate_id", "decision"]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

decision_counts = Counter(row["decision"] for row in out_rows)
policy_counts = Counter(str(row.get("output_token_policy_blocker") or "none") for row in out_rows)
report = {
    "status": "run78f2_complete" if out_rows else "blocked_no_78f_rows",
    "epoch_dir": str(epoch_dir),
    "default_rows_path": str(default_rows_path),
    "whatif_rows_path": str(whatif_rows_path),
    "default_rows_loaded": len(default_rows),
    "whatif_rows_loaded": len(whatif_rows),
    "malformed_default_rows": default_bad,
    "malformed_whatif_rows": whatif_bad,
    "policy_clean_exit_ready_count": sum(1 for row in out_rows if row["output_token_policy_pass"] and row["exit_route_exists"] and not row["exit_dust"]),
    "policy_clean_roundtrip_positive_count": sum(1 for row in out_rows if row["roundtrip_positive"]),
    "policy_clean_flatten_template_ready_count": sum(1 for row in out_rows if row["flatten_template_ready"]),
    "policy_clean_no_send_ready_count": sum(1 for row in out_rows if row["decision"] == "POLICY_CLEAN_FLATTEN_READY_NO_SEND"),
    "policy_exception_dependent_count": sum(1 for row in out_rows if row["policy_exception_dependent"]),
    "whatif_no_send_ready_count": sum(1 for row in out_rows if row["whatif_decision"] in {"DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND"}),
    "decision_counts": dict(decision_counts),
    "default_policy_blocker_counts": dict(policy_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Run78F2 policy-clean flatten search complete"
finish

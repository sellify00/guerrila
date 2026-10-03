#!/usr/bin/env bash
# 66ctfee_continuation_fee_tip_basis_review.sh - review sender vs bundle fee/tip basis for Continuation fee-drag rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTFEE - Continuation Fee/Tip Basis Review"

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
STEP="${SHADOW_RUN66CTFEE_STEP:-66ctfee_continuation_fee_tip_basis_review}"
SOURCE_ROWS="${SHADOW_RUN66CTFEE_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66cte_continuation_negative_after_cost_autopsy_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctfee_continuation_fee_tip_basis_review_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctfee_continuation_fee_tip_basis_review.json"
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


def route_family_for(row):
    if boolish(row.get("bundle_no_submit_ready")) or boolish(row.get("jito_bundle_possible")):
        return "jito_bundle"
    if boolish(row.get("helius_sender_no_send_ready")) or boolish(row.get("sender_possible")):
        return "helius_sender_dual"
    family = text(row.get("template_family"))
    return family or "unknown"


def fee_basis_for(row):
    if safe_int(row.get("base_net_ev")) != 0:
        return "base_net_ev_after_fee_tip_raw"
    if safe_int(row.get("route_specific_ev_after_cost_raw")) != 0:
        return "route_specific_ev_after_cost_raw"
    return "unknown"


rows = [
    row for row in load_jsonl(source_path)
    if text(row.get("dominant_negative_driver")) == "FEE_TIP_DRAG"
]
out = []
for row in rows:
    route_family = route_family_for(row)
    gross_continuation_ev = safe_int(row.get("gross_plus_costs"))
    net_after_all_costs = safe_int(row.get("net_after_all_costs"))
    priority_fee = safe_int(row.get("pm_lock_fee_raw") or row.get("pm_reserved_fee_raw"))
    bundle_tip = safe_int(row.get("pm_lock_tip_raw") or row.get("pm_reserved_tip_raw"))
    sender_fee = priority_fee if route_family == "helius_sender_dual" else 0
    entry_fee = sender_fee
    exit_fee = 0
    fee_unit_basis = fee_basis_for(row)

    net_if_sender_cost_model = (
        gross_continuation_ev - sender_fee - bundle_tip
        if route_family == "helius_sender_dual"
        else net_after_all_costs
    )
    net_if_bundle_cost_model = (
        gross_continuation_ev - bundle_tip
        if gross_continuation_ev or bundle_tip
        else net_after_all_costs
    )

    duplicated_tip_detected = False
    fee_model_error = False
    if route_family == "helius_sender_dual":
        # Sender is expected to include both a CU-price fee and a tip transfer.
        # Treat tip duplication as an error only if the row simultaneously claims
        # bundle-only readiness while charging sender and bundle costs together.
        duplicated_tip_detected = boolish(row.get("bundle_no_submit_ready")) and sender_fee > 0 and bundle_tip > 0
        fee_model_error = duplicated_tip_detected

    if fee_model_error:
        review_outcome = "FEE_MODEL_ERROR_DUPLICATED_TIP"
    elif route_family == "helius_sender_dual" and net_if_bundle_cost_model > 0 and not boolish(row.get("jito_bundle_possible")):
        review_outcome = "BUNDLE_COUNTERFACTUAL_POSITIVE_ONLY"
    elif route_family == "helius_sender_dual" and net_if_sender_cost_model > 0:
        review_outcome = "SENDER_MODEL_POSITIVE"
    else:
        review_outcome = "SENDER_PRIORITY_PLUS_TIP_STILL_NEGATIVE"

    item = {
        **row,
        "gross_continuation_ev": gross_continuation_ev,
        "entry_fee": entry_fee,
        "exit_fee": exit_fee,
        "priority_fee": priority_fee,
        "sender_fee": sender_fee,
        "bundle_tip": bundle_tip,
        "duplicated_tip_detected": duplicated_tip_detected,
        "fee_unit_basis": fee_unit_basis,
        "route_family": route_family,
        "sender_vs_bundle_cost_model": (
            "sender_priority_fee_plus_tip_vs_bundle_tip_only"
            if route_family == "helius_sender_dual"
            else "bundle_tip_only"
            if route_family == "jito_bundle"
            else "unknown"
        ),
        "net_after_all_costs": net_after_all_costs,
        "net_if_sender_cost_model": net_if_sender_cost_model,
        "net_if_bundle_cost_model": net_if_bundle_cost_model,
        "fee_model_error": fee_model_error,
        "net_positive_after_corrected_cost": fee_model_error and net_if_sender_cost_model > 0,
        "counterfactual_bundle_positive": net_if_bundle_cost_model > 0,
        "review_outcome": review_outcome,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

decision_counts = Counter(row["review_outcome"] for row in out)
summary = {
    "status": "CONTINUATION_FEE_TIP_BASIS_REVIEW_COMPLETE",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "fee_model_error_rows": sum(1 for row in out if row["fee_model_error"]),
    "net_positive_after_corrected_cost_rows": sum(1 for row in out if row["net_positive_after_corrected_cost"]),
    "counterfactual_bundle_positive_rows": sum(1 for row in out if row["counterfactual_bundle_positive"]),
    "review_outcome_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "fee_model_error_rows": summary["fee_model_error_rows"],
    "net_positive_after_corrected_cost_rows": summary["net_positive_after_corrected_cost_rows"],
    "counterfactual_bundle_positive_rows": summary["counterfactual_bundle_positive_rows"],
    "review_outcome_counts": dict(decision_counts),
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

pass "Run66CTFEE continuation fee/tip basis review written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

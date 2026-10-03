#!/usr/bin/env bash
# 66cte_continuation_negative_after_cost_autopsy.sh - autopsy Continuation rows blocked by negative after-cost EV

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTE - Continuation Negative-After-Cost Autopsy"

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
STEP="${SHADOW_RUN66CTE_STEP:-66cte_continuation_negative_after_cost_autopsy}"
SOURCE_ROWS="${SHADOW_RUN66CTE_SOURCE_ROWS:-}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66ct2_continuation_execution_rule_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66cte_continuation_negative_after_cost_autopsy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66cte_continuation_negative_after_cost_autopsy.json"
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


rows = [
    row for row in load_jsonl(source_path)
    if text(row.get("decision")) == "BLOCKED_NEGATIVE_AFTER_COST"
]
out = []
for row in rows:
    fee_cost = safe_int(row.get("pm_lock_fee_raw") or row.get("pm_reserved_fee_raw"))
    tip_cost = safe_int(row.get("pm_lock_tip_raw") or row.get("pm_reserved_tip_raw")) + safe_int(row.get("extra_tip_cost"))
    rent_cost = safe_int(row.get("ata_create_rent_lamports")) + safe_int(row.get("ata_create_fee_lamports"))
    slippage_cost = max(0, safe_int(row.get("entry_quote_expected_out")) - safe_int(row.get("entry_min_out"))) + max(0, safe_int(row.get("exit_quote_expected_out")) - safe_int(row.get("exit_min_out")))
    adverse_cost = abs(min(0, safe_int(row.get("max_adverse_excursion"))))
    net_after_all_costs = (
        safe_int(row.get("route_specific_ev_after_cost_raw"))
        or safe_int(row.get("base_net_ev"))
        or safe_int(row.get("net_ev_with_ata_burned"))
        or safe_int(row.get("net_ev_with_ata_recovered"))
    )
    gross_plus_costs = net_after_all_costs + fee_cost + tip_cost + rent_cost + slippage_cost + adverse_cost

    if gross_plus_costs > 0 and fee_cost + tip_cost > 0:
        dominant_driver = "FEE_TIP_DRAG"
    elif gross_plus_costs > 0 and rent_cost > 0:
        dominant_driver = "RENT_DRAG"
    elif gross_plus_costs > 0 and slippage_cost > 0:
        dominant_driver = "SLIPPAGE_DRAG"
    elif adverse_cost > 0:
        dominant_driver = "ADVERSE_EXCURSION_DRAG"
    else:
        dominant_driver = "THESIS_OR_MODEL_NEGATIVE"

    item = {
        **row,
        "entry_quote_positive": safe_int(row.get("entry_min_out")) > 0 or boolish(row.get("entry_rule_ready")),
        "exit_quote_positive": safe_int(row.get("exit_min_out")) > 0 or boolish(row.get("sender_possible")) or boolish(row.get("helius_sender_no_send_ready")) or boolish(row.get("bundle_no_submit_ready")),
        "gross_thesis_positive": boolish(row.get("entry_rule_ready")) and boolish(row.get("exit_rule_ready")) and boolish(row.get("max_adverse_excursion_bounded")) and (safe_int(row.get("same_direction_followthrough_score")) >= 0 or safe_int(row.get("same_direction_continuation_pressure_score")) >= 0),
        "fees_negative": fee_cost > 0,
        "tip_negative": tip_cost > 0,
        "slippage_negative": slippage_cost > 0,
        "rent_negative": rent_cost > 0,
        "adverse_excursion_adjustment_negative": adverse_cost > 0,
        "fee_cost_lamports": fee_cost,
        "tip_cost_lamports": tip_cost,
        "slippage_cost_raw": slippage_cost,
        "rent_cost_lamports": rent_cost,
        "adverse_excursion_adjustment_raw": adverse_cost,
        "net_after_all_costs": net_after_all_costs,
        "gross_plus_costs": gross_plus_costs,
        "dominant_negative_driver": dominant_driver,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    }
    out.append(item)

decision_counts = Counter(row["dominant_negative_driver"] for row in out)
summary = {
    "status": "CONTINUATION_NEGATIVE_AFTER_COST_AUTOPSY_COMPLETE",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "fee_tip_drag_rows": decision_counts.get("FEE_TIP_DRAG", 0),
    "rent_drag_rows": decision_counts.get("RENT_DRAG", 0),
    "slippage_drag_rows": decision_counts.get("SLIPPAGE_DRAG", 0),
    "adverse_excursion_drag_rows": decision_counts.get("ADVERSE_EXCURSION_DRAG", 0),
    "thesis_or_model_negative_rows": decision_counts.get("THESIS_OR_MODEL_NEGATIVE", 0),
    "dominant_negative_driver_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "dominant_negative_driver_counts": dict(decision_counts),
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

pass "Run66CTE continuation negative-after-cost autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

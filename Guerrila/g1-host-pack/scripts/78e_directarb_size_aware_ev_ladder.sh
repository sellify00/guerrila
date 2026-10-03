#!/usr/bin/env bash
# 78E - size-aware EV ladder for live-track-scope strict DirectArb ev_not_positive rows

source "$(dirname "$0")/lib.sh"
script_header "78E - DirectArb Size-Aware EV Ladder"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78E_STEP:-epoch8_auto_strict_directarb_size_aware_ev_ladder}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78e_directarb_size_aware_ev_ladder.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78E_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78E_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
PM_AUDIT_STEP="${SHADOW_RUN78E_PM_AUDIT_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
PM_AUDIT_ROWS_PATH="${SHADOW_RUN78E_PM_AUDIT_ROWS_PATH:-$RESULTS_ROOT/$PM_AUDIT_STEP/rows.jsonl}"
ARCH_TAIL_STEP="${SHADOW_RUN78E_ARCH_TAIL_STEP:-epoch8_auto_strict_directarb_architecture_tail_autopsy}"
ARCH_TAIL_ROWS_PATH="${SHADOW_RUN78E_ARCH_TAIL_ROWS_PATH:-$RESULTS_ROOT/$ARCH_TAIL_STEP/rows.jsonl}"
SIZE_LADDER_SOL_CSV="${SHADOW_RUN78E_SIZE_LADDER_SOL_CSV:-1,0.5,0.25,0.1,0.05,0.025,0.01,0.005}"
WSOL_MINT="So11111111111111111111111111111111111111112"
CORE_INPUT_MINTS_CSV="${SHADOW_RUN78E_CORE_INPUT_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$AUTOPSY_ROWS_PATH" "$PM_AUDIT_ROWS_PATH" "$ARCH_TAIL_ROWS_PATH" "$SIZE_LADDER_SOL_CSV" "$WSOL_MINT" "$CORE_INPUT_MINTS_CSV" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter
from decimal import Decimal
from pathlib import Path

autopsy_rows_path = Path(sys.argv[1])
pm_rows_path = Path(sys.argv[2])
tail_rows_path = Path(sys.argv[3])
size_ladder = [Decimal(item.strip()) for item in sys.argv[4].split(",") if item.strip()]
wsol_mint = sys.argv[5].strip()
core_input_mints = {item.strip() for item in sys.argv[6].split(",") if item.strip()}
summary_path = Path(sys.argv[7])
summary_json_path = Path(sys.argv[8])
rows_jsonl_path = Path(sys.argv[9])
rows_csv_path = Path(sys.argv[10])


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


def safe_int(value):
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


def key4(row: dict):
    return (
        str(row.get("source_step") or ""),
        str(row.get("signature") or ""),
        str(row.get("pool") or ""),
        str(row.get("mint_in") or ""),
    )


autopsy_rows, autopsy_bad = parse_jsonl(autopsy_rows_path)
pm_rows, pm_bad = parse_jsonl(pm_rows_path)
tail_rows, tail_bad = parse_jsonl(tail_rows_path)

pm_by_key = {key4(row): row for row in pm_rows}
tail_by_key = {key4(row): row for row in tail_rows}

out_rows = []
row_decisions = {}

for row in autopsy_rows:
    if str(row.get("first_failed") or "") != "ev_not_positive":
        continue

    key = key4(row)
    tail_row = tail_by_key.get(key, {})
    pm_row = pm_by_key.get(key, {})
    mint_in = str(row.get("mint_in") or "")
    route_family = str(row.get("route_family") or "")
    owner_mode = str(row.get("owner_mode") or "")
    is_live_track_scope = as_bool(tail_row.get("is_live_track_scope"))
    if not tail_row:
        is_live_track_scope = (
            mint_in in core_input_mints
            and route_family == "tpu_quic_public"
            and owner_mode in {"", "directarb"}
        )
    tail_classification = str(tail_row.get("classification") or "")
    if tail_classification.startswith(("OUT_OF_SCOPE_", "PERMANENT_")):
        is_live_track_scope = False

    original_required_raw = safe_int(row.get("pm_required_input_raw"))
    original_expected_out_raw = safe_int(row.get("quote_expected_out_raw"))
    original_min_out_raw = safe_int(row.get("min_out_raw"))
    original_retained_ev_raw = safe_int(row.get("net_ev_before_fee_tip"))
    original_tip_raw = safe_int(row.get("tip_lamports"))
    base_fee_raw = safe_int(row.get("base_fee"))
    template_ready = as_bool(row.get("template_ready"))
    pm_available_balance_raw = safe_int(pm_row.get("pm_available_balance_raw") or row.get("pm_available_input_raw"))

    if not is_live_track_scope or mint_in != wsol_mint:
        row_decisions[key] = "OUT_OF_SCOPE"
    elif original_required_raw <= 0 or original_expected_out_raw <= 0 or original_min_out_raw <= 0:
        row_decisions[key] = "QUOTE_FAIL_AT_SMALL_SIZE"
    else:
        any_positive_small_size = False
        any_quote_fail = False
        any_pm_ready = False
        for size_sol in size_ladder:
            size_raw = sol_to_lamports(size_sol)
            if size_raw <= 0:
                continue
            ratio = Decimal(size_raw) / Decimal(original_required_raw)
            projected_expected_out_raw = int((Decimal(original_expected_out_raw) * ratio).to_integral_value())
            projected_min_out_raw = int((Decimal(original_min_out_raw) * ratio).to_integral_value())
            projected_retained_ev_raw = int((Decimal(original_retained_ev_raw) * ratio).to_integral_value())
            projected_tip_raw = int((Decimal(original_tip_raw) * ratio).to_integral_value())
            projected_net_ev_after_fee_tip = projected_retained_ev_raw - projected_tip_raw - base_fee_raw
            quote_ok = projected_expected_out_raw > 0
            min_out_positive = projected_min_out_raw > 0
            pm_ready = pm_available_balance_raw >= size_raw > 0
            any_pm_ready = any_pm_ready or pm_ready
            any_quote_fail = any_quote_fail or not (quote_ok and min_out_positive)
            if (
                size_raw < original_required_raw
                and quote_ok
                and min_out_positive
                and pm_ready
                and template_ready
                and projected_net_ev_after_fee_tip > 0
            ):
                any_positive_small_size = True

            out_rows.append(
                {
                    "source_step": str(row.get("source_step") or ""),
                    "signature": str(row.get("signature") or ""),
                    "pool": str(row.get("pool") or ""),
                    "mint_in": mint_in,
                    "mint_out": str(row.get("mint_out") or ""),
                    "route_family": route_family,
                    "size_sol": float(size_sol),
                    "size_raw": size_raw,
                    "original_required_input_raw": original_required_raw,
                    "projected_quote_expected_out_raw": projected_expected_out_raw,
                    "projected_min_out_raw": projected_min_out_raw,
                    "projected_retained_ev_raw": projected_retained_ev_raw,
                    "projected_tip_lamports": projected_tip_raw,
                    "projected_net_ev_after_fee_tip": projected_net_ev_after_fee_tip,
                    "quote_ok": quote_ok,
                    "min_out_positive": min_out_positive,
                    "pm_ready": pm_ready,
                    "template_ready": template_ready,
                    "is_live_track_scope": is_live_track_scope,
                    "ev_projection_basis": "size_scaled_quote_ratio",
                    "tail_classification": tail_classification,
                    "row_decision": "",
                }
            )

        if any_positive_small_size:
            row_decisions[key] = "POSITIVE_SMALL_SIZE"
        elif not any_pm_ready:
            row_decisions[key] = "PM_FAIL"
        elif any_quote_fail:
            row_decisions[key] = "QUOTE_FAIL_AT_SMALL_SIZE"
        else:
            row_decisions[key] = "NEGATIVE_ALL_SIZES"

for row in out_rows:
    row["row_decision"] = row_decisions.get(key4(row), "OUT_OF_SCOPE")

fieldnames = [
    "source_step",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "size_sol",
    "size_raw",
    "original_required_input_raw",
    "projected_quote_expected_out_raw",
    "projected_min_out_raw",
    "projected_retained_ev_raw",
    "projected_tip_lamports",
    "projected_net_ev_after_fee_tip",
    "quote_ok",
    "min_out_positive",
    "pm_ready",
    "template_ready",
    "is_live_track_scope",
    "ev_projection_basis",
    "tail_classification",
    "row_decision",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow(row)

decision_counts = Counter(row_decisions.values())

summary = {
    "status": "run78e_complete" if row_decisions else "blocked_no_live_track_scope_ev_not_positive_rows",
    "source_row_count": len(row_decisions),
    "size_row_count": len(out_rows),
    "row_decision_counts": dict(decision_counts),
    "malformed_autopsy_rows": autopsy_bad,
    "malformed_pm_rows": pm_bad,
    "malformed_arch_tail_rows": tail_bad,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"source_row_count={summary['source_row_count']}",
    f"size_row_count={summary['size_row_count']}",
    "row_decision_counts=" + json.dumps(summary["row_decision_counts"], separators=(",", ":")),
    f"malformed_autopsy_rows={summary['malformed_autopsy_rows']}",
    f"malformed_pm_rows={summary['malformed_pm_rows']}",
    f"malformed_arch_tail_rows={summary['malformed_arch_tail_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78E size-aware EV ladder complete"
finish

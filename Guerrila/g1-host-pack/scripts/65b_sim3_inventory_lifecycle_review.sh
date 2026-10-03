#!/usr/bin/env bash
# 65b_sim3_inventory_lifecycle_review.sh - lifecycle review for simulation-ready held-input mints; no acquisition

source "$(dirname "$0")/lib.sh"
script_header "65B-SIM3 - Inventory Lifecycle Review"

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
STEP="${SHADOW_RUN65B_SIM3_STEP:-65b_sim3_inventory_lifecycle_review}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM3_STATUS_PREFIX:-run65b_sim3_inventory_lifecycle_review}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM3_REPORT_BASENAME:-run65b_sim3_inventory_lifecycle_review}"
RUN65B_SIM2_STEP="${SHADOW_RUN65B_SIM3_SOURCE_STEP:-65b_sim2_signed_swap_simulation_20260524_5m}"
SOURCE_ROWS="$RESULTS_ROOT/$RUN65B_SIM2_STEP/run65b_sim2_signed_swap_simulation_rows.jsonl"
INVENTORY_TTL_SECONDS="${SHADOW_RUN65B_SIM3_INVENTORY_TTL_SECONDS:-300}"
MAX_INVENTORY_AT_RISK_USDC="${SHADOW_RUN65B_SIM3_MAX_INVENTORY_AT_RISK_USDC:-100.0}"
SOL_USD_EQUIVALENT="${SHADOW_RUN65B_SIM3_SOL_USD_EQUIVALENT:-1000.0}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_sim3_inventory_lifecycle_review_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run65B-SIM2 rows: $SOURCE_ROWS"
    finish
    exit 1
fi

python3 - \
    "$SOURCE_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$INVENTORY_TTL_SECONDS" \
    "$MAX_INVENTORY_AT_RISK_USDC" \
    "$SOL_USD_EQUIVALENT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

rows_path = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
inventory_ttl_seconds = int(sys.argv[7] or "300")
max_inventory_at_risk_usdc = float(sys.argv[8] or "100.0")
sol_usd_equivalent = float(sys.argv[9] or "1000.0")


def load_jsonl(path):
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


out = []
for row in load_jsonl(rows_path):
    required_usdc = safe_float(row.get("required_acquisition_usdc"))
    exit_value_usdc = safe_float(row.get("exit_value_usdc"))
    roundtrip_loss = required_usdc - exit_value_usdc if required_usdc > 0 and exit_value_usdc > 0 else None
    roundtrip_loss_pct = (roundtrip_loss / required_usdc * 100.0) if roundtrip_loss is not None and required_usdc > 0 else None
    expected_unlocked_ev_usdc = safe_float(row.get("strict_ev_sol_equivalent")) * sol_usd_equivalent
    net_after_carry = (
        expected_unlocked_ev_usdc - max(roundtrip_loss or 0.0, 0.0)
        if expected_unlocked_ev_usdc is not None
        else None
    )
    auto_flatten_available = bool(row.get("exit_quote_ok")) and exit_value_usdc >= 0.10
    flatten_simulation_passed = False
    flatten_simulation_status = "not_run_no_inventory_acquired"
    pm_reserve_sim_ok = False
    pm_reserve_sim_status = "not_run_no_acquisition"
    exposure_ok = required_usdc > 0 and required_usdc <= max_inventory_at_risk_usdc
    signed_swap_ok = bool(row.get("simulate_transaction_ok")) and row.get("signed_swap_simulation_status") == "passed"
    route_stability_status = "signed_entry_swap_simulation_passed" if signed_swap_ok else "signed_swap_simulation_failed"

    if not signed_swap_ok:
        decision = "REJECT_ROUTE_UNSTABLE"
    elif not row.get("entry_quote_ok") or not row.get("exit_quote_ok") or not auto_flatten_available:
        decision = "RETRY_LATER"
    elif not exposure_ok:
        decision = "REJECT_EXPOSURE_TOO_HIGH"
    elif net_after_carry is None or net_after_carry <= 0:
        decision = "REJECT_LOW_EV"
    else:
        decision = "INVENTORY_LIFECYCLE_REVIEW"

    exposure_efficiency = (
        safe_int(row.get("expected_would_fire_delta")) / required_usdc
        if required_usdc > 0
        else 0.0
    )
    out.append({
        "mint": row.get("mint"),
        "rank": row.get("rank"),
        "entry_quote": {
            "ok": bool(row.get("entry_quote_ok")),
            "amount_raw": row.get("entry_amount_raw"),
            "expected_out_raw": row.get("expected_out_raw"),
            "min_out_raw": row.get("min_out_raw"),
            "route_plan_labels": row.get("entry_route_plan_labels") or [],
        },
        "exit_quote": {
            "ok": bool(row.get("exit_quote_ok")),
            "exit_value_usdc": round_or_none(exit_value_usdc),
            "exit_value_ratio": row.get("exit_value_ratio"),
            "exit_out_raw": row.get("exit_out_raw"),
            "route_plan_labels": row.get("exit_route_plan_labels") or [],
        },
        "roundtrip_loss_usdc": round_or_none(roundtrip_loss),
        "roundtrip_loss_pct": round_or_none(roundtrip_loss_pct),
        "max_inventory_at_risk_usdc": round_or_none(required_usdc),
        "expected_unlocked_ev_usdc": round_or_none(expected_unlocked_ev_usdc),
        "expected_unlocked_would_fire_delta": safe_int(row.get("expected_would_fire_delta")),
        "inventory_ttl_seconds": inventory_ttl_seconds,
        "auto_flatten_available": auto_flatten_available,
        "auto_flatten_route": row.get("exit_route_plan_labels") or [],
        "flatten_simulation_passed": flatten_simulation_passed,
        "flatten_simulation_status": flatten_simulation_status,
        "PM_reserve_sim_ok": pm_reserve_sim_ok,
        "PM_reserve_sim_status": pm_reserve_sim_status,
        "net_after_inventory_carry_cost_usdc": round_or_none(net_after_carry),
        "route_stability_status": route_stability_status,
        "signed_swap_simulation_passed": signed_swap_ok,
        "exposure_efficiency_delta_per_usdc": round_or_none(exposure_efficiency),
        "decision": decision,
        "acquisition_allowed": False,
        "live_send_allowed": False,
        "wire_send": 0,
        "live_landed": 0,
    })

out.sort(
    key=lambda item: (
        item["decision"] == "INVENTORY_LIFECYCLE_REVIEW",
        safe_float(item.get("exposure_efficiency_delta_per_usdc")),
        safe_float(item.get("net_after_inventory_carry_cost_usdc")),
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(rows_path),
    "row_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "lifecycle_review_count": sum(1 for row in out if row["decision"] == "INVENTORY_LIFECYCLE_REVIEW"),
    "signed_swap_simulation_passed_count": sum(1 for row in out if row["signed_swap_simulation_passed"]),
    "auto_flatten_available_count": sum(1 for row in out if row["auto_flatten_available"]),
    "PM_reserve_sim_ok_count": sum(1 for row in out if row["PM_reserve_sim_ok"]),
    "acquisition_allowed": False,
    "live_send_allowed": False,
    "wire_send": 0,
    "live_landed": 0,
    "top_items": out,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_rows={rows_path}",
    f"row_count={summary['row_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"lifecycle_review_count={summary['lifecycle_review_count']}",
    f"signed_swap_simulation_passed_count={summary['signed_swap_simulation_passed_count']}",
    f"auto_flatten_available_count={summary['auto_flatten_available_count']}",
    f"PM_reserve_sim_ok_count={summary['PM_reserve_sim_ok_count']}",
    "acquisition_allowed=false",
    "live_send_allowed=false",
    "wire_send=0",
    "live_landed=0",
]
if out:
    top = out[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_expected_unlocked_would_fire_delta={top['expected_unlocked_would_fire_delta']}",
        f"top_max_inventory_at_risk_usdc={top['max_inventory_at_risk_usdc']}",
        f"top_roundtrip_loss_usdc={top['roundtrip_loss_usdc']}",
        f"top_net_after_inventory_carry_cost_usdc={top['net_after_inventory_carry_cost_usdc']}",
        f"top_exposure_efficiency_delta_per_usdc={top['exposure_efficiency_delta_per_usdc']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B-SIM3 inventory lifecycle review written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

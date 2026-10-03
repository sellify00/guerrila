#!/usr/bin/env bash
# 65b_sim4_pm_inventory_reserve_review.sh - synthetic PM reserve/release proof for held-input bands; no acquisition/no send

source "$(dirname "$0")/lib.sh"
script_header "65B-SIM4 - PM Inventory Reserve Review"

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
STEP="${SHADOW_RUN65B_SIM4_STEP:-65b_sim4_pm_inventory_reserve_review}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM4_STATUS_PREFIX:-run65b_sim4_pm_inventory_reserve_review}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM4_REPORT_BASENAME:-run65b_sim4_pm_inventory_reserve_review}"
RUN65B_SIM3_STEP="${SHADOW_RUN65B_SIM4_SOURCE_STEP:-65b_sim3_inventory_lifecycle_review_20260524_5m}"
SOURCE_ROWS="$RESULTS_ROOT/$RUN65B_SIM3_STEP/run65b_sim3_inventory_lifecycle_review_rows.jsonl"
FEE_RESERVE_LAMPORTS="${SHADOW_RUN65B_SIM4_FEE_RESERVE_LAMPORTS:-5000}"
TIP_RESERVE_LAMPORTS="${SHADOW_RUN65B_SIM4_TIP_RESERVE_LAMPORTS:-0}"
FEE_BALANCE_LAMPORTS="${SHADOW_RUN65B_SIM4_FEE_BALANCE_LAMPORTS:-10000000}"
TIP_BALANCE_LAMPORTS="${SHADOW_RUN65B_SIM4_TIP_BALANCE_LAMPORTS:-10000000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_sim4_pm_inventory_reserve_review_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run65B-SIM3 rows: $SOURCE_ROWS"
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
    "$FEE_RESERVE_LAMPORTS" \
    "$TIP_RESERVE_LAMPORTS" \
    "$FEE_BALANCE_LAMPORTS" \
    "$TIP_BALANCE_LAMPORTS" <<'PY'
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
fee_reserve_lamports = int(sys.argv[7] or "5000")
tip_reserve_lamports = int(sys.argv[8] or "0")
fee_balance_lamports = int(sys.argv[9] or "10000000")
tip_balance_lamports = int(sys.argv[10] or "10000000")


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


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def reserve_blocker(
    *,
    source_ready,
    synthetic_inventory_applied,
    has_mint,
    required_raw,
    simulated_balance,
    fee_ok,
    tip_ok,
    held_input_classified,
    schema_ok,
):
    if not schema_ok:
        return "reserve_schema_gap"
    if not has_mint:
        return "inventory_fence_missing_mint"
    if not source_ready:
        return "actual_inventory_required"
    if not synthetic_inventory_applied:
        return "synthetic_inventory_not_applied"
    if not held_input_classified:
        return "missing_held_input_classification"
    if simulated_balance < required_raw or required_raw <= 0:
        return "actual_inventory_required"
    if not fee_ok or not tip_ok:
        return "insufficient_fee_or_tip_balance"
    return "none"


out = []
for idx, row in enumerate(load_jsonl(rows_path), start=1):
    entry = row.get("entry_quote") if isinstance(row.get("entry_quote"), dict) else {}
    mint = str(row.get("mint") or "").strip()
    expected_out_raw = safe_int(entry.get("expected_out_raw"))
    min_out_raw = safe_int(entry.get("min_out_raw"))
    required_inventory_raw = min_out_raw or expected_out_raw
    simulated_inventory_balance_raw = min_out_raw or expected_out_raw
    synthetic_inventory_applied = row.get("decision") == "INVENTORY_LIFECYCLE_REVIEW" and simulated_inventory_balance_raw > 0
    held_input_classified = bool(row.get("signed_swap_simulation_passed")) and bool(row.get("auto_flatten_available"))
    schema_ok = required_inventory_raw > 0 and simulated_inventory_balance_raw > 0
    fee_ok = fee_balance_lamports >= fee_reserve_lamports
    tip_ok = tip_balance_lamports >= tip_reserve_lamports
    has_mint = bool(mint)
    source_ready = bool(row.get("signed_swap_simulation_passed"))
    blocker = reserve_blocker(
        source_ready=source_ready,
        synthetic_inventory_applied=synthetic_inventory_applied,
        has_mint=has_mint,
        required_raw=required_inventory_raw,
        simulated_balance=simulated_inventory_balance_raw,
        fee_ok=fee_ok,
        tip_ok=tip_ok,
        held_input_classified=held_input_classified,
        schema_ok=schema_ok,
    )

    inventory_fence_before = {
        "mint": mint,
        "current_allocation_raw": 0,
        "absolute_ceiling_raw": simulated_inventory_balance_raw if synthetic_inventory_applied else 0,
        "reserved_raw": 0,
        "pending_count": 0,
    }
    reserve_ok = blocker == "none"
    if reserve_ok:
        allocation_after = required_inventory_raw
        reserved_after = required_inventory_raw
        pending_after_reserve = 1
    else:
        allocation_after = 0
        reserved_after = 0
        pending_after_reserve = 0
    inventory_fence_after_reserve = {
        "mint": mint,
        "current_allocation_raw": allocation_after,
        "absolute_ceiling_raw": inventory_fence_before["absolute_ceiling_raw"],
        "reserved_raw": reserved_after,
        "pending_count": pending_after_reserve,
    }

    release_blocker = "none"
    if reserve_ok:
        allocation_after_release = 0
        reserved_after_release = 0
        pending_after = 0
        release_ok = True
    else:
        allocation_after_release = 0
        reserved_after_release = 0
        pending_after = 0
        release_ok = False
        release_blocker = "reserve_not_created"

    inventory_fence_after_release = {
        "mint": mint,
        "current_allocation_raw": allocation_after_release,
        "absolute_ceiling_raw": inventory_fence_before["absolute_ceiling_raw"],
        "reserved_raw": reserved_after_release,
        "pending_count": pending_after,
        "terminal_reason": "ObserveOnlyNoSend" if reserve_ok else "NoReserve",
    }
    pm_reserve_input_ok = reserve_ok and simulated_inventory_balance_raw >= required_inventory_raw
    pm_reserve_inventory_band_ok = reserve_ok and inventory_fence_after_reserve["current_allocation_raw"] <= inventory_fence_after_reserve["absolute_ceiling_raw"]
    pm_release_on_no_send_ok = release_ok and pending_after == 0 and inventory_fence_after_release["reserved_raw"] == 0
    pm_reserve_fee_ok = fee_ok
    pm_reserve_tip_ok = tip_ok
    decision = "PM_RESERVE_REVIEW_READY" if (
        pm_reserve_input_ok
        and pm_reserve_inventory_band_ok
        and pm_release_on_no_send_ok
        and pending_after == 0
    ) else "PM_RESERVE_BLOCKED"

    out.append({
        "rank": row.get("rank") or idx,
        "mint": mint,
        "required_acquisition_usdc": row.get("max_inventory_at_risk_usdc"),
        "required_inventory_raw": required_inventory_raw,
        "required_inventory_raw_basis": "entry_min_out_raw" if min_out_raw > 0 else "entry_expected_out_raw",
        "simulated_inventory_balance_raw": simulated_inventory_balance_raw,
        "simulated_inventory_balance_basis": "synthetic_post_acquisition_min_out",
        "pm_reserve_input_ok": pm_reserve_input_ok,
        "pm_reserve_fee_ok": pm_reserve_fee_ok,
        "pm_reserve_tip_ok": pm_reserve_tip_ok,
        "pm_reserve_inventory_band_ok": pm_reserve_inventory_band_ok,
        "pm_release_on_no_send_ok": pm_release_on_no_send_ok,
        "pm_pending_after": pending_after,
        "inventory_fence_before": inventory_fence_before,
        "inventory_fence_after_reserve": inventory_fence_after_reserve,
        "inventory_fence_after_release": inventory_fence_after_release,
        "reserve_blocker": blocker,
        "release_blocker": release_blocker,
        "synthetic_inventory_applied": synthetic_inventory_applied,
        "held_input_classification_ok": held_input_classified,
        "schema_ok": schema_ok,
        "fee_reserve_lamports": fee_reserve_lamports,
        "tip_reserve_lamports": tip_reserve_lamports,
        "fee_balance_lamports": fee_balance_lamports,
        "tip_balance_lamports": tip_balance_lamports,
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "acquisition_allowed": False,
        "live_send_allowed": False,
    })

out.sort(key=lambda item: (item["decision"] == "PM_RESERVE_REVIEW_READY", safe_float(item.get("required_acquisition_usdc"))), reverse=True)
write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(rows_path),
    "row_count": len(out),
    "pm_reserve_review_ready_count": sum(1 for row in out if row["decision"] == "PM_RESERVE_REVIEW_READY"),
    "pm_reserve_input_ok_count": sum(1 for row in out if row["pm_reserve_input_ok"]),
    "pm_reserve_inventory_band_ok_count": sum(1 for row in out if row["pm_reserve_inventory_band_ok"]),
    "pm_release_on_no_send_ok_count": sum(1 for row in out if row["pm_release_on_no_send_ok"]),
    "pm_pending_after_zero_count": sum(1 for row in out if row["pm_pending_after"] == 0),
    "reserve_blocker_counts": dict(Counter(row["reserve_blocker"] for row in out)),
    "release_blocker_counts": dict(Counter(row["release_blocker"] for row in out)),
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
    f"pm_reserve_review_ready_count={summary['pm_reserve_review_ready_count']}",
    f"pm_reserve_input_ok_count={summary['pm_reserve_input_ok_count']}",
    f"pm_reserve_inventory_band_ok_count={summary['pm_reserve_inventory_band_ok_count']}",
    f"pm_release_on_no_send_ok_count={summary['pm_release_on_no_send_ok_count']}",
    f"pm_pending_after_zero_count={summary['pm_pending_after_zero_count']}",
    f"reserve_blocker_counts={json.dumps(summary['reserve_blocker_counts'], sort_keys=True)}",
    f"release_blocker_counts={json.dumps(summary['release_blocker_counts'], sort_keys=True)}",
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
        f"top_required_inventory_raw={top['required_inventory_raw']}",
        f"top_simulated_inventory_balance_raw={top['simulated_inventory_balance_raw']}",
        f"top_reserve_blocker={top['reserve_blocker']}",
        f"top_release_blocker={top['release_blocker']}",
        f"top_pm_pending_after={top['pm_pending_after']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B-SIM4 PM inventory reserve review written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

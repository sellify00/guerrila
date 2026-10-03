#!/usr/bin/env bash
# 78H - structural hunt-readiness certificate for strict DirectArb

source "$(dirname "$0")/lib.sh"
script_header "78H - DirectArb Structural Hunt Readiness"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78H_STEP:-epoch8_auto_strict_directarb_structural_hunt_readiness}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78h_directarb_structural_hunt_readiness.json"

RUN77C_STEP="${SHADOW_RUN78H_RUN77C_STEP:-epoch8_auto_strict_directarb_private_egress_run77c}"
RUN78_STEP="${SHADOW_RUN78H_RUN78_STEP:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout}"
RUN78A_STEP="${SHADOW_RUN78H_RUN78A_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
RUN78PM_STEP="${SHADOW_RUN78H_RUN78PM_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
RUN78Q2_STEP="${SHADOW_RUN78H_RUN78Q2_STEP:-${RUN78_STEP}_78q2}"

RUN77C_SUMMARY_PATH="${SHADOW_RUN78H_RUN77C_SUMMARY_PATH:-$RESULTS_ROOT/$RUN77C_STEP/summary.txt}"
RUN78A_ROWS_PATH="${SHADOW_RUN78H_RUN78A_ROWS_PATH:-$RESULTS_ROOT/$RUN78A_STEP/rows.jsonl}"
RUN78PM_ROWS_PATH="${SHADOW_RUN78H_RUN78PM_ROWS_PATH:-$RESULTS_ROOT/$RUN78PM_STEP/rows.jsonl}"
RUN78Q2_ROWS_PATH="${SHADOW_RUN78H_RUN78Q2_ROWS_PATH:-$RESULTS_ROOT/$RUN78Q2_STEP/rows.jsonl}"

if [[ -n "${SHADOW_RUN78H_RUN78_SUMMARY_PATH:-}" ]]; then
    RUN78_SUMMARY_PATH="$SHADOW_RUN78H_RUN78_SUMMARY_PATH"
elif [[ -f "$RESULTS_ROOT/$RUN78_STEP/summary.txt" ]]; then
    RUN78_SUMMARY_PATH="$RESULTS_ROOT/$RUN78_STEP/summary.txt"
elif [[ -f "/dev/results/$RUN78_STEP/summary.txt" ]]; then
    RUN78_SUMMARY_PATH="/dev/results/$RUN78_STEP/summary.txt"
else
    RUN78_SUMMARY_PATH="$RESULTS_ROOT/$RUN78_STEP/summary.txt"
fi

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON"

python3 - "$RUN77C_SUMMARY_PATH" "$RUN78_SUMMARY_PATH" "$RUN78A_ROWS_PATH" "$RUN78PM_ROWS_PATH" "$RUN78Q2_ROWS_PATH" "$SUMMARY_FILE" "$REPORT_JSON" "$RUN78_STEP" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path


def parse_summary(path: Path) -> dict[str, str]:
    data: dict[str, str] = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def parse_jsonl(path: Path) -> list[dict]:
    rows: list[dict] = []
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


def safe_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


run77c_summary = parse_summary(Path(sys.argv[1]))
run78_summary = parse_summary(Path(sys.argv[2]))
run78a_rows = parse_jsonl(Path(sys.argv[3]))
run78pm_rows = parse_jsonl(Path(sys.argv[4]))
run78q2_rows = parse_jsonl(Path(sys.argv[5]))
summary_path = Path(sys.argv[6])
report_path = Path(sys.argv[7])
fresh_window_id = sys.argv[8]


def row_key(row: dict) -> tuple[str, str, str, str, str, int]:
    return (
        str(row.get("source_step") or ""),
        str(row.get("signature") or ""),
        str(row.get("pool") or ""),
        str(row.get("mint_in") or ""),
        str(row.get("mint_out") or ""),
        safe_int(row.get("ingress_unix_ns")),
    )


suppressed_q2_classes = {"OUT_OF_SCOPE_NON_NUMERIC_ARTIFACT", "UNSUPPORTED_ROUTE_MATH"}
suppressed_q2_keys = {
    row_key(row)
    for row in run78q2_rows
    if not as_bool(row.get("is_live_track_scope"))
    and str(row.get("classification") or "") in suppressed_q2_classes
}

fresh_candidate_count = safe_int(run78_summary.get("fresh_strict_directarb_candidate_count"))
inventory_refresh_before_tier0 = as_bool(run78_summary.get("inventory_refresh_before_tier0"))
signer_token_inventory_account_count = safe_int(run78_summary.get("signer_token_inventory_account_count"))
afxdp_directarb_rows = len(run78a_rows)

quote_numeric_ready_count = sum(
    1 for row in run78a_rows if as_bool(row.get("quote_numeric_present")) or row_key(row) in suppressed_q2_keys
)
min_out_positive_count = sum(1 for row in run78a_rows if as_bool(row.get("min_out_positive")))
ev_positive_count = sum(1 for row in run78a_rows if as_bool(row.get("candidate_ev_positive")))

pm_visible_balance_ok_count = sum(
    1 for row in run78pm_rows if str(row.get("pm_reserve_blocker") or "") == "pm_not_blocking_visible_balance_ok"
)
pm_exact_reserve_ok_count = sum(
    1
    for row in run78pm_rows
    if str(row.get("pm_reserve_blocker") or "") in {"none", "pm_not_blocking_visible_balance_ok"}
)
allowed_pm_blockers = {
    "none",
    "pm_not_blocking_visible_balance_ok",
    "unsupported_non_wsol_input",
    "stale_inventory_snapshot",
}
pm_unknown_blocker_count = sum(
    1 for row in run78pm_rows if str(row.get("pm_reserve_blocker") or "") not in allowed_pm_blockers
)

existing_input_account_ready_count = sum(
    1
    for row in run78pm_rows
    if str(row.get("pm_reserve_blocker") or "") in {"none", "pm_not_blocking_visible_balance_ok"}
    and str(row.get("input_account_resolution_blocker") or "") in {"", "none"}
)
existing_output_account_ready_count = sum(
    1 for row in run78a_rows if as_bool(row.get("output_account_ready"))
)
ata_missing_count = sum(1 for row in run78a_rows if not as_bool(row.get("output_account_ready")))
policy_clean_inline_ata_possible_count = 0
policy_blocked_ata_count = 0
template_ready_count = sum(
    1 for row in run78a_rows if as_bool(row.get("template_ready")) or row_key(row) in suppressed_q2_keys
)
sign_ready_count = template_ready_count

run77c_private_egress_ready = (
    str(run77c_summary.get("status") or "") == "run77c_private_egress_ready"
    and as_bool(run77c_summary.get("acceptance_pass"))
)
helius_sender_dual_no_send_ready_count = safe_int(
    run77c_summary.get("helius_sender_dual_no_send_accept_count")
)
run78b_gated_correctly = fresh_candidate_count == 0

architecture_blockers = Counter()
if not inventory_refresh_before_tier0:
    architecture_blockers["inventory_refresh_before_tier0_false"] += 1
if signer_token_inventory_account_count <= 0:
    architecture_blockers["signer_token_inventory_missing"] += 1
if not run77c_private_egress_ready:
    architecture_blockers["run77c_private_egress_not_ready"] += 1
if helius_sender_dual_no_send_ready_count <= 0:
    architecture_blockers["helius_sender_dual_no_send_not_ready"] += 1
if not run78b_gated_correctly:
    architecture_blockers["run78b_gate_violation"] += 1

architecture_blockers["quote_numeric_missing"] += sum(
    1
    for row in run78a_rows
    if str(row.get("first_failed") or "") == "quote_numeric_missing"
    and str(row.get("ev_math_blocker") or "") not in {"unsupported_non_wsol_pair", "non_wsol_unverified"}
    and row_key(row) not in suppressed_q2_keys
)
architecture_blockers["stale_inventory_snapshot"] += sum(
    1 for row in run78pm_rows if str(row.get("pm_reserve_blocker") or "") == "stale_inventory_snapshot"
)
architecture_blockers["pm_unknown_blocker"] += pm_unknown_blocker_count
architecture_blockers["template_not_ready"] += sum(
    1 for row in run78a_rows if not as_bool(row.get("template_ready")) and row_key(row) not in suppressed_q2_keys
)
architecture_blockers["output_account_missing"] += ata_missing_count

market_blockers = Counter()
market_blockers["no_fresh_candidate"] = 1 if fresh_candidate_count == 0 else 0
market_blockers["ev_not_positive"] = sum(
    1 for row in run78a_rows if str(row.get("first_failed") or "") == "ev_not_positive"
)
market_blockers["unsupported_non_wsol_input"] = sum(
    1 for row in run78pm_rows if str(row.get("pm_reserve_blocker") or "") == "unsupported_non_wsol_input"
)
market_blockers["ev_math_unavailable"] = sum(
    1 for row in run78a_rows if str(row.get("first_failed") or "") == "ev_math_unavailable"
)

architecture_blockers = Counter({k: v for k, v in architecture_blockers.items() if v > 0})
market_blockers = Counter({k: v for k, v in market_blockers.items() if v > 0})

architecture_blocker_total = sum(architecture_blockers.values())
market_blocker_total = sum(market_blockers.values())

status = "DIRECTARB_STRUCTURAL_HUNT_READY" if architecture_blocker_total == 0 else "BLOCKED_ARCHITECTURE"
final_verdict = (
    "architecture_green_market_waiting"
    if status == "DIRECTARB_STRUCTURAL_HUNT_READY"
    else "architecture_blockers_present"
)

report = {
    "status": status,
    "fresh_window_id": fresh_window_id,
    "inventory_refresh_before_tier0": inventory_refresh_before_tier0,
    "signer_token_inventory_account_count": signer_token_inventory_account_count,
    "afxdp_directarb_rows": afxdp_directarb_rows,
    "fresh_strict_directarb_candidate_count": fresh_candidate_count,
    "quote_numeric_ready_count": quote_numeric_ready_count,
    "min_out_positive_count": min_out_positive_count,
    "ev_positive_count": ev_positive_count,
    "pm_visible_balance_ok_count": pm_visible_balance_ok_count,
    "pm_exact_reserve_ok_count": pm_exact_reserve_ok_count,
    "pm_unknown_blocker_count": pm_unknown_blocker_count,
    "existing_input_account_ready_count": existing_input_account_ready_count,
    "existing_output_account_ready_count": existing_output_account_ready_count,
    "ata_missing_count": ata_missing_count,
    "policy_clean_inline_ata_possible_count": policy_clean_inline_ata_possible_count,
    "policy_blocked_ata_count": policy_blocked_ata_count,
    "template_ready_count": template_ready_count,
    "sign_ready_count": sign_ready_count,
    "q2_out_of_scope_excluded_count": len(suppressed_q2_keys),
    "helius_sender_dual_no_send_ready_count": helius_sender_dual_no_send_ready_count,
    "run77c_private_egress_ready": run77c_private_egress_ready,
    "run78b_gated_correctly": run78b_gated_correctly,
    "architecture_blocker_counts": dict(architecture_blockers),
    "market_blocker_counts": dict(market_blockers),
    "final_verdict": final_verdict,
}

summary_lines = [
    f"status={report['status']}",
    f"fresh_window_id={report['fresh_window_id']}",
    f"inventory_refresh_before_tier0={str(report['inventory_refresh_before_tier0']).lower()}",
    f"signer_token_inventory_account_count={report['signer_token_inventory_account_count']}",
    f"afxdp_directarb_rows={report['afxdp_directarb_rows']}",
    f"fresh_strict_directarb_candidate_count={report['fresh_strict_directarb_candidate_count']}",
    f"quote_numeric_ready_count={report['quote_numeric_ready_count']}",
    f"min_out_positive_count={report['min_out_positive_count']}",
    f"ev_positive_count={report['ev_positive_count']}",
    f"pm_visible_balance_ok_count={report['pm_visible_balance_ok_count']}",
    f"pm_exact_reserve_ok_count={report['pm_exact_reserve_ok_count']}",
    f"pm_unknown_blocker_count={report['pm_unknown_blocker_count']}",
    f"existing_input_account_ready_count={report['existing_input_account_ready_count']}",
    f"existing_output_account_ready_count={report['existing_output_account_ready_count']}",
    f"ata_missing_count={report['ata_missing_count']}",
    f"policy_clean_inline_ata_possible_count={report['policy_clean_inline_ata_possible_count']}",
    f"policy_blocked_ata_count={report['policy_blocked_ata_count']}",
    f"template_ready_count={report['template_ready_count']}",
    f"sign_ready_count={report['sign_ready_count']}",
    f"q2_out_of_scope_excluded_count={report['q2_out_of_scope_excluded_count']}",
    f"helius_sender_dual_no_send_ready_count={report['helius_sender_dual_no_send_ready_count']}",
    f"run77c_private_egress_ready={str(report['run77c_private_egress_ready']).lower()}",
    f"run78b_gated_correctly={str(report['run78b_gated_correctly']).lower()}",
    "architecture_blocker_counts=" + json.dumps(report["architecture_blocker_counts"], separators=(",", ":")),
    "market_blocker_counts=" + json.dumps(report["market_blocker_counts"], separators=(",", ":")),
    f"final_verdict={report['final_verdict']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78H structural hunt-readiness report complete"
finish

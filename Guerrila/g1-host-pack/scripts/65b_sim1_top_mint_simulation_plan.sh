#!/usr/bin/env bash
# 65b_sim1_top_mint_simulation_plan.sh - top held-input overlay plan; no acquisition/no execute

source "$(dirname "$0")/lib.sh"
script_header "65B-SIM1 - Top-Mint Simulation Plan"

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
STEP="${SHADOW_RUN65B_SIM1_STEP:-65b_sim1_top_mint_simulation_plan}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM1_STATUS_PREFIX:-run65b_sim1_top_mint_simulation_plan}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM1_REPORT_BASENAME:-run65b_sim1_top_mint_simulation_plan}"
RUN65B_SIM_STEP="${SHADOW_RUN65B_SIM1_SOURCE_STEP:-65b_one_mint_simulation_candidates_20260524_5m}"
SOURCE_ROWS="$RESULTS_ROOT/$RUN65B_SIM_STEP/run65b_one_mint_simulation_candidates.jsonl"
TOP_N="${SHADOW_RUN65B_SIM1_TOP_N:-3}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_sim1_top_mint_simulation_plan_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run65B simulation candidate rows: $SOURCE_ROWS"
    finish
    exit 1
fi

python3 - "$SOURCE_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" "$TOP_N" <<'PY'
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
top_n = max(1, int(sys.argv[7] or "3"))

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

def text(v):
    return str(v or "").strip()

def safe_float(v):
    try:
        return float(v or 0.0)
    except Exception:
        return 0.0

def safe_int(v):
    try:
        return int(v or 0)
    except Exception:
        return 0

source = [row for row in load_jsonl(rows_path) if text(row.get("decision")) == "SIMULATE_ONE_MINT"]
source.sort(key=lambda item: (safe_float(item.get("strict_ev_sol_equivalent")), safe_int(item.get("expected_would_fire_delta"))), reverse=True)

out = []
for rank, row in enumerate(source[:top_n], start=1):
    signed_status = text(row.get("signed_swap_simulation_status")) or "not_run"
    route_status = text(row.get("route_stability_status")) or "unproven_quote_only"
    entry_ok = bool(row.get("entry_quote_ok"))
    exit_ok = bool(row.get("exit_quote_ok"))
    policy_ok = text(row.get("policy_verdict")) == "pass"
    strict_ev_positive = safe_float(row.get("strict_ev_sol_equivalent")) > 0
    expected_delta_positive = safe_int(row.get("expected_would_fire_delta")) > 0
    gates = {
        "entry_quote_ok": entry_ok,
        "exit_quote_ok": exit_ok,
        "policy_pass": policy_ok,
        "route_stability_acceptable": route_status != "drifted_and_sim_failed",
        "signed_swap_simulation_passed": signed_status == "passed",
        "strict_ev_positive": strict_ev_positive,
        "expected_would_fire_delta_positive": expected_delta_positive,
    }
    if not policy_ok:
        decision = "REJECT_POLICY"
    elif not entry_ok or not exit_ok:
        decision = "RETRY_LATER"
    elif route_status == "drifted_and_sim_failed":
        decision = "REJECT_ROUTE_UNSTABLE"
    elif not strict_ev_positive or not expected_delta_positive:
        decision = "REJECT_LOW_EV"
    elif signed_status != "passed":
        decision = "PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN"
    else:
        decision = "SIMULATE_ONE_MINT_READY"
    out.append({
        "rank": rank,
        "mint": row.get("mint"),
        "token_program": row.get("token_program"),
        "owner_mode": row.get("owner_mode"),
        "pool": row.get("pool"),
        "required_acquisition_usdc": row.get("required_acquisition_usdc"),
        "exit_value_usdc": row.get("exit_value_usdc"),
        "exit_loss_pct": row.get("exit_loss_pct"),
        "expected_would_fire_delta": safe_int(row.get("expected_would_fire_delta")),
        "strict_ev_sol_equivalent": row.get("strict_ev_sol_equivalent"),
        "entry_quote_ok": entry_ok,
        "exit_quote_ok": exit_ok,
        "route_stability_status": route_status,
        "signed_swap_simulation_status": signed_status,
        "policy_verdict": row.get("policy_verdict"),
        "recurrence_count": safe_int(row.get("recurrence_count")),
        "gate_summary": gates,
        "decision": decision,
        "execute_mode_allowed": False,
        "acquisition_allowed": False,
        "live_send_allowed": False,
        "wire_send": 0,
        "live_landed": 0,
    })

write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(rows_path),
    "source_simulate_one_mint_count": len(source),
    "top_n": top_n,
    "row_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "simulation_ready_count": sum(1 for row in out if row["decision"] == "SIMULATE_ONE_MINT_READY"),
    "plan_only_signed_swap_sim_not_run_count": sum(1 for row in out if row["decision"] == "PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN"),
    "execute_mode_allowed": False,
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
    f"source_simulate_one_mint_count={summary['source_simulate_one_mint_count']}",
    f"row_count={summary['row_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"simulation_ready_count={summary['simulation_ready_count']}",
    f"plan_only_signed_swap_sim_not_run_count={summary['plan_only_signed_swap_sim_not_run_count']}",
    "execute_mode_allowed=false",
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
        f"top_required_acquisition_usdc={top['required_acquisition_usdc']}",
        f"top_exit_value_usdc={top['exit_value_usdc']}",
        f"top_expected_would_fire_delta={top['expected_would_fire_delta']}",
        f"top_signed_swap_simulation_status={top['signed_swap_simulation_status']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B-SIM1 top-mint simulation plan written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

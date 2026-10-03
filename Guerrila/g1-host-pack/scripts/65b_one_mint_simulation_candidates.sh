#!/usr/bin/env bash
# 65b_one_mint_simulation_candidates.sh - rank Run65B overlay rows for later one-mint simulation only

source "$(dirname "$0")/lib.sh"
script_header "65B - One-Mint Simulation Candidate Ranker"

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
STEP="${SHADOW_RUN65B_SIM_STEP:-65b_one_mint_simulation_candidates}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM_STATUS_PREFIX:-run65b_one_mint_simulation_candidates}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM_REPORT_BASENAME:-run65b_one_mint_simulation_candidates}"
RUN65C_STEP="${SHADOW_RUN65B_SIM_RUN65C_STEP:-65c_policy_liquidity_unlock_audit_20260524_5m}"
RUN65C_ROWS="$RESULTS_ROOT/$RUN65C_STEP/run65b_strict_overlay_audit_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_one_mint_simulation_candidates.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN65C_ROWS" ]]; then
    fail "Missing Run65C rows: $RUN65C_ROWS"
    finish
    exit 1
fi

python3 - "$RUN65C_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
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

def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows
def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")
def text(v): return str(v or "").strip()
def safe_float(v):
    try: return float(v or 0.0)
    except Exception: return 0.0
def safe_int(v):
    try: return int(v or 0)
    except Exception: return 0

source_rows = [row for row in load_jsonl(rows_path) if text(row.get("decision")) == "SIMULATE_ONLY"]
out = []
for row in source_rows:
    source_policy = text(row.get("policy_verdict"))
    policy_verdict = "pass" if bool(row.get("policy_pass")) or source_policy in {"ok", "metadata_only_token2022_allowed"} else source_policy
    route_stability_status = text(row.get("route_stability_status")) or "unproven_quote_only"
    signed_swap_simulation_status = "not_run_ranker_only"
    entry_quote_ok = bool(row.get("entry_quote_ok"))
    exit_quote_ok = bool(row.get("exit_quote_ok"))
    exit_value = safe_float(row.get("exit_value_usdc"))
    acquisition = safe_float(row.get("required_acquisition_usdc"))
    exit_loss = safe_float(row.get("exit_loss_pct"))
    expected_delta = safe_int(row.get("expected_would_fire_delta"))
    strict_ev = safe_float(row.get("strict_ev_sol_equivalent"))
    if policy_verdict != "pass":
        decision = "REJECT_POLICY"
    elif not exit_quote_ok or exit_value < 0.10 or (acquisition > 0 and exit_value < acquisition * 0.50):
        decision = "REJECT_DUST_EXIT"
    elif route_stability_status == "drifted_and_sim_failed":
        decision = "REJECT_ROUTE_UNSTABLE"
    elif signed_swap_simulation_status == "failed":
        decision = "REJECT_ROUTE_UNSTABLE"
    elif not entry_quote_ok:
        decision = "RETRY_LATER"
    elif expected_delta <= 0 or strict_ev <= 0:
        decision = "REJECT_LOW_EV"
    else:
        decision = "SIMULATE_ONE_MINT"
    out.append({
        "mint": row.get("mint"),
        "token_program": row.get("token_program"),
        "owner_mode": row.get("owner_mode"),
        "pool": row.get("pool"),
        "required_acquisition_usdc": row.get("required_acquisition_usdc"),
        "exit_value_usdc": row.get("exit_value_usdc"),
        "exit_loss_pct": row.get("exit_loss_pct"),
        "expected_would_fire_delta": expected_delta,
        "strict_ev_sol_equivalent": row.get("strict_ev_sol_equivalent"),
        "entry_quote_ok": entry_quote_ok,
        "exit_quote_ok": exit_quote_ok,
        "route_stability_status": route_stability_status,
        "signed_swap_simulation_status": signed_swap_simulation_status,
        "policy_verdict": policy_verdict,
        "source_policy_verdict": source_policy,
        "recurrence_count": safe_int(row.get("fresh_epoch_frequency") or row.get("row_count")),
        "decision": decision,
        "sample_candidate_id": row.get("sample_candidate_id"),
        "execute_mode_allowed": False,
        "live_send_allowed": False,
    })
out.sort(key=lambda item: (item["decision"] == "SIMULATE_ONE_MINT", safe_float(item["strict_ev_sol_equivalent"]), item["expected_would_fire_delta"]), reverse=True)
write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "source_run65c_rows": str(rows_path),
    "simulate_only_source_count": len(source_rows),
    "row_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "simulate_one_mint_count": sum(1 for row in out if row["decision"] == "SIMULATE_ONE_MINT"),
    "simulate_one_mint_expected_would_fire_delta": sum(row["expected_would_fire_delta"] for row in out if row["decision"] == "SIMULATE_ONE_MINT"),
    "execute_mode_allowed": False,
    "live_send_allowed": False,
    "top_items": out[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_run65c_rows={rows_path}",
    f"simulate_only_source_count={summary['simulate_only_source_count']}",
    f"simulate_one_mint_count={summary['simulate_one_mint_count']}",
    f"simulate_one_mint_expected_would_fire_delta={summary['simulate_one_mint_expected_would_fire_delta']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "execute_mode_allowed=false",
    "live_send_allowed=false",
]
if out:
    top = out[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_required_acquisition_usdc={top['required_acquisition_usdc']}",
        f"top_exit_value_usdc={top['exit_value_usdc']}",
        f"top_expected_would_fire_delta={top['expected_would_fire_delta']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B one-mint simulation candidates written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

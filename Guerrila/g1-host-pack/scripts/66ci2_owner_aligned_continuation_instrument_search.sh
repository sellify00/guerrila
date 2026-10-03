#!/usr/bin/env bash
# 66ci2_owner_aligned_continuation_instrument_search.sh - exact-positive continuation instrument search; no acquisition/no send

source "$(dirname "$0")/lib.sh"
script_header "66CI2 - Owner-Aligned Continuation Instrument Search"

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
STEP="${SHADOW_RUN66CI2_STEP:-66ci2_owner_aligned_continuation_instrument_search}"
STATUS_PREFIX="${SHADOW_RUN66CI2_STATUS_PREFIX:-run66ci2_owner_aligned_continuation_instrument_search}"
REPORT_BASENAME="${SHADOW_RUN66CI2_REPORT_BASENAME:-run66ci2_owner_aligned_continuation_instrument_search}"
READINESS_GLOB="${SHADOW_RUN66CI2_READINESS_GLOB:-66u_repeatability_chain_20260524_3x5m_retry3*_66c_readiness/run66c_continuation_readiness_rows.jsonl}"
RUN66CI_STEP="${SHADOW_RUN66CI2_RUN66CI_STEP:-66ci_continuation_held_input_inventory_planner_20260524_3x5m}"
RUN66CI_ROWS="$RESULTS_ROOT/$RUN66CI_STEP/run66ci_continuation_held_input_inventory_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66ci2_owner_aligned_continuation_instrument_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN66CI_ROWS" ]]; then
    fail "Missing Run66CI rows: $RUN66CI_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RESULTS_ROOT" \
    "$READINESS_GLOB" \
    "$RUN66CI_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

results_root = Path(sys.argv[1])
readiness_glob = sys.argv[2]
ci_rows_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
rows_jsonl = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return str(value or "").strip()


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


def boolish(value):
    return bool(value)


def is_exact_positive_continuation(row):
    return (
        text(row.get("owner_mode")) == "continuation"
        and boolish(row.get("exact_quote_match"))
        and safe_int(row.get("min_out_raw")) > 0
        and boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    )


ci_by_mint = {text(row.get("mint")): row for row in load_jsonl(ci_rows_path)}
readiness_paths = sorted(results_root.glob(readiness_glob))
exact_rows = []
drop_ignored_count = 0
for path in readiness_paths:
    for row in load_jsonl(path):
        if text(row.get("owner_mode")) == "drop":
            drop_ignored_count += 1
            continue
        if is_exact_positive_continuation(row):
            enriched = dict(row)
            enriched["source_readiness_path"] = str(path)
            exact_rows.append(enriched)

instrument_groups = defaultdict(list)
for row in exact_rows:
    if not boolish(row.get("baseline_would_fire")) and boolish(row.get("synthetic_inventory_overlay_would_fire")):
        instrument_groups[text(row.get("mint_in"))].append(row)

out = []
for mint, rows in instrument_groups.items():
    ci = ci_by_mint.get(mint, {})
    continuation_delta = len(rows)
    exact_quote_count = sum(1 for row in rows if boolish(row.get("exact_quote_match")))
    min_out_count = sum(1 for row in rows if safe_int(row.get("min_out_raw")) > 0)
    matched_positive_count = sum(1 for row in rows if boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive")))
    total_ev_raw = sum(safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw")) for row in rows)
    policy_verdict = text(ci.get("policy_verdict")) or "missing_policy"
    exit_quote_ok = boolish(ci.get("exit_quote_ok"))
    exit_dust = safe_float(ci.get("exit_value_usdc")) < 0.10
    auto_flatten_available = boolish(ci.get("auto_flatten_available"))
    net_after_carry = safe_float(ci.get("net_after_inventory_carry_cost_usdc"))
    if exact_quote_count <= 0:
        decision = "REJECT_NO_EXACT_QUOTE"
    elif min_out_count <= 0:
        decision = "REJECT_NO_MIN_OUT"
    elif matched_positive_count <= 0:
        decision = "REJECT_NO_MATCHED_ROLLOUT_EV"
    elif policy_verdict != "pass":
        decision = "REJECT_POLICY"
    elif not exit_quote_ok or exit_dust:
        decision = "REJECT_DUST_EXIT"
    elif net_after_carry <= 0:
        decision = "REJECT_LOW_EV"
    else:
        decision = "CONTINUATION_INSTRUMENT_CANDIDATE"

    out.append({
        "mint": mint,
        "owner_mode": "continuation",
        "continuation_would_fire_delta": continuation_delta,
        "drop_would_fire_delta": 0,
        "exact_quote_match_count": exact_quote_count,
        "min_out_positive_count": min_out_count,
        "matched_rollout_net_ev_after_fee_tip_positive_count": matched_positive_count,
        "total_matched_rollout_net_ev_after_fee_tip_raw": total_ev_raw,
        "sample_count": ci.get("sample_count"),
        "sample_names": ci.get("sample_names") or [],
        "row_count": len(rows),
        "policy_verdict": policy_verdict,
        "token_program": ci.get("token_program"),
        "decimals": ci.get("decimals"),
        "entry_quote_ok": boolish(ci.get("entry_quote_ok")),
        "exit_quote_ok": exit_quote_ok,
        "exit_value_usdc": ci.get("exit_value_usdc"),
        "exit_dust": exit_dust,
        "exit_loss_pct": ci.get("exit_loss_pct"),
        "auto_flatten_available": auto_flatten_available,
        "required_inventory_raw": ci.get("required_inventory_raw"),
        "required_inventory_usdc": ci.get("required_inventory_usdc"),
        "max_inventory_at_risk_usdc": ci.get("max_inventory_at_risk_usdc"),
        "roundtrip_loss_usdc": ci.get("roundtrip_loss_usdc"),
        "net_after_inventory_carry_cost_usdc": ci.get("net_after_inventory_carry_cost_usdc"),
        "route_stability_status": ci.get("route_stability_status"),
        "signed_swap_simulation_passed": boolish(ci.get("signed_swap_simulation_passed")),
        "decision": decision,
        "acquisition_allowed": False,
        "live_send_allowed": False,
        "wire_send": 0,
        "live_landed": 0,
    })

out.sort(
    key=lambda row: (
        row["decision"] == "CONTINUATION_INSTRUMENT_CANDIDATE",
        safe_float(row.get("net_after_inventory_carry_cost_usdc")),
        safe_int(row.get("continuation_would_fire_delta")),
        safe_int(row.get("total_matched_rollout_net_ev_after_fee_tip_raw")),
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, out)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_readiness_paths": [str(path) for path in readiness_paths],
    "source_66ci_rows": str(ci_rows_path),
    "exact_positive_continuation_rows": len(exact_rows),
    "owner_aligned_inventory_only_rows": sum(row["continuation_would_fire_delta"] for row in out),
    "instrument_group_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "continuation_instrument_candidate_count": sum(1 for row in out if row["decision"] == "CONTINUATION_INSTRUMENT_CANDIDATE"),
    "drop_ignored_count": drop_ignored_count,
    "wire_send": 0,
    "live_landed": 0,
    "acquisition_allowed": False,
    "top_items": out[:10],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"exact_positive_continuation_rows={summary['exact_positive_continuation_rows']}",
    f"owner_aligned_inventory_only_rows={summary['owner_aligned_inventory_only_rows']}",
    f"instrument_group_count={summary['instrument_group_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"continuation_instrument_candidate_count={summary['continuation_instrument_candidate_count']}",
    f"drop_ignored_count={drop_ignored_count}",
    "acquisition_allowed=false",
    "wire_send=0",
    "live_landed=0",
]
if out:
    top = out[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_continuation_would_fire_delta={top['continuation_would_fire_delta']}",
        f"top_exact_quote_match_count={top['exact_quote_match_count']}",
        f"top_min_out_positive_count={top['min_out_positive_count']}",
        f"top_matched_rollout_net_ev_after_fee_tip_positive_count={top['matched_rollout_net_ev_after_fee_tip_positive_count']}",
        f"top_policy_verdict={top['policy_verdict']}",
        f"top_exit_quote_ok={str(top['exit_quote_ok']).lower()}",
        f"top_exit_dust={str(top['exit_dust']).lower()}",
        f"top_net_after_inventory_carry_cost_usdc={top['net_after_inventory_carry_cost_usdc']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66CI2 owner-aligned continuation instrument search written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

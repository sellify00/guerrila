#!/usr/bin/env bash
# 65b_sim5_one_mint_conversion_replay.sh - one-mint synthetic held-input conversion replay; no acquisition/no send

source "$(dirname "$0")/lib.sh"
script_header "65B-SIM5 - One Mint Synthetic Conversion Replay"

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
STEP="${SHADOW_RUN65B_SIM5_STEP:-65b_sim5_one_mint_conversion_replay}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM5_STATUS_PREFIX:-run65b_sim5_one_mint_conversion_replay}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM5_REPORT_BASENAME:-run65b_sim5_one_mint_conversion_replay}"
TARGET_MINT="${SHADOW_RUN65B_SIM5_TARGET_MINT:-vRseBFqTy9QLmmo5qGiwo74AVpdqqMTnxPqWoWMpump}"
TARGET_OWNER_MODE="${SHADOW_RUN65B_SIM5_TARGET_OWNER_MODE:-continuation}"

RUN65B_SIM1_STEP="${SHADOW_RUN65B_SIM5_SIM1_STEP:-65b_sim1_top_mint_simulation_plan_20260524_5m}"
RUN65B_SIM2_STEP="${SHADOW_RUN65B_SIM5_SIM2_STEP:-65b_sim2_signed_swap_simulation_20260524_5m}"
RUN65B_SIM3_STEP="${SHADOW_RUN65B_SIM5_SIM3_STEP:-65b_sim3_inventory_lifecycle_review_20260524_5m}"
RUN65B_SIM4_STEP="${SHADOW_RUN65B_SIM5_SIM4_STEP:-65b_sim4_pm_inventory_reserve_review_20260524_5m}"

SIM1_ROWS="$RESULTS_ROOT/$RUN65B_SIM1_STEP/run65b_sim1_top_mint_simulation_plan_rows.jsonl"
SIM2_ROWS="$RESULTS_ROOT/$RUN65B_SIM2_STEP/run65b_sim2_signed_swap_simulation_rows.jsonl"
SIM3_ROWS="$RESULTS_ROOT/$RUN65B_SIM3_STEP/run65b_sim3_inventory_lifecycle_review_rows.jsonl"
SIM4_ROWS="$RESULTS_ROOT/$RUN65B_SIM4_STEP/run65b_sim4_pm_inventory_reserve_review_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_sim5_one_mint_conversion_replay_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

for required in "$SIM1_ROWS" "$SIM2_ROWS" "$SIM3_ROWS" "$SIM4_ROWS"; do
    if [[ ! -f "$required" ]]; then
        fail "Missing required SIM5 source rows: $required"
        finish
        exit 1
    fi
done

python3 - \
    "$RESULTS_ROOT" \
    "$TARGET_MINT" \
    "$SIM1_ROWS" \
    "$SIM2_ROWS" \
    "$SIM3_ROWS" \
    "$SIM4_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$TARGET_OWNER_MODE" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

results_root = Path(sys.argv[1])
target_mint = str(sys.argv[2] or "").strip()
sim1_rows_path = Path(sys.argv[3])
sim2_rows_path = Path(sys.argv[4])
sim3_rows_path = Path(sys.argv[5])
sim4_rows_path = Path(sys.argv[6])
summary_file = Path(sys.argv[7])
summary_json = Path(sys.argv[8])
rows_jsonl = Path(sys.argv[9])
step = sys.argv[10]
status_prefix = sys.argv[11]
target_owner_mode = str(sys.argv[12] or "continuation").strip() or "continuation"


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


def first_for_mint(rows, key="mint"):
    matches = [row for row in rows if text(row.get(key)) == target_mint]
    matches.sort(key=lambda row: (safe_int(row.get("expected_would_fire_delta") or row.get("expected_unlocked_would_fire_delta")), safe_float(row.get("strict_ev_sol_equivalent"))), reverse=True)
    return matches[0] if matches else {}


def count_owner_delta(rows):
    counts = Counter()
    for row in rows:
        owner = text(row.get("owner_mode")) or "unknown"
        counts[owner] += safe_int(row.get("expected_would_fire_delta"))
    return counts


sim1_rows = load_jsonl(sim1_rows_path)
sim2_rows = load_jsonl(sim2_rows_path)
sim3_rows = load_jsonl(sim3_rows_path)
sim4_rows = load_jsonl(sim4_rows_path)

sim1_target_rows = [row for row in sim1_rows if text(row.get("mint")) == target_mint]
sim2_target = first_for_mint(sim2_rows)
sim3_target = first_for_mint(sim3_rows)
sim4_target = first_for_mint(sim4_rows)
owner_counts = count_owner_delta(sim1_target_rows)

readiness_paths = sorted(results_root.glob("66u_repeatability_chain_20260524_3x5m_retry3*_66c_readiness/run66c_continuation_readiness_rows.jsonl"))
continuation_rows = []
for path in readiness_paths:
    for row in load_jsonl(path):
        if text(row.get("mint_in")) == target_mint or text(row.get("mint_out")) == target_mint:
            enriched = dict(row)
            enriched["source_readiness_path"] = str(path)
            continuation_rows.append(enriched)

exact_quote_match_count = sum(1 for row in continuation_rows if boolish(row.get("exact_quote_match")))
numeric_quote_min_out_count = sum(1 for row in continuation_rows if safe_int(row.get("min_out_raw")) > 0)
min_out_positive_count = numeric_quote_min_out_count
matched_positive_count = sum(1 for row in continuation_rows if boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive")))
continuation_would_fire_delta = sum(
    1
    for row in continuation_rows
    if not boolish(row.get("baseline_would_fire")) and boolish(row.get("synthetic_inventory_overlay_would_fire"))
)

entry_quote = sim3_target.get("entry_quote") if isinstance(sim3_target.get("entry_quote"), dict) else {}
exit_quote = sim3_target.get("exit_quote") if isinstance(sim3_target.get("exit_quote"), dict) else {}

entry_quote_ok = boolish(sim2_target.get("entry_quote_ok")) or boolish(entry_quote.get("ok"))
exit_quote_ok = boolish(sim2_target.get("exit_quote_ok")) or boolish(exit_quote.get("ok"))
auto_flatten_available = boolish(sim3_target.get("auto_flatten_available"))
flatten_simulation_passed = boolish(sim3_target.get("flatten_simulation_passed"))
pm_reserve_ok = (
    boolish(sim4_target.get("pm_reserve_input_ok"))
    and boolish(sim4_target.get("pm_reserve_inventory_band_ok"))
    and boolish(sim4_target.get("pm_reserve_fee_ok"))
    and boolish(sim4_target.get("pm_reserve_tip_ok"))
)
pm_release_ok = boolish(sim4_target.get("pm_release_on_no_send_ok"))
pm_pending_after = safe_int(sim4_target.get("pm_pending_after"))

baseline_would_fire = 0
synthetic_target_would_fire = safe_int(sim2_target.get("expected_would_fire_delta") or sim3_target.get("expected_unlocked_would_fire_delta"))
would_fire_delta = synthetic_target_would_fire - baseline_would_fire
drop_would_fire_delta = owner_counts.get("drop", 0)
rebalancing_would_fire_delta = owner_counts.get("rebalancing", 0)
directarb_would_fire_delta = owner_counts.get("directarb", 0)
housekeeping_would_fire_delta = owner_counts.get("housekeeping", 0)
if target_owner_mode == "continuation":
    target_owner_mode_delta = continuation_would_fire_delta
else:
    target_owner_mode_delta = owner_counts.get(target_owner_mode, 0)

if not sim2_target:
    first_blocker = "blocked_no_sim2_target_mint"
elif not sim3_target:
    first_blocker = "blocked_no_sim3_lifecycle_target_mint"
elif not sim4_target:
    first_blocker = "blocked_no_sim4_pm_target_mint"
elif target_owner_mode_delta <= 0 and drop_would_fire_delta > 0:
    first_blocker = "DROP_ONLY_RESEARCH"
elif target_owner_mode_delta <= 0:
    first_blocker = "WRONG_OWNER_MODE"
elif exact_quote_match_count <= 0:
    first_blocker = "NO_EXACT_QUOTE_PROOF"
elif min_out_positive_count <= 0:
    first_blocker = "NO_MIN_OUT_PROOF"
elif matched_positive_count <= 0:
    first_blocker = "NO_MATCHED_ROLLOUT_EV"
elif not entry_quote_ok:
    first_blocker = "blocked_entry_quote"
elif not exit_quote_ok:
    first_blocker = "blocked_exit_quote"
elif not auto_flatten_available:
    first_blocker = "blocked_no_auto_flatten"
elif not pm_reserve_ok:
    first_blocker = "blocked_pm_reserve"
elif not pm_release_ok or pm_pending_after != 0:
    first_blocker = "blocked_pm_release"
else:
    first_blocker = "none"

passed = first_blocker == "none"
classification = "ACQUISITION_REVIEW_ELIGIBLE" if passed else first_blocker
status_suffix = "ACQUISITION_REVIEW_ELIGIBLE" if passed else first_blocker

row = {
    "target_mint": target_mint,
    "target_owner_mode": target_owner_mode,
    "mode": "no-send",
    "synthetic_inventory": True,
    "acquisition": False,
    "live_send": False,
    "baseline_would_fire": baseline_would_fire,
    "synthetic_target_would_fire": synthetic_target_would_fire,
    "synthetic_vRse_would_fire": synthetic_target_would_fire,
    "would_fire_delta": would_fire_delta,
    "target_owner_mode_delta": target_owner_mode_delta,
    "continuation_would_fire_delta": continuation_would_fire_delta,
    "rebalancing_would_fire_delta": rebalancing_would_fire_delta,
    "directarb_would_fire_delta": directarb_would_fire_delta,
    "housekeeping_would_fire_delta": housekeeping_would_fire_delta,
    "drop_would_fire_delta": drop_would_fire_delta,
    "owner_delta_counts": dict(owner_counts),
    "continuation_readiness_source_count": len(readiness_paths),
    "continuation_rows_for_mint": len(continuation_rows),
    "exact_quote_match_count": exact_quote_match_count,
    "numeric_quote_min_out_count": numeric_quote_min_out_count,
    "min_out_positive_count": min_out_positive_count,
    "matched_rollout_net_ev_after_fee_tip_positive_count": matched_positive_count,
    "entry_quote_ok": entry_quote_ok,
    "exit_quote_ok": exit_quote_ok,
    "auto_flatten_available": auto_flatten_available,
    "flatten_simulation_passed": flatten_simulation_passed,
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "pm_pending_after": pm_pending_after,
    "max_inventory_at_risk_usdc": sim3_target.get("max_inventory_at_risk_usdc") or sim2_target.get("required_acquisition_usdc"),
    "roundtrip_loss_usdc": sim3_target.get("roundtrip_loss_usdc"),
    "net_after_inventory_carry_cost_usdc": sim3_target.get("net_after_inventory_carry_cost_usdc"),
    "inventory_ttl_seconds": sim3_target.get("inventory_ttl_seconds"),
    "required_inventory_raw": sim4_target.get("required_inventory_raw"),
    "required_acquisition_usdc": sim4_target.get("required_acquisition_usdc") or sim2_target.get("required_acquisition_usdc"),
    "send_transaction_called": False,
    "sendBundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
    "first_replay_blocker": first_blocker,
    "owner_mode_gate_classification": classification,
    "decision": "ONE_MINT_ACQUISITION_PLAN_REVIEW" if passed else classification,
}
write_jsonl(rows_jsonl, [row])

summary = {
    "status": f"{status_prefix}_{status_suffix}",
    "step": step,
    "target_mint": target_mint,
    "target_owner_mode": target_owner_mode,
    "source_sim1_rows": str(sim1_rows_path),
    "source_sim2_rows": str(sim2_rows_path),
    "source_sim3_rows": str(sim3_rows_path),
    "source_sim4_rows": str(sim4_rows_path),
    "source_continuation_readiness_paths": [str(path) for path in readiness_paths],
    "baseline_would_fire": baseline_would_fire,
    "synthetic_target_would_fire": synthetic_target_would_fire,
    "synthetic_vRse_would_fire": synthetic_target_would_fire,
    "would_fire_delta": would_fire_delta,
    "target_owner_mode_delta": target_owner_mode_delta,
    "continuation_would_fire_delta": continuation_would_fire_delta,
    "rebalancing_would_fire_delta": rebalancing_would_fire_delta,
    "directarb_would_fire_delta": directarb_would_fire_delta,
    "housekeeping_would_fire_delta": housekeeping_would_fire_delta,
    "drop_would_fire_delta": drop_would_fire_delta,
    "exact_quote_match_count": exact_quote_match_count,
    "numeric_quote_min_out_count": numeric_quote_min_out_count,
    "min_out_positive_count": min_out_positive_count,
    "matched_rollout_net_ev_after_fee_tip_positive_count": matched_positive_count,
    "entry_quote_ok": entry_quote_ok,
    "exit_quote_ok": exit_quote_ok,
    "auto_flatten_available": auto_flatten_available,
    "flatten_simulation_passed": flatten_simulation_passed,
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "pm_pending_after": pm_pending_after,
    "max_inventory_at_risk_usdc": row["max_inventory_at_risk_usdc"],
    "roundtrip_loss_usdc": row["roundtrip_loss_usdc"],
    "net_after_inventory_carry_cost_usdc": row["net_after_inventory_carry_cost_usdc"],
    "inventory_ttl_seconds": row["inventory_ttl_seconds"],
    "required_inventory_raw": row["required_inventory_raw"],
    "required_acquisition_usdc": row["required_acquisition_usdc"],
    "first_replay_blocker": first_blocker,
    "owner_mode_gate_classification": classification,
    "decision": row["decision"],
    "acquisition_allowed": False,
    "live_send_allowed": False,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"target_mint={target_mint}",
    f"target_owner_mode={target_owner_mode}",
    "mode=no-send",
    "synthetic_inventory=true",
    "acquisition=false",
    "live_send=false",
    f"baseline_would_fire={baseline_would_fire}",
    f"synthetic_target_would_fire={synthetic_target_would_fire}",
    f"synthetic_vRse_would_fire={synthetic_target_would_fire}",
    f"would_fire_delta={would_fire_delta}",
    f"target_owner_mode_delta={target_owner_mode_delta}",
    f"continuation_would_fire_delta={continuation_would_fire_delta}",
    f"rebalancing_would_fire_delta={rebalancing_would_fire_delta}",
    f"directarb_would_fire_delta={directarb_would_fire_delta}",
    f"housekeeping_would_fire_delta={housekeeping_would_fire_delta}",
    f"drop_would_fire_delta={drop_would_fire_delta}",
    f"exact_quote_match_count={exact_quote_match_count}",
    f"numeric_quote_min_out_count={numeric_quote_min_out_count}",
    f"min_out_positive_count={min_out_positive_count}",
    f"matched_rollout_net_ev_after_fee_tip_positive_count={matched_positive_count}",
    f"entry_quote_ok={str(entry_quote_ok).lower()}",
    f"exit_quote_ok={str(exit_quote_ok).lower()}",
    f"auto_flatten_available={str(auto_flatten_available).lower()}",
    f"flatten_simulation_passed={str(flatten_simulation_passed).lower()}",
    f"pm_reserve_ok={str(pm_reserve_ok).lower()}",
    f"pm_release_ok={str(pm_release_ok).lower()}",
    f"pm_pending_after={pm_pending_after}",
    f"max_inventory_at_risk_usdc={row['max_inventory_at_risk_usdc']}",
    f"roundtrip_loss_usdc={row['roundtrip_loss_usdc']}",
    f"net_after_inventory_carry_cost_usdc={row['net_after_inventory_carry_cost_usdc']}",
    f"inventory_ttl_seconds={row['inventory_ttl_seconds']}",
    f"required_inventory_raw={row['required_inventory_raw']}",
    f"first_replay_blocker={first_blocker}",
    f"owner_mode_gate_classification={classification}",
    f"decision={row['decision']}",
    "send_transaction_called=false",
    "sendBundle_called=false",
    "wire_send=0",
    "live_landed=0",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B-SIM5 one-mint synthetic conversion replay written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

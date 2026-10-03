#!/usr/bin/env bash
# 66r_rebalancing_exact_positive_readiness_overlay.sh - exact-positive rebalancing readiness overlay, no-send only

source "$(dirname "$0")/lib.sh"
script_header "66R - Rebalancing Exact-Positive Readiness Overlay"

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
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66R_OVERLAY_STEP:-66r_rebalancing_exact_positive_readiness_overlay}"
STATUS_PREFIX="${SHADOW_RUN66R_OVERLAY_STATUS_PREFIX:-run66r_rebalancing_exact_positive_readiness_overlay}"
REPORT_BASENAME="${SHADOW_RUN66R_OVERLAY_REPORT_BASENAME:-run66r_rebalancing_exact_positive_readiness_overlay}"
RUN66Q2_STEP="${SHADOW_RUN66R_OVERLAY_RUN66Q2_STEP:-66q2_rebalancing_exact_quote_route_ev_20260524_5m_bridge2}"
RUN66_STEP="${SHADOW_RUN66R_OVERLAY_RUN66_STEP:-66_rebalancing_rule_proof_20260524_5m_quote_overlay}"
RUN66A_STEP="${SHADOW_RUN66R_OVERLAY_RUN66A_STEP:-66a_rebalancing_account_ata_unlock_planner_20260524_5m_bridge2}"
RUN66Q2_ROWS="${SHADOW_RUN66R_OVERLAY_Q2_ROWS:-$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl}"
RUN66_ROWS="${SHADOW_RUN66R_OVERLAY_RULE_ROWS:-$RESULTS_ROOT/$RUN66_STEP/run66_rebalancing_rule_rows.jsonl}"
RUN66A_ROWS="${SHADOW_RUN66R_OVERLAY_ATA_ROWS:-$RESULTS_ROOT/$RUN66A_STEP/run66a_rebalancing_account_ata_unlock_rows.jsonl}"
HYDK_MINT="${SHADOW_RUN66R_OVERLAY_HYDK_MINT:-HyDKNdnhZNVYQMruBevbNsUWruA9STmAQrS4srXApump}"
Q2_MATCH_POLICY="${SHADOW_RUN66R_OVERLAY_Q2_MATCH_POLICY:-exact_matched_rollout_only}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl"
HYDK_ROWS_JSONL="$RESULT_DIR/run66r_hydk_synthetic_ata_overlay_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$HYDK_ROWS_JSONL"

if [[ ! -f "$RUN66Q2_ROWS" ]]; then
    fail "Missing Run66Q2 rows: $RUN66Q2_ROWS"
    finish
    exit 1
fi
if [[ ! -f "$RUN66_ROWS" ]]; then
    fail "Missing Run66 rule rows: $RUN66_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RUN66Q2_ROWS" \
    "$RUN66_ROWS" \
    "$RUN66A_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$HYDK_ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$HYDK_MINT" \
    "$Q2_MATCH_POLICY" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

q2_rows_path = Path(sys.argv[1])
rule_rows_path = Path(sys.argv[2])
ata_rows_path = Path(sys.argv[3]) if sys.argv[3] else None
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
rows_jsonl = Path(sys.argv[6])
hydk_rows_jsonl = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
hydk_mint = sys.argv[10]
q2_match_policy = sys.argv[11]


def load_jsonl(path):
    rows = []
    if not Path(path).exists():
        return rows
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


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def boolish(value):
    return bool(value)


def first_blocker(row, account=None, inventory=None, quote=None, pm=None):
    if not row:
        return "rule_row_missing"
    account_ready = boolish(row.get("account_ready")) if account is None else bool(account)
    inventory_ready = boolish(row.get("inventory_ready")) if inventory is None else bool(inventory)
    quote_ready = boolish(row.get("quote_ready")) if quote is None else bool(quote)
    numeric_quote = boolish(row.get("numeric_quote_min_out")) if quote is None else bool(quote)
    pm_ok = boolish(row.get("pm_reserve_sim_ok")) if pm is None else bool(pm)
    checks = [
        ("entry_rule_missing", boolish(row.get("entry_rule_present"))),
        ("exit_rule_missing", boolish(row.get("exit_rule_present"))),
        ("route_unusable", boolish(row.get("route_usable"))),
        ("inventory_not_ready", inventory_ready),
        ("account_ata_missing", account_ready),
        ("template_missing", boolish(row.get("template_ready"))),
        ("quote_missing", quote_ready),
        ("numeric_quote_min_out_missing", numeric_quote),
        ("adverse_excursion_exceeded", boolish(row.get("adverse_excursion_ok"))),
        ("ev_not_positive_after_cost", boolish(row.get("positive_after_cost"))),
        ("pm_reserve_sim_failed", pm_ok),
    ]
    for blocker, ok in checks:
        if not ok:
            return blocker
    return "none"


def would_fire(blocker):
    return blocker == "none"


def is_exact_positive_q2(row):
    return (
        boolish(row.get("exact_quote_match"))
        and boolish(row.get("quote_numeric_preserved"))
        and boolish(row.get("min_out_positive"))
        and boolish(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    )


def is_policy_positive_q2(row, policy):
    if not boolish(row.get("quote_numeric_preserved")):
        return False
    if not boolish(row.get("min_out_positive")):
        return False
    if policy == "exact_matched_rollout_only":
        return is_exact_positive_q2(row)
    if policy == "route_cost_exact_or_unique_safe":
        source = text(row.get("quote_identity_source"))
        return (
            (source.startswith("exact_") or source.startswith("unique_safe_"))
            and boolish(row.get("net_ev_after_fee_tip_positive"))
        )
    raise ValueError(f"Unsupported SHADOW_RUN66R_OVERLAY_Q2_MATCH_POLICY: {policy}")


q2_rows = load_jsonl(q2_rows_path)
rule_by_candidate = {text(row.get("candidate_id")): row for row in load_jsonl(rule_rows_path) if text(row.get("candidate_id"))}
ata_rows = load_jsonl(ata_rows_path) if ata_rows_path and ata_rows_path.exists() else []
hydk_ata = next((row for row in ata_rows if text(row.get("mint")) == hydk_mint), {})

overlay_rows = []
for q2 in q2_rows:
    if not is_policy_positive_q2(q2, q2_match_policy):
        continue
    cid = text(q2.get("candidate_id"))
    rule = rule_by_candidate.get(cid)
    baseline = first_blocker(rule)
    quote_overlay = first_blocker(rule, quote=True)
    synthetic_ata = first_blocker(rule, quote=True, account=True)
    synthetic_inventory = first_blocker(rule, quote=True, inventory=True, pm=True)
    combined = first_blocker(rule, quote=True, account=True, inventory=True, pm=True)
    mint_in = text(q2.get("mint_in") or (rule or {}).get("mint_in"))
    mint_out = text(q2.get("mint_out") or (rule or {}).get("mint_out"))
    is_hydk = hydk_mint in {mint_in, mint_out}
    overlay_rows.append({
        "candidate_id": cid,
        "pool": q2.get("pool") or (rule or {}).get("pool"),
        "venue": q2.get("venue") or (rule or {}).get("venue"),
        "mint_in": mint_in,
        "mint_out": mint_out,
        "is_hydk": is_hydk,
        "run66_rule_row_present": rule is not None,
        "exact_quote_match": boolish(q2.get("exact_quote_match")),
        "quote_identity_source": q2.get("quote_identity_source"),
        "quote_identity_class": (
            "exact" if text(q2.get("quote_identity_source")).startswith("exact_")
            else "unique_safe" if text(q2.get("quote_identity_source")).startswith("unique_safe_")
            else "other"
        ),
        "q2_match_policy": q2_match_policy,
        "q2_policy_positive": True,
        "expected_out_raw": safe_int(q2.get("expected_out_raw")),
        "min_out_raw": safe_int(q2.get("min_out_raw")),
        "matched_rollout_net_ev_after_fee_tip_raw": safe_int(q2.get("matched_rollout_net_ev_after_fee_tip_raw")),
        "matched_rollout_net_ev_after_fee_tip_positive": boolish(q2.get("matched_rollout_net_ev_after_fee_tip_positive")),
        "route_cost_model_net_ev_after_fee_tip_raw": safe_int(q2.get("net_ev_after_fee_tip_raw")),
        "route_cost_model_net_ev_after_fee_tip_positive": boolish(q2.get("net_ev_after_fee_tip_positive")),
        "entry_rule_present": boolish((rule or {}).get("entry_rule_present")),
        "exit_rule_present": boolish((rule or {}).get("exit_rule_present")),
        "route_usable": boolish((rule or {}).get("route_usable")),
        "inventory_ready": boolish((rule or {}).get("inventory_ready")),
        "account_ready": boolish((rule or {}).get("account_ready")),
        "template_ready": boolish((rule or {}).get("template_ready")),
        "quote_ready": boolish((rule or {}).get("quote_ready")),
        "numeric_quote_min_out": boolish((rule or {}).get("numeric_quote_min_out")),
        "adverse_excursion_ok": boolish((rule or {}).get("adverse_excursion_ok")),
        "positive_after_cost": boolish((rule or {}).get("positive_after_cost")),
        "pm_reserve_sim_ok": boolish((rule or {}).get("pm_reserve_sim_ok")),
        "reserve_displacement_raw": (rule or {}).get("reserve_displacement_raw"),
        "reserve_displacement_bps": (rule or {}).get("reserve_displacement_bps") or (rule or {}).get("observed_displacement_bps"),
        "reserve_displacement_source": (rule or {}).get("reserve_displacement_source"),
        "pre_impulse_state": (rule or {}).get("pre_impulse_state"),
        "post_impulse_state": (rule or {}).get("post_impulse_state"),
        "restoration_pressure_score": (rule or {}).get("restoration_pressure_score"),
        "restoration_pressure_measured": boolish((rule or {}).get("restoration_pressure_measured")),
        "restoration_pressure_source": (rule or {}).get("restoration_pressure_source"),
        "target_recovery_fraction": (rule or {}).get("target_recovery_fraction"),
        "recovery_fraction_target": (rule or {}).get("recovery_fraction_target") or (rule or {}).get("target_recovery_fraction"),
        "max_horizon_ms": (rule or {}).get("max_horizon_ms"),
        "expected_restoration_path": (rule or {}).get("expected_restoration_path"),
        "exit_side": (rule or {}).get("exit_side"),
        "continuation_pressure_score": (rule or {}).get("continuation_pressure_score"),
        "same_direction_followthrough_score": (rule or {}).get("same_direction_followthrough_score"),
        "absorption_score": (rule or {}).get("absorption_score"),
        "route_lag_conflict": boolish((rule or {}).get("route_lag_conflict")),
        "liquidity_migration_signal": boolish((rule or {}).get("liquidity_migration_signal")),
        "toxic_flow_signal": boolish((rule or {}).get("toxic_flow_signal")),
        "false_restoration_gate_input_ready": boolish((rule or {}).get("false_restoration_gate_input_ready")),
        "false_restoration_reason": (rule or {}).get("false_restoration_reason"),
        "pm_owner_lock_ready": boolish((rule or {}).get("pm_owner_lock_ready")),
        "pm_owner_lock_owner_mode": (rule or {}).get("pm_owner_lock_owner_mode"),
        "pm_owner_lock_candidate_id": (rule or {}).get("pm_owner_lock_candidate_id"),
        "pm_owner_lock_conflict_domain": (rule or {}).get("pm_owner_lock_conflict_domain"),
        "pm_owner_lock_status": (rule or {}).get("pm_owner_lock_status"),
        "pm_owner_lock_source": (rule or {}).get("pm_owner_lock_source"),
        "pm_owner_lock_notional_raw": (rule or {}).get("pm_owner_lock_notional_raw"),
        "pm_reserved_notional_raw": (rule or {}).get("pm_reserved_notional_raw"),
        "pm_reserved_fee_raw": (rule or {}).get("pm_reserved_fee_raw"),
        "pm_reserved_tip_raw": (rule or {}).get("pm_reserved_tip_raw"),
        "pm_release_on_no_send_ok": boolish((rule or {}).get("pm_release_on_no_send_ok")),
        "pm_pending_after": (rule or {}).get("pm_pending_after"),
        "baseline_blocker": baseline,
        "quote_overlay_blocker": quote_overlay,
        "synthetic_ata_overlay_blocker": synthetic_ata,
        "synthetic_inventory_overlay_blocker": synthetic_inventory,
        "combined_synthetic_readiness_blocker": combined,
        "baseline_would_fire": would_fire(baseline),
        "quote_overlay_would_fire": would_fire(quote_overlay),
        "synthetic_ata_overlay_would_fire": would_fire(synthetic_ata),
        "synthetic_inventory_overlay_would_fire": would_fire(synthetic_inventory),
        "combined_synthetic_readiness_would_fire": would_fire(combined),
        "wire_send": 0,
        "live_landed": 0,
    })

overlay_rows.sort(
    key=lambda row: (
        row["combined_synthetic_readiness_would_fire"],
        row["synthetic_ata_overlay_would_fire"],
        row["matched_rollout_net_ev_after_fee_tip_raw"],
    ),
    reverse=True,
)
hydk_rows = [row for row in overlay_rows if row["is_hydk"]]
write_jsonl(rows_jsonl, overlay_rows)
write_jsonl(hydk_rows_jsonl, hydk_rows)


def count(field, rows=overlay_rows):
    return sum(1 for row in rows if row.get(field))


def blocker_counts(field, rows=overlay_rows):
    return dict(Counter(row[field] for row in rows))


hydk_summary = {
    "mint": hydk_mint,
    "planner_decision": hydk_ata.get("decision"),
    "planner_ata_review_decision": hydk_ata.get("ata_review_decision"),
    "planner_precreate_candidate_quote_positive_rows": safe_int(hydk_ata.get("precreate_candidate_quote_positive_rows")),
    "planner_precreate_candidate_net_ev_after_fee_tip_positive_rows": safe_int(hydk_ata.get("precreate_candidate_net_ev_after_fee_tip_positive_rows")),
    "planner_precreate_candidate_expected_would_fire_delta_after_quote_overlay": safe_int(hydk_ata.get("precreate_candidate_expected_would_fire_delta_after_quote_overlay")),
    "synthetic_rows_seen": len(hydk_rows),
    "exact_quote_positive_rows": count("exact_quote_match", hydk_rows),
    "matched_rollout_positive_rows": count("matched_rollout_net_ev_after_fee_tip_positive", hydk_rows),
    "synthetic_ata_would_fire_delta": sum(1 for row in hydk_rows if not row["baseline_would_fire"] and row["synthetic_ata_overlay_would_fire"]),
    "combined_synthetic_readiness_would_fire_delta": sum(1 for row in hydk_rows if not row["baseline_would_fire"] and row["combined_synthetic_readiness_would_fire"]),
    "baseline_blocker_counts": blocker_counts("baseline_blocker", hydk_rows),
    "synthetic_ata_blocker_counts": blocker_counts("synthetic_ata_overlay_blocker", hydk_rows),
    "combined_synthetic_blocker_counts": blocker_counts("combined_synthetic_readiness_blocker", hydk_rows),
    "unit_exit_probe_value_usdc": hydk_ata.get("unit_exit_probe_value_usdc"),
    "full_band_exit_probe_value_usdc": hydk_ata.get("full_band_exit_probe_value_usdc"),
    "review_result": "SYNTHETIC_ATA_OVERLAY_FAIL_NOT_ATA_ONLY" if hydk_rows and count("synthetic_ata_overlay_would_fire", hydk_rows) == 0 else "SYNTHETIC_ATA_OVERLAY_REVIEW",
}

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "q2_match_policy": q2_match_policy,
    "source_run66q2_rows": str(q2_rows_path),
    "source_run66_rule_rows": str(rule_rows_path),
    "source_run66a_rows": str(ata_rows_path) if ata_rows_path else "",
    "exact_positive_q2_rows": sum(1 for row in q2_rows if is_exact_positive_q2(row)),
    "policy_positive_q2_rows": sum(1 for row in q2_rows if is_policy_positive_q2(row, q2_match_policy)),
    "exact_positive_rows_joined_to_run66": sum(1 for row in overlay_rows if row["run66_rule_row_present"]),
    "baseline_would_fire": count("baseline_would_fire"),
    "quote_overlay_would_fire": count("quote_overlay_would_fire"),
    "synthetic_ata_overlay_would_fire": count("synthetic_ata_overlay_would_fire"),
    "synthetic_inventory_overlay_would_fire": count("synthetic_inventory_overlay_would_fire"),
    "combined_synthetic_readiness_would_fire": count("combined_synthetic_readiness_would_fire"),
    "baseline_blocker_counts": blocker_counts("baseline_blocker"),
    "quote_overlay_blocker_counts": blocker_counts("quote_overlay_blocker"),
    "synthetic_ata_blocker_counts": blocker_counts("synthetic_ata_overlay_blocker"),
    "synthetic_inventory_blocker_counts": blocker_counts("synthetic_inventory_overlay_blocker"),
    "combined_synthetic_blocker_counts": blocker_counts("combined_synthetic_readiness_blocker"),
    "ata_only_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["synthetic_ata_overlay_would_fire"]),
    "inventory_only_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["synthetic_inventory_overlay_would_fire"]),
    "combined_readiness_convertible_exact_positive_rows": sum(1 for row in overlay_rows if not row["baseline_would_fire"] and row["combined_synthetic_readiness_would_fire"]),
    "hydk": hydk_summary,
    "wire_send": 0,
    "live_landed": 0,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"q2_match_policy={q2_match_policy}",
    f"source_run66q2_rows={q2_rows_path}",
    f"source_run66_rule_rows={rule_rows_path}",
    f"exact_positive_q2_rows={summary['exact_positive_q2_rows']}",
    f"policy_positive_q2_rows={summary['policy_positive_q2_rows']}",
    f"exact_positive_rows_joined_to_run66={summary['exact_positive_rows_joined_to_run66']}",
    f"baseline_would_fire={summary['baseline_would_fire']}",
    f"quote_overlay_would_fire={summary['quote_overlay_would_fire']}",
    f"synthetic_ata_overlay_would_fire={summary['synthetic_ata_overlay_would_fire']}",
    f"synthetic_inventory_overlay_would_fire={summary['synthetic_inventory_overlay_would_fire']}",
    f"combined_synthetic_readiness_would_fire={summary['combined_synthetic_readiness_would_fire']}",
    f"ata_only_convertible_exact_positive_rows={summary['ata_only_convertible_exact_positive_rows']}",
    f"inventory_only_convertible_exact_positive_rows={summary['inventory_only_convertible_exact_positive_rows']}",
    f"combined_readiness_convertible_exact_positive_rows={summary['combined_readiness_convertible_exact_positive_rows']}",
    f"baseline_blocker_counts={json.dumps(summary['baseline_blocker_counts'], sort_keys=True)}",
    f"synthetic_ata_blocker_counts={json.dumps(summary['synthetic_ata_blocker_counts'], sort_keys=True)}",
    f"combined_synthetic_blocker_counts={json.dumps(summary['combined_synthetic_blocker_counts'], sort_keys=True)}",
    f"hydk_synthetic_rows_seen={hydk_summary['synthetic_rows_seen']}",
    f"hydk_exact_quote_positive_rows={hydk_summary['exact_quote_positive_rows']}",
    f"hydk_matched_rollout_positive_rows={hydk_summary['matched_rollout_positive_rows']}",
    f"hydk_synthetic_ata_would_fire_delta={hydk_summary['synthetic_ata_would_fire_delta']}",
    f"hydk_combined_synthetic_readiness_would_fire_delta={hydk_summary['combined_synthetic_readiness_would_fire_delta']}",
    f"hydk_baseline_blocker_counts={json.dumps(hydk_summary['baseline_blocker_counts'], sort_keys=True)}",
    f"hydk_synthetic_ata_blocker_counts={json.dumps(hydk_summary['synthetic_ata_blocker_counts'], sort_keys=True)}",
    f"hydk_review_result={hydk_summary['review_result']}",
    "wire_send=0",
    "live_landed=0",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66R exact-positive readiness overlay written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

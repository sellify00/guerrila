#!/usr/bin/env bash
# 66_rebalancing_rule_proof.sh - convert Run65 rebalancing markout reservoir into no-send rule proof rows

source "$(dirname "$0")/lib.sh"
script_header "66 - Rebalancing Rule Proof"

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
STEP="${SHADOW_RUN66R_STEP:-66_rebalancing_rule_proof}"
STATUS_PREFIX="${SHADOW_RUN66R_STATUS_PREFIX:-run66_rebalancing_rule_proof}"
REPORT_BASENAME="${SHADOW_RUN66R_REPORT_BASENAME:-run66_rebalancing_rule_proof}"

RUN65_STEP="${SHADOW_RUN66R_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
RUN66Q_STEP="${SHADOW_RUN66R_RUN66Q_STEP:-66q_rebalancing_quote_minout_preservation_20260524_5m}"
RUN66Q_ROWS="${SHADOW_RUN66R_QUOTE_ROWS:-$RESULTS_ROOT/$RUN66Q_STEP/run66q_rebalancing_quote_minout_rows.jsonl}"

MAX_ADVERSE_BPS="${SHADOW_RUN66R_MAX_ADVERSE_BPS:-50}"
MIN_DISPLACEMENT_BPS="${SHADOW_RUN66R_MIN_DISPLACEMENT_BPS:-25}"
MIN_NET_EV_SOL="${SHADOW_RUN66R_MIN_NET_EV_SOL:-0.0}"
TPU_COST_LAMPORTS="${SHADOW_RUN66R_TPU_COST_LAMPORTS:-5000}"
PRIVATE_ROUTE_COST_LAMPORTS="${SHADOW_RUN66R_PRIVATE_ROUTE_COST_LAMPORTS:-200000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
RULE_ROWS="$RESULT_DIR/run66_rebalancing_rule_rows.jsonl"
RULE_SUMMARY="$RESULT_DIR/run66_rebalancing_rule_summary.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$RULE_ROWS" "$RULE_SUMMARY"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - \
    "$RUN65_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$RULE_ROWS" \
    "$RULE_SUMMARY" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$MAX_ADVERSE_BPS" \
    "$MIN_DISPLACEMENT_BPS" \
    "$MIN_NET_EV_SOL" \
    "$TPU_COST_LAMPORTS" \
    "$PRIVATE_ROUTE_COST_LAMPORTS" \
    "$RUN66Q_ROWS" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
PRIVATE_ROUTES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle", "helius_sender", "helius_sender_dual"}
PUBLIC_ROUTES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rule_rows_path = Path(sys.argv[4])
rule_summary_path = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
max_adverse_bps = float(sys.argv[8] or "50")
min_displacement_bps = float(sys.argv[9] or "25")
min_net_ev_sol = float(sys.argv[10] or "0.0")
tpu_cost_lamports = int(sys.argv[11] or "5000")
private_route_cost_lamports = int(sys.argv[12] or "200000")
run66q_rows_path = Path(sys.argv[13]) if len(sys.argv) > 13 and sys.argv[13] else None


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return (value or "").strip() if isinstance(value, str) else str(value or "").strip()


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


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def raw_to_sol(raw, mint):
    raw = safe_int(raw)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000 / 1000.0
    return None


def route_family(row):
    explicit = text(row.get("target_route_family")).lower()
    if explicit:
        return "public_rpc_scout" if explicit in {"public", "public_rpc"} else explicit
    route = text(row.get("route")).lower()
    if "jito" in route:
        return "jito_bundle"
    if "harmonic" in route:
        return "harmonic_bundle"
    return route or "unknown"


def route_cost_lamports(route):
    return private_route_cost_lamports if route in PRIVATE_ROUTES else tpu_cost_lamports


def horizon_snapshot(markouts, horizon_name):
    if not isinstance(markouts, dict) or not horizon_name:
        return {}
    for group_name in ("time_horizons", "slot_horizons", "event_horizons"):
        group = markouts.get(group_name)
        if isinstance(group, dict):
            snapshot = group.get(horizon_name)
            if isinstance(snapshot, dict):
                return snapshot
    return {}


def horizon_to_ms(markouts, horizon_name, current_unix_ns):
    if not horizon_name:
        return None
    if horizon_name.endswith("ms"):
        try:
            return int(float(horizon_name[:-2]))
        except Exception:
            return None
    snapshot = horizon_snapshot(markouts, horizon_name)
    best_unix_ns = safe_int(snapshot.get("best_unix_ns"))
    if best_unix_ns > 0 and current_unix_ns > 0 and best_unix_ns >= current_unix_ns:
        return int((best_unix_ns - current_unix_ns) / 1_000_000)
    if horizon_name.endswith("_slot"):
        slots = safe_int(horizon_name.split("_", 1)[0])
        return slots * 400 if slots > 0 else None
    if horizon_name.startswith("next_same_pool_event_"):
        event_count = safe_int(horizon_name.rsplit("_", 1)[-1])
        return event_count * 400 if event_count > 0 else None
    return None


def bounded_score(value, low=0.0, high=1.0):
    try:
        value = float(value)
    except Exception:
        value = 0.0
    return max(low, min(high, value))


def restoration_pressure(best_markout_bps, min_displacement_bps, future_events):
    if future_events <= 0 or best_markout_bps <= 0:
        return 0.0
    denominator = max(float(min_displacement_bps) * 4.0, 1.0)
    return bounded_score(best_markout_bps / denominator)


def recovery_fraction(best_markout_bps, min_displacement_bps):
    if best_markout_bps <= 0:
        return None
    denominator = max(float(min_displacement_bps) * 2.0, 1.0)
    return bounded_score(best_markout_bps / denominator, 0.01, 1.0)


def pm_conflict_domain(row, route):
    return ":".join([
        text(row.get("pool")),
        text(row.get("venue")),
        text(row.get("mint_in")),
        text(row.get("mint_out")),
        text(route),
    ])


def sample_ratio(numerator, denominator):
    numerator = safe_int(numerator)
    denominator = safe_int(denominator)
    if denominator <= 0:
        return 0.0
    return bounded_score(numerator / denominator)


def first_blocker(row):
    if not row["entry_rule_present"]:
        return "entry_rule_missing"
    if not row["exit_rule_present"]:
        return "exit_rule_missing"
    if not row["route_usable"]:
        return "route_unusable"
    if not row["inventory_ready"]:
        return "inventory_not_ready"
    if not row["account_ready"]:
        return "account_ata_missing"
    if not row["template_ready"]:
        return "template_missing"
    if not row["quote_ready"]:
        return "quote_missing"
    if not row["numeric_quote_min_out"]:
        return "numeric_quote_min_out_missing"
    if not row["adverse_excursion_ok"]:
        return "adverse_excursion_exceeded"
    if not row["positive_after_cost"]:
        return "ev_not_positive_after_cost"
    if not row["pm_reserve_sim_ok"]:
        return "pm_reserve_sim_failed"
    return "none"


run65 = load_json(run65_json)
inputs = run65.get("inputs") or {}
candidate_path = Path(inputs.get("run39_candidate_autopsy") or "")
if not candidate_path.exists():
    raise SystemExit(f"candidate_autopsy missing: {candidate_path}")

rows = load_jsonl(candidate_path)
quote_overlay_by_candidate = {}
if run66q_rows_path and run66q_rows_path.exists():
    for quote_row in load_jsonl(run66q_rows_path):
        cid = text(quote_row.get("candidate_id"))
        if cid and quote_row.get("quote_numeric_preserved") and quote_row.get("quote_match_exact_or_unique_safe"):
            quote_overlay_by_candidate[cid] = quote_row

rebalancing_rows = [row for row in rows if text(row.get("selected_owner")).lower() == "rebalancing"]
seen = set()
deduped = []
for row in rebalancing_rows:
    cid = text(row.get("candidate_id"))
    if cid and cid in seen:
        continue
    seen.add(cid)
    deduped.append(row)

rule_rows = []
for row in deduped:
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    route = route_family(row)
    best_horizon = text(markouts.get("best_horizon")) or "none"
    best_net_raw = safe_int(markouts.get("best_horizon_net_after_fee_lamports"))
    immediate_raw = safe_int(row.get("net_after_fee_lamports"))
    retained_raw = safe_int(row.get("retained_ev_lamports"))
    best_markout_raw = safe_int(markouts.get("best_horizon_markout_lamports"))
    best_markout_bps = safe_float(markouts.get("best_horizon_markout_bps"))
    max_adverse_bps_value = safe_float(markouts.get("max_adverse_excursion_bps"))
    max_adverse_raw = safe_int(markouts.get("max_adverse_excursion_lamports"))
    future_events = safe_int(markouts.get("future_same_pool_event_count"))
    event_horizons = markouts.get("event_horizons") if isinstance(markouts.get("event_horizons"), dict) else {}
    best_snapshot = horizon_snapshot(markouts, best_horizon)
    sample_counts = [
        safe_int((value or {}).get("sample_count"))
        for value in event_horizons.values()
        if isinstance(value, dict)
    ]
    same_direction_counts = [
        safe_int((value or {}).get("same_direction_sample_count"))
        for value in event_horizons.values()
        if isinstance(value, dict)
    ]
    route_cost = route_cost_lamports(route)
    best_net_after_route_cost = best_net_raw - route_cost
    best_net_sol = raw_to_sol(best_net_after_route_cost, mint_in)
    entry_rule_present = best_horizon != "none" and best_markout_bps >= min_displacement_bps and future_events > 0
    exit_rule_present = best_horizon.startswith("next_same_pool_event") or best_horizon.endswith("_slot")
    quote_raw = safe_int(row.get("quote_expected_out_lamports")) or safe_int(row.get("expected_out_raw"))
    min_out_raw = safe_int(row.get("min_out_lamports")) or safe_int(row.get("min_out_raw"))
    account_ready = boolish(row.get("output_account_ready")) or boolish(row.get("has_output_ata")) or boolish(row.get("output_ata_initialized"))
    inventory_ready = boolish(row.get("approved_held_input")) and (boolish(row.get("input_balance_ok")) or boolish(row.get("input_balance_sufficient")))
    template_ready = boolish(row.get("template_ready")) or boolish(row.get("would_execute_current_template"))
    quote_ready = boolish(row.get("quote_ready_or_classified")) or quote_raw > 0
    numeric_quote_min_out = quote_raw > 0 and min_out_raw > 0
    route_usable = route in PUBLIC_ROUTES or route in PRIVATE_ROUTES
    adverse_loss_bps = abs(max_adverse_bps_value) if max_adverse_bps_value < 0 else 0.0
    adverse_excursion_ok = adverse_loss_bps <= max_adverse_bps
    positive_after_cost = best_net_sol is not None and best_net_sol > min_net_ev_sol
    pm_reserve_sim_ok = inventory_ready and safe_int(row.get("trade_size_lamports")) > 0
    pm_owner_lock_ready = pm_reserve_sim_ok and text(row.get("selected_owner")).lower() == "rebalancing"
    restoration_score = restoration_pressure(best_markout_bps, min_displacement_bps, future_events)
    target_recovery_fraction = recovery_fraction(best_markout_bps, min_displacement_bps)
    restoration_measured = restoration_score > 0 and target_recovery_fraction is not None
    max_event_sample_count = max(sample_counts) if sample_counts else 0
    max_same_direction_sample_count = max(same_direction_counts) if same_direction_counts else 0
    same_direction_followthrough_score = (
        1.0 if boolish(markouts.get("follow_through")) else sample_ratio(max_same_direction_sample_count, max_event_sample_count)
    )
    absorption_score = 1.0 if boolish(markouts.get("absorption")) else 0.0
    continuation_pressure_score = max(
        same_direction_followthrough_score,
        1.0 if text(markouts.get("continuation_label")) == "follow_through" else 0.0,
    )
    false_restoration_input_ready = restoration_measured
    quote_overlay = quote_overlay_by_candidate.get(text(row.get("candidate_id"))) or {}
    overlay_expected_out = safe_int(quote_overlay.get("expected_out_raw"))
    overlay_min_out = safe_int(quote_overlay.get("min_out_raw"))
    overlay_target_out = safe_int(quote_overlay.get("target_out_raw"))
    if overlay_expected_out > 0:
        quote_raw = overlay_expected_out
    if overlay_min_out > 0:
        min_out_raw = overlay_min_out
    quote_ready = quote_ready or overlay_expected_out > 0 or overlay_target_out > 0 or overlay_min_out > 0
    numeric_quote_min_out = quote_raw > 0 and min_out_raw > 0
    probe = {
        "candidate_id": row.get("candidate_id"),
        "selected_owner": text(row.get("selected_owner")).lower(),
        "selected_owner_score": safe_float(row.get("selected_owner_score")),
        "owner_mode_score_rebalancing": safe_float(row.get("owner_mode_score_rebalancing") or row.get("rebalancing_score")),
        "owner_mode_score_rebalancing_source": text(row.get("owner_mode_score_rebalancing_source")) or "missing",
        "owner_mode_score_continuation": safe_float(row.get("owner_mode_score_continuation") or row.get("continuation_score")),
        "owner_mode_score_backrun": safe_float(row.get("owner_mode_score_backrun") or row.get("backrun_score")),
        "owner_mode_score_drop": safe_float(row.get("owner_mode_score_drop") or row.get("drop_score")),
        "owner_mode_score_flatten": safe_float(row.get("owner_mode_score_flatten") or row.get("flatten_score")),
        "owner_mode_score_housekeeping": safe_float(row.get("owner_mode_score_housekeeping") or row.get("housekeeping_score")),
        "slot": safe_int(row.get("slot")),
        "source": row.get("source"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": mint_in,
        "mint_out": mint_out,
        "route_family": route,
        "entry_side": f"{mint_in}->{mint_out}" if mint_in and mint_out else None,
        "entry_rule_present": entry_rule_present,
        "entry_rule_count": 1 if entry_rule_present else 0,
        "displacement_threshold_bps": min_displacement_bps,
        "observed_displacement_bps": round_or_none(best_markout_bps, 6),
        "restoration_target": best_horizon,
        "exit_rule_present": exit_rule_present,
        "exit_rule_count": 1 if exit_rule_present else 0,
        "max_hold_horizon": best_horizon,
        "max_horizon_ms": horizon_to_ms(markouts, best_horizon, safe_int(row.get("unix_ns"))),
        "stop_loss_bps": max_adverse_bps,
        "max_adverse_excursion_bps": round_or_none(max_adverse_bps_value, 6),
        "max_adverse_loss_bps": round_or_none(adverse_loss_bps, 6),
        "max_adverse_excursion_lamports": max_adverse_raw,
        "adverse_excursion_ok": adverse_excursion_ok,
        "best_horizon": best_horizon,
        "future_same_pool_event_count": future_events,
        "max_event_sample_count": max_event_sample_count,
        "max_same_direction_sample_count": max_same_direction_sample_count,
        "follow_through": boolish(markouts.get("follow_through")),
        "reversal": boolish(markouts.get("reversal")),
        "absorption": boolish(markouts.get("absorption")),
        "reserve_displacement_raw": best_markout_raw if best_markout_raw > 0 else None,
        "reserve_displacement_bps": round_or_none(best_markout_bps, 6),
        "reserve_displacement_source": "same_pool_markout_model",
        "pre_impulse_state": json.dumps({
            "slot": safe_int(row.get("slot")),
            "unix_ns": safe_int(row.get("unix_ns")),
            "net_after_fee_lamports": immediate_raw,
            "retained_ev_lamports": retained_raw,
        }, sort_keys=True),
        "post_impulse_state": json.dumps({
            "horizon": best_horizon,
            "slot": safe_int(best_snapshot.get("best_slot")),
            "unix_ns": safe_int(best_snapshot.get("best_unix_ns")),
            "net_after_fee_lamports": best_net_raw,
            "markout_lamports": best_markout_raw,
        }, sort_keys=True),
        "restoration_pressure_score": round_or_none(restoration_score, 6),
        "restoration_pressure_measured": restoration_measured,
        "restoration_pressure_source": "same_pool_markout_model",
        "target_recovery_fraction": round_or_none(target_recovery_fraction, 6),
        "recovery_fraction_target": round_or_none(target_recovery_fraction, 6),
        "expected_restoration_path": f"{mint_out}->{mint_in}@{best_horizon}" if mint_in and mint_out and best_horizon else None,
        "exit_side": f"{mint_out}->{mint_in}" if mint_in and mint_out else None,
        "continuation_pressure_score": round_or_none(continuation_pressure_score, 6),
        "same_direction_followthrough_score": round_or_none(same_direction_followthrough_score, 6),
        "absorption_score": round_or_none(absorption_score, 6),
        "route_lag_conflict": False,
        "liquidity_migration_signal": False,
        "toxic_flow_signal": False,
        "false_restoration_gate_input_ready": false_restoration_input_ready,
        "false_restoration_reason": "inputs_ready" if false_restoration_input_ready else "restoration_pressure_not_measured",
        "pm_owner_lock_ready": pm_owner_lock_ready,
        "pm_owner_lock_owner_mode": "rebalancing" if pm_owner_lock_ready else None,
        "pm_owner_lock_candidate_id": row.get("candidate_id") if pm_owner_lock_ready else None,
        "pm_owner_lock_conflict_domain": pm_conflict_domain(row, route) if pm_owner_lock_ready else None,
        "pm_owner_lock_status": "structural_no_send_lock" if pm_owner_lock_ready else "not_ready",
        "pm_owner_lock_source": "rule_proof_pm_reserve_sim",
        "pm_owner_lock_notional_raw": safe_int(row.get("trade_size_lamports")) if pm_owner_lock_ready else 0,
        "pm_reserved_notional_raw": safe_int(row.get("trade_size_lamports")) if pm_owner_lock_ready else 0,
        "pm_reserved_fee_raw": route_cost,
        "pm_reserved_tip_raw": route_cost if route in PRIVATE_ROUTES else 0,
        "pm_release_on_no_send_ok": True,
        "pm_pending_after": 0,
        "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "immediate_net_raw": immediate_raw,
        "later_horizon_net_raw": best_net_raw,
        "retained_ev_raw": retained_raw,
        "estimated_route_cost_lamports": route_cost,
        "route_specific_ev_after_cost_raw": best_net_after_route_cost,
        "route_specific_ev_after_cost_sol": round_or_none(best_net_sol, 12),
        "route_usable": route_usable,
        "inventory_ready": inventory_ready,
        "account_ready": account_ready,
        "template_ready": template_ready,
        "quote_ready": quote_ready,
        "quote_expected_out_raw": quote_raw,
        "quote_target_out_raw": overlay_target_out,
        "min_out_raw": min_out_raw,
        "numeric_quote_min_out": numeric_quote_min_out,
        "quote_overlay_applied": bool(quote_overlay),
        "quote_match_mode": quote_overlay.get("quote_match_mode"),
        "quote_first_blocker": quote_overlay.get("first_quote_blocker"),
        "positive_after_cost": positive_after_cost,
        "pm_reserve_sim_ok": pm_reserve_sim_ok,
    }
    blocker = first_blocker(probe)
    probe["rule_blocker"] = blocker
    probe["rebalancing_would_fire"] = blocker == "none"
    rule_rows.append(probe)

rule_rows.sort(key=lambda row: (row["rebalancing_would_fire"], row["route_specific_ev_after_cost_sol"] or 0.0, row["future_same_pool_event_count"]), reverse=True)
write_jsonl(rule_rows_path, rule_rows)

pool_groups = defaultdict(lambda: {"rows": 0, "would_fire": 0, "ev": 0.0, "blockers": Counter(), "best_horizons": Counter()})
for row in rule_rows:
    key = (row["pool"], row["mint_in"], row["mint_out"], row["route_family"])
    group = pool_groups[key]
    group["rows"] += 1
    group["would_fire"] += 1 if row["rebalancing_would_fire"] else 0
    group["ev"] += row["route_specific_ev_after_cost_sol"] or 0.0
    group["blockers"][row["rule_blocker"]] += 1
    group["best_horizons"][row["best_horizon"]] += 1

top_rule_classes = []
for (pool, mint_in, mint_out, route), group in pool_groups.items():
    top_rule_classes.append({
        "pool": pool,
        "mint_in": mint_in,
        "mint_out": mint_out,
        "route_family": route,
        "rows": group["rows"],
        "would_fire": group["would_fire"],
        "route_specific_ev_after_cost_sol": round_or_none(group["ev"], 9),
        "blocker_counts": dict(group["blockers"]),
        "best_horizon_counts": dict(group["best_horizons"]),
    })
top_rule_classes.sort(key=lambda item: (item["would_fire"], item["route_specific_ev_after_cost_sol"], item["rows"]), reverse=True)

summary = {
    "status": "run66_rebalancing_rule_proof_complete",
    "source_run65_json": str(run65_json),
    "source_run66q_rows": str(run66q_rows_path) if run66q_rows_path else None,
    "candidate_autopsy_path": str(candidate_path),
    "rebalancing_quote_overlay_rows_loaded": len(quote_overlay_by_candidate),
    "rebalancing_quote_overlay_applied": sum(1 for row in rule_rows if row["quote_overlay_applied"]),
    "rebalancing_rows_seen": len(deduped),
    "rebalancing_rule_candidates": sum(1 for row in rule_rows if row["entry_rule_present"] and row["exit_rule_present"]),
    "rebalancing_entry_rule_count": sum(1 for row in rule_rows if row["entry_rule_present"]),
    "rebalancing_exit_rule_count": sum(1 for row in rule_rows if row["exit_rule_present"]),
    "rebalancing_pm_reserve_sim_ok": sum(1 for row in rule_rows if row["pm_reserve_sim_ok"]),
    "rebalancing_account_ready": sum(1 for row in rule_rows if row["account_ready"]),
    "rebalancing_inventory_ready": sum(1 for row in rule_rows if row["inventory_ready"]),
    "rebalancing_quote_ready": sum(1 for row in rule_rows if row["quote_ready"]),
    "rebalancing_numeric_quote_min_out": sum(1 for row in rule_rows if row["numeric_quote_min_out"]),
    "rebalancing_positive_after_cost": sum(1 for row in rule_rows if row["positive_after_cost"]),
    "rebalancing_would_fire": sum(1 for row in rule_rows if row["rebalancing_would_fire"]),
    "rebalancing_max_adverse_excursion_bps_min": round_or_none(min((row["max_adverse_excursion_bps"] for row in rule_rows if row["max_adverse_excursion_bps"] is not None), default=0.0), 6),
    "rebalancing_max_adverse_loss_bps_max": round_or_none(max((row["max_adverse_loss_bps"] for row in rule_rows if row["max_adverse_loss_bps"] is not None), default=0.0), 6),
    "rebalancing_best_horizon_counts": dict(Counter(row["best_horizon"] for row in rule_rows)),
    "rebalancing_rule_blocker_counts": dict(Counter(row["rule_blocker"] for row in rule_rows)),
    "top_rule_classes": top_rule_classes[:20],
    "top_rows": rule_rows[:20],
}
payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "summary": summary,
    "policy": {
        "max_adverse_bps": max_adverse_bps,
        "min_displacement_bps": min_displacement_bps,
        "min_net_ev_sol": min_net_ev_sol,
        "tpu_cost_lamports": tpu_cost_lamports,
        "private_route_cost_lamports": private_route_cost_lamports,
    },
}

rule_summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={payload['status']}",
    f"source_run65_json={run65_json}",
    f"source_run66q_rows={run66q_rows_path}",
    f"candidate_autopsy_path={candidate_path}",
    f"rebalancing_quote_overlay_rows_loaded={summary['rebalancing_quote_overlay_rows_loaded']}",
    f"rebalancing_quote_overlay_applied={summary['rebalancing_quote_overlay_applied']}",
    f"rebalancing_rows_seen={summary['rebalancing_rows_seen']}",
    f"rebalancing_rule_candidates={summary['rebalancing_rule_candidates']}",
    f"rebalancing_entry_rule_count={summary['rebalancing_entry_rule_count']}",
    f"rebalancing_exit_rule_count={summary['rebalancing_exit_rule_count']}",
    f"rebalancing_pm_reserve_sim_ok={summary['rebalancing_pm_reserve_sim_ok']}",
    f"rebalancing_account_ready={summary['rebalancing_account_ready']}",
    f"rebalancing_inventory_ready={summary['rebalancing_inventory_ready']}",
    f"rebalancing_quote_ready={summary['rebalancing_quote_ready']}",
    f"rebalancing_numeric_quote_min_out={summary['rebalancing_numeric_quote_min_out']}",
    f"rebalancing_positive_after_cost={summary['rebalancing_positive_after_cost']}",
    f"rebalancing_would_fire={summary['rebalancing_would_fire']}",
    f"rebalancing_rule_blocker_counts={json.dumps(summary['rebalancing_rule_blocker_counts'], sort_keys=True)}",
    f"rebalancing_best_horizon_counts={json.dumps(summary['rebalancing_best_horizon_counts'], sort_keys=True)}",
]
if top_rule_classes:
    top = top_rule_classes[0]
    lines.extend([
        f"top_rule_pool={top['pool']}",
        f"top_rule_mint_in={top['mint_in']}",
        f"top_rule_mint_out={top['mint_out']}",
        f"top_rule_rows={top['rows']}",
        f"top_rule_would_fire={top['would_fire']}",
        f"top_rule_ev_after_cost_sol={top['route_specific_ev_after_cost_sol']}",
        f"top_rule_blocker_counts={json.dumps(top['blocker_counts'], sort_keys=True)}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66 rebalancing rule proof written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

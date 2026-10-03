#!/usr/bin/env bash
# 49_hunt_to_execution_conversion.sh - convert Run48 HUNT rows into execution-oriented no-send plans

source "$(dirname "$0")/lib.sh"
script_header "49 - HUNT-to-Execution Conversion"

assert_cmd_exists python3

RESULT_DIR="${1:-}"
if [[ -z "$RESULT_DIR" ]]; then
    fail "Usage: $0 <run47-run48-result-dir>"
    finish
    exit 1
fi

if [[ ! -d "$RESULT_DIR" ]]; then
    fail "Result directory does not exist: $RESULT_DIR"
    finish
    exit 1
fi

CANDIDATE_AUTOPSY_JSONL="$RESULT_DIR/candidate_autopsy.jsonl"
OWNER_MODE_HUNT_WATCH_DENY_JSON="$RESULT_DIR/owner_mode_hunt_watch_deny.json"
FILTER_REPAIR_PLAN_JSON="$RESULT_DIR/filter_repair_plan.json"
ROUTE_CONVERSION_BLOCKERS_JSON="$RESULT_DIR/route_conversion_blockers.json"
INVENTORY_BAND_CANDIDATES_JSON="$RESULT_DIR/inventory_band_candidates.json"
RUN49_HUNT_CONVERSION_PLAN_JSON="$RESULT_DIR/run49_hunt_conversion_plan.json"
RUN49_TOP_HUNT_CANDIDATES_JSON="$RESULT_DIR/run49_top_hunt_candidates.json"
RUN49_ROUTE_TEMPLATE_BLOCKERS_JSON="$RESULT_DIR/run49_route_template_blockers.json"
RUN49_INVENTORY_PREFLIGHT_PLAN_JSON="$RESULT_DIR/run49_inventory_preflight_plan.json"
RUN49_CONTINUATION_HUNT_REPORT_JSON="$RESULT_DIR/run49_continuation_hunt_report.json"

missing=0
for required_path in \
    "$CANDIDATE_AUTOPSY_JSONL" \
    "$OWNER_MODE_HUNT_WATCH_DENY_JSON" \
    "$FILTER_REPAIR_PLAN_JSON" \
    "$ROUTE_CONVERSION_BLOCKERS_JSON" \
    "$INVENTORY_BAND_CANDIDATES_JSON"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required artifact is missing: $required_path"
        missing=1
    fi
done

if [[ "$missing" == "1" ]]; then
    finish
    exit 1
fi

python3 - \
    "$RESULT_DIR" \
    "$CANDIDATE_AUTOPSY_JSONL" \
    "$OWNER_MODE_HUNT_WATCH_DENY_JSON" \
    "$FILTER_REPAIR_PLAN_JSON" \
    "$ROUTE_CONVERSION_BLOCKERS_JSON" \
    "$INVENTORY_BAND_CANDIDATES_JSON" \
    "$RUN49_HUNT_CONVERSION_PLAN_JSON" \
    "$RUN49_TOP_HUNT_CANDIDATES_JSON" \
    "$RUN49_ROUTE_TEMPLATE_BLOCKERS_JSON" \
    "$RUN49_INVENTORY_PREFLIGHT_PLAN_JSON" \
    "$RUN49_CONTINUATION_HUNT_REPORT_JSON" <<'PY'
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

RESULT_DIR = Path(sys.argv[1])
CANDIDATE_AUTOPSY_PATH = Path(sys.argv[2])
OWNER_MODE_HUNT_WATCH_DENY_PATH = Path(sys.argv[3])
FILTER_REPAIR_PLAN_PATH = Path(sys.argv[4])
ROUTE_CONVERSION_BLOCKERS_PATH = Path(sys.argv[5])
INVENTORY_BAND_CANDIDATES_PATH = Path(sys.argv[6])
RUN49_HUNT_CONVERSION_PLAN_PATH = Path(sys.argv[7])
RUN49_TOP_HUNT_CANDIDATES_PATH = Path(sys.argv[8])
RUN49_ROUTE_TEMPLATE_BLOCKERS_PATH = Path(sys.argv[9])
RUN49_INVENTORY_PREFLIGHT_PLAN_PATH = Path(sys.argv[10])
RUN49_CONTINUATION_HUNT_REPORT_PATH = Path(sys.argv[11])

SYSTEM_PROGRAM_ID = "11111111111111111111111111111111"
PROMOTABLE_OWNER_MODES = {"continuation", "rebalancing", "housekeeping", "backrun", "flatten"}
JITO_ROUTES = {"jito_bundle", "harmonic_bundle"}
PUBLIC_ROUTES = {"public_rpc_scout", "public_rpc"}
SUPPORTED_OBSERVED_ROUTES = JITO_ROUTES | PUBLIC_ROUTES


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


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


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = (len(ordered) - 1) * fraction
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return ordered[lower]
    weight = index - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def round_or_none(value, digits=4):
    return None if value is None else round(value, digits)


def row_owner(row):
    return (row.get("selected_owner") or "drop").strip() or "drop"


def row_markouts(row):
    return row.get("markouts") or {}


def row_best_markout_lamports(row):
    return safe_int(row_markouts(row).get("best_horizon_markout_lamports"))


def row_best_net_after_fee_lamports(row):
    return safe_int(row_markouts(row).get("best_horizon_net_after_fee_lamports"))


def row_owner_score(row):
    return safe_float(row.get("selected_owner_score"))


def row_is_research_positive(row):
    return bool(row.get("research_positive"))


def classify_hunt_watch_deny(row):
    owner = row_owner(row)
    best_markout = row_best_markout_lamports(row)
    best_net = row_best_net_after_fee_lamports(row)
    score = row_owner_score(row)
    repairable_bucket = (row.get("first_rejection_stage") or "") in {
        "source_not_afxdp",
        "route_not_tpu_ready",
        "not_held_input",
        "not_positive_ev",
    } or (row.get("execution_blocker") or "") in {
        "missing_output_ata",
        "output_ata_uninitialized",
        "spl_input_balance_insufficient",
    }
    if (
        owner in PROMOTABLE_OWNER_MODES
        and row_is_research_positive(row)
        and best_markout > 0
        and best_net > 0
        and (score >= 0.45 or best_markout >= 20_000)
    ):
        return "hunt"
    if row_is_research_positive(row) and (
        best_markout > 0 or owner in PROMOTABLE_OWNER_MODES or repairable_bucket
    ):
        return "watch"
    return "deny"


def top_counter(counter_obj, limit=10, label="value"):
    return [{label: key, "count": count} for key, count in counter_obj.most_common(limit)]


def horizon_dict(markouts, bucket_name, horizon_name):
    horizon = ((markouts.get(bucket_name) or {}).get(horizon_name)) if markouts else None
    return horizon if isinstance(horizon, dict) else {}


def safe_spl_output_mint(row):
    token_program = row.get("inventory_output_token_program")
    return bool(token_program and token_program.startswith("Token"))


def inventory_status(row):
    mint_in = row.get("mint_in") or ""
    if mint_in == SYSTEM_PROGRAM_ID:
        return "deny_bad_mint"
    if row.get("has_input_ata") and row.get("input_balance_sufficient"):
        return "ready"
    if row.get("approved_held_input") and not row.get("has_input_ata"):
        return "create_input_ata"
    if row.get("approved_held_input") and not row.get("input_balance_sufficient"):
        return "top_up_existing_band"
    if not row.get("approved_held_input"):
        return "new_inventory_band_required"
    return "inventory_unknown"


def output_ata_needed(row):
    blocker = row.get("execution_blocker") or ""
    return (not row.get("has_output_ata")) or (not row.get("output_account_ready")) or blocker in {
        "missing_output_ata",
        "output_ata_uninitialized",
    }


def route_profile(row):
    observed_route = row.get("route") or ""
    target_route = row.get("target_route_family") or observed_route
    jito_ready = observed_route in JITO_ROUTES or target_route in JITO_ROUTES
    public_ready = observed_route in PUBLIC_ROUTES or target_route in PUBLIC_ROUTES
    tpu_ready = bool(row.get("route_tpu_ready"))
    route_ready = bool(observed_route and observed_route != "drop")
    return {
        "observed_route": observed_route,
        "target_route_family": target_route or None,
        "route_ready": route_ready,
        "jito_ready": jito_ready,
        "public_ready": public_ready,
        "tpu_ready": tpu_ready,
    }


def source_softening_needed(row):
    return (row.get("source") or "") != "afxdp"


def current_path(row):
    route = route_profile(row)
    if route["jito_ready"]:
        return "jito_nosend_first"
    if route["tpu_ready"]:
        return "tpu_nosend_first"
    if route["public_ready"]:
        return "public_nosend_first"
    if route["route_ready"]:
        return "route_conversion_required"
    return "route_unresolved"


def next_action(row, route_lookup, inventory_lookup):
    mint_in = row.get("mint_in") or ""
    inv_status = inventory_status(row)
    route = route_profile(row)
    route_key = (
        row.get("pool") or "",
        route["observed_route"],
        row.get("mint_in") or "",
        row.get("mint_out") or "",
    )
    route_item = route_lookup.get(route_key)
    inventory_item = inventory_lookup.get(mint_in)
    if mint_in == SYSTEM_PROGRAM_ID:
        return "deny_bad_mint_with_evidence"
    if inv_status == "create_input_ata":
        return "create_input_ata_then_retry_jito_nosend"
    if inv_status == "top_up_existing_band":
        return "top_up_inventory_then_retry_jito_nosend"
    if inv_status == "new_inventory_band_required":
        return "decide_new_inventory_band_before_execution"
    if output_ata_needed(row):
        if safe_spl_output_mint(row):
            return "precreate_output_ata_then_retry"
        return "block_on_output_mint_proof_before_preflight"
    if not row.get("template_ready"):
        return "build_or_validate_template_first"
    if route["jito_ready"]:
        if source_softening_needed(row):
            return "soften_source_grade_then_jito_nosend"
        return "promote_to_jito_nosend_candidate"
    if route["tpu_ready"]:
        return "promote_to_tpu_nosend_candidate"
    if route["public_ready"]:
        return "promote_to_public_nosend_candidate"
    if route_item and route_item.get("blocker_class") == "route_family_policy_gap":
        return "resolve_route_policy_or_egress_lane"
    if inventory_item and inventory_item.get("action") == "consider_new_inventory_band":
        return "evaluate_inventory_band_then_reassess_route"
    return "watch_until_route_or_template_path_is_clear"


def repeat_count(row):
    markouts = row_markouts(row)
    future_events = safe_int(markouts.get("future_same_pool_event_count"))
    if future_events > 0:
        return future_events
    event_horizons = markouts.get("event_horizons") or {}
    counts = [
        safe_int((value or {}).get("sample_count"))
        for value in event_horizons.values()
        if isinstance(value, dict)
    ]
    return max(counts) if counts else 0


def continuation_event_stats(row):
    markouts = row_markouts(row)
    values = []
    maes = []
    for value in (markouts.get("event_horizons") or {}).values():
        if isinstance(value, dict):
            values.append(safe_int(value.get("markout_lamports")))
            maes.append(safe_int(value.get("max_adverse_excursion_lamports")))
    return {
        "median_event_markout_lamports": statistics.median(values) if values else None,
        "p90_event_markout_lamports": percentile(values, 0.90),
        "median_event_mae_lamports": statistics.median(maes) if maes else None,
        "p90_event_mae_lamports": percentile(maes, 0.90),
    }


def conversion_speed_score(row, route_lookup, inventory_lookup):
    score = 0.0
    inv_status = inventory_status(row)
    route = route_profile(row)
    mint_in = row.get("mint_in") or ""
    if inv_status == "ready":
        score += 28
    elif inv_status == "top_up_existing_band":
        score += 12
    elif inv_status == "create_input_ata":
        score += 8
    if not output_ata_needed(row):
        score += 15
    elif safe_spl_output_mint(row):
        score += 4
    if row.get("template_ready"):
        score += 15
    if route["jito_ready"]:
        score += 18
    elif route["tpu_ready"]:
        score += 14
    elif route["public_ready"]:
        score += 10
    elif route["route_ready"]:
        score += 4
    if (row.get("source") or "") == "afxdp":
        score += 10
    elif not source_softening_needed(row):
        score += 6
    else:
        score += 2
    if row.get("positive_ev"):
        score += 6
    if row.get("tiny_cap_safe"):
        score += 8
    score += min(repeat_count(row), 20)
    score += min(row_owner_score(row) * 10, 10)
    score += min(row_best_markout_lamports(row) / 250_000, 12)
    trade_size = safe_int(row.get("trade_size_lamports"))
    if trade_size > 0:
        if trade_size <= 50_000_000:
            score += 8
        elif trade_size <= 200_000_000:
            score += 5
        else:
            score += 2
    if mint_in == SYSTEM_PROGRAM_ID or (row.get("mint_out") or "") == SYSTEM_PROGRAM_ID:
        score -= 40
    if not row.get("template_ready"):
        score -= 12
    if inv_status == "new_inventory_band_required":
        score -= 12
    if output_ata_needed(row) and not safe_spl_output_mint(row):
        score -= 12
    if source_softening_needed(row):
        score -= 3
    route_item = route_lookup.get(
        (
            row.get("pool") or "",
            route["observed_route"],
            row.get("mint_in") or "",
            row.get("mint_out") or "",
        )
    )
    if route_item and route_item.get("blocker_class") == "route_family_policy_gap" and not route["jito_ready"]:
        score -= 8
    inventory_item = inventory_lookup.get(mint_in)
    if inventory_item and inventory_item.get("action") == "deny_bad_mint":
        score -= 25
    return round(score, 4)


def compact_candidate(row):
    route = route_profile(row)
    return {
        "candidate_id": row.get("candidate_id"),
        "owner_mode": row_owner(row),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "source": row.get("source"),
        "route": route["observed_route"],
        "best_horizon": row_markouts(row).get("best_horizon"),
        "best_horizon_markout_lamports": row_best_markout_lamports(row),
        "best_horizon_net_after_fee_lamports": row_best_net_after_fee_lamports(row),
        "selected_owner_score": round_or_none(row_owner_score(row), 4),
        "repeat_count": repeat_count(row),
        "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "next_action": row.get("next_action"),
        "conversion_speed_score": row.get("conversion_speed_score"),
    }


candidate_rows = load_jsonl(CANDIDATE_AUTOPSY_PATH)
owner_mode_hunt_watch_deny = load_json(OWNER_MODE_HUNT_WATCH_DENY_PATH)
filter_repair_plan = load_json(FILTER_REPAIR_PLAN_PATH)
route_conversion_blockers = load_json(ROUTE_CONVERSION_BLOCKERS_PATH)
inventory_band_candidates = load_json(INVENTORY_BAND_CANDIDATES_PATH)

route_lookup = {}
for item in route_conversion_blockers.get("items", []):
    key = (
        item.get("pool") or "",
        item.get("observed_route") or "",
        item.get("mint_in") or "",
        item.get("mint_out") or "",
    )
    route_lookup[key] = item

inventory_lookup = {
    item.get("mint_in") or "": item
    for item in inventory_band_candidates.get("items", [])
    if item.get("mint_in")
}

hunt_rows = []
for row in candidate_rows:
    if classify_hunt_watch_deny(row) != "hunt":
        continue
    route = route_profile(row)
    inv_status = inventory_status(row)
    enriched = dict(row)
    enriched["required_inventory"] = inv_status
    enriched["output_ata_needed"] = output_ata_needed(row)
    enriched["route_ready"] = route["route_ready"]
    enriched["jito_ready"] = route["jito_ready"]
    enriched["public_ready"] = route["public_ready"]
    enriched["tpu_ready"] = route["tpu_ready"]
    enriched["current_execution_path"] = current_path(row)
    enriched["safe_size_lamports"] = safe_int(row.get("trade_size_lamports")) if row.get("tiny_cap_safe") else None
    enriched["source_softening_needed"] = source_softening_needed(row)
    enriched["next_action"] = next_action(row, route_lookup, inventory_lookup)
    enriched["repeat_count"] = repeat_count(row)
    enriched["conversion_speed_score"] = conversion_speed_score(row, route_lookup, inventory_lookup)
    hunt_rows.append(enriched)

hunt_rows.sort(
    key=lambda row: (
        safe_float(row.get("conversion_speed_score")),
        row_best_net_after_fee_lamports(row),
        row_best_markout_lamports(row),
        row_owner_score(row),
    ),
    reverse=True,
)

route_blocker_groups = defaultdict(list)
for row in hunt_rows:
    key = (
        row.get("pool") or "",
        row.get("route") or "",
        row.get("mint_in") or "",
        row.get("mint_out") or "",
    )
    route_blocker_groups[key].append(row)

route_template_blockers = []
for key, rows in route_blocker_groups.items():
    pool, observed_route, mint_in, mint_out = key
    route_info = route_lookup.get(key, {})
    template_ready_count = sum(1 for row in rows if row.get("template_ready"))
    jito_ready_count = sum(1 for row in rows if row.get("jito_ready"))
    public_ready_count = sum(1 for row in rows if row.get("public_ready"))
    tpu_ready_count = sum(1 for row in rows if row.get("tpu_ready"))
    can_execute_as_observed_jito_nosend = observed_route in JITO_ROUTES and template_ready_count > 0
    if not template_ready_count:
        blocker_focus = "template_gap"
    elif observed_route in JITO_ROUTES and not tpu_ready_count and not public_ready_count:
        blocker_focus = "route_policy_or_egress_lane"
    elif route_info.get("blocker_class"):
        blocker_focus = route_info.get("blocker_class")
    else:
        blocker_focus = "mixed"
    route_template_blockers.append(
        {
            "pool": pool,
            "venue": rows[0].get("venue"),
            "mint_in": mint_in,
            "mint_out": mint_out,
            "observed_route": observed_route,
            "target_route_family": rows[0].get("target_route_family"),
            "row_count": len(rows),
            "owner_mode_counts": dict(Counter(row_owner(row) for row in rows)),
            "source_counts": dict(Counter((row.get("source") or "unknown") for row in rows)),
            "template_ready_count": template_ready_count,
            "jito_ready_count": jito_ready_count,
            "public_ready_count": public_ready_count,
            "tpu_ready_count": tpu_ready_count,
            "positive_after_fee_count": sum(1 for row in rows if row_best_net_after_fee_lamports(row) > 0),
            "positive_markout_count": sum(1 for row in rows if row_best_markout_lamports(row) > 0),
            "can_execute_as_observed_jito_nosend": can_execute_as_observed_jito_nosend,
            "can_convert_to_tpu_public": bool(tpu_ready_count or public_ready_count),
            "blocker_focus": blocker_focus,
            "route_conversion_blocker": route_info.get("blocker_class") or blocker_focus,
            "recommended_action": (
                "promote_jito_nosend_first_then_evaluate_tpu_public_conversion"
                if can_execute_as_observed_jito_nosend and blocker_focus == "route_policy_or_egress_lane"
                else (
                    "build_template_then_reassess_route"
                    if blocker_focus == "template_gap"
                    else (
                        "map_tpu_public_conversion_path"
                        if blocker_focus in {"route_policy_or_egress_lane", "route_family_policy_gap"}
                        else "keep_as_jito_nosend_candidate"
                    )
                )
            ),
            "sample_candidates": [compact_candidate(row) for row in rows[:10]],
        }
    )

route_template_blockers.sort(
    key=lambda item: (
        item["positive_after_fee_count"],
        item["positive_markout_count"],
        item["row_count"],
    ),
    reverse=True,
)

inventory_groups = defaultdict(list)
for row in hunt_rows:
    inventory_groups[row.get("mint_in") or ""].append(row)

inventory_preflight_items = []
for mint_in, rows in inventory_groups.items():
    statuses = Counter(row.get("required_inventory") for row in rows)
    any_output_ata_needed = any(row.get("output_ata_needed") for row in rows)
    primary_status = statuses.most_common(1)[0][0] if statuses else "inventory_unknown"
    if mint_in == SYSTEM_PROGRAM_ID:
        next_step = "deny_bad_mint"
    elif primary_status == "top_up_existing_band":
        next_step = "top_up_existing_band"
    elif primary_status == "create_input_ata":
        next_step = "create_input_ata"
    elif primary_status == "new_inventory_band_required":
        next_step = "evaluate_new_inventory_band"
    elif any_output_ata_needed:
        next_step = "precreate_output_ata"
    else:
        next_step = "inventory_ready"
    inventory_preflight_items.append(
        {
            "mint_in": mint_in,
            "row_count": len(rows),
            "inventory_status_counts": dict(statuses),
            "dominant_owner_mode": Counter(row_owner(row) for row in rows).most_common(1)[0][0],
            "positive_after_fee_count": sum(1 for row in rows if row_best_net_after_fee_lamports(row) > 0),
            "positive_markout_count": sum(1 for row in rows if row_best_markout_lamports(row) > 0),
            "output_ata_needed_count": sum(1 for row in rows if row.get("output_ata_needed")),
            "next_step": next_step,
            "sample_candidates": [compact_candidate(row) for row in rows[:10]],
        }
    )

inventory_preflight_items.sort(
    key=lambda item: (
        item["positive_after_fee_count"],
        item["positive_markout_count"],
        item["row_count"],
    ),
    reverse=True,
)

continuation_hunt_rows = [row for row in hunt_rows if row_owner(row) == "continuation"]
continuation_pool_groups = defaultdict(list)
for row in continuation_hunt_rows:
    continuation_pool_groups[row.get("pool") or ""].append(row)

continuation_by_pool = []
for pool, rows in continuation_pool_groups.items():
    markouts = [row_best_markout_lamports(row) for row in rows]
    nets = [row_best_net_after_fee_lamports(row) for row in rows]
    maes = [safe_int(row_markouts(row).get("max_adverse_excursion_lamports")) for row in rows]
    repeats = [repeat_count(row) for row in rows]
    sizes = [safe_int(row.get("trade_size_lamports")) for row in rows if safe_int(row.get("trade_size_lamports")) > 0]
    continuation_by_pool.append(
        {
            "pool": pool,
            "venue": rows[0].get("venue"),
            "mint_in": rows[0].get("mint_in"),
            "mint_out": rows[0].get("mint_out"),
            "row_count": len(rows),
            "best_horizon_counts": dict(Counter((row_markouts(row).get("best_horizon") or "unknown") for row in rows)),
            "median_markout_lamports": statistics.median(markouts) if markouts else None,
            "p90_markout_lamports": percentile(markouts, 0.90),
            "median_net_after_fee_lamports": statistics.median(nets) if nets else None,
            "p90_net_after_fee_lamports": percentile(nets, 0.90),
            "max_adverse_excursion_lamports": min(maes) if maes else None,
            "repeat_count_max": max(repeats) if repeats else 0,
            "repeat_count_median": statistics.median(repeats) if repeats else None,
            "safe_size_lamports": min(sizes) if sizes else None,
            "sample_candidates": [compact_candidate(row) for row in rows[:10]],
        }
    )

continuation_by_pool.sort(
    key=lambda item: (
        safe_int(item["p90_markout_lamports"]),
        safe_int(item["row_count"]),
    ),
    reverse=True,
)

continuation_candidate_rows = []
for row in continuation_hunt_rows:
    stats = continuation_event_stats(row)
    continuation_candidate_rows.append(
        {
            "candidate_id": row.get("candidate_id"),
            "venue": row.get("venue"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "source": row.get("source"),
            "route": row.get("route"),
            "best_horizon": row_markouts(row).get("best_horizon"),
            "best_horizon_markout_lamports": row_best_markout_lamports(row),
            "best_horizon_net_after_fee_lamports": row_best_net_after_fee_lamports(row),
            "max_adverse_excursion_lamports": safe_int(row_markouts(row).get("max_adverse_excursion_lamports")),
            "repeat_count": repeat_count(row),
            "safe_size_lamports": row.get("safe_size_lamports"),
            "median_event_markout_lamports": stats["median_event_markout_lamports"],
            "p90_event_markout_lamports": stats["p90_event_markout_lamports"],
            "median_event_mae_lamports": stats["median_event_mae_lamports"],
            "p90_event_mae_lamports": stats["p90_event_mae_lamports"],
            "next_action": row.get("next_action"),
            "conversion_speed_score": row.get("conversion_speed_score"),
        }
    )

continuation_candidate_rows.sort(
    key=lambda item: (
        safe_int(item["best_horizon_net_after_fee_lamports"]),
        safe_int(item["best_horizon_markout_lamports"]),
        safe_float(item["conversion_speed_score"]),
    ),
    reverse=True,
)

run49_candidates = []
for index, row in enumerate(hunt_rows, start=1):
    route = route_profile(row)
    run49_candidates.append(
        {
            "rank": index,
            "candidate_id": row.get("candidate_id"),
            "owner_mode": row_owner(row),
            "venue": row.get("venue"),
            "pool": row.get("pool"),
            "mint_in": row.get("mint_in"),
            "mint_out": row.get("mint_out"),
            "source": row.get("source"),
            "route": route["observed_route"],
            "target_route_family": route["target_route_family"],
            "best_horizon": row_markouts(row).get("best_horizon"),
            "best_horizon_markout_lamports": row_best_markout_lamports(row),
            "best_horizon_net_after_fee": row_best_net_after_fee_lamports(row),
            "required_inventory": row.get("required_inventory"),
            "output_ata_needed": row.get("output_ata_needed"),
            "template_ready": bool(row.get("template_ready")),
            "route_ready": bool(row.get("route_ready")),
            "tpu_ready": bool(row.get("tpu_ready")),
            "jito_ready": bool(row.get("jito_ready")),
            "public_ready": bool(row.get("public_ready")),
            "source_softening_needed": bool(row.get("source_softening_needed")),
            "repeat_count": row.get("repeat_count"),
            "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
            "safe_size_lamports": row.get("safe_size_lamports"),
            "tiny_cap_safe": bool(row.get("tiny_cap_safe")),
            "conversion_speed_score": row.get("conversion_speed_score"),
            "current_execution_path": row.get("current_execution_path"),
            "first_rejection_stage": row.get("first_rejection_stage"),
            "execution_blocker": row.get("execution_blocker"),
            "next_action": row.get("next_action"),
            "why_selected": row.get("why_selected"),
        }
    )

run49_hunt_conversion_plan = {
    "result_dir": str(RESULT_DIR),
    "summary": {
        "hunt_count": len(run49_candidates),
        "owner_mode_counts": dict(Counter(item["owner_mode"] for item in run49_candidates)),
        "next_action_counts": dict(Counter(item["next_action"] for item in run49_candidates)),
        "current_execution_path_counts": dict(Counter(item["current_execution_path"] for item in run49_candidates)),
        "template_ready_count": sum(1 for item in run49_candidates if item["template_ready"]),
        "inventory_ready_count": sum(1 for item in run49_candidates if item["required_inventory"] == "ready"),
        "output_ata_needed_count": sum(1 for item in run49_candidates if item["output_ata_needed"]),
        "jito_ready_count": sum(1 for item in run49_candidates if item["jito_ready"]),
        "tpu_ready_count": sum(1 for item in run49_candidates if item["tpu_ready"]),
        "public_ready_count": sum(1 for item in run49_candidates if item["public_ready"]),
        "afxdp_source_count": sum(1 for item in run49_candidates if item["source"] == "afxdp"),
        "external_source_count": sum(1 for item in run49_candidates if item["source"] != "afxdp"),
    },
    "filter_repair_bucket_actions": filter_repair_plan.get("bucket_actions", []),
    "candidates": run49_candidates,
}

run49_top_hunt_candidates = {
    "summary": {
        "top_10_owner_counts": dict(Counter(item["owner_mode"] for item in run49_candidates[:10])),
        "top_25_owner_counts": dict(Counter(item["owner_mode"] for item in run49_candidates[:25])),
        "jito_nosend_first_count": sum(1 for item in run49_candidates if item["current_execution_path"] == "jito_nosend_first"),
    },
    "top_candidates": run49_candidates[:25],
    "top_by_owner_mode": {
        owner: [item for item in run49_candidates if item["owner_mode"] == owner][:10]
        for owner in sorted({item["owner_mode"] for item in run49_candidates})
    },
}

run49_route_template_blockers = {
    "summary": {
        "group_count": len(route_template_blockers),
        "dominant_group_pool": route_template_blockers[0]["pool"] if route_template_blockers else None,
        "dominant_group_route": route_template_blockers[0]["observed_route"] if route_template_blockers else None,
    },
    "items": route_template_blockers,
}

run49_inventory_preflight_plan = {
    "summary": {
        "mint_group_count": len(inventory_preflight_items),
        "inventory_ready_group_count": sum(1 for item in inventory_preflight_items if item["next_step"] == "inventory_ready"),
        "new_inventory_band_group_count": sum(
            1 for item in inventory_preflight_items if item["next_step"] == "evaluate_new_inventory_band"
        ),
        "top_up_group_count": sum(1 for item in inventory_preflight_items if item["next_step"] == "top_up_existing_band"),
        "create_input_ata_group_count": sum(1 for item in inventory_preflight_items if item["next_step"] == "create_input_ata"),
        "deny_bad_mint_group_count": sum(1 for item in inventory_preflight_items if item["next_step"] == "deny_bad_mint"),
    },
    "items": inventory_preflight_items,
}

run49_continuation_hunt_report = {
    "summary": {
        "continuation_hunt_count": len(continuation_candidate_rows),
        "pool_count": len(continuation_by_pool),
        "positive_50ms_count": sum(
            1
            for row in continuation_hunt_rows
            if safe_int(horizon_dict(row_markouts(row), "time_horizons", "50ms").get("markout_lamports")) > 0
        ),
        "positive_100ms_count": sum(
            1
            for row in continuation_hunt_rows
            if safe_int(horizon_dict(row_markouts(row), "time_horizons", "100ms").get("markout_lamports")) > 0
        ),
        "positive_250ms_count": sum(
            1
            for row in continuation_hunt_rows
            if safe_int(horizon_dict(row_markouts(row), "time_horizons", "250ms").get("markout_lamports")) > 0
        ),
        "positive_500ms_count": sum(
            1
            for row in continuation_hunt_rows
            if safe_int(horizon_dict(row_markouts(row), "time_horizons", "500ms").get("markout_lamports")) > 0
        ),
        "positive_next_event_count": sum(
            1
            for row in continuation_hunt_rows
            if safe_int(horizon_dict(row_markouts(row), "event_horizons", "next_same_pool_event_1").get("markout_lamports")) > 0
        ),
    },
    "by_pool": continuation_by_pool,
    "candidates": continuation_candidate_rows,
}

RUN49_HUNT_CONVERSION_PLAN_PATH.write_text(json.dumps(run49_hunt_conversion_plan, indent=2) + "\n", encoding="utf-8")
RUN49_TOP_HUNT_CANDIDATES_PATH.write_text(json.dumps(run49_top_hunt_candidates, indent=2) + "\n", encoding="utf-8")
RUN49_ROUTE_TEMPLATE_BLOCKERS_PATH.write_text(
    json.dumps(run49_route_template_blockers, indent=2) + "\n",
    encoding="utf-8",
)
RUN49_INVENTORY_PREFLIGHT_PLAN_PATH.write_text(
    json.dumps(run49_inventory_preflight_plan, indent=2) + "\n",
    encoding="utf-8",
)
RUN49_CONTINUATION_HUNT_REPORT_PATH.write_text(
    json.dumps(run49_continuation_hunt_report, indent=2) + "\n",
    encoding="utf-8",
)

print(f"run49_hunt_conversion_plan={RUN49_HUNT_CONVERSION_PLAN_PATH}")
print(f"run49_top_hunt_candidates={RUN49_TOP_HUNT_CANDIDATES_PATH}")
print(f"run49_route_template_blockers={RUN49_ROUTE_TEMPLATE_BLOCKERS_PATH}")
print(f"run49_inventory_preflight_plan={RUN49_INVENTORY_PREFLIGHT_PLAN_PATH}")
print(f"run49_continuation_hunt_report={RUN49_CONTINUATION_HUNT_REPORT_PATH}")
print(f"hunt_count={len(run49_candidates)}")
print(f"top_candidate_pool={run49_candidates[0]['pool'] if run49_candidates else ''}")
print(f"dominant_route_blocker_pool={route_template_blockers[0]['pool'] if route_template_blockers else ''}")
print(f"continuation_hunt_count={len(continuation_candidate_rows)}")
PY

pass "Generated Run49 HUNT conversion artifacts in $RESULT_DIR"
finish

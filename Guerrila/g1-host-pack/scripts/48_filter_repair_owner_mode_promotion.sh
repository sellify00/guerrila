#!/usr/bin/env bash
# 48_filter_repair_owner_mode_promotion.sh - replay Run47 outputs into Run48 repair reports

source "$(dirname "$0")/lib.sh"
script_header "48 - Filter Repair / Owner Mode Promotion"

assert_cmd_exists python3

RESULT_DIR="${1:-}"
if [[ -z "$RESULT_DIR" ]]; then
    fail "Usage: $0 <run47-result-dir>"
    finish
    exit 1
fi

if [[ ! -d "$RESULT_DIR" ]]; then
    fail "Result directory does not exist: $RESULT_DIR"
    finish
    exit 1
fi

CANDIDATE_AUTOPSY_JSONL="$RESULT_DIR/candidate_autopsy.jsonl"
OWNER_MODE_SCORECARD_JSON="$RESULT_DIR/owner_mode_scorecard.json"
FILTER_FALSE_NEGATIVE_JSON="$RESULT_DIR/filter_false_negative_report.json"
CONTINUATION_P0_MARKOUT_JSON="$RESULT_DIR/continuation_p0_markout_report.json"
TARGET_FILTER_REJECTION_JSON="$RESULT_DIR/target_filter_rejection_report.json"
SURFACE_SCORECARD_JSON="$RESULT_DIR/surface_scorecard.json"

FILTER_REPAIR_PLAN_JSON="$RESULT_DIR/filter_repair_plan.json"
OWNER_MODE_HUNT_WATCH_DENY_JSON="$RESULT_DIR/owner_mode_hunt_watch_deny.json"
APPROVED_ATA_PREFLIGHT_QUEUE_JSON="$RESULT_DIR/approved_ata_preflight_queue.json"
ROUTE_CONVERSION_BLOCKERS_JSON="$RESULT_DIR/route_conversion_blockers.json"
INVENTORY_BAND_CANDIDATES_JSON="$RESULT_DIR/inventory_band_candidates.json"

missing=0
for required_path in \
    "$CANDIDATE_AUTOPSY_JSONL" \
    "$OWNER_MODE_SCORECARD_JSON" \
    "$FILTER_FALSE_NEGATIVE_JSON" \
    "$CONTINUATION_P0_MARKOUT_JSON" \
    "$TARGET_FILTER_REJECTION_JSON"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required Run47 artifact is missing: $required_path"
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
    "$OWNER_MODE_SCORECARD_JSON" \
    "$FILTER_FALSE_NEGATIVE_JSON" \
    "$CONTINUATION_P0_MARKOUT_JSON" \
    "$TARGET_FILTER_REJECTION_JSON" \
    "$SURFACE_SCORECARD_JSON" \
    "$FILTER_REPAIR_PLAN_JSON" \
    "$OWNER_MODE_HUNT_WATCH_DENY_JSON" \
    "$APPROVED_ATA_PREFLIGHT_QUEUE_JSON" \
    "$ROUTE_CONVERSION_BLOCKERS_JSON" \
    "$INVENTORY_BAND_CANDIDATES_JSON" <<'PY'
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

RESULT_DIR = Path(sys.argv[1])
CANDIDATE_AUTOPSY_PATH = Path(sys.argv[2])
OWNER_MODE_SCORECARD_PATH = Path(sys.argv[3])
FILTER_FALSE_NEGATIVE_PATH = Path(sys.argv[4])
CONTINUATION_P0_MARKOUT_PATH = Path(sys.argv[5])
TARGET_FILTER_REJECTION_PATH = Path(sys.argv[6])
SURFACE_SCORECARD_PATH = Path(sys.argv[7])
FILTER_REPAIR_PLAN_PATH = Path(sys.argv[8])
OWNER_MODE_HUNT_WATCH_DENY_PATH = Path(sys.argv[9])
APPROVED_ATA_PREFLIGHT_QUEUE_PATH = Path(sys.argv[10])
ROUTE_CONVERSION_BLOCKERS_PATH = Path(sys.argv[11])
INVENTORY_BAND_CANDIDATES_PATH = Path(sys.argv[12])

SYSTEM_PROGRAM_ID = "11111111111111111111111111111111"
SPL_TOKEN_PROGRAMS = {
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
}
PROMOTABLE_OWNER_MODES = {"continuation", "rebalancing", "housekeeping", "backrun", "flatten"}
EXECUTION_BLOCKER_BUCKETS = {
    "missing_output_ata": "missing_output_ata",
    "output_ata_uninitialized": "missing_output_ata",
    "spl_input_balance_insufficient": "spl_input_balance_insufficient",
}
MAJOR_BUCKET_CONFIG = {
    "source_not_afxdp": {
        "bucket_kind": "first_rejection_stage",
        "action": "soften_to_research_only",
        "primary_output": "owner_mode_hunt_watch_deny.json",
        "why": "External Geyser rows are useful for research scoring even when they are not canonical AF_XDP.",
    },
    "route_not_tpu_ready": {
        "bucket_kind": "first_rejection_stage",
        "action": "convert_to_preflight_task",
        "primary_output": "route_conversion_blockers.json",
        "why": "Observed route mismatch is an execution-lane blocker, not evidence that the surface is dead.",
    },
    "not_held_input": {
        "bucket_kind": "first_rejection_stage",
        "action": "convert_to_preflight_task",
        "primary_output": "inventory_band_candidates.json",
        "why": "Inventory gaps should become explicit band-expansion decisions instead of hard research drops.",
    },
    "not_positive_ev": {
        "bucket_kind": "first_rejection_stage",
        "action": "convert_to_owner_mode_candidate",
        "primary_output": "owner_mode_hunt_watch_deny.json",
        "why": "Run47 already proved some t0-negative rows become positive under continuation and rebalancing horizons.",
    },
    "missing_output_ata": {
        "bucket_kind": "execution_blocker",
        "action": "convert_to_preflight_task",
        "primary_output": "approved_ata_preflight_queue.json",
        "why": "Output ATA gaps should be queued as preflight tasks rather than erasing research-positive rows.",
    },
    "spl_input_balance_insufficient": {
        "bucket_kind": "execution_blocker",
        "action": "convert_to_preflight_task",
        "primary_output": "inventory_band_candidates.json",
        "why": "Existing approved inputs with insufficient balance need an explicit top-up band decision.",
    },
}


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


def top_counter_items(counter_obj, limit=10, label="value"):
    return [{label: item, "count": count} for item, count in counter_obj.most_common(limit)]


def row_markouts(row):
    return row.get("markouts") or {}


def row_best_markout_lamports(row):
    return safe_int(row_markouts(row).get("best_horizon_markout_lamports"))


def row_best_net_after_fee_lamports(row):
    return safe_int(row_markouts(row).get("best_horizon_net_after_fee_lamports"))


def row_owner(row):
    return (row.get("selected_owner") or "drop").strip() or "drop"


def row_owner_score(row):
    return safe_float(row.get("selected_owner_score"))


def row_is_research_positive(row):
    return bool(row.get("research_positive"))


def row_positive_markout(row):
    return row_best_markout_lamports(row) > 0


def row_positive_after_fee(row):
    return row_best_net_after_fee_lamports(row) > 0


def compact_candidate(row):
    return {
        "candidate_id": row.get("candidate_id"),
        "source": row.get("source"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "route": row.get("route"),
        "target_route_family": row.get("target_route_family"),
        "first_rejection_stage": row.get("first_rejection_stage"),
        "execution_blocker": row.get("execution_blocker"),
        "selected_owner": row_owner(row),
        "selected_owner_score": round(row_owner_score(row), 4),
        "best_horizon": row_markouts(row).get("best_horizon"),
        "best_horizon_markout_lamports": row_best_markout_lamports(row),
        "best_horizon_net_after_fee_lamports": row_best_net_after_fee_lamports(row),
        "positive_ev": bool(row.get("positive_ev")),
        "research_positive": row_is_research_positive(row),
        "approved_held_input": bool(row.get("approved_held_input")),
        "has_input_ata": bool(row.get("has_input_ata")),
        "has_output_ata": bool(row.get("has_output_ata")),
        "why_selected": row.get("why_selected"),
        "all_failed_filters": row.get("all_failed_filters") or [],
    }


def dominant_owner(rows):
    counts = Counter(row_owner(row) for row in rows)
    return counts.most_common(1)[0][0] if counts else "drop"


def rows_for_bucket(rows, bucket_name):
    if bucket_name in EXECUTION_BLOCKER_BUCKETS:
        return [
            row
            for row in rows
            if EXECUTION_BLOCKER_BUCKETS.get(row.get("execution_blocker")) == bucket_name
        ]
    return [row for row in rows if row.get("first_rejection_stage") == bucket_name]


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
    } or EXECUTION_BLOCKER_BUCKETS.get(row.get("execution_blocker") or "") in {
        "missing_output_ata",
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


def unique_non_empty(values):
    return sorted({value for value in values if value})


candidate_rows = load_jsonl(CANDIDATE_AUTOPSY_PATH)
owner_mode_scorecard = load_json(OWNER_MODE_SCORECARD_PATH)
filter_false_negative_report = load_json(FILTER_FALSE_NEGATIVE_PATH)
continuation_markout_report = load_json(CONTINUATION_P0_MARKOUT_PATH)
target_filter_rejection_report = load_json(TARGET_FILTER_REJECTION_PATH)
surface_scorecard = load_json(SURFACE_SCORECARD_PATH) if SURFACE_SCORECARD_PATH.exists() else {}

false_negative_by_filter = {
    item.get("filter_name"): item
    for item in filter_false_negative_report.get("filters", [])
    if item.get("filter_name")
}
approved_held_input_mints = set(
    ((surface_scorecard.get("inventory_context") or {}).get("approved_held_input_mints")) or []
)

hunt_watch_deny_rows = defaultdict(list)
for row in candidate_rows:
    hunt_watch_deny_rows[classify_hunt_watch_deny(row)].append(row)

owner_mode_hunt_watch_deny = {
    "summary": {
        "total_rows": len(candidate_rows),
        "hunt_count": len(hunt_watch_deny_rows["hunt"]),
        "watch_count": len(hunt_watch_deny_rows["watch"]),
        "deny_count": len(hunt_watch_deny_rows["deny"]),
        "by_owner": dict(Counter(row_owner(row) for row in candidate_rows)),
        "by_label_and_owner": {
            label: dict(Counter(row_owner(row) for row in rows))
            for label, rows in hunt_watch_deny_rows.items()
        },
    },
    "top_hunt_candidates": [
        compact_candidate(row)
        for row in sorted(
            hunt_watch_deny_rows["hunt"],
            key=lambda item: (
                row_best_net_after_fee_lamports(item),
                row_best_markout_lamports(item),
                row_owner_score(item),
            ),
            reverse=True,
        )[:50]
    ],
    "top_watch_candidates": [
        compact_candidate(row)
        for row in sorted(
            hunt_watch_deny_rows["watch"],
            key=lambda item: (
                row_best_markout_lamports(item),
                row_best_net_after_fee_lamports(item),
                row_owner_score(item),
            ),
            reverse=True,
        )[:100]
    ],
    "top_deny_candidates": [
        compact_candidate(row)
        for row in sorted(
            hunt_watch_deny_rows["deny"],
            key=lambda item: (
                row_owner_score(item),
                row_best_markout_lamports(item),
            ),
            reverse=True,
        )[:100]
    ],
}

ata_rows = [
    row
    for row in candidate_rows
    if EXECUTION_BLOCKER_BUCKETS.get(row.get("execution_blocker") or "") == "missing_output_ata"
]
ata_groups = defaultdict(list)
for row in ata_rows:
    key = (
        row.get("mint_out") or "",
        row.get("inventory_output_token_program") or "",
    )
    ata_groups[key].append(row)

queued_ata_items = []
excluded_ata_items = []
for (mint_out, token_program), rows in ata_groups.items():
    item = {
        "mint_out": mint_out,
        "token_program": token_program or None,
        "row_count": len(rows),
        "research_positive_count": sum(1 for row in rows if row_is_research_positive(row)),
        "positive_markout_count": sum(1 for row in rows if row_positive_markout(row)),
        "positive_after_fee_count": sum(1 for row in rows if row_positive_after_fee(row)),
        "dominant_owner_mode": dominant_owner(rows),
        "top_pools": top_counter_items(Counter(row.get("pool") or "" for row in rows), limit=5, label="pool"),
        "top_routes": top_counter_items(Counter(row.get("route") or "" for row in rows), limit=5, label="route"),
        "sample_candidates": [compact_candidate(row) for row in rows[:10]],
    }
    queueable = (
        mint_out
        and mint_out != SYSTEM_PROGRAM_ID
        and token_program in SPL_TOKEN_PROGRAMS
        and item["research_positive_count"] > 0
    )
    if queueable:
        item["action"] = "precreate_output_ata"
        item["action_type"] = "convert_to_preflight_task"
        item["rationale"] = "Recurring research-positive rows are blocked only by missing output ATA."
        queued_ata_items.append(item)
    else:
        item["action"] = "exclude_from_queue"
        item["action_type"] = "deny_with_evidence"
        item["rationale"] = (
            "Output mint lacks a trusted SPL token program or the rows are not research-positive enough to justify preflight."
        )
        excluded_ata_items.append(item)

approved_ata_preflight_queue = {
    "summary": {
        "total_missing_output_ata_rows": len(ata_rows),
        "queued_count": len(queued_ata_items),
        "excluded_count": len(excluded_ata_items),
    },
    "queued": sorted(
        queued_ata_items,
        key=lambda item: (
            item["positive_after_fee_count"],
            item["positive_markout_count"],
            item["row_count"],
        ),
        reverse=True,
    ),
    "excluded": sorted(
        excluded_ata_items,
        key=lambda item: (
            item["research_positive_count"],
            item["row_count"],
        ),
        reverse=True,
    ),
}

route_rows = [row for row in candidate_rows if row.get("first_rejection_stage") == "route_not_tpu_ready"]
route_groups = defaultdict(list)
for row in route_rows:
    key = (
        row.get("venue") or "",
        row.get("pool") or "",
        row.get("mint_in") or "",
        row.get("mint_out") or "",
        row.get("route") or "",
        row.get("target_route_family") or "",
    )
    route_groups[key].append(row)

route_conversion_items = []
for key, rows in route_groups.items():
    venue, pool, mint_in, mint_out, observed_route, target_route_family = key
    template_ready_count = sum(1 for row in rows if row.get("template_ready"))
    quote_ready_count = sum(1 for row in rows if row.get("quote_ready_or_classified"))
    input_ready_count = sum(1 for row in rows if row.get("input_account_ready"))
    output_ready_count = sum(1 for row in rows if row.get("output_account_ready"))
    if template_ready_count < len(rows):
        blocker_class = "template_gap"
        recommended_action = "build_template_then_convert_route"
    elif input_ready_count < len(rows) or output_ready_count < len(rows):
        blocker_class = "account_gap"
        recommended_action = "preflight_accounts_then_convert_route"
    elif quote_ready_count < len(rows):
        blocker_class = "quote_classification_gap"
        recommended_action = "retain_in_owner_mode_scoring_until_quote_classification_is_stable"
    else:
        blocker_class = "route_family_policy_gap"
        recommended_action = "map_route_family_conversion_for_tpu_or_public_execution"
    route_conversion_items.append(
        {
            "venue": venue,
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "observed_route": observed_route,
            "target_route_family": target_route_family or None,
            "row_count": len(rows),
            "research_positive_count": sum(1 for row in rows if row_is_research_positive(row)),
            "positive_after_fee_count": sum(1 for row in rows if row_positive_after_fee(row)),
            "positive_markout_count": sum(1 for row in rows if row_positive_markout(row)),
            "template_ready_count": template_ready_count,
            "quote_ready_or_classified_count": quote_ready_count,
            "input_account_ready_count": input_ready_count,
            "output_account_ready_count": output_ready_count,
            "dominant_owner_mode": dominant_owner(rows),
            "owner_mode_counts": dict(Counter(row_owner(row) for row in rows)),
            "source_counts": dict(Counter((row.get("source") or "unknown") for row in rows)),
            "blocker_class": blocker_class,
            "recommended_action": recommended_action,
            "sample_candidates": [compact_candidate(row) for row in rows[:10]],
        }
    )

route_conversion_blockers = {
    "summary": {
        "total_route_blocked_rows": len(route_rows),
        "group_count": len(route_conversion_items),
        "route_policy_gap_groups": sum(1 for item in route_conversion_items if item["blocker_class"] == "route_family_policy_gap"),
        "template_gap_groups": sum(1 for item in route_conversion_items if item["blocker_class"] == "template_gap"),
        "account_gap_groups": sum(1 for item in route_conversion_items if item["blocker_class"] == "account_gap"),
        "quote_gap_groups": sum(1 for item in route_conversion_items if item["blocker_class"] == "quote_classification_gap"),
    },
    "items": sorted(
        route_conversion_items,
        key=lambda item: (
            item["positive_after_fee_count"],
            item["positive_markout_count"],
            item["row_count"],
        ),
        reverse=True,
    ),
}

inventory_rows = [
    row
    for row in candidate_rows
    if row.get("first_rejection_stage") == "not_held_input"
    or EXECUTION_BLOCKER_BUCKETS.get(row.get("execution_blocker") or "") == "spl_input_balance_insufficient"
]
inventory_groups = defaultdict(list)
for row in inventory_rows:
    key = row.get("mint_in") or ""
    inventory_groups[key].append(row)

inventory_items = []
for mint_in, rows in inventory_groups.items():
    approved_count = sum(1 for row in rows if row.get("approved_held_input") or mint_in in approved_held_input_mints)
    insufficient_balance_count = sum(
        1 for row in rows if EXECUTION_BLOCKER_BUCKETS.get(row.get("execution_blocker") or "") == "spl_input_balance_insufficient"
    )
    positive_after_fee_count = sum(1 for row in rows if row_positive_after_fee(row))
    positive_markout_count = sum(1 for row in rows if row_positive_markout(row))
    dominant = dominant_owner(rows)
    if mint_in == SYSTEM_PROGRAM_ID:
        action = "deny_bad_mint"
        action_type = "deny_with_evidence"
    elif approved_count > 0 and insufficient_balance_count > 0:
        action = "top_up_existing_band"
        action_type = "convert_to_preflight_task"
    elif positive_after_fee_count > 0 and dominant != "drop":
        action = "consider_new_inventory_band"
        action_type = "convert_to_preflight_task"
    elif positive_markout_count > 0:
        action = "keep_watch_only_until_inventory_policy_expands"
        action_type = "soften_to_research_only"
    else:
        action = "deny_bad_mint"
        action_type = "deny_with_evidence"
    inventory_items.append(
        {
            "mint_in": mint_in,
            "row_count": len(rows),
            "approved_held_input_count": approved_count,
            "insufficient_balance_count": insufficient_balance_count,
            "research_positive_count": sum(1 for row in rows if row_is_research_positive(row)),
            "positive_markout_count": positive_markout_count,
            "positive_after_fee_count": positive_after_fee_count,
            "dominant_owner_mode": dominant,
            "owner_mode_counts": dict(Counter(row_owner(row) for row in rows)),
            "top_pools": top_counter_items(Counter(row.get("pool") or "" for row in rows), limit=10, label="pool"),
            "top_output_mints": top_counter_items(Counter(row.get("mint_out") or "" for row in rows), limit=10, label="mint_out"),
            "action": action,
            "action_type": action_type,
            "sample_candidates": [compact_candidate(row) for row in rows[:10]],
        }
    )

inventory_band_candidates = {
    "summary": {
        "total_inventory_blocked_rows": len(inventory_rows),
        "mint_count": len(inventory_items),
        "top_up_existing_band_count": sum(1 for item in inventory_items if item["action"] == "top_up_existing_band"),
        "new_band_candidate_count": sum(1 for item in inventory_items if item["action"] == "consider_new_inventory_band"),
        "watch_only_count": sum(
            1 for item in inventory_items if item["action"] == "keep_watch_only_until_inventory_policy_expands"
        ),
        "deny_count": sum(1 for item in inventory_items if item["action"] == "deny_bad_mint"),
    },
    "items": sorted(
        inventory_items,
        key=lambda item: (
            item["positive_after_fee_count"],
            item["positive_markout_count"],
            item["row_count"],
        ),
        reverse=True,
    ),
}

bucket_rows = {}
for bucket_name in MAJOR_BUCKET_CONFIG:
    rows = rows_for_bucket(candidate_rows, bucket_name)
    if rows:
        bucket_rows[bucket_name] = rows

filter_repair_buckets = []
for bucket_name, config in MAJOR_BUCKET_CONFIG.items():
    rows = bucket_rows.get(bucket_name, [])
    if not rows:
        continue
    false_negative_summary = false_negative_by_filter.get(bucket_name, {})
    positive_markout_after_reject_count = false_negative_summary.get(
        "positive_markout_after_reject_count",
        sum(1 for row in rows if row_positive_markout(row)),
    )
    positive_after_fee_after_reject_count = false_negative_summary.get(
        "positive_after_fee_after_reject_count",
        sum(1 for row in rows if row_positive_after_fee(row)),
    )
    false_negative_rate = false_negative_summary.get("false_negative_rate")
    if false_negative_rate is None:
        false_negative_rate = round(positive_markout_after_reject_count / len(rows), 4) if rows else 0.0
    filter_repair_buckets.append(
        {
            "bucket_name": bucket_name,
            "bucket_kind": config["bucket_kind"],
            "rejected_count": len(rows),
            "research_positive_count": sum(1 for row in rows if row_is_research_positive(row)),
            "positive_markout_after_reject_count": positive_markout_after_reject_count,
            "positive_after_fee_after_reject_count": positive_after_fee_after_reject_count,
            "false_negative_rate": false_negative_rate,
            "dominant_owner_mode": false_negative_summary.get("best_owner_mode_after_reject") or dominant_owner(rows),
            "action_type": config["action"],
            "primary_output": config["primary_output"],
            "rationale": config["why"],
            "top_pools": top_counter_items(Counter(row.get("pool") or "" for row in rows), limit=5, label="pool"),
            "top_mint_pairs": top_counter_items(
                Counter(f"{row.get('mint_in') or ''}->{row.get('mint_out') or ''}" for row in rows),
                limit=5,
                label="mint_pair",
            ),
        }
    )

filter_repair_plan = {
    "result_dir": str(RESULT_DIR),
    "inputs": {
        "candidate_autopsy": str(CANDIDATE_AUTOPSY_PATH),
        "owner_mode_scorecard": str(OWNER_MODE_SCORECARD_PATH),
        "filter_false_negative_report": str(FILTER_FALSE_NEGATIVE_PATH),
        "continuation_p0_markout_report": str(CONTINUATION_P0_MARKOUT_PATH),
        "target_filter_rejection_report": str(TARGET_FILTER_REJECTION_PATH),
        "surface_scorecard": str(SURFACE_SCORECARD_PATH) if SURFACE_SCORECARD_PATH.exists() else None,
    },
    "outputs": {
        "filter_repair_plan": str(FILTER_REPAIR_PLAN_PATH),
        "owner_mode_hunt_watch_deny": str(OWNER_MODE_HUNT_WATCH_DENY_PATH),
        "approved_ata_preflight_queue": str(APPROVED_ATA_PREFLIGHT_QUEUE_PATH),
        "route_conversion_blockers": str(ROUTE_CONVERSION_BLOCKERS_PATH),
        "inventory_band_candidates": str(INVENTORY_BAND_CANDIDATES_PATH),
    },
    "summary": {
        "total_observed": owner_mode_scorecard.get("total_observed"),
        "research_positive_count": owner_mode_scorecard.get("research_positive_count"),
        "execution_blocked_count": owner_mode_scorecard.get("execution_blocked_count"),
        "continuation_watch_count": owner_mode_scorecard.get("continuation_watch_count"),
        "continuation_hunt_count": owner_mode_scorecard.get("continuation_hunt_count"),
        "major_bucket_count": len(filter_repair_buckets),
        "hunt_count": owner_mode_hunt_watch_deny["summary"]["hunt_count"],
        "watch_count": owner_mode_hunt_watch_deny["summary"]["watch_count"],
        "deny_count": owner_mode_hunt_watch_deny["summary"]["deny_count"],
        "ata_queue_count": approved_ata_preflight_queue["summary"]["queued_count"],
        "route_conversion_group_count": route_conversion_blockers["summary"]["group_count"],
        "inventory_band_mint_count": inventory_band_candidates["summary"]["mint_count"],
    },
    "bucket_actions": sorted(
        filter_repair_buckets,
        key=lambda item: (
            safe_float(item["false_negative_rate"]),
            safe_int(item["rejected_count"]),
        ),
        reverse=True,
    ),
}

FILTER_REPAIR_PLAN_PATH.write_text(json.dumps(filter_repair_plan, indent=2) + "\n", encoding="utf-8")
OWNER_MODE_HUNT_WATCH_DENY_PATH.write_text(
    json.dumps(owner_mode_hunt_watch_deny, indent=2) + "\n",
    encoding="utf-8",
)
APPROVED_ATA_PREFLIGHT_QUEUE_PATH.write_text(
    json.dumps(approved_ata_preflight_queue, indent=2) + "\n",
    encoding="utf-8",
)
ROUTE_CONVERSION_BLOCKERS_PATH.write_text(
    json.dumps(route_conversion_blockers, indent=2) + "\n",
    encoding="utf-8",
)
INVENTORY_BAND_CANDIDATES_PATH.write_text(
    json.dumps(inventory_band_candidates, indent=2) + "\n",
    encoding="utf-8",
)

print(f"filter_repair_plan={FILTER_REPAIR_PLAN_PATH}")
print(f"owner_mode_hunt_watch_deny={OWNER_MODE_HUNT_WATCH_DENY_PATH}")
print(f"approved_ata_preflight_queue={APPROVED_ATA_PREFLIGHT_QUEUE_PATH}")
print(f"route_conversion_blockers={ROUTE_CONVERSION_BLOCKERS_PATH}")
print(f"inventory_band_candidates={INVENTORY_BAND_CANDIDATES_PATH}")
print(f"hunt_count={owner_mode_hunt_watch_deny['summary']['hunt_count']}")
print(f"watch_count={owner_mode_hunt_watch_deny['summary']['watch_count']}")
print(f"deny_count={owner_mode_hunt_watch_deny['summary']['deny_count']}")
print(f"ata_queue_count={approved_ata_preflight_queue['summary']['queued_count']}")
print(f"route_conversion_group_count={route_conversion_blockers['summary']['group_count']}")
print(f"inventory_band_mint_count={inventory_band_candidates['summary']['mint_count']}")
PY

pass "Generated Run48 repair artifacts in $RESULT_DIR"
finish

#!/usr/bin/env bash
# 51_route_conversion.sh - offline public/TPU route-conversion report

source "$(dirname "$0")/lib.sh"
script_header "51 - Route Conversion"

assert_cmd_exists python3

RESULT_DIR="${1:-}"
if [[ -z "$RESULT_DIR" ]]; then
    fail "Usage: $0 <run47-run50-result-dir>"
    finish
    exit 1
fi

if [[ ! -d "$RESULT_DIR" ]]; then
    fail "Result directory does not exist: $RESULT_DIR"
    finish
    exit 1
fi

RUN49_HUNT_CONVERSION_PLAN_JSON="$RESULT_DIR/run49_hunt_conversion_plan.json"
RUN49_ROUTE_TEMPLATE_BLOCKERS_JSON="$RESULT_DIR/run49_route_template_blockers.json"
RUN49_INVENTORY_PREFLIGHT_PLAN_JSON="$RESULT_DIR/run49_inventory_preflight_plan.json"
RUN51_ROUTE_CONVERSION_SUMMARY_JSON="$RESULT_DIR/run51_route_conversion_summary.json"
RUN51_TOP_ROUTE_CONVERSION_TARGETS_JSON="$RESULT_DIR/run51_top_route_conversion_targets.json"
RUN51_PUBLIC_ROUTE_CONVERSION_BLOCKERS_JSON="$RESULT_DIR/run51_public_route_conversion_blockers.json"
RUN51_TPU_ROUTE_CONVERSION_BLOCKERS_JSON="$RESULT_DIR/run51_tpu_route_conversion_blockers.json"
RUN51_CONVERSION_TASK_QUEUE_JSON="$RESULT_DIR/run51_conversion_task_queue.json"

missing=0
for required_path in "$RUN49_HUNT_CONVERSION_PLAN_JSON" "$RUN49_ROUTE_TEMPLATE_BLOCKERS_JSON" "$RUN49_INVENTORY_PREFLIGHT_PLAN_JSON"; do
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
    "$RUN49_HUNT_CONVERSION_PLAN_JSON" \
    "$RUN49_ROUTE_TEMPLATE_BLOCKERS_JSON" \
    "$RUN49_INVENTORY_PREFLIGHT_PLAN_JSON" \
    "$RUN51_ROUTE_CONVERSION_SUMMARY_JSON" \
    "$RUN51_TOP_ROUTE_CONVERSION_TARGETS_JSON" \
    "$RUN51_PUBLIC_ROUTE_CONVERSION_BLOCKERS_JSON" \
    "$RUN51_TPU_ROUTE_CONVERSION_BLOCKERS_JSON" \
    "$RUN51_CONVERSION_TASK_QUEUE_JSON" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

RESULT_DIR = Path(sys.argv[1])
RUN49_HUNT_CONVERSION_PLAN_PATH = Path(sys.argv[2])
RUN49_ROUTE_TEMPLATE_BLOCKERS_PATH = Path(sys.argv[3])
RUN49_INVENTORY_PREFLIGHT_PLAN_PATH = Path(sys.argv[4])
RUN51_ROUTE_CONVERSION_SUMMARY_PATH = Path(sys.argv[5])
RUN51_TOP_ROUTE_CONVERSION_TARGETS_PATH = Path(sys.argv[6])
RUN51_PUBLIC_ROUTE_CONVERSION_BLOCKERS_PATH = Path(sys.argv[7])
RUN51_TPU_ROUTE_CONVERSION_BLOCKERS_PATH = Path(sys.argv[8])
RUN51_CONVERSION_TASK_QUEUE_PATH = Path(sys.argv[9])

JITO_ROUTES = {"jito_bundle", "harmonic_bundle"}
PUBLIC_SUPPORTED_VENUES = {"raydium_clmm"}
PUBLIC_SHAPE_SOURCES = {"external_geyser", "public_geyser", "public_rpc", "public"}


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def compact_candidate(row):
    return {
        "rank": row.get("rank"),
        "candidate_id": row.get("candidate_id"),
        "owner_mode": row.get("owner_mode"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "source": row.get("source"),
        "route": row.get("route"),
        "target_route_family": row.get("target_route_family"),
        "required_inventory": row.get("required_inventory"),
        "output_ata_needed": bool(row.get("output_ata_needed")),
        "template_ready": bool(row.get("template_ready")),
        "tpu_ready": bool(row.get("tpu_ready")),
        "public_ready": bool(row.get("public_ready")),
        "tiny_cap_safe": bool(row.get("tiny_cap_safe")),
        "source_softening_needed": bool(row.get("source_softening_needed")),
        "conversion_speed_score": row.get("conversion_speed_score"),
        "next_action": row.get("next_action"),
    }


run49_plan = load_json(RUN49_HUNT_CONVERSION_PLAN_PATH)
route_template_blockers = load_json(RUN49_ROUTE_TEMPLATE_BLOCKERS_PATH)
inventory_preflight_plan = load_json(RUN49_INVENTORY_PREFLIGHT_PLAN_PATH)
run49_candidates = list(run49_plan.get("candidates") or [])

route_group_by_pool = {
    item.get("pool"): item for item in route_template_blockers.get("items", []) if item.get("pool")
}
inventory_group_by_mint = {
    item.get("mint_in"): item for item in inventory_preflight_plan.get("items", []) if item.get("mint_in")
}


def public_blocker_for(row):
    venue = row.get("venue") or ""
    source = row.get("source") or ""
    required_inventory = row.get("required_inventory") or ""
    observed_route = row.get("route") or ""
    if venue not in PUBLIC_SUPPORTED_VENUES:
        return {
            "stage": "route_shape",
            "blocker": "wrong_venue",
            "detail": "public_rpc_scout is currently clamped to raydium_clmm",
            "next_action": "add_public_route_support_for_venue",
        }
    if required_inventory != "ready":
        blocker = {
            "create_input_ata": "missing_input_ata",
            "top_up_existing_band": "input_balance_insufficient",
            "new_inventory_band_required": "inventory_band_required",
            "inventory_unknown": "inventory_unknown",
            "deny_bad_mint": "deny_bad_mint",
        }.get(required_inventory, required_inventory)
        return {
            "stage": "inventory_preflight",
            "blocker": blocker,
            "detail": f"required_inventory={required_inventory}",
            "next_action": row.get("next_action") or "repair_inventory",
        }
    if bool(row.get("output_ata_needed")):
        return {
            "stage": "inventory_preflight",
            "blocker": "missing_output_ata",
            "detail": "output ATA or output account readiness is still missing",
            "next_action": "precreate_output_ata",
        }
    if not bool(row.get("template_ready")):
        return {
            "stage": "template_family",
            "blocker": "missing_template_family",
            "detail": "template readiness is false for public conversion",
            "next_action": "build_public_template_family",
        }
    if bool(row.get("public_ready")):
        return {
            "stage": "ready",
            "blocker": "public_ready",
            "detail": "candidate is already public-ready",
            "next_action": "run_public_nosend",
        }
    if source not in {"afxdp"}:
        return {
            "stage": "source_gate",
            "blocker": "source_cleanup_required",
            "detail": "current public config still requires AF_XDP-backed live source for production public scout",
            "next_action": "move_candidate_to_afxdp_or_relax_public_source_gate",
        }
    if observed_route in JITO_ROUTES:
        return {
            "stage": "route_policy",
            "blocker": "route_policy_private_only",
            "detail": "observed route is private-only and needs public route-family conversion",
            "next_action": "map_public_route_conversion_path",
        }
    return {
        "stage": "route_policy",
        "blocker": "public_route_not_ready",
        "detail": f"observed route={observed_route or 'unknown'} is not yet public-ready",
        "next_action": "map_public_route_conversion_path",
    }


def tpu_blocker_for(row):
    required_inventory = row.get("required_inventory") or ""
    observed_route = row.get("route") or ""
    group = route_group_by_pool.get(row.get("pool"))
    if required_inventory != "ready":
        blocker = {
            "create_input_ata": "missing_input_ata",
            "top_up_existing_band": "input_balance_insufficient",
            "new_inventory_band_required": "inventory_band_required",
            "inventory_unknown": "inventory_unknown",
            "deny_bad_mint": "deny_bad_mint",
        }.get(required_inventory, required_inventory)
        return {
            "stage": "inventory_preflight",
            "blocker": blocker,
            "detail": f"required_inventory={required_inventory}",
            "next_action": row.get("next_action") or "repair_inventory",
        }
    if bool(row.get("output_ata_needed")):
        return {
            "stage": "inventory_preflight",
            "blocker": "missing_output_ata",
            "detail": "output ATA or output account readiness is still missing",
            "next_action": "precreate_output_ata",
        }
    if not bool(row.get("template_ready")):
        return {
            "stage": "template_family",
            "blocker": "missing_template_family",
            "detail": "template readiness is false for TPU conversion",
            "next_action": "build_tpu_template_family",
        }
    if (row.get("source") or "") != "afxdp":
        return {
            "stage": "source_gate",
            "blocker": "source_not_afxdp",
            "detail": "TPU route conversion still expects AF_XDP/private-grade source",
            "next_action": "move_candidate_to_afxdp",
        }
    if not bool(row.get("tiny_cap_safe")):
        return {
            "stage": "sizing",
            "blocker": "not_tiny_cap_safe",
            "detail": "candidate shape is not yet safe at tiny-live sizing",
            "next_action": "reduce_size_or_reclassify",
        }
    if bool(row.get("tpu_ready")):
        return {
            "stage": "ready",
            "blocker": "tpu_ready",
            "detail": "candidate is already TPU-ready",
            "next_action": "run_tpu_nosend",
        }
    if group and group.get("route_conversion_blocker"):
        blocker = group.get("route_conversion_blocker")
        next_action = (
            "map_tpu_public_conversion_path"
            if blocker in {"route_family_policy_gap", "route_policy_or_egress_lane"}
            else group.get("recommended_action") or "map_tpu_public_conversion_path"
        )
        return {
            "stage": "route_policy",
            "blocker": blocker,
            "detail": group.get("blocker_focus") or "route conversion blocker",
            "next_action": next_action,
        }
    if observed_route in JITO_ROUTES:
        return {
            "stage": "route_policy",
            "blocker": "route_family_policy_gap",
            "detail": "observed route remains jito_bundle without TPU/public conversion",
            "next_action": "map_tpu_public_conversion_path",
        }
    return {
        "stage": "route_policy",
        "blocker": "tpu_route_not_ready",
        "detail": f"observed route={observed_route or 'unknown'} is not yet TPU-ready",
        "next_action": "map_tpu_public_conversion_path",
    }


def public_conversion_score(row, blocker):
    score = safe_float(row.get("conversion_speed_score"))
    penalties = {
        "wrong_venue": 220.0,
        "inventory_band_required": 60.0,
        "missing_input_ata": 45.0,
        "input_balance_insufficient": 30.0,
        "missing_output_ata": 25.0,
        "source_cleanup_required": 12.0,
        "missing_template_family": 35.0,
        "route_policy_private_only": 8.0,
        "public_route_not_ready": 12.0,
    }
    return round(score - penalties.get(blocker["blocker"], 10.0), 4)


def tpu_conversion_score(row, blocker):
    score = safe_float(row.get("conversion_speed_score"))
    penalties = {
        "inventory_band_required": 50.0,
        "missing_input_ata": 45.0,
        "input_balance_insufficient": 30.0,
        "missing_output_ata": 25.0,
        "source_not_afxdp": 20.0,
        "not_tiny_cap_safe": 18.0,
        "missing_template_family": 35.0,
        "route_family_policy_gap": 10.0,
        "route_policy_or_egress_lane": 10.0,
        "tpu_route_not_ready": 15.0,
    }
    return round(score - penalties.get(blocker["blocker"], 10.0), 4)


public_items = []
tpu_items = []
for row in run49_candidates:
    group = route_group_by_pool.get(row.get("pool"))
    inventory_group = inventory_group_by_mint.get(row.get("mint_in"))
    public_blocker = public_blocker_for(row)
    tpu_blocker = tpu_blocker_for(row)
    public_items.append(
        {
            **compact_candidate(row),
            "public_blocker_stage": public_blocker["stage"],
            "public_blocker": public_blocker["blocker"],
            "public_blocker_detail": public_blocker["detail"],
            "public_next_action": public_blocker["next_action"],
            "public_conversion_score": public_conversion_score(row, public_blocker),
            "route_group_blocker": group.get("route_conversion_blocker") if group else None,
            "inventory_group_next_step": inventory_group.get("next_step") if inventory_group else None,
        }
    )
    tpu_items.append(
        {
            **compact_candidate(row),
            "tpu_blocker_stage": tpu_blocker["stage"],
            "tpu_blocker": tpu_blocker["blocker"],
            "tpu_blocker_detail": tpu_blocker["detail"],
            "tpu_next_action": tpu_blocker["next_action"],
            "tpu_conversion_score": tpu_conversion_score(row, tpu_blocker),
            "route_group_blocker": group.get("route_conversion_blocker") if group else None,
            "route_group_recommended_action": group.get("recommended_action") if group else None,
            "route_group_row_count": group.get("row_count") if group else 0,
            "inventory_group_next_step": inventory_group.get("next_step") if inventory_group else None,
        }
    )

public_items.sort(
    key=lambda item: (
        item["public_conversion_score"],
        safe_float(item.get("conversion_speed_score")),
        -(item.get("rank") or 10**9),
    ),
    reverse=True,
)
tpu_items.sort(
    key=lambda item: (
        item["tpu_conversion_score"],
        safe_float(item.get("conversion_speed_score")),
        -(item.get("rank") or 10**9),
    ),
    reverse=True,
)

top_overall = compact_candidate(run49_candidates[0]) if run49_candidates else None
closest_public = public_items[0] if public_items else None
closest_tpu = tpu_items[0] if tpu_items else None
dominant_route_group = route_template_blockers.get("items", [None])[0] if route_template_blockers.get("items") else None

task_queue = []
if dominant_route_group:
    task_queue.append(
        {
            "task_id": "dominant-tpu-route-gap",
            "priority": 1,
            "lane": "tpu",
            "pool": dominant_route_group.get("pool"),
            "venue": dominant_route_group.get("venue"),
            "owner_mode": max(dominant_route_group.get("owner_mode_counts", {}), key=dominant_route_group.get("owner_mode_counts", {}).get, default=None),
            "blocker": dominant_route_group.get("route_conversion_blocker"),
            "recommended_action": "map_tpu_public_conversion_path",
            "why_now": "dominant positive HUNT group is inventory-ready, AF_XDP-heavy, tiny-cap-safe, and still blocked only by route-family conversion",
            "evidence": {
                "row_count": dominant_route_group.get("row_count"),
                "template_ready_count": dominant_route_group.get("template_ready_count"),
                "jito_ready_count": dominant_route_group.get("jito_ready_count"),
                "tpu_ready_count": dominant_route_group.get("tpu_ready_count"),
                "public_ready_count": dominant_route_group.get("public_ready_count"),
                "source_counts": dominant_route_group.get("source_counts"),
            },
        }
    )
if closest_public:
    task_queue.append(
        {
            "task_id": "closest-public-cleanup",
            "priority": 2,
            "lane": "public",
            "pool": closest_public.get("pool"),
            "venue": closest_public.get("venue"),
            "owner_mode": closest_public.get("owner_mode"),
            "blocker": closest_public.get("public_blocker"),
            "recommended_action": closest_public.get("public_next_action"),
            "why_now": "closest public-supported candidate is within the current CLMM public lane but still blocked by ATA/source cleanup rather than opportunity quality",
            "evidence": {
                "rank": closest_public.get("rank"),
                "required_inventory": closest_public.get("required_inventory"),
                "output_ata_needed": closest_public.get("output_ata_needed"),
                "source": closest_public.get("source"),
                "public_blocker_stage": closest_public.get("public_blocker_stage"),
                "public_conversion_score": closest_public.get("public_conversion_score"),
            },
        }
    )

unsupported_public_count = sum(1 for item in public_items if item["public_blocker"] == "wrong_venue")
if unsupported_public_count:
    task_queue.append(
        {
            "task_id": "public-venue-support-expansion",
            "priority": 3,
            "lane": "public",
            "pool": None,
            "venue": "multiple",
            "owner_mode": None,
            "blocker": "wrong_venue",
            "recommended_action": "decide_next_public_supported_venue",
            "why_now": "most HUNT surface is stranded outside the current CLMM-only public lane",
            "evidence": {
                "unsupported_public_candidate_count": unsupported_public_count,
                "top_stranded_venue_counts": dict(Counter(item["venue"] for item in public_items if item["public_blocker"] == "wrong_venue").most_common(5)),
            },
        }
    )

run51_summary = {
    "result_dir": str(RESULT_DIR),
    "summary": {
        "hunt_count": len(run49_candidates),
        "public_supported_venue_count": sum(1 for row in run49_candidates if (row.get("venue") or "") in PUBLIC_SUPPORTED_VENUES),
        "public_ready_count": sum(1 for row in run49_candidates if bool(row.get("public_ready"))),
        "tpu_ready_count": sum(1 for row in run49_candidates if bool(row.get("tpu_ready"))),
        "public_blocker_counts": dict(Counter(item["public_blocker"] for item in public_items)),
        "tpu_blocker_counts": dict(Counter(item["tpu_blocker"] for item in tpu_items)),
        "dominant_tpu_pool": closest_tpu.get("pool") if closest_tpu else None,
        "closest_public_pool": closest_public.get("pool") if closest_public else None,
        "recommended_next_step": "route_conversion_not_live_retry",
    },
    "engine_support_snapshot": {
        "public_supported_venues": sorted(PUBLIC_SUPPORTED_VENUES),
        "public_shape_sources": sorted(PUBLIC_SHAPE_SOURCES),
        "public_live_config_require_afxdp_source": True,
        "tpu_quic_public_reuses_jito_template_family": True,
        "route_doctrine": {
            "public_rpc": "proof_or_stepping_stone",
            "tpu_quic_public": "target_public_route",
            "jito": "observed_route_only_not_dependency",
        },
        "evidence": {
            "public_scout_shape_wrong_venue": "crates/shadow-tier0/src/money_worker.rs:3512",
            "public_scout_route_override": "crates/shadow-tier0/src/money_worker.rs:3893",
            "tpu_route_override": "crates/shadow-tier0/src/money_worker.rs:3603",
            "tpu_build_template_family": "crates/shadow-core/src/assembly.rs:738",
        },
    },
    "top_targets": {
        "overall": top_overall,
        "closest_public": closest_public,
        "closest_tpu": closest_tpu,
    },
}

run51_top_targets = {
    "summary": {
        "overall_top_count": min(25, len(run49_candidates)),
        "public_top_count": min(15, len(public_items)),
        "tpu_top_count": min(15, len(tpu_items)),
    },
    "overall_top_candidates": [compact_candidate(row) for row in run49_candidates[:25]],
    "public_top_candidates": public_items[:15],
    "tpu_top_candidates": tpu_items[:15],
}

run51_public_blockers = {
    "summary": {
        "candidate_count": len(public_items),
        "supported_venue_count": sum(1 for item in public_items if item["venue"] in PUBLIC_SUPPORTED_VENUES),
        "top_blockers": dict(Counter(item["public_blocker"] for item in public_items).most_common(10)),
        "closest_public_pool": closest_public.get("pool") if closest_public else None,
    },
    "items": public_items,
}

run51_tpu_blockers = {
    "summary": {
        "candidate_count": len(tpu_items),
        "afxdp_source_count": sum(1 for item in tpu_items if item["source"] == "afxdp"),
        "top_blockers": dict(Counter(item["tpu_blocker"] for item in tpu_items).most_common(10)),
        "dominant_tpu_pool": closest_tpu.get("pool") if closest_tpu else None,
    },
    "items": tpu_items,
}

run51_task_queue = {
    "summary": {
        "task_count": len(task_queue),
        "highest_priority_task": task_queue[0]["task_id"] if task_queue else None,
    },
    "tasks": task_queue,
}

RUN51_ROUTE_CONVERSION_SUMMARY_PATH.write_text(
    json.dumps(run51_summary, indent=2) + "\n",
    encoding="utf-8",
)
RUN51_TOP_ROUTE_CONVERSION_TARGETS_PATH.write_text(
    json.dumps(run51_top_targets, indent=2) + "\n",
    encoding="utf-8",
)
RUN51_PUBLIC_ROUTE_CONVERSION_BLOCKERS_PATH.write_text(
    json.dumps(run51_public_blockers, indent=2) + "\n",
    encoding="utf-8",
)
RUN51_TPU_ROUTE_CONVERSION_BLOCKERS_PATH.write_text(
    json.dumps(run51_tpu_blockers, indent=2) + "\n",
    encoding="utf-8",
)
RUN51_CONVERSION_TASK_QUEUE_PATH.write_text(
    json.dumps(run51_task_queue, indent=2) + "\n",
    encoding="utf-8",
)

print(f"run51_route_conversion_summary={RUN51_ROUTE_CONVERSION_SUMMARY_PATH}")
print(f"run51_top_route_conversion_targets={RUN51_TOP_ROUTE_CONVERSION_TARGETS_PATH}")
print(f"run51_public_route_conversion_blockers={RUN51_PUBLIC_ROUTE_CONVERSION_BLOCKERS_PATH}")
print(f"run51_tpu_route_conversion_blockers={RUN51_TPU_ROUTE_CONVERSION_BLOCKERS_PATH}")
print(f"run51_conversion_task_queue={RUN51_CONVERSION_TASK_QUEUE_PATH}")
print(f"hunt_count={len(run49_candidates)}")
print(f"closest_public_pool={closest_public.get('pool') if closest_public else ''}")
print(f"closest_public_blocker={closest_public.get('public_blocker') if closest_public else ''}")
print(f"dominant_tpu_pool={closest_tpu.get('pool') if closest_tpu else ''}")
print(f"dominant_tpu_blocker={closest_tpu.get('tpu_blocker') if closest_tpu else ''}")
PY

status=$?
if [[ $status -ne 0 ]]; then
    fail "Run51 route conversion generation failed"
    finish
    exit $status
fi

pass "Run51 route conversion artifacts generated"
finish
exit 0

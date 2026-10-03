#!/usr/bin/env python3

import argparse
import json
from pathlib import Path
from typing import Any, Optional

QUOTE_TRACE_PREFIX = "TRACE clmm_exact_quote_probe "


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build an offline CLMM quote-readiness report from a census selector "
            "report and optional shadow-state quote-probe traces."
        )
    )
    parser.add_argument(
        "--census-report",
        required=True,
        help="Path to run/clmm_census_report.py JSON output",
    )
    parser.add_argument(
        "--shadow-state-log",
        help="Optional shadow-state log containing TRACE clmm_exact_quote_probe lines",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=10,
        help="Number of ranked candidates to include (default: 10)",
    )
    parser.add_argument("--output", help="Optional JSON output path")
    return parser.parse_args()


def parse_fields(line: str, prefix: str) -> Optional[dict[str, str]]:
    if prefix not in line:
        return None
    payload = line.split(prefix, 1)[1].strip()
    fields: dict[str, str] = {}
    for token in payload.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        fields[key] = value
    return fields or None


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def load_quote_traces(path: Optional[Path]) -> dict[tuple[str, str, str], dict[str, str]]:
    if path is None:
        return {}
    if not path.exists():
        raise FileNotFoundError(f"shadow-state log not found: {path}")
    traces: dict[tuple[str, str, str], dict[str, str]] = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        fields = parse_fields(line, QUOTE_TRACE_PREFIX)
        if fields is None:
            continue
        pool = fields.get("pool")
        mint_in = fields.get("mint_in")
        mint_out = fields.get("mint_out")
        if not pool or not mint_in or not mint_out:
            continue
        traces[(pool, mint_in, mint_out)] = fields
    return traces


def choose_candidates(report: dict[str, Any]) -> list[dict[str, Any]]:
    for key in (
        "filtered_selection_candidates",
        "global_selection_candidates",
        "filtered_top_pairs",
        "global_top_pairs",
    ):
        candidates = report.get(key)
        if isinstance(candidates, list) and candidates:
            return candidates
    return []


def quote_publishable(candidate: dict[str, Any]) -> bool:
    selection = candidate.get("selection") or {}
    blocker_class = selection.get("blocker_class")
    if blocker_class in {"inventory", "policy_filter", "pre_process", "economics"}:
        return False
    blocker = candidate.get("next_blocker") or ""
    return not blocker.startswith("pre_process_")


def stage_reached(candidate: dict[str, Any]) -> str:
    selection = candidate.get("selection") or {}
    return selection.get("furthest_stage") or "unknown"


def inferred_inventory_gate_ok(candidate: dict[str, Any]) -> Optional[bool]:
    inventory = candidate.get("inventory_readiness")
    if not isinstance(inventory, dict) or not inventory:
        return None
    return bool(
        inventory.get("input_inventory_present")
        and inventory.get("input_initialized")
        and inventory.get("input_funded")
        and inventory.get("output_inventory_present")
        and inventory.get("output_initialized")
    )


def would_be_quote_request_publishable_if_filters_relaxed(
    candidate: dict[str, Any],
) -> Optional[bool]:
    if quote_publishable(candidate):
        return True
    blocker = candidate.get("next_blocker") or ""
    inventory_gate_ok = inferred_inventory_gate_ok(candidate)
    if blocker == "pre_process_proof_filter":
        return inventory_gate_ok
    if blocker == "pre_process_net_after_fee_floor":
        return False
    return None


def net_after_fee_estimate(candidate: dict[str, Any]) -> tuple[Optional[int], Optional[str]]:
    blocker = candidate.get("next_blocker") or ""
    if blocker == "pre_process_net_after_fee_floor":
        return (None, "below_floor")
    return (None, None)


def classify_from_trace(trace: dict[str, str]) -> tuple[str, Optional[str], str]:
    status = trace.get("status")
    if status == "quote_ready":
        return ("quote_ready", None, "compare_against_rpc_simulation")
    if status == "missing_amm_config":
        return ("missing_amm_config", "amm_config", "hydrate_amm_config")
    if status in {"missing_tick_arrays", "missing_tick_array_window"}:
        return ("missing_tick_array", "tick_array_window", "hydrate_tick_array_window")
    if status in {"missing_pool_quote_core", "pool_missing", "missing_reserves"}:
        return ("stale_state", "pool_quote_core", "refresh_shadow_state")
    if status == "missing_mint_in":
        return ("stale_state", "mint_state", "refresh_shadow_state")
    if status == "invalid_mint_pair":
        return ("stale_state", "mint_pair", "inspect_pair_mapping")
    if status == "quote_build_failed":
        return (
            "stale_state",
            trace.get("quote_error"),
            "inspect_quote_math_or_state_freshness",
        )
    return ("stale_state", status, "refresh_shadow_state")


def classify_candidate(
    candidate: dict[str, Any], trace: Optional[dict[str, str]]
) -> tuple[str, Optional[str], str]:
    if trace is not None:
        return classify_from_trace(trace)

    selection = candidate.get("selection") or {}
    blocker_class = selection.get("blocker_class")
    blocker = candidate.get("next_blocker") or ""
    suggested = selection.get("suggested_action") or "observe_only"

    if blocker_class == "inventory":
        return ("inventory_blocked", blocker, suggested)
    if blocker_class in {"policy_filter", "pre_process", "economics"} or blocker.startswith(
        "pre_process_"
    ):
        return ("policy_blocked", blocker, suggested)

    quote_statuses = candidate.get("quote_request_statuses") or {}
    if any(status in quote_statuses for status in ("quote_matched", "matched", "fresh_match", "ok")):
        return ("quote_ready", None, "compare_against_rpc_simulation")
    if "quote_wait_timeout" in quote_statuses or blocker == "quote_request_quote_wait_timeout":
        return ("stale_state", "quote_wait_timeout", "refresh_shadow_state")
    if blocker.startswith("quote_request_"):
        return ("stale_state", blocker, "debug_quote_path")

    process_outcomes = candidate.get("process_outcomes") or {}
    if process_outcomes.get("ok", 0) > 0:
        return ("quote_ready", None, "compare_against_rpc_simulation")

    return ("stale_state", blocker or None, "refresh_shadow_state")


def audit_classification(
    candidate: dict[str, Any], quote_readiness: str, missing_state_component: Optional[str]
) -> str:
    blocker = candidate.get("next_blocker") or ""
    if blocker == "pre_process_proof_filter":
        return "proof_filter_blocked"
    if blocker == "pre_process_net_after_fee_floor":
        return "net_after_fee_floor_blocked"
    if quote_readiness == "inventory_blocked":
        return "missing_inventory"
    if quote_readiness in {"missing_amm_config", "missing_tick_array"}:
        return "missing_exact_quote_state"
    if quote_readiness == "stale_state":
        return "stale_state"
    if quote_publishable(candidate):
        return "policy_pass_candidate"
    if missing_state_component:
        return missing_state_component
    return "policy_blocked"


def selector_bucket(
    candidate: dict[str, Any],
    classification: str,
    quote_readiness: str,
) -> str:
    if classification == "net_after_fee_floor_blocked":
        return "DENY"
    if classification == "policy_pass_candidate":
        return "HUNT"
    if classification == "proof_filter_blocked":
        return "WATCH"
    if classification in {
        "missing_inventory",
        "missing_exact_quote_state",
        "stale_state",
    }:
        return "WATCH"
    if quote_readiness == "quote_ready":
        return "HUNT"
    return "WATCH"


def build_candidate_report(
    candidate: dict[str, Any], rank: int, trace: Optional[dict[str, str]]
) -> dict[str, Any]:
    readiness, missing_state_component, next_action = classify_candidate(candidate, trace)
    selection = candidate.get("selection") or {}
    inventory_readiness = candidate.get("inventory_readiness") or {}
    classification = audit_classification(candidate, readiness, missing_state_component)
    bucket = selector_bucket(candidate, classification, readiness)
    relaxed_publishable = would_be_quote_request_publishable_if_filters_relaxed(candidate)
    net_after_fee_lamports, net_after_fee_signal = net_after_fee_estimate(candidate)
    return {
        "rank": rank,
        "pair": f"{candidate.get('mint_in')} -> {candidate.get('mint_out')}",
        "pool": candidate.get("pool"),
        "mint_in": candidate.get("mint_in"),
        "mint_out": candidate.get("mint_out"),
        "route_count": candidate.get("route_count", 0),
        "actionable_route_count": candidate.get("actionable_route_count", 0),
        "stage_reached": stage_reached(candidate),
        "audit_classification": classification,
        "selector_bucket": bucket,
        "quote_request_publishable": quote_publishable(candidate),
        "would_be_quote_request_publishable_if_filters_relaxed": relaxed_publishable,
        "quote_readiness": readiness,
        "missing_state_component": missing_state_component,
        "recommended_next_action": next_action,
        "next_blocker": candidate.get("next_blocker"),
        "selection_score": selection.get("selection_score"),
        "furthest_stage": selection.get("furthest_stage"),
        "blocker_class": selection.get("blocker_class"),
        "net_after_fee_estimate_lamports": net_after_fee_lamports,
        "net_after_fee_estimate_signal": net_after_fee_signal,
        "proof_ready_basic": selection.get("proof_ready_basic"),
        "input_funded": selection.get("input_funded"),
        "inventory_readiness": inventory_readiness,
        "shadow_state_probe": trace,
    }


def main() -> int:
    args = parse_args()
    census_report_path = Path(args.census_report)
    shadow_state_log_path = Path(args.shadow_state_log) if args.shadow_state_log else None
    output_path = Path(args.output) if args.output else None

    if not census_report_path.exists():
        raise FileNotFoundError(f"census report not found: {census_report_path}")

    report = load_json(census_report_path)
    candidates = choose_candidates(report)
    traces = load_quote_traces(shadow_state_log_path)

    items = []
    for rank, candidate in enumerate(candidates[: args.top], start=1):
        key = (candidate.get("pool"), candidate.get("mint_in"), candidate.get("mint_out"))
        trace = traces.get(key) if None not in key else None
        items.append(build_candidate_report(candidate, rank, trace))

    audit_summary = {
        "selector_buckets": {},
        "audit_classifications": {},
        "stage_reached": {},
    }
    for item in items:
        audit_summary["selector_buckets"][item["selector_bucket"]] = (
            audit_summary["selector_buckets"].get(item["selector_bucket"], 0) + 1
        )
        audit_summary["audit_classifications"][item["audit_classification"]] = (
            audit_summary["audit_classifications"].get(item["audit_classification"], 0) + 1
        )
        audit_summary["stage_reached"][item["stage_reached"]] = (
            audit_summary["stage_reached"].get(item["stage_reached"], 0) + 1
        )

    payload = {
        "census_report": str(census_report_path),
        "shadow_state_log": str(shadow_state_log_path) if shadow_state_log_path else None,
        "candidate_count": len(items),
        "audit_summary": audit_summary,
        "items": items,
    }

    if output_path is not None:
        output_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    print("CLMM policy audit")
    print(f"census_report={census_report_path}")
    if shadow_state_log_path is not None:
        print(f"shadow_state_log={shadow_state_log_path}")
    print(f"candidate_count={len(items)}")
    if audit_summary["selector_buckets"]:
        print(f"selector_buckets={audit_summary['selector_buckets']}")
    for item in items:
        print(
            "  "
            f"rank={item['rank']} pool={item['pool']} mint_in={item['mint_in']} "
            f"mint_out={item['mint_out']} bucket={item['selector_bucket']} "
            f"classification={item['audit_classification']} stage={item['stage_reached']} "
            f"quote_readiness={item['quote_readiness']} "
            f"relaxed_publishable={item['would_be_quote_request_publishable_if_filters_relaxed']} "
            f"recommended_next_action={item['recommended_next_action']}"
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

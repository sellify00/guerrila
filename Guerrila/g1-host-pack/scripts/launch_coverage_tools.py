#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
METEORA_DAMM_V2_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"
WSOL_MINT = "So11111111111111111111111111111111111111112"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value is None or value == "":
            return default
        return int(float(value))
    except Exception:
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value is None or value == "":
            return default
        return float(value)
    except Exception:
        return default


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            doc = json.loads(line)
        except Exception:
            continue
        if isinstance(doc, dict):
            rows.append(doc)
    return rows


def write_summary(summary: dict[str, Any], json_path: Path, txt_path: Path) -> None:
    json_path.parent.mkdir(parents=True, exist_ok=True)
    txt_path.parent.mkdir(parents=True, exist_ok=True)
    json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, (dict, list)):
            value_s = json.dumps(value, sort_keys=True)
        else:
            value_s = str(value).lower() if isinstance(value, bool) else str(value)
        lines.append(f"{key}={value_s}")
    txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def pct(numerator: int, denominator: int) -> float:
    return round((numerator / denominator) * 100.0, 4) if denominator > 0 else 0.0


def launch_group_count(rows: list[dict[str, Any]]) -> int:
    return len({text(row.get("launch_group_id")) for row in rows if text(row.get("launch_group_id"))})


def unique_pool_count(rows: list[dict[str, Any]], venue: str = "") -> int:
    pools = set()
    for row in rows:
        if venue and text(row.get("venue")) != venue:
            continue
        pool = text(row.get("pool"))
        if pool:
            pools.add(pool)
    return len(pools)


def counter_from_rows(rows: list[dict[str, Any]], field: str) -> dict[str, int]:
    out: Counter[str] = Counter()
    for row in rows:
        value = row.get(field)
        if isinstance(value, list):
            for item in value:
                item_s = text(item)
                if item_s:
                    out[item_s] += 1
        else:
            value_s = text(value)
            if value_s:
                out[value_s] += 1
    return dict(out)


def program_counts_from_rows(rows: list[dict[str, Any]]) -> Counter[str]:
    counts: Counter[str] = Counter()
    for row in rows:
        programs = row.get("target_program_ids")
        if not isinstance(programs, list):
            programs = [row.get("target_program_id")]
        seen: set[str] = set()
        for program in programs:
            program_s = text(program)
            if program_s and program_s not in seen:
                seen.add(program_s)
                counts[program_s] += 1
    return counts


def decoded_launch_mints_from_rows(rows: list[dict[str, Any]]) -> set[str]:
    """Return mints that were already classified by an upstream launch decoder.

    Raw LaserStream rows expose tx bytes/program IDs/account keys. They do not,
    by themselves, prove a unique launch birth. Keep this strict so realtime
    coverage cannot accidentally report generic Pump/PumpSwap activity as
    decoded launch count.
    """
    mints: set[str] = set()
    mint_keys = (
        "launch_mint",
        "candidate_mint",
        "mint",
        "target_mint",
        "pool_mint",
        "base_mint",
        "quote_mint",
    )
    for row in rows:
        for key in mint_keys:
            mint = text(row.get(key))
            if mint and mint != WSOL_MINT:
                mints.add(mint)
    return mints


def chain_estimate_count(window_seconds: int, estimate_per_hour: float) -> int:
    if estimate_per_hour <= 0 or window_seconds <= 0:
        return 0
    return int(round(estimate_per_hour * (window_seconds / 3600.0)))


def build_census_summary(args: argparse.Namespace) -> dict[str, Any]:
    ls13a_summary = load_json(args.ls13a_summary)
    launch0_summary = load_json(args.launch0_summary)
    hot_summary = load_json(args.hot_summary)
    ls13a_rows = load_jsonl(args.ls13a_rows)
    launch0_rows = load_jsonl(args.launch0_rows)

    window_seconds = safe_int(args.window_seconds)
    estimate_per_hour = safe_float(args.chain_estimate_per_hour)
    chain_estimated = chain_estimate_count(window_seconds, estimate_per_hour)

    program_counts = dict(ls13a_summary.get("program_id_counts") or {})
    recognized_venue_counts = dict(ls13a_summary.get("recognized_venue_counts") or {})
    decision_counts = dict(hot_summary.get("decision_counts") or {})

    helius_raw = safe_int(ls13a_summary.get("preprocessed_raw_tx_seen_count"))
    helius_bytes = safe_int(ls13a_summary.get("preprocessed_tx_bytes_available_count"))
    decoded_launches = safe_int(launch0_summary.get("launch_seen_count")) or launch_group_count(launch0_rows)
    pump_program_seen = safe_int(program_counts.get(PUMPFUN_AMM_PROGRAM_ID))
    meteora_program_seen = safe_int(program_counts.get(METEORA_DAMM_V2_PROGRAM_ID))
    hot_launches_seen = safe_int(hot_summary.get("eligible_pump_rows"))
    trigger_b_rows = safe_int(hot_summary.get("trigger_b_pass_rows"))
    entry_quality_evaluated = sum(
        count
        for decision, count in decision_counts.items()
        if text(decision).startswith("HOT_BLOCK_ENTRY_QUALITY") or text(decision).startswith("HOT_BLOCK_ENTRY_EDGE")
    )
    exit_liquidity_evaluated = sum(
        count
        for decision, count in decision_counts.items()
        if "EXIT" in text(decision) or "LIQUIDITY" in text(decision)
    )
    live_candidates = (
        safe_int(hot_summary.get("hot_ready_ordered_bundle_rows"))
        + safe_int(hot_summary.get("hot_ready_late_follow_rows"))
        + safe_int(hot_summary.get("attempt_count"))
    )

    return {
        "status": "launch0g_global_launch_birth_census_complete",
        "run_root": text(args.run_root),
        "window_seconds": window_seconds,
        "chain_estimated_launch_count": chain_estimated,
        "chain_estimate_per_hour": estimate_per_hour,
        "chain_estimate_basis": "env_or_operator_default_not_chain_truth" if chain_estimated else "not_configured",
        "helius_raw_create_count": helius_raw,
        "helius_preprocessed_create_count": helius_bytes,
        "helius_preprocessed_decodable_tx_count": safe_int(ls13a_summary.get("preprocessed_decodable_tx_count")),
        "helius_preprocessed_supported_venue_count": safe_int(ls13a_summary.get("preprocessed_supported_venue_count")),
        "pump_create_count": pump_program_seen,
        "pump_create_count_basis": "pump_amm_program_rows_seen_not_instruction_create",
        "pumpswap_pool_count": unique_pool_count(launch0_rows, "pumpfun_amm"),
        "migration_pool_count": meteora_program_seen,
        "migration_pool_count_basis": "meteora_damm_v2_program_rows_seen",
        "decoded_launch_count": decoded_launches,
        "launch_state_created_count": hot_launches_seen or decoded_launches,
        "trigger_b_evaluated_count": trigger_b_rows or safe_int(launch0_summary.get("first_buy_seen_count")),
        "entry_quality_evaluated_count": entry_quality_evaluated,
        "exit_liquidity_evaluated_count": exit_liquidity_evaluated,
        "live_attempt_candidate_count": live_candidates,
        "global_launches_seen": decoded_launches,
        "hot_executor_launches_seen": hot_launches_seen,
        "coverage_ratio": pct(decoded_launches, chain_estimated),
        "hot_vs_decoded_coverage_ratio": pct(hot_launches_seen, decoded_launches),
        "raw_source_vs_chain_estimate_ratio": pct(helius_raw, chain_estimated),
        "program_id_counts": program_counts,
        "recognized_venue_counts": recognized_venue_counts,
        "launch0_direction_counts": dict(launch0_summary.get("direction_counts") or {}),
        "hot_decision_counts": decision_counts,
        "source_paths": {
            "ls13a_summary": text(args.ls13a_summary),
            "ls13a_rows": text(args.ls13a_rows),
            "launch0_summary": text(args.launch0_summary),
            "launch0_rows": text(args.launch0_rows),
            "hot_summary": text(args.hot_summary) if args.hot_summary else "",
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def build_gap_summary(args: argparse.Namespace) -> dict[str, Any]:
    census = load_json(args.census_summary)
    ls13a_summary = load_json(args.ls13a_summary)
    launch0_summary = load_json(args.launch0_summary)
    hot_summary = load_json(args.hot_summary)
    launch0_rows = load_jsonl(args.launch0_rows)
    hot_decision_rows = load_jsonl(args.hot_decision_rows)

    chain_estimated = safe_int(census.get("chain_estimated_launch_count"))
    helius_raw = safe_int(census.get("helius_raw_create_count"))
    decoded = safe_int(census.get("decoded_launch_count"))
    hot_seen = safe_int(census.get("hot_executor_launches_seen"))

    decision_counts = Counter(dict(hot_summary.get("decision_counts") or {}))
    for row in hot_decision_rows:
        decision = text(row.get("decision"))
        if decision:
            decision_counts[decision] += 1

    unsupported_program_count = max(
        0,
        safe_int(ls13a_summary.get("preprocessed_raw_tx_seen_count"))
        - safe_int(ls13a_summary.get("preprocessed_supported_venue_count")),
    )
    decode_failed_count = max(
        0,
        safe_int(ls13a_summary.get("preprocessed_tx_bytes_available_count"))
        - safe_int(ls13a_summary.get("preprocessed_decodable_tx_count")),
    ) + safe_int(decision_counts.get("PARK_TX_DECODE_FAILED"))
    no_pool_join = sum(1 for row in launch0_rows if text(row.get("pair_class")) != "core_launch_pair")
    no_trade_history = sum(1 for row in launch0_rows if text(row.get("direction_class")) == "unsupported_direction")
    alt_unresolved = safe_int(decision_counts.get("PARK_ALT_UNSUPPORTED_HOT"))
    filter_too_narrow = safe_int(decision_counts.get("PARK_NOT_PUMPFUN"))
    evaluated = safe_int(hot_summary.get("trigger_b_pass_rows")) or safe_int(launch0_summary.get("first_buy_seen_count"))

    buckets = {
        "MISSED_NOT_IN_SOURCE": max(0, chain_estimated - helius_raw) if chain_estimated else 0,
        "MISSED_UNSUPPORTED_PROGRAM": unsupported_program_count,
        "MISSED_DECODE_FAILED": decode_failed_count,
        "MISSED_NO_POOL_JOIN": no_pool_join + safe_int(decision_counts.get("PARK_NO_CORE_LAUNCH_EVENT")),
        "MISSED_NO_TRADE_HISTORY": no_trade_history,
        "MISSED_ALT_UNRESOLVED": alt_unresolved,
        "MISSED_FILTER_TOO_NARROW": filter_too_narrow + max(0, decoded - hot_seen),
        "EVALUATED": evaluated,
    }
    top_miss_reason = "none"
    miss_only = {k: v for k, v in buckets.items() if k != "EVALUATED"}
    if miss_only:
        top_miss_reason = max(miss_only.items(), key=lambda item: item[1])[0]

    return {
        "status": "launch0i_launch_decode_gap_report_complete",
        "run_root": text(args.run_root),
        "chain_estimated_launch_count": chain_estimated,
        "helius_raw_create_count": helius_raw,
        "decoded_launch_count": decoded,
        "hot_executor_launches_seen": hot_seen,
        "coverage_ratio": census.get("coverage_ratio", 0),
        "miss_bucket_counts": buckets,
        "top_miss_reason": top_miss_reason,
        "decision_counts": dict(decision_counts),
        "ls13a_blocker_counts": dict(ls13a_summary.get("blocker_counts") or {}),
        "launch0_direction_counts": dict(launch0_summary.get("direction_counts") or {}),
        "wire_send": 0,
        "live_landed": 0,
    }


def build_reconcile_summary(args: argparse.Namespace) -> dict[str, Any]:
    census = load_json(args.census_summary)
    hot_summary = load_json(args.hot_summary)
    gap_summary = load_json(args.gap_summary)
    quote_state_summary = load_json(args.quote_state_summary)
    entry_quality_summary = load_json(args.entry_quality_summary)
    decoded = safe_int(census.get("decoded_launch_count") or census.get("unique_launch_states") or census.get("unique_births_decoded"))
    hot_seen = safe_int(hot_summary.get("eligible_pump_rows"))
    trigger_b = safe_int(hot_summary.get("trigger_b_pass_rows"))
    entry_ready = safe_int(hot_summary.get("attempt_count")) + safe_int(hot_summary.get("hot_ready_late_follow_rows")) + safe_int(hot_summary.get("hot_ready_ordered_bundle_rows"))
    chain_estimated = safe_int(census.get("chain_estimated_launch_count"))
    coverage_ratio = safe_float(census.get("coverage_ratio")) or pct(decoded, chain_estimated)
    status = "COVERAGE_REPAIR_REQUIRED" if coverage_ratio and coverage_ratio < safe_float(args.min_coverage_pct, 20.0) else "COVERAGE_RECONCILED"
    census_status = text(census.get("status"))
    if census_status == "launch0d_realtime_birth_decoder_complete":
        top_miss_reason = text(census.get("top_decode_blocker")) or text(gap_summary.get("top_miss_reason")) or "unknown"
    else:
        top_miss_reason = text(gap_summary.get("top_miss_reason")) or text(census.get("top_decode_blocker")) or "unknown"

    return {
        "status": status,
        "global_launches_seen": safe_int(census.get("global_launches_seen") or census.get("unique_launch_states") or census.get("unique_births_decoded")),
        "decoded_launches": decoded,
        "unique_launch_states": safe_int(census.get("unique_launch_states")),
        "unique_births_decoded": safe_int(census.get("unique_births_decoded")),
        "unique_mints_seen": safe_int(census.get("unique_mints_seen")),
        "true_create_signature_births": safe_int(census.get("true_create_signature_births")),
        "trade_derived_states": safe_int(census.get("trade_derived_states")),
        "pool_derived_states": safe_int(census.get("pool_derived_states")),
        "migration_derived_states": safe_int(census.get("migration_derived_states")),
        "unproven_observed_states": safe_int(census.get("unproven_observed_states")),
        "source_provenance_counts": census.get("source_provenance_counts") or {},
        "observed_state_not_create_proven_count": safe_int(census.get("observed_state_not_create_proven_count")),
        "exact_birth_decode_ready": bool(census.get("exact_birth_decode_ready")),
        "first_buy_state_count": safe_int(census.get("first_buy_state_count")),
        "first_sell_state_count": safe_int(census.get("first_sell_state_count")),
        "pump_curve_births": safe_int(census.get("pump_curve_births")),
        "pumpswap_births": safe_int(census.get("pumpswap_births")),
        "migration_births": safe_int(census.get("migration_births")),
        "hot_executor_launches_seen": hot_seen,
        "quote_state_ready_rows": safe_int(quote_state_summary.get("quote_state_ready_rows")),
        "exit_liquidity_guard_ready_rows": safe_int(quote_state_summary.get("exit_liquidity_guard_ready_rows")),
        "top_quote_state_blocker": text(quote_state_summary.get("top_quote_state_blocker")) or "none",
        "quote_state_blocker_counts": quote_state_summary.get("quote_state_blocker_counts") or {},
        "trigger_b_rows": trigger_b or safe_int(quote_state_summary.get("trigger_b_rows")) or safe_int(census.get("trigger_b_rows")),
        "entry_quality_evaluated_rows": safe_int(entry_quality_summary.get("entry_quality_evaluated_rows")) or safe_int(quote_state_summary.get("entry_quality_evaluated_rows")) or safe_int(census.get("entry_quality_evaluated_rows")),
        "entry_quality_ready_rows": safe_int(entry_quality_summary.get("entry_quality_ready_rows")) or (safe_int(hot_summary.get("latest_entry_quality_ready")) + safe_int(hot_summary.get("attempt_count")) + safe_int(census.get("entry_quality_ready_rows"))),
        "entry_quality_missing_required_field_rows": safe_int(census.get("entry_quality_missing_required_field_rows")),
        "top_entry_quality_blocker": text(entry_quality_summary.get("top_entry_quality_blocker")) or "none",
        "entry_quality_blocker_counts": entry_quality_summary.get("entry_quality_blocker_counts") or census.get("entry_quality_blocker_counts") or {},
        "live_attempt_candidate_count": entry_ready,
        "coverage_ratio": coverage_ratio,
        "hot_vs_decoded_coverage_ratio": pct(hot_seen, decoded),
        "top_miss_reason": top_miss_reason,
        "top_decode_blocker": text(census.get("top_decode_blocker")) or "unknown",
        "miss_bucket_counts": gap_summary.get("miss_bucket_counts") or {},
        "first_blocker_counts": census.get("first_blocker_counts") or {},
        "source_paths": {
            "census_summary": text(args.census_summary),
            "hot_summary": text(args.hot_summary),
            "gap_summary": text(args.gap_summary),
            "quote_state_summary": text(args.quote_state_summary),
            "entry_quality_summary": text(args.entry_quality_summary),
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def build_realtime_summary(args: argparse.Namespace) -> dict[str, Any]:
    stream_summary = load_json(args.stream_summary)
    stream_rows = load_jsonl(args.stream_rows)
    hot_summary = load_json(args.hot_summary)
    requested = safe_int(args.requested_duration_seconds)
    actual = safe_int(args.actual_elapsed_seconds)
    min_required = int(math_floor(requested * 0.95))
    programs = program_counts_from_rows(stream_rows)
    decoded_launch_mints = decoded_launch_mints_from_rows(stream_rows)
    unique_signatures = len({text(row.get("target_signature")) for row in stream_rows if text(row.get("target_signature"))})
    pump_program_rows = safe_int(programs.get(PUMP_BONDING_CURVE_PROGRAM_ID))
    pumpswap_program_rows = safe_int(programs.get(PUMPFUN_AMM_PROGRAM_ID))
    migration_program_rows = safe_int(programs.get(METEORA_DAMM_V2_PROGRAM_ID))

    return {
        "status": "REALTIME_CENSUS" if actual >= min_required else "REALTIME_CENSUS_DURATION_SHORT",
        "census_mode": "REALTIME_CENSUS",
        "run_root": text(args.run_root),
        "requested_duration_seconds": requested,
        "actual_elapsed_seconds": actual,
        "duration_acceptance_min_seconds": min_required,
        "duration_acceptance_pass": actual >= min_required,
        "wall_clock_start": text(args.wall_clock_start),
        "wall_clock_end": text(args.wall_clock_end),
        "laserstream_stream_status": text(stream_summary.get("status")),
        "laserstream_stream_error": stream_summary.get("stream_error"),
        "stream_rows_seen": len(stream_rows),
        "unique_target_signatures": unique_signatures or safe_int(stream_summary.get("unique_target_signatures")),
        "pump_create_seen": pump_program_rows,
        "pumpswap_create_seen": pumpswap_program_rows,
        "migration_seen": migration_program_rows,
        "pump_create_seen_basis": "program_row_count_not_unique_birth",
        "pumpswap_create_seen_basis": "program_row_count_not_unique_birth",
        "migration_seen_basis": "program_row_count_not_unique_birth",
        "launch_program_activity_rows": pump_program_rows + pumpswap_program_rows,
        "decoded_launches": len(decoded_launch_mints),
        "decoded_launches_basis": "decoded_unique_mint_count",
        "launch_decode_available": bool(decoded_launch_mints),
        "unique_launch_mints": len(decoded_launch_mints),
        "hot_executor_seen": safe_int(hot_summary.get("eligible_pump_rows")),
        "hot_trigger_b_rows": safe_int(hot_summary.get("trigger_b_pass_rows")),
        "program_id_counts": dict(programs),
        "source_paths": {
            "stream_summary": text(args.stream_summary),
            "stream_rows": text(args.stream_rows),
            "hot_summary": text(args.hot_summary) if args.hot_summary else "",
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def math_floor(value: float) -> int:
    return int(value // 1)


def build_source_matrix(args: argparse.Namespace) -> dict[str, Any]:
    realtime = load_json(args.realtime_summary)
    realtime_decoder = load_json(args.realtime_decoder_summary)
    census = load_json(args.census_summary)
    hot_summary = load_json(args.hot_summary)
    processed_summary = load_json(args.processed_summary)
    enhanced_summary = load_json(args.enhanced_summary)

    rows: list[dict[str, Any]] = []

    def add_row(name: str, create_count: int, decoded_count: int, unique_mints: int, path: str, status: str = "observed") -> None:
        overlap = min(create_count, safe_int(hot_summary.get("eligible_pump_rows"))) if create_count > 0 else 0
        rows.append(
            {
                "source_name": name,
                "status": status,
                "create_count": create_count,
                "decoded_count": decoded_count,
                "unique_mints": unique_mints,
                "overlap_with_ground_truth": overlap,
                "missed_count": max(0, create_count - decoded_count),
                "miss_reason": "decode_or_filter_gap" if create_count > decoded_count else "none",
                "source_path": path,
            }
        )

    add_row(
        "LaserStream preprocessed realtime",
        safe_int(realtime.get("launch_program_activity_rows")) or safe_int(realtime.get("stream_rows_seen")),
        safe_int(realtime.get("decoded_launches")),
        safe_int(realtime.get("unique_launch_mints") or realtime.get("decoded_launches")),
        text(args.realtime_summary),
        "observed" if realtime else "missing",
    )
    add_row(
        "LaserStream realtime birth decoder",
        safe_int(realtime_decoder.get("decoded_activity_rows")) or safe_int(realtime_decoder.get("activity_rows_seen")),
        safe_int(realtime_decoder.get("unique_launch_states") or realtime_decoder.get("unique_births_decoded") or realtime_decoder.get("decoded_launch_count")),
        safe_int(realtime_decoder.get("unique_mints_seen")),
        text(args.realtime_decoder_summary),
        "observed" if realtime_decoder else "missing",
    )
    add_row(
        "LaserStream preprocessed backfill",
        safe_int(census.get("helius_raw_create_count")),
        safe_int(census.get("decoded_launch_count")),
        safe_int(census.get("decoded_launch_count")),
        text(args.census_summary),
        "observed" if census else "missing",
    )
    add_row(
        "hot executor stream",
        safe_int(hot_summary.get("stream_rows_seen")),
        safe_int(hot_summary.get("eligible_pump_rows")),
        safe_int(hot_summary.get("eligible_pump_rows")),
        text(args.hot_summary),
        "observed" if hot_summary else "missing",
    )
    add_row(
        "Helius processed stream",
        safe_int(processed_summary.get("create_count") or processed_summary.get("decoded_launches")),
        safe_int(processed_summary.get("decoded_count") or processed_summary.get("decoded_launches")),
        safe_int(processed_summary.get("unique_mints")),
        text(args.processed_summary),
        "observed" if processed_summary else "not_configured",
    )
    add_row(
        "Helius raw/enhanced create history",
        safe_int(enhanced_summary.get("create_count") or enhanced_summary.get("decoded_launches")),
        safe_int(enhanced_summary.get("decoded_count") or enhanced_summary.get("decoded_launches")),
        safe_int(enhanced_summary.get("unique_mints")),
        text(args.enhanced_summary),
        "observed" if enhanced_summary else "not_configured",
    )

    best = max(rows, key=lambda row: safe_int(row.get("decoded_count")), default={})
    return {
        "status": "launch0s_source_breadth_matrix_complete",
        "source_count": len(rows),
        "best_source_name": text(best.get("source_name")),
        "best_source_decoded_count": safe_int(best.get("decoded_count")),
        "matrix_rows": rows,
        "wire_send": 0,
        "live_landed": 0,
    }


def command_census(args: argparse.Namespace) -> int:
    write_summary(build_census_summary(args), args.summary_json, args.summary_txt)
    return 0


def command_gap(args: argparse.Namespace) -> int:
    write_summary(build_gap_summary(args), args.summary_json, args.summary_txt)
    return 0


def command_reconcile(args: argparse.Namespace) -> int:
    write_summary(build_reconcile_summary(args), args.summary_json, args.summary_txt)
    return 0


def command_realtime(args: argparse.Namespace) -> int:
    write_summary(build_realtime_summary(args), args.summary_json, args.summary_txt)
    return 0


def command_source_matrix(args: argparse.Namespace) -> int:
    write_summary(build_source_matrix(args), args.summary_json, args.summary_txt)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("census")
    p.add_argument("--run-root", type=Path, required=True)
    p.add_argument("--window-seconds", required=True)
    p.add_argument("--chain-estimate-per-hour", default="1000")
    p.add_argument("--ls13a-summary", type=Path, required=True)
    p.add_argument("--ls13a-rows", type=Path, required=True)
    p.add_argument("--launch0-summary", type=Path, required=True)
    p.add_argument("--launch0-rows", type=Path, required=True)
    p.add_argument("--hot-summary", type=Path)
    p.add_argument("--summary-json", type=Path, required=True)
    p.add_argument("--summary-txt", type=Path, required=True)
    p.set_defaults(func=command_census)

    p = sub.add_parser("gap")
    p.add_argument("--run-root", type=Path, required=True)
    p.add_argument("--census-summary", type=Path, required=True)
    p.add_argument("--ls13a-summary", type=Path, required=True)
    p.add_argument("--launch0-summary", type=Path, required=True)
    p.add_argument("--launch0-rows", type=Path, required=True)
    p.add_argument("--hot-summary", type=Path)
    p.add_argument("--hot-decision-rows", type=Path)
    p.add_argument("--summary-json", type=Path, required=True)
    p.add_argument("--summary-txt", type=Path, required=True)
    p.set_defaults(func=command_gap)

    p = sub.add_parser("reconcile")
    p.add_argument("--census-summary", type=Path, required=True)
    p.add_argument("--hot-summary", type=Path)
    p.add_argument("--gap-summary", type=Path)
    p.add_argument("--quote-state-summary", type=Path)
    p.add_argument("--entry-quality-summary", type=Path)
    p.add_argument("--min-coverage-pct", default="20")
    p.add_argument("--summary-json", type=Path, required=True)
    p.add_argument("--summary-txt", type=Path, required=True)
    p.set_defaults(func=command_reconcile)

    p = sub.add_parser("realtime")
    p.add_argument("--run-root", type=Path, required=True)
    p.add_argument("--requested-duration-seconds", required=True)
    p.add_argument("--actual-elapsed-seconds", required=True)
    p.add_argument("--wall-clock-start", required=True)
    p.add_argument("--wall-clock-end", required=True)
    p.add_argument("--stream-summary", type=Path, required=True)
    p.add_argument("--stream-rows", type=Path, required=True)
    p.add_argument("--hot-summary", type=Path)
    p.add_argument("--summary-json", type=Path, required=True)
    p.add_argument("--summary-txt", type=Path, required=True)
    p.set_defaults(func=command_realtime)

    p = sub.add_parser("source-matrix")
    p.add_argument("--realtime-summary", type=Path)
    p.add_argument("--realtime-decoder-summary", type=Path)
    p.add_argument("--census-summary", type=Path)
    p.add_argument("--hot-summary", type=Path)
    p.add_argument("--processed-summary", type=Path)
    p.add_argument("--enhanced-summary", type=Path)
    p.add_argument("--summary-json", type=Path, required=True)
    p.add_argument("--summary-txt", type=Path, required=True)
    p.set_defaults(func=command_source_matrix)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())

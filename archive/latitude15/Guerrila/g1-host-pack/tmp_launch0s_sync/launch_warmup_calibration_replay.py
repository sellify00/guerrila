#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
from typing import Any


BUCKETS: list[tuple[str, int, int | None]] = [
    ("0-1s", 0, 1_000),
    ("1-2s", 1_000, 2_000),
    ("2-5s", 2_000, 5_000),
    ("5-10s", 5_000, 10_000),
    ("10-30s", 10_000, 30_000),
    ("30-60s", 30_000, 60_000),
    ("1-5m", 60_000, 300_000),
    ("5-15m", 300_000, 900_000),
    ("15-60m", 900_000, 3_600_000),
    ("60m+", 3_600_000, None),
]

BASE_RESERVE_BLOCKERS = {"BASE_RESERVE_BELOW_FLOOR", "BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR"}
PRICE_IMPACT_BLOCKERS = {"PRICE_IMPACT_TOO_HIGH", "BONDING_CURVE_PRICE_IMPACT_TOO_HIGH"}
EXIT_ZERO_BLOCKERS = {"EXIT_QUOTE_ZERO", "BONDING_CURVE_EXIT_QUOTE_ZERO"}
STATE_MISSING_BLOCKERS = {
    "QUOTE_MINT_MISSING",
    "QUOTE_POOL_MISSING",
    "ENTRY_QUOTE_MISSING",
    "BONDING_CURVE_STATE_MISSING",
    "BONDING_CURVE_RESERVES_MISSING",
    "BONDING_CURVE_PARSE_FAILED",
}
MIGRATION_BLOCKERS = {"BONDING_CURVE_COMPLETE_OR_MIGRATED"}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            rows.append(value)
    return rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def bucket_for_age(age_ms: int) -> str:
    if age_ms < 0:
        return "unknown"
    for label, start, end in BUCKETS:
        if age_ms >= start and (end is None or age_ms < end):
            return label
    return "unknown"


def bucket_min_age_ms(label: str) -> int:
    for bucket_label, start, _end in BUCKETS:
        if bucket_label == label:
            return start
    return -1


def evaluate_entry_quality(row: dict[str, Any], min_price_change_bps: int, max_sell_count_2s: int, panic_drop_2s_bps: int) -> tuple[str, str]:
    if safe_int(row.get("entry_quality_baseline_expected_out_raw")) <= 0:
        return "HOT_BLOCK_ENTRY_QUALITY_WARMUP", "entry_quality_warmup"
    if text(row.get("last_trade_side")) != "buy":
        return "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE", "entry_quality_last_trade_not_buy"
    if (
        safe_int(row.get("sell_count_2s")) > max_sell_count_2s
        or safe_int(row.get("net_buy_pressure_2s")) <= 0
        or boolish(row.get("sell_pressure_seen"))
        or boolish(row.get("creator_sell_seen"))
    ):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_sell_pressure"
    if safe_int(row.get("panic_drop_2s_bps")) >= max(1, panic_drop_2s_bps):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_panic_drop_2s"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < min_price_change_bps:
        return "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP", "entry_quality_price_not_up"
    return "ENTRY_QUALITY_READY", "none"


def evaluate_entry_edge(
    row: dict[str, Any],
    quality_ready: bool,
    edge_min_price_change_bps: int,
    edge_max_price_change_bps: int,
    edge_min_net_buy_pressure_5s: int,
    max_sell_count_2s: int,
) -> tuple[str, str]:
    if not quality_ready:
        return "HOT_BLOCK_ENTRY_EDGE_QUALITY_NOT_READY", "entry_edge_quality_not_ready"
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "HOT_BLOCK_ENTRY_EDGE_EXIT_LIQUIDITY", "entry_edge_exit_liquidity"
    if text(row.get("last_trade_side")) != "buy":
        return "HOT_BLOCK_ENTRY_EDGE_LAST_TRADE", "entry_edge_last_trade_not_buy"
    if safe_int(row.get("sell_count_2s")) > max_sell_count_2s:
        return "HOT_BLOCK_ENTRY_EDGE_SELL_PRESSURE", "entry_edge_sell_pressure"
    if safe_int(row.get("net_buy_pressure_5s")) < edge_min_net_buy_pressure_5s:
        return "HOT_BLOCK_ENTRY_EDGE_BUY_PRESSURE", "entry_edge_buy_pressure_5s"
    price_change = safe_int(row.get("price_change_since_trigger_bps"), -1)
    if price_change < edge_min_price_change_bps:
        return "HOT_BLOCK_ENTRY_EDGE_PRICE_WEAK", "entry_edge_price_change_below_cost_floor"
    if edge_max_price_change_bps > 0 and price_change > edge_max_price_change_bps:
        return "HOT_BLOCK_ENTRY_EDGE_PRICE_OVERHEATED", "entry_edge_price_overheated"
    return "ENTRY_EDGE_READY", "none"


def event_ns(row: dict[str, Any]) -> int:
    return max(
        safe_int(row.get("create_seen_ns")),
        safe_int(row.get("first_buy_seen_ns")),
        safe_int(row.get("first_sell_seen_ns")),
        safe_int(row.get("birth_seen_ns")),
    )


def birth_ns(row: dict[str, Any]) -> int:
    return safe_int(row.get("create_seen_ns") or row.get("birth_seen_ns") or row.get("first_buy_seen_ns") or row.get("first_sell_seen_ns"))


def median_or_none(values: list[int]) -> int | None:
    if not values:
        return None
    return int(median(values))


def max_or_none(values: list[int]) -> int | None:
    return max(values) if values else None


def metric_value(row: dict[str, Any], keys: tuple[str, ...]) -> int | None:
    for key in keys:
        if row.get(key) not in (None, ""):
            return safe_int(row.get(key))
    return None


def empty_bucket(label: str) -> dict[str, Any]:
    return {
        "bucket": label,
        "launch_states_seen": 0,
        "buy_quote_ready_rows": 0,
        "sell_quote_ready_rows": 0,
        "exit_liquidity_guard_ready_rows": 0,
        "entry_quality_ready_rows": 0,
        "entry_edge_ready_rows": 0,
        "base_reserve_below_floor_rows": 0,
        "price_impact_too_high_rows": 0,
        "exit_quote_zero_rows": 0,
        "complete_or_migrated_rows": 0,
        "quote_state_missing_rows": 0,
        "migration_route_missing_rows": 0,
        "route_family_counts": {},
        "quote_blocker_counts": {},
        "entry_quality_decision_counts": {},
        "entry_edge_decision_counts": {},
        "median_peak_executable_exit_gain_bps": None,
        "max_peak_executable_exit_gain_bps": None,
        "median_max_adverse_excursion_bps": None,
        "median_time_to_peak_ms": None,
        "median_time_to_first_exit_deterioration_ms": None,
        "median_time_to_liquidity_collapse_ms": None,
        "markout_rows_with_peak": 0,
        "markout_missing_rows": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0T warmup calibration replay over hydrated launch quote-state rows.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--quote-state-rows", type=Path, required=True)
    parser.add_argument("--quote-state-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--panic-drop-2s-bps", type=int, default=700)
    parser.add_argument("--edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    args = parser.parse_args()

    rows = load_jsonl(args.quote_state_rows)
    quote_summary = load_json(args.quote_state_summary)
    max_seen_ns = max([event_ns(row) for row in rows] + [0])
    bucket_rows: dict[str, list[dict[str, Any]]] = defaultdict(list)
    evaluated_rows: list[dict[str, Any]] = []
    for row in rows:
        start_ns = birth_ns(row)
        age_ms = ((max_seen_ns - start_ns) // 1_000_000) if max_seen_ns > 0 and start_ns > 0 else -1
        bucket = bucket_for_age(age_ms)
        quality_decision, quality_blocker = evaluate_entry_quality(
            row,
            args.min_price_change_bps,
            args.max_sell_count_2s,
            args.panic_drop_2s_bps,
        )
        quality_ready = quality_decision == "ENTRY_QUALITY_READY"
        edge_decision, edge_blocker = evaluate_entry_edge(
            row,
            quality_ready,
            args.edge_min_price_change_bps,
            args.edge_max_price_change_bps,
            args.edge_min_net_buy_pressure_5s,
            args.max_sell_count_2s,
        )
        enriched = dict(row)
        enriched.update(
            {
                "warmup_age_ms": age_ms,
                "warmup_bucket": bucket,
                "entry_quality_decision": quality_decision,
                "entry_quality_first_blocker": quality_blocker,
                "entry_quality_ready": quality_ready,
                "entry_edge_decision": edge_decision,
                "entry_edge_first_blocker": edge_blocker,
                "entry_edge_ready": edge_decision == "ENTRY_EDGE_READY",
            }
        )
        bucket_rows[bucket].append(enriched)
        evaluated_rows.append(enriched)

    bucket_summaries: list[dict[str, Any]] = []
    ordered_labels = [label for label, _start, _end in BUCKETS] + ["unknown"]
    for label in ordered_labels:
        rows_for_bucket = bucket_rows.get(label, [])
        summary = empty_bucket(label)
        route_counts: Counter[str] = Counter()
        blocker_counts: Counter[str] = Counter()
        quality_counts: Counter[str] = Counter()
        edge_counts: Counter[str] = Counter()
        peak_values: list[int] = []
        adverse_values: list[int] = []
        time_to_peak_values: list[int] = []
        exit_deterioration_values: list[int] = []
        liquidity_collapse_values: list[int] = []
        for row in rows_for_bucket:
            blocker = text(row.get("first_quote_state_blocker")) or "unknown"
            summary["launch_states_seen"] += 1
            summary["buy_quote_ready_rows"] += int(boolish(row.get("entry_quote_ok")))
            summary["sell_quote_ready_rows"] += int(boolish(row.get("exit_quote_ok")))
            summary["exit_liquidity_guard_ready_rows"] += int(boolish(row.get("exit_liquidity_guard_ready")))
            summary["entry_quality_ready_rows"] += int(boolish(row.get("entry_quality_ready")))
            summary["entry_edge_ready_rows"] += int(boolish(row.get("entry_edge_ready")))
            summary["base_reserve_below_floor_rows"] += int(blocker in BASE_RESERVE_BLOCKERS)
            summary["price_impact_too_high_rows"] += int(blocker in PRICE_IMPACT_BLOCKERS)
            summary["exit_quote_zero_rows"] += int(blocker in EXIT_ZERO_BLOCKERS)
            summary["complete_or_migrated_rows"] += int(blocker in MIGRATION_BLOCKERS)
            summary["quote_state_missing_rows"] += int(blocker in STATE_MISSING_BLOCKERS)
            summary["migration_route_missing_rows"] += int(blocker in MIGRATION_BLOCKERS)
            route_counts[text(row.get("route_family")) or "unknown"] += 1
            blocker_counts[blocker] += 1
            quality_counts[text(row.get("entry_quality_decision"))] += 1
            edge_counts[text(row.get("entry_edge_decision"))] += 1
            peak = metric_value(row, ("peak_executable_exit_gain_bps", "peak_gain_bps"))
            adverse = metric_value(row, ("max_adverse_excursion_bps", "max_adverse_bps"))
            time_to_peak = metric_value(row, ("time_to_peak_ms", "time_to_peak"))
            exit_deterioration = metric_value(row, ("time_to_first_exit_deterioration_ms", "time_to_first_exit_deterioration"))
            liquidity_collapse = metric_value(row, ("time_to_liquidity_collapse_ms", "time_to_liquidity_collapse"))
            if peak is None:
                summary["markout_missing_rows"] += 1
            else:
                summary["markout_rows_with_peak"] += 1
                peak_values.append(peak)
            if adverse is not None:
                adverse_values.append(adverse)
            if time_to_peak is not None:
                time_to_peak_values.append(time_to_peak)
            if exit_deterioration is not None:
                exit_deterioration_values.append(exit_deterioration)
            if liquidity_collapse is not None:
                liquidity_collapse_values.append(liquidity_collapse)

        summary["route_family_counts"] = dict(route_counts)
        summary["quote_blocker_counts"] = dict(blocker_counts)
        summary["entry_quality_decision_counts"] = dict(quality_counts)
        summary["entry_edge_decision_counts"] = dict(edge_counts)
        summary["median_peak_executable_exit_gain_bps"] = median_or_none(peak_values)
        summary["max_peak_executable_exit_gain_bps"] = max_or_none(peak_values)
        summary["median_max_adverse_excursion_bps"] = median_or_none(adverse_values)
        summary["median_time_to_peak_ms"] = median_or_none(time_to_peak_values)
        summary["median_time_to_first_exit_deterioration_ms"] = median_or_none(exit_deterioration_values)
        summary["median_time_to_liquidity_collapse_ms"] = median_or_none(liquidity_collapse_values)
        bucket_summaries.append(summary)

    entry_quality_ready_rows = sum(1 for row in evaluated_rows if boolish(row.get("entry_quality_ready")))
    entry_edge_ready_rows = sum(1 for row in evaluated_rows if boolish(row.get("entry_edge_ready")))
    exit_liquidity_ready_rows = sum(1 for row in evaluated_rows if boolish(row.get("exit_liquidity_guard_ready")))
    quote_state_ready_rows = sum(1 for row in evaluated_rows if boolish(row.get("quote_state_ready")))
    blocker_counts = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in evaluated_rows)
    state_missing_rows = sum(count for blocker, count in blocker_counts.items() if blocker in STATE_MISSING_BLOCKERS)
    migration_rows = sum(count for blocker, count in blocker_counts.items() if blocker in MIGRATION_BLOCKERS)
    best_bucket = ""
    for summary in bucket_summaries:
        if summary["entry_edge_ready_rows"] > 0 and summary["exit_liquidity_guard_ready_rows"] > 0:
            best_bucket = text(summary.get("bucket"))
            break
    if not best_bucket:
        for summary in bucket_summaries:
            if summary["entry_quality_ready_rows"] > 0 and summary["exit_liquidity_guard_ready_rows"] > 0:
                best_bucket = text(summary.get("bucket"))
                break

    quote_state_missing_dominant = state_missing_rows > quote_state_ready_rows
    if entry_quality_ready_rows <= 0:
        status = "PARK_WARMUP_NO_ENTRY_QUALITY"
    elif exit_liquidity_ready_rows <= 0:
        status = "PARK_WARMUP_EXIT_LIQUIDITY_TOO_WEAK"
    elif quote_state_missing_dominant:
        status = "PARK_WARMUP_STATE_MISSING"
    elif not best_bucket:
        status = "PARK_WARMUP_NO_USABLE_BUCKET"
    else:
        status = "LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS"

    recommended = {
        "mode": "R3_HOLD_WARMUP_READY" if status == "LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS" else status,
        "best_entry_bucket": best_bucket or "none",
        "normal_warmup_min_age_ms": bucket_min_age_ms(best_bucket),
        "early_override_allowed": True,
        "early_override_requires": [
            "quote_state_ready=true",
            "exit_liquidity_guard_ready=true",
            "entry_edge_ready=true",
            "exit_quote_nonzero=true",
            "price_impact_within_limit=true",
        ],
        "do_not_finalize_from_subsecond_scalp_pnl": True,
    }
    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "wire_send": safe_int(quote_summary.get("wire_send")),
        "live_landed": safe_int(quote_summary.get("live_landed")),
        "unique_launch_states": len(evaluated_rows),
        "quote_state_ready_rows": quote_state_ready_rows,
        "exit_liquidity_guard_ready_rows": exit_liquidity_ready_rows,
        "entry_quality_ready_rows": entry_quality_ready_rows,
        "entry_edge_ready_rows": entry_edge_ready_rows,
        "state_missing_rows": state_missing_rows,
        "complete_or_migrated_rows": migration_rows,
        "quote_state_missing_dominant": quote_state_missing_dominant,
        "top_quote_state_blocker": blocker_counts.most_common(1)[0][0] if blocker_counts else "none",
        "quote_state_blocker_counts": dict(blocker_counts),
        "best_entry_bucket": best_bucket or "none",
        "recommended_warmup_rule": recommended,
        "bucket_summaries": bucket_summaries,
        "bucket_rows_path": str(args.out_dir / "bucket_rows.jsonl"),
        "acceptance_pass": status == "LAUNCH0T_WARMUP_CALIBRATION_REPLAY_PASS",
        "next_gate": "LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN" if migration_rows > quote_state_ready_rows else "R3_HOLD_NO_WIRE_OR_GUARDED_BATCH_REVIEW",
    }

    out_dir = args.out_dir
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    write_jsonl(out_dir / "bucket_rows.jsonl", evaluated_rows)
    print(f"status={status}")
    print(f"acceptance_pass={str(summary['acceptance_pass']).lower()}")
    print(f"best_entry_bucket={summary['best_entry_bucket']}")
    print(f"entry_quality_ready_rows={entry_quality_ready_rows}")
    print(f"entry_edge_ready_rows={entry_edge_ready_rows}")
    print(f"exit_liquidity_guard_ready_rows={exit_liquidity_ready_rows}")
    print(f"complete_or_migrated_rows={migration_rows}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

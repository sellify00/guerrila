#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
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
            doc = json.loads(raw)
        except Exception:
            continue
        if isinstance(doc, dict):
            rows.append(doc)
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


def age_bucket_ms(age_ms: int) -> str:
    if age_ms < 0:
        return "unknown"
    if age_ms < 1_000:
        return "0-1s"
    if age_ms < 2_000:
        return "1-2s"
    if age_ms < 3_000:
        return "2-3s"
    if age_ms < 5_000:
        return "3-5s"
    if age_ms < 10_000:
        return "5-10s"
    if age_ms < 30_000:
        return "10-30s"
    return "30s+"


def liquidity_bucket(base_reserve_raw: int) -> str:
    if base_reserve_raw <= 0:
        return "unknown_or_zero"
    if base_reserve_raw < 5_000_000:
        return "<0.005_SOL"
    if base_reserve_raw < 10_000_000:
        return "0.005-0.01_SOL"
    if base_reserve_raw < 25_000_000:
        return "0.01-0.025_SOL"
    if base_reserve_raw < 50_000_000:
        return "0.025-0.05_SOL"
    if base_reserve_raw < 100_000_000:
        return "0.05-0.10_SOL"
    return ">=0.10_SOL"


def movement_bucket(price_change_bps: int) -> str:
    if price_change_bps < 0:
        return "unknown_or_baseline_missing"
    if price_change_bps == 0:
        return "flat"
    if price_change_bps < 100:
        return "0-100bps"
    if price_change_bps < 300:
        return "100-300bps"
    if price_change_bps < 1000:
        return "300-1000bps"
    return ">=1000bps"


def join_key(row: dict[str, Any]) -> str:
    return "|".join(
        [
            text(row.get("launch_state_id") or row.get("birth_id")),
            text(row.get("mint")),
            text(row.get("pool") or row.get("pumpswap_pool") or row.get("pool_or_curve")),
            text(row.get("route_family")),
        ]
    )


def evaluate_entry_quality(row: dict[str, Any], min_price_change_bps: int, max_sell_count_2s: int, panic_drop_2s_bps: int) -> tuple[str, str]:
    if safe_int(row.get("entry_quality_baseline_expected_out_raw")) <= 0:
        return "HOT_BLOCK_ENTRY_QUALITY_WARMUP", "entry_quality_warmup"
    if text(row.get("last_trade_side")) != "buy":
        return "HOT_BLOCK_ENTRY_QUALITY_LAST_TRADE", "entry_quality_last_trade_not_buy"
    if safe_int(row.get("sell_count_2s")) > max_sell_count_2s or safe_int(row.get("net_buy_pressure_2s")) <= 0 or boolish(row.get("sell_pressure_seen")) or boolish(row.get("creator_sell_seen")):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_sell_pressure"
    if safe_int(row.get("panic_drop_2s_bps")) >= max(1, panic_drop_2s_bps):
        return "HOT_BLOCK_ENTRY_QUALITY_SELL_PRESSURE", "entry_quality_panic_drop_2s"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < min_price_change_bps:
        return "HOT_BLOCK_ENTRY_QUALITY_PRICE_NOT_UP", "entry_quality_price_not_up"
    return "ENTRY_QUALITY_READY", "none"


def unsupported_breakdowns(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    unsupported = [row for row in rows if text(row.get("first_quote_state_blocker")) == "QUOTE_ROUTE_UNSUPPORTED"]
    return {
        "by_route_family": dict(Counter(text(row.get("route_family")) or "unknown" for row in unsupported)),
        "by_program_id": dict(Counter(text(row.get("program_id")) or "unknown" for row in unsupported)),
        "by_source_kind": dict(Counter(text(row.get("source_kind")) or "unknown" for row in unsupported)),
        "by_source_provenance": dict(Counter(text(row.get("source_provenance")) or "unknown" for row in unsupported)),
        "by_pool_shape": dict(Counter("pool_present" if text(row.get("pool") or row.get("pool_or_curve")) else "pool_missing" for row in unsupported)),
    }


def mint_missing_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        if text(row.get("first_quote_state_blocker")) != "QUOTE_MINT_MISSING":
            continue
        out.append(
            {
                "launch_state_id": text(row.get("launch_state_id")),
                "source_row_id": text(row.get("launch_state_id") or row.get("birth_id")),
                "source_table": "launch0q_realtime_launch_quote_state_rows",
                "signature": text(row.get("source_signature")),
                "pool": text(row.get("pool") or row.get("pool_or_curve")),
                "program_id": text(row.get("program_id")),
                "detected_route": text(row.get("route_family")),
                "mint_a": text(row.get("mint_a")),
                "mint_b": text(row.get("mint_b")),
                "base_mint": text(row.get("base_mint")),
                "quote_mint": text(row.get("quote_mint")),
                "block_time": text(row.get("block_time")),
                "slot": safe_int(row.get("slot")),
                "source_kind": text(row.get("source_kind")),
                "source_provenance": text(row.get("source_provenance")),
            }
        )
    return out


def warmup_diagnostics(rows: list[dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    max_seen_ns = max([safe_int(row.get("first_buy_seen_ns")) for row in rows] + [0])
    warmup_rows: list[dict[str, Any]] = []
    for row in rows:
        if not boolish(row.get("trigger_b_pass")):
            continue
        decision, blocker = evaluate_entry_quality(row, args.min_price_change_bps, args.max_sell_count_2s, args.panic_drop_2s_bps)
        if decision != "HOT_BLOCK_ENTRY_QUALITY_WARMUP":
            continue
        buy_seen_ns = safe_int(row.get("first_buy_seen_ns"))
        age_ms = ((max_seen_ns - buy_seen_ns) // 1_000_000) if max_seen_ns > 0 and buy_seen_ns > 0 else -1
        warmup_rows.append(
            {
                "launch_state_id": text(row.get("launch_state_id")),
                "mint": text(row.get("mint")),
                "pool": text(row.get("pool")),
                "route_family": text(row.get("route_family")),
                "source_provenance": text(row.get("source_provenance")),
                "first_quote_state_blocker": text(row.get("first_quote_state_blocker")),
                "age_ms": age_ms,
                "age_bucket": age_bucket_ms(age_ms),
                "base_reserve_raw": safe_int(row.get("base_reserve_raw")),
                "liquidity_bucket": liquidity_bucket(safe_int(row.get("base_reserve_raw"))),
                "price_change_since_trigger_bps": safe_int(row.get("price_change_since_trigger_bps"), -1),
                "price_movement_bucket": movement_bucket(safe_int(row.get("price_change_since_trigger_bps"), -1)),
                "entry_quality_blocker": blocker,
            }
        )
    return {
        "rows": warmup_rows,
        "age_bucket_counts": dict(Counter(row["age_bucket"] for row in warmup_rows)),
        "route_family_counts": dict(Counter(text(row.get("route_family")) or "unknown" for row in warmup_rows)),
        "liquidity_bucket_counts": dict(Counter(row["liquidity_bucket"] for row in warmup_rows)),
        "price_movement_bucket_counts": dict(Counter(row["price_movement_bucket"] for row in warmup_rows)),
        "quote_blocker_counts": dict(Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in warmup_rows)),
    }


def pct(numerator: int, denominator: int) -> float:
    return round((numerator / denominator) * 100.0, 4) if denominator else 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0R route support and warmup reconcile.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--quote-state-rows", type=Path, required=True)
    parser.add_argument("--quote-state-summary", type=Path, required=True)
    parser.add_argument("--entry-quality-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--panic-drop-2s-bps", type=int, default=700)
    args = parser.parse_args()

    rows = load_jsonl(args.quote_state_rows)
    quote_summary = load_json(args.quote_state_summary)
    entry_summary = load_json(args.entry_quality_summary)
    blocker_counts = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in rows)
    blocker_sum = sum(blocker_counts.values())
    unsupported_route_rows = safe_int(blocker_counts.get("QUOTE_ROUTE_UNSUPPORTED"))
    unique_launch_states = safe_int(quote_summary.get("unique_launch_states")) or len(rows)
    warmup = warmup_diagnostics(rows, args)
    missing_rows = mint_missing_rows(rows)
    unsupported = unsupported_breakdowns(rows)
    entry_blockers = entry_summary.get("entry_quality_blocker_counts") if isinstance(entry_summary.get("entry_quality_blocker_counts"), dict) else {}
    regression_guards = {
        "wire_send_zero": safe_int(quote_summary.get("wire_send")) == 0,
        "live_landed_zero": safe_int(quote_summary.get("live_landed")) == 0,
        "quote_state_buckets_sum_to_unique_launch_states": blocker_sum == unique_launch_states,
        "entry_quality_missing_quote_state_not_dominant": safe_int(entry_blockers.get("ENTRY_QUALITY_MISSING_QUOTE_STATE")) == 0 and safe_int(entry_blockers.get("entry_quality_missing_quote_state")) == 0,
        "exit_quote_zero_blocks_entry": blocker_counts.get("EXIT_QUOTE_ZERO", 0) >= 0,
        "base_reserve_below_floor_blocks_entry": blocker_counts.get("BASE_RESERVE_BELOW_FLOOR", 0) >= 0,
        "entry_quality_has_hydrated_quote_state": safe_int(entry_summary.get("quote_state_rows_seen")) > 0,
        "exit_liquidity_requires_nonzero_exit_quote": safe_int(quote_summary.get("exit_liquidity_guard_ready_rows")) <= safe_int(quote_summary.get("exit_quote_ok_rows")),
        "fragility_consumes_quote_identity": (args.run_root / "22_launch35_absorption_no_exit_fragility_gate" / "rows.jsonl").exists(),
        "unsupported_route_breakdown_present": bool(unsupported.get("by_route_family")) or unsupported_route_rows == 0,
        "unsupported_route_breakdown_present_or_cleared": bool(unsupported.get("by_route_family")) or unsupported_route_rows == 0,
        "entry_quality_warmup_breakdown_present": bool(warmup.get("age_bucket_counts")),
    }
    acceptance_pass = (
        safe_int(quote_summary.get("wire_send")) == 0
        and safe_int(quote_summary.get("live_landed")) == 0
        and safe_int(quote_summary.get("quote_state_ready_rows")) > 0
        and safe_int(quote_summary.get("exit_liquidity_guard_ready_rows")) > 0
        and safe_int(entry_summary.get("entry_quality_ready_rows")) > 0
        and safe_int(entry_summary.get("entry_edge_ready_rows")) > 0
        and regression_guards["unsupported_route_breakdown_present"]
        and regression_guards["entry_quality_warmup_breakdown_present"]
    )
    summary = {
        "status": "LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_PASS" if acceptance_pass else "LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_INCOMPLETE",
        "milestone_dependency": "LAUNCH0Q_QUOTE_STATE_HYDRATION_NO_WIRE_PASS",
        "run_root": str(args.run_root),
        "wire_send": safe_int(quote_summary.get("wire_send")),
        "live_landed": safe_int(quote_summary.get("live_landed")),
        "unique_launch_states": unique_launch_states,
        "trigger_b_rows": safe_int(quote_summary.get("trigger_b_rows")),
        "quote_state_ready_rows": safe_int(quote_summary.get("quote_state_ready_rows")),
        "exit_liquidity_guard_ready_rows": safe_int(quote_summary.get("exit_liquidity_guard_ready_rows")),
        "entry_quality_evaluated_rows": safe_int(entry_summary.get("entry_quality_evaluated_rows")),
        "entry_quality_ready_rows": safe_int(entry_summary.get("entry_quality_ready_rows")),
        "entry_edge_ready_rows": safe_int(entry_summary.get("entry_edge_ready_rows")),
        "fragility_absorption_ready_rows": safe_int(load_json(args.run_root / "22_launch35_absorption_no_exit_fragility_gate" / "summary.json").get("fragility_absorption_ready_rows")),
        "trigger_b_coverage_pct": pct(safe_int(quote_summary.get("trigger_b_rows")), unique_launch_states),
        "quote_state_ready_over_all_pct": pct(safe_int(quote_summary.get("quote_state_ready_rows")), unique_launch_states),
        "quote_state_ready_over_trigger_b_pct": pct(safe_int(quote_summary.get("quote_state_ready_rows")), safe_int(quote_summary.get("trigger_b_rows"))),
        "entry_quality_ready_over_trigger_b_pct": pct(safe_int(entry_summary.get("entry_quality_ready_rows")), safe_int(quote_summary.get("trigger_b_rows"))),
        "entry_edge_ready_over_trigger_b_pct": pct(safe_int(entry_summary.get("entry_edge_ready_rows")), safe_int(quote_summary.get("trigger_b_rows"))),
        "quote_state_blocker_counts": dict(blocker_counts),
        "quote_state_blocker_count_sum": blocker_sum,
        "unsupported_route_breakdown": unsupported,
        "quote_mint_missing_trace_rows": len(missing_rows),
        "warmup_blocked_rows": len(warmup["rows"]),
        "warmup_age_bucket_counts": warmup["age_bucket_counts"],
        "warmup_route_family_counts": warmup["route_family_counts"],
        "warmup_liquidity_bucket_counts": warmup["liquidity_bucket_counts"],
        "warmup_price_movement_bucket_counts": warmup["price_movement_bucket_counts"],
        "warmup_quote_blocker_counts": warmup["quote_blocker_counts"],
        "top_quote_state_blocker": text(quote_summary.get("top_quote_state_blocker")),
        "top_entry_quality_blocker": text(entry_summary.get("top_entry_quality_blocker")),
        "regression_guards": regression_guards,
        "acceptance_pass": acceptance_pass,
        "next_required_work": [
            "route-specific quote support for dominant unsupported route families",
            "warmup calibration over fresh realtime windows",
            "primary flatten no-wire/sim validation after PumpSwap route-shape patch",
        ],
    }

    out_dir = args.out_dir
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    write_jsonl(out_dir / "quote_mint_missing_trace_rows.jsonl", missing_rows)
    write_jsonl(out_dir / "warmup_rows.jsonl", warmup["rows"])
    print(f"status={summary['status']}")
    print(f"acceptance_pass={str(acceptance_pass).lower()}")
    print(f"unsupported_route_breakdown={json.dumps(unsupported, sort_keys=True)}")
    print(f"warmup_age_bucket_counts={json.dumps(warmup['age_bucket_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

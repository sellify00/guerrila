#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from statistics import median
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


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    lines = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def first_value(data: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in data and data.get(key) not in (None, ""):
            return data.get(key)
    return None


def attempt_summaries(run_root: Path) -> list[dict[str, Any]]:
    rows = []
    for path in sorted(run_root.glob("attempts/attempt_*/launch5_one_mint_live_execute_summary.json")):
        data = load_json(path)
        if not data:
            continue
        data["_attempt_dir"] = path.parent.name
        data["_run_root"] = str(run_root)
        rows.append(data)
    return rows


def extract_attempt(data: dict[str, Any], label: str) -> dict[str, Any]:
    pnl_after = safe_int(first_value(data, "pnl_after_rent_recovery", "pnl_lamports"))
    peak = safe_int(first_value(data, "peak_executable_exit_gain_bps", "peak_gain_bps"))
    return {
        "label": label,
        "run_root": text(data.get("_run_root")),
        "attempt": text(data.get("_attempt_dir")),
        "mint": text(first_value(data, "selected_mint", "launch_mint", "target_mint")),
        "pool": text(first_value(data, "selected_pool", "pool", "target_pool")),
        "warmup_tier": text(first_value(data, "b002_plus_active_curve_warmup_tier", "warmup_tier")),
        "route_family": text(first_value(data, "late_follow_buy_route_family", "route_family", "venue")),
        "quote_state_ready": boolish(first_value(data, "b002_plus_active_curve_quote_state_ready", "quote_state_ready", "direct_entry_quote_ok")),
        "exit_liquidity_guard_ready": boolish(first_value(data, "b002_plus_active_curve_exit_liquidity_guard_ready", "exit_liquidity_guard_ready", "exit_fragility_guard_ok")),
        "executable_exit_value_slope_1s": first_value(data, "executable_exit_value_slope_1s_bps"),
        "executable_exit_value_slope_2s": first_value(data, "executable_exit_value_slope_2s_bps", "b002_plus_extension_exit_value_slope_2s_bps"),
        "exit_quote_stability_trend": safe_int(first_value(data, "exit_quote_stability_degrade_bps")),
        "reserve_pressure_score": safe_int(first_value(data, "reserve_pressure_score")),
        "base_reserve_decline_velocity_bps": first_value(data, "base_reserve_decline_velocity_bps", "b002_plus_extension_base_reserve_velocity_bps"),
        "price_change_since_trigger_bps": first_value(data, "price_change_since_trigger_bps", "b002_plus_active_curve_price_change_since_trigger_bps"),
        "price_change_state": text(first_value(data, "b002_plus_active_curve_price_change_state")) or "UNKNOWN",
        "sell_quality": text(first_value(data, "sell_quality")) or "UNKNOWN",
        "post_sell_absorption_score": safe_int(first_value(data, "post_sell_absorption_score")),
        "exit_reason": text(first_value(data, "exit_trigger_reason", "flatten_reason", "exit_reason")),
        "peak_executable_exit_gain_bps": peak,
        "pnl_after_rent_recovery": pnl_after,
        "strong_extension": peak >= 2000 or pnl_after > 0,
        "weak_extension": peak < 1500 and pnl_after <= 0,
    }


def median_int(rows: list[dict[str, Any]], key: str) -> int | None:
    values = [safe_int(row.get(key), 0) for row in rows if row.get(key) not in (None, "")]
    if not values:
        return None
    return int(median(values))


def summarize_class(rows: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "count": len(rows),
        "warmup_tier_counts": dict(Counter(text(row.get("warmup_tier")) for row in rows)),
        "sell_quality_counts": dict(Counter(text(row.get("sell_quality")) for row in rows)),
        "exit_reason_counts": dict(Counter(text(row.get("exit_reason")) for row in rows)),
        "median_peak_executable_exit_gain_bps": median_int(rows, "peak_executable_exit_gain_bps"),
        "median_pnl_after_rent_recovery": median_int(rows, "pnl_after_rent_recovery"),
        "median_exit_quote_stability_degrade_bps": median_int(rows, "exit_quote_stability_trend"),
    }


def command(args: argparse.Namespace) -> int:
    rows: list[dict[str, Any]] = []
    for run_text in args.positive_root:
        run_root = Path(run_text)
        rows.extend(extract_attempt(row, "positive") for row in attempt_summaries(run_root))
    for run_text in args.negative_root:
        run_root = Path(run_text)
        rows.extend(extract_attempt(row, "negative") for row in attempt_summaries(run_root))

    strong_rows = [row for row in rows if row["strong_extension"]]
    weak_rows = [row for row in rows if row["weak_extension"]]
    summary = {
        "status": "B002_PLUS_ACTIVE_CURVE_OUTCOME_AUTOPSY_COMPLETE",
        "rows_seen": len(rows),
        "strong_extension_rows": len(strong_rows),
        "weak_extension_rows": len(weak_rows),
        "strong_extension_profile": summarize_class(strong_rows),
        "weak_extension_profile": summarize_class(weak_rows),
        "recommended_gate": {
            "tier_a": "allow with structural/liquidity gates and non-negative real price-change if present",
            "tier_b": "require executable_exit_value_slope_2s_bps > 0 and base_reserve_decline_velocity_bps >= 0",
            "do_not_require": ["last_trade_side=buy", "buy-led momentum"],
        },
    }
    out_dir = Path(args.out_dir)
    write_jsonl(out_dir / "rows.jsonl", rows)
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="B002_PLUS active-curve outcome autopsy.")
    parser.add_argument("--positive-root", action="append", default=[])
    parser.add_argument("--negative-root", action="append", default=[])
    parser.add_argument("--out-dir", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return command(args)


if __name__ == "__main__":
    raise SystemExit(main())

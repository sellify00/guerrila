from __future__ import annotations

import argparse
import json
from pathlib import Path
from statistics import median
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


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


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def split_paths(value: str) -> list[Path]:
    return [Path(item).expanduser() for item in value.split(",") if item.strip()]


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    snap = row.get("presend_last_snapshot")
    if isinstance(snap, dict) and key in snap:
        return snap.get(key)
    return None


def attempt_rows(root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for attempt_dir in sorted((root / "attempts").glob("attempt_*")):
        row = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
        if not row:
            row = load_json(attempt_dir / "launch5_one_mint_live_execute.json")
        if row:
            row["_attempt"] = attempt_dir.name
            row["_run_root"] = str(root)
            rows.append(row)
    return rows


def feature_row(row: dict[str, Any], label: str) -> dict[str, Any]:
    exit_reason = text(nested_get(row, "exit_trigger_reason") or nested_get(row, "flatten_reason"))
    return {
        "label": label,
        "run_root": row.get("_run_root"),
        "attempt": row.get("_attempt"),
        "mint": text(nested_get(row, "selected_mint") or nested_get(row, "target_mint")),
        "pool": text(nested_get(row, "selected_pool") or nested_get(row, "target_pool")),
        "launch_age_ms": safe_int(nested_get(row, "target_seen_to_execute_start_us")) // 1000,
        "warmup_bucket": text(nested_get(row, "warmup_bucket")),
        "signal_age_ms": safe_int(nested_get(row, "signal_age_ms") or nested_get(row, "signal_age_ms_at_selection")),
        "price_change_since_trigger_bps": safe_int(nested_get(row, "price_change_since_trigger_bps"), -1),
        "net_buy_pressure_2s": safe_int(nested_get(row, "net_buy_pressure_2s")),
        "net_buy_pressure_5s": safe_int(nested_get(row, "net_buy_pressure_5s")),
        "sell_count_2s": safe_int(nested_get(row, "sell_count_2s")),
        "sell_count_5s": safe_int(nested_get(row, "sell_count_5s")),
        "sell_quality": text(nested_get(row, "sell_quality") or "NO_SELL"),
        "post_sell_absorption_score": safe_int(nested_get(row, "post_sell_absorption_score")),
        "base_reserve_to_position_ratio": safe_float(nested_get(row, "base_reserve_to_position_ratio")),
        "reserve_after_exit_to_position_ratio": safe_float(nested_get(row, "reserve_after_exit_to_position_ratio")),
        "exit_price_impact_bps": safe_int(nested_get(row, "exit_price_impact_bps") or nested_get(row, "exit_price_impact_bps_at_entry")),
        "exit_quote_stability_score": safe_int(nested_get(row, "exit_quote_stability_score")),
        "exit_quote_zero_recent_count": safe_int(nested_get(row, "exit_quote_zero_recent_count")),
        "base_reserve_decline_velocity_bps": safe_int(nested_get(row, "base_reserve_decline_velocity_bps")),
        "entry_quote_ok": boolish(nested_get(row, "entry_quote_ok") or nested_get(row, "direct_entry_quote_ok")),
        "exit_quote_ok": boolish(nested_get(row, "exit_quote_ok") or nested_get(row, "direct_exit_quote_ok")),
        "exit_liquidity_guard_ready": boolish(nested_get(row, "exit_liquidity_guard_ok")),
        "exit_reason": exit_reason,
        "pnl_after_rent_recovery": safe_int(nested_get(row, "pnl_after_rent_recovery") or nested_get(row, "pnl_lamports")),
        "burn_close_landed": boolish(nested_get(row, "burn_close_landed")),
    }


def nz(values: list[float]) -> list[float]:
    return [value for value in values if value not in (0, -1)]


def min_or(values: list[float], default: float) -> float:
    values = nz(values)
    return min(values) if values else default


def max_or(values: list[float], default: float) -> float:
    values = nz(values)
    return max(values) if values else default


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch44 Batch002 fingerprint extractor.")
    parser.add_argument("--positive-roots", default="")
    parser.add_argument("--bad-roots", default="")
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    positive_rows: list[dict[str, Any]] = []
    bad_rows: list[dict[str, Any]] = []
    for root in split_paths(args.positive_roots):
        for row in attempt_rows(root):
            if boolish(row.get("acquisition_landed")) and safe_int(row.get("pnl_after_rent_recovery"), safe_int(row.get("pnl_lamports"))) > 0 and not boolish(row.get("burn_close_landed")):
                positive_rows.append(feature_row(row, "BATCH002_POSITIVE"))
    for root in split_paths(args.bad_roots):
        for row in attempt_rows(root):
            pnl = safe_int(row.get("pnl_after_rent_recovery"), safe_int(row.get("pnl_lamports")))
            if boolish(row.get("burn_close_landed")) or pnl < 0:
                bad_rows.append(feature_row(row, "BAD_OR_TAIL"))

    positives = positive_rows
    live_thresholds = {
        "max_signal_age_ms": int(max_or([row["signal_age_ms"] for row in positives], 60_000)),
        "max_sell_count_2s": int(max_or([row["sell_count_2s"] for row in positives], 0)),
        "max_sell_count_5s": int(max_or([row["sell_count_5s"] for row in positives], 1)),
        "min_net_buy_pressure_2s": int(min_or([row["net_buy_pressure_2s"] for row in positives], -10)),
        "min_net_buy_pressure_5s": int(min_or([row["net_buy_pressure_5s"] for row in positives], -10)),
        "min_price_change_since_trigger_bps": int(min_or([row["price_change_since_trigger_bps"] for row in positives], -1)),
        "max_price_change_since_trigger_bps": int(max_or([row["price_change_since_trigger_bps"] for row in positives], 10_000)),
        "min_base_reserve_to_position_ratio": max(50.0, min_or([row["base_reserve_to_position_ratio"] for row in positives], 50.0) * 0.8),
        "min_reserve_after_exit_to_position_ratio": max(10.0, min_or([row["reserve_after_exit_to_position_ratio"] for row in positives], 10.0) * 0.8),
        "max_exit_price_impact_bps": int(max(1000, max_or([row["exit_price_impact_bps"] for row in positives], 1000))),
        "max_exit_quote_zero_recent_count": 0,
        "max_base_reserve_decline_velocity_bps": 0,
        "allowed_sell_quality": sorted({row["sell_quality"] for row in positives if row["sell_quality"]} | {"NO_SELL", "ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"}),
    }
    summary = {
        "status": "BATCH002_FINGERPRINT_EXTRACTED",
        "positive_rows": len(positive_rows),
        "bad_rows": len(bad_rows),
        "positive_median_pnl_after_rent_recovery": median([row["pnl_after_rent_recovery"] for row in positive_rows]) if positive_rows else 0,
        "live_thresholds": live_thresholds,
    }
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "positive_rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in positive_rows) + ("\n" if positive_rows else ""), encoding="utf-8")
    (out_dir / "bad_rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in bad_rows) + ("\n" if bad_rows else ""), encoding="utf-8")
    (out_dir / "fingerprint.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{key}={value}" for key, value in summary.items() if key != "live_thresholds") + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

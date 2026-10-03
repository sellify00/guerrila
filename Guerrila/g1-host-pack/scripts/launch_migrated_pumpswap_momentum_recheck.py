#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


READY_DECISION = "MIGRATION_PUMPSWAP_ROUTE_READY"
READY_R3_DECISION = "LAUNCH0V_MIGRATED_PUMPSWAP_R3_READY"
BENIGN_SELL_LABELS = {"BENIGN_SELL", "ABSORBED_SELL", "ROUTINE_PROFIT_TAKE"}
TOXIC_SELL_LABELS = {
    "CREATOR_SELL",
    "TOXIC_SELL",
    "INSIDER_CLUSTER_SELL",
    "CASCADE_SELL_PRESSURE",
    "LIQUIDITY_EXIT_RISK",
    "EXIT_LIQUIDITY_RISK",
}


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


def migrated_quote_ready(row: dict[str, Any]) -> bool:
    return (
        text(row.get("decision")) == READY_DECISION
        or text(row.get("migrated_pumpswap_r3_decision")) in {READY_R3_DECISION, "PARK_MIGRATED_PUMPSWAP_ENTRY_QUALITY", "PARK_MIGRATED_PUMPSWAP_ENTRY_EDGE"}
        or boolish(row.get("quote_state_ready"))
    ) and boolish(row.get("exit_liquidity_guard_ready"))


def sell_quality(row: dict[str, Any]) -> str:
    for key in ("sell_quality", "latest_sell_quality", "sell_quality_label", "last_sell_quality"):
        value = text(row.get(key))
        if value:
            return value
    if boolish(row.get("creator_sell_seen")):
        return "CREATOR_SELL"
    return ""


def momentum_state_missing(row: dict[str, Any]) -> bool:
    return (
        not text(row.get("last_trade_side"))
        and safe_int(row.get("last_buy_age_ms"), -1) < 0
        and safe_int(row.get("last_sell_age_ms"), -1) < 0
        and safe_int(row.get("net_buy_pressure_2s")) == 0
        and safe_int(row.get("net_buy_pressure_5s")) == 0
        and safe_int(row.get("price_change_since_migration_bps"), safe_int(row.get("price_change_since_trigger_bps"), 0)) == 0
    )


def recheck(row: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    if not migrated_quote_ready(row):
        return "PARK_MIGRATED_PUMPSWAP_NOT_QUOTE_READY", text(row.get("decision")) or "quote_not_ready"
    quality = sell_quality(row)
    if quality in TOXIC_SELL_LABELS:
        return "PARK_MIGRATED_PUMPSWAP_TOXIC_SELL", quality
    if momentum_state_missing(row):
        return "PARK_MIGRATED_PUMPSWAP_MOMENTUM_STATE_MISSING", "momentum_state_missing"

    last_trade = text(row.get("last_trade_side"))
    net_2s = safe_int(row.get("net_buy_pressure_2s"))
    net_5s = safe_int(row.get("net_buy_pressure_5s"))
    sell_count_2s = safe_int(row.get("sell_count_2s"))
    last_sell_age = safe_int(row.get("last_sell_age_ms"), -1)
    absorption = safe_int(row.get("post_sell_absorption_score"), -1)
    price_change = safe_int(row.get("price_change_since_migration_bps"), safe_int(row.get("price_change_since_trigger_bps"), 0))

    if last_trade == "buy" and (net_2s > 0 or net_5s >= args.min_net_buy_pressure_5s):
        if price_change < args.min_price_change_bps:
            return "PARK_MIGRATED_PUMPSWAP_PRICE_NOT_UP", "price_change_since_migration_below_threshold"
        return "MIGRATED_PUMPSWAP_MOMENTUM_RECHECK_READY", "none"

    absorbed_sell = (
        quality in BENIGN_SELL_LABELS
        and sell_count_2s <= args.max_sell_count_2s
        and net_5s >= args.min_net_buy_pressure_5s
        and (last_sell_age < 0 or last_sell_age >= args.min_absorbed_sell_age_ms)
        and (absorption < 0 or absorption >= args.min_absorption_score)
    )
    if absorbed_sell:
        if price_change < args.min_price_change_bps:
            return "PARK_MIGRATED_PUMPSWAP_PRICE_NOT_UP", "absorbed_sell_price_change_below_threshold"
        return "MIGRATED_PUMPSWAP_MOMENTUM_RECHECK_READY", "absorbed_sell"

    if last_trade != "buy":
        return "PARK_MIGRATED_PUMPSWAP_LAST_TRADE_NOT_BUY", "last_trade_not_buy_or_not_absorbed"
    if net_2s <= 0 and net_5s < args.min_net_buy_pressure_5s:
        return "PARK_MIGRATED_PUMPSWAP_BUY_PRESSURE_WEAK", "buy_pressure_weak"
    return "PARK_MIGRATED_PUMPSWAP_ABSORPTION_WEAK", "absorption_weak"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0V2 migrated PumpSwap momentum recheck.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-net-buy-pressure-5s", type=int, default=1)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--min-absorbed-sell-age-ms", type=int, default=3000)
    parser.add_argument("--min-absorption-score", type=int, default=60)
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    args = parser.parse_args()

    rows = load_jsonl(args.rows)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    route_ready_rows = 0
    momentum_ready_rows = 0
    missing_state_rows = 0

    for row in rows:
        decision, blocker = recheck(row, args)
        route_ready = migrated_quote_ready(row)
        route_ready_rows += int(route_ready)
        momentum_ready_rows += int(decision == "MIGRATED_PUMPSWAP_MOMENTUM_RECHECK_READY")
        missing_state_rows += int(decision == "PARK_MIGRATED_PUMPSWAP_MOMENTUM_STATE_MISSING")
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch0v2_migrated_pumpswap_momentum_recheck",
                "migrated_pumpswap_momentum_decision": decision,
                "migrated_pumpswap_momentum_first_blocker": blocker,
                "migrated_pumpswap_momentum_ready": decision == "MIGRATED_PUMPSWAP_MOMENTUM_RECHECK_READY",
                "sell_quality": sell_quality(row),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1

    if momentum_ready_rows > 0:
        status = "LAUNCH0V2_MIGRATED_PUMPSWAP_MOMENTUM_RECHECK_READY"
    elif route_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_NO_QUOTE_READY_ROWS"
    elif missing_state_rows == route_ready_rows:
        status = "PARK_MIGRATED_PUMPSWAP_MOMENTUM_STATE_MISSING"
    else:
        status = "PARK_MIGRATED_PUMPSWAP_MOMENTUM_NOT_ALIVE"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "rows_path_input": str(args.rows),
        "route_ready_rows": route_ready_rows,
        "momentum_ready_rows": momentum_ready_rows,
        "momentum_state_missing_rows": missing_state_rows,
        "last_trade_not_buy_rows": decision_counts.get("PARK_MIGRATED_PUMPSWAP_LAST_TRADE_NOT_BUY", 0),
        "toxic_sell_rows": decision_counts.get("PARK_MIGRATED_PUMPSWAP_TOXIC_SELL", 0),
        "absorbed_sell_ready_rows": blocker_counts.get("absorbed_sell", 0),
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": momentum_ready_rows > 0,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "route_ready_rows",
        "momentum_ready_rows",
        "momentum_state_missing_rows",
        "last_trade_not_buy_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

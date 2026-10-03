from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path
from typing import Any


SELL_QUALITY_LABELS = {
    "BENIGN_SELL",
    "ABSORBED_SELL",
    "ROUTINE_PROFIT_TAKE",
    "TOXIC_SELL",
    "CREATOR_SELL",
    "INSIDER_CLUSTER_SELL",
    "CASCADE_SELL_PRESSURE",
    "LIQUIDITY_EXIT_RISK",
}

HOLD_NORMAL = "HOLD_NORMAL"
HOLD_HEALTHY_PULLBACK = "HOLD_HEALTHY_PULLBACK"
HOLD_SELL_ABSORBED = "HOLD_SELL_ABSORBED"
TIGHTEN_STOP = "TIGHTEN_STOP"
EXIT_DRAWDOWN_FROM_HIGH = "EXIT_DRAWDOWN_FROM_HIGH"
EXIT_PANIC_DROP = "EXIT_PANIC_DROP"
EXIT_SELL_PRESSURE_DOMINANT = "EXIT_SELL_PRESSURE_DOMINANT"
EXIT_CREATOR_SELL = "EXIT_CREATOR_SELL"
EXIT_TOP_HOLDER_DUMP = "EXIT_TOP_HOLDER_DUMP"
EXIT_LIQUIDITY_COLLAPSE = "EXIT_LIQUIDITY_COLLAPSE"
EXIT_ROUTE_DETERIORATED = "EXIT_ROUTE_DETERIORATED"
EXIT_CATASTROPHE = "EXIT_CATASTROPHE"


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


def boolish(value: Any, default: bool = False) -> bool:
    if value is None or value == "":
        return default
    return str(value).strip().lower() in {"1", "true", "yes", "y", "on"}


def clamp(value: int, lo: int = 0, hi: int = 100) -> int:
    return max(lo, min(hi, value))


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def bps_gain(current_value: int, base_value: int) -> int:
    if base_value <= 0 or current_value <= base_value:
        return 0
    return max(0, ((current_value - base_value) * 10_000) // base_value)


def median_int(values: list[int]) -> int:
    values = [value for value in values if value > 0]
    if not values:
        return 0
    return int(statistics.median(values))


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists() or path.is_dir():
        return []
    out: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except Exception:
            continue
        if isinstance(row, dict):
            out.append(row)
    return out


def row_ts(row: dict[str, Any], index: int) -> float:
    for key in ("ts", "unix_ts", "wall_time", "sample_time"):
        value = safe_float(row.get(key))
        if value > 0:
            return value
    ns = safe_int(row.get("ns") or row.get("sample_ns") or row.get("seen_ns"))
    if ns > 0:
        return ns / 1_000_000_000.0
    return float(index)


def price_value(row: dict[str, Any]) -> int:
    for key in (
        "current_price",
        "current_price_lamports",
        "current_price_raw",
        "exit_expected_out_raw",
        "expected_out_raw",
        "price_lamports",
        "price_raw",
    ):
        value = safe_int(row.get(key))
        if value > 0:
            return value
    return 0


def enrich_sequence(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    ordered = sorted(enumerate(rows), key=lambda item: row_ts(item[1], item[0]))
    entry_price = 0
    local_high = 0
    entry_liquidity = 0
    enriched: list[dict[str, Any]] = []
    for index, row in ordered:
        price = price_value(row)
        if entry_price <= 0 and price > 0:
            entry_price = price
        if local_high <= 0:
            local_high = price
        local_high = max(local_high, price)
        liquidity = safe_int(row.get("pool_sol_liquidity_lamports") or row.get("sol_liquidity_lamports"))
        if entry_liquidity <= 0 and liquidity > 0:
            entry_liquidity = liquidity
        enriched.append(
            {
                **row,
                "entry_price": safe_int(row.get("entry_price")) or entry_price,
                "current_price": safe_int(row.get("current_price")) or price,
                "local_high_price": safe_int(row.get("local_high_price")) or local_high,
                "drawdown_from_entry_bps": safe_int(row.get("drawdown_from_entry_bps")) or bps_drop(entry_price, price),
                "drawdown_from_high_bps": safe_int(row.get("drawdown_from_high_bps")) or bps_drop(local_high, price),
                "liquidity_drop_bps": safe_int(row.get("liquidity_drop_bps")) or bps_drop(entry_liquidity, liquidity),
            }
        )
    return enriched


def classify_sell_quality(row: dict[str, Any]) -> dict[str, Any]:
    buy_count_10s = safe_int(row.get("buy_count_10s") or row.get("buy_count_recent"))
    sell_count_10s = safe_int(row.get("sell_count_10s") or row.get("sell_count_recent"))
    buy_volume_10s = safe_int(row.get("buy_volume_10s") or row.get("buy_volume_recent"))
    sell_volume_10s = safe_int(row.get("sell_volume_10s") or row.get("sell_volume_recent"))
    net_buy_pressure_10s = safe_int(row.get("net_buy_pressure_10s"), buy_count_10s - sell_count_10s)
    largest_sell_10s = safe_int(row.get("largest_sell_10s"))
    median_buy_size_10s = safe_int(row.get("median_buy_size_10s") or row.get("median_buy_10s"))
    pool_liquidity = safe_int(row.get("pool_sol_liquidity_lamports") or row.get("sol_liquidity_lamports"))
    largest_sell_vs_pool_bps = safe_int(row.get("largest_sell_vs_pool_bps"))
    if largest_sell_vs_pool_bps <= 0 and largest_sell_10s > 0 and pool_liquidity > 0:
        largest_sell_vs_pool_bps = (largest_sell_10s * 10_000) // pool_liquidity
    largest_sell_vs_recent_buy_median = 0
    if largest_sell_10s > 0 and median_buy_size_10s > 0:
        largest_sell_vs_recent_buy_median = int(largest_sell_10s / max(1, median_buy_size_10s))

    drawdown_from_high_bps = safe_int(row.get("drawdown_from_high_bps"))
    price_drop_10s_bps = safe_int(row.get("price_drop_10s_bps") or row.get("panic_drop_10s_bps") or row.get("drop_10s_bps"))
    exit_quote_available = boolish(row.get("exit_quote_available"), True) and safe_int(row.get("exit_expected_out_raw") or row.get("expected_out_raw")) > 0
    exit_quote_dust = boolish(row.get("exit_quote_dust"))
    exit_price_impact_bps = safe_int(row.get("exit_price_impact_bps"))
    liquidity_drop_bps = safe_int(row.get("liquidity_drop_bps"))
    creator_sell_seen = boolish(row.get("creator_sell_seen")) or boolish(row.get("seller_is_creator"))
    insider_sell_seen = boolish(row.get("insider_cluster_sell_seen")) or boolish(row.get("seller_is_first_buyer"))
    top_holder_sell_seen = boolish(row.get("top_holder_sell_seen")) or boolish(row.get("seller_is_top_holder"))
    buy_response_after_sell_5s = safe_int(row.get("buy_response_after_sell_5s") or row.get("buy_count_after_sell_5s"))
    buy_response_after_sell_15s = safe_int(row.get("buy_response_after_sell_15s") or row.get("buy_count_after_sell_15s"))
    new_buyers_after_sell = safe_int(row.get("new_buyers_after_sell"))
    net_buy_pressure_after_sell = safe_int(row.get("net_buy_pressure_after_sell"), buy_response_after_sell_5s - sell_count_10s)

    exit_risk_score = 0
    if not exit_quote_available or exit_quote_dust:
        exit_risk_score += 60
    if exit_price_impact_bps >= 1500:
        exit_risk_score += 25
    if liquidity_drop_bps >= 2000:
        exit_risk_score += 25
    if largest_sell_vs_pool_bps >= 500:
        exit_risk_score += 15
    exit_risk_score = clamp(exit_risk_score)

    post_sell_absorption_score = 0
    if sell_count_10s > 0 and drawdown_from_high_bps < 500:
        post_sell_absorption_score += 35
    if buy_response_after_sell_5s > 0 or buy_response_after_sell_15s > 0:
        post_sell_absorption_score += 25
    if new_buyers_after_sell > 0:
        post_sell_absorption_score += 20
    if net_buy_pressure_after_sell > 0 or net_buy_pressure_10s > 0:
        post_sell_absorption_score += 20
    post_sell_absorption_score = clamp(post_sell_absorption_score)

    toxic_sell_score = 0
    if creator_sell_seen:
        toxic_sell_score += 100
    if insider_sell_seen:
        toxic_sell_score += 70
    if top_holder_sell_seen:
        toxic_sell_score += 60
    if sell_count_10s > 0 and sell_count_10s > max(1, int(buy_count_10s * 1.5)):
        toxic_sell_score += 25
    if sell_volume_10s > 0 and buy_volume_10s > 0 and sell_volume_10s > int(buy_volume_10s * 1.5):
        toxic_sell_score += 25
    if net_buy_pressure_10s <= -2 and sell_count_10s > 0:
        toxic_sell_score += 20
    if largest_sell_vs_recent_buy_median >= 2:
        toxic_sell_score += 20
    if drawdown_from_high_bps >= 800:
        toxic_sell_score += 20
    if price_drop_10s_bps >= 800:
        toxic_sell_score += 20
    toxic_sell_score = clamp(toxic_sell_score)

    if creator_sell_seen:
        sell_quality = "CREATOR_SELL"
    elif insider_sell_seen:
        sell_quality = "INSIDER_CLUSTER_SELL"
    elif exit_risk_score >= 60:
        sell_quality = "LIQUIDITY_EXIT_RISK"
    elif sell_count_10s > 1 and toxic_sell_score >= 45:
        sell_quality = "CASCADE_SELL_PRESSURE"
    elif toxic_sell_score >= 60:
        sell_quality = "TOXIC_SELL"
    elif sell_count_10s > 0 and post_sell_absorption_score >= 60:
        sell_quality = "ABSORBED_SELL"
    elif sell_count_10s > 0 and largest_sell_vs_recent_buy_median <= 1 and drawdown_from_high_bps < 800:
        sell_quality = "ROUTINE_PROFIT_TAKE"
    elif sell_count_10s > 0:
        sell_quality = "BENIGN_SELL"
    else:
        sell_quality = "NO_SELL"

    return {
        "sell_quality": sell_quality,
        "post_sell_absorption_score": post_sell_absorption_score,
        "toxic_sell_score": toxic_sell_score,
        "exit_risk_score": exit_risk_score,
        "largest_sell_vs_pool_bps": largest_sell_vs_pool_bps,
        "largest_sell_vs_recent_buy_median": largest_sell_vs_recent_buy_median,
    }


def classify_chaos(row: dict[str, Any]) -> dict[str, Any]:
    sell = classify_sell_quality(row)
    drawdown_from_high_bps = safe_int(row.get("drawdown_from_high_bps"))
    drawdown_from_entry_bps = safe_int(row.get("drawdown_from_entry_bps"))
    price_drop_10s_bps = safe_int(row.get("price_drop_10s_bps") or row.get("drop_10s_bps"))
    price_drop_60s_bps = safe_int(row.get("price_drop_60s_bps") or row.get("drop_60s_bps"))
    price_drop_5m_bps = safe_int(row.get("price_drop_5m_bps") or row.get("drop_5m_bps"))
    liquidity_drop_bps = safe_int(row.get("liquidity_drop_bps"))
    exit_quote_available = boolish(row.get("exit_quote_available"), True) and safe_int(row.get("exit_expected_out_raw") or row.get("expected_out_raw")) > 0
    exit_dust = boolish(row.get("exit_dust")) or boolish(row.get("exit_quote_dust"))
    exit_price_impact_bps = safe_int(row.get("exit_price_impact_bps"))
    buy_count_10s = safe_int(row.get("buy_count_10s"))
    sell_count_10s = safe_int(row.get("sell_count_10s"))
    net_buy_pressure_10s = safe_int(row.get("net_buy_pressure_10s"), buy_count_10s - sell_count_10s)

    catastrophe_score = 0
    if sell["sell_quality"] == "CREATOR_SELL":
        catastrophe_score += 100
    if sell["sell_quality"] in {"INSIDER_CLUSTER_SELL", "LIQUIDITY_EXIT_RISK"}:
        catastrophe_score += 70
    if drawdown_from_high_bps >= 2500:
        catastrophe_score += 50
    if price_drop_60s_bps >= 2000:
        catastrophe_score += 45
    if liquidity_drop_bps >= 3000:
        catastrophe_score += 45
    if not exit_quote_available or exit_dust:
        catastrophe_score += 70
    catastrophe_score = clamp(catastrophe_score)

    chaos_score = 0
    chaos_score += min(35, drawdown_from_high_bps // 50)
    chaos_score += min(25, price_drop_10s_bps // 40)
    chaos_score += min(20, price_drop_60s_bps // 100)
    chaos_score += min(20, max(0, -net_buy_pressure_10s) * 5)
    chaos_score += sell["toxic_sell_score"] // 3
    if exit_price_impact_bps >= 1500:
        chaos_score += 20
    chaos_score = clamp(chaos_score)

    if sell["sell_quality"] == "CREATOR_SELL":
        exit_decision = EXIT_CREATOR_SELL
    elif boolish(row.get("top_holder_sell_seen")):
        exit_decision = EXIT_TOP_HOLDER_DUMP
    elif catastrophe_score >= 80:
        exit_decision = EXIT_CATASTROPHE
    elif not exit_quote_available or exit_dust:
        exit_decision = EXIT_ROUTE_DETERIORATED
    elif liquidity_drop_bps >= 3000:
        exit_decision = EXIT_LIQUIDITY_COLLAPSE
    elif price_drop_10s_bps >= 800 or price_drop_60s_bps >= 2000:
        exit_decision = EXIT_PANIC_DROP
    elif sell["sell_quality"] in {"TOXIC_SELL", "CASCADE_SELL_PRESSURE"} and drawdown_from_high_bps >= 800:
        exit_decision = EXIT_SELL_PRESSURE_DOMINANT
    elif drawdown_from_high_bps >= 2500:
        exit_decision = EXIT_DRAWDOWN_FROM_HIGH
    elif sell["sell_quality"] == "ABSORBED_SELL":
        exit_decision = HOLD_SELL_ABSORBED
    elif sell["sell_quality"] == "ROUTINE_PROFIT_TAKE" or (sell_count_10s > buy_count_10s and sell_count_10s > 0):
        exit_decision = TIGHTEN_STOP
    elif drawdown_from_entry_bps > 0 or drawdown_from_high_bps > 0:
        exit_decision = HOLD_HEALTHY_PULLBACK
    else:
        exit_decision = HOLD_NORMAL

    return {
        **sell,
        "chaos_score": chaos_score,
        "catastrophe_score": catastrophe_score,
        "exit_decision": exit_decision,
        "price_drop_5m_bps": price_drop_5m_bps,
    }


def evaluate_rows(rows: list[dict[str, Any]]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    enriched = enrich_sequence(rows)
    out_rows: list[dict[str, Any]] = []
    decision_counts: dict[str, int] = {}
    sell_quality_counts: dict[str, int] = {}
    first_exit_row: dict[str, Any] | None = None
    for row in enriched:
        classification = classify_chaos(row)
        out = {**row, **classification}
        out_rows.append(out)
        decision_counts[classification["exit_decision"]] = decision_counts.get(classification["exit_decision"], 0) + 1
        sell_quality_counts[classification["sell_quality"]] = sell_quality_counts.get(classification["sell_quality"], 0) + 1
        if first_exit_row is None and classification["exit_decision"].startswith("EXIT_"):
            first_exit_row = out

    final = first_exit_row or (out_rows[-1] if out_rows else {})
    summary = {
        "status": "EXIT_TRIGGERED" if first_exit_row else "HOLD_VALID",
        "rows_seen": len(rows),
        "rows_classified": len(out_rows),
        "sell_quality_counts": sell_quality_counts,
        "exit_decision_counts": decision_counts,
        "exit_decision": text(final.get("exit_decision")) or HOLD_NORMAL,
        "sell_quality": text(final.get("sell_quality")) or "NO_SELL",
        "chaos_score": safe_int(final.get("chaos_score")),
        "catastrophe_score": safe_int(final.get("catastrophe_score")),
        "toxic_sell_score": safe_int(final.get("toxic_sell_score")),
        "post_sell_absorption_score": safe_int(final.get("post_sell_absorption_score")),
        "exit_risk_score": safe_int(final.get("exit_risk_score")),
        "drawdown_from_high_bps": safe_int(final.get("drawdown_from_high_bps")),
        "price_drop_10s_bps": safe_int(final.get("price_drop_10s_bps") or final.get("drop_10s_bps")),
        "price_drop_60s_bps": safe_int(final.get("price_drop_60s_bps") or final.get("drop_60s_bps")),
        "liquidity_drop_bps": safe_int(final.get("liquidity_drop_bps")),
        "generated_at_unix": int(time.time()),
    }
    return summary, out_rows


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD chaos/catastrophe and sell-quality detector.")
    parser.add_argument("--snapshots-jsonl", required=True)
    parser.add_argument("--summary-json", required=True)
    parser.add_argument("--rows-jsonl", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    rows = load_jsonl(Path(args.snapshots_jsonl))
    summary, out_rows = evaluate_rows(rows)
    Path(args.summary_json).parent.mkdir(parents=True, exist_ok=True)
    Path(args.summary_json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    Path(args.rows_jsonl).write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")
    print("\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

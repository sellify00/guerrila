from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path
from typing import Any

from launch_chaos_detector import classify_chaos


EXIT_HARD_STOP_FROM_ENTRY = "EXIT_HARD_STOP_FROM_ENTRY"
EXIT_TRAILING_STOP_FROM_HIGH = "EXIT_TRAILING_STOP_FROM_HIGH"
EXIT_PANIC_DROP_10S = "EXIT_PANIC_DROP_10S"
EXIT_PANIC_DROP_60S = "EXIT_PANIC_DROP_60S"
EXIT_SELL_PRESSURE_DOMINANT = "EXIT_SELL_PRESSURE_DOMINANT"
EXIT_CREATOR_SELL = "EXIT_CREATOR_SELL"
EXIT_TOP_HOLDER_SELL = "EXIT_TOP_HOLDER_SELL"
EXIT_EXIT_QUOTE_DETERIORATED = "EXIT_EXIT_QUOTE_DETERIORATED"
EXIT_LIQUIDITY_COLLAPSE = "EXIT_LIQUIDITY_COLLAPSE"
EXIT_TTL = "EXIT_TTL"


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


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists() or path.is_dir():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except Exception:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def bps_gain(current_value: int, base_value: int) -> int:
    if base_value <= 0 or current_value <= base_value:
        return 0
    return max(0, ((current_value - base_value) * 10_000) // base_value)


def bps_abs_move(lhs: int, rhs: int) -> int:
    high = max(lhs, rhs)
    low = min(lhs, rhs)
    if high <= 0 or low <= 0:
        return 0
    return max(0, ((high - low) * 10_000) // high)


def median_int(values: list[int]) -> int:
    values = [value for value in values if value >= 0]
    if not values:
        return 0
    return int(statistics.median(values))


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


def row_ts(row: dict[str, Any], index: int) -> float:
    for key in ("ts", "unix_ts", "wall_time", "sample_time"):
        value = safe_float(row.get(key))
        if value > 0:
            return value
    ns = safe_int(row.get("ns") or row.get("sample_ns") or row.get("seen_ns"))
    if ns > 0:
        return ns / 1_000_000_000.0
    return float(index)


class LaunchExitSentinel:
    def __init__(self, args: argparse.Namespace):
        self.entry_guard_seconds = max(0, safe_int(args.entry_guard_seconds, 120))
        self.hard_stop_bps = max(0, safe_int(args.hard_stop_bps, 1000))
        self.trailing_stop_bps = max(0, safe_int(args.trailing_stop_bps, 1500))
        self.trailing_arm_gain_bps = max(0, safe_int(args.trailing_arm_gain_bps, 0))
        self.panic_drop_10s_bps = max(0, safe_int(args.panic_drop_10s_bps, 800))
        self.panic_drop_60s_bps = max(0, safe_int(args.panic_drop_60s_bps, 1500))
        self.adaptive_floor_bps = max(0, safe_int(args.adaptive_floor_bps, 800))
        self.adaptive_noise_multiplier = max(1.0, safe_float(args.adaptive_noise_multiplier, 3.0))
        self.max_hold_seconds = max(1, safe_int(args.max_hold_seconds, 3600))
        self.sell_pressure_ratio = max(1.0, safe_float(args.sell_pressure_ratio, 1.5))
        self.net_sell_pressure_threshold = max(0, safe_int(args.net_sell_pressure_threshold, 2))
        self.large_sell_vs_median_buy_ratio = max(1.0, safe_float(args.large_sell_vs_median_buy_ratio, 2.0))
        self.max_exit_price_impact_bps = max(0, safe_int(args.max_exit_price_impact_bps, 1500))
        self.max_exit_slippage_bps = max(0, safe_int(args.max_exit_slippage_bps, 2000))
        self.liquidity_drop_bps = max(0, safe_int(args.liquidity_drop_bps, 2000))

    def window_high(self, ordered: list[tuple[int, dict[str, Any]]], ts: float, seconds: float, fallback: int) -> int:
        return max(
            [
                price_value(other)
                for other_index, other in ordered
                if 0 <= ts - row_ts(other, other_index) <= seconds and price_value(other) > 0
            ]
            or [fallback]
        )

    def recent_noise_bps(self, ordered: list[tuple[int, dict[str, Any]]], ts: float) -> int:
        recent = [
            (row_ts(other, other_index), price_value(other))
            for other_index, other in ordered
            if 0 <= ts - row_ts(other, other_index) <= 30 and price_value(other) > 0
        ]
        recent.sort()
        moves = [bps_abs_move(recent[idx - 1][1], recent[idx][1]) for idx in range(1, len(recent))]
        return median_int(moves)

    def flow_pressure_dominant(self, row: dict[str, Any]) -> bool:
        buy_count_10s = safe_int(row.get("buy_count_10s") or row.get("buy_count_recent"))
        sell_count_10s = safe_int(row.get("sell_count_10s") or row.get("sell_count_recent"))
        net_buy_pressure_10s = safe_int(row.get("net_buy_pressure_10s"), buy_count_10s - sell_count_10s)
        largest_sell_10s = safe_int(row.get("largest_sell_10s"))
        median_buy_size_10s = safe_int(row.get("median_buy_size_10s") or row.get("median_buy_10s"))
        if sell_count_10s > 0 and sell_count_10s > buy_count_10s * self.sell_pressure_ratio:
            return True
        if net_buy_pressure_10s <= -self.net_sell_pressure_threshold and sell_count_10s > 0:
            return True
        if median_buy_size_10s > 0 and largest_sell_10s >= int(median_buy_size_10s * self.large_sell_vs_median_buy_ratio):
            return True
        return False

    def evaluate(self, snapshots: list[dict[str, Any]]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
        ordered = sorted(enumerate(snapshots), key=lambda item: row_ts(item[1], item[0]))
        if not ordered:
            return {
                "status": "NO_POSITION_SNAPSHOTS",
                "exit_triggered": False,
                "exit_reason": "none",
                "rows_seen": 0,
            }, []

        entry_price = 0
        local_high = 0
        entry_ts = row_ts(ordered[0][1], ordered[0][0])
        entry_liquidity = 0
        rows_out: list[dict[str, Any]] = []
        exit_reason = ""
        exit_row: dict[str, Any] | None = None

        for index, row in ordered:
            ts = row_ts(row, index)
            price = price_value(row)
            if entry_price <= 0 and price > 0:
                entry_price = price
            if local_high <= 0:
                local_high = price
            local_high = max(local_high, price)
            liquidity = safe_int(row.get("pool_sol_liquidity_lamports") or row.get("sol_liquidity_lamports"))
            if entry_liquidity <= 0 and liquidity > 0:
                entry_liquidity = liquidity

            recent_10s_high = self.window_high(ordered, ts, 10.0, price)
            recent_60s_high = self.window_high(ordered, ts, 60.0, price)
            drawdown_from_entry_bps = bps_drop(entry_price, price)
            drawdown_from_high_bps = bps_drop(local_high, price)
            price_drop_10s_bps = bps_drop(recent_10s_high, price)
            price_drop_60s_bps = bps_drop(recent_60s_high, price)
            recent_noise_bps = self.recent_noise_bps(ordered, ts)
            adaptive_drop_threshold_bps = max(self.adaptive_floor_bps, int(recent_noise_bps * self.adaptive_noise_multiplier))
            effective_trailing_stop_bps = max(self.trailing_stop_bps, adaptive_drop_threshold_bps)
            liquidity_drop_bps = bps_drop(entry_liquidity, liquidity)
            buy_count_10s = safe_int(row.get("buy_count_10s") or row.get("buy_count_recent"))
            sell_count_10s = safe_int(row.get("sell_count_10s") or row.get("sell_count_recent"))
            net_buy_pressure_10s = safe_int(row.get("net_buy_pressure_10s"), buy_count_10s - sell_count_10s)
            largest_sell_10s = safe_int(row.get("largest_sell_10s"))
            exit_expected_out_raw = safe_int(row.get("exit_expected_out_raw") or row.get("expected_out_raw"))
            exit_quote_available = boolish(row.get("exit_quote_available"), True) and exit_expected_out_raw > 0
            exit_quote_dust = boolish(row.get("exit_quote_dust"))
            exit_price_impact_bps = safe_int(row.get("exit_price_impact_bps"))
            exit_slippage_bps = safe_int(row.get("exit_slippage_bps"))
            time_held_seconds = max(0, int(ts - entry_ts))
            peak_gain_bps = bps_gain(local_high, entry_price)

            position_state = "ENTRY_GUARD" if time_held_seconds <= self.entry_guard_seconds else "TREND_HOLD"
            if self.flow_pressure_dominant(row) or exit_price_impact_bps >= self.max_exit_price_impact_bps > 0:
                position_state = "DE_RISK"

            row_out = {
                **row,
                "entry_price": entry_price,
                "current_price": price,
                "local_high_price": local_high,
                "drawdown_from_entry_bps": drawdown_from_entry_bps,
                "drawdown_from_high_bps": drawdown_from_high_bps,
                "price_drop_10s_bps": price_drop_10s_bps,
                "price_drop_60s_bps": price_drop_60s_bps,
                "recent_noise_bps": recent_noise_bps,
                "adaptive_drop_threshold_bps": adaptive_drop_threshold_bps,
                "effective_trailing_stop_bps": effective_trailing_stop_bps,
                "buy_count_10s": buy_count_10s,
                "sell_count_10s": sell_count_10s,
                "net_buy_pressure_10s": net_buy_pressure_10s,
                "largest_sell_10s": largest_sell_10s,
                "creator_sell_seen": boolish(row.get("creator_sell_seen")) or boolish(row.get("insider_cluster_sell_seen")),
                "top_holder_sell_seen": boolish(row.get("top_holder_sell_seen")),
                "exit_quote_available": exit_quote_available,
                "exit_expected_out_raw": exit_expected_out_raw,
                "exit_price_impact_bps": exit_price_impact_bps,
                "liquidity_drop_bps": liquidity_drop_bps,
                "time_held_seconds": time_held_seconds,
                "position_state": position_state,
                "exit_reason": "",
            }
            chaos = classify_chaos(row_out)
            row_out.update(chaos)
            sell_quality = text(row_out.get("sell_quality"))
            detector_decision = text(row_out.get("exit_decision"))
            if detector_decision == "TIGHTEN_STOP":
                row_out["position_state"] = "DE_RISK"

            if detector_decision.startswith("EXIT_"):
                exit_reason = detector_decision
            elif row_out["creator_sell_seen"]:
                exit_reason = EXIT_CREATOR_SELL
            elif row_out["top_holder_sell_seen"]:
                exit_reason = EXIT_TOP_HOLDER_SELL
            elif not exit_quote_available or exit_quote_dust or exit_expected_out_raw <= 0:
                exit_reason = EXIT_EXIT_QUOTE_DETERIORATED
            elif self.hard_stop_bps > 0 and drawdown_from_entry_bps >= self.hard_stop_bps:
                exit_reason = EXIT_HARD_STOP_FROM_ENTRY
            elif self.panic_drop_10s_bps > 0 and price_drop_10s_bps >= self.panic_drop_10s_bps:
                exit_reason = EXIT_PANIC_DROP_10S
            elif self.panic_drop_60s_bps > 0 and price_drop_60s_bps >= self.panic_drop_60s_bps:
                exit_reason = EXIT_PANIC_DROP_60S
            elif (
                position_state != "ENTRY_GUARD"
                and self.trailing_stop_bps > 0
                and peak_gain_bps >= self.trailing_arm_gain_bps
                and drawdown_from_high_bps >= effective_trailing_stop_bps
            ):
                exit_reason = EXIT_TRAILING_STOP_FROM_HIGH
            elif self.flow_pressure_dominant(row) and sell_quality not in {"ABSORBED_SELL", "BENIGN_SELL", "ROUTINE_PROFIT_TAKE"}:
                exit_reason = EXIT_SELL_PRESSURE_DOMINANT
            elif self.max_exit_price_impact_bps > 0 and exit_price_impact_bps >= self.max_exit_price_impact_bps:
                exit_reason = EXIT_EXIT_QUOTE_DETERIORATED
            elif self.max_exit_slippage_bps > 0 and exit_slippage_bps >= self.max_exit_slippage_bps:
                exit_reason = EXIT_EXIT_QUOTE_DETERIORATED
            elif self.liquidity_drop_bps > 0 and liquidity_drop_bps >= self.liquidity_drop_bps:
                exit_reason = EXIT_LIQUIDITY_COLLAPSE
            elif time_held_seconds >= self.max_hold_seconds:
                exit_reason = EXIT_TTL

            if exit_reason and exit_row is None:
                row_out["position_state"] = "EXIT_NOW"
                row_out["exit_reason"] = exit_reason
                exit_row = row_out
            rows_out.append(row_out)
            if exit_row is not None:
                break

        final_row = exit_row or rows_out[-1]
        return {
            "status": "EXIT_TRIGGERED" if exit_row else "HOLD_VALID",
            "exit_triggered": exit_row is not None,
            "exit_reason": text(final_row.get("exit_reason")) or "none",
            "exit_decision": text(final_row.get("exit_decision")) or "HOLD_NORMAL",
            "sell_quality": text(final_row.get("sell_quality")) or "NO_SELL",
            "chaos_score": safe_int(final_row.get("chaos_score")),
            "catastrophe_score": safe_int(final_row.get("catastrophe_score")),
            "toxic_sell_score": safe_int(final_row.get("toxic_sell_score")),
            "post_sell_absorption_score": safe_int(final_row.get("post_sell_absorption_score")),
            "exit_risk_score": safe_int(final_row.get("exit_risk_score")),
            "rows_seen": len(snapshots),
            "rows_evaluated": len(rows_out),
            "entry_price": entry_price,
            "current_price": safe_int(final_row.get("current_price")),
            "local_high_price": safe_int(final_row.get("local_high_price")),
            "drawdown_from_entry_bps": safe_int(final_row.get("drawdown_from_entry_bps")),
            "drawdown_from_high_bps": safe_int(final_row.get("drawdown_from_high_bps")),
            "price_drop_10s_bps": safe_int(final_row.get("price_drop_10s_bps")),
            "price_drop_60s_bps": safe_int(final_row.get("price_drop_60s_bps")),
            "recent_noise_bps": safe_int(final_row.get("recent_noise_bps")),
            "adaptive_drop_threshold_bps": safe_int(final_row.get("adaptive_drop_threshold_bps")),
            "buy_count_10s": safe_int(final_row.get("buy_count_10s")),
            "sell_count_10s": safe_int(final_row.get("sell_count_10s")),
            "net_buy_pressure_10s": safe_int(final_row.get("net_buy_pressure_10s")),
            "exit_quote_available": boolish(final_row.get("exit_quote_available")),
            "exit_expected_out_raw": safe_int(final_row.get("exit_expected_out_raw")),
            "exit_price_impact_bps": safe_int(final_row.get("exit_price_impact_bps")),
            "liquidity_drop_bps": safe_int(final_row.get("liquidity_drop_bps")),
            "position_state": text(final_row.get("position_state")),
            "time_held_seconds": safe_int(final_row.get("time_held_seconds")),
            "generated_at_unix": int(time.time()),
        }, rows_out


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD position monitor exit sentinel.")
    parser.add_argument("--snapshots-jsonl", required=True)
    parser.add_argument("--summary-json", required=True)
    parser.add_argument("--rows-jsonl", required=True)
    parser.add_argument("--entry-guard-seconds", default="120")
    parser.add_argument("--hard-stop-bps", default="1000")
    parser.add_argument("--trailing-stop-bps", default="1500")
    parser.add_argument("--trailing-arm-gain-bps", default="0")
    parser.add_argument("--panic-drop-10s-bps", default="800")
    parser.add_argument("--panic-drop-60s-bps", default="1500")
    parser.add_argument("--adaptive-floor-bps", default="800")
    parser.add_argument("--adaptive-noise-multiplier", default="3")
    parser.add_argument("--sell-pressure-ratio", default="1.5")
    parser.add_argument("--net-sell-pressure-threshold", default="2")
    parser.add_argument("--large-sell-vs-median-buy-ratio", default="2")
    parser.add_argument("--max-hold-seconds", default="3600")
    parser.add_argument("--max-exit-price-impact-bps", default="1500")
    parser.add_argument("--max-exit-slippage-bps", default="2000")
    parser.add_argument("--liquidity-drop-bps", default="2000")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    snapshots = load_jsonl(Path(args.snapshots_jsonl))
    summary, rows = LaunchExitSentinel(args).evaluate(snapshots)
    Path(args.summary_json).parent.mkdir(parents=True, exist_ok=True)
    Path(args.summary_json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    Path(args.rows_jsonl).write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )
    print("\n".join(f"{key}={value}" for key, value in summary.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

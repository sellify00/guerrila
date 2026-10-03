#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ENTRY_DELAYS: list[tuple[str, int]] = [
    ("create_plus_0ms", 0),
    ("first_buy_plus_250ms", 250),
    ("first_buy_plus_500ms", 500),
    ("first_buy_plus_1s", 1_000),
    ("first_buy_plus_2s", 2_000),
    ("first_buy_plus_5s", 5_000),
    ("warmup_10_30s", 10_000),
]


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


def load_any(path: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    if not path.exists():
        return [], {}
    raw = path.read_text(encoding="utf-8", errors="replace").strip()
    if not raw:
        return [], {}
    if raw.startswith("{") or raw.startswith("["):
        try:
            doc = json.loads(raw)
        except Exception:
            doc = None
        if isinstance(doc, list):
            return [row for row in doc if isinstance(row, dict)], {}
        if isinstance(doc, dict):
            for key in ("analyses", "rows", "samples", "data"):
                value = doc.get(key)
                if isinstance(value, list):
                    return [row for row in value if isinstance(row, dict)], doc
            return [doc], doc
    rows: list[dict[str, Any]] = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except Exception:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows, {}


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


def bps(current: float, entry: float) -> int:
    if entry <= 0 or current <= 0:
        return 0
    return int(round(((current - entry) / entry) * 10_000.0))


def price_from_return(create_price: float, return_fraction: Any) -> float:
    if create_price <= 0 or return_fraction in (None, ""):
        return 0.0
    return create_price * (1.0 + safe_float(return_fraction))


def aggregate_entry_price(row: dict[str, Any], label: str) -> tuple[float, str]:
    create_price = safe_float(row.get("createPrice"))
    first_price = safe_float(row.get("firstPrice"))
    if label == "create_plus_0ms":
        return create_price, "create_price"
    if label in {"first_buy_plus_250ms", "first_buy_plus_500ms", "first_buy_plus_1s", "first_buy_plus_2s"}:
        return first_price or create_price, "first_trade_price"
    if label == "first_buy_plus_5s":
        for key in ("trade5ReturnVsCreate", "trade10ReturnVsCreate", "returnAt60sVsCreate"):
            value = price_from_return(create_price, row.get(key))
            if value > 0:
                return value, key
        return first_price or create_price, "fallback_first_trade_price"
    if label == "warmup_10_30s":
        for key in ("returnAt60sVsCreate", "trade10ReturnVsCreate", "trade5ReturnVsCreate"):
            value = price_from_return(create_price, row.get(key))
            if value > 0:
                return value, key
        return first_price or create_price, "fallback_first_trade_price"
    return first_price or create_price, "fallback"


def aggregate_rows(rows: list[dict[str, Any]], args: argparse.Namespace) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    spend = args.max_spend_lamports
    for row in rows:
        mint = text(row.get("mint"))
        if not mint:
            continue
        peak_price = safe_float(row.get("peakPrice"))
        create_price = safe_float(row.get("createPrice"))
        first_price = safe_float(row.get("firstPrice"))
        time_to_peak_ms = safe_int(row.get("secondsToPeak"), safe_int(row.get("timeToPeakSeconds"), 0)) * 1000
        first_trade_after_create_ms = int(round(safe_float(row.get("firstTradeSecondsAfterCreate")) * 1000.0))
        first_sell_after_create_ms = int(round(safe_float(row.get("firstSellSecondsAfterCreate"), -1.0) * 1000.0))
        completed = boolish(row.get("completedWithinWindow"))
        for label, delay_ms in ENTRY_DELAYS:
            entry_price, entry_price_source = aggregate_entry_price(row, label)
            if entry_price <= 0 or peak_price <= 0:
                continue
            entry_anchor_ms = delay_ms
            if label.startswith("first_buy"):
                entry_anchor_ms = first_trade_after_create_ms + delay_ms
            peak_gain_bps = bps(peak_price, entry_price)
            first_trade_gain_bps = bps(first_price, create_price) if create_price > 0 and first_price > 0 else 0
            max_adverse = bps(safe_float(row.get("troughPrice")), entry_price)
            gross_opportunity = (spend * peak_gain_bps) // 10_000
            net_after_cost = gross_opportunity - args.true_cost_floor_lamports
            break_even_bps = int(round((args.true_cost_floor_lamports / max(1, spend)) * 10_000.0))
            capture = peak_gain_bps >= break_even_bps + args.profit_buffer_bps
            if completed:
                exit_reason = "SIM_CURVE_COMPLETED_OR_MIGRATED"
            elif capture:
                exit_reason = "SIM_COST_PROTECTOR_TRAILING"
            elif peak_gain_bps <= 0:
                exit_reason = "SIM_NO_USABLE_EXTENSION"
            else:
                exit_reason = "SIM_WEAK_EXTENSION"
            no_exit_tail = False
            if completed and not args.treat_completion_as_no_exit:
                no_exit_tail = False
            out.append(
                {
                    "record_type": "launch54_birth_ramp_capture_sim",
                    "sim_basis": "aggregate_probe_analysis",
                    "mint": mint,
                    "symbol": text(row.get("symbol")),
                    "bonding_curve": text(row.get("bondingCurve") or row.get("bonding_curve")),
                    "entry_delay": label,
                    "entry_delay_ms": entry_anchor_ms,
                    "entry_price": entry_price,
                    "entry_price_source": entry_price_source,
                    "create_price": create_price,
                    "first_trade_price": first_price,
                    "first_trade_gain_bps": first_trade_gain_bps,
                    "peak_price": peak_price,
                    "peak_executable_exit_gain_bps": peak_gain_bps,
                    "max_adverse_excursion_bps": max_adverse,
                    "exit_reason": exit_reason,
                    "time_to_peak_ms": time_to_peak_ms,
                    "time_to_first_exit_deterioration_ms": first_sell_after_create_ms if first_sell_after_create_ms >= 0 else None,
                    "net_pnl_after_fees_tips_rent": net_after_cost,
                    "no_exit_tail": no_exit_tail,
                    "capture_rate_hit": capture,
                    "break_even_bps": break_even_bps,
                    "profit_buffer_bps": args.profit_buffer_bps,
                }
            )
    return out


def point_ts(row: dict[str, Any], index: int) -> int:
    for key in ("seen_ns", "sample_ns", "ns", "target_receive_ns", "provider_created_at_ns"):
        value = safe_int(row.get(key))
        if value > 0:
            return value // 1_000_000
    for key in ("ts_ms", "timestamp_ms", "sample_time_ms"):
        value = safe_int(row.get(key))
        if value > 0:
            return value
    for key in ("ts", "timestamp", "unix_ts", "sample_time"):
        value = safe_float(row.get(key))
        if value > 0:
            return int(round(value * 1000.0))
    return index


def point_price(row: dict[str, Any]) -> float:
    for key in (
        "executable_exit_value_lamports",
        "exit_expected_out_raw",
        "price_lamports",
        "current_price_lamports",
        "price_raw",
        "price",
        "current_price",
    ):
        value = safe_float(row.get(key))
        if value > 0:
            return value
    return 0.0


def group_key(row: dict[str, Any], index: int) -> str:
    for key in ("mint", "launch_mint", "selected_mint", "launch_state_id", "candidate_id", "bonding_curve", "pool"):
        value = text(row.get(key))
        if value:
            return value
    return f"row_{index}"


def timeseries_rows(rows: list[dict[str, Any]], args: argparse.Namespace) -> list[dict[str, Any]]:
    grouped: dict[str, list[tuple[int, dict[str, Any]]]] = defaultdict(list)
    for index, row in enumerate(rows):
        grouped[group_key(row, index)].append((index, row))
    out: list[dict[str, Any]] = []
    for key, items in grouped.items():
        ordered = sorted(items, key=lambda item: point_ts(item[1], item[0]))
        priced = [(point_ts(row, idx), row, point_price(row)) for idx, row in ordered if point_price(row) > 0]
        if len(priced) < 2:
            continue
        birth_ms = min(point_ts(row, idx) for idx, row in ordered)
        buy_times = [point_ts(row, idx) for idx, row in ordered if text(row.get("last_trade_side")) == "buy" or text(row.get("source_kind")).endswith("BUY")]
        first_buy_ms = min(buy_times) if buy_times else birth_ms
        for label, delay_ms in ENTRY_DELAYS:
            entry_target = birth_ms + delay_ms if label == "create_plus_0ms" else first_buy_ms + delay_ms
            entry_candidates = [(ts, row, price) for ts, row, price in priced if ts >= entry_target]
            if not entry_candidates:
                continue
            entry_ts, entry_row, entry_price = entry_candidates[0]
            future = [(ts, row, price) for ts, row, price in priced if ts >= entry_ts]
            peak_ts, peak_row, peak_price = max(future, key=lambda item: item[2])
            trough_price = min(price for _ts, _row, price in future)
            peak_gain_bps = bps(peak_price, entry_price)
            max_adverse = min(0, bps(trough_price, entry_price))
            gross = (args.max_spend_lamports * peak_gain_bps) // 10_000
            net = gross - args.true_cost_floor_lamports
            break_even_bps = int(round((args.true_cost_floor_lamports / max(1, args.max_spend_lamports)) * 10_000.0))
            capture = peak_gain_bps >= break_even_bps + args.profit_buffer_bps
            exit_reason = "SIM_COST_PROTECTOR_TRAILING" if capture else "SIM_WEAK_EXTENSION"
            out.append(
                {
                    "record_type": "launch54_birth_ramp_capture_sim",
                    "sim_basis": "timeseries_jsonl",
                    "mint": text(entry_row.get("mint") or entry_row.get("selected_mint") or key),
                    "bonding_curve": text(entry_row.get("bonding_curve") or entry_row.get("pool")),
                    "entry_delay": label,
                    "entry_delay_ms": delay_ms,
                    "entry_price": entry_price,
                    "entry_price_source": "first_sample_at_or_after_delay",
                    "peak_price": peak_price,
                    "peak_executable_exit_gain_bps": peak_gain_bps,
                    "max_adverse_excursion_bps": max_adverse,
                    "exit_reason": exit_reason,
                    "time_to_peak_ms": max(0, peak_ts - entry_ts),
                    "time_to_first_exit_deterioration_ms": None,
                    "net_pnl_after_fees_tips_rent": net,
                    "no_exit_tail": False,
                    "capture_rate_hit": capture,
                    "break_even_bps": break_even_bps,
                    "profit_buffer_bps": args.profit_buffer_bps,
                }
            )
    return out


def percentile(values: list[int], pct: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * pct))))
    return ordered[index]


def build_summary(args: argparse.Namespace, rows: list[dict[str, Any]], input_doc: dict[str, Any]) -> dict[str, Any]:
    by_delay: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        by_delay[text(row.get("entry_delay"))].append(row)
    delay_summaries: dict[str, dict[str, Any]] = {}
    for delay, delay_rows in by_delay.items():
        pnl_values = [safe_int(row.get("net_pnl_after_fees_tips_rent")) for row in delay_rows]
        gain_values = [safe_int(row.get("peak_executable_exit_gain_bps")) for row in delay_rows]
        captures = [row for row in delay_rows if boolish(row.get("capture_rate_hit"))]
        delay_summaries[delay] = {
            "rows": len(delay_rows),
            "capture_count": len(captures),
            "capture_rate": round(len(captures) / max(1, len(delay_rows)), 4),
            "median_peak_executable_exit_gain_bps": percentile(gain_values, 0.5),
            "p75_peak_executable_exit_gain_bps": percentile(gain_values, 0.75),
            "median_net_pnl_after_fees_tips_rent": percentile(pnl_values, 0.5),
            "positive_net_count": sum(1 for value in pnl_values if value > 0),
            "no_exit_tail_count": sum(1 for row in delay_rows if boolish(row.get("no_exit_tail"))),
        }
    best_delay = "none"
    best_capture = -1.0
    for delay, summary in delay_summaries.items():
        capture = safe_float(summary.get("capture_rate"), 0.0)
        if capture > best_capture:
            best_delay = delay
            best_capture = capture
    capture_rows = [row for row in rows if boolish(row.get("capture_rate_hit"))]
    status = "LAUNCH54_BIRTH_RAMP_CAPTURE_SIM_PASS" if capture_rows else "PARK_LAUNCH54_NO_CAPTURE_AFTER_COST"
    return {
        "status": status,
        "input_path": str(args.input_path),
        "input_rows_seen": safe_int(input_doc.get("sampledMints"), len(rows)) if input_doc else len(rows),
        "sim_rows": len(rows),
        "unique_mints": len({text(row.get("mint")) for row in rows if text(row.get("mint"))}),
        "max_spend_lamports": args.max_spend_lamports,
        "true_cost_floor_lamports": args.true_cost_floor_lamports,
        "profit_buffer_bps": args.profit_buffer_bps,
        "best_capture_delay": best_delay,
        "best_capture_rate": best_capture if best_capture >= 0 else 0,
        "capture_count": len(capture_rows),
        "no_exit_tail_count": sum(1 for row in rows if boolish(row.get("no_exit_tail"))),
        "exit_reason_counts": dict(Counter(text(row.get("exit_reason")) for row in rows)),
        "delay_summaries": delay_summaries,
        "source_method": input_doc.get("method") if isinstance(input_doc.get("method"), dict) else {},
        "rows_path": str(args.rows_jsonl),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(capture_rows),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch54 birth-ramp capture simulator.")
    parser.add_argument("--input-path", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    parser.add_argument("--rows-jsonl", type=Path, required=True)
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--true-cost-floor-lamports", type=int, default=1_640_000)
    parser.add_argument("--profit-buffer-bps", type=int, default=500)
    parser.add_argument("--treat-completion-as-no-exit", action="store_true")
    args = parser.parse_args()

    raw_rows, input_doc = load_any(args.input_path)
    if raw_rows and ("createPrice" in raw_rows[0] or "peakPrice" in raw_rows[0]):
        sim_rows = aggregate_rows(raw_rows, args)
    else:
        sim_rows = timeseries_rows(raw_rows, args)
    summary = build_summary(args, sim_rows, input_doc)
    write_jsonl(args.rows_jsonl, sim_rows)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    for key in (
        "status",
        "input_rows_seen",
        "sim_rows",
        "unique_mints",
        "best_capture_delay",
        "best_capture_rate",
        "capture_count",
        "no_exit_tail_count",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

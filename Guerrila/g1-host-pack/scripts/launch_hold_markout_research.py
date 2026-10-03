from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path
from typing import Any


HORIZONS = [
    ("return_1m_bps", 60),
    ("return_5m_bps", 300),
    ("return_15m_bps", 900),
    ("return_1h_bps", 3600),
    ("return_4h_bps", 14400),
    ("return_24h_bps", 86400),
    ("return_48h_bps", 172800),
    ("return_72h_bps", 259200),
]


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


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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


def row_ts(row: dict[str, Any], fallback: int) -> float:
    for key in ("ts", "unix_ts", "sample_time"):
        value = safe_float(row.get(key))
        if value > 0:
            return value
    ns = safe_int(row.get("ns") or row.get("sample_ns") or row.get("seen_ns"))
    if ns > 0:
        return ns / 1_000_000_000.0
    return float(fallback)


def price_value(row: dict[str, Any]) -> int:
    for key in ("price_lamports", "current_price_lamports", "exit_expected_out_raw", "expected_out_raw", "price_raw"):
        value = safe_int(row.get(key))
        if value > 0:
            return value
    return 0


def bps_change(current: int, entry: int) -> int:
    if entry <= 0:
        return 0
    return ((current - entry) * 10_000) // entry


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return ((high_value - current_value) * 10_000) // high_value


def candidate_key(row: dict[str, Any], index: int) -> str:
    for key in ("candidate_id", "launch_group_id", "mint", "launch_mint", "selected_mint"):
        value = text(row.get(key))
        if value:
            return value
    return f"row_{index}"


def analyze(rows: list[dict[str, Any]]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    grouped: dict[str, list[tuple[int, dict[str, Any]]]] = defaultdict(list)
    for index, row in enumerate(rows):
        grouped[candidate_key(row, index)].append((index, row))

    out_rows = []
    for key, items in grouped.items():
        ordered = sorted(items, key=lambda item: row_ts(item[1], item[0]))
        entry_ts = row_ts(ordered[0][1], ordered[0][0])
        entry_price = price_value(ordered[0][1])
        if entry_price <= 0:
            continue
        peak_price = entry_price
        trough_before_peak = entry_price
        time_to_peak = 0
        first_major_drawdown_seconds = None
        horizon_returns: dict[str, int | None] = {name: None for name, _ in HORIZONS}

        for index, row in ordered:
            ts = row_ts(row, index)
            price = price_value(row)
            if price <= 0:
                continue
            if price > peak_price:
                peak_price = price
                time_to_peak = int(ts - entry_ts)
            if ts <= entry_ts + time_to_peak:
                trough_before_peak = min(trough_before_peak, price)
            if first_major_drawdown_seconds is None and bps_drop(peak_price, price) >= 1000:
                first_major_drawdown_seconds = int(ts - entry_ts)
            for name, horizon in HORIZONS:
                if horizon_returns[name] is None and ts >= entry_ts + horizon:
                    horizon_returns[name] = bps_change(price, entry_price)

        result = {
            "candidate_id": key,
            "entry_price": entry_price,
            "peak_price": peak_price,
            "peak_return_bps": bps_change(peak_price, entry_price),
            "max_drawdown_before_peak_bps": bps_drop(entry_price, trough_before_peak),
            "time_to_peak_seconds": time_to_peak,
            "time_to_first_major_drawdown_seconds": first_major_drawdown_seconds,
            "sample_count": len(ordered),
        }
        result.update(horizon_returns)
        out_rows.append(result)

    summary = {
        "status": "HOLD_MARKOUT_READY" if out_rows else "NO_HOLD_MARKOUT_ROWS",
        "input_rows_seen": len(rows),
        "candidate_count": len(out_rows),
        "return_1m_available_count": sum(1 for row in out_rows if row.get("return_1m_bps") is not None),
        "return_5m_available_count": sum(1 for row in out_rows if row.get("return_5m_bps") is not None),
        "return_15m_available_count": sum(1 for row in out_rows if row.get("return_15m_bps") is not None),
        "return_1h_available_count": sum(1 for row in out_rows if row.get("return_1h_bps") is not None),
        "return_24h_available_count": sum(1 for row in out_rows if row.get("return_24h_bps") is not None),
    }
    return summary, out_rows


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Launch R2-HOLD long-horizon markout research.")
    parser.add_argument("--input-jsonl", required=True)
    parser.add_argument("--summary-json", required=True)
    parser.add_argument("--rows-jsonl", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    summary, rows = analyze(load_jsonl(Path(args.input_jsonl)))
    Path(args.summary_json).parent.mkdir(parents=True, exist_ok=True)
    Path(args.summary_json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    Path(args.rows_jsonl).write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")
    print("\n".join(f"{key}={value}" for key, value in summary.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

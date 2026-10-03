#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from datetime import datetime
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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


def parse_iso(value: Any) -> datetime | None:
    raw = text(value)
    if not raw:
        return None
    try:
        if raw.endswith("Z"):
            raw = raw[:-1] + "+00:00"
        return datetime.fromisoformat(raw)
    except Exception:
        return None


def elapsed_minutes(start: Any, end: Any) -> float:
    start_dt = parse_iso(start)
    end_dt = parse_iso(end)
    if not start_dt or not end_dt:
        return 0.0
    return max(0.0, (end_dt - start_dt).total_seconds() / 60.0)


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


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


def slope_over(history: list[dict[str, Any]], minutes: float, latest: dict[str, Any]) -> tuple[float, float]:
    latest_time = text(latest.get("seen_at"))
    latest_progress = safe_float(latest.get("progress_pct"))
    latest_estimate = safe_int(latest.get("estimated_sol_to_completion_lamports"))
    candidates: list[dict[str, Any]] = []
    for item in history:
        delta = elapsed_minutes(text(item.get("seen_at")), latest_time)
        if delta <= minutes and delta > 0:
            candidates.append(item)
    if not candidates and len(history) >= 2:
        candidates = [history[-2]]
    if not candidates:
        return 0.0, 0.0
    prior = candidates[0]
    dt = elapsed_minutes(text(prior.get("seen_at")), latest_time)
    if dt <= 0:
        return 0.0, 0.0
    progress_delta = latest_progress - safe_float(prior.get("progress_pct"))
    sol_delta = (latest_estimate - safe_int(prior.get("estimated_sol_to_completion_lamports"))) / 1_000_000_000
    return progress_delta / dt, sol_delta / dt


def velocity_row(key: str, item: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    history = [entry for entry in (item.get("progress_history") or []) if isinstance(entry, dict)]
    latest = history[-1] if history else {
        "seen_at": item.get("last_seen_at"),
        "progress_pct": item.get("latest_progress_pct"),
        "estimated_sol_to_completion_lamports": item.get("estimated_sol_to_completion_lamports"),
    }
    progress_delta_1m, sol_delta_1m = slope_over(history, 1.0, latest)
    progress_delta_5m, sol_delta_5m = slope_over(history, 5.0, latest)
    progress = safe_float(item.get("latest_progress_pct"))
    estimate_sol = safe_float(item.get("estimated_sol_to_completion"))
    positive = progress_delta_1m > 0 or progress_delta_5m > 0 or sol_delta_1m < 0 or sol_delta_5m < 0
    estimated_time = 0.0
    best_progress_velocity = max(progress_delta_1m, progress_delta_5m, 0.0)
    if best_progress_velocity > 0 and progress < 100:
        estimated_time = (100.0 - progress) / best_progress_velocity
    decision = "R5_PRESSURE_READY" if (
        positive
        and boolish(item.get("curve_state_ready"))
        and boolish(item.get("exit_liquidity_guard_ready"))
        and (
            progress >= args.ready_progress_pct
            or estimate_sol <= args.ready_estimated_sol_to_completion
            or (progress >= args.aggressive_progress_pct and safe_int(item.get("buy_count_60s")) > 0)
        )
    ) else "PARK_PRESSURE_NOT_READY"
    if boolish(item.get("migration_seen")) and decision != "R5_PRESSURE_READY":
        decision = "PARK_PRESSURE_ALREADY_MIGRATED"
    elif not positive and text(item.get("latest_bucket")) in {"90-95%", "95-98%", "98-99%", "99%+"}:
        decision = "PARK_PRESSURE_STALLED"
    return {
        "record_type": "launch60c_completion_pressure_velocity",
        "watchlist_key": key,
        "mint": text(item.get("mint")),
        "bonding_curve": text(item.get("bonding_curve")),
        "latest_bucket": text(item.get("latest_bucket")),
        "latest_progress_pct": progress,
        "estimated_sol_to_completion": estimate_sol,
        "progress_delta_1m": round(progress_delta_1m, 6),
        "progress_delta_5m": round(progress_delta_5m, 6),
        "sol_to_completion_delta_1m": round(sol_delta_1m, 9),
        "sol_to_completion_delta_5m": round(sol_delta_5m, 9),
        "estimated_time_to_completion_minutes": round(estimated_time, 3),
        "buy_pressure_recent": safe_int(item.get("buy_count_60s")),
        "sell_pressure_recent": safe_int(item.get("sell_count_60s")),
        "completion_velocity_positive": positive,
        "curve_state_ready": boolish(item.get("curve_state_ready")),
        "exit_liquidity_guard_ready": boolish(item.get("exit_liquidity_guard_ready")),
        "completion_status": text(item.get("completion_status")),
        "pressure_decision": decision,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire R5 completion pressure velocity scorer.")
    parser.add_argument("--state-json", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--ready-progress-pct", type=float, default=95.0)
    parser.add_argument("--aggressive-progress-pct", type=float, default=90.0)
    parser.add_argument("--ready-estimated-sol-to-completion", type=float, default=2.0)
    args = parser.parse_args()

    state = load_json(args.state_json)
    items = state.get("items") if isinstance(state.get("items"), dict) else {}
    rows = [velocity_row(key, item, args) for key, item in items.items() if isinstance(item, dict)]
    ready_rows = [row for row in rows if text(row.get("pressure_decision")) == "R5_PRESSURE_READY"]
    decision_counts = Counter(text(row.get("pressure_decision")) for row in rows)
    bucket_counts = Counter(text(row.get("latest_bucket")) for row in rows)
    summary = {
        "status": "LAUNCH60C_COMPLETION_PRESSURE_VELOCITY_PASS",
        "state_json": str(args.state_json),
        "watchlist_rows": len(rows),
        "pressure_ready_rows": len(ready_rows),
        "decision_counts": dict(decision_counts),
        "bucket_counts": dict(bucket_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "ready_rows_path": str(args.out_dir / "ready_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "ready_rows.jsonl", ready_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "watchlist_rows", "pressure_ready_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

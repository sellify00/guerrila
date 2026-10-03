#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


TRACK_BUCKETS = ("70-80%", "80-90%", "90-95%", "95-98%", "98-99%", "99%+")


def now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


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


def seconds_between(start: Any, end: Any) -> float:
    start_dt = parse_iso(start)
    end_dt = parse_iso(end)
    if not start_dt or not end_dt:
        return 0.0
    return max(0.0, (end_dt - start_dt).total_seconds())


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


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


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


def bucket_for(progress: float) -> str:
    if progress >= 99.0:
        return "99%+"
    if progress >= 98.0:
        return "98-99%"
    if progress >= 95.0:
        return "95-98%"
    if progress >= 90.0:
        return "90-95%"
    if progress >= 80.0:
        return "80-90%"
    if progress >= 70.0:
        return "70-80%"
    return "<70%"


def key_for(row: dict[str, Any]) -> str:
    mint = text(row.get("mint") or row.get("launch_mint"))
    curve = text(row.get("bonding_curve") or row.get("pool_or_curve"))
    if mint and curve:
        return f"{mint}|{curve}"
    return mint or curve or text(row.get("launch_state_id") or row.get("source_signature"))


def index_by_keys(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        key = key_for(row)
        if key:
            out[key] = row
        mint = text(row.get("mint") or row.get("launch_mint"))
        if mint:
            out.setdefault(mint, row)
    return out


def migration_index(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not (
            row.get("migration_instruction_seen")
            or row.get("complete_pump_amm_migration_event_seen")
            or text(row.get("pumpswap_pool"))
        ):
            continue
        for key in {key_for(row), text(row.get("mint"))}:
            if key:
                out[key] = row
    return out


def load_state(path: Path) -> dict[str, Any]:
    state = load_json(path)
    if not state:
        return {"version": 1, "items": {}}
    if "items" not in state or not isinstance(state.get("items"), dict):
        return {"version": 1, "items": {}}
    return state


def update_item(
    existing: dict[str, Any],
    row: dict[str, Any],
    buy: dict[str, Any],
    migration: dict[str, Any],
    current_iso: str,
    run_root: str,
) -> dict[str, Any]:
    progress = safe_float(row.get("curve_progress_pct"))
    bucket = bucket_for(progress)
    prior_bucket = text(existing.get("latest_bucket"))
    first_seen_at = text(existing.get("first_seen_at")) or current_iso
    bucket_entered_at = text(existing.get("bucket_entered_at")) if prior_bucket == bucket else current_iso
    latest_buy_time = text(buy.get("latest_buy_time") or existing.get("last_buy_time"))
    latest_sell_time = text(buy.get("latest_sell_time") or existing.get("last_sell_time"))
    migration_seen = boolish(migration.get("migration_instruction_seen") or migration.get("complete_pump_amm_migration_event_seen"))
    pool = text(migration.get("pumpswap_pool") or existing.get("pumpswap_pool"))
    estimate_lamports = safe_int(row.get("estimated_sol_to_completion_lamports"))
    history = [item for item in (existing.get("progress_history") or []) if isinstance(item, dict)]
    previous = history[-1] if history else {}
    previous_progress = safe_float(previous.get("progress_pct"), progress)
    previous_estimate = safe_int(previous.get("estimated_sol_to_completion_lamports"), estimate_lamports)
    previous_time = text(previous.get("seen_at") or existing.get("last_seen_at") or current_iso)
    elapsed_minutes = seconds_between(previous_time, current_iso) / 60.0
    progress_delta = progress - previous_progress
    progress_delta_per_min = (progress_delta / elapsed_minutes) if elapsed_minutes > 0 else 0.0
    sol_to_completion_delta = (estimate_lamports - previous_estimate) / 1_000_000_000
    time_in_bucket_seconds = seconds_between(bucket_entered_at, current_iso)
    curve_state_ready = boolish(row.get("curve_account_exists") or row.get("bonding_curve_account_fetch_ok"))
    exit_liquidity_ready = boolish(row.get("exit_liquidity_guard_ready"))
    buy_count_60s = safe_int(buy.get("buy_count_60s"))
    sell_count_60s = safe_int(buy.get("sell_count_60s"))
    if progress >= 95.0 and curve_state_ready and exit_liquidity_ready and buy_count_60s > 0:
        status = "R5_PRE_MIGRATION_READY"
    elif progress >= 90.0 and progress_delta_per_min > 0 and curve_state_ready and exit_liquidity_ready and buy_count_60s > 0:
        status = "R5_PRE_MIGRATION_READY"
    elif migration_seen:
        status = "PARK_MIGRATED_WITHOUT_ENTRY"
    elif progress >= 90.0 and not (curve_state_ready and exit_liquidity_ready):
        status = "PARK_EXIT_LIQUIDITY"
    elif progress >= 90.0 and progress_delta_per_min <= 0 and buy_count_60s <= 0:
        status = "PARK_STALLED_PROGRESS"
    elif bucket in {"70-80%", "80-90%", "90-95%"}:
        status = f"WATCH_{bucket.replace('-', '_').replace('%', '')}"
    else:
        status = "WATCH_COMPLETION"
    history.append(
        {
            "seen_at": current_iso,
            "progress_pct": progress,
            "bucket": bucket,
            "estimated_sol_to_completion_lamports": estimate_lamports,
            "buy_count_60s": buy_count_60s,
            "sell_count_60s": sell_count_60s,
            "run_root": run_root,
        }
    )
    history = history[-50:]
    return {
        **existing,
        "mint": text(row.get("mint") or row.get("launch_mint") or existing.get("mint")),
        "bonding_curve": text(row.get("bonding_curve") or row.get("pool_or_curve") or existing.get("bonding_curve")),
        "first_seen_at": first_seen_at,
        "last_seen_at": current_iso,
        "first_seen_progress_pct": safe_float(existing.get("first_seen_progress_pct"), progress),
        "latest_progress_pct": progress,
        "first_seen_bucket": text(existing.get("first_seen_bucket")) or bucket,
        "latest_bucket": bucket,
        "bucket_entered_at": bucket_entered_at,
        "time_in_bucket_seconds": int(time_in_bucket_seconds),
        "time_in_bucket_minutes": round(time_in_bucket_seconds / 60.0, 3),
        "estimated_sol_to_completion_lamports": estimate_lamports,
        "estimated_sol_to_completion": round(estimate_lamports / 1_000_000_000, 9),
        "curve_state_ready": curve_state_ready,
        "exit_liquidity_guard_ready": exit_liquidity_ready,
        "last_buy_time": latest_buy_time,
        "last_sell_time": latest_sell_time,
        "last_buy_seen": latest_buy_time,
        "last_sell_seen": latest_sell_time,
        "buy_count_60s": buy_count_60s,
        "sell_count_60s": sell_count_60s,
        "net_buy_pressure_60s": safe_int(buy.get("net_buy_pressure_60s")),
        "progress_delta": round(progress_delta, 6),
        "progress_delta_per_min": round(progress_delta_per_min, 6),
        "sol_to_completion_delta": round(sol_to_completion_delta, 9),
        "completion_velocity_positive": progress_delta_per_min > 0 or sol_to_completion_delta < 0,
        "completion_status": status,
        "migration_seen": migration_seen or boolish(existing.get("migration_seen")),
        "pumpswap_pool_joined": bool(pool),
        "pumpswap_pool": pool,
        "progress_history": history,
        "last_run_root": run_root,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Persistent no-wire R5 pre-migration watchlist updater.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--buy-resolver-rows", type=Path)
    parser.add_argument("--migration-rows", type=Path)
    parser.add_argument("--state-json", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--current-time-iso", default="")
    parser.add_argument("--min-track-progress-pct", type=float, default=70.0)
    args = parser.parse_args()

    current_iso = args.current_time_iso or now_iso()
    completion_rows = load_jsonl(args.completion_rows)
    buy_by_key = index_by_keys(load_jsonl(args.buy_resolver_rows))
    migration_by_key = migration_index(load_jsonl(args.migration_rows))
    state = load_state(args.state_json)
    items: dict[str, Any] = dict(state.get("items") or {})
    tracked_this_run = 0

    for row in completion_rows:
        progress = safe_float(row.get("curve_progress_pct"))
        if progress < args.min_track_progress_pct:
            continue
        bucket = bucket_for(progress)
        if bucket not in TRACK_BUCKETS:
            continue
        key = key_for(row)
        if not key:
            continue
        buy = buy_by_key.get(key) or buy_by_key.get(text(row.get("mint"))) or {}
        migration = migration_by_key.get(key) or migration_by_key.get(text(row.get("mint"))) or {}
        items[key] = update_item(dict(items.get(key) or {}), row, buy, migration, current_iso, str(args.run_root))
        tracked_this_run += 1

    active_items = [dict(item, watchlist_key=key) for key, item in items.items()]
    bucket_counts = Counter(text(item.get("latest_bucket")) for item in active_items)
    readyish = [item for item in active_items if text(item.get("completion_status")) == "R5_PRE_MIGRATION_READY"]
    status_counts = Counter(text(item.get("completion_status")) for item in active_items)
    updated_state = {
        "version": 1,
        "updated_at": current_iso,
        "last_run_root": str(args.run_root),
        "items": items,
    }
    summary = {
        "status": "LAUNCH62D_PRE_MIGRATION_PERSISTENT_WATCHLIST_PASS",
        "run_root": str(args.run_root),
        "state_json": str(args.state_json),
        "tracked_this_run": tracked_this_run,
        "watchlist_size": len(items),
        "bucket_counts": dict(bucket_counts),
        "status_counts": dict(status_counts),
        "progress_70_80": safe_int(bucket_counts.get("70-80%")),
        "progress_80_90": safe_int(bucket_counts.get("80-90%")),
        "progress_90_95": safe_int(bucket_counts.get("90-95%")),
        "progress_95_98": safe_int(bucket_counts.get("95-98%")),
        "progress_98_99": safe_int(bucket_counts.get("98-99%")),
        "progress_99_plus": safe_int(bucket_counts.get("99%+")),
        "recent_buy_watchlist_rows": sum(1 for item in active_items if safe_int(item.get("buy_count_60s")) > 0),
        "recent_sell_watchlist_rows": sum(1 for item in active_items if safe_int(item.get("sell_count_60s")) > 0),
        "migration_seen_watchlist_rows": sum(1 for item in active_items if boolish(item.get("migration_seen"))),
        "pumpswap_pool_joined_watchlist_rows": sum(1 for item in active_items if boolish(item.get("pumpswap_pool_joined"))),
        "r5_watchlist_readyish_rows": len(readyish),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "readyish_rows_path": str(args.out_dir / "readyish_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_json(args.state_json, updated_state)
    write_jsonl(args.out_dir / "rows.jsonl", active_items)
    write_jsonl(args.out_dir / "readyish_rows.jsonl", readyish)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "tracked_this_run",
        "watchlist_size",
        "progress_70_80",
        "progress_80_90",
        "progress_90_95",
        "progress_95_98",
        "progress_98_99",
        "progress_99_plus",
        "r5_watchlist_readyish_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

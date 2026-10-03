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


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            item = json.loads(raw)
        except Exception:
            continue
        if isinstance(item, dict):
            rows.append(item)
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


def completion_progress_pct(row: dict[str, Any]) -> tuple[float, str]:
    real_token = safe_int(row.get("real_token_reserves"))
    total_supply = safe_int(row.get("total_supply"))
    virtual_token = safe_int(row.get("virtual_token_reserves"))
    if total_supply > 0 and total_supply >= real_token:
        return max(0.0, min(100.0, 100.0 * (1.0 - (real_token / total_supply)))), "total_supply_real_token_reserve"
    if virtual_token > 0 and virtual_token >= real_token:
        return max(0.0, min(100.0, 100.0 * (1.0 - (real_token / virtual_token)))), "virtual_token_reserve_proxy"
    return 0.0, "progress_unavailable"


def progress_bucket(progress: float) -> str:
    if progress >= 99.0:
        return "99%+"
    if progress >= 98.0:
        return "98-99%"
    if progress >= 95.0:
        return "95-98%"
    if progress >= 90.0:
        return "90-95%"
    return "<90%"


def estimated_sol_to_completion(row: dict[str, Any]) -> tuple[int, str]:
    virtual_sol = safe_int(row.get("virtual_sol_reserves"))
    virtual_token = safe_int(row.get("virtual_token_reserves"))
    real_token = safe_int(row.get("real_token_reserves"))
    if virtual_sol <= 0 or virtual_token <= 0 or real_token <= 0:
        return 0, "estimate_unavailable"
    if real_token >= virtual_token:
        return 0, "estimate_singular"
    # Constant product inverse for buying roughly the remaining real-token reserve.
    return max(0, (real_token * virtual_sol) // max(1, virtual_token - real_token)), "virtual_cp_solve_remaining_real_tokens"


def completion_decision(row: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    if text(row.get("route_family")) != "PUMP_BONDING_CURVE":
        return "PARK_COMPLETION_UNSUPPORTED_ROUTE", "route_family"
    if boolish(row.get("complete")) or boolish(row.get("curve_complete")):
        return "PARK_COMPLETION_ALREADY_COMPLETE", "complete"
    if not boolish(row.get("curve_account_exists") or row.get("bonding_curve_account_fetch_ok")):
        return "PARK_COMPLETION_CURVE_STATE_MISSING", "curve_account_exists"
    if safe_int(row.get("real_token_reserves")) <= 0:
        return "PARK_COMPLETION_REAL_TOKEN_RESERVES_EMPTY", "real_token_reserves"
    if not boolish(row.get("entry_quote_ok")):
        return "PARK_COMPLETION_ENTRY_QUOTE_NOT_READY", "entry_quote_ok"
    if not boolish(row.get("exit_quote_ok")):
        return "PARK_COMPLETION_EXIT_QUOTE_NOT_READY", "exit_quote_ok"
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "PARK_COMPLETION_EXIT_LIQUIDITY", "exit_liquidity_guard_ready"
    progress = safe_float(row.get("curve_progress_pct"))
    if progress < args.min_progress_pct:
        return "PARK_COMPLETION_PROGRESS_BELOW_THRESHOLD", "curve_progress_pct"
    return "PRE_MIGRATION_COMPLETION_WATCH_READY", "none"


def enrich_row(row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    out = dict(row)
    progress, progress_source = completion_progress_pct(out)
    estimate_lamports, estimate_source = estimated_sol_to_completion(out)
    out.update(
        {
            "record_type": "launch60_curve_completion_watch",
            "curve_progress_pct": round(progress, 6),
            "curve_progress_bucket": progress_bucket(progress),
            "curve_progress_source": progress_source,
            "estimated_sol_to_completion_lamports": estimate_lamports,
            "estimated_sol_to_completion_sol": round(estimate_lamports / 1_000_000_000, 9),
            "estimated_sol_to_completion_source": estimate_source,
            "buy_pressure_recent": safe_int(out.get("net_buy_pressure_5s") or out.get("net_buy_pressure_10s")),
            "buy_count_5s": safe_int(out.get("buy_count_5s")),
            "sell_count_5s": safe_int(out.get("sell_count_5s")),
            "complete": boolish(out.get("complete") or out.get("curve_complete")),
            "wire_send": 0,
            "live_landed": 0,
        }
    )
    decision, blocker = completion_decision(out, args)
    out["completion_watch_decision"] = decision
    out["completion_watch_first_blocker"] = blocker
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch60 no-wire Pump curve completion watcher.")
    parser.add_argument("--input-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    args = parser.parse_args()

    source_rows = load_jsonl(args.input_rows)
    rows = [enrich_row(row, args) for row in source_rows]
    ready_rows = [row for row in rows if text(row.get("completion_watch_decision")) == "PRE_MIGRATION_COMPLETION_WATCH_READY"]
    bucket_counts = Counter(text(row.get("curve_progress_bucket")) for row in rows)
    ready_bucket_counts = Counter(text(row.get("curve_progress_bucket")) for row in ready_rows)
    decision_counts = Counter(text(row.get("completion_watch_decision")) for row in rows)
    estimates = [safe_int(row.get("estimated_sol_to_completion_lamports")) for row in ready_rows if safe_int(row.get("estimated_sol_to_completion_lamports")) > 0]
    status = "LAUNCH60_CURVE_COMPLETION_WATCH_PASS" if ready_rows else "PARK_LAUNCH60_NO_PRE_MIGRATION_READY_ROWS"
    summary = {
        "status": status,
        "input_rows": str(args.input_rows),
        "rows_seen": len(rows),
        "pre_migration_ready_rows": len(ready_rows),
        "min_progress_pct": args.min_progress_pct,
        "bucket_counts": dict(bucket_counts),
        "ready_bucket_counts": dict(ready_bucket_counts),
        "decision_counts": dict(decision_counts),
        "median_estimated_sol_to_completion_lamports": int(median(estimates)) if estimates else 0,
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "ready_rows_path": str(args.out_dir / "ready_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(ready_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "ready_rows.jsonl", ready_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "rows_seen", "pre_migration_ready_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"ready_bucket_counts={json.dumps(summary['ready_bucket_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

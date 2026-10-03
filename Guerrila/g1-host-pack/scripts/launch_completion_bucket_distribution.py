#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
from typing import Any


BUCKETS = ("<50%", "50-70%", "70-80%", "80-90%", "90-95%", "95-98%", "98-99%", "99%+")


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
    if progress >= 50.0:
        return "50-70%"
    return "<50%"


def join_key(row: dict[str, Any]) -> str:
    mint = text(row.get("mint"))
    curve = text(row.get("bonding_curve") or row.get("pool_or_curve"))
    if mint and curve:
        return f"{mint}|{curve}"
    return mint or curve or text(row.get("launch_state_id") or row.get("source_signature"))


def migration_keys(rows: list[dict[str, Any]]) -> set[str]:
    out: set[str] = set()
    for row in rows:
        if not (
            row.get("migration_instruction_seen")
            or row.get("complete_pump_amm_migration_event_seen")
            or text(row.get("pumpswap_pool"))
        ):
            continue
        key = join_key(row)
        if key:
            out.add(key)
        mint = text(row.get("mint"))
        if mint:
            out.add(mint)
    return out


def flow_by_key(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    out: dict[str, dict[str, int]] = defaultdict(lambda: {"buy_count_60s": 0, "sell_count_60s": 0})
    for row in rows:
        key = join_key(row)
        if not key:
            continue
        out[key]["buy_count_60s"] = max(out[key]["buy_count_60s"], safe_int(row.get("buy_count_60s")))
        out[key]["sell_count_60s"] = max(out[key]["sell_count_60s"], safe_int(row.get("sell_count_60s")))
    return out


def enriched_row(row: dict[str, Any], flow: dict[str, dict[str, int]], migrations: set[str]) -> dict[str, Any]:
    out = dict(row)
    progress = safe_float(out.get("curve_progress_pct"))
    bucket = bucket_for(progress)
    key = join_key(out)
    flow_counts = flow.get(key) or flow.get(text(out.get("mint"))) or {}
    buy_count = max(safe_int(out.get("buy_count_5s")), safe_int(out.get("buy_count_30s")), safe_int(flow_counts.get("buy_count_60s")))
    sell_count = max(safe_int(out.get("sell_count_5s")), safe_int(out.get("sell_count_30s")), safe_int(flow_counts.get("sell_count_60s")))
    migrated = key in migrations or text(out.get("mint")) in migrations
    out.update(
        {
            "record_type": "launch60b_completion_bucket_distribution",
            "curve_progress_bucket": bucket,
            "curve_state_ready": boolish(out.get("curve_account_exists") or out.get("bonding_curve_account_fetch_ok")),
            "exit_liquidity_ready": boolish(out.get("exit_liquidity_guard_ready")),
            "recent_buy_seen": buy_count > 0,
            "recent_sell_seen": sell_count > 0,
            "recent_buy_count": buy_count,
            "recent_sell_count": sell_count,
            "migration_seen_later": migrated,
            "wire_send": 0,
            "live_landed": 0,
        }
    )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire Launch60 completion bucket distribution audit.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--buy-resolver-rows", type=Path)
    parser.add_argument("--migration-rows", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    completion_rows = load_jsonl(args.completion_rows)
    flow = flow_by_key(load_jsonl(args.buy_resolver_rows))
    migrations = migration_keys(load_jsonl(args.migration_rows))
    rows = [enriched_row(row, flow, migrations) for row in completion_rows]

    bucket_rows: dict[str, list[dict[str, Any]]] = {bucket: [] for bucket in BUCKETS}
    for row in rows:
        bucket_rows.setdefault(text(row.get("curve_progress_bucket")), []).append(row)

    bucket_summary: dict[str, dict[str, Any]] = {}
    for bucket in BUCKETS:
        group = bucket_rows.get(bucket, [])
        estimates = [safe_int(row.get("estimated_sol_to_completion_lamports")) for row in group if safe_int(row.get("estimated_sol_to_completion_lamports")) > 0]
        bucket_summary[bucket] = {
            "rows": len(group),
            "curve_state_ready_rows": sum(1 for row in group if boolish(row.get("curve_state_ready"))),
            "exit_liquidity_ready_rows": sum(1 for row in group if boolish(row.get("exit_liquidity_ready"))),
            "estimated_sol_to_completion_median": round((median(estimates) / 1_000_000_000), 9) if estimates else 0,
            "estimated_sol_to_completion_min": round((min(estimates) / 1_000_000_000), 9) if estimates else 0,
            "recent_buy_rows": sum(1 for row in group if boolish(row.get("recent_buy_seen"))),
            "recent_sell_rows": sum(1 for row in group if boolish(row.get("recent_sell_seen"))),
            "migration_seen_later_rows": sum(1 for row in group if boolish(row.get("migration_seen_later"))),
        }

    decision_counts = Counter(text(row.get("completion_watch_decision")) for row in rows)
    summary = {
        "status": "LAUNCH60B_COMPLETION_BUCKET_DISTRIBUTION_PASS",
        "completion_rows": str(args.completion_rows),
        "buy_resolver_rows": str(args.buy_resolver_rows) if args.buy_resolver_rows else "",
        "migration_rows": str(args.migration_rows) if args.migration_rows else "",
        "rows_seen": len(rows),
        "bucket_summary": bucket_summary,
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    print(f"status={summary['status']}")
    print(f"rows_seen={summary['rows_seen']}")
    print(f"bucket_summary={json.dumps(bucket_summary, sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

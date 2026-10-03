#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except Exception:
        return default


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


def mint_key(row: dict[str, Any]) -> str:
    return text(row.get("mint") or row.get("launch_mint"))


def by_mint(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        mint = mint_key(row)
        if mint:
            out[mint] = row
    return out


def watch_row(initial: dict[str, Any], current: dict[str, Any], buy_state: dict[str, Any], bridge: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    progress_start = safe_float(initial.get("curve_progress_pct"))
    progress_end = safe_float(current.get("curve_progress_pct"), progress_start)
    est_start = safe_int(initial.get("estimated_sol_to_completion_lamports"))
    est_end = safe_int(current.get("estimated_sol_to_completion_lamports"), est_start)
    migration_seen = boolish(bridge.get("migration_seen"))
    pool_joined = boolish(bridge.get("pumpswap_pool_joined")) or boolish(bridge.get("pumpswap_pool"))
    quote_ready = boolish(bridge.get("post_migration_quote_ready")) or boolish(bridge.get("pumpswap_quote_state_ready"))
    bridge_ready = text(bridge.get("followthrough_decision")) == "PRE_MIGRATION_BRIDGE_FOLLOWTHROUGH_READY"
    final_decision = "PRE_MIGRATION_CANDIDATE_WATCHING"
    if progress_start < args.min_progress_pct:
        final_decision = "PARK_PRE_MIGRATION_PROGRESS_BELOW_THRESHOLD"
    elif bridge_ready:
        final_decision = "PRE_MIGRATION_CANDIDATE_BRIDGE_READY"
    elif migration_seen and not pool_joined:
        final_decision = "PARK_PRE_MIGRATION_MIGRATED_POOL_NOT_JOINED"
    elif migration_seen and pool_joined and not quote_ready:
        final_decision = "PARK_PRE_MIGRATION_POST_MIGRATION_QUOTE_NOT_READY"
    elif progress_end < args.min_progress_pct:
        final_decision = "PARK_PRE_MIGRATION_EXITED_COMPLETION_ZONE"
    elif progress_end <= progress_start and est_end >= est_start and not boolish(buy_state.get("pre_migration_buy_resolved")):
        final_decision = "PARK_PRE_MIGRATION_SELL_DERIVED_OR_STALLED"
    return {
        "record_type": "launch62c_pre_migration_candidate_watch",
        "mint": mint_key(initial),
        "bonding_curve": text(initial.get("bonding_curve")),
        "curve_progress_start": progress_start,
        "curve_progress_end": progress_end,
        "estimated_sol_to_completion_start": est_start,
        "estimated_sol_to_completion_end": est_end,
        "buy_count": safe_int(buy_state.get("buy_count_60s")),
        "sell_count": safe_int(buy_state.get("sell_count_60s")),
        "net_buy_pressure_60s": safe_int(buy_state.get("net_buy_pressure_60s")),
        "latest_buy_signature": text(buy_state.get("latest_buy_signature")),
        "migration_seen": migration_seen,
        "pumpswap_pool_joined": pool_joined,
        "post_migration_quote_ready": quote_ready,
        "bridge_ready": bridge_ready,
        "final_decision": final_decision,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire watcher summary for current pre-migration candidates.")
    parser.add_argument("--initial-completion-rows", type=Path, required=True)
    parser.add_argument("--current-completion-rows", type=Path, default=None)
    parser.add_argument("--buy-resolver-rows", type=Path, default=None)
    parser.add_argument("--bridge-followthrough-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    args = parser.parse_args()

    initial_rows = [row for row in load_jsonl(args.initial_completion_rows) if safe_float(row.get("curve_progress_pct")) >= args.min_progress_pct]
    current_rows = load_jsonl(args.current_completion_rows) if args.current_completion_rows else initial_rows
    buy_rows = by_mint(load_jsonl(args.buy_resolver_rows))
    bridge_rows = by_mint(load_jsonl(args.bridge_followthrough_rows))
    current_by_mint = by_mint(current_rows)
    rows = [
        watch_row(row, current_by_mint.get(mint_key(row), row), buy_rows.get(mint_key(row), {}), bridge_rows.get(mint_key(row), {}), args)
        for row in initial_rows
    ]
    decision_counts = Counter(text(row.get("final_decision")) for row in rows)
    ready_rows = [row for row in rows if text(row.get("final_decision")) == "PRE_MIGRATION_CANDIDATE_BRIDGE_READY"]
    status = "LAUNCH62C_PRE_MIGRATION_CANDIDATE_WATCH_PASS" if ready_rows else "PARK_LAUNCH62C_NO_BRIDGE_READY_CANDIDATES"
    summary = {
        "status": status,
        "initial_rows_seen": len(initial_rows),
        "candidate_watch_rows": len(rows),
        "bridge_ready_rows": len(ready_rows),
        "decision_counts": dict(decision_counts),
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
    for key in ("status", "candidate_watch_rows", "bridge_ready_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

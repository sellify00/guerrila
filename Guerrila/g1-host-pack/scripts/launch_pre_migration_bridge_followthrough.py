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


def first_text(row: dict[str, Any] | None, *keys: str) -> str:
    if not row:
        return ""
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def mint_key(row: dict[str, Any] | None) -> str:
    return first_text(row, "mint", "launch_mint", "base_mint", "quote_mint")


def best_by_mint(rows: list[dict[str, Any]], ready_key: str = "") -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        mint = mint_key(row)
        if not mint:
            continue
        current = out.get(mint)
        if current is None:
            out[mint] = row
            continue
        score = (1 if ready_key and boolish(row.get(ready_key)) else 0, 1 if first_text(row, "pumpswap_pool", "pool") else 0)
        current_score = (1 if ready_key and boolish(current.get(ready_key)) else 0, 1 if first_text(current, "pumpswap_pool", "pool") else 0)
        if score > current_score:
            out[mint] = row
    return out


def follow_row(
    initial: dict[str, Any],
    current: dict[str, Any] | None,
    migration: dict[str, Any] | None,
    route: dict[str, Any] | None,
    args: argparse.Namespace,
) -> dict[str, Any]:
    now = current or initial
    mint = mint_key(initial)
    initial_progress = safe_float(initial.get("curve_progress_pct"))
    current_progress = safe_float(now.get("curve_progress_pct"))
    initial_est = safe_int(initial.get("estimated_sol_to_completion_lamports"))
    current_est = safe_int(now.get("estimated_sol_to_completion_lamports"))
    migration_seen = bool(migration and (boolish(migration.get("migration_instruction_seen")) or boolish(migration.get("complete_pump_amm_migration_event_seen")) or boolish(migration.get("migration_event_seen"))))
    pool = first_text(route, "pumpswap_pool", "pool") or first_text(migration, "pumpswap_pool", "pool")
    route_ready = bool(route and text(route.get("decision")) == "MIGRATION_PUMPSWAP_ROUTE_READY")
    quote_ready = bool(route and (route_ready or boolish(route.get("quote_state_ready")) or boolish(route.get("pumpswap_quote_state_ready"))))
    exit_ready = bool(route and (route_ready or boolish(route.get("pumpswap_exit_liquidity_guard_ready")) or boolish(route.get("exit_liquidity_guard_ready"))))
    decision = "FOLLOWTHROUGH_WAITING_FOR_MIGRATION"
    blocker = "migration_event_or_instruction"
    if initial_progress < args.min_progress_pct:
        decision, blocker = "PARK_FOLLOWTHROUGH_PROGRESS_BELOW_THRESHOLD", "curve_progress_pct"
    elif boolish(now.get("complete")) and not pool:
        decision, blocker = "PARK_FOLLOWTHROUGH_COMPLETED_NO_POOL", "pumpswap_pool"
    elif not boolish(now.get("exit_liquidity_guard_ready")) and not migration_seen:
        decision, blocker = "PARK_FOLLOWTHROUGH_DIED_EXIT_LIQUIDITY", "exit_liquidity_guard_ready"
    elif migration_seen and not pool:
        decision, blocker = "PARK_FOLLOWTHROUGH_MIGRATION_POOL_NOT_JOINED", "pumpswap_pool"
    elif migration_seen and pool and not quote_ready:
        decision, blocker = "PARK_FOLLOWTHROUGH_POST_MIGRATION_QUOTE_NOT_READY", "pumpswap_quote_state_ready"
    elif migration_seen and pool and quote_ready and not exit_ready:
        decision, blocker = "PARK_FOLLOWTHROUGH_POST_MIGRATION_EXIT_LIQUIDITY", "pumpswap_exit_liquidity_guard_ready"
    elif migration_seen and pool and quote_ready and exit_ready:
        decision, blocker = "PRE_MIGRATION_BRIDGE_FOLLOWTHROUGH_READY", "none"
    elif getattr(args, "same_completion_snapshot", False):
        decision, blocker = "FOLLOWTHROUGH_WAITING_FOR_MIGRATION", "same_snapshot_no_migration"
    elif current_est >= initial_est and current_progress <= initial_progress:
        decision, blocker = "PARK_FOLLOWTHROUGH_STALLED", "progress_not_improving"
    return {
        "record_type": "launch62b_pre_migration_bridge_followthrough",
        "mint": mint,
        "bonding_curve": first_text(initial, "bonding_curve"),
        "initial_curve_progress_pct": initial_progress,
        "current_curve_progress_pct": current_progress,
        "progress_bucket_changes": f"{text(initial.get('curve_progress_bucket'))}->{text(now.get('curve_progress_bucket'))}",
        "estimated_sol_to_completion_initial_lamports": initial_est,
        "estimated_sol_to_completion_current_lamports": current_est,
        "estimated_sol_to_completion_changes": current_est - initial_est,
        "buy_pressure": safe_int(now.get("buy_pressure_recent"), safe_int(now.get("net_buy_pressure_5s"))),
        "time_in_bucket": safe_float(now.get("time_in_bucket_seconds"), 0),
        "migration_seen": migration_seen,
        "migration_signature": first_text(route, "migration_signature", "migration_proof_signature") or first_text(migration, "migration_signature", "signature"),
        "pumpswap_pool_joined": bool(pool),
        "pumpswap_pool": pool,
        "post_migration_quote_ready": quote_ready,
        "pumpswap_exit_liquidity_guard_ready": exit_ready,
        "followthrough_decision": decision,
        "followthrough_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire pre-migration bridge followthrough monitor.")
    parser.add_argument("--initial-completion-rows", type=Path, required=True)
    parser.add_argument("--current-completion-rows", type=Path, default=None)
    parser.add_argument("--migration-rows", type=Path, default=None)
    parser.add_argument("--pumpswap-route-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    args = parser.parse_args()
    args.same_completion_snapshot = (
        not args.current_completion_rows
        or str(args.current_completion_rows) == str(args.initial_completion_rows)
    )

    initial_rows = load_jsonl(args.initial_completion_rows)
    current_rows = load_jsonl(args.current_completion_rows) if args.current_completion_rows else initial_rows
    migration_rows = load_jsonl(args.migration_rows)
    route_rows = load_jsonl(args.pumpswap_route_rows)
    current_by_mint = best_by_mint(current_rows, "exit_liquidity_guard_ready")
    migrations = best_by_mint(migration_rows)
    routes = best_by_mint(route_rows, "pumpswap_exit_liquidity_guard_ready")
    rows = [follow_row(row, current_by_mint.get(mint_key(row)), migrations.get(mint_key(row)), routes.get(mint_key(row)), args) for row in initial_rows]
    decision_counts = Counter(text(row.get("followthrough_decision")) for row in rows)
    ready_rows = [row for row in rows if text(row.get("followthrough_decision")) == "PRE_MIGRATION_BRIDGE_FOLLOWTHROUGH_READY"]
    waiting_rows = [row for row in rows if text(row.get("followthrough_decision")) == "FOLLOWTHROUGH_WAITING_FOR_MIGRATION"]
    if ready_rows:
        status = "LAUNCH62B_PRE_MIGRATION_BRIDGE_FOLLOWTHROUGH_PASS"
    elif waiting_rows:
        status = "PARK_LAUNCH62B_WAITING_FOR_MIGRATION"
    else:
        status = "PARK_LAUNCH62B_STALLED_OR_NOT_READY"
    summary = {
        "status": status,
        "initial_completion_rows": str(args.initial_completion_rows),
        "current_completion_rows": str(args.current_completion_rows) if args.current_completion_rows else str(args.initial_completion_rows),
        "migration_rows": str(args.migration_rows) if args.migration_rows else "",
        "pumpswap_route_rows": str(args.pumpswap_route_rows) if args.pumpswap_route_rows else "",
        "initial_rows_seen": len(initial_rows),
        "current_rows_seen": len(current_rows),
        "migration_rows_seen": len(migration_rows),
        "pumpswap_route_rows_seen": len(route_rows),
        "bridge_followthrough_ready_rows": len(ready_rows),
        "waiting_for_migration_rows": len(waiting_rows),
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
    for key in ("status", "initial_rows_seen", "migration_rows_seen", "pumpswap_route_rows_seen", "bridge_followthrough_ready_rows", "waiting_for_migration_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

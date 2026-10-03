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
        score = (1 if ready_key and boolish(row.get(ready_key)) else 0, 1 if text(row.get("pumpswap_pool")) else 0)
        current_score = (1 if ready_key and boolish(current.get(ready_key)) else 0, 1 if text(current.get("pumpswap_pool")) else 0)
        if score > current_score:
            out[mint] = row
    return out


def bridge_row(
    completion: dict[str, Any],
    migration: dict[str, Any] | None,
    route: dict[str, Any] | None,
    args: argparse.Namespace,
) -> dict[str, Any]:
    progress = safe_float(completion.get("curve_progress_pct"))
    migration_seen = bool(
        migration
        and (
            boolish(migration.get("migration_instruction_seen"))
            or boolish(migration.get("complete_pump_amm_migration_event_seen"))
            or boolish(migration.get("migration_event_seen"))
        )
    )
    pool = text((route or {}).get("pumpswap_pool") or (migration or {}).get("pumpswap_pool"))
    route_ready = bool(route and text(route.get("decision")) == "MIGRATION_PUMPSWAP_ROUTE_READY")
    quote_ready = bool(route and (route_ready or boolish(route.get("quote_state_ready")) or boolish(route.get("pumpswap_quote_state_ready"))))
    exit_ready = bool(route and boolish(route.get("pumpswap_exit_liquidity_guard_ready") or route.get("exit_liquidity_guard_ready")))
    decision = "MIGRATION_BRIDGE_READY"
    blocker = "none"
    if progress < args.min_progress_pct:
        decision, blocker = "PARK_BRIDGE_PROGRESS_BELOW_THRESHOLD", "curve_progress_pct"
    elif boolish(completion.get("complete")):
        decision, blocker = "PARK_BRIDGE_COMPLETED_PRE_ENTRY", "complete"
    elif not migration_seen:
        decision, blocker = "WAITING_FOR_MIGRATION", "migration_event_or_instruction"
    elif not pool:
        decision, blocker = "PARK_BRIDGE_MIGRATION_POOL_NOT_JOINED", "pumpswap_pool"
    elif not route_ready:
        decision, blocker = "PARK_BRIDGE_PUMPSWAP_ROUTE_NOT_READY", "route_join_decision"
    elif not quote_ready:
        decision, blocker = "PARK_BRIDGE_PUMPSWAP_QUOTE_NOT_READY", "quote_state_ready"
    elif not exit_ready:
        decision, blocker = "PARK_BRIDGE_PUMPSWAP_EXIT_LIQUIDITY", "exit_liquidity_guard_ready"
    return {
        "record_type": "launch62_migration_bridge_monitor",
        "mint": mint_key(completion),
        "bonding_curve": text(completion.get("bonding_curve")),
        "associated_bonding_curve": text(completion.get("associated_bonding_curve")),
        "curve_progress_pct": progress,
        "curve_progress_bucket": text(completion.get("curve_progress_bucket")),
        "estimated_sol_to_completion_lamports": completion.get("estimated_sol_to_completion_lamports"),
        "migration_signature": text((route or {}).get("migration_signature") or (migration or {}).get("migration_signature") or (migration or {}).get("signature")),
        "migration_instruction_seen": bool(migration and boolish(migration.get("migration_instruction_seen"))),
        "migration_event_seen": bool(migration and (boolish(migration.get("complete_pump_amm_migration_event_seen")) or boolish(migration.get("migration_event_seen")))),
        "pumpswap_pool": pool,
        "pumpswap_pool_join_source": text((route or {}).get("pool_join_source") or (migration or {}).get("pool_join_source")),
        "pumpswap_quote_state_ready": quote_ready,
        "pumpswap_exit_liquidity_guard_ready": exit_ready,
        "pumpswap_exit_expected_out_raw": (route or {}).get("pumpswap_exit_expected_out_raw") or (route or {}).get("exit_expected_out_raw"),
        "pumpswap_exit_price_impact_bps": (route or {}).get("pumpswap_exit_price_impact_bps") or (route or {}).get("exit_price_impact_bps"),
        "bridge_decision": decision,
        "bridge_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch62 no-wire migration bridge monitor.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--migration-rows", type=Path, default=None)
    parser.add_argument("--pumpswap-route-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    args = parser.parse_args()

    completion_rows = load_jsonl(args.completion_rows)
    migration_rows = load_jsonl(args.migration_rows)
    route_rows = load_jsonl(args.pumpswap_route_rows)
    migrations = best_by_mint(migration_rows)
    routes = best_by_mint(route_rows, "pumpswap_exit_liquidity_guard_ready")
    rows = [bridge_row(row, migrations.get(mint_key(row)), routes.get(mint_key(row)), args) for row in completion_rows]
    decision_counts = Counter(text(row.get("bridge_decision")) for row in rows)
    ready_rows = [row for row in rows if text(row.get("bridge_decision")) == "MIGRATION_BRIDGE_READY"]
    waiting_rows = [row for row in rows if text(row.get("bridge_decision")) == "WAITING_FOR_MIGRATION"]
    status = "LAUNCH62_MIGRATION_BRIDGE_MONITOR_PASS" if ready_rows else "PARK_LAUNCH62_WAITING_FOR_MIGRATION_OR_ROUTE"
    summary = {
        "status": status,
        "completion_rows": str(args.completion_rows),
        "migration_rows": str(args.migration_rows) if args.migration_rows else "",
        "pumpswap_route_rows": str(args.pumpswap_route_rows) if args.pumpswap_route_rows else "",
        "completion_rows_seen": len(completion_rows),
        "migration_rows_seen": len(migration_rows),
        "pumpswap_route_rows_seen": len(route_rows),
        "bridge_ready_rows": len(ready_rows),
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
    for key in ("status", "completion_rows_seen", "migration_rows_seen", "pumpswap_route_rows_seen", "bridge_ready_rows", "waiting_for_migration_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

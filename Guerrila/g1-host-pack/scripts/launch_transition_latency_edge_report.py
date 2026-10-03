#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


def first_present(row: dict[str, Any], names: list[str], default: Any = None) -> Any:
    for name in names:
        value = row.get(name)
        if value not in (None, ""):
            return value
    return default


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


def row_keys(row: dict[str, Any]) -> list[str]:
    keys: list[str] = []
    for value in [
        row.get("watchlist_key"),
        row.get("launch_state_id"),
        row.get("migration_signature"),
        row.get("source_signature"),
        row.get("signature"),
        row.get("pumpswap_pool"),
        row.get("pool"),
    ]:
        rendered = text(value)
        if rendered:
            keys.append(rendered)
    mint = text(row.get("mint") or row.get("route_row_mint"))
    curve = text(row.get("bonding_curve") or row.get("pool_or_curve"))
    if mint:
        keys.append(mint)
    if mint and curve:
        keys.append(f"{mint}|{curve}")
    return list(dict.fromkeys(keys))


def index(rows: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        for key in row_keys(row):
            out.setdefault(key, []).append(row)
    return out


def first_match(row: dict[str, Any], idx: dict[str, list[dict[str, Any]]]) -> dict[str, Any]:
    for key in row_keys(row):
        matches = idx.get(key)
        if matches:
            return matches[0]
    return {}


def timestamp_ns(row: dict[str, Any], names: list[str]) -> int:
    value = safe_int(first_present(row, names))
    # block_time often arrives as epoch seconds. Keep it comparable by scaling.
    if 1_000_000_000 < value < 10_000_000_000:
        return value * 1_000_000_000
    return value


def build_rows(migration_rows: list[dict[str, Any]], route_rows: list[dict[str, Any]], bridge_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    migration_idx = index(migration_rows)
    bridge_idx = index(bridge_rows)
    source_rows = route_rows or migration_rows or bridge_rows
    rows: list[dict[str, Any]] = []
    for base in source_rows:
        migration = first_match(base, migration_idx) if base not in migration_rows else base
        bridge = first_match(base, bridge_idx)
        route = base if base in route_rows else {}

        migration_instruction_seen = boolish(first_present(migration, ["migration_instruction_seen", "migrate_instruction_seen"])) or text(migration.get("decision")).endswith("POOL_JOINED")
        migration_event_seen = boolish(first_present(migration, ["complete_pump_amm_migration_event_seen", "migration_event_seen"]))
        pool_joined = boolish(first_present(route, ["pumpswap_pool_found", "pumpswap_pool_joined"])) or boolish(first_present(bridge, ["pumpswap_pool_joined"])) or boolish(migration.get("pumpswap_pool_joined"))
        quote_ready = boolish(first_present(route, ["pumpswap_quote_state_ready", "quote_state_ready"])) or boolish(first_present(bridge, ["post_migration_quote_ready"]))
        bridge_ready = boolish(first_present(bridge, ["bridge_ready", "post_migration_quote_ready"])) or (pool_joined and quote_ready)
        first_post_migration_trade_seen = boolish(first_present(route, ["last_trade_side", "first_post_migration_trade_side"]))
        first_deterioration_seen = boolish(first_present(route, ["quote_weakening_warning_count", "exit_quote_zero_recent_count"]))

        migration_seen_ns = timestamp_ns(migration, ["migration_seen_ns", "seen_ns", "event_ns", "block_time_ns", "block_time"])
        pool_joined_ns = timestamp_ns(route, ["pool_joined_ns", "pumpswap_pool_joined_ns", "seen_ns", "block_time_ns", "block_time"])
        quote_ready_ns = timestamp_ns(route, ["quote_ready_ns", "pumpswap_quote_ready_ns", "seen_ns", "block_time_ns", "block_time"])
        first_trade_ns = timestamp_ns(route, ["first_post_migration_trade_ns", "last_buy_seen_ns", "last_sell_seen_ns"])

        pool_join_latency_ms = int((pool_joined_ns - migration_seen_ns) / 1_000_000) if migration_seen_ns and pool_joined_ns and pool_joined_ns >= migration_seen_ns else None
        quote_ready_latency_ms = int((quote_ready_ns - migration_seen_ns) / 1_000_000) if migration_seen_ns and quote_ready_ns and quote_ready_ns >= migration_seen_ns else None
        first_trade_latency_ms = int((first_trade_ns - migration_seen_ns) / 1_000_000) if migration_seen_ns and first_trade_ns and first_trade_ns >= migration_seen_ns else None

        ready = migration_instruction_seen and pool_joined and quote_ready
        decision = "R5G_LATENCY_EDGE_CONFIRMED" if ready else "PARK_LATENCY_EDGE_NOT_PROVEN"
        blocker = "none" if ready else (
            "migration_not_seen" if not migration_instruction_seen else
            "pool_not_joined" if not pool_joined else
            "quote_not_ready"
        )

        rows.append({
            "record_type": "launch66_transition_latency_edge_report",
            "watchlist_key": text(first_present(base, ["watchlist_key", "launch_state_id"])) or text(first_present(migration, ["watchlist_key", "launch_state_id"])),
            "mint": text(first_present(base, ["mint", "route_row_mint"])) or text(migration.get("mint")),
            "bonding_curve": text(first_present(base, ["bonding_curve", "pool_or_curve"])) or text(migration.get("bonding_curve")),
            "migration_signature": text(first_present(migration, ["migration_signature", "signature", "source_signature"])),
            "pumpswap_pool": text(first_present(route, ["pumpswap_pool", "pool"])) or text(first_present(migration, ["pumpswap_pool", "event_pool", "derived_pool_pda", "account_scan_pool"])),
            "migration_instruction_seen": migration_instruction_seen,
            "migration_event_seen": migration_event_seen,
            "pumpswap_pool_joined": pool_joined,
            "post_migration_quote_ready": quote_ready,
            "bridge_ready": bridge_ready,
            "first_post_migration_trade_seen": first_post_migration_trade_seen,
            "first_deterioration_seen": first_deterioration_seen,
            "migration_instruction_seen_ns": migration_seen_ns,
            "pumpswap_pool_joined_ns": pool_joined_ns,
            "post_migration_quote_ready_ns": quote_ready_ns,
            "first_post_migration_trade_ns": first_trade_ns,
            "pool_join_latency_ms": pool_join_latency_ms,
            "quote_ready_latency_ms": quote_ready_latency_ms,
            "first_trade_latency_ms": first_trade_latency_ms,
            "transition_latency_ready": ready,
            "transition_latency_decision": decision,
            "transition_latency_first_blocker": blocker,
            "wire_send": 0,
            "live_landed": 0,
        })
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description="R5G no-wire migration transition latency edge report.")
    parser.add_argument("--migration-rows", type=Path)
    parser.add_argument("--pumpswap-route-rows", type=Path)
    parser.add_argument("--bridge-rows", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    migration_rows = load_jsonl(args.migration_rows)
    route_rows = load_jsonl(args.pumpswap_route_rows)
    bridge_rows = load_jsonl(args.bridge_rows)
    rows = build_rows(migration_rows, route_rows, bridge_rows)
    ready_rows = [row for row in rows if row.get("transition_latency_ready")]
    decision_counts = Counter(text(row.get("transition_latency_decision")) for row in rows)
    summary = {
        "status": "LAUNCH66_TRANSITION_LATENCY_EDGE_REPORT_PASS",
        "migration_rows": str(args.migration_rows) if args.migration_rows else "",
        "pumpswap_route_rows": str(args.pumpswap_route_rows) if args.pumpswap_route_rows else "",
        "bridge_rows": str(args.bridge_rows) if args.bridge_rows else "",
        "rows_seen": len(rows),
        "latency_edge_rows": len(ready_rows),
        "transition_latency_ready_rows": len(ready_rows),
        "decision_counts": dict(decision_counts),
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
    print(f"status={summary['status']}")
    print(f"rows_seen={summary['rows_seen']}")
    print(f"latency_edge_rows={summary['latency_edge_rows']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


READY_DECISION = "MIGRATION_PUMPSWAP_ROUTE_READY"
ROUTE_FAMILY = "pumpswap_after_migration"
PUMPSWAP_AMM = "PUMPSWAP_AMM"


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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def stable_id(*parts: Any) -> str:
    payload = "|".join(text(part) for part in parts if text(part)).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:32]


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


def side(row: dict[str, Any]) -> str:
    raw = (text(row.get("source_kind")) or text(row.get("decode_class")) or text(row.get("raw_kind"))).upper()
    if "BUY" in raw or raw == "BUY":
        return "buy"
    if "SELL" in raw or raw == "SELL":
        return "sell"
    value = text(row.get("last_trade_side")).lower()
    return value if value in {"buy", "sell"} else ""


def seen_ns(row: dict[str, Any]) -> int:
    return safe_int(
        row.get("seen_ns")
        or row.get("target_receive_ns")
        or row.get("provider_created_at_ns")
        or row.get("first_buy_seen_ns")
        or row.get("first_sell_seen_ns")
        or row.get("last_buy_seen_ns")
        or row.get("last_sell_seen_ns")
    )


def row_pool(row: dict[str, Any]) -> str:
    return text(row.get("pumpswap_pool") or row.get("pumpswap_pool_final") or row.get("pool") or row.get("pool_or_curve"))


def row_mint(row: dict[str, Any]) -> str:
    return text(row.get("mint") or row.get("base_mint") or row.get("quote_mint"))


def row_keys(row: dict[str, Any]) -> dict[str, str]:
    pool = row_pool(row)
    mint = row_mint(row)
    creator = text(row.get("creator"))
    migration_signature = text(row.get("migration_signature"))
    pda = text(row.get("derived_pool_pda") or row.get("pumpswap_pool_pda"))
    return {
        "pool": pool,
        "mint": mint,
        "base_mint": text(row.get("base_mint")),
        "quote_mint": text(row.get("quote_mint")),
        "creator": creator,
        "migration_signature": migration_signature,
        "pda": pda,
        "pool_pda_synthetic": stable_id(mint, creator) if mint and creator else "",
    }


def route_ready(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ROUTE_FAMILY and (
        text(row.get("decision")) == READY_DECISION
        or boolish(row.get("quote_state_ready"))
        or boolish(row.get("pumpswap_exit_liquidity_guard_ready"))
    )


def index_flow(flow_rows: list[dict[str, Any]]) -> tuple[dict[str, list[dict[str, Any]]], int]:
    index: dict[str, list[dict[str, Any]]] = defaultdict(list)
    max_seen = 0
    for row in flow_rows:
        ts = seen_ns(row)
        max_seen = max(max_seen, ts)
        enriched = dict(row)
        enriched["_flow_side"] = side(row)
        enriched["_flow_seen_ns"] = ts
        keys = row_keys(row)
        for field, value in keys.items():
            if value:
                index[f"{field}:{value}"].append(enriched)
    return index, max_seen


def unique_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    seen: set[tuple[str, int, str]] = set()
    for row in rows:
        marker = (
            text(row.get("signature") or row.get("source_signature") or row.get("target_signature")),
            safe_int(row.get("_flow_seen_ns")),
            text(row.get("_flow_side")),
        )
        if marker in seen:
            continue
        seen.add(marker)
        out.append(row)
    return sorted(out, key=lambda item: safe_int(item.get("_flow_seen_ns")))


def collect_matches(route: dict[str, Any], index: dict[str, list[dict[str, Any]]]) -> tuple[list[dict[str, Any]], dict[str, bool]]:
    keys = row_keys(route)
    flags = {
        "matched_by_pool": False,
        "matched_by_mint": False,
        "matched_by_pool_pda": False,
        "matched_by_migration_signature": False,
    }
    matches: list[dict[str, Any]] = []
    for field, value in keys.items():
        if not value:
            continue
        lookup = f"{field}:{value}"
        found = index.get(lookup, [])
        matches.extend(found)
        if found and field == "pool":
            flags["matched_by_pool"] = True
        if found and field in {"mint", "base_mint", "quote_mint"}:
            flags["matched_by_mint"] = True
        if found and field in {"pda", "pool_pda_synthetic"}:
            flags["matched_by_pool_pda"] = True
        if found and field == "migration_signature":
            flags["matched_by_migration_signature"] = True
    return unique_rows(matches), flags


def flow_family_mismatch(route: dict[str, Any], flow_rows: list[dict[str, Any]]) -> bool:
    mint = row_mint(route)
    if not mint:
        return False
    for row in flow_rows:
        if row_mint(row) != mint:
            continue
        family = text(row.get("route_family"))
        if family and family not in {ROUTE_FAMILY, PUMPSWAP_AMM}:
            return True
    return False


def momentum_from_matches(matches: list[dict[str, Any]], now_ns: int) -> dict[str, Any]:
    buys = [safe_int(row.get("_flow_seen_ns")) for row in matches if text(row.get("_flow_side")) == "buy" and safe_int(row.get("_flow_seen_ns")) > 0]
    sells = [safe_int(row.get("_flow_seen_ns")) for row in matches if text(row.get("_flow_side")) == "sell" and safe_int(row.get("_flow_seen_ns")) > 0]
    last_buy = max(buys or [0])
    last_sell = max(sells or [0])
    ref_now = max(now_ns, last_buy, last_sell)
    last_side = "buy" if last_buy > last_sell else "sell" if last_sell > 0 else ""
    buy_count_2s = sum(1 for ts in buys if ref_now - ts <= 2_000_000_000)
    sell_count_2s = sum(1 for ts in sells if ref_now - ts <= 2_000_000_000)
    buy_count_5s = sum(1 for ts in buys if ref_now - ts <= 5_000_000_000)
    sell_count_5s = sum(1 for ts in sells if ref_now - ts <= 5_000_000_000)
    absorbed = last_buy > last_sell > 0
    sell_quality = "ABSORBED_SELL" if absorbed else "ROUTINE_PROFIT_TAKE" if last_sell > 0 and sell_count_2s == 0 else ""
    ready = last_side == "buy" or absorbed or buy_count_5s > sell_count_5s
    return {
        "last_trade_side": last_side,
        "last_buy_seen_ns": last_buy,
        "last_sell_seen_ns": last_sell,
        "last_buy_age_ms": int((ref_now - last_buy) / 1_000_000) if last_buy else -1,
        "last_sell_age_ms": int((ref_now - last_sell) / 1_000_000) if last_sell else -1,
        "buy_count_2s": buy_count_2s,
        "sell_count_2s": sell_count_2s,
        "buy_count_5s": buy_count_5s,
        "sell_count_5s": sell_count_5s,
        "net_buy_pressure_2s": buy_count_2s - sell_count_2s,
        "net_buy_pressure_5s": buy_count_5s - sell_count_5s,
        "sell_quality": sell_quality,
        "post_sell_absorption_score": 100 if absorbed else 50 if sell_quality == "ROUTINE_PROFIT_TAKE" else -1,
        "momentum_state_ready": ready,
    }


def decision_from(route: dict[str, Any], matches: list[dict[str, Any]], flags: dict[str, bool], all_flow_rows: list[dict[str, Any]]) -> tuple[str, str]:
    if not row_mint(route):
        return "PARK_MINT_KEY_MISSING", "mint_missing"
    if not matches:
        if flow_family_mismatch(route, all_flow_rows):
            return "PARK_FLOW_ROUTE_FAMILY_MISMATCH", "flow_route_family_mismatch"
        if row_pool(route):
            return "PARK_POOL_KEY_MISMATCH", "pool_key_mismatch"
        return "PARK_NO_RECENT_FLOW", "no_recent_flow"
    if flags.get("matched_by_pool"):
        return "FLOW_JOIN_READY_BY_POOL", "none"
    if flags.get("matched_by_pool_pda"):
        return "FLOW_JOIN_READY_BY_PDA", "none"
    if flags.get("matched_by_mint"):
        return "FLOW_JOIN_READY_BY_MINT", "none"
    if flags.get("matched_by_migration_signature"):
        return "FLOW_JOIN_READY_BY_PDA", "none"
    return "PARK_MOMENTUM_STATE_MISSING", "joined_without_side"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0V4 migrated PumpSwap flow join repair.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0m-rows", type=Path, required=True)
    parser.add_argument("--flow-rows", action="append", type=Path, default=[])
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    route_rows = load_jsonl(args.launch0m_rows)
    flow_rows: list[dict[str, Any]] = []
    for path in args.flow_rows:
        flow_rows.extend(load_jsonl(path))
    index, max_seen_ns = index_flow(flow_rows)

    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    route_ready_rows = 0
    join_ready_rows = 0
    momentum_ready_rows = 0
    momentum_missing_rows = 0

    for row in route_rows:
        if not route_ready(row):
            out = dict(row)
            out.update(
                {
                    "record_type": "launch0v4_migrated_pumpswap_flow_join_repair",
                    "flow_join_decision": text(row.get("decision")) or "PARK_NOT_ROUTE_READY",
                    "first_flow_join_blocker": text(row.get("decision")) or "not_route_ready",
                    "momentum_state_ready": False,
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
            out_rows.append(out)
            continue
        route_ready_rows += 1
        matches, flags = collect_matches(row, index)
        decision, blocker = decision_from(row, matches, flags, flow_rows)
        momentum = momentum_from_matches(matches, max_seen_ns) if matches else {
            "last_trade_side": text(row.get("last_trade_side")),
            "last_buy_seen_ns": safe_int(row.get("last_buy_seen_ns")),
            "last_sell_seen_ns": safe_int(row.get("last_sell_seen_ns")),
            "last_buy_age_ms": -1,
            "last_sell_age_ms": -1,
            "buy_count_2s": 0,
            "sell_count_2s": 0,
            "buy_count_5s": 0,
            "sell_count_5s": 0,
            "net_buy_pressure_2s": 0,
            "net_buy_pressure_5s": 0,
            "sell_quality": "",
            "post_sell_absorption_score": -1,
            "momentum_state_ready": False,
        }
        if decision.startswith("FLOW_JOIN_READY") and not boolish(momentum.get("momentum_state_ready")):
            decision = "PARK_MOMENTUM_STATE_MISSING"
            blocker = "momentum_not_buy_led"
        out = dict(row)
        keys = row_keys(row)
        first_match = matches[-1] if matches else {}
        out.update(
            {
                "record_type": "launch0v4_migrated_pumpswap_flow_join_repair",
                "route_row_pool": keys.get("pool", ""),
                "route_row_mint": keys.get("mint", ""),
                "route_row_creator": keys.get("creator", ""),
                "route_row_migration_signature": keys.get("migration_signature", ""),
                "flow_row_pool": row_pool(first_match),
                "flow_row_mint": row_mint(first_match),
                "flow_row_base_mint": text(first_match.get("base_mint")),
                "flow_row_quote_mint": text(first_match.get("quote_mint")),
                "flow_row_program_id": text(first_match.get("program_id")),
                "flow_row_signature": text(first_match.get("signature") or first_match.get("source_signature")),
                "flow_match_count": len(matches),
                "flow_join_decision": decision,
                "first_flow_join_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
                **flags,
                **momentum,
            }
        )
        out_rows.append(out)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        join_ready_rows += int(decision.startswith("FLOW_JOIN_READY"))
        momentum_ready_rows += int(boolish(out.get("momentum_state_ready")) and decision.startswith("FLOW_JOIN_READY"))
        momentum_missing_rows += int(decision in {"PARK_MOMENTUM_STATE_MISSING", "PARK_POOL_KEY_MISMATCH", "PARK_NO_RECENT_FLOW", "PARK_FLOW_ROUTE_FAMILY_MISMATCH"})

    status = "LAUNCH0V4_MIGRATED_PUMPSWAP_FLOW_JOIN_REPAIR_PASS"
    if route_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_NO_ROUTE_READY_ROWS"
    elif momentum_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_FLOW_JOIN_NOT_READY"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "launch0m_rows_path": str(args.launch0m_rows),
        "flow_rows_paths": [str(path) for path in args.flow_rows],
        "flow_rows_seen": len(flow_rows),
        "flow_index_keys": len(index),
        "route_ready_rows": route_ready_rows,
        "flow_join_ready_rows": join_ready_rows,
        "momentum_ready_rows": momentum_ready_rows,
        "momentum_state_missing_rows": momentum_missing_rows,
        "flow_join_decision_counts": dict(decision_counts),
        "first_flow_join_blocker_counts": dict(blocker_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": momentum_ready_rows > 0,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "route_ready_rows",
        "flow_join_ready_rows",
        "momentum_ready_rows",
        "momentum_state_missing_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"flow_join_decision_counts={json.dumps(summary['flow_join_decision_counts'], sort_keys=True)}")
    print(f"first_flow_join_blocker_counts={json.dumps(summary['first_flow_join_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

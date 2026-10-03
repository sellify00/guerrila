#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


READY_DECISION = "MIGRATION_PUMPSWAP_ROUTE_READY"


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


def trade_side(row: dict[str, Any]) -> str:
    value = (text(row.get("source_kind")) or text(row.get("decode_class"))).upper()
    if "BUY" in value:
        return "buy"
    if "SELL" in value:
        return "sell"
    return ""


def seen_ns(row: dict[str, Any]) -> int:
    return safe_int(
        row.get("seen_ns")
        or row.get("target_receive_ns")
        or row.get("provider_created_at_ns")
        or row.get("first_buy_seen_ns")
        or row.get("first_sell_seen_ns")
    )


def keys_for_row(row: dict[str, Any]) -> set[str]:
    keys: set[str] = set()
    for field in ("mint", "pumpswap_pool", "pool", "pool_or_curve"):
        value = text(row.get(field))
        if value:
            keys.add(value)
    return keys


def build_flow_index(flow_rows: list[dict[str, Any]]) -> tuple[dict[str, list[dict[str, Any]]], int]:
    index: dict[str, list[dict[str, Any]]] = defaultdict(list)
    max_seen = 0
    for row in flow_rows:
        side = trade_side(row)
        ts = seen_ns(row)
        max_seen = max(max_seen, ts)
        if not side or ts <= 0:
            continue
        enriched = dict(row)
        enriched["_flow_side"] = side
        enriched["_flow_seen_ns"] = ts
        for key in keys_for_row(row):
            index[key].append(enriched)
    return index, max_seen


def unique_flow(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    seen: set[tuple[str, int, str]] = set()
    for row in rows:
        marker = (text(row.get("signature") or row.get("source_signature")), safe_int(row.get("_flow_seen_ns")), text(row.get("_flow_side")))
        if marker in seen:
            continue
        seen.add(marker)
        out.append(row)
    return sorted(out, key=lambda item: safe_int(item.get("_flow_seen_ns")))


def hydrate_flow(row: dict[str, Any], index: dict[str, list[dict[str, Any]]], now_ns: int) -> tuple[dict[str, Any], str]:
    matches: list[dict[str, Any]] = []
    for key in keys_for_row(row):
        matches.extend(index.get(key, []))
    matches = unique_flow(matches)
    out = dict(row)
    if not matches:
        out.update(
            {
                "momentum_state_ready": False,
                "first_momentum_blocker": "MOMENTUM_FLOW_SOURCE_MISSING",
                "last_trade_side": text(row.get("last_trade_side")),
                "last_buy_seen_ns": safe_int(row.get("last_buy_seen_ns") or row.get("first_buy_seen_ns")),
                "last_sell_seen_ns": safe_int(row.get("last_sell_seen_ns") or row.get("first_sell_seen_ns")),
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
            }
        )
        return out, "MOMENTUM_FLOW_SOURCE_MISSING"

    last_buy = max([safe_int(item.get("_flow_seen_ns")) for item in matches if text(item.get("_flow_side")) == "buy"] or [0])
    last_sell = max([safe_int(item.get("_flow_seen_ns")) for item in matches if text(item.get("_flow_side")) == "sell"] or [0])
    last_trade = "buy" if last_buy > last_sell else "sell" if last_sell > 0 else ""
    ref_now = max(now_ns, last_buy, last_sell)
    buy_count_2s = sum(1 for item in matches if text(item.get("_flow_side")) == "buy" and ref_now - safe_int(item.get("_flow_seen_ns")) <= 2_000_000_000)
    sell_count_2s = sum(1 for item in matches if text(item.get("_flow_side")) == "sell" and ref_now - safe_int(item.get("_flow_seen_ns")) <= 2_000_000_000)
    buy_count_5s = sum(1 for item in matches if text(item.get("_flow_side")) == "buy" and ref_now - safe_int(item.get("_flow_seen_ns")) <= 5_000_000_000)
    sell_count_5s = sum(1 for item in matches if text(item.get("_flow_side")) == "sell" and ref_now - safe_int(item.get("_flow_seen_ns")) <= 5_000_000_000)
    buy_after_sell = last_buy > last_sell > 0
    sell_quality = "ABSORBED_SELL" if buy_after_sell else "ROUTINE_PROFIT_TAKE" if last_sell > 0 and sell_count_2s == 0 else ""
    absorption = 100 if buy_after_sell else 50 if sell_quality == "ROUTINE_PROFIT_TAKE" else -1
    ready = last_trade == "buy" or buy_after_sell or buy_count_5s > sell_count_5s
    blocker = "none" if ready else "MOMENTUM_NOT_BUY_LED"
    out.update(
        {
            "momentum_state_ready": ready,
            "first_momentum_blocker": blocker,
            "last_trade_side": last_trade,
            "last_buy_seen_ns": last_buy,
            "last_sell_seen_ns": last_sell,
            "last_buy_age_ms": int((ref_now - last_buy) / 1_000_000) if last_buy > 0 else -1,
            "last_sell_age_ms": int((ref_now - last_sell) / 1_000_000) if last_sell > 0 else -1,
            "buy_count_2s": buy_count_2s,
            "sell_count_2s": sell_count_2s,
            "buy_count_5s": buy_count_5s,
            "sell_count_5s": sell_count_5s,
            "net_buy_pressure_2s": buy_count_2s - sell_count_2s,
            "net_buy_pressure_5s": buy_count_5s - sell_count_5s,
            "sell_quality": sell_quality,
            "post_sell_absorption_score": absorption,
            "momentum_flow_match_count": len(matches),
        }
    )
    return out, blocker


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0V3 migrated PumpSwap flow state hydrator.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0m-rows", type=Path, required=True)
    parser.add_argument("--flow-rows", action="append", type=Path, default=[])
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    rows = load_jsonl(args.launch0m_rows)
    flow_rows: list[dict[str, Any]] = []
    for path in args.flow_rows:
        flow_rows.extend(load_jsonl(path))
    index, max_seen_ns = build_flow_index(flow_rows)

    out_rows: list[dict[str, Any]] = []
    blocker_counts: Counter[str] = Counter()
    route_ready_rows = 0
    momentum_ready_rows = 0
    momentum_state_missing_rows = 0
    flow_matched_rows = 0

    for row in rows:
        route_ready = text(row.get("decision")) == READY_DECISION or boolish(row.get("quote_state_ready"))
        if not route_ready:
            out = dict(row)
            out.update(
                {
                    "record_type": "launch0v3_migrated_pumpswap_flow_state_hydrator",
                    "momentum_state_ready": False,
                    "first_momentum_blocker": text(row.get("decision")) or "not_route_ready",
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
            out_rows.append(out)
            blocker_counts[text(out.get("first_momentum_blocker"))] += 1
            continue
        route_ready_rows += 1
        hydrated, blocker = hydrate_flow(row, index, max_seen_ns)
        hydrated.update(
            {
                "record_type": "launch0v3_migrated_pumpswap_flow_state_hydrator",
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(hydrated)
        blocker_counts[blocker] += 1
        momentum_ready_rows += int(boolish(hydrated.get("momentum_state_ready")))
        missing = text(hydrated.get("first_momentum_blocker")) == "MOMENTUM_FLOW_SOURCE_MISSING"
        momentum_state_missing_rows += int(missing)
        flow_matched_rows += int(not missing)

    if momentum_state_missing_rows == route_ready_rows and route_ready_rows > 0:
        status = "PARK_MIGRATED_PUMPSWAP_MOMENTUM_FLOW_SOURCE_MISSING"
    elif momentum_ready_rows > 0:
        status = "LAUNCH0V3_MIGRATED_PUMPSWAP_FLOW_STATE_HYDRATION_PASS"
    else:
        status = "PARK_MIGRATED_PUMPSWAP_MOMENTUM_NOT_READY"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "launch0m_rows_path": str(args.launch0m_rows),
        "flow_rows_paths": [str(path) for path in args.flow_rows],
        "flow_rows_seen": len(flow_rows),
        "flow_index_keys": len(index),
        "route_ready_rows": route_ready_rows,
        "flow_matched_route_ready_rows": flow_matched_rows,
        "momentum_state_ready_rows": momentum_ready_rows,
        "momentum_state_missing_rows": momentum_state_missing_rows,
        "first_momentum_blocker_counts": dict(blocker_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": momentum_state_missing_rows == 0 and route_ready_rows > 0,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "flow_rows_seen",
        "route_ready_rows",
        "flow_matched_route_ready_rows",
        "momentum_state_ready_rows",
        "momentum_state_missing_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"first_momentum_blocker_counts={json.dumps(summary['first_momentum_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

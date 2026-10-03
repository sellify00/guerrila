#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
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


def key_for(row: dict[str, Any]) -> str:
    key = text(row.get("watchlist_key") or row.get("launch_state_id"))
    if key:
        return key
    mint = text(first_present(row, ["mint", "launch_mint", "route_row_mint"]))
    curve = text(first_present(row, ["bonding_curve", "pool_or_curve", "pumpswap_pool"]))
    if mint and curve:
        return f"{mint}|{curve}"
    return mint or curve


def creator_for(row: dict[str, Any]) -> str:
    return text(first_present(row, ["creator", "creator_candidate", "creator_wallet", "deployer", "user"]))


def build_creator_stats(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    stats: dict[str, dict[str, Any]] = defaultdict(lambda: {
        "rows": 0,
        "mints": set(),
        "creator_sell_rows": 0,
        "creator_buy_rows": 0,
        "top_holder_sell_rows": 0,
    })
    for row in rows:
        creator = creator_for(row)
        if not creator:
            continue
        stat = stats[creator]
        stat["rows"] += 1
        mint = text(first_present(row, ["mint", "launch_mint", "route_row_mint"]))
        if mint:
            stat["mints"].add(mint)
        if boolish(first_present(row, ["creator_sell_seen", "creator_sell_detected"])):
            stat["creator_sell_rows"] += 1
        if boolish(first_present(row, ["creator_buy_seen", "creator_buy_detected"])):
            stat["creator_buy_rows"] += 1
        if boolish(first_present(row, ["top_holder_sell_seen", "top_holder_dump_seen"])):
            stat["top_holder_sell_rows"] += 1
    out: dict[str, dict[str, Any]] = {}
    for creator, stat in stats.items():
        out[creator] = {
            "rows": int(stat["rows"]),
            "mint_count": len(stat["mints"]),
            "creator_sell_rows": int(stat["creator_sell_rows"]),
            "creator_buy_rows": int(stat["creator_buy_rows"]),
            "top_holder_sell_rows": int(stat["top_holder_sell_rows"]),
        }
    return out


def classify_row(row: dict[str, Any], source_path: str, stats: dict[str, dict[str, Any]]) -> dict[str, Any]:
    creator = creator_for(row)
    stat = stats.get(creator, {})
    creator_sell_seen = boolish(first_present(row, ["creator_sell_seen", "creator_sell_detected"]))
    creator_buy_seen = boolish(first_present(row, ["creator_buy_seen", "creator_buy_detected"]))
    top_holder_sell_seen = boolish(first_present(row, ["top_holder_sell_seen", "top_holder_dump_seen"]))
    repeat_creator_seen = safe_int(stat.get("mint_count")) > 1
    sell_quality = text(row.get("sell_quality"))
    toxic_sell = sell_quality in {"TOXIC_SELL", "CREATOR_SELL", "TOP_HOLDER_DUMP", "CASCADE_SELL_PRESSURE", "LIQUIDITY_EXIT_RISK"}
    dump_risk = creator_sell_seen or top_holder_sell_seen or toxic_sell

    score = 50
    if creator:
        score += 10
    if creator_buy_seen:
        score += 10
    if repeat_creator_seen:
        score += 5
    if dump_risk:
        score -= 60
    score = max(0, min(100, score))

    if dump_risk:
        decision = "PARK_CREATOR_DUMP_RISK"
        blocker = "creator_or_top_holder_dump_risk"
    elif not creator:
        decision = "PARK_ACTOR_UNKNOWN"
        blocker = "actor_unknown"
    else:
        decision = "R5H_ACTOR_QUALITY_POSITIVE"
        blocker = "none"

    return {
        "record_type": "launch67_creator_incentive_actor_map",
        "source_path": source_path,
        "watchlist_key": key_for(row),
        "mint": text(first_present(row, ["mint", "launch_mint", "route_row_mint"])),
        "bonding_curve": text(first_present(row, ["bonding_curve", "pool_or_curve"])),
        "pumpswap_pool": text(row.get("pumpswap_pool")),
        "creator": creator,
        "creator_buy_seen": creator_buy_seen,
        "creator_sell_seen": creator_sell_seen,
        "top_holder_sell_seen": top_holder_sell_seen,
        "repeat_creator_seen": repeat_creator_seen,
        "creator_historical_mint_count": safe_int(stat.get("mint_count")),
        "creator_historical_sell_rows": safe_int(stat.get("creator_sell_rows")),
        "sell_quality": sell_quality,
        "actor_quality_score": score,
        "actor_quality_not_negative": decision != "PARK_CREATOR_DUMP_RISK",
        "actor_quality_decision": decision,
        "actor_quality_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="R5H no-wire creator/actor quality map.")
    parser.add_argument("--rows", type=Path, action="append", default=[])
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    raw_rows: list[tuple[str, dict[str, Any]]] = []
    all_rows: list[dict[str, Any]] = []
    for path in args.rows:
        loaded = load_jsonl(path)
        all_rows.extend(loaded)
        raw_rows.extend((str(path), row) for row in loaded)

    stats = build_creator_stats(all_rows)
    rows = [classify_row(row, source_path, stats) for source_path, row in raw_rows]
    ready_rows = [row for row in rows if row.get("actor_quality_decision") == "R5H_ACTOR_QUALITY_POSITIVE"]
    decision_counts = Counter(text(row.get("actor_quality_decision")) for row in rows)
    summary = {
        "status": "LAUNCH67_CREATOR_INCENTIVE_ACTOR_MAP_PASS",
        "input_paths": [str(path) for path in args.rows],
        "rows_seen": len(rows),
        "actor_quality_positive_rows": len(ready_rows),
        "actor_quality_not_negative_rows": sum(1 for row in rows if boolish(row.get("actor_quality_not_negative"))),
        "creator_count": len(stats),
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
    print(f"actor_quality_positive_rows={summary['actor_quality_positive_rows']}")
    print(f"actor_quality_not_negative_rows={summary['actor_quality_not_negative_rows']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

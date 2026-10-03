#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"
READY_BLOCKER = "QUOTE_STATE_READY"
KNOWN_BLOCKERS = {
    "BONDING_CURVE_STATE_MISSING",
    "BONDING_CURVE_RESERVES_MISSING",
    "BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP",
    "BONDING_CURVE_COMPLETE_OR_MIGRATED",
    "BONDING_CURVE_COMPLETE",
    "BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR",
    "BONDING_CURVE_PRICE_IMPACT_TOO_HIGH",
    "QUOTE_ROUTE_UNSUPPORTED",
    "MINT_OR_CURVE_MISSING",
}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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


def active_curve(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ACTIVE_ROUTE and not boolish(row.get("curve_complete"))


def normalized_blocker(row: dict[str, Any]) -> str:
    blocker = text(row.get("first_quote_state_blocker")) or text(row.get("active_curve_r3_first_blocker"))
    if not text(row.get("mint")) or not (text(row.get("bonding_curve")) or text(row.get("pool"))):
        return "MINT_OR_CURVE_MISSING"
    if blocker == "BONDING_CURVE_COMPLETE_OR_MIGRATED":
        return "BONDING_CURVE_COMPLETE"
    if blocker in KNOWN_BLOCKERS:
        return blocker
    if not blocker:
        return "UNKNOWN_QUOTE_BLOCKER"
    return blocker


def quote_not_ready(row: dict[str, Any]) -> bool:
    return active_curve(row) and text(row.get("first_quote_state_blocker")) != READY_BLOCKER


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U3 active bonding-curve quote blocker audit.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    rows = load_jsonl(args.launch0u_rows)
    out_rows: list[dict[str, Any]] = []
    quote_blocker_counts: Counter[str] = Counter()
    quote_ready_rows = 0
    active_rows = 0
    mint_or_curve_missing_rows = 0

    for row in rows:
        is_active = active_curve(row)
        is_quote_not_ready = quote_not_ready(row)
        blocker = normalized_blocker(row) if is_quote_not_ready else "none"
        decision = "PARK_NOT_ACTIVE_BONDING_CURVE"
        if is_active:
            active_rows += 1
            decision = "ACTIVE_CURVE_QUOTE_READY" if not is_quote_not_ready else f"ACTIVE_CURVE_QUOTE_BLOCK_{blocker}"
        if is_active and not is_quote_not_ready:
            quote_ready_rows += 1
        if is_quote_not_ready:
            quote_blocker_counts[blocker] += 1
            mint_or_curve_missing_rows += int(blocker == "MINT_OR_CURVE_MISSING")
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch0u3_active_curve_quote_blocker_audit",
                "active_curve_quote_audit_decision": decision,
                "active_curve_quote_blocker": blocker,
                "active_curve_quote_not_ready": is_quote_not_ready,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)

    top_blocker = max(quote_blocker_counts.items(), key=lambda item: item[1])[0] if quote_blocker_counts else "none"
    summary = {
        "status": "LAUNCH0U3_ACTIVE_CURVE_QUOTE_BLOCKER_AUDIT_COMPLETE",
        "run_root": str(args.run_root),
        "launch0u_rows_path": str(args.launch0u_rows),
        "active_bonding_curve_rows": active_rows,
        "quote_ready_rows": quote_ready_rows,
        "quote_not_ready_rows": sum(quote_blocker_counts.values()),
        "top_quote_blocker": top_blocker,
        "bonding_curve_state_missing_rows": quote_blocker_counts.get("BONDING_CURVE_STATE_MISSING", 0),
        "bonding_curve_reserves_missing_rows": quote_blocker_counts.get("BONDING_CURVE_RESERVES_MISSING", 0),
        "bonding_curve_real_sol_zero_birth_warmup_rows": quote_blocker_counts.get("BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP", 0),
        "bonding_curve_complete_rows": quote_blocker_counts.get("BONDING_CURVE_COMPLETE", 0),
        "bonding_curve_base_reserve_below_floor_rows": quote_blocker_counts.get("BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR", 0),
        "bonding_curve_price_impact_too_high_rows": quote_blocker_counts.get("BONDING_CURVE_PRICE_IMPACT_TOO_HIGH", 0),
        "quote_route_unsupported_rows": quote_blocker_counts.get("QUOTE_ROUTE_UNSUPPORTED", 0),
        "mint_or_curve_missing_rows": mint_or_curve_missing_rows,
        "quote_blocker_counts": dict(quote_blocker_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "active_bonding_curve_rows",
        "quote_ready_rows",
        "quote_not_ready_rows",
        "top_quote_blocker",
        "bonding_curve_state_missing_rows",
        "bonding_curve_base_reserve_below_floor_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"quote_blocker_counts={json.dumps(summary['quote_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

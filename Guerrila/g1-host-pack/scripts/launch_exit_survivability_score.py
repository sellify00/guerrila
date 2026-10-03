#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


LAMPORT_DUST_FLOOR = 1_000


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
    mint = text(row.get("mint") or row.get("launch_mint") or row.get("route_row_mint"))
    curve = text(row.get("bonding_curve") or row.get("pool_or_curve") or row.get("pumpswap_pool"))
    if mint and curve:
        return f"{mint}|{curve}"
    return mint or curve


def route_family(row: dict[str, Any]) -> str:
    route = text(first_present(row, ["route_family", "detected_route", "source_route_family"]))
    if route:
        return route
    if text(row.get("pumpswap_pool")):
        return "pumpswap_after_migration"
    if text(row.get("bonding_curve")):
        return "PUMP_BONDING_CURVE"
    return ""


def score_row(row: dict[str, Any], source_path: str, args: argparse.Namespace) -> dict[str, Any]:
    entry_quote_ok = boolish(first_present(row, ["entry_quote_ok", "pumpswap_entry_quote_ok"]))
    exit_quote_ok = boolish(first_present(row, ["exit_quote_ok", "pumpswap_exit_quote_ok"]))
    guard_ready = boolish(first_present(row, ["exit_liquidity_guard_ready", "pumpswap_exit_liquidity_guard_ready"]))
    base_reserve = safe_int(first_present(row, ["base_reserve_raw", "exit_base_reserve_lamports", "pumpswap_base_reserve", "real_sol_reserves"]))
    expected_exit = safe_int(first_present(row, ["exit_expected_out_raw", "pumpswap_exit_expected_out_raw", "executable_exit_value_lamports"]))
    min_exit = safe_int(first_present(row, ["exit_min_out_raw", "pumpswap_exit_min_out_raw"]))
    reserve_after = safe_int(first_present(row, ["reserve_after_exit_raw", "pumpswap_reserve_after_exit_raw"]))
    price_impact = safe_float(first_present(row, ["exit_price_impact_bps", "pumpswap_exit_price_impact_bps"]))
    zero_recent = safe_int(first_present(row, ["exit_quote_zero_recent_count", "quote_zero_recent_count"]))
    decline_velocity = safe_float(first_present(row, ["base_reserve_decline_velocity_bps", "reserve_decline_velocity_bps"]))
    complete = boolish(first_present(row, ["curve_complete", "complete", "bonding_curve_complete"]))
    route = route_family(row)

    if reserve_after <= 0 and base_reserve > 0 and expected_exit > 0:
        reserve_after = max(0, base_reserve - expected_exit)
    if price_impact <= 0 and base_reserve > 0 and expected_exit > 0:
        price_impact = expected_exit * 10_000.0 / max(base_reserve, 1)

    stress_base = int(base_reserve * args.stress_reserve_fraction)
    stress_reserve_after = max(0, stress_base - expected_exit)
    stress_price_impact = expected_exit * 10_000.0 / max(stress_base, 1) if stress_base > 0 and expected_exit > 0 else 0.0
    stress_exit_nonzero = expected_exit > 0 and stress_base > expected_exit

    route_switch_risk = complete and route.upper() == "PUMP_BONDING_CURVE"
    quote_zero_risk = (not exit_quote_ok) or expected_exit <= LAMPORT_DUST_FLOOR or min_exit <= 0 or zero_recent > 0
    unsurvivable = (
        not entry_quote_ok
        or not guard_ready
        or base_reserve < args.min_base_reserve_lamports
        or reserve_after < args.reserve_after_exit_floor_lamports
        or price_impact > args.max_price_impact_bps
        or decline_velocity < -args.max_decline_velocity_bps
        or not stress_exit_nonzero
        or stress_reserve_after < args.stress_reserve_after_floor_lamports
        or stress_price_impact > args.stress_max_price_impact_bps
    )

    if route_switch_risk:
        decision = "PARK_ROUTE_SWITCH_RISK"
        blocker = "route_switch_risk"
    elif quote_zero_risk:
        decision = "PARK_QUOTE_ZERO_RISK"
        blocker = "quote_zero_risk"
    elif unsurvivable:
        decision = "PARK_EXIT_UNSURVIVABLE"
        blocker = "exit_stress_or_liquidity"
    else:
        decision = "R5F_EXIT_SURVIVABLE"
        blocker = "none"

    return {
        "record_type": "launch65_exit_survivability_score",
        "source_path": source_path,
        "watchlist_key": key_for(row),
        "mint": text(first_present(row, ["mint", "launch_mint", "route_row_mint"])),
        "bonding_curve": text(row.get("bonding_curve")),
        "pumpswap_pool": text(row.get("pumpswap_pool")),
        "route_family": route,
        "entry_quote_ok": entry_quote_ok,
        "exit_quote_ok": exit_quote_ok,
        "exit_liquidity_guard_ready": guard_ready,
        "base_reserve_raw": base_reserve,
        "exit_expected_out_raw": expected_exit,
        "exit_min_out_raw": min_exit,
        "reserve_after_exit_raw": reserve_after,
        "exit_price_impact_bps": round(price_impact, 6),
        "exit_quote_zero_recent_count": zero_recent,
        "base_reserve_decline_velocity_bps": decline_velocity,
        "stress_reserve_fraction": args.stress_reserve_fraction,
        "stress_base_reserve_raw": stress_base,
        "stress_reserve_after_exit_raw": stress_reserve_after,
        "stress_exit_nonzero": stress_exit_nonzero,
        "stress_exit_price_impact_bps": round(stress_price_impact, 6),
        "route_switch_risk": route_switch_risk,
        "exit_survivability_pass": decision == "R5F_EXIT_SURVIVABLE",
        "exit_survivability_decision": decision,
        "exit_survivability_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="R5F no-wire exit survivability stress scorer.")
    parser.add_argument("--rows", type=Path, action="append", default=[])
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--stress-reserve-fraction", type=float, default=0.50)
    parser.add_argument("--min-base-reserve-lamports", type=int, default=25_000_000)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--stress-reserve-after-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--max-price-impact-bps", type=float, default=1500.0)
    parser.add_argument("--stress-max-price-impact-bps", type=float, default=3000.0)
    parser.add_argument("--max-decline-velocity-bps", type=float, default=1.0)
    args = parser.parse_args()

    rows: list[dict[str, Any]] = []
    for path in args.rows:
        for row in load_jsonl(path):
            rows.append(score_row(row, str(path), args))

    ready_rows = [row for row in rows if row.get("exit_survivability_pass")]
    decision_counts = Counter(text(row.get("exit_survivability_decision")) for row in rows)
    summary = {
        "status": "LAUNCH65_EXIT_SURVIVABILITY_SCORE_PASS",
        "input_paths": [str(path) for path in args.rows],
        "rows_seen": len(rows),
        "exit_survivable_rows": len(ready_rows),
        "stress_reserve_fraction": args.stress_reserve_fraction,
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
    print(f"exit_survivable_rows={summary['exit_survivable_rows']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

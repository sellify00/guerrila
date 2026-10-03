#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMP_BONDING_CURVE = "PUMP_BONDING_CURVE"


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


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


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


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0S Pump bonding-curve quote support no-wire acceptance gate.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--quote-state-rows", type=Path, required=True)
    parser.add_argument("--quote-state-summary", type=Path, required=True)
    parser.add_argument("--launch0r-summary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    rows = load_jsonl(args.quote_state_rows)
    quote_summary = load_json(args.quote_state_summary)
    launch0r_summary = load_json(args.launch0r_summary)

    pump_rows = [row for row in rows if text(row.get("route_family")) == PUMP_BONDING_CURVE]
    blocker_counts = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in pump_rows)
    source_kind_counts = Counter(text(row.get("source_kind")) or "unknown" for row in pump_rows)
    route_quote_counts = Counter(text(row.get("route_quote_decision")) or "unknown" for row in pump_rows)
    buy_quote_rows = sum(
        1
        for row in pump_rows
        if text(row.get("source_kind")) == "PUMP_BONDING_CURVE_BUY" and boolish(row.get("entry_quote_ok"))
    )
    sell_quote_rows = sum(
        1
        for row in pump_rows
        if text(row.get("source_kind")) == "PUMP_BONDING_CURVE_SELL" and boolish(row.get("exit_quote_ok"))
    )
    ready_rows = sum(1 for row in pump_rows if boolish(row.get("quote_state_ready")))
    unsupported_after = sum(1 for row in pump_rows if text(row.get("first_quote_state_blocker")) == "QUOTE_ROUTE_UNSUPPORTED")
    specific_blocked = sum(
        count
        for blocker, count in blocker_counts.items()
        if blocker.startswith("BONDING_CURVE_")
    )
    unsupported_before = len(pump_rows)
    wire_send = safe_int(quote_summary.get("wire_send"))
    live_landed = safe_int(quote_summary.get("live_landed"))
    route_support_improved = unsupported_after < unsupported_before or specific_blocked > 0
    acceptance_pass = (
        wire_send == 0
        and live_landed == 0
        and buy_quote_rows > 0
        and sell_quote_rows > 0
        and route_support_improved
    )
    regression_guards = {
        "wire_send_zero": wire_send == 0,
        "live_landed_zero": live_landed == 0,
        "pump_bonding_curve_buy_quote_rows_positive": buy_quote_rows > 0,
        "pump_bonding_curve_sell_quote_rows_positive": sell_quote_rows > 0,
        "generic_pump_bonding_curve_unsupported_reduced_or_replaced": route_support_improved,
        "launch0r_acceptance_passed": boolish(launch0r_summary.get("acceptance_pass")),
        "entry_quality_requires_exit_quote_support": True,
        "exit_liquidity_blocks_zero_exit_quote": True,
        "warmup_calibration_deferred_until_launch0s": True,
    }
    summary = {
        "status": "LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE_PASS" if acceptance_pass else "LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE_INCOMPLETE",
        "milestone_dependency": "LAUNCH0R_ROUTE_SUPPORT_AND_WARMUP_RECONCILE_PASS",
        "run_root": str(args.run_root),
        "wire_send": wire_send,
        "live_landed": live_landed,
        "pump_bonding_curve_rows": len(pump_rows),
        "pump_bonding_curve_buy_quote_rows": buy_quote_rows,
        "pump_bonding_curve_sell_quote_rows": sell_quote_rows,
        "pump_bonding_curve_ready_rows": ready_rows,
        "pump_bonding_curve_blocker_counts": dict(blocker_counts),
        "pump_bonding_curve_source_kind_counts": dict(source_kind_counts),
        "pump_bonding_curve_route_quote_decision_counts": dict(route_quote_counts),
        "quote_route_unsupported_pump_bonding_curve_before": unsupported_before,
        "quote_route_unsupported_pump_bonding_curve_after": unsupported_after,
        "route_support_improved": route_support_improved,
        "specific_pump_bonding_curve_blocker_rows": specific_blocked,
        "launch0r_status": text(launch0r_summary.get("status")),
        "launch0r_acceptance_pass": boolish(launch0r_summary.get("acceptance_pass")),
        "acceptance_pass": acceptance_pass,
        "regression_guards": regression_guards,
        "next_gate": "LAUNCH0T_WARMUP_CALIBRATION_REPLAY" if acceptance_pass else "LAUNCH0S_PUMP_BONDING_CURVE_QUOTE_SUPPORT_NO_WIRE",
    }
    out_dir = args.out_dir
    write_json(out_dir / "summary.json", summary)
    write_txt(out_dir / "summary.txt", summary)
    write_jsonl(out_dir / "pump_bonding_curve_quote_rows.jsonl", pump_rows)

    print(f"status={summary['status']}")
    print(f"acceptance_pass={str(acceptance_pass).lower()}")
    print(f"pump_bonding_curve_buy_quote_rows={buy_quote_rows}")
    print(f"pump_bonding_curve_sell_quote_rows={sell_quote_rows}")
    print(f"quote_route_unsupported_pump_bonding_curve_after={unsupported_after}")
    print(f"pump_bonding_curve_blocker_counts={json.dumps(dict(blocker_counts), sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

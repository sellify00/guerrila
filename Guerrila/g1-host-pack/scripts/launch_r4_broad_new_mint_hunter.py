#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


READY_LABEL = "R4_HUNTER_READY"
ROUTE_FAMILIES = {"PUMP_BONDING_CURVE", "pumpswap_after_migration", "PUMPSWAP_AMM"}


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
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def row_route_family(row: dict[str, Any]) -> str:
    return text(row.get("route_family") or row.get("route_quote_family") or row.get("source_route_family"))


def exit_expected_out(row: dict[str, Any]) -> int:
    for key in (
        "exit_expected_out_raw",
        "exit_quote_expected_out_raw",
        "pumpswap_exit_expected_out_raw",
        "direct_exit_expected_out_raw",
    ):
        value = safe_int(row.get(key), -1)
        if value >= 0:
            return value
    return 0


def entry_quote_ok(row: dict[str, Any]) -> bool:
    if boolish(row.get("entry_quote_ok")):
        return True
    if safe_int(row.get("entry_expected_out_raw")) > 0:
        return True
    if safe_int(row.get("entry_quote_expected_out_raw")) > 0:
        return True
    return False


def exit_quote_ok(row: dict[str, Any]) -> bool:
    if boolish(row.get("exit_quote_ok")) or boolish(row.get("exit_liquidity_guard_ready")):
        return True
    return exit_expected_out(row) > 0


def launch_age_ms(row: dict[str, Any]) -> int:
    for key in ("launch_age_ms", "warmup_age_ms", "signal_age_ms", "target_age_ms"):
        value = safe_int(row.get(key), -1)
        if value >= 0:
            return value
    return -1


def creator_sell_seen(row: dict[str, Any]) -> bool:
    return boolish(row.get("creator_sell_seen")) or boolish(row.get("creator_sell_detected"))


def policy_hard_deny(row: dict[str, Any]) -> bool:
    blocker = text(row.get("policy_blocker") or row.get("first_policy_blocker"))
    if blocker and blocker not in {"none", "policy_pass", "PASS"}:
        return True
    return boolish(row.get("policy_hard_deny"))


def route_supported(row: dict[str, Any]) -> bool:
    family = row_route_family(row)
    if family in ROUTE_FAMILIES:
        return True
    decision = text(row.get("decision") or row.get("route_quote_decision"))
    return decision in {
        "MIGRATION_PUMPSWAP_ROUTE_READY",
        "BONDING_CURVE_QUOTE_STATE_READY",
        "QUOTE_STATE_READY",
    }


def classify(row: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    if not route_supported(row):
        return "PARK_R4_ROUTE_UNSUPPORTED", text(row.get("decision") or row.get("first_quote_state_blocker")) or "route_unsupported"
    age = launch_age_ms(row)
    if args.max_launch_age_ms > 0 and age >= 0 and age > args.max_launch_age_ms:
        return "PARK_R4_STALE_LAUNCH", f"age_ms={age}"
    if not boolish(row.get("quote_state_ready")) and text(row.get("first_quote_state_blocker")) not in {"QUOTE_STATE_READY", ""}:
        return "PARK_R4_NO_EXIT", text(row.get("first_quote_state_blocker")) or "quote_state_not_ready"
    if not entry_quote_ok(row):
        return "PARK_R4_NO_ENTRY", "entry_quote_missing_or_zero"
    if not exit_quote_ok(row):
        return "PARK_R4_NO_EXIT", "exit_quote_missing_or_zero"
    if exit_expected_out(row) <= 0:
        return "PARK_R4_NO_EXIT", "exit_expected_out_zero"
    if boolish(row.get("exit_quote_dust")) or boolish(row.get("exit_dust")):
        return "PARK_R4_NO_EXIT", "exit_quote_dust"
    if creator_sell_seen(row):
        return "PARK_R4_CREATOR_SELL", "creator_sell_seen"
    if policy_hard_deny(row):
        return "PARK_R4_POLICY", text(row.get("policy_blocker")) or "policy_hard_deny"
    if not boolish(args.assume_pm_ready):
        return "PARK_R4_PM", "pm_not_assumed_ready"
    if not boolish(args.monitor_ready):
        return "PARK_R4_MONITOR_NOT_READY", "monitor_not_ready"
    return READY_LABEL, "none"


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch R4 broad new-mint hunter no-wire candidate gate.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--rows", type=Path, action="append", default=[])
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-launch-age-ms", type=int, default=60_000)
    parser.add_argument("--assume-pm-ready", default="true")
    parser.add_argument("--monitor-ready", default="true")
    args = parser.parse_args()

    input_rows: list[dict[str, Any]] = []
    for path in args.rows:
        input_rows.extend(load_jsonl(path))

    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    route_counts: Counter[str] = Counter()

    for row in input_rows:
        decision, blocker = classify(row, args)
        enriched = dict(row)
        enriched.update(
            {
                "record_type": "launch_r4_broad_new_mint_hunter",
                "r4_hunter_decision": decision,
                "r4_hunter_first_blocker": blocker,
                "r4_hunter_ready": decision == READY_LABEL,
                "launch_hunter_mode": True,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        route_counts[row_route_family(row) or "unknown"] += 1

    ready_rows = [row for row in out_rows if boolish(row.get("r4_hunter_ready"))]
    summary = {
        "status": "LAUNCH_R4_HUNTER_READY" if ready_rows else "PARK_R4_NO_READY_ROWS",
        "run_root": str(args.run_root),
        "input_rows": len(input_rows),
        "r4_hunter_ready_rows": len(ready_rows),
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
        "route_family_counts": dict(route_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(ready_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "r4_hunter_ready_rows", "wire_send", "live_landed", "acceptance_pass"):
        value = summary[key]
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

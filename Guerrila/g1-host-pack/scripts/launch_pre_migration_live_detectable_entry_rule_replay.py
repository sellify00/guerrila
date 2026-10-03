#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from statistics import median
from typing import Any, Callable


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


def mint_key(row: dict[str, Any]) -> str:
    return text(row.get("mint") or row.get("launch_mint"))


def by_mint(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    return {mint_key(row): row for row in rows if mint_key(row)}


def parse_floats(raw: str) -> list[float]:
    return [safe_float(item) for item in raw.split(",") if text(item)]


def merge_rows(resolved_rows: list[dict[str, Any]], markout_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    markouts = by_mint(markout_rows)
    rows: list[dict[str, Any]] = []
    for row in resolved_rows:
        markout = markouts.get(mint_key(row), {})
        merged = dict(row)
        for key in (
            "migration_observed",
            "time_to_migration_seconds",
            "peak_executable_exit_gain_bps",
            "net_pnl_after_fees_tips_rent",
            "post_migration_exit_ready",
            "no_exit_tail",
        ):
            if key in markout:
                merged[key] = markout.get(key)
        rows.append(merged)
    return rows


def summarize_rule(name: str, predicate: Callable[[dict[str, Any]], bool], rows: list[dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    selected = [row for row in rows if predicate(row)]
    migrated = [
        row
        for row in selected
        if boolish(row.get("migration_observed"))
        and (args.migration_ttl_seconds <= 0 or safe_float(row.get("time_to_migration_seconds"), 10**18) <= args.migration_ttl_seconds)
    ]
    peaks = [safe_float(row.get("peak_executable_exit_gain_bps")) for row in selected if row.get("peak_executable_exit_gain_bps") is not None]
    net_values = [safe_int(row.get("net_pnl_after_fees_tips_rent")) for row in selected if row.get("net_pnl_after_fees_tips_rent") is not None]
    no_migration_loss_count = len(selected) - len(migrated)
    net_after_costs = sum(net_values) - (no_migration_loss_count * args.no_migration_loss_lamports) if net_values or args.no_migration_loss_lamports else None
    profitable = [row for row in selected if boolish(row.get("migration_observed")) and boolish(row.get("post_migration_exit_ready")) and safe_int(row.get("net_pnl_after_fees_tips_rent"), -1) > 0]
    decision = "PRE_MIGRATION_LIVE_RULE_READY"
    if not selected:
        decision = "PARK_LIVE_RULE_NO_CANDIDATES"
    elif not migrated:
        decision = "PARK_LIVE_RULE_NO_MIGRATION_WITHIN_TTL"
    elif not peaks:
        decision = "PARK_LIVE_RULE_MARKOUT_MISSING"
    elif not profitable:
        decision = "PARK_LIVE_RULE_NOT_PROFITABLE"
    else:
        decision = "PRE_MIGRATION_LIVE_RULE_PROFITABLE"
    return {
        "record_type": "launch61d_live_detectable_entry_rule_replay",
        "entry_rule": name,
        "candidate_count": len(selected),
        "completion_rate": (len(migrated) / len(selected)) if selected else 0,
        "median_time_to_migration": median([safe_float(row.get("time_to_migration_seconds")) for row in migrated]) if migrated else None,
        "peak_pumpswap_exit_gain_bps": median(peaks) if peaks else None,
        "median_peak_pumpswap_exit_gain_bps": median(peaks) if peaks else None,
        "net_after_costs": net_after_costs,
        "no_migration_loss_count": no_migration_loss_count,
        "profitable_after_costs_count": len(profitable),
        "post_migration_exit_ready_count": sum(1 for row in selected if boolish(row.get("post_migration_exit_ready"))),
        "live_rule_decision": decision,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay live-detectable R5 pre-migration entry rules.")
    parser.add_argument("--resolved-buy-rows", type=Path, required=True)
    parser.add_argument("--markout-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--progress-thresholds", default="90,95,98,99")
    parser.add_argument("--sol-thresholds", default="5,2,1,0.5")
    parser.add_argument("--migration-ttl-seconds", type=float, default=1800)
    parser.add_argument("--no-migration-loss-lamports", type=int, default=0)
    args = parser.parse_args()

    rows = merge_rows(load_jsonl(args.resolved_buy_rows), load_jsonl(args.markout_rows))
    rule_defs: list[tuple[str, Callable[[dict[str, Any]], bool]]] = []
    for threshold in parse_floats(args.progress_thresholds):
        rule_defs.append((f"curve_progress>={threshold:g}%", lambda row, t=threshold: safe_float(row.get("curve_progress_pct")) >= t))
        rule_defs.append((f"curve_progress>={threshold:g}%+buy60", lambda row, t=threshold: safe_float(row.get("curve_progress_pct")) >= t and safe_int(row.get("buy_count_60s")) > 0))
        rule_defs.append((f"curve_progress>={threshold:g}%+net_buy_pressure_positive", lambda row, t=threshold: safe_float(row.get("curve_progress_pct")) >= t and safe_int(row.get("net_buy_pressure_60s")) > 0))
    for threshold in parse_floats(args.sol_thresholds):
        lamports = int(threshold * 1_000_000_000)
        rule_defs.append((f"estimated_sol_to_completion<={threshold:g}SOL", lambda row, l=lamports: 0 < safe_int(row.get("estimated_sol_to_completion_lamports")) <= l))
        rule_defs.append((f"estimated_sol_to_completion<={threshold:g}SOL+buy60", lambda row, l=lamports: 0 < safe_int(row.get("estimated_sol_to_completion_lamports")) <= l and safe_int(row.get("buy_count_60s")) > 0))
    rule_defs.append(("recent_buy_within_30s", lambda row: safe_int(row.get("buy_count_30s")) > 0))
    rule_defs.append(("recent_buy_within_60s", lambda row: safe_int(row.get("buy_count_60s")) > 0))
    rule_defs.append(("net_buy_pressure_60s_positive", lambda row: safe_int(row.get("net_buy_pressure_60s")) > 0))
    out_rows = [summarize_rule(name, predicate, rows, args) for name, predicate in rule_defs]
    decision_counts = Counter(text(row.get("live_rule_decision")) for row in out_rows)
    profitable_rules = [row for row in out_rows if text(row.get("live_rule_decision")) == "PRE_MIGRATION_LIVE_RULE_PROFITABLE"]
    ready_rules = [row for row in out_rows if text(row.get("live_rule_decision")) in {"PRE_MIGRATION_LIVE_RULE_READY", "PRE_MIGRATION_LIVE_RULE_PROFITABLE"}]
    status = "LAUNCH61D_LIVE_DETECTABLE_ENTRY_RULE_REPLAY_PASS" if profitable_rules else "PARK_LAUNCH61D_NO_PROFITABLE_LIVE_RULE"
    summary = {
        "status": status,
        "resolved_buy_rows": str(args.resolved_buy_rows),
        "markout_rows": str(args.markout_rows) if args.markout_rows else "",
        "resolved_rows_seen": len(rows),
        "entry_rules_tested": len(out_rows),
        "ready_entry_rules": len(ready_rules),
        "profitable_entry_rules": len(profitable_rules),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(profitable_rules),
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "resolved_rows_seen", "entry_rules_tested", "ready_entry_rules", "profitable_entry_rules", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

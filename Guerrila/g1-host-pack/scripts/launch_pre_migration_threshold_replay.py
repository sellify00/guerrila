#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from statistics import median
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


def parse_floats(raw: str) -> list[float]:
    values: list[float] = []
    for item in raw.split(","):
        item = item.strip()
        if not item:
            continue
        values.append(safe_float(item))
    return values


def rule_rows(rows: list[dict[str, Any]], rule_kind: str, threshold: float) -> list[dict[str, Any]]:
    if rule_kind == "progress":
        return [row for row in rows if safe_float(row.get("curve_progress_pct")) >= threshold]
    if rule_kind == "sol_to_completion":
        lamports = int(threshold * 1_000_000_000)
        return [row for row in rows if 0 < safe_int(row.get("estimated_sol_to_completion_lamports")) <= lamports]
    return []


def rule_label(rule_kind: str, threshold: float) -> str:
    if rule_kind == "progress":
        return f"curve_progress>={threshold:g}%"
    return f"estimated_sol_to_completion<={threshold:g}SOL"


def summarize_rule(rows: list[dict[str, Any]], rule_kind: str, threshold: float, args: argparse.Namespace) -> dict[str, Any]:
    selected = rule_rows(rows, rule_kind, threshold)
    migrated = [row for row in selected if boolish(row.get("migration_observed"))]
    profitable = [row for row in selected if boolish(row.get("profitable_after_costs"))]
    exit_ready = [row for row in selected if boolish(row.get("post_migration_exit_ready"))]
    no_migration = [row for row in selected if not boolish(row.get("migration_observed"))]
    peaks = [safe_float(row.get("peak_executable_exit_gain_bps")) for row in selected if row.get("peak_executable_exit_gain_bps") is not None]
    times = [safe_float(row.get("time_to_migration_seconds")) for row in migrated if row.get("time_to_migration_seconds") is not None]
    net_values = [safe_int(row.get("net_pnl_after_fees_tips_rent")) for row in selected if row.get("net_pnl_after_fees_tips_rent") is not None]
    net_after_costs = sum(net_values) - (len(no_migration) * args.no_migration_loss_lamports)
    decision = "PRE_MIGRATION_THRESHOLD_RULE_READY"
    if not selected:
        decision = "PARK_THRESHOLD_NO_CANDIDATES"
    elif not migrated:
        decision = "PARK_THRESHOLD_NO_MIGRATION_COMPLETION"
    elif not peaks:
        decision = "PARK_THRESHOLD_MARKOUT_MISSING"
    elif not profitable:
        decision = "PARK_THRESHOLD_NO_PROFITABLE_ROWS"
    else:
        decision = "PRE_MIGRATION_THRESHOLD_RULE_PROFITABLE"
    return {
        "record_type": "launch61b_pre_migration_threshold_replay",
        "entry_rule": rule_label(rule_kind, threshold),
        "rule_kind": rule_kind,
        "threshold": threshold,
        "candidate_count": len(selected),
        "migration_observed_count": len(migrated),
        "completion_rate": (len(migrated) / len(selected)) if selected else 0,
        "median_time_to_migration": median(times) if times else None,
        "median_time_to_migration_seconds": median(times) if times else None,
        "median_peak_executable_gain_bps": median(peaks) if peaks else None,
        "max_peak_executable_gain_bps": max(peaks) if peaks else None,
        "profitable_after_costs_count": len(profitable),
        "no_migration_loss_count": len(no_migration),
        "post_migration_exit_ready_count": len(exit_ready),
        "net_after_costs": net_after_costs if net_values or args.no_migration_loss_lamports else None,
        "threshold_replay_decision": decision,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay live-detectable pre-migration entry thresholds.")
    parser.add_argument("--markout-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--progress-thresholds", default="90,95,98,99")
    parser.add_argument("--sol-thresholds", default="5,2,1,0.5")
    parser.add_argument("--no-migration-loss-lamports", type=int, default=0)
    args = parser.parse_args()

    rows = load_jsonl(args.markout_rows)
    out_rows: list[dict[str, Any]] = []
    for threshold in parse_floats(args.progress_thresholds):
        out_rows.append(summarize_rule(rows, "progress", threshold, args))
    for threshold in parse_floats(args.sol_thresholds):
        out_rows.append(summarize_rule(rows, "sol_to_completion", threshold, args))
    decision_counts = Counter(text(row.get("threshold_replay_decision")) for row in out_rows)
    profitable_rules = [row for row in out_rows if text(row.get("threshold_replay_decision")) == "PRE_MIGRATION_THRESHOLD_RULE_PROFITABLE"]
    ready_rules = [row for row in out_rows if text(row.get("threshold_replay_decision")) in {"PRE_MIGRATION_THRESHOLD_RULE_READY", "PRE_MIGRATION_THRESHOLD_RULE_PROFITABLE"}]
    status = "LAUNCH61B_PRE_MIGRATION_THRESHOLD_REPLAY_PASS" if profitable_rules else "PARK_LAUNCH61B_NO_PROFITABLE_THRESHOLD_RULE"
    if ready_rules and not profitable_rules:
        status = "PARK_LAUNCH61B_MARKOUT_OR_PROFITABILITY_INCOMPLETE"
    summary = {
        "status": status,
        "markout_rows": str(args.markout_rows),
        "markout_rows_seen": len(rows),
        "entry_rules_tested": len(out_rows),
        "profitable_entry_rules": len(profitable_rules),
        "ready_entry_rules": len(ready_rules),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(profitable_rules),
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "markout_rows_seen", "entry_rules_tested", "ready_entry_rules", "profitable_entry_rules", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

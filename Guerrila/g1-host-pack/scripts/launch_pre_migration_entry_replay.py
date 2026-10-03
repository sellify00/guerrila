#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
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


def markout_by_mint(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        mint = mint_key(row)
        if not mint:
            continue
        current = out.get(mint)
        if current is None or safe_float(row.get("peak_executable_exit_gain_bps"), -1) > safe_float(current.get("peak_executable_exit_gain_bps"), -1):
            out[mint] = row
    return out


def threshold_label(threshold: float) -> str:
    return f"curve_progress_{threshold:g}%"


def estimate_net_pnl_lamports(row: dict[str, Any], markout: dict[str, Any], args: argparse.Namespace) -> int:
    explicit = safe_int(markout.get("net_pnl_after_fees_tips_rent"), None)  # type: ignore[arg-type]
    if explicit is not None:
        return explicit
    explicit = safe_int(markout.get("pnl_after_rent_recovery"), None)  # type: ignore[arg-type]
    if explicit is not None:
        return explicit
    peak_bps = safe_float(markout.get("peak_executable_exit_gain_bps"), safe_float(markout.get("peak_return_bps")))
    gross = int(args.max_spend_lamports * peak_bps / 10_000.0)
    return gross - args.fee_tip_slippage_lamports + args.rent_recovery_lamports


def replay_row(row: dict[str, Any], markout: dict[str, Any] | None, threshold: float, args: argparse.Namespace) -> dict[str, Any]:
    progress = safe_float(row.get("curve_progress_pct"))
    out = {
        "record_type": "launch61_pre_migration_entry_replay",
        "mint": mint_key(row),
        "bonding_curve": text(row.get("bonding_curve")),
        "pumpswap_pool": text(markout.get("pumpswap_pool")) if markout else "",
        "entry_condition": threshold_label(threshold),
        "curve_progress_pct": progress,
        "curve_progress_bucket": text(row.get("curve_progress_bucket")),
        "entry_quote_ok": boolish(row.get("entry_quote_ok")),
        "exit_quote_ok": boolish(row.get("exit_quote_ok")),
        "exit_liquidity_guard_ready": boolish(row.get("exit_liquidity_guard_ready")),
        "estimated_sol_to_completion_lamports": safe_int(row.get("estimated_sol_to_completion_lamports")),
        "wire_send": 0,
        "live_landed": 0,
    }
    if not markout:
        out.update(
            {
                "replay_decision": "PARK_PRE_MIGRATION_MARKOUT_MISSING",
                "migration_observed": False,
                "peak_executable_exit_gain_bps": None,
                "net_pnl_after_fees_tips_rent": None,
                "profitable_after_costs": False,
                "no_exit_tail": False,
            }
        )
        return out
    peak_bps = safe_float(markout.get("peak_executable_exit_gain_bps"), safe_float(markout.get("peak_return_bps")))
    net_pnl = estimate_net_pnl_lamports(row, markout, args)
    no_exit_tail = boolish(markout.get("no_exit_tail")) or text(markout.get("exit_reason")) in {"EXIT_EXIT_QUOTE_ZERO", "UNWINDLESS_BURN_RECONCILED"}
    migrated = boolish(markout.get("migration_observed") or markout.get("migrated") or markout.get("pumpswap_pool"))
    decision = "PRE_MIGRATION_REPLAY_PROFITABLE" if migrated and net_pnl > 0 and not no_exit_tail else "PARK_PRE_MIGRATION_REPLAY_NOT_PROFITABLE"
    if no_exit_tail:
        decision = "PARK_PRE_MIGRATION_NO_EXIT_TAIL"
    elif not migrated:
        decision = "PARK_PRE_MIGRATION_NOT_MIGRATED"
    out.update(
        {
            "replay_decision": decision,
            "migration_observed": migrated,
            "peak_executable_exit_gain_bps": peak_bps,
            "time_to_peak_after_migration_seconds": safe_float(markout.get("time_to_peak_after_migration_seconds"), safe_float(markout.get("time_to_peak_seconds"))),
            "net_pnl_after_fees_tips_rent": net_pnl,
            "profitable_after_costs": net_pnl > 0,
            "no_exit_tail": no_exit_tail,
            "exit_reason": text(markout.get("exit_reason")),
        }
    )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch61 no-wire pre-migration entry replay.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--markout-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--thresholds", default="90,95,98,99")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--fee-tip-slippage-lamports", type=int, default=1_500_000)
    parser.add_argument("--rent-recovery-lamports", type=int, default=2_039_280)
    args = parser.parse_args()

    completion_rows = load_jsonl(args.completion_rows)
    markout_rows = load_jsonl(args.markout_rows)
    markouts = markout_by_mint(markout_rows)
    thresholds = [safe_float(item) for item in args.thresholds.split(",") if text(item)]
    rows: list[dict[str, Any]] = []
    for threshold in thresholds:
        for row in completion_rows:
            if safe_float(row.get("curve_progress_pct")) < threshold:
                continue
            if not boolish(row.get("entry_quote_ok")) or not boolish(row.get("exit_quote_ok")):
                continue
            rows.append(replay_row(row, markouts.get(mint_key(row)), threshold, args))

    decision_counts = Counter(text(row.get("replay_decision")) for row in rows)
    by_condition: dict[str, dict[str, Any]] = {}
    for condition, group in defaultdict(list, {c: [row for row in rows if text(row.get("entry_condition")) == c] for c in {text(row.get("entry_condition")) for row in rows}}).items():
        peaks = [safe_float(row.get("peak_executable_exit_gain_bps")) for row in group if row.get("peak_executable_exit_gain_bps") is not None]
        profitable = [row for row in group if boolish(row.get("profitable_after_costs"))]
        by_condition[condition] = {
            "rows": len(group),
            "markout_available_rows": sum(1 for row in group if text(row.get("replay_decision")) != "PARK_PRE_MIGRATION_MARKOUT_MISSING"),
            "profitable_rows": len(profitable),
            "no_exit_tail_rows": sum(1 for row in group if boolish(row.get("no_exit_tail"))),
            "median_peak_executable_exit_gain_bps": median(peaks) if peaks else None,
            "capture_rate": (len(profitable) / len(group)) if group else 0,
        }
    profitable_rows = [row for row in rows if boolish(row.get("profitable_after_costs"))]
    status = "LAUNCH61_PRE_MIGRATION_ENTRY_REPLAY_PASS" if profitable_rows else "PARK_LAUNCH61_NO_PROFITABLE_REPLAY_ROWS"
    if not markout_rows:
        status = "PARK_LAUNCH61_MARKOUT_INPUT_MISSING"
    summary = {
        "status": status,
        "completion_rows": str(args.completion_rows),
        "markout_rows": str(args.markout_rows) if args.markout_rows else "",
        "completion_rows_seen": len(completion_rows),
        "markout_rows_seen": len(markout_rows),
        "replay_rows": len(rows),
        "profitable_replay_rows": len(profitable_rows),
        "decision_counts": dict(decision_counts),
        "condition_summary": by_condition,
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(profitable_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "completion_rows_seen", "markout_rows_seen", "replay_rows", "profitable_replay_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

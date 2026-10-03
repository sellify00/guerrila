from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path
from statistics import mean
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


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


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    snapshot = row.get("presend_last_snapshot")
    if isinstance(snapshot, dict):
        if key in snapshot:
            return snapshot.get(key)
        for quote_key in ("entry_quote", "exit_quote"):
            quote = snapshot.get(quote_key)
            if isinstance(quote, dict) and key in quote:
                return quote.get(key)
    return None


def first_int(row: dict[str, Any], *keys: str) -> int:
    for key in keys:
        parsed = safe_int(nested_get(row, key))
        if parsed:
            return parsed
    return 0


def first_float(row: dict[str, Any], *keys: str) -> float:
    for key in keys:
        parsed = safe_float(nested_get(row, key))
        if parsed:
            return parsed
    return 0.0


def first_text(row: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = nested_get(row, key)
        if text(value):
            return text(value)
    return ""


def avg_int(values: list[int]) -> float | None:
    return mean(values) if values else None


def collect_roots(args: argparse.Namespace) -> list[Path]:
    if args.run_root:
        return [Path(item).resolve() for item in args.run_root]
    results_root = Path(args.results_root).resolve()
    roots = sorted(results_root.glob("run_launch10_hot_r2_press_window_*"), key=lambda path: path.stat().st_mtime, reverse=True)
    if args.max_roots > 0:
        roots = roots[: args.max_roots]
    return roots


def pnl_after_rent(summary: dict[str, Any], attempt_dir: Path) -> int:
    recovery = load_json(attempt_dir / "22_launch22_post_flatten_account_close_recovery" / "summary.json")
    return safe_int(summary.get("pnl_after_rent_recovery"), safe_int(recovery.get("pnl_after_rent_recovery"), safe_int(summary.get("pnl_lamports"))))


def is_no_exit_tail(summary: dict[str, Any]) -> bool:
    return (
        first_text(summary, "final_status", "status") == "UNWINDLESS_BURN_RECONCILED"
        or first_text(summary, "exit_trigger_reason", "exit_reason", "flatten_reason") == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(summary.get("burn_close_landed"))
        or boolish(summary.get("auto_quarantine_invoked"))
    )


def structural_decision(summary: dict[str, Any], args: argparse.Namespace, require_stability_checks: bool) -> tuple[str, str]:
    spend = (
        first_int(summary, "required_spend_lamports", "entry_size_lamports")
        or int(round(first_float(summary, "max_spend_sol") * LAMPORTS_PER_SOL))
        or args.max_spend_lamports
    )
    spend = max(1, spend)
    expected = first_int(summary, "exit_quote_expected_out_raw", "exit_expected_out_raw")
    min_out = first_int(summary, "exit_quote_min_out_raw", "exit_min_out_raw")
    base = first_int(summary, "base_reserve_raw_at_entry", "exit_base_reserve_lamports", "base_reserve_raw")
    reserve_after = first_int(summary, "reserve_after_hypothetical_exit", "exit_reserve_after_exit_lamports", "reserve_after_exit_raw")
    if reserve_after <= 0 and base > 0:
        reserve_after = max(0, base - expected)
    base_ratio = first_float(summary, "base_reserve_to_position_ratio") or (base / spend if base > 0 else 0.0)
    impact = first_int(summary, "exit_price_impact_bps_at_entry", "exit_guard_price_impact_bps", "exit_price_impact_bps")
    if impact <= 0 and base > 0 and expected > 0:
        impact = (expected * 10_000) // base
    zero_recent = safe_int(nested_get(summary, "exit_quote_zero_recent_count"))
    decline = first_int(summary, "base_reserve_decline_velocity_bps")
    stability_checks = first_int(summary, "exit_quote_stability_checks", "exit_quote_stability_observed_checks")
    stability_present = nested_get(summary, "exit_quote_stable_across_checks") is not None or stability_checks > 0
    stability_ok = boolish(nested_get(summary, "exit_quote_stable_across_checks")) if stability_present else False

    if expected <= 0 or min_out <= 0:
        return "PARK_STRUCTURAL_EXIT_QUOTE_ZERO", "exit_quote_zero"
    if args.min_base_reserve_position_ratio > 0 and base_ratio < args.min_base_reserve_position_ratio:
        return "PARK_STRUCTURAL_BASE_RESERVE_RATIO", "base_reserve_to_position_ratio_low"
    if args.reserve_after_exit_floor_lamports > 0 and reserve_after < args.reserve_after_exit_floor_lamports:
        return "PARK_STRUCTURAL_RESERVE_AFTER_EXIT_FLOOR", "reserve_after_hypothetical_exit_below_floor"
    if args.max_exit_price_impact_bps > 0 and impact > args.max_exit_price_impact_bps:
        return "PARK_STRUCTURAL_PRICE_IMPACT", "exit_price_impact_too_high"
    if args.max_quote_zero_recent_count >= 0 and zero_recent > args.max_quote_zero_recent_count:
        return "PARK_STRUCTURAL_QUOTE_ZERO_RECENT", "exit_quote_zero_recent"
    if args.max_base_reserve_decline_bps >= 0 and decline > args.max_base_reserve_decline_bps:
        return "PARK_STRUCTURAL_BASE_RESERVE_DECLINE", "base_reserve_decline_velocity_negative"
    if require_stability_checks and stability_checks < args.exit_quote_stability_checks:
        return "PARK_STRUCTURAL_QUOTE_STABILITY_NOT_PROVEN", "exit_quote_stability_checks_too_low"
    if stability_present and not stability_ok:
        return "PARK_STRUCTURAL_QUOTE_UNSTABLE", "exit_quote_unstable"
    return "STRUCTURAL_NO_EXIT_FILTER_PASS", "none"


def attempt_row(run_root: Path, attempt_dir: Path, args: argparse.Namespace) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    pnl = pnl_after_rent(summary, attempt_dir)
    strict_decision, strict_blocker = structural_decision(summary, args, True)
    compat_decision, compat_blocker = structural_decision(summary, args, False)
    no_exit_tail = is_no_exit_tail(summary)
    return {
        "run_root": str(run_root),
        "attempt": attempt_dir.name,
        "selected_mint": first_text(summary, "selected_mint", "target_mint", "mint"),
        "selected_pool": first_text(summary, "selected_pool", "target_pool", "pool"),
        "status": first_text(summary, "final_status", "status"),
        "acquisition_landed": boolish(summary.get("acquisition_landed")),
        "flatten_landed": boolish(summary.get("flatten_landed")),
        "burn_close_landed": boolish(summary.get("burn_close_landed")),
        "manual_intervention_required": boolish(summary.get("manual_intervention_required")),
        "final_inventory_raw": safe_int(summary.get("final_inventory_raw")),
        "pm_pending_after": safe_int(summary.get("pm_pending_after")),
        "exit_reason": first_text(summary, "exit_trigger_reason", "exit_reason", "flatten_reason"),
        "peak_executable_exit_gain_bps": first_int(summary, "peak_executable_exit_gain_bps", "peak_gain_bps"),
        "pnl_after_rent_recovery": pnl,
        "is_no_exit_tail": no_exit_tail,
        "normal_positive": (not no_exit_tail) and pnl > 0,
        "normal_negative": (not no_exit_tail) and pnl < 0,
        "strict_structural_decision": strict_decision,
        "strict_structural_blocker": strict_blocker,
        "strict_structural_filter_pass": strict_decision == "STRUCTURAL_NO_EXIT_FILTER_PASS",
        "compat_structural_decision": compat_decision,
        "compat_structural_blocker": compat_blocker,
        "compat_structural_filter_pass": compat_decision == "STRUCTURAL_NO_EXIT_FILTER_PASS",
        "strict_no_exit_tail_blocked": no_exit_tail and strict_decision != "STRUCTURAL_NO_EXIT_FILTER_PASS",
        "compat_no_exit_tail_blocked": no_exit_tail and compat_decision != "STRUCTURAL_NO_EXIT_FILTER_PASS",
    }


def collect_attempts(args: argparse.Namespace) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for run_root in collect_roots(args):
        attempts_dir = run_root / "attempts"
        if not attempts_dir.exists():
            continue
        for attempt_dir in sorted(attempts_dir.glob("attempt_*")):
            summary_path = attempt_dir / "launch5_one_mint_live_execute_summary.json"
            if summary_path.exists():
                row = attempt_row(run_root, attempt_dir, args)
                if args.only_landed and not row["acquisition_landed"]:
                    continue
                rows.append(row)
    return rows


def attempts_needed(expectancy_lamports: float | None, sol_usd: float, target_usd: float) -> int | None:
    if expectancy_lamports is None or expectancy_lamports <= 0 or sol_usd <= 0 or target_usd <= 0:
        return None
    target_lamports = (target_usd / sol_usd) * LAMPORTS_PER_SOL
    return max(1, math.ceil(target_lamports / expectancy_lamports))


def summarize(rows: list[dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    old_net = sum(safe_int(row.get("pnl_after_rent_recovery")) for row in rows)
    strict_kept = [row for row in rows if boolish(row.get("strict_structural_filter_pass"))]
    compat_kept = [row for row in rows if boolish(row.get("compat_structural_filter_pass"))]
    oracle_tail_removed = [row for row in rows if not boolish(row.get("is_no_exit_tail"))]
    normal_wins = [safe_int(row.get("pnl_after_rent_recovery")) for row in rows if boolish(row.get("normal_positive"))]
    normal_losses = [safe_int(row.get("pnl_after_rent_recovery")) for row in rows if boolish(row.get("normal_negative"))]
    tail_losses = [safe_int(row.get("pnl_after_rent_recovery")) for row in rows if boolish(row.get("is_no_exit_tail"))]
    strict_net = sum(safe_int(row.get("pnl_after_rent_recovery")) for row in strict_kept)
    compat_net = sum(safe_int(row.get("pnl_after_rent_recovery")) for row in compat_kept)
    oracle_net = sum(safe_int(row.get("pnl_after_rent_recovery")) for row in oracle_tail_removed)
    strict_expectancy = strict_net / len(strict_kept) if strict_kept else None
    compat_expectancy = compat_net / len(compat_kept) if compat_kept else None
    oracle_expectancy = oracle_net / len(oracle_tail_removed) if oracle_tail_removed else None
    decision_counts = Counter(row["strict_structural_decision"] for row in rows)
    compat_decision_counts = Counter(row["compat_structural_decision"] for row in rows)
    return {
        "status": "TAIL_REMOVED_EXPECTANCY_REPORT_READY",
        "historical_attempts": len(rows),
        "landed_attempts": sum(1 for row in rows if boolish(row.get("acquisition_landed"))),
        "normal_positive_count": len(normal_wins),
        "normal_negative_count": len(normal_losses),
        "no_exit_tail_count": len(tail_losses),
        "no_exit_tail_blocked_by_new_filter": sum(1 for row in rows if boolish(row.get("strict_no_exit_tail_blocked"))),
        "no_exit_tail_blocked_by_new_filter_compat": sum(1 for row in rows if boolish(row.get("compat_no_exit_tail_blocked"))),
        "net_pnl_old": old_net,
        "net_pnl_after_tail_filter": strict_net,
        "net_pnl_after_tail_filter_compat": compat_net,
        "net_pnl_after_no_exit_tail_removed_oracle": oracle_net,
        "avg_normal_win": avg_int(normal_wins),
        "avg_normal_loss": avg_int(normal_losses),
        "avg_tail_loss": avg_int(tail_losses),
        "tail_adjusted_expectancy_per_attempt": strict_expectancy,
        "tail_adjusted_expectancy_per_attempt_compat": compat_expectancy,
        "tail_adjusted_expectancy_per_attempt_oracle": oracle_expectancy,
        "attempts_needed_for_60_usd_day": attempts_needed(strict_expectancy, args.sol_usd, args.daily_target_usd),
        "attempts_needed_for_60_usd_day_compat": attempts_needed(compat_expectancy, args.sol_usd, args.daily_target_usd),
        "attempts_needed_for_60_usd_day_oracle": attempts_needed(oracle_expectancy, args.sol_usd, args.daily_target_usd),
        "strict_kept_attempts": len(strict_kept),
        "compat_kept_attempts": len(compat_kept),
        "oracle_tail_removed_kept_attempts": len(oracle_tail_removed),
        "strict_structural_decision_counts": dict(decision_counts),
        "compat_structural_decision_counts": dict(compat_decision_counts),
        "sol_usd": args.sol_usd,
        "daily_target_usd": args.daily_target_usd,
        "only_landed": args.only_landed,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch41 tail-removed expectancy report.")
    parser.add_argument("--run-root", action="append", default=[])
    parser.add_argument("--results-root", default="g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--max-roots", type=int, default=20)
    parser.add_argument("--only-landed", default="true")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--min-base-reserve-position-ratio", type=float, default=50.0)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=25_000_000)
    parser.add_argument("--exit-quote-stability-checks", type=int, default=5)
    parser.add_argument("--max-quote-zero-recent-count", type=int, default=0)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1000)
    parser.add_argument("--max-base-reserve-decline-bps", type=int, default=0)
    parser.add_argument("--sol-usd", type=float, default=0.0)
    parser.add_argument("--daily-target-usd", type=float, default=60.0)
    args = parser.parse_args()
    args.only_landed = boolish(args.only_landed)

    rows = collect_attempts(args)
    out_base = Path(args.out_dir).resolve() if args.out_dir else (Path(args.results_root).resolve() / "22_launch41_tail_removed_expectancy_report")
    out_base.mkdir(parents=True, exist_ok=True)
    rows_path = out_base / "rows.jsonl"
    rows_path.write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    summary = summarize(rows, args)
    summary["rows_path"] = str(rows_path)
    (out_base / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_base / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key in (
        "status",
        "historical_attempts",
        "normal_positive_count",
        "normal_negative_count",
        "no_exit_tail_count",
        "no_exit_tail_blocked_by_new_filter",
        "net_pnl_old",
        "net_pnl_after_tail_filter",
        "tail_adjusted_expectancy_per_attempt",
        "attempts_needed_for_60_usd_day",
    ):
        print(f"{key}={summary[key]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

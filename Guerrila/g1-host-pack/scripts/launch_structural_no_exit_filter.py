from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


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
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


def iter_jsonl(path: Path):
    if not path.exists():
        return
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            yield value


def nested_get(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    for nested_key in ("presend_last_snapshot", "latest_attempt_summary", "latest_attempt_report"):
        nested = row.get(nested_key)
        if isinstance(nested, dict):
            if key in nested:
                return nested.get(key)
            deeper = nested.get("presend_last_snapshot")
            if isinstance(deeper, dict) and key in deeper:
                return deeper.get(key)
    return None


def first_int(row: dict[str, Any], *keys: str) -> int:
    for key in keys:
        value = nested_get(row, key)
        parsed = safe_int(value)
        if parsed:
            return parsed
    return 0


def first_float(row: dict[str, Any], *keys: str) -> float:
    for key in keys:
        value = nested_get(row, key)
        parsed = safe_float(value)
        if parsed:
            return parsed
    return 0.0


def first_text(row: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = nested_get(row, key)
        if text(value):
            return text(value)
    return ""


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def collect_input_rows(run_root: Path) -> list[tuple[str, dict[str, Any]]]:
    rows: list[tuple[str, dict[str, Any]]] = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        summary_path = attempt_dir / "launch5_one_mint_live_execute_summary.json"
        if summary_path.exists():
            rows.append((str(summary_path), load_json(summary_path)))
    for rel in (
        "launch10_hot_stream_executor_rows.jsonl",
        "launch10_hot_gate_rows.jsonl",
        "22_launch10_hot_stream_executor/rows.jsonl",
        "launch0q_realtime_launch_quote_state_rows.jsonl",
        "22_launch0m_migration_pumpswap_route_join/rows.jsonl",
    ):
        path = run_root / rel
        for row in iter_jsonl(path) or []:
            rows.append((str(path), row))
    return [(path, row) for path, row in rows if row]


def evaluate(path: str, row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    spend_lamports = (
        first_int(row, "required_spend_lamports", "size_lamports", "entry_size_lamports")
        or int(round(first_float(row, "max_spend_sol") * 1_000_000_000))
        or args.max_spend_lamports
    )
    spend_lamports = max(1, spend_lamports)
    expected_out = first_int(row, "exit_expected_out_raw", "exit_quote_expected_out_raw")
    min_out = first_int(row, "exit_min_out_raw", "exit_quote_min_out_raw")
    base_reserve = first_int(
        row,
        "base_reserve_raw_at_entry",
        "exit_base_reserve_lamports",
        "base_reserve_raw",
        "pumpswap_base_reserve",
    )
    reserve_after = first_int(
        row,
        "reserve_after_hypothetical_exit",
        "exit_reserve_after_exit_lamports",
        "reserve_after_exit_raw",
    )
    if reserve_after <= 0 and base_reserve > 0:
        reserve_after = max(0, base_reserve - expected_out)
    base_ratio = first_float(row, "base_reserve_to_position_ratio")
    if base_ratio <= 0 and base_reserve > 0:
        base_ratio = base_reserve / spend_lamports
    reserve_after_ratio = first_float(row, "reserve_after_exit_to_position_ratio")
    if reserve_after_ratio <= 0 and reserve_after > 0:
        reserve_after_ratio = reserve_after / spend_lamports
    impact_bps = first_int(row, "exit_price_impact_bps_at_entry", "exit_guard_price_impact_bps", "exit_price_impact_bps")
    if impact_bps <= 0 and base_reserve > 0 and expected_out > 0:
        impact_bps = (expected_out * 10_000) // base_reserve
    zero_recent = safe_int(nested_get(row, "exit_quote_zero_recent_count"))
    decline_bps = first_int(row, "base_reserve_decline_velocity_bps")
    stability_checks = first_int(row, "exit_quote_stability_checks", "exit_quote_stability_observed_checks")
    stability_ok = boolish(nested_get(row, "exit_quote_stable_across_checks"))
    stability_present = nested_get(row, "exit_quote_stable_across_checks") is not None or stability_checks > 0
    if not stability_present:
        stability_ok = False
    is_no_exit_tail = (
        first_text(row, "final_status", "status") == "UNWINDLESS_BURN_RECONCILED"
        or first_text(row, "exit_trigger_reason", "exit_reason", "flatten_reason") == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(nested_get(row, "burn_close_landed"))
    )

    decision = "STRUCTURAL_NO_EXIT_FILTER_PASS"
    blocker = "none"
    if expected_out <= 0 or min_out <= 0:
        decision, blocker = "PARK_STRUCTURAL_EXIT_QUOTE_ZERO", "exit_quote_zero"
    elif args.min_base_reserve_position_ratio > 0 and base_ratio < args.min_base_reserve_position_ratio:
        decision, blocker = "PARK_STRUCTURAL_BASE_RESERVE_RATIO", "base_reserve_to_position_ratio_low"
    elif args.reserve_after_exit_floor_lamports > 0 and reserve_after < args.reserve_after_exit_floor_lamports:
        decision, blocker = "PARK_STRUCTURAL_RESERVE_AFTER_EXIT_FLOOR", "reserve_after_hypothetical_exit_below_floor"
    elif args.max_exit_price_impact_bps > 0 and impact_bps > args.max_exit_price_impact_bps:
        decision, blocker = "PARK_STRUCTURAL_PRICE_IMPACT", "exit_price_impact_too_high"
    elif args.max_quote_zero_recent_count >= 0 and zero_recent > args.max_quote_zero_recent_count:
        decision, blocker = "PARK_STRUCTURAL_QUOTE_ZERO_RECENT", "exit_quote_zero_recent"
    elif args.max_base_reserve_decline_bps >= 0 and decline_bps > args.max_base_reserve_decline_bps:
        decision, blocker = "PARK_STRUCTURAL_BASE_RESERVE_DECLINE", "base_reserve_decline_velocity_negative"
    elif args.require_stability_checks and stability_checks < args.exit_quote_stability_checks:
        decision, blocker = "PARK_STRUCTURAL_QUOTE_STABILITY_NOT_PROVEN", "exit_quote_stability_checks_too_low"
    elif stability_present and not stability_ok:
        decision, blocker = "PARK_STRUCTURAL_QUOTE_UNSTABLE", "exit_quote_unstable"

    return {
        "source_path": path,
        "selected_mint": first_text(row, "selected_mint", "target_mint", "mint", "launch_mint"),
        "selected_pool": first_text(row, "selected_pool", "target_pool", "pool"),
        "entry_size_lamports": spend_lamports,
        "exit_expected_out_raw": expected_out,
        "exit_min_out_raw": min_out,
        "base_reserve_raw_at_entry": base_reserve,
        "reserve_after_hypothetical_exit": reserve_after,
        "base_reserve_to_position_ratio": base_ratio,
        "reserve_after_exit_to_position_ratio": reserve_after_ratio,
        "exit_price_impact_bps_at_entry": impact_bps,
        "exit_quote_zero_recent_count": zero_recent,
        "base_reserve_decline_velocity_bps": decline_bps,
        "exit_quote_stability_checks": stability_checks,
        "exit_quote_stable_across_checks": stability_ok,
        "structural_no_exit_filter_pass": decision == "STRUCTURAL_NO_EXIT_FILTER_PASS",
        "structural_no_exit_decision": decision,
        "structural_no_exit_first_blocker": blocker,
        "would_block_no_exit_tail": is_no_exit_tail and decision != "STRUCTURAL_NO_EXIT_FILTER_PASS",
        "is_no_exit_tail": is_no_exit_tail,
        "final_status": first_text(row, "final_status", "status"),
        "exit_reason": first_text(row, "exit_trigger_reason", "exit_reason", "flatten_reason"),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch39 structural no-exit filter.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--min-base-reserve-position-ratio", type=float, default=50.0)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=25_000_000)
    parser.add_argument("--exit-quote-stability-checks", type=int, default=5)
    parser.add_argument("--max-quote-zero-recent-count", type=int, default=0)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1000)
    parser.add_argument("--max-base-reserve-decline-bps", type=int, default=0)
    parser.add_argument("--require-stability-checks", default="true")
    args = parser.parse_args()
    args.require_stability_checks = boolish(args.require_stability_checks)

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch39_structural_no_exit_filter"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = [evaluate(path, row, args) for path, row in collect_input_rows(run_root)]
    decision_counts = Counter(row["structural_no_exit_decision"] for row in rows)
    no_exit_tails = sum(1 for row in rows if row.get("is_no_exit_tail"))
    summary = {
        "status": "STRUCTURAL_NO_EXIT_FILTER_AUDITED",
        "run_root": str(run_root),
        "rows_seen": len(rows),
        "structural_no_exit_filter_pass_rows": decision_counts.get("STRUCTURAL_NO_EXIT_FILTER_PASS", 0),
        "blocked_rows": len(rows) - decision_counts.get("STRUCTURAL_NO_EXIT_FILTER_PASS", 0),
        "no_exit_tail_count": no_exit_tails,
        "would_block_no_exit_tail_count": sum(1 for row in rows if row.get("would_block_no_exit_tail")),
        "decision_counts": dict(decision_counts),
        "min_base_reserve_position_ratio": args.min_base_reserve_position_ratio,
        "reserve_after_exit_floor_lamports": args.reserve_after_exit_floor_lamports,
        "exit_quote_stability_checks": args.exit_quote_stability_checks,
        "max_quote_zero_recent_count": args.max_quote_zero_recent_count,
        "max_exit_price_impact_bps": args.max_exit_price_impact_bps,
        "max_base_reserve_decline_bps": args.max_base_reserve_decline_bps,
        "require_stability_checks": args.require_stability_checks,
    }
    (out_dir / "rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key in ("status", "rows_seen", "structural_no_exit_filter_pass_rows", "no_exit_tail_count", "would_block_no_exit_tail_count"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

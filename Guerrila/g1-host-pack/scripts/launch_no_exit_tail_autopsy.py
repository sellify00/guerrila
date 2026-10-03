from __future__ import annotations

import argparse
import json
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


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def monitor_samples(summary: dict[str, Any]) -> list[dict[str, Any]]:
    samples = summary.get("monitor_samples")
    if isinstance(samples, list):
        return [item for item in samples if isinstance(item, dict)]
    samples_path = text(summary.get("monitor_samples_path"))
    if samples_path:
        path = Path(samples_path)
        if path.exists():
            rows = []
            for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
                try:
                    loaded = json.loads(raw)
                except Exception:
                    continue
                if isinstance(loaded, dict):
                    rows.append(loaded)
            return rows
    return []


def relative_ms(base_ns: int, event_ns: int) -> int | None:
    if base_ns <= 0 or event_ns <= 0:
        return None
    return max(0, (event_ns - base_ns) // 1_000_000)


def first_sample_event_ms(samples: list[dict[str, Any]], base_ns: int, predicate) -> int | None:
    for sample in samples:
        if predicate(sample):
            ns = safe_int(sample.get("sample_ns") or sample.get("snapshot_ns") or sample.get("ns"))
            return relative_ms(base_ns, ns)
    return None


def structural_decision(summary: dict[str, Any], args: argparse.Namespace) -> tuple[str, str]:
    spend = first_int(summary, "required_spend_lamports", "entry_size_lamports") or int(round(first_float(summary, "max_spend_sol") * 1_000_000_000)) or args.max_spend_lamports
    spend = max(1, spend)
    expected = first_int(summary, "exit_quote_expected_out_raw", "exit_expected_out_raw")
    min_out = first_int(summary, "exit_quote_min_out_raw", "exit_min_out_raw")
    base = first_int(summary, "base_reserve_raw_at_entry", "exit_base_reserve_lamports", "base_reserve_raw")
    reserve_after = first_int(summary, "reserve_after_hypothetical_exit", "exit_reserve_after_exit_lamports", "reserve_after_exit_raw")
    if reserve_after <= 0 and base > 0:
        reserve_after = max(0, base - expected)
    ratio = first_float(summary, "base_reserve_to_position_ratio") or (base / spend if base > 0 else 0.0)
    impact = first_int(summary, "exit_price_impact_bps_at_entry", "exit_guard_price_impact_bps", "exit_price_impact_bps")
    if impact <= 0 and base > 0 and expected > 0:
        impact = (expected * 10_000) // base
    zero_recent = safe_int(nested_get(summary, "exit_quote_zero_recent_count"))
    decline = first_int(summary, "base_reserve_decline_velocity_bps")
    if expected <= 0 or min_out <= 0:
        return "PARK_STRUCTURAL_EXIT_QUOTE_ZERO", "exit_quote_zero"
    if ratio < args.min_base_reserve_position_ratio:
        return "PARK_STRUCTURAL_BASE_RESERVE_RATIO", "base_reserve_to_position_ratio_low"
    if reserve_after < args.reserve_after_exit_floor_lamports:
        return "PARK_STRUCTURAL_RESERVE_AFTER_EXIT_FLOOR", "reserve_after_hypothetical_exit_below_floor"
    if impact > args.max_exit_price_impact_bps:
        return "PARK_STRUCTURAL_PRICE_IMPACT", "exit_price_impact_too_high"
    if zero_recent > args.max_quote_zero_recent_count:
        return "PARK_STRUCTURAL_QUOTE_ZERO_RECENT", "exit_quote_zero_recent"
    if args.max_base_reserve_decline_bps >= 0 and decline > args.max_base_reserve_decline_bps:
        return "PARK_STRUCTURAL_BASE_RESERVE_DECLINE", "base_reserve_decline_velocity_negative"
    stability_checks = first_int(summary, "exit_quote_stability_checks", "exit_quote_stability_observed_checks")
    if stability_checks < args.exit_quote_stability_checks:
        return "PARK_STRUCTURAL_QUOTE_STABILITY_NOT_PROVEN", "exit_quote_stability_checks_too_low"
    return "STRUCTURAL_NO_EXIT_FILTER_PASS", "none"


def autopsy_attempt(attempt_dir: Path, args: argparse.Namespace) -> dict[str, Any] | None:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    if not summary:
        return None
    exit_reason = first_text(summary, "exit_trigger_reason", "exit_reason", "flatten_reason")
    is_tail = (
        first_text(summary, "final_status", "status") == "UNWINDLESS_BURN_RECONCILED"
        or exit_reason == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(summary.get("burn_close_landed"))
    )
    if not is_tail and not args.include_all_attempts:
        return None

    samples = monitor_samples(summary)
    entry_base_ns = first_int(summary, "entry_send_result_ns", "entry_confirmation_ended_ns", "monitor_started_ns")
    first_zero_ms = first_sample_event_ms(
        samples,
        entry_base_ns,
        lambda sample: safe_int(sample.get("exit_quote_expected_out_raw") or sample.get("exit_expected_out_raw")) <= 0
        or text(sample.get("exit_reason") or sample.get("exit_trigger_reason")) == "EXIT_EXIT_QUOTE_ZERO",
    )
    first_liquidity_warning_ms = first_sample_event_ms(
        samples,
        entry_base_ns,
        lambda sample: not boolish(sample.get("exit_fragility_guard_ok", True))
        or not boolish(sample.get("exit_liquidity_guard_ok", True))
        or safe_int(sample.get("exit_price_impact_bps")) > args.max_exit_price_impact_bps,
    )
    first_sell_or_drain_ms = first_sample_event_ms(
        samples,
        entry_base_ns,
        lambda sample: safe_int(sample.get("sell_count_10s") or sample.get("sell_count_5s")) > 0
        or safe_int(sample.get("pool_liquidity_drop_bps")) > 0,
    )
    entry_size = first_int(summary, "required_spend_lamports", "entry_size_lamports") or int(round(first_float(summary, "max_spend_sol") * 1_000_000_000)) or args.max_spend_lamports
    base_reserve = first_int(summary, "base_reserve_raw_at_entry", "exit_base_reserve_lamports", "base_reserve_raw")
    reserve_after = first_int(summary, "reserve_after_hypothetical_exit", "exit_reserve_after_exit_lamports", "reserve_after_exit_raw")
    expected_entry = first_int(summary, "presend_entry_expected_out_raw", "entry_expected_out_raw")
    exit_expected_entry = first_int(summary, "exit_quote_expected_out_raw", "exit_expected_out_raw")
    exit_min_entry = first_int(summary, "exit_quote_min_out_raw", "exit_min_out_raw")
    if reserve_after <= 0 and base_reserve > 0:
        reserve_after = max(0, base_reserve - exit_expected_entry)
    structural, structural_blocker = structural_decision(summary, args)
    return {
        "attempt": attempt_dir.name,
        "mint": first_text(summary, "selected_mint", "target_mint", "mint"),
        "pool": first_text(summary, "selected_pool", "target_pool", "pool"),
        "entry_time": first_int(summary, "entry_send_result_ns", "entry_confirmation_ended_ns"),
        "entry_size_lamports": entry_size,
        "entry_expected_out_raw": expected_entry,
        "exit_expected_out_raw_at_entry": exit_expected_entry,
        "exit_min_out_raw_at_entry": exit_min_entry,
        "base_reserve_raw_at_entry": base_reserve,
        "token_reserve_raw_at_entry": first_int(summary, "current_quote_reserve_raw", "token_reserve_raw_at_entry"),
        "base_reserve_to_position_ratio": first_float(summary, "base_reserve_to_position_ratio") or (base_reserve / max(1, entry_size)),
        "reserve_after_hypothetical_exit": reserve_after,
        "exit_price_impact_bps_at_entry": first_int(summary, "exit_guard_price_impact_bps", "exit_price_impact_bps_at_entry", "exit_price_impact_bps"),
        "exit_quote_stability_checks": first_int(summary, "exit_quote_stability_checks", "exit_quote_stability_observed_checks"),
        "exit_quote_stability_degrade_bps": first_int(summary, "exit_quote_stability_degrade_bps"),
        "base_reserve_decline_velocity_bps": first_int(summary, "base_reserve_decline_velocity_bps"),
        "base_reserve_raw_at_exit": first_int(summary, "pool_liquidity_current_lamports", "exit_base_reserve_lamports"),
        "exit_quote_expected_out_at_exit": first_int(summary, "exit_quote_expected_out_raw", "exit_expected_out_raw"),
        "time_from_entry_to_exit_quote_zero_ms": first_zero_ms,
        "first_liquidity_warning_ms": first_liquidity_warning_ms,
        "first_sell_or_drain_event": first_sell_or_drain_ms,
        "creator_or_top_holder_sell": boolish(summary.get("creator_sell_seen")) or boolish(summary.get("top_holder_sell_seen")),
        "exit_reason": exit_reason,
        "auto_quarantine_invoked": boolish(summary.get("auto_quarantine_invoked")),
        "burn_close_landed": boolish(summary.get("burn_close_landed")),
        "pnl_after_rent_recovery": safe_int(summary.get("pnl_after_rent_recovery")),
        "structural_no_exit_decision_at_entry": structural,
        "structural_no_exit_first_blocker": structural_blocker,
        "new_filter_would_block": structural != "STRUCTURAL_NO_EXIT_FILTER_PASS",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch38 no-exit tail autopsy.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--include-all-attempts", action="store_true")
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--min-base-reserve-position-ratio", type=float, default=50.0)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=25_000_000)
    parser.add_argument("--max-quote-zero-recent-count", type=int, default=0)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1000)
    parser.add_argument("--max-base-reserve-decline-bps", type=int, default=0)
    parser.add_argument("--exit-quote-stability-checks", type=int, default=5)
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch38_no_exit_tail_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        row = autopsy_attempt(attempt_dir, args)
        if row:
            rows.append(row)
    summary = {
        "status": "NO_EXIT_TAIL_AUTOPSY_COMPLETE",
        "run_root": str(run_root),
        "no_exit_tail_rows": sum(1 for row in rows if row.get("burn_close_landed") or row.get("exit_reason") == "EXIT_EXIT_QUOTE_ZERO"),
        "rows_emitted": len(rows),
        "new_filter_would_block_count": sum(1 for row in rows if row.get("new_filter_would_block")),
        "tail_pnl_after_rent_recovery_total": sum(safe_int(row.get("pnl_after_rent_recovery")) for row in rows),
    }
    (out_dir / "rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text(
        "\n".join(f"{key}={value}" for key, value in summary.items()) + "\n",
        encoding="utf-8",
    )
    for key, value in summary.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


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


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def sample_ns(sample: dict[str, Any]) -> int:
    ns = safe_int(sample.get("sample_ns") or sample.get("snapshot_ns") or sample.get("ns"))
    if ns > 0:
        return ns
    ts = safe_float(sample.get("ts"))
    return int(ts * 1_000_000_000) if ts > 0 else 0


def sample_reserve(sample: dict[str, Any]) -> int:
    return (
        safe_int(sample.get("pool_liquidity_current_lamports"))
        or safe_int(sample.get("base_reserve_raw"))
        or safe_int(sample.get("exit_base_reserve_lamports"))
    )


def sample_expected(sample: dict[str, Any]) -> int:
    return (
        safe_int(sample.get("exit_quote_expected_out_raw"))
        or safe_int(sample.get("exit_expected_out_raw"))
        or safe_int(sample.get("expected_out_raw"))
    )


def load_attempt_summaries(run_root: Path) -> list[tuple[Path, dict[str, Any]]]:
    rows: list[tuple[Path, dict[str, Any]]] = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
        if not summary:
            summary = load_json(attempt_dir / "launch5_one_mint_live_execute.json")
        if summary:
            rows.append((attempt_dir, summary))
    return rows


def is_no_exit_tail(summary: dict[str, Any]) -> bool:
    exit_reason = text(summary.get("exit_trigger_reason") or summary.get("exit_reason") or summary.get("flatten_reason"))
    return (
        text(summary.get("final_status") or summary.get("status")) == "UNWINDLESS_BURN_RECONCILED"
        or exit_reason == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(summary.get("burn_close_landed"))
    )


def load_full_attempt(attempt_dir: Path, summary: dict[str, Any]) -> dict[str, Any]:
    full = load_json(attempt_dir / "launch5_one_mint_live_execute.json")
    if full:
        merged = dict(summary)
        merged.update(full)
        return merged
    return summary


def monitor_samples(summary: dict[str, Any]) -> list[dict[str, Any]]:
    samples = summary.get("monitor_samples")
    if isinstance(samples, list):
        return [item for item in samples if isinstance(item, dict)]
    samples = summary.get("monitor_samples_tail")
    if isinstance(samples, list):
        return [item for item in samples if isinstance(item, dict)]
    path = Path(text(summary.get("monitor_samples_path")))
    if path.exists():
        rows: list[dict[str, Any]] = []
        for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                loaded = json.loads(raw)
            except Exception:
                continue
            if isinstance(loaded, dict):
                rows.append(loaded)
        return rows
    return []


def sample_after(samples: list[dict[str, Any]], entry_ns: int, seconds: float) -> dict[str, Any]:
    target_ns = entry_ns + int(seconds * 1_000_000_000)
    for sample in samples:
        ns = sample_ns(sample)
        if ns >= target_ns:
            return sample
    return {}


def first_zero_sample(samples: list[dict[str, Any]]) -> dict[str, Any]:
    for sample in samples:
        if sample_expected(sample) <= 0 or safe_int(sample.get("executable_exit_value_lamports")) <= 0:
            return sample
    return {}


def first_reserve_drop_sample(samples: list[dict[str, Any]], entry_reserve: int) -> dict[str, Any]:
    if entry_reserve <= 0:
        return {}
    for sample in samples:
        reserve = sample_reserve(sample)
        if reserve > 0 and reserve < entry_reserve:
            return sample
        if reserve <= 1:
            return sample
    return {}


def ns_delta_ms(start_ns: int, end_ns: int) -> int | None:
    if start_ns <= 0 or end_ns <= 0:
        return None
    return max(0, (end_ns - start_ns) // 1_000_000)


def autopsy_attempt(attempt_dir: Path, raw_summary: dict[str, Any]) -> dict[str, Any] | None:
    summary = load_full_attempt(attempt_dir, raw_summary)
    if not is_no_exit_tail(summary):
        return None
    samples = sorted(monitor_samples(summary), key=sample_ns)
    entry_ns = safe_int(summary.get("entry_send_result_ns") or summary.get("entry_confirmation_ended_ns"))
    zero_sample = first_zero_sample(samples)
    zero_ns = sample_ns(zero_sample)
    first_drop_sample = first_reserve_drop_sample(samples, safe_int(summary.get("pool_liquidity_entry_lamports")))
    first_drop_ns = sample_ns(first_drop_sample)
    reserve_entry = (
        safe_int(summary.get("pool_liquidity_entry_lamports"))
        or safe_int(summary.get("base_reserve_raw_at_entry"))
        or safe_int(summary.get("exit_base_reserve_lamports"))
    )
    reserve_1s = sample_reserve(sample_after(samples, entry_ns, 1.0))
    reserve_2s = sample_reserve(sample_after(samples, entry_ns, 2.0))
    reserve_5s = sample_reserve(sample_after(samples, entry_ns, 5.0))
    reserve_zero = sample_reserve(zero_sample)
    before_zero = {}
    if zero_ns:
        before = [sample for sample in samples if sample_ns(sample) and sample_ns(sample) < zero_ns]
        before_zero = before[-1] if before else {}
    return {
        "attempt": attempt_dir.name,
        "mint": text(summary.get("selected_mint") or summary.get("target_mint") or summary.get("mint")),
        "pool": text(summary.get("selected_pool") or summary.get("target_pool") or summary.get("pool")),
        "entry_signature": text(summary.get("acquisition_tx_signature")),
        "entry_time_ns": entry_ns,
        "exit_quote_zero_time_ns": zero_ns or None,
        "base_reserve_at_entry": reserve_entry,
        "base_reserve_after_entry_1s": reserve_1s,
        "base_reserve_after_entry_2s": reserve_2s,
        "base_reserve_after_entry_5s": reserve_5s,
        "base_reserve_before_zero": sample_reserve(before_zero),
        "base_reserve_at_zero": reserve_zero,
        "reserve_drop_bps_1s": bps_drop(reserve_entry, reserve_1s),
        "reserve_drop_bps_2s": bps_drop(reserve_entry, reserve_2s),
        "reserve_drop_bps_5s": bps_drop(reserve_entry, reserve_5s),
        "time_to_first_reserve_drop_ms": ns_delta_ms(entry_ns, first_drop_ns),
        "time_to_exit_quote_zero_ms": ns_delta_ms(entry_ns, zero_ns),
        "last_buy_before_collapse": summary.get("last_buy_seen_ns"),
        "last_sell_before_collapse": summary.get("last_sell_seen_ns"),
        "creator_sell_seen": boolish(summary.get("creator_sell_seen")),
        "top_holder_sell_seen": boolish(summary.get("top_holder_sell_seen")),
        "largest_sell_before_collapse": safe_int(summary.get("largest_sell_10s") or summary.get("largest_sell_before_collapse")),
        "route_family": text(summary.get("late_follow_buy_route_family")),
        "reserve_source": "pool_liquidity_current_lamports/base_reserve_raw",
        "quote_source": text(summary.get("quote_source_used") or (summary.get("presend_last_snapshot") or {}).get("quote_source_used")),
        "reference_price_gain_bps": safe_int(summary.get("reference_price_gain_bps")),
        "executable_exit_gain_bps": safe_int(summary.get("executable_exit_gain_bps")),
        "peak_executable_exit_gain_bps": safe_int(summary.get("peak_executable_exit_gain_bps")),
        "executable_exit_value_lamports": safe_int(summary.get("executable_exit_value_lamports")),
        "exit_reason": text(summary.get("exit_trigger_reason") or summary.get("flatten_reason")),
        "pnl_after_rent_recovery": safe_int(summary.get("pnl_after_rent_recovery") or summary.get("pnl_lamports")),
        "route_or_reserve_mismatch_suspected": reserve_entry > 0 and reserve_zero <= 1 and not boolish(summary.get("creator_sell_seen")),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch42 post-entry liquidity cliff autopsy.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch42_post_entry_liquidity_cliff_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for attempt_dir, summary in load_attempt_summaries(run_root):
        row = autopsy_attempt(attempt_dir, summary)
        if row:
            rows.append(row)
    summary = {
        "status": "POST_ENTRY_LIQUIDITY_CLIFF_AUTOPSY_COMPLETE",
        "run_root": str(run_root),
        "tail_rows": len(rows),
        "route_or_reserve_mismatch_suspected_count": sum(1 for row in rows if row.get("route_or_reserve_mismatch_suspected")),
        "exit_quote_zero_rows": sum(1 for row in rows if row.get("exit_reason") == "EXIT_EXIT_QUOTE_ZERO"),
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

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


def sample_expected(sample: dict[str, Any]) -> int:
    return safe_int(sample.get("exit_quote_expected_out_raw") or sample.get("exit_expected_out_raw") or sample.get("expected_out_raw"))


def sample_executable(sample: dict[str, Any]) -> int:
    return safe_int(sample.get("executable_exit_value_lamports"), sample_expected(sample))


def sample_reserve(sample: dict[str, Any]) -> int:
    return safe_int(sample.get("pool_liquidity_current_lamports") or sample.get("base_reserve_raw"))


def load_samples(summary: dict[str, Any]) -> list[dict[str, Any]]:
    samples = summary.get("monitor_samples")
    if isinstance(samples, list):
        return [item for item in samples if isinstance(item, dict)]
    samples = summary.get("monitor_samples_tail")
    if isinstance(samples, list):
        return [item for item in samples if isinstance(item, dict)]
    return []


def is_no_exit_tail(summary: dict[str, Any]) -> bool:
    exit_reason = text(summary.get("exit_trigger_reason") or summary.get("flatten_reason"))
    return (
        text(summary.get("final_status") or summary.get("status")) == "UNWINDLESS_BURN_RECONCILED"
        or exit_reason == "EXIT_EXIT_QUOTE_ZERO"
        or boolish(summary.get("burn_close_landed"))
    )


def first_zero_ns(samples: list[dict[str, Any]]) -> int:
    for sample in samples:
        if sample_expected(sample) <= 0 or sample_executable(sample) <= 0:
            return sample_ns(sample)
    return 0


def replay_attempt(attempt_dir: Path, args: argparse.Namespace) -> dict[str, Any] | None:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute.json")
    if not summary:
        summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    if not summary:
        return None
    samples = sorted(load_samples(summary), key=sample_ns)
    entry_ns = safe_int(summary.get("entry_send_result_ns") or summary.get("entry_confirmation_ended_ns"))
    peak_exec = 0
    weakening_streak = 0
    last_expected = 0
    first_trigger = ""
    first_trigger_ns = 0
    for idx, sample in enumerate(samples):
        ns = sample_ns(sample)
        elapsed_s = max(0.0, (ns - entry_ns) / 1_000_000_000) if entry_ns and ns else 0.0
        expected = sample_expected(sample)
        executable = sample_executable(sample)
        reserve = sample_reserve(sample)
        peak_exec = max(peak_exec, executable)
        if last_expected > 0 and expected > 0 and expected < last_expected:
            weakening_streak += 1
        elif expected > 0:
            weakening_streak = 0
        if expected > 0:
            last_expected = expected
        previous = samples[:idx]
        recent_1s = [sample_reserve(row) for row in previous if ns and sample_ns(row) and ns - sample_ns(row) <= 1_000_000_000]
        recent_2s = [sample_reserve(row) for row in previous if ns and sample_ns(row) and ns - sample_ns(row) <= 2_000_000_000]
        recent_5s = [sample_reserve(row) for row in previous if ns and sample_ns(row) and ns - sample_ns(row) <= 5_000_000_000]
        drop_1s = bps_drop(max(recent_1s + [reserve]) if recent_1s or reserve else 0, reserve)
        drop_2s = bps_drop(max(recent_2s + [reserve]) if recent_2s or reserve else 0, reserve)
        drop_5s = bps_drop(max(recent_5s + [reserve]) if recent_5s or reserve else 0, reserve)
        value_collapse = bps_drop(peak_exec, executable)
        current_gain = safe_int(sample.get("executable_exit_gain_bps") or sample.get("current_gain_bps"))
        pre_arm_weak = (
            args.pre_arm_timeout_seconds > 0
            and elapsed_s >= args.pre_arm_timeout_seconds
            and current_gain < args.pre_arm_min_gain_bps
            and not boolish(sample.get("cost_profit_protector_armed"))
        )
        if args.drop_1s_bps > 0 and drop_1s >= args.drop_1s_bps:
            first_trigger, first_trigger_ns = "EXIT_LIQUIDITY_CLIFF_1S", ns
            break
        if args.drop_2s_bps > 0 and drop_2s >= args.drop_2s_bps:
            first_trigger, first_trigger_ns = "EXIT_LIQUIDITY_CLIFF_2S", ns
            break
        if args.drop_5s_bps > 0 and drop_5s >= args.drop_5s_bps:
            first_trigger, first_trigger_ns = "EXIT_LIQUIDITY_CLIFF_5S", ns
            break
        if args.exit_value_collapse_bps > 0 and value_collapse >= args.exit_value_collapse_bps:
            first_trigger, first_trigger_ns = "EXIT_EXIT_VALUE_COLLAPSE", ns
            break
        if args.quote_weakening_checks > 0 and weakening_streak >= args.quote_weakening_checks:
            first_trigger, first_trigger_ns = "EXIT_QUOTE_WEAKENING", ns
            break
        if pre_arm_weak:
            first_trigger, first_trigger_ns = "EXIT_PRE_ARM_WEAK_TIMEOUT", ns
            break
    zero_ns = first_zero_ns(samples)
    return {
        "attempt": attempt_dir.name,
        "mint": text(summary.get("selected_mint") or summary.get("target_mint")),
        "pool": text(summary.get("selected_pool") or summary.get("target_pool")),
        "is_no_exit_tail": is_no_exit_tail(summary),
        "first_sentinel_trigger": first_trigger,
        "first_sentinel_trigger_ns": first_trigger_ns or None,
        "exit_quote_zero_ns": zero_ns or None,
        "would_intercept_before_quote_zero": bool(first_trigger_ns and zero_ns and first_trigger_ns < zero_ns),
        "final_status": text(summary.get("final_status") or summary.get("status")),
        "exit_reason": text(summary.get("exit_trigger_reason") or summary.get("flatten_reason")),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch43 liquidity cliff sentinel replay.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--drop-1s-bps", type=int, default=1500)
    parser.add_argument("--drop-2s-bps", type=int, default=2500)
    parser.add_argument("--drop-5s-bps", type=int, default=4000)
    parser.add_argument("--exit-value-collapse-bps", type=int, default=2000)
    parser.add_argument("--quote-weakening-checks", type=int, default=2)
    parser.add_argument("--pre-arm-timeout-seconds", type=int, default=30)
    parser.add_argument("--pre-arm-min-gain-bps", type=int, default=500)
    args = parser.parse_args()
    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch43_liquidity_cliff_sentinel"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        row = replay_attempt(attempt_dir, args)
        if row:
            rows.append(row)
    tail_rows = [row for row in rows if row.get("is_no_exit_tail")]
    summary = {
        "status": "LIQUIDITY_CLIFF_SENTINEL_REPLAY_COMPLETE",
        "run_root": str(run_root),
        "rows_seen": len(rows),
        "no_exit_tail_count": len(tail_rows),
        "would_intercept_tail_count": sum(1 for row in tail_rows if row.get("would_intercept_before_quote_zero")),
        "acceptance_pass": bool(tail_rows) and all(row.get("would_intercept_before_quote_zero") for row in tail_rows),
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

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def iter_docs(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    if root.is_file():
        row = load_json(root)
        return [(root, row)] if row else []
    if not root.exists():
        return []
    paths = list(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    paths.extend(root.glob("**/launch5_one_mint_live_execute.json"))
    docs: list[tuple[Path, dict[str, Any]]] = []
    for path in sorted(set(paths), key=lambda item: str(item)):
        row = load_json(path)
        if row:
            docs.append((path, row))
    return docs


def nested(row: dict[str, Any], key: str) -> Any:
    if key in row:
        return row.get(key)
    latest = row.get("latest_attempt_summary")
    if isinstance(latest, dict) and key in latest:
        return latest.get(key)
    latest_report = row.get("latest_attempt_report")
    if isinstance(latest_report, dict) and key in latest_report:
        return latest_report.get(key)
    return None


def first_int(row: dict[str, Any], *keys: str) -> int:
    for key in keys:
        parsed = safe_int(nested(row, key))
        if parsed:
            return parsed
    return 0


def first_text(row: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = text(nested(row, key))
        if value:
            return value
    return ""


def monitor_samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = nested(row, "monitor_samples_tail")
    return [item for item in raw if isinstance(item, dict)] if isinstance(raw, list) else []


def sample_value(sample: dict[str, Any], key: str) -> int:
    return safe_int(sample.get(key))


def bps_drop(high_value: int, current_value: int) -> int:
    if high_value <= 0 or current_value >= high_value:
        return 0
    return max(0, ((high_value - current_value) * 10_000) // high_value)


def reserve_change(samples: list[dict[str, Any]], window_seconds: float, latest_reserve: int) -> int:
    if not samples or latest_reserve <= 0:
        return 0
    latest_ts = safe_float(samples[-1].get("ts"))
    candidates = [
        sample_value(sample, "base_reserve_raw") or sample_value(sample, "exit_base_reserve_lamports")
        for sample in samples
        if latest_ts - safe_float(sample.get("ts")) <= window_seconds
    ]
    candidates = [value for value in candidates if value > 0]
    if not candidates:
        return 0
    return bps_drop(max(candidates), latest_reserve)


def time_from_positive_to_zero_ms(samples: list[dict[str, Any]], exit_zero: bool) -> int | None:
    if not exit_zero or not samples:
        return None
    positive_ts = 0.0
    zero_ts = 0.0
    for sample in samples:
        expected = sample_value(sample, "expected_out_raw")
        executable = sample_value(sample, "executable_exit_value_lamports")
        ts = safe_float(sample.get("ts"))
        if expected > 0 or executable > 0:
            positive_ts = ts
        if positive_ts and expected <= 0 and executable <= 0:
            zero_ts = ts
            break
    if positive_ts and zero_ts:
        return max(0, int((zero_ts - positive_ts) * 1000))
    return None


def classify(row: dict[str, Any], base_reserve: int, reserve_after: int, impact_bps: int, exit_zero: bool) -> str:
    combined = lower(
        " ".join(
            [
                first_text(row, "exit_trigger_reason", "flatten_reason", "exit_reason"),
                first_text(row, "first_failure", "initial_flatten_failure"),
            ]
        )
    )
    if exit_zero or "exit_quote_expected_out_zero" in combined or "exit_quote_zero" in combined:
        return "EXIT_EXIT_QUOTE_ZERO"
    if base_reserve > 0 and base_reserve < 25_000_000:
        return "EXIT_EXIT_LIQUIDITY_DETERIORATING"
    if reserve_after > 0 and reserve_after < 10_000_000:
        return "EXIT_EXIT_LIQUIDITY_DETERIORATING"
    if impact_bps >= 1500:
        return "EXIT_EXIT_LIQUIDITY_DETERIORATING"
    return first_text(row, "exit_trigger_reason", "flatten_reason", "exit_reason") or "HOLD_OR_UNKNOWN"


def build_row(path: Path, row: dict[str, Any]) -> dict[str, Any]:
    samples = monitor_samples(row)
    latest_sample = samples[-1] if samples else {}
    expected_out = first_int(row, "exit_quote_expected_out_raw", "expected_out_raw") or sample_value(latest_sample, "expected_out_raw")
    min_out = first_int(row, "exit_quote_min_out_raw", "min_out_raw") or sample_value(latest_sample, "min_out_raw")
    executable_value = first_int(row, "executable_exit_value_lamports") or sample_value(latest_sample, "executable_exit_value_lamports")
    base_reserve = (
        first_int(row, "base_reserve_raw", "exit_base_reserve_lamports")
        or sample_value(latest_sample, "base_reserve_raw")
        or sample_value(latest_sample, "exit_base_reserve_lamports")
    )
    reserve_after = first_int(row, "reserve_after_exit_raw", "exit_reserve_after_exit_lamports")
    impact_bps = first_int(row, "exit_price_impact_bps")
    exit_zero = expected_out <= 0 or min_out <= 0 or executable_value <= 0 or "zero" in lower(first_text(row, "exit_trigger_reason", "flatten_reason", "first_failure"))
    decision = classify(row, base_reserve, reserve_after, impact_bps, exit_zero)
    return {
        "source_path": str(path),
        "selected_mint": first_text(row, "selected_mint", "target_mint"),
        "selected_pool": first_text(row, "selected_pool", "target_pool"),
        "reference_price_gain_bps": first_int(row, "reference_price_gain_bps", "current_gain_bps"),
        "executable_exit_gain_bps": first_int(row, "executable_exit_gain_bps"),
        "executable_exit_value_lamports": executable_value,
        "exit_quote_expected_out_raw": expected_out,
        "exit_quote_min_out_raw": min_out,
        "exit_price_impact_bps": impact_bps,
        "base_reserve_raw": base_reserve,
        "base_reserve_change_1s_bps": reserve_change(samples, 1.0, base_reserve),
        "base_reserve_change_5s_bps": reserve_change(samples, 5.0, base_reserve),
        "base_reserve_change_10s_bps": reserve_change(samples, 10.0, base_reserve),
        "reserve_after_exit_raw": reserve_after,
        "quote_stability_score": max(0, 10_000 - first_int(row, "exit_quote_stability_degrade_bps")),
        "exit_quote_zero_count": first_int(row, "exit_quote_zero_count") or (1 if exit_zero else 0),
        "time_from_exit_quote_positive_to_zero_ms": time_from_positive_to_zero_ms(samples, exit_zero),
        "exit_reason": first_text(row, "exit_trigger_reason", "flatten_reason", "exit_reason"),
        "first_failure": first_text(row, "first_failure", "initial_flatten_failure"),
        "decision": decision,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Autopsy Launch R3-HOLD executable exit-liquidity fragility.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH25_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()
    root = Path(args.run_root)
    rows = [build_row(path, row) for path, row in iter_docs(root)]
    decision_counts: dict[str, int] = {}
    for row in rows:
        decision = text(row.get("decision"))
        decision_counts[decision] = decision_counts.get(decision, 0) + 1
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch25_exit_liquidity_fragility_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    summary = {
        "rows_seen": len(rows),
        "exit_quote_zero_rows": decision_counts.get("EXIT_EXIT_QUOTE_ZERO", 0),
        "liquidity_deteriorating_rows": decision_counts.get("EXIT_EXIT_LIQUIDITY_DETERIORATING", 0),
        "decision_counts": decision_counts,
        "latest_decision": rows[-1]["decision"] if rows else "",
        "latest_base_reserve_raw": rows[-1]["base_reserve_raw"] if rows else 0,
        "latest_executable_exit_value_lamports": rows[-1]["executable_exit_value_lamports"] if rows else 0,
        "latest_time_from_exit_quote_positive_to_zero_ms": rows[-1]["time_from_exit_quote_positive_to_zero_ms"] if rows else None,
    }
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()) + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

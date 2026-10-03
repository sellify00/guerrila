from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from statistics import median
from typing import Any


MIN_BASE_VARIANTS = [5_000_000, 10_000_000, 25_000_000, 50_000_000]
MAX_IMPACT_VARIANTS = [1000, 1500, 2500]
STABILITY_CHECK_VARIANTS = [1, 2, 3]
RESERVE_AFTER_FLOOR_LAMPORTS = 10_000_000
MAX_STABILITY_DEGRADE_BPS = 1500


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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


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
    paths.extend(root.glob("**/launch10_hot_stream_executor_summary.json"))
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
    if isinstance(latest, dict):
        if key in latest:
            return latest.get(key)
        snapshot = latest.get("presend_last_snapshot")
        if isinstance(snapshot, dict) and key in snapshot:
            return snapshot.get(key)
    latest_report = row.get("latest_attempt_report")
    if isinstance(latest_report, dict):
        if key in latest_report:
            return latest_report.get(key)
        snapshot = latest_report.get("presend_last_snapshot")
        if isinstance(snapshot, dict) and key in snapshot:
            return snapshot.get(key)
    snapshot = row.get("presend_last_snapshot")
    if isinstance(snapshot, dict) and key in snapshot:
        return snapshot.get(key)
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


def candidate_row(path: Path, row: dict[str, Any]) -> dict[str, Any]:
    expected = first_int(row, "exit_quote_expected_out_raw", "exit_expected_out_raw", "expected_out_raw")
    min_out = first_int(row, "exit_quote_min_out_raw", "exit_min_out_raw", "min_out_raw")
    base_reserve = first_int(row, "base_reserve_raw", "exit_base_reserve_lamports", "sell_reserve_out_raw")
    reserve_after = first_int(row, "reserve_after_exit_raw", "exit_reserve_after_exit_lamports")
    if reserve_after <= 0 and base_reserve > 0 and expected > 0:
        reserve_after = max(0, base_reserve - expected)
    return {
        "source_path": str(path),
        "selected_mint": first_text(row, "selected_mint", "target_mint", "launch_mint"),
        "expected_out_raw": expected,
        "min_out_raw": min_out,
        "base_reserve_raw": base_reserve,
        "reserve_after_exit_raw": reserve_after,
        "exit_price_impact_bps": first_int(row, "exit_price_impact_bps", "exit_guard_price_impact_bps"),
        "exit_quote_stability_checks": first_int(row, "exit_quote_stability_checks"),
        "exit_quote_stability_degrade_bps": first_int(row, "exit_quote_stability_degrade_bps"),
        "exit_quote_stable_across_checks": boolish(nested(row, "exit_quote_stable_across_checks")),
        "peak_executable_exit_gain_bps": first_int(row, "peak_executable_exit_gain_bps", "executable_exit_gain_bps"),
        "exit_quote_zero_after_entry": "exit_quote_zero" in lower(first_text(row, "exit_trigger_reason", "flatten_reason", "first_failure", "initial_flatten_failure")),
        "simulated_no_exit_loss": "burn" in lower(first_text(row, "final_status", "status")) or "unwindless" in lower(first_text(row, "final_status", "status")),
    }


def variant_passes(row: dict[str, Any], min_base: int, max_impact: int, required_checks: int) -> bool:
    if safe_int(row.get("expected_out_raw")) <= 0 or safe_int(row.get("min_out_raw")) <= 0:
        return False
    if safe_int(row.get("base_reserve_raw")) < min_base:
        return False
    if safe_int(row.get("reserve_after_exit_raw")) < RESERVE_AFTER_FLOOR_LAMPORTS:
        return False
    impact = safe_int(row.get("exit_price_impact_bps"))
    if impact > 0 and impact > max_impact:
        return False
    observed_checks = safe_int(row.get("exit_quote_stability_checks"))
    if required_checks > 1 and observed_checks > 0 and observed_checks < required_checks:
        return False
    degrade = safe_int(row.get("exit_quote_stability_degrade_bps"))
    if degrade >= MAX_STABILITY_DEGRADE_BPS:
        return False
    if required_checks > 1 and observed_checks > 0 and not boolish(row.get("exit_quote_stable_across_checks")):
        return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Sweep Launch R3-HOLD exit-liquidity guard thresholds over artifacts.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH27_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    root = Path(args.run_root)
    candidates = [candidate_row(path, row) for path, row in iter_docs(root)]
    rows: list[dict[str, Any]] = []
    for min_base in MIN_BASE_VARIANTS:
        for max_impact in MAX_IMPACT_VARIANTS:
            for checks in STABILITY_CHECK_VARIANTS:
                passed = [row for row in candidates if variant_passes(row, min_base, max_impact, checks)]
                peak_values = [safe_int(row.get("peak_executable_exit_gain_bps")) for row in passed if safe_int(row.get("peak_executable_exit_gain_bps"))]
                rows.append(
                    {
                        "guard_variant": f"base={min_base}:impact={max_impact}:checks={checks}",
                        "min_base_reserve_lamports": min_base,
                        "max_exit_price_impact_bps": max_impact,
                        "quote_stability_checks": checks,
                        "candidate_count": len(candidates),
                        "would_send_count": len(passed),
                        "exit_quote_zero_after_entry_count": sum(1 for row in passed if row.get("exit_quote_zero_after_entry")),
                        "simulated_no_exit_loss_count": sum(1 for row in passed if row.get("simulated_no_exit_loss")),
                        "median_peak_executable_gain_bps": int(median(peak_values)) if peak_values else 0,
                    }
                )

    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch27_exit_liquidity_threshold_sweep"
    out_dir.mkdir(parents=True, exist_ok=True)
    summary = {
        "candidate_count": len(candidates),
        "variants_seen": len(rows),
        "max_would_send_count": max((safe_int(row.get("would_send_count")) for row in rows), default=0),
        "current_guard_would_send_count": next(
            (
                safe_int(row.get("would_send_count"))
                for row in rows
                if row["min_base_reserve_lamports"] == 25_000_000
                and row["max_exit_price_impact_bps"] == 1500
                and row["quote_stability_checks"] == 3
            ),
            0,
        ),
    }
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{key}={value}" for key, value in summary.items()) + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

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
    paths.extend(root.glob("**/summary.json"))
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
    latest_report = row.get("latest_attempt_report")
    if isinstance(latest_report, dict):
        if key in latest_report:
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


def samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = nested(row, "monitor_samples_tail")
    return raw if isinstance(raw, list) else []


def last_sample_int(row: dict[str, Any], key: str) -> int:
    for sample in reversed(samples(row)):
        if isinstance(sample, dict):
            parsed = safe_int(sample.get(key))
            if parsed:
                return parsed
    return 0


def classify(row: dict[str, Any], executable_value: int, reference_gain: int, executable_drawdown: int) -> str:
    reason = first_text(row, "exit_trigger_reason", "flatten_reason", "exit_reason", "first_failure")
    combined = lower(reason + " " + first_text(row, "initial_flatten_failure", "first_failure"))
    if "exit_quote_expected_out_zero" in combined or "exit_quote_zero" in combined:
        if reference_gain > 0 and executable_value <= 0:
            return "EXIT_REFERENCE_PRICE_FAKE_GAIN"
        return "EXIT_EXIT_QUOTE_ZERO"
    if reason == "EXIT_COST_PROTECTOR_TRAILING":
        return "EXIT_REALISABLE_PROFIT_TRAILING"
    if reason == "EXIT_REALISABLE_PROFIT_TRAILING":
        return reason
    if executable_drawdown >= 2500:
        return "EXIT_REALISABLE_PNL_DRAWDOWN"
    if "route" in combined or "liquidity" in combined:
        return "EXIT_EXIT_LIQUIDITY_DETERIORATING"
    return reason or "HOLD_OR_UNKNOWN"


def build_row(path: Path, row: dict[str, Any]) -> dict[str, Any]:
    reference_gain = first_int(row, "reference_price_gain_bps", "current_gain_bps")
    executable_gain = first_int(row, "executable_exit_gain_bps")
    executable_value = first_int(row, "executable_exit_value_lamports")
    if executable_value <= 0 and "zero" not in lower(first_text(row, "exit_trigger_reason", "flatten_reason", "first_failure")):
        executable_value = first_int(row, "estimated_exit_value_lamports")
    expected_out = first_int(row, "exit_quote_expected_out_raw", "expected_out_raw") or last_sample_int(row, "expected_out_raw")
    min_out = first_int(row, "exit_quote_min_out_raw", "min_out_raw") or last_sample_int(row, "min_out_raw")
    executable_drawdown = first_int(row, "executable_exit_drawdown_bps", "drawdown_from_peak_bps")
    if executable_gain <= 0 and executable_value > 0:
        entry_value = first_int(row, "entry_executable_exit_value_lamports", "entry_value_lamports")
        if entry_value > 0 and executable_value > entry_value:
            executable_gain = ((executable_value - entry_value) * 10_000) // entry_value
    decision = classify(row, executable_value, reference_gain, executable_drawdown)
    return {
        "source_path": str(path),
        "selected_mint": first_text(row, "selected_mint", "target_mint"),
        "selected_pool": first_text(row, "selected_pool", "target_pool"),
        "reference_price_gain_bps": reference_gain,
        "peak_reference_price_gain_bps": first_int(row, "peak_reference_price_gain_bps", "peak_gain_bps"),
        "executable_exit_gain_bps": executable_gain,
        "peak_executable_exit_gain_bps": first_int(row, "peak_executable_exit_gain_bps", "peak_gain_bps"),
        "executable_exit_drawdown_bps": executable_drawdown,
        "executable_exit_value_lamports": executable_value,
        "exit_quote_expected_out_raw": expected_out,
        "exit_quote_min_out_raw": min_out,
        "exit_quote_zero_count": first_int(row, "exit_quote_zero_count") or (1 if decision in {"EXIT_EXIT_QUOTE_ZERO", "EXIT_REFERENCE_PRICE_FAKE_GAIN"} else 0),
        "pnl_lamports": first_int(row, "pnl_lamports"),
        "pnl_after_rent_recovery": first_int(row, "pnl_after_rent_recovery", "pnl_after_lifecycle_lamports"),
        "exit_reason": first_text(row, "exit_trigger_reason", "flatten_reason", "exit_reason"),
        "first_failure": first_text(row, "first_failure", "initial_flatten_failure"),
        "decision": decision,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Summarize Launch R3-HOLD realisable exit PnL from run artifacts.")
    parser.add_argument("--run-root", default=os.environ.get("SHADOW_LAUNCH26_RUN_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    root = Path(args.run_root)
    rows = [build_row(path, row) for path, row in iter_docs(root)]
    decision_counts: dict[str, int] = {}
    for row in rows:
        decision = text(row.get("decision"))
        decision_counts[decision] = decision_counts.get(decision, 0) + 1
    fake_gain_rows = decision_counts.get("EXIT_REFERENCE_PRICE_FAKE_GAIN", 0)
    quote_zero_rows = decision_counts.get("EXIT_EXIT_QUOTE_ZERO", 0)
    out_dir = Path(args.out_dir) if args.out_dir else root / "22_launch26_realisable_pnl_monitor"
    out_dir.mkdir(parents=True, exist_ok=True)
    summary = {
        "rows_seen": len(rows),
        "reference_price_fake_gain_rows": fake_gain_rows,
        "exit_quote_zero_rows": quote_zero_rows,
        "decision_counts": decision_counts,
        "latest_decision": rows[-1]["decision"] if rows else "",
        "latest_reference_price_gain_bps": rows[-1]["reference_price_gain_bps"] if rows else 0,
        "latest_executable_exit_gain_bps": rows[-1]["executable_exit_gain_bps"] if rows else 0,
        "latest_executable_exit_value_lamports": rows[-1]["executable_exit_value_lamports"] if rows else 0,
    }
    (out_dir / "rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "summary.txt").write_text("\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()) + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

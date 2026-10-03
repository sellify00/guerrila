from __future__ import annotations

import argparse
import json
from collections import Counter
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


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def attempt_rows(root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in sorted((root / "attempts").glob("attempt_*/launch5_one_mint_live_execute_summary.json")):
        row = load_json(path)
        if row:
            row["_attempt"] = path.parent.name
            rows.append(row)
    return rows


def samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = row.get("monitor_samples_tail")
    return raw if isinstance(raw, list) else []


def bps_drop(high: int, low: int) -> int:
    if high <= 0 or low < 0 or low >= high:
        return 0
    return ((high - low) * 10_000) // high


def autopsy_row(row: dict[str, Any]) -> dict[str, Any]:
    tail = samples(row)
    expected_values = [safe_int(item.get("expected_out_raw") or item.get("exit_quote_expected_out_raw")) for item in tail]
    expected_values = [value for value in expected_values if value > 0]
    exit_expected = safe_int(row.get("exit_quote_expected_out_raw"))
    peak_expected = max(expected_values + [exit_expected, 0])
    exit_quote_drop_bps = bps_drop(peak_expected, exit_expected) if exit_expected > 0 else 10_000 if peak_expected > 0 else 0
    sample_peak_gain = max([safe_int(item.get("executable_exit_gain_bps")) for item in tail] + [safe_int(row.get("peak_executable_exit_gain_bps"))])
    sample_peak_exit_value = max([safe_int(item.get("executable_exit_value_lamports")) for item in tail] + [safe_int(row.get("peak_executable_exit_value_lamports"))])
    return {
        "attempt": row.get("_attempt"),
        "mint": text(row.get("selected_mint") or row.get("target_mint")),
        "pool": text(row.get("selected_pool") or row.get("target_pool")),
        "exit_reason": text(row.get("exit_trigger_reason") or row.get("flatten_reason")),
        "exit_gain_bps": safe_int(row.get("executable_exit_gain_bps")),
        "peak_executable_exit_gain_bps": safe_int(row.get("peak_executable_exit_gain_bps")),
        "sample_peak_executable_exit_gain_bps": sample_peak_gain,
        "sample_peak_executable_exit_value_lamports": sample_peak_exit_value,
        "exit_quote_drop_bps": exit_quote_drop_bps,
        "exit_quote_expected_out_raw": exit_expected,
        "exit_quote_zero_seen": boolish(row.get("exit_quote_zero_seen")) or exit_expected <= 0,
        "exit_quote_weakening_streak": safe_int(row.get("exit_quote_weakening_streak")),
        "base_reserve_at_exit": safe_int(row.get("pool_liquidity_current_lamports") or row.get("base_reserve_raw")),
        "exit_price_impact_bps": safe_int(row.get("exit_price_impact_bps")),
        "pool_liquidity_drop_5s_bps": safe_int(row.get("pool_liquidity_drop_5s_bps")),
        "would_hold_30s_peak_gain_bps": None,
        "would_hold_60s_peak_gain_bps": None,
        "would_hold_5m_peak_gain_bps": None,
        "forward_markout_available": False,
        "forward_markout_note": "not_available_from_stopped_monitor_samples",
        "would_hit_no_exit_if_held": None,
        "would_hit_cost_protector_if_held": None,
        "pnl_after_rent_recovery": safe_int(row.get("pnl_after_rent_recovery"), safe_int(row.get("pnl_lamports"))),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch47 B002 clone exit-policy autopsy.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = attempt_rows(root)
    quote_weakening = [row for row in rows if text(row.get("exit_trigger_reason")) == "EXIT_QUOTE_WEAKENING"]
    autopsies = [autopsy_row(row) for row in quote_weakening]
    decision_counts = Counter(text(row.get("exit_trigger_reason") or row.get("flatten_reason")) for row in rows)
    summary = {
        "status": "LAUNCH47_B002_EXIT_POLICY_AUTOPSY_COMPLETE",
        "run_root": str(root),
        "attempt_rows": len(rows),
        "exit_reason_counts": dict(decision_counts),
        "quote_weakening_attempts": len(quote_weakening),
        "quote_weakening_zero_quote_count": sum(1 for row in autopsies if row["exit_quote_zero_seen"]),
        "quote_weakening_forward_markout_available_count": sum(1 for row in autopsies if row["forward_markout_available"]),
        "total_pnl_after_rent_recovery": sum(safe_int(row.get("pnl_after_rent_recovery"), safe_int(row.get("pnl_lamports"))) for row in rows),
    }
    (out_dir / "quote_weakening_autopsy_rows.jsonl").write_text(
        "\n".join(json.dumps(row, sort_keys=True) for row in autopsies) + ("\n" if autopsies else ""),
        encoding="utf-8",
    )
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from launch_chaos_detector import evaluate_rows, load_jsonl, safe_float, safe_int, text


def bps_delta(exit_price: int, compare_price: int) -> int:
    if exit_price <= 0 or compare_price <= 0:
        return 0
    return ((compare_price - exit_price) * 10_000) // exit_price


def main() -> int:
    parser = argparse.ArgumentParser(description="Backtest Launch chaos detector labels against markout rows.")
    parser.add_argument("--snapshots-jsonl", required=True)
    parser.add_argument("--summary-json", required=True)
    parser.add_argument("--rows-jsonl", required=True)
    args = parser.parse_args()

    rows = load_jsonl(Path(args.snapshots_jsonl))
    detector_summary, classified = evaluate_rows(rows)
    out_rows: list[dict[str, Any]] = []
    false_exit_count = 0
    late_exit_count = 0
    catastrophe_caught_count = 0
    missed_profit_bps_values: list[int] = []
    saved_loss_bps_values: list[int] = []

    for row in classified:
        exit_decision = text(row.get("exit_decision"))
        exit_price = safe_int(row.get("current_price") or row.get("exit_expected_out_raw") or row.get("expected_out_raw"))
        actual_peak_after_entry = safe_int(row.get("actual_peak_after_entry") or row.get("peak_after_entry") or row.get("local_high_price"))
        actual_trough_after_entry = safe_int(row.get("actual_trough_after_entry") or row.get("trough_after_entry"))
        hold_1h_price = safe_int(row.get("return_1h_price") or row.get("price_1h") or row.get("exit_expected_out_1h_raw"))
        max_drawdown_after_sell = safe_int(row.get("max_drawdown_after_sell") or row.get("max_drawdown_after_sell_bps"))
        catastrophic_after = max_drawdown_after_sell >= 2500 or safe_int(row.get("catastrophe_after")) > 0

        missed_profit_bps = bps_delta(exit_price, actual_peak_after_entry) if exit_decision.startswith("EXIT_") else 0
        saved_loss_bps = bps_delta(actual_trough_after_entry, exit_price) if exit_decision.startswith("EXIT_") and actual_trough_after_entry > 0 else 0
        if exit_decision.startswith("EXIT_") and hold_1h_price > exit_price:
            false_exit_count += 1
        if not exit_decision.startswith("EXIT_") and catastrophic_after:
            late_exit_count += 1
        if exit_decision.startswith("EXIT_") and catastrophic_after:
            catastrophe_caught_count += 1
        if missed_profit_bps:
            missed_profit_bps_values.append(missed_profit_bps)
        if saved_loss_bps:
            saved_loss_bps_values.append(saved_loss_bps)

        out_rows.append(
            {
                **row,
                "would_exit": exit_decision.startswith("EXIT_"),
                "would_exit_reason": exit_decision,
                "exit_price": exit_price,
                "actual_peak_after_entry": actual_peak_after_entry,
                "actual_trough_after_entry": actual_trough_after_entry,
                "saved_loss_vs_trough_bps": saved_loss_bps,
                "missed_profit_vs_peak_bps": missed_profit_bps,
                "false_exit": exit_decision.startswith("EXIT_") and hold_1h_price > exit_price,
                "late_exit": (not exit_decision.startswith("EXIT_")) and catastrophic_after,
                "catastrophe_caught": exit_decision.startswith("EXIT_") and catastrophic_after,
                "return_1m": safe_float(row.get("return_1m")),
                "return_5m": safe_float(row.get("return_5m")),
                "return_15m": safe_float(row.get("return_15m")),
                "return_1h": safe_float(row.get("return_1h")),
            }
        )

    summary = {
        **detector_summary,
        "status": "BACKTEST_COMPLETE",
        "rows_backtested": len(out_rows),
        "false_exit_count": false_exit_count,
        "late_exit_count": late_exit_count,
        "catastrophe_caught_count": catastrophe_caught_count,
        "avg_missed_profit_vs_peak_bps": int(sum(missed_profit_bps_values) / len(missed_profit_bps_values)) if missed_profit_bps_values else 0,
        "avg_saved_loss_vs_trough_bps": int(sum(saved_loss_bps_values) / len(saved_loss_bps_values)) if saved_loss_bps_values else 0,
    }

    Path(args.summary_json).parent.mkdir(parents=True, exist_ok=True)
    Path(args.summary_json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    Path(args.rows_jsonl).write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in out_rows), encoding="utf-8")
    print("\n".join(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}" for key, value in summary.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

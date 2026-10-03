from __future__ import annotations

import argparse
import csv
import json
import os
import statistics
from pathlib import Path
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


def load_json(path: Path) -> dict[str, Any]:
    try:
        loaded = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return loaded if isinstance(loaded, dict) else {}


def find_reports(root: Path) -> list[Path]:
    reports: list[Path] = []
    if root.is_file():
        return [root]
    if root.exists():
        reports.extend(root.glob("**/launch5_one_mint_live_execute.json"))
        reports.extend(root.glob("**/launch5_one_mint_live_execute_summary.json"))
    unique = {p.resolve(): p for p in reports}
    return sorted(unique.values(), key=lambda p: p.stat().st_mtime)


def bps(numerator: int, denominator: int) -> int:
    if denominator <= 0:
        return 0
    return int(round((numerator * 10_000) / denominator))


def row_from_report(path: Path, report: dict[str, Any]) -> dict[str, Any]:
    entry_ns = safe_int(report.get("entry_send_ns")) or safe_int(report.get("entry_confirmation_started_ns"))
    exit_ns = safe_int(report.get("entry_confirmation_ended_ns"))
    if boolish(report.get("flatten_landed")):
        samples = report.get("monitor_samples_tail") if isinstance(report.get("monitor_samples_tail"), list) else []
        for sample in reversed(samples):
            exit_ns = max(exit_ns, safe_int((sample or {}).get("sample_ns")))
    time_held_ms = max(0, (exit_ns - entry_ns) // 1_000_000) if entry_ns > 0 and exit_ns > 0 else 0
    net_pnl = safe_int(report.get("pnl_lamports"))
    buy_tip = safe_int(report.get("sender_tip_lamports")) if boolish(report.get("send_transaction_called")) else 0
    sell_tip = safe_int(report.get("sender_tip_lamports")) if boolish(report.get("flatten_tx_signature")) else 0
    base_fees = safe_int(report.get("base_tx_fee_lamports")) if report.get("base_tx_fee_lamports") else 0
    return {
        "report_path": str(path),
        "run_root": str(next((parent for parent in path.parents if parent.name.startswith("run_launch")), path.parent)),
        "entry_reason": text(report.get("trigger_mode") or "trigger_B"),
        "entry_mode": text(report.get("entry_mode_selected")),
        "selected_mint": text(report.get("selected_mint") or report.get("target_mint")),
        "selected_pool": text(report.get("selected_pool") or report.get("target_pool")),
        "target_tx_signature": text(report.get("target_tx_signature") or report.get("target_signature")),
        "acquisition_tx_signature": text(report.get("acquisition_tx_signature")),
        "time_held_ms": time_held_ms,
        "exit_reason": text(report.get("exit_trigger_reason") or report.get("flatten_reason") or report.get("first_failure")),
        "entry_value_lamports": safe_int(report.get("entry_value_lamports")),
        "peak_exit_value_lamports": safe_int(report.get("peak_exit_value_lamports")),
        "estimated_exit_value_lamports": safe_int(report.get("estimated_exit_value_lamports")),
        "max_favorable_excursion_bps": safe_int(report.get("peak_gain_bps")),
        "max_adverse_excursion_bps": max(safe_int(report.get("drop_from_entry_bps")), safe_int(report.get("drawdown_from_peak_bps"))),
        "gross_pnl_lamports": safe_int(report.get("gross_price_pnl_lamports"), net_pnl),
        "buy_tip_lamports": buy_tip,
        "sell_tip_lamports": sell_tip,
        "base_fees_lamports": base_fees,
        "fees_tips_rent_lamports": buy_tip + sell_tip + base_fees + safe_int(report.get("inline_account_rent_lamports")) + safe_int(report.get("pump_account_setup_lamports")),
        "net_pnl_lamports": net_pnl,
        "creator_sell_detected": boolish(report.get("creator_sell_seen") or report.get("creator_sell_detected")),
        "sell_quality": text(report.get("sell_quality")),
        "chaos_score_at_exit": safe_int(report.get("chaos_score")),
        "catastrophe_score_at_exit": safe_int(report.get("catastrophe_score")),
        "would_hold_1h_pnl_lamports": safe_int(report.get("would_hold_1h_pnl_lamports")),
        "would_hold_4h_pnl_lamports": safe_int(report.get("would_hold_4h_pnl_lamports")),
        "would_hold_24h_pnl_lamports": safe_int(report.get("would_hold_24h_pnl_lamports")),
        "final_status": text(report.get("final_status") or report.get("status")),
        "first_failure": text(report.get("first_failure")),
        "wire_send": safe_int(report.get("wire_send")),
        "acquisition_landed": boolish(report.get("acquisition_landed")),
        "flatten_landed": boolish(report.get("flatten_landed")),
        "final_inventory_raw": safe_int(report.get("final_inventory_raw")),
        "max_loss_breached": boolish(report.get("max_loss_breached")),
        "pending_entry_exposure": boolish(report.get("pending_entry_exposure")),
        "entry_final_state": text(report.get("entry_final_state")),
    }


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    attempts = [r for r in rows if safe_int(r.get("wire_send")) > 0 or text(r.get("acquisition_tx_signature"))]
    landed = [r for r in attempts if boolish(r.get("acquisition_landed"))]
    flat = [r for r in landed if boolish(r.get("flatten_landed")) and safe_int(r.get("final_inventory_raw")) <= 0]
    pnls = [safe_int(r.get("net_pnl_lamports")) for r in flat if safe_int(r.get("net_pnl_lamports")) != 0 or text(r.get("final_status"))]
    wins = [p for p in pnls if p > 0]
    losses = [p for p in pnls if p < 0]
    total_win = sum(wins)
    total_loss = abs(sum(losses))
    return {
        "rows_seen": len(rows),
        "attempt_count": len(attempts),
        "landed_count": len(landed),
        "flat_count": len(flat),
        "win_count": len(wins),
        "loss_count": len(losses),
        "win_rate": (len(wins) / len(pnls)) if pnls else 0.0,
        "average_win_lamports": int(statistics.mean(wins)) if wins else 0,
        "average_loss_lamports": int(statistics.mean(losses)) if losses else 0,
        "largest_loss_lamports": min(losses) if losses else 0,
        "median_hold_time_ms": int(statistics.median([safe_int(r.get("time_held_ms")) for r in flat])) if flat else 0,
        "profit_factor": (total_win / total_loss) if total_loss else (999999.0 if total_win else 0.0),
        "expectancy_per_trade_lamports": int(sum(pnls) / len(pnls)) if pnls else 0,
        "expectancy_per_day_lamports": sum(pnls),
        "false_exit_count": sum(1 for r in flat if safe_int(r.get("would_hold_1h_pnl_lamports")) > safe_int(r.get("net_pnl_lamports")) and text(r.get("exit_reason")).startswith("EXIT_")),
        "late_exit_count": sum(1 for r in flat if boolish(r.get("max_loss_breached")) or safe_int(r.get("catastrophe_score_at_exit")) >= 60 and safe_int(r.get("net_pnl_lamports")) < 0),
        "catastrophe_caught_count": sum(1 for r in flat if "CATASTROPHE" in text(r.get("exit_reason")) or "PANIC" in text(r.get("exit_reason")) or "CREATOR" in text(r.get("exit_reason"))),
        "net_pnl_lamports": sum(pnls),
        "win_loss_pnl_bps": bps(total_win, total_loss) if total_loss else 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Build Launch R2-HOLD expectancy ledger.")
    parser.add_argument("--results-root", default=os.environ.get("SHADOW_LAUNCH16_EXPECTANCY_ROOT") or os.environ.get("SHADOW_RESULTS_DIR") or "g1-host-pack/results")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()
    root = Path(args.results_root)
    out_dir = Path(args.out_dir) if args.out_dir else root / "launch_r2_expectancy_ledger"
    reports = find_reports(root)
    rows = [row_from_report(path, load_json(path)) for path in reports]
    summary = summarize(rows)
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "22_launch16_r2_expectancy_ledger_rows.jsonl").write_text("\n".join(json.dumps(row, sort_keys=True) for row in rows) + ("\n" if rows else ""), encoding="utf-8")
    with (out_dir / "22_launch16_r2_expectancy_ledger_rows.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=sorted(rows[0].keys()) if rows else ["report_path"])
        writer.writeheader()
        writer.writerows(rows)
    (out_dir / "22_launch16_r2_expectancy_ledger_summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "22_launch16_r2_expectancy_ledger_summary.txt").write_text("\n".join(f"{k}={v}" for k, v in summary.items()) + "\n", encoding="utf-8")
    print(f"attempt_count={summary['attempt_count']}")
    print(f"expectancy_per_trade_lamports={summary['expectancy_per_trade_lamports']}")
    print(f"profit_factor={summary['profit_factor']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

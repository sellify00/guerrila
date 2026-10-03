from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from statistics import mean
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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


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
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def append_jsonl(path: Path, row: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, sort_keys=True) + "\n")


def avg(rows: list[dict[str, Any]], key: str) -> float | None:
    values = [safe_float(row.get(key)) for row in rows if row.get(key) not in (None, "")]
    return mean(values) if values else None


def attempt_row(run_root: Path, attempt_dir: Path, label: str) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    recovery = load_json(attempt_dir / "22_launch22_post_flatten_account_close_recovery" / "summary.json")
    pnl_after_rent = safe_int(summary.get("pnl_after_rent_recovery"), safe_int(recovery.get("pnl_after_rent_recovery")))
    row = {
        "label": label,
        "run_root": str(run_root),
        "attempt": attempt_dir.name,
        "selected_mint": text(summary.get("selected_mint")),
        "status": text(summary.get("final_status") or summary.get("status")),
        "acquisition_landed": boolish(summary.get("acquisition_landed")),
        "flatten_landed": boolish(summary.get("flatten_landed")),
        "burn_close_landed": boolish(summary.get("burn_close_landed")),
        "auto_quarantine_invoked": boolish(summary.get("auto_quarantine_invoked")),
        "final_inventory_raw": safe_int(summary.get("final_inventory_raw")),
        "pm_pending_after": safe_int(summary.get("pm_pending_after")),
        "entry_quality_ready": boolish(summary.get("entry_quality_ready")),
        "entry_edge_ready": boolish(summary.get("entry_edge_ready")),
        "entry_quality_decision": text(summary.get("entry_quality_decision")),
        "entry_edge_decision": text(summary.get("entry_edge_decision")),
        "price_change_since_trigger_bps": safe_int(summary.get("price_change_since_trigger_bps"), -1),
        "signal_age_ms": safe_int(summary.get("signal_age_ms"), -1),
        "last_trade_side": text(summary.get("last_trade_side")),
        "net_buy_pressure_2s": safe_int(summary.get("net_buy_pressure_2s")),
        "net_buy_pressure_5s": safe_int(summary.get("net_buy_pressure_5s")),
        "sell_count_2s": safe_int(summary.get("sell_count_2s")),
        "sell_count_5s": safe_int(summary.get("sell_count_5s")),
        "sell_quality": text(summary.get("sell_quality")),
        "post_sell_absorption_score": safe_int(summary.get("post_sell_absorption_score")),
        "peak_executable_exit_gain_bps": safe_int(summary.get("peak_executable_exit_gain_bps"), safe_int(summary.get("peak_gain_bps"))),
        "peak_reference_price_gain_bps": safe_int(summary.get("peak_reference_price_gain_bps")),
        "cost_profit_protector_armed": boolish(summary.get("cost_profit_protector_armed")),
        "cost_profit_arm_gain_bps": safe_int(summary.get("cost_profit_arm_gain_bps")),
        "exit_reason": text(summary.get("exit_reason") or summary.get("flatten_reason")),
        "exit_quote_zero_seen": boolish(summary.get("exit_quote_zero_seen")),
        "exit_fragility_guard_ok": boolish(summary.get("exit_fragility_guard_ok")),
        "exit_fragility_first_blocker": text(summary.get("exit_fragility_first_blocker")),
        "base_reserve_to_position_ratio": safe_float(summary.get("base_reserve_to_position_ratio")),
        "reserve_after_exit_to_position_ratio": safe_float(summary.get("reserve_after_exit_to_position_ratio")),
        "hold_v2_normal_panic_suppressed_count": safe_int(summary.get("hold_v2_normal_panic_suppressed_count")),
        "pnl_lamports": safe_int(summary.get("pnl_lamports")),
        "pnl_after_rent_recovery": pnl_after_rent,
        "rent_recovered_lamports": safe_int(summary.get("rent_recovered_lamports"), safe_int(recovery.get("rent_recovered_lamports"))),
    }
    row["outcome_bucket"] = "positive_after_rent" if pnl_after_rent > 0 else "negative_after_rent" if pnl_after_rent < 0 else "flat_or_unknown"
    return row


def collect(run_root: Path, label: str) -> list[dict[str, Any]]:
    rows = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        if (attempt_dir / "launch5_one_mint_live_execute_summary.json").exists():
            rows.append(attempt_row(run_root, attempt_dir, label))
    return rows


def label_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    exit_counts = Counter(row["exit_reason"] or "none" for row in rows)
    outcome_counts = Counter(row["outcome_bucket"] for row in rows)
    return {
        "attempt_rows": len(rows),
        "landed": sum(1 for row in rows if boolish(row.get("acquisition_landed"))),
        "flattened": sum(1 for row in rows if boolish(row.get("flatten_landed"))),
        "burn_closed": sum(1 for row in rows if boolish(row.get("burn_close_landed"))),
        "net_after_rent_total": sum(safe_int(row.get("pnl_after_rent_recovery")) for row in rows),
        "avg_peak_executable_exit_gain_bps": avg(rows, "peak_executable_exit_gain_bps"),
        "avg_price_change_since_trigger_bps": avg(rows, "price_change_since_trigger_bps"),
        "avg_signal_age_ms": avg(rows, "signal_age_ms"),
        "cost_protector_armed_count": sum(1 for row in rows if boolish(row.get("cost_profit_protector_armed"))),
        "exit_quote_zero_seen_count": sum(1 for row in rows if boolish(row.get("exit_quote_zero_seen"))),
        "outcome_counts": dict(outcome_counts),
        "exit_reason_counts": dict(exit_counts),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch36 batch outcome diff.")
    parser.add_argument("--batch-a-root", required=True)
    parser.add_argument("--batch-b-root", required=True)
    parser.add_argument("--batch-a-label", default="batch_a")
    parser.add_argument("--batch-b-label", default="batch_b")
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    root_a = Path(args.batch_a_root).resolve()
    root_b = Path(args.batch_b_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else root_b / "22_launch36_batch_outcome_diff"
    rows = collect(root_a, args.batch_a_label) + collect(root_b, args.batch_b_label)
    rows_path = out_dir / "rows.jsonl"
    rows_path.parent.mkdir(parents=True, exist_ok=True)
    rows_path.write_text("", encoding="utf-8")
    for row in rows:
        append_jsonl(rows_path, row)

    rows_a = [row for row in rows if row["label"] == args.batch_a_label]
    rows_b = [row for row in rows if row["label"] == args.batch_b_label]
    summary = {
        "status": "BATCH_OUTCOME_DIFF_READY",
        "batch_a_label": args.batch_a_label,
        "batch_a_root": str(root_a),
        "batch_b_label": args.batch_b_label,
        "batch_b_root": str(root_b),
        "batch_a": label_summary(rows_a),
        "batch_b": label_summary(rows_b),
        "delta_net_after_rent_total": sum(safe_int(row.get("pnl_after_rent_recovery")) for row in rows_b) - sum(safe_int(row.get("pnl_after_rent_recovery")) for row in rows_a),
        "rows_path": str(rows_path),
    }
    write_json(out_dir / "summary.json", summary)
    for key, value in summary.items():
        print(f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

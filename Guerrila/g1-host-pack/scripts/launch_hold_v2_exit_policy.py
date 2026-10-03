from __future__ import annotations

import argparse
import json
from collections import Counter
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


CATASTROPHE_EXITS = {
    "EXIT_CREATOR_SELL",
    "EXIT_TOP_HOLDER_SELL",
    "EXIT_TOP_HOLDER_DUMP",
    "EXIT_EXIT_QUOTE_ZERO",
    "EXIT_LIQUIDITY_COLLAPSE",
    "EXIT_ROUTE_DETERIORATED",
    "EXIT_PRICE_IMPACT_TOO_HIGH",
    "EXIT_CATASTROPHE",
}
NORMAL_VOLATILITY_EXITS = {
    "EXIT_FAST_DROP_1S",
    "EXIT_FAST_DROP_2S",
    "EXIT_FAST_DROP_5S",
    "EXIT_PANIC_DROP_10S",
    "EXIT_PANIC_DROP_60S",
    "EXIT_REALISABLE_PNL_DRAWDOWN",
    "EXIT_TRAILING_STOP_FROM_HIGH",
    "EXIT_HARD_STOP_FROM_ENTRY",
}


def attempt_row(run_root: Path, attempt_dir: Path) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    recovery = load_json(attempt_dir / "22_launch22_post_flatten_account_close_recovery" / "summary.json")
    exit_reason = text(summary.get("exit_reason") or summary.get("flatten_reason"))
    suppressed = safe_int(summary.get("hold_v2_normal_panic_suppressed_count"))
    row = {
        "run_root": str(run_root),
        "attempt": attempt_dir.name,
        "selected_mint": text(summary.get("selected_mint")),
        "selected_pool": text(summary.get("selected_pool")),
        "status": text(summary.get("final_status") or summary.get("status")),
        "acquisition_landed": boolish(summary.get("acquisition_landed")),
        "flatten_landed": boolish(summary.get("flatten_landed")),
        "burn_close_landed": boolish(summary.get("burn_close_landed")),
        "final_inventory_raw": safe_int(summary.get("final_inventory_raw")),
        "pm_pending_after": safe_int(summary.get("pm_pending_after")),
        "exit_reason": exit_reason,
        "exit_class": "catastrophe" if exit_reason in CATASTROPHE_EXITS else "normal_volatility" if exit_reason in NORMAL_VOLATILITY_EXITS else "profit_protector" if exit_reason == "EXIT_REALISABLE_PROFIT_TRAILING" else "other",
        "launch_hold_v2_enabled": boolish(summary.get("launch_hold_v2_enabled")),
        "launch_hold_v2_warmup_seconds": safe_int(summary.get("launch_hold_v2_warmup_seconds")),
        "hold_v2_normal_panic_suppressed_count": suppressed,
        "hold_v2_last_suppressed_exit": text(summary.get("hold_v2_last_suppressed_exit")),
        "hold_v2_effective_hard_stop_bps": safe_int(summary.get("hold_v2_effective_hard_stop_bps")),
        "cost_profit_protector_armed": boolish(summary.get("cost_profit_protector_armed")),
        "cost_profit_arm_gain_bps": safe_int(summary.get("cost_profit_arm_gain_bps")),
        "peak_executable_exit_gain_bps": safe_int(summary.get("peak_executable_exit_gain_bps"), safe_int(summary.get("peak_gain_bps"))),
        "peak_reference_price_gain_bps": safe_int(summary.get("peak_reference_price_gain_bps")),
        "pnl_lamports": safe_int(summary.get("pnl_lamports")),
        "pnl_after_rent_recovery": safe_int(summary.get("pnl_after_rent_recovery"), safe_int(recovery.get("pnl_after_rent_recovery"))),
        "rent_recovered_lamports": safe_int(summary.get("rent_recovered_lamports"), safe_int(recovery.get("rent_recovered_lamports"))),
        "sell_quality": text(summary.get("sell_quality")),
        "chaos_exit_decision": text(summary.get("chaos_exit_decision")),
        "exit_fragility_guard_ok": boolish(summary.get("exit_fragility_guard_ok")),
        "exit_fragility_first_blocker": text(summary.get("exit_fragility_first_blocker")),
    }
    row["hold_v2_policy_observed"] = boolish(row.get("launch_hold_v2_enabled")) or suppressed > 0
    return row


def collect_rows(run_root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for attempt_dir in sorted((run_root / "attempts").glob("attempt_*")):
        if (attempt_dir / "launch5_one_mint_live_execute_summary.json").exists():
            rows.append(attempt_row(run_root, attempt_dir))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch34 HOLD-V2 exit policy audit.")
    parser.add_argument("--run-root", required=True)
    parser.add_argument("--out-dir", default="")
    args = parser.parse_args()

    run_root = Path(args.run_root).resolve()
    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_root / "22_launch34_hold_v2_exit_policy"
    rows = collect_rows(run_root)
    rows_path = out_dir / "rows.jsonl"
    rows_path.parent.mkdir(parents=True, exist_ok=True)
    rows_path.write_text("", encoding="utf-8")
    for row in rows:
        append_jsonl(rows_path, row)

    exit_counts = Counter(row["exit_reason"] or "none" for row in rows)
    class_counts = Counter(row["exit_class"] for row in rows)
    summary = {
        "status": "HOLD_V2_EXIT_POLICY_AUDITED",
        "run_root": str(run_root),
        "attempt_rows": len(rows),
        "hold_v2_policy_observed_rows": sum(1 for row in rows if boolish(row.get("hold_v2_policy_observed"))),
        "panic_suppressed_attempt_rows": sum(1 for row in rows if safe_int(row.get("hold_v2_normal_panic_suppressed_count")) > 0),
        "panic_suppressed_total": sum(safe_int(row.get("hold_v2_normal_panic_suppressed_count")) for row in rows),
        "cost_protector_armed_count": sum(1 for row in rows if boolish(row.get("cost_profit_protector_armed"))),
        "profit_trailing_exit_count": exit_counts.get("EXIT_REALISABLE_PROFIT_TRAILING", 0),
        "normal_volatility_exit_count": class_counts.get("normal_volatility", 0),
        "catastrophe_exit_count": class_counts.get("catastrophe", 0),
        "exit_reason_counts": dict(exit_counts),
        "exit_class_counts": dict(class_counts),
        "rows_path": str(rows_path),
    }
    write_json(out_dir / "summary.json", summary)
    for key, value in summary.items():
        if key.endswith("_counts"):
            print(f"{key}={json.dumps(value, sort_keys=True)}")
        else:
            print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

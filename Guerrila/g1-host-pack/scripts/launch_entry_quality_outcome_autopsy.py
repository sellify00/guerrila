from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from statistics import mean
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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


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


def avg(rows: list[dict[str, Any]], key: str) -> float | None:
    values = [safe_int(row.get(key)) for row in rows if row.get(key) not in (None, "")]
    return mean(values) if values else None


def proposed_edge_ready(
    row: dict[str, Any],
    min_price_change_bps: int,
    max_price_change_bps: int,
    max_signal_age_ms: int,
    min_net_buy_pressure_5s: int,
) -> tuple[bool, str]:
    if not boolish(row.get("entry_quality_ready")):
        return False, "entry_quality_not_ready"
    if not (boolish(row.get("exit_liquidity_guard_ready")) or boolish(row.get("exit_liquidity_guard_ok")) or boolish(row.get("direct_exit_quote_ok"))):
        return False, "exit_liquidity_not_ready"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < min_price_change_bps:
        return False, "price_change_below_edge_floor"
    if safe_int(row.get("net_buy_pressure_5s")) < min_net_buy_pressure_5s:
        return False, "net_buy_pressure_5s_not_positive"
    signal_age_ms = safe_int(row.get("signal_age_ms"), -1)
    if max_signal_age_ms > 0 and signal_age_ms >= 0 and signal_age_ms > max_signal_age_ms:
        return False, "signal_age_above_edge_limit"
    if safe_int(row.get("sell_count_2s")) != 0:
        return False, "sell_count_2s_positive"
    if text(row.get("last_trade_side")) != "buy":
        return False, "last_trade_not_buy"
    if max_price_change_bps > 0 and safe_int(row.get("price_change_since_trigger_bps"), -1) > max_price_change_bps:
        return False, "price_change_overheated"
    return True, "none"


def attempt_row(
    run_root: Path,
    attempt_dir: Path,
    run_label: str,
    min_price_change_bps: int,
    max_price_change_bps: int,
    max_signal_age_ms: int,
    min_net_buy_pressure_5s: int,
) -> dict[str, Any]:
    summary = load_json(attempt_dir / "launch5_one_mint_live_execute_summary.json")
    recovery = load_json(attempt_dir / "22_launch22_post_flatten_account_close_recovery" / "summary.json")
    protector = load_json(attempt_dir / "22_launch23_cost_aware_profit_protector" / "summary.json")
    pnl_after_rent = safe_int(summary.get("pnl_after_rent_recovery"), safe_int(recovery.get("pnl_after_rent_recovery")))
    row: dict[str, Any] = {
        "run_label": run_label,
        "run_root": str(run_root),
        "attempt": attempt_dir.name,
        "status": text(summary.get("final_status") or summary.get("status")),
        "selected_mint": text(summary.get("selected_mint") or summary.get("launch_mint") or summary.get("mint")),
        "selected_pool": text(summary.get("selected_pool") or summary.get("pool")),
        "entry_quality_ready": boolish(summary.get("entry_quality_ready")),
        "entry_quality_decision": text(summary.get("entry_quality_decision")),
        "entry_edge_ready": boolish(summary.get("entry_edge_ready")),
        "entry_edge_decision": text(summary.get("entry_edge_decision")),
        "exit_liquidity_guard_ready": boolish(summary.get("exit_liquidity_guard_ready")),
        "exit_liquidity_guard_ok": boolish(summary.get("exit_liquidity_guard_ok")),
        "direct_exit_quote_ok": boolish(summary.get("direct_exit_quote_ok")),
        "exit_expected_out_raw": safe_int(summary.get("exit_expected_out_raw")),
        "exit_min_out_raw": safe_int(summary.get("exit_min_out_raw")),
        "exit_non_dust": boolish(summary.get("exit_non_dust")),
        "entry_quality_score": safe_int(summary.get("entry_quality_score")),
        "reserve_pressure_score": safe_int(summary.get("reserve_pressure_score")),
        "buyer_quality_score": safe_int(summary.get("buyer_quality_score")),
        "overhang_score": safe_int(summary.get("overhang_score")) or safe_int(summary.get("sellable_overhang_score")),
        "sellable_overhang_score": safe_int(summary.get("sellable_overhang_score")),
        "absorption_score": safe_int(summary.get("absorption_score")),
        "exit_liquidity_score": safe_int(summary.get("exit_liquidity_score")),
        "price_change_since_trigger_bps": safe_int(summary.get("price_change_since_trigger_bps"), -1),
        "net_buy_pressure_2s": safe_int(summary.get("net_buy_pressure_2s")),
        "net_buy_pressure_5s": safe_int(summary.get("net_buy_pressure_5s")),
        "buy_count_5s": safe_int(summary.get("buy_count_5s")),
        "sell_count_5s": safe_int(summary.get("sell_count_5s")),
        "sell_count_2s": safe_int(summary.get("sell_count_2s")),
        "last_trade_side": text(summary.get("last_trade_side")),
        "last_buy_age_ms": safe_int(summary.get("last_buy_age_ms"), -1),
        "last_sell_age_ms": safe_int(summary.get("last_sell_age_ms"), -1),
        "signal_age_ms": safe_int(summary.get("signal_age_ms"), -1),
        "break_even_bps": safe_int(protector.get("break_even_bps"), safe_int(summary.get("break_even_bps"))),
        "arm_gain_bps": safe_int(protector.get("arm_gain_bps"), safe_int(summary.get("arm_gain_bps"))),
        "peak_executable_exit_gain_bps": safe_int(summary.get("peak_executable_exit_gain_bps"), safe_int(summary.get("peak_gain_bps"))),
        "peak_reference_price_gain_bps": safe_int(summary.get("peak_reference_price_gain_bps")),
        "pnl_lamports": safe_int(summary.get("pnl_lamports")),
        "pnl_after_rent_recovery": pnl_after_rent,
        "exit_reason": text(summary.get("exit_reason")),
        "first_failure": text(summary.get("first_failure")) or "none",
        "acquisition_landed": boolish(summary.get("acquisition_landed")),
        "flatten_landed": boolish(summary.get("flatten_landed")),
        "rent_recovered_lamports": safe_int(summary.get("rent_recovered_lamports"), safe_int(recovery.get("rent_recovered_lamports"))),
        "final_inventory_raw": safe_int(summary.get("final_inventory_raw")),
        "pm_pending_after": safe_int(summary.get("pm_pending_after")),
    }
    ready, blocker = proposed_edge_ready(row, min_price_change_bps, max_price_change_bps, max_signal_age_ms, min_net_buy_pressure_5s)
    row["proposed_entry_edge_ready"] = ready
    row["proposed_entry_edge_blocker"] = blocker
    row["outcome_bucket"] = "positive_after_rent" if pnl_after_rent > 0 else "negative_after_rent" if pnl_after_rent < 0 else "flat_or_unknown"
    row["reached_cost_protector_zone"] = safe_int(row.get("peak_executable_exit_gain_bps")) >= safe_int(row.get("arm_gain_bps"))
    return row


def collect_attempts(
    run_root: Path,
    run_label: str,
    min_price_change_bps: int,
    max_price_change_bps: int,
    max_signal_age_ms: int,
    min_net_buy_pressure_5s: int,
) -> list[dict[str, Any]]:
    attempts_root = run_root / "attempts"
    rows = []
    for attempt_dir in sorted(attempts_root.glob("attempt_*")):
        summary_path = attempt_dir / "launch5_one_mint_live_execute_summary.json"
        if summary_path.exists():
            rows.append(attempt_row(run_root, attempt_dir, run_label, min_price_change_bps, max_price_change_bps, max_signal_age_ms, min_net_buy_pressure_5s))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch33 entry-quality outcome autopsy.")
    parser.add_argument("--positive-root", default="")
    parser.add_argument("--negative-root", default="")
    parser.add_argument("--run-root", action="append", default=[])
    parser.add_argument("--out-dir", default="")
    parser.add_argument("--edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--edge-max-signal-age-ms", type=int, default=1000)
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    args = parser.parse_args()

    run_specs: list[tuple[str, Path]] = []
    if args.positive_root:
        run_specs.append(("positive_reference", Path(args.positive_root).resolve()))
    if args.negative_root:
        run_specs.append(("negative_reference", Path(args.negative_root).resolve()))
    for item in args.run_root:
        if "=" in item:
            label, path = item.split("=", 1)
        else:
            path = item
            label = Path(path).name
        run_specs.append((label, Path(path).resolve()))

    if not run_specs:
        raise SystemExit("at least one run root is required")

    out_dir = Path(args.out_dir).resolve() if args.out_dir else run_specs[-1][1] / "22_launch33_entry_quality_outcome_autopsy"
    out_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, Any]] = []
    for label, root in run_specs:
        rows.extend(
            collect_attempts(
                root,
                label,
                args.edge_min_price_change_bps,
                args.edge_max_price_change_bps,
                args.edge_max_signal_age_ms,
                args.edge_min_net_buy_pressure_5s,
            )
        )

    rows_path = out_dir / "rows.jsonl"
    rows_path.write_text("", encoding="utf-8")
    for row in rows:
        append_jsonl(rows_path, row)

    by_bucket: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_run: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        by_bucket[text(row.get("outcome_bucket"))].append(row)
        by_run[text(row.get("run_label"))].append(row)

    summary: dict[str, Any] = {
        "status": "LAUNCH33_ENTRY_QUALITY_OUTCOME_AUTOPSY_COMPLETE",
        "out_dir": str(out_dir),
        "run_roots": {label: str(root) for label, root in run_specs},
        "attempt_rows": len(rows),
        "edge_min_price_change_bps": args.edge_min_price_change_bps,
        "edge_max_price_change_bps": args.edge_max_price_change_bps,
        "edge_max_signal_age_ms": args.edge_max_signal_age_ms,
        "edge_min_net_buy_pressure_5s": args.edge_min_net_buy_pressure_5s,
        "outcome_counts": dict(Counter(text(row.get("outcome_bucket")) for row in rows)),
        "proposed_entry_edge_ready_count": sum(1 for row in rows if boolish(row.get("proposed_entry_edge_ready"))),
        "proposed_entry_edge_blocker_counts": dict(Counter(text(row.get("proposed_entry_edge_blocker")) for row in rows)),
        "by_outcome": {},
        "by_run": {},
    }
    metric_keys = [
        "price_change_since_trigger_bps",
        "net_buy_pressure_2s",
        "net_buy_pressure_5s",
        "buy_count_5s",
        "sell_count_5s",
        "signal_age_ms",
        "break_even_bps",
        "arm_gain_bps",
        "peak_executable_exit_gain_bps",
        "pnl_after_rent_recovery",
    ]
    for bucket, bucket_rows in sorted(by_bucket.items()):
        summary["by_outcome"][bucket] = {
            "count": len(bucket_rows),
            "proposed_entry_edge_ready_count": sum(1 for row in bucket_rows if boolish(row.get("proposed_entry_edge_ready"))),
            "cost_protector_zone_count": sum(1 for row in bucket_rows if boolish(row.get("reached_cost_protector_zone"))),
            "averages": {key: avg(bucket_rows, key) for key in metric_keys},
        }
    for label, run_rows in sorted(by_run.items()):
        summary["by_run"][label] = {
            "count": len(run_rows),
            "positive_after_rent_count": sum(1 for row in run_rows if text(row.get("outcome_bucket")) == "positive_after_rent"),
            "negative_after_rent_count": sum(1 for row in run_rows if text(row.get("outcome_bucket")) == "negative_after_rent"),
            "proposed_entry_edge_ready_count": sum(1 for row in run_rows if boolish(row.get("proposed_entry_edge_ready"))),
            "total_pnl_after_rent_recovery": sum(safe_int(row.get("pnl_after_rent_recovery")) for row in run_rows),
            "averages": {key: avg(run_rows, key) for key in metric_keys},
        }

    write_json(out_dir / "summary.json", summary)
    lines = [
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    ]
    (out_dir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

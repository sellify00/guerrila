#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from statistics import median
from typing import Any


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


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


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            item = json.loads(raw)
        except Exception:
            continue
        if isinstance(item, dict):
            rows.append(item)
    return rows


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_txt(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def first_text(row: dict[str, Any] | None, *keys: str) -> str:
    if not row:
        return ""
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_int(row: dict[str, Any] | None, *keys: str, default: int = 0) -> int:
    if not row:
        return default
    for key in keys:
        value = safe_int(row.get(key), default=None)  # type: ignore[arg-type]
        if value is not None:
            return value
    return default


def first_float(row: dict[str, Any] | None, *keys: str, default: float = 0.0) -> float:
    if not row:
        return default
    for key in keys:
        value = row.get(key)
        if value not in (None, ""):
            return safe_float(value, default)
    return default


def mint_key(row: dict[str, Any] | None) -> str:
    return first_text(row, "mint", "launch_mint", "base_mint", "quote_mint")


def best_by_mint(rows: list[dict[str, Any]], score_keys: tuple[str, ...]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        mint = mint_key(row)
        if not mint:
            continue
        current = out.get(mint)
        if current is None or score(row, score_keys) > score(current, score_keys):
            out[mint] = row
    return out


def score(row: dict[str, Any], score_keys: tuple[str, ...]) -> tuple[int, ...]:
    values: list[int] = []
    for key in score_keys:
        if key.endswith("_bool"):
            values.append(1 if boolish(row.get(key[:-5])) else 0)
        else:
            values.append(safe_int(row.get(key)))
    return tuple(values)


def ns_delta_seconds(start_ns: int, end_ns: int) -> float | None:
    if start_ns <= 0 or end_ns <= 0 or end_ns < start_ns:
        return None
    return round((end_ns - start_ns) / 1_000_000_000.0, 6)


def estimate_net_pnl(markout: dict[str, Any] | None, args: argparse.Namespace) -> int | None:
    if not markout:
        return None
    explicit = first_int(markout, "net_pnl_after_fees_tips_rent", "pnl_after_rent_recovery", default=None)  # type: ignore[arg-type]
    if explicit is not None:
        return explicit
    peak_bps = first_float(markout, "peak_executable_exit_gain_bps", "peak_return_bps", default=-1)
    if peak_bps < 0:
        return None
    gross = int(args.max_spend_lamports * peak_bps / 10_000.0)
    return gross - args.fee_tip_slippage_lamports + args.rent_recovery_lamports


def build_row(
    completion: dict[str, Any],
    migration: dict[str, Any] | None,
    route: dict[str, Any] | None,
    markout: dict[str, Any] | None,
    args: argparse.Namespace,
) -> dict[str, Any]:
    mint = mint_key(completion)
    pre_buy_sig = first_text(completion, "last_buy_signature", "latest_buy_signature", "first_buy_signature")
    pre_buy_time = first_int(completion, "last_buy_seen_ns", "latest_buy_seen_ns", "first_buy_seen_ns")
    migration_sig = first_text(route, "migration_signature", "migration_proof_signature") or first_text(migration, "migration_signature", "signature")
    migration_time = first_int(migration, "migration_seen_ns", "seen_ns", "create_seen_ns", "block_time_ns")
    pool = first_text(route, "pumpswap_pool", "pool") or first_text(migration, "pumpswap_pool", "pool")
    route_ready = bool(route and text(route.get("decision")) == "MIGRATION_PUMPSWAP_ROUTE_READY")
    quote_ready = bool(route and (route_ready or boolish(route.get("quote_state_ready")) or boolish(route.get("pumpswap_quote_state_ready"))))
    exit_ready = bool(route and (route_ready or boolish(route.get("pumpswap_exit_liquidity_guard_ready")) or boolish(route.get("exit_liquidity_guard_ready"))))
    peak_bps = first_float(markout, "peak_executable_exit_gain_bps", "peak_return_bps", default=-1)
    net_pnl = estimate_net_pnl(markout, args)
    migration_seen = bool(migration_sig or pool or (migration and (boolish(migration.get("migration_instruction_seen")) or boolish(migration.get("complete_pump_amm_migration_event_seen")))))
    no_exit_tail = boolish((markout or {}).get("no_exit_tail")) or text((markout or {}).get("exit_reason")) in {"EXIT_EXIT_QUOTE_ZERO", "UNWINDLESS_BURN_RECONCILED"}

    decision = "PRE_MIGRATION_MARKOUT_INPUT_READY"
    blocker = "none"
    if safe_float(completion.get("curve_progress_pct")) < args.min_progress_pct:
        decision, blocker = "PARK_MARKOUT_PROGRESS_BELOW_THRESHOLD", "curve_progress_pct"
    elif not pre_buy_sig:
        decision, blocker = "PARK_MARKOUT_PRE_MIGRATION_BUY_MISSING", "pre_migration_buy_signature"
    elif not migration_seen:
        decision, blocker = "PARK_MARKOUT_MIGRATION_MISSING", "migration_signature"
    elif not pool:
        decision, blocker = "PARK_MARKOUT_PUMPSWAP_POOL_MISSING", "pumpswap_pool"
    elif not quote_ready or not exit_ready:
        decision, blocker = "PARK_MARKOUT_POST_MIGRATION_QUOTE_MISSING", "pumpswap_quote_state_ready"
    elif peak_bps < 0:
        decision, blocker = "PARK_MARKOUT_PEAK_MISSING", "peak_executable_exit_gain_bps"
    elif no_exit_tail:
        decision, blocker = "PARK_MARKOUT_NO_EXIT_TAIL", "no_exit_tail"
    elif net_pnl is not None and net_pnl > 0:
        decision, blocker = "PRE_MIGRATION_MARKOUT_PROFITABLE", "none"
    else:
        decision, blocker = "PARK_MARKOUT_NOT_PROFITABLE", "net_pnl_after_fees_tips_rent"

    return {
        "record_type": "launch61a_pre_migration_markout_input",
        "mint": mint,
        "bonding_curve": first_text(completion, "bonding_curve"),
        "pre_migration_buy_signature": pre_buy_sig,
        "pre_migration_buy_time": pre_buy_time,
        "pre_migration_buy_size": first_int(completion, "last_buy_size", "latest_buy_size", "first_buy_size"),
        "curve_progress_at_buy": safe_float(completion.get("curve_progress_pct")),
        "curve_progress_pct": safe_float(completion.get("curve_progress_pct")),
        "curve_progress_bucket": text(completion.get("curve_progress_bucket")),
        "estimated_sol_to_completion": safe_float(completion.get("estimated_sol_to_completion_sol")),
        "estimated_sol_to_completion_lamports": safe_int(completion.get("estimated_sol_to_completion_lamports")),
        "migration_signature": migration_sig,
        "migration_time": migration_time,
        "time_to_migration_seconds": ns_delta_seconds(pre_buy_time, migration_time),
        "migration_observed": migration_seen,
        "pumpswap_pool": pool,
        "post_migration_exit_quotes": {
            "pumpswap_exit_expected_out_raw": first_int(route, "pumpswap_exit_expected_out_raw", "exit_expected_out_raw"),
            "pumpswap_exit_min_out_raw": first_int(route, "pumpswap_exit_min_out_raw", "exit_min_out_raw"),
            "pumpswap_exit_price_impact_bps": first_int(route, "pumpswap_exit_price_impact_bps", "exit_price_impact_bps"),
            "pumpswap_quote_state_ready": quote_ready,
            "pumpswap_exit_liquidity_guard_ready": exit_ready,
        },
        "post_migration_exit_ready": quote_ready and exit_ready,
        "peak_executable_exit_gain_bps": None if peak_bps < 0 else peak_bps,
        "time_to_peak_after_migration": first_float(markout, "time_to_peak_after_migration_seconds", "time_to_peak_seconds", default=0),
        "time_to_peak_after_migration_seconds": first_float(markout, "time_to_peak_after_migration_seconds", "time_to_peak_seconds", default=0),
        "net_pnl_after_fees_tips_rent": net_pnl,
        "profitable_after_costs": bool(net_pnl is not None and net_pnl > 0 and not no_exit_tail),
        "no_exit_tail": no_exit_tail,
        "exit_reason": first_text(markout, "exit_reason"),
        "markout_input_decision": decision,
        "markout_input_first_blocker": blocker,
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Build no-wire pre-migration markout input rows.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--migration-rows", type=Path, default=None)
    parser.add_argument("--pumpswap-route-rows", type=Path, default=None)
    parser.add_argument("--post-migration-markout-rows", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    parser.add_argument("--max-spend-lamports", type=int, default=10_000_000)
    parser.add_argument("--fee-tip-slippage-lamports", type=int, default=1_500_000)
    parser.add_argument("--rent-recovery-lamports", type=int, default=2_039_280)
    args = parser.parse_args()

    completion_rows = load_jsonl(args.completion_rows)
    migration_rows = load_jsonl(args.migration_rows)
    route_rows = load_jsonl(args.pumpswap_route_rows)
    markout_rows = load_jsonl(args.post_migration_markout_rows)
    migrations = best_by_mint(migration_rows, ("migration_instruction_seen_bool", "complete_pump_amm_migration_event_seen_bool"))
    routes = best_by_mint(route_rows, ("pumpswap_exit_liquidity_guard_ready_bool", "quote_state_ready_bool", "pumpswap_exit_expected_out_raw"))
    markouts = best_by_mint(markout_rows, ("peak_executable_exit_gain_bps", "net_pnl_after_fees_tips_rent"))
    rows = [build_row(row, migrations.get(mint_key(row)), routes.get(mint_key(row)), markouts.get(mint_key(row)), args) for row in completion_rows]
    decision_counts = Counter(text(row.get("markout_input_decision")) for row in rows)
    eligible_rows = [row for row in rows if safe_float(row.get("curve_progress_pct")) >= args.min_progress_pct]
    ready_rows = [row for row in rows if text(row.get("markout_input_decision")) in {"PRE_MIGRATION_MARKOUT_INPUT_READY", "PRE_MIGRATION_MARKOUT_PROFITABLE", "PARK_MARKOUT_NOT_PROFITABLE"}]
    profitable_rows = [row for row in rows if boolish(row.get("profitable_after_costs"))]
    peak_values = [safe_float(row.get("peak_executable_exit_gain_bps")) for row in rows if row.get("peak_executable_exit_gain_bps") is not None]
    if profitable_rows:
        status = "LAUNCH61A_PRE_MIGRATION_MARKOUT_INPUT_PASS"
    elif any(text(row.get("markout_input_decision")) == "PARK_MARKOUT_PEAK_MISSING" for row in eligible_rows):
        status = "PARK_LAUNCH61A_POST_MIGRATION_MARKOUT_MISSING"
    elif any(text(row.get("markout_input_decision")) == "PARK_MARKOUT_PRE_MIGRATION_BUY_MISSING" for row in eligible_rows):
        status = "PARK_LAUNCH61A_PRE_MIGRATION_BUY_MISSING"
    elif any(boolish(row.get("migration_observed")) for row in eligible_rows):
        status = "PARK_LAUNCH61A_MARKOUT_NOT_PROFITABLE_OR_INCOMPLETE"
    else:
        status = "PARK_LAUNCH61A_MIGRATION_INPUT_MISSING"
    summary = {
        "status": status,
        "completion_rows": str(args.completion_rows),
        "migration_rows": str(args.migration_rows) if args.migration_rows else "",
        "pumpswap_route_rows": str(args.pumpswap_route_rows) if args.pumpswap_route_rows else "",
        "post_migration_markout_rows": str(args.post_migration_markout_rows) if args.post_migration_markout_rows else "",
        "completion_rows_seen": len(completion_rows),
        "eligible_completion_rows": len(eligible_rows),
        "migration_rows_seen": len(migration_rows),
        "pumpswap_route_rows_seen": len(route_rows),
        "post_migration_markout_rows_seen": len(markout_rows),
        "markout_input_rows": len(rows),
        "markout_input_ready_rows": len(ready_rows),
        "migration_joined_count": sum(1 for row in rows if boolish(row.get("migration_observed"))),
        "eligible_migration_joined_count": sum(1 for row in eligible_rows if boolish(row.get("migration_observed"))),
        "pumpswap_pool_joined_count": sum(1 for row in rows if text(row.get("pumpswap_pool"))),
        "eligible_pumpswap_pool_joined_count": sum(1 for row in eligible_rows if text(row.get("pumpswap_pool"))),
        "post_migration_exit_ready_count": sum(1 for row in rows if boolish(row.get("post_migration_exit_ready"))),
        "eligible_post_migration_exit_ready_count": sum(1 for row in eligible_rows if boolish(row.get("post_migration_exit_ready"))),
        "peak_markout_ready_count": len(peak_values),
        "profitable_markout_rows": len(profitable_rows),
        "median_peak_executable_exit_gain_bps": median(peak_values) if peak_values else None,
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "ready_rows_path": str(args.out_dir / "ready_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(profitable_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "ready_rows.jsonl", ready_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "completion_rows_seen", "eligible_completion_rows", "eligible_migration_joined_count", "eligible_pumpswap_pool_joined_count", "eligible_post_migration_exit_ready_count", "peak_markout_ready_count", "profitable_markout_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

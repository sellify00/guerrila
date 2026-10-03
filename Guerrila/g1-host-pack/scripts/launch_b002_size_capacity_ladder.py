#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


LAMPORTS_PER_SOL = 1_000_000_000


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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}
    return value if isinstance(value, dict) else {}


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
    lines = []
    for key, value in summary.items():
        if isinstance(value, bool):
            rendered = str(value).lower()
        elif isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def first_present(data: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        value = data.get(key)
        if value not in (None, ""):
            return value
    return default


def merge_attempt_json(attempt_dir: Path) -> dict[str, Any]:
    merged: dict[str, Any] = {}
    for name in [
        "proposal.json",
        "input_manifest.json",
        "launch5_one_mint_live_execute.json",
        "launch5_one_mint_live_execute_summary.json",
    ]:
        path = attempt_dir / name
        if path.exists():
            merged.update(load_json(path))
    return merged


def cpmm_out(base_reserve: int, token_reserve: float, base_in: int) -> int:
    if base_reserve <= 0 or token_reserve <= 0 or base_in <= 0:
        return 0
    return int(token_reserve * base_in / (base_reserve + base_in))


def cpmm_base_out(base_reserve: int, token_reserve: float, token_in: int) -> int:
    if base_reserve <= 0 or token_reserve <= 0 or token_in <= 0:
        return 0
    return int(base_reserve * token_in / (token_reserve + token_in))


def infer_token_reserve_from_buy(base_reserve: int, base_in: int, token_out: int) -> float:
    if base_reserve <= 0 or base_in <= 0 or token_out <= 0:
        return 0.0
    return float(token_out) * float(base_reserve + base_in) / float(base_in)


def infer_token_reserve_from_sell(base_reserve: int, token_in: int, base_out: int) -> float:
    if base_reserve <= 0 or token_in <= 0 or base_out <= 0 or base_out >= base_reserve:
        return 0.0
    return float(token_in) * ((float(base_reserve) / float(base_out)) - 1.0)


def attempt_dirs(run_root: Path) -> list[Path]:
    return sorted((run_root / "attempts").glob("attempt_*"))


def build_attempt_base(attempt_dir: Path) -> dict[str, Any]:
    data = merge_attempt_json(attempt_dir)
    entry_value = safe_int(first_present(data, "entry_value_lamports", default=0))
    entry_expected = safe_int(first_present(data, "entry_expected_out_raw", "presend_entry_expected_out_raw", default=0))
    entry_min = safe_int(first_present(data, "entry_min_out_raw", "presend_entry_min_out_raw", default=entry_expected))
    exit_token_in = safe_int(first_present(data, "presend_exit_quote_input_raw", default=entry_min or entry_expected))
    current_exit_value = safe_int(first_present(data, "estimated_exit_value_lamports", "exit_quote_expected_out_raw", "exit_expected_out_raw", default=0))
    peak_exit_value = safe_int(first_present(data, "peak_exit_value_lamports", default=current_exit_value))
    entry_base_reserve = safe_int(first_present(data, "pool_liquidity_entry_lamports", default=0))
    exit_base_reserve = safe_int(first_present(data, "exit_base_reserve_lamports", "pool_liquidity_current_lamports", default=0))
    entry_token_reserve = infer_token_reserve_from_buy(entry_base_reserve, entry_value, entry_expected)
    exit_token_reserve = infer_token_reserve_from_sell(exit_base_reserve, exit_token_in, current_exit_value)
    fixed_fee = (
        safe_int(first_present(data, "entry_base_fee_lamports", default=5000))
        + safe_int(first_present(data, "exit_base_fee_lamports", default=5000))
        + safe_int(first_present(data, "entry_tip_lamports", default=200000))
        + safe_int(first_present(data, "exit_tip_lamports", default=200000))
    )
    rent = safe_int(first_present(data, "ata_rent_lamports", "inline_account_rent_lamports", default=0))
    return {
        "attempt_name": attempt_dir.name,
        "mint": text(first_present(data, "selected_mint", "target_mint", "launch_mint")),
        "pool": text(first_present(data, "selected_pool", "target_pool", "pool")),
        "route_family": text(first_present(data, "late_follow_buy_route_family", "route_family")),
        "entry_value_lamports": entry_value,
        "entry_expected_out_raw": entry_expected,
        "entry_min_out_raw": entry_min,
        "exit_token_in_raw": exit_token_in,
        "current_exit_value_lamports": current_exit_value,
        "peak_exit_value_lamports": peak_exit_value,
        "entry_base_reserve_lamports": entry_base_reserve,
        "exit_base_reserve_lamports": exit_base_reserve,
        "entry_token_reserve_inferred": entry_token_reserve,
        "exit_token_reserve_inferred": exit_token_reserve,
        "fixed_fee_lamports": fixed_fee,
        "rent_lamports": rent,
        "observed_peak_gain_bps": safe_int(first_present(data, "peak_gain_bps", "latest_peak_gain_bps", default=0)),
        "observed_current_gain_bps": safe_int(first_present(data, "current_gain_bps", default=0)),
        "observed_pnl_after_rent_recovery": safe_int(first_present(data, "pnl_after_rent_recovery", default=0)),
        "flatten_landed": boolish(first_present(data, "flatten_landed", "primary_flatten_landed", default=False)),
        "rent_recovered": safe_int(first_present(data, "rent_recovered_lamports", default=0)) > 0,
        "exit_reason": text(first_present(data, "exit_reason", default="")),
    }


def simulate_size(base: dict[str, Any], size_sol: float, args: argparse.Namespace) -> dict[str, Any]:
    size_lamports = int(round(size_sol * LAMPORTS_PER_SOL))
    entry_base_reserve = safe_int(base.get("entry_base_reserve_lamports"))
    exit_base_reserve = safe_int(base.get("exit_base_reserve_lamports"))
    entry_token_reserve = safe_float(base.get("entry_token_reserve_inferred"))
    exit_token_reserve = safe_float(base.get("exit_token_reserve_inferred"))
    fixed_fee = safe_int(base.get("fixed_fee_lamports"))
    rent = safe_int(base.get("rent_lamports"))

    modeled_token_out = cpmm_out(entry_base_reserve, entry_token_reserve, size_lamports)
    modeled_current_exit = cpmm_base_out(exit_base_reserve, exit_token_reserve, modeled_token_out)
    original_current = max(1, safe_int(base.get("current_exit_value_lamports")))
    peak_ratio = safe_int(base.get("peak_exit_value_lamports")) / original_current if original_current > 0 else 1.0
    modeled_peak_exit = int(modeled_current_exit * peak_ratio)
    modeled_net_after_rent = modeled_current_exit - size_lamports - fixed_fee
    modeled_peak_net_after_rent = modeled_peak_exit - size_lamports - fixed_fee
    entry_price_impact_bps = (size_lamports * 10_000.0 / max(entry_base_reserve, 1)) if entry_base_reserve > 0 else 999_999.0
    exit_price_impact_bps = (modeled_current_exit * 10_000.0 / max(exit_base_reserve, 1)) if exit_base_reserve > 0 else 999_999.0
    reserve_after_exit = max(0, exit_base_reserve - modeled_current_exit)
    base_reserve_to_position_ratio = exit_base_reserve / max(size_lamports, 1)
    reserve_after_exit_to_position_ratio = reserve_after_exit / max(size_lamports, 1)
    current_gain_bps = int(((modeled_current_exit - size_lamports) * 10_000) / max(size_lamports, 1))
    peak_gain_bps = int(((modeled_peak_exit - size_lamports) * 10_000) / max(size_lamports, 1))
    cost_protector_armed = peak_gain_bps >= args.cost_protector_arm_bps
    no_exit_loss_floor = size_lamports + fixed_fee

    blockers: list[str] = []
    if modeled_token_out <= 0:
        blockers.append("ENTRY_QUOTE_ZERO")
    if modeled_current_exit <= 0:
        blockers.append("EXIT_QUOTE_ZERO")
    if entry_price_impact_bps > args.max_entry_price_impact_bps:
        blockers.append("ENTRY_PRICE_IMPACT_TOO_HIGH")
    if exit_price_impact_bps > args.max_exit_price_impact_bps:
        blockers.append("EXIT_PRICE_IMPACT_TOO_HIGH")
    if base_reserve_to_position_ratio < args.min_base_reserve_to_position_ratio:
        blockers.append("BASE_RESERVE_TO_POSITION_TOO_LOW")
    if reserve_after_exit_to_position_ratio < args.min_reserve_after_exit_to_position_ratio:
        blockers.append("RESERVE_AFTER_EXIT_TO_POSITION_TOO_LOW")
    if not cost_protector_armed:
        blockers.append("COST_PROTECTOR_NOT_ARMED")
    if modeled_net_after_rent <= 0:
        blockers.append("CURRENT_EXIT_NET_NEGATIVE")
    if modeled_peak_net_after_rent <= 0:
        blockers.append("PEAK_EXIT_NET_NEGATIVE")

    mechanical_pass = not blockers
    max_loss_lamports = int(round(args.max_loss_sol * LAMPORTS_PER_SOL))
    risk_budget_pass = no_exit_loss_floor <= max_loss_lamports

    return {
        **{k: base[k] for k in ["attempt_name", "mint", "pool", "route_family", "exit_reason"]},
        "size_sol": size_sol,
        "size_lamports": size_lamports,
        "modeled_entry_token_out_raw": modeled_token_out,
        "modeled_current_exit_value_lamports": modeled_current_exit,
        "modeled_peak_exit_value_lamports": modeled_peak_exit,
        "modeled_current_gain_bps": current_gain_bps,
        "modeled_peak_gain_bps": peak_gain_bps,
        "modeled_net_after_rent_lamports": modeled_net_after_rent,
        "modeled_peak_net_after_rent_lamports": modeled_peak_net_after_rent,
        "entry_price_impact_bps": round(entry_price_impact_bps, 3),
        "exit_price_impact_bps": round(exit_price_impact_bps, 3),
        "exit_base_reserve_lamports": exit_base_reserve,
        "reserve_after_exit_lamports": reserve_after_exit,
        "base_reserve_to_position_ratio": round(base_reserve_to_position_ratio, 6),
        "reserve_after_exit_to_position_ratio": round(reserve_after_exit_to_position_ratio, 6),
        "cost_protector_armed": cost_protector_armed,
        "no_exit_loss_floor_lamports": no_exit_loss_floor,
        "max_loss_lamports": max_loss_lamports,
        "mechanical_capacity_pass": mechanical_pass,
        "risk_budget_pass": risk_budget_pass,
        "decision": "B002_SIZE_CAPACITY_PASS" if mechanical_pass else "PARK_B002_SIZE_CAPACITY",
        "first_blocker": blockers[0] if blockers else "none",
        "blockers": blockers,
        "wire_send": 0,
        "live_landed": 0,
    }


def parse_sizes(value: str) -> list[float]:
    out: list[float] = []
    for part in value.split(","):
        part = part.strip()
        if not part:
            continue
        out.append(float(part))
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire Batch002 size-capacity replay ladder.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--sizes-sol", default="0.01,0.025,0.05,0.1,0.25,0.5,1.0")
    parser.add_argument("--max-loss-sol", type=float, default=0.015)
    parser.add_argument("--max-entry-price-impact-bps", type=float, default=1500.0)
    parser.add_argument("--max-exit-price-impact-bps", type=float, default=1500.0)
    parser.add_argument("--min-base-reserve-to-position-ratio", type=float, default=50.0)
    parser.add_argument("--min-reserve-after-exit-to-position-ratio", type=float, default=25.0)
    parser.add_argument("--cost-protector-arm-bps", type=int, default=2000)
    args = parser.parse_args()

    bases = [build_attempt_base(path) for path in attempt_dirs(args.run_root)]
    rows = [simulate_size(base, size, args) for size in parse_sizes(args.sizes_sol) for base in bases]
    pass_rows = [row for row in rows if boolish(row.get("mechanical_capacity_pass"))]
    by_size: dict[str, dict[str, Any]] = {}
    for size in parse_sizes(args.sizes_sol):
        size_rows = [row for row in rows if safe_float(row.get("size_sol")) == size]
        by_size[str(size)] = {
            "attempts": len(size_rows),
            "mechanical_capacity_pass_rows": sum(1 for row in size_rows if boolish(row.get("mechanical_capacity_pass"))),
            "risk_budget_pass_rows": sum(1 for row in size_rows if boolish(row.get("risk_budget_pass"))),
            "total_modeled_net_after_rent_lamports": sum(safe_int(row.get("modeled_net_after_rent_lamports")) for row in size_rows),
            "total_modeled_peak_net_after_rent_lamports": sum(safe_int(row.get("modeled_peak_net_after_rent_lamports")) for row in size_rows),
            "max_entry_price_impact_bps": max([safe_float(row.get("entry_price_impact_bps")) for row in size_rows] or [0.0]),
            "max_exit_price_impact_bps": max([safe_float(row.get("exit_price_impact_bps")) for row in size_rows] or [0.0]),
            "min_base_reserve_to_position_ratio": min([safe_float(row.get("base_reserve_to_position_ratio")) for row in size_rows] or [0.0]),
            "min_reserve_after_exit_to_position_ratio": min([safe_float(row.get("reserve_after_exit_to_position_ratio")) for row in size_rows] or [0.0]),
        }
    decision_counts = Counter(text(row.get("decision")) for row in rows)
    blocker_counts = Counter(text(row.get("first_blocker")) for row in rows if text(row.get("first_blocker")) != "none")
    summary = {
        "status": "B002_SIZE_CAPACITY_LADDER_PASS",
        "run_root": str(args.run_root),
        "attempts_seen": len(bases),
        "sizes_sol": parse_sizes(args.sizes_sol),
        "rows_seen": len(rows),
        "mechanical_capacity_pass_rows": len(pass_rows),
        "largest_full_pass_size_sol": max([safe_float(size) for size, doc in by_size.items() if doc["mechanical_capacity_pass_rows"] == doc["attempts"]] or [0.0]),
        "largest_full_risk_budget_pass_size_sol": max([safe_float(size) for size, doc in by_size.items() if doc["risk_budget_pass_rows"] == doc["attempts"]] or [0.0]),
        "by_size": by_size,
        "decision_counts": dict(decision_counts),
        "blocker_counts": dict(blocker_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "ready_rows_path": str(args.out_dir / "ready_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "ready_rows.jsonl", pass_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    print(f"status={summary['status']}")
    print(f"attempts_seen={summary['attempts_seen']}")
    print(f"largest_full_pass_size_sol={summary['largest_full_pass_size_sol']}")
    print(f"largest_full_risk_budget_pass_size_sol={summary['largest_full_risk_budget_pass_size_sol']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    print("wire_send=0")
    print("live_landed=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

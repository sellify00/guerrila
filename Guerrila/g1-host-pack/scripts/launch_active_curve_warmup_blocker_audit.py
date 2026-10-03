#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from decimal import Decimal
from pathlib import Path
from typing import Any


ACTIVE_ROUTE = "PUMP_BONDING_CURVE"
READY_BLOCKER = "QUOTE_STATE_READY"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
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


def sol_to_lamports(value: str) -> int:
    try:
        return int((Decimal(str(value)) * Decimal(1_000_000_000)).to_integral_value())
    except Exception:
        return 0


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            value = json.loads(raw)
        except Exception:
            continue
        if isinstance(value, dict):
            rows.append(value)
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


def active_curve(row: dict[str, Any]) -> bool:
    return (
        text(row.get("route_family")) == ACTIVE_ROUTE
        and not boolish(row.get("curve_complete"))
        and text(row.get("first_quote_state_blocker")) != "BONDING_CURVE_COMPLETE_OR_MIGRATED"
    )


def true_loss_floor(args: argparse.Namespace) -> int:
    return (
        sol_to_lamports(args.max_spend_sol)
        + args.entry_tip_lamports
        + args.exit_tip_lamports
        + args.entry_base_fee_lamports
        + args.exit_base_fee_lamports
        + args.unrecoverable_inline_account_lamports
        + args.slippage_buffer_lamports
        + args.panic_exit_buffer_lamports
    )


def audit_decision(row: dict[str, Any], args: argparse.Namespace, loss_floor: int) -> tuple[str, str, dict[str, Any]]:
    max_loss_lamports = sol_to_lamports(args.max_loss_sol)
    max_cash_lamports = sol_to_lamports(args.max_cash_at_risk_sol)
    cost_ready = max_loss_lamports >= loss_floor
    cash_ready = max_cash_lamports >= max_loss_lamports and max_cash_lamports >= loss_floor
    pm_ready = args.assume_pm_ready and cash_ready
    account_ready = args.inline_account_recovery_proven and cash_ready
    fields = {
        "configured_max_loss_lamports": max_loss_lamports,
        "configured_max_cash_at_risk_lamports": max_cash_lamports,
        "true_loss_floor_lamports": loss_floor,
        "cost_floor_ready": cost_ready,
        "pm_ready": pm_ready,
        "account_rent_ready": account_ready,
    }
    if not active_curve(row):
        return "PARK_NOT_ACTIVE_BONDING_CURVE", "not_active_bonding_curve", fields
    if text(row.get("first_quote_state_blocker")) != READY_BLOCKER:
        return "ACTIVE_CURVE_AUDIT_QUOTE_NOT_READY", text(row.get("first_quote_state_blocker")) or "quote_not_ready", fields
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "ACTIVE_CURVE_AUDIT_EXIT_LIQUIDITY_FRAGILE", "exit_liquidity_fragile", fields
    if boolish(row.get("creator_sell_seen")) or boolish(row.get("policy_hard_deny")):
        return "ACTIVE_CURVE_AUDIT_POLICY_BLOCKED", "policy_blocked", fields
    if not boolish(row.get("entry_edge_ready")):
        return "ACTIVE_CURVE_AUDIT_ENTRY_EDGE_WEAK", text(row.get("entry_edge_first_blocker")) or "entry_edge_weak", fields
    if not cost_ready:
        return "ACTIVE_CURVE_AUDIT_COST_FLOOR_NOT_READY", "cost_floor_not_ready", fields
    if not account_ready:
        return "ACTIVE_CURVE_AUDIT_ACCOUNT_RENT_NOT_READY", "account_rent_not_ready", fields
    if not pm_ready:
        return "ACTIVE_CURVE_AUDIT_PM_NOT_READY", "pm_not_ready", fields
    if boolish(row.get("active_curve_r3_ready")):
        return "ACTIVE_CURVE_AUDIT_R3_READY", "none", fields
    return "ACTIVE_CURVE_AUDIT_WARMUP_AGE_NOT_READY", text(row.get("warmup_bucket")) or "warmup_age_not_ready", fields


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U2 active-curve warmup blocker audit.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u-rows", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-spend-sol", default="0.01")
    parser.add_argument("--max-loss-sol", default="0.015")
    parser.add_argument("--max-cash-at-risk-sol", default="0.025")
    parser.add_argument("--entry-tip-lamports", type=int, default=200_000)
    parser.add_argument("--exit-tip-lamports", type=int, default=200_000)
    parser.add_argument("--entry-base-fee-lamports", type=int, default=5_000)
    parser.add_argument("--exit-base-fee-lamports", type=int, default=5_000)
    parser.add_argument("--unrecoverable-inline-account-lamports", type=int, default=0)
    parser.add_argument("--slippage-buffer-lamports", type=int, default=1_000_000)
    parser.add_argument("--panic-exit-buffer-lamports", type=int, default=1_000_000)
    parser.add_argument("--inline-account-recovery-proven", default="true")
    parser.add_argument("--assume-pm-ready", default="true")
    args = parser.parse_args()
    args.inline_account_recovery_proven = boolish(args.inline_account_recovery_proven)
    args.assume_pm_ready = boolish(args.assume_pm_ready)
    loss_floor = true_loss_floor(args)

    rows = load_jsonl(args.launch0u_rows)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    warmup_bucket_counts: Counter[str] = Counter()
    edge_ready_rows = 0
    active_rows = 0
    r3_ready_rows = 0

    for row in rows:
        decision, blocker, fields = audit_decision(row, args, loss_floor)
        enriched = dict(row)
        enriched.update(fields)
        enriched.update(
            {
                "record_type": "launch0u2_active_curve_warmup_blocker_audit",
                "active_curve_audit_decision": decision,
                "active_curve_audit_first_blocker": blocker,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        warmup_bucket_counts[text(row.get("warmup_bucket")) or "unknown"] += 1
        if active_curve(row):
            active_rows += 1
            edge_ready_rows += int(boolish(row.get("entry_edge_ready")))
            r3_ready_rows += int(boolish(row.get("active_curve_r3_ready")))

    summary = {
        "status": "LAUNCH0U2_ACTIVE_CURVE_WARMUP_BLOCKER_AUDIT_COMPLETE",
        "run_root": str(args.run_root),
        "launch0u_rows_path": str(args.launch0u_rows),
        "active_bonding_curve_rows": active_rows,
        "entry_edge_ready_rows": edge_ready_rows,
        "active_curve_r3_ready_rows": r3_ready_rows,
        "warmup_age_not_ready_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_WARMUP_AGE_NOT_READY", 0),
        "pm_not_ready_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_PM_NOT_READY", 0),
        "cost_floor_not_ready_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_COST_FLOOR_NOT_READY", 0),
        "account_rent_not_ready_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_ACCOUNT_RENT_NOT_READY", 0),
        "exit_liquidity_fragile_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_EXIT_LIQUIDITY_FRAGILE", 0),
        "policy_blocked_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_POLICY_BLOCKED", 0),
        "entry_edge_weak_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_ENTRY_EDGE_WEAK", 0),
        "quote_not_ready_rows": decision_counts.get("ACTIVE_CURVE_AUDIT_QUOTE_NOT_READY", 0),
        "true_loss_floor_lamports": loss_floor,
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
        "warmup_bucket_counts": dict(warmup_bucket_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": True,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "active_bonding_curve_rows",
        "entry_edge_ready_rows",
        "active_curve_r3_ready_rows",
        "warmup_age_not_ready_rows",
        "entry_edge_weak_rows",
        "quote_not_ready_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

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
READY_IDENTITIES = {"IDENTITY_READY_FROM_IX", "IDENTITY_READY_FROM_PDA"}


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


def row_key(row: dict[str, Any]) -> str:
    for fields in (
        ("launch_state_id",),
        ("mint", "bonding_curve"),
        ("mint", "pool_or_curve"),
        ("mint", "source_signature"),
    ):
        value = "|".join(text(row.get(field)) for field in fields)
        if value.replace("|", ""):
            return value
    return ""


def fragility_index(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for row in rows:
        key = row_key(row)
        if key:
            out[key] = row
    return out


def active_curve(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ACTIVE_ROUTE and not boolish(row.get("curve_complete"))


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


def classify(row: dict[str, Any], fragility: dict[str, Any], args: argparse.Namespace, loss_floor: int) -> tuple[str, str, dict[str, Any]]:
    max_loss = sol_to_lamports(args.max_loss_sol)
    max_cash = sol_to_lamports(args.max_cash_at_risk_sol)
    cost_ready = max_loss >= loss_floor
    cash_ready = max_cash >= max_loss and max_cash >= loss_floor
    pm_ready = args.assume_pm_ready and cash_ready
    fields = {
        "configured_max_loss_lamports": max_loss,
        "configured_max_cash_at_risk_lamports": max_cash,
        "true_loss_floor_lamports": loss_floor,
        "cost_floor_ready": cost_ready,
        "pm_ready": pm_ready,
    }
    if not active_curve(row):
        return "NOT_IDENTITY_READY", "not_active_curve", fields
    if text(row.get("identity_decision")) not in READY_IDENTITIES:
        return "NOT_IDENTITY_READY", text(row.get("identity_decision")) or "identity_not_ready", fields
    if text(row.get("first_quote_state_blocker")) != READY_BLOCKER and not boolish(row.get("quote_state_ready")):
        return "EXIT_LIQUIDITY_BLOCKED", text(row.get("first_quote_state_blocker")) or "quote_not_ready", fields
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "EXIT_LIQUIDITY_BLOCKED", "exit_liquidity_guard_not_ready", fields
    if fragility and not boolish(fragility.get("fragility_absorption_ready")) and text(fragility.get("fragility_absorption_decision")) != "FRAGILITY_ABSORPTION_READY":
        return "FRAGILITY_BLOCKED", text(fragility.get("fragility_absorption_decision")) or "fragility_not_ready", fields
    if not boolish(row.get("entry_edge_ready")):
        return "ENTRY_EDGE_WEAK", text(row.get("entry_edge_first_blocker")) or "entry_edge_not_ready", fields
    if not cost_ready:
        return "COST_FLOOR_BLOCKED", "cost_floor_not_ready", fields
    if not pm_ready:
        return "PM_BLOCKED", "pm_not_ready", fields
    if boolish(row.get("active_curve_r3_ready")):
        return "ACTIVE_CURVE_R3_READY", "none", fields
    return "WARMUP_NOT_READY", text(row.get("warmup_bucket")) or text(row.get("active_curve_r3_first_blocker")) or "warmup_not_ready", fields


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U7 active curve R3-ready blocker split.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u5-rows", type=Path, required=True)
    parser.add_argument("--fragility-rows", type=Path)
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
    parser.add_argument("--assume-pm-ready", default="true")
    args = parser.parse_args()
    args.assume_pm_ready = boolish(args.assume_pm_ready)

    rows = load_jsonl(args.launch0u5_rows)
    frag_by_key = fragility_index(load_jsonl(args.fragility_rows))
    loss_floor = true_loss_floor(args)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    identity_ready_rows = 0
    r3_ready_rows = 0

    for row in rows:
        key = row_key(row)
        frag = frag_by_key.get(key, {})
        decision, blocker, fields = classify(row, frag, args, loss_floor)
        identity_ready_rows += int(text(row.get("identity_decision")) in READY_IDENTITIES and active_curve(row))
        r3_ready_rows += int(decision == "ACTIVE_CURVE_R3_READY")
        enriched = dict(row)
        enriched.update(fields)
        enriched.update(
            {
                "record_type": "launch0u7_active_curve_r3_ready_blocker_split",
                "r3_ready_blocker_decision": decision,
                "r3_ready_first_blocker": blocker,
                "fragility_absorption_decision": text(frag.get("fragility_absorption_decision")),
                "fragility_absorption_ready": boolish(frag.get("fragility_absorption_ready")),
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1

    summary = {
        "status": "LAUNCH0U7_ACTIVE_CURVE_R3_READY_BLOCKER_SPLIT_COMPLETE",
        "run_root": str(args.run_root),
        "launch0u5_rows_path": str(args.launch0u5_rows),
        "fragility_rows_path": str(args.fragility_rows or ""),
        "identity_ready_rows": identity_ready_rows,
        "active_curve_r3_ready_rows": r3_ready_rows,
        "entry_edge_weak_rows": decision_counts.get("ENTRY_EDGE_WEAK", 0),
        "warmup_not_ready_rows": decision_counts.get("WARMUP_NOT_READY", 0),
        "exit_liquidity_blocked_rows": decision_counts.get("EXIT_LIQUIDITY_BLOCKED", 0),
        "cost_floor_blocked_rows": decision_counts.get("COST_FLOOR_BLOCKED", 0),
        "pm_blocked_rows": decision_counts.get("PM_BLOCKED", 0),
        "fragility_blocked_rows": decision_counts.get("FRAGILITY_BLOCKED", 0),
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
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
        "identity_ready_rows",
        "active_curve_r3_ready_rows",
        "entry_edge_weak_rows",
        "warmup_not_ready_rows",
        "exit_liquidity_blocked_rows",
        "cost_floor_blocked_rows",
        "pm_blocked_rows",
        "fragility_blocked_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

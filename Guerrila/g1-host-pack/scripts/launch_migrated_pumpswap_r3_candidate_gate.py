#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from decimal import Decimal
from pathlib import Path
from typing import Any


READY_DECISION = "MIGRATION_PUMPSWAP_ROUTE_READY"
ROUTE_FAMILY = "pumpswap_after_migration"
TOXIC_SELL_LABELS = {
    "CREATOR_SELL",
    "TOXIC_SELL",
    "INSIDER_CLUSTER_SELL",
    "CASCADE_SELL_PRESSURE",
    "LIQUIDITY_EXIT_RISK",
    "EXIT_LIQUIDITY_RISK",
}


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


def sell_quality_ok(row: dict[str, Any]) -> tuple[bool, str]:
    if boolish(row.get("creator_sell_seen")):
        return False, "creator_sell"
    labels = {
        text(row.get("sell_quality")),
        text(row.get("latest_sell_quality")),
        text(row.get("sell_quality_label")),
        text(row.get("last_sell_quality")),
    }
    toxic = sorted(label for label in labels if label in TOXIC_SELL_LABELS)
    if toxic:
        return False, toxic[0]
    if boolish(row.get("toxic_sell_seen")) or boolish(row.get("cascade_sell_pressure_seen")):
        return False, "toxic_or_cascade_sell"
    if safe_int(row.get("sell_count_2s")) > 0 and safe_int(row.get("net_buy_pressure_2s")) <= 0:
        return False, "recent_sell_pressure_not_absorbed"
    return True, "none"


def entry_quality(row: dict[str, Any], args: argparse.Namespace) -> tuple[bool, str]:
    if safe_int(row.get("entry_quality_baseline_expected_out_raw")) <= 0:
        return False, "entry_quality_warmup"
    if text(row.get("last_trade_side")) != "buy":
        return False, "entry_quality_last_trade_not_buy"
    if safe_int(row.get("net_buy_pressure_2s")) <= 0:
        return False, "entry_quality_buy_pressure_2s"
    if safe_int(row.get("sell_count_2s")) > args.max_sell_count_2s:
        return False, "entry_quality_sell_count_2s"
    if safe_int(row.get("panic_drop_2s_bps")) >= max(1, args.panic_drop_2s_bps):
        return False, "entry_quality_panic_drop_2s"
    if safe_int(row.get("price_change_since_trigger_bps"), -1) < args.min_price_change_bps:
        return False, "entry_quality_price_not_up"
    ok, blocker = sell_quality_ok(row)
    if not ok:
        return False, blocker
    return True, "none"


def entry_edge(row: dict[str, Any], args: argparse.Namespace, quality_ready: bool) -> tuple[bool, str]:
    if not quality_ready:
        return False, "entry_edge_quality_not_ready"
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return False, "entry_edge_exit_liquidity"
    if safe_int(row.get("net_buy_pressure_5s")) < args.edge_min_net_buy_pressure_5s:
        return False, "entry_edge_buy_pressure_5s"
    signal_age_ms = safe_int(row.get("signal_age_ms"), -1)
    if args.edge_max_signal_age_ms > 0 and signal_age_ms >= 0 and signal_age_ms > args.edge_max_signal_age_ms:
        return False, "entry_edge_signal_stale"
    price_change = safe_int(row.get("price_change_since_trigger_bps"), -1)
    if price_change < args.edge_min_price_change_bps:
        return False, "entry_edge_price_change_below_cost_floor"
    if args.edge_max_price_change_bps > 0 and price_change > args.edge_max_price_change_bps:
        return False, "entry_edge_price_overheated"
    return True, "none"


def true_loss_floor(args: argparse.Namespace) -> int:
    max_spend = sol_to_lamports(args.max_spend_sol)
    return (
        max_spend
        + args.entry_tip_lamports
        + args.exit_tip_lamports
        + args.entry_base_fee_lamports
        + args.exit_base_fee_lamports
        + args.unrecoverable_inline_account_lamports
        + args.slippage_buffer_lamports
        + args.panic_exit_buffer_lamports
    )


def route_ready(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ROUTE_FAMILY and (
        text(row.get("decision")) == READY_DECISION
        or boolish(row.get("quote_state_ready"))
        or boolish(row.get("pumpswap_exit_liquidity_guard_ready"))
    )


def row_decision(row: dict[str, Any], args: argparse.Namespace, loss_floor: int) -> tuple[str, str, dict[str, Any]]:
    max_loss_lamports = sol_to_lamports(args.max_loss_sol)
    max_cash_at_risk_lamports = sol_to_lamports(args.max_cash_at_risk_sol)
    cost_ready = max_loss_lamports >= loss_floor
    cash_ready = max_cash_at_risk_lamports >= max_loss_lamports and max_cash_at_risk_lamports >= loss_floor
    pm_ready = args.assume_pm_ready and cash_ready
    account_ready = args.inline_account_recovery_proven and cash_ready

    fields: dict[str, Any] = {
        "configured_max_spend_lamports": sol_to_lamports(args.max_spend_sol),
        "configured_max_loss_lamports": max_loss_lamports,
        "configured_max_cash_at_risk_lamports": max_cash_at_risk_lamports,
        "true_loss_floor_lamports": loss_floor,
        "true_loss_floor_ready": cost_ready,
        "pm_ready": pm_ready,
        "account_rent_cash_at_risk_ready": account_ready,
        "hold_v2_allowed": boolish(args.hold_v2_allowed),
    }

    if text(row.get("route_family")) != ROUTE_FAMILY:
        return "PARK_NOT_MIGRATED_PUMPSWAP", "not_migrated_pumpswap", fields
    if text(row.get("decision")) != READY_DECISION and not boolish(row.get("quote_state_ready")):
        return "PARK_MIGRATED_PUMPSWAP_LIQUIDITY", text(row.get("decision")) or "quote_state_not_ready", fields
    if not boolish(row.get("exit_liquidity_guard_ready")):
        return "PARK_MIGRATED_PUMPSWAP_LIQUIDITY", "exit_liquidity_not_ready", fields
    ok, blocker = sell_quality_ok(row)
    fields["sell_quality_ok"] = ok
    fields["sell_quality_blocker"] = blocker
    if not ok:
        return "PARK_MIGRATED_PUMPSWAP_POLICY", blocker, fields
    quality_ready, quality_blocker = entry_quality(row, args)
    fields["entry_quality_ready"] = quality_ready
    fields["entry_quality_first_blocker"] = quality_blocker
    edge_ready, edge_blocker = entry_edge(row, args, quality_ready)
    fields["entry_edge_ready"] = edge_ready
    fields["entry_edge_first_blocker"] = edge_blocker
    fields["launch_hunter_mode"] = boolish(args.hunter_mode)
    if not boolish(args.hunter_mode) and not quality_ready:
        return "PARK_MIGRATED_PUMPSWAP_ENTRY_QUALITY", quality_blocker, fields
    if not boolish(args.hunter_mode) and not edge_ready:
        return "PARK_MIGRATED_PUMPSWAP_ENTRY_EDGE", edge_blocker, fields
    if not fields["hold_v2_allowed"]:
        return "PARK_MIGRATED_PUMPSWAP_POLICY", "hold_v2_not_allowed", fields
    if not cost_ready:
        return "PARK_MIGRATED_PUMPSWAP_COST_FLOOR", "true_loss_floor_above_max_loss", fields
    if not pm_ready:
        return "PARK_MIGRATED_PUMPSWAP_PM", "pm_not_ready_or_cash_at_risk", fields
    if not account_ready:
        return "PARK_MIGRATED_PUMPSWAP_COST_FLOOR", "account_rent_cash_at_risk_not_ready", fields
    if boolish(args.hunter_mode):
        return "LAUNCH0V_MIGRATED_PUMPSWAP_HUNTER_READY", "none", fields
    return "LAUNCH0V_MIGRATED_PUMPSWAP_R3_READY", "none", fields


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0V migrated PumpSwap R3 no-wire candidate gate.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0m-rows", type=Path, required=True)
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
    parser.add_argument("--hold-v2-allowed", default="true")
    parser.add_argument("--hunter-mode", default="false")
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--panic-drop-2s-bps", type=int, default=700)
    parser.add_argument("--edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--edge-max-signal-age-ms", type=int, default=8000)
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    raw_args = parser.parse_args()

    raw_args.inline_account_recovery_proven = boolish(raw_args.inline_account_recovery_proven)
    raw_args.assume_pm_ready = boolish(raw_args.assume_pm_ready)
    raw_args.hold_v2_allowed = boolish(raw_args.hold_v2_allowed)
    loss_floor = true_loss_floor(raw_args)

    rows = load_jsonl(raw_args.launch0m_rows)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    launch0m_rows_seen = len(rows)
    migrated_rows_seen = 0
    quote_ready_rows = 0
    exit_ready_rows = 0
    quality_ready_rows = 0
    edge_ready_rows = 0
    hold_v2_allowed_rows = 0
    sell_quality_ok_rows = 0
    true_loss_floor_ready_rows = 0
    pm_ready_rows = 0
    account_ready_rows = 0
    r3_ready_rows = 0

    for row in rows:
        decision, blocker, fields = row_decision(row, raw_args, loss_floor)
        enriched = dict(row)
        enriched.update(fields)
        enriched.update(
            {
                "record_type": "launch0v_migrated_pumpswap_r3_candidate_gate",
                "migrated_pumpswap_r3_decision": decision,
                "migrated_pumpswap_r3_first_blocker": blocker,
                "migrated_pumpswap_r3_ready": decision in {"LAUNCH0V_MIGRATED_PUMPSWAP_R3_READY", "LAUNCH0V_MIGRATED_PUMPSWAP_HUNTER_READY"},
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        candidate_row = route_ready(row)
        if candidate_row:
            migrated_rows_seen += 1
            quote_ready_rows += 1
            if boolish(row.get("exit_liquidity_guard_ready")) or boolish(row.get("pumpswap_exit_liquidity_guard_ready")):
                exit_ready_rows += 1
            quality_ready_rows += int(boolish(enriched.get("entry_quality_ready")))
            edge_ready_rows += int(boolish(enriched.get("entry_edge_ready")))
            hold_v2_allowed_rows += int(boolish(enriched.get("hold_v2_allowed")))
            sell_quality_ok_rows += int(boolish(enriched.get("sell_quality_ok")))
            true_loss_floor_ready_rows += int(boolish(enriched.get("true_loss_floor_ready")))
            pm_ready_rows += int(boolish(enriched.get("pm_ready")))
            account_ready_rows += int(boolish(enriched.get("account_rent_cash_at_risk_ready")))
            r3_ready_rows += int(boolish(enriched.get("migrated_pumpswap_r3_ready")))

    if r3_ready_rows > 0:
        status = "LAUNCH0V_MIGRATED_PUMPSWAP_HUNTER_READY" if boolish(raw_args.hunter_mode) else "LAUNCH0V_MIGRATED_PUMPSWAP_R3_READY"
    elif migrated_rows_seen == 0:
        status = "PARK_MIGRATED_PUMPSWAP_NO_ROWS"
    elif quote_ready_rows == 0 or exit_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_LIQUIDITY"
    elif edge_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_ENTRY_EDGE"
    elif true_loss_floor_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_COST_FLOOR"
    elif pm_ready_rows == 0:
        status = "PARK_MIGRATED_PUMPSWAP_PM"
    else:
        status = "PARK_MIGRATED_PUMPSWAP_POLICY"

    summary = {
        "status": status,
        "run_root": str(raw_args.run_root),
        "launch0m_rows_path": str(raw_args.launch0m_rows),
        "launch0m_rows_seen": launch0m_rows_seen,
        "migrated_pumpswap_rows_seen": migrated_rows_seen,
        "quote_state_ready_rows": quote_ready_rows,
        "exit_liquidity_guard_ready_rows": exit_ready_rows,
        "entry_quality_ready_rows": quality_ready_rows,
        "entry_edge_ready_rows": edge_ready_rows,
        "hold_v2_allowed_rows": hold_v2_allowed_rows,
        "sell_quality_ok_rows": sell_quality_ok_rows,
        "true_loss_floor_ready_rows": true_loss_floor_ready_rows,
        "pm_ready_rows": pm_ready_rows,
        "account_rent_cash_at_risk_ready_rows": account_ready_rows,
        "r3_ready_rows": r3_ready_rows,
        "launch_hunter_mode": boolish(raw_args.hunter_mode),
        "true_loss_floor_lamports": loss_floor,
        "decision_counts": dict(decision_counts),
        "first_blocker_counts": dict(blocker_counts),
        "rows_path": str(raw_args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": r3_ready_rows > 0,
    }
    write_jsonl(raw_args.out_dir / "rows.jsonl", out_rows)
    write_json(raw_args.out_dir / "summary.json", summary)
    write_txt(raw_args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "acceptance_pass",
        "migrated_pumpswap_rows_seen",
        "quote_state_ready_rows",
        "exit_liquidity_guard_ready_rows",
        "entry_quality_ready_rows",
        "entry_edge_ready_rows",
        "r3_ready_rows",
        "wire_send",
        "live_landed",
    ):
        value = summary[key]
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

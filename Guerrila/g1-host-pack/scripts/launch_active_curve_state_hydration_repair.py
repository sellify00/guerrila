#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from decimal import Decimal
from pathlib import Path
from typing import Any

from solders.pubkey import Pubkey
from spl.token.instructions import get_associated_token_address


PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_BONDING_CURVE = "PUMP_BONDING_CURVE"
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


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def active_curve(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == PUMP_BONDING_CURVE and not boolish(row.get("curve_complete"))


def derive_bonding_curve_pda(mint: str) -> tuple[str, str]:
    if not mint:
        return "", ""
    try:
        mint_pk = Pubkey.from_string(mint)
        program_pk = Pubkey.from_string(PUMP_PROGRAM_ID)
        curve_pk, _bump = Pubkey.find_program_address([b"bonding-curve", bytes(mint_pk)], program_pk)
        associated_pk = get_associated_token_address(curve_pk, mint_pk)
        return str(curve_pk), str(associated_pk)
    except Exception:
        return "", ""


def rpc_result(lane: Any, rpc_url: str, method: str, params: list[Any]) -> tuple[Any, str]:
    response, error = lane.rpc(rpc_url, method, params)
    if error is not None:
        return None, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    return (response or {}).get("result", response), ""


def current_slot(lane: Any, rpc_url: str) -> int:
    result, _error = rpc_result(lane, rpc_url, "getSlot", [{"commitment": "processed"}])
    return safe_int(result)


def hydrate_with_candidate(
    row: dict[str, Any],
    lane: Any,
    hydrator: Any,
    args: argparse.Namespace,
    bonding_curve: str,
    associated_bonding_curve: str,
    curve_source: str,
    current_slot_value: int,
) -> dict[str, Any]:
    out = dict(row)
    state_row = dict(row)
    state_row["bonding_curve"] = bonding_curve
    state_row["pool"] = bonding_curve
    state_row["pool_or_curve"] = bonding_curve
    state_row["associated_bonding_curve"] = associated_bonding_curve
    before_blocker = text(row.get("first_quote_state_blocker"))
    hydrated = hydrator.hydrate_pump_bonding_curve_row(out, state_row, lane, {}, args)
    state_slot = safe_int(hydrated.get("slot"))
    state_age_ms = max(0, (current_slot_value - state_slot) * 400) if current_slot_value > 0 and state_slot > 0 else 0
    owner_ok = text(hydrated.get("bonding_curve_owner")) == PUMP_PROGRAM_ID
    hydrated.update(
        {
            "record_type": "launch0u4_active_curve_state_hydration_repair",
            "bonding_curve_original": text(row.get("bonding_curve")),
            "associated_bonding_curve_original": text(row.get("associated_bonding_curve")),
            "bonding_curve_pda_derived": curve_source == "derived_pda",
            "bonding_curve_account_fetch_ok": text(hydrated.get("first_quote_state_blocker")) != "BONDING_CURVE_STATE_MISSING",
            "bonding_curve_owner_ok": owner_ok,
            "state_slot": state_slot,
            "state_age_ms": state_age_ms,
            "state_hydration_source": curve_source,
            "first_state_blocker": text(hydrated.get("first_quote_state_blocker")),
            "quote_ready_after_hydration": text(hydrated.get("first_quote_state_blocker")) == READY_BLOCKER,
            "quote_state_blocker_before_u4": before_blocker,
            "wire_send": 0,
            "live_landed": 0,
        }
    )
    return hydrated


def entry_quality(row: dict[str, Any], args: argparse.Namespace) -> tuple[bool, str]:
    if not boolish(row.get("quote_state_ready")):
        return False, "entry_quality_quote_not_ready"
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


def choose_best_repair(
    row: dict[str, Any],
    lane: Any,
    hydrator: Any,
    args: argparse.Namespace,
    current_slot_value: int,
) -> dict[str, Any]:
    mint = text(row.get("mint"))
    derived_curve, derived_assoc = derive_bonding_curve_pda(mint)
    candidates: list[tuple[str, str, str]] = []
    original_curve = text(row.get("bonding_curve") or row.get("pool") or row.get("pool_or_curve"))
    original_assoc = text(row.get("associated_bonding_curve"))
    if original_curve:
        candidates.append((original_curve, original_assoc, "original_row"))
    if derived_curve and derived_curve != original_curve:
        candidates.append((derived_curve, derived_assoc, "derived_pda"))
    elif derived_curve and original_curve:
        candidates.append((derived_curve, derived_assoc or original_assoc, "derived_pda"))

    if not candidates:
        out = dict(row)
        out.update(
            {
                "record_type": "launch0u4_active_curve_state_hydration_repair",
                "bonding_curve_pda_derived": False,
                "bonding_curve_account_fetch_ok": False,
                "bonding_curve_owner_ok": False,
                "first_state_blocker": "MINT_OR_CURVE_MISSING",
                "quote_ready_after_hydration": False,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        return out

    repaired_rows = [
        hydrate_with_candidate(row, lane, hydrator, args, curve, assoc, source, current_slot_value)
        for curve, assoc, source in candidates
    ]
    for candidate in repaired_rows:
        if boolish(candidate.get("quote_ready_after_hydration")):
            return candidate
    for candidate in repaired_rows:
        if boolish(candidate.get("bonding_curve_account_fetch_ok")):
            return candidate
    return repaired_rows[0]


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U4 active bonding-curve state hydration repair.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--quote-hydrator-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-spend-sol", default="0.01")
    parser.add_argument("--max-loss-sol", default="0.015")
    parser.add_argument("--slippage-bps", type=int, default=100)
    parser.add_argument("--fee-budget-lamports", type=int, default=410_000)
    parser.add_argument("--exit-safety-bps", type=int, default=10_000)
    parser.add_argument("--min-base-reserve-lamports", type=int, default=25_000_000)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1500)
    parser.add_argument("--min-price-change-bps", type=int, default=100)
    parser.add_argument("--max-sell-count-2s", type=int, default=0)
    parser.add_argument("--panic-drop-2s-bps", type=int, default=700)
    parser.add_argument("--edge-min-price-change-bps", type=int, default=300)
    parser.add_argument("--edge-max-price-change-bps", type=int, default=1500)
    parser.add_argument("--edge-max-signal-age-ms", type=int, default=8000)
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    args = parser.parse_args()
    args.max_loss_lamports = sol_to_lamports(args.max_loss_sol)

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_u4")
    hydrator = load_module(args.quote_hydrator_path, "launch_realtime_quote_state_hydrator_for_u4")
    rows = load_jsonl(args.launch0u_rows)
    current_slot_value = current_slot(lane, args.rpc_url)

    out_rows: list[dict[str, Any]] = []
    before_blockers: Counter[str] = Counter()
    after_blockers: Counter[str] = Counter()
    active_rows = 0
    state_missing_before = 0
    fetch_ok_rows = 0
    owner_ok_rows = 0
    derived_pda_rows = 0
    quote_ready_before = 0
    quote_ready_after = 0
    real_sol_zero_birth_warmup_rows = 0
    state_hydrated_but_quote_blocked_rows = 0
    entry_quality_ready_after = 0
    entry_edge_ready_after = 0

    for row in rows:
        if not active_curve(row):
            out_rows.append(dict(row, record_type="launch0u4_active_curve_state_hydration_repair", wire_send=0, live_landed=0))
            continue
        active_rows += 1
        before = text(row.get("first_quote_state_blocker")) or "unknown"
        before_blockers[before] += 1
        quote_ready_before += int(before == READY_BLOCKER or boolish(row.get("quote_state_ready")))
        state_missing_before += int(before == "BONDING_CURVE_STATE_MISSING")

        if before != "BONDING_CURVE_STATE_MISSING":
            repaired = dict(row)
            repaired.update(
                {
                    "record_type": "launch0u4_active_curve_state_hydration_repair",
                    "bonding_curve_original": text(row.get("bonding_curve")),
                    "associated_bonding_curve_original": text(row.get("associated_bonding_curve")),
                    "bonding_curve_pda_derived": False,
                    "bonding_curve_account_fetch_ok": before == READY_BLOCKER,
                    "bonding_curve_owner_ok": boolish(row.get("bonding_curve_owner_ok")) or before == READY_BLOCKER,
                    "state_hydration_source": "preserved_existing_quote_state",
                    "first_state_blocker": before,
                    "quote_ready_after_hydration": before == READY_BLOCKER or boolish(row.get("quote_state_ready")),
                    "quote_state_blocker_before_u4": before,
                    "wire_send": 0,
                    "live_landed": 0,
                }
            )
        else:
            repaired = choose_best_repair(row, lane, hydrator, args, current_slot_value)
        quality_ready, quality_blocker = entry_quality(repaired, args)
        edge_ready, edge_blocker = entry_edge(repaired, args, quality_ready)
        repaired.update(
            {
                "entry_quality_ready": quality_ready,
                "entry_quality_decision": "ENTRY_QUALITY_READY" if quality_ready else "HOT_BLOCK_ENTRY_QUALITY_REPAIRED",
                "entry_quality_first_blocker": quality_blocker,
                "entry_edge_ready": edge_ready,
                "entry_edge_decision": "ENTRY_EDGE_READY" if edge_ready else "HOT_BLOCK_ENTRY_EDGE_REPAIRED",
                "entry_edge_first_blocker": edge_blocker,
            }
        )
        after = text(repaired.get("first_quote_state_blocker")) or text(repaired.get("first_state_blocker")) or "unknown"
        after_blockers[after] += 1
        fetch_ok_rows += int(boolish(repaired.get("bonding_curve_account_fetch_ok")))
        owner_ok_rows += int(boolish(repaired.get("bonding_curve_owner_ok")))
        derived_pda_rows += int(boolish(repaired.get("bonding_curve_pda_derived")))
        quote_ready_after += int(boolish(repaired.get("quote_ready_after_hydration")) or boolish(repaired.get("quote_state_ready")))
        real_sol_zero_birth_warmup_rows += int(after == "BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP")
        state_hydrated_but_quote_blocked_rows += int(boolish(repaired.get("bonding_curve_account_fetch_ok")) and not boolish(repaired.get("quote_ready_after_hydration")) and not boolish(repaired.get("quote_state_ready")))
        entry_quality_ready_after += int(quality_ready)
        entry_edge_ready_after += int(edge_ready)
        out_rows.append(repaired)

    state_missing_after = after_blockers.get("BONDING_CURVE_STATE_MISSING", 0)
    status = "LAUNCH0U4_ACTIVE_CURVE_STATE_HYDRATION_REPAIR_PASS"
    if state_missing_after >= state_missing_before and quote_ready_after <= quote_ready_before:
        status = "PARK_ACTIVE_CURVE_STATE_HYDRATION_NOT_IMPROVED"
    elif entry_edge_ready_after == 0:
        status = "PARK_ACTIVE_CURVE_STATE_HYDRATED_ENTRY_EDGE_NOT_READY"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "launch0u_rows_path": str(args.launch0u_rows),
        "active_bonding_curve_rows": active_rows,
        "bonding_curve_state_missing_before": state_missing_before,
        "bonding_curve_state_missing_after": state_missing_after,
        "bonding_curve_account_fetch_ok_rows": fetch_ok_rows,
        "bonding_curve_owner_ok_rows": owner_ok_rows,
        "bonding_curve_pda_derived_rows": derived_pda_rows,
        "bonding_curve_real_sol_zero_birth_warmup_rows": real_sol_zero_birth_warmup_rows,
        "state_hydrated_but_quote_blocked_rows": state_hydrated_but_quote_blocked_rows,
        "quote_ready_rows_before": quote_ready_before,
        "quote_ready_rows_after_hydration": quote_ready_after,
        "entry_quality_ready_rows_after_hydration": entry_quality_ready_after,
        "entry_edge_ready_rows_after_hydration": entry_edge_ready_after,
        "before_blocker_counts": dict(before_blockers),
        "after_blocker_counts": dict(after_blockers),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": state_missing_after < state_missing_before or quote_ready_after > quote_ready_before,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "active_bonding_curve_rows",
        "bonding_curve_state_missing_before",
        "bonding_curve_state_missing_after",
        "bonding_curve_account_fetch_ok_rows",
        "bonding_curve_real_sol_zero_birth_warmup_rows",
        "quote_ready_rows_before",
        "quote_ready_rows_after_hydration",
        "entry_edge_ready_rows_after_hydration",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"after_blocker_counts={json.dumps(summary['after_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from pathlib import Path
from typing import Any

from solders.pubkey import Pubkey
from spl.token.instructions import get_associated_token_address


PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
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


def derive_bonding_curve_pda(mint: str) -> tuple[str, str]:
    if not mint:
        return "", ""
    try:
        mint_pk = Pubkey.from_string(mint)
        program_pk = Pubkey.from_string(PUMP_PROGRAM_ID)
        curve_pk, _bump = Pubkey.find_program_address([b"bonding-curve", bytes(mint_pk)], program_pk)
        assoc_pk = get_associated_token_address(curve_pk, mint_pk)
        return str(curve_pk), str(assoc_pk)
    except Exception:
        return "", ""


def active_source(row: dict[str, Any]) -> bool:
    return text(row.get("route_family")) == ACTIVE_ROUTE


def event_ns(row: dict[str, Any]) -> int:
    values = [
        safe_int(row.get("create_seen_ns")),
        safe_int(row.get("first_buy_seen_ns")),
        safe_int(row.get("first_sell_seen_ns")),
        safe_int(row.get("birth_seen_ns")),
    ]
    return max(values)


def choose_source_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_mint: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not active_source(row):
            continue
        mint = text(row.get("mint"))
        if not mint:
            continue
        current = by_mint.get(mint)
        if current is None:
            by_mint[mint] = dict(row)
            continue
        # Prefer create-proven rows, then earliest buy, then newest observed state.
        current_score = (
            10 if text(current.get("source_provenance")) == "CREATE_PROVEN" else 0,
            5 if text(current.get("first_buy_signature")) else 0,
            -event_ns(current),
        )
        next_score = (
            10 if text(row.get("source_provenance")) == "CREATE_PROVEN" else 0,
            5 if text(row.get("first_buy_signature")) else 0,
            -event_ns(row),
        )
        if next_score > current_score:
            by_mint[mint] = dict(row)
    return sorted(by_mint.values(), key=lambda row: text(row.get("mint")))


def entry_quality(row: dict[str, Any], args: argparse.Namespace) -> tuple[bool, str]:
    if not boolish(row.get("quote_state_ready")):
        return False, "entry_quality_quote_not_ready"
    if safe_int(row.get("entry_quality_baseline_expected_out_raw")) <= 0:
        return False, "entry_quality_warmup"
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
    price_change = safe_int(row.get("price_change_since_trigger_bps"), -1)
    if price_change < args.edge_min_price_change_bps:
        return False, "entry_edge_price_change_below_cost_floor"
    if args.edge_max_price_change_bps > 0 and price_change > args.edge_max_price_change_bps:
        return False, "entry_edge_price_overheated"
    return True, "none"


def hydrate_canonical_row(
    source: dict[str, Any],
    lane: Any,
    hydrator: Any,
    args: argparse.Namespace,
) -> dict[str, Any]:
    mint = text(source.get("mint"))
    curve, assoc = derive_bonding_curve_pda(mint)
    out = {
        "record_type": "launch0u8_active_curve_fresh_state_filter",
        "launch_state_id": text(source.get("launch_state_id") or source.get("birth_id")) or mint,
        "birth_id": text(source.get("birth_id") or source.get("launch_state_id")) or mint,
        "source_provenance": text(source.get("source_provenance")) or "TRADE_DERIVED",
        "source_kind": text(source.get("source_kind")) or "UNKNOWN_PUMP_ACTIVITY",
        "pump_account_layout": text(source.get("pump_account_layout")),
        "mint": mint,
        "creator": text(source.get("creator")),
        "route_family": ACTIVE_ROUTE,
        "program_id": PUMP_PROGRAM_ID,
        "bonding_curve": curve,
        "associated_bonding_curve": assoc,
        "pool": curve,
        "pool_or_curve": curve,
        "source_signature": text(source.get("source_signature")),
        "create_signature": text(source.get("create_signature")),
        "create_seen_ns": safe_int(source.get("create_seen_ns")),
        "first_buy_signature": text(source.get("first_buy_signature")),
        "first_buy_seen_ns": safe_int(source.get("first_buy_seen_ns")),
        "first_sell_signature": text(source.get("first_sell_signature")),
        "first_sell_seen_ns": safe_int(source.get("first_sell_seen_ns")),
        "birth_seen_ns": safe_int(source.get("birth_seen_ns") or source.get("create_seen_ns") or source.get("first_buy_seen_ns") or source.get("first_sell_seen_ns")),
        "trigger_b_pass": bool(text(source.get("first_buy_signature"))),
        "last_trade_side": text(source.get("last_trade_side")) or ("buy" if text(source.get("first_buy_signature")) and not text(source.get("first_sell_signature")) else "sell" if text(source.get("first_sell_signature")) else ""),
        "net_buy_pressure_2s": safe_int(source.get("net_buy_pressure_2s"), 1 if text(source.get("first_buy_signature")) else 0),
        "net_buy_pressure_5s": safe_int(source.get("net_buy_pressure_5s"), 1 if text(source.get("first_buy_signature")) else 0),
        "sell_count_2s": safe_int(source.get("sell_count_2s"), 0),
        "sell_count_5s": safe_int(source.get("sell_count_5s"), 1 if text(source.get("first_sell_signature")) else 0),
        "panic_drop_2s_bps": safe_int(source.get("panic_drop_2s_bps"), 0),
        "creator_sell_seen": boolish(source.get("creator_sell_seen")),
        "bonding_curve_pda_derived": True,
        "derived_bonding_curve_pda": curve,
        "derived_associated_bonding_curve_pda": assoc,
        "bonding_curve_identity_source": "canonical_mint_pda",
        "wire_send": 0,
        "live_landed": 0,
    }
    if not mint:
        out["first_quote_state_blocker"] = "QUOTE_MINT_MISSING"
        return out
    if not curve:
        out["first_quote_state_blocker"] = "BONDING_CURVE_PDA_DERIVE_FAILED"
        return out
    hydrated = hydrator.hydrate_pump_bonding_curve_row(out, out, lane, {}, args)
    quality_ready, quality_blocker = entry_quality(hydrated, args)
    edge_ready, edge_blocker = entry_edge(hydrated, args, quality_ready)
    hydrated.update(
        {
            "curve_account_exists": boolish(hydrated.get("bonding_curve_account_fetch_ok")),
            "owner_ok": boolish(hydrated.get("bonding_curve_owner_ok")),
            "complete": boolish(hydrated.get("curve_complete")),
            "entry_quality_ready": quality_ready,
            "entry_quality_decision": "ENTRY_QUALITY_READY" if quality_ready else "HOT_BLOCK_ENTRY_QUALITY_CANONICAL_ACTIVE_CURVE",
            "entry_quality_first_blocker": quality_blocker,
            "entry_edge_ready": edge_ready,
            "entry_edge_decision": "ENTRY_EDGE_READY" if edge_ready else "HOT_BLOCK_ENTRY_EDGE_CANONICAL_ACTIVE_CURVE",
            "entry_edge_first_blocker": edge_blocker,
            "wire_send": 0,
            "live_landed": 0,
        }
    )
    return hydrated


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U8 canonical active Pump bonding-curve fresh state filter.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--input-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--quote-hydrator-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-spend-sol", default="0.01")
    parser.add_argument("--max-loss-lamports", type=int, default=15_000_000)
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
    parser.add_argument("--edge-min-net-buy-pressure-5s", type=int, default=1)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_u8")
    hydrator = load_module(args.quote_hydrator_path, "launch_realtime_quote_state_hydrator_for_u8")
    source_rows = load_jsonl(args.input_rows)
    canonical_sources = choose_source_rows(source_rows)
    out_rows = [hydrate_canonical_row(row, lane, hydrator, args) for row in canonical_sources]

    blocker_counts = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in out_rows)
    layout_counts = Counter(text(row.get("pump_account_layout")) or "unknown" for row in out_rows)
    provenance_counts = Counter(text(row.get("source_provenance")) or "unknown" for row in out_rows)
    active_ready_rows = [row for row in out_rows if boolish(row.get("quote_state_ready"))]
    exit_ready_rows = [row for row in out_rows if boolish(row.get("exit_liquidity_guard_ready"))]
    edge_ready_rows = [row for row in out_rows if boolish(row.get("entry_edge_ready"))]
    complete_rows = [row for row in out_rows if boolish(row.get("complete")) or text(row.get("first_quote_state_blocker")) == "BONDING_CURVE_COMPLETE_OR_MIGRATED"]
    status = "LAUNCH0U8_ACTIVE_CURVE_FRESH_STATE_FILTER_PASS"
    if not out_rows:
        status = "PARK_LAUNCH0U8_NO_ACTIVE_CURVE_MINTS"
    elif not active_ready_rows:
        status = "PARK_LAUNCH0U8_NO_QUOTE_STATE_READY"
    elif not edge_ready_rows:
        status = "PARK_LAUNCH0U8_ENTRY_EDGE_NOT_READY"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "input_rows": str(args.input_rows),
        "source_rows_seen": len(source_rows),
        "canonical_active_curve_mints": len(out_rows),
        "curve_account_exists_rows": sum(1 for row in out_rows if boolish(row.get("curve_account_exists"))),
        "owner_ok_rows": sum(1 for row in out_rows if boolish(row.get("owner_ok"))),
        "complete_false_rows": len(out_rows) - len(complete_rows),
        "complete_or_migrated_rows": len(complete_rows),
        "quote_state_ready_rows": len(active_ready_rows),
        "entry_quote_ok_rows": sum(1 for row in out_rows if boolish(row.get("entry_quote_ok"))),
        "exit_quote_ok_rows": sum(1 for row in out_rows if boolish(row.get("exit_quote_ok"))),
        "exit_liquidity_guard_ready_rows": len(exit_ready_rows),
        "entry_quality_ready_rows": sum(1 for row in out_rows if boolish(row.get("entry_quality_ready"))),
        "entry_edge_ready_rows": len(edge_ready_rows),
        "active_curve_r3_ready_rows": len(edge_ready_rows),
        "bonding_curve_state_missing_rows": blocker_counts.get("BONDING_CURVE_STATE_MISSING", 0),
        "bonding_curve_real_sol_zero_birth_warmup_rows": blocker_counts.get("BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP", 0),
        "quote_blocker_counts": dict(blocker_counts),
        "pump_account_layout_counts": dict(layout_counts),
        "source_provenance_counts": dict(provenance_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(edge_ready_rows),
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "canonical_active_curve_mints",
        "curve_account_exists_rows",
        "complete_false_rows",
        "quote_state_ready_rows",
        "exit_liquidity_guard_ready_rows",
        "entry_edge_ready_rows",
        "active_curve_r3_ready_rows",
        "bonding_curve_state_missing_rows",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"quote_blocker_counts={json.dumps(summary['quote_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

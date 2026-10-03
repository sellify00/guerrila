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


PUMPSWAP_AMM = "PUMPSWAP_AMM"
PUMP_BONDING_CURVE = "PUMP_BONDING_CURVE"
PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_BONDING_CURVE_STATE_LEN = 8 + (8 * 5) + 1


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


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            doc = json.loads(raw)
        except Exception:
            continue
        if isinstance(doc, dict):
            rows.append(doc)
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


def route_pool(row: dict[str, Any]) -> str:
    return text(row.get("pumpswap_pool") or row.get("pool") or row.get("selected_pool") or row.get("pool_or_curve"))


def route_mint(row: dict[str, Any]) -> str:
    return text(row.get("mint") or row.get("launch_mint") or row.get("selected_mint") or row.get("target_mint"))


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


def baseline_key(row: dict[str, Any]) -> str:
    return "|".join(
        [
            text(row.get("launch_state_id") or row.get("birth_id")),
            route_mint(row),
            route_pool(row),
            text(row.get("route_family")),
        ]
    )


def infer_last_trade_side(row: dict[str, Any]) -> str:
    buy_ns = safe_int(row.get("latest_buy_seen_ns") or row.get("last_buy_seen_ns") or row.get("first_buy_seen_ns"))
    sell_ns = safe_int(row.get("latest_sell_seen_ns") or row.get("last_sell_seen_ns") or row.get("first_sell_seen_ns"))
    if buy_ns > 0 and sell_ns > buy_ns:
        return "sell"
    if buy_ns > 0:
        return "buy"
    if sell_ns > 0:
        return "sell"
    return ""


def price_change_bps_from_quote(baseline_expected_out: int, current_expected_out: int) -> int:
    # Fixed SOL-in quotes return fewer launch tokens as price rises.
    if baseline_expected_out <= 0 or current_expected_out <= 0:
        return -1
    return int(round(((baseline_expected_out - current_expected_out) / baseline_expected_out) * 10_000.0))


def impact_bps(exit_expected_out_raw: int, base_reserve_raw: int) -> int:
    if exit_expected_out_raw <= 0 or base_reserve_raw <= 0:
        return 0
    return int(round((exit_expected_out_raw / base_reserve_raw) * 10_000.0))


def decode_pump_bonding_curve_state(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) < PUMP_BONDING_CURVE_STATE_LEN:
        return None, "BONDING_CURVE_STATE_MISSING"
    try:
        virtual_token_reserves = int.from_bytes(raw[8:16], "little")
        virtual_sol_reserves = int.from_bytes(raw[16:24], "little")
        real_token_reserves = int.from_bytes(raw[24:32], "little")
        real_sol_reserves = int.from_bytes(raw[32:40], "little")
        token_total_supply = int.from_bytes(raw[40:48], "little")
        complete = bool(raw[48])
    except Exception:
        return None, "BONDING_CURVE_PARSE_FAILED"
    if virtual_token_reserves <= 0 or virtual_sol_reserves <= 0:
        return None, "BONDING_CURVE_RESERVES_MISSING"
    return {
        "virtual_token_reserves": virtual_token_reserves,
        "virtual_sol_reserves": virtual_sol_reserves,
        "real_token_reserves": real_token_reserves,
        "real_sol_reserves": real_sol_reserves,
        "token_total_supply": token_total_supply,
        "curve_complete": complete,
    }, None


def load_bonding_curve_state(
    lane: Any,
    rpc_url: str,
    bonding_curve: str,
    associated_bonding_curve: str,
) -> tuple[dict[str, Any], str | None]:
    keys = [bonding_curve]
    if associated_bonding_curve and associated_bonding_curve != bonding_curve:
        keys.append(associated_bonding_curve)
    accounts, slot, error = lane.get_multiple_accounts(rpc_url, keys)
    if error:
        return {}, "BONDING_CURVE_STATE_MISSING"
    curve_account = accounts.get(bonding_curve) or {}
    if not curve_account.get("exists"):
        return {}, "BONDING_CURVE_STATE_MISSING"
    decoded, decode_error = decode_pump_bonding_curve_state(curve_account.get("data_raw", b""))
    if decode_error or not decoded:
        return {}, decode_error or "BONDING_CURVE_PARSE_FAILED"
    decoded.update(
        {
            "slot": slot,
            "bonding_curve": bonding_curve,
            "associated_bonding_curve": associated_bonding_curve,
            "associated_bonding_curve_exists": bool((accounts.get(associated_bonding_curve) or {}).get("exists")) if associated_bonding_curve else False,
            "bonding_curve_owner": text(curve_account.get("owner")),
            "bonding_curve_lamports": safe_int(curve_account.get("lamports")),
        }
    )
    return decoded, None


def bonding_curve_quote_ladder(
    lane: Any,
    state: dict[str, Any],
    max_spend_sol: str,
    slippage_bps: int,
    fee_budget_lamports: int,
    max_loss_lamports: int,
    exit_safety_bps: int,
) -> dict[str, Any]:
    spend_lamports = lane.sol_to_lamports(safe_float(max_spend_sol))
    virtual_sol = safe_int(state.get("virtual_sol_reserves"))
    virtual_token = safe_int(state.get("virtual_token_reserves"))
    real_sol = safe_int(state.get("real_sol_reserves"))
    entry_expected = lane.constant_product_out(virtual_sol, virtual_token, spend_lamports)
    entry_min = lane.min_out_from_expected(entry_expected, slippage_bps)
    exit_quote_input = max(0, (entry_min * max(0, min(exit_safety_bps, 10_000))) // 10_000)
    exit_expected = lane.constant_product_out(virtual_token, virtual_sol, exit_quote_input)
    exit_min = lane.min_out_from_expected(exit_expected, slippage_bps)
    immediate_roundtrip_loss = (spend_lamports + fee_budget_lamports) - exit_expected

    if entry_expected <= 0 or entry_min <= 0:
        decision = "BONDING_CURVE_ENTRY_QUOTE_MISSING"
        first_blocker = "ENTRY_QUOTE_MISSING"
    elif exit_expected <= 0:
        decision = "BONDING_CURVE_EXIT_QUOTE_ZERO"
        first_blocker = "BONDING_CURVE_EXIT_QUOTE_ZERO"
    elif exit_min <= 0 or exit_expected <= 1:
        decision = "BONDING_CURVE_EXIT_DUST"
        first_blocker = "EXIT_DUST"
    elif real_sol <= 0:
        decision = "BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP"
        first_blocker = "BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP"
    elif immediate_roundtrip_loss > max_loss_lamports:
        decision = "BONDING_CURVE_SIZE_NEGATIVE_TOO_LARGE"
        first_blocker = "max_loss_required_exceeds_r3"
    else:
        decision = "BONDING_CURVE_QUOTE_STATE_READY"
        first_blocker = "none"

    return {
        "size_sol": safe_float(max_spend_sol),
        "size_lamports": spend_lamports,
        "entry_expected_out_raw": entry_expected,
        "entry_min_out_raw": entry_min,
        "exit_quote_input_raw": exit_quote_input,
        "exit_expected_out_raw": exit_expected,
        "exit_min_out_raw": exit_min,
        "immediate_roundtrip_loss_lamports": immediate_roundtrip_loss,
        "max_loss_required_sol": lane.lamports_to_sol(max(0, immediate_roundtrip_loss)),
        "decision": decision,
        "first_blocker": first_blocker,
    }


def update_baseline(
    out: dict[str, Any],
    baselines: dict[str, Any],
    entry_expected: int,
    entry_min: int,
) -> tuple[int, bool]:
    key = baseline_key(out)
    baseline_doc = baselines.get(key) if isinstance(baselines.get(key), dict) else {}
    baseline_expected = safe_int(baseline_doc.get("entry_expected_out_raw"))
    baseline_warmup = baseline_expected <= 0
    if baseline_warmup and entry_expected > 0:
        baselines[key] = {
            "entry_expected_out_raw": entry_expected,
            "entry_min_out_raw": entry_min,
            "source_signature": text(out.get("source_signature")),
            "launch_state_id": text(out.get("launch_state_id")),
            "mint": text(out.get("mint")),
            "pool": text(out.get("pool")),
            "route_family": text(out.get("route_family")),
        }
    return baseline_expected, baseline_warmup


def first_blocker(row: dict[str, Any], args: argparse.Namespace) -> str:
    existing = text(row.get("first_quote_state_blocker"))
    if existing.startswith("BONDING_CURVE_") and existing != "BONDING_CURVE_QUOTE_STATE_READY":
        return existing
    route_family = text(row.get("route_family"))
    if not route_mint(row):
        return "QUOTE_MINT_MISSING"
    if not route_pool(row):
        return "QUOTE_POOL_MISSING"
    if route_family not in {PUMPSWAP_AMM, PUMP_BONDING_CURVE}:
        return "QUOTE_ROUTE_UNSUPPORTED"
    if not boolish(row.get("entry_quote_ok")):
        return "BONDING_CURVE_RESERVES_MISSING" if route_family == PUMP_BONDING_CURVE else "ENTRY_QUOTE_MISSING"
    if not boolish(row.get("exit_quote_ok")):
        return "BONDING_CURVE_EXIT_QUOTE_ZERO" if route_family == PUMP_BONDING_CURVE else "EXIT_QUOTE_ZERO"
    if not boolish(row.get("exit_non_dust")):
        return "EXIT_DUST"
    if safe_int(row.get("base_reserve_raw")) < args.min_base_reserve_lamports:
        return "BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR" if route_family == PUMP_BONDING_CURVE else "BASE_RESERVE_BELOW_FLOOR"
    if safe_int(row.get("reserve_after_exit_raw")) < args.reserve_after_exit_floor_lamports:
        return "BONDING_CURVE_BASE_RESERVE_BELOW_FLOOR" if route_family == PUMP_BONDING_CURVE else "BASE_RESERVE_BELOW_FLOOR"
    if args.max_exit_price_impact_bps > 0 and safe_int(row.get("exit_price_impact_bps")) > args.max_exit_price_impact_bps:
        return "BONDING_CURVE_PRICE_IMPACT_TOO_HIGH" if route_family == PUMP_BONDING_CURVE else "PRICE_IMPACT_TOO_HIGH"
    return "QUOTE_STATE_READY"


def hydrate_pump_bonding_curve_row(
    out: dict[str, Any],
    state_row: dict[str, Any],
    lane: Any,
    baselines: dict[str, Any],
    args: argparse.Namespace,
) -> dict[str, Any]:
    mint = route_mint(state_row) or route_mint(out)
    bonding_curve = text(state_row.get("bonding_curve") or state_row.get("pool_or_curve") or out.get("pool"))
    associated_bonding_curve = text(state_row.get("associated_bonding_curve"))
    bonding_curve_source = "row"
    derived_curve, derived_associated = ("", "")
    if not bonding_curve and mint:
        derived_curve, derived_associated = derive_bonding_curve_pda(mint)
        bonding_curve = derived_curve
        associated_bonding_curve = associated_bonding_curve or derived_associated
        bonding_curve_source = "derived_pda"
    elif mint:
        derived_curve, derived_associated = derive_bonding_curve_pda(mint)
    out.update(
        {
            "bonding_curve": bonding_curve,
            "associated_bonding_curve": associated_bonding_curve,
            "pumpswap_pool": text(state_row.get("pumpswap_pool")),
            "pool": bonding_curve,
            "pool_or_curve": bonding_curve,
            "bonding_curve_pda_derived": bonding_curve_source == "derived_pda",
            "derived_bonding_curve_pda": derived_curve,
            "derived_associated_bonding_curve_pda": derived_associated,
            "bonding_curve_identity_source": bonding_curve_source,
        }
    )
    if not bonding_curve:
        out["first_quote_state_blocker"] = "BONDING_CURVE_STATE_MISSING"
        out["route_quote_decision"] = "BONDING_CURVE_STATE_MISSING"
        return out

    curve_state, state_error = load_bonding_curve_state(lane, args.rpc_url, bonding_curve, associated_bonding_curve)
    if (state_error or not curve_state) and derived_curve and derived_curve != bonding_curve:
        fallback_state, fallback_error = load_bonding_curve_state(lane, args.rpc_url, derived_curve, derived_associated)
        if fallback_state and not fallback_error:
            bonding_curve = derived_curve
            associated_bonding_curve = derived_associated
            bonding_curve_source = "derived_pda_fallback"
            out.update(
                {
                    "bonding_curve": bonding_curve,
                    "associated_bonding_curve": associated_bonding_curve,
                    "pool": bonding_curve,
                    "pool_or_curve": bonding_curve,
                    "bonding_curve_pda_derived": True,
                    "bonding_curve_identity_source": bonding_curve_source,
                }
            )
            curve_state = fallback_state
            state_error = None
    if state_error or not curve_state:
        out["first_quote_state_blocker"] = state_error or "BONDING_CURVE_STATE_MISSING"
        out["route_quote_decision"] = out["first_quote_state_blocker"]
        out["bonding_curve_account_fetch_ok"] = False
        return out

    out.update(curve_state)
    out["bonding_curve_account_fetch_ok"] = True
    out["bonding_curve_owner_ok"] = text(out.get("bonding_curve_owner")) == PUMP_PROGRAM_ID
    if boolish(curve_state.get("curve_complete")):
        out["first_quote_state_blocker"] = "BONDING_CURVE_COMPLETE_OR_MIGRATED"
        out["route_quote_decision"] = "BONDING_CURVE_COMPLETE_OR_MIGRATED"
        return out

    ladder = bonding_curve_quote_ladder(
        lane,
        curve_state,
        args.max_spend_sol,
        args.slippage_bps,
        args.fee_budget_lamports,
        args.max_loss_lamports,
        args.exit_safety_bps,
    )
    entry_expected = safe_int(ladder.get("entry_expected_out_raw"))
    entry_min = safe_int(ladder.get("entry_min_out_raw"))
    exit_expected = safe_int(ladder.get("exit_expected_out_raw"))
    exit_min = safe_int(ladder.get("exit_min_out_raw"))
    base_reserve = safe_int(curve_state.get("real_sol_reserves"))
    token_reserve = safe_int(curve_state.get("real_token_reserves"))
    reserve_after = max(0, base_reserve - exit_expected)
    baseline_expected, baseline_warmup = update_baseline(out, baselines, entry_expected, entry_min)

    out.update(
        {
            "entry_quote_ok": entry_expected > 0 and entry_min > 0,
            "entry_expected_out_raw": entry_expected,
            "entry_min_out_raw": entry_min,
            "entry_quote_expected_out_raw": entry_expected,
            "entry_quote_min_out_raw": entry_min,
            "exit_quote_ok": exit_expected > 0 and exit_min > 0,
            "exit_expected_out_raw": exit_expected,
            "exit_min_out_raw": exit_min,
            "exit_quote_expected_out_raw": exit_expected,
            "exit_quote_min_out_raw": exit_min,
            "exit_non_dust": exit_expected > 1 and exit_min > 0,
            "base_reserve_raw": base_reserve,
            "token_reserve_raw": token_reserve,
            "reserve_after_exit_raw": reserve_after,
            "exit_base_reserve_lamports": base_reserve,
            "exit_reserve_after_exit_lamports": reserve_after,
            "exit_price_impact_bps": impact_bps(exit_expected, base_reserve),
            "entry_quality_baseline_expected_out_raw": baseline_expected,
            "entry_quality_baseline_warmup": baseline_warmup,
            "price_change_since_trigger_bps": price_change_bps_from_quote(baseline_expected, entry_expected),
            "size_ladder_decision": text(ladder.get("decision")),
            "size_ladder_first_blocker": text(ladder.get("first_blocker")),
            "route_quote_decision": text(ladder.get("decision")),
            "bonding_curve_quote_math": "virtual_reserves_constant_product",
            "bonding_curve_liquidity_safety": "real_sol_reserves",
        }
    )
    if text(ladder.get("first_blocker")).startswith("BONDING_CURVE_"):
        out["first_quote_state_blocker"] = text(ladder.get("first_blocker"))
    else:
        out["first_quote_state_blocker"] = first_blocker(out, args)
    out["exit_liquidity_guard_ready"] = out["first_quote_state_blocker"] == "QUOTE_STATE_READY"
    out["quote_state_ready"] = out["exit_liquidity_guard_ready"]
    if out["quote_state_ready"]:
        out["route_quote_decision"] = "BONDING_CURVE_QUOTE_STATE_READY"
    return out


def hydrated_row(
    state_row: dict[str, Any],
    lane: Any,
    baselines: dict[str, Any],
    args: argparse.Namespace,
) -> dict[str, Any]:
    mint = route_mint(state_row)
    pool = route_pool(state_row)
    route_family = text(state_row.get("route_family"))
    source_provenance = text(state_row.get("source_provenance")) or "UNPROVEN"
    out: dict[str, Any] = {
        "record_type": "launch0q_realtime_quote_state",
        "launch_state_id": text(state_row.get("launch_state_id") or state_row.get("birth_id")),
        "birth_id": text(state_row.get("birth_id") or state_row.get("launch_state_id")),
        "source_provenance": source_provenance,
        "mint": mint,
        "pool": pool,
        "pumpswap_pool": pool if route_family == PUMPSWAP_AMM else "",
        "bonding_curve": text(state_row.get("bonding_curve")) if route_family == PUMP_BONDING_CURVE else "",
        "associated_bonding_curve": text(state_row.get("associated_bonding_curve")),
        "pool_or_curve": text(state_row.get("pool_or_curve") or pool),
        "route_family": route_family,
        "program_id": text(state_row.get("program_id")),
        "source_signature": text(state_row.get("source_signature")),
        "source_kind": text(state_row.get("source_kind")),
        "create_signature": text(state_row.get("create_signature")),
        "create_seen_ns": safe_int(state_row.get("create_seen_ns")),
        "migration_signature": text(state_row.get("migration_signature")),
        "first_buy_signature": text(state_row.get("first_buy_signature")),
        "first_buy_seen_ns": safe_int(state_row.get("first_buy_seen_ns")),
        "first_sell_signature": text(state_row.get("first_sell_signature")),
        "first_sell_seen_ns": safe_int(state_row.get("first_sell_seen_ns")),
        "birth_seen_ns": safe_int(state_row.get("create_seen_ns") or state_row.get("first_buy_seen_ns") or state_row.get("first_sell_seen_ns")),
        "trigger_b_pass": bool(text(state_row.get("first_buy_signature"))),
        "entry_quality_enabled": True,
        "quote_state_ready": False,
        "entry_quote_ok": False,
        "entry_expected_out_raw": 0,
        "entry_min_out_raw": 0,
        "exit_quote_ok": False,
        "exit_expected_out_raw": 0,
        "exit_min_out_raw": 0,
        "exit_non_dust": False,
        "base_reserve_raw": 0,
        "token_reserve_raw": 0,
        "reserve_after_exit_raw": 0,
        "exit_price_impact_bps": 0,
        "exit_liquidity_guard_ready": False,
        "first_quote_state_blocker": "",
        "last_trade_side": infer_last_trade_side(state_row),
        "net_buy_pressure_2s": 1 if text(state_row.get("first_buy_signature")) and infer_last_trade_side(state_row) == "buy" else 0,
        "net_buy_pressure_5s": 1 if text(state_row.get("first_buy_signature")) and infer_last_trade_side(state_row) == "buy" else 0,
        "sell_count_2s": 0,
        "sell_count_5s": 1 if text(state_row.get("first_sell_signature")) and infer_last_trade_side(state_row) == "sell" else 0,
        "panic_drop_2s_bps": 0,
        "creator_sell_seen": False,
        "sell_pressure_seen": infer_last_trade_side(state_row) == "sell",
    }
    if not mint:
        out["first_quote_state_blocker"] = "QUOTE_MINT_MISSING"
        return out
    if not pool and route_family != PUMP_BONDING_CURVE:
        out["first_quote_state_blocker"] = "QUOTE_POOL_MISSING"
        return out
    if route_family == PUMP_BONDING_CURVE:
        return hydrate_pump_bonding_curve_row(out, state_row, lane, baselines, args)
    if route_family != PUMPSWAP_AMM:
        out["first_quote_state_blocker"] = "QUOTE_ROUTE_UNSUPPORTED"
        return out

    state, state_error = lane.load_pumpfun_state(args.rpc_url, pool, mint)
    if state_error or not state:
        out["first_quote_state_blocker"] = "ENTRY_QUOTE_MISSING"
        out["quote_state_error"] = state_error or "pool_state_missing"
        return out

    ladder_rows, _ladder_summary = lane.pumpfun_size_ladder(
        state,
        [args.max_spend_sol],
        args.slippage_bps,
        args.fee_budget_lamports,
        args.max_loss_lamports,
        args.exit_safety_bps,
    )
    ladder = ladder_rows[0] if ladder_rows else {}
    entry_expected = safe_int(ladder.get("entry_expected_out_raw"))
    entry_min = safe_int(ladder.get("entry_min_out_raw"))
    exit_expected = safe_int(ladder.get("exit_expected_out_raw"))
    exit_min = safe_int(ladder.get("exit_min_out_raw"))
    base_reserve = safe_int(state.get("core_reserve_raw"))
    token_reserve = safe_int(state.get("launch_reserve_raw"))
    reserve_after = max(0, base_reserve - exit_expected)

    baseline_expected, baseline_warmup = update_baseline(out, baselines, entry_expected, entry_min)

    out.update(
        {
            "entry_quote_ok": entry_expected > 0 and entry_min > 0,
            "entry_expected_out_raw": entry_expected,
            "entry_min_out_raw": entry_min,
            "entry_quote_expected_out_raw": entry_expected,
            "entry_quote_min_out_raw": entry_min,
            "exit_quote_ok": exit_expected > 0 and exit_min > 0,
            "exit_expected_out_raw": exit_expected,
            "exit_min_out_raw": exit_min,
            "exit_quote_expected_out_raw": exit_expected,
            "exit_quote_min_out_raw": exit_min,
            "exit_non_dust": exit_expected > 1 and exit_min > 0,
            "base_reserve_raw": base_reserve,
            "token_reserve_raw": token_reserve,
            "reserve_after_exit_raw": reserve_after,
            "exit_base_reserve_lamports": base_reserve,
            "exit_reserve_after_exit_lamports": reserve_after,
            "exit_price_impact_bps": impact_bps(exit_expected, base_reserve),
            "entry_quality_baseline_expected_out_raw": baseline_expected,
            "entry_quality_baseline_warmup": baseline_warmup,
            "price_change_since_trigger_bps": price_change_bps_from_quote(baseline_expected, entry_expected),
            "size_ladder_decision": text(ladder.get("decision")),
            "size_ladder_first_blocker": text(ladder.get("first_blocker")),
            "route_quote_decision": "PUMPSWAP_AMM_QUOTE_STATE_READY" if text(ladder.get("decision")) == "SIZE_EXIT_READY" else text(ladder.get("decision")),
        }
    )
    out["first_quote_state_blocker"] = first_blocker(out, args)
    out["exit_liquidity_guard_ready"] = out["first_quote_state_blocker"] == "QUOTE_STATE_READY"
    out["quote_state_ready"] = out["exit_liquidity_guard_ready"]
    return out


def build_summary(args: argparse.Namespace, rows: list[dict[str, Any]]) -> dict[str, Any]:
    blocker_counts = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in rows)
    route_counts = Counter(text(row.get("route_family")) or "unknown" for row in rows)
    provenance_counts = Counter(text(row.get("source_provenance")) or "unknown" for row in rows)
    route_quote_counts = Counter(text(row.get("route_quote_decision")) or "unknown" for row in rows)
    pump_bc_rows = [row for row in rows if text(row.get("route_family")) == PUMP_BONDING_CURVE]
    pump_bc_blockers = Counter(text(row.get("first_quote_state_blocker")) or "unknown" for row in pump_bc_rows)
    pump_bc_fetch_ok_rows = sum(1 for row in pump_bc_rows if boolish(row.get("bonding_curve_account_fetch_ok")))
    pump_bc_derived_pda_rows = sum(1 for row in pump_bc_rows if boolish(row.get("bonding_curve_pda_derived")))
    pump_bc_real_sol_zero_rows = pump_bc_blockers.get("BONDING_CURVE_REAL_SOL_ZERO_BIRTH_WARMUP", 0)
    pump_bc_buy_quote_rows = sum(
        1
        for row in pump_bc_rows
        if text(row.get("source_kind")) == "PUMP_BONDING_CURVE_BUY" and boolish(row.get("entry_quote_ok"))
    )
    pump_bc_sell_quote_rows = sum(
        1
        for row in pump_bc_rows
        if text(row.get("source_kind")) == "PUMP_BONDING_CURVE_SELL" and boolish(row.get("exit_quote_ok"))
    )
    pump_bc_ready_rows = sum(1 for row in pump_bc_rows if boolish(row.get("quote_state_ready")))
    trigger_b_rows = sum(1 for row in rows if boolish(row.get("trigger_b_pass")))
    quote_ready = sum(1 for row in rows if boolish(row.get("quote_state_ready")))
    entry_quote_ok = sum(1 for row in rows if boolish(row.get("entry_quote_ok")))
    exit_quote_ok = sum(1 for row in rows if boolish(row.get("exit_quote_ok")))
    exit_ready = sum(1 for row in rows if boolish(row.get("exit_liquidity_guard_ready")))
    top_blocker = max(blocker_counts.items(), key=lambda item: item[1])[0] if blocker_counts else "none"
    return {
        "status": "launch0q_realtime_launch_quote_state_hydrator_complete",
        "run_root": str(args.run_root),
        "input_rows": str(args.input_rows),
        "unique_launch_states": len(rows),
        "quote_state_evaluated_rows": len(rows),
        "quote_state_ready_rows": quote_ready,
        "trigger_b_rows": trigger_b_rows,
        "entry_quote_ok_rows": entry_quote_ok,
        "exit_quote_ok_rows": exit_quote_ok,
        "exit_liquidity_guard_ready_rows": exit_ready,
        "entry_quality_evaluated_rows": trigger_b_rows,
        "entry_quality_ready_rows": 0,
        "top_quote_state_blocker": top_blocker,
        "quote_state_blocker_counts": dict(blocker_counts),
        "route_family_counts": dict(route_counts),
        "source_provenance_counts": dict(provenance_counts),
        "route_quote_decision_counts": dict(route_quote_counts),
        "pump_bonding_curve_rows": len(pump_bc_rows),
        "pump_bonding_curve_buy_quote_rows": pump_bc_buy_quote_rows,
        "pump_bonding_curve_sell_quote_rows": pump_bc_sell_quote_rows,
        "pump_bonding_curve_ready_rows": pump_bc_ready_rows,
        "pump_bonding_curve_account_fetch_ok_rows": pump_bc_fetch_ok_rows,
        "pump_bonding_curve_derived_pda_rows": pump_bc_derived_pda_rows,
        "pump_bonding_curve_real_sol_zero_birth_warmup_rows": pump_bc_real_sol_zero_rows,
        "pump_bonding_curve_blocker_counts": dict(pump_bc_blockers),
        "min_base_reserve_lamports": args.min_base_reserve_lamports,
        "reserve_after_exit_floor_lamports": args.reserve_after_exit_floor_lamports,
        "max_exit_price_impact_bps": args.max_exit_price_impact_bps,
        "max_spend_sol": args.max_spend_sol,
        "slippage_bps": args.slippage_bps,
        "baseline_cache_path": str(args.baseline_cache),
        "rows_path": str(args.rows_out),
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Hydrate realtime launch states with executable quote/liquidity state.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--input-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--max-spend-sol", default="0.01")
    parser.add_argument("--max-loss-lamports", type=int, default=15_000_000)
    parser.add_argument("--slippage-bps", type=int, default=100)
    parser.add_argument("--fee-budget-lamports", type=int, default=410_000)
    parser.add_argument("--exit-safety-bps", type=int, default=10_000)
    parser.add_argument("--min-base-reserve-lamports", type=int, default=25_000_000)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1500)
    parser.add_argument("--baseline-cache", type=Path, required=True)
    parser.add_argument("--rows-out", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_quote_state")
    states = load_jsonl(args.input_rows)
    baselines = load_json(args.baseline_cache)
    rows = [hydrated_row(row, lane, baselines, args) for row in states]
    write_json(args.baseline_cache, baselines)
    write_jsonl(args.rows_out, rows)
    summary = build_summary(args, rows)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

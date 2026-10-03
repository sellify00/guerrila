#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
METEORA_DAMM_V2_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2tibnN3cH5iWkD7rP8J7u"
CORE_MINTS = {WSOL_MINT, USDC_MINT, USDT_MINT}

PUMPSWAP_BUY_EXACT_QUOTE_IN = bytes([198, 46, 21, 82, 180, 217, 232, 112])
PUMPSWAP_SELL = bytes([51, 230, 133, 164, 1, 127, 131, 173])
PUMPSWAP_CREATE_POOL = bytes([233, 146, 209, 142, 207, 104, 64, 188])

PUMP_BC_CREATE = bytes([24, 30, 200, 40, 5, 28, 7, 119])
PUMP_BC_BUY = bytes([102, 6, 61, 18, 1, 218, 235, 234])
PUMP_BC_SELL = bytes([51, 230, 133, 164, 1, 127, 131, 173])


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


def stable_id(*parts: Any) -> str:
    payload = "|".join(text(part) for part in parts if text(part)).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:32]


def load_json(path: Path | None) -> dict[str, Any]:
    if not path or not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
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
    lines: list[str] = []
    for key, value in summary.items():
        if isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True)
        elif isinstance(value, bool):
            rendered = str(value).lower()
        else:
            rendered = str(value)
        lines.append(f"{key}={rendered}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def core_launch_pair(mint_a: str, mint_b: str) -> tuple[str, str, str]:
    if mint_a in CORE_MINTS and mint_b and mint_b not in CORE_MINTS:
        return "core_launch_pair", mint_a, mint_b
    if mint_b in CORE_MINTS and mint_a and mint_a not in CORE_MINTS:
        return "core_launch_pair", mint_b, mint_a
    if mint_a in CORE_MINTS and mint_b in CORE_MINTS:
        return "core_core_pair", "", ""
    if mint_a and mint_b and mint_a != mint_b:
        return "cross_token_pair", "", ""
    return "unknown_pair", "", ""


def prefix_kind(program_id: str, data: bytes) -> str:
    prefix = data[:8]
    if program_id == PUMPFUN_AMM_PROGRAM_ID:
        if prefix == PUMPSWAP_CREATE_POOL:
            return "create_pool"
        if prefix == PUMPSWAP_BUY_EXACT_QUOTE_IN:
            return "buy"
        if prefix == PUMPSWAP_SELL:
            return "sell"
    if program_id == PUMP_BONDING_CURVE_PROGRAM_ID:
        if prefix == PUMP_BC_CREATE:
            return "create"
        if prefix == PUMP_BC_BUY:
            return "buy"
        if prefix == PUMP_BC_SELL:
            return "sell"
    if program_id == METEORA_DAMM_V2_PROGRAM_ID:
        return "migration_or_pool_activity"
    return "other"


def event_key(event: dict[str, Any]) -> str:
    mint = text(event.get("mint"))
    bonding_curve = text(event.get("bonding_curve"))
    pool = text(event.get("pumpswap_pool"))
    create_signature = text(event.get("create_signature"))
    if mint or bonding_curve or pool:
        return stable_id(mint, bonding_curve, pool)
    if create_signature:
        return stable_id(create_signature)
    return stable_id(event.get("signature"), event.get("instruction_index"))


def classify_view(row: dict[str, Any], parsed: dict[str, Any], view: dict[str, Any], signer: str, utils: Any) -> dict[str, Any]:
    program_id = text(view.get("program_id"))
    accounts = [text(item) for item in (view.get("accounts") or [])]
    data = view.get("data") or b""
    kind = prefix_kind(program_id, data)
    signature = text(row.get("target_signature"))
    seen_ns = safe_int(row.get("target_receive_ns") or row.get("provider_created_at_ns"))
    base = {
        "record_type": "launch0d_realtime_birth_activity",
        "signature": signature,
        "seen_ns": seen_ns,
        "slot": safe_int(row.get("target_slot")),
        "program_id": program_id,
        "instruction_index": safe_int(view.get("instruction_index"), -1),
        "raw_kind": kind,
        "creator": signer,
        "mint": "",
        "bonding_curve": "",
        "associated_bonding_curve": "",
        "pumpswap_pool": "",
        "route_family": "",
        "decode_class": "UNKNOWN_PUMP_ACTIVITY",
        "birth_decode_source": "realtime_preprocessed_tx",
        "decode_confidence": "low",
        "first_blocker": "",
    }

    if program_id == PUMPFUN_AMM_PROGRAM_ID:
        base["route_family"] = "PUMPSWAP_AMM"
        if kind == "create_pool":
            base.update(
                {
                    "decode_class": "PUMPSWAP_POOL_CREATE",
                    "pumpswap_pool": text(accounts[0]) if accounts else "",
                    "decode_confidence": "medium",
                    "first_blocker": "NO_MINT_RESOLVED",
                }
            )
        elif kind == "buy" and len(accounts) >= 23:
            mint_in = text(accounts[4])
            mint_out = text(accounts[3])
            pair_class, _core, launch_mint = core_launch_pair(mint_in, mint_out)
            base.update(
                {
                    "decode_class": "PUMPSWAP_BUY",
                    "pumpswap_pool": text(accounts[0]),
                    "mint": launch_mint,
                    "decode_confidence": "high" if pair_class == "core_launch_pair" else "low",
                    "first_blocker": "" if pair_class == "core_launch_pair" else "NO_MINT_RESOLVED",
                }
            )
        elif kind == "sell" and len(accounts) >= 21:
            mint_in = text(accounts[3])
            mint_out = text(accounts[4])
            pair_class, _core, launch_mint = core_launch_pair(mint_in, mint_out)
            base.update(
                {
                    "decode_class": "PUMPSWAP_SELL",
                    "pumpswap_pool": text(accounts[0]),
                    "mint": launch_mint,
                    "decode_confidence": "high" if pair_class == "core_launch_pair" else "low",
                    "first_blocker": "" if pair_class == "core_launch_pair" else "NO_MINT_RESOLVED",
                }
            )
        else:
            base["first_blocker"] = "NO_POOL_RESOLVED" if kind == "other" else "NO_MINT_RESOLVED"
        return base

    if program_id == PUMP_BONDING_CURVE_PROGRAM_ID:
        base["route_family"] = "PUMP_BONDING_CURVE"
        if kind == "create" and len(accounts) >= 3:
            # create_v2 account order: mint, mint_authority, bonding_curve,
            # associated_bonding_curve, global, user, ...
            create_v2 = len(accounts) >= 16
            base.update(
                {
                    "decode_class": "PUMP_BONDING_CURVE_CREATE",
                    "mint": text(accounts[0]),
                    "bonding_curve": text(accounts[2]) if create_v2 else text(accounts[1]),
                    "associated_bonding_curve": text(accounts[3]) if create_v2 and len(accounts) > 3 else text(accounts[2]),
                    "creator": text(accounts[5]) if create_v2 and len(accounts) > 5 else text(accounts[6]) if len(accounts) > 6 else signer,
                    "decode_confidence": "medium",
                    "pump_account_layout": "create_v2" if create_v2 else "legacy_create",
                }
            )
        elif kind == "buy" and len(accounts) >= 5:
            # buy_v2 account order: global, base_mint, quote_mint, ...,
            # bonding_curve, associated_base_bonding_curve, ..., user, ...
            buy_v2 = len(accounts) >= 27
            base.update(
                {
                    "decode_class": "PUMP_BONDING_CURVE_BUY",
                    "mint": text(accounts[1]) if buy_v2 else text(accounts[2]),
                    "bonding_curve": text(accounts[10]) if buy_v2 and len(accounts) > 10 else text(accounts[3]),
                    "associated_bonding_curve": text(accounts[11]) if buy_v2 and len(accounts) > 11 else text(accounts[4]),
                    "creator": text(accounts[13]) if buy_v2 and len(accounts) > 13 else text(accounts[6]) if len(accounts) > 6 else signer,
                    "decode_confidence": "medium",
                    "pump_account_layout": "buy_v2" if buy_v2 else "legacy_buy",
                }
            )
        elif kind == "sell" and len(accounts) >= 5:
            # sell_v2 account order matches buy_v2 except the global volume
            # accumulator account is absent: base_mint is #2, curve is #11.
            sell_v2 = len(accounts) >= 26
            base.update(
                {
                    "decode_class": "PUMP_BONDING_CURVE_SELL",
                    "mint": text(accounts[1]) if sell_v2 else text(accounts[2]),
                    "bonding_curve": text(accounts[10]) if sell_v2 and len(accounts) > 10 else text(accounts[3]),
                    "associated_bonding_curve": text(accounts[11]) if sell_v2 and len(accounts) > 11 else text(accounts[4]),
                    "creator": text(accounts[13]) if sell_v2 and len(accounts) > 13 else text(accounts[6]) if len(accounts) > 6 else signer,
                    "decode_confidence": "medium",
                    "pump_account_layout": "sell_v2" if sell_v2 else "legacy_sell",
                }
            )
        else:
            base["first_blocker"] = "NO_BONDING_CURVE_RESOLVED" if kind != "other" else "UNSUPPORTED_PROGRAM"
        if not text(base.get("mint")):
            base["first_blocker"] = "NO_MINT_RESOLVED"
        if text(base.get("mint")) and not text(base.get("bonding_curve")):
            base["first_blocker"] = "NO_BONDING_CURVE_RESOLVED"
        return base

    if program_id == METEORA_DAMM_V2_PROGRAM_ID:
        base.update(
            {
                "route_family": "METEORA_DAMM_V2",
                "decode_class": "PUMP_MIGRATION",
                "decode_confidence": "low",
                "first_blocker": "NO_POOL_RESOLVED",
            }
        )
        return base

    base["first_blocker"] = "UNSUPPORTED_PROGRAM"
    return base


def merge_activity(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    # If a create_pool appears in the same tx as a decoded buy/sell, borrow the
    # resolved mint/pool so the birth-state row is useful and dedupable.
    by_signature: dict[str, list[dict[str, Any]]] = {}
    for event in events:
        by_signature.setdefault(text(event.get("signature")), []).append(event)
    for group in by_signature.values():
        resolved = next((event for event in group if text(event.get("mint")) and text(event.get("pumpswap_pool"))), None)
        if not resolved:
            continue
        for event in group:
            if text(event.get("decode_class")) == "PUMPSWAP_POOL_CREATE":
                event["mint"] = text(resolved.get("mint"))
                event["pumpswap_pool"] = text(event.get("pumpswap_pool")) or text(resolved.get("pumpswap_pool"))
                event["first_blocker"] = "" if text(event.get("mint")) else "NO_MINT_RESOLVED"
                event["decode_confidence"] = "high" if text(event.get("mint")) else event.get("decode_confidence")
    return events


def build_birth_rows(events: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], int]:
    states: dict[str, dict[str, Any]] = {}
    duplicate_rows = 0
    seen_activity_ids: set[str] = set()
    for event in events:
        activity_id = stable_id(event.get("signature"), event.get("instruction_index"), event.get("decode_class"))
        if activity_id in seen_activity_ids:
            duplicate_rows += 1
            continue
        seen_activity_ids.add(activity_id)
        if not text(event.get("mint")) and not text(event.get("bonding_curve")) and not text(event.get("pumpswap_pool")):
            continue
        key = event_key(event)
        state = states.get(key)
        if state is None:
            state = {
                "record_type": "launch0d_realtime_launch_state",
                "launch_state_id": key,
                "birth_id": key,
                "mint": text(event.get("mint")),
                "creator": text(event.get("creator")),
                "create_signature": "",
                "create_seen_ns": 0,
                "route_family": text(event.get("route_family")),
                "program_id": text(event.get("program_id")),
                "bonding_curve": text(event.get("bonding_curve")),
                "associated_bonding_curve": text(event.get("associated_bonding_curve")),
                "pumpswap_pool": text(event.get("pumpswap_pool")),
                "migration_signature": "",
                "first_buy_signature": "",
                "first_buy_seen_ns": 0,
                "first_sell_signature": "",
                "first_sell_seen_ns": 0,
                "birth_decode_source": text(event.get("birth_decode_source")),
                "decode_confidence": text(event.get("decode_confidence")) or "low",
                "confidence": text(event.get("decode_confidence")) or "low",
                "source_signature": text(event.get("signature")),
                "source_kind": text(event.get("decode_class")) or "UNKNOWN_PUMP_ACTIVITY",
                "pump_account_layout": text(event.get("pump_account_layout")),
                "source_provenance": "UNPROVEN",
                "pool_or_curve": text(event.get("pumpswap_pool")) or text(event.get("bonding_curve")),
                "first_blocker": text(event.get("first_blocker")),
            }
            states[key] = state
        else:
            for field in ("mint", "creator", "route_family", "program_id", "bonding_curve", "associated_bonding_curve", "pumpswap_pool", "pump_account_layout"):
                if not text(state.get(field)) and text(event.get(field)):
                    state[field] = text(event.get(field))
            state["pool_or_curve"] = text(state.get("pumpswap_pool")) or text(state.get("bonding_curve"))
            if text(event.get("decode_confidence")) == "high":
                state["decode_confidence"] = "high"
                state["confidence"] = "high"
            elif state.get("decode_confidence") == "low" and text(event.get("decode_confidence")) == "medium":
                state["decode_confidence"] = "medium"
                state["confidence"] = "medium"
            if text(state.get("first_blocker")) and not text(event.get("first_blocker")):
                state["first_blocker"] = ""

        decode_class = text(event.get("decode_class"))
        seen_ns = safe_int(event.get("seen_ns"))
        signature = text(event.get("signature"))
        if decode_class in {"PUMPSWAP_POOL_CREATE", "PUMP_BONDING_CURVE_CREATE"}:
            if not text(state.get("create_signature")) or seen_ns < safe_int(state.get("create_seen_ns")):
                state["create_signature"] = signature
                state["create_seen_ns"] = seen_ns
                state["source_signature"] = signature
                state["source_kind"] = decode_class
        elif decode_class in {"PUMPSWAP_BUY", "PUMP_BONDING_CURVE_BUY"}:
            if not text(state.get("first_buy_signature")) or seen_ns < safe_int(state.get("first_buy_seen_ns")):
                state["first_buy_signature"] = signature
                state["first_buy_seen_ns"] = seen_ns
            if not text(state.get("create_signature")) and not text(state.get("source_signature")):
                state["source_signature"] = signature
                state["source_kind"] = decode_class
        elif decode_class in {"PUMPSWAP_SELL", "PUMP_BONDING_CURVE_SELL"}:
            if not text(state.get("first_sell_signature")) or seen_ns < safe_int(state.get("first_sell_seen_ns")):
                state["first_sell_signature"] = signature
                state["first_sell_seen_ns"] = seen_ns
            if not text(state.get("create_signature")) and not text(state.get("source_signature")):
                state["source_signature"] = signature
                state["source_kind"] = decode_class
        elif decode_class == "PUMP_MIGRATION":
            if not text(state.get("migration_signature")):
                state["migration_signature"] = signature
            if not text(state.get("source_signature")):
                state["source_signature"] = signature
                state["source_kind"] = decode_class

    for state in states.values():
        if text(state.get("create_signature")):
            provenance = "CREATE_PROVEN"
        elif text(state.get("migration_signature")):
            provenance = "MIGRATION_DERIVED"
        elif text(state.get("first_buy_signature")) or text(state.get("first_sell_signature")):
            provenance = "TRADE_DERIVED"
        elif text(state.get("pumpswap_pool")):
            provenance = "POOL_DERIVED"
        else:
            provenance = "UNPROVEN"
        state["source_provenance"] = provenance
        state["pool_or_curve"] = text(state.get("pumpswap_pool")) or text(state.get("bonding_curve"))

    return sorted(states.values(), key=lambda row: (safe_int(row.get("create_seen_ns")) or safe_int(row.get("first_buy_seen_ns")), text(row.get("birth_id")))), duplicate_rows


def decode_rows(stream_rows: list[dict[str, Any]], utils: Any) -> tuple[list[dict[str, Any]], list[dict[str, Any]], Counter[str]]:
    events: list[dict[str, Any]] = []
    blocker_counts: Counter[str] = Counter()
    for row in stream_rows:
        tx_base64 = text(row.get("target_tx_base64"))
        if not tx_base64:
            blocker_counts["MISSING_TX_BYTES"] += 1
            continue
        parsed, parse_error = utils.parse_wire_tx(tx_base64)
        if parsed is None:
            blocker_counts["DECODE_FAILED"] += 1
            events.append(
                {
                    "record_type": "launch0d_realtime_birth_activity",
                    "signature": text(row.get("target_signature")),
                    "seen_ns": safe_int(row.get("target_receive_ns") or row.get("provider_created_at_ns")),
                    "decode_class": "UNKNOWN_PUMP_ACTIVITY",
                    "route_family": "",
                    "program_id": "",
                    "first_blocker": parse_error or "DECODE_FAILED",
                }
            )
            continue
        static_keys = list(parsed.get("static_account_keys") or [])
        signer = text(static_keys[0] if static_keys else "")
        views = utils.instruction_views(parsed, static_keys)
        relevant = [
            view
            for view in views
            if text(view.get("program_id")) in {PUMPFUN_AMM_PROGRAM_ID, PUMP_BONDING_CURVE_PROGRAM_ID, METEORA_DAMM_V2_PROGRAM_ID}
        ]
        if not relevant:
            blocker_counts["UNSUPPORTED_PROGRAM"] += 1
            continue
        for view in relevant:
            event = classify_view(row, parsed, view, signer, utils)
            if text(event.get("first_blocker")):
                blocker_counts[text(event.get("first_blocker"))] += 1
            events.append(event)
    merged_events = merge_activity(events)
    birth_rows, duplicate_rows = build_birth_rows(merged_events)
    if duplicate_rows:
        blocker_counts["DUPLICATE_ACTIVITY_ROW"] += duplicate_rows
    return merged_events, birth_rows, blocker_counts


def build_summary(args: argparse.Namespace, stream_rows: list[dict[str, Any]], activity_rows: list[dict[str, Any]], birth_rows: list[dict[str, Any]], blocker_counts: Counter[str]) -> dict[str, Any]:
    decode_class_counts = Counter(text(row.get("decode_class")) for row in activity_rows if text(row.get("decode_class")))
    route_counts = Counter(text(row.get("route_family")) for row in birth_rows if text(row.get("route_family")))
    confidence_counts = Counter(text(row.get("decode_confidence")) for row in birth_rows if text(row.get("decode_confidence")))
    provenance_counts = Counter(text(row.get("source_provenance")) for row in birth_rows if text(row.get("source_provenance")))
    top_blocker = "none"
    if blocker_counts:
        top_blocker = max(blocker_counts.items(), key=lambda item: item[1])[0]
    requested = safe_int(args.requested_duration_seconds)
    estimate_per_hour = float(args.chain_estimate_per_hour)
    chain_estimated = int(round(estimate_per_hour * (requested / 3600.0))) if requested > 0 and estimate_per_hour > 0 else 0
    unique_mints = {text(row.get("mint")) for row in birth_rows if text(row.get("mint"))}
    unique_launch_states = len(birth_rows)
    true_create_signature_births = sum(1 for row in birth_rows if text(row.get("create_signature")))
    first_buy_state_count = sum(1 for row in birth_rows if text(row.get("first_buy_signature")))
    first_sell_state_count = sum(1 for row in birth_rows if text(row.get("first_sell_signature")))
    trade_derived_states = safe_int(provenance_counts.get("TRADE_DERIVED"))
    pool_derived_states = safe_int(provenance_counts.get("POOL_DERIVED"))
    migration_derived_states = safe_int(provenance_counts.get("MIGRATION_DERIVED"))
    unproven_observed_states = safe_int(provenance_counts.get("UNPROVEN"))
    return {
        "status": "launch0d_realtime_birth_decoder_complete",
        "run_root": text(args.run_root),
        "requested_duration_seconds": requested,
        "chain_estimated_launch_count": chain_estimated,
        "chain_estimate_per_hour": estimate_per_hour,
        "chain_estimate_basis": "env_or_operator_default_not_chain_truth" if chain_estimated else "not_configured",
        "activity_rows_seen": len(stream_rows),
        "decoded_activity_rows": len(activity_rows),
        "birth_state_definition": "unique_mint_pool_or_curve_state_from_realtime_activity",
        "launch_state_definition": "unique_mint_pool_or_curve_state_from_realtime_activity",
        "unique_launch_states": unique_launch_states,
        "unique_births_decoded": unique_launch_states,
        "decoded_launch_count": unique_launch_states,
        "global_launches_seen": unique_launch_states,
        "unique_mints_seen": len(unique_mints),
        "true_create_signature_births": true_create_signature_births,
        "trade_derived_states": trade_derived_states,
        "pool_derived_states": pool_derived_states,
        "migration_derived_states": migration_derived_states,
        "unproven_observed_states": unproven_observed_states,
        "observed_state_not_create_proven_count": max(0, unique_launch_states - true_create_signature_births),
        "exact_birth_decode_ready": true_create_signature_births > 0,
        "first_buy_state_count": first_buy_state_count,
        "first_sell_state_count": first_sell_state_count,
        "pump_curve_births": safe_int(route_counts.get("PUMP_BONDING_CURVE")),
        "pumpswap_births": safe_int(route_counts.get("PUMPSWAP_AMM")),
        "migration_births": safe_int(route_counts.get("METEORA_DAMM_V2")),
        "migration_activity_rows": safe_int(decode_class_counts.get("PUMP_MIGRATION")),
        "trigger_b_rows": first_buy_state_count,
        "entry_quality_ready_rows": 0,
        "entry_quality_evaluated_rows": first_buy_state_count,
        "entry_quality_missing_required_field_rows": first_buy_state_count,
        "entry_quality_blocker_counts": {"ENTRY_QUALITY_MISSING_QUOTE_STATE": first_buy_state_count} if first_buy_state_count else {},
        "coverage_ratio": round((unique_launch_states / chain_estimated) * 100.0, 4) if chain_estimated > 0 else 0.0,
        "top_decode_blocker": top_blocker,
        "decode_class_counts": dict(decode_class_counts),
        "route_family_counts": dict(route_counts),
        "decode_confidence_counts": dict(confidence_counts),
        "source_provenance_counts": dict(provenance_counts),
        "first_blocker_counts": dict(blocker_counts),
        "source_paths": {
            "stream_rows": text(args.stream_rows),
            "birth_rows": text(args.birth_rows),
            "activity_rows": text(args.activity_rows),
        },
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--stream-rows", type=Path, required=True)
    parser.add_argument("--utils-path", type=Path, required=True)
    parser.add_argument("--requested-duration-seconds", default="0")
    parser.add_argument("--chain-estimate-per-hour", default="1000")
    parser.add_argument("--birth-rows", type=Path, required=True)
    parser.add_argument("--activity-rows", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    args = parser.parse_args()

    utils = load_module(args.utils_path, "runls13_utils_birth_decoder")
    stream_rows = load_jsonl(args.stream_rows)
    activity_rows, birth_rows, blocker_counts = decode_rows(stream_rows, utils)
    write_jsonl(args.activity_rows, activity_rows)
    write_jsonl(args.birth_rows, birth_rows)
    summary = build_summary(args, stream_rows, activity_rows, birth_rows, blocker_counts)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter, defaultdict
from decimal import Decimal
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
    return text(row.get("route_family")) == ACTIVE_ROUTE and not boolish(row.get("curve_complete"))


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


def rpc_result(lane: Any, rpc_url: str, method: str, params: list[Any]) -> tuple[Any, str]:
    response, error = lane.rpc(rpc_url, method, params)
    if error is not None:
        return None, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    return (response or {}).get("result", response), ""


def tx_account_keys(tx: dict[str, Any]) -> list[str]:
    message = ((tx.get("transaction") or {}).get("message")) or {}
    keys: list[str] = []
    for item in message.get("accountKeys") or []:
        keys.append(text(item.get("pubkey")) if isinstance(item, dict) else text(item))
    loaded = ((tx.get("meta") or {}).get("loadedAddresses")) or {}
    for item in loaded.get("writable") or []:
        keys.append(text(item))
    for item in loaded.get("readonly") or []:
        keys.append(text(item))
    return [item for item in keys if item]


def tx_instructions(tx: dict[str, Any]) -> list[dict[str, Any]]:
    message = ((tx.get("transaction") or {}).get("message")) or {}
    out = [item for item in (message.get("instructions") or []) if isinstance(item, dict)]
    for inner in (tx.get("meta") or {}).get("innerInstructions") or []:
        for item in inner.get("instructions") or []:
            if isinstance(item, dict):
                out.append(item)
    return out


def instruction_program_id(ix: dict[str, Any], keys: list[str]) -> str:
    explicit = text(ix.get("programId"))
    if explicit:
        return explicit
    idx = safe_int(ix.get("programIdIndex"), -1)
    return keys[idx] if 0 <= idx < len(keys) else ""


def instruction_accounts(ix: dict[str, Any], keys: list[str]) -> list[str]:
    out: list[str] = []
    for item in ix.get("accounts") or []:
        if isinstance(item, int):
            if 0 <= item < len(keys):
                out.append(keys[item])
        else:
            item_text = text(item)
            if item_text:
                out.append(item_text)
    return out


def source_signatures(row: dict[str, Any]) -> list[str]:
    out: list[str] = []
    for field in ("source_signature", "create_signature", "first_buy_signature", "first_sell_signature", "target_signature"):
        value = text(row.get(field))
        if value and value not in out:
            out.append(value)
    return out


def build_activity_index(activity_rows: list[dict[str, Any]]) -> tuple[dict[str, list[dict[str, Any]]], dict[str, list[dict[str, Any]]]]:
    by_signature: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_mint: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in activity_rows:
        signature = text(row.get("signature") or row.get("source_signature") or row.get("target_signature"))
        mint = text(row.get("mint"))
        if signature:
            by_signature[signature].append(row)
        if mint:
            by_mint[mint].append(row)
    return by_signature, by_mint


def ix_identity_from_activity(row: dict[str, Any]) -> tuple[str, str, dict[str, bool]]:
    decode_class = text(row.get("decode_class") or row.get("source_kind")).upper()
    flags = {
        "pump_create_ix_seen": "CREATE" in decode_class,
        "pump_buy_ix_seen": "BUY" in decode_class,
        "pump_sell_ix_seen": "SELL" in decode_class,
    }
    return text(row.get("bonding_curve")), text(row.get("associated_bonding_curve")), flags


def find_ix_identity(
    row: dict[str, Any],
    activity_by_signature: dict[str, list[dict[str, Any]]],
    activity_by_mint: dict[str, list[dict[str, Any]]],
) -> tuple[str, str, dict[str, bool]]:
    mint = text(row.get("mint"))
    flags = {"pump_create_ix_seen": False, "pump_buy_ix_seen": False, "pump_sell_ix_seen": False}
    candidates: list[dict[str, Any]] = []
    for signature in source_signatures(row):
        candidates.extend(activity_by_signature.get(signature, []))
    if mint:
        candidates.extend(activity_by_mint.get(mint, []))

    for item in candidates:
        if text(item.get("route_family")) != ACTIVE_ROUTE:
            continue
        curve, assoc, item_flags = ix_identity_from_activity(item)
        for key, value in item_flags.items():
            flags[key] = flags[key] or value
        if curve:
            return curve, assoc, flags
    return "", "", flags


def infer_identity_from_accounts(accounts: list[str]) -> tuple[str, str, str]:
    best: tuple[int, str, str, str] = (0, "", "", "")
    account_set = set(accounts)
    for candidate_mint in accounts:
        curve, assoc = derive_bonding_curve_pda(candidate_mint)
        if not curve or curve not in account_set:
            continue
        score = 2 + int(bool(assoc and assoc in account_set))
        if score > best[0]:
            best = (score, candidate_mint, curve, assoc)
    return best[1], best[2], best[3]


def fetch_source_tx_identity(
    row: dict[str, Any],
    lane: Any,
    rpc_url: str,
    tx_cache: dict[str, tuple[int, str, str, str, dict[str, bool], str]],
    max_fetches: list[int],
) -> tuple[int, str, str, str, dict[str, bool], str]:
    flags = {"pump_create_ix_seen": False, "pump_buy_ix_seen": False, "pump_sell_ix_seen": False}
    mint = text(row.get("mint"))
    for signature in source_signatures(row):
        if signature in tx_cache:
            return tx_cache[signature]
        if max_fetches[0] <= 0:
            return 0, "", "", "", flags, "tx_fetch_budget_exhausted"
        max_fetches[0] -= 1
        tx, error = rpc_result(
            lane,
            rpc_url,
            "getTransaction",
            [
                signature,
                {
                    "encoding": "json",
                    "commitment": "confirmed",
                    "maxSupportedTransactionVersion": 0,
                },
            ],
        )
        if error or not isinstance(tx, dict):
            tx_cache[signature] = (0, "", "", "", flags, error or "tx_missing")
            continue
        keys = tx_account_keys(tx)
        account_keys_count = len(keys)
        best_curve = ""
        best_assoc = ""
        best_mint = ""
        for ix in tx_instructions(tx):
            if instruction_program_id(ix, keys) != PUMP_PROGRAM_ID:
                continue
            accounts = instruction_accounts(ix, keys)
            inferred_mint, inferred_curve, inferred_assoc = infer_identity_from_accounts(accounts)
            if inferred_curve:
                best_mint = best_mint or inferred_mint
                best_curve = best_curve or inferred_curve
                best_assoc = best_assoc or inferred_assoc
            if len(accounts) >= 3 and mint and accounts[0] == mint:
                flags["pump_create_ix_seen"] = True
                best_mint = best_mint or accounts[0]
                best_curve = best_curve or accounts[1]
                best_assoc = best_assoc or accounts[2]
            if len(accounts) >= 5 and mint and accounts[2] == mint:
                # Buy and sell share the same mint/curve/associated positions.
                best_mint = best_mint or accounts[2]
                best_curve = best_curve or accounts[3]
                best_assoc = best_assoc or accounts[4]
                flags["pump_buy_ix_seen"] = True
                flags["pump_sell_ix_seen"] = True
        tx_cache[signature] = (account_keys_count, best_mint, best_curve, best_assoc, flags, "")
        if best_curve:
            return tx_cache[signature]
    return 0, "", "", "", flags, "source_tx_identity_not_found"


def hydrate_candidate(
    source_row: dict[str, Any],
    lane: Any,
    hydrator: Any,
    args: argparse.Namespace,
    curve: str,
    associated: str,
) -> dict[str, Any]:
    out = dict(source_row)
    state_row = dict(source_row)
    state_row["bonding_curve"] = curve
    state_row["associated_bonding_curve"] = associated
    state_row["pool"] = curve
    state_row["pool_or_curve"] = curve
    return hydrator.hydrate_pump_bonding_curve_row(out, state_row, lane, {}, args)


def classify_identity(
    hydrated: dict[str, Any],
    source: str,
    has_mint: bool,
    has_ix_curve: bool,
    derived_curve: str,
    ix_curve: str,
) -> tuple[str, str]:
    blocker = text(hydrated.get("first_quote_state_blocker"))
    owner_ok = text(hydrated.get("bonding_curve_owner")) == PUMP_PROGRAM_ID
    if blocker == READY_BLOCKER:
        return ("IDENTITY_READY_FROM_PDA" if source == "derived_pda" else "IDENTITY_READY_FROM_IX"), "none"
    if not has_mint:
        return "PARK_NO_MINT", "no_mint"
    if not has_ix_curve and source == "ix":
        return "PARK_NO_BONDING_CURVE_IN_IX", "no_bonding_curve_in_ix"
    if ix_curve and derived_curve and ix_curve != derived_curve:
        return "PARK_PDA_DERIVE_MISMATCH", "ix_curve_differs_from_derived_pda"
    if blocker == "BONDING_CURVE_STATE_MISSING":
        return "PARK_ACCOUNT_NOT_FOUND", "account_not_found"
    if text(hydrated.get("bonding_curve_owner")) and not owner_ok:
        return "PARK_OWNER_PROGRAM_MISMATCH", "owner_program_mismatch"
    return "PARK_STATE_DECODE_FAILED", blocker or "state_decode_failed"


def choose_identity(
    row: dict[str, Any],
    lane: Any,
    hydrator: Any,
    args: argparse.Namespace,
    ix_curve: str,
    ix_assoc: str,
    derived_curve: str,
    derived_assoc: str,
) -> tuple[dict[str, Any], str, str]:
    candidates: list[tuple[str, str, str]] = []
    if ix_curve:
        candidates.append(("ix", ix_curve, ix_assoc))
    original_curve = text(row.get("bonding_curve") or row.get("pool") or row.get("pool_or_curve"))
    original_assoc = text(row.get("associated_bonding_curve"))
    if original_curve and original_curve not in {curve for _source, curve, _assoc in candidates}:
        candidates.append(("row", original_curve, original_assoc))
    if derived_curve and derived_curve not in {curve for _source, curve, _assoc in candidates}:
        candidates.append(("derived_pda", derived_curve, derived_assoc))
    if not candidates:
        out = dict(row)
        return out, "PARK_NO_BONDING_CURVE_IN_IX", "no_identity_candidate"

    hydrated_candidates: list[tuple[dict[str, Any], str, str, str]] = []
    for source, curve, assoc in candidates:
        hydrated = hydrate_candidate(row, lane, hydrator, args, curve, assoc)
        decision, blocker = classify_identity(hydrated, source, bool(text(row.get("mint"))), bool(ix_curve), derived_curve, ix_curve)
        hydrated_candidates.append((hydrated, source, decision, blocker))
        if decision in {"IDENTITY_READY_FROM_IX", "IDENTITY_READY_FROM_PDA"}:
            return hydrated, decision, blocker

    for hydrated, _source, decision, blocker in hydrated_candidates:
        if text(hydrated.get("first_quote_state_blocker")) != "BONDING_CURVE_STATE_MISSING":
            return hydrated, decision, blocker
    hydrated, _source, decision, blocker = hydrated_candidates[0]
    return hydrated, decision, blocker


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0U5 active curve account identity autopsy.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--launch0u-rows", type=Path, required=True)
    parser.add_argument("--activity-rows", action="append", type=Path, default=[])
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--quote-hydrator-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-tx-fetches", type=int, default=300)
    parser.add_argument("--max-spend-sol", default="0.01")
    parser.add_argument("--max-loss-sol", default="0.015")
    parser.add_argument("--slippage-bps", type=int, default=100)
    parser.add_argument("--fee-budget-lamports", type=int, default=410_000)
    parser.add_argument("--exit-safety-bps", type=int, default=10_000)
    parser.add_argument("--min-base-reserve-lamports", type=int, default=25_000_000)
    parser.add_argument("--reserve-after-exit-floor-lamports", type=int, default=10_000_000)
    parser.add_argument("--max-exit-price-impact-bps", type=int, default=1500)
    args = parser.parse_args()
    args.max_loss_lamports = sol_to_lamports(args.max_loss_sol)

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_u5")
    hydrator = load_module(args.quote_hydrator_path, "launch_realtime_quote_state_hydrator_for_u5")
    rows = load_jsonl(args.launch0u_rows)
    activity_rows: list[dict[str, Any]] = []
    for path in args.activity_rows:
        activity_rows.extend(load_jsonl(path))
    activity_by_signature, activity_by_mint = build_activity_index(activity_rows)

    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    blocker_counts: Counter[str] = Counter()
    before_blockers: Counter[str] = Counter()
    after_blockers: Counter[str] = Counter()
    tx_cache: dict[str, tuple[int, str, str, str, dict[str, bool], str]] = {}
    tx_fetch_budget = [args.max_tx_fetches]

    active_rows = 0
    state_missing_before = 0
    state_missing_after = 0
    quote_ready_before = 0
    quote_ready_after = 0
    identity_ready_rows = 0

    for row in rows:
        if not active_curve(row):
            out_rows.append(dict(row, record_type="launch0u5_active_curve_account_identity_autopsy", wire_send=0, live_landed=0))
            continue

        active_rows += 1
        before = text(row.get("first_quote_state_blocker")) or "unknown"
        before_blockers[before] += 1
        state_missing_before += int(before == "BONDING_CURVE_STATE_MISSING")
        quote_ready_before += int(before == READY_BLOCKER or boolish(row.get("quote_state_ready")))

        ix_curve, ix_assoc, activity_flags = find_ix_identity(row, activity_by_signature, activity_by_mint)
        account_keys_count, tx_mint, tx_curve, tx_assoc, tx_flags, tx_error = fetch_source_tx_identity(
            row, lane, args.rpc_url, tx_cache, tx_fetch_budget
        )
        for key, value in tx_flags.items():
            activity_flags[key] = activity_flags.get(key, False) or value
        effective_row = dict(row)
        if tx_mint and tx_curve:
            effective_row["mint_original"] = text(row.get("mint"))
            effective_row["mint"] = tx_mint
            effective_row["bonding_curve_original"] = text(row.get("bonding_curve"))
            effective_row["associated_bonding_curve_original"] = text(row.get("associated_bonding_curve"))
            ix_curve, ix_assoc = tx_curve, tx_assoc
        elif not ix_curve and tx_curve:
            ix_curve, ix_assoc = tx_curve, tx_assoc
        derived_curve, derived_assoc = derive_bonding_curve_pda(text(effective_row.get("mint")))

        if before == "BONDING_CURVE_STATE_MISSING":
            repaired, decision, blocker = choose_identity(effective_row, lane, hydrator, args, ix_curve, ix_assoc, derived_curve, derived_assoc)
        else:
            repaired = dict(effective_row)
            decision = "IDENTITY_READY_FROM_IX" if before == READY_BLOCKER and ix_curve else "IDENTITY_READY_FROM_PDA" if before == READY_BLOCKER and derived_curve else "PARK_STATE_DECODE_FAILED"
            blocker = "none" if before == READY_BLOCKER else before

        after = text(repaired.get("first_quote_state_blocker")) or before
        state_missing_after += int(after == "BONDING_CURVE_STATE_MISSING")
        quote_ready_after += int(after == READY_BLOCKER or boolish(repaired.get("quote_state_ready")))
        identity_ready_rows += int(decision in {"IDENTITY_READY_FROM_IX", "IDENTITY_READY_FROM_PDA"})
        decision_counts[decision] += 1
        blocker_counts[blocker] += 1
        after_blockers[after] += 1

        enriched = dict(repaired)
        enriched.update(
            {
                "record_type": "launch0u5_active_curve_account_identity_autopsy",
                "launch_state_id": text(row.get("launch_state_id")),
                "mint_original": text(row.get("mint")),
                "mint_from_ix": tx_mint,
                "source_signature": text(row.get("source_signature")),
                "source_kind": text(row.get("source_kind")),
                "program_id": text(row.get("program_id")),
                "account_keys_count": account_keys_count,
                "pump_create_ix_seen": bool(activity_flags.get("pump_create_ix_seen")),
                "pump_buy_ix_seen": bool(activity_flags.get("pump_buy_ix_seen")),
                "pump_sell_ix_seen": bool(activity_flags.get("pump_sell_ix_seen")),
                "bonding_curve_from_ix": ix_curve,
                "associated_bonding_curve_from_ix": ix_assoc,
                "derived_bonding_curve_pda": derived_curve,
                "derived_associated_bonding_curve_pda": derived_assoc,
                "rpc_fetch_bonding_curve_from_ix_ok": ix_curve and after != "BONDING_CURVE_STATE_MISSING",
                "rpc_fetch_derived_bonding_curve_ok": derived_curve and text(repaired.get("bonding_curve")) == derived_curve and after != "BONDING_CURVE_STATE_MISSING",
                "owner_program_ok": text(repaired.get("bonding_curve_owner")) == PUMP_PROGRAM_ID,
                "state_decode_ok": after == READY_BLOCKER,
                "identity_decision": decision,
                "first_identity_blocker": blocker,
                "source_tx_fetch_error": tx_error,
                "wire_send": 0,
                "live_landed": 0,
            }
        )
        out_rows.append(enriched)

    status = "LAUNCH0U5_ACTIVE_CURVE_ACCOUNT_IDENTITY_AUTOPSY_PASS"
    if identity_ready_rows == 0:
        status = "PARK_ACTIVE_CURVE_IDENTITY_NOT_READY"
    elif state_missing_after >= state_missing_before and quote_ready_after <= quote_ready_before:
        status = "PARK_ACTIVE_CURVE_IDENTITY_NO_QUOTE_IMPROVEMENT"

    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "launch0u_rows_path": str(args.launch0u_rows),
        "activity_rows_paths": [str(path) for path in args.activity_rows],
        "activity_rows_seen": len(activity_rows),
        "active_bonding_curve_rows": active_rows,
        "identity_ready_rows": identity_ready_rows,
        "bonding_curve_state_missing_before": state_missing_before,
        "bonding_curve_state_missing_after": state_missing_after,
        "quote_ready_rows_before": quote_ready_before,
        "quote_ready_rows_after_identity": quote_ready_after,
        "tx_fetches_used": args.max_tx_fetches - tx_fetch_budget[0],
        "identity_decision_counts": dict(decision_counts),
        "first_identity_blocker_counts": dict(blocker_counts),
        "before_blocker_counts": dict(before_blockers),
        "after_blocker_counts": dict(after_blockers),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": identity_ready_rows > 0 and (state_missing_after < state_missing_before or quote_ready_after > quote_ready_before),
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "active_bonding_curve_rows",
        "identity_ready_rows",
        "bonding_curve_state_missing_before",
        "bonding_curve_state_missing_after",
        "quote_ready_rows_before",
        "quote_ready_rows_after_identity",
        "wire_send",
        "live_landed",
    ):
        print(f"{key}={summary[key]}")
    print(f"identity_decision_counts={json.dumps(summary['identity_decision_counts'], sort_keys=True)}")
    print(f"first_identity_blocker_counts={json.dumps(summary['first_identity_blocker_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

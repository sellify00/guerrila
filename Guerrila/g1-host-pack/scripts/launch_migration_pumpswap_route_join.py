#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


PUMPSWAP_AMM = "PUMPSWAP_AMM"
PUMPSWAP_AFTER_MIGRATION = "pumpswap_after_migration"
PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
COMPLETE_BLOCKER = "BONDING_CURVE_COMPLETE_OR_MIGRATED"
WSOL_MINT = "So11111111111111111111111111111111111111112"
NON_POOL_ACCOUNTS = {
    "",
    "11111111111111111111111111111111",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    "ComputeBudget111111111111111111111111111111",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    PUMPFUN_AMM_PROGRAM_ID,
    WSOL_MINT,
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


def account_keys_from_tx(tx: dict[str, Any]) -> list[str]:
    message = (((tx.get("transaction") or {}).get("message")) or {})
    keys: list[str] = []
    for item in message.get("accountKeys") or []:
        if isinstance(item, dict):
            keys.append(text(item.get("pubkey")))
        else:
            keys.append(text(item))
    loaded = ((tx.get("meta") or {}).get("loadedAddresses")) or {}
    for item in loaded.get("writable") or []:
        keys.append(text(item))
    for item in loaded.get("readonly") or []:
        keys.append(text(item))
    return keys


def tx_instructions(tx: dict[str, Any]) -> list[dict[str, Any]]:
    message = (((tx.get("transaction") or {}).get("message")) or {})
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
    if 0 <= idx < len(keys):
        return keys[idx]
    return ""


def instruction_account_pubkeys(ix: dict[str, Any], keys: list[str]) -> list[str]:
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


def unique_preserve_order(items: list[str]) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for item in items:
        item_text = text(item)
        if not item_text or item_text in seen:
            continue
        seen.add(item_text)
        out.append(item_text)
    return out


def rpc_result(lane: Any, rpc_url: str, method: str, params: list[Any]) -> tuple[Any, str]:
    response, error = lane.rpc(rpc_url, method, params)
    if error is not None:
        return None, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    return (response or {}).get("result", response), ""


def verify_pool(lane: Any, rpc_url: str, pool: str, mint: str) -> tuple[dict[str, Any], str]:
    if not pool or not mint:
        return {}, "pool_or_mint_missing"
    state, error = lane.load_pumpfun_state(rpc_url, pool, mint)
    if error or not state:
        return {}, error or "pumpswap_state_missing"
    return state, ""


def find_pool_from_program_accounts(lane: Any, rpc_url: str, mint: str) -> tuple[str, str]:
    offsets = [
        safe_int(getattr(lane, "PUMPFUN_BASE_MINT_OFFSET", 43), 43),
        safe_int(getattr(lane, "PUMPFUN_QUOTE_MINT_OFFSET", 75), 75),
    ]
    last_error = ""
    for offset in offsets:
        result, error = rpc_result(
            lane,
            rpc_url,
            "getProgramAccounts",
            [
                PUMPFUN_AMM_PROGRAM_ID,
                {
                    "encoding": "base64",
                    "commitment": "processed",
                    "filters": [{"memcmp": {"offset": offset, "bytes": mint}}],
                },
            ],
        )
        if error:
            last_error = error
            continue
        if not isinstance(result, list):
            continue
        for item in result:
            pool = text((item or {}).get("pubkey"))
            state, verify_error = verify_pool(lane, rpc_url, pool, mint)
            if state and not verify_error:
                return pool, ""
            last_error = verify_error or last_error
    return "", last_error or "pumpswap_pool_not_found_by_program_accounts"


def candidate_pool_pubkeys_from_tx(tx: dict[str, Any], mint: str) -> tuple[list[str], str]:
    keys = account_keys_from_tx(tx)
    pamm_ix_accounts: list[str] = []
    saw_pamm_ix = False
    for ix in tx_instructions(tx):
        if instruction_program_id(ix, keys) != PUMPFUN_AMM_PROGRAM_ID:
            continue
        saw_pamm_ix = True
        pamm_ix_accounts.extend(instruction_account_pubkeys(ix, keys))
    if pamm_ix_accounts:
        source = "rpc_mint_signature_pamm_instruction_scan"
        raw_candidates = pamm_ix_accounts
    elif PUMPFUN_AMM_PROGRAM_ID in keys:
        # Some routed swaps only expose the pAMM program via nested routing context.
        # Fall back to decoding the transaction account set; the discriminator check
        # keeps this read-only scan precise without depending on account positions.
        source = "rpc_mint_signature_account_set_scan"
        raw_candidates = keys
    else:
        return [], ""
    blocked = NON_POOL_ACCOUNTS | {mint}
    return unique_preserve_order([item for item in raw_candidates if item not in blocked]), source


def find_pool_in_candidates(
    lane: Any,
    rpc_url: str,
    candidates: list[str],
    mint: str,
) -> tuple[str, str]:
    if not candidates:
        return "", "candidate_pool_empty"
    accounts, _slot, account_error = lane.get_multiple_accounts(rpc_url, candidates)
    if account_error:
        return "", account_error
    for candidate in candidates:
        raw = (accounts.get(candidate) or {}).get("data_raw", b"")
        pool_doc, pool_error = lane.decode_pumpfun_pool(raw)
        if pool_error or not pool_doc:
            continue
        mint_set = {pool_doc.get("base_mint"), pool_doc.get("quote_mint")}
        if mint in mint_set and WSOL_MINT in mint_set:
            state, verify_error = verify_pool(lane, rpc_url, candidate, mint)
            if state and not verify_error:
                return candidate, ""
            return "", verify_error or "pumpswap_state_missing"
    return "", "pumpswap_pool_not_found_in_candidate_accounts"


def local_pools_by_mint(rows: list[dict[str, Any]]) -> dict[str, list[str]]:
    out: dict[str, list[str]] = defaultdict(list)
    for row in rows:
        if text(row.get("route_family")) != PUMPSWAP_AMM:
            continue
        mint = text(row.get("mint"))
        pool = text(row.get("pumpswap_pool") or row.get("pool"))
        if mint and pool and pool not in out[mint]:
            out[mint].append(pool)
    return out


def provenance_candidates(rows: list[dict[str, Any]], provenance_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    if not provenance_rows:
        return [row for row in rows if text(row.get("first_quote_state_blocker")) == COMPLETE_BLOCKER]
    source_by_key: dict[str, dict[str, Any]] = {}
    for row in rows:
        keys = [
            text(row.get("launch_state_id")),
            "|".join([text(row.get("mint")), text(row.get("bonding_curve")), text(row.get("source_signature"))]),
            "|".join([text(row.get("mint")), text(row.get("bonding_curve")), text(row.get("first_sell_signature"))]),
            "|".join([text(row.get("mint")), text(row.get("bonding_curve")), text(row.get("first_buy_signature"))]),
        ]
        for key in keys:
            if key:
                source_by_key[key] = row
    out: list[dict[str, Any]] = []
    for prov in provenance_rows:
        keys = [
            text(prov.get("launch_state_id")),
            "|".join([text(prov.get("mint")), text(prov.get("bonding_curve")), text(prov.get("source_signature"))]),
            "|".join([text(prov.get("mint")), text(prov.get("bonding_curve")), text(prov.get("first_sell_signature"))]),
            "|".join([text(prov.get("mint")), text(prov.get("bonding_curve")), text(prov.get("first_buy_signature"))]),
        ]
        source = {}
        for key in keys:
            if key and key in source_by_key:
                source = source_by_key[key]
                break
        merged = {**source, **prov}
        out.append(merged)
    return out


def watcher_candidates(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        if not text(row.get("pumpswap_pool")) or not text(row.get("mint")):
            continue
        decision = text(row.get("decision"))
        if decision not in {"MIGRATION_EVENT_POOL_JOINED", "MIGRATE_INSTRUCTION_POOL_JOINED"}:
            continue
        out.append(
            {
                **row,
                "source_provenance": "MIGRATION_DERIVED",
                "source_kind": "PUMPSWAP_AFTER_MIGRATION_WATCHER",
                "migration_provenance_decision": "MIGRATION_EVENT_PROVEN" if decision == "MIGRATION_EVENT_POOL_JOINED" else "MIGRATION_TX_PROVEN",
                "pumpswap_pool_final": text(row.get("pumpswap_pool")),
                "pool_join_source": text(row.get("pool_join_source")) or "MIGRATE_INSTRUCTION",
            }
        )
    return out


def find_pool_from_rpc(
    lane: Any,
    rpc_url: str,
    mint: str,
    signature_limit: int,
    tx_scan_limit: int,
    enable_history_scan: bool,
) -> tuple[str, str, str]:
    pool, gpa_error = find_pool_from_program_accounts(lane, rpc_url, mint)
    if pool:
        return pool, "", "rpc_get_program_accounts_mint_memcmp"
    if not enable_history_scan:
        return "", "", gpa_error or "pumpswap_pool_not_found_by_program_accounts"

    signatures, sig_error = rpc_result(
        lane,
        rpc_url,
        "getSignaturesForAddress",
        [mint, {"limit": max(1, signature_limit)}],
    )
    if sig_error or not isinstance(signatures, list):
        return "", "", sig_error or "signature_search_empty"
    scanned = 0
    for sig_doc in signatures:
        signature = text((sig_doc or {}).get("signature"))
        if not signature:
            continue
        tx, tx_error = rpc_result(
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
        scanned += 1
        if tx_error or not isinstance(tx, dict):
            if scanned >= tx_scan_limit:
                break
            continue
        keys = account_keys_from_tx(tx)
        candidates, pool_source = candidate_pool_pubkeys_from_tx(tx, mint)
        pool, pool_error = find_pool_in_candidates(lane, rpc_url, candidates, mint)
        if pool:
            return pool, signature, pool_source
        if candidates and pool_error not in {"pumpswap_pool_not_found_in_candidate_accounts", "candidate_pool_empty"}:
            return "", "", pool_error
        if scanned >= tx_scan_limit:
            break
    return "", "", gpa_error or "pumpswap_pool_not_found_in_mint_history"


def quote_decision(
    lane: Any,
    state: dict[str, Any],
    max_spend_sol: str,
    max_loss_lamports: int,
    slippage_bps: int,
    fee_budget_lamports: int,
    exit_safety_bps: int,
    min_base_reserve_lamports: int,
    reserve_after_exit_floor_lamports: int,
    max_exit_price_impact_bps: int,
) -> tuple[str, dict[str, Any]]:
    ladder_rows, _summary = lane.pumpfun_size_ladder(
        state,
        [max_spend_sol],
        slippage_bps,
        fee_budget_lamports,
        max_loss_lamports,
        exit_safety_bps,
    )
    ladder = ladder_rows[0] if ladder_rows else {}
    entry_expected = safe_int(ladder.get("entry_expected_out_raw"))
    entry_min = safe_int(ladder.get("entry_min_out_raw"))
    exit_expected = safe_int(ladder.get("exit_expected_out_raw"))
    exit_min = safe_int(ladder.get("exit_min_out_raw"))
    base_reserve = safe_int(state.get("core_reserve_raw"))
    token_reserve = safe_int(state.get("launch_reserve_raw"))
    reserve_after = max(0, base_reserve - exit_expected)
    impact = int(round((exit_expected / base_reserve) * 10_000.0)) if exit_expected > 0 and base_reserve > 0 else 0
    fields = {
        "pumpswap_base_reserve": base_reserve,
        "pumpswap_token_reserve": token_reserve,
        "pumpswap_entry_quote_ok": entry_expected > 0 and entry_min > 0,
        "pumpswap_entry_expected_out_raw": entry_expected,
        "pumpswap_entry_min_out_raw": entry_min,
        "pumpswap_exit_quote_ok": exit_expected > 0 and exit_min > 0,
        "pumpswap_exit_expected_out_raw": exit_expected,
        "pumpswap_exit_min_out_raw": exit_min,
        "pumpswap_exit_non_dust": exit_expected > 1 and exit_min > 0,
        "pumpswap_reserve_after_exit_raw": reserve_after,
        "pumpswap_exit_price_impact_bps": impact,
        "pumpswap_size_ladder_decision": text(ladder.get("decision")),
        "pumpswap_size_ladder_first_blocker": text(ladder.get("first_blocker")),
    }
    if not fields["pumpswap_entry_quote_ok"]:
        return "PARK_PUMPSWAP_RESERVES_MISSING", fields
    if not fields["pumpswap_exit_quote_ok"]:
        return "PARK_PUMPSWAP_EXIT_QUOTE_ZERO", fields
    if not fields["pumpswap_exit_non_dust"]:
        return "PARK_PUMPSWAP_EXIT_DUST", fields
    if base_reserve < min_base_reserve_lamports or reserve_after < reserve_after_exit_floor_lamports:
        return "PARK_PUMPSWAP_BASE_RESERVE_BELOW_FLOOR", fields
    if max_exit_price_impact_bps > 0 and impact > max_exit_price_impact_bps:
        return "PARK_PUMPSWAP_PRICE_IMPACT_TOO_HIGH", fields
    return "MIGRATION_PUMPSWAP_ROUTE_READY", fields


def main() -> int:
    parser = argparse.ArgumentParser(description="Launch0M completed/migrated bonding-curve to PumpSwap route join.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--quote-state-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
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
    parser.add_argument("--signature-search-limit", type=int, default=100)
    parser.add_argument("--tx-scan-limit", type=int, default=100)
    parser.add_argument("--enable-history-scan", action="store_true")
    parser.add_argument("--migration-provenance-rows", type=Path)
    parser.add_argument("--migration-watcher-rows", type=Path)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_migration_join")
    rows = load_jsonl(args.quote_state_rows)
    migration_provenance_rows = load_jsonl(args.migration_provenance_rows)
    migration_watcher_rows = load_jsonl(args.migration_watcher_rows)
    local_pools = local_pools_by_mint(rows)
    candidates = provenance_candidates(rows, migration_provenance_rows) + watcher_candidates(migration_watcher_rows)
    pool_cache: dict[str, tuple[str, str, str]] = {}
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    pool_found_count = 0
    quote_ready_rows = 0
    exit_ready_rows = 0

    for row in candidates:
        mint = text(row.get("mint"))
        bonding_curve = text(row.get("bonding_curve"))
        associated = text(row.get("associated_bonding_curve"))
        migration_signature = text(row.get("migration_signature"))
        migration_decision = text(row.get("migration_provenance_decision"))
        pool = text(row.get("pumpswap_pool_final"))
        pool_source = ""
        proof_signature = ""
        state: dict[str, Any] = {}
        state_error = ""
        row_quote: dict[str, Any] = {
            "pumpswap_base_reserve": 0,
            "pumpswap_token_reserve": 0,
            "pumpswap_entry_quote_ok": False,
            "pumpswap_entry_expected_out_raw": 0,
            "pumpswap_entry_min_out_raw": 0,
            "pumpswap_exit_quote_ok": False,
            "pumpswap_exit_expected_out_raw": 0,
            "pumpswap_exit_min_out_raw": 0,
            "pumpswap_exit_non_dust": False,
            "pumpswap_reserve_after_exit_raw": 0,
            "pumpswap_exit_price_impact_bps": 0,
        }
        if not mint:
            decision = "PARK_MIGRATION_STATE_UNRESOLVED"
        elif migration_provenance_rows and not pool:
            decision = migration_decision or "PARK_MIGRATION_POOL_NOT_FOUND"
        else:
            if pool:
                state, state_error = verify_pool(lane, args.rpc_url, pool, mint)
                pool_source = text(row.get("pool_join_source")) or "migration_provenance"
            for local_pool in local_pools.get(mint, []):
                if pool:
                    break
                state, state_error = verify_pool(lane, args.rpc_url, local_pool, mint)
                if state and not state_error:
                    pool = local_pool
                    pool_source = "local_realtime_pumpswap_state"
                    break
            if not pool:
                if mint not in pool_cache:
                    pool_cache[mint] = find_pool_from_rpc(
                        lane,
                        args.rpc_url,
                        mint,
                        args.signature_search_limit,
                        args.tx_scan_limit,
                        args.enable_history_scan,
                    )
                pool, proof_signature, pool_source = pool_cache[mint]
                if pool:
                    state, state_error = verify_pool(lane, args.rpc_url, pool, mint)
            if not pool:
                decision = "PARK_MIGRATION_POOL_NOT_FOUND"
            elif state_error or not state:
                pool_found_count += 1
                decision = "PARK_PUMPSWAP_RESERVES_MISSING"
            else:
                pool_found_count += 1
                decision, quote_fields = quote_decision(
                    lane,
                    state,
                    args.max_spend_sol,
                    args.max_loss_lamports,
                    args.slippage_bps,
                    args.fee_budget_lamports,
                    args.exit_safety_bps,
                    args.min_base_reserve_lamports,
                    args.reserve_after_exit_floor_lamports,
                    args.max_exit_price_impact_bps,
                )
                row_quote = quote_fields
                quote_ready_rows += int(decision == "MIGRATION_PUMPSWAP_ROUTE_READY")
                exit_ready_rows += int(decision == "MIGRATION_PUMPSWAP_ROUTE_READY")

        if state and not row_quote["pumpswap_base_reserve"]:
            row_quote["pumpswap_base_reserve"] = safe_int(state.get("core_reserve_raw"))
            row_quote["pumpswap_token_reserve"] = safe_int(state.get("launch_reserve_raw"))
        decision_counts[decision] += 1
        ready = decision == "MIGRATION_PUMPSWAP_ROUTE_READY"
        out = {
            "record_type": "launch0m_migration_pumpswap_route_join",
            "launch_state_id": text(row.get("launch_state_id")),
            "mint": mint,
            "bonding_curve": bonding_curve,
            "associated_bonding_curve": associated,
            "migration_signature": migration_signature or proof_signature,
            "migration_proof_signature": proof_signature,
            "pumpswap_pool": pool,
            "pool": pool,
            "pumpswap_pool_found": bool(pool),
            "pumpswap_pool_source": pool_source,
            "migration_provenance_decision": migration_decision,
            "pool_join_source": text(row.get("pool_join_source")) or pool_source,
            "route_family": PUMPSWAP_AFTER_MIGRATION,
            "source_provenance": "MIGRATION_DERIVED",
            "source_kind": "PUMPSWAP_AFTER_MIGRATION_JOIN",
            "trigger_b_pass": boolish(row.get("trigger_b_pass")),
            "first_buy_signature": text(row.get("first_buy_signature")),
            "first_buy_seen_ns": safe_int(row.get("first_buy_seen_ns")),
            "first_sell_signature": text(row.get("first_sell_signature")),
            "first_sell_seen_ns": safe_int(row.get("first_sell_seen_ns")),
            "last_trade_side": text(row.get("last_trade_side")),
            "net_buy_pressure_2s": safe_int(row.get("net_buy_pressure_2s")),
            "net_buy_pressure_5s": safe_int(row.get("net_buy_pressure_5s")),
            "sell_count_2s": safe_int(row.get("sell_count_2s")),
            "sell_count_5s": safe_int(row.get("sell_count_5s")),
            "entry_quote_ok": ready,
            "entry_expected_out_raw": safe_int(row_quote.get("pumpswap_entry_expected_out_raw")),
            "entry_min_out_raw": safe_int(row_quote.get("pumpswap_entry_min_out_raw")),
            "entry_quote_expected_out_raw": safe_int(row_quote.get("pumpswap_entry_expected_out_raw")),
            "entry_quote_min_out_raw": safe_int(row_quote.get("pumpswap_entry_min_out_raw")),
            "exit_quote_ok": ready,
            "exit_expected_out_raw": safe_int(row_quote.get("pumpswap_exit_expected_out_raw")),
            "exit_min_out_raw": safe_int(row_quote.get("pumpswap_exit_min_out_raw")),
            "exit_quote_expected_out_raw": safe_int(row_quote.get("pumpswap_exit_expected_out_raw")),
            "exit_quote_min_out_raw": safe_int(row_quote.get("pumpswap_exit_min_out_raw")),
            "exit_non_dust": boolish(row_quote.get("pumpswap_exit_non_dust")),
            "base_reserve_raw": safe_int(row_quote.get("pumpswap_base_reserve")),
            "token_reserve_raw": safe_int(row_quote.get("pumpswap_token_reserve")),
            "reserve_after_exit_raw": safe_int(row_quote.get("pumpswap_reserve_after_exit_raw")),
            "exit_base_reserve_lamports": safe_int(row_quote.get("pumpswap_base_reserve")),
            "exit_reserve_after_exit_lamports": safe_int(row_quote.get("pumpswap_reserve_after_exit_raw")),
            "exit_price_impact_bps": safe_int(row_quote.get("pumpswap_exit_price_impact_bps")),
            "entry_quality_baseline_expected_out_raw": safe_int(row_quote.get("pumpswap_entry_expected_out_raw")) if ready else 0,
            "entry_quality_baseline_warmup": not ready,
            "price_change_since_trigger_bps": 0 if ready else -1,
            "quote_state_ready": ready,
            "exit_liquidity_guard_ready": ready,
            "pumpswap_exit_liquidity_guard_ready": ready,
            "first_quote_state_blocker": "QUOTE_STATE_READY" if ready else decision,
            "decision": decision,
            **row_quote,
        }
        out_rows.append(out)

    summary = {
        "status": "LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN_PASS" if pool_found_count > 0 and quote_ready_rows > 0 and exit_ready_rows > 0 else "LAUNCH0M_MIGRATION_PUMPSWAP_ROUTE_JOIN_INCOMPLETE",
        "run_root": text(args.run_root),
        "input_rows": text(args.quote_state_rows),
        "completed_or_migrated_input_rows": len(candidates),
        "pumpswap_pool_found_count": pool_found_count,
        "pumpswap_quote_state_ready_rows": quote_ready_rows,
        "pumpswap_exit_liquidity_guard_ready_rows": exit_ready_rows,
        "decision_counts": dict(decision_counts),
        "rows_path": text(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": pool_found_count > 0 and quote_ready_rows > 0 and exit_ready_rows > 0,
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    print(f"status={summary['status']}")
    print(f"acceptance_pass={str(summary['acceptance_pass']).lower()}")
    print(f"completed_or_migrated_input_rows={len(candidates)}")
    print(f"pumpswap_pool_found_count={pool_found_count}")
    print(f"pumpswap_quote_state_ready_rows={quote_ready_rows}")
    print(f"pumpswap_exit_liquidity_guard_ready_rows={exit_ready_rows}")
    print(f"decision_counts={json.dumps(dict(decision_counts), sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

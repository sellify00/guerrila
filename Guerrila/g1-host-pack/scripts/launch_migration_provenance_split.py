#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
WSOL_MINT = "So11111111111111111111111111111111111111112"
COMPLETE_BLOCKER = "BONDING_CURVE_COMPLETE_OR_MIGRATED"


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
    return text(value).lower() in {"1", "true", "yes", "y", "on", "pass", "ready"}


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


def explicit_event_pool(row: dict[str, Any]) -> str:
    for key in (
        "event_pool",
        "complete_pump_amm_migration_event_pool",
        "migration_event_pool",
        "migration_pool",
        "pumpswap_pool_final",
    ):
        value = text(row.get(key))
        if value:
            return value
    return ""


def unique_signatures(row: dict[str, Any]) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for key in (
        "migration_signature",
        "source_signature",
        "first_buy_signature",
        "first_sell_signature",
        "create_signature",
    ):
        value = text(row.get(key))
        if value and value not in seen:
            seen.add(value)
            out.append(value)
    return out


def tx_logs(tx: dict[str, Any]) -> list[str]:
    logs = (tx.get("meta") or {}).get("logMessages") or []
    return [text(item) for item in logs if text(item)]


def migration_event_seen(tx: dict[str, Any]) -> bool:
    joined = "\n".join(tx_logs(tx)).lower()
    return "completepumpammmigrationevent" in joined or "complete_pump_amm_migration" in joined


def pump_migration_instruction_seen(join: Any, tx: dict[str, Any]) -> bool:
    keys = join.account_keys_from_tx(tx)
    if PUMPFUN_AMM_PROGRAM_ID not in keys:
        return False
    for ix in join.tx_instructions(tx):
        if join.instruction_program_id(ix, keys) == PUMP_PROGRAM_ID:
            return True
    return False


def rpc_tx(join: Any, lane: Any, rpc_url: str, signature: str) -> tuple[dict[str, Any], str]:
    tx, error = join.rpc_result(
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
        return {}, error or "tx_not_found"
    return tx, ""


def migration_pool_from_signatures(
    join: Any,
    lane: Any,
    rpc_url: str,
    mint: str,
    signatures: list[str],
) -> tuple[str, str, bool, bool, str]:
    event_seen = False
    instruction_seen = False
    last_error = ""
    for signature in signatures:
        tx, error = rpc_tx(join, lane, rpc_url, signature)
        if error or not tx:
            last_error = error
            continue
        event_seen = event_seen or migration_event_seen(tx)
        instruction_seen = instruction_seen or pump_migration_instruction_seen(join, tx)
        candidates, _source = join.candidate_pool_pubkeys_from_tx(tx, mint)
        pool, pool_error = join.find_pool_in_candidates(lane, rpc_url, candidates, mint)
        if pool:
            return pool, signature, event_seen, instruction_seen, ""
        last_error = pool_error or last_error
    return "", "", event_seen, instruction_seen, last_error


def scan_address_signatures(
    join: Any,
    lane: Any,
    rpc_url: str,
    address: str,
    limit: int,
) -> list[str]:
    if not address or limit <= 0:
        return []
    result, error = join.rpc_result(lane, rpc_url, "getSignaturesForAddress", [address, {"limit": limit}])
    if error or not isinstance(result, list):
        return []
    return [text((item or {}).get("signature")) for item in result if text((item or {}).get("signature"))]


def b58_bytes(value: str) -> bytes:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    n = 0
    for char in value:
        n = n * 58 + alphabet.index(char)
    raw = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = len(value) - len(value.lstrip("1"))
    return b"\x00" * pad + raw


def derived_pool_candidates(row: dict[str, Any]) -> list[str]:
    creator = text(row.get("creator") or row.get("coin_creator") or row.get("user"))
    mint = text(row.get("mint"))
    if not creator or not mint:
        return []
    try:
        from solders.pubkey import Pubkey  # type: ignore

        creator_pk = Pubkey.from_string(creator)
        mint_pk = Pubkey.from_string(mint)
        wsol_pk = Pubkey.from_string(WSOL_MINT)
        program_pk = Pubkey.from_string(PUMPFUN_AMM_PROGRAM_ID)
        index_seeds = [bytes([0]), (0).to_bytes(2, "little"), (0).to_bytes(8, "little")]
        out: list[str] = []
        for index_seed in index_seeds:
            pool, _bump = Pubkey.find_program_address(
                [b"pool", index_seed, bytes(creator_pk), bytes(mint_pk), bytes(wsol_pk)],
                program_pk,
            )
            pool_text = str(pool)
            if pool_text not in out:
                out.append(pool_text)
        return out
    except Exception:
        # Keep this deterministic if solders is absent; the wrapper will report
        # PDA derivation as unavailable rather than guessing a pool address.
        try:
            _ = b58_bytes(creator), b58_bytes(mint)
        except Exception:
            return []
        return []


def first_verified_pool(join: Any, lane: Any, rpc_url: str, pools: list[str], mint: str) -> str:
    for pool in pools:
        state, error = join.verify_pool(lane, rpc_url, pool, mint)
        if state and not error:
            return pool
    return ""


def classify_row(
    row: dict[str, Any],
    join: Any,
    lane: Any,
    rpc_url: str,
    signature_scan_limit: int,
) -> dict[str, Any]:
    mint = text(row.get("mint"))
    bonding_curve = text(row.get("bonding_curve"))
    associated = text(row.get("associated_bonding_curve"))
    creator = text(row.get("creator") or row.get("coin_creator") or row.get("user"))
    complete = boolish(row.get("curve_complete"))
    real_token_reserves = safe_int(row.get("real_token_reserves"))
    event_pool = explicit_event_pool(row)
    migration_signature = text(row.get("migration_signature"))
    migration_instruction = False
    event_seen = bool(event_pool)
    migration_proof_signature = ""
    tx_pool = ""
    derived_pool = ""
    account_scan_pool = ""
    pool_final = ""
    pool_join_source = ""

    if event_pool:
        verified = first_verified_pool(join, lane, rpc_url, [event_pool], mint)
        if verified:
            pool_final = verified
            pool_join_source = "MIGRATION_EVENT"
            decision = "MIGRATION_EVENT_PROVEN"
        else:
            decision = "PARK_MIGRATION_POOL_NOT_FOUND"
    else:
        signatures = unique_signatures(row)
        if signature_scan_limit > 0:
            for addr in (bonding_curve, associated, mint):
                for signature in scan_address_signatures(join, lane, rpc_url, addr, signature_scan_limit):
                    if signature not in signatures:
                        signatures.append(signature)
        tx_pool, migration_proof_signature, event_seen, migration_instruction, _pool_error = migration_pool_from_signatures(
            join,
            lane,
            rpc_url,
            mint,
            signatures,
        )
        if tx_pool and (migration_instruction or event_seen):
            pool_final = tx_pool
            pool_join_source = "MIGRATE_INSTRUCTION" if migration_instruction else "MIGRATION_EVENT"
            decision = "MIGRATION_TX_PROVEN" if migration_instruction else "MIGRATION_EVENT_PROVEN"
        else:
            derived_candidates = derived_pool_candidates(row)
            derived_pool = first_verified_pool(join, lane, rpc_url, derived_candidates, mint)
            if derived_pool:
                pool_final = derived_pool
                pool_join_source = "DERIVED_PDA"
                decision = "PUMPSWAP_POOL_DERIVED_BY_PDA"
            else:
                account_scan_pool, _scan_error = join.find_pool_from_program_accounts(lane, rpc_url, mint)
                if account_scan_pool:
                    pool_final = account_scan_pool
                    pool_join_source = "ACCOUNT_SCAN"
                    decision = "PUMPSWAP_POOL_FOUND_BY_ACCOUNT_SCAN"
                elif migration_instruction or event_seen:
                    decision = "PARK_MIGRATION_POOL_NOT_FOUND"
                elif complete:
                    decision = "CURVE_COMPLETE_NOT_MIGRATED"
                elif text(row.get("route_family")) == "PUMPSWAP_AMM":
                    decision = "EXISTING_PUMPSWAP_ACTIVITY_ONLY"
                else:
                    decision = "PARK_MIGRATION_STATE_UNRESOLVED"

    return {
        **row,
        "record_type": "launch0m2_migration_provenance_split",
        "creator": creator,
        "bonding_curve_complete": complete,
        "real_token_reserves": real_token_reserves,
        "migration_signature": migration_signature or migration_proof_signature,
        "migration_proof_signature": migration_proof_signature,
        "migration_instruction_seen": migration_instruction,
        "complete_pump_amm_migration_event_seen": event_seen,
        "event_pool": event_pool,
        "derived_pool_pda": derived_pool,
        "account_scan_pool": account_scan_pool,
        "pumpswap_pool_final": pool_final,
        "pumpswap_pool": pool_final,
        "pool_join_source": pool_join_source,
        "migration_provenance_decision": decision,
        "decision": decision,
        "first_migration_blocker": "" if pool_final else decision,
        "wire_send": 0,
        "live_landed": 0,
    }


def build_summary(args: argparse.Namespace, rows: list[dict[str, Any]]) -> dict[str, Any]:
    decision_counts = Counter(text(row.get("migration_provenance_decision")) for row in rows)
    source_counts = Counter(text(row.get("pool_join_source")) or "none" for row in rows)
    joined = sum(1 for row in rows if text(row.get("pumpswap_pool_final")))
    migration_proven = sum(
        1
        for row in rows
        if boolish(row.get("migration_instruction_seen")) or boolish(row.get("complete_pump_amm_migration_event_seen"))
    )
    status = "LAUNCH0M2_MIGRATION_PROVENANCE_SPLIT_INCOMPLETE"
    if migration_proven > 0 and joined > 0:
        status = "LAUNCH0M2_MIGRATION_PROVENANCE_SPLIT_PASS"
    elif rows and decision_counts.get("CURVE_COMPLETE_NOT_MIGRATED", 0) >= max(1, int(len(rows) * 0.8)):
        status = "PARK_COMPLETED_NOT_MIGRATED"
    return {
        "status": status,
        "run_root": text(args.run_root),
        "input_rows": text(args.quote_state_rows),
        "completed_or_migrated_input_rows": len(rows),
        "curve_complete_not_migrated_count": safe_int(decision_counts.get("CURVE_COMPLETE_NOT_MIGRATED")),
        "migration_tx_or_event_proven_count": migration_proven,
        "migration_tx_proven_count": safe_int(decision_counts.get("MIGRATION_TX_PROVEN")),
        "migration_event_proven_count": safe_int(decision_counts.get("MIGRATION_EVENT_PROVEN")),
        "pumpswap_pool_derived_by_pda_count": safe_int(decision_counts.get("PUMPSWAP_POOL_DERIVED_BY_PDA")),
        "pumpswap_pool_found_by_account_scan_count": safe_int(decision_counts.get("PUMPSWAP_POOL_FOUND_BY_ACCOUNT_SCAN")),
        "existing_pumpswap_activity_only_count": safe_int(decision_counts.get("EXISTING_PUMPSWAP_ACTIVITY_ONLY")),
        "pumpswap_pool_joined_count": joined,
        "pool_join_source_counts": dict(source_counts),
        "decision_counts": dict(decision_counts),
        "rows_path": text(args.rows_out),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": status == "LAUNCH0M2_MIGRATION_PROVENANCE_SPLIT_PASS",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Split complete bonding curves from proven PumpSwap migrations.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--quote-state-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--launch0m-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--signature-scan-limit", type=int, default=0)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_migration_provenance")
    join = load_module(args.launch0m_path, "launch0m_for_migration_provenance")
    quote_rows = load_jsonl(args.quote_state_rows)
    candidates = [row for row in quote_rows if text(row.get("first_quote_state_blocker")) == COMPLETE_BLOCKER]
    out_rows = [classify_row(row, join, lane, args.rpc_url, args.signature_scan_limit) for row in candidates]
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    summary = build_summary(
        argparse.Namespace(run_root=args.run_root, quote_state_rows=args.quote_state_rows, rows_out=args.out_dir / "rows.jsonl"),
        out_rows,
    )
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    print(f"status={summary['status']}")
    print(f"acceptance_pass={str(summary['acceptance_pass']).lower()}")
    print(f"completed_or_migrated_input_rows={summary['completed_or_migrated_input_rows']}")
    print(f"migration_tx_or_event_proven_count={summary['migration_tx_or_event_proven_count']}")
    print(f"pumpswap_pool_joined_count={summary['pumpswap_pool_joined_count']}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

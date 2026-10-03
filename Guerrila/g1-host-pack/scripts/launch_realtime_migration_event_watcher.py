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


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


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


def row_signature(row: dict[str, Any]) -> str:
    return text(row.get("target_signature") or row.get("signature") or row.get("source_signature"))


def list_field(row: dict[str, Any], key: str) -> list[str]:
    value = row.get(key)
    if isinstance(value, list):
        return [text(item) for item in value if text(item)]
    return []


def candidate_signatures(stream_rows: list[dict[str, Any]], activity_rows: list[dict[str, Any]], max_candidates: int) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for row in stream_rows:
        sig = row_signature(row)
        if not sig or sig in seen:
            continue
        programs = set(list_field(row, "target_program_ids"))
        keys = set(list_field(row, "target_account_keys"))
        if PUMP_PROGRAM_ID in programs and (PUMPFUN_AMM_PROGRAM_ID in programs or PUMPFUN_AMM_PROGRAM_ID in keys):
            seen.add(sig)
            out.append(sig)
    for row in activity_rows:
        sig = row_signature(row)
        if not sig or sig in seen:
            continue
        if text(row.get("decode_class")) == "PUMP_MIGRATION" or text(row.get("migration_signature")):
            seen.add(sig)
            out.append(sig)
    return out[:max_candidates] if max_candidates > 0 else out


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


def migration_event_seen(tx: dict[str, Any]) -> bool:
    logs = [text(item).lower() for item in (tx.get("meta") or {}).get("logMessages") or []]
    joined = "\n".join(logs)
    return "completepumpammmigrationevent" in joined or "complete_pump_amm_migration" in joined


def migration_instruction_seen(join: Any, tx: dict[str, Any]) -> bool:
    keys = join.account_keys_from_tx(tx)
    if PUMPFUN_AMM_PROGRAM_ID not in keys:
        return False
    for ix in join.tx_instructions(tx):
        if join.instruction_program_id(ix, keys) == PUMP_PROGRAM_ID:
            return True
    return False


def decode_pool_candidates(join: Any, lane: Any, rpc_url: str, tx: dict[str, Any]) -> list[dict[str, Any]]:
    keys = join.account_keys_from_tx(tx)
    raw_candidates: list[str] = []
    for ix in join.tx_instructions(tx):
        if join.instruction_program_id(ix, keys) == PUMPFUN_AMM_PROGRAM_ID:
            raw_candidates.extend(join.instruction_account_pubkeys(ix, keys))
    if not raw_candidates and PUMPFUN_AMM_PROGRAM_ID in keys:
        raw_candidates = keys
    blocked = {
        "",
        PUMP_PROGRAM_ID,
        PUMPFUN_AMM_PROGRAM_ID,
        WSOL_MINT,
        "11111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        "ComputeBudget111111111111111111111111111111",
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    }
    candidates = join.unique_preserve_order([item for item in raw_candidates if item not in blocked])
    if not candidates:
        return []
    accounts, _slot, error = lane.get_multiple_accounts(rpc_url, candidates)
    if error:
        return []
    out: list[dict[str, Any]] = []
    for candidate in candidates:
        pool_doc, pool_error = lane.decode_pumpfun_pool((accounts.get(candidate) or {}).get("data_raw", b""))
        if pool_error or not pool_doc:
            continue
        mint_set = {text(pool_doc.get("base_mint")), text(pool_doc.get("quote_mint"))}
        if WSOL_MINT not in mint_set:
            continue
        mint = next((item for item in mint_set if item and item != WSOL_MINT), "")
        if not mint:
            continue
        state, state_error = join.verify_pool(lane, rpc_url, candidate, mint)
        out.append(
            {
                "pumpswap_pool": candidate,
                "mint": mint,
                "pool_state_ok": bool(state and not state_error),
                "pool_state_error": state_error or "",
                "pumpswap_base_reserve": safe_int(state.get("core_reserve_raw")) if state else 0,
                "pumpswap_token_reserve": safe_int(state.get("launch_reserve_raw")) if state else 0,
            }
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="No-wire realtime Pump migration event/instruction watcher.")
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--stream-rows", type=Path, required=True)
    parser.add_argument("--legacy-stream-rows", type=Path)
    parser.add_argument("--new-stream-rows", type=Path)
    parser.add_argument("--compact-stream-rows", type=Path)
    parser.add_argument("--activity-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--launch0m-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--max-candidates", type=int, default=500)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_migration_watcher")
    join = load_module(args.launch0m_path, "launch0m_for_migration_watcher")
    stream_rows = load_jsonl(args.stream_rows)
    activity_rows = load_jsonl(args.activity_rows)
    signatures = candidate_signatures(stream_rows, activity_rows, args.max_candidates)
    out_rows: list[dict[str, Any]] = []
    decision_counts: Counter[str] = Counter()
    for signature in signatures:
        tx, tx_error = rpc_tx(join, lane, args.rpc_url, signature)
        event_seen = bool(tx) and migration_event_seen(tx)
        instruction_seen = bool(tx) and migration_instruction_seen(join, tx)
        pools = decode_pool_candidates(join, lane, args.rpc_url, tx) if tx else []
        if pools and event_seen:
            decision = "MIGRATION_EVENT_POOL_JOINED"
        elif pools and instruction_seen:
            decision = "MIGRATE_INSTRUCTION_POOL_JOINED"
        elif instruction_seen or event_seen:
            decision = "PARK_PUMPSWAP_POOL_NOT_FOUND"
        else:
            decision = "PARK_MIGRATION_EVENT_MISSING"
        decision_counts[decision] += 1
        base = {
            "record_type": "launch0m3_realtime_migration_event_watcher",
            "signature": signature,
            "migration_instruction_seen": instruction_seen,
            "complete_pump_amm_migration_event_seen": event_seen,
            "pool_join_source": "MIGRATION_EVENT" if event_seen and pools else "MIGRATE_INSTRUCTION" if instruction_seen and pools else "",
            "decision": decision,
            "tx_error": tx_error,
            "wire_send": 0,
            "live_landed": 0,
        }
        if pools:
            for pool in pools:
                out_rows.append({**base, **pool, "route_family": "pumpswap_after_migration"})
        else:
            out_rows.append(base)

    joined_rows = [row for row in out_rows if text(row.get("pumpswap_pool"))]
    quote_ready_rows = [row for row in joined_rows if safe_int(row.get("pumpswap_base_reserve")) > 0 and safe_int(row.get("pumpswap_token_reserve")) > 0]
    migration_seen = [row for row in out_rows if row.get("migration_instruction_seen") or row.get("complete_pump_amm_migration_event_seen")]
    status = "LAUNCH0M3_REALTIME_MIGRATION_EVENT_WATCHER_PASS" if migration_seen and joined_rows else "PARK_MIGRATION_EVENT_MISSING"
    if migration_seen and not joined_rows:
        status = "PARK_PUMPSWAP_POOL_NOT_FOUND"
    summary = {
        "status": status,
        "run_root": str(args.run_root),
        "migration_stream_path_used": str(args.stream_rows),
        "migration_stream_path_exists": args.stream_rows.exists(),
        "legacy_stream_path_exists": bool(args.legacy_stream_rows and args.legacy_stream_rows.exists()),
        "new_stream_path_exists": bool(args.new_stream_rows and args.new_stream_rows.exists()),
        "compact_stream_path_exists": bool(args.compact_stream_rows and args.compact_stream_rows.exists()),
        "compact_stream_path": str(args.compact_stream_rows or ""),
        "migration_stream_rows_read": len(stream_rows),
        "migration_activity_rows": len(out_rows),
        "candidate_signatures": len(signatures),
        "migrate_instruction_seen_count": sum(1 for row in out_rows if row.get("migration_instruction_seen")),
        "migration_event_seen_count": sum(1 for row in out_rows if row.get("complete_pump_amm_migration_event_seen")),
        "pumpswap_pool_joined_count": len(joined_rows),
        "pumpswap_quote_state_ready_rows": len(quote_ready_rows),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": status == "LAUNCH0M3_REALTIME_MIGRATION_EVENT_WATCHER_PASS",
    }
    write_jsonl(args.out_dir / "rows.jsonl", out_rows)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in (
        "status",
        "acceptance_pass",
        "migration_activity_rows",
        "migrate_instruction_seen_count",
        "migration_event_seen_count",
        "pumpswap_pool_joined_count",
        "pumpswap_quote_state_ready_rows",
        "wire_send",
        "live_landed",
    ):
        value = summary[key]
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

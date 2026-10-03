#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
from collections import Counter
from pathlib import Path
from typing import Any


PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_BC_BUY = bytes([102, 6, 61, 18, 1, 218, 235, 234])
PUMP_BC_SELL = bytes([51, 230, 133, 164, 1, 127, 131, 173])
B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


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
    if not path or not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            item = json.loads(raw)
        except Exception:
            continue
        if isinstance(item, dict):
            rows.append(item)
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


def b58decode(raw: str) -> bytes:
    num = 0
    for char in raw:
        num *= 58
        idx = B58_ALPHABET.find(char)
        if idx < 0:
            raise ValueError("invalid_base58")
        num += idx
    data = num.to_bytes((num.bit_length() + 7) // 8, "big") if num else b""
    pad = 0
    for char in raw:
        if char == "1":
            pad += 1
        else:
            break
    return b"\x00" * pad + data


def le_u64(data: bytes, offset: int) -> int:
    if len(data) < offset + 8:
        return 0
    return int.from_bytes(data[offset:offset + 8], "little")


def account_keys_from_tx(tx: dict[str, Any]) -> list[str]:
    message = (((tx.get("transaction") or {}).get("message")) or {})
    keys: list[str] = []
    for item in message.get("accountKeys") or []:
        keys.append(text(item.get("pubkey")) if isinstance(item, dict) else text(item))
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
    return keys[idx] if 0 <= idx < len(keys) else ""


def instruction_accounts(ix: dict[str, Any], keys: list[str]) -> list[str]:
    out: list[str] = []
    for item in ix.get("accounts") or []:
        if isinstance(item, int):
            if 0 <= item < len(keys):
                out.append(keys[item])
        else:
            out.append(text(item))
    return out


def instruction_data(ix: dict[str, Any]) -> bytes:
    value = ix.get("data")
    if isinstance(value, list) and value:
        value = value[0]
    if isinstance(value, str):
        try:
            return b58decode(value)
        except Exception:
            return b""
    return b""


def rpc_call(lane: Any, rpc_url: str, method: str, params: list[Any]) -> tuple[Any, str]:
    response, error = lane.rpc(rpc_url, method, params)
    if error is not None:
        return None, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    return (response or {}).get("result", response), ""


def fetch_signatures(lane: Any, rpc_url: str, address: str, limit: int) -> tuple[list[dict[str, Any]], str]:
    result, error = rpc_call(lane, rpc_url, "getSignaturesForAddress", [address, {"limit": max(1, limit)}])
    if error or not isinstance(result, list):
        return [], error or "signature_fetch_failed"
    return [row for row in result if isinstance(row, dict)], ""


def fetch_tx(lane: Any, rpc_url: str, signature: str) -> tuple[dict[str, Any], str]:
    result, error = rpc_call(
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
    if error or not isinstance(result, dict):
        return {}, error or "tx_fetch_failed"
    return result, ""


def target_ref_time(row: dict[str, Any]) -> int:
    ns_values = [
        safe_int(row.get("birth_seen_ns")),
        safe_int(row.get("first_sell_seen_ns")),
        safe_int(row.get("first_buy_seen_ns")),
        safe_int(row.get("source_seen_ns")),
        safe_int(row.get("create_seen_ns")),
    ]
    ns = max(ns_values)
    return int(ns / 1_000_000_000) if ns > 0 else 0


def decode_trade_events(tx: dict[str, Any], signature: str, block_time: int, target: dict[str, Any]) -> list[dict[str, Any]]:
    keys = account_keys_from_tx(tx)
    target_mint = text(target.get("mint"))
    target_curve = text(target.get("bonding_curve"))
    target_associated = text(target.get("associated_bonding_curve"))
    out: list[dict[str, Any]] = []
    for ix in tx_instructions(tx):
        if instruction_program_id(ix, keys) != PUMP_PROGRAM_ID:
            continue
        data = instruction_data(ix)
        accounts = instruction_accounts(ix, keys)
        side = ""
        mint = ""
        curve = ""
        associated = ""
        sol_amount = 0
        token_amount = 0
        if data[:8] == PUMP_BC_BUY:
            side = "buy"
            token_amount = le_u64(data, 8)
            sol_amount = le_u64(data, 16)
            buy_v2 = len(accounts) >= 27
            mint = text(accounts[1]) if buy_v2 and len(accounts) > 1 else text(accounts[2]) if len(accounts) > 2 else ""
            curve = text(accounts[10]) if buy_v2 and len(accounts) > 10 else text(accounts[3]) if len(accounts) > 3 else ""
            associated = text(accounts[11]) if buy_v2 and len(accounts) > 11 else text(accounts[4]) if len(accounts) > 4 else ""
        elif data[:8] == PUMP_BC_SELL:
            side = "sell"
            token_amount = le_u64(data, 8)
            sol_amount = le_u64(data, 16)
            sell_v2 = len(accounts) >= 26
            mint = text(accounts[1]) if sell_v2 and len(accounts) > 1 else text(accounts[2]) if len(accounts) > 2 else ""
            curve = text(accounts[10]) if sell_v2 and len(accounts) > 10 else text(accounts[3]) if len(accounts) > 3 else ""
            associated = text(accounts[11]) if sell_v2 and len(accounts) > 11 else text(accounts[4]) if len(accounts) > 4 else ""
        if not side:
            continue
        if not (
            (target_mint and mint == target_mint)
            or (target_curve and curve == target_curve)
            or (target_associated and associated == target_associated)
        ):
            continue
        out.append(
            {
                "signature": signature,
                "block_time": block_time,
                "mint": mint,
                "bonding_curve": curve,
                "associated_bonding_curve": associated,
                "side": side,
                "sol_amount": sol_amount,
                "token_amount": token_amount,
            }
        )
    return out


def resolve_one(lane: Any, rpc_url: str, target: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    addresses = []
    for value in (target.get("bonding_curve"), target.get("associated_bonding_curve"), target.get("mint")):
        value_text = text(value)
        if value_text and value_text not in addresses:
            addresses.append(value_text)
    all_sigs: dict[str, dict[str, Any]] = {}
    errors: list[str] = []
    for address in addresses:
        sigs, error = fetch_signatures(lane, rpc_url, address, args.max_signatures)
        if error:
            errors.append(f"{address}:{error}")
        for row in sigs:
            sig = text(row.get("signature"))
            if sig:
                all_sigs.setdefault(sig, row)
    reference_time = target_ref_time(target)
    min_time = reference_time - int(args.max_age_seconds) if reference_time and args.max_age_seconds > 0 else 0
    candidates = sorted(
        all_sigs.values(),
        key=lambda row: safe_int(row.get("blockTime")),
        reverse=True,
    )
    if min_time:
        candidates = [row for row in candidates if safe_int(row.get("blockTime")) >= min_time]
    tx_scanned = 0
    events: list[dict[str, Any]] = []
    for sig_doc in candidates:
        if tx_scanned >= args.max_tx_scan:
            break
        signature = text(sig_doc.get("signature"))
        if not signature:
            continue
        tx, error = fetch_tx(lane, rpc_url, signature)
        tx_scanned += 1
        if error or not tx:
            errors.append(f"{signature}:{error}")
            continue
        events.extend(decode_trade_events(tx, signature, safe_int(sig_doc.get("blockTime")), target))
    buys = sorted([event for event in events if text(event.get("side")) == "buy"], key=lambda row: safe_int(row.get("block_time")), reverse=True)
    sells = sorted([event for event in events if text(event.get("side")) == "sell"], key=lambda row: safe_int(row.get("block_time")), reverse=True)
    latest_buy = buys[0] if buys else {}
    decision = "PRE_MIGRATION_HISTORY_BUY_RESOLVED"
    blocker = "none"
    if not addresses:
        decision, blocker = "PARK_HISTORY_TARGET_MISSING", "address"
    elif not all_sigs:
        decision, blocker = "PARK_HISTORY_MISSING", "signatures"
    elif not events and errors:
        decision, blocker = "PARK_HISTORY_TRADE_DECODE_FAILED", "trade_decode"
    elif not buys and sells:
        decision, blocker = "PARK_HISTORY_SELL_DERIVED_ONLY", "recent_buy"
    elif not buys:
        decision, blocker = "PARK_HISTORY_NO_RECENT_BUY", "recent_buy"
    return {
        "record_type": "launch61e_pre_migration_bounded_history_fetch",
        "mint": text(target.get("mint")),
        "bonding_curve": text(target.get("bonding_curve")),
        "associated_bonding_curve": text(target.get("associated_bonding_curve")),
        "curve_progress_pct": safe_float(target.get("curve_progress_pct")),
        "estimated_sol_to_completion_lamports": safe_int(target.get("estimated_sol_to_completion_lamports")),
        "addresses_scanned": addresses,
        "signatures_seen": len(all_sigs),
        "tx_scanned": tx_scanned,
        "trade_events_seen": len(events),
        "buy_count": len(buys),
        "sell_count": len(sells),
        "latest_buy_signature": text(latest_buy.get("signature")),
        "latest_buy_time": safe_int(latest_buy.get("block_time")),
        "latest_buy_sol": safe_int(latest_buy.get("sol_amount")),
        "latest_buy_token_out": safe_int(latest_buy.get("token_amount")),
        "pre_migration_buy_resolved": bool(decision == "PRE_MIGRATION_HISTORY_BUY_RESOLVED"),
        "history_fetch_decision": decision,
        "first_blocker": blocker,
        "rpc_error_sample": ";".join(errors[:3]),
        "wire_send": 0,
        "live_landed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Bounded RPC/history fetch for pre-migration Pump buy events.")
    parser.add_argument("--completion-rows", type=Path, required=True)
    parser.add_argument("--launch-lane-path", type=Path, required=True)
    parser.add_argument("--rpc-url", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--min-progress-pct", type=float, default=90.0)
    parser.add_argument("--max-signatures", type=int, default=150)
    parser.add_argument("--max-tx-scan", type=int, default=80)
    parser.add_argument("--max-age-seconds", type=float, default=1800)
    args = parser.parse_args()

    lane = load_module(args.launch_lane_path, "launch_follow_lane_for_pre_migration_history")
    targets = [row for row in load_jsonl(args.completion_rows) if safe_float(row.get("curve_progress_pct")) >= args.min_progress_pct]
    rows = [resolve_one(lane, args.rpc_url, target, args) for target in targets]
    decision_counts = Counter(text(row.get("history_fetch_decision")) for row in rows)
    resolved = [row for row in rows if boolish(row.get("pre_migration_buy_resolved"))]
    status = "LAUNCH61E_PRE_MIGRATION_BOUNDED_HISTORY_FETCH_PASS" if resolved else "PARK_LAUNCH61E_BUY_HISTORY_NOT_RESOLVED"
    summary = {
        "status": status,
        "completion_rows": str(args.completion_rows),
        "eligible_completion_rows": len(targets),
        "history_rows": len(rows),
        "pre_migration_buy_resolved_rows": len(resolved),
        "decision_counts": dict(decision_counts),
        "rows_path": str(args.out_dir / "rows.jsonl"),
        "resolved_rows_path": str(args.out_dir / "resolved_rows.jsonl"),
        "wire_send": 0,
        "live_landed": 0,
        "acceptance_pass": bool(resolved),
    }
    write_jsonl(args.out_dir / "rows.jsonl", rows)
    write_jsonl(args.out_dir / "resolved_rows.jsonl", resolved)
    write_json(args.out_dir / "summary.json", summary)
    write_txt(args.out_dir / "summary.txt", summary)
    for key in ("status", "eligible_completion_rows", "pre_migration_buy_resolved_rows", "wire_send", "live_landed"):
        print(f"{key}={summary[key]}")
    print(f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

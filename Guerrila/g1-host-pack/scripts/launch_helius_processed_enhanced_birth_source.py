#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import time
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any


PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMPFUN_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
METEORA_DAMM_V2_PROGRAM_ID = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2tibnN3cH5iWkD7rP8J7u"
CORE_MINTS = {WSOL_MINT, USDC_MINT, USDT_MINT}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except Exception:
        return default


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


def http_get_json(url: str, timeout: int) -> tuple[Any | None, str]:
    try:
        req = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "shadow-launch0p/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8", errors="replace")), ""
    except Exception as exc:
        return None, str(exc)


def transaction_mints(tx: dict[str, Any]) -> set[str]:
    mints: set[str] = set()
    for transfer in tx.get("tokenTransfers") or []:
        mint = text(transfer.get("mint"))
        if mint and mint not in CORE_MINTS:
            mints.add(mint)
    for event in (tx.get("events") or {}).values():
        if isinstance(event, dict):
            for key in ("mint", "tokenMint", "baseMint", "quoteMint"):
                mint = text(event.get(key))
                if mint and mint not in CORE_MINTS:
                    mints.add(mint)
    return mints


def fetch_address_transactions(api_key: str, address: str, limit: int, timeout: int) -> tuple[list[dict[str, Any]], str]:
    params = urllib.parse.urlencode({"api-key": api_key, "limit": limit})
    url = f"https://api.helius.xyz/v0/addresses/{urllib.parse.quote(address)}/transactions?{params}"
    payload, error = http_get_json(url, timeout)
    if error:
        return [], error
    if not isinstance(payload, list):
        return [], "unexpected_response"
    return [row for row in payload if isinstance(row, dict)], ""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--api-key", required=True)
    parser.add_argument("--addresses-csv", default=",".join([PUMP_BONDING_CURVE_PROGRAM_ID, PUMPFUN_AMM_PROGRAM_ID, METEORA_DAMM_V2_PROGRAM_ID]))
    parser.add_argument("--limit-per-address", default="100")
    parser.add_argument("--timeout-seconds", default="30")
    parser.add_argument("--rows-jsonl", type=Path, required=True)
    parser.add_argument("--summary-json", type=Path, required=True)
    parser.add_argument("--summary-txt", type=Path, required=True)
    args = parser.parse_args()

    addresses = [item.strip() for item in text(args.addresses_csv).split(",") if item.strip()]
    limit = max(1, min(100, safe_int(args.limit_per_address, 100)))
    timeout = max(1, safe_int(args.timeout_seconds, 30))
    rows: list[dict[str, Any]] = []
    unique_mints: set[str] = set()
    address_counts: Counter[str] = Counter()
    errors: dict[str, str] = {}
    start = int(time.time())

    for address in addresses:
        txs, error = fetch_address_transactions(args.api_key, address, limit, timeout)
        if error:
            errors[address] = error
        address_counts[address] = len(txs)
        for tx in txs:
            mints = sorted(transaction_mints(tx))
            unique_mints.update(mints)
            rows.append(
                {
                    "record_type": "launch0p_helius_enhanced_tx",
                    "source_name": "Helius raw/enhanced create history",
                    "program_address": address,
                    "signature": text(tx.get("signature")),
                    "timestamp": safe_int(tx.get("timestamp")),
                    "type": text(tx.get("type")),
                    "source": text(tx.get("source")),
                    "description": text(tx.get("description")),
                    "non_core_mints": mints,
                    "mint_count": len(mints),
                }
            )

    summary = {
        "status": "launch0p_helius_processed_enhanced_birth_source_complete" if not errors else "launch0p_helius_processed_enhanced_birth_source_partial",
        "run_root": text(args.run_root),
        "source_name": "Helius raw/enhanced create history",
        "address_count": len(addresses),
        "limit_per_address": limit,
        "create_count": len(rows),
        "decoded_count": len(unique_mints),
        "decoded_launches": len(unique_mints),
        "unique_mints": len(unique_mints),
        "unique_mints_seen": len(unique_mints),
        "address_transaction_counts": dict(address_counts),
        "errors": errors,
        "first_error": next(iter(errors.values()), ""),
        "elapsed_seconds": int(time.time()) - start,
        "source_paths": {
            "rows_jsonl": text(args.rows_jsonl),
        },
        "wire_send": 0,
        "live_landed": 0,
    }
    write_jsonl(args.rows_jsonl, rows)
    write_json(args.summary_json, summary)
    write_txt(args.summary_txt, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

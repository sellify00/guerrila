#!/usr/bin/env bash
# 80A - build a volatility-qualified external Solana hunting universe

source "$(dirname "$0")/lib.sh"
script_header "80A - Volatility Habitat Universe Builder"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN80A_STEP:-epoch8_auto_volatility_habitat_run80a_external_universe_builder}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run80a_volatility_habitat_universe_builder.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

SEED_TOKENS_CSV="${SHADOW_RUN80A_SEED_TOKENS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2,7xKXtg2CW1dsoGqYJcW5K63Vf6kRrK4r5xL2U5Fj4Y9X,DezXAZ8z7PnrnRJjz3wXBoRgixCa6YaB1pPB263Kpump,4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R}"
TOP_N="${SHADOW_RUN80A_TOP_N:-180}"
MIN_LIQUIDITY_USD="${SHADOW_RUN80A_MIN_LIQUIDITY_USD:-25000}"
MIN_TXNS_M5="${SHADOW_RUN80A_MIN_TXNS_M5:-20}"
RPC_URL="${SHADOW_RUN80A_RPC_URL:-$(default_http_rpc_url)}"
INVENTORY_PATH="${SHADOW_RUN80A_SIGNER_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_PATH="${SHADOW_RUN80A_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
BIRDEYE_JSON_PATH="${SHADOW_RUN80A_BIRDEYE_JSON_PATH:-}"
BITQUERY_JSON_PATH="${SHADOW_RUN80A_BITQUERY_JSON_PATH:-}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$SEED_TOKENS_CSV" "$TOP_N" "$MIN_LIQUIDITY_USD" "$MIN_TXNS_M5" "$RPC_URL" \
    "$INVENTORY_PATH" "$SIGNER_PATH" "$BIRDEYE_JSON_PATH" "$BITQUERY_JSON_PATH" \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import math
import ssl
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

seed_tokens = [item.strip() for item in sys.argv[1].split(",") if item.strip()]
top_n = int(sys.argv[2])
min_liquidity_usd = float(sys.argv[3])
min_txns_m5 = int(sys.argv[4])
rpc_url = sys.argv[5]
inventory_path = Path(sys.argv[6]) if sys.argv[6] else None
signer_path = Path(sys.argv[7]) if sys.argv[7] else None
birdeye_json_path = Path(sys.argv[8]) if sys.argv[8] else None
bitquery_json_path = Path(sys.argv[9]) if sys.argv[9] else None
summary_path = Path(sys.argv[10])
summary_json_path = Path(sys.argv[11])
rows_jsonl_path = Path(sys.argv[12])
rows_csv_path = Path(sys.argv[13])

TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN_2022_PROGRAM_ID = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ATA_PROGRAM_ID = "ATokenGPvbdGVxr1b2hvZbsiW5xWH25efTNsLJA8knL"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2"
RAY_MINT = "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R"
CORE_MINTS = {WSOL_MINT, USDC_MINT, USDT_MINT, RAY_MINT}
SUPPORTED_VENUES = {
    "raydium_cpmm",
    "raydium_amm_v4",
    "raydium_clmm",
    "meteora_damm_v2",
    "orca_whirlpool",
}
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def clamp(value: float, lo: float = 0.0, hi: float = 1.0) -> float:
    return max(lo, min(hi, value))


def read_text_via_sudo_or_open(path: Path | None) -> str | None:
    if path is None:
        return None
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        try:
            return subprocess.check_output(["sudo", "cat", str(path)], text=True)
        except Exception:
            return None


def b58decode(text: str) -> bytes:
    value = 0
    for char in text.strip():
        value = value * 58 + BASE58_ALPHABET.index(char)
    raw = value.to_bytes((value.bit_length() + 7) // 8, "big") if value else b""
    pad = len(text) - len(text.lstrip("1"))
    return (b"\x00" * pad) + raw


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        rpc_url,
        data=data,
        headers={"Content-Type": "application/json", "User-Agent": "shadow-run80a/1.0"},
    )
    if rpc_url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def http_json(url: str):
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Mozilla/5.0 shadow-run80a",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def score_log(value: float, low: float, high: float) -> float:
    value = max(value, 0.0)
    if value <= 0.0:
        return 0.0
    low = max(low, 1e-9)
    high = max(high, low * 1.0001)
    numerator = math.log10(max(value, low)) - math.log10(low)
    denominator = math.log10(high) - math.log10(low)
    return clamp(numerator / denominator if denominator else 0.0)


def volatility_score(row: dict) -> float:
    m5 = abs(safe_float((row.get("priceChange") or {}).get("m5")))
    h1 = abs(safe_float((row.get("priceChange") or {}).get("h1")))
    h24 = abs(safe_float((row.get("priceChange") or {}).get("h24")))
    weighted = max(m5, h1 * 0.65, h24 * 0.20)
    return clamp(weighted / 8.0)


def map_venue_family(dex_id: str, labels: list[str]) -> str:
    dex = (dex_id or "").strip().lower()
    label_set = {str(label or "").strip().lower() for label in labels}
    if dex == "raydium":
        if "clmm" in label_set:
            return "raydium_clmm"
        if "cpmm" in label_set:
            return "raydium_cpmm"
        return "raydium_amm_v4"
    if dex == "meteora":
        return "meteora_damm_v2"
    if dex == "orca":
        return "orca_whirlpool"
    return "unknown"


def slippage_proxy_bps_1k(liquidity_usd: float) -> float | None:
    if liquidity_usd <= 0:
        return None
    return round(min(10_000.0, (5_000.0 * 1_000.0) / liquidity_usd), 2)


def load_inventory() -> tuple[dict[str, list[dict]], str | None]:
    entries_by_mint: dict[str, list[dict]] = {}
    owner_pubkey = None
    raw_inventory = read_text_via_sudo_or_open(inventory_path)
    raw_signer = read_text_via_sudo_or_open(signer_path)
    if raw_signer:
        try:
            arr = json.loads(raw_signer)
            if isinstance(arr, list) and len(arr) >= 64:
                raw = bytes(int(value) for value in arr[-32:])
                value = int.from_bytes(raw, "big")
                encoded = ""
                while value:
                    value, rem = divmod(value, 58)
                    encoded = BASE58_ALPHABET[rem] + encoded
                pad = 0
                for byte in raw:
                    if byte == 0:
                        pad += 1
                    else:
                        break
                owner_pubkey = ("1" * pad) + (encoded or "1")
        except Exception:
            owner_pubkey = None
    if raw_inventory:
        try:
            payload = json.loads(raw_inventory)
            for entry in payload.get("entries") or []:
                mint = str(entry.get("mint_pubkey") or "").strip()
                if not mint:
                    continue
                entries_by_mint.setdefault(mint, []).append(entry)
        except Exception:
            entries_by_mint = {}
    return entries_by_mint, owner_pubkey


def load_dexscreener_universe() -> tuple[list[dict], list[str]]:
    rows: list[dict] = []
    source_notes: list[str] = []
    seen_pairs: set[str] = set()
    for token in seed_tokens:
        url = f"https://api.dexscreener.com/token-pairs/v1/solana/{urllib.parse.quote(token)}"
        try:
            payload = http_json(url)
            source_notes.append(f"dexscreener_seed_ok:{token}:{len(payload)}")
        except Exception as exc:
            source_notes.append(f"dexscreener_seed_fail:{token}:{exc}")
            continue
        for row in payload:
            pair_address = str(row.get("pairAddress") or "").strip()
            if not pair_address or pair_address in seen_pairs:
                continue
            seen_pairs.add(pair_address)
            rows.append(row)
        time.sleep(0.15)
    return rows, source_notes


def maybe_load_optional_json(path: Path | None) -> list[dict]:
    if path is None or not str(path):
        return []
    if not path.exists():
        return []
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return []
    if isinstance(payload, list):
        return payload
    if isinstance(payload, dict):
        for key in ("data", "rows", "pairs", "result"):
            value = payload.get(key)
            if isinstance(value, list):
                return value
    return []


def normalize_optional_rows(rows: list[dict], source_name: str) -> list[dict]:
    out = []
    for row in rows:
        pair_address = str(
            row.get("pairAddress")
            or row.get("pool")
            or row.get("poolAddress")
            or row.get("address")
            or ""
        ).strip()
        dex_id = str(row.get("dexId") or row.get("venue") or row.get("dex") or "").strip().lower()
        base = row.get("baseToken") or {
            "address": row.get("base_mint") or row.get("baseMint"),
            "symbol": row.get("base_symbol") or row.get("baseSymbol"),
            "name": row.get("base_name") or row.get("baseName"),
        }
        quote = row.get("quoteToken") or {
            "address": row.get("quote_mint") or row.get("quoteMint"),
            "symbol": row.get("quote_symbol") or row.get("quoteSymbol"),
            "name": row.get("quote_name") or row.get("quoteName"),
        }
        if not pair_address or not base.get("address") or not quote.get("address"):
            continue
        out.append(
            {
                "chainId": "solana",
                "dexId": dex_id,
                "pairAddress": pair_address,
                "labels": row.get("labels") or [],
                "baseToken": base,
                "quoteToken": quote,
                "txns": row.get("txns") or {},
                "volume": row.get("volume") or {},
                "priceChange": row.get("priceChange") or {},
                "liquidity": row.get("liquidity") or {},
                "info": row.get("info") or {},
                "_source_name": source_name,
            }
        )
    return out


def classify_mint_policy(mint: str, meta: dict | None) -> str:
    if mint in CORE_MINTS:
        return "core_allowlist"
    if not meta:
        return "unknown_mint_meta"
    owner = str(meta.get("owner") or "")
    info = ((meta.get("data") or {}).get("parsed") or {}).get("info") or {}
    if owner == TOKEN_2022_PROGRAM_ID:
        return "token2022_explicit_allow_required"
    if owner != TOKEN_PROGRAM_ID:
        return "unsupported_token_program"
    if info.get("freezeAuthority"):
        return "freeze_authority_present"
    if info.get("mintAuthority"):
        return "mint_authority_present"
    return "pass"


def combine_policy(base_policy: str, quote_policy: str) -> str:
    priority = [
        "token2022_explicit_allow_required",
        "unsupported_token_program",
        "freeze_authority_present",
        "mint_authority_present",
        "unknown_mint_meta",
    ]
    for value in priority:
        if base_policy == value or quote_policy == value:
            return value
    return "pass"


def account_readiness_status(base_mint: str, quote_mint: str, entries_by_mint: dict[str, list[dict]]) -> str:
    if base_mint in CORE_MINTS and quote_mint in CORE_MINTS:
        return "core_pair_ready"
    if base_mint in entries_by_mint or quote_mint in entries_by_mint:
        return "signer_inventory_present"
    return "no_inventory_match"


def account_readiness_score(status: str) -> float:
    return {
        "core_pair_ready": 1.0,
        "signer_inventory_present": 0.75,
        "no_inventory_match": 0.35,
    }.get(status, 0.35)


raw_rows, source_notes = load_dexscreener_universe()
birdeye_rows = normalize_optional_rows(maybe_load_optional_json(birdeye_json_path), "birdeye_json")
bitquery_rows = normalize_optional_rows(maybe_load_optional_json(bitquery_json_path), "bitquery_json")
all_rows = list(raw_rows)
seen_pair_addresses = {str(row.get("pairAddress") or "") for row in raw_rows}
for row in birdeye_rows + bitquery_rows:
    pair_address = str(row.get("pairAddress") or "")
    if pair_address and pair_address not in seen_pair_addresses:
        seen_pair_addresses.add(pair_address)
        all_rows.append(row)

entries_by_mint, signer_owner_pubkey = load_inventory()

mint_addresses = sorted(
    {
        str((row.get("baseToken") or {}).get("address") or "")
        for row in all_rows
    }
    | {
        str((row.get("quoteToken") or {}).get("address") or "")
        for row in all_rows
    }
)
mint_addresses = [mint for mint in mint_addresses if mint]

mint_meta: dict[str, dict | None] = {}
chunk_size = 100
for offset in range(0, len(mint_addresses), chunk_size):
    chunk = mint_addresses[offset : offset + chunk_size]
    try:
        response = rpc(
            "getMultipleAccounts",
            [chunk, {"encoding": "jsonParsed", "commitment": "confirmed"}],
        )
        values = (((response.get("result") or {}).get("value")) or [])
        for mint, meta in zip(chunk, values):
            mint_meta[mint] = meta
    except Exception:
        for mint in chunk:
            mint_meta[mint] = None


normalized_rows: list[dict] = []
for row in all_rows:
    if str(row.get("chainId") or "") != "solana":
        continue
    base = row.get("baseToken") or {}
    quote = row.get("quoteToken") or {}
    base_mint = str(base.get("address") or "").strip()
    quote_mint = str(quote.get("address") or "").strip()
    pair_address = str(row.get("pairAddress") or "").strip()
    if not base_mint or not quote_mint or not pair_address:
        continue

    liquidity_usd = safe_float((row.get("liquidity") or {}).get("usd"))
    tx_count_m5 = safe_int(((row.get("txns") or {}).get("m5") or {}).get("buys")) + safe_int(
        ((row.get("txns") or {}).get("m5") or {}).get("sells")
    )
    if liquidity_usd < min_liquidity_usd or tx_count_m5 < min_txns_m5:
        continue

    volume_m5 = safe_float((row.get("volume") or {}).get("m5"))
    volume_h1 = safe_float((row.get("volume") or {}).get("h1"))
    price_change_m5 = safe_float((row.get("priceChange") or {}).get("m5"))
    price_change_h1 = safe_float((row.get("priceChange") or {}).get("h1"))

    venue_family = map_venue_family(str(row.get("dexId") or ""), row.get("labels") or [])
    base_policy = classify_mint_policy(base_mint, mint_meta.get(base_mint))
    quote_policy = classify_mint_policy(quote_mint, mint_meta.get(quote_mint))
    pair_policy = combine_policy(base_policy, quote_policy)
    readiness_status = account_readiness_status(base_mint, quote_mint, entries_by_mint)

    liquidity_score = score_log(liquidity_usd, min_liquidity_usd, 2_500_000.0)
    event_rate_score = score_log(tx_count_m5, min_txns_m5, 500.0)
    turnover_score = clamp((volume_m5 / liquidity_usd) / 0.35) if liquidity_usd > 0 else 0.0
    venue_supported_score = 1.0 if venue_family in SUPPORTED_VENUES else 0.0
    exit_liquidity_score = 1.0 if quote_mint in CORE_MINTS else clamp(score_log(liquidity_usd, 50_000.0, 2_500_000.0))
    policy_clean_score = 1.0 if pair_policy == "pass" else 0.0
    readiness_score = account_readiness_score(readiness_status)
    ext_score = (
        volatility_score(row)
        * liquidity_score
        * event_rate_score
        * max(turnover_score, 0.10)
        * venue_supported_score
        * exit_liquidity_score
        * max(readiness_score, 0.10)
        * policy_clean_score
    )

    normalized_rows.append(
        {
            "pair": pair_address,
            "pair_hex": b58decode(pair_address).hex(),
            "source_names": sorted(
                {
                    str(row.get("_source_name") or "dexscreener")
                }
            ),
            "venue": venue_family,
            "dex_id": str(row.get("dexId") or ""),
            "labels": row.get("labels") or [],
            "base_mint": base_mint,
            "base_symbol": str(base.get("symbol") or ""),
            "quote_mint": quote_mint,
            "quote_symbol": str(quote.get("symbol") or ""),
            "liquidity_usd": round(liquidity_usd, 2),
            "volume_5m": round(volume_m5, 2),
            "volume_1h": round(volume_h1, 2),
            "price_change_5m": round(price_change_m5, 4),
            "price_change_1h": round(price_change_h1, 4),
            "tx_count_5m": tx_count_m5,
            "spread_estimate_bps": None,
            "slippage_estimate_bps_1k_notional": slippage_proxy_bps_1k(liquidity_usd),
            "token_policy_status": pair_policy,
            "base_policy_status": base_policy,
            "quote_policy_status": quote_policy,
            "account_readiness_status": readiness_status,
            "external_volatility_score": round(volatility_score(row), 6),
            "external_liquidity_score": round(liquidity_score, 6),
            "external_event_rate_score": round(event_rate_score, 6),
            "external_turnover_score": round(turnover_score, 6),
            "external_exit_liquidity_score": round(exit_liquidity_score, 6),
            "venue_supported_score": round(venue_supported_score, 6),
            "policy_clean_score": round(policy_clean_score, 6),
            "account_readiness_score": round(readiness_score, 6),
            "external_habitat_score": round(ext_score, 6),
        }
    )

normalized_rows.sort(
    key=lambda row: (
        row["external_habitat_score"],
        row["liquidity_usd"],
        row["tx_count_5m"],
        row["volume_5m"],
    ),
    reverse=True,
)
normalized_rows = normalized_rows[:top_n]

fieldnames = list(normalized_rows[0].keys()) if normalized_rows else [
    "pair",
    "pair_hex",
    "source_names",
    "venue",
    "dex_id",
    "labels",
    "base_mint",
    "base_symbol",
    "quote_mint",
    "quote_symbol",
    "liquidity_usd",
    "volume_5m",
    "volume_1h",
    "price_change_5m",
    "price_change_1h",
    "tx_count_5m",
    "spread_estimate_bps",
    "slippage_estimate_bps_1k_notional",
    "token_policy_status",
    "base_policy_status",
    "quote_policy_status",
    "account_readiness_status",
    "external_volatility_score",
    "external_liquidity_score",
    "external_event_rate_score",
    "external_turnover_score",
    "external_exit_liquidity_score",
    "venue_supported_score",
    "policy_clean_score",
    "account_readiness_score",
    "external_habitat_score",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in normalized_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in normalized_rows:
        csv_row = dict(row)
        csv_row["source_names"] = ",".join(csv_row["source_names"])
        csv_row["labels"] = ",".join(csv_row["labels"])
        writer.writerow(csv_row)

policy_counts = Counter(row["token_policy_status"] for row in normalized_rows)
venue_counts = Counter(row["venue"] for row in normalized_rows)
readiness_counts = Counter(row["account_readiness_status"] for row in normalized_rows)

summary = {
    "status": "run80a_complete" if normalized_rows else "blocked_no_external_universe_rows",
    "seed_token_count": len(seed_tokens),
    "seed_tokens": seed_tokens,
    "external_source_notes": source_notes,
    "dexscreener_raw_pair_count": len(raw_rows),
    "birdeye_optional_pair_count": len(birdeye_rows),
    "bitquery_optional_pair_count": len(bitquery_rows),
    "candidate_row_count": len(normalized_rows),
    "top_n": top_n,
    "policy_status_counts": dict(policy_counts),
    "venue_counts": dict(venue_counts),
    "account_readiness_counts": dict(readiness_counts),
    "rpc_url": rpc_url,
    "signer_owner_pubkey": signer_owner_pubkey,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"seed_token_count={summary['seed_token_count']}",
    f"candidate_row_count={summary['candidate_row_count']}",
    f"top_n={summary['top_n']}",
    f"dexscreener_raw_pair_count={summary['dexscreener_raw_pair_count']}",
    f"birdeye_optional_pair_count={summary['birdeye_optional_pair_count']}",
    f"bitquery_optional_pair_count={summary['bitquery_optional_pair_count']}",
    "policy_status_counts=" + json.dumps(summary["policy_status_counts"], separators=(",", ":")),
    "venue_counts=" + json.dumps(summary["venue_counts"], separators=(",", ":")),
    "account_readiness_counts=" + json.dumps(summary["account_readiness_counts"], separators=(",", ":")),
    "external_source_notes=" + json.dumps(summary["external_source_notes"], separators=(",", ":")),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run80A volatility habitat universe builder complete"
finish

#!/usr/bin/env bash
# 78F - certify exit/flatten route readiness for otherwise-clean cap-1 DirectArb rows.

source "$(dirname "$0")/lib.sh"
script_header "78F - DirectArb Exit/Flatten Route Certificate"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EPOCH_DIR=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            EPOCH_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$EPOCH_DIR" ]]; then
    EPOCH_DIR="${SHADOW_RUN78F_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
fi
if [[ ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found: $EPOCH_DIR"
    exit 1
fi
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78f_directarb_exit_flatten_route_certificate"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78f_directarb_exit_flatten_route_certificate.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

RPC_URL="${SHADOW_RUN78F_RPC_URL:-${SHADOW_LOCAL_AGAVE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN78F_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN78F_EXIT_SLIPPAGE_BPS:-100}"
EXIT_FEE_TIP_LAMPORTS="${SHADOW_RUN78F_EXIT_FEE_TIP_LAMPORTS:-200000}"
MIN_EXIT_WS0L_RAW="${SHADOW_RUN78F_MIN_EXIT_WSOL_RAW:-10000}"
MIN_EXIT_USDC_RAW="${SHADOW_RUN78F_MIN_EXIT_USDC_RAW:-1000}"
MAX_HOLD_TTL_MS="${SHADOW_RUN78F_MAX_HOLD_TTL_MS:-5000}"
MAX_DECIMALS="${SHADOW_RUN78F_MAX_DECIMALS:-12}"
ALLOW_TOKEN2022="${SHADOW_RUN78F_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN78F_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN78F_ALLOW_MINT_AUTHORITY:-false}"
DENYLIST_MINTS="${SHADOW_RUN78F_DENYLIST_MINTS:-}"

python3 - \
    "$EPOCH_DIR" \
    "$OUTPUT_DIR" \
    "$SUMMARY_FILE" \
    "$REPORT_JSON" \
    "$ROWS_JSONL" \
    "$ROWS_CSV" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$INVENTORY_PATH" \
    "$EXIT_SLIPPAGE_BPS" \
    "$EXIT_FEE_TIP_LAMPORTS" \
    "$MIN_EXIT_WS0L_RAW" \
    "$MIN_EXIT_USDC_RAW" \
    "$MAX_HOLD_TTL_MS" \
    "$MAX_DECIMALS" \
    "$ALLOW_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$DENYLIST_MINTS" <<'PY'
from __future__ import annotations

import csv
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from decimal import Decimal, InvalidOperation
from pathlib import Path

epoch_dir = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])
rpc_url = sys.argv[7]
quote_api_url = sys.argv[8]
inventory_path = Path(sys.argv[9])
exit_slippage_bps = int(sys.argv[10] or "100")
exit_fee_tip_lamports = int(sys.argv[11] or "200000")
min_exit_wsol_raw = int(sys.argv[12] or "10000")
min_exit_usdc_raw = int(sys.argv[13] or "1000")
max_hold_ttl_ms = int(sys.argv[14] or "5000")
max_decimals = int(sys.argv[15] or "12")
allow_token2022 = str(sys.argv[16]).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(sys.argv[17]).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(sys.argv[18]).lower() in {"1", "true", "yes", "on"}
denylist_mints = {item.strip() for item in sys.argv[19].split(",") if item.strip()}

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2"
SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
CORE_STABLE_MINTS = {USDC_MINT, USDT_MINT}


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def safe_decimal(value) -> Decimal | None:
    try:
        if value in (None, ""):
            return None
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def text(value) -> str:
    return str(value or "").strip()


def placeholder_url(url: str) -> bool:
    return not url or "CHANGE_ME" in url or "${" in url


def row_key(row: dict) -> tuple[str, str, str, str]:
    return (
        text(row.get("signature") or row.get("observed_tx_signature")),
        text(row.get("pool")),
        text(row.get("mint_in")),
        text(row.get("mint_out")),
    )


def http_json(url: str) -> tuple[dict | None, dict | None]:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    attempts = 3
    delay = 0.75
    for attempt in range(1, attempts + 1):
        try:
            if url.startswith("https://"):
                with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                    return json.loads(response.read().decode("utf-8")), None
            with urllib.request.urlopen(request, timeout=20) as response:
                return json.loads(response.read().decode("utf-8")), None
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            try:
                payload = json.loads(body)
            except Exception:
                payload = {"error": body}
            payload["http_status"] = exc.code
            payload["attempts"] = attempt
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            return None, payload
        except Exception as exc:
            if attempt < attempts:
                time.sleep(delay)
                delay *= 2
                continue
            return None, {"error": str(exc), "attempts": attempt}
    return None, {"error": "request_failed"}


def rpc(method: str, params=None) -> tuple[dict | None, dict | None]:
    if placeholder_url(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        if rpc_url.startswith("https://"):
            with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                body = json.loads(response.read().decode("utf-8"))
        else:
            with urllib.request.urlopen(request, timeout=20) as response:
                body = json.loads(response.read().decode("utf-8"))
        if "error" in body:
            return None, body["error"]
        return body.get("result"), None
    except Exception as exc:
        return None, {"error": str(exc)}


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int) -> tuple[dict | None, dict | None]:
    if amount_raw <= 0:
        return None, {"error": "amount_zero"}
    if input_mint == output_mint:
        return {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "inAmount": str(amount_raw),
            "outAmount": str(amount_raw),
            "otherAmountThreshold": str(amount_raw),
            "routePlan": [{"swapInfo": {"label": "identity"}}],
        }, None
    if placeholder_url(quote_api_url):
        return None, {"error": "quote_api_unconfigured"}
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(amount_raw),
        "slippageBps": str(exit_slippage_bps),
    }
    return http_json(quote_api_url + "?" + urllib.parse.urlencode(params))


def route_labels(quote: dict | None) -> list[str]:
    labels: list[str] = []
    if not quote:
        return labels
    for item in quote.get("routePlan") or []:
        if not isinstance(item, dict):
            continue
        swap = item.get("swapInfo") or {}
        label = text(swap.get("label"))
        if label:
            labels.append(label)
    return labels


def mint_metadata(mint: str) -> dict:
    base = {
        "mint": mint,
        "rpc_audit_ok": False,
        "rpc_policy_snapshot_stable": False,
        "initialized": False,
        "token_program": "",
        "decimals": None,
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "rpc_error": "",
    }

    def parse(result: dict) -> dict:
        value = result.get("value") or {}
        data = value.get("data") or {}
        parsed = data.get("parsed") or {}
        info = parsed.get("info") or {}
        extensions = info.get("extensions") or parsed.get("extensions") or []
        names: list[str] = []
        for item in extensions:
            if isinstance(item, str):
                names.append(item)
            elif isinstance(item, dict):
                names.append(text(item.get("extension") or item.get("type") or item.get("name")))
        return {
            "rpc_audit_ok": bool(value),
            "initialized": bool(info.get("isInitialized", False)),
            "token_program": text(value.get("owner")),
            "decimals": info.get("decimals"),
            "freeze_authority": text(info.get("freezeAuthority")),
            "mint_authority": text(info.get("mintAuthority")),
            "extensions": sorted([name for name in names if name]),
        }

    first, first_error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    second, second_error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    if first_error or not first:
        base["rpc_error"] = json.dumps(first_error or {"error": "missing_account"}, sort_keys=True)
        return base
    try:
        parsed_first = parse(first)
        parsed_second = parse(second or {})
        base.update(parsed_first)
        stable_keys = ("initialized", "token_program", "decimals", "freeze_authority", "mint_authority", "extensions")
        base["rpc_policy_snapshot_stable"] = all(parsed_first.get(key) == parsed_second.get(key) for key in stable_keys)
    except Exception as exc:
        base["rpc_error"] = str(exc)
    return base


def policy_for_mint(mint: str, meta: dict) -> tuple[bool, str]:
    if mint in denylist_mints:
        return False, "denylisted"
    if mint in {WSOL_MINT, USDC_MINT, USDT_MINT} and meta.get("token_program") == SPL_TOKEN_PROGRAM:
        decimals = safe_int(meta.get("decimals"), -1)
        if 0 <= decimals <= max_decimals:
            return True, "core_token_policy"
    if not meta.get("rpc_audit_ok"):
        return False, "rpc_audit_unavailable"
    if not meta.get("rpc_policy_snapshot_stable"):
        return False, "rpc_policy_snapshot_unstable"
    if not meta.get("initialized"):
        return False, "mint_uninitialized"
    token_program = text(meta.get("token_program"))
    if token_program == TOKEN2022_PROGRAM and not allow_token2022:
        return False, "token2022_explicit_allow_required"
    if token_program not in {SPL_TOKEN_PROGRAM, TOKEN2022_PROGRAM}:
        return False, "token_program_not_allowed"
    if meta.get("freeze_authority") and not allow_freeze_authority:
        return False, "freeze_authority_present"
    if meta.get("mint_authority") and not allow_mint_authority:
        return False, "mint_authority_present"
    decimals = meta.get("decimals")
    if not isinstance(decimals, int) or decimals < 0 or decimals > max_decimals:
        return False, "decimals_not_sane"
    extensions = meta.get("extensions") or []
    if extensions and token_program == TOKEN2022_PROGRAM and not allow_token2022:
        return False, "extensions_require_token2022_allow"
    return True, "none"


def load_inventory(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def inventory_entry_for_mint(inventory: dict, mint: str) -> dict:
    for entry in inventory.get("accounts") or []:
        if text(entry.get("mint_pubkey")) == mint:
            return entry
    return {}


def decimal_text(value) -> str:
    if value is None:
        return ""
    if isinstance(value, Decimal):
        text_value = format(value.normalize(), "f")
        if "." in text_value:
            text_value = text_value.rstrip("0").rstrip(".")
        return text_value or "0"
    return str(value)


rows_78c3_path = epoch_dir / "78c3_directarb_cap1_nearmiss_autopsy" / "rows.jsonl"
raw_path = epoch_dir / os.environ.get("SHADOW_RUN78F_RUN78_STEP", "78c2_run78_cap1_fresh_runtime_quote_nosend") / "run78c_selector_rejections.jsonl"
rows_78c3, malformed_78c3 = parse_jsonl(rows_78c3_path)
raw_rows, malformed_raw = parse_jsonl(raw_path)
raw_by_key = {row_key(row): row for row in raw_rows}

target_rows: list[dict] = []
for row in rows_78c3:
    if not truth(row.get("would_pass_if_exit_route_ignored")):
        continue
    raw = raw_by_key.get(row_key(row), {})
    merged = dict(raw)
    merged.update(row)
    target_rows.append(merged)

inventory = load_inventory(inventory_path)
metadata_cache: dict[str, dict] = {}
quote_cache: dict[tuple[str, str, int], tuple[dict | None, dict | None]] = {}

sol_usdc_quote, sol_usdc_error = quote_exact_in(WSOL_MINT, USDC_MINT, 1_000_000_000)
sol_usdc_raw = safe_int((sol_usdc_quote or {}).get("outAmount"))
sol_usd = (Decimal(sol_usdc_raw) / Decimal(1_000_000)) if sol_usdc_raw > 0 else None

out_rows: list[dict] = []
for index, row in enumerate(target_rows, start=1):
    mint_out = text(row.get("mint_out"))
    entry_amount_in_raw = safe_int(
        row.get("pm_reserve_requested_raw")
        or row.get("input_amount_raw")
        or row.get("exact_input_required_raw")
        or 1_000_000_000
    )
    entry_expected_out_raw = safe_int(row.get("expected_out_raw") or row.get("quote_expected_out_raw"))
    entry_min_out_raw = safe_int(row.get("min_out_raw"))
    entry_net_ev_after_fee_tip = safe_int(
        row.get("candidate_net_after_fee_raw")
        or row.get("net_ev_after_fee_tip")
        or row.get("net_ev_after_fee_tip_raw")
    )
    entry_tip = safe_int(row.get("policy_tip_raw") or row.get("tip_lamports"))
    entry_base_fee = safe_int(row.get("base_fee_raw") or row.get("base_fee"))
    entry_plus_exit_fee_tip = entry_tip + entry_base_fee + exit_fee_tip_lamports

    meta = metadata_cache.setdefault(mint_out, mint_metadata(mint_out))
    output_token_policy_pass, policy_blocker = policy_for_mint(mint_out, meta)
    output_inventory = inventory_entry_for_mint(inventory, mint_out)
    output_token_account_ready = truth(row.get("output_account_ready")) or bool(output_inventory)
    output_decimals = safe_int(row.get("mint_out_decimals"), -1)
    if output_decimals < 0 and isinstance(meta.get("decimals"), int):
        output_decimals = int(meta["decimals"])

    amount_to_flatten_raw = entry_min_out_raw if entry_min_out_raw > 0 else entry_expected_out_raw
    quote_key_wsol = (mint_out, WSOL_MINT, amount_to_flatten_raw)
    if quote_key_wsol not in quote_cache:
        quote_cache[quote_key_wsol] = quote_exact_in(mint_out, WSOL_MINT, amount_to_flatten_raw)
    exit_wsol_quote, exit_wsol_error = quote_cache[quote_key_wsol]
    exit_quote_out_to_wsol = safe_int((exit_wsol_quote or {}).get("outAmount"))
    exit_min_out_wsol = safe_int((exit_wsol_quote or {}).get("otherAmountThreshold"))

    quote_key_usdc = (mint_out, USDC_MINT, amount_to_flatten_raw)
    if quote_key_usdc not in quote_cache:
        quote_cache[quote_key_usdc] = quote_exact_in(mint_out, USDC_MINT, amount_to_flatten_raw)
    exit_usdc_quote, exit_usdc_error = quote_cache[quote_key_usdc]
    exit_quote_out_to_usdc = safe_int((exit_usdc_quote or {}).get("outAmount"))
    exit_min_out_usdc = safe_int((exit_usdc_quote or {}).get("otherAmountThreshold"))

    exit_route_exists = exit_quote_out_to_wsol > 0 or exit_quote_out_to_usdc > 0
    if exit_quote_out_to_wsol > 0:
        exit_route_family = "jupiter_exact_in_to_wsol"
        exit_min_out = exit_min_out_wsol
    elif exit_quote_out_to_usdc > 0:
        exit_route_family = "jupiter_exact_in_to_usdc"
        exit_min_out = exit_min_out_usdc
    else:
        exit_route_family = ""
        exit_min_out = 0

    exit_value_usd = None
    if exit_quote_out_to_usdc > 0:
        exit_value_usd = Decimal(exit_quote_out_to_usdc) / Decimal(1_000_000)
    elif exit_quote_out_to_wsol > 0 and sol_usd is not None:
        exit_value_usd = Decimal(exit_quote_out_to_wsol) / Decimal(1_000_000_000) * sol_usd

    exit_dust = True
    if exit_quote_out_to_wsol > 0:
        exit_dust = exit_quote_out_to_wsol < min_exit_wsol_raw
    elif exit_quote_out_to_usdc > 0:
        exit_dust = exit_quote_out_to_usdc < min_exit_usdc_raw

    roundtrip_net_after_all_costs = None
    if exit_quote_out_to_wsol > 0:
        roundtrip_net_after_all_costs = exit_quote_out_to_wsol - entry_amount_in_raw - entry_plus_exit_fee_tip
    roundtrip_net_after_all_costs_usd = None
    if roundtrip_net_after_all_costs is not None and sol_usd is not None:
        roundtrip_net_after_all_costs_usd = Decimal(roundtrip_net_after_all_costs) / Decimal(1_000_000_000) * sol_usd
    roundtrip_positive = roundtrip_net_after_all_costs is not None and roundtrip_net_after_all_costs > 0

    flatten_pm_model_ok = (
        output_token_account_ready
        and output_token_policy_pass
        and amount_to_flatten_raw > 0
    )
    flatten_template_ready = (
        flatten_pm_model_ok
        and exit_route_exists
        and not exit_dust
        and roundtrip_positive
    )
    auto_flatten_possible = flatten_template_ready and max_hold_ttl_ms >= 0

    if not output_token_account_ready:
        decision = "PARK_TEMPLATE_UNSUPPORTED"
    elif not output_token_policy_pass:
        decision = "PARK_POLICY"
    elif not exit_route_exists:
        decision = "PARK_MISSING_EXIT_ROUTE"
    elif exit_dust:
        decision = "PARK_DUST_EXIT"
    elif not roundtrip_positive:
        decision = "PARK_NEGATIVE_ROUNDTRIP"
    elif not flatten_template_ready or not auto_flatten_possible:
        decision = "PARK_TEMPLATE_UNSUPPORTED"
    elif mint_out == WSOL_MINT:
        decision = "DIRECTARB_EXIT_READY"
    else:
        decision = "DIRECTARB_FLATTEN_READY_NO_SEND"

    out_rows.append(
        {
            "candidate_id": row.get("candidate_id") or f"run78f_exit_{index}",
            "source_candidate_id": row.get("candidate_id"),
            "signature": text(row.get("signature")),
            "slot": safe_int(row.get("slot")),
            "pool": text(row.get("pool")),
            "venue": text(row.get("venue") or row.get("route")),
            "source": text(row.get("source")),
            "route": text(row.get("route")),
            "mint_in": text(row.get("mint_in")),
            "mint_out": mint_out,
            "entry_amount_in_raw": entry_amount_in_raw,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "entry_net_ev_after_fee_tip": entry_net_ev_after_fee_tip,
            "output_token_account_ready": output_token_account_ready,
            "output_token_account": text(output_inventory.get("token_account_pubkey")),
            "output_token_policy_pass": output_token_policy_pass,
            "output_token_policy_blocker": policy_blocker,
            "output_token_program": text(meta.get("token_program")),
            "output_decimals": output_decimals,
            "output_freeze_authority_present": bool(meta.get("freeze_authority")),
            "output_mint_authority_present": bool(meta.get("mint_authority")),
            "output_token2022": text(meta.get("token_program")) == TOKEN2022_PROGRAM,
            "output_extensions": "|".join(meta.get("extensions") or []),
            "exit_route_exists": exit_route_exists,
            "exit_route_family": exit_route_family,
            "exit_route_labels_wsol": "|".join(route_labels(exit_wsol_quote)),
            "exit_route_labels_usdc": "|".join(route_labels(exit_usdc_quote)),
            "exit_quote_out_to_wsol": exit_quote_out_to_wsol,
            "exit_quote_out_to_usdc": exit_quote_out_to_usdc,
            "exit_min_out": exit_min_out,
            "exit_value_usd": decimal_text(exit_value_usd),
            "exit_slippage_bps": exit_slippage_bps,
            "exit_dust": exit_dust,
            "roundtrip_net_after_all_costs": roundtrip_net_after_all_costs if roundtrip_net_after_all_costs is not None else "",
            "roundtrip_net_after_all_costs_usd": decimal_text(roundtrip_net_after_all_costs_usd),
            "entry_plus_exit_fee_tip": entry_plus_exit_fee_tip,
            "max_hold_ttl_ms": max_hold_ttl_ms,
            "auto_flatten_possible": auto_flatten_possible,
            "flatten_template_ready": flatten_template_ready,
            "flatten_pm_model_ok": flatten_pm_model_ok,
            "exit_wsol_error": json.dumps(exit_wsol_error, sort_keys=True) if exit_wsol_error else "",
            "exit_usdc_error": json.dumps(exit_usdc_error, sort_keys=True) if exit_usdc_error else "",
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
fieldnames = list(out_rows[0].keys()) if out_rows else [
    "candidate_id",
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "decision",
]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

decision_counts = Counter(row["decision"] for row in out_rows)
policy_counts = Counter(row["output_token_policy_blocker"] for row in out_rows)
route_family_counts = Counter(row["exit_route_family"] or "none" for row in out_rows)
report = {
    "status": "run78f_complete" if out_rows else "blocked_no_exit_ignored_rows",
    "epoch_dir": str(epoch_dir),
    "source_78c3_rows": str(rows_78c3_path),
    "source_raw_rows": str(raw_path),
    "signer_inventory_path": str(inventory_path),
    "rpc_url": rpc_url,
    "quote_api_url": quote_api_url,
    "source_exit_ignored_count": len(target_rows),
    "row_count": len(out_rows),
    "malformed_78c3_rows": malformed_78c3,
    "malformed_raw_rows": malformed_raw,
    "output_token_account_ready_count": sum(1 for row in out_rows if row["output_token_account_ready"]),
    "output_token_policy_pass_count": sum(1 for row in out_rows if row["output_token_policy_pass"]),
    "exit_route_exists_count": sum(1 for row in out_rows if row["exit_route_exists"]),
    "exit_dust_count": sum(1 for row in out_rows if row["exit_dust"]),
    "roundtrip_positive_count": sum(
        1
        for row in out_rows
        if str(row["roundtrip_net_after_all_costs"]).strip()
        and safe_int(row["roundtrip_net_after_all_costs"]) > 0
    ),
    "flatten_template_ready_count": sum(1 for row in out_rows if row["flatten_template_ready"]),
    "auto_flatten_possible_count": sum(1 for row in out_rows if row["auto_flatten_possible"]),
    "directarb_exit_ready_count": decision_counts.get("DIRECTARB_EXIT_READY", 0),
    "directarb_flatten_ready_no_send_count": decision_counts.get("DIRECTARB_FLATTEN_READY_NO_SEND", 0),
    "decision_counts": dict(decision_counts),
    "policy_blocker_counts": dict(policy_counts),
    "exit_route_family_counts": dict(route_family_counts),
    "sol_usdc_quote_raw": sol_usdc_raw,
    "sol_usd_implied": decimal_text(sol_usd),
    "exit_slippage_bps": exit_slippage_bps,
    "exit_fee_tip_lamports": exit_fee_tip_lamports,
    "max_hold_ttl_ms": max_hold_ttl_ms,
    "allow_token2022": allow_token2022,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Run78F exit/flatten certificate complete"
finish

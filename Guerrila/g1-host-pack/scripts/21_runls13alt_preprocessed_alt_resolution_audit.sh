#!/usr/bin/env bash
# 21_runls13alt_preprocessed_alt_resolution_audit.sh — hydrate ALT-backed preprocessed tx rows via RPC and expose resolved program coverage

source "$(dirname "$0")/lib.sh"
script_header "21 LS13ALT — Preprocessed ALT Resolution Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
UTILS_PATH="$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py"
RESULTS_ROOT="${SHADOW_RUNLS13ALT_RESULTS_ROOT:-$PACK_DIR/results}"
STEP="${SHADOW_RUNLS13ALT_STEP:-21_runls13alt_preprocessed_alt_resolution_audit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
ROWS_PATH="$RESULT_DIR/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/runls13alt_preprocessed_alt_resolution_audit_summary.json"
INPUT_ROWS_PATH="${SHADOW_RUNLS13ALT_INPUT_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"

is_placeholder_value() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == *CHANGE_ME* || "$value" == *'${'* ]]
}

append_api_key_if_needed() {
    local url="$1"
    local api_key="$2"
    if is_placeholder_value "$url" || is_placeholder_value "$api_key"; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"api-key="* || "$url" == *"apikey="* ]]; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"?"* ]]; then
        printf '%s&api-key=%s\n' "$url" "$api_key"
    else
        printf '%s?api-key=%s\n' "$url" "$api_key"
    fi
}

RPC_URL="${SHADOW_RUNLS13ALT_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}"
HELIUS_RPC_API_KEY="${SHADOW_RUNLS13ALT_RPC_API_KEY:-${HELIUS_API_KEY:-${HELIUS_BUNDLE_API_KEY:-}}}"
RPC_URL="$(append_api_key_if_needed "$RPC_URL" "$HELIUS_RPC_API_KEY")"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$ROWS_PATH" "$SUMMARY_JSON_PATH"

if [[ ! -f "$INPUT_ROWS_PATH" ]]; then
    fail "RunLS13ALT input rows missing: $INPUT_ROWS_PATH"
    finish
    exit 1
fi
if [[ ! -f "$UTILS_PATH" ]]; then
    fail "RunLS13ALT utils missing: $UTILS_PATH"
    finish
    exit 1
fi
if is_placeholder_value "$RPC_URL"; then
    fail "RunLS13ALT RPC URL missing"
    finish
    exit 1
fi

python3 - "$UTILS_PATH" "$INPUT_ROWS_PATH" "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE" "$RPC_URL" <<'PY'
from __future__ import annotations

import base64
import importlib.util
import json
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

utils_path = Path(sys.argv[1])
input_rows_path = Path(sys.argv[2])
rows_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
rpc_url = sys.argv[6]

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)


def rpc(method: str, params: list[Any]) -> dict[str, Any]:
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    request = urllib.request.Request(rpc_url, data=payload, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8", errors="replace"))


def fetch_alt_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], dict[str, str]]:
    fetched: dict[str, dict[str, Any]] = {}
    blockers: dict[str, str] = {}
    if not pubkeys:
        return fetched, blockers

    for start in range(0, len(pubkeys), 100):
        chunk = pubkeys[start:start + 100]
        try:
            response = rpc(
                "getMultipleAccounts",
                [chunk, {"encoding": "base64", "commitment": "processed"}],
            )
        except urllib.error.URLError:
            for pubkey in chunk:
                blockers[pubkey] = "ALT_FETCH_FAILED"
            continue
        except Exception:
            for pubkey in chunk:
                blockers[pubkey] = "ALT_FETCH_FAILED"
            continue

        if response.get("error"):
            for pubkey in chunk:
                blockers[pubkey] = "ALT_FETCH_FAILED"
            continue

        values = (((response.get("result") or {}).get("value")) or [])
        for pubkey, value in zip(chunk, values):
            if not value:
                blockers[pubkey] = "ALT_NOT_FOUND"
                continue
            data_field = value.get("data") if isinstance(value, dict) else None
            if not isinstance(data_field, list) or not data_field:
                blockers[pubkey] = "ALT_FETCH_FAILED"
                continue
            try:
                raw_data = base64.b64decode(str(data_field[0]), validate=True)
            except Exception:
                blockers[pubkey] = "ALT_FETCH_FAILED"
                continue
            decoded, decode_error = utils.decode_alt_account_data(raw_data)
            if decode_error:
                blockers[pubkey] = decode_error
                continue
            fetched[pubkey] = decoded or {}
    return fetched, blockers


def supported_pool_accounts(parsed: dict[str, Any], resolved_account_keys: list[str]) -> list[str]:
    pools: list[str] = []
    seen: set[str] = set()
    for ix in utils.instruction_views(parsed, resolved_account_keys):
        program_id = ix.get("program_id")
        accounts = ix.get("accounts") or []
        if program_id == "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK" and len(accounts) >= 3:
            candidate = accounts[2]
        elif program_id == "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C" and len(accounts) >= 4:
            candidate = accounts[3]
        elif program_id == "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" and len(accounts) >= 2:
            candidate = accounts[1]
        elif program_id == "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG" and len(accounts) >= 2:
            candidate = accounts[1]
        elif program_id == "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA" and len(accounts) >= 1:
            candidate = accounts[0]
        else:
            continue
        if candidate and candidate not in seen:
            seen.add(candidate)
            pools.append(candidate)
    return pools


rows = utils.load_jsonl(input_rows_path)
parsed_cache: dict[str, dict[str, Any]] = {}
alt_pubkeys: set[str] = set()

for row in rows:
    tx_base64 = utils.text(row.get("target_tx_base64"))
    if not tx_base64 or not utils.as_bool(row.get("target_tx_bytes_available")):
        continue
    parsed, parse_error = utils.parse_wire_tx(tx_base64)
    if parse_error or parsed is None:
        continue
    signature = utils.text(row.get("target_signature"))
    parsed_cache[signature] = parsed
    for lookup in parsed.get("address_table_lookups") or []:
        alt_pubkeys.add(utils.text(lookup.get("account_key")))

alt_tables, alt_table_blockers = fetch_alt_accounts(sorted(pubkey for pubkey in alt_pubkeys if pubkey))

output_rows: list[dict[str, Any]] = []
first_alt_blocker_counts: Counter[str] = Counter()
program_ids_before_alt_counts: Counter[str] = Counter()
program_ids_after_alt_counts: Counter[str] = Counter()

rows_with_alt_lookups = 0
lookup_table_fetch_ok_rows = 0
alt_resolution_ready_rows = 0
alt_resolution_blocked_count = 0
supported_program_after_alt_count = 0

for row in rows:
    signature = utils.text(row.get("target_signature"))
    parsed = parsed_cache.get(signature)
    parse_error = None
    if parsed is None and utils.text(row.get("target_tx_base64")) and utils.as_bool(row.get("target_tx_bytes_available")):
        parsed, parse_error = utils.parse_wire_tx(utils.text(row.get("target_tx_base64")))

    message_version = "unknown"
    uses_address_lookup_tables = False
    alt_lookup_count = 0
    lookup_table_accounts: list[str] = []
    program_ids_before_alt: list[str] = []
    program_ids_after_alt: list[str] = []
    resolved_account_keys: list[str] = []
    pool_accounts_after_alt: list[str] = []
    lookup_results: list[dict[str, Any]] = []
    lookup_table_fetch_ok = False
    unresolved_alt_count = 0
    first_alt_blocker = "none"
    total_lookup_table_addresses_count = 0

    if parsed is None:
        first_alt_blocker = "TX_DECODE_FAILED"
    else:
        message_version = str(parsed.get("version"))
        lookup_table_accounts = [utils.text(lookup.get("account_key")) for lookup in parsed.get("address_table_lookups") or []]
        uses_address_lookup_tables = bool(lookup_table_accounts)
        alt_lookup_count = len(lookup_table_accounts)
        if uses_address_lookup_tables:
            rows_with_alt_lookups += 1
        program_ids_before_alt = utils.top_level_program_ids(parsed)
        for program_id in program_ids_before_alt:
            program_ids_before_alt_counts[program_id] += 1

        lookup_details = {}
        for pubkey in lookup_table_accounts:
            if pubkey in alt_tables:
                lookup_details[pubkey] = alt_tables[pubkey]
            else:
                lookup_details[pubkey] = {"blocker": alt_table_blockers.get(pubkey, "ALT_NOT_FOUND")}

        resolved = utils.resolve_address_table_lookups(parsed, alt_tables)
        resolved_account_keys = list(resolved.get("resolved_account_keys") or [])
        lookup_results = list(resolved.get("lookup_results") or [])
        unresolved_alt_count = utils.as_int(resolved.get("unresolved_alt_count"))
        total_lookup_table_addresses_count = sum(utils.as_int(item.get("lookup_table_addresses_count")) for item in lookup_results)
        lookup_table_fetch_ok = bool(lookup_results) and all(utils.as_bool(item.get("lookup_table_fetch_ok")) for item in lookup_results)
        if not lookup_results and not uses_address_lookup_tables:
            lookup_table_fetch_ok = True

        for result in lookup_results:
            blocker = utils.text(result.get("lookup_table_blocker"))
            if blocker and blocker != "none" and first_alt_blocker == "none":
                first_alt_blocker = blocker

        if uses_address_lookup_tables and lookup_table_fetch_ok and unresolved_alt_count == 0:
            lookup_table_fetch_ok_rows += 1

        program_ids_after_alt = utils.top_level_program_ids(parsed, resolved_account_keys)
        for program_id in program_ids_after_alt:
            program_ids_after_alt_counts[program_id] += 1

        pool_accounts_after_alt = supported_pool_accounts(parsed, resolved_account_keys)
        if any(program_id in utils.SUPPORTED_DECODER_PROGRAMS for program_id in program_ids_after_alt):
            supported_program_after_alt_count += 1

        if uses_address_lookup_tables and first_alt_blocker == "none" and unresolved_alt_count == 0:
            alt_resolution_ready_rows += 1
        elif uses_address_lookup_tables:
            alt_resolution_blocked_count += 1

    first_alt_blocker_counts[first_alt_blocker] += 1
    output_rows.append(
        {
            "record_type": "runls13alt_preprocessed_alt_resolution_audit_row",
            "target_signature": signature,
            "message_version": message_version,
            "uses_address_lookup_tables": uses_address_lookup_tables,
            "alt_lookup_count": alt_lookup_count,
            "lookup_table_accounts": lookup_table_accounts,
            "lookup_table_fetch_ok": lookup_table_fetch_ok,
            "lookup_table_addresses_count": total_lookup_table_addresses_count,
            "resolved_account_keys_count": len(resolved_account_keys) if resolved_account_keys else len((parsed or {}).get("static_account_keys") or []),
            "program_ids_before_alt": program_ids_before_alt,
            "program_ids_after_alt": program_ids_after_alt,
            "pool_accounts_after_alt": pool_accounts_after_alt,
            "resolved_account_keys": resolved_account_keys,
            "lookup_results": lookup_results,
            "unresolved_alt_count": unresolved_alt_count,
            "first_alt_blocker": first_alt_blocker,
            "parse_error": parse_error,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

rows_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in output_rows),
    encoding="utf-8",
)

summary = {
    "status": "runls13alt_preprocessed_alt_resolution_audit_complete",
    "rows_seen": len(output_rows),
    "rows_with_alt_lookups": rows_with_alt_lookups,
    "lookup_table_fetch_ok_rows": lookup_table_fetch_ok_rows,
    "alt_resolution_ready_rows": alt_resolution_ready_rows,
    "alt_resolution_blocked_count": alt_resolution_blocked_count,
    "supported_program_after_alt_count": supported_program_after_alt_count,
    "first_alt_blocker_counts": dict(first_alt_blocker_counts),
    "program_ids_before_alt_counts": dict(program_ids_before_alt_counts),
    "program_ids_after_alt_counts": dict(program_ids_after_alt_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"rows_seen={summary['rows_seen']}",
    f"rows_with_alt_lookups={summary['rows_with_alt_lookups']}",
    f"lookup_table_fetch_ok_rows={summary['lookup_table_fetch_ok_rows']}",
    f"alt_resolution_ready_rows={summary['alt_resolution_ready_rows']}",
    f"alt_resolution_blocked_count={summary['alt_resolution_blocked_count']}",
    f"supported_program_after_alt_count={summary['supported_program_after_alt_count']}",
    "first_alt_blocker_counts=" + json.dumps(summary["first_alt_blocker_counts"], sort_keys=True),
    "program_ids_before_alt_counts=" + json.dumps(summary["program_ids_before_alt_counts"], sort_keys=True),
    "program_ids_after_alt_counts=" + json.dumps(summary["program_ids_after_alt_counts"], sort_keys=True),
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "RunLS13ALT rows written to $ROWS_PATH"
info "RunLS13ALT summary written to $SUMMARY_JSON_PATH"
finish

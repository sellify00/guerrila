#!/usr/bin/env bash
# 78PM - classify PM/core-balance visibility blockers for strict DirectArb Run78 near-miss rows

source "$(dirname "$0")/lib.sh"
script_header "78PM - DirectArb PM Core Balance Visibility Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78PM_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78pm_directarb_pm_core_balance_visibility_audit.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

AUTOPSY_STEP="${SHADOW_RUN78PM_AUTOPSY_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
AUTOPSY_ROWS_PATH="${SHADOW_RUN78PM_AUTOPSY_ROWS_PATH:-$RESULTS_ROOT/$AUTOPSY_STEP/rows.jsonl}"
SIZE_LADDER_STEP="${SHADOW_RUN78PM_SIZE_LADDER_STEP:-epoch8_auto_strict_directarb_private_egress_run78s_pm_reserve_size_ladder}"
SIZE_LADDER_ROWS_PATH="${SHADOW_RUN78PM_SIZE_LADDER_ROWS_PATH:-$RESULTS_ROOT/$SIZE_LADDER_STEP/rows.jsonl}"
RUN78_STEPS_CSV="${SHADOW_RUN78PM_SOURCE_STEPS:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout,epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout_rerun1}"

INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
SIGNER_PATH="${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL_OVERRIDE="${SHADOW_RUN78PM_RPC_URL:-}"
SOL_USD_EQUIVALENT="${SHADOW_RUN78PM_SOL_USD_EQUIVALENT:-${SHADOW_RUN78_SOL_USD_EQUIVALENT:-}}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

export AUTOPSY_ROWS_PATH SIZE_LADDER_ROWS_PATH INVENTORY_PATH SIGNER_PATH RPC_URL_OVERRIDE
export SUMMARY_FILE REPORT_JSON ROWS_JSONL ROWS_CSV RUN78_STEPS_CSV SOL_USD_EQUIVALENT

python3 - <<'PY'
from __future__ import annotations

import csv
import json
import os
import ssl
import subprocess
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

try:
    from solders.pubkey import Pubkey
except Exception as exc:  # pragma: no cover - host precondition
    raise SystemExit(f"missing solders.pubkey: {exc}")

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN_2022_PROGRAM_ID = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ATA_PROGRAM_ID = "ATokenGPvbdGVxr1b2hvZbsiW5xWH25efTNsLJA8knL"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2"
CORE_INPUT_MINTS = {WSOL_MINT, USDC_MINT, USDT_MINT}


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def truth(value) -> str:
    return "true" if value else "false"


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def read_text_via_sudo_or_open(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        return subprocess.check_output(["sudo", "cat", str(path)], text=True)


def parse_jsonl(path: Path):
    rows = []
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


def b58encode(raw: bytes) -> str:
    if not raw:
        return ""
    zeros = 0
    for value in raw:
        if value == 0:
            zeros += 1
        else:
            break
    number = int.from_bytes(raw, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def load_worker_signer_pubkey_text(raw_text: str, path: Path) -> str:
    raw = json.loads(raw_text)
    if not isinstance(raw, list) or len(raw) < 64:
        raise ValueError(f"unexpected signer keypair shape in {path}")
    return b58encode(bytes(int(value) for value in raw[-32:]))


def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
    )
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def normalize_account(program_id: str, owner_pubkey: str, row: dict) -> dict | None:
    pubkey = str(row.get("pubkey") or "").strip()
    parsed = (((row.get("account") or {}).get("data") or {}).get("parsed") or {})
    info = parsed.get("info") or {}
    mint_pubkey = str(info.get("mint") or "").strip()
    if not pubkey or not mint_pubkey:
        return None
    token_amount = info.get("tokenAmount") or {}
    state = str(info.get("state") or "").strip().lower()
    amount_raw = safe_int(token_amount.get("amount"))
    initialized = state in {"initialized", "frozen"}
    delegate = str(info.get("delegate") or "").strip()
    close_authority = str(info.get("closeAuthority") or "").strip()
    return {
        "mint_pubkey": mint_pubkey,
        "token_account_pubkey": pubkey,
        "token_program_id": program_id,
        "initialized": bool(initialized),
        "amount_raw": amount_raw,
        "owner_pubkey": owner_pubkey,
        "delegate": delegate or None,
        "close_authority": close_authority or None,
    }


def score_entry(entry: dict):
    return (
        0 if entry.get("initialized") else 1,
        -safe_int(entry.get("amount_raw")),
        str(entry.get("token_account_pubkey") or ""),
    )


def best_entry(entries: list[dict]) -> dict | None:
    if not entries:
        return None
    return sorted(entries, key=score_entry)[0]


def derive_ata(owner_pubkey: str, token_program_id: str, mint_pubkey: str) -> str:
    ata, _ = Pubkey.find_program_address(
        [
            bytes(Pubkey.from_string(owner_pubkey)),
            bytes(Pubkey.from_string(token_program_id)),
            bytes(Pubkey.from_string(mint_pubkey)),
        ],
        Pubkey.from_string(ATA_PROGRAM_ID),
    )
    return str(ata)


def read_owner_program(mint_pubkey: str, cache: dict[str, str], rpc_url: str) -> str:
    if mint_pubkey in cache:
        return cache[mint_pubkey]
    response = rpc(
        rpc_url,
        "getAccountInfo",
        [mint_pubkey, {"encoding": "base64", "commitment": "confirmed"}],
    )
    value = response.get("result") if isinstance(response, dict) else response
    value = value.get("value") if isinstance(value, dict) else value
    owner = str((value or {}).get("owner") or "").strip()
    cache[mint_pubkey] = owner
    return owner


def read_mint_decimals(mint_pubkey: str, cache: dict[str, int], rpc_url: str) -> int:
    if mint_pubkey in cache:
        return cache[mint_pubkey]
    response = rpc(
        rpc_url,
        "getTokenSupply",
        [mint_pubkey, {"commitment": "confirmed"}],
    )
    value = (((response.get("result") or {}).get("value")) or {})
    decimals = safe_int(value.get("decimals"))
    cache[mint_pubkey] = decimals
    return decimals


def normalize_native(raw: int, decimals: int) -> float | None:
    if raw <= 0:
        return 0.0
    if decimals < 0:
        return None
    return raw / (10 ** decimals)


def resolve_pm_view_entry(owner_pubkey: str, mint_pubkey: str, owner_program_id: str, inventory_entries_by_mint, inventory_entries_by_token_account):
    derived_ata = ""
    derived_entry = None
    if owner_program_id in {TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID}:
        derived_ata = derive_ata(owner_pubkey, owner_program_id, mint_pubkey)
        derived_entry = inventory_entries_by_token_account.get(derived_ata)
        if derived_entry is not None:
            return derived_ata, derived_entry, "derived_ata_inventory_match"
    entries = inventory_entries_by_mint.get(mint_pubkey) or []
    if len(entries) == 1:
        return derived_ata, entries[0], "unique_by_mint_fallback"
    if entries:
        return derived_ata, best_entry(entries), "best_by_mint_nonunique_fallback"
    return derived_ata, None, "no_inventory_entry"


def ladder_group(rows):
    grouped = defaultdict(list)
    for row in rows:
        grouped[(str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or ""))].append(row)
    return grouped


autopsy_rows_path = Path(os.environ["AUTOPSY_ROWS_PATH"])
size_ladder_rows_path = Path(os.environ["SIZE_LADDER_ROWS_PATH"])
inventory_path = Path(os.environ["INVENTORY_PATH"])
signer_path = Path(os.environ["SIGNER_PATH"])
summary_path = Path(os.environ["SUMMARY_FILE"])
report_path = Path(os.environ["REPORT_JSON"])
rows_jsonl_path = Path(os.environ["ROWS_JSONL"])
rows_csv_path = Path(os.environ["ROWS_CSV"])
source_steps = [item.strip() for item in os.environ.get("RUN78_STEPS_CSV", "").split(",") if item.strip()]
sol_usd_equivalent = safe_float(os.environ.get("SOL_USD_EQUIVALENT"))

autopsy_rows, autopsy_malformed = parse_jsonl(autopsy_rows_path)
ladder_rows, ladder_malformed = parse_jsonl(size_ladder_rows_path)
inventory = json.loads(read_text_via_sudo_or_open(inventory_path))
owner_pubkey = str(inventory.get("owner_pubkey") or "").strip() or load_worker_signer_pubkey_text(
    read_text_via_sudo_or_open(signer_path),
    signer_path,
)
inventory_rpc_url = str(inventory.get("rpc_url") or "").strip()
rpc_url = str(os.environ.get("RPC_URL_OVERRIDE") or "").strip() or inventory_rpc_url
if not rpc_url:
    raise SystemExit("missing RPC URL override and inventory rpc_url")

inventory_accounts = inventory.get("accounts") or []
inventory_entries_by_mint: dict[str, list[dict]] = defaultdict(list)
inventory_entries_by_token_account: dict[str, dict] = {}
for entry in inventory_accounts:
    mint = str(entry.get("mint_pubkey") or "").strip()
    token_account = str(entry.get("token_account_pubkey") or "").strip()
    if not mint or not token_account:
        continue
    inventory_entries_by_mint[mint].append(entry)
    inventory_entries_by_token_account[token_account] = entry

chain_entries_by_mint: dict[str, list[dict]] = defaultdict(list)
chain_entries_by_token_account: dict[str, dict] = {}
chain_slot = None
for program_id in (TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID):
    response = rpc(
        rpc_url,
        "getTokenAccountsByOwner",
        [
            owner_pubkey,
            {"programId": program_id},
            {"encoding": "jsonParsed", "commitment": "confirmed"},
        ],
    )
    if "error" in response:
        raise RuntimeError(f"rpc error for {program_id}: {response['error']}")
    result = response.get("result") or {}
    chain_slot = (result.get("context") or {}).get("slot") or chain_slot
    for row in result.get("value") or []:
        entry = normalize_account(program_id, owner_pubkey, row)
        if not entry:
            continue
        chain_entries_by_mint[entry["mint_pubkey"]].append(entry)
        chain_entries_by_token_account[entry["token_account_pubkey"]] = entry

native_balance_response = rpc(
    rpc_url,
    "getBalance",
    [owner_pubkey, {"commitment": "confirmed"}],
)
native_balance_raw = safe_int((((native_balance_response.get("result") or {}).get("value"))))

owner_program_cache: dict[str, str] = {}
mint_decimals_cache: dict[str, int] = {}
ladder_rows_by_key = ladder_group(ladder_rows)
out_rows = []

for row in autopsy_rows:
    signature = str(row.get("signature") or "")
    pool = str(row.get("pool") or "")
    mint_in = str(row.get("mint_in") or "")
    mint_out = str(row.get("mint_out") or "")
    source_step = str(row.get("source_step") or "")
    required_input_raw = safe_int(row.get("pm_required_input_raw"))
    first_failed = str(row.get("first_failed") or "")
    route_family = str(row.get("route_family") or "")
    base_fee = safe_int(row.get("base_fee"))
    tip_lamports = safe_int(row.get("tip_lamports"))
    autopsy_pm_available_input_raw = safe_int(row.get("pm_available_input_raw"))
    autopsy_pm_reserve_can_lock_exact_input = as_bool(row.get("pm_reserve_can_lock_exact_input"))
    owner_program_id = read_owner_program(mint_in, owner_program_cache, rpc_url)
    mint_in_decimals = read_mint_decimals(mint_in, mint_decimals_cache, rpc_url)
    is_wsol_input = mint_in == WSOL_MINT
    is_core_input = mint_in in CORE_INPUT_MINTS
    is_supported_for_size_ladder = is_wsol_input

    derived_ata, pm_view_entry, pm_view_source = resolve_pm_view_entry(
        owner_pubkey,
        mint_in,
        owner_program_id,
        inventory_entries_by_mint,
        inventory_entries_by_token_account,
    )
    chain_entries = chain_entries_by_mint.get(mint_in) or []
    chain_best = best_entry(chain_entries)
    chain_ata_entry = chain_entries_by_token_account.get(derived_ata) if derived_ata else None
    inventory_entries = inventory_entries_by_mint.get(mint_in) or []
    inventory_entry_count = len(inventory_entries)
    inventory_best = best_entry(inventory_entries)

    expected_input_token_account = derived_ata or str((pm_view_entry or {}).get("token_account_pubkey") or "")
    expected_input_token_account_present = chain_ata_entry is not None if derived_ata else pm_view_entry is not None
    expected_input_token_account_owner = owner_pubkey if expected_input_token_account_present else ""
    expected_input_token_account_balance_raw = safe_int((chain_ata_entry or {}).get("amount_raw"))

    inventory_json_balance_raw = safe_int((inventory_best or {}).get("amount_raw"))
    live_rpc_balance_raw = safe_int((chain_best or {}).get("amount_raw"))
    pm_fence_balance_raw = None
    pm_available_balance_raw = safe_int((pm_view_entry or {}).get("amount_raw"))
    pm_reserve_requested_raw = required_input_raw
    pm_reserve_available_raw = pm_available_balance_raw

    required_input_native = normalize_native(required_input_raw, mint_in_decimals)
    if mint_in in {USDC_MINT, USDT_MINT}:
        required_input_usd = required_input_native
    elif is_wsol_input and sol_usd_equivalent > 0:
        required_input_usd = required_input_native * sol_usd_equivalent if required_input_native is not None else None
    else:
        required_input_usd = None

    expected_balance = max(expected_input_token_account_balance_raw, 0)
    live_or_expected_balance = max(live_rpc_balance_raw, expected_balance)
    pm_visible_balance_ok = pm_available_balance_raw >= required_input_raw > 0
    fee_tip_ok = native_balance_raw >= (base_fee + tip_lamports)
    pm_view_is_fallback = pm_view_source in {"unique_by_mint_fallback", "best_by_mint_nonunique_fallback"}
    derived_expected_mismatch = (
        expected_input_token_account_present
        and pm_view_entry is not None
        and str((pm_view_entry or {}).get("token_account_pubkey") or "") != expected_input_token_account
    )
    autopsy_pm_reports_shortfall = (
        (not autopsy_pm_reserve_can_lock_exact_input)
        and autopsy_pm_available_input_raw < required_input_raw
    )
    size_rows = ladder_rows_by_key.get((source_step, signature, pool), [])
    supported_sizes = sorted(
        float(str(item.get("size_sol")))
        for item in size_rows
        if item.get("size_raw") is not None
    )
    size_ladder_min_supported_size = supported_sizes[0] if supported_sizes else None
    size_ladder_pm_pass_any = any(as_bool(item.get("pm_reserve_ok")) for item in size_rows)

    blocker = ""
    detail = ""

    if pm_visible_balance_ok and not fee_tip_ok:
        blocker = "fee_or_tip_balance_missing"
        detail = "native_sol_balance_below_base_fee_plus_tip"
    elif pm_visible_balance_ok and first_failed in {"ev_not_positive", "ev_math_unavailable", "quote_numeric_missing"}:
        blocker = "pm_not_blocking_visible_balance_ok"
        detail = f"visible_balance_satisfies_exact_input__first_failed={first_failed}__pm_view_source={pm_view_source}"
    elif pm_visible_balance_ok and autopsy_pm_reports_shortfall:
        if is_wsol_input and live_or_expected_balance > 0 and autopsy_pm_available_input_raw == 0:
            if inventory_entry_count > 1:
                blocker = "multiple_accounts_ambiguous"
                detail = "autopsy_pm_zero_with_multiple_inventory_wsol_accounts"
            else:
                blocker = "stale_inventory_snapshot"
                detail = "autopsy_pm_zero_but_inventory_and_live_wsol_visible"
        elif derived_expected_mismatch:
            blocker = "mint_index_mismatch"
            detail = "derived_expected_account_differs_from_pm_visible_account"
        elif pm_view_is_fallback and live_or_expected_balance > 0:
            blocker = "mint_index_mismatch"
            detail = f"autopsy_pm_shortfall_but_fallback_inventory_visible__pm_view_source={pm_view_source}"
        else:
            blocker = "mint_index_mismatch"
            detail = f"autopsy_pm_shortfall_inconsistent_with_visible_balance__pm_view_source={pm_view_source}"
    elif pm_visible_balance_ok:
        blocker = "pm_not_blocking_visible_balance_ok"
        detail = f"visible_balance_satisfies_exact_input__pm_view_source={pm_view_source}"
    elif not is_supported_for_size_ladder and not is_core_input and pm_available_balance_raw == 0 and live_or_expected_balance == 0:
        blocker = "unsupported_non_wsol_input"
        detail = "non_core_input_and_no_visible_inventory"
    elif is_wsol_input and native_balance_raw >= required_input_raw > 0 and live_or_expected_balance == 0 and pm_available_balance_raw == 0:
        blocker = "native_sol_vs_wsol_confusion"
        detail = "native_sol_sufficient_but_no_wrapped_sol_visible"
    elif is_wsol_input and live_or_expected_balance > 0 and pm_available_balance_raw == 0:
        if pm_view_entry is None:
            if inventory_entry_count > 1:
                blocker = "multiple_accounts_ambiguous"
                detail = "live_wsol_balance_visible_but_multiple_inventory_accounts_exist"
            else:
                blocker = "stale_inventory_snapshot"
                detail = "live_wsol_balance_visible_but_pm_view_missing"
        elif expected_input_token_account and str((pm_view_entry or {}).get("token_account_pubkey") or "") != expected_input_token_account:
            blocker = "mint_index_mismatch"
            detail = "pm_view_selected_non_expected_wsol_account"
        else:
            blocker = "stale_inventory_snapshot"
            detail = "live_wsol_balance_visible_but_inventory_snapshot_zero"
    elif expected_input_token_account_present and expected_balance >= required_input_raw > 0 and (
        pm_view_entry is None
        or str((pm_view_entry or {}).get("token_account_pubkey") or "") != expected_input_token_account
        or pm_available_balance_raw < required_input_raw
    ):
        if pm_view_entry is not None and str((pm_view_entry or {}).get("token_account_pubkey") or "") != expected_input_token_account:
            blocker = "mint_index_mismatch"
            detail = "derived_expected_account_has_balance_but_pm_view_points_elsewhere"
        else:
            blocker = "stale_inventory_snapshot"
            detail = "expected_account_live_balance_not_reflected_in_inventory_snapshot"
    elif live_rpc_balance_raw > 0 and pm_available_balance_raw == 0:
        if pm_view_entry is None:
            blocker = "stale_inventory_snapshot"
            detail = "live_best_balance_visible_but_no_inventory_entry"
        else:
            blocker = "mint_index_mismatch"
            detail = "pm_view_balance_zero_while_live_best_entry_has_balance"
    elif pm_available_balance_raw > 0 and pm_available_balance_raw < required_input_raw:
        if is_wsol_input and native_balance_raw >= required_input_raw:
            blocker = "native_sol_vs_wsol_confusion"
            detail = "wrapped_sol_underfunded_while_native_sol_is_sufficient"
        elif live_or_expected_balance > pm_available_balance_raw:
            blocker = "stale_inventory_snapshot"
            detail = "inventory_snapshot_understates_live_visible_balance"
        else:
            blocker = "actual_insufficient_input_balance"
            detail = "pm_view_balance_below_exact_input_requirement"
    elif pm_view_entry is None and live_or_expected_balance == 0:
        blocker = "template_account_missing"
        detail = "no_inventory_entry_and_no_live_token_account_balance"
    else:
        blocker = "mint_index_mismatch"
        detail = f"residual_visibility_mismatch__pm_view_source={pm_view_source}"

    out_rows.append(
        {
            "signature": signature,
            "pool": pool,
            "mint_in": mint_in,
            "mint_out": mint_out,
            "route_family": route_family,
            "required_input_raw": required_input_raw,
            "required_input_native": required_input_native,
            "required_input_usd": required_input_usd,
            "expected_input_token_account": expected_input_token_account,
            "expected_input_token_account_present": expected_input_token_account_present,
            "expected_input_token_account_owner": expected_input_token_account_owner,
            "expected_input_token_account_balance_raw": expected_input_token_account_balance_raw,
            "inventory_json_balance_raw": inventory_json_balance_raw,
            "live_rpc_balance_raw": live_rpc_balance_raw,
            "pm_fence_balance_raw": pm_fence_balance_raw,
            "pm_available_balance_raw": pm_available_balance_raw,
            "is_wsol_input": is_wsol_input,
            "is_core_input": is_core_input,
            "is_supported_for_size_ladder": is_supported_for_size_ladder,
            "autopsy_pm_available_input_raw": autopsy_pm_available_input_raw,
            "autopsy_pm_reserve_can_lock_exact_input": autopsy_pm_reserve_can_lock_exact_input,
            "pm_reserve_requested_raw": pm_reserve_requested_raw,
            "pm_reserve_available_raw": pm_reserve_available_raw,
            "pm_available_fee_balance": native_balance_raw,
            "pm_available_tip_balance": native_balance_raw,
            "pm_reserve_blocker": blocker,
            "pm_reserve_blocker_detail": detail,
            "pm_reserve_blocker_evidence": pm_view_source,
            "input_account_resolution_blocker": blocker if blocker in {
                "multiple_accounts_ambiguous",
                "stale_inventory_snapshot",
                "mint_index_mismatch",
                "template_account_missing",
                "native_sol_vs_wsol_confusion",
            } else "none",
            "first_failed": first_failed,
            "size_ladder_min_supported_size": size_ladder_min_supported_size,
            "size_ladder_pm_pass_any": size_ladder_pm_pass_any,
            "source_step": source_step,
            "inventory_slot": safe_int(inventory.get("slot")),
            "chain_slot": safe_int(chain_slot),
            "worker_signer_native_sol_balance_raw": native_balance_raw,
            "pm_gate_layer": "pre_pm_snapshot_inventory_view",
        }
    )

fieldnames = [
    "signature",
    "pool",
    "mint_in",
    "mint_out",
    "route_family",
    "required_input_raw",
    "required_input_native",
    "required_input_usd",
    "expected_input_token_account",
    "expected_input_token_account_present",
    "expected_input_token_account_owner",
    "expected_input_token_account_balance_raw",
    "inventory_json_balance_raw",
    "live_rpc_balance_raw",
    "pm_fence_balance_raw",
    "pm_available_balance_raw",
    "is_wsol_input",
    "is_core_input",
    "is_supported_for_size_ladder",
    "autopsy_pm_available_input_raw",
    "autopsy_pm_reserve_can_lock_exact_input",
    "pm_reserve_requested_raw",
    "pm_reserve_available_raw",
    "pm_available_fee_balance",
    "pm_available_tip_balance",
    "pm_reserve_blocker",
    "pm_reserve_blocker_detail",
    "pm_reserve_blocker_evidence",
    "input_account_resolution_blocker",
    "first_failed",
    "size_ladder_min_supported_size",
    "size_ladder_pm_pass_any",
    "source_step",
    "inventory_slot",
    "chain_slot",
    "worker_signer_native_sol_balance_raw",
    "pm_gate_layer",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in out_rows:
        writer.writerow({key: row.get(key) for key in fieldnames})

blocker_counts = Counter(row["pm_reserve_blocker"] for row in out_rows)
classified_count = sum(1 for row in out_rows if row["pm_reserve_blocker"])
unknown_count = sum(1 for row in out_rows if row["pm_reserve_blocker"] == "pm_schema_gap")

report = {
    "status": "run78pm_complete" if out_rows else "blocked_no_autopsy_rows",
    "source_steps": source_steps,
    "inventory_path": str(inventory_path),
    "signer_path": str(signer_path),
    "rpc_url": rpc_url,
    "signer_pubkey": owner_pubkey,
    "inventory_slot": safe_int(inventory.get("slot")),
    "chain_slot": safe_int(chain_slot),
    "worker_signer_native_sol_balance_raw": native_balance_raw,
    "pm_gate_layer": "pre_pm_snapshot_inventory_view",
    "pm_blocker_classified_count": classified_count,
    "unknown_pm_blocker_count": unknown_count,
    "pm_blocker_counts": dict(blocker_counts),
    "autopsy_rows_path": str(autopsy_rows_path),
    "size_ladder_rows_path": str(size_ladder_rows_path),
    "malformed_autopsy_rows": autopsy_malformed,
    "malformed_size_ladder_rows": ladder_malformed,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={report['status']}",
    f"pm_gate_layer={report['pm_gate_layer']}",
    f"pm_blocker_classified_count={report['pm_blocker_classified_count']}",
    f"unknown_pm_blocker_count={report['unknown_pm_blocker_count']}",
    "pm_blocker_counts=" + json.dumps(report["pm_blocker_counts"], separators=(",", ":")),
    f"inventory_path={report['inventory_path']}",
    f"inventory_slot={report['inventory_slot']}",
    f"chain_slot={report['chain_slot']}",
    f"worker_signer_native_sol_balance_raw={report['worker_signer_native_sol_balance_raw']}",
    f"rows_jsonl={report['rows_jsonl']}",
    f"rows_csv={report['rows_csv']}",
]

report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78PM PM/core-balance visibility audit complete"
finish

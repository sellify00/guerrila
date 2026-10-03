#!/usr/bin/env bash
# 21_lsprebrinv_tiny_inventory_band_sim.sh — no-send tiny Pump.fun inventory-band viability sim

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV — Tiny Inventory Band Sim"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRINV_STEP:-21_lsprebrinv_tiny_inventory_band_sim}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_tiny_inventory_band_sim_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_tiny_inventory_band_sim_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
DIRECT_ROWS_PATH="${SHADOW_LSPREBRINV_DIRECT_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}}"
RPC_URL="${SHADOW_LSPREBRINV_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
MAX_SPEND_SOL="${SHADOW_LSPREBRINV_MAX_SPEND_SOL:-0.005}"
MAX_LOSS_SOL="${SHADOW_LSPREBRINV_MAX_LOSS_SOL:-0.002}"
TTL_SECONDS="${SHADOW_LSPREBRINV_TTL_SECONDS:-300}"
STOP_LOSS_BPS="${SHADOW_LSPREBRINV_STOP_LOSS_BPS:-300}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LSPREBRINV_BASE_TX_FEE_LAMPORTS:-5000}"
SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_TIP_LAMPORTS:-0}}}"
SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-${SHADOW_LSPREBRINV_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}}"
TIP_LAMPORTS="$SENDER_MIN_TIP_LAMPORTS"
ALLOW_TOKEN2022="${SHADOW_LSPREBRINV_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_LSPREBRINV_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_LSPREBRINV_ALLOW_MINT_AUTHORITY:-false}"
MAX_ROWS="${SHADOW_LSPREBRINV_MAX_ROWS:-0}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$DIRECT_ROWS_PATH" ]] || DIRECT_ROWS_PATH="$(latest_rows_for_step runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$DIRECT_ROWS_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$MAX_SPEND_SOL" "$MAX_LOSS_SOL" "$TTL_SECONDS" "$STOP_LOSS_BPS" "$SLIPPAGE_BPS" "$BASE_TX_FEE_LAMPORTS" "$TIP_LAMPORTS" "$SENDER_TIP_ACCOUNT" "$ALLOW_TOKEN2022" "$ALLOW_FREEZE_AUTHORITY" "$ALLOW_MINT_AUTHORITY" "$MAX_ROWS" <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    direct_rows_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    signer_path_arg,
    rpc_url,
    max_spend_sol_arg,
    max_loss_sol_arg,
    ttl_seconds_arg,
    stop_loss_bps_arg,
    slippage_bps_arg,
    base_tx_fee_lamports_arg,
    tip_lamports_arg,
    sender_tip_account_arg,
    allow_token2022_arg,
    allow_freeze_authority_arg,
    allow_mint_authority_arg,
    max_rows_arg,
) = sys.argv[1:20]

direct_rows_path = Path(direct_rows_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
max_spend_sol = float(max_spend_sol_arg or "0.005")
max_loss_sol = float(max_loss_sol_arg or "0.002")
ttl_seconds = max(1, int(float(ttl_seconds_arg or "300")))
stop_loss_bps = max(1, int(float(stop_loss_bps_arg or "300")))
slippage_bps = max(0, int(float(slippage_bps_arg or "100")))
base_tx_fee_lamports = max(0, int(float(base_tx_fee_lamports_arg or "5000")))
tip_lamports = max(0, int(float(tip_lamports_arg or "0")))
sender_tip_account = str(sender_tip_account_arg or "").strip()
allow_token2022 = str(allow_token2022_arg).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(allow_freeze_authority_arg).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(allow_mint_authority_arg).lower() in {"1", "true", "yes", "on"}
max_rows = max(0, int(max_rows_arg or "0"))

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def is_placeholder(value: Any) -> bool:
    raw = text(value)
    return not raw or "CHANGE_ME" in raw or "${" in raw


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def http_post_json(url: str, payload: dict[str, Any]):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def b58(data: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    num = int.from_bytes(data, "big")
    out = ""
    while num:
        num, rem = divmod(num, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    if pad or out:
        return alphabet[0] * pad + out
    return alphabet[0]


def rpc(method: str, params=None):
    if is_placeholder(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    try:
        response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    except Exception:
        return {}, 0, "PRE_TARGET_POOL_STATE_MISSING"
    if error is not None:
        return {}, 0, "PRE_TARGET_POOL_STATE_MISSING"
    result = (response or {}).get("result", response) or {}
    context = result.get("context") or {}
    slot = safe_int(context.get("slot"))
    values = result.get("value") or []
    accounts: dict[str, dict[str, Any]] = {}
    import base64
    for pubkey, value in zip(pubkeys, values):
        if not value:
            accounts[pubkey] = {"exists": False, "data_raw": b""}
            continue
        data_field = value.get("data") if isinstance(value, dict) else None
        raw = b""
        if isinstance(data_field, list) and data_field:
            try:
                raw = base64.b64decode(str(data_field[0]), validate=True)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return accounts, slot, None


def get_balance(pubkey: str) -> tuple[int, dict[str, Any] | None]:
    result, error = rpc("getBalance", [pubkey, {"commitment": "confirmed"}])
    if error is not None:
        return 0, error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        maybe = value.get("value", value)
    else:
        maybe = 0
    return safe_int(maybe), None


def get_token_accounts_by_owner(owner: str, mint: str) -> list[dict[str, Any]]:
    result, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "confirmed"}],
    )
    if error is not None:
        return []
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        rows = value.get("value") or []
    else:
        rows = []
    out = []
    for row in rows:
        parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
        info = parsed.get("info") if isinstance(parsed, dict) else {}
        token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
        out.append(
            {
                "pubkey": text((row or {}).get("pubkey")),
                "amount_raw": safe_int((token_amount or {}).get("amount")),
                "decimals": safe_int((token_amount or {}).get("decimals")),
            }
        )
    return out


def mint_metadata(mint: str) -> dict[str, Any]:
    result, error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    out = {
        "mint": mint,
        "token_program": "",
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "rpc_error": error or {},
    }
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        if mint == WSOL_MINT:
            out["token_program"] = TOKEN_PROGRAM
        return out
    out["token_program"] = text(value.get("owner"))
    parsed = value.get("data", {}).get("parsed", {}) if isinstance(value.get("data"), dict) else {}
    info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
    out["freeze_authority"] = text(info.get("freezeAuthority"))
    out["mint_authority"] = text(info.get("mintAuthority"))
    extensions = []
    raw_extensions = info.get("extensions") if isinstance(info.get("extensions"), list) else []
    for item in raw_extensions:
        if isinstance(item, dict):
            ext_name = text(item.get("extension"))
            if ext_name:
                extensions.append(ext_name)
        elif isinstance(item, str):
            extensions.append(item)
    out["extensions"] = extensions
    return out


def policy_for_mint(meta: dict[str, Any]) -> tuple[bool, str]:
    token_program = text(meta.get("token_program"))
    if token_program == TOKEN2022_PROGRAM and not allow_token2022:
        return False, "token2022_not_allowed"
    if text(meta.get("freeze_authority")) and not allow_freeze_authority:
        return False, "freeze_authority_present"
    if text(meta.get("mint_authority")) and not allow_mint_authority:
        return False, "mint_authority_present"
    normalized_exts = {text(item).replace("_", "").replace("-", "").lower() for item in meta.get("extensions", [])}
    dangerous = sorted(ext for ext in normalized_exts if ext in DANGEROUS_TOKEN2022_EXTENSIONS)
    if dangerous:
        return False, f"token2022_extension_{dangerous[0]}"
    return True, ""


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) != PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "PRE_TARGET_POOL_STATE_MISSING"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "PUMPFUN_AMM_DECODER_MISSING"
    return {
        "base_mint": b58(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int | None:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return None
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def min_out_from_expected(expected_out: int, slippage_bps_value: int) -> int:
    if expected_out <= 0:
        return 0
    return max(0, (expected_out * max(0, 10_000 - slippage_bps_value)) // 10_000)


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
        return 0
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return (amount_in * reserve_out) // denominator


owner_pubkey = ""
if signer_keypair_path and signer_keypair_path.exists():
    try:
        from solders.keypair import Keypair

        owner_pubkey = str(Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8")).pubkey())
    except Exception:
        owner_pubkey = ""

rows = [
    row
    for row in load_jsonl(direct_rows_path)
    if text(row.get("decision")).startswith("PUMPFUN_DIRECT_ROUTE_READY")
    and text(row.get("our_output_mint")) == WSOL_MINT
    and text(row.get("our_input_mint")) not in {"", WSOL_MINT}
]
if max_rows > 0:
    rows = rows[:max_rows]

max_spend_lamports = max(1, int(round(max_spend_sol * 1_000_000_000)))
max_loss_lamports = max(0, int(round(max_loss_sol * 1_000_000_000)))

out_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

policy_pass_rows = 0
direct_entry_quote_ok_rows = 0
direct_exit_quote_ok_rows = 0
exit_non_dust_rows = 0
auto_flatten_available_rows = 0
pm_reserve_inventory_ok_rows = 0
pm_release_ok_rows = 0
max_loss_defined_rows = len(rows)
sim_ready_rows = 0

for index, row in enumerate(rows, start=1):
    candidate_id = text(row.get("candidate_id")) or text(row.get("target_signature")) or f"tiny_inventory_band_{index}"
    candidate_mint = text(row.get("our_input_mint"))
    pool = text(row.get("pool"))
    candidate_meta = mint_metadata(candidate_mint)
    policy_pass, policy_blocker = policy_for_mint(candidate_meta)
    if policy_pass:
        policy_pass_rows += 1

    owner_native_balance_raw, _ = get_balance(owner_pubkey) if owner_pubkey else (0, None)
    wsol_accounts = get_token_accounts_by_owner(owner_pubkey, WSOL_MINT) if owner_pubkey else []
    wsol_entry = max(wsol_accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
    wsol_balance_raw = safe_int(wsol_entry.get("amount_raw"))
    wsol_account_pubkey = text(wsol_entry.get("pubkey"))
    token_accounts = get_token_accounts_by_owner(owner_pubkey, candidate_mint) if owner_pubkey else []
    token_entry = max(token_accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
    token_account_pubkey = text(token_entry.get("pubkey"))
    token_balance_raw = safe_int(token_entry.get("amount_raw"))

    accounts, slot, state_error = get_multiple_accounts([pool])
    pool_account = accounts.get(pool, {})
    pool_doc, pool_error = decode_pumpfun_pool(pool_account.get("data_raw", b"")) if not state_error else (None, state_error)
    direct_entry_expected_out_raw = 0
    direct_entry_min_out_raw = 0
    direct_exit_expected_out_raw = 0
    direct_exit_min_out_raw = 0
    current_base_reserve_raw = 0
    current_quote_reserve_raw = 0
    immediate_roundtrip_loss_lamports = max_spend_lamports

    if pool_doc and {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} == {candidate_mint, WSOL_MINT}:
        vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
        if not vault_error:
            current_base_reserve_raw = decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b"")) or 0
            current_quote_reserve_raw = decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b"")) or 0
            if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == candidate_mint:
                entry_reserve_in_raw = current_base_reserve_raw
                entry_reserve_out_raw = current_quote_reserve_raw
                exit_reserve_in_post_raw = max(0, current_quote_reserve_raw - 0)
                exit_reserve_out_post_raw = max(0, current_base_reserve_raw + 0)
            else:
                entry_reserve_in_raw = current_quote_reserve_raw
                entry_reserve_out_raw = current_base_reserve_raw
                exit_reserve_in_post_raw = max(0, current_base_reserve_raw - 0)
                exit_reserve_out_post_raw = max(0, current_quote_reserve_raw + 0)

            direct_entry_expected_out_raw = constant_product_out(entry_reserve_in_raw, entry_reserve_out_raw, max_spend_lamports)
            direct_entry_min_out_raw = min_out_from_expected(direct_entry_expected_out_raw, slippage_bps)

            if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == candidate_mint:
                post_base_reserve_raw = current_base_reserve_raw + max_spend_lamports
                post_quote_reserve_raw = max(0, current_quote_reserve_raw - direct_entry_expected_out_raw)
                direct_exit_expected_out_raw = constant_product_out(post_quote_reserve_raw, post_base_reserve_raw, direct_entry_expected_out_raw)
            else:
                post_quote_reserve_raw = current_quote_reserve_raw + max_spend_lamports
                post_base_reserve_raw = max(0, current_base_reserve_raw - direct_entry_expected_out_raw)
                direct_exit_expected_out_raw = constant_product_out(post_base_reserve_raw, post_quote_reserve_raw, direct_entry_expected_out_raw)

            direct_exit_min_out_raw = min_out_from_expected(direct_exit_expected_out_raw, slippage_bps)
            immediate_roundtrip_loss_lamports = max(0, max_spend_lamports - direct_exit_expected_out_raw + (2 * base_tx_fee_lamports) + (2 * tip_lamports))

    direct_entry_quote_ok = direct_entry_expected_out_raw > 0 and direct_entry_min_out_raw > 0
    direct_exit_quote_ok = direct_exit_expected_out_raw > 0 and direct_exit_min_out_raw > 0
    exit_non_dust = direct_exit_expected_out_raw > 1
    auto_flatten_available = direct_exit_quote_ok and exit_non_dust and bool(text(row.get("pumpfun_direct_selected_output_account_pubkey")))
    if direct_entry_quote_ok:
        direct_entry_quote_ok_rows += 1
    if direct_exit_quote_ok:
        direct_exit_quote_ok_rows += 1
    if exit_non_dust:
        exit_non_dust_rows += 1
    if auto_flatten_available:
        auto_flatten_available_rows += 1

    entry_input_account_ready = bool(wsol_account_pubkey) and wsol_balance_raw >= max_spend_lamports
    entry_output_account_ready = bool(token_account_pubkey)
    entry_output_derived_only = not entry_output_account_ready and bool(text(row.get("pumpfun_direct_derived_input_account_pubkey")))
    pm_reserve_fee_ok = owner_native_balance_raw >= ((2 * base_tx_fee_lamports) + (2 * tip_lamports))
    pm_reserve_tip_ok = owner_native_balance_raw >= (2 * tip_lamports if tip_lamports > 0 else 0)
    pm_reserve_inventory_ok = entry_input_account_ready and pm_reserve_fee_ok
    pm_release_ok = True
    if pm_reserve_inventory_ok:
        pm_reserve_inventory_ok_rows += 1
    if pm_release_ok:
        pm_release_ok_rows += 1

    if not policy_pass:
        decision = "PARK_POLICY"
        first_blocker = policy_blocker
    elif current_base_reserve_raw <= 0 or current_quote_reserve_raw <= 0:
        decision = "PARK_POOL_STATE_MISSING"
        first_blocker = pool_error or state_error or "pool_state_missing"
    elif not direct_entry_quote_ok:
        decision = "PARK_DIRECT_ENTRY_QUOTE_MISSING"
        first_blocker = "entry_quote_non_positive"
    elif not direct_exit_quote_ok:
        decision = "PARK_DIRECT_EXIT_QUOTE_MISSING"
        first_blocker = "exit_quote_non_positive"
    elif not exit_non_dust:
        decision = "PARK_EXIT_DUST"
        first_blocker = "exit_non_dust_false"
    elif not auto_flatten_available:
        decision = "PARK_AUTO_FLATTEN_UNAVAILABLE"
        first_blocker = "auto_flatten_unavailable"
    elif not pm_reserve_inventory_ok:
        decision = "PARK_PM"
        first_blocker = "wsol_or_fee_reserve_missing"
    elif immediate_roundtrip_loss_lamports > max_loss_lamports:
        decision = "PARK_MAX_LOSS_EXCEEDED"
        first_blocker = "immediate_roundtrip_loss_exceeds_cap"
    elif not entry_output_account_ready:
        decision = "PARK_ENTRY_OUTPUT_ACCOUNT_MISSING"
        first_blocker = "entry_output_account_missing_requires_precreate"
    else:
        decision = "TINY_INVENTORY_BAND_SIM_READY"
        first_blocker = "none"
        sim_ready_rows += 1

    decision_counts[decision] += 1
    out_rows.append(
        {
            "record_type": "runlsprebrinv_tiny_inventory_band_sim_row",
            "candidate_id": candidate_id,
            "candidate_mint": candidate_mint,
            "pool": pool,
            "venue": text(row.get("venue")),
            "owner_pubkey": owner_pubkey,
            "max_spend_sol": max_spend_sol,
            "max_spend_lamports": max_spend_lamports,
            "max_loss_sol": max_loss_sol,
            "max_loss_lamports": max_loss_lamports,
            "ttl_seconds": ttl_seconds,
            "stop_loss_bps": stop_loss_bps,
            "policy_pass": policy_pass,
            "policy_blocker": policy_blocker,
            "entry_input_mint": WSOL_MINT,
            "entry_output_mint": candidate_mint,
            "entry_input_account_pubkey": wsol_account_pubkey,
            "entry_input_balance_raw": wsol_balance_raw,
            "entry_input_account_ready": entry_input_account_ready,
            "entry_output_account_pubkey": token_account_pubkey,
            "entry_output_account_ready": entry_output_account_ready,
            "entry_output_derived_only": entry_output_derived_only,
            "entry_output_derived_pubkey": text(row.get("pumpfun_direct_derived_input_account_pubkey")),
            "exit_input_account_pubkey": token_account_pubkey or text(row.get("pumpfun_direct_derived_input_account_pubkey")),
            "exit_input_account_ready": entry_output_account_ready,
            "exit_output_account_pubkey": wsol_account_pubkey,
            "exit_output_account_ready": bool(wsol_account_pubkey),
            "current_pool_state_slot": slot,
            "current_base_reserve_raw": current_base_reserve_raw,
            "current_quote_reserve_raw": current_quote_reserve_raw,
            "direct_entry_quote_ok": direct_entry_quote_ok,
            "direct_entry_expected_out_raw": direct_entry_expected_out_raw,
            "direct_entry_min_out_raw": direct_entry_min_out_raw,
            "direct_exit_quote_ok": direct_exit_quote_ok,
            "direct_exit_expected_out_raw": direct_exit_expected_out_raw,
            "direct_exit_min_out_raw": direct_exit_min_out_raw,
            "exit_non_dust": exit_non_dust,
            "auto_flatten_available": auto_flatten_available,
            "immediate_roundtrip_loss_lamports": immediate_roundtrip_loss_lamports,
            "immediate_roundtrip_loss_sol": immediate_roundtrip_loss_lamports / 1_000_000_000.0,
            "owner_native_balance_raw": owner_native_balance_raw,
            "sender_tip_lamports": tip_lamports,
            "sender_tip_account": sender_tip_account,
            "sender_tip_instruction_present": bool(sender_tip_account and tip_lamports > 0),
            "pm_reserve_fee_ok": pm_reserve_fee_ok,
            "pm_reserve_tip_ok": pm_reserve_tip_ok,
            "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
            "pm_release_ok": pm_release_ok,
            "max_loss_defined": max_loss_lamports > 0,
            "decision": decision,
            "first_inventory_band_blocker": first_blocker,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

summary = {
    "status": "TINY_INVENTORY_BAND_SIM_READY" if sim_ready_rows > 0 else (max(decision_counts, key=decision_counts.get) if decision_counts else "PARK_NO_ROWS"),
    "source_rows": str(direct_rows_path),
    "rows_seen": len(out_rows),
    "sim_ready_rows": sim_ready_rows,
    "policy_pass_rows": policy_pass_rows,
    "direct_entry_quote_ok_rows": direct_entry_quote_ok_rows,
    "direct_exit_quote_ok_rows": direct_exit_quote_ok_rows,
    "exit_non_dust_rows": exit_non_dust_rows,
    "auto_flatten_available_rows": auto_flatten_available_rows,
    "pm_reserve_inventory_ok_rows": pm_reserve_inventory_ok_rows,
    "pm_release_ok_rows": pm_release_ok_rows,
    "max_loss_defined_rows": max_loss_defined_rows,
    "decision_counts": dict(decision_counts),
    "max_spend_sol": max_spend_sol,
    "max_loss_sol": max_loss_sol,
    "ttl_seconds": ttl_seconds,
    "stop_loss_bps": stop_loss_bps,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(direct_rows_path),
    "signer_path": str(signer_keypair_path or ""),
    "rpc_url": rpc_url,
    "max_spend_sol": max_spend_sol,
    "max_loss_sol": max_loss_sol,
    "ttl_seconds": ttl_seconds,
    "stop_loss_bps": stop_loss_bps,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV tiny inventory band sim written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

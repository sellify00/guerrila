#!/usr/bin/env bash
# 21_lsprebrpf_pumpfun_amm_direct_route_probe.sh — prove direct Pump.fun AMM route shape for projected Backrun rows without send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-PF — Pump.fun Direct Route Probe"

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
STEP="${SHADOW_LSPREBRPF_STEP:-21_lsprebrpf_pumpfun_amm_direct_route_probe}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrpf_pumpfun_amm_direct_route_probe_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PREBR0_ROWS_PATH="${SHADOW_LSPREBRPF_PREBR0_ROWS_PATH:-$RESULTS_ROOT/21_runlsprebr0_preprocessed_projection_quote/runlsprebr0_preprocessed_projection_quote_rows.jsonl}"
LS13A_ROWS_PATH="${SHADOW_LSPREBRPF_LS13A_ROWS_PATH:-$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
ALT_AUDIT_ROWS_PATH="${SHADOW_LSPREBRPF_ALT_AUDIT_ROWS_PATH:-$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}"
RPC_URL="${SHADOW_LSPREBRPF_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
ALLOW_TOKEN2022="${SHADOW_LSPREBRPF_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_LSPREBRPF_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_LSPREBRPF_ALLOW_MINT_AUTHORITY:-false}"
MAX_ROWS="${SHADOW_LSPREBRPF_MAX_ROWS:-0}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PREBR0_ROWS_PATH" ]] || PREBR0_ROWS_PATH="$(latest_rows_for_step runlsprebr0_preprocessed_projection_quote_rows.jsonl)"
[[ -f "$LS13A_ROWS_PATH" ]] || LS13A_ROWS_PATH="$(latest_rows_for_step runls13a_preprocessed_backrun_target_census_rows.jsonl)"
[[ -f "$ALT_AUDIT_ROWS_PATH" ]] || ALT_AUDIT_ROWS_PATH="$(latest_rows_for_step runls13alt_preprocessed_alt_resolution_audit_rows.jsonl)"

if [[ -z "$PREBR0_ROWS_PATH" || ! -f "$PREBR0_ROWS_PATH" ]]; then
    fail "Pump.fun direct-route probe prebr0 rows missing: ${PREBR0_ROWS_PATH:-<empty>}"
    finish
    exit 1
fi
if [[ -z "$LS13A_ROWS_PATH" || ! -f "$LS13A_ROWS_PATH" ]]; then
    fail "Pump.fun direct-route probe LS13A rows missing: ${LS13A_ROWS_PATH:-<empty>}"
    finish
    exit 1
fi
if [[ -z "$ALT_AUDIT_ROWS_PATH" || ! -f "$ALT_AUDIT_ROWS_PATH" ]]; then
    fail "Pump.fun direct-route probe ALT audit rows missing: ${ALT_AUDIT_ROWS_PATH:-<empty>}"
    finish
    exit 1
fi

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PACK_DIR/scripts/runls13_preprocessed_tx_utils.py" "$PREBR0_ROWS_PATH" "$LS13A_ROWS_PATH" "$ALT_AUDIT_ROWS_PATH" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$ALLOW_TOKEN2022" "$ALLOW_FREEZE_AUTHORITY" "$ALLOW_MINT_AUTHORITY" "$MAX_ROWS" <<'PY'
from __future__ import annotations

import importlib.util
import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    utils_path_arg,
    prebr0_rows_arg,
    ls13a_rows_arg,
    alt_rows_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    signer_path_arg,
    rpc_url,
    allow_token2022_arg,
    allow_freeze_authority_arg,
    allow_mint_authority_arg,
    max_rows_arg,
) = sys.argv[1:16]

utils_path = Path(utils_path_arg)
prebr0_rows_path = Path(prebr0_rows_arg)
ls13a_rows_path = Path(ls13a_rows_arg)
alt_rows_path = Path(alt_rows_arg)
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
allow_token2022 = str(allow_token2022_arg).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(allow_freeze_authority_arg).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(allow_mint_authority_arg).lower() in {"1", "true", "yes", "on"}
max_rows = max(0, int(max_rows_arg or "0"))

spec = importlib.util.spec_from_file_location("runls13_utils", utils_path)
utils = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(utils)

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
PUMPFUN_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
BUY_EXACT_QUOTE_IN_DISCRIMINATOR = bytes([198, 46, 21, 82, 180, 217, 232, 112])
SELL_DISCRIMINATOR = bytes([51, 230, 133, 164, 1, 127, 131, 173])
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}
MALFORMED_ZERO_PUBKEY = "111111111111111111111111111111111"
SYSTEM_PROGRAM_PUBKEY = "11111111111111111111111111111111"


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


def normalize_pubkey(value: Any) -> str:
    raw = text(value)
    if raw == MALFORMED_ZERO_PUBKEY:
        return SYSTEM_PROGRAM_PUBKEY
    return raw


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists() or not path.is_file():
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


def rpc(method: str, params=None):
    if is_placeholder(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_latest_blockhash() -> tuple[str, dict[str, Any] | None]:
    result, error = rpc("getLatestBlockhash")
    if error is not None:
        return "", error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value", value)
    blockhash = text((value or {}).get("blockhash"))
    if not blockhash:
        return "", {"error": "latest_blockhash_missing"}
    return blockhash, None


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


def first_or_derived_account(owner_pubkey: str, mint: str, token_program: str):
    accounts = get_token_accounts_by_owner(owner_pubkey, mint) if owner_pubkey and mint else []
    richest = max(accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
    actual_pubkey = text(richest.get("pubkey"))
    actual_balance_raw = safe_int(richest.get("amount_raw"))
    derived_pubkey = ""
    derived_error = ""
    if solders_ready and owner_pubkey and mint and token_program and get_associated_token_address is not None:
        try:
            derived_pubkey = str(
                get_associated_token_address(
                    SoldersPubkey.from_string(owner_pubkey),
                    SoldersPubkey.from_string(mint),
                    SoldersPubkey.from_string(token_program),
                )
            )
        except Exception as exc:
            derived_error = str(exc)
    selected_pubkey = actual_pubkey or derived_pubkey
    return {
        "actual_pubkey": actual_pubkey,
        "actual_balance_raw": actual_balance_raw,
        "actual_count": len(accounts),
        "derived_pubkey": derived_pubkey,
        "derived_error": derived_error,
        "selected_pubkey": selected_pubkey,
    }


def permission_map(parsed: dict[str, Any], resolved_account_keys: list[str], alt_row: dict[str, Any]) -> dict[int, tuple[bool, bool]]:
    header = parsed.get("header") or {}
    num_required_signatures = safe_int(header.get("num_required_signatures"))
    num_readonly_signed_accounts = safe_int(header.get("num_readonly_signed_accounts"))
    num_readonly_unsigned_accounts = safe_int(header.get("num_readonly_unsigned_accounts"))
    static_account_keys = list(parsed.get("static_account_keys") or [])
    static_count = len(static_account_keys)
    loaded_writable_count = safe_int(alt_row.get("resolved_writable_count"))
    permissions: dict[int, tuple[bool, bool]] = {}
    for idx in range(len(resolved_account_keys)):
        if idx < static_count:
            is_signer = idx < num_required_signatures
            if is_signer:
                is_writable = idx < max(0, num_required_signatures - num_readonly_signed_accounts)
            else:
                is_writable = idx < max(num_required_signatures, static_count - num_readonly_unsigned_accounts)
        else:
            loaded_index = idx - static_count
            is_signer = False
            is_writable = loaded_index < loaded_writable_count
        permissions[idx] = (is_signer, is_writable)
    return permissions


def encode_direct_ix_data(route_mode: str, amount_in_raw: int, min_out_raw: int) -> bytes:
    if route_mode == "sell":
        return SELL_DISCRIMINATOR + int(amount_in_raw).to_bytes(8, "little") + int(min_out_raw).to_bytes(8, "little")
    if route_mode == "buy_exact_quote_in":
        return (
            BUY_EXACT_QUOTE_IN_DISCRIMINATOR
            + int(amount_in_raw).to_bytes(8, "little")
            + int(min_out_raw).to_bytes(8, "little")
            + bytes([1])
        )
    return b""


AccountMeta = None
Instruction = None
Keypair = None
Hash = None
Message = None
SoldersPubkey = None
Transaction = None
get_associated_token_address = None
solders_ready = False
solders_error = ""
owner_keypair = None
owner_pubkey = ""

if signer_keypair_path and signer_keypair_path.exists():
    try:
        from solders.hash import Hash as SoldersHash
        from solders.instruction import AccountMeta as SoldersAccountMeta, Instruction as SoldersInstruction
        from solders.keypair import Keypair as SoldersKeypair
        from solders.message import Message as SoldersMessage
        from solders.pubkey import Pubkey as SoldersPubkeyCls
        from solders.transaction import Transaction as SoldersTransaction
        from spl.token.instructions import get_associated_token_address as spl_get_associated_token_address

        AccountMeta = SoldersAccountMeta
        Instruction = SoldersInstruction
        Keypair = SoldersKeypair
        Hash = SoldersHash
        Message = SoldersMessage
        SoldersPubkey = SoldersPubkeyCls
        Transaction = SoldersTransaction
        get_associated_token_address = spl_get_associated_token_address
        owner_keypair = Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8"))
        owner_pubkey = str(owner_keypair.pubkey())
        solders_ready = True
    except Exception as exc:
        solders_error = str(exc)

prebr0_rows = [
    row
    for row in load_jsonl(prebr0_rows_path)
    if text(row.get("venue")) == "pumpfun_amm"
    and text(row.get("projection_blocker")) == "PROJECTION_READY"
    and safe_int(row.get("projected_expected_out_raw")) > 0
    and safe_int(row.get("projected_min_out_raw")) > 0
]
if max_rows > 0:
    prebr0_rows = prebr0_rows[:max_rows]

ls13a_by_signature = {text(row.get("target_signature")): row for row in load_jsonl(ls13a_rows_path) if text(row.get("target_signature"))}
alt_by_signature = {text(row.get("target_signature")): row for row in load_jsonl(alt_rows_path) if text(row.get("target_signature"))}

out_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

pumpfun_direct_route_exists_rows = 0
pumpfun_direct_quote_available_rows = 0
pumpfun_direct_min_out_positive_rows = 0
pumpfun_direct_template_ready_rows = 0
pumpfun_direct_tx_buildable_rows = 0
pumpfun_direct_current_account_ready_rows = 0
pumpfun_direct_hypothetical_only_rows = 0

for index, row in enumerate(prebr0_rows, start=1):
    signature = text(row.get("target_signature"))
    candidate_id = text(row.get("target_entity_id")) or signature or f"pumpfun_direct_probe_{index}"
    ls13a_row = ls13a_by_signature.get(signature, {})
    alt_row = alt_by_signature.get(signature, {})
    target_tx_base64 = text(ls13a_row.get("target_tx_base64"))
    parsed = None
    parse_error = ""
    resolved_account_keys = [normalize_pubkey(item) for item in (alt_row.get("resolved_account_keys") or [])]
    target_ix = None
    target_ix_error = ""
    target_accounts: list[str] = []
    target_route_mode = ""
    direct_route_mode = ""
    direct_ix_data = b""
    direct_ix_accounts: list[str] = []
    direct_route_exists = False
    direct_quote_available = safe_int(row.get("projected_expected_out_raw")) > 0
    direct_min_out_positive = safe_int(row.get("projected_min_out_raw")) > 0
    direct_template_ready = False
    direct_tx_buildable = False
    direct_tx_serialized_len = 0
    latest_blockhash = ""
    latest_blockhash_error = {}
    decision = "PARK_PUMPFUN_DIRECT_ROUTE_TEMPLATE_MISSING"
    first_blocker = "target_tx_missing"
    build_error = ""

    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    our_backrun_direction = text(row.get("our_backrun_direction"))
    our_input_raw = safe_int(row.get("our_input_raw"))
    projected_expected_out_raw = safe_int(row.get("projected_expected_out_raw"))
    projected_min_out_raw = safe_int(row.get("projected_min_out_raw"))
    our_input_mint, our_output_mint = ("", "")
    if "->" in our_backrun_direction:
        our_input_mint, our_output_mint = [part.strip() for part in our_backrun_direction.split("->", 1)]

    input_meta = mint_metadata(our_input_mint) if our_input_mint else {}
    output_meta = mint_metadata(our_output_mint) if our_output_mint else {}
    input_policy_pass, input_policy_blocker = policy_for_mint(input_meta) if input_meta else (False, "input_mint_missing")
    output_policy_pass, output_policy_blocker = policy_for_mint(output_meta) if output_meta else (False, "output_mint_missing")
    input_token_program = text(input_meta.get("token_program")) or (TOKEN_PROGRAM if our_input_mint == WSOL_MINT else "")
    output_token_program = text(output_meta.get("token_program")) or (TOKEN_PROGRAM if our_output_mint == WSOL_MINT else "")

    input_account_doc = first_or_derived_account(owner_pubkey, our_input_mint, input_token_program) if owner_pubkey and our_input_mint else {}
    output_account_doc = first_or_derived_account(owner_pubkey, our_output_mint, output_token_program) if owner_pubkey and our_output_mint else {}
    input_account_ready = bool(text(input_account_doc.get("actual_pubkey"))) and safe_int(input_account_doc.get("actual_balance_raw")) >= our_input_raw > 0
    output_account_ready = bool(text(output_account_doc.get("actual_pubkey")))

    if target_tx_base64:
        parsed, parse_error = utils.parse_wire_tx(target_tx_base64)
    if parsed is None:
        first_blocker = parse_error or "target_tx_parse_failed"
    else:
        if not resolved_account_keys:
            resolved_account_keys = list(parsed.get("static_account_keys") or [])
        for ix in utils.instruction_views(parsed, resolved_account_keys):
            if text(ix.get("program_id")) != PUMPFUN_PROGRAM_ID:
                continue
            if safe_int(ix.get("instruction_index")) != safe_int(row.get("target_instruction_index")):
                continue
            target_ix = ix
            break
        if target_ix is None:
            first_blocker = "target_pumpfun_instruction_missing"
        else:
            target_accounts = [normalize_pubkey(item) for item in (target_ix.get("accounts") or [])]
            target_data = target_ix.get("data") or b""
            if target_data[:8] == BUY_EXACT_QUOTE_IN_DISCRIMINATOR:
                target_route_mode = "buy_exact_quote_in"
            elif target_data[:8] == SELL_DISCRIMINATOR:
                target_route_mode = "sell"
            if len(target_accounts) < 9:
                first_blocker = "target_accounts_missing"
            elif not our_input_mint or not our_output_mint or our_input_raw <= 0:
                first_blocker = "direct_shape_missing"
            elif our_input_mint == target_accounts[3] and our_output_mint == target_accounts[4]:
                direct_route_mode = "sell"
                direct_route_exists = True
            elif our_input_mint == target_accounts[4] and our_output_mint == target_accounts[3]:
                direct_route_mode = "buy_exact_quote_in"
                direct_route_exists = True
            else:
                first_blocker = "direct_direction_not_mappable"

    if direct_route_exists:
        pumpfun_direct_route_exists_rows += 1
    if direct_quote_available:
        pumpfun_direct_quote_available_rows += 1
    if direct_min_out_positive:
        pumpfun_direct_min_out_positive_rows += 1

    if direct_route_exists and direct_quote_available and direct_min_out_positive:
        direct_ix_data = encode_direct_ix_data(direct_route_mode, our_input_raw, projected_min_out_raw)
        if not direct_ix_data:
            first_blocker = "direct_ix_encode_failed"
        elif not input_policy_pass or not output_policy_pass:
            decision = "PARK_PUMPFUN_DIRECT_ROUTE_POLICY"
            first_blocker = input_policy_blocker if not input_policy_pass else output_policy_blocker
        elif not solders_ready or owner_keypair is None or AccountMeta is None or Instruction is None or Message is None or Transaction is None or Hash is None or SoldersPubkey is None:
            decision = "PARK_PUMPFUN_DIRECT_ROUTE_SIGNER_UNAVAILABLE"
            first_blocker = solders_error or "solders_unavailable"
        else:
            permission_by_index = permission_map(parsed or {}, resolved_account_keys, alt_row)
            selected_input_pubkey = text(input_account_doc.get("selected_pubkey"))
            selected_output_pubkey = text(output_account_doc.get("selected_pubkey"))
            if not selected_input_pubkey:
                selected_input_pubkey = text(input_account_doc.get("derived_pubkey"))
            if not selected_output_pubkey:
                selected_output_pubkey = text(output_account_doc.get("derived_pubkey"))
            replacements = {
                1: owner_pubkey,
            }
            if direct_route_mode == "sell":
                replacements[5] = selected_input_pubkey
                replacements[6] = selected_output_pubkey
            elif direct_route_mode == "buy_exact_quote_in":
                replacements[5] = selected_output_pubkey
                replacements[6] = selected_input_pubkey
            if not replacements.get(5) or not replacements.get(6):
                decision = "PARK_PUMPFUN_DIRECT_ROUTE_TEMPLATE_MISSING"
                first_blocker = "derived_user_token_account_missing"
            else:
                try:
                    metas = []
                    for pos, acct_index in enumerate(target_ix.get("account_indices") or []):
                        if pos >= len(target_accounts):
                            raise RuntimeError("target_account_position_out_of_bounds")
                        pubkey_str = normalize_pubkey(replacements.get(pos, target_accounts[pos]))
                        is_signer, is_writable = permission_by_index.get(safe_int(acct_index), (False, False))
                        metas.append(
                            AccountMeta(
                                SoldersPubkey.from_string(pubkey_str),
                                is_signer,
                                is_writable,
                            )
                        )
                    program_id = SoldersPubkey.from_string(PUMPFUN_PROGRAM_ID)
                    instruction = Instruction(program_id, direct_ix_data, metas)
                    blockhash, latest_blockhash_error = get_latest_blockhash()
                    latest_blockhash = blockhash
                    if not blockhash:
                        decision = "PARK_PUMPFUN_DIRECT_ROUTE_BLOCKHASH"
                        first_blocker = text((latest_blockhash_error or {}).get("error") or "latest_blockhash_missing")
                    else:
                        message = Message([instruction], owner_keypair.pubkey())
                        tx = Transaction([owner_keypair], message, Hash.from_string(blockhash))
                        direct_tx_serialized_len = len(bytes(tx))
                        direct_template_ready = True
                        direct_tx_buildable = direct_tx_serialized_len > 0
                        if direct_tx_buildable:
                            decision = (
                                "PUMPFUN_DIRECT_ROUTE_READY_CURRENT_ACCOUNTS"
                                if input_account_ready and output_account_ready
                                else "PUMPFUN_DIRECT_ROUTE_READY_HYPOTHETICAL"
                            )
                            first_blocker = "none"
                        else:
                            decision = "PARK_PUMPFUN_DIRECT_ROUTE_TX_BUILD_FAILED"
                            first_blocker = "tx_serialized_len_zero"
                except Exception as exc:
                    build_error = str(exc)
                    decision = "PARK_PUMPFUN_DIRECT_ROUTE_TX_BUILD_FAILED"
                    first_blocker = build_error or "tx_build_failed"

    if direct_template_ready:
        pumpfun_direct_template_ready_rows += 1
    if direct_tx_buildable:
        pumpfun_direct_tx_buildable_rows += 1
    if decision == "PUMPFUN_DIRECT_ROUTE_READY_CURRENT_ACCOUNTS":
        pumpfun_direct_current_account_ready_rows += 1
    if decision == "PUMPFUN_DIRECT_ROUTE_READY_HYPOTHETICAL":
        pumpfun_direct_hypothetical_only_rows += 1

    decision_counts[decision] += 1
    out_rows.append(
        {
            **row,
            "record_type": "runlsprebrpf_pumpfun_amm_direct_route_probe_row",
            "candidate_id": candidate_id,
            "target_signature": signature,
            "target_route_mode": target_route_mode,
            "direct_route_mode": direct_route_mode,
            "direct_route_program_id": PUMPFUN_PROGRAM_ID,
            "our_input_mint": our_input_mint,
            "our_output_mint": our_output_mint,
            "pumpfun_direct_route_exists": direct_route_exists,
            "pumpfun_direct_quote_available": direct_quote_available,
            "pumpfun_direct_min_out_positive": direct_min_out_positive,
            "pumpfun_direct_template_ready": direct_template_ready,
            "pumpfun_direct_tx_buildable": direct_tx_buildable,
            "pumpfun_direct_tx_serialized_len": direct_tx_serialized_len,
            "pumpfun_direct_instruction_len": len(direct_ix_data),
            "pumpfun_direct_instruction_accounts_count": len(target_accounts),
            "pumpfun_direct_selected_input_account_pubkey": text(input_account_doc.get("selected_pubkey")),
            "pumpfun_direct_selected_output_account_pubkey": text(output_account_doc.get("selected_pubkey")),
            "pumpfun_direct_actual_input_account_pubkey": text(input_account_doc.get("actual_pubkey")),
            "pumpfun_direct_actual_output_account_pubkey": text(output_account_doc.get("actual_pubkey")),
            "pumpfun_direct_derived_input_account_pubkey": text(input_account_doc.get("derived_pubkey")),
            "pumpfun_direct_derived_output_account_pubkey": text(output_account_doc.get("derived_pubkey")),
            "pumpfun_direct_input_account_ready": input_account_ready,
            "pumpfun_direct_output_account_ready": output_account_ready,
            "pumpfun_direct_input_account_count": safe_int(input_account_doc.get("actual_count")),
            "pumpfun_direct_output_account_count": safe_int(output_account_doc.get("actual_count")),
            "pumpfun_direct_input_actual_balance_raw": safe_int(input_account_doc.get("actual_balance_raw")),
            "pumpfun_direct_input_token_program": input_token_program,
            "pumpfun_direct_output_token_program": output_token_program,
            "pumpfun_direct_input_policy_pass": input_policy_pass,
            "pumpfun_direct_output_policy_pass": output_policy_pass,
            "pumpfun_direct_input_policy_blocker": input_policy_blocker,
            "pumpfun_direct_output_policy_blocker": output_policy_blocker,
            "pumpfun_direct_latest_blockhash": latest_blockhash,
            "pumpfun_direct_latest_blockhash_error": latest_blockhash_error or {},
            "decision": decision,
            "first_direct_route_blocker": first_blocker,
            "build_error": build_error,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

summary = {
    "status": "PUMPFUN_DIRECT_ROUTE_READY" if pumpfun_direct_tx_buildable_rows > 0 else (max(decision_counts, key=decision_counts.get) if decision_counts else "PARK_PUMPFUN_DIRECT_ROUTE_NO_ROWS"),
    "source_rows": str(prebr0_rows_path),
    "rows_seen": len(out_rows),
    "pumpfun_direct_route_exists_rows": pumpfun_direct_route_exists_rows,
    "pumpfun_direct_quote_available_rows": pumpfun_direct_quote_available_rows,
    "pumpfun_direct_min_out_positive_rows": pumpfun_direct_min_out_positive_rows,
    "pumpfun_direct_template_ready_rows": pumpfun_direct_template_ready_rows,
    "pumpfun_direct_tx_buildable_rows": pumpfun_direct_tx_buildable_rows,
    "pumpfun_direct_current_account_ready_rows": pumpfun_direct_current_account_ready_rows,
    "pumpfun_direct_hypothetical_only_rows": pumpfun_direct_hypothetical_only_rows,
    "decision_counts": dict(decision_counts),
    "owner_pubkey": owner_pubkey,
    "solders_ready": solders_ready,
    "solders_error": solders_error,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(prebr0_rows_path),
    "ls13a_rows": str(ls13a_rows_path),
    "alt_rows": str(alt_rows_path),
    "signer_path": str(signer_keypair_path or ""),
    "rpc_url": rpc_url,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for item in out_rows:
        fh.write(json.dumps(item, sort_keys=True) + "\n")
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

pass "RunLSPREBRPF Pump.fun direct route probe written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

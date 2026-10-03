#!/usr/bin/env bash
# 21_lsprebrinv_r1_one_mint_proposal.sh — build a one-mint R1 Backrun inventory-band proposal package without send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1 — One-Mint Proposal"

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
STEP="${SHADOW_LSPREBRINV_R1_STEP:-21_lsprebrinv_r1_one_mint_proposal}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_one_mint_proposal_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_one_mint_proposal_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PROPOSAL_JSON="$RESULT_DIR/r1_one_mint_proposal.json"
PROPOSAL_SUMMARY="$RESULT_DIR/proposal_summary.txt"

DIRECT_ROWS_PATH="${SHADOW_LSPREBRINV_R1_DIRECT_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LSPREBRINV_R1_SIM_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_tiny_inventory_band_sim/runlsprebrinv_tiny_inventory_band_sim_rows.jsonl}"
SIZE_LADDER_SUMMARY_PATH="${SHADOW_LSPREBRINV_R1_SIZE_LADDER_SUMMARY_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_exit_quote_size_ladder/runlsprebrinv_r1_exit_quote_size_ladder_summary.json}"
SIZE_LADDER_ROWS_PATH="${SHADOW_LSPREBRINV_R1_SIZE_LADDER_ROWS_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_exit_quote_size_ladder/runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRINV_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBRPF_SIGNER_KEYPAIR_PATH:-${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}}}}"
RPC_URL="${SHADOW_LSPREBRINV_R1_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

TARGET_MINT="${SHADOW_LSPREBRINV_R1_MINT:-HXnPCo2RAQ4FZYwKw1hhUvUPkGzxKKQp3a6Zw5SkYE5w}"
TARGET_POOL="${SHADOW_LSPREBRINV_R1_POOL:-HvMUfyV1oPAFq5WAkKu297f9NCyAzsP8VA6QhA3yqAzp}"
RISK_TIER="${SHADOW_LSPREBRINV_R1_RISK_TIER:-R1_BACKRUN_TINY_INVENTORY_BAND}"

MAX_SPEND_SOL="${SHADOW_LSPREBRINV_R1_MAX_SPEND_SOL:-0.005}"
MAX_LOSS_SOL="${SHADOW_LSPREBRINV_R1_MAX_LOSS_SOL:-}"
TTL_SECONDS="${SHADOW_LSPREBRINV_R1_TTL_SECONDS:-300}"
STOP_LOSS_BPS="${SHADOW_LSPREBRINV_R1_STOP_LOSS_BPS:-300}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LSPREBRINV_R1_PRIORITY_FEE_LAMPORTS:-0}"
SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_R1_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_R1_TIP_LAMPORTS:-0}}}"
SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-${SHADOW_LSPREBRINV_R1_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}}"
TIP_LAMPORTS="$SENDER_MIN_TIP_LAMPORTS"
MAX_LOSS_FLOOR_SOL="${SHADOW_LSPREBRINV_R1_MAX_LOSS_FLOOR_SOL:-0.0025}"
UNRECOVERED_RENT_BUFFER_SOL="${SHADOW_LSPREBRINV_R1_UNRECOVERED_RENT_BUFFER_SOL:-0.00005}"
RENT_RECOVERED_IN_MODEL="${SHADOW_LSPREBRINV_R1_RENT_RECOVERED_IN_MODEL:-false}"
ALLOW_TOKEN2022="${SHADOW_LSPREBRINV_R1_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_LSPREBRINV_R1_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_LSPREBRINV_R1_ALLOW_MINT_AUTHORITY:-false}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

latest_file_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$DIRECT_ROWS_PATH" ]] || DIRECT_ROWS_PATH="$(latest_rows_for_step runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_tiny_inventory_band_sim_rows.jsonl)"
[[ -f "$SIZE_LADDER_SUMMARY_PATH" ]] || SIZE_LADDER_SUMMARY_PATH="$(latest_file_for_step runlsprebrinv_r1_exit_quote_size_ladder_summary.json)"
[[ -f "$SIZE_LADDER_ROWS_PATH" ]] || SIZE_LADDER_ROWS_PATH="$(latest_rows_for_step runlsprebrinv_r1_exit_quote_size_ladder_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$PROPOSAL_JSON" "$PROPOSAL_SUMMARY"

python3 - "$STEP" "$DIRECT_ROWS_PATH" "$SIM_ROWS_PATH" "$SIZE_LADDER_SUMMARY_PATH" "$SIZE_LADDER_ROWS_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$PROPOSAL_JSON" "$PROPOSAL_SUMMARY" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$RISK_TIER" "$MAX_SPEND_SOL" "$MAX_LOSS_SOL" "$TTL_SECONDS" "$STOP_LOSS_BPS" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" "$TIP_LAMPORTS" "$SENDER_TIP_ACCOUNT" "$MAX_LOSS_FLOOR_SOL" "$UNRECOVERED_RENT_BUFFER_SOL" "$RENT_RECOVERED_IN_MODEL" "$ALLOW_TOKEN2022" "$ALLOW_FREEZE_AUTHORITY" "$ALLOW_MINT_AUTHORITY" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    step_name_arg,
    direct_rows_arg,
    sim_rows_arg,
    size_ladder_summary_arg,
    size_ladder_rows_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    proposal_arg,
    proposal_summary_arg,
    signer_path_arg,
    rpc_url,
    target_mint_arg,
    target_pool_arg,
    risk_tier_arg,
    max_spend_sol_arg,
    max_loss_sol_arg,
    ttl_seconds_arg,
    stop_loss_bps_arg,
    base_tx_fee_lamports_arg,
    priority_fee_lamports_arg,
    tip_lamports_arg,
    sender_tip_account_arg,
    max_loss_floor_sol_arg,
    unrecovered_rent_buffer_sol_arg,
    rent_recovered_in_model_arg,
    allow_token2022_arg,
    allow_freeze_authority_arg,
    allow_mint_authority_arg,
) = sys.argv[1:32]

step_name = str(step_name_arg or "21_lsprebrinv_r1_one_mint_proposal").strip() or "21_lsprebrinv_r1_one_mint_proposal"
direct_rows_path = Path(direct_rows_arg) if direct_rows_arg else Path("")
sim_rows_path = Path(sim_rows_arg) if sim_rows_arg else Path("")
size_ladder_summary_path = Path(size_ladder_summary_arg) if size_ladder_summary_arg else Path("")
size_ladder_rows_path = Path(size_ladder_rows_arg) if size_ladder_rows_arg else Path("")
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
proposal_path = Path(proposal_arg)
proposal_summary_path = Path(proposal_summary_arg)
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()
risk_tier = str(risk_tier_arg or "R1_BACKRUN_TINY_INVENTORY_BAND").strip() or "R1_BACKRUN_TINY_INVENTORY_BAND"
max_spend_sol = float(max_spend_sol_arg or "0.005")
configured_max_loss_sol = str(max_loss_sol_arg or "").strip()
ttl_seconds = max(1, int(float(ttl_seconds_arg or "300")))
stop_loss_bps = max(1, int(float(stop_loss_bps_arg or "300")))
base_tx_fee_lamports = max(0, int(float(base_tx_fee_lamports_arg or "5000")))
priority_fee_lamports = max(0, int(float(priority_fee_lamports_arg or "0")))
tip_lamports = max(0, int(float(tip_lamports_arg or "0")))
sender_tip_account = str(sender_tip_account_arg or "").strip()
max_loss_floor_sol = max(0.0, float(max_loss_floor_sol_arg or "0.0025"))
unrecovered_rent_buffer_sol = max(0.0, float(unrecovered_rent_buffer_sol_arg or "0.00005"))
rent_recovered_in_model = str(rent_recovered_in_model_arg).lower() in {"1", "true", "yes", "on"}
allow_token2022 = str(allow_token2022_arg).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(allow_freeze_authority_arg).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(allow_mint_authority_arg).lower() in {"1", "true", "yes", "on"}

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
SPL_TOKEN_ACCOUNT_LEN = 165
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}
HELIUS_ALLOWED_TIP_ACCOUNTS = {
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
}
TOKEN2022_EXTENSION_NAMES = {
    1: "transferFeeConfig",
    2: "transferFeeAmount",
    3: "mintCloseAuthority",
    4: "confidentialTransferMint",
    5: "confidentialTransferAccount",
    6: "defaultAccountState",
    7: "immutableOwner",
    8: "memoTransfer",
    9: "nonTransferable",
    10: "interestBearingConfig",
    11: "cpiGuard",
    12: "permanentDelegate",
    13: "nonTransferableAccount",
    14: "transferHook",
    15: "transferHookAccount",
    16: "confidentialTransferFeeConfig",
    17: "confidentialTransferFeeAmount",
    18: "metadataPointer",
    19: "tokenMetadata",
    20: "groupPointer",
    21: "tokenGroup",
    22: "groupMemberPointer",
    23: "tokenGroupMember",
    24: "scaledUiAmountConfig",
    25: "pausableConfig",
    26: "pausableAccount",
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


def load_json_file(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


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


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def fetch_ata_rent_lamports() -> int:
    result, error = rpc("getMinimumBalanceForRentExemption", [SPL_TOKEN_ACCOUNT_LEN])
    if error is not None:
        return 2_039_280
    value = (result or {}).get("result", result)
    return safe_int(value, 2_039_280)


def fetch_mint_account(mint: str) -> tuple[dict[str, Any], dict[str, Any] | None]:
    result, error = rpc("getAccountInfo", [mint, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return {"mint": mint, "metadata_available": False, "rpc_error": error}, error if isinstance(error, dict) else {"error": str(error)}
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        return {"mint": mint, "metadata_available": False, "rpc_error": {"error": "mint_value_missing"}}, {"error": "mint_value_missing"}
    owner = text(value.get("owner"))
    data_field = value.get("data")
    raw = b""
    if isinstance(data_field, list) and data_field:
        try:
            raw = base64.b64decode(str(data_field[0]), validate=True)
        except Exception:
            raw = b""
    mint_authority = ""
    freeze_authority = ""
    decimals = None
    if len(raw) >= 45:
        decimals = raw[44]
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_authority = b58(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_authority = b58(raw[50:82])
    extensions: list[str] = []
    cursor = 82
    if owner == TOKEN2022_PROGRAM:
        while cursor + 4 <= len(raw):
            ext_type = int.from_bytes(raw[cursor:cursor + 2], "little")
            ext_len = int.from_bytes(raw[cursor + 2:cursor + 4], "little")
            cursor += 4
            if ext_type == 0 and ext_len == 0:
                break
            extensions.append(TOKEN2022_EXTENSION_NAMES.get(ext_type, f"extension_{ext_type}"))
            cursor += max(0, ext_len)
    return (
        {
            "mint": mint,
            "metadata_available": True,
            "token_program": owner,
            "mint_authority": mint_authority,
            "freeze_authority": freeze_authority,
            "decimals": decimals,
            "extensions": sorted(set(extensions)),
        },
        None,
    )


def account_exists(pubkey: str) -> bool:
    if not pubkey:
        return False
    result, error = rpc("getAccountInfo", [pubkey, {"encoding": "base64", "commitment": "confirmed"}])
    if error is not None:
        return False
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    return isinstance(value, dict)


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


def policy_for_mint(metadata: dict[str, Any]) -> tuple[str, bool]:
    if not metadata.get("metadata_available"):
        return "metadata_unavailable", False
    freeze_authority = text(metadata.get("freeze_authority"))
    mint_authority = text(metadata.get("mint_authority"))
    if freeze_authority and not allow_freeze_authority:
        return "freeze_authority_present", False
    if mint_authority and not allow_mint_authority:
        return "mint_authority_present", False
    token_program = text(metadata.get("token_program"))
    extensions = {lower(item) for item in metadata.get("extensions") or []}
    dangerous_extensions = sorted(item for item in extensions if item in DANGEROUS_TOKEN2022_EXTENSIONS)
    if token_program == TOKEN_PROGRAM:
        return "pass", True
    if token_program == TOKEN2022_PROGRAM:
        if dangerous_extensions:
            return "unsupported_token2022_extensions", False
        if not allow_token2022:
            return "token2022_requires_explicit_allow", False
        return "pass", True
    return "unsupported_token_program", False


def pick_row(rows: list[dict[str, Any]], pool_key: str, mint_key: str, *mint_fields: str) -> dict[str, Any] | None:
    matches: list[dict[str, Any]] = []
    for row in rows:
        if pool_key and text(row.get("pool")) != pool_key:
            continue
        row_mints = {text(row.get(field)) for field in mint_fields if text(row.get(field))}
        if mint_key and mint_key not in row_mints:
            continue
        matches.append(row)
    if not matches:
        return None
    return matches[0]


direct_rows = load_jsonl(direct_rows_path)
sim_rows = load_jsonl(sim_rows_path)
size_ladder_summary = load_json_file(size_ladder_summary_path)
size_ladder_rows = load_jsonl(size_ladder_rows_path)
direct_row = pick_row(direct_rows, target_pool, target_mint, "our_input_mint", "our_output_mint", "candidate_mint")
sim_row = pick_row(sim_rows, target_pool, target_mint, "candidate_mint", "entry_output_mint")

decision_counts: Counter[str] = Counter()
rows_out: list[dict[str, Any]] = []

mint_metadata, mint_metadata_error = fetch_mint_account(target_mint) if target_mint else ({"metadata_available": False}, {"error": "mint_missing"})
mint_policy_blocker, mint_policy_pass = policy_for_mint(mint_metadata) if target_mint else ("mint_missing", False)
dangerous_extensions = sorted(
    item for item in (lower(extension) for extension in (mint_metadata.get("extensions") or []))
    if item in DANGEROUS_TOKEN2022_EXTENSIONS
)
dangerous_token2022_extensions = bool(dangerous_extensions)
token_program = text(mint_metadata.get("token_program")) or text((direct_row or {}).get("pumpfun_direct_input_token_program"))
freeze_authority = text(mint_metadata.get("freeze_authority"))
mint_authority = text(mint_metadata.get("mint_authority"))

ata_rent_lamports = fetch_ata_rent_lamports()
ata_creation_fee_lamports = base_tx_fee_lamports
entry_fee_lamports = base_tx_fee_lamports
flatten_fee_lamports = base_tx_fee_lamports
close_account_fee_lamports = base_tx_fee_lamports

owner_pubkey = text((sim_row or {}).get("owner_pubkey")) or text((direct_row or {}).get("owner_pubkey"))
entry_output_account_pubkey = text((sim_row or {}).get("entry_output_account_pubkey"))
entry_output_derived_pubkey = text((sim_row or {}).get("entry_output_derived_pubkey")) or text((direct_row or {}).get("pumpfun_direct_derived_input_account_pubkey"))
if entry_output_derived_pubkey or entry_output_account_pubkey:
    entry_output_account_exists = account_exists(entry_output_derived_pubkey or entry_output_account_pubkey)
else:
    entry_output_account_exists = boolish((sim_row or {}).get("entry_output_account_ready"))
entry_output_account_needed = True
ata_precreate_required = entry_output_account_needed and not entry_output_account_exists
ata_precreate_policy_clean = mint_policy_pass and not dangerous_token2022_extensions

close_account_possible = bool(
    target_mint
    and owner_pubkey
    and entry_output_derived_pubkey
    and token_program in {TOKEN_PROGRAM, TOKEN2022_PROGRAM}
    and mint_policy_pass
)
close_account_proof = (
    "signer_owned_standard_token_account_close_supported_after_flatten_to_zero"
    if close_account_possible
    else "close_account_not_proven"
)
close_account_recovery_lamports = ata_rent_lamports if close_account_possible else 0
rent_recovered_in_model = rent_recovered_in_model and close_account_possible
ata_rent_at_risk_lamports = ata_rent_lamports if ata_precreate_required and not rent_recovered_in_model else 0

immediate_roundtrip_loss_lamports = safe_int((sim_row or {}).get("immediate_roundtrip_loss_lamports"))
required_loss_budget_lamports = immediate_roundtrip_loss_lamports + ata_creation_fee_lamports + ata_rent_at_risk_lamports + priority_fee_lamports
required_loss_budget_with_buffer_lamports = required_loss_budget_lamports + sol_to_lamports(unrecovered_rent_buffer_sol)

configured_max_loss_lamports = sol_to_lamports(float(configured_max_loss_sol)) if configured_max_loss_sol else 0
sim_max_loss_lamports = safe_int((sim_row or {}).get("max_loss_lamports"))
recommended_max_loss_lamports = max(
    configured_max_loss_lamports,
    sim_max_loss_lamports,
    sol_to_lamports(max_loss_floor_sol),
    required_loss_budget_with_buffer_lamports,
)
effective_max_loss_lamports = recommended_max_loss_lamports
max_loss_budget_includes_unrecovered_ata_rent = effective_max_loss_lamports >= required_loss_budget_lamports
max_loss_budget_source = "env_override" if configured_max_loss_lamports > 0 else "derived_recommended"

owner_native_balance_raw = safe_int((sim_row or {}).get("owner_native_balance_raw"))
pm_reserve_inventory_ok = boolish((sim_row or {}).get("pm_reserve_inventory_ok"))
pm_release_ok = boolish((sim_row or {}).get("pm_release_ok"))
sender_tip_lamports = tip_lamports
sender_tip_account_allowed = sender_tip_account in HELIUS_ALLOWED_TIP_ACCOUNTS if sender_tip_account else False
sender_tip_instruction_present = bool(sender_tip_lamports > 0 and sender_tip_account_allowed)
sender_tip_total_lamports = 2 * sender_tip_lamports
pm_reserve_tip_ok = owner_native_balance_raw >= sender_tip_total_lamports
pm_reserve_fee_budget_lamports = ata_creation_fee_lamports + entry_fee_lamports + flatten_fee_lamports + (2 * priority_fee_lamports) + sender_tip_total_lamports
pm_reserve_fee_ok = owner_native_balance_raw >= pm_reserve_fee_budget_lamports
pm_reserve_rent_budget_lamports = pm_reserve_fee_budget_lamports + ata_rent_lamports
pm_reserve_rent_ok = owner_native_balance_raw >= pm_reserve_rent_budget_lamports
net_after_sender_tip_still_acceptable = immediate_roundtrip_loss_lamports <= effective_max_loss_lamports

pumpfun_direct_entry_quote_ok = boolish((sim_row or {}).get("direct_entry_quote_ok"))
pumpfun_direct_exit_quote_ok = boolish((sim_row or {}).get("direct_exit_quote_ok"))
auto_flatten_available = boolish((sim_row or {}).get("auto_flatten_available"))
exit_non_dust = boolish((sim_row or {}).get("exit_non_dust"))
entry_output_account_needed = True

size_ladder_exact_row = None
for row in size_ladder_rows:
    if text(row.get("mint")) != target_mint or text(row.get("pool")) != target_pool:
        continue
    try:
        row_size = float(row.get("size_sol"))
    except (TypeError, ValueError):
        continue
    if abs(row_size - max_spend_sol) <= 1e-12:
        size_ladder_exact_row = row
        break

size_ladder_ready_rows = sum(
    1
    for row in size_ladder_rows
    if text(row.get("mint")) == target_mint
    and text(row.get("pool")) == target_pool
    and text(row.get("decision")) == "SIZE_EXIT_READY"
)
size_ladder_has_ready_size = size_ladder_ready_rows > 0
size_ladder_best_ready_size_sol = text(size_ladder_summary.get("best_ready_size_sol")) or "none"
size_ladder_exact_size_found = size_ladder_exact_row is not None
size_ladder_exact_decision = text((size_ladder_exact_row or {}).get("decision")) or "missing"
size_ladder_exact_exit_expected_out_raw = safe_int((size_ladder_exact_row or {}).get("exit_expected_out_raw"))
size_ladder_exact_exit_min_out_raw = safe_int((size_ladder_exact_row or {}).get("exit_min_out_raw"))
size_ladder_exact_first_blocker = text((size_ladder_exact_row or {}).get("first_blocker")) or "size_ladder_exact_row_missing"
size_ladder_exact_exit_non_dust = size_ladder_exact_exit_expected_out_raw > 1 and size_ladder_exact_exit_min_out_raw > 0
r1_exit_nonzero_size_band_ready = (
    size_ladder_exact_size_found
    and size_ladder_exact_decision == "SIZE_EXIT_READY"
    and size_ladder_exact_exit_expected_out_raw > 0
    and size_ladder_exact_exit_min_out_raw > 0
    and size_ladder_exact_exit_non_dust
)

direct_route_ready = bool(
    direct_row
    and boolish(direct_row.get("pumpfun_direct_route_exists"))
    and boolish(direct_row.get("pumpfun_direct_quote_available"))
    and boolish(direct_row.get("pumpfun_direct_min_out_positive"))
    and boolish(direct_row.get("pumpfun_direct_template_ready"))
    and boolish(direct_row.get("pumpfun_direct_tx_buildable"))
)

if not direct_row or not sim_row:
    verdict = "PARK_DIRECT_ROUTE_STALE"
    first_blocker = "proposal_source_rows_missing"
elif not mint_policy_pass or dangerous_token2022_extensions:
    verdict = "PARK_POLICY"
    first_blocker = mint_policy_blocker or "policy_failed"
elif not direct_route_ready:
    verdict = "PARK_DIRECT_ROUTE_STALE"
    first_blocker = text((direct_row or {}).get("first_direct_route_blocker")) or "direct_route_not_ready"
elif not pumpfun_direct_entry_quote_ok or not pumpfun_direct_exit_quote_ok:
    verdict = "PARK_DIRECT_ROUTE_STALE"
    first_blocker = "direct_quote_stale_or_non_positive"
elif not r1_exit_nonzero_size_band_ready:
    verdict = "PARK_EXIT_SIZE_BAND_NOT_READY"
    first_blocker = size_ladder_exact_first_blocker
elif sender_tip_lamports > 0 and not sender_tip_account_allowed:
    verdict = "PARK_SENDER_TIP_ACCOUNT_INVALID"
    first_blocker = "sender_tip_account_not_allowed"
elif sender_tip_lamports > 0 and not sender_tip_instruction_present:
    verdict = "PARK_SENDER_TIP_MISSING"
    first_blocker = "sender_tip_instruction_missing"
elif not auto_flatten_available or not exit_non_dust:
    verdict = "PARK_EXIT_NOT_READY"
    first_blocker = "auto_flatten_or_exit_non_dust_failed"
elif not pm_reserve_inventory_ok or not pm_reserve_fee_ok or not pm_reserve_rent_ok or not pm_release_ok:
    verdict = "PARK_PM"
    first_blocker = "pm_reserve_or_release_failed"
elif sender_tip_lamports > 0 and not net_after_sender_tip_still_acceptable:
    verdict = "PARK_SENDER_TIP_ECONOMICS"
    first_blocker = "net_after_sender_tip_not_acceptable"
elif rent_recovered_in_model and not close_account_possible:
    verdict = "PARK_CLOSE_ACCOUNT_NOT_PROVEN"
    first_blocker = "close_account_recovery_requested_without_proof"
elif not max_loss_budget_includes_unrecovered_ata_rent:
    verdict = "PARK_RENT_NOT_ACCOUNTED"
    first_blocker = "unrecovered_ata_rent_exceeds_loss_budget"
elif ata_precreate_required:
    verdict = "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY"
    first_blocker = "entry_output_account_missing_requires_precreate"
else:
    verdict = "R1_PROPOSAL_READY_ONE_MINT_LIVE_WITH_SENDER_TIP" if sender_tip_lamports > 0 else "R1_PROPOSAL_READY_ONE_MINT_LIVE"
    first_blocker = "none"

decision_counts[verdict] += 1
candidate_id = text((sim_row or {}).get("candidate_id")) or text((direct_row or {}).get("candidate_id"))
proposal_row = {
    "record_type": "runlsprebrinv_r1_one_mint_proposal_row",
    "candidate_id": candidate_id,
    "risk_tier": risk_tier,
    "mint": target_mint,
    "pool": target_pool,
    "venue": text((sim_row or {}).get("venue")) or text((direct_row or {}).get("venue")) or "pumpfun_amm",
    "owner_pubkey": owner_pubkey,
    "mint_policy_pass": mint_policy_pass,
    "mint_policy_blocker": mint_policy_blocker,
    "freeze_authority": freeze_authority or None,
    "mint_authority": mint_authority or None,
    "dangerous_token2022_extensions": dangerous_token2022_extensions,
    "token_program": token_program,
    "token2022_extensions": mint_metadata.get("extensions") or [],
    "pumpfun_direct_entry_quote_ok": pumpfun_direct_entry_quote_ok,
    "pumpfun_direct_exit_quote_ok": pumpfun_direct_exit_quote_ok,
    "r1_exit_nonzero_size_band_ready": r1_exit_nonzero_size_band_ready,
    "size_ladder_has_ready_size": size_ladder_has_ready_size,
    "size_ladder_ready_rows": size_ladder_ready_rows,
    "size_ladder_best_ready_size_sol": size_ladder_best_ready_size_sol,
    "size_ladder_exact_size_found": size_ladder_exact_size_found,
    "size_ladder_exact_decision": size_ladder_exact_decision,
    "size_ladder_exact_exit_expected_out_raw": size_ladder_exact_exit_expected_out_raw,
    "size_ladder_exact_exit_min_out_raw": size_ladder_exact_exit_min_out_raw,
    "size_ladder_exact_exit_non_dust": size_ladder_exact_exit_non_dust,
    "size_ladder_exact_first_blocker": size_ladder_exact_first_blocker,
    "auto_flatten_available": auto_flatten_available,
    "exit_non_dust": exit_non_dust,
    "entry_output_account_exists": entry_output_account_exists,
    "entry_output_account_needed": entry_output_account_needed,
    "ata_precreate_required": ata_precreate_required,
    "ata_precreate_policy_clean": ata_precreate_policy_clean,
    "entry_output_account_pubkey": entry_output_account_pubkey or None,
    "entry_output_derived_pubkey": entry_output_derived_pubkey or None,
    "ata_rent_lamports": ata_rent_lamports,
    "ata_rent_sol": lamports_to_sol(ata_rent_lamports),
    "ata_creation_fee_lamports": ata_creation_fee_lamports,
    "ata_creation_fee_sol": lamports_to_sol(ata_creation_fee_lamports),
    "close_account_possible": close_account_possible,
    "close_account_proof": close_account_proof,
    "close_account_recovery_lamports": close_account_recovery_lamports,
    "close_account_fee_lamports": close_account_fee_lamports,
    "rent_recovered_in_model": rent_recovered_in_model,
    "ata_rent_at_risk_lamports": ata_rent_at_risk_lamports,
    "priority_fee_lamports": priority_fee_lamports,
    "tip_lamports": tip_lamports,
    "sender_tip_lamports": sender_tip_lamports,
    "sender_tip_total_lamports": sender_tip_total_lamports,
    "sender_tip_account": sender_tip_account,
    "sender_tip_instruction_present": sender_tip_instruction_present,
    "sender_tip_account_allowed": sender_tip_account_allowed,
    "pm_reserve_tip_ok": pm_reserve_tip_ok,
    "max_loss_budget_includes_sender_tip": effective_max_loss_lamports >= sender_tip_total_lamports,
    "net_after_sender_tip_still_acceptable": net_after_sender_tip_still_acceptable,
    "entry_fee_lamports": entry_fee_lamports,
    "flatten_fee_lamports": flatten_fee_lamports,
    "immediate_roundtrip_loss_lamports": immediate_roundtrip_loss_lamports,
    "immediate_roundtrip_loss_sol": lamports_to_sol(immediate_roundtrip_loss_lamports),
    "required_loss_budget_lamports": required_loss_budget_lamports,
    "required_loss_budget_with_buffer_lamports": required_loss_budget_with_buffer_lamports,
    "max_spend_sol": max_spend_sol,
    "max_spend_lamports": sol_to_lamports(max_spend_sol),
    "max_loss_sol": lamports_to_sol(effective_max_loss_lamports),
    "max_loss_lamports": effective_max_loss_lamports,
    "max_loss_budget_source": max_loss_budget_source,
    "max_loss_budget_includes_unrecovered_ata_rent": max_loss_budget_includes_unrecovered_ata_rent,
    "ttl_seconds": ttl_seconds,
    "stop_loss_bps": stop_loss_bps,
    "pm_reserve_inventory_ok": pm_reserve_inventory_ok,
    "pm_reserve_fee_ok": pm_reserve_fee_ok,
    "pm_reserve_rent_ok": pm_reserve_rent_ok,
    "pm_reserve_fee_budget_lamports": pm_reserve_fee_budget_lamports,
    "pm_reserve_rent_budget_lamports": pm_reserve_rent_budget_lamports,
    "pm_release_ok": pm_release_ok,
    "decision": verdict,
    "first_proposal_blocker": first_blocker,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
rows_out.append(proposal_row)

source_artifacts = {
    "direct_route_rows": str(direct_rows_path) if direct_rows_path.exists() else "",
    "tiny_inventory_band_rows": str(sim_rows_path) if sim_rows_path.exists() else "",
    "size_ladder_summary": str(size_ladder_summary_path) if size_ladder_summary_path.exists() else "",
    "size_ladder_rows": str(size_ladder_rows_path) if size_ladder_rows_path.exists() else "",
}
proposal_doc = {
    "package_name": step_name,
    "risk_tier": risk_tier,
    "final_verdict": verdict,
    "final_decision": "READY" if verdict.startswith("R1_PROPOSAL_READY") else "PARK",
    "first_blocker_if_rejected": first_blocker,
    "mint": target_mint,
    "pool": target_pool,
    "venue": proposal_row["venue"],
    "candidate_id": candidate_id,
    "source_artifacts": source_artifacts,
    **proposal_row,
}

summary = {
    "status": verdict,
    "risk_tier": risk_tier,
    "proposal_ready_rows": 1 if verdict.startswith("R1_PROPOSAL_READY") else 0,
    "ata_precreate_only_ready_rows": 1 if verdict == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY" else 0,
    "one_mint_live_ready_rows": 1 if verdict == "R1_PROPOSAL_READY_ONE_MINT_LIVE" else 0,
    "decision_counts": dict(decision_counts),
    "mint": target_mint,
    "pool": target_pool,
    "ata_precreate_required": ata_precreate_required,
    "close_account_possible": close_account_possible,
    "rent_recovered_in_model": rent_recovered_in_model,
    "max_loss_budget_includes_unrecovered_ata_rent": max_loss_budget_includes_unrecovered_ata_rent,
    "r1_exit_nonzero_size_band_ready": r1_exit_nonzero_size_band_ready,
    "size_ladder_exact_decision": size_ladder_exact_decision,
    "size_ladder_best_ready_size_sol": size_ladder_best_ready_size_sol,
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {
    "direct_rows_path": str(direct_rows_path),
    "sim_rows_path": str(sim_rows_path),
    "size_ladder_summary_path": str(size_ladder_summary_path),
    "size_ladder_rows_path": str(size_ladder_rows_path),
    "signer_path": str(signer_keypair_path or ""),
    "rpc_url": rpc_url,
    "target_mint": target_mint,
    "target_pool": target_pool,
    "risk_tier": risk_tier,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in rows_out:
        fh.write(json.dumps(row, indent=None, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
proposal_path.write_text(json.dumps(proposal_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary_lines = [
    f"status={verdict}",
    f"risk_tier={risk_tier}",
    f"mint={target_mint}",
    f"pool={target_pool}",
    f"ata_precreate_required={str(ata_precreate_required).lower()}",
    f"r1_exit_nonzero_size_band_ready={str(r1_exit_nonzero_size_band_ready).lower()}",
    f"size_ladder_exact_decision={size_ladder_exact_decision}",
    f"ata_rent_lamports={ata_rent_lamports}",
    f"ata_creation_fee_lamports={ata_creation_fee_lamports}",
    f"close_account_possible={str(close_account_possible).lower()}",
    f"rent_recovered_in_model={str(rent_recovered_in_model).lower()}",
    f"max_loss_budget_includes_unrecovered_ata_rent={str(max_loss_budget_includes_unrecovered_ata_rent).lower()}",
    f"sender_tip_lamports={sender_tip_lamports}",
    f"sender_tip_account={sender_tip_account or 'none'}",
    f"sender_tip_instruction_present={str(sender_tip_instruction_present).lower()}",
    f"sender_tip_account_allowed={str(sender_tip_account_allowed).lower()}",
    f"pm_reserve_tip_ok={str(pm_reserve_tip_ok).lower()}",
    f"max_loss_budget_includes_sender_tip={str((effective_max_loss_lamports >= sender_tip_total_lamports)).lower()}",
    f"net_after_sender_tip_still_acceptable={str(net_after_sender_tip_still_acceptable).lower()}",
    f"wire_send=0",
    f"live_landed=0",
    f"send_transaction_called=false",
    f"sendBundle_called=false",
]
summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")

proposal_summary_lines = [
    f"package={proposal_doc['package_name']}",
    f"risk_tier={risk_tier}",
    f"final_verdict={verdict}",
    f"final_decision={proposal_doc['final_decision']}",
    f"first_blocker_if_rejected={first_blocker}",
    f"mint={target_mint}",
    f"pool={target_pool}",
    f"venue={proposal_row['venue']}",
    f"mint_policy_pass={str(mint_policy_pass).lower()}",
    f"freeze_authority={freeze_authority or 'none'}",
    f"mint_authority={mint_authority or 'none'}",
    f"dangerous_token2022_extensions={str(dangerous_token2022_extensions).lower()}",
    f"pumpfun_direct_entry_quote_ok={str(pumpfun_direct_entry_quote_ok).lower()}",
    f"pumpfun_direct_exit_quote_ok={str(pumpfun_direct_exit_quote_ok).lower()}",
    f"sender_tip_lamports={sender_tip_lamports}",
    f"sender_tip_account={sender_tip_account or 'none'}",
    f"sender_tip_instruction_present={str(sender_tip_instruction_present).lower()}",
    f"sender_tip_account_allowed={str(sender_tip_account_allowed).lower()}",
    f"pm_reserve_tip_ok={str(pm_reserve_tip_ok).lower()}",
    f"max_loss_budget_includes_sender_tip={str((effective_max_loss_lamports >= sender_tip_total_lamports)).lower()}",
    f"net_after_sender_tip_still_acceptable={str(net_after_sender_tip_still_acceptable).lower()}",
    f"r1_exit_nonzero_size_band_ready={str(r1_exit_nonzero_size_band_ready).lower()}",
    f"size_ladder_has_ready_size={str(size_ladder_has_ready_size).lower()}",
    f"size_ladder_best_ready_size_sol={size_ladder_best_ready_size_sol}",
    f"size_ladder_exact_decision={size_ladder_exact_decision}",
    f"size_ladder_exact_exit_expected_out_raw={size_ladder_exact_exit_expected_out_raw}",
    f"size_ladder_exact_exit_min_out_raw={size_ladder_exact_exit_min_out_raw}",
    f"auto_flatten_available={str(auto_flatten_available).lower()}",
    f"exit_non_dust={str(exit_non_dust).lower()}",
    f"entry_output_account_exists={str(entry_output_account_exists).lower()}",
    f"entry_output_account_needed=true",
    f"ata_precreate_required={str(ata_precreate_required).lower()}",
    f"ata_precreate_policy_clean={str(ata_precreate_policy_clean).lower()}",
    f"ata_rent_lamports={ata_rent_lamports}",
    f"ata_creation_fee_lamports={ata_creation_fee_lamports}",
    f"close_account_possible={str(close_account_possible).lower()}",
    f"rent_recovered_in_model={str(rent_recovered_in_model).lower()}",
    f"max_spend_sol={max_spend_sol}",
    f"max_loss_sol={lamports_to_sol(effective_max_loss_lamports)}",
    f"ttl_seconds={ttl_seconds}",
    f"stop_loss_bps={stop_loss_bps}",
    f"immediate_roundtrip_loss_sol={lamports_to_sol(immediate_roundtrip_loss_lamports)}",
    f"pm_reserve_inventory_ok={str(pm_reserve_inventory_ok).lower()}",
    f"pm_reserve_fee_ok={str(pm_reserve_fee_ok).lower()}",
    f"pm_reserve_rent_ok={str(pm_reserve_rent_ok).lower()}",
    f"pm_release_ok={str(pm_release_ok).lower()}",
    f"max_loss_budget_includes_unrecovered_ata_rent={str(max_loss_budget_includes_unrecovered_ata_rent).lower()}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
proposal_summary_path.write_text("\n".join(proposal_summary_lines) + "\n", encoding="utf-8")
PY

pass "RunLSPREBRINV R1 proposal written to $PROPOSAL_SUMMARY"
cat "$PROPOSAL_SUMMARY"
finish

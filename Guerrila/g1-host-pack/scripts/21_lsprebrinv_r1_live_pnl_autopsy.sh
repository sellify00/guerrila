#!/usr/bin/env bash
# 21_lsprebrinv_r1_live_pnl_autopsy.sh — realized PnL autopsy for one landed R1 inventory-band loop

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1A — Live PnL Autopsy"

assert_root
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
STEP="${SHADOW_LSPREBRINV_R1A_STEP:-21_lsprebrinv_r1_live_pnl_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
REPORT_JSON="$RESULT_DIR/runlsprebrinv_r1_live_pnl_autopsy.json"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_live_pnl_autopsy_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

LIVE_REPORT_JSON_PATH="${SHADOW_LSPREBRINV_R1A_LIVE_REPORT_JSON_PATH:-}"
RPC_URL="${SHADOW_LSPREBRINV_R1A_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
TARGET_MINT="${SHADOW_R1_MINT:-}"
TARGET_POOL="${SHADOW_R1_POOL:-}"
WSOL_MINT="So11111111111111111111111111111111111111112"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBRINV_R1A_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
SENDER_TIP_ACCOUNT="${SHADOW_HELIUS_SENDER_TIP_ACCOUNT:-${SHADOW_LSPREBRINV_R1L_SENDER_TIP_ACCOUNT:-5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn}}"
SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-${SHADOW_LSPREBRINV_R1L_SENDER_MIN_TIP_LAMPORTS:-0}}"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$RESULTS_ROOT" "$INPUT_MANIFEST" "$REPORT_JSON" "$SUMMARY_JSON" "$SUMMARY_TXT" "$LIVE_REPORT_JSON_PATH" "$RPC_URL" "$TARGET_MINT" "$TARGET_POOL" "$WSOL_MINT" "$SIGNER_KEYPAIR_PATH" "$SENDER_TIP_ACCOUNT" "$SENDER_MIN_TIP_LAMPORTS" <<'PY'
from __future__ import annotations

import base64
import json
import re
import ssl
import sys
import urllib.request
from pathlib import Path
from typing import Any

(
    results_root_arg,
    manifest_arg,
    report_arg,
    summary_json_arg,
    summary_txt_arg,
    live_report_path_arg,
    rpc_url,
    target_mint_arg,
    target_pool_arg,
    wsol_mint,
    signer_keypair_path_arg,
    sender_tip_account_arg,
    sender_min_tip_lamports_arg,
) = sys.argv[1:14]

results_root = Path(results_root_arg)
manifest_path = Path(manifest_arg)
report_path = Path(report_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
live_report_path = Path(str(live_report_path_arg).strip()).expanduser() if str(live_report_path_arg).strip() else None
target_mint = str(target_mint_arg or "").strip()
target_pool = str(target_pool_arg or "").strip()
sender_tip_account = str(sender_tip_account_arg or "").strip()
sender_min_tip_lamports = int(float(sender_min_tip_lamports_arg or "0"))

PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def b58encode(raw: bytes) -> str:
    zeros = 0
    for byte in raw:
        if byte == 0:
            zeros += 1
        else:
            break
    num = int.from_bytes(raw, "big")
    chars: list[str] = []
    while num > 0:
        num, rem = divmod(num, 58)
        chars.append(ALPHABET[rem])
    return ("1" * zeros) + "".join(reversed(chars or ["1"]))


def http_post_json(url: str, payload: dict[str, Any]):
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    context = ssl.create_default_context() if url.startswith("https://") else None
    with urllib.request.urlopen(req, timeout=30, context=context) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(method: str, params: list[Any] | None = None) -> dict[str, Any]:
    payload: dict[str, Any] = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    response = http_post_json(rpc_url, payload)
    if isinstance(response, dict) and response.get("error") is not None:
        raise RuntimeError(f"{method}_failed:{response['error']}")
    return response


def find_latest_live_report(root: Path, mint: str, pool: str) -> Path:
    matches: list[Path] = []
    any_matches: list[Path] = []
    for path in root.rglob("runlsprebrinv_r1_one_mint_live_execute.json"):
        try:
            obj = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            continue
        if mint and text(obj.get("target_mint")) != mint:
            continue
        if pool and text(obj.get("target_pool")) != pool:
            continue
        any_matches.append(path)
        if text(obj.get("acquisition_tx_signature")) and text(obj.get("flatten_tx_signature")):
            matches.append(path)
    if matches:
        return max(matches, key=lambda item: item.stat().st_mtime)
    if any_matches:
        return max(any_matches, key=lambda item: item.stat().st_mtime)
    raise RuntimeError("live_report_missing")


def candidate_live_reports(root: Path, mint: str, pool: str) -> list[Path]:
    matches: list[Path] = []
    for path in root.rglob("runlsprebrinv_r1_one_mint_live_execute.json"):
        try:
            obj = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            continue
        if mint and text(obj.get("target_mint")) != mint:
            continue
        if pool and text(obj.get("target_pool")) != pool:
            continue
        matches.append(path)
    return sorted(matches, key=lambda item: item.stat().st_mtime, reverse=True)


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str]:
    if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pumpfun_pool_account_too_short"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pumpfun_pool_discriminator_mismatch"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, ""


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def get_multiple_accounts(pubkeys: list[str]) -> dict[str, bytes]:
    result = (rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "confirmed"}]).get("result") or {})
    values = result.get("value") or []
    out: dict[str, bytes] = {}
    for key, value in zip(pubkeys, values):
        raw = b""
        if isinstance(value, dict):
            data = value.get("data")
            if isinstance(data, list) and data:
                raw = base64.b64decode(text(data[0]).encode("ascii"), validate=False)
        out[key] = raw
    return out


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if reserve_in <= 0 or reserve_out <= 0 or amount_in <= 0:
        return 0
    numerator = reserve_out * amount_in
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return numerator // denominator


def parse_owner_pubkey(report: dict[str, Any]) -> str:
    for field_name in ("owner_pubkey", "worker_pubkey", "signer_pubkey"):
        candidate = text(report.get(field_name))
        if candidate:
            return candidate
    for log_name in ("refresh_log_after_acquisition", "refresh_log_after_flatten"):
        log = text(report.get(log_name))
        match = re.search(r"owner_pubkey=([1-9A-HJ-NP-Za-km-z]{32,44})", log)
        if match:
            return match.group(1)
    raise RuntimeError("owner_pubkey_missing")


def parse_pool_vaults(pool_pubkey: str) -> dict[str, Any]:
    account_raw = get_multiple_accounts([pool_pubkey]).get(pool_pubkey, b"")
    pool_doc, pool_error = decode_pumpfun_pool(account_raw)
    if pool_error or not pool_doc:
        raise RuntimeError(pool_error or "pool_decode_failed")
    return pool_doc


def get_tx(signature: str) -> dict[str, Any]:
    return (rpc("getTransaction", [signature, {"encoding": "json", "maxSupportedTransactionVersion": 0, "commitment": "confirmed"}]).get("result") or {})


def token_balance_maps(tx: dict[str, Any]) -> tuple[dict[int, dict[str, Any]], dict[int, dict[str, Any]]]:
    pre = {safe_int(item.get("accountIndex")): item for item in (tx.get("meta") or {}).get("preTokenBalances", [])}
    post = {safe_int(item.get("accountIndex")): item for item in (tx.get("meta") or {}).get("postTokenBalances", [])}
    return pre, post


def token_amount(item: dict[str, Any] | None) -> int:
    ui = (item or {}).get("uiTokenAmount") or {}
    return safe_int(ui.get("amount"))


def owner_token_delta(tx: dict[str, Any], owner: str, mint: str) -> int:
    pre, post = token_balance_maps(tx)
    indexes = set(pre) | set(post)
    total_pre = 0
    total_post = 0
    for idx in indexes:
        pre_item = pre.get(idx)
        post_item = post.get(idx)
        owner_match = text((pre_item or post_item or {}).get("owner")) == owner
        mint_match = text((pre_item or post_item or {}).get("mint")) == mint
        if owner_match and mint_match:
            total_pre += token_amount(pre_item)
            total_post += token_amount(post_item)
    return total_post - total_pre


def owner_native_delta(tx: dict[str, Any], owner: str) -> int:
    keys = ((tx.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
    pre = (tx.get("meta") or {}).get("preBalances") or []
    post = (tx.get("meta") or {}).get("postBalances") or []
    for idx, key in enumerate(keys):
        if text(key) == owner:
            return safe_int(post[idx]) - safe_int(pre[idx])
    return 0


def tip_delta(tx: dict[str, Any], tip_account: str, fallback_tip: int) -> int:
    if not tip_account:
        return fallback_tip
    keys = ((tx.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
    pre = (tx.get("meta") or {}).get("preBalances") or []
    post = (tx.get("meta") or {}).get("postBalances") or []
    for idx, key in enumerate(keys):
        if text(key) == tip_account:
            delta = safe_int(post[idx]) - safe_int(pre[idx])
            return max(0, delta)
    return fallback_tip


def created_account_rent(tx: dict[str, Any]) -> tuple[int, list[dict[str, Any]]]:
    keys = ((tx.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
    pre = (tx.get("meta") or {}).get("preBalances") or []
    post = (tx.get("meta") or {}).get("postBalances") or []
    total = 0
    rows: list[dict[str, Any]] = []
    for idx, key in enumerate(keys):
        pre_amt = safe_int(pre[idx]) if idx < len(pre) else 0
        post_amt = safe_int(post[idx]) if idx < len(post) else 0
        if pre_amt == 0 and post_amt > 0:
            total += post_amt
            rows.append({"account": text(key), "lamports": post_amt})
    return total, rows


def fee_token_delta_raw(tx: dict[str, Any], owner: str, pool: str, mint: str) -> int:
    pre, post = token_balance_maps(tx)
    indexes = set(pre) | set(post)
    total = 0
    for idx in indexes:
        pre_item = pre.get(idx)
        post_item = post.get(idx)
        item = post_item or pre_item or {}
        if text(item.get("mint")) != mint:
            continue
        item_owner = text(item.get("owner"))
        if item_owner in {owner, pool}:
            continue
        total += max(0, token_amount(post_item) - token_amount(pre_item))
    return total


def pool_reserves_from_tx(tx: dict[str, Any], pool: str, base_mint: str, quote_mint: str) -> tuple[int, int]:
    pre, _ = token_balance_maps(tx)
    indexes = set(pre)
    base_amount = 0
    quote_amount = 0
    for idx in indexes:
        item = pre.get(idx) or {}
        if text(item.get("owner")) != pool:
            continue
        if text(item.get("mint")) == base_mint:
            base_amount += token_amount(item)
        elif text(item.get("mint")) == quote_mint:
            quote_amount += token_amount(item)
    return base_amount, quote_amount


def latest_or_specified_live_report() -> Path:
    if live_report_path is not None and live_report_path.is_file():
        return live_report_path
    return find_latest_live_report(results_root, target_mint, target_pool)


live_report = load_json(latest_or_specified_live_report())
owner_pubkey = parse_owner_pubkey(live_report)
target_mint = target_mint or text(live_report.get("target_mint"))
target_pool = target_pool or text(live_report.get("target_pool"))
acquisition_signature = text(live_report.get("acquisition_tx_signature"))
flatten_signature = text(live_report.get("flatten_tx_signature"))
if not acquisition_signature or not flatten_signature:
    for path in candidate_live_reports(results_root, target_mint, target_pool):
        obj = load_json(path)
        if not acquisition_signature:
            candidate_sig = text(obj.get("acquisition_tx_signature"))
            if candidate_sig and candidate_sig != "none":
                acquisition_signature = candidate_sig
        if not flatten_signature:
            candidate_sig = text(obj.get("flatten_tx_signature"))
            if candidate_sig and candidate_sig != "none":
                flatten_signature = candidate_sig
        if acquisition_signature and flatten_signature:
            break
if not acquisition_signature or not flatten_signature:
    raise RuntimeError("landed_signatures_missing")

pool_doc = parse_pool_vaults(target_pool)
quote_mint = pool_doc["base_mint"] if pool_doc["base_mint"] == wsol_mint else pool_doc["quote_mint"]

acq_tx = get_tx(acquisition_signature)
flat_tx = get_tx(flatten_signature)

buy_quote_in_lamports = max(0, -owner_token_delta(acq_tx, owner_pubkey, wsol_mint))
buy_token_out_raw = max(0, owner_token_delta(acq_tx, owner_pubkey, target_mint))
sell_quote_out_lamports = max(0, owner_token_delta(flat_tx, owner_pubkey, wsol_mint))
sell_token_in_raw = max(0, -owner_token_delta(flat_tx, owner_pubkey, target_mint))
owner_native_total_delta = owner_native_delta(acq_tx, owner_pubkey) + owner_native_delta(flat_tx, owner_pubkey)
owner_wsol_total_delta = owner_token_delta(acq_tx, owner_pubkey, wsol_mint) + owner_token_delta(flat_tx, owner_pubkey, wsol_mint)

entry_price = (buy_quote_in_lamports / buy_token_out_raw) if buy_token_out_raw > 0 else 0.0
exit_price = (sell_quote_out_lamports / sell_token_in_raw) if sell_token_in_raw > 0 else 0.0

gross_price_pnl_lamports = sell_quote_out_lamports - buy_quote_in_lamports
buy_sender_tip_lamports = tip_delta(acq_tx, sender_tip_account, sender_min_tip_lamports)
sell_sender_tip_lamports = tip_delta(flat_tx, sender_tip_account, sender_min_tip_lamports)
buy_base_fee_lamports = safe_int((acq_tx.get("meta") or {}).get("fee"))
sell_base_fee_lamports = safe_int((flat_tx.get("meta") or {}).get("fee"))
priority_fee_lamports = 0

entry_fee_token_raw = fee_token_delta_raw(acq_tx, owner_pubkey, target_pool, target_mint)
exit_fee_token_raw = fee_token_delta_raw(flat_tx, owner_pubkey, target_pool, target_mint)
entry_pumpfun_fee_lamports = round(entry_fee_token_raw * entry_price)
exit_pumpfun_fee_lamports = round(exit_fee_token_raw * exit_price)
pumpfun_fee_lamports = entry_pumpfun_fee_lamports + exit_pumpfun_fee_lamports

acq_created_rent_lamports, acq_created_accounts = created_account_rent(acq_tx)
flat_created_rent_lamports, flat_created_accounts = created_account_rent(flat_tx)
observed_created_account_lamports = acq_created_rent_lamports + flat_created_rent_lamports
account_creation_rent_lamports = max(
    0,
    -(owner_native_total_delta + buy_sender_tip_lamports + sell_sender_tip_lamports + buy_base_fee_lamports + sell_base_fee_lamports + priority_fee_lamports),
)

entry_base_reserve_raw, entry_quote_reserve_raw = pool_reserves_from_tx(acq_tx, target_pool, wsol_mint, target_mint)
entry_theoretical_total_token_out_raw = constant_product_out(entry_base_reserve_raw, entry_quote_reserve_raw, buy_quote_in_lamports)
entry_actual_total_token_out_raw = buy_token_out_raw + entry_fee_token_raw
entry_execution_shortfall_token_raw = max(0, entry_theoretical_total_token_out_raw - entry_actual_total_token_out_raw)
entry_execution_shortfall_lamports = round(entry_execution_shortfall_token_raw * entry_price)

exit_quote_reserve_raw, exit_base_reserve_raw = pool_reserves_from_tx(flat_tx, target_pool, target_mint, wsol_mint)
exit_theoretical_quote_out_raw = constant_product_out(exit_quote_reserve_raw, exit_base_reserve_raw, sell_token_in_raw)
exit_execution_shortfall_lamports = max(0, exit_theoretical_quote_out_raw - sell_quote_out_lamports)
slippage_loss_lamports = entry_execution_shortfall_lamports + exit_execution_shortfall_lamports

reported_total_realized_pnl_lamports = safe_int(live_report.get("pnl_lamports"))
chain_total_realized_pnl_lamports = owner_native_total_delta + owner_wsol_total_delta
native_fixed_drag_lamports = max(0, -owner_native_total_delta)
reconstructed_chain_total_pnl_lamports = (
    gross_price_pnl_lamports
    - buy_sender_tip_lamports
    - sell_sender_tip_lamports
    - buy_base_fee_lamports
    - sell_base_fee_lamports
    - priority_fee_lamports
    - account_creation_rent_lamports
)
reconciliation_residual_lamports = chain_total_realized_pnl_lamports - reconstructed_chain_total_pnl_lamports
rounding_loss_lamports = max(0, abs(reconciliation_residual_lamports))
reported_executor_pnl_gap_lamports = chain_total_realized_pnl_lamports - reported_total_realized_pnl_lamports
total_realized_pnl_lamports = chain_total_realized_pnl_lamports

report = {
    "status": "R1_LIVE_PNL_AUTOPSY_READY",
    "live_report_json_path": str(latest_or_specified_live_report()),
    "rpc_url": rpc_url,
    "owner_pubkey": owner_pubkey,
    "mint": target_mint,
    "pool": target_pool,
    "acquisition_tx_signature": acquisition_signature,
    "flatten_tx_signature": flatten_signature,
    "entry_price": entry_price,
    "exit_price": exit_price,
    "buy_quote_in_lamports": buy_quote_in_lamports,
    "buy_token_out_raw": buy_token_out_raw,
    "sell_quote_out_lamports": sell_quote_out_lamports,
    "sell_token_in_raw": sell_token_in_raw,
    "owner_native_total_delta_lamports": owner_native_total_delta,
    "owner_wsol_total_delta_lamports": owner_wsol_total_delta,
    "native_fixed_drag_lamports": native_fixed_drag_lamports,
    "gross_price_pnl_lamports": gross_price_pnl_lamports,
    "buy_sender_tip_lamports": buy_sender_tip_lamports,
    "sell_sender_tip_lamports": sell_sender_tip_lamports,
    "buy_base_fee_lamports": buy_base_fee_lamports,
    "sell_base_fee_lamports": sell_base_fee_lamports,
    "priority_fee_lamports": priority_fee_lamports,
    "pumpfun_fee_lamports": pumpfun_fee_lamports,
    "entry_fee_token_raw": entry_fee_token_raw,
    "exit_fee_token_raw": exit_fee_token_raw,
    "entry_pumpfun_fee_lamports": entry_pumpfun_fee_lamports,
    "exit_pumpfun_fee_lamports": exit_pumpfun_fee_lamports,
    "slippage_loss_lamports": slippage_loss_lamports,
    "entry_execution_shortfall_lamports": entry_execution_shortfall_lamports,
    "exit_execution_shortfall_lamports": exit_execution_shortfall_lamports,
    "rounding_loss_lamports": rounding_loss_lamports,
    "account_creation_rent_lamports": account_creation_rent_lamports,
    "pump_account_setup_lamports": account_creation_rent_lamports,
    "pump_account_setup_recoverable": False,
    "observed_created_account_lamports": observed_created_account_lamports,
    "acquisition_created_accounts": acq_created_accounts,
    "flatten_created_accounts": flat_created_accounts,
    "entry_theoretical_total_token_out_raw": entry_theoretical_total_token_out_raw,
    "entry_actual_total_token_out_raw": entry_actual_total_token_out_raw,
    "exit_theoretical_quote_out_raw": exit_theoretical_quote_out_raw,
    "total_realized_pnl_lamports": total_realized_pnl_lamports,
    "reported_total_realized_pnl_lamports": reported_total_realized_pnl_lamports,
    "reported_executor_pnl_gap_lamports": reported_executor_pnl_gap_lamports,
    "reconstructed_chain_total_pnl_lamports": reconstructed_chain_total_pnl_lamports,
    "reconciliation_residual_lamports": reconciliation_residual_lamports,
}

manifest = {
    "live_report_json_path": str(latest_or_specified_live_report()),
    "rpc_url": rpc_url,
    "mint": target_mint,
    "pool": target_pool,
    "signer_keypair_path": signer_keypair_path_arg,
    "sender_tip_account": sender_tip_account,
    "sender_min_tip_lamports": sender_min_tip_lamports,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            "status=R1_LIVE_PNL_AUTOPSY_READY",
            f"mint={target_mint}",
            f"pool={target_pool}",
            f"entry_price={entry_price}",
            f"exit_price={exit_price}",
            f"gross_price_pnl_lamports={gross_price_pnl_lamports}",
            f"buy_sender_tip_lamports={buy_sender_tip_lamports}",
            f"sell_sender_tip_lamports={sell_sender_tip_lamports}",
            f"buy_base_fee_lamports={buy_base_fee_lamports}",
            f"sell_base_fee_lamports={sell_base_fee_lamports}",
            f"priority_fee_lamports={priority_fee_lamports}",
            f"pumpfun_fee_lamports={pumpfun_fee_lamports}",
            f"entry_pumpfun_fee_lamports={entry_pumpfun_fee_lamports}",
            f"exit_pumpfun_fee_lamports={exit_pumpfun_fee_lamports}",
            f"slippage_loss_lamports={slippage_loss_lamports}",
            f"entry_execution_shortfall_lamports={entry_execution_shortfall_lamports}",
            f"exit_execution_shortfall_lamports={exit_execution_shortfall_lamports}",
            f"rounding_loss_lamports={rounding_loss_lamports}",
            f"account_creation_rent_lamports={account_creation_rent_lamports}",
            f"pump_account_setup_lamports={account_creation_rent_lamports}",
            "pump_account_setup_recoverable=false",
            f"native_fixed_drag_lamports={native_fixed_drag_lamports}",
            f"total_realized_pnl_lamports={total_realized_pnl_lamports}",
            f"reported_total_realized_pnl_lamports={reported_total_realized_pnl_lamports}",
            f"reported_executor_pnl_gap_lamports={reported_executor_pnl_gap_lamports}",
            f"reconciliation_residual_lamports={reconciliation_residual_lamports}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV R1 live PnL autopsy written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

#!/usr/bin/env bash
# 21_lsprebr2_preprocessed_backrun_executable_shape_filter.sh — classify projected preprocessed Backrun rows by executable input shape

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR2 — Executable Shape Filter"

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
STEP="${SHADOW_LSPREBR2_STEP:-21_lsprebr2_preprocessed_backrun_executable_shape_filter}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebr2_preprocessed_backrun_executable_shape_filter_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
PREBR0_ROWS_PATH="${SHADOW_LSPREBR2_PREBR0_ROWS_PATH:-$RESULTS_ROOT/21_runlsprebr0_preprocessed_projection_quote/runlsprebr0_preprocessed_projection_quote_rows.jsonl}"
QUOTE_API_URL="${SHADOW_LSPREBR2_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SLIPPAGE_BPS="${SHADOW_LSPREBR2_SLIPPAGE_BPS:-100}"
SIGNER_KEYPAIR_PATH="${SHADOW_LSPREBR2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_LSPREBR2_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
ALLOW_TOKEN2022="${SHADOW_LSPREBR2_ALLOW_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_LSPREBR2_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_LSPREBR2_ALLOW_MINT_AUTHORITY:-false}"
MAX_ROWS="${SHADOW_LSPREBR2_MAX_ROWS:-0}"
INPUT_MODE="${SHADOW_LSPREBR_INPUT_MODE:-${SHADOW_LSPREBR2_INPUT_MODE:-all}}"

latest_prebr0_rows() {
    find "$RESULTS_ROOT" -type f -name runlsprebr0_preprocessed_projection_quote_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PREBR0_ROWS_PATH" ]] || PREBR0_ROWS_PATH="$(latest_prebr0_rows)"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PREBR0_ROWS_PATH" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$QUOTE_API_URL" "$SLIPPAGE_BPS" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$ALLOW_TOKEN2022" "$ALLOW_FREEZE_AUTHORITY" "$ALLOW_MINT_AUTHORITY" "$MAX_ROWS" "$INPUT_MODE" <<'PY'
from __future__ import annotations

import json
import os
import re
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    prebr0_rows_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    quote_api_url,
    slippage_bps_arg,
    signer_path_arg,
    rpc_url,
    allow_token2022_arg,
    allow_freeze_authority_arg,
    allow_mint_authority_arg,
    max_rows_arg,
    input_mode_arg,
) = sys.argv[1:16]

prebr0_rows_path = Path(prebr0_rows_arg) if prebr0_rows_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
slippage_bps = max(0, int(float(slippage_bps_arg or 100)))
signer_keypair_path = Path(signer_path_arg) if signer_path_arg else None
allow_token2022 = str(allow_token2022_arg).lower() in {"1", "true", "yes", "on"}
allow_freeze_authority = str(allow_freeze_authority_arg).lower() in {"1", "true", "yes", "on"}
allow_mint_authority = str(allow_mint_authority_arg).lower() in {"1", "true", "yes", "on"}
max_rows = max(0, int(max_rows_arg or "0"))
input_mode = str(input_mode_arg or "all").strip() or "all"

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
DANGEROUS_TOKEN2022_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}


def load_jsonl(path: Path | None) -> list[dict[str, Any]]:
    if not path or not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


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


def http_json(url: str):
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "shadow-lsprebr2/1.0"})
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=25, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


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


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int):
    if not input_mint or not output_mint or amount_raw <= 0:
        return None, {"error": "missing_quote_input"}
    params = urllib.parse.urlencode(
        {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": str(int(amount_raw)),
            "slippageBps": str(int(slippage_bps)),
            "swapMode": "ExactIn",
            "restrictIntermediateTokens": "true",
        }
    )
    return http_json(f"{quote_api_url}?{params}")


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
        "decimals": None,
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "rpc_error": error or {},
    }
    value = (result or {}).get("result", result)
    if isinstance(value, dict):
        value = value.get("value")
    if not isinstance(value, dict):
        return out
    out["token_program"] = text(value.get("owner"))
    parsed = value.get("data", {}).get("parsed", {}) if isinstance(value.get("data"), dict) else {}
    info = parsed.get("info", {}) if isinstance(parsed, dict) else {}
    out["decimals"] = info.get("decimals")
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


signer_staged = bool(signer_keypair_path and signer_keypair_path.exists())
solders_ready = False
solders_error = ""
owner_pubkey = ""
if signer_staged:
    try:
        from solders.keypair import Keypair

        owner_pubkey = str(Keypair.from_json(signer_keypair_path.read_text(encoding="utf-8")).pubkey())
        solders_ready = True
    except Exception as exc:
        solders_error = str(exc)


rows = [
    row
    for row in load_jsonl(prebr0_rows_path)
    if text(row.get("projection_blocker")) == "PROJECTION_READY"
]
if max_rows > 0:
    rows = rows[:max_rows]

out = []
decision_counts: Counter[str] = Counter()
projected_rows_total = len(rows)
projected_ev_positive_rows = 0
projected_ev_positive_held_input_rows = 0
projected_ev_positive_core_input_rows = 0
projected_ev_positive_unheld_token_rows = 0
projected_ev_positive_held_or_core_input_rows = 0
backrun_executable_shape_ready_count = 0

for index, row in enumerate(rows, start=1):
    candidate_id = text(row.get("target_entity_id")) or text(row.get("target_signature")) or f"preprocessed_executable_shape_{index}"
    target_signature = text(row.get("target_signature"))
    venue = text(row.get("venue"))
    pool = text(row.get("pool"))
    target_direction = text(row.get("target_direction"))
    target_amount_in_raw = safe_int(row.get("target_amount_in_raw"))
    our_backrun_direction = text(row.get("our_backrun_direction"))
    if "->" in our_backrun_direction:
        our_input_mint, our_output_mint = [part.strip() for part in our_backrun_direction.split("->", 1)]
    else:
        our_input_mint, our_output_mint = "", ""
    our_input_raw = safe_int(row.get("our_input_raw"))
    projected_ev_after_tip_positive = boolish(row.get("net_ev_after_tip_positive"))
    if projected_ev_after_tip_positive:
        projected_ev_positive_rows += 1

    input_mint_is_core = our_input_mint in {WSOL_MINT, USDC_MINT, USDT_MINT}
    owner_native_balance_raw, owner_native_balance_error = get_balance(owner_pubkey) if owner_pubkey else (0, {"error": "owner_pubkey_missing"})

    input_accounts = []
    our_input_balance_raw = 0
    our_input_account_ready = False
    if our_input_mint == WSOL_MINT:
        our_input_balance_raw = owner_native_balance_raw
        our_input_account_ready = owner_native_balance_raw >= our_input_raw > 0
        input_account_count = 1 if owner_native_balance_raw > 0 else 0
    elif our_input_mint:
        input_accounts = get_token_accounts_by_owner(owner_pubkey, our_input_mint) if owner_pubkey else []
        richest_input = max(input_accounts, key=lambda item: safe_int(item.get("amount_raw")), default={})
        our_input_balance_raw = safe_int(richest_input.get("amount_raw"))
        our_input_account_ready = bool(text(richest_input.get("pubkey"))) and our_input_balance_raw >= our_input_raw > 0
        input_account_count = len(input_accounts)
    else:
        input_account_count = 0

    input_mint_is_held = our_input_balance_raw > 0
    if projected_ev_after_tip_positive and input_mint_is_held:
        projected_ev_positive_held_input_rows += 1
    if projected_ev_after_tip_positive and input_mint_is_core:
        projected_ev_positive_core_input_rows += 1
    if projected_ev_after_tip_positive and not input_mint_is_core and not input_mint_is_held:
        projected_ev_positive_unheld_token_rows += 1
    input_mode_allowed = input_mint_is_core or input_mint_is_held
    if projected_ev_after_tip_positive and input_mode_allowed:
        projected_ev_positive_held_or_core_input_rows += 1

    output_account_ready = True if our_output_mint == WSOL_MINT else False
    output_account_count = 1 if output_account_ready else 0
    if our_output_mint and our_output_mint != WSOL_MINT and owner_pubkey:
        output_accounts = get_token_accounts_by_owner(owner_pubkey, our_output_mint)
        output_account_count = len(output_accounts)
        output_account_ready = output_account_count > 0

    input_meta = mint_metadata(our_input_mint) if our_input_mint else {}
    output_meta = mint_metadata(our_output_mint) if our_output_mint else {}
    input_policy_pass, input_policy_blocker = policy_for_mint(input_meta) if input_meta else (False, "input_mint_missing")
    output_policy_pass, output_policy_blocker = policy_for_mint(output_meta) if output_meta else (False, "output_mint_missing")

    fresh_quote, fresh_quote_error = quote_exact_in(our_input_mint, our_output_mint, our_input_raw) if our_input_raw > 0 else (None, {"error": "input_amount_non_positive"})
    fresh_route_source = "jupiter_exact_in" if our_input_mint and our_output_mint else ""
    fresh_quote_out_raw = safe_int((fresh_quote or {}).get("outAmount"))
    fresh_min_out_raw = safe_int((fresh_quote or {}).get("otherAmountThreshold"))
    fresh_quote_positive = fresh_quote_out_raw > 0
    fresh_min_out_positive = fresh_min_out_raw > 0
    fresh_route_exists = fresh_quote_positive or not (isinstance(fresh_quote_error, dict) and text(fresh_quote_error.get("errorCode")) == "NO_ROUTES_FOUND")

    if not input_policy_pass or not output_policy_pass:
        decision = "PARK_POLICY"
        first_blocker = input_policy_blocker if not input_policy_pass else output_policy_blocker
    elif not our_input_mint or not our_output_mint or our_input_raw <= 0:
        decision = "PARK_UNSUPPORTED_INPUT"
        first_blocker = "input_shape_missing"
    elif not projected_ev_after_tip_positive:
        decision = "PARK_EV_NEGATIVE"
        first_blocker = "projected_ev_non_positive"
    elif not input_mint_is_held and not input_mint_is_core:
        decision = "PARK_EXECUTABLE_INPUT_NOT_HELD"
        first_blocker = "input_not_held"
    elif not our_input_account_ready:
        decision = "PARK_INPUT_ACCOUNT_MISSING"
        first_blocker = "input_balance_or_account_missing"
    elif not output_account_ready:
        decision = "PARK_INPUT_ACCOUNT_MISSING"
        first_blocker = "output_account_missing"
    elif not fresh_route_exists or not fresh_quote_positive or not fresh_min_out_positive:
        decision = "PARK_NO_FRESH_ROUTE"
        first_blocker = text((fresh_quote_error or {}).get("errorCode") or (fresh_quote_error or {}).get("error") or "no_fresh_route")
    else:
        decision = "BACKRUN_EXECUTABLE_SHAPE_READY"
        first_blocker = "none"
        backrun_executable_shape_ready_count += 1

    decision_counts[decision] += 1
    out.append(
        {
            **row,
            "candidate_id": candidate_id,
            "target_signature": target_signature,
            "target_entity_id": text(row.get("target_entity_id")),
            "venue": venue,
            "pool": pool,
            "target_direction": target_direction,
            "target_amount_in_raw": target_amount_in_raw,
            "our_backrun_direction": our_backrun_direction,
            "our_input_mint": our_input_mint,
            "our_output_mint": our_output_mint,
            "our_input_account_ready": our_input_account_ready,
            "our_input_balance_raw": our_input_balance_raw,
            "our_output_account_ready": output_account_ready,
            "our_output_account_count": output_account_count,
            "our_input_account_count": input_account_count,
            "owner_pubkey": owner_pubkey,
            "owner_native_balance_raw": owner_native_balance_raw,
            "owner_native_balance_error": owner_native_balance_error or {},
            "input_mint_is_core": input_mint_is_core,
            "input_mint_is_held": input_mint_is_held,
            "input_mode": input_mode,
            "input_mode_allowed": input_mode_allowed,
            "input_policy_pass": input_policy_pass,
            "output_policy_pass": output_policy_pass,
            "input_policy_blocker": input_policy_blocker,
            "output_policy_blocker": output_policy_blocker,
            "fresh_route_exists": fresh_route_exists,
            "fresh_route_source": fresh_route_source,
            "fresh_quote_positive": fresh_quote_positive,
            "fresh_min_out_positive": fresh_min_out_positive,
            "fresh_quote_out_raw": fresh_quote_out_raw,
            "fresh_min_out_raw": fresh_min_out_raw,
            "fresh_quote": fresh_quote or {"ok": False, "error": fresh_quote_error or {"error": "fresh_quote_missing"}},
            "projected_ev_after_tip_positive": projected_ev_after_tip_positive,
            "decision": decision,
            "first_executable_shape_blocker": first_blocker,
            "signer_staged": signer_staged,
            "solders_ready": solders_ready,
            "solders_error": solders_error,
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

status = "BACKRUN_EXECUTABLE_SHAPE_READY" if backrun_executable_shape_ready_count > 0 else (max(decision_counts, key=decision_counts.get) if decision_counts else "PARK_HELD_INPUT_FILTER_ZERO")
summary = {
    "status": status,
    "source_rows": str(prebr0_rows_path or ""),
    "rows_seen": len(out),
    "projected_rows_total": projected_rows_total,
    "projected_ev_positive_rows": projected_ev_positive_rows,
    "projected_ev_positive_held_input_rows": projected_ev_positive_held_input_rows,
    "projected_ev_positive_core_input_rows": projected_ev_positive_core_input_rows,
    "projected_ev_positive_unheld_token_rows": projected_ev_positive_unheld_token_rows,
    "projected_ev_positive_held_or_core_input_rows": projected_ev_positive_held_or_core_input_rows,
    "held_input_projected_ev_positive_count": projected_ev_positive_held_input_rows,
    "core_input_projected_ev_positive_count": projected_ev_positive_core_input_rows,
    "backrun_executable_shape_ready_count": backrun_executable_shape_ready_count,
    "decision_counts": dict(decision_counts),
    "input_mode": input_mode,
    "owner_pubkey": owner_pubkey,
    "signer_staged": signer_staged,
    "solders_ready": solders_ready,
    "solders_error": solders_error,
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
manifest = {
    "source_rows": str(prebr0_rows_path or ""),
    "result_dir": str(result_dir),
    "quote_api_url": quote_api_url,
    "rpc_url": rpc_url,
    "signer_staged": signer_staged,
    "input_mode": input_mode,
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v}"
        for k, v in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBR2 executable shape filter written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

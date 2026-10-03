#!/usr/bin/env bash
# 21_runls11e_target_freshness_bundle_eligibility.sh — prove selected target tx is still bundle-eligible

source "$(dirname "$0")/lib.sh"
script_header "21E — RunLS11E Target Freshness / Bundle Eligibility"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
SELECTED_PATH="${2:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
PAYLOAD_PATH="${3:-$INNER_STEP_DIR/runls11_bundle_payload_base64.json}"
PROBE_ROWS_PATH="${4:-$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl}"
OUT_PATH="$INNER_STEP_DIR/runls11e_target_freshness_bundle_eligibility.json"
OUT_ROWS_PATH="$INNER_STEP_DIR/runls11e_target_freshness_bundle_eligibility_rows.jsonl"

if [[ ! -f "$SELECTED_PATH" ]]; then
    fail "RunLS11 selected bundle doc missing: $SELECTED_PATH"
    finish
    exit 1
fi
if [[ ! -f "$PAYLOAD_PATH" ]]; then
    fail "RunLS11 bundle payload doc missing: $PAYLOAD_PATH"
    finish
    exit 1
fi
if [[ ! -f "$PROBE_ROWS_PATH" ]]; then
    fail "RunLS11 probe rows missing: $PROBE_ROWS_PATH"
    finish
    exit 1
fi

python3 - "$SELECTED_PATH" "$PAYLOAD_PATH" "$PROBE_ROWS_PATH" "$OUT_PATH" "$OUT_ROWS_PATH" \
    "${SHADOW_SOLANA_RPC_URL:-}" \
    "${SHADOW_RPC_URL:-}" \
    "${SHADOW_PUBLIC_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_API_KEY:-}" <<'PY'
from __future__ import annotations

import base64
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

selected_path = Path(sys.argv[1])
payload_path = Path(sys.argv[2])
probe_rows_path = Path(sys.argv[3])
out_path = Path(sys.argv[4])
out_rows_path = Path(sys.argv[5])
shadow_solana_rpc_url = sys.argv[6]
shadow_rpc_url = sys.argv[7]
shadow_public_rpc_url = sys.argv[8]
shadow_helius_bundle_rpc_url = sys.argv[9]
shadow_helius_bundle_api_key = sys.argv[10]
local_agave_rpc_url = os.environ.get("SHADOW_LOCAL_AGAVE_RPC_URL", "http://127.0.0.1:8899")

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def is_placeholder(value: str | None) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


def add_api_key(url: str, api_key: str) -> str:
    if is_placeholder(url) or not api_key:
        return url
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment)
    )


def redacted_url(url: str) -> str:
    if is_placeholder(url):
        return url or ""
    parsed = urllib.parse.urlsplit(url)
    if not parsed.query:
        return url
    pairs = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    redacted = []
    for key, value in pairs:
        if key.lower() in {"api-key", "apikey", "key", "token"}:
            redacted.append((key, "REDACTED" if value else value))
        else:
            redacted.append((key, value))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(redacted), parsed.fragment)
    )


def b58encode(data: bytes) -> str:
    num = int.from_bytes(data, "big")
    encoded = ""
    while num:
        num, rem = divmod(num, 58)
        encoded = BASE58_ALPHABET[rem] + encoded
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    return BASE58_ALPHABET[0] * pad + (encoded or BASE58_ALPHABET[0])


def hex_signature_to_b58(value: str) -> str:
    value = value.strip()
    try:
        if len(value) == 128 and all(ch in "0123456789abcdefABCDEF" for ch in value):
            return b58encode(bytes.fromhex(value))
    except ValueError:
        pass
    return value


def decode_shortvec(data: bytes, offset: int) -> tuple[int, int]:
    size = 0
    shift = 0
    while True:
        if offset >= len(data):
            raise ValueError("shortvec_out_of_bounds")
        elem = data[offset]
        offset += 1
        size |= (elem & 0x7F) << shift
        if elem & 0x80 == 0:
            break
        shift += 7
        if shift > 28:
            raise ValueError("shortvec_too_large")
    return size, offset


def extract_recent_blockhash(tx_base64: str) -> tuple[str | None, str | None, int]:
    try:
        data = base64.b64decode(tx_base64)
        sig_count, offset = decode_shortvec(data, 0)
        offset += sig_count * 64
        if offset >= len(data):
            return None, "message_offset_out_of_bounds", len(data)
        first_message_byte = data[offset]
        versioned = bool(first_message_byte & 0x80)
        if versioned:
            offset += 1
        offset += 3
        account_count, offset = decode_shortvec(data, offset)
        offset += account_count * 32
        if offset + 32 > len(data):
            return None, "recent_blockhash_out_of_bounds", len(data)
        return b58encode(data[offset:offset + 32]), None, len(data)
    except Exception as exc:  # noqa: BLE001
        return None, str(exc), 0


def rpc_call(url: str, method: str, params: Any) -> dict[str, Any]:
    if is_placeholder(url):
        return {"reachable": False, "probe": "placeholder", "method_available": False}
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(
        url,
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    context = ssl.create_default_context() if url.startswith("https://") else None
    try:
        with urllib.request.urlopen(req, timeout=20, context=context) as response:
            text = response.read().decode("utf-8", errors="replace")
            status = response.status
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8", errors="replace")
        status = exc.code
    except Exception as exc:  # noqa: BLE001
        return {
            "reachable": False,
            "probe": str(exc),
            "method_available": False,
        }
    try:
        payload = json.loads(text) if text else {}
    except json.JSONDecodeError:
        payload = {"raw": text[:240]}
    err = payload.get("error") if isinstance(payload, dict) else None
    err_msg = str(err.get("message", "")) if isinstance(err, dict) else ""
    method_missing = (
        isinstance(err, dict)
        and (err.get("code") == -32601 or "method not found" in err_msg.lower())
    )
    auth_failed = status in {401, 403} or "api key" in err_msg.lower() or "unauthorized" in err_msg.lower()
    return {
        "reachable": status < 500 and not auth_failed,
        "probe": f"http_{status}",
        "method_available": not method_missing and not auth_failed,
        "auth_failed": auth_failed,
        "error_code": err.get("code") if isinstance(err, dict) else None,
        "error_message": err_msg[:240] if err_msg else None,
        "result": payload.get("result") if isinstance(payload, dict) else None,
        "has_result": isinstance(payload, dict) and "result" in payload,
    }


selected = json.loads(selected_path.read_text(encoding="utf-8"))
payload = json.loads(payload_path.read_text(encoding="utf-8"))
target_signature_hex = str(selected.get("selected_target_signature") or "")
target_signature = hex_signature_to_b58(target_signature_hex)
selected_backrun_signature = str(selected.get("selected_backrun_signature") or "")
txs_base64 = payload.get("txs_base64") if isinstance(payload.get("txs_base64"), list) else []
target_seen_ns = selected.get("selected_afxdp_seen_ns") or selected.get("selected_first_seen_ns")
projection_complete_ns = selected.get("selected_current_quote_ready_ns") or selected.get("selected_current_route_ready_ns")
bundle_ready_ns = selected.get("selected_current_trade_ready_ns") or selected.get("selected_current_candidate_emit_ns")

target_probe_row = None
for raw in probe_rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    if row.get("record_type") != "runls11_bundle_probe":
        continue
    if row.get("our_signature_hex") == selected_backrun_signature:
        target_probe_row = row
        break
    if not target_probe_row and row.get("target_signature_hex") == target_signature_hex:
        target_probe_row = row

target_tx_base64 = ""
if target_probe_row:
    target_tx_base64 = str(target_probe_row.get("target_tx_base64") or "")
if not target_tx_base64 and txs_base64:
    target_tx_base64 = str(txs_base64[0] or "")

bundle_endpoint = add_api_key(
    shadow_helius_bundle_rpc_url
    or shadow_solana_rpc_url
    or shadow_rpc_url
    or shadow_public_rpc_url,
    shadow_helius_bundle_api_key,
)

target_recent_blockhash, target_tx_decode_error, target_tx_decoded_len_bytes = extract_recent_blockhash(target_tx_base64)
first_status_check_start_ns = time.monotonic_ns() if target_signature else None
signature_probe = rpc_call(
    bundle_endpoint,
    "getSignatureStatuses",
    [[target_signature], {"searchTransactionHistory": True}],
) if target_signature else {"reachable": False, "probe": "missing_target_signature", "result": None}
local_slot_probe = rpc_call(
    local_agave_rpc_url,
    "getSlot",
    [{"commitment": "processed"}],
) if target_signature else {"reachable": False, "probe": "missing_target_signature", "result": None}
first_status_check_end_ns = time.monotonic_ns() if target_signature else None
signature_status_value = None
try:
    signature_status_value = signature_probe.get("result", {}).get("value", [None])[0]
except Exception:  # noqa: BLE001
    signature_status_value = None
local_current_slot_at_first_check = None
try:
    local_current_slot_at_first_check = local_slot_probe.get("result")
except Exception:  # noqa: BLE001
    local_current_slot_at_first_check = None

blockhash_check_start_ns = time.monotonic_ns() if target_recent_blockhash else None
blockhash_probe = rpc_call(
    bundle_endpoint,
    "isBlockhashValid",
    [target_recent_blockhash, {"commitment": "processed"}],
) if target_recent_blockhash else {"reachable": False, "probe": "missing_recent_blockhash", "result": None}
blockhash_check_end_ns = time.monotonic_ns() if target_recent_blockhash else None
target_recent_blockhash_valid = False
try:
    target_recent_blockhash_valid = bool(blockhash_probe.get("result", {}).get("value"))
except Exception:  # noqa: BLE001
    target_recent_blockhash_valid = False

simulate_params = [
    {"encodedTransactions": txs_base64},
    {
        "preExecutionAccountsConfigs": [None for _ in txs_base64],
        "postExecutionAccountsConfigs": [None for _ in txs_base64],
        "skipSigVerify": True,
        "simulationBank": {"commitment": {"commitment": "processed"}},
        "transactionEncoding": "base64",
        "replaceRecentBlockhash": False,
    },
]
provider_sim_start_ns = time.monotonic_ns() if txs_base64 else None
simulate_probe = rpc_call(bundle_endpoint, "simulateBundle", simulate_params) if txs_base64 else {
    "reachable": False,
    "probe": "no_payload",
    "method_available": False,
    "result": None,
}
provider_sim_end_ns = time.monotonic_ns() if txs_base64 else None
simulate_result = simulate_probe.get("result")
simulate_summary = None
simulate_bundle_pass = False
if isinstance(simulate_result, dict):
    value = simulate_result.get("value")
    if isinstance(value, dict):
        simulate_summary = value.get("summary")
        simulate_bundle_pass = simulate_summary == "succeeded"

post_provider_status_check_start_ns = time.monotonic_ns() if target_signature else None
post_provider_signature_probe = rpc_call(
    bundle_endpoint,
    "getSignatureStatuses",
    [[target_signature], {"searchTransactionHistory": True}],
) if target_signature else {"reachable": False, "probe": "missing_target_signature", "result": None}
post_provider_status_check_end_ns = time.monotonic_ns() if target_signature else None
post_provider_signature_status_value = None
try:
    post_provider_signature_status_value = post_provider_signature_probe.get("result", {}).get("value", [None])[0]
except Exception:  # noqa: BLE001
    post_provider_signature_status_value = None


def delta_us(start_ns, end_ns):
    try:
        if start_ns is None or end_ns is None:
            return None
        start = int(start_ns)
        end = int(end_ns)
        if end < start:
            return None
        return (end - start) // 1000
    except (TypeError, ValueError):
        return None


first_status_check_latency_us = delta_us(first_status_check_start_ns, first_status_check_end_ns)
provider_sim_latency_us = delta_us(provider_sim_start_ns, provider_sim_end_ns)
bundle_ready_to_status_start_us = delta_us(bundle_ready_ns, first_status_check_start_ns)
bundle_ready_to_provider_sim_start_us = delta_us(bundle_ready_ns, provider_sim_start_ns)
target_seen_to_bundle_ready_us = delta_us(target_seen_ns, bundle_ready_ns)
target_seen_to_projection_us = delta_us(target_seen_ns, projection_complete_ns)

bundle_tx_count = len(txs_base64)
target_tx_first = bool(target_tx_base64 and txs_base64 and txs_base64[0] == target_tx_base64)
our_backrun_tx_second = bool(bundle_tx_count >= 2 and txs_base64[1])
tip_present = bool(payload.get("tip_present"))

blockers = []
if not selected.get("selected_target_tx_bytes_available") or not target_tx_base64:
    blockers.append("blocked_target_tx_bytes_missing")
if signature_status_value is not None:
    blockers.append("blocked_target_already_landed")
if target_tx_decode_error:
    blockers.append("blocked_target_tx_decode_failed")
if target_recent_blockhash and not target_recent_blockhash_valid:
    blockers.append("blocked_target_recent_blockhash_invalid")
if bundle_tx_count > 5:
    blockers.append("blocked_bundle_tx_count_gt_5")
if not target_tx_first:
    blockers.append("blocked_target_not_first")
if not our_backrun_tx_second:
    blockers.append("blocked_backrun_not_second")
if not tip_present:
    blockers.append("blocked_tip_missing")
if simulate_probe.get("method_available") and not simulate_bundle_pass:
    blockers.append("blocked_simulate_bundle_failed")

if not blockers:
    selection_result = "target_fresh_bundle_eligible"
elif "blocked_target_already_landed" in blockers:
    selection_result = "blocked_target_already_landed"
elif "blocked_target_recent_blockhash_invalid" in blockers:
    selection_result = "blocked_target_recent_blockhash_invalid"
else:
    selection_result = blockers[0]

summary = {
    "record_type": "runls11e_target_freshness_bundle_eligibility",
    "selection_result": selection_result,
    "blockers": blockers,
    "target_signature": target_signature_hex,
    "target_signature_base58": target_signature,
    "target_seen_ns": target_seen_ns,
    "projection_complete_ns": projection_complete_ns,
    "bundle_ready_ns": bundle_ready_ns,
    "target_seen_to_bundle_ready_us": target_seen_to_bundle_ready_us,
    "target_seen_to_projection_us": target_seen_to_projection_us,
    "target_tx_bytes_available": bool(selected.get("selected_target_tx_bytes_available") and target_tx_base64),
    "target_tx_decoded_len_bytes": target_tx_decoded_len_bytes,
    "target_tx_decode_error": target_tx_decode_error,
    "target_signature_status_before_bundle": signature_status_value,
    "target_signature_status_after_bundle_ready": signature_status_value,
    "target_signature_status_probe": signature_probe.get("probe"),
    "target_signature_status_reachable": bool(signature_probe.get("reachable")),
    "first_status_check_start_ns": first_status_check_start_ns,
    "first_status_check_end_ns": first_status_check_end_ns,
    "first_status_check_latency_us": first_status_check_latency_us,
    "first_status_check_probe": signature_probe.get("probe"),
    "bundle_ready_to_status_start_us": bundle_ready_to_status_start_us,
    "local_current_slot_at_first_check": local_current_slot_at_first_check,
    "local_current_slot_probe": local_slot_probe.get("probe"),
    "local_current_slot_reachable": bool(local_slot_probe.get("reachable")),
    "target_already_landed": signature_status_value is not None,
    "target_recent_blockhash": target_recent_blockhash,
    "target_recent_blockhash_valid": target_recent_blockhash_valid,
    "target_recent_blockhash_probe": blockhash_probe.get("probe"),
    "blockhash_check_start_ns": blockhash_check_start_ns,
    "blockhash_check_end_ns": blockhash_check_end_ns,
    "bundle_tx_count": bundle_tx_count,
    "bundle_tx_count_max_allowed": 5,
    "target_tx_first": target_tx_first,
    "our_backrun_tx_second": our_backrun_tx_second,
    "tip_present": tip_present,
    "simulate_bundle_available": bool(simulate_probe.get("reachable") and simulate_probe.get("method_available")),
    "simulate_bundle_probe": simulate_probe.get("probe"),
    "simulate_bundle_error_code": simulate_probe.get("error_code"),
    "simulate_bundle_error_message": simulate_probe.get("error_message"),
    "simulate_bundle_summary": simulate_summary,
    "simulateBundle_pass": simulate_bundle_pass,
    "provider_sim_start_ns": provider_sim_start_ns,
    "provider_sim_end_ns": provider_sim_end_ns,
    "provider_sim_latency_us": provider_sim_latency_us,
    "bundle_ready_to_provider_sim_start_us": bundle_ready_to_provider_sim_start_us,
    "target_signature_status_after_provider_sim": post_provider_signature_status_value,
    "target_signature_status_after_provider_sim_probe": post_provider_signature_probe.get("probe"),
    "post_provider_status_check_start_ns": post_provider_status_check_start_ns,
    "post_provider_status_check_end_ns": post_provider_status_check_end_ns,
    "provider": "jito_via_helius",
    "provider_endpoint": redacted_url(bundle_endpoint),
    "sendBundle_called": False,
    "send_bundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
}

row = {
    "target_signature": target_signature_hex,
    "target_signature_base58": target_signature,
    "selection_result": selection_result,
    "target_seen_ns": target_seen_ns,
    "projection_complete_ns": projection_complete_ns,
    "bundle_ready_ns": bundle_ready_ns,
    "target_already_landed": signature_status_value is not None,
    "target_recent_blockhash_valid": target_recent_blockhash_valid,
    "target_tx_first": target_tx_first,
    "our_backrun_tx_second": our_backrun_tx_second,
    "simulateBundle_pass": simulate_bundle_pass,
    "first_status_check_start_ns": first_status_check_start_ns,
    "first_status_check_end_ns": first_status_check_end_ns,
    "first_status_check_latency_us": first_status_check_latency_us,
    "bundle_ready_to_status_start_us": bundle_ready_to_status_start_us,
    "local_current_slot_at_first_check": local_current_slot_at_first_check,
    "provider_sim_start_ns": provider_sim_start_ns,
    "provider_sim_end_ns": provider_sim_end_ns,
    "provider_sim_latency_us": provider_sim_latency_us,
    "bundle_ready_to_provider_sim_start_us": bundle_ready_to_provider_sim_start_us,
    "post_provider_status_check_start_ns": post_provider_status_check_start_ns,
    "post_provider_status_check_end_ns": post_provider_status_check_end_ns,
    "sendBundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
}

out_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
out_rows_path.write_text(json.dumps(row) + "\n", encoding="utf-8")
PY

info "Target freshness / bundle eligibility summary written to $OUT_PATH"
info "Target freshness / bundle eligibility rows written to $OUT_ROWS_PATH"
if python3 - "$OUT_PATH" <<'PY'
import json
import sys
from pathlib import Path

doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
raise SystemExit(0 if doc.get("selection_result") == "target_fresh_bundle_eligible" else 1)
PY
then
    pass "Selected target is fresh and bundle eligible"
else
    fail "Selected target is not live-bundle eligible; see $OUT_PATH"
fi
finish

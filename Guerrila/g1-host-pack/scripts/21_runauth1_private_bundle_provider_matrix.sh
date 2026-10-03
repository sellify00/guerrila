#!/usr/bin/env bash
# 21_runauth1_private_bundle_provider_matrix.sh — provider/auth matrix for private bundle readiness

source "$(dirname "$0")/lib.sh"
script_header "AUTH1 — Private Bundle Provider Matrix"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
SELECTED_PATH="${2:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
PAYLOAD_PATH="${3:-$INNER_STEP_DIR/runls11_bundle_payload_base64.json}"
WINDOW_CONFIG_PATH="${4:-$INNER_STEP_DIR/config.window_local.yaml}"
OUT_MATRIX_PATH="$INNER_STEP_DIR/runauth1_private_bundle_provider_matrix.json"
OUT_ROWS_PATH="$INNER_STEP_DIR/runauth1_private_bundle_provider_rows.jsonl"
OUT_SELECTED_PATH="$INNER_STEP_DIR/runauth1_selected_provider.json"

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

python3 - "$SELECTED_PATH" "$PAYLOAD_PATH" "$WINDOW_CONFIG_PATH" "$OUT_MATRIX_PATH" "$OUT_ROWS_PATH" "$OUT_SELECTED_PATH" \
    "${SHADOW_SOLANA_RPC_URL:-}" \
    "${SHADOW_RPC_URL:-}" \
    "${SHADOW_PUBLIC_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_API_KEY:-}" \
    "${SHADOW_HELIUS_SENDER_API_KEY:-}" \
    "${SHADOW_JITO_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_HARMONIC_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_JITO_AUTH_PATH:-}" \
    "${SHADOW_HARMONIC_AUTH_PATH:-}" \
    "${SHADOW_HELIUS_HARMONIC_BUNDLE_RPC_URL:-}" \
    "${SHADOW_HELIUS_HARMONIC_BUNDLE_API_KEY:-}" <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

selected_path = Path(sys.argv[1])
payload_path = Path(sys.argv[2])
window_config_path = Path(sys.argv[3])
out_matrix_path = Path(sys.argv[4])
out_rows_path = Path(sys.argv[5])
out_selected_path = Path(sys.argv[6])
shadow_solana_rpc_url = sys.argv[7]
shadow_rpc_url = sys.argv[8]
shadow_public_rpc_url = sys.argv[9]
shadow_helius_bundle_rpc_url = sys.argv[10]
shadow_helius_bundle_api_key = sys.argv[11]
shadow_helius_sender_api_key = sys.argv[12]
jito_status_url = sys.argv[13]
harmonic_status_url = sys.argv[14]
jito_auth_path = Path(sys.argv[15]) if sys.argv[15] else None
harmonic_auth_path = Path(sys.argv[16]) if sys.argv[16] else None
helius_harmonic_bundle_rpc_url = sys.argv[17]
helius_harmonic_bundle_api_key = sys.argv[18]


def is_placeholder(value: str | None) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


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


def add_api_key(url: str, api_key: str) -> str:
    if is_placeholder(url) or not api_key:
        return url
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    lower = {key.lower() for key, _ in query}
    if "api-key" not in lower:
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment)
    )


def has_api_key(url: str, explicit_key: str = "") -> bool:
    if explicit_key:
        return True
    if is_placeholder(url):
        return False
    parsed = urllib.parse.urlsplit(url)
    for key, value in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True):
        if key.lower() in {"api-key", "apikey"} and value and "CHANGE_ME" not in value:
            return True
    return False


def probe_url(url: str, method: str = "GET") -> tuple[bool, str]:
    if is_placeholder(url):
        return False, "placeholder"
    req = urllib.request.Request(url, method=method)
    context = ssl.create_default_context() if url.startswith("https://") else None
    try:
        with urllib.request.urlopen(req, timeout=10, context=context) as response:
            return True, f"http_{response.status}"
    except urllib.error.HTTPError as exc:
        return True, f"http_{exc.code}"
    except Exception as exc:
        return False, str(exc)


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
        with urllib.request.urlopen(req, timeout=15, context=context) as response:
            text = response.read().decode("utf-8", errors="replace")
            status = response.status
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8", errors="replace")
        status = exc.code
    except Exception as exc:
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
        "has_result": isinstance(payload, dict) and "result" in payload,
    }


def parse_window_config(path: Path) -> dict[str, Any]:
    out: dict[str, Any] = {"tip_accounts": []}
    if not path.exists():
        return out
    stack: list[tuple[int, str]] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        while stack and stack[-1][0] >= indent:
            stack.pop()
        if stripped.endswith(":") and not stripped.startswith("- "):
            key = stripped[:-1]
            prefix = stack[-1][1] + "." if stack else ""
            stack.append((indent, prefix + key))
            continue
        current = stack[-1][1] if stack else ""
        if current == "jito" and stripped.startswith("block_engine_url:"):
            out["jito_block_engine_url"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito" and stripped.startswith("auth_mode:"):
            out["jito_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito.json_rpc" and stripped.startswith("endpoint:"):
            out["jito_json_rpc_endpoint"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito.json_rpc" and stripped.startswith("enabled:"):
            out["jito_json_rpc_enabled"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "execution" and stripped.startswith("jito_tip_account:"):
            out["jito_tip_account"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "helius_sender.tip_accounts" and stripped.startswith("- "):
            out.setdefault("tip_accounts", []).append(stripped[2:].strip().strip('"'))
        elif current == "harmonic" and stripped.startswith("auth_mode:"):
            out["harmonic_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "harmonic.region_endpoints" and stripped.startswith("- "):
            out.setdefault("harmonic_region_endpoints", []).append(stripped[2:].strip().strip('"'))
    return out


selected = json.loads(selected_path.read_text(encoding="utf-8"))
payload = json.loads(payload_path.read_text(encoding="utf-8"))
config = parse_window_config(window_config_path)
txs_base64 = payload.get("txs_base64") if isinstance(payload.get("txs_base64"), list) else []
tip_accounts = list(config.get("tip_accounts", []))
if config.get("jito_tip_account"):
    tip_accounts.append(str(config.get("jito_tip_account")))
tip_accounts = list(dict.fromkeys([item for item in tip_accounts if item]))

helius_rpc_base = (
    shadow_helius_bundle_rpc_url
    or shadow_solana_rpc_url
    or shadow_rpc_url
    or shadow_public_rpc_url
)
helius_rpc = add_api_key(helius_rpc_base, shadow_helius_bundle_api_key)
helius_harmonic_rpc = add_api_key(helius_harmonic_bundle_rpc_url, helius_harmonic_bundle_api_key)
jito_direct_submit = str(config.get("jito_json_rpc_endpoint") or config.get("jito_block_engine_url") or "")
harmonic_direct_submit = str((config.get("harmonic_region_endpoints") or [""])[0])


def build_helius_row(provider_route_name: str, endpoint: str, explicit_key: str, configured: bool) -> dict[str, Any]:
    auth_present = configured and has_api_key(endpoint, explicit_key)
    status_probe = rpc_call(endpoint, "getBundleStatuses", [["0" * 64]]) if configured else {
        "reachable": False,
        "probe": "not_configured",
        "method_available": False,
    }
    simulate_probe = rpc_call(
        endpoint,
        "simulateBundle",
        [{"encodedTransactions": txs_base64}],
    ) if configured and txs_base64 else {
        "reachable": False,
        "probe": "no_payload" if configured else "not_configured",
        "method_available": False,
    }
    return {
        "provider_route_name": provider_route_name,
        "provider": provider_route_name,
        "provider_auth_source": "helius_rpc_api_key",
        "provider_endpoint": redacted_url(endpoint),
        "configured": configured,
        "auth_material_present": auth_present,
        "tip_accounts_available": bool(tip_accounts),
        "tip_accounts_count": len(tip_accounts),
        "status_api_reachable": bool(status_probe.get("reachable") and status_probe.get("method_available")),
        "status_api_probe": status_probe.get("probe"),
        "status_method_available": bool(status_probe.get("method_available")),
        "simulate_bundle_available": bool(simulate_probe.get("reachable") and simulate_probe.get("method_available")),
        "simulate_bundle_probe": simulate_probe.get("probe"),
        "simulate_bundle_error_code": simulate_probe.get("error_code"),
        "simulate_bundle_error_message": simulate_probe.get("error_message"),
        "send_bundle_disabled_in_test": True,
        "send_bundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }


def build_direct_row(provider_route_name: str, submit_endpoint: str, status_url: str, auth_path: Path | None, auth_source: str) -> dict[str, Any]:
    submit_reachable, submit_probe = probe_url(submit_endpoint)
    status_reachable, status_probe = probe_url(status_url)
    auth_present = bool(auth_path and auth_path.exists())
    return {
        "provider_route_name": provider_route_name,
        "provider": provider_route_name,
        "provider_auth_source": auth_source,
        "provider_endpoint": redacted_url(submit_endpoint),
        "configured": not is_placeholder(submit_endpoint),
        "auth_material_present": auth_present,
        "tip_accounts_available": bool(tip_accounts),
        "tip_accounts_count": len(tip_accounts),
        "status_api_reachable": status_reachable,
        "status_api_probe": status_probe,
        "submit_endpoint_reachable": submit_reachable,
        "submit_endpoint_probe": submit_probe,
        "simulate_bundle_available": False,
        "simulate_bundle_probe": "not_applicable_direct_provider",
        "send_bundle_disabled_in_test": True,
        "send_bundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
        "auth_path_present": auth_present,
    }


rows = [
    build_helius_row("jito_via_helius", helius_rpc, shadow_helius_bundle_api_key, not is_placeholder(helius_rpc)),
    build_helius_row(
        "harmonic_via_helius",
        helius_harmonic_rpc,
        helius_harmonic_bundle_api_key,
        not is_placeholder(helius_harmonic_rpc),
    ),
    build_direct_row("jito_direct", jito_direct_submit, jito_status_url, jito_auth_path, "jito_keypair"),
    build_direct_row("harmonic_direct", harmonic_direct_submit, harmonic_status_url, harmonic_auth_path, "harmonic_keypair"),
]

for row in rows:
    row["passes_auth_readiness"] = bool(
        row.get("auth_material_present")
        and row.get("status_api_reachable")
        and row.get("tip_accounts_available")
        and row.get("send_bundle_disabled_in_test")
    )
    row["selected_target_tx_bytes_available"] = bool(selected.get("selected_target_tx_bytes_available"))
    row["bundle_envelope_built"] = bool(selected.get("bundle_envelope_built"))
    row["bundle_tx_count"] = payload.get("bundle_tx_count")
    row["selected_route_family"] = selected.get("selected_route_family")

priority = {"jito_via_helius": 0, "harmonic_via_helius": 1, "jito_direct": 2, "harmonic_direct": 3}
passing = sorted(
    [row for row in rows if row["passes_auth_readiness"]],
    key=lambda row: priority.get(str(row["provider_route_name"]), 99),
)
selected_provider = passing[0] if passing else None

summary = {
    "record_type": "runauth1_private_bundle_provider_matrix",
    "selected_artifact": str(selected_path),
    "payload_artifact": str(payload_path),
    "provider_count": len(rows),
    "passing_provider_count": len(passing),
    "first_passing_provider": selected_provider.get("provider_route_name") if selected_provider else None,
    "first_passing_provider_auth_source": selected_provider.get("provider_auth_source") if selected_provider else None,
    "first_passing_provider_endpoint": selected_provider.get("provider_endpoint") if selected_provider else None,
    "selected_target_tx_bytes_available": bool(selected.get("selected_target_tx_bytes_available")),
    "bundle_envelope_built": bool(selected.get("bundle_envelope_built")),
    "bundle_tx_count": payload.get("bundle_tx_count"),
    "send_bundle_disabled_in_test": True,
    "send_bundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
    "providers": rows,
}

out_matrix_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
with out_rows_path.open("w", encoding="utf-8") as handle:
    for row in rows:
        handle.write(json.dumps(row) + "\n")
out_selected_path.write_text(json.dumps(selected_provider or {}, indent=2) + "\n", encoding="utf-8")
PY

info "Provider matrix written to $OUT_MATRIX_PATH"
info "Provider matrix rows written to $OUT_ROWS_PATH"
info "Selected provider written to $OUT_SELECTED_PATH"
finish

#!/usr/bin/env bash
# 21_runls11c_provider_readiness_smoke.sh — provider readiness smoke without submitting a bundle

source "$(dirname "$0")/lib.sh"
script_header "21C — RunLS11C Provider Readiness Smoke"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
SELECTED_PATH="${2:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
OUT_PATH="$INNER_STEP_DIR/runls11_provider_readiness_smoke.json"
WINDOW_CONFIG_PATH="$INNER_STEP_DIR/config.window_local.yaml"
PAYLOAD_PATH="$INNER_STEP_DIR/runls11_bundle_payload_base64.json"
PM_REPORT_PATH="$INNER_STEP_DIR/runls11_pm_reserve_report.json"
AUTH1_SELECTED_PATH="${SHADOW_RUNAUTH1_SELECTED_PROVIDER_PATH:-$INNER_STEP_DIR/runauth1_selected_provider.json}"

if [[ ! -f "$SELECTED_PATH" ]]; then
    fail "RunLS11 selected bundle doc missing: $SELECTED_PATH"
    finish
    exit 1
fi

python3 - "$SELECTED_PATH" "$OUT_PATH" "$WINDOW_CONFIG_PATH" "$PAYLOAD_PATH" "$PM_REPORT_PATH" "$AUTH1_SELECTED_PATH" \
    "${SHADOW_JITO_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_HARMONIC_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_JITO_AUTH_PATH:-}" \
    "${SHADOW_HARMONIC_AUTH_PATH:-}" \
    "${SHADOW_SOLANA_RPC_URL:-}" \
    "${SHADOW_RPC_URL:-}" \
    "${SHADOW_PUBLIC_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_RPC_URL:-}" \
    "${SHADOW_HELIUS_BUNDLE_API_KEY:-}" \
    "${SHADOW_HELIUS_SENDER_API_KEY:-}" \
    <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.request
from pathlib import Path

selected_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
window_config_path = Path(sys.argv[3])
payload_path = Path(sys.argv[4])
pm_report_path = Path(sys.argv[5])
auth1_selected_path = Path(sys.argv[6])
jito_status_url = sys.argv[7]
harmonic_status_url = sys.argv[8]
jito_auth_path = Path(sys.argv[9]) if sys.argv[9] else None
harmonic_auth_path = Path(sys.argv[10]) if sys.argv[10] else None
shadow_solana_rpc_url = sys.argv[11]
shadow_rpc_url = sys.argv[12]
shadow_public_rpc_url = sys.argv[13]
shadow_helius_bundle_rpc_url = sys.argv[14]
shadow_helius_bundle_api_key = sys.argv[15]
shadow_helius_sender_api_key = sys.argv[16]


def is_placeholder(value: str) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


def probe_url(url: str) -> tuple[bool, str]:
    if is_placeholder(url):
        return False, "placeholder"
    req = urllib.request.Request(url, method="GET")
    context = ssl.create_default_context() if url.startswith("https://") else None
    try:
        with urllib.request.urlopen(req, timeout=10, context=context) as response:
            return True, f"http_{response.status}"
    except urllib.error.HTTPError as exc:  # type: ignore[attr-defined]
        return True, f"http_{exc.code}"
    except Exception as exc:  # noqa: BLE001
        return False, str(exc)


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def redacted_url(url: str) -> str:
    if is_placeholder(url):
        return url or ""
    import urllib.parse

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
    import urllib.parse

    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment)
    )


def has_api_key(url: str, explicit_key: str = "") -> bool:
    if explicit_key:
        return True
    if is_placeholder(url):
        return False
    import urllib.parse

    parsed = urllib.parse.urlsplit(url)
    return any(
        key.lower() in {"api-key", "apikey"} and value and "CHANGE_ME" not in value
        for key, value in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    )


def rpc_probe(url: str, method: str, params) -> tuple[bool, str, bool]:
    if is_placeholder(url):
        return False, "placeholder", False
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
    except urllib.error.HTTPError as exc:  # type: ignore[attr-defined]
        text = exc.read().decode("utf-8", errors="replace")
        status = exc.code
    except Exception as exc:  # noqa: BLE001
        return False, str(exc), False
    try:
        payload = json.loads(text) if text else {}
    except json.JSONDecodeError:
        payload = {}
    err = payload.get("error") if isinstance(payload, dict) else None
    err_msg = str(err.get("message", "")) if isinstance(err, dict) else ""
    method_missing = (
        isinstance(err, dict)
        and (err.get("code") == -32601 or "method not found" in err_msg.lower())
    )
    auth_failed = status in {401, 403} or "api key" in err_msg.lower() or "unauthorized" in err_msg.lower()
    return status < 500 and not auth_failed, f"http_{status}", not method_missing and not auth_failed


def parse_window_config(path: Path) -> dict:
    if not path.exists():
        return {}
    lines = path.read_text(encoding="utf-8").splitlines()
    out: dict[str, str] = {}
    in_jito = False
    in_harmonic = False
    in_json_rpc = False
    in_region_endpoints = False
    for raw in lines:
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent == 0:
            in_jito = stripped == "jito:"
            in_harmonic = stripped == "harmonic:"
            in_json_rpc = False
            in_region_endpoints = False
            continue
        if in_jito:
            if indent == 2 and stripped == "json_rpc:":
                in_json_rpc = True
                continue
            if indent == 2 and stripped.startswith("block_engine_url:"):
                out["jito_block_engine_url"] = stripped.split(":", 1)[1].strip().strip('"')
            elif indent == 2 and stripped.startswith("auth_mode:"):
                out["jito_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
            elif in_json_rpc and indent == 4 and stripped.startswith("endpoint:"):
                out["jito_json_rpc_endpoint"] = stripped.split(":", 1)[1].strip().strip('"')
            elif in_json_rpc and indent == 4 and stripped.startswith("enabled:"):
                out["jito_json_rpc_enabled"] = stripped.split(":", 1)[1].strip().strip('"')
            elif indent <= 2 and stripped != "json_rpc:":
                in_json_rpc = False
        if in_harmonic:
            if indent == 2 and stripped == "region_endpoints:":
                in_region_endpoints = True
                continue
            if indent == 2 and stripped.startswith("auth_mode:"):
                out["harmonic_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
            elif in_region_endpoints and indent >= 4 and stripped.startswith("- "):
                out.setdefault("harmonic_region_endpoint", stripped[2:].strip().strip('"'))
            elif indent <= 2 and stripped != "region_endpoints:":
                in_region_endpoints = False
    return out


selected = json.loads(selected_path.read_text(encoding="utf-8"))
payload_doc = load_json(payload_path)
pm_doc = load_json(pm_report_path)
auth1_selected = load_json(auth1_selected_path)
route_family = selected.get("selected_route_family")
target_ready = bool(selected.get("selected_target_tx_bytes_available"))
config = parse_window_config(window_config_path)
txs_base64 = payload_doc.get("txs_base64") if isinstance(payload_doc.get("txs_base64"), list) else []

provider_override = auth1_selected.get("provider_route_name") or ""

if provider_override == "jito_via_helius":
    provider = "jito_via_helius"
    auth_source = "helius_rpc_api_key"
    submit_endpoint = shadow_helius_bundle_rpc_url or shadow_solana_rpc_url or shadow_rpc_url or shadow_public_rpc_url
    submit_endpoint = add_api_key(submit_endpoint, shadow_helius_bundle_api_key)
    provider_submit_endpoint = redacted_url(submit_endpoint)
    provider_auth_material_present = has_api_key(submit_endpoint, shadow_helius_bundle_api_key)
    status_reachable, status_probe, status_method_available = rpc_probe(submit_endpoint, "getBundleStatuses", [["0" * 64]])
    simulate_reachable, simulate_probe, simulate_method_available = rpc_probe(
        submit_endpoint,
        "simulateBundle",
        [{"encodedTransactions": txs_base64}],
    ) if txs_base64 else (False, "no_payload", False)
    submit_reachable = status_reachable
    submit_probe = status_probe
    simulate_bundle_available = bool(simulate_reachable and simulate_method_available)
elif route_family == "harmonic_bundle":
    status_url = harmonic_status_url
    auth_path = harmonic_auth_path
    provider = "harmonic_bundle"
    submit_endpoint = config.get("harmonic_region_endpoint", "")
    auth_source = "harmonic_keypair"
    provider_submit_endpoint = submit_endpoint
    provider_auth_material_present = bool(auth_path and auth_path.exists())
    status_reachable, status_probe = probe_url(status_url)
    submit_reachable, submit_probe = probe_url(submit_endpoint)
    simulate_bundle_available = False
else:
    status_url = jito_status_url
    auth_path = jito_auth_path
    provider = "jito_bundle"
    submit_endpoint = config.get("jito_json_rpc_endpoint", "") or config.get("jito_block_engine_url", "")
    auth_source = "jito_keypair"
    provider_submit_endpoint = submit_endpoint
    provider_auth_material_present = bool(auth_path and auth_path.exists())
    status_reachable, status_probe = probe_url(status_url)
    submit_reachable, submit_probe = probe_url(submit_endpoint)
    simulate_bundle_available = False

doc = {
    "provider": provider,
    "provider_route_name": provider_override or provider,
    "provider_auth_source": auth_source,
    "selected_route_family": route_family,
    "target_tx_bytes_available": target_ready,
    "selected_target_tx_bytes_available": target_ready,
    "bundle_envelope_built": bool(selected.get("bundle_envelope_built")),
    "bundle_tx_count": payload_doc.get("bundle_tx_count"),
    "tip_present": bool(payload_doc.get("tip_present")),
    "min_out_positive": bool(payload_doc.get("min_out_positive")),
    "net_ev_after_tip_positive": bool(payload_doc.get("net_ev_positive_after_tip")),
    "selected_pm_reserve_created": bool(pm_doc.get("selected_pm_reserve_created")),
    "selected_pm_reserve_released": bool(pm_doc.get("selected_pm_reserve_released")),
    "provider_submit_endpoint": provider_submit_endpoint,
    "provider_submit_endpoint_reachable": submit_reachable,
    "provider_submit_probe": submit_probe,
    "provider_status_url": redacted_url(status_url if provider_override != "jito_via_helius" else submit_endpoint),
    "provider_status_api_reachable": status_reachable,
    "provider_status_probe": status_probe,
    "provider_auth_material_present": provider_auth_material_present,
    "simulate_bundle_pass": None,
    "simulate_bundle_available": simulate_bundle_available,
    "simulate_bundle_unavailable_but_payload_valid": bool(target_ready and not simulate_bundle_available),
    "send_bundle_disabled_in_mode": True,
    "wire_send": 0,
    "live_landed": 0,
}
out_path.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
PY

info "Provider readiness smoke written to $OUT_PATH"
finish

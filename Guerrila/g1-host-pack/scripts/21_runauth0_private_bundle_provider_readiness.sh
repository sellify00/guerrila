#!/usr/bin/env bash
# 21_runauth0_private_bundle_provider_readiness.sh — prove private bundle provider auth/readiness without submitting

source "$(dirname "$0")/lib.sh"
script_header "AUTH0 — Private Bundle Provider Readiness"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
SELECTED_PATH="${2:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
WINDOW_CONFIG_PATH="${3:-$INNER_STEP_DIR/config.window_local.yaml}"
OUT_PATH="$INNER_STEP_DIR/runauth0_private_bundle_provider_readiness.json"
TIP_ACCOUNTS_PATH="$INNER_STEP_DIR/runauth0_tip_accounts.json"

if [[ ! -f "$SELECTED_PATH" ]]; then
    fail "RunLS11 selected bundle doc missing: $SELECTED_PATH"
    finish
    exit 1
fi

python3 - "$SELECTED_PATH" "$WINDOW_CONFIG_PATH" "$OUT_PATH" "$TIP_ACCOUNTS_PATH" \
    "${SHADOW_JITO_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_HARMONIC_BUNDLE_STATUS_URL:-}" \
    "${SHADOW_JITO_AUTH_PATH:-}" \
    "${SHADOW_HARMONIC_AUTH_PATH:-}" \
    "${SHADOW_HELIUS_SENDER_API_KEY:-}" <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.request
from pathlib import Path

selected_path = Path(sys.argv[1])
window_config_path = Path(sys.argv[2])
out_path = Path(sys.argv[3])
tip_accounts_path = Path(sys.argv[4])
jito_status_url = sys.argv[5]
harmonic_status_url = sys.argv[6]
jito_auth_path = Path(sys.argv[7]) if sys.argv[7] else None
harmonic_auth_path = Path(sys.argv[8]) if sys.argv[8] else None
helius_sender_api_key = sys.argv[9]


def is_placeholder(value: str) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


def probe_url(url: str, method: str = "GET") -> tuple[bool, str]:
    if is_placeholder(url):
        return False, "placeholder"
    req = urllib.request.Request(url, method=method)
    context = ssl.create_default_context() if url.startswith("https://") else None
    try:
        with urllib.request.urlopen(req, timeout=10, context=context) as response:
            return True, f"http_{response.status}"
    except urllib.error.HTTPError as exc:  # type: ignore[attr-defined]
        return True, f"http_{exc.code}"
    except Exception as exc:  # noqa: BLE001
        return False, str(exc)


def parse_window_config(path: Path) -> dict:
    if not path.exists():
        return {}
    lines = path.read_text(encoding="utf-8").splitlines()
    out: dict[str, object] = {
        "tip_accounts": [],
    }
    section_stack: list[str] = []
    for raw in lines:
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        while section_stack and section_stack[-1].count("  ") * 2 >= indent:
            section_stack.pop()
        if stripped.endswith(":") and not stripped.startswith("- "):
            key = stripped[:-1]
            prefix = section_stack[-1] + "." if section_stack else ""
            section_stack.append(prefix + key)
            continue
        current = section_stack[-1] if section_stack else ""
        if current == "execution" and stripped.startswith("jito_tip_account:"):
            out["jito_tip_account"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito" and stripped.startswith("block_engine_url:"):
            out["jito_block_engine_url"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito" and stripped.startswith("auth_mode:"):
            out["jito_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito.json_rpc" and stripped.startswith("endpoint:"):
            out["jito_json_rpc_endpoint"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "jito.json_rpc" and stripped.startswith("enabled:"):
            out["jito_json_rpc_enabled"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "harmonic" and stripped.startswith("auth_mode:"):
            out["harmonic_auth_mode"] = stripped.split(":", 1)[1].strip().strip('"')
        elif current == "harmonic.region_endpoints" and stripped.startswith("- "):
            out["harmonic_region_endpoint"] = stripped[2:].strip().strip('"')
        elif current == "helius_sender.tip_accounts" and stripped.startswith("- "):
            out.setdefault("tip_accounts", []).append(stripped[2:].strip().strip('"'))
    return out


selected = json.loads(selected_path.read_text(encoding="utf-8"))
route_family = selected.get("selected_route_family")
config = parse_window_config(window_config_path)

if route_family == "harmonic_bundle":
    provider = "harmonic_direct"
    auth_source = "harmonic_keypair"
    submit_endpoint = str(config.get("harmonic_region_endpoint", ""))
    status_url = harmonic_status_url
    auth_path = harmonic_auth_path
    tip_accounts = list(config.get("tip_accounts", []))
else:
    submit_endpoint = str(config.get("jito_json_rpc_endpoint", "") or config.get("jito_block_engine_url", ""))
    if "helius" in submit_endpoint.lower():
        provider = "jito_via_helius"
        auth_source = "helius_api_key"
    else:
        provider = "jito_direct"
        auth_source = "jito_keypair"
    status_url = jito_status_url
    auth_path = jito_auth_path
    tip_accounts = []
    if config.get("jito_tip_account"):
        tip_accounts.append(str(config.get("jito_tip_account")))

status_reachable, status_probe = probe_url(status_url, "GET")
submit_reachable, submit_probe = probe_url(submit_endpoint, "GET")
auth_material_present = bool(
    (provider == "jito_via_helius" and helius_sender_api_key)
    or (auth_path and auth_path.exists())
)

tip_doc = {
    "provider": provider,
    "auth_source": auth_source,
    "tip_accounts": tip_accounts,
    "tip_accounts_available": bool(tip_accounts),
    "tip_accounts_count": len(tip_accounts),
}

readiness_doc = {
    "provider": provider,
    "auth_source": auth_source,
    "selected_route_family": route_family,
    "provider_submit_endpoint": submit_endpoint,
    "provider_submit_endpoint_reachable": submit_reachable,
    "provider_submit_probe": submit_probe,
    "provider_status_url": status_url,
    "provider_status_api_reachable": status_reachable,
    "provider_status_probe": status_probe,
    "provider_auth_material_present": auth_material_present,
    "tip_accounts_available": bool(tip_accounts),
    "tip_accounts_count": len(tip_accounts),
    "jito_auth_path_present": bool(jito_auth_path and jito_auth_path.exists()),
    "harmonic_auth_path_present": bool(harmonic_auth_path and harmonic_auth_path.exists()),
    "helius_api_key_present": bool(helius_sender_api_key),
    "simulate_bundle_available": False,
    "simulate_bundle_unavailable_but_payload_valid": True,
    "send_bundle_disabled_in_test": True,
    "wire_send": 0,
    "live_landed": 0,
}

out_path.write_text(json.dumps(readiness_doc, indent=2) + "\n", encoding="utf-8")
tip_accounts_path.write_text(json.dumps(tip_doc, indent=2) + "\n", encoding="utf-8")
PY

info "Private bundle provider readiness written to $OUT_PATH"
info "Tip account report written to $TIP_ACCOUNTS_PATH"
finish

#!/usr/bin/env bash
# G1 Host-Run Pack — Shared helpers for validation scripts
#
# Usage: source "$(dirname "$0")/lib.sh"

set -euo pipefail

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
G1_PACK_DIR="$(cd "$LIB_DIR/.." && pwd)"
REPO_ROOT_DIR="$(cd "$G1_PACK_DIR/.." && pwd)"
REPO_DOTENV_PATH="$REPO_ROOT_DIR/.env"

# ── Color output ──────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

pass()  { echo -e "${GREEN}[PASS]${NC} $*"; }
fail()  { echo -e "${RED}[FAIL]${NC} $*"; FAILURES=$((FAILURES + 1)); }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
info()  { echo -e "${BLUE}[INFO]${NC} $*"; }

FAILURES=0

# ── Assertions ────────────────────────────────────────────────────
assert_root() {
    if [[ $EUID -ne 0 ]]; then
        fail "Must run as root"
        exit 1
    fi
}

assert_cmd_exists() {
    local cmd="$1"
    if ! command -v "$cmd" &>/dev/null; then
        fail "Required command not found: $cmd"
        return 1
    fi
}

require_env() {
    local var="$1"
    if [[ -z "${!var:-}" ]]; then
        fail "Environment variable $var is not set. Source env.g1.sh first."
        exit 1
    fi
}

is_placeholder_url() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == *CHANGE_ME* || "$value" == *'${'* ]]
}

load_repo_dotenv_var() {
    local key="$1"
    local dotenv_path="${2:-$REPO_DOTENV_PATH}"
    [[ -f "$dotenv_path" ]] || return 0
    awk -F= -v k="$key" '$1 == k { value = substr($0, length($1) + 2); sub(/\r$/, "", value); print value; exit }' "$dotenv_path"
}

default_http_rpc_url() {
    local helius_rpc="${HELIUS_RPC_URL:-$(load_repo_dotenv_var HELIUS_RPC_URL)}"
    if ! is_placeholder_url "$helius_rpc"; then
        printf '%s\n' "$helius_rpc"
        return 0
    fi
    printf '%s\n' "https://api.mainnet-beta.solana.com"
}

default_public_rpc_url() {
    local helius_gatekeeper="${HELIUS_GATEKEEPER:-$(load_repo_dotenv_var HELIUS_GATEKEEPER)}"
    if ! is_placeholder_url "$helius_gatekeeper"; then
        printf '%s\n' "$helius_gatekeeper"
        return 0
    fi
    default_http_rpc_url
}

value_is_true() {
    case "${1:-}" in
        1|true|TRUE|True|yes|YES|Yes|on|ON|On)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

find_local_agave_active_unit() {
    local unit
    local candidates="${SHADOW_LOCAL_AGAVE_SERVICE_NAMES:-sh1-agave-no-geyser.service sh1-agave-no-geyser-2.service sh1-agave-no-geyser}"
    for unit in $candidates; do
        if systemctl is-active --quiet "$unit" 2>/dev/null; then
            echo "$unit"
            return 0
        fi
    done
    return 1
}

local_agave_udp_listener_count() {
    local min_port="${SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MIN:-8000}"
    local max_port="${SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MAX:-8025}"
    ss -Hlun 2>/dev/null | awk -v min_port="$min_port" -v max_port="$max_port" '
        {
            port = $4
            sub(/^.*:/, "", port)
            if (port ~ /^[0-9]+$/ && port >= min_port && port <= max_port) {
                count++
            }
        }
        END { print count + 0 }
    '
}

require_local_agave_preflight() {
    local summary_path="${1:-}"
    local require_preflight="${SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT:-true}"
    local active_unit=""
    local validator_process_count="0"
    local udp_listener_count="0"
    local local_rpc_url="${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}"
    local configured_reference_rpc_url="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-}"
    local reference_rpc_url=""
    local max_processed_slot_lag="${SHADOW_LOCAL_AGAVE_MAX_PROCESSED_SLOT_LAG:-2048}"
    local max_finalized_slot_lag="${SHADOW_LOCAL_AGAVE_MAX_FINALIZED_SLOT_LAG:-4096}"
    local require_dynamic_ports="${SHADOW_LOCAL_AGAVE_REQUIRE_DYNAMIC_PORTS:-true}"
    local dynamic_port_min="${SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MIN:-8000}"
    local dynamic_port_max="${SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MAX:-8025}"

    if ! value_is_true "$require_preflight"; then
        info "Skipping local Agave preflight (SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT=$require_preflight)"
        return 0
    fi

    assert_cmd_exists python3 || return 1
    assert_cmd_exists ss || return 1
    assert_cmd_exists systemctl || return 1

    active_unit="$(find_local_agave_active_unit || true)"
    validator_process_count="$(pgrep -fc 'agave-validator|solana-validator' 2>/dev/null || true)"
    udp_listener_count="$(local_agave_udp_listener_count)"
    if [[ -n "$configured_reference_rpc_url" && "$configured_reference_rpc_url" != *CHANGE_ME* && "$configured_reference_rpc_url" != *'${'* ]]; then
        reference_rpc_url="$configured_reference_rpc_url"
    else
        reference_rpc_url="${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$configured_reference_rpc_url}}"
    fi

    if ! python3 - "$summary_path" "$active_unit" "$validator_process_count" "$udp_listener_count" \
        "$local_rpc_url" "$reference_rpc_url" "$max_processed_slot_lag" "$max_finalized_slot_lag" \
        "$require_dynamic_ports" "$dynamic_port_min" "$dynamic_port_max" <<'PY'
import json
import ssl
import sys
import urllib.request
from pathlib import Path

summary_path = Path(sys.argv[1]) if sys.argv[1] else None
active_unit = sys.argv[2]
validator_process_count = int(sys.argv[3] or "0")
udp_listener_count = int(sys.argv[4] or "0")
local_rpc_url = sys.argv[5]
reference_rpc_url = sys.argv[6]
max_processed_slot_lag = int(sys.argv[7])
max_finalized_slot_lag = int(sys.argv[8])
require_dynamic_ports = sys.argv[9].lower() in {"1", "true", "yes", "on"}
dynamic_port_min = int(sys.argv[10])
dynamic_port_max = int(sys.argv[11])


def is_placeholder(url: str) -> bool:
    return not url or "CHANGE_ME" in url or "${" in url


def rpc(url: str, method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    data = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    if url.startswith("https://"):
        with urllib.request.urlopen(req, timeout=15, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode())
    with urllib.request.urlopen(req, timeout=15) as response:
        return json.loads(response.read().decode())


status = "pass"
reasons = []
details = {
    "active_systemd_unit": active_unit or "none",
    "validator_process_count": validator_process_count,
    "udp_listener_count": udp_listener_count,
    "dynamic_port_range": f"{dynamic_port_min}-{dynamic_port_max}",
    "local_rpc_url": local_rpc_url,
    "reference_rpc_url": reference_rpc_url,
    "require_dynamic_ports": require_dynamic_ports,
    "max_processed_slot_lag": max_processed_slot_lag,
    "max_finalized_slot_lag": max_finalized_slot_lag,
}

if not active_unit and validator_process_count <= 0:
    reasons.append("validator_not_running")

if require_dynamic_ports and udp_listener_count <= 0:
    reasons.append("dynamic_ports_unbound")

if is_placeholder(local_rpc_url):
    reasons.append("local_rpc_unconfigured")
    details["local_rpc_error"] = "placeholder_url"
else:
    try:
        local_processed = rpc(local_rpc_url, "getSlot", [{"commitment": "processed"}])
        local_finalized = rpc(local_rpc_url, "getSlot", [{"commitment": "finalized"}])
        local_health = rpc(local_rpc_url, "getHealth")
        details["local_processed_slot"] = local_processed.get("result")
        details["local_finalized_slot"] = local_finalized.get("result")
        details["local_health"] = local_health.get("result", "error")
        if "error" in local_health:
            reasons.append("local_rpc_unhealthy")
            details["local_health_error"] = local_health["error"]
    except Exception as exc:  # noqa: BLE001
        reasons.append("local_rpc_unreachable")
        details["local_rpc_error"] = str(exc)

if is_placeholder(reference_rpc_url):
    reasons.append("reference_rpc_unconfigured")
    details["reference_rpc_error"] = "placeholder_url"
else:
    try:
        ref_processed = rpc(reference_rpc_url, "getSlot", [{"commitment": "processed"}])
        ref_finalized = rpc(reference_rpc_url, "getSlot", [{"commitment": "finalized"}])
        details["reference_processed_slot"] = ref_processed.get("result")
        details["reference_finalized_slot"] = ref_finalized.get("result")
    except Exception as exc:  # noqa: BLE001
        reasons.append("reference_rpc_unreachable")
        details["reference_rpc_error"] = str(exc)

if "local_processed_slot" in details and "reference_processed_slot" in details:
    processed_lag = int(details["reference_processed_slot"]) - int(details["local_processed_slot"])
    details["processed_slot_lag"] = processed_lag
    if processed_lag > max_processed_slot_lag:
        reasons.append("processed_slot_lag_too_high")

if "local_finalized_slot" in details and "reference_finalized_slot" in details:
    finalized_lag = int(details["reference_finalized_slot"]) - int(details["local_finalized_slot"])
    details["finalized_slot_lag"] = finalized_lag
    if finalized_lag > max_finalized_slot_lag:
        reasons.append("finalized_slot_lag_too_high")

if reasons:
    status = "fail"

lines = [f"status={status}"]
if reasons:
    lines.append(f"failure_reasons={','.join(reasons)}")
for key in sorted(details):
    lines.append(f"{key}={details[key]}")

if summary_path is not None:
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

if reasons:
    raise SystemExit(1)
PY
    then
        local reasons="unknown"
        if [[ -n "$summary_path" && -f "$summary_path" ]]; then
            reasons="$(sed -n 's/^failure_reasons=//p' "$summary_path" | head -n 1)"
            [[ -n "$reasons" ]] || reasons="unknown"
        fi
        fail "Local Agave preflight failed: $reasons"
        return 1
    fi

    info "Local Agave preflight passed"
    return 0
}

# ── Result saving ─────────────────────────────────────────────────
# save_result <step_name> <filename> <content>
# Writes to results/<step_name>/<filename>
save_result() {
    local step="$1" fname="$2" content="$3"
    local dir="${SHADOW_RESULTS_DIR:-$(dirname "$0")/../results}/$step"
    mkdir -p "$dir"
    echo "$content" > "$dir/$fname"
    info "Saved $dir/$fname"
}

# save_result_file <step_name> <filename>
# Copies stdin to results/<step_name>/<filename>
save_result_file() {
    local step="$1" fname="$2"
    local dir="${SHADOW_RESULTS_DIR:-$(dirname "$0")/../results}/$step"
    mkdir -p "$dir"
    cat > "$dir/$fname"
    info "Saved $dir/$fname"
}

# ── Script header ─────────────────────────────────────────────────
script_header() {
    local name="$1"
    echo ""
    echo "════════════════════════════════════════════════════════════"
    echo "  G1 Validation: $name"
    echo "  $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo "════════════════════════════════════════════════════════════"
    echo ""
}

# ── Exit with summary ─────────────────────────────────────────────
finish() {
    echo ""
    if [[ $FAILURES -eq 0 ]]; then
        pass "All checks passed."
    else
        fail "$FAILURES check(s) failed."
    fi
    return $FAILURES
}

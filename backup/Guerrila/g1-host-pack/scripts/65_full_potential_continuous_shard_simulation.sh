#!/usr/bin/env bash
# 65_full_potential_continuous_shard_simulation.sh - fresh full-potential continuous shard simulation

source "$(dirname "$0")/lib.sh"
script_header "65 - Full-Potential Continuous Shard Simulation"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE-}"
CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="${SHADOW_RUN39_USE_SAFE_ENO1_PROFILE-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_NIC_QUEUE" ]] && export SHADOW_NIC_QUEUE="$CALLER_SHADOW_NIC_QUEUE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_XDP_ATTACH_MODE" ]] && export SHADOW_XDP_ATTACH_MODE="$CALLER_SHADOW_XDP_ATTACH_MODE"
[[ -n "$CALLER_SHADOW_AFXDP_SAFE_MODE" ]] && export SHADOW_AFXDP_SAFE_MODE="$CALLER_SHADOW_AFXDP_SAFE_MODE"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE" ]] && export SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$CALLER_SHADOW_RUN39_NATIVE_OOB_ARM_FILE"
[[ -n "$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE" ]] && export SHADOW_RUN39_USE_SAFE_ENO1_PROFILE="$CALLER_SHADOW_RUN39_USE_SAFE_ENO1_PROFILE"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN65_STEP:-65_full_potential_continuous_shard_simulation}"
STATUS_PREFIX="${SHADOW_RUN65_STATUS_PREFIX:-run65_full_potential_continuous_shard_simulation}"
REPORT_BASENAME="${SHADOW_RUN65_REPORT_BASENAME:-run65_full_potential_continuous_shard_simulation}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
PRE_PREFLIGHT_FILE="$RESULT_DIR/pre_run_local_agave_preflight.txt"
POST_PREFLIGHT_FILE="$RESULT_DIR/post_run_local_agave_preflight.txt"

RUN61_STEP="${SHADOW_RUN65_RUN61_STEP:-$STEP}"
RUN61_SUMMARY="$RESULTS_ROOT/$RUN61_STEP/summary.txt"
RUN61_JSON="$RESULTS_ROOT/$RUN61_STEP/run61_continuous_shadow_live_simulation.json"

RUN55_STEP="${SHADOW_RUN65_RUN55_STEP:-${RUN61_STEP}_run55}"
RUN39_STEP="${SHADOW_RUN65_RUN39_STEP:-${RUN55_STEP}_run39}"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"
RUN39_SCORECARD_JSON="$RUN39_RESULT_DIR/surface_scorecard.json"
RUN39_OWNER_MODE_JSON="$RUN39_RESULT_DIR/owner_mode_scorecard.json"

RUN54_STEP="${SHADOW_RUN65_RUN54_STEP:-${RUN61_STEP}_run54}"
RUN54_SUMMARY="$RESULTS_ROOT/$RUN54_STEP/summary.txt"

RUN62_STEP="${SHADOW_RUN65_RUN62_STEP:-${STEP}_run62}"
RUN62_SUMMARY="$RESULTS_ROOT/$RUN62_STEP/summary.txt"
RUN62_JSON="$RESULTS_ROOT/$RUN62_STEP/run62_conversion_autopsy.json"

RUN64E_STEP="${SHADOW_RUN65_RUN64E_STEP:-${STEP}_run64e}"
RUN64E_SUMMARY="$RESULTS_ROOT/$RUN64E_STEP/summary.txt"
RUN64E_JSON="$RESULTS_ROOT/$RUN64E_STEP/run64e_exit_qualified_held_input_analysis.json"

WINDOW_SECONDS="${SHADOW_RUN65_WINDOW_SECONDS:-900}"
SHARD_COUNTS="${SHADOW_RUN65_SHARD_COUNTS:-1,2,4,8,16}"
EXECUTABLE_ROUTE_FAMILIES="${SHADOW_RUN65_EXECUTABLE_ROUTE_FAMILIES:-tpu_quic_public,public_rpc_scout}"
NO_SCALE_MINTS="${SHADOW_RUN65_NO_SCALE_MINTS:-8Nh2DSKq2JgK2AWg6tqi6NBUbbFsG47GVjhhri53WScx}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN65_MIN_RECURRENCE_COUNT:-2}"
SKIP_RUN61="${SHADOW_RUN65_SKIP_RUN61:-false}"
RPC_URL="${SHADOW_RUN65_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
PUBLIC_RPC_ENDPOINT="${SHADOW_RUN65_PUBLIC_RPC_ENDPOINT:-${SHADOW_PUBLIC_RPC_URL:-$RPC_URL}}"
SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-${RESULTS_ROOT}/run39_native_oob_ready.flag}"
ACTIVE_CONFIG="${SHADOW_RUN65_ACTIVE_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}"
RUN65_CONFIG_BASE="${SHADOW_RUN65_CONFIG_BASE_PATH:-$RESULT_DIR/config.run65.public_selector.base.yaml}"
AGAVE_ENTRYPOINT_HOST="${SHADOW_RUN65_AGAVE_ENTRYPOINT_HOST:-entrypoint.mainnet-beta.solana.com}"
ENABLE_PUBLIC_SELECTOR_RENDER="${SHADOW_RUN65_ENABLE_PUBLIC_SELECTOR_RENDER:-false}"
RUN39_CONFIG_SOURCE="$ACTIVE_CONFIG"

export SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}"
export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL}}"

RUN61_RC=0
RUN62_RC=0
RUN64E_RC=0
POST_RUN_RESTORED=false

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

bool_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

preflight_reasons() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    sed -n 's/^failure_reasons=//p' "$path" | tail -n 1
}

generate_run65_config_base() {
    python3 - "$ACTIVE_CONFIG" "$RUN65_CONFIG_BASE" "$PUBLIC_RPC_ENDPOINT" <<'PY'
import re
import sys
from pathlib import Path

src_path = Path(sys.argv[1])
dst_path = Path(sys.argv[2])
public_rpc_endpoint = sys.argv[3]

text = src_path.read_text(encoding="utf-8")


def replace_in_block(text: str, block_name: str, child_indent: int, replacements: dict[str, str]) -> str:
    lines = text.splitlines()
    inside = False
    block_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not inside:
            if stripped == f"{block_name}:":
                inside = True
                block_indent = indent
            continue
        if stripped and indent <= block_indent:
            inside = False
            if stripped == f"{block_name}:":
                inside = True
                block_indent = indent
            continue
        for key, value in replacements.items():
            expected_prefix = " " * child_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"


def replace_in_nested_block(
    text: str,
    parent: str,
    child: str,
    child_key_indent: int,
    replacements: dict[str, str],
) -> str:
    lines = text.splitlines()
    in_parent = False
    in_child = False
    parent_indent = 0
    child_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not in_parent:
            if stripped == f"{parent}:":
                in_parent = True
                parent_indent = indent
            continue
        if stripped and indent <= parent_indent:
            in_parent = False
            in_child = False
            if stripped == f"{parent}:":
                in_parent = True
                parent_indent = indent
            continue
        if not in_child:
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        if stripped and indent <= child_indent:
            in_child = False
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        for key, value in replacements.items():
            expected_prefix = " " * child_key_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"


def replace_line(text: str, pattern: str, replacement: str) -> str:
    if not re.search(pattern, text, flags=re.M):
        raise SystemExit(f"pattern not found: {pattern}")
    return re.sub(pattern, replacement, text, count=1, flags=re.M)


text = replace_in_block(
    text,
    "public_rpc",
    2,
    {
        "enabled": "true",
        "endpoint": f"\"{public_rpc_endpoint}\"",
    },
)
text = replace_in_nested_block(
    text,
    "r1",
    "public_scout",
    4,
    {
        "enabled": "true",
        "mode": "auto_admission",
    },
)
text = replace_in_nested_block(
    text,
    "submit",
    "routing",
    4,
    {
        "default_jito_searcher": "public_rpc_scout",
        "enable_harmonic_lane": "false",
    },
)
text = replace_line(
    text,
    r'(^\s*shock_research_export_path:\s*).+$',
    r'\1""',
) if re.search(r'^\s*shock_research_export_path:\s*', text, flags=re.M) else text

dst_path.write_text(text, encoding="utf-8")
PY
}

agave_dns_ready() {
    python3 - "$AGAVE_ENTRYPOINT_HOST" <<'PY'
import socket
import sys

host = sys.argv[1]
socket.getaddrinfo(host, 8001, type=socket.SOCK_STREAM)
PY
}

agave_last_start_failed_due_to_dns() {
    journalctl -u sh1-agave-no-geyser-2.service -n 40 --no-pager 2>/dev/null | grep -q "Temporary failure in name resolution"
}

recover_agave_dns() {
    warn "Recovering DNS before Agave restart"
    if systemctl list-unit-files systemd-resolved.service >/dev/null 2>&1; then
        systemctl restart systemd-resolved.service || true
    fi
    if command -v resolvectl >/dev/null 2>&1; then
        resolvectl flush-caches || true
    fi
    sleep 5
}

wait_for_agave_dns() {
    local label="$1"
    local attempt
    for attempt in $(seq 1 12); do
        if agave_dns_ready; then
            info "Agave entrypoint DNS is ready for $label"
            return 0
        fi
        warn "Waiting for Agave entrypoint DNS ($label) attempt=$attempt host=$AGAVE_ENTRYPOINT_HOST"
        if (( attempt == 3 || attempt == 8 )); then
            recover_agave_dns
        fi
        sleep 5
    done
    fail "Timed out waiting for Agave entrypoint DNS ($label)"
    return 1
}

reclaim_eno1() {
    local pattern
    for pattern in 'shadow-tier0' 'shadow-oracle'; do
        local pids remaining
        pids="$(pgrep -f "$pattern" || true)"
        if [[ -n "$pids" ]]; then
            info "Stopping pattern=$pattern pids=$pids"
            # shellcheck disable=SC2086
            kill $pids || true
            sleep 3
            remaining="$(pgrep -f "$pattern" || true)"
            if [[ -n "$remaining" ]]; then
                warn "Force stopping pattern=$pattern pids=$remaining"
                # shellcheck disable=SC2086
                kill -9 $remaining || true
            fi
        fi
    done
    ip link set dev eno1 xdp off || true
    rm -f /sys/fs/bpf/shadow_xsks_map 2>/dev/null || true
}

wait_for_green_local_agave() {
    local output_file="$1"
    local label="$2"
    local attempt
    for attempt in $(seq 1 20); do
        info "Waiting for green local Agave ($label) attempt=$attempt"
        if require_local_agave_preflight "$output_file"; then
            pass "Local Agave preflight passed for $label"
            return 0
        fi
        warn "Local Agave still red for $label: $(preflight_reasons "$output_file")"
        if agave_last_start_failed_due_to_dns; then
            fail "Local Agave restart hit DNS resolution failure for $label"
            return 2
        fi
        sleep 30
    done
    fail "Timed out waiting for green local Agave ($label)"
    return 1
}

restore_agave_posture() {
    local output_file="$1"
    local label="$2"
    local attempt wait_rc
    reclaim_eno1
    for attempt in $(seq 1 4); do
        wait_for_agave_dns "${label}_restart_${attempt}" || true
        systemctl restart sh1-agave-no-geyser-2.service
        wait_rc=0
        wait_for_green_local_agave "$output_file" "${label}_attempt_${attempt}" || wait_rc=$?
        if [[ "$wait_rc" -eq 0 ]]; then
            return 0
        fi
        if [[ "$wait_rc" -eq 2 ]] || agave_last_start_failed_due_to_dns; then
            recover_agave_dns
            continue
        fi
        return "$wait_rc"
    done
    fail "Unable to restore Agave posture for $label after repeated restart attempts"
    return 1
}

post_run_cleanup() {
    if bool_true "$SKIP_RUN61"; then
        return 0
    fi
    if bool_true "$POST_RUN_RESTORED"; then
        return 0
    fi
    info "Restoring Agave-first posture after Run65"
    if restore_agave_posture "$POST_PREFLIGHT_FILE" "post_run_cleanup"; then
        POST_RUN_RESTORED=true
    fi
}

trap post_run_cleanup EXIT

if bool_true "$SKIP_RUN61"; then
    info "Skipping fresh Run61 bounded window; reusing existing artifacts for $RUN61_STEP"
    for required_path in \
        "$RUN61_JSON" \
        "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
        "$RUN39_SCORECARD_JSON" \
        "$RUN39_OWNER_MODE_JSON"
    do
        if [[ ! -f "$required_path" ]]; then
            fail "Missing required existing Run61 artifact: $required_path"
            exit 2
        fi
    done
else
    info "Preparing Agave-first host posture before Run65"
    restore_agave_posture "$PRE_PREFLIGHT_FILE" "pre_run"

    if bool_true "$ENABLE_PUBLIC_SELECTOR_RENDER"; then
        generate_run65_config_base
        RUN39_CONFIG_SOURCE="$RUN65_CONFIG_BASE"
        info "Generated Run65 config base: $RUN65_CONFIG_BASE"
    else
        info "Using active Run65 config source without public-selector mutation: $RUN39_CONFIG_SOURCE"
    fi

    touch "$OOB_ARM_FILE"
    info "OOB arm file touched: $OOB_ARM_FILE"

    set +e
    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN61_STEP="$RUN61_STEP" \
        SHADOW_RUN61_WINDOW_SECONDS="$WINDOW_SECONDS" \
        SHADOW_RUN39_CONFIG_SRC="$RUN39_CONFIG_SOURCE" \
        SHADOW_RUN55_PUBLIC_RPC_ENDPOINT="$PUBLIC_RPC_ENDPOINT" \
        "$(dirname "$0")/61_post_shock_conversion.sh"
    RUN61_RC=$?
    set -e

    if [[ "$RUN61_RC" -ne 0 ]]; then
        fail "Run65 failed during fresh Run61 bounded window"
        exit 1
    fi

    info "Restoring Agave-first posture immediately after the AF_XDP window"
    restore_agave_posture "$POST_PREFLIGHT_FILE" "post_window"
    POST_RUN_RESTORED=true
fi

set +e
env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN62_STEP="$RUN62_STEP" \
    SHADOW_RUN62_RUN61_STEP="$RUN61_STEP" \
    SHADOW_RUN62_RUN39_STEP="$RUN39_STEP" \
    "$(dirname "$0")/62_conversion_autopsy.sh"
RUN62_RC=$?
set -e

if [[ "$RUN62_RC" -ne 0 ]]; then
    fail "Run65 failed during fresh Run62 autopsy"
    exit 1
fi

env \
    SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$RPC_URL" \
    "$(dirname "$0")/refresh_signer_token_inventory.sh"

set +e
env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN64E_STEP="$RUN64E_STEP" \
    SHADOW_RUN64E_RUN61_STEP="$RUN61_STEP" \
    SHADOW_RUN64E_RUN62_STEP="$RUN62_STEP" \
    SHADOW_RUN64E_RUN39_STEP="$RUN39_STEP" \
    SHADOW_RUN64E_RPC_URL="$RPC_URL" \
    SHADOW_RUN64E_MIN_RECURRENCE_COUNT="$MIN_RECURRENCE_COUNT" \
    SHADOW_RUN64E_NO_SCALE_MINTS="$NO_SCALE_MINTS" \
    "$(dirname "$0")/64e_exit_qualified_held_input_analysis.sh"
RUN64E_RC=$?
set -e

if [[ "$RUN64E_RC" -ne 0 ]]; then
    fail "Run65 failed during fresh Run64E exit-qualified held-input analysis"
    exit 1
fi

python3 - \
    "$RUN61_JSON" \
    "$RUN61_SUMMARY" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SCORECARD_JSON" \
    "$RUN39_OWNER_MODE_JSON" \
    "$RUN54_SUMMARY" \
    "$RUN62_JSON" \
    "$RUN62_SUMMARY" \
    "$RUN64E_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$ACTIVE_CONFIG" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$WINDOW_SECONDS" \
    "$SHARD_COUNTS" \
    "$EXECUTABLE_ROUTE_FAMILIES" \
    "$NO_SCALE_MINTS" \
    "$RPC_URL" \
    "$RUN61_RC" \
    "$RUN62_RC" \
    "$RUN64E_RC" <<'PY'
import json
import math
import ssl
import sys
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

run61_json_path = Path(sys.argv[1])
run61_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
scorecard_path = Path(sys.argv[4])
owner_mode_path = Path(sys.argv[5])
run54_summary_path = Path(sys.argv[6])
run62_json_path = Path(sys.argv[7])
run62_summary_path = Path(sys.argv[8])
run64e_json_path = Path(sys.argv[9])
inventory_path = Path(sys.argv[10])
active_config_path = Path(sys.argv[11])
summary_file = Path(sys.argv[12])
summary_json = Path(sys.argv[13])
step = sys.argv[14]
status_prefix = sys.argv[15]
window_seconds = int(sys.argv[16] or "900")
shard_counts = [int(item) for item in (sys.argv[17] or "1,2,4,8,16").split(",") if item.strip()]
executable_route_families = [item.strip() for item in (sys.argv[18] or "tpu_quic_public,public_rpc_scout").split(",") if item.strip()]
no_scale_mints = {item.strip() for item in (sys.argv[19] or "").split(",") if item.strip()}
rpc_url = sys.argv[20]
run61_rc = int(sys.argv[21])
run62_rc = int(sys.argv[22])
run64e_rc = int(sys.argv[23])

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}
ALL_OWNER_MODES = ["directarb", "backrun", "continuation", "housekeeping", "rebalancing", "flatten", "drop"]
POST_SHOCK_OWNER_MODES = ["continuation", "housekeeping", "rebalancing", "flatten", "drop"]
PRIVATE_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}
EXECUTABLE_ROUTE_FAMILIES = set(executable_route_families)
MAX_SHARDS = max(shard_counts) if shard_counts else 1
TEMPLATE_FIXABLE_VENUES = {"raydium_clmm", "raydium_cpmm", "raydium_v4", "meteora_damm_v2", "pumpfun_amm"}
FIXABLE_EXECUTION_BLOCKERS = {"missing_output_ata", "output_ata_uninitialized"}
OWNER_PRIORITY = {
    "directarb": 0,
    "backrun": 1,
    "continuation": 2,
    "rebalancing": 3,
    "housekeeping": 4,
    "flatten": 5,
    "drop": 6,
}
ROUTE_DURATION_SLOTS = {
    "tpu_quic_public": 1,
    "public_rpc_scout": 2,
}


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def load_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def load_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key] = value
    return data


def text_value(value):
    return (value or "").strip() if isinstance(value, str) else str(value or "").strip()


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def owner_mode(row):
    return text_value(row.get("selected_owner")).lower() or "drop"


def positive_research(row):
    return bool(row.get("research_positive")) or safe_int(row.get("net_after_fee_lamports")) > 0


def beneficial_owner_signal(row):
    owner = owner_mode(row)
    if owner in {"flatten", "drop"}:
        return safe_int(row.get("retained_ev_lamports")) > 0 or safe_int(best_horizon_net_after_fee_lamports(row)) > 0
    return positive_research(row)


def route_family(row):
    explicit = text_value(row.get("target_route_family")).lower()
    if explicit:
        if explicit in {"public_rpc", "public", "public_rpc_scout"}:
            return "public_rpc_scout"
        return explicit
    tokens = [
        text_value(row.get("route")).lower(),
        text_value(row.get("route_reason")).lower(),
    ]
    joined = " ".join(token for token in tokens if token)
    if "tpu" in joined:
        return "tpu_quic_public"
    if "public" in joined:
        return "public_rpc_scout"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    for token in tokens:
        if token:
            return token
    return "unknown"


def best_horizon_name(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return text_value(markouts.get("best_horizon")) or "none"


def best_horizon_net_after_fee_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports"))


def dedupe_key(row):
    candidate_id = text_value(row.get("candidate_id"))
    if candidate_id:
        return candidate_id
    return "|".join(
        [
            text_value(row.get("slot")),
            text_value(row.get("signature")),
            text_value(row.get("pool")),
            text_value(row.get("mint_in")),
            text_value(row.get("mint_out")),
            owner_mode(row),
            route_family(row),
            text_value(row.get("trade_size_lamports")),
        ]
    )


def owner_priority(owner):
    return OWNER_PRIORITY.get(owner, 99)


def percentile(values, pct):
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = (len(ordered) - 1) * pct
    low = math.floor(index)
    high = math.ceil(index)
    if low == high:
        return ordered[low]
    weight = index - low
    return ordered[low] * (1 - weight) + ordered[high] * weight


def placeholder_url(url: str) -> bool:
    return not url or "CHANGE_ME" in url or "${" in url


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=45, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def fetch_mint_metadata(mints):
    result = {
        WSOL_MINT: {"decimals": 9, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDC_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDT_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
    }
    if placeholder_url(rpc_url):
        return result
    pending = [mint for mint in sorted(set(mints)) if mint not in result]
    for index in range(0, len(pending), 100):
        chunk = pending[index:index + 100]
        try:
            response = rpc("getMultipleAccounts", [chunk, {"encoding": "jsonParsed", "commitment": "confirmed"}])
        except Exception:
            for mint in chunk:
                result[mint] = {
                    "decimals": None,
                    "token_program": None,
                }
            continue
        while isinstance(response, list):
            response = response[0] if response else {}
        result_field = response.get("result") if isinstance(response, dict) else {}
        while isinstance(result_field, list):
            result_field = result_field[0] if result_field else {}
        values = (result_field.get("value") if isinstance(result_field, dict) else []) or []
        if not isinstance(values, list):
            values = []
        for mint, value in zip(chunk, values):
            try:
                while isinstance(value, list):
                    value = value[0] if value else None
                if not isinstance(value, dict):
                    raise TypeError("value_not_dict")
                data_field = value.get("data")
                parsed_field = data_field.get("parsed") if isinstance(data_field, dict) else {}
                info_field = parsed_field.get("info") if isinstance(parsed_field, dict) else {}
                info = info_field if isinstance(info_field, dict) else {}
                owner = text_value(value.get("owner")) or None
                result[mint] = {
                    "decimals": info.get("decimals"),
                    "token_program": owner,
                }
            except Exception:
                result[mint] = {
                    "decimals": None,
                    "token_program": None,
                }
    return result


run61_payload = load_json(run61_json_path)
run61_summary = load_summary(run61_summary_path)
run54_summary = load_summary(run54_summary_path)
run62_payload = load_json(run62_json_path)
run62_summary = load_summary(run62_summary_path)
run64e_payload = load_json(run64e_json_path)
inventory_payload = load_json(inventory_path)
scorecard_payload = load_json(scorecard_path)
owner_mode_payload = load_json(owner_mode_path)
rows = load_jsonl(candidate_autopsy_path)
sol_usd_equivalent = safe_float((run61_payload.get("inputs") or {}).get("sol_usd_equivalent")) or safe_float(run61_summary.get("sol_usd_equivalent")) or 1000.0

inventory_accounts = inventory_payload.get("accounts") or []
actual_inventory_balances_raw = {
    text_value(item.get("mint_pubkey")): safe_int(item.get("amount_raw"))
    for item in inventory_accounts
    if text_value(item.get("mint_pubkey"))
}
actual_inventory_accounts = {
    text_value(item.get("mint_pubkey")): text_value(item.get("token_account_pubkey")) or f"ata:{text_value(item.get('mint_pubkey'))}"
    for item in inventory_accounts
    if text_value(item.get("mint_pubkey"))
}

unique_rows = []
seen = set()
for row in rows:
    key = dedupe_key(row)
    if key in seen:
        continue
    seen.add(key)
    unique_rows.append(row)

candidate_count = len(rows)
deduped_count = len(unique_rows)
suppressed_duplicate_count = candidate_count - deduped_count

unique_mints = set()
for row in unique_rows:
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    if mint_in:
        unique_mints.add(mint_in)
    if mint_out:
        unique_mints.add(mint_out)
mint_metadata = fetch_mint_metadata(unique_mints)

rank_lookup = {}
for item in run64e_payload.get("ranked_not_held_input_mints") or []:
    mint = text_value(item.get("mint"))
    if mint:
        rank_lookup[mint] = item
recommended_mints = {
    text_value(item.get("mint"))
    for item in (run64e_payload.get("recommended_candidates") or [])
    if text_value(item.get("mint"))
}
full_ready_candidate_mints = {
    text_value(item.get("mint"))
    for item in (run64e_payload.get("ranked_not_held_input_mints") or [])
    if text_value(item.get("mint")) and text_value(item.get("mint")) not in no_scale_mints
}

approved_held_input_mints = set((run62_payload.get("inventory_context") or {}).get("approved_held_input_mints") or [])
held_input_mints = set((run62_payload.get("inventory_context") or {}).get("held_input_mints") or [])


def input_decimals(mint):
    return mint_metadata.get(mint, {}).get("decimals")


def normalize_raw_amount(raw_value, mint):
    raw_value = safe_int(raw_value)
    if mint == WSOL_MINT:
        return raw_value / 1_000_000_000.0
    if mint in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0
    decimals = input_decimals(mint)
    if isinstance(decimals, int) and 0 <= decimals <= 18:
        return raw_value / (10 ** decimals)
    return None


def usd_equivalent_for_raw(raw_value, mint):
    native = normalize_raw_amount(raw_value, mint)
    if native is None:
        rank = rank_lookup.get(mint) or {}
        band_raw = safe_int(rank.get("required_band_input_raw_p90")) or safe_int(rank.get("required_band_input_raw_max"))
        acquisition_cost_usdc = safe_float(rank.get("acquisition_cost_usdc"))
        if band_raw > 0 and acquisition_cost_usdc > 0:
            return raw_value / band_raw * acquisition_cost_usdc
        return None
    if mint in {USDC_MINT, USDT_MINT}:
        return native
    if mint == WSOL_MINT:
        return native * sol_usd_equivalent
    rank = rank_lookup.get(mint) or {}
    band_raw = safe_int(rank.get("required_band_input_raw_p90")) or safe_int(rank.get("required_band_input_raw_max"))
    acquisition_cost_usdc = safe_float(rank.get("acquisition_cost_usdc"))
    if band_raw > 0 and acquisition_cost_usdc > 0:
        return raw_value / band_raw * acquisition_cost_usdc
    return None


def sol_equivalent_for_raw(raw_value, mint):
    usd_value = usd_equivalent_for_raw(raw_value, mint)
    if usd_value is None or not sol_usd_equivalent:
        if mint == WSOL_MINT:
            return normalize_raw_amount(raw_value, mint)
        return None
    return usd_value / sol_usd_equivalent


def effective_route_family(row):
    route = route_family(row)
    if route in {"public", "public_rpc", "public_rpc_scout"}:
        return "public_rpc_scout"
    return route


def size_bucket_label(row):
    mint_in = text_value(row.get("mint_in"))
    raw_value = safe_int(row.get("trade_size_lamports"))
    native_value = normalize_raw_amount(raw_value, mint_in)
    usd_value = usd_equivalent_for_raw(raw_value, mint_in)
    if mint_in == WSOL_MINT and native_value is not None:
        caps = [
            (0.005, "<=_5_usdc_or_0.005_sol_equiv"),
            (0.01, "<=_10_usdc_or_0.01_sol_equiv"),
            (0.025, "<=_25_usdc_or_0.025_sol_equiv"),
            (0.05, "<=_50_usdc_or_0.05_sol_equiv"),
            (0.1, "<=_100_usdc_or_0.1_sol_equiv"),
            (0.25, "<=_250_usdc_or_0.25_sol_equiv"),
            (0.5, "<=_500_usdc_or_0.5_sol_equiv"),
            (1.0, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2.0, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if native_value <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    if usd_value is not None:
        caps = [
            (5, "<=_5_usdc_or_0.005_sol_equiv"),
            (10, "<=_10_usdc_or_0.01_sol_equiv"),
            (25, "<=_25_usdc_or_0.025_sol_equiv"),
            (50, "<=_50_usdc_or_0.05_sol_equiv"),
            (100, "<=_100_usdc_or_0.1_sol_equiv"),
            (250, "<=_250_usdc_or_0.25_sol_equiv"),
            (500, "<=_500_usdc_or_0.5_sol_equiv"),
            (1000, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2000, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if usd_value <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    return "unbucketed_other_input_units"


def compact_row(row):
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    trade_size_raw = safe_int(row.get("trade_size_lamports"))
    net_after_fee_raw = safe_int(row.get("net_after_fee_lamports"))
    retained_ev_raw = safe_int(row.get("retained_ev_lamports"))
    best_horizon_raw = safe_int(best_horizon_net_after_fee_lamports(row))
    route = effective_route_family(row)
    return {
        "candidate_id": text_value(row.get("candidate_id")) or None,
        "slot": safe_int(row.get("slot")),
        "owner_mode": owner_mode(row),
        "venue": text_value(row.get("venue")) or None,
        "pool": text_value(row.get("pool")) or None,
        "route_family": route,
        "input_mint": mint_in or None,
        "input_decimals": input_decimals(mint_in),
        "input_native_size": round_or_none(normalize_raw_amount(trade_size_raw, mint_in), 9),
        "input_usd_equivalent": round_or_none(usd_equivalent_for_raw(trade_size_raw, mint_in), 9),
        "input_sol_equivalent": round_or_none(sol_equivalent_for_raw(trade_size_raw, mint_in), 9),
        "output_mint": mint_out or None,
        "output_decimals": input_decimals(mint_out),
        "retained_ev_input_native": round_or_none(normalize_raw_amount(retained_ev_raw, mint_in), 9),
        "policy_tip_input_native": round_or_none(normalize_raw_amount(safe_int(row.get("policy_tip_lamports")), mint_in), 9),
        "net_after_fee_input_native": round_or_none(normalize_raw_amount(net_after_fee_raw, mint_in), 9),
        "net_after_fee_usd_equivalent": round_or_none(usd_equivalent_for_raw(net_after_fee_raw, mint_in), 9),
        "net_after_fee_sol_equivalent": round_or_none(sol_equivalent_for_raw(net_after_fee_raw, mint_in), 9),
        "best_horizon_name": best_horizon_name(row),
        "best_horizon_net_after_fee_sol_equivalent": round_or_none(sol_equivalent_for_raw(best_horizon_raw, mint_in), 9),
        "selected_owner_score": safe_float(row.get("selected_owner_score")),
        "execution_blocker": text_value(row.get("execution_blocker")).lower(),
        "first_rejection_stage": text_value(row.get("first_rejection_stage")).lower(),
        "size_bucket": size_bucket_label(row),
        "output_account_fixable": output_account_fixable(row),
        "template_fixable": template_fixable(row),
    }


def output_account_fixable(row):
    return text_value(row.get("execution_blocker")).lower() in {"missing_output_ata", "output_ata_uninitialized"} or text_value(
        row.get("first_rejection_stage")
    ).lower() == "output_account_not_ready"


def template_fixable(row):
    return text_value(row.get("first_rejection_stage")).lower() == "template_not_ready" and text_value(
        row.get("venue")
    ).lower() in TEMPLATE_FIXABLE_VENUES


def output_account_ready_or_fixable(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized") or output_account_fixable(row)


def template_ready_or_fixable(row):
    return bool_field(row, "template_ready", "would_execute_current_template") or template_fixable(row)


def execution_blocker_allows_no_send_conversion(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    return blocker in {"", "none"} or blocker in FIXABLE_EXECUTION_BLOCKERS


def shadow_ready_nonroute(row):
    return (
        bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"
    ) and beneficial_owner_signal(row) and bool_field(
        row, "input_account_ready", "has_input_ata", "input_ata_initialized"
    ) and output_account_ready_or_fixable(row) and template_ready_or_fixable(row) and bool_field(
        row, "quote_ready_or_classified", "would_admit"
    ) and execution_blocker_allows_no_send_conversion(row)


def actual_inventory_gate(row):
    mint_in = text_value(row.get("mint_in"))
    if mint_in in no_scale_mints:
        return False
    return bool_field(row, "approved_held_input", "is_approved_held_input") and bool_field(
        row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient"
    )


directarb_rows = [row for row in unique_rows if owner_mode(row) == "directarb"]
route_counter = Counter(effective_route_family(row) for row in unique_rows)
owner_selected_counts = Counter(owner_mode(row) for row in unique_rows)
blocked_private_route_count = sum(route_counter[route] for route in PRIVATE_ROUTE_FAMILIES if route in route_counter)
blocked_route_unavailable_count = sum(
    count for route, count in route_counter.items() if route not in EXECUTABLE_ROUTE_FAMILIES and route not in PRIVATE_ROUTE_FAMILIES
)

per_mint_trade_stats = defaultdict(list)
for row in unique_rows:
    mint_in = text_value(row.get("mint_in"))
    raw_trade = safe_int(row.get("trade_size_lamports"))
    if mint_in and raw_trade > 0:
        per_mint_trade_stats[mint_in].append(raw_trade)


def synthetic_balance_for_mint(mint, multiplier):
    values = per_mint_trade_stats.get(mint) or []
    if not values:
        rank = rank_lookup.get(mint) or {}
        fallback = safe_int(rank.get("required_band_input_raw_p90")) or safe_int(rank.get("required_band_input_raw_max"))
        return max(fallback, 0)
    p90 = int(math.ceil(percentile(values, 0.90) or 0))
    return max(1, p90 * max(1, multiplier))


class InventoryModel:
    def __init__(self, name, extra_input_mints=None, full_ready=False):
        self.name = name
        self.extra_input_mints = set(extra_input_mints or [])
        self.full_ready = full_ready
        self.extra_balances_raw = {}
        multiplier = MAX_SHARDS if full_ready else 1
        for mint in self.extra_input_mints:
            self.extra_balances_raw[mint] = synthetic_balance_for_mint(mint, multiplier)

    def input_account_key(self, mint):
        if mint in actual_inventory_accounts:
            return actual_inventory_accounts[mint]
        if mint in self.extra_input_mints or self.full_ready:
            return f"synthetic_input:{mint}"
        return None

    def output_account_key(self, mint):
        if self.full_ready:
            return f"synthetic_output:{mint}"
        return f"ata:{mint}" if mint else None

    def inventory_ready(self, row):
        mint_in = text_value(row.get("mint_in"))
        if mint_in in no_scale_mints:
            return False
        if self.full_ready and mint_in:
            return True
        if actual_inventory_gate(row):
            return True
        return mint_in in self.extra_input_mints

    def input_account_ready(self, row):
        mint_in = text_value(row.get("mint_in"))
        if mint_in in no_scale_mints:
            return False
        if self.full_ready and mint_in:
            return True
        if bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized"):
            return True
        return mint_in in self.extra_input_mints

    def output_account_ready(self, row):
        if self.full_ready:
            return True
        return output_account_ready_or_fixable(row)

    def base_balances(self):
        balances = dict(actual_inventory_balances_raw)
        for mint, amount in self.extra_balances_raw.items():
            balances[mint] = max(balances.get(mint, 0), amount)
        if self.full_ready:
            for mint in per_mint_trade_stats:
                if mint in no_scale_mints:
                    continue
                balances[mint] = max(balances.get(mint, 0), synthetic_balance_for_mint(mint, MAX_SHARDS))
        return balances


models = {
    "actual_inventory_result": InventoryModel("actual_inventory_result"),
    "exit_qualified_inventory_counterfactual": InventoryModel("exit_qualified_inventory_counterfactual", recommended_mints),
    "full_ready_counterfactual": InventoryModel("full_ready_counterfactual", full_ready_candidate_mints, full_ready=True),
}


def blocker_reason(row, model, include_owner_mode=False):
    route = effective_route_family(row)
    if route not in EXECUTABLE_ROUTE_FAMILIES:
        if route in PRIVATE_ROUTE_FAMILIES:
            return "route_private_only"
        return "route_unavailable"
    if not (bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"):
        return "source_not_afxdp"
    if not model.inventory_ready(row):
        return "not_held_input"
    if not model.input_account_ready(row):
        return "input_account_not_ready"
    mint_in = text_value(row.get("mint_in"))
    balance_raw = model.base_balances().get(mint_in, 0)
    trade_raw = safe_int(row.get("trade_size_lamports"))
    if balance_raw < trade_raw:
        return "inventory_insufficient"
    if not model.output_account_ready(row):
        return "account_ata_missing"
    if not template_ready_or_fixable(row):
        return "template_missing"
    if not bool_field(row, "quote_ready_or_classified", "would_admit"):
        return "quote_missing"
    if not beneficial_owner_signal(row):
        return "not_positive_research"
    if not execution_blocker_allows_no_send_conversion(row):
        return text_value(row.get("execution_blocker")).lower()
    if include_owner_mode and owner_mode(row) not in ALL_OWNER_MODES:
        return "owner_mode_unknown"
    return "none"


def primary_ev_by_category(row):
    mint_in = text_value(row.get("mint_in"))
    immediate = safe_int(row.get("net_after_fee_lamports"))
    best_horizon = safe_int(best_horizon_net_after_fee_lamports(row))
    retained = safe_int(row.get("retained_ev_lamports"))
    owner = owner_mode(row)
    immediate_sol = sol_equivalent_for_raw(immediate, mint_in)
    later_sol = sol_equivalent_for_raw(best_horizon, mint_in)
    saved_sol = sol_equivalent_for_raw(retained, mint_in)
    if owner in {"continuation", "housekeeping", "rebalancing"}:
        primary = later_sol if later_sol is not None and later_sol > 0 else immediate_sol
        category = "later_horizon"
    elif owner in {"flatten", "drop"}:
        primary = saved_sol if saved_sol is not None and saved_sol > 0 else later_sol
        category = "saved_loss"
    else:
        primary = immediate_sol
        category = "immediate"
    return {
        "category": category,
        "primary_sol_equivalent": primary,
        "immediate_sol_equivalent": immediate_sol,
        "later_horizon_sol_equivalent": later_sol,
        "saved_loss_sol_equivalent": saved_sol,
    }


def conflict_key(row):
    return (
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
        effective_route_family(row),
        size_bucket_label(row),
    )


def row_sort_key(row):
    ev = primary_ev_by_category(row)["primary_sol_equivalent"]
    return (
        safe_int(row.get("slot")),
        owner_priority(owner_mode(row)),
        -(ev if ev is not None else -1e18),
        -safe_float(row.get("selected_owner_score")),
        text_value(row.get("candidate_id")),
    )


def simulate(model: InventoryModel, shard_count: int):
    ordered_rows = sorted(unique_rows, key=row_sort_key)
    shard_available = [0 for _ in range(shard_count)]
    active_allocations = []
    conflict_locks = {}
    input_locks = {}
    output_locks = {}
    base_balances = model.base_balances()
    max_capital_used_usd = 0.0
    total_capital_usd = 0.0
    for mint, amount in base_balances.items():
        usd_value = usd_equivalent_for_raw(amount, mint)
        if usd_value is not None:
            total_capital_usd += usd_value

    counts = {
        "would_fire_total": 0,
        "would_fire_directarb": 0,
        "would_fire_backrun": 0,
        "would_fire_continuation": 0,
        "would_fire_housekeeping": 0,
        "would_fire_rebalancing": 0,
        "would_fire_flatten": 0,
        "would_fire_drop": 0,
        "would_enqueue_tpu_count": 0,
        "would_public_rpc_count": 0,
        "would_wire_send_count": 0,
        "suppressed_duplicate_count": suppressed_duplicate_count,
        "suppressed_conflict_domain_count": 0,
        "suppressed_no_free_shard_count": 0,
        "suppressed_inventory_unavailable_count": 0,
        "suppressed_account_lock_count": 0,
        "suppressed_capital_unavailable_count": 0,
        "suppressed_route_unavailable_count": 0,
        "blocked_account_not_ready_count": 0,
        "blocked_other_count": 0,
    }
    missed = {
        "missed_ev_due_to_serial_execution": 0.0,
        "missed_ev_due_to_inventory": 0.0,
        "missed_ev_due_to_route": 0.0,
        "missed_ev_due_to_account_conflict": 0.0,
        "missed_ev_due_to_capital": 0.0,
    }
    blocker_counts = Counter()
    owner_fire_counts = Counter()
    owner_capture_counts = Counter()
    shard_fire_counts = Counter()
    scheduled_rows = []
    route_dispatch = Counter()
    current_slot = None
    capital_by_slot = defaultdict(float)

    for row in ordered_rows:
        slot = safe_int(row.get("slot"))
        current_slot = slot if current_slot is None else current_slot
        expired = [item for item in active_allocations if item["release_slot"] <= slot]
        active_allocations = [item for item in active_allocations if item["release_slot"] > slot]
        conflict_locks = {key: value for key, value in conflict_locks.items() if value > slot}
        input_locks = {key: value for key, value in input_locks.items() if value > slot}
        output_locks = {key: value for key, value in output_locks.items() if value > slot}

        ev_info = primary_ev_by_category(row)
        ev_sol = ev_info["primary_sol_equivalent"] or 0.0
        owner = owner_mode(row)
        route = effective_route_family(row)
        reason = blocker_reason(row, model)
        if reason != "none":
            blocker_counts[reason] += 1
            if reason in {"route_private_only", "route_unavailable"}:
                counts["suppressed_route_unavailable_count"] += 1
                missed["missed_ev_due_to_route"] += ev_sol
            elif reason in {"not_held_input", "inventory_insufficient"}:
                counts["suppressed_inventory_unavailable_count"] += 1
                missed["missed_ev_due_to_inventory"] += ev_sol
            elif reason in {"input_account_not_ready", "account_ata_missing"}:
                counts["blocked_account_not_ready_count"] += 1
                missed["missed_ev_due_to_account_conflict"] += ev_sol
            else:
                counts["blocked_other_count"] += 1
            continue

        free_shard = next((index for index, free_at in enumerate(shard_available) if free_at <= slot), None)
        if free_shard is None:
            counts["suppressed_no_free_shard_count"] += 1
            blocker_counts["no_free_shard"] += 1
            missed["missed_ev_due_to_serial_execution"] += ev_sol
            continue

        ckey = conflict_key(row)
        if ckey in conflict_locks:
            counts["suppressed_conflict_domain_count"] += 1
            blocker_counts["conflict_domain"] += 1
            missed["missed_ev_due_to_serial_execution"] += ev_sol
            continue

        mint_in = text_value(row.get("mint_in"))
        mint_out = text_value(row.get("mint_out"))
        input_account = model.input_account_key(mint_in)
        output_account = model.output_account_key(mint_out)
        if (input_account and input_account in input_locks) or (output_account and output_account in output_locks):
            counts["suppressed_account_lock_count"] += 1
            blocker_counts["account_lock_conflict"] += 1
            missed["missed_ev_due_to_account_conflict"] += ev_sol
            continue

        trade_raw = safe_int(row.get("trade_size_lamports"))
        allocated_raw = sum(item["amount_raw"] for item in active_allocations if item["mint_in"] == mint_in)
        if base_balances.get(mint_in, 0) < allocated_raw + trade_raw:
            counts["suppressed_capital_unavailable_count"] += 1
            blocker_counts["capital_unavailable"] += 1
            missed["missed_ev_due_to_capital"] += ev_sol
            continue

        duration = ROUTE_DURATION_SLOTS.get(route, 1)
        release_slot = slot + duration
        shard_available[free_shard] = release_slot
        conflict_locks[ckey] = release_slot
        if input_account:
            input_locks[input_account] = release_slot
        if output_account:
            output_locks[output_account] = release_slot
        active_allocations.append({"release_slot": release_slot, "mint_in": mint_in, "amount_raw": trade_raw})

        input_usd = usd_equivalent_for_raw(trade_raw, mint_in) or 0.0
        capital_by_slot[slot] += input_usd
        max_capital_used_usd = max(max_capital_used_usd, capital_by_slot[slot])

        counts["would_fire_total"] += 1
        counts[f"would_fire_{owner}"] += 1
        owner_fire_counts[owner] += 1
        owner_capture_counts[owner] += 1
        shard_fire_counts[free_shard + 1] += 1
        route_dispatch[route] += 1
        if route == "tpu_quic_public":
            counts["would_enqueue_tpu_count"] += 1
        elif route == "public_rpc_scout":
            counts["would_public_rpc_count"] += 1

        scheduled_rows.append(
            {
                "shard": free_shard + 1,
                "release_slot": release_slot,
                "ev": ev_info,
                **compact_row(row),
            }
        )

    captured_immediate = sum(item["ev"]["immediate_sol_equivalent"] or 0.0 for item in scheduled_rows)
    captured_later = sum(item["ev"]["later_horizon_sol_equivalent"] or 0.0 for item in scheduled_rows)
    captured_saved = sum(item["ev"]["saved_loss_sol_equivalent"] or 0.0 for item in scheduled_rows)
    captured_total = sum(item["ev"]["primary_sol_equivalent"] or 0.0 for item in scheduled_rows)
    missed_total = sum(missed.values())
    best_owner_mode = owner_capture_counts.most_common(1)[0][0] if owner_capture_counts else "none"
    worst_blocker = blocker_counts.most_common(1)[0][0] if blocker_counts else "none"
    conflict_rate = (
        (counts["suppressed_conflict_domain_count"] + counts["suppressed_account_lock_count"] + counts["suppressed_no_free_shard_count"])
        / deduped_count
        if deduped_count
        else 0.0
    )
    capital_utilization = (max_capital_used_usd / total_capital_usd) if total_capital_usd > 0 else 0.0

    return {
        "model": model.name,
        "shards": shard_count,
        "would_fire_count": counts["would_fire_total"],
        "directarb_fire_count": counts["would_fire_directarb"],
        "post_shock_fire_count": counts["would_fire_continuation"] + counts["would_fire_housekeeping"] + counts["would_fire_rebalancing"] + counts["would_fire_flatten"] + counts["would_fire_drop"],
        "captured_ev_sol_equivalent": round_or_none(captured_total, 9),
        "captured_immediate_ev_sol_equivalent": round_or_none(captured_immediate, 9),
        "captured_later_horizon_ev_sol_equivalent": round_or_none(captured_later, 9),
        "captured_saved_loss_ev_sol_equivalent": round_or_none(captured_saved, 9),
        "missed_ev_sol_equivalent": round_or_none(missed_total, 9),
        "missed_ev_due_to_serial_execution": round_or_none(missed["missed_ev_due_to_serial_execution"], 9),
        "missed_ev_due_to_inventory": round_or_none(missed["missed_ev_due_to_inventory"], 9),
        "missed_ev_due_to_route": round_or_none(missed["missed_ev_due_to_route"], 9),
        "missed_ev_due_to_account_conflict": round_or_none(missed["missed_ev_due_to_account_conflict"], 9),
        "missed_ev_due_to_capital_unavailable": round_or_none(missed["missed_ev_due_to_capital"], 9),
        "conflict_rate": round_or_none(conflict_rate, 9),
        "capital_utilization": round_or_none(capital_utilization, 9),
        "best_owner_mode": best_owner_mode,
        "worst_blocker": worst_blocker,
        "route_dispatch": dict(route_dispatch),
        "would_fire_by_owner": dict(owner_fire_counts),
        "would_fire_by_shard": {str(key): value for key, value in sorted(shard_fire_counts.items())},
        "suppression_metrics": counts,
        "blocker_counts": dict(blocker_counts),
        "top_scheduled_candidates": scheduled_rows[:20],
    }


inventory_model_results = {}
for model_name, model in models.items():
    shard_results = [simulate(model, shard_count) for shard_count in shard_counts]
    inventory_model_results[model_name] = {
        "shard_comparison": shard_results,
        "best_shard_result": max(shard_results, key=lambda item: (item["would_fire_count"], safe_float(item["captured_ev_sol_equivalent"]))),
        "input_mints_added": sorted(model.extra_input_mints),
        "synthetic_balances_raw": model.extra_balances_raw,
    }


def actual_gate_for_owner(row, model):
    return blocker_reason(row, model) == "none"


actual_model = models["actual_inventory_result"]
post_shock_autopsy = {}
for owner in POST_SHOCK_OWNER_MODES:
    scoped_rows = [row for row in unique_rows if owner_mode(row) == owner]
    blocker_counter = Counter(blocker_reason(row, actual_model) for row in scoped_rows)
    pair_counter = Counter((text_value(row.get("pool")), text_value(row.get("mint_in")), text_value(row.get("mint_out"))) for row in scoped_rows)
    post_shock_autopsy[owner] = {
        "rows": len(scoped_rows),
        "positive_immediate": sum(1 for row in scoped_rows if safe_int(row.get("net_after_fee_lamports")) > 0),
        "positive_later_horizon": sum(1 for row in scoped_rows if safe_int(best_horizon_net_after_fee_lamports(row)) > 0),
        "best_horizon": Counter(best_horizon_name(row) for row in scoped_rows).most_common(1)[0][0] if scoped_rows else "none",
        "would_fire": sum(1 for row in scoped_rows if actual_gate_for_owner(row, actual_model)),
        "saved_loss": round_or_none(
            sum(sol_equivalent_for_raw(safe_int(row.get("retained_ev_lamports")), text_value(row.get("mint_in"))) or 0.0 for row in scoped_rows),
            9,
        ),
        "top_pool_direction": " -> ".join(value for value in pair_counter.most_common(1)[0][0] if value) if pair_counter else "none",
        "main_blocker": blocker_counter.most_common(1)[0][0] if blocker_counter else "none",
        "immediate_ev_sol_equivalent": round_or_none(
            sum(sol_equivalent_for_raw(safe_int(row.get("net_after_fee_lamports")), text_value(row.get("mint_in"))) or 0.0 for row in scoped_rows),
            9,
        ),
        "later_horizon_ev_sol_equivalent": round_or_none(
            sum(sol_equivalent_for_raw(safe_int(best_horizon_net_after_fee_lamports(row)), text_value(row.get("mint_in"))) or 0.0 for row in scoped_rows),
            9,
        ),
        "saved_loss_ev_sol_equivalent": round_or_none(
            sum(sol_equivalent_for_raw(safe_int(row.get("retained_ev_lamports")), text_value(row.get("mint_in"))) or 0.0 for row in scoped_rows),
            9,
        ),
    }

directarb_seen = (run62_payload.get("directarb_autopsy") or {}).get("total_rows", len(directarb_rows))
directarb_unique = (run62_payload.get("directarb_autopsy") or {}).get("unique_opportunity_count", len({text_value(row.get("candidate_id")) for row in directarb_rows}))
directarb_quote_ok = (run62_payload.get("directarb_autopsy") or {}).get("quote_valid_count", sum(1 for row in directarb_rows if bool_field(row, "quote_ready_or_classified", "would_admit")))
directarb_route_usable = (run62_payload.get("directarb_autopsy") or {}).get("route_usable_count", sum(1 for row in directarb_rows if effective_route_family(row) in EXECUTABLE_ROUTE_FAMILIES))
directarb_inventory_ready = (run62_payload.get("directarb_autopsy") or {}).get("inventory_ready_count", sum(1 for row in directarb_rows if actual_inventory_gate(row)))
directarb_account_ready = (run62_payload.get("directarb_autopsy") or {}).get("ata_ready_count", sum(1 for row in directarb_rows if bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")))
directarb_template_ready = (run62_payload.get("directarb_autopsy") or {}).get("template_ready_count", sum(1 for row in directarb_rows if bool_field(row, "template_ready", "would_execute_current_template")))
directarb_unit_positive = (run62_payload.get("directarb_autopsy") or {}).get("unit_normalized_positive_ev_count", sum(1 for row in directarb_rows if safe_int(row.get("net_after_fee_lamports")) > 0))
directarb_would_fire = (run62_payload.get("directarb_autopsy") or {}).get("would_fire_count", sum(1 for row in directarb_rows if actual_gate_for_owner(row, actual_model)))
directarb_blocker_top = run62_summary.get("directarb_main_blocker") or ((run62_payload.get("directarb_autopsy") or {}).get("blocker_counts") or {})
if isinstance(directarb_blocker_top, dict):
    directarb_blocker_top = Counter(directarb_blocker_top).most_common(1)[0][0] if directarb_blocker_top else "none"
directarb_optimal_size_bucket = (run62_payload.get("directarb_autopsy") or {}).get("optimal_size_bucket_by_count") or "none"

directarb_ev_by_size_bucket = defaultdict(lambda: {"count": 0, "net_after_fee_sol_equivalent": 0.0})
for row in directarb_rows:
    bucket = size_bucket_label(row)
    directarb_ev_by_size_bucket[bucket]["count"] += 1
    directarb_ev_by_size_bucket[bucket]["net_after_fee_sol_equivalent"] += sol_equivalent_for_raw(
        safe_int(row.get("net_after_fee_lamports")), text_value(row.get("mint_in"))
    ) or 0.0

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_json": str(run61_json_path),
        "run61_summary": str(run61_summary_path),
        "run39_candidate_autopsy": str(candidate_autopsy_path),
        "run39_scorecard": str(scorecard_path),
        "run39_owner_mode_scorecard": str(owner_mode_path),
        "run54_summary": str(run54_summary_path),
        "run62_json": str(run62_json_path),
        "run62_summary": str(run62_summary_path),
        "run64e_json": str(run64e_json_path),
        "signer_token_inventory_path": str(inventory_path),
        "active_config_path": str(active_config_path),
        "rpc_url": rpc_url,
        "window_seconds": window_seconds,
        "shard_counts": shard_counts,
    },
    "run_status": {
        "run61_rc": run61_rc,
        "run61_status": run61_summary.get("status") or run61_payload.get("status"),
        "run54_status": run54_summary.get("status"),
        "run62_rc": run62_rc,
        "run62_status": run62_summary.get("status") or run62_payload.get("status"),
        "run64e_rc": run64e_rc,
        "run64e_status": (run64e_payload.get("status") if isinstance(run64e_payload, dict) else None),
    },
        "route_policy": {
            "executable_route_families": sorted(EXECUTABLE_ROUTE_FAMILIES),
            "jito_bundle_executable_count": 0,
            "harmonic_bundle_executable_count": 0,
            "tpu_quic_public_candidate_count": route_counter.get("tpu_quic_public", 0),
            "public_rpc_scout_candidate_count": route_counter.get("public_rpc_scout", 0),
            "blocked_private_route_count": blocked_private_route_count,
            "blocked_route_unavailable_count": blocked_route_unavailable_count,
        },
        "conversion_policy": {
            "no_send_fixable_output_account_count": sum(1 for row in unique_rows if output_account_fixable(row)),
            "no_send_fixable_template_count": sum(1 for row in unique_rows if template_fixable(row)),
            "policy": "no_send_ata_template_fixups_allowed",
        },
        "continuous_metrics": {
            "continuous_candidates_seen": candidate_count,
            "candidate_deduped_count": deduped_count,
            "owner_selected_counts": {owner: owner_selected_counts.get(owner, 0) for owner in ALL_OWNER_MODES},
        },
    "directarb_autopsy": {
        "directarb_seen": directarb_seen,
        "directarb_unique": directarb_unique,
        "directarb_quote_ok": directarb_quote_ok,
        "directarb_route_usable": directarb_route_usable,
        "directarb_inventory_ready": directarb_inventory_ready,
        "directarb_account_ready": directarb_account_ready,
        "directarb_template_ready": directarb_template_ready,
        "directarb_unit_positive": directarb_unit_positive,
        "directarb_would_fire": directarb_would_fire,
        "directarb_blocker_top": directarb_blocker_top,
        "directarb_optimal_size_bucket": directarb_optimal_size_bucket,
        "directarb_ev_by_size_bucket": {
            bucket: {
                "count": values["count"],
                "net_after_fee_sol_equivalent": round_or_none(values["net_after_fee_sol_equivalent"], 9),
            }
            for bucket, values in sorted(directarb_ev_by_size_bucket.items())
        },
    },
    "post_shock_autopsy": post_shock_autopsy,
    "inventory_models": inventory_model_results,
    "coinini_classification": run64e_payload.get("coinini_override"),
}

summary_lines = [
    f"status={payload['status']}",
    f"run61_status={payload['run_status']['run61_status']}",
    f"run62_status={payload['run_status']['run62_status']}",
    f"run64e_status={payload['run_status']['run64e_status']}",
    f"continuous_candidates_seen={candidate_count}",
    f"candidate_deduped_count={deduped_count}",
    f"owner_selected_directarb={owner_selected_counts.get('directarb', 0)}",
    f"owner_selected_backrun={owner_selected_counts.get('backrun', 0)}",
    f"owner_selected_continuation={owner_selected_counts.get('continuation', 0)}",
    f"owner_selected_housekeeping={owner_selected_counts.get('housekeeping', 0)}",
    f"owner_selected_rebalancing={owner_selected_counts.get('rebalancing', 0)}",
    f"owner_selected_flatten={owner_selected_counts.get('flatten', 0)}",
    f"owner_selected_drop={owner_selected_counts.get('drop', 0)}",
    f"jito_bundle_executable_count=0",
        f"harmonic_bundle_executable_count=0",
        f"tpu_quic_public_candidate_count={route_counter.get('tpu_quic_public', 0)}",
        f"public_rpc_scout_candidate_count={route_counter.get('public_rpc_scout', 0)}",
        f"blocked_private_route_count={blocked_private_route_count}",
        f"blocked_route_unavailable_count={blocked_route_unavailable_count}",
        f"conversion_policy={payload['conversion_policy']['policy']}",
        f"no_send_fixable_output_account_count={payload['conversion_policy']['no_send_fixable_output_account_count']}",
        f"no_send_fixable_template_count={payload['conversion_policy']['no_send_fixable_template_count']}",
    ]

for model_name, data in inventory_model_results.items():
    label = model_name.replace("_result", "").replace("_counterfactual", "")
    for shard_result in data["shard_comparison"]:
        shard = shard_result["shards"]
        summary_lines.extend(
            [
                f"{label}_shards_{shard}_would_fire_count={shard_result['would_fire_count']}",
                f"{label}_shards_{shard}_directarb_fire_count={shard_result['directarb_fire_count']}",
                f"{label}_shards_{shard}_post_shock_fire_count={shard_result['post_shock_fire_count']}",
                f"{label}_shards_{shard}_captured_ev_sol_equivalent={shard_result['captured_ev_sol_equivalent']}",
                f"{label}_shards_{shard}_missed_ev_sol_equivalent={shard_result['missed_ev_sol_equivalent']}",
                f"{label}_shards_{shard}_best_owner_mode={shard_result['best_owner_mode']}",
                f"{label}_shards_{shard}_worst_blocker={shard_result['worst_blocker']}",
            ]
        )

best_actual = inventory_model_results["actual_inventory_result"]["best_shard_result"]
summary_lines.extend(
    [
        f"would_fire_total={best_actual['would_fire_count']}",
        f"would_fire_directarb={best_actual['would_fire_by_owner'].get('directarb', 0)}",
        f"would_fire_backrun={best_actual['would_fire_by_owner'].get('backrun', 0)}",
        f"would_fire_continuation={best_actual['would_fire_by_owner'].get('continuation', 0)}",
        f"would_fire_housekeeping={best_actual['would_fire_by_owner'].get('housekeeping', 0)}",
        f"would_fire_rebalancing={best_actual['would_fire_by_owner'].get('rebalancing', 0)}",
        f"would_enqueue_tpu_count={best_actual['suppression_metrics']['would_enqueue_tpu_count']}",
        f"would_public_rpc_count={best_actual['suppression_metrics']['would_public_rpc_count']}",
        f"would_wire_send_count=0",
        f"suppressed_duplicate_count={best_actual['suppression_metrics']['suppressed_duplicate_count']}",
        f"suppressed_conflict_domain_count={best_actual['suppression_metrics']['suppressed_conflict_domain_count']}",
        f"suppressed_no_free_shard_count={best_actual['suppression_metrics']['suppressed_no_free_shard_count']}",
        f"suppressed_inventory_unavailable_count={best_actual['suppression_metrics']['suppressed_inventory_unavailable_count']}",
        f"suppressed_account_lock_count={best_actual['suppression_metrics']['suppressed_account_lock_count']}",
        f"suppressed_capital_unavailable_count={best_actual['suppression_metrics']['suppressed_capital_unavailable_count']}",
        f"suppressed_route_unavailable_count={best_actual['suppression_metrics']['suppressed_route_unavailable_count']}",
        f"missed_ev_due_to_serial_execution={best_actual['missed_ev_due_to_serial_execution']}",
        f"missed_ev_due_to_inventory={best_actual['missed_ev_due_to_inventory']}",
        f"missed_ev_due_to_route={best_actual['missed_ev_due_to_route']}",
        f"missed_ev_due_to_account_conflict={best_actual['missed_ev_due_to_account_conflict']}",
        f"directarb_seen={directarb_seen}",
        f"directarb_unique={directarb_unique}",
        f"directarb_quote_ok={directarb_quote_ok}",
        f"directarb_route_usable={directarb_route_usable}",
        f"directarb_inventory_ready={directarb_inventory_ready}",
        f"directarb_account_ready={directarb_account_ready}",
        f"directarb_template_ready={directarb_template_ready}",
        f"directarb_unit_positive={directarb_unit_positive}",
        f"directarb_would_fire={directarb_would_fire}",
        f"directarb_blocker_top={directarb_blocker_top}",
        f"directarb_optimal_size_bucket={directarb_optimal_size_bucket}",
    ]
)

coinini = run64e_payload.get("coinini_override") if isinstance(run64e_payload, dict) else None
if isinstance(coinini, dict):
    summary_lines.extend(
        [
            f"coinini_inventory_policy_classification={text_value(coinini.get('inventory_policy_classification'))}",
            f"coinini_expansion_verdict={text_value(coinini.get('expansion_verdict'))}",
        ]
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run65 shard simulation written to $SUMMARY_FILE"
tail -n 160 "$SUMMARY_FILE" || true
finish

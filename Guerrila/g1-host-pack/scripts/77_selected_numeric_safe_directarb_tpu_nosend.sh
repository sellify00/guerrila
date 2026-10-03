#!/usr/bin/env bash
# 77_selected_numeric_safe_directarb_tpu_nosend.sh - replay a selected numeric-safe DirectArb class through TPU no-send

source "$(dirname "$0")/lib.sh"
script_header "77 - Selected Numeric-Safe DirectArb TPU No-Send"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
XDP_DIR="$PACK_DIR/xdp"
XDP_OBJ="$XDP_DIR/shadow_rx_meta.bpf.o"
XDP_QUEUE_COUNT=1
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_NIC_QUEUE="${SHADOW_NIC_QUEUE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL-}"
CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL-}"
CALLER_SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL-}"
CALLER_SHADOW_RUN40_RPC_URL="${SHADOW_RUN40_RPC_URL-}"

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
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

# Run77 is the canonical selected replay proof. Keep the safe profile's eno1
# ownership defaults, but do not inherit its xdpgeneric/copy proof posture.
export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN77_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN77_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
export SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-false}"
REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

is_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

require_env SHADOW_NIC_IFACE

STEP="${SHADOW_RUN77_STEP:-77_selected_numeric_safe_directarb_tpu_nosend}"
INNER_STEP="${SHADOW_RUN77_INNER_STEP:-${STEP}_run40}"
SCORECARD_PATH="${SHADOW_RUN77_SCORECARD_PATH:-$RESULTS_ROOT/76_numeric_safe_directarb_selector/run76_selected_scorecard.json}"
CONFIG_SOURCE="${SHADOW_RUN77_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
WINDOW_SECONDS="${SHADOW_RUN77_WINDOW_SECONDS:-600}"
MAX_INPUT_SOL="${SHADOW_RUN77_MAX_INPUT_SOL:-1.0}"
MAX_POSITION_SOL_PER_MINT="${SHADOW_RUN77_MAX_POSITION_SOL_PER_MINT:-1.0}"
MAX_DAILY_LOSS_SOL="${SHADOW_RUN77_MAX_DAILY_LOSS_SOL:-0.5}"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULTS_ROOT/${STEP}.pre_attach_local_agave_preflight.txt"

if [[ ! -f "$SCORECARD_PATH" ]]; then
    fail "Missing Run76 selected scorecard: $SCORECARD_PATH"
    finish
    exit 1
fi

config_afxdp_bind_mode() {
    local path="$1"
    awk -F: '
        /^[[:space:]]*afxdp_bind_mode[[:space:]]*:/ {
            value = $2
            gsub(/[[:space:]"\047]/, "", value)
            print value
            exit
        }
    ' "$path"
}

require_canonical_run77_posture() {
    local config_path="$1"
    local bind_mode
    bind_mode="$(config_afxdp_bind_mode "$config_path")"

    if [[ "$SHADOW_XDP_ATTACH_MODE" != "native" ]]; then
        fail "Run77 requires native AF_XDP zero-copy proof posture; SHADOW_XDP_ATTACH_MODE=$SHADOW_XDP_ATTACH_MODE"
        finish
        exit 1
    fi
    if [[ "$bind_mode" != "zero_copy" ]]; then
        fail "Run77 requires afxdp_bind_mode=zero_copy in $config_path; got ${bind_mode:-missing}"
        finish
        exit 1
    fi
}

attached_xdp_mode() {
    local first_line
    first_line="$(ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | head -n 1 || true)"
    if [[ "$first_line" == *"xdpgeneric"* ]]; then
        echo "xdpgeneric"
    elif [[ "$first_line" == *" xdp "* ]]; then
        echo "native"
    else
        echo "none"
    fi
}

attached_xdp_prog_name() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | sed -n 's/.*prog\/xdp id [0-9]\+ name \([^ ]\+\).*/\1/p' | head -n 1
}

apply_safe_afxdp_posture() {
    local feature
    info "Applying R1-AFXDP-ENO1-SAFE host posture on $SHADOW_NIC_IFACE"
    ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
        warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s); continuing with current layout"
    for feature in gro lro tso gso rx-gro-hw tx-gso-partial; do
        ethtool -K "$SHADOW_NIC_IFACE" "$feature" off >/dev/null 2>&1 || \
            warn "Could not disable $feature on $SHADOW_NIC_IFACE"
    done
    ethtool -K "$SHADOW_NIC_IFACE" ntuple off >/dev/null 2>&1 || \
        warn "Could not disable ntuple filters on $SHADOW_NIC_IFACE"
}

ensure_afxdp_xdp_path() {
    assert_cmd_exists clang
    assert_cmd_exists llvm-strip
    assert_cmd_exists make
    assert_cmd_exists bpftool
    assert_cmd_exists ethtool

    [[ -d /sys/fs/bpf ]] || mkdir -p /sys/fs/bpf
    if ! mountpoint -q /sys/fs/bpf; then
        info "Mounting bpffs at /sys/fs/bpf"
        mount -t bpf bpf /sys/fs/bpf
    fi

    if [[ ! -f "$XDP_OBJ" ]]; then
        info "Building XDP program at $XDP_OBJ"
        (
            cd "$XDP_DIR"
            make
        )
    fi

    if is_true "$SAFE_MODE"; then
        apply_safe_afxdp_posture
    else
        ethtool -L "$SHADOW_NIC_IFACE" combined "$XDP_QUEUE_COUNT" >/dev/null 2>&1 || \
            warn "Could not set $SHADOW_NIC_IFACE to $XDP_QUEUE_COUNT combined queue(s); continuing with current layout"
    fi

    local attached_mode attached_prog reload_xdp
    attached_mode="$(attached_xdp_mode)"
    attached_prog="$(attached_xdp_prog_name)"
    reload_xdp="true"
    if is_true "$REUSE_ATTACHED_XDP" && [[ "$attached_mode" == "$SHADOW_XDP_ATTACH_MODE" ]] && [[ "$attached_prog" == shadow_rx_meta_* ]]; then
        info "Reusing existing XDP program on $SHADOW_NIC_IFACE (mode=$attached_mode prog=$attached_prog)"
        reload_xdp="false"
    fi

    if [[ "$reload_xdp" == "true" ]]; then
        info "Loading XDP program on $SHADOW_NIC_IFACE (mode=$SHADOW_XDP_ATTACH_MODE)"
        ip link set dev "$SHADOW_NIC_IFACE" xdp off 2>/dev/null || true
        ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric off 2>/dev/null || true
        case "$SHADOW_XDP_ATTACH_MODE" in
            native)
                ip link set dev "$SHADOW_NIC_IFACE" xdp obj "$XDP_OBJ" sec xdp
                ;;
            xdpgeneric)
                ip link set dev "$SHADOW_NIC_IFACE" xdpgeneric obj "$XDP_OBJ" sec xdp
                ;;
            *)
                fail "Unsupported SHADOW_XDP_ATTACH_MODE=$SHADOW_XDP_ATTACH_MODE"
                exit 1
                ;;
        esac
    fi

    attached_mode="$(attached_xdp_mode)"
    if [[ "$attached_mode" != "$SHADOW_XDP_ATTACH_MODE" ]]; then
        fail "XDP attach mode mismatch on $SHADOW_NIC_IFACE: expected $SHADOW_XDP_ATTACH_MODE, observed $attached_mode"
        exit 1
    fi

    info "Pinning XSKMAP at $XSKMAP_PIN"
    rm -f "$XSKMAP_PIN" 2>/dev/null || true
    local prog_id map_ids map_id
    prog_id="$(ip -details link show dev "$SHADOW_NIC_IFACE" | sed -n 's/.*prog\/xdp id \([0-9]\+\).*/\1/p' | head -n 1)"
    if [[ -z "$prog_id" ]]; then
        fail "Could not resolve attached XDP program id on $SHADOW_NIC_IFACE"
        exit 1
    fi
    map_ids="$(bpftool prog show id "$prog_id" | sed -n 's/.*map_ids \(.*\)$/\1/p' | tr ',' ' ')"
    for candidate in $map_ids; do
        if bpftool map show id "$candidate" 2>/dev/null | grep -q 'name xsks_map'; then
            map_id="$candidate"
            break
        fi
    done
    if [[ -z "${map_id:-}" ]]; then
        fail "Could not resolve xsks_map id from attached XDP program $prog_id"
        exit 1
    fi
    bpftool map pin id "$map_id" "$XSKMAP_PIN"

    if [[ ! -e "$XSKMAP_PIN" ]]; then
        fail "Pinned XSKMAP missing at $XSKMAP_PIN"
        exit 1
    fi
    export SHADOW_XSKMAP_PIN="$XSKMAP_PIN"
    pass "XDP/XSKMAP ready on $SHADOW_NIC_IFACE"
}

NORMALIZED_CONFIG="$RESULTS_ROOT/${STEP}.run40.config.yaml"
mkdir -p "$RESULTS_ROOT"
if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run77 config source: $CONFIG_SOURCE"
    finish
    exit 1
fi
tr -d '\r' < "$CONFIG_SOURCE" > "$NORMALIZED_CONFIG"
export SHADOW_RUN40_CONFIG="$NORMALIZED_CONFIG"

require_canonical_run77_posture "$NORMALIZED_CONFIG"

if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
    finish
    exit 1
fi
export SHADOW_RUN40_TRUST_PREATTACH_AGAVE_PREFLIGHT=true
export SHADOW_RUN40_PREATTACH_AGAVE_PREFLIGHT_FILE="$PREATTACH_AGAVE_PREFLIGHT_FILE"

ensure_afxdp_xdp_path

export SHADOW_RUN52A_STEP="$STEP"
export SHADOW_RUN52A_INNER_STEP="$INNER_STEP"
export SHADOW_RUN52_TARGET_MODE="scorecard_selected_replay_tpu_nosend"
export SHADOW_RUN52A_SCORECARD_PATH="$SCORECARD_PATH"
export SHADOW_RUN52A_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN52A_MAX_INPUT_SOL="$MAX_INPUT_SOL"
export SHADOW_RUN52A_MAX_POSITION_SOL_PER_MINT="$MAX_POSITION_SOL_PER_MINT"
export SHADOW_RUN52A_MAX_DAILY_LOSS_SOL="$MAX_DAILY_LOSS_SOL"

exec "$PACK_DIR/scripts/52a_tpu_route_conversion_9i1gx.sh"

#!/usr/bin/env bash
# epoch_canonical_monetization.sh - canonical fresh monetization epoch wrapper

source "$(dirname "$0")/lib.sh"
script_header "Epoch - Canonical Monetization Window"

assert_root

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-/home/ubuntu/shadow-host-audit/dpdk-runs}"
EPOCH_NAME_RAW="${SHADOW_EPOCH_NAME:-epoch1}"
WINDOW_SECONDS="${SHADOW_EPOCH_WINDOW_SECONDS:-900}"
SHARD_COUNTS="${SHADOW_EPOCH_SHARD_COUNTS:-1,2,4,8,16}"
EXECUTABLE_ROUTE_FAMILIES="${SHADOW_EPOCH_EXECUTABLE_ROUTE_FAMILIES:-tpu_quic_public,public_rpc_scout}"

EPOCH_SLUG="$(printf '%s' "$EPOCH_NAME_RAW" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9._-' '_')"
EPOCH_SLUG="${EPOCH_SLUG##_}"
EPOCH_SLUG="${EPOCH_SLUG%%_}"
if [[ -z "$EPOCH_SLUG" ]]; then
    fail "Epoch name resolved to an empty slug"
    finish
    exit 2
fi

STEP="${EPOCH_SLUG}_canonical_monetization"

exec env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN65_STEP="$STEP" \
    SHADOW_RUN65_STATUS_PREFIX="$STEP" \
    SHADOW_RUN65_REPORT_BASENAME="$STEP" \
    SHADOW_RUN65_RUN61_STEP="$STEP" \
    SHADOW_RUN65_RUN62_STEP="${STEP}_run62" \
    SHADOW_RUN65_RUN64E_STEP="${STEP}_run64e" \
    SHADOW_RUN65_WINDOW_SECONDS="$WINDOW_SECONDS" \
    SHADOW_RUN65_SHARD_COUNTS="$SHARD_COUNTS" \
    SHADOW_RUN65_EXECUTABLE_ROUTE_FAMILIES="$EXECUTABLE_ROUTE_FAMILIES" \
    SHADOW_RUN65_ENABLE_PUBLIC_SELECTOR_RENDER=false \
    "$(dirname "$0")/65_full_potential_continuous_shard_simulation.sh"

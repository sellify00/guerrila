#!/usr/bin/env bash
# 61_post_shock_conversion.sh - continuous shadow-live no-send simulation

source "$(dirname "$0")/lib.sh"
script_header "61 - Continuous Shadow-Live Simulation"

assert_root

STEP="${SHADOW_RUN61_STEP:-61_continuous_shadow_live_simulation}"
REPORT_BASENAME="${SHADOW_RUN61_REPORT_BASENAME:-run61_continuous_shadow_live_simulation}"
STATUS_PREFIX="${SHADOW_RUN61_STATUS_PREFIX:-run61_continuous_shadow_live_simulation}"
WINDOW_SECONDS="${SHADOW_RUN61_WINDOW_SECONDS:-900}"
SIZE_BUCKETS_SOL="${SHADOW_RUN61_SIZE_BUCKETS_SOL:-0.005,0.01,0.025,0.05,0.1,0.25,0.5,1.0,2.0}"
SIZE_BUCKETS_USD="${SHADOW_RUN61_SIZE_BUCKETS_USD:-5,10,25,50,100,250,500,1000,2000}"

exec env \
    SHADOW_RUN60_STEP="$STEP" \
    SHADOW_RUN60_REPORT_BASENAME="$REPORT_BASENAME" \
    SHADOW_RUN60_STATUS_PREFIX="$STATUS_PREFIX" \
    SHADOW_RUN60_WINDOW_SECONDS="$WINDOW_SECONDS" \
    SHADOW_RUN60_SIZE_BUCKETS_SOL="$SIZE_BUCKETS_SOL" \
    SHADOW_RUN60_SIZE_BUCKETS_USD="$SIZE_BUCKETS_USD" \
    "$(dirname "$0")/60_post_shock_capital_efficiency.sh"

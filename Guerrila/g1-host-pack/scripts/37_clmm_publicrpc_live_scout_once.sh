#!/usr/bin/env bash
# 37_clmm_publicrpc_live_scout_once.sh - one-shot R1-PUBLIC-A live scout wrapper

set -euo pipefail

if [[ -z "${SHADOW_RUN37_PUBLIC_RPC_ENDPOINT:-}" ]]; then
    echo "SHADOW_RUN37_PUBLIC_RPC_ENDPOINT is required" >&2
    exit 1
fi

export SHADOW_RUN29C_STEP="${SHADOW_RUN37_STEP:-37_clmm_publicrpc_live_scout_once}"
export SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT="$SHADOW_RUN37_PUBLIC_RPC_ENDPOINT"
export SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT=false
export SHADOW_RUN29C_ONE_SHOT_MODE=true
export SHADOW_RUN29C_VALIDATE_PUBLIC_SCOUT_PROFILE=true
export SHADOW_RUN29C_RESULT_GRACE_SECONDS="${SHADOW_RUN37_RESULT_GRACE_SECONDS:-45}"

exec "$(dirname "$0")/29_clmm_publicrpc_blackhole_proof.sh" "$@"

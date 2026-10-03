#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

G1_LOCAL_ENV="/etc/shadow-engine/env.g1.local.sh"
if [[ -f "$G1_LOCAL_ENV" && -r "$G1_LOCAL_ENV" ]]; then
    # shellcheck disable=SC1090
    source "$G1_LOCAL_ENV"
fi

ensure_base_dirs
require_file "$SH1_IDENTITY_PATH"

VALIDATOR_BIN="$(agave_bin agave-validator || true)"
if [[ -z "$VALIDATOR_BIN" ]]; then
    fail "agave-validator not found. Run 10_build_agave.sh first."
    exit 1
fi

LOG_FILE="${SH1_LOG_DIR}/agave-${SH1_CLUSTER}-no-geyser.log"
build_validator_args VALIDATOR_ARGS "$LOG_FILE"

if [[ -n "$SH1_SOLANA_METRICS_CONFIG" ]]; then
    export SOLANA_METRICS_CONFIG="$SH1_SOLANA_METRICS_CONFIG"
fi

VALIDATOR_CPU_AFFINITY="${SH1_VALIDATOR_CPU_AFFINITY:-${SHADOW_LOCAL_AGAVE_CPU_AFFINITY:-}}"

exec > >(tee -a "$LOG_FILE") 2>&1
info "Starting non-voting ${SH1_CLUSTER} validator without Yellowstone"
info "Log file: $LOG_FILE"
if [[ -n "$VALIDATOR_CPU_AFFINITY" ]]; then
    require_cmd taskset
    info "Applying validator CPU affinity at startup: $VALIDATOR_CPU_AFFINITY"
    exec taskset -c "$VALIDATOR_CPU_AFFINITY" "$VALIDATOR_BIN" "${VALIDATOR_ARGS[@]}"
fi

exec "$VALIDATOR_BIN" "${VALIDATOR_ARGS[@]}"

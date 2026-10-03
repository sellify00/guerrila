#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

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

exec > >(tee -a "$LOG_FILE") 2>&1
info "Starting non-voting ${SH1_CLUSTER} validator without Yellowstone"
info "Log file: $LOG_FILE"
exec "$VALIDATOR_BIN" "${VALIDATOR_ARGS[@]}"

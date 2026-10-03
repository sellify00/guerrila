#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
fi

require_cmd cargo
require_cmd timeout

MANIFEST_PATH="$(yellowstone_client_manifest)"
require_file "$MANIFEST_PATH"
require_file "$SH1_YELLOWSTONE_CONFIG_PATH"

ENDPOINT="http://${SH1_YELLOWSTONE_GRPC_ADDRESS}"
COMMON_ARGS=(cargo run --release --manifest-path "$MANIFEST_PATH" --bin client -- --endpoint "$ENDPOINT")
if [[ -n "$SH1_YELLOWSTONE_X_TOKEN" ]]; then
    COMMON_ARGS+=(--x-token "$SH1_YELLOWSTONE_X_TOKEN")
fi

info "Checking Yellowstone health endpoint"
"${COMMON_ARGS[@]}" health-check
"${COMMON_ARGS[@]}" get-version

run_bounded_subscribe() {
    local label="$1"
    shift

    info "$label"
    set +e
    timeout 15s "${COMMON_ARGS[@]}" "$@"
    local status=$?
    set -e

    if [[ "$status" != "0" && "$status" != "124" ]]; then
        fail "Yellowstone subscribe failed with status $status"
    fi
}

run_bounded_subscribe "Streaming slots for 15 seconds" subscribe --slots
run_bounded_subscribe "Streaming transactions for 15 seconds" subscribe --transactions --transactions-vote false

pass "Yellowstone client smoke run completed"

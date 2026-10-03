#!/usr/bin/env bash
# 22_launch16_late_follow_entry_confirmation_autopsy.sh — split late-follow send truth

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch16 — Late-Follow Entry Confirmation Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

[[ -f "$PACK_DIR/env.g1.sh" ]] && source_crlf_safe "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RUN_ROOT="${1:-${SHADOW_LAUNCH16_RUN_ROOT:-}}"
RPC_URL="${SHADOW_LAUNCH16_RPC_URL:-${SHADOW_LAUNCH5_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-}}}}}"
export SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"

ARGS=(
    "$PACK_DIR/scripts/launch_entry_confirmation_autopsy.py"
    --rpc-url "$RPC_URL"
)
if [[ -n "$RUN_ROOT" ]]; then
    ARGS+=(--run-root "$RUN_ROOT")
fi

python3 "${ARGS[@]}"
finish

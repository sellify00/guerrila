#!/usr/bin/env bash
# 21_runls13_laserstream_preprocessed_target_freshness.sh — LaserStream preprocessed target freshness proof

source "$(dirname "$0")/lib.sh"
script_header "21 — RunLS13 LaserStream Preprocessed Target Freshness"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"
STEP="21_runls13_laserstream_preprocessed_target_freshness"
WINDOW_SECONDS="${1:-${SHADOW_RUNLS13_WINDOW_SECONDS:-30}}"
WINDOW_RESULT_DIR="${SHADOW_RUNLS13_RESULT_DIR:-$PACK_DIR/results/$STEP/window1}"
INNER_STEP_DIR="${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}"
SELECTED_BUNDLE_JSON="${SHADOW_RUNLS13_SELECTED_BUNDLE_JSON:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
PROVIDER_JSON="${SHADOW_RUNLS13_PROVIDER_JSON:-$INNER_STEP_DIR/runauth1_selected_provider.json}"
BINARY="${SHADOW_RUNLS13_BINARY:-$PACK_DIR/artifacts/release-guerrila/runls13-preprocessed-probe}"

mkdir -p "$WINDOW_RESULT_DIR"

is_placeholder_value() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == *CHANGE_ME* || "$value" == *'${'* ]]
}

append_api_key_if_needed() {
    local url="$1"
    local api_key="$2"
    if is_placeholder_value "$url" || is_placeholder_value "$api_key"; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"api-key="* || "$url" == *"apikey="* ]]; then
        printf '%s\n' "$url"
        return 0
    fi
    if [[ "$url" == *"?"* ]]; then
        printf '%s&api-key=%s\n' "$url" "$api_key"
    else
        printf '%s?api-key=%s\n' "$url" "$api_key"
    fi
}

LASERSTREAM_ENDPOINT="${SHADOW_LASERSTREAM_PREPROCESSED_URL:-${HELIUS_LASERSTREAM_PREPROCESSED_URL:-${HELIUS_PREPROCESSED_LASERSTREAM_URL:-${HELIUS_LASERSTREAM_URL:-${SHADOW_GEYSER_ENDPOINT:-}}}}}"
LASERSTREAM_API_KEY="${SHADOW_LASERSTREAM_PREPROCESSED_API_KEY:-${HELIUS_LASERSTREAM_PREPROCESSED_API_KEY:-${HELIUS_PREPROCESSED_LASERSTREAM_API_KEY:-${HELIUS_LASERSTREAM_API_KEY:-${SHADOW_GEYSER_TOKEN:-}}}}}"
RPC_URL="${SHADOW_RUNLS13_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-${HELIUS_GATEKEEPER_URL:-${HELIUS_GATEKEEPER:-}}}}}}}"
HELIUS_BUNDLE_API_KEY="${SHADOW_HELIUS_BUNDLE_API_KEY:-${HELIUS_API_KEY:-${HELIUS_LASERSTREAM_API_KEY:-}}}"
RPC_URL="$(append_api_key_if_needed "$RPC_URL" "$HELIUS_BUNDLE_API_KEY")"

if [[ ! -x "$BINARY" ]]; then
    if [[ -x "$REPO_ROOT/target/release-guerrila/runls13-preprocessed-probe" ]]; then
        BINARY="$REPO_ROOT/target/release-guerrila/runls13-preprocessed-probe"
    elif [[ -x "$REPO_ROOT/target/debug/runls13-preprocessed-probe" ]]; then
        BINARY="$REPO_ROOT/target/debug/runls13-preprocessed-probe"
    else
        fail "RunLS13 binary missing: $BINARY"
    fi
fi

if is_placeholder_value "$LASERSTREAM_ENDPOINT"; then
    fail "LaserStream preprocessed endpoint missing"
fi
if is_placeholder_value "$LASERSTREAM_API_KEY"; then
    fail "LaserStream preprocessed API key missing"
fi
if is_placeholder_value "$RPC_URL"; then
    fail "RunLS13 RPC URL missing"
fi
if [[ -f "$SELECTED_BUNDLE_JSON" ]]; then
    pass "Using selected LS11 bundle context: $SELECTED_BUNDLE_JSON"
else
    warn "Selected LS11 bundle context not found; LS13 will run unfiltered: $SELECTED_BUNDLE_JSON"
fi
if [[ -f "$PROVIDER_JSON" ]]; then
    pass "Using RunAUTH1 selected provider context: $PROVIDER_JSON"
else
    warn "RunAUTH1 selected provider context not found: $PROVIDER_JSON"
fi

if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

ARGS=(
    --endpoint "$LASERSTREAM_ENDPOINT"
    --api-key "$LASERSTREAM_API_KEY"
    --rpc-url "$RPC_URL"
    --out-dir "$WINDOW_RESULT_DIR"
    --window-seconds "$WINDOW_SECONDS"
)

if [[ -f "$SELECTED_BUNDLE_JSON" ]]; then
    ARGS+=(--selected-bundle-json "$SELECTED_BUNDLE_JSON")
fi
if [[ -f "$PROVIDER_JSON" ]]; then
    ARGS+=(--provider-json "$PROVIDER_JSON")
fi
if [[ -n "${SHADOW_RUNLS13_ACCOUNT_INCLUDE:-}" ]]; then
    for account in ${SHADOW_RUNLS13_ACCOUNT_INCLUDE//,/ }; do
        [[ -n "$account" ]] && ARGS+=(--account-include "$account")
    done
fi
if value_is_true "${SHADOW_RUNLS13_DIRECT_PROBE:-true}"; then
    ARGS+=(--direct-probe)
fi

set +e
"$BINARY" "${ARGS[@]}"
rc=$?
set -e

SUMMARY_PATH="$WINDOW_RESULT_DIR/runls13_preprocessed_target_freshness_summary.json"
ROWS_PATH="$WINDOW_RESULT_DIR/runls13_preprocessed_target_freshness_rows.jsonl"

if [[ -f "$SUMMARY_PATH" ]]; then
    info "RunLS13 summary: $SUMMARY_PATH"
else
    fail "RunLS13 summary missing: $SUMMARY_PATH"
fi
if [[ -s "$ROWS_PATH" ]]; then
    info "RunLS13 rows: $ROWS_PATH"
else
    fail "RunLS13 rows missing or empty: $ROWS_PATH"
fi

if [[ $rc -eq 0 ]]; then
    pass "RunLS13 preprocessed target is fresh and bundle eligible"
else
    fail "RunLS13 blocked safely; see $SUMMARY_PATH"
fi

finish
exit "$rc"

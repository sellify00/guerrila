#!/usr/bin/env bash
# 22_launch0rt_realtime_birth_rate_probe.sh — duration-validated no-wire Launch real-time stream census

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0RT — Realtime Birth Rate Probe"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

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

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

REQUESTED_DURATION_SECONDS="${1:-${SHADOW_LAUNCH0RT_DURATION_SECONDS:-3600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_ROOT="${SHADOW_LAUNCH0RT_RUN_ROOT:-$RESULTS_ROOT/run_launch0rt_realtime_birth_rate_probe_${REQUESTED_DURATION_SECONDS}s_${RUN_STAMP}}"
STREAM_DIR="$RUN_ROOT/laserstream_preprocessed_realtime"
if [[ "${SHADOW_LAUNCH0RT_KEEP_STDOUT_JSONL:-false}" == "true" ]]; then
    STREAM_STDOUT="$RUN_ROOT/runls13_preprocessed_stream.stdout.jsonl"
else
    STREAM_STDOUT="/dev/null"
fi
STREAM_STDERR="$RUN_ROOT/runls13_preprocessed_stream.stderr.log"
SUMMARY_JSON="$RUN_ROOT/launch0rt_realtime_birth_rate_probe_summary.json"
SUMMARY_TXT="$RUN_ROOT/summary.txt"

LASERSTREAM_ENDPOINT="${SHADOW_LASERSTREAM_PREPROCESSED_URL:-${HELIUS_LASERSTREAM_PREPROCESSED_URL:-${HELIUS_PREPROCESSED_LASERSTREAM_URL:-${HELIUS_LASERSTREAM_URL:-${SHADOW_GEYSER_ENDPOINT:-}}}}}"
LASERSTREAM_API_KEY="${SHADOW_LASERSTREAM_PREPROCESSED_API_KEY:-${HELIUS_LASERSTREAM_PREPROCESSED_API_KEY:-${HELIUS_PREPROCESSED_LASERSTREAM_API_KEY:-${HELIUS_LASERSTREAM_API_KEY:-${SHADOW_GEYSER_TOKEN:-}}}}}"
RPC_URL="${SHADOW_RUNLS13_RPC_URL:-${SHADOW_HELIUS_BUNDLE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_RPC_URL:-${SHADOW_PUBLIC_RPC_URL:-${HELIUS_GATEKEEPER_URL:-${HELIUS_GATEKEEPER:-}}}}}}}"
HELIUS_BUNDLE_API_KEY="${SHADOW_HELIUS_BUNDLE_API_KEY:-${HELIUS_API_KEY:-${HELIUS_LASERSTREAM_API_KEY:-}}}"
RPC_URL="$(append_api_key_if_needed "$RPC_URL" "$HELIUS_BUNDLE_API_KEY")"
STREAM_BINARY="${SHADOW_RUNLS13_BINARY:-}"

if [[ -z "$STREAM_BINARY" || ! -x "$STREAM_BINARY" ]]; then
    if [[ -x "$REPO_ROOT/target/release-guerrila/runls13-preprocessed-probe" ]]; then
        STREAM_BINARY="$REPO_ROOT/target/release-guerrila/runls13-preprocessed-probe"
    elif [[ -x "$REPO_ROOT/target/debug/runls13-preprocessed-probe" ]]; then
        STREAM_BINARY="$REPO_ROOT/target/debug/runls13-preprocessed-probe"
    elif [[ -x "$PACK_DIR/artifacts/release-guerrila/runls13-preprocessed-probe" ]]; then
        STREAM_BINARY="$PACK_DIR/artifacts/release-guerrila/runls13-preprocessed-probe"
    else
        fail "RunLS13 binary missing"
    fi
fi

is_placeholder_value "$LASERSTREAM_ENDPOINT" && fail "LaserStream preprocessed endpoint missing"
is_placeholder_value "$LASERSTREAM_API_KEY" && fail "LaserStream preprocessed API key missing"
is_placeholder_value "$RPC_URL" && fail "RunLS13 RPC URL missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

mkdir -p "$STREAM_DIR"

ARGS=(
    --endpoint "$LASERSTREAM_ENDPOINT"
    --api-key "$LASERSTREAM_API_KEY"
    --rpc-url "$RPC_URL"
    --out-dir "$STREAM_DIR"
    --window-seconds "$REQUESTED_DURATION_SECONDS"
    --stream-jsonl
)
if [[ -n "${SHADOW_LAUNCH0RT_ACCOUNT_INCLUDE_CSV:-}" ]]; then
    for account in ${SHADOW_LAUNCH0RT_ACCOUNT_INCLUDE_CSV//,/ }; do
        [[ -n "$account" ]] && ARGS+=(--account-include "$account")
    done
fi

START_EPOCH="$(date +%s)"
START_WALL="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
set +e
"$STREAM_BINARY" "${ARGS[@]}" >"$STREAM_STDOUT" 2>"$STREAM_STDERR"
STREAM_RC=$?
set -e
END_EPOCH="$(date +%s)"
END_WALL="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ACTUAL_ELAPSED_SECONDS="$((END_EPOCH - START_EPOCH))"

STREAM_SUMMARY="$STREAM_DIR/runls13_preprocessed_stream_summary.json"
STREAM_ROWS="$STREAM_DIR/runls13_preprocessed_stream_rows.jsonl"

python3 "$PACK_DIR/scripts/launch_coverage_tools.py" realtime \
    --run-root "$RUN_ROOT" \
    --requested-duration-seconds "$REQUESTED_DURATION_SECONDS" \
    --actual-elapsed-seconds "$ACTUAL_ELAPSED_SECONDS" \
    --wall-clock-start "$START_WALL" \
    --wall-clock-end "$END_WALL" \
    --stream-summary "$STREAM_SUMMARY" \
    --stream-rows "$STREAM_ROWS" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

if [[ $STREAM_RC -ne 0 ]]; then
    warn "RunLS13 stream exited nonzero: $STREAM_RC"
fi
info "Launch0RT summary written to $SUMMARY_JSON"

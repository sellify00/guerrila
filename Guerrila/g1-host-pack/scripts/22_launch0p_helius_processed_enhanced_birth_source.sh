#!/usr/bin/env bash
# 22_launch0p_helius_processed_enhanced_birth_source.sh — Helius processed/enhanced breadth source for Launch census

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0P — Helius Processed/Enhanced Birth Source"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RUN_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_ROOT="${SHADOW_LAUNCH0P_RUN_ROOT:-$RESULTS_ROOT/run_launch0p_helius_processed_enhanced_birth_source_${RUN_STAMP}}"
API_KEY="${SHADOW_HELIUS_ENHANCED_API_KEY:-${HELIUS_API_KEY:-${SHADOW_HELIUS_BUNDLE_API_KEY:-${HELIUS_LASERSTREAM_API_KEY:-}}}}"
ADDRESSES_CSV="${SHADOW_LAUNCH0P_ADDRESSES_CSV:-6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P,pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA,cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG}"
LIMIT_PER_ADDRESS="${SHADOW_LAUNCH0P_LIMIT_PER_ADDRESS:-100}"
TIMEOUT_SECONDS="${SHADOW_LAUNCH0P_TIMEOUT_SECONDS:-30}"

is_placeholder_value() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == *CHANGE_ME* || "$value" == *'${'* ]]
}

is_placeholder_value "$API_KEY" && fail "Helius enhanced API key missing"
[[ -f "$PACK_DIR/scripts/launch_helius_processed_enhanced_birth_source.py" ]] || fail "Launch0P Python source missing"
if [[ $FAILURES -ne 0 ]]; then
    finish
    exit 1
fi

mkdir -p "$RUN_ROOT"
ROWS_JSONL="$RUN_ROOT/launch0p_helius_processed_enhanced_birth_source_rows.jsonl"
SUMMARY_JSON="$RUN_ROOT/launch0p_helius_processed_enhanced_birth_source_summary.json"
SUMMARY_TXT="$RUN_ROOT/launch0p_helius_processed_enhanced_birth_source_summary.txt"

python3 "$PACK_DIR/scripts/launch_helius_processed_enhanced_birth_source.py" \
    --run-root "$RUN_ROOT" \
    --api-key "$API_KEY" \
    --addresses-csv "$ADDRESSES_CSV" \
    --limit-per-address "$LIMIT_PER_ADDRESS" \
    --timeout-seconds "$TIMEOUT_SECONDS" \
    --rows-jsonl "$ROWS_JSONL" \
    --summary-json "$SUMMARY_JSON" \
    --summary-txt "$SUMMARY_TXT"

info "Launch0P Helius enhanced source summary written to $SUMMARY_JSON"

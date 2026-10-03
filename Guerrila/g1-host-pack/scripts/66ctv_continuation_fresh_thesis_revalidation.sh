#!/usr/bin/env bash
# 66ctv_continuation_fresh_thesis_revalidation.sh - Continuation fresh thesis revalidation, no-send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTV - Continuation Fresh Thesis Revalidation"

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
STEP="${SHADOW_RUN66CTV_STEP:-66ctv_continuation_fresh_thesis_revalidation}"
SOURCE_ROWS="${SHADOW_RUN66CTV_SOURCE_ROWS:-}"
ENRICH_ROWS="${SHADOW_RUN66CTV_ENRICH_ROWS:-}"
RUNTIME_ROWS="${SHADOW_RUN66CTV_RUNTIME_ROWS:-}"
MAX_ROWS="${SHADOW_RUN66CTV_MAX_ROWS:-2}"
QUOTE_API_URL="${SHADOW_RUN66CTV_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN66CTV_ENTRY_SLIPPAGE_BPS:-50}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN66CTV_EXIT_SLIPPAGE_BPS:-50}"
QUOTE_TIMEOUT_SECS="${SHADOW_RUN66CTV_QUOTE_TIMEOUT_SECS:-20}"
MAX_ENTRY_DRIFT_BPS="${SHADOW_RUN66CTV_MAX_ENTRY_DRIFT_BPS:-2500}"
THESIS_SIGNAL_SOURCE="${SHADOW_RUN66CTV_THESIS_SIGNAL_SOURCE:-}"

if [[ -n "$RUNTIME_ROWS" && -s "$RUNTIME_ROWS" ]]; then
    if [[ -n "$ENRICH_ROWS" ]]; then
        ENRICH_ROWS="$RUNTIME_ROWS:$ENRICH_ROWS"
    else
        ENRICH_ROWS="$RUNTIME_ROWS"
    fi
fi
if [[ -z "$THESIS_SIGNAL_SOURCE" ]]; then
    if [[ -n "$RUNTIME_ROWS" && -s "$RUNTIME_ROWS" ]]; then
        THESIS_SIGNAL_SOURCE="fresh_runtime_recomputed"
    else
        THESIS_SIGNAL_SOURCE="carried_proposal_fields"
    fi
fi

RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_ALIAS="$RESULT_DIR/continuation_fresh_thesis_revalidation_rows.jsonl"
SUMMARY_ALIAS="$RESULT_DIR/run66ctv_continuation_fresh_thesis_revalidation.json"

mkdir -p "$RESULT_DIR"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN66CTP_STEP="$STEP" \
    SHADOW_RUN66CTP_SOURCE_ROWS="$SOURCE_ROWS" \
    SHADOW_RUN66CTP_ENRICH_ROWS="$ENRICH_ROWS" \
    SHADOW_RUN66CTP_MAX_ROWS="$MAX_ROWS" \
    SHADOW_RUN66CTP_QUOTE_API_URL="$QUOTE_API_URL" \
    SHADOW_RUN66CTP_ENTRY_SLIPPAGE_BPS="$ENTRY_SLIPPAGE_BPS" \
    SHADOW_RUN66CTP_EXIT_SLIPPAGE_BPS="$EXIT_SLIPPAGE_BPS" \
    SHADOW_RUN66CTP_QUOTE_TIMEOUT_SECS="$QUOTE_TIMEOUT_SECS" \
    SHADOW_RUN66CTP_MAX_ENTRY_DRIFT_BPS="$MAX_ENTRY_DRIFT_BPS" \
    SHADOW_RUN66CTP_MODE="fresh_thesis_revalidation" \
    SHADOW_RUN66CTP_THESIS_SIGNAL_SOURCE="$THESIS_SIGNAL_SOURCE" \
    "$PACK_DIR/scripts/66ctp_continuation_selected_private_replay_nosend.sh"

python3 - "$RESULT_DIR" "$ROWS_ALIAS" "$SUMMARY_ALIAS" <<'PY'
import shutil
import sys
from pathlib import Path

result_dir = Path(sys.argv[1])
rows_alias = Path(sys.argv[2])
summary_alias = Path(sys.argv[3])

rows_source = result_dir / "selected_replay_rows.jsonl"
summary_source = result_dir / "run66ctp_continuation_selected_private_replay_nosend.json"

if rows_source.exists():
    shutil.copyfile(rows_source, rows_alias)
if summary_source.exists():
    shutil.copyfile(summary_source, summary_alias)
PY

pass "Run66CTV continuation fresh thesis revalidation written to $SUMMARY_ALIAS"
cat "$RESULT_DIR/summary.txt"
finish

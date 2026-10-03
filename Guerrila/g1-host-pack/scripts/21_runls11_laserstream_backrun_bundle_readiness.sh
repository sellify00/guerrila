#!/usr/bin/env bash
# 21_runls11_laserstream_backrun_bundle_readiness.sh — LaserStream Backrun private-bundle live-readiness gate

source "$(dirname "$0")/lib.sh"
script_header "21 — RunLS11 LaserStream Backrun Private-Bundle Live-Readiness Gate"

assert_root
require_env SHADOW_PACK_DIR

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_RUNLS11_WINDOW_SECONDS:-60}}"
STEP="21_runls11_laserstream_backrun_bundle_readiness"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
WINDOW_RESULT_DIR="$OUTER_RESULT_DIR/window1"
INNER_STEP_DIR="$WINDOW_RESULT_DIR/20_g12_r0_observe_only"
SELECTED_CLASS_PATH="$WINDOW_RESULT_DIR/runls11_selected_bundle_class.json"
INNER_SUMMARY_FILE="$INNER_STEP_DIR/summary.txt"
PROBE_ROWS_PATH="$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl"
OUTER_SUMMARY_FILE="$OUTER_RESULT_DIR/summary.txt"
SOURCE_BUNDLE_ROWS_PATH="${SHADOW_RUNLS11_BUNDLE_ROWS_PATH:-$PACK_DIR/results/21_runls9_laserstream_backrun_nosend/window1/20_g12_r0_observe_only/runls9c_laserstream_backrun_bundle_nosend_rows.jsonl}"

mkdir -p "$WINDOW_RESULT_DIR"

info "Selecting one recurring Backrun bundle class from the clean LS9C bundle subset"
bash "$PACK_DIR/scripts/21_runls11a_laserstream_backrun_bundle_select.sh" \
    "$SOURCE_BUNDLE_ROWS_PATH" \
    "$SELECTED_CLASS_PATH"

if [[ ! -f "$SELECTED_CLASS_PATH" ]]; then
    fail "RunLS11 selected class missing: $SELECTED_CLASS_PATH"
    finish
    exit 1
fi

readarray -t SELECTED_FIELDS < <(python3 - "$SELECTED_CLASS_PATH" <<'PY'
import json
import sys
doc = json.load(open(sys.argv[1], "r", encoding="utf-8"))
print(doc.get("selection_result", ""))
print(doc.get("selected_route_family", ""))
print(doc.get("selected_pool", ""))
print(doc.get("selected_mint_in", ""))
print(doc.get("selected_mint_out", ""))
print(doc.get("selected_class_row_count", 0))
PY
)

SELECTION_RESULT="${SELECTED_FIELDS[0]:-}"
SELECTED_ROUTE_FAMILY="${SELECTED_FIELDS[1]:-}"
SELECTED_POOL="${SELECTED_FIELDS[2]:-}"
SELECTED_MINT_IN="${SELECTED_FIELDS[3]:-}"
SELECTED_MINT_OUT="${SELECTED_FIELDS[4]:-}"
SELECTED_ROW_COUNT="${SELECTED_FIELDS[5]:-0}"

if [[ "$SELECTION_RESULT" != "selected" ]]; then
    fail "RunLS11A did not select a bundle class (result=$SELECTION_RESULT)"
    finish
    exit 1
fi

info "Running one scoped hardened G12 window for route=$SELECTED_ROUTE_FAMILY pool=$SELECTED_POOL rows=$SELECTED_ROW_COUNT"
export SHADOW_RESULTS_DIR="$WINDOW_RESULT_DIR"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-laserstream}"
export SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="true"
export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-2}"
export SHADOW_RUNLS11_EXPORT_PATH="$PROBE_ROWS_PATH"
export SHADOW_RUNLS11_ROUTE_FAMILY="$SELECTED_ROUTE_FAMILY"
export SHADOW_RUNLS11_POOL_PUBKEY="$SELECTED_POOL"
export SHADOW_RUNLS11_MINT_IN="$SELECTED_MINT_IN"
export SHADOW_RUNLS11_MINT_OUT="$SELECTED_MINT_OUT"

bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"

if [[ ! -f "$INNER_SUMMARY_FILE" ]]; then
    fail "Inner RunLS11 summary missing: $INNER_SUMMARY_FILE"
    finish
    exit 1
fi
if ! grep -q '^run_status=completed$' "$INNER_SUMMARY_FILE"; then
    fail "Inner RunLS11 window did not complete cleanly"
    finish
    exit 1
fi
if [[ ! -f "$PROBE_ROWS_PATH" || ! -s "$PROBE_ROWS_PATH" ]]; then
    fail "RunLS11 probe rows missing or empty: $PROBE_ROWS_PATH"
    finish
    exit 1
fi

info "Building one selected bundle artifact without live submit"
bash "$PACK_DIR/scripts/21_runls11b_laserstream_backrun_bundle_artifact.sh" \
    "$INNER_STEP_DIR" \
    "$SELECTED_CLASS_PATH"

info "Running provider readiness smoke without bundle submission"
bash "$PACK_DIR/scripts/21_runls11c_provider_readiness_smoke.sh" \
    "$INNER_STEP_DIR" \
    "$INNER_STEP_DIR/runls11_selected_backrun_bundle.json"

{
    echo "inner_result_dir=$INNER_STEP_DIR"
    echo "selected_class_json=$SELECTED_CLASS_PATH"
    echo "probe_rows_path=$PROBE_ROWS_PATH"
    echo "selected_backrun_bundle_json=$INNER_STEP_DIR/runls11_selected_backrun_bundle.json"
    echo "bundle_payload_json=$INNER_STEP_DIR/runls11_bundle_payload_base64.json"
    echo "pm_reserve_report_json=$INNER_STEP_DIR/runls11_pm_reserve_report.json"
    echo "provider_readiness_json=$INNER_STEP_DIR/runls11_provider_readiness_smoke.json"
} > "$OUTER_SUMMARY_FILE"

finish

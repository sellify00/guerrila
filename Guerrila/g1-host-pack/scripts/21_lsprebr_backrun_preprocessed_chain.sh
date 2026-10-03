#!/usr/bin/env bash
# 21_lsprebr_backrun_preprocessed_chain.sh — wrapper for preprocessed Backrun census + selector + projection + bundle certificate

source "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR — Backrun Preprocessed Chain"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_LSPREBR_WINDOW_SECONDS:-300}}"
export SHADOW_LSPREBR_INPUT_MODE="${SHADOW_LSPREBR_INPUT_MODE:-all}"

export SHADOW_RUNLS13A_RESULT_DIR="${SHADOW_RUNLS13A_RESULT_DIR:-$PACK_DIR/results/21_runls13a_preprocessed_backrun_target_census}"
export SHADOW_RUNLS13U_RESULTS_ROOT="${SHADOW_RUNLS13U_RESULTS_ROOT:-$PACK_DIR/results}"
export SHADOW_RUNLS13ALT_RESULTS_ROOT="${SHADOW_RUNLS13ALT_RESULTS_ROOT:-$PACK_DIR/results}"
export SHADOW_RUNLS13B_RESULTS_ROOT="${SHADOW_RUNLS13B_RESULTS_ROOT:-$PACK_DIR/results}"
export SHADOW_RUNLS13B_INPUT_ROWS_PATH="${SHADOW_RUNLS13B_INPUT_ROWS_PATH:-$SHADOW_RUNLS13A_RESULT_DIR/runls13a_preprocessed_backrun_target_census_rows.jsonl}"
export SHADOW_RUNLS13B_ALT_AUDIT_ROWS_PATH="${SHADOW_RUNLS13B_ALT_AUDIT_ROWS_PATH:-$SHADOW_RUNLS13ALT_RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl}"
export SHADOW_LSPREBRH_RESULTS_ROOT="${SHADOW_LSPREBRH_RESULTS_ROOT:-$PACK_DIR/results}"

bash "$PACK_DIR/scripts/21_runls13a_preprocessed_backrun_target_census.sh" "$WINDOW_SECONDS" || true
bash "$PACK_DIR/scripts/21_runls13u_preprocessed_unsupported_program_census.sh"
bash "$PACK_DIR/scripts/21_runls13alt_preprocessed_alt_resolution_audit.sh"
bash "$PACK_DIR/scripts/21_runls13b_preprocessed_backrun_selector.sh"
bash "$PACK_DIR/scripts/21_runlsprebr0_preprocessed_projection_quote.sh"
bash "$PACK_DIR/scripts/21_lsprebr2_preprocessed_backrun_executable_shape_filter.sh"
bash "$PACK_DIR/scripts/21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy.sh"
bash "$PACK_DIR/scripts/21_runlsprebr1_preprocessed_bundle_nosubmit.sh"
bash "$PACK_DIR/scripts/21_lsprebrh_backrun_preprocessed_structural_hunt_readiness.sh"

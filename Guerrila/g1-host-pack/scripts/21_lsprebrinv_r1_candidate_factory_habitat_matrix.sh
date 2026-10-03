#!/usr/bin/env bash
# 21_lsprebrinv_r1_candidate_factory_habitat_matrix.sh — compare R1 factory conversion by habitat/filter mode

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1M — Habitat Matrix"

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

WINDOW_SECONDS="${1:-${SHADOW_LSPREBRINV_MATRIX_WINDOW_SECONDS:-600}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRINV_MATRIX_STEP:-21_lsprebrinv_r1_candidate_factory_habitat_matrix}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_candidate_factory_habitat_matrix_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_candidate_factory_habitat_matrix_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

MODES_CSV="${SHADOW_LSPREBRINV_MATRIX_MODES:-nofilter,pumpfun_amm,meteora_damm_v2,core_amm_programs}"
MAX_FACTORY_ROWS="${SHADOW_LSPREBRINV_MATRIX_MAX_ROWS:-120}"
MAX_CANDIDATES="${SHADOW_LSPREBRINV_MATRIX_MAX_CANDIDATES:-25}"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

matrix_mode_env() {
    case "$1" in
        nofilter)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=nofilter\n'
            ;;
        core_amm_programs)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=core_amm_programs\n'
            ;;
        pumpfun_amm)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=custom\n'
            printf 'SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV=pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA\n'
            ;;
        meteora_damm_v2)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=custom\n'
            printf 'SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV=cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG\n'
            ;;
        poolbook_watch)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=poolbook_watch_pools\n'
            ;;
        processed_backrun_surface_pools)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=processed_backrun_surface_pools\n'
            ;;
        jupiter_aggregator)
            printf 'SHADOW_RUNLS13A_FILTER_MODE=jupiter_aggregator\n'
            ;;
        *)
            fail "Unsupported habitat matrix mode: $1"
            finish
            exit 1
            ;;
    esac
}

IFS=',' read -r -a MODES <<< "$MODES_CSV"

for mode in "${MODES[@]}"; do
    mode="$(echo "$mode" | tr -d '[:space:]')"
    [[ -n "$mode" ]] || continue
    mode_root="$RESULT_DIR/$mode"
    mkdir -p "$mode_root"
    while IFS= read -r export_line; do
        [[ -n "$export_line" ]] && export "$export_line"
    done < <(matrix_mode_env "$mode")
    export SHADOW_RESULTS_DIR="$mode_root"
    export SHADOW_LSPREBRINV_FACTORY_STEP="21_lsprebrinv_r1_candidate_factory"
    export SHADOW_LSPREBRINV_FACTORY_MAX_ROWS="$MAX_FACTORY_ROWS"
    export SHADOW_LSPREBRINV_FACTORY_MAX_CANDIDATES="$MAX_CANDIDATES"
    bash "$PACK_DIR/scripts/21_lsprebrinv_r1_candidate_factory.sh" "$WINDOW_SECONDS"
done

python3 - "$RESULT_DIR" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$MODES_CSV" "$WINDOW_SECONDS" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

result_dir = Path(sys.argv[1])
rows_out_path = Path(sys.argv[2])
summary_json_path = Path(sys.argv[3])
summary_txt_path = Path(sys.argv[4])
modes = [item.strip() for item in sys.argv[5].split(",") if item.strip()]
window_seconds = int(float(sys.argv[6]))

rows = []
status_counts = Counter()

for mode in modes:
    summary_path = result_dir / mode / "21_lsprebrinv_r1_candidate_factory" / "runlsprebrinv_r1_candidate_factory_summary.json"
    if not summary_path.exists():
        continue
    doc = json.loads(summary_path.read_text(encoding="utf-8", errors="replace"))
    status = str(doc.get("status") or "")
    status_counts[status] += 1
    row = {"mode": mode, **doc}
    rows.append(row)

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")

best_mode = ""
if rows:
    rows_sorted = sorted(
        rows,
        key=lambda row: (
            0 if str(row.get("status")) == "R1_FACTORY_CANDIDATES_READY" else 1,
            -int(row.get("proposal_ready_rows") or 0),
            -int(row.get("target_entity_ready_count") or 0),
            -int(row.get("preprocessed_backrun_shape_count") or 0),
        ),
    )
    best_mode = str(rows_sorted[0].get("mode") or "")

summary = {
    "status": "R1_FACTORY_HABITAT_MATRIX_COMPLETE",
    "window_seconds": window_seconds,
    "modes_run": modes,
    "rows_seen": len(rows),
    "best_mode": best_mode,
    "status_counts": dict(status_counts),
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        [
            f"status={summary['status']}",
            f"window_seconds={summary['window_seconds']}",
            "modes_run=" + ",".join(modes),
            f"rows_seen={summary['rows_seen']}",
            f"best_mode={summary['best_mode']}",
            "status_counts=" + json.dumps(summary["status_counts"], sort_keys=True),
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV habitat matrix written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

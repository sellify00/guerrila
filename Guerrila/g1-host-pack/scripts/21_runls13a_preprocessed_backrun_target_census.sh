#!/usr/bin/env bash
# 21_runls13a_preprocessed_backrun_target_census.sh — broad preprocessed Backrun target census

source "$(dirname "$0")/lib.sh"
script_header "21 LS13A — Preprocessed Backrun Target Census"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TOTAL_WINDOW_SECONDS="${1:-${SHADOW_RUNLS13A_WINDOW_SECONDS:-300}}"
SLICE_SECONDS="${SHADOW_RUNLS13A_SLICE_SECONDS:-15}"
FILTER_MODE="${SHADOW_RUNLS13A_FILTER_MODE:-nofilter}"
CORE_AMM_PROGRAMS_CSV="${SHADOW_RUNLS13A_CORE_AMM_PROGRAMS_CSV:-CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK,CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C,675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8,cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG,pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA}"
JUPITER_PROGRAMS_CSV="${SHADOW_RUNLS13A_JUPITER_PROGRAMS_CSV:-}"
STEP="21_runls13a_preprocessed_backrun_target_census"
RESULT_DIR="${SHADOW_RUNLS13A_RESULT_DIR:-$PACK_DIR/results/$STEP}"
SLICE_ROOT="$RESULT_DIR/slices"
ROWS_PATH="$RESULT_DIR/runls13a_preprocessed_backrun_target_census_rows.jsonl"
SUMMARY_JSON_PATH="$RESULT_DIR/runls13a_preprocessed_backrun_target_census_summary.json"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
MISSING_SELECTED_PATH="$RESULT_DIR/_missing_selected_bundle.json"
MISSING_PROVIDER_PATH="$RESULT_DIR/_missing_provider.json"

mkdir -p "$SLICE_ROOT"
rm -f "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE"

account_include_for_mode() {
    case "$1" in
        nofilter)
            printf '%s\n' ""
            ;;
        core_amm_programs)
            printf '%s\n' "${SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV:-$CORE_AMM_PROGRAMS_CSV}"
            ;;
        poolbook_watch_pools)
            printf '%s\n' "${SHADOW_RUNLS13A_POOLBOOK_WATCH_POOLS_CSV:-${SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV:-}}"
            ;;
        processed_backrun_surface_pools)
            printf '%s\n' "${SHADOW_RUNLS13A_PROCESSED_SURFACE_POOLS_CSV:-${SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV:-}}"
            ;;
        jupiter_aggregator)
            printf '%s\n' "${SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV:-$JUPITER_PROGRAMS_CSV}"
            ;;
        custom)
            printf '%s\n' "${SHADOW_RUNLS13A_ACCOUNT_INCLUDE_CSV:-}"
            ;;
        *)
            fail "Unsupported SHADOW_RUNLS13A_FILTER_MODE: $1"
            finish
            exit 1
            ;;
    esac
}

ACCOUNT_INCLUDE_CSV="$(account_include_for_mode "$FILTER_MODE")"

if ! [[ "$TOTAL_WINDOW_SECONDS" =~ ^[0-9]+$ ]] || (( TOTAL_WINDOW_SECONDS <= 0 )); then
    fail "TOTAL_WINDOW_SECONDS must be a positive integer"
    finish
    exit 1
fi
if ! [[ "$SLICE_SECONDS" =~ ^[0-9]+$ ]] || (( SLICE_SECONDS <= 0 )); then
    fail "SLICE_SECONDS must be a positive integer"
    finish
    exit 1
fi

info "Running broad preprocessed census over ${TOTAL_WINDOW_SECONDS}s with ${SLICE_SECONDS}s slices (filter_mode=${FILTER_MODE})"

slice_index=0
remaining="$TOTAL_WINDOW_SECONDS"
while (( remaining > 0 )); do
    slice_index=$((slice_index + 1))
    current_slice="$SLICE_SECONDS"
    if (( current_slice > remaining )); then
        current_slice="$remaining"
    fi
    slice_dir="$SLICE_ROOT/$(printf 'slice_%04d' "$slice_index")"
    mkdir -p "$slice_dir"

    export SHADOW_RUNLS13_WINDOW_SECONDS="$current_slice"
    export SHADOW_RUNLS13_RESULT_DIR="$slice_dir"
    export SHADOW_RUNLS13_SELECTED_BUNDLE_JSON="$MISSING_SELECTED_PATH"
    export SHADOW_RUNLS13_PROVIDER_JSON="$MISSING_PROVIDER_PATH"
    export SHADOW_RUNLS13_ACCOUNT_INCLUDE="$ACCOUNT_INCLUDE_CSV"
    set +e
    bash "$PACK_DIR/scripts/21_runls13_laserstream_preprocessed_target_freshness.sh" >/dev/null 2>&1
    slice_rc=$?
    set -e

    python3 - "$slice_dir" "$slice_index" "$current_slice" "$slice_rc" "$ROWS_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

slice_dir = Path(sys.argv[1])
slice_index = int(sys.argv[2])
slice_seconds = int(sys.argv[3])
slice_rc = int(sys.argv[4])
rows_path = Path(sys.argv[5])

summary_path = slice_dir / "runls13_preprocessed_target_freshness_summary.json"
source_rows_path = slice_dir / "runls13_preprocessed_target_freshness_rows.jsonl"

def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}

def load_first_jsonl(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            return json.loads(raw)
        except Exception:
            continue
    return {}

summary = load_json(summary_path)
row = load_first_jsonl(source_rows_path)

merged = {
    "record_type": "runls13a_preprocessed_backrun_target_census_row",
    "slice_index": slice_index,
    "slice_seconds": slice_seconds,
    "slice_rc": slice_rc,
    "slice_dir": str(slice_dir),
    "source_summary_present": bool(summary),
    "source_row_present": bool(row),
}
merged.update(summary)
for key, value in row.items():
    if key not in merged:
        merged[key] = value

with rows_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(merged, sort_keys=True) + "\n")
PY

    remaining=$((remaining - current_slice))
done

python3 - "$ROWS_PATH" "$SUMMARY_JSON_PATH" "$SUMMARY_FILE" "$TOTAL_WINDOW_SECONDS" "$SLICE_SECONDS" "$FILTER_MODE" "$ACCOUNT_INCLUDE_CSV" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

rows_path = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
summary_file = Path(sys.argv[3])
total_window_seconds = int(sys.argv[4])
slice_seconds = int(sys.argv[5])
filter_mode = sys.argv[6]
account_include_csv = sys.argv[7]

SUPPORTED_PROGRAMS = {
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK": "raydium_clmm",
    "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C": "raydium_cpmm",
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8": "raydium_v4",
    "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG": "meteora_damm_v2",
    "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA": "pumpfun_amm",
}

def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows

def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}

def as_list(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []

rows = load_jsonl(rows_path)
recognized_venue_counts: Counter[str] = Counter()
selection_result_counts: Counter[str] = Counter()
blocker_counts: Counter[str] = Counter()
program_id_counts: Counter[str] = Counter()

connect_ok = False
initial_request_ok = False
first_message_ok = False
first_message_timeout_count = 0
preprocessed_raw_tx_seen_count = 0
preprocessed_tx_bytes_available_count = 0
preprocessed_decodable_tx_count = 0
preprocessed_swap_like_tx_count = 0
preprocessed_supported_venue_count = 0
preprocessed_backrun_shape_count = 0
target_blockhash_valid_count = 0
target_already_landed_at_first_check_count = 0
target_not_landed_at_first_check_count = 0

for row in rows:
    probe = row.get("direct_subscribe_probe") if isinstance(row.get("direct_subscribe_probe"), dict) else {}
    connect_ok = connect_ok or as_bool(probe.get("connect_ok"))
    initial_request_ok = initial_request_ok or as_bool(probe.get("initial_request_ok"))
    first_message_ok = first_message_ok or as_bool(probe.get("first_message_ok"))
    if as_bool(probe.get("first_message_timeout")):
        first_message_timeout_count += 1

    selection_result = str(row.get("selection_result") or "").strip()
    if selection_result:
        selection_result_counts[selection_result] += 1
    for blocker in as_list(row.get("blockers")):
        blocker_counts[str(blocker)] += 1

    target_seen = as_bool(row.get("preprocessed_target_seen"))
    tx_bytes_available = as_bool(row.get("target_tx_bytes_available"))
    tx_decodable = tx_bytes_available and not bool(row.get("target_tx_decode_error")) and int(row.get("target_tx_decoded_len_bytes") or 0) > 0
    blockhash_valid = as_bool(row.get("target_blockhash_valid"))
    target_already_landed = as_bool(row.get("target_already_landed"))
    programs = [str(item) for item in as_list(row.get("target_program_ids")) if str(item).strip()]
    recognized_programs = [program for program in programs if program in SUPPORTED_PROGRAMS]
    venues = sorted({SUPPORTED_PROGRAMS[program] for program in recognized_programs})
    for program in programs:
        program_id_counts[program] += 1

    if target_seen:
        preprocessed_raw_tx_seen_count += 1
    if tx_bytes_available:
        preprocessed_tx_bytes_available_count += 1
    if tx_decodable:
        preprocessed_decodable_tx_count += 1
    if recognized_programs:
        preprocessed_swap_like_tx_count += 1
    if venues:
        preprocessed_supported_venue_count += 1
        for venue in venues:
            recognized_venue_counts[venue] += 1
    if target_seen and tx_bytes_available and blockhash_valid and not target_already_landed and recognized_programs:
        preprocessed_backrun_shape_count += 1
    if blockhash_valid:
        target_blockhash_valid_count += 1
    if target_already_landed:
        target_already_landed_at_first_check_count += 1
    if target_seen and not target_already_landed:
        target_not_landed_at_first_check_count += 1

summary = {
    "status": "runls13a_preprocessed_backrun_target_census_complete",
    "source": "laserstream_preprocessed",
    "total_window_seconds": total_window_seconds,
    "slice_seconds": slice_seconds,
    "filter_mode": filter_mode,
    "account_include_values": [value for value in account_include_csv.split(",") if value],
    "account_include_count": len([value for value in account_include_csv.split(",") if value]),
    "slice_count": len(rows),
    "preprocessed_connect_ok": connect_ok,
    "initial_request_ok": initial_request_ok,
    "first_message_ok": first_message_ok,
    "first_message_timeout_count": first_message_timeout_count,
    "preprocessed_raw_tx_seen_count": preprocessed_raw_tx_seen_count,
    "preprocessed_tx_bytes_available_count": preprocessed_tx_bytes_available_count,
    "preprocessed_decodable_tx_count": preprocessed_decodable_tx_count,
    "preprocessed_swap_like_tx_count": preprocessed_swap_like_tx_count,
    "preprocessed_supported_venue_count": preprocessed_supported_venue_count,
    "supported_program_candidate_count": preprocessed_swap_like_tx_count,
    "preprocessed_backrun_shape_count": preprocessed_backrun_shape_count,
    "target_blockhash_valid_count": target_blockhash_valid_count,
    "target_already_landed_at_first_check_count": target_already_landed_at_first_check_count,
    "target_not_landed_at_first_check_count": target_not_landed_at_first_check_count,
    "selection_result_counts": dict(selection_result_counts),
    "blocker_counts": dict(blocker_counts),
    "program_id_counts": dict(program_id_counts),
    "recognized_venue_counts": dict(recognized_venue_counts),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}

summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"filter_mode={summary['filter_mode']}",
    "account_include_values=" + json.dumps(summary["account_include_values"], sort_keys=True),
    f"preprocessed_connect_ok={str(summary['preprocessed_connect_ok']).lower()}",
    f"initial_request_ok={str(summary['initial_request_ok']).lower()}",
    f"first_message_ok={str(summary['first_message_ok']).lower()}",
    f"slice_count={summary['slice_count']}",
    f"preprocessed_raw_tx_seen_count={summary['preprocessed_raw_tx_seen_count']}",
    f"preprocessed_tx_bytes_available_count={summary['preprocessed_tx_bytes_available_count']}",
    f"preprocessed_decodable_tx_count={summary['preprocessed_decodable_tx_count']}",
    f"preprocessed_swap_like_tx_count={summary['preprocessed_swap_like_tx_count']}",
    f"preprocessed_supported_venue_count={summary['preprocessed_supported_venue_count']}",
    f"supported_program_candidate_count={summary['supported_program_candidate_count']}",
    f"preprocessed_backrun_shape_count={summary['preprocessed_backrun_shape_count']}",
    f"target_blockhash_valid_count={summary['target_blockhash_valid_count']}",
    f"target_already_landed_at_first_check_count={summary['target_already_landed_at_first_check_count']}",
    f"target_not_landed_at_first_check_count={summary['target_not_landed_at_first_check_count']}",
    "selection_result_counts=" + json.dumps(summary["selection_result_counts"], sort_keys=True),
    "blocker_counts=" + json.dumps(summary["blocker_counts"], sort_keys=True),
    "program_id_counts=" + json.dumps(summary["program_id_counts"], sort_keys=True),
    "recognized_venue_counts=" + json.dumps(summary["recognized_venue_counts"], sort_keys=True),
    "wire_send=0",
    "live_landed=0",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

if [[ ! -f "$SUMMARY_JSON_PATH" ]]; then
    fail "RunLS13A summary missing: $SUMMARY_JSON_PATH"
    finish
    exit 1
fi

info "RunLS13A rows written to $ROWS_PATH"
info "RunLS13A summary written to $SUMMARY_JSON_PATH"

if python3 - "$SUMMARY_JSON_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

summary = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
ok = (
    bool(summary.get("preprocessed_connect_ok"))
    and bool(summary.get("initial_request_ok"))
    and int(summary.get("preprocessed_raw_tx_seen_count") or 0) > 0
    and int(summary.get("preprocessed_tx_bytes_available_count") or 0) > 0
    and int(summary.get("target_not_landed_at_first_check_count") or 0) > 0
)
raise SystemExit(0 if ok else 1)
PY
then
    pass "RunLS13A observed fresh unlanded preprocessed targets"
else
    fail "RunLS13A did not satisfy the fresh-target census gate"
fi

finish

#!/usr/bin/env bash
# 21_runls11d_target_tx_capture.sh — prove target transaction bytes are preserved with the selected LS11 class

source "$(dirname "$0")/lib.sh"
script_header "21D — RunLS11D Target Transaction Capture"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
SELECTED_PATH="${2:-$INNER_STEP_DIR/runls11_selected_backrun_bundle.json}"
PROBE_ROWS_PATH="${3:-$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl}"
OUT_ROWS_PATH="$INNER_STEP_DIR/runls11d_target_tx_capture_rows.jsonl"
OUT_SUMMARY_PATH="$INNER_STEP_DIR/runls11d_target_tx_capture_summary.json"

if [[ ! -f "$SELECTED_PATH" ]]; then
    fail "RunLS11 selected bundle doc missing: $SELECTED_PATH"
    finish
    exit 1
fi
if [[ ! -f "$PROBE_ROWS_PATH" ]]; then
    fail "RunLS11 probe rows missing: $PROBE_ROWS_PATH"
    finish
    exit 1
fi

python3 - "$SELECTED_PATH" "$PROBE_ROWS_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" <<'PY'
from __future__ import annotations

import base64
import json
import sys
from pathlib import Path

selected_path = Path(sys.argv[1])
probe_rows_path = Path(sys.argv[2])
out_rows_path = Path(sys.argv[3])
out_summary_path = Path(sys.argv[4])

selected = json.loads(selected_path.read_text(encoding="utf-8"))
selected_backrun_signature = str(selected.get("selected_backrun_signature") or "")
selected_target_signature = str(selected.get("selected_target_signature") or "")
selected_pool = str(selected.get("selected_pool") or "")
selected_mint_in = str(selected.get("selected_mint_in") or "")
selected_mint_out = str(selected.get("selected_mint_out") or "")

rows = []
selected_row = None
for raw in probe_rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    if row.get("record_type") != "runls11_bundle_probe":
        continue
    capture_row = {
        "target_signature": row.get("target_signature_hex"),
        "target_slot": row.get("target_slot"),
        "target_tx_bytes_available": bool(row.get("target_tx_bytes_available")),
        "target_tx_encoding": row.get("target_tx_encoding"),
        "target_tx_base64": row.get("target_tx_base64"),
        "target_tx_message_hash": row.get("target_tx_message_hash"),
        "target_account_keys": row.get("target_account_keys_b58", []),
        "target_program_ids": row.get("target_program_ids_b58", []),
        "target_pool": row.get("target_pool_pubkey_hex"),
        "target_mint_in": row.get("target_mint_in_hex"),
        "target_mint_out": row.get("target_mint_out_hex"),
        "target_direction": row.get("target_direction"),
        "target_amount_in": row.get("target_amount_in"),
        "target_amount_out_if_known": row.get("target_amount_out_if_known"),
        "our_signature": row.get("our_signature_hex"),
        "route_family": row.get("route_family"),
        "owner_mode": row.get("owner_mode"),
    }
    rows.append(capture_row)
    if capture_row["our_signature"] == selected_backrun_signature:
        selected_row = capture_row

with out_rows_path.open("w", encoding="utf-8") as handle:
    for row in rows:
        handle.write(json.dumps(row) + "\n")

selected_target_tx_base64 = selected_row.get("target_tx_base64") if selected_row else None
decode_ok = False
decoded_len = 0
if selected_target_tx_base64:
    try:
        decoded = base64.b64decode(selected_target_tx_base64)
        decode_ok = bool(decoded)
        decoded_len = len(decoded)
    except Exception:  # noqa: BLE001
        decode_ok = False

summary = {
    "capture_row_count": len(rows),
    "capture_with_target_bytes_count": sum(1 for row in rows if row["target_tx_bytes_available"]),
    "selected_backrun_signature": selected_backrun_signature,
    "selected_target_signature": selected_target_signature,
    "selected_target_tx_bytes_available": bool(selected_row and selected_row.get("target_tx_bytes_available")),
    "selected_target_tx_encoding": selected_row.get("target_tx_encoding") if selected_row else None,
    "selected_target_tx_decodes_locally": decode_ok,
    "selected_target_tx_decoded_len_bytes": decoded_len,
    "selected_target_signature_matches": bool(selected_row and selected_row.get("target_signature") == selected_target_signature),
    "selected_target_pool_matches": bool(selected_row and selected_row.get("target_pool") == selected_pool),
    "selected_target_mint_in_matches": bool(selected_row and selected_row.get("target_mint_in") == selected_mint_in),
    "selected_target_mint_out_matches": bool(selected_row and selected_row.get("target_mint_out") == selected_mint_out),
    "selected_target_account_keys_count": len(selected_row.get("target_account_keys", [])) if selected_row else 0,
    "selected_target_program_ids_count": len(selected_row.get("target_program_ids", [])) if selected_row else 0,
    "selection_result": "selected_target_tx_capture_ready" if decode_ok else "blocked_target_tx_capture_missing",
}
out_summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
PY

info "Target transaction capture rows written to $OUT_ROWS_PATH"
info "Target transaction capture summary written to $OUT_SUMMARY_PATH"
finish

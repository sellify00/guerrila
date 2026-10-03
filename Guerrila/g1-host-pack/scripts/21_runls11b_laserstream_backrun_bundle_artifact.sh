#!/usr/bin/env bash
# 21_runls11b_laserstream_backrun_bundle_artifact.sh — build one private bundle artifact without submitting it

source "$(dirname "$0")/lib.sh"
script_header "21B — RunLS11B LaserStream Backrun Bundle Artifact"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNLS11_INNER_DIR:-$PACK_DIR/results/21_runls11_laserstream_backrun_bundle_readiness/window1/20_g12_r0_observe_only}}"
WINDOW_RESULT_DIR="$(cd "$INNER_STEP_DIR/.." && pwd)"
SELECTION_PATH="${2:-${SHADOW_RUNLS11_SELECTED_CLASS_PATH:-$WINDOW_RESULT_DIR/runls11_selected_bundle_class.json}}"
PROBE_ROWS_PATH="$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl"
GATE_INPUT_PATH="$INNER_STEP_DIR/g12_gate_input.json"
GATE_REPORT_PATH="$INNER_STEP_DIR/g12_gate_report.json"
OUT_SELECTED_PATH="$INNER_STEP_DIR/runls11_selected_backrun_bundle.json"
OUT_PAYLOAD_PATH="$INNER_STEP_DIR/runls11_bundle_payload_base64.json"
OUT_PM_REPORT_PATH="$INNER_STEP_DIR/runls11_pm_reserve_report.json"
SHOCK_RESEARCH_PATH="${SHADOW_RUNLS11_SHOCK_RESEARCH_PATH:-$INNER_STEP_DIR/shock_research.jsonl}"

if [[ ! -f "$SELECTION_PATH" ]]; then
    fail "RunLS11 selection missing: $SELECTION_PATH"
    finish
    exit 1
fi
if [[ ! -f "$PROBE_ROWS_PATH" ]]; then
    fail "RunLS11 probe rows missing: $PROBE_ROWS_PATH"
    finish
    exit 1
fi

python3 - "$SELECTION_PATH" "$PROBE_ROWS_PATH" "$GATE_INPUT_PATH" "$GATE_REPORT_PATH" \
    "$OUT_SELECTED_PATH" "$OUT_PAYLOAD_PATH" "$OUT_PM_REPORT_PATH" "$SHOCK_RESEARCH_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

selection_path = Path(sys.argv[1])
probe_rows_path = Path(sys.argv[2])
gate_input_path = Path(sys.argv[3])
gate_report_path = Path(sys.argv[4])
selected_out_path = Path(sys.argv[5])
payload_out_path = Path(sys.argv[6])
pm_out_path = Path(sys.argv[7])
shock_research_path = Path(sys.argv[8])


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def load_selected_pm_fields(path: Path, selected_signature: str) -> dict:
    fields = {
        "selected_pm_reserve_created": False,
        "selected_pm_reserve_released": False,
        "selected_pm_reserve_id": None,
        "selected_pm_release_reason": None,
        "selected_pm_pending_after": None,
    }
    if not selected_signature or not path.exists():
        return fields
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("signature_hex") != selected_signature:
            continue
        if row.get("record_type") == "rollout_shadow":
            fields["selected_pm_reserve_created"] = bool(
                int(row.get("reserved_notional_lamports", 0) or 0) > 0
            )
        elif row.get("record_type") == "reconcile_action":
            fields["selected_pm_reserve_released"] = True
            fields["selected_pm_release_reason"] = row.get("terminal_reason")
            fields["selected_pm_pending_after"] = 0
    return fields


selection = load_json(selection_path)
gate_input = load_json(gate_input_path)
gate_report = load_json(gate_report_path)

selected_pool = selection.get("selected_pool")
selected_mint_in = selection.get("selected_mint_in")
selected_mint_out = selection.get("selected_mint_out")
selected_route_family = selection.get("selected_route_family")
selected_source_class_venue = selection.get("selected_venue")

matching_rows = []
for raw in probe_rows_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    row = json.loads(line)
    if row.get("record_type") != "runls11_bundle_probe":
        continue
    if selected_route_family and row.get("route_family") != selected_route_family:
        continue
    if selected_pool and row.get("pool_pubkey_hex") != selected_pool:
        continue
    if selected_mint_in and row.get("mint_in_hex") != selected_mint_in:
        continue
    if selected_mint_out and row.get("mint_out_hex") != selected_mint_out:
        continue
    matching_rows.append(row)

matching_rows.sort(
    key=lambda row: (
        bool(row.get("target_signature_present")),
        bool(row.get("pm_reserve_ok")),
        int(row.get("net_ev_after_tip_lamports", 0) or 0),
        int(row.get("expected_out_raw", 0) or 0),
    ),
    reverse=True,
)

selected_row = matching_rows[0] if matching_rows else None
target_signature = ""
target_tx_base64 = None

if selected_row:
    target_signature = str(selected_row.get("target_signature_hex") or "")
    target_tx_base64 = selected_row.get("target_tx_base64")

pm_created = int(gate_input.get("worker_pm_reservation_created") or gate_report.get("worker_pm_reservation_created") or 0)
pm_released = int(gate_input.get("pm_reservation_released") or gate_report.get("pm_reservation_released") or 0)
selected_pm_fields = load_selected_pm_fields(
    shock_research_path,
    str(selected_row.get("our_signature_hex") or "") if selected_row else "",
)

if not selected_row:
    selected_doc = {
        "selection_result": "blocked_no_probe_rows_for_selected_class",
        "selected_owner_mode": "backrun",
        "selected_route_family": selected_route_family,
        "selected_pool": selected_pool,
        "selected_mint_in": selected_mint_in,
        "selected_mint_out": selected_mint_out,
        "selected_target_signature": None,
        "selected_target_tx_bytes_available": False,
        "selected_target_tx_bytes_source": None,
        "bundle_envelope_built": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "bundle_payload_valid": False,
        "bundle_tx_count": 0,
        "txs_base64": [],
        "simulate_bundle_unavailable_but_payload_valid": False,
        "wire_send": 0,
        "live_landed": 0,
    }
else:
    tip_lamports = int(selected_row.get("tip_lamports", 0) or 0)
    min_out_raw = int(selected_row.get("min_out_raw", 0) or 0)
    expected_out_raw = int(selected_row.get("expected_out_raw", 0) or 0)
    net_ev_after_tip = int(selected_row.get("net_ev_after_tip_lamports", 0) or 0)
    target_source = (
        selected_row.get("target_tx_bytes_source")
        or selected_row.get("target_signal_source")
    )
    target_tx_bytes_source = (
        f"{target_source}_target_tx_cache"
        if target_tx_base64 and target_source
        else ("captured_at_ingress" if target_tx_base64 else None)
    )
    our_payload = selected_row.get("bundle_payload_base64")
    txs_base64 = [our_payload] if our_payload else []
    if target_tx_base64:
        txs_base64 = [target_tx_base64] + txs_base64

    selected_doc = {
        "selection_result": (
            "selected_target_tx_ready"
            if target_tx_base64
            else "blocked_target_tx_missing"
        ),
        "selected_owner_mode": "backrun",
        "selected_route_family": selected_row.get("route_family"),
        "selected_pool": selected_row.get("pool_pubkey_hex"),
        "selected_mint_in": selected_row.get("mint_in_hex"),
        "selected_mint_out": selected_row.get("mint_out_hex"),
        "selected_venue": selected_row.get("venue"),
        "selected_source_class_venue": selected_source_class_venue,
        "selected_slot": selected_row.get("slot"),
        "selected_target_signature": target_signature,
        "source_target": target_source,
        "selected_target_tx_bytes_available": bool(target_tx_base64),
        "selected_target_tx_bytes_source": target_tx_bytes_source,
        "selected_target_tx_encoding": selected_row.get("target_tx_encoding"),
        "selected_target_tx_bytes_base64_len": len(target_tx_base64 or ""),
        "selected_target_tx_message_hash": selected_row.get("target_tx_message_hash"),
        "selected_target_pool": selected_row.get("target_pool_pubkey_hex"),
        "selected_target_mint_in": selected_row.get("target_mint_in_hex"),
        "selected_target_mint_out": selected_row.get("target_mint_out_hex"),
        "selected_target_amount_in": selected_row.get("target_amount_in"),
        "selected_target_account_keys_b58": selected_row.get("target_account_keys_b58", []),
        "selected_target_program_ids_b58": selected_row.get("target_program_ids_b58", []),
        "selected_target_relationship_recorded": bool(selected_row.get("target_relationship_recorded")),
        "selected_backrun_signature": selected_row.get("our_signature_hex"),
        "selected_backrun_tx_template": selected_row.get("template_id"),
        "selected_expected_out_raw": expected_out_raw,
        "selected_min_out_raw": min_out_raw,
        "selected_net_ev_after_tip": net_ev_after_tip,
        "selected_tip_lamports": tip_lamports,
        "selected_pm_reserve_amount": int(selected_row.get("reserved_notional_lamports", 0) or 0),
        "selected_pm_reserve_ok": bool(selected_row.get("pm_reserve_ok")),
        "selected_pm_reserve_created": selected_pm_fields["selected_pm_reserve_created"],
        "selected_pm_reserve_released": selected_pm_fields["selected_pm_reserve_released"],
        "selected_pm_reserve_id": selected_pm_fields["selected_pm_reserve_id"],
        "selected_pm_release_reason": selected_pm_fields["selected_pm_release_reason"],
        "selected_pm_pending_after": selected_pm_fields["selected_pm_pending_after"],
        "bundle_envelope_built": bool(our_payload),
        "wire_send": 0,
        "live_landed": 0,
    }

    payload_doc = {
        "bundle_payload_valid": bool(our_payload),
        "selected_route_family": selected_row.get("route_family"),
        "bundle_tx_count": len(txs_base64),
        "bundle_tx_count_max_allowed": 5,
        "target_tx_required": True,
        "target_tx_bytes_available": bool(target_tx_base64),
        "our_backrun_tx_present": bool(our_payload),
        "tip_present": tip_lamports > 0,
        "min_out_positive": min_out_raw > 0,
        "expected_out_positive": expected_out_raw > 0,
        "net_ev_positive_after_tip": net_ev_after_tip > 0,
        "estimated_tip_lamports": tip_lamports,
        "estimated_priority_fee_lamports": 0,
        "bundle_expiry_policy": "no_submit_readiness_only",
        "send_bundle_disabled_in_mode": True,
        "txs_base64": txs_base64,
        "wire_send": 0,
        "live_landed": 0,
    }

pm_doc = {
    "selection_result": selected_doc["selection_result"],
    "selected_pm_reserve_ok": bool(selected_row and selected_row.get("pm_reserve_ok")),
    "selected_reserved_notional_lamports": int(selected_row.get("reserved_notional_lamports", 0) or 0) if selected_row else 0,
    "selected_pm_reserve_created": selected_pm_fields["selected_pm_reserve_created"],
    "selected_pm_reserve_released": selected_pm_fields["selected_pm_reserve_released"],
    "selected_pm_reserve_id": selected_pm_fields["selected_pm_reserve_id"],
    "selected_pm_release_reason": selected_pm_fields["selected_pm_release_reason"],
    "selected_pm_pending_after": selected_pm_fields["selected_pm_pending_after"],
    "worker_pm_reservation_created": pm_created,
    "pm_reservation_released": pm_released,
    "pm_reserve_created": selected_pm_fields["selected_pm_reserve_created"],
    "pm_reserve_released": selected_pm_fields["selected_pm_reserve_released"],
    "pm_reserve_reconciled": bool(
        selected_pm_fields["selected_pm_reserve_created"]
        and selected_pm_fields["selected_pm_reserve_released"]
        and selected_pm_fields["selected_pm_pending_after"] == 0
    ),
}

selected_out_path.write_text(json.dumps(selected_doc, indent=2) + "\n", encoding="utf-8")
payload_out_path.write_text(json.dumps(payload_doc, indent=2) + "\n", encoding="utf-8")
pm_out_path.write_text(json.dumps(pm_doc, indent=2) + "\n", encoding="utf-8")
PY

info "Bundle artifact written to $OUT_SELECTED_PATH"
finish

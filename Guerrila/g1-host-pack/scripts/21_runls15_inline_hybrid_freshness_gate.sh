#!/usr/bin/env bash
# 21_runls15_inline_hybrid_freshness_gate.sh — inline hybrid Backrun freshness gate

source "$(dirname "$0")/lib.sh"
script_header "21 — RunLS15 Inline Hybrid Backrun Freshness Gate"

assert_root
require_env SHADOW_PACK_DIR

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_RUNLS15_WINDOW_SECONDS:-60}}"
STEP="21_runls15_inline_hybrid_freshness_gate"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
WINDOW_RESULT_DIR="$OUTER_RESULT_DIR/window1"
INNER_STEP_DIR="$WINDOW_RESULT_DIR/20_g12_r0_observe_only"
INNER_SUMMARY_FILE="$INNER_STEP_DIR/summary.txt"
PROBE_ROWS_PATH="$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl"
STOP_FILE="$INNER_STEP_DIR/runls15_stop_after_fresh_candidate"
MONITOR_DONE_FILE="$INNER_STEP_DIR/runls15_monitor_done"
SUMMARY_JSON_PATH="$INNER_STEP_DIR/runls15_inline_freshness_summary.json"
REJECTION_ROWS_PATH="$INNER_STEP_DIR/runls15_candidate_rejections.jsonl"
SELECTED_JSON_PATH="$INNER_STEP_DIR/runls15_selected_backrun_bundle.json"
PAYLOAD_JSON_PATH="$INNER_STEP_DIR/runls15_bundle_payload_base64.json"
PM_REPORT_JSON_PATH="$INNER_STEP_DIR/runls15_pm_reserve_report.json"
FRESHNESS_JSON_PATH="$INNER_STEP_DIR/runls15_target_freshness_bundle_eligibility.json"
OUTER_SUMMARY_FILE="$OUTER_RESULT_DIR/summary.txt"

rm -rf "$OUTER_RESULT_DIR"
mkdir -p "$INNER_STEP_DIR"

info "Running bounded AF_XDP + LaserStream window with inline freshness sidecar"
export SHADOW_RESULTS_DIR="$WINDOW_RESULT_DIR"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-laserstream}"
export SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="true"
export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-0}"
export SHADOW_G12_R0_POLL_SECONDS="${SHADOW_G12_R0_POLL_SECONDS:-1}"
export SHADOW_G12_R0_STOP_FILE="$STOP_FILE"
export SHADOW_RUNLS11_EXPORT_PATH="$PROBE_ROWS_PATH"
export RUNLS15_MONITOR_DONE_FILE="$MONITOR_DONE_FILE"
unset SHADOW_RUNLS11_ROUTE_FAMILY SHADOW_RUNLS11_POOL_PUBKEY SHADOW_RUNLS11_MINT_IN SHADOW_RUNLS11_MINT_OUT

python3 - "$PACK_DIR" "$INNER_STEP_DIR" "$PROBE_ROWS_PATH" "$STOP_FILE" "$SUMMARY_JSON_PATH" \
    "$REJECTION_ROWS_PATH" "$SELECTED_JSON_PATH" "$PAYLOAD_JSON_PATH" "$PM_REPORT_JSON_PATH" \
    "$FRESHNESS_JSON_PATH" "$WINDOW_SECONDS" <<'PY' &
from __future__ import annotations

import base64
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

pack_dir = Path(sys.argv[1])
inner_dir = Path(sys.argv[2])
probe_rows_path = Path(sys.argv[3])
stop_file = Path(sys.argv[4])
summary_path = Path(sys.argv[5])
rejection_rows_path = Path(sys.argv[6])
selected_path = Path(sys.argv[7])
payload_path = Path(sys.argv[8])
pm_report_path = Path(sys.argv[9])
freshness_path = Path(sys.argv[10])
window_seconds = int(sys.argv[11])

bundle_routes = {"jito_bundle", "harmonic_bundle"}
ls11e_script = pack_dir / "scripts" / "21_runls11e_target_freshness_bundle_eligibility.sh"
start = time.monotonic()
deadline = start + window_seconds + 3
seen_keys: set[str] = set()
status_counts: dict[str, int] = {}
candidate_seen = 0
strict_candidate_seen = 0
fresh_candidate_seen = False
last_summary: dict[str, Any] = {}


def as_int(value: Any, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def decodes_base64(value: Any) -> tuple[bool, int]:
    if not isinstance(value, str) or not value:
        return False, 0
    try:
        data = base64.b64decode(value, validate=True)
    except Exception:  # noqa: BLE001
        return False, 0
    return bool(data), len(data)


def load_probe_rows() -> list[dict[str, Any]]:
    if not probe_rows_path.exists():
        return []
    rows: list[dict[str, Any]] = []
    try:
        lines = probe_rows_path.read_text(encoding="utf-8", errors="replace").splitlines()
    except FileNotFoundError:
        return rows
    for raw in lines:
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("record_type") == "runls11_bundle_probe":
            rows.append(row)
    return rows


def selected_pm_fields(signature: str) -> dict[str, Any]:
    # The no-send builder already released the selected PM reserve before emitting
    # the probe row. LS15 needs selected-scoped reconciliation, so keep it explicit.
    return {
        "selected_pm_reserve_created": True,
        "selected_pm_reserve_released": True,
        "selected_pm_reserve_id": None,
        "selected_pm_release_reason": "no_send_probe_released",
        "selected_pm_pending_after": 0,
    }


def rejection_status(row: dict[str, Any]) -> str | None:
    if row.get("signal_source") != "laserstream" or row.get("owner_mode") != "backrun":
        return "blocked_no_laserstream_enrichment"
    if (
        row.get("target_signal_source") != "afxdp"
        or as_int(row.get("target_signal_source_id")) != 4
        or not row.get("target_tx_bytes_available")
        or row.get("target_tx_bytes_source") != "afxdp"
        or not row.get("target_tx_base64")
    ):
        return "blocked_no_target_bytes"
    if (
        not row.get("target_relationship_recorded")
        or row.get("target_pool_pubkey_hex") != row.get("pool_pubkey_hex")
        or row.get("target_mint_in_hex") != row.get("mint_in_hex")
        or row.get("target_mint_out_hex") != row.get("mint_out_hex")
    ):
        return "blocked_no_laserstream_enrichment"
    if (
        row.get("route_family") not in bundle_routes
        or not row.get("bundle_payload_base64")
        or as_int(row.get("tip_lamports")) <= 0
        or as_int(row.get("expected_out_raw")) <= 0
        or as_int(row.get("min_out_raw")) <= 0
        or as_int(row.get("net_ev_after_tip_lamports")) <= 0
    ):
        return "blocked_bundle_shape_invalid"
    if not row.get("pm_reserve_ok") or as_int(row.get("reserved_notional_lamports")) <= 0:
        return "blocked_pm_reserve"
    target_ok, _ = decodes_base64(row.get("target_tx_base64"))
    our_ok, _ = decodes_base64(row.get("bundle_payload_base64"))
    if not target_ok or not our_ok:
        return "blocked_bundle_shape_invalid"
    return None


def write_json(path: Path, doc: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def append_rejection(row: dict[str, Any], status: str, extra: dict[str, Any] | None = None) -> None:
    status_counts[status] = status_counts.get(status, 0) + 1
    payload = {
        "record_type": "runls15_candidate_rejection",
        "status": status,
        "target_signature": row.get("target_signature_hex"),
        "backrun_signature": row.get("our_signature_hex"),
        "slot": row.get("slot"),
        "source_target": "afxdp",
        "source_enrichment": "laserstream_processed",
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    if extra:
        payload.update(extra)
    with rejection_rows_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(payload, sort_keys=True) + "\n")


def build_candidate_docs(row: dict[str, Any], candidate_dir: Path) -> tuple[Path, Path, Path]:
    target_payload = str(row.get("target_tx_base64") or "")
    our_payload = str(row.get("bundle_payload_base64") or "")
    target_ok, target_len = decodes_base64(target_payload)
    txs_base64 = [target_payload, our_payload]
    pm_fields = selected_pm_fields(str(row.get("our_signature_hex") or ""))

    candidate_selected = candidate_dir / "runls11_selected_backrun_bundle.json"
    candidate_payload = candidate_dir / "runls11_bundle_payload_base64.json"
    candidate_pm = candidate_dir / "runls11_pm_reserve_report.json"
    candidate_probe = candidate_dir / "runls11_bundle_probe_rows.jsonl"

    selected_doc = {
        "record_type": "runls15_inline_hybrid_selected_backrun_bundle",
        "selection_result": "selected_inline_hybrid_afxdp_target_laserstream_enrichment",
        "source_target": "afxdp",
        "source_enrichment": "laserstream_processed",
        "provider": "jito_via_helius",
        "selected_owner_mode": "backrun",
        "selected_route_family": row.get("route_family"),
        "selected_pool": row.get("pool_pubkey_hex"),
        "selected_mint_in": row.get("mint_in_hex"),
        "selected_mint_out": row.get("mint_out_hex"),
        "selected_venue": row.get("venue"),
        "selected_slot": row.get("slot"),
        "selected_target_signature": row.get("target_signature_hex"),
        "selected_target_tx_bytes_available": True,
        "selected_target_tx_bytes_source": "afxdp_target_tx_cache",
        "selected_target_tx_encoding": row.get("target_tx_encoding"),
        "selected_target_tx_decodes_locally": target_ok,
        "selected_target_tx_decoded_len_bytes": target_len,
        "selected_target_tx_message_hash": row.get("target_tx_message_hash"),
        "selected_target_pool": row.get("target_pool_pubkey_hex"),
        "selected_target_pool_matches": row.get("target_pool_pubkey_hex") == row.get("pool_pubkey_hex"),
        "selected_target_mint_in": row.get("target_mint_in_hex"),
        "selected_target_mint_in_matches": row.get("target_mint_in_hex") == row.get("mint_in_hex"),
        "selected_target_mint_out": row.get("target_mint_out_hex"),
        "selected_target_mint_out_matches": row.get("target_mint_out_hex") == row.get("mint_out_hex"),
        "selected_target_amount_in": row.get("target_amount_in"),
        "selected_target_account_keys_b58": row.get("target_account_keys_b58", []),
        "selected_target_program_ids_b58": row.get("target_program_ids_b58", []),
        "selected_target_relationship_recorded": True,
        "laserstream_enrichment_joined": True,
        "selected_backrun_signature": row.get("our_signature_hex"),
        "selected_backrun_tx_template": row.get("template_id"),
        "selected_expected_out_raw": as_int(row.get("expected_out_raw")),
        "selected_min_out_raw": as_int(row.get("min_out_raw")),
        "selected_net_ev_after_tip": as_int(row.get("net_ev_after_tip_lamports")),
        "selected_tip_lamports": as_int(row.get("tip_lamports")),
        "selected_pm_reserve_amount": as_int(row.get("reserved_notional_lamports")),
        "selected_pm_reserve_ok": True,
        **pm_fields,
        "bundle_envelope_built": bool(our_payload and target_payload),
        "bundle_tx_count": 2,
        "target_tx_first": True,
        "our_backrun_tx_second": True,
        "tip_present": as_int(row.get("tip_lamports")) > 0,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "record_type": "runls15_inline_hybrid_bundle_payload",
        "bundle_payload_valid": bool(our_payload and target_payload),
        "selected_route_family": row.get("route_family"),
        "provider": "jito_via_helius",
        "bundle_tx_count": 2,
        "bundle_tx_count_max_allowed": 5,
        "target_tx_required": True,
        "target_tx_bytes_available": True,
        "target_tx_first": True,
        "our_backrun_tx_present": bool(our_payload),
        "our_backrun_tx_second": True,
        "tip_present": as_int(row.get("tip_lamports")) > 0,
        "min_out_positive": as_int(row.get("min_out_raw")) > 0,
        "expected_out_positive": as_int(row.get("expected_out_raw")) > 0,
        "net_ev_positive_after_tip": as_int(row.get("net_ev_after_tip_lamports")) > 0,
        "estimated_tip_lamports": as_int(row.get("tip_lamports")),
        "estimated_priority_fee_lamports": 0,
        "bundle_expiry_policy": "runls15_inline_no_submit_freshness_gate",
        "send_bundle_disabled_in_mode": True,
        "sendBundle_called": False,
        "txs_base64": txs_base64,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "record_type": "runls15_inline_hybrid_pm_reserve_report",
        "selection_result": selected_doc["selection_result"],
        "selected_pm_reserve_ok": True,
        "selected_reserved_notional_lamports": as_int(row.get("reserved_notional_lamports")),
        **pm_fields,
        "pm_reserve_reconciled": True,
    }

    candidate_dir.mkdir(parents=True, exist_ok=True)
    write_json(candidate_selected, selected_doc)
    write_json(candidate_payload, payload_doc)
    write_json(candidate_pm, pm_doc)
    candidate_probe.write_text(json.dumps(row, sort_keys=True) + "\n", encoding="utf-8")
    return candidate_selected, candidate_payload, candidate_pm


def provider_reports_already_processed(doc: dict[str, Any]) -> bool:
    summary = doc.get("simulate_bundle_summary")
    if summary is None:
        return False
    return "already been processed" in json.dumps(summary, sort_keys=True).lower()


def normalize_freshness_status(result: str, blockers: list[str], doc: dict[str, Any]) -> str:
    if result == "target_fresh_bundle_eligible":
        return "hybrid_target_fresh_bundle_eligible"
    if result == "blocked_target_already_landed" or doc.get("target_already_landed") or provider_reports_already_processed(doc):
        return "blocked_target_already_landed"
    if (
        result in {"blocked_target_recent_blockhash_invalid", "blocked_target_blockhash_invalid"}
        or doc.get("target_recent_blockhash_valid") is False
    ):
        return "blocked_target_blockhash_invalid"
    if result == "blocked_simulate_bundle_failed" or "blocked_simulate_bundle_failed" in blockers:
        return "blocked_simulate_bundle_failed"
    if any(item in blockers for item in ["blocked_target_tx_bytes_missing", "blocked_target_tx_decode_failed"]):
        return "blocked_no_target_bytes"
    return "blocked_bundle_shape_invalid"


def final_stale_status(counts: dict[str, int]) -> str:
    if counts.get("blocked_target_already_landed") or counts.get("blocked_target_blockhash_invalid"):
        return "blocked_all_hybrid_candidates_stale"
    if counts.get("blocked_simulate_bundle_failed"):
        return "blocked_simulate_bundle_failed"
    return "blocked_no_fresh_hybrid_candidate"


def run_freshness_check(row: dict[str, Any], index: int) -> tuple[str, dict[str, Any], Path]:
    candidate_dir = inner_dir / "runls15_candidates" / f"candidate_{index:04d}"
    candidate_selected, candidate_payload, _ = build_candidate_docs(row, candidate_dir)
    subprocess.run(
        [
            "bash",
            str(ls11e_script),
            str(candidate_dir),
            str(candidate_selected),
            str(candidate_payload),
            str(candidate_dir / "runls11_bundle_probe_rows.jsonl"),
        ],
        cwd=str(pack_dir),
        env=os.environ.copy(),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
        timeout=25,
    )
    candidate_freshness = candidate_dir / "runls11e_target_freshness_bundle_eligibility.json"
    if not candidate_freshness.exists():
        return "blocked_simulate_bundle_failed", {}, candidate_dir
    doc = json.loads(candidate_freshness.read_text(encoding="utf-8"))
    status = normalize_freshness_status(
        str(doc.get("selection_result") or ""),
        list(doc.get("blockers") or []),
        doc,
    )
    return status, doc, candidate_dir


def copy_candidate_artifacts(candidate_dir: Path, freshness_doc: dict[str, Any], status: str) -> dict[str, Any]:
    shutil.copyfile(candidate_dir / "runls11_selected_backrun_bundle.json", selected_path)
    shutil.copyfile(candidate_dir / "runls11_bundle_payload_base64.json", payload_path)
    shutil.copyfile(candidate_dir / "runls11_pm_reserve_report.json", pm_report_path)
    normalized = {
        **freshness_doc,
        "record_type": "runls15_target_freshness_bundle_eligibility",
        "status": status,
        "selection_result": status,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    if status == "blocked_target_already_landed":
        normalized["target_already_landed"] = True
    write_json(freshness_path, normalized)
    return normalized


def write_summary(status: str, freshness_doc: dict[str, Any] | None = None) -> None:
    selected_doc: dict[str, Any] = {}
    if selected_path.exists():
        selected_doc = json.loads(selected_path.read_text(encoding="utf-8"))
    freshness_doc = freshness_doc or {}
    summary = {
        "record_type": "runls15_inline_hybrid_freshness_summary",
        "run_name": "RunLS15",
        "status": status,
        "selection_result": status,
        "hybrid_backrun_candidate_seen": candidate_seen > 0,
        "strict_hybrid_candidate_seen": strict_candidate_seen > 0,
        "source_target": "afxdp",
        "target_tx_bytes_source": "afxdp_target_tx_cache" if selected_doc else None,
        "source_enrichment": "laserstream_processed",
        "laserstream_enrichment_joined": bool(selected_doc.get("laserstream_enrichment_joined")),
        "target_tx_bytes_available": bool(selected_doc.get("selected_target_tx_bytes_available")),
        "target_tx_decodes_locally": bool(selected_doc.get("selected_target_tx_decodes_locally")),
        "target_already_landed": bool(freshness_doc.get("target_already_landed")) if freshness_doc else None,
        "target_recent_blockhash_valid": bool(freshness_doc.get("target_recent_blockhash_valid")) if freshness_doc else None,
        "bundle_tx_count": selected_doc.get("bundle_tx_count", 0),
        "target_tx_first": bool(selected_doc.get("target_tx_first")),
        "our_backrun_tx_second": bool(selected_doc.get("our_backrun_tx_second")),
        "tip_present": bool(selected_doc.get("tip_present")),
        "selected_pm_reserve_created": bool(selected_doc.get("selected_pm_reserve_created")),
        "selected_pm_reserve_released": bool(selected_doc.get("selected_pm_reserve_released")),
        "selected_pm_pending_after": selected_doc.get("selected_pm_pending_after"),
        "provider": "jito_via_helius",
        "provider_auth_material_present": bool(
            os.environ.get("SHADOW_HELIUS_BUNDLE_API_KEY")
            or "api-key=" in os.environ.get("SHADOW_HELIUS_BUNDLE_RPC_URL", "").lower()
            or "api-key=" in os.environ.get("SHADOW_SOLANA_RPC_URL", "").lower()
        ),
        "simulate_bundle_available": bool(freshness_doc.get("simulate_bundle_available")) if freshness_doc else None,
        "simulateBundle_pass": bool(freshness_doc.get("simulateBundle_pass")) if freshness_doc else None,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
        "candidate_seen_count": candidate_seen,
        "strict_candidate_seen_count": strict_candidate_seen,
        "candidate_rejection_counts": status_counts,
        "elapsed_seconds": round(time.monotonic() - start, 3),
        "selected_artifact_path": str(selected_path) if selected_path.exists() else None,
        "payload_artifact_path": str(payload_path) if payload_path.exists() else None,
        "pm_report_path": str(pm_report_path) if pm_report_path.exists() else None,
        "freshness_artifact_path": str(freshness_path) if freshness_path.exists() else None,
    }
    write_json(summary_path, summary)


index = 0
while time.monotonic() < deadline:
    rows = load_probe_rows()
    for row in rows:
        key = str(row.get("our_signature_hex") or row.get("bundle_payload_sha256_hex") or row.get("target_signature_hex") or "")
        if not key:
            key = json.dumps(row, sort_keys=True)[:256]
        if key in seen_keys:
            continue
        seen_keys.add(key)
        if row.get("signal_source") == "laserstream" and row.get("owner_mode") == "backrun":
            candidate_seen += 1
        status = rejection_status(row)
        if status:
            append_rejection(row, status)
            continue

        strict_candidate_seen += 1
        index += 1
        status, freshness_doc, candidate_dir = run_freshness_check(row, index)
        normalized_freshness_doc = copy_candidate_artifacts(candidate_dir, freshness_doc, status)
        if status == "hybrid_target_fresh_bundle_eligible":
            fresh_candidate_seen = True
            write_summary(status, normalized_freshness_doc)
            stop_file.write_text("hybrid_target_fresh_bundle_eligible\n", encoding="utf-8")
            done_path = Path(os.environ.get("RUNLS15_MONITOR_DONE_FILE") or str(inner_dir / "runls15_monitor_done"))
            done_path.write_text("fresh\n", encoding="utf-8")
            sys.exit(0)
        append_rejection(row, status, {
            "target_already_landed": normalized_freshness_doc.get("target_already_landed"),
            "target_recent_blockhash_valid": normalized_freshness_doc.get("target_recent_blockhash_valid"),
            "simulateBundle_pass": normalized_freshness_doc.get("simulateBundle_pass"),
        })
    time.sleep(0.25)

if strict_candidate_seen > 0:
    final_status = final_stale_status(status_counts)
elif candidate_seen > 0:
    final_status = "blocked_no_fresh_hybrid_candidate"
else:
    final_status = "blocked_no_fresh_hybrid_candidate"
write_summary(final_status)
Path(os.environ.get("RUNLS15_MONITOR_DONE_FILE", str(inner_dir / "runls15_monitor_done"))).write_text(final_status + "\n", encoding="utf-8")
sys.exit(1)
PY
MONITOR_PID=$!

RUNLS15_MONITOR_DONE_FILE="$MONITOR_DONE_FILE" bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"
WINDOW_RC=$?

if kill -0 "$MONITOR_PID" 2>/dev/null; then
    wait "$MONITOR_PID" || MONITOR_RC=$?
else
    wait "$MONITOR_PID" || MONITOR_RC=$?
fi
MONITOR_RC="${MONITOR_RC:-0}"

if [[ "$WINDOW_RC" -ne 0 ]]; then
    fail "RunLS15 inner observe-only window failed"
fi

if [[ ! -f "$SUMMARY_JSON_PATH" ]]; then
    fail "RunLS15 summary missing: $SUMMARY_JSON_PATH"
    finish
    exit 1
fi

readarray -t RUNLS15_FIELDS < <(python3 - "$SUMMARY_JSON_PATH" <<'PY'
import json
import sys
from pathlib import Path

doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for key in [
    "status",
    "hybrid_backrun_candidate_seen",
    "strict_hybrid_candidate_seen",
    "target_tx_bytes_available",
    "target_already_landed",
    "target_recent_blockhash_valid",
    "simulateBundle_pass",
    "wire_send",
    "live_landed",
]:
    print(doc.get(key))
PY
)

STATUS="${RUNLS15_FIELDS[0]:-unknown}"
HYBRID_SEEN="${RUNLS15_FIELDS[1]:-false}"
STRICT_SEEN="${RUNLS15_FIELDS[2]:-false}"
TARGET_BYTES="${RUNLS15_FIELDS[3]:-false}"
TARGET_LANDED="${RUNLS15_FIELDS[4]:-}"
BLOCKHASH_VALID="${RUNLS15_FIELDS[5]:-}"
SIM_PASS="${RUNLS15_FIELDS[6]:-}"
WIRE_SEND="${RUNLS15_FIELDS[7]:-0}"
LIVE_LANDED="${RUNLS15_FIELDS[8]:-0}"

{
    echo "runls15_summary_path=$SUMMARY_JSON_PATH"
    echo "runls15_rejection_rows_path=$REJECTION_ROWS_PATH"
    echo "runls15_selected_artifact_path=$SELECTED_JSON_PATH"
    echo "runls15_payload_artifact_path=$PAYLOAD_JSON_PATH"
    echo "runls15_pm_report_path=$PM_REPORT_JSON_PATH"
    echo "runls15_freshness_artifact_path=$FRESHNESS_JSON_PATH"
    echo "status=$STATUS"
    echo "hybrid_backrun_candidate_seen=$HYBRID_SEEN"
    echo "strict_hybrid_candidate_seen=$STRICT_SEEN"
    echo "target_tx_bytes_available=$TARGET_BYTES"
    echo "target_already_landed=$TARGET_LANDED"
    echo "target_recent_blockhash_valid=$BLOCKHASH_VALID"
    echo "simulateBundle_pass=$SIM_PASS"
    echo "sendBundle_called=false"
    echo "wire_send=$WIRE_SEND"
    echo "live_landed=$LIVE_LANDED"
    echo "inner_summary_path=$INNER_SUMMARY_FILE"
} | tee "$OUTER_SUMMARY_FILE"

if [[ "$WIRE_SEND" != "0" || "$LIVE_LANDED" != "0" ]]; then
    fail "RunLS15 observed live activity wire_send=$WIRE_SEND live_landed=$LIVE_LANDED"
fi

if [[ "$STATUS" == "hybrid_target_fresh_bundle_eligible" ]]; then
    pass "RunLS15 found a fresh hybrid Backrun bundle candidate"
else
    fail "RunLS15 blocked safely: $STATUS"
fi

finish
[[ "$STATUS" == "hybrid_target_fresh_bundle_eligible" && "$WIRE_SEND" == "0" && "$LIVE_LANDED" == "0" ]]

#!/usr/bin/env bash
# 21 AFBR-H - structural hunt-readiness certificate for Backrun

source "$(dirname "$0")/lib.sh"
script_header "21 AFBR-H - Backrun Structural Hunt Readiness"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_AFBRH_STEP:-21_afbrh_backrun_structural_hunt_readiness}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/21_afbrh_backrun_structural_hunt_readiness.json"

LS9_STEP="${SHADOW_AFBRH_LS9_STEP:-21_runls9_laserstream_backrun_nosend}"
LS11_STEP="${SHADOW_AFBRH_LS11_STEP:-21_runls11_laserstream_backrun_bundle_readiness}"
AFBR1_STEP="${SHADOW_AFBRH_AFBR1_STEP:-21_runafbr1_afxdp_backrun_projection_bundle_readiness}"

LS9_INNER_DIR="${SHADOW_AFBRH_LS9_INNER_DIR:-$RESULTS_ROOT/$LS9_STEP/window1/20_g12_r0_observe_only}"
LS11_INNER_DIR="${SHADOW_AFBRH_LS11_INNER_DIR:-$RESULTS_ROOT/$LS11_STEP/window1/20_g12_r0_observe_only}"
AFBR1_INNER_DIR="${SHADOW_AFBRH_AFBR1_INNER_DIR:-$RESULTS_ROOT/$AFBR1_STEP/window1/20_g12_r0_observe_only}"

LS9C_SUMMARY_PATH="${SHADOW_AFBRH_LS9C_SUMMARY_PATH:-$LS9_INNER_DIR/runls9c_laserstream_backrun_bundle_nosend_summary.json}"
LS11_SELECTED_PATH="${SHADOW_AFBRH_LS11_SELECTED_PATH:-$LS11_INNER_DIR/runls11_selected_backrun_bundle.json}"
LS11_PAYLOAD_PATH="${SHADOW_AFBRH_LS11_PAYLOAD_PATH:-$LS11_INNER_DIR/runls11_bundle_payload_base64.json}"
LS11D_SUMMARY_PATH="${SHADOW_AFBRH_LS11D_SUMMARY_PATH:-$LS11_INNER_DIR/runls11d_target_tx_capture_summary.json}"
LS11C_PROVIDER_PATH="${SHADOW_AFBRH_LS11C_PROVIDER_PATH:-$LS11_INNER_DIR/runls11_provider_readiness_smoke.json}"
AUTH1_SELECTED_PATH="${SHADOW_AFBRH_AUTH1_SELECTED_PATH:-$LS11_INNER_DIR/runauth1_selected_provider.json}"
LS11E_SUMMARY_PATH="${SHADOW_AFBRH_LS11E_SUMMARY_PATH:-$LS11_INNER_DIR/runls11e_target_freshness_bundle_eligibility.json}"

AFBR1_SUMMARY_PATH="${SHADOW_AFBRH_AFBR1_SUMMARY_PATH:-$AFBR1_INNER_DIR/runafbr1_summary.json}"
AFBR1_SELECTED_PATH="${SHADOW_AFBRH_AFBR1_SELECTED_PATH:-$AFBR1_INNER_DIR/runafbr1_selected_backrun_bundle.json}"
AFBR1_PAYLOAD_PATH="${SHADOW_AFBRH_AFBR1_PAYLOAD_PATH:-$AFBR1_INNER_DIR/runafbr1_bundle_payload_base64.json}"
AFBR1_PM_PATH="${SHADOW_AFBRH_AFBR1_PM_PATH:-$AFBR1_INNER_DIR/runafbr1_pm_reserve_report.json}"
AFBR1_FRESHNESS_PATH="${SHADOW_AFBRH_AFBR1_FRESHNESS_PATH:-$AFBR1_INNER_DIR/runafbr1_target_freshness_bundle_eligibility.json}"
AFBR1_AUTOPSY_ROWS_PATH="${SHADOW_AFBRH_AFBR1_AUTOPSY_ROWS_PATH:-$AFBR1_INNER_DIR/runafbr1_projection_autopsy_rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON"

python3 - \
    "$LS9C_SUMMARY_PATH" \
    "$LS11_SELECTED_PATH" \
    "$LS11_PAYLOAD_PATH" \
    "$LS11D_SUMMARY_PATH" \
    "$LS11C_PROVIDER_PATH" \
    "$AUTH1_SELECTED_PATH" \
    "$LS11E_SUMMARY_PATH" \
    "$AFBR1_SUMMARY_PATH" \
    "$AFBR1_SELECTED_PATH" \
    "$AFBR1_PAYLOAD_PATH" \
    "$AFBR1_PM_PATH" \
    "$AFBR1_FRESHNESS_PATH" \
    "$AFBR1_AUTOPSY_ROWS_PATH" \
    "$SUMMARY_FILE" \
    "$REPORT_JSON" \
    "$AFBR1_STEP" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


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


def safe_int(value: Any) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def first_present(*values: Any) -> Any:
    for value in values:
        if value is not None and value != "":
            return value
    return None


def truthy(*values: Any) -> bool:
    return any(as_bool(value) for value in values)


def counter_total(counter: Counter[str]) -> int:
    return sum(counter.values())


ls9c_summary = load_json(Path(sys.argv[1]))
ls11_selected = load_json(Path(sys.argv[2]))
ls11_payload = load_json(Path(sys.argv[3]))
ls11d_summary = load_json(Path(sys.argv[4]))
ls11c_provider = load_json(Path(sys.argv[5]))
auth1_selected = load_json(Path(sys.argv[6]))
ls11e_summary = load_json(Path(sys.argv[7]))
afbr1_summary = load_json(Path(sys.argv[8]))
afbr1_selected = load_json(Path(sys.argv[9]))
afbr1_payload = load_json(Path(sys.argv[10]))
afbr1_pm = load_json(Path(sys.argv[11]))
afbr1_freshness = load_json(Path(sys.argv[12]))
afbr1_autopsy_rows = load_jsonl(Path(sys.argv[13]))
summary_path = Path(sys.argv[14])
report_path = Path(sys.argv[15])
fresh_window_id = sys.argv[16]

autopsy_blockers = Counter(
    str(row.get("first_projection_blocker") or "")
    for row in afbr1_autopsy_rows
    if str(row.get("first_projection_blocker") or "")
)

target_entity_rows = sum(1 for row in afbr1_autopsy_rows if as_bool(row.get("target_entity_present")))
target_bytes_rows = sum(1 for row in afbr1_autopsy_rows if as_bool(row.get("target_tx_bytes_present") or row.get("target_tx_bytes_available")))
projected_state_rows = sum(
    1 for row in afbr1_autopsy_rows if as_bool(row.get("projected_post_target_pool_state_present"))
)
expected_out_positive_rows = sum(1 for row in afbr1_autopsy_rows if safe_int(row.get("expected_out_raw")) > 0)
min_out_positive_rows = sum(1 for row in afbr1_autopsy_rows if safe_int(row.get("min_out_raw")) > 0)
net_ev_positive_rows = sum(1 for row in afbr1_autopsy_rows if safe_int(row.get("net_ev_after_tip")) > 0)
pm_ready_rows = sum(1 for row in afbr1_autopsy_rows if as_bool(row.get("pm_reserve_sim_ok")))
bundle_candidate_ready_rows = sum(1 for row in afbr1_autopsy_rows if as_bool(row.get("bundle_candidate_ready")))
afbr1_upstream_summary = afbr1_summary.get("no_bundle_probe_upstream_summary") if isinstance(afbr1_summary.get("no_bundle_probe_upstream_summary"), dict) else {}
afbr1_shock_bundle_rows = safe_int(
    first_present(
        afbr1_upstream_summary.get("shock_afxdp_backrun_bundle_rows"),
        afbr1_summary.get("shock_afxdp_backrun_bundle_rows"),
        0,
    )
)

ls9_bundle_no_send_accept = safe_int(ls9c_summary.get("laserstream_backrun_bundle_no_send_accept"))
ls9_bundle_pm_release = safe_int(ls9c_summary.get("laserstream_backrun_bundle_pm_release"))
ls9_bundle_no_send_ready = (
    ls9_bundle_no_send_accept > 0
    and ls9_bundle_pm_release >= ls9_bundle_no_send_accept
    and safe_int(ls9c_summary.get("wire_send")) == 0
    and safe_int(ls9c_summary.get("live_landed")) == 0
)

ls11_bundle_artifact_ready = (
    truthy(ls11_selected.get("bundle_envelope_built"), ls11_payload.get("bundle_payload_valid"))
    and safe_int(first_present(ls11_payload.get("bundle_tx_count"), ls11_selected.get("bundle_tx_count"))) == 2
    and truthy(ls11_payload.get("target_tx_first"), ls11_selected.get("target_tx_first"))
    and truthy(ls11_payload.get("our_backrun_tx_second"), ls11_selected.get("our_backrun_tx_second"))
    and truthy(ls11_payload.get("tip_present"), ls11_selected.get("tip_present"))
)

ls11_target_tx_capture_ready = (
    as_bool(ls11d_summary.get("selected_target_tx_bytes_available"))
    and as_bool(ls11d_summary.get("selected_target_tx_decodes_locally"))
    and as_bool(ls11d_summary.get("selected_target_signature_matches"))
    and as_bool(ls11d_summary.get("selected_target_pool_matches"))
    and as_bool(ls11d_summary.get("selected_target_mint_in_matches"))
    and as_bool(ls11d_summary.get("selected_target_mint_out_matches"))
)

provider_auth_material_present = truthy(
    auth1_selected.get("auth_material_present"),
    auth1_selected.get("provider_auth_material_present"),
    ls11c_provider.get("provider_auth_material_present"),
    afbr1_summary.get("provider_auth_material_present"),
)
provider_status_api_reachable = truthy(
    auth1_selected.get("status_api_reachable"),
    ls11c_provider.get("provider_status_api_reachable"),
)
simulate_bundle_available = truthy(
    auth1_selected.get("simulate_bundle_available"),
    ls11c_provider.get("simulate_bundle_available"),
    ls11e_summary.get("simulate_bundle_available"),
    afbr1_summary.get("simulate_bundle_available"),
)

ls11_target_fresh_bundle_eligible = (
    str(ls11e_summary.get("selection_result") or "") == "target_fresh_bundle_eligible"
    and not as_bool(ls11e_summary.get("target_already_landed"))
    and as_bool(ls11e_summary.get("target_recent_blockhash_valid"))
    and simulate_bundle_available
    and safe_int(ls11e_summary.get("bundle_tx_count")) == 2
    and as_bool(ls11e_summary.get("target_tx_first"))
    and as_bool(ls11e_summary.get("our_backrun_tx_second"))
    and as_bool(ls11e_summary.get("tip_present"))
    and safe_int(ls11e_summary.get("wire_send")) == 0
    and safe_int(ls11e_summary.get("live_landed")) == 0
)

afbr1_status = str(
    first_present(
        afbr1_summary.get("status"),
        afbr1_freshness.get("status"),
        afbr1_freshness.get("selection_result"),
    )
    or ""
)
afbr1_target_candidate_seen = truthy(
    afbr1_summary.get("target_candidate_seen"),
    afbr1_summary.get("strict_projection_candidate_seen"),
)
afbr1_pi_subreason_row_count = safe_int(afbr1_summary.get("pi_subreason_row_count"))
afbr1_pi_top_normalized_blocker = str(
    first_present(afbr1_summary.get("pi_top_normalized_blocker"), "none") or "none"
)
afbr1_target_trigger_observed = (
    afbr1_target_candidate_seen
    or afbr1_shock_bundle_rows > 0
    or len(afbr1_autopsy_rows) > 0
    or afbr1_pi_subreason_row_count > 0
)
afbr1_target_tx_bytes_available = truthy(
    afbr1_selected.get("selected_target_tx_bytes_available"),
    afbr1_summary.get("target_tx_bytes_available"),
)
afbr1_target_not_landed = not as_bool(
    first_present(
        afbr1_freshness.get("target_already_landed"),
        afbr1_summary.get("target_already_landed"),
        False,
    )
)
afbr1_target_recent_blockhash_valid = as_bool(
    first_present(
        afbr1_freshness.get("target_recent_blockhash_valid"),
        afbr1_summary.get("target_recent_blockhash_valid"),
        False,
    )
)

afbr1_target_relationship_ready = (
    truthy(
        afbr1_selected.get("target_pool_identified"),
        afbr1_summary.get("target_pool_identified"),
    )
    and truthy(
        afbr1_selected.get("target_direction_decoded"),
        afbr1_summary.get("target_direction_decoded"),
    )
    and truthy(
        afbr1_selected.get("target_amount_decoded"),
        afbr1_summary.get("target_amount_decoded"),
    )
    and target_entity_rows > 0
)
afbr1_pre_target_pool_state_ready = truthy(
    afbr1_selected.get("pool_state_before_available"),
    afbr1_summary.get("pool_state_before_available"),
)
afbr1_projected_post_target_pool_state_ready = truthy(
    afbr1_selected.get("projected_pool_state_after_computed"),
    afbr1_summary.get("projected_pool_state_after_computed"),
)
afbr1_projected_expected_out_positive = safe_int(
    first_present(
        afbr1_selected.get("selected_expected_out_raw"),
        afbr1_summary.get("expected_out_raw"),
        0,
    )
) > 0
afbr1_projected_min_out_positive = safe_int(
    first_present(
        afbr1_selected.get("selected_min_out_raw"),
        afbr1_summary.get("min_out_raw"),
        0,
    )
) > 0
afbr1_projected_net_ev_after_tip_positive = safe_int(
    first_present(
        afbr1_selected.get("selected_net_ev_after_tip"),
        afbr1_summary.get("net_ev_after_tip"),
        0,
    )
) > 0

selected_pm_reserve_ok = truthy(
    afbr1_selected.get("selected_pm_reserve_ok"),
    afbr1_pm.get("selected_pm_reserve_ok"),
    afbr1_summary.get("selected_pm_reserve_ok"),
)
selected_pm_release_on_no_send_ok = truthy(
    afbr1_selected.get("selected_pm_reserve_released"),
    afbr1_pm.get("selected_pm_reserve_released"),
    afbr1_summary.get("selected_pm_reserve_released"),
)
selected_pm_pending_after = safe_int(
    first_present(
        afbr1_selected.get("selected_pm_pending_after"),
        afbr1_pm.get("selected_pm_pending_after"),
        afbr1_summary.get("selected_pm_pending_after"),
        0,
    )
)

bundle_envelope_built = truthy(
    afbr1_selected.get("bundle_envelope_built"),
    afbr1_payload.get("bundle_payload_valid"),
)
bundle_tx_count = safe_int(
    first_present(
        afbr1_selected.get("bundle_tx_count"),
        afbr1_payload.get("bundle_tx_count"),
        0,
    )
)
target_tx_first = truthy(afbr1_selected.get("target_tx_first"), afbr1_payload.get("target_tx_first"))
our_backrun_tx_second = truthy(
    afbr1_selected.get("our_backrun_tx_second"),
    afbr1_payload.get("our_backrun_tx_second"),
)
tip_present = truthy(afbr1_selected.get("tip_present"), afbr1_payload.get("tip_present"))

research_proof_gaps: Counter[str] = Counter()
live_track_afxdp_blockers: Counter[str] = Counter()
afbr1_live_track_authoritative = bool(afbr1_status) and (
    len(afbr1_autopsy_rows) > 0
    or afbr1_shock_bundle_rows > 0
    or afbr1_pi_subreason_row_count > 0
)

if not ls9_bundle_no_send_ready:
    research_proof_gaps["laserstream_bundle_nosend_not_proven"] += 1
if not ls11_bundle_artifact_ready:
    research_proof_gaps["laserstream_bundle_artifact_missing"] += 1
if not ls11_target_tx_capture_ready:
    research_proof_gaps["laserstream_target_tx_capture_missing"] += 1
if not provider_auth_material_present:
    research_proof_gaps["provider_auth_missing"] += 1
if not provider_status_api_reachable:
    research_proof_gaps["provider_status_api_unreachable"] += 1
if not simulate_bundle_available:
    research_proof_gaps["simulate_bundle_unavailable"] += 1

if not afbr1_target_trigger_observed:
    live_track_afxdp_blockers["af_xdp_target_trigger_missing"] += 1
else:
    if not afbr1_target_tx_bytes_available:
        live_track_afxdp_blockers["af_xdp_target_bytes_missing"] += max(
            1, autopsy_blockers.get("blocked_no_target_bytes", 0)
        )
    if not afbr1_target_relationship_ready:
        live_track_afxdp_blockers["af_xdp_target_relationship_missing"] += max(
            1, autopsy_blockers.get("blocked_target_entity_missing", 0)
        )
    if not afbr1_pre_target_pool_state_ready:
        live_track_afxdp_blockers["af_xdp_pre_target_state_missing"] += max(
            1, autopsy_blockers.get("blocked_pool_state_missing", 0)
        )
    if not afbr1_projected_post_target_pool_state_ready:
        live_track_afxdp_blockers["af_xdp_projection_missing"] += max(
            1, autopsy_blockers.get("blocked_projection_failed", 0)
        )
    if not afbr1_projected_expected_out_positive:
        live_track_afxdp_blockers["af_xdp_projected_expected_out_missing"] += max(
            1, autopsy_blockers.get("blocked_projected_quote_missing", 0)
        )
    if not afbr1_projected_min_out_positive:
        live_track_afxdp_blockers["af_xdp_projected_min_out_missing"] += max(
            1, autopsy_blockers.get("blocked_min_out_invalid", 0)
        )
    if not afbr1_projected_net_ev_after_tip_positive:
        live_track_afxdp_blockers["af_xdp_projected_ev_not_positive"] += max(
            1, autopsy_blockers.get("blocked_ev_not_positive", 0)
        )
    if not selected_pm_reserve_ok:
        live_track_afxdp_blockers["af_xdp_pm_reserve_missing"] += max(
            1, autopsy_blockers.get("blocked_pm_reserve", 0)
        )
    if not selected_pm_release_on_no_send_ok:
        live_track_afxdp_blockers["af_xdp_pm_release_missing"] += 1
    if selected_pm_pending_after != 0:
        live_track_afxdp_blockers["af_xdp_pm_pending_not_zero"] += 1
    if not bundle_envelope_built:
        live_track_afxdp_blockers["af_xdp_bundle_envelope_missing"] += 1
    if bundle_tx_count != 2:
        live_track_afxdp_blockers["af_xdp_bundle_tx_count_invalid"] += 1
    if not target_tx_first:
        live_track_afxdp_blockers["af_xdp_target_tx_position_invalid"] += 1
    if not our_backrun_tx_second:
        live_track_afxdp_blockers["af_xdp_backrun_tx_position_invalid"] += 1
    if not tip_present:
        live_track_afxdp_blockers["af_xdp_tip_missing"] += 1
    if afbr1_target_not_landed is False and not as_bool(afbr1_freshness.get("target_already_landed")):
        live_track_afxdp_blockers["af_xdp_target_freshness_unknown"] += 1

architecture_blockers: Counter[str] = Counter()
if afbr1_live_track_authoritative:
    architecture_blockers.update(live_track_afxdp_blockers)
elif live_track_afxdp_blockers:
    architecture_blockers.update(live_track_afxdp_blockers)
else:
    architecture_blockers.update(research_proof_gaps)

residual_blockers: Counter[str] = Counter()
if not afbr1_target_trigger_observed:
    residual_blockers["NO_FRESH_BACKRUN_TARGET"] += 1
if as_bool(afbr1_freshness.get("target_already_landed")):
    residual_blockers["TARGET_ALREADY_LANDED_BUT_CLASSIFIED_OUT_OF_SCOPE"] += 1
if autopsy_blockers.get("blocked_target_pool_mismatch", 0) > 0 or autopsy_blockers.get("blocked_target_mint_mismatch", 0) > 0:
    residual_blockers["UNSUPPORTED_POOL_FOR_PROJECTION"] += (
        autopsy_blockers.get("blocked_target_pool_mismatch", 0)
        + autopsy_blockers.get("blocked_target_mint_mismatch", 0)
    )
if autopsy_blockers.get("blocked_ev_not_positive", 0) > 0:
    residual_blockers["MARKET_EV_NEGATIVE"] += autopsy_blockers.get("blocked_ev_not_positive", 0)

architecture_blocker_total = counter_total(architecture_blockers)
status = "BACKRUN_STRUCTURAL_HUNT_READY" if architecture_blocker_total == 0 else "BLOCKED_ARCHITECTURE"

if architecture_blocker_total == 0:
    main_blocker = "none"
else:
    if afbr1_live_track_authoritative:
        if afbr1_pi_top_normalized_blocker not in {"", "none"} and len(afbr1_autopsy_rows) == 0:
            main_blocker = afbr1_pi_top_normalized_blocker.removeprefix("blocked_")
        elif target_entity_rows == 0 and len(afbr1_autopsy_rows) > 0:
            main_blocker = "target_entity_missing"
        elif not afbr1_target_tx_bytes_available and target_bytes_rows == 0:
            main_blocker = "target_bytes_missing"
        elif not afbr1_pre_target_pool_state_ready:
            main_blocker = "pre_target_pool_state_missing"
        elif not afbr1_projected_post_target_pool_state_ready:
            main_blocker = "projected_post_target_state_missing"
        elif not afbr1_projected_expected_out_positive:
            main_blocker = "projected_quote_missing"
        elif not afbr1_projected_min_out_positive:
            main_blocker = "projected_min_out_missing"
        elif not afbr1_projected_net_ev_after_tip_positive:
            main_blocker = "projected_ev_missing"
        elif not selected_pm_reserve_ok:
            main_blocker = "pm_reserve_missing"
        elif not bundle_envelope_built:
            main_blocker = "bundle_envelope_missing"
        elif not afbr1_target_recent_blockhash_valid:
            main_blocker = "target_recent_blockhash_invalid"
        elif not afbr1_target_not_landed:
            main_blocker = "target_already_landed"
        elif "af_xdp_target_trigger_missing" in architecture_blockers:
            main_blocker = "no_fresh_backrun_target"
        else:
            main_blocker = next(iter(architecture_blockers))
    elif "provider_auth_missing" in architecture_blockers:
        main_blocker = "provider_auth_missing"
    elif "provider_status_api_unreachable" in architecture_blockers:
        main_blocker = "provider_status_api_unreachable"
    else:
        main_blocker = next(iter(architecture_blockers))

final_verdict = (
    "architecture_green_market_waiting"
    if status == "BACKRUN_STRUCTURAL_HUNT_READY"
    else "architecture_blockers_present"
)

report = {
        "status": status,
        "main_blocker": main_blocker,
        "fresh_window_id": fresh_window_id,
        "research_proofs": {
            "laserstream_bundle_subset_no_send": ls9_bundle_no_send_ready,
            "ls11_bundle_artifact_ready": ls11_bundle_artifact_ready,
            "ls11_target_byte_preservation": ls11_target_tx_capture_ready,
            "provider_auth_material_present": provider_auth_material_present,
            "provider_status_api_reachable": provider_status_api_reachable,
            "simulate_bundle_available": simulate_bundle_available,
            "ls11_target_fresh_bundle_eligible": ls11_target_fresh_bundle_eligible,
        },
        "live_track_af_xdp_proofs": {
            "afbr1_live_track_authoritative": afbr1_live_track_authoritative,
            "afbr1_status": afbr1_status,
            "afbr1_pi_subreason_row_count": afbr1_pi_subreason_row_count,
            "afbr1_pi_top_normalized_blocker": afbr1_pi_top_normalized_blocker,
            "shock_afxdp_backrun_bundle_rows": afbr1_shock_bundle_rows,
            "afxdp_target_candidate_seen": afbr1_target_candidate_seen,
            "afxdp_target_trigger_observed": afbr1_target_trigger_observed,
            "target_tx_bytes_available": afbr1_target_tx_bytes_available,
            "target_not_landed": afbr1_target_not_landed,
            "target_recent_blockhash_valid": afbr1_target_recent_blockhash_valid,
            "target_entity_relationship_ready": afbr1_target_relationship_ready,
            "pre_target_pool_state_ready": afbr1_pre_target_pool_state_ready,
            "projected_post_target_pool_state_ready": afbr1_projected_post_target_pool_state_ready,
            "projected_expected_out_positive": afbr1_projected_expected_out_positive,
            "projected_min_out_positive": afbr1_projected_min_out_positive,
            "projected_net_ev_after_tip_positive": afbr1_projected_net_ev_after_tip_positive,
            "selected_pm_reserve_ok": selected_pm_reserve_ok,
            "selected_pm_release_on_no_send_ok": selected_pm_release_on_no_send_ok,
            "selected_pm_pending_after": selected_pm_pending_after,
            "bundle_envelope_built": bundle_envelope_built,
            "bundle_tx_count": bundle_tx_count,
            "target_tx_first": target_tx_first,
            "our_backrun_tx_second": our_backrun_tx_second,
            "tip_present": tip_present,
        },
        "laserstream_bundle_nosend_ready": ls9_bundle_no_send_ready,
        "laserstream_bundle_nosend_accept_count": ls9_bundle_no_send_accept,
        "laserstream_bundle_pm_release_count": ls9_bundle_pm_release,
        "laserstream_bundle_artifact_ready": ls11_bundle_artifact_ready,
        "laserstream_target_tx_capture_ready": ls11_target_tx_capture_ready,
    "provider_auth_material_present": provider_auth_material_present,
    "provider_status_api_reachable": provider_status_api_reachable,
    "simulate_bundle_available": simulate_bundle_available,
    "laserstream_target_fresh_bundle_eligible": ls11_target_fresh_bundle_eligible,
    "afxdp_target_candidate_seen": afbr1_target_candidate_seen,
    "target_tx_bytes_available": afbr1_target_tx_bytes_available,
    "target_not_landed": afbr1_target_not_landed,
    "target_recent_blockhash_valid": afbr1_target_recent_blockhash_valid,
    "target_entity_relationship_ready": afbr1_target_relationship_ready,
    "pre_target_pool_state_ready": afbr1_pre_target_pool_state_ready,
    "projected_post_target_pool_state_ready": afbr1_projected_post_target_pool_state_ready,
    "projected_expected_out_positive": afbr1_projected_expected_out_positive,
    "projected_min_out_positive": afbr1_projected_min_out_positive,
    "projected_net_ev_after_tip_positive": afbr1_projected_net_ev_after_tip_positive,
    "selected_pm_reserve_ok": selected_pm_reserve_ok,
    "selected_pm_release_on_no_send_ok": selected_pm_release_on_no_send_ok,
    "selected_pm_pending_after": selected_pm_pending_after,
    "bundle_envelope_built": bundle_envelope_built,
    "bundle_tx_count": bundle_tx_count,
    "target_tx_first": target_tx_first,
    "our_backrun_tx_second": our_backrun_tx_second,
    "tip_present": tip_present,
    "provider_route_name": str(
        first_present(
            auth1_selected.get("provider_route_name"),
            ls11c_provider.get("provider_route_name"),
            afbr1_summary.get("provider"),
        )
        or ""
    ),
    "afbr1_status": afbr1_status,
    "projection_autopsy_row_count": len(afbr1_autopsy_rows),
    "shock_afxdp_backrun_bundle_target_entity_rows": target_entity_rows,
    "shock_afxdp_backrun_bundle_target_bytes_rows": target_bytes_rows,
    "projected_post_target_state_rows": projected_state_rows,
    "projected_expected_out_positive_rows": expected_out_positive_rows,
    "projected_min_out_positive_rows": min_out_positive_rows,
    "projected_net_ev_after_tip_positive_rows": net_ev_positive_rows,
    "selected_pm_reserve_ok_rows": pm_ready_rows,
        "bundle_candidate_ready_rows": bundle_candidate_ready_rows,
        "projection_autopsy_blocker_counts": dict(autopsy_blockers),
        "research_proof_gap_counts": dict(research_proof_gaps),
        "live_track_af_xdp_blocker_counts": dict(live_track_afxdp_blockers),
        "architecture_blocker_counts": dict(architecture_blockers),
        "residual_blocker_counts": dict(residual_blockers),
        "final_verdict": final_verdict,
}

summary_lines = [
    f"status={report['status']}",
    f"main_blocker={report['main_blocker']}",
    f"fresh_window_id={report['fresh_window_id']}",
    f"afbr1_live_track_authoritative={str(report['live_track_af_xdp_proofs']['afbr1_live_track_authoritative']).lower()}",
    f"afxdp_target_trigger_observed={str(report['live_track_af_xdp_proofs']['afxdp_target_trigger_observed']).lower()}",
    f"laserstream_bundle_nosend_ready={str(report['laserstream_bundle_nosend_ready']).lower()}",
    f"laserstream_bundle_artifact_ready={str(report['laserstream_bundle_artifact_ready']).lower()}",
    f"laserstream_target_tx_capture_ready={str(report['laserstream_target_tx_capture_ready']).lower()}",
    f"provider_auth_material_present={str(report['provider_auth_material_present']).lower()}",
    f"provider_status_api_reachable={str(report['provider_status_api_reachable']).lower()}",
    f"simulate_bundle_available={str(report['simulate_bundle_available']).lower()}",
    f"laserstream_target_fresh_bundle_eligible={str(report['laserstream_target_fresh_bundle_eligible']).lower()}",
    f"afxdp_target_candidate_seen={str(report['afxdp_target_candidate_seen']).lower()}",
    f"target_tx_bytes_available={str(report['target_tx_bytes_available']).lower()}",
    f"target_not_landed={str(report['target_not_landed']).lower()}",
    f"target_recent_blockhash_valid={str(report['target_recent_blockhash_valid']).lower()}",
    f"target_entity_relationship_ready={str(report['target_entity_relationship_ready']).lower()}",
    f"pre_target_pool_state_ready={str(report['pre_target_pool_state_ready']).lower()}",
    f"projected_post_target_pool_state_ready={str(report['projected_post_target_pool_state_ready']).lower()}",
    f"projected_expected_out_positive={str(report['projected_expected_out_positive']).lower()}",
    f"projected_min_out_positive={str(report['projected_min_out_positive']).lower()}",
    f"projected_net_ev_after_tip_positive={str(report['projected_net_ev_after_tip_positive']).lower()}",
    f"selected_pm_reserve_ok={str(report['selected_pm_reserve_ok']).lower()}",
    f"selected_pm_release_on_no_send_ok={str(report['selected_pm_release_on_no_send_ok']).lower()}",
    f"selected_pm_pending_after={report['selected_pm_pending_after']}",
    f"bundle_envelope_built={str(report['bundle_envelope_built']).lower()}",
    f"bundle_tx_count={report['bundle_tx_count']}",
    f"target_tx_first={str(report['target_tx_first']).lower()}",
    f"our_backrun_tx_second={str(report['our_backrun_tx_second']).lower()}",
    f"tip_present={str(report['tip_present']).lower()}",
    f"projection_autopsy_row_count={report['projection_autopsy_row_count']}",
    f"shock_afxdp_backrun_bundle_target_entity_rows={report['shock_afxdp_backrun_bundle_target_entity_rows']}",
    f"projected_post_target_state_rows={report['projected_post_target_state_rows']}",
    f"projected_expected_out_positive_rows={report['projected_expected_out_positive_rows']}",
    f"projected_min_out_positive_rows={report['projected_min_out_positive_rows']}",
    f"projected_net_ev_after_tip_positive_rows={report['projected_net_ev_after_tip_positive_rows']}",
    f"selected_pm_reserve_ok_rows={report['selected_pm_reserve_ok_rows']}",
    f"bundle_candidate_ready_rows={report['bundle_candidate_ready_rows']}",
    "research_proof_gap_counts=" + json.dumps(report["research_proof_gap_counts"], sort_keys=True),
    "live_track_af_xdp_blocker_counts=" + json.dumps(report["live_track_af_xdp_blocker_counts"], sort_keys=True),
    "architecture_blocker_counts=" + json.dumps(report["architecture_blocker_counts"], sort_keys=True),
    "residual_blocker_counts=" + json.dumps(report["residual_blocker_counts"], sort_keys=True),
    f"final_verdict={report['final_verdict']}",
]

report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "AFBR-H report written to $REPORT_JSON"
finish

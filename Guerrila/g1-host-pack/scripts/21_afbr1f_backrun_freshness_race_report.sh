#!/usr/bin/env bash
# 21_afbr1f_backrun_freshness_race_report.sh — per-candidate freshness timing report for AFBR1

source "$(dirname "$0")/lib.sh"
script_header "21F — AFBR1 Backrun Freshness Race Report"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INNER_STEP_DIR="${1:-${SHADOW_RUNAFBR1_INNER_DIR:-$PACK_DIR/results/21_runafbr1_afxdp_backrun_projection_bundle_readiness/window1/20_g12_r0_observe_only}}"
SUMMARY_JSON_PATH="$INNER_STEP_DIR/runafbr1_summary.json"
AUTOPSY_ROWS_PATH="$INNER_STEP_DIR/runafbr1_projection_autopsy_rows.jsonl"
FRESHNESS_JSON_PATH="$INNER_STEP_DIR/runafbr1_target_freshness_bundle_eligibility.json"
SELECTED_JSON_PATH="$INNER_STEP_DIR/runafbr1_selected_backrun_bundle.json"
CANDIDATE_DIR="$INNER_STEP_DIR/runafbr1_candidates"
OUT_JSON_PATH="$INNER_STEP_DIR/runafbr1f_backrun_freshness_race_report.json"
OUT_ROWS_PATH="$INNER_STEP_DIR/runafbr1f_backrun_freshness_race_rows.jsonl"
OUT_SUMMARY_PATH="$INNER_STEP_DIR/runafbr1f_backrun_freshness_race_report.txt"

if [[ ! -f "$SUMMARY_JSON_PATH" ]]; then
    fail "RunAFBR1 summary missing: $SUMMARY_JSON_PATH"
    finish
    exit 1
fi

python3 - "$INNER_STEP_DIR" "$SUMMARY_JSON_PATH" "$AUTOPSY_ROWS_PATH" "$FRESHNESS_JSON_PATH" \
    "$SELECTED_JSON_PATH" "$CANDIDATE_DIR" "$OUT_JSON_PATH" "$OUT_ROWS_PATH" "$OUT_SUMMARY_PATH" <<'PY'
from __future__ import annotations

import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Any

inner_dir = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
autopsy_rows_path = Path(sys.argv[3])
freshness_path = Path(sys.argv[4])
selected_path = Path(sys.argv[5])
candidate_dir = Path(sys.argv[6])
out_json_path = Path(sys.argv[7])
out_rows_path = Path(sys.argv[8])
out_summary_path = Path(sys.argv[9])
shock_research_path = inner_dir / "shock_research.jsonl"
source_arbiter_path = inner_dir / "laserstream_vs_afxdp_first_seen.jsonl"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def load_source_arbiter_rows(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("record_type") == "source_arbiter":
            rows.append(row)
    return rows


def as_int(value: Any, default: int = 0) -> int:
    try:
        if value is None or value == "":
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if value is None:
        return False
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def first_value(doc: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in doc and doc[key] not in (None, ""):
            return doc[key]
    return None


def first_text(doc: dict[str, Any], *keys: str) -> str | None:
    value = first_value(doc, *keys)
    if value in (None, ""):
        return None
    text = str(value)
    return text if text else None


def first_nonzero_int(doc: dict[str, Any], *keys: str) -> int | None:
    for key in keys:
        value = as_int(doc.get(key), 0)
        if value > 0:
            return value
    return None


def delta_us(start_ns: int | None, end_ns: int | None) -> int | None:
    if start_ns is None or end_ns is None or end_ns < start_ns:
        return None
    return (end_ns - start_ns) // 1_000


def ns_from_delta(base_ns: int | None, doc: dict[str, Any], *delta_keys: str) -> int | None:
    if base_ns is None:
        return None
    for key in delta_keys:
        value = first_value(doc, key)
        if value in (None, ""):
            continue
        try:
            delta = int(value)
        except (TypeError, ValueError):
            continue
        if delta < 0:
            continue
        return base_ns + (delta * 1_000)
    return None


def compact_json(value: Any) -> str | None:
    if value in (None, "", [], {}):
        return None
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def hex_signature_to_base58(signature_hex: str | None) -> str | None:
    if not signature_hex:
        return None
    try:
        data = bytes.fromhex(signature_hex)
    except ValueError:
        return None
    if not data:
        return None
    value = int.from_bytes(data, "big")
    encoded = ""
    while value:
        value, remainder = divmod(value, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    leading_zeroes = 0
    for byte in data:
        if byte == 0:
            leading_zeroes += 1
        else:
            break
    return ("1" * leading_zeroes) + (encoded or "1")


def timing_field_count(doc: dict[str, Any]) -> int:
    return sum(
        1
        for key in (
            "timestamp_basis",
            "afxdp_seen_ns",
            "first_seen_ns",
            "current_normalized_record_ns",
            "current_worker_seen_ns",
            "current_route_ready_ns",
            "current_quote_ready_ns",
            "current_pm_ready_ns",
            "current_candidate_emit_ns",
            "current_trade_ready_ns",
            "trade_ready_delta_us",
        )
        if first_value(doc, key) not in (None, "")
    )


def source_preference(doc: dict[str, Any]) -> int:
    source = first_text(doc, "signal_source", "source_target", "source")
    if source == "afxdp":
        return 2
    if source == "laserstream":
        return 1
    return 0


def percentile(values: list[int], pct: int) -> int | None:
    if not values:
        return None
    if pct <= 0:
        return min(values)
    if pct >= 100:
        return max(values)
    ordered = sorted(values)
    index = math.ceil((pct / 100) * len(ordered)) - 1
    index = max(0, min(index, len(ordered) - 1))
    return ordered[index]


def provider_reports_already_processed(doc: dict[str, Any]) -> bool:
    hay = json.dumps(doc, sort_keys=True).lower()
    return "already been processed" in hay or "already processed" in hay


def normalize_freshness_status(doc: dict[str, Any]) -> str:
    result = str(doc.get("selection_result") or "")
    blockers = list(doc.get("blockers") or [])
    if result == "target_fresh_bundle_eligible":
        return "afbr_target_fresh_bundle_eligible_no_send"
    if result == "blocked_target_already_landed" or doc.get("target_already_landed") or provider_reports_already_processed(doc):
        return "blocked_target_already_processed"
    if result in {"blocked_target_recent_blockhash_invalid", "blocked_target_blockhash_invalid"} or doc.get("target_recent_blockhash_valid") is False:
        return "blocked_blockhash_invalid"
    if result == "blocked_simulate_bundle_failed" or "blocked_simulate_bundle_failed" in blockers:
        return "blocked_provider_simulation_failed"
    if any(item in blockers for item in ("blocked_target_tx_bytes_missing", "blocked_target_tx_decode_failed")):
        return "blocked_no_target_bytes"
    return result or "blocked_bundle_shape_invalid"


def classify_provider_sim_failure(doc: dict[str, Any], row: dict[str, Any], autopsy: dict[str, Any]) -> str | None:
    if normalize_freshness_status(doc) != "blocked_provider_simulation_failed":
        return None
    blockers = list(doc.get("blockers") or [])
    err_msg = json.dumps(
        {
            "simulate_bundle_error_message": doc.get("simulate_bundle_error_message"),
            "simulate_bundle_summary": doc.get("simulate_bundle_summary"),
            "blockers": blockers,
        },
        sort_keys=True,
    ).lower()
    if provider_reports_already_processed(doc) or "already processed" in err_msg:
        return "provider_sim_failed_due_to_target_processed"
    if doc.get("target_recent_blockhash_valid") is False or "blockhash" in err_msg:
        return "provider_sim_failed_due_to_invalid_blockhash"
    if any(
        item in blockers
        for item in (
            "blocked_bundle_tx_count_gt_5",
            "blocked_target_not_first",
            "blocked_backrun_not_second",
            "blocked_tip_missing",
        )
    ):
        return "provider_sim_failed_due_to_bundle_shape"
    if autopsy.get("first_projection_blocker") not in (None, "", "none") or not as_bool(row.get("pm_reserve_ok")):
        return "provider_sim_failed_due_to_projection_mismatch"
    return "provider_sim_failed_due_to_provider_error"


summary = load_json(summary_path)
autopsy_rows = load_jsonl(autopsy_rows_path)
shock_rows = load_jsonl(shock_research_path)
source_arbiter_rows = load_source_arbiter_rows(source_arbiter_path)


def find_autopsy_row(selected_doc: dict[str, Any], probe_row: dict[str, Any]) -> dict[str, Any]:
    backrun_sig = first_text(selected_doc, "selected_backrun_signature") or first_text(probe_row, "our_signature_hex", "backrun_signature")
    target_sig = first_text(selected_doc, "selected_target_signature") or first_text(probe_row, "target_signature_hex", "target_signature")
    for preferred_source in ("shock_research", "runls11_bundle_probe"):
        for row in autopsy_rows:
            if preferred_source and first_text(row, "source_artifact") != preferred_source:
                continue
            if backrun_sig and first_text(row, "backrun_signature") == backrun_sig:
                return row
            if target_sig and first_text(row, "target_signature") == target_sig:
                return row
    return {}


def find_shock_row(selected_doc: dict[str, Any], probe_row: dict[str, Any]) -> dict[str, Any]:
    backrun_sig = first_text(selected_doc, "selected_backrun_signature") or first_text(probe_row, "our_signature_hex", "backrun_signature", "signature_hex")
    target_sig = first_text(selected_doc, "selected_target_signature") or first_text(probe_row, "target_signature_hex", "target_signature")
    target_pool = first_text(selected_doc, "selected_target_pool", "selected_pool") or first_text(probe_row, "target_pool_pubkey_hex", "target_pool", "pool_pubkey_hex", "pool")
    best_match: dict[str, Any] | None = None
    best_score: tuple[int, int, int] | None = None
    for row in shock_rows:
        if first_text(row, "owner_mode", "selected_owner") not in (None, "backrun"):
            owner_value = first_text(row, "owner_mode", "selected_owner")
            if owner_value != "backrun":
                continue
        score = 0
        if backrun_sig and first_text(row, "our_signature_hex", "backrun_signature", "signature_hex") == backrun_sig:
            score = max(score, 3)
        if target_sig and first_text(row, "target_signature_hex", "target_signature", "signature_hex") == target_sig:
            score = max(score, 2)
        if target_pool and first_text(row, "target_pool_pubkey_hex", "target_pool", "pool_pubkey_hex", "pool") == target_pool:
            row_amount = as_int(first_value(row, "target_amount_in", "target_amount_in_raw"), 0)
            probe_amount = as_int(first_value(probe_row, "target_amount_in", "target_amount_in_raw"), 0)
            if row_amount <= 0 or probe_amount <= 0 or row_amount == probe_amount:
                score = max(score, 1)
        if score <= 0:
            continue
        row_score = (score, source_preference(row), timing_field_count(row))
        if best_score is None or row_score > best_score:
            best_match = row
            best_score = row_score
    return best_match or {}


def find_source_arbiter_row(selected_doc: dict[str, Any], probe_row: dict[str, Any]) -> dict[str, Any]:
    backrun_sig_b58 = hex_signature_to_base58(
        first_text(selected_doc, "selected_backrun_signature")
        or first_text(probe_row, "our_signature_hex", "backrun_signature", "signature_hex")
    )
    target_sig_b58 = hex_signature_to_base58(
        first_text(selected_doc, "selected_target_signature")
        or first_text(probe_row, "target_signature_hex", "target_signature")
    )
    best_match: dict[str, Any] | None = None
    best_score: tuple[int, int] | None = None
    for row in source_arbiter_rows:
        if first_text(row, "owner_mode") not in (None, "backrun"):
            continue
        if first_text(row, "route", "route_family_label") not in (None, "jito_bundle", "harmonic_bundle"):
            continue
        signature_b58 = first_text(row, "signature")
        if not signature_b58:
            continue
        score = 0
        if backrun_sig_b58 and signature_b58 == backrun_sig_b58:
            score = max(score, 2)
        if target_sig_b58 and signature_b58 == target_sig_b58:
            score = max(score, 1)
        if score <= 0:
            continue
        row_score = (score, timing_field_count(row))
        if best_score is None or row_score > best_score:
            best_match = row
            best_score = row_score
    return best_match or {}


def classify_freshness_race(
    freshness_doc: dict[str, Any],
    final_blocker: str,
    bundle_ready_ns: int | None,
    projection_complete_ns: int | None,
    first_status_check_start_ns: int | None,
    provider_sim_start_ns: int | None,
    provider_sim_end_ns: int | None,
    provider_sim_failure: str | None,
) -> str:
    first_status = freshness_doc.get("target_signature_status_before_bundle")
    after_provider_status = freshness_doc.get("target_signature_status_after_provider_sim")
    if first_status is not None:
        if (
            projection_complete_ns is not None
            and first_status_check_start_ns is not None
            and first_status_check_start_ns <= projection_complete_ns
        ):
            return "PROCESSED_DURING_PROJECTION"
        return "ALREADY_PROCESSED_AT_FIRST_CHECK"
    if provider_sim_failure == "provider_sim_failed_due_to_target_processed" or (
        final_blocker == "blocked_provider_simulation_failed" and after_provider_status is not None
    ):
        return "PROCESSED_DURING_PROVIDER_SIM"
    if bundle_ready_ns is not None and final_blocker == "afbr_target_fresh_bundle_eligible_no_send":
        return "FRESH_AT_BUNDLE_READY"
    if (
        bundle_ready_ns is not None
        and freshness_doc.get("target_recent_blockhash_valid") is True
        and first_status is None
        and (provider_sim_start_ns is None or provider_sim_end_ns is None)
    ):
        return "FRESH_AT_BUNDLE_READY"
    return "STATUS_UNKNOWN"


candidate_payloads: list[tuple[str, dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]] = []
if candidate_dir.exists():
    for path in sorted(candidate_dir.glob("candidate_*")):
        probe_rows = load_jsonl(path / "runls11_bundle_probe_rows.jsonl")
        probe_row = probe_rows[0] if probe_rows else {}
        selected_doc = load_json(path / "runls11_selected_backrun_bundle.json")
        freshness_doc = load_json(path / "runls11e_target_freshness_bundle_eligibility.json")
        pm_doc = load_json(path / "runls11_pm_reserve_report.json")
        candidate_payloads.append((path.name, selected_doc, freshness_doc, pm_doc, probe_row))

if not candidate_payloads:
    candidate_payloads.append(
        (
            "candidate_0000",
            load_json(selected_path),
            load_json(freshness_path),
            {},
            {},
        )
    )

rows_out: list[dict[str, Any]] = []
final_blocker_counts: Counter[str] = Counter()
provider_sim_failure_counts: Counter[str] = Counter()

for candidate_name, selected_doc, freshness_doc, pm_doc, probe_row in candidate_payloads:
    autopsy = find_autopsy_row(selected_doc, probe_row)
    shock_row = find_shock_row(selected_doc, probe_row)
    source_arbiter_row = find_source_arbiter_row(selected_doc, probe_row)
    if shock_row:
        merged_probe_row = dict(shock_row)
        merged_probe_row.update({k: v for k, v in probe_row.items() if v not in (None, "")})
        probe_row = merged_probe_row
    if source_arbiter_row:
        merged_probe_row = dict(source_arbiter_row)
        merged_probe_row.update({k: v for k, v in probe_row.items() if v not in (None, "")})
        probe_row = merged_probe_row

    final_blocker = normalize_freshness_status(freshness_doc) if freshness_doc else (
        first_text(autopsy, "first_projection_blocker", "projection_blocker") or summary.get("status") or "unknown"
    )
    provider_sim_failure = classify_provider_sim_failure(freshness_doc, probe_row, autopsy)
    if provider_sim_failure:
        provider_sim_failure_counts[provider_sim_failure] += 1

    target_signature = first_text(selected_doc, "selected_target_signature") or first_text(probe_row, "target_signature_hex", "target_signature")
    target_pool = first_text(selected_doc, "selected_target_pool", "selected_pool") or first_text(probe_row, "target_pool_pubkey_hex", "target_pool", "pool_pubkey_hex", "pool")
    target_mint_in = first_text(selected_doc, "selected_target_mint_in", "selected_mint_in") or first_text(probe_row, "target_mint_in_hex", "target_mint_in", "mint_in_hex", "mint_in")
    target_mint_out = first_text(selected_doc, "selected_target_mint_out", "selected_mint_out") or first_text(probe_row, "target_mint_out_hex", "target_mint_out", "mint_out_hex", "mint_out")

    target_seen_ns = first_nonzero_int(
        probe_row,
        "afxdp_seen_ns",
        "first_seen_ns",
        "ingress_unix_ns",
    ) or first_nonzero_int(selected_doc, "selected_afxdp_seen_ns", "selected_first_seen_ns")
    target_tx_bytes_available_ns = target_seen_ns if as_bool(first_value(selected_doc, "selected_target_tx_bytes_available")) or as_bool(probe_row.get("target_tx_bytes_available")) else None
    target_entity_linked_ns = first_nonzero_int(probe_row, "current_worker_seen_ns", "current_normalized_record_ns") or first_nonzero_int(
        selected_doc,
        "selected_current_worker_seen_ns",
        "selected_current_normalized_record_ns",
    ) if (
        as_int(first_value(probe_row, "target_entity_id"), 0) > 0 or as_int(first_value(autopsy, "target_entity_id"), 0) > 0
    ) else None
    if target_entity_linked_ns is None and target_seen_ns is not None:
        target_entity_linked_ns = ns_from_delta(
            target_seen_ns,
            probe_row if probe_row else selected_doc,
            "worker_seen_delta_us",
            "normalized_record_delta_us",
            "decode_delta_us",
        )
    projection_complete_ns = first_nonzero_int(probe_row, "current_quote_ready_ns", "current_route_ready_ns") if (
        as_bool(first_value(selected_doc, "projected_pool_state_after_computed"))
        or as_bool(first_value(autopsy, "projected_post_target_pool_state_present"))
    ) else None
    if projection_complete_ns is None:
        projection_complete_ns = first_nonzero_int(
            selected_doc,
            "selected_current_quote_ready_ns",
            "selected_current_route_ready_ns",
        )
    if projection_complete_ns is None and target_seen_ns is not None:
        projection_complete_ns = ns_from_delta(
            target_seen_ns,
            probe_row if probe_row else selected_doc,
            "quote_ready_delta_us",
            "route_ready_delta_us",
        )
        if projection_complete_ns is None:
            projection_complete_ns = ns_from_delta(
                target_seen_ns,
                selected_doc,
                "selected_quote_ready_delta_us",
                "selected_route_ready_delta_us",
            )
    quote_complete_ns = first_nonzero_int(probe_row, "current_quote_ready_ns") if (
        as_int(first_value(selected_doc, "selected_expected_out_raw"), 0) > 0
        or as_int(first_value(probe_row, "expected_out_raw", "quote_expected_out_lamports"), 0) > 0
    ) else None
    if quote_complete_ns is None:
        quote_complete_ns = first_nonzero_int(selected_doc, "selected_current_quote_ready_ns")
    if quote_complete_ns is None and target_seen_ns is not None:
        quote_complete_ns = ns_from_delta(target_seen_ns, probe_row if probe_row else selected_doc, "quote_ready_delta_us")
        if quote_complete_ns is None:
            quote_complete_ns = ns_from_delta(target_seen_ns, selected_doc, "selected_quote_ready_delta_us")
    pm_reserve_complete_ns = first_nonzero_int(probe_row, "current_pm_ready_ns") if (
        as_bool(first_value(selected_doc, "selected_pm_reserve_ok"))
        or as_bool(first_value(probe_row, "pm_reserve_ok"))
    ) else None
    if pm_reserve_complete_ns is None:
        pm_reserve_complete_ns = first_nonzero_int(selected_doc, "selected_current_pm_ready_ns")
    if pm_reserve_complete_ns is None and target_seen_ns is not None:
        pm_reserve_complete_ns = ns_from_delta(target_seen_ns, probe_row if probe_row else selected_doc, "pm_ready_delta_us")
        if pm_reserve_complete_ns is None:
            pm_reserve_complete_ns = ns_from_delta(target_seen_ns, selected_doc, "selected_pm_ready_delta_us")
    bundle_ready_ns = first_nonzero_int(probe_row, "current_trade_ready_ns", "current_candidate_emit_ns") if (
        as_bool(first_value(selected_doc, "bundle_envelope_built"))
        or as_int(first_value(selected_doc, "bundle_tx_count"), 0) == 2
    ) else None
    if bundle_ready_ns is None:
        bundle_ready_ns = first_nonzero_int(
            selected_doc,
            "selected_current_trade_ready_ns",
            "selected_current_candidate_emit_ns",
        )
    if bundle_ready_ns is None and target_seen_ns is not None:
        bundle_ready_ns = ns_from_delta(
            target_seen_ns,
            probe_row if probe_row else selected_doc,
            "trade_ready_delta_us",
            "candidate_emit_delta_us",
        )
        if bundle_ready_ns is None:
            bundle_ready_ns = ns_from_delta(
                target_seen_ns,
                selected_doc,
                "selected_trade_ready_delta_us",
                "selected_candidate_emit_delta_us",
            )

    target_confirmation_status = first_value(freshness_doc, "target_signature_status_before_bundle")
    first_status_check_start_ns = first_nonzero_int(freshness_doc, "first_status_check_start_ns")
    first_status_check_end_ns = first_nonzero_int(freshness_doc, "first_status_check_end_ns")
    provider_sim_start_ns = first_nonzero_int(freshness_doc, "provider_sim_start_ns")
    provider_sim_end_ns = first_nonzero_int(freshness_doc, "provider_sim_end_ns")
    post_provider_status_check_start_ns = first_nonzero_int(freshness_doc, "post_provider_status_check_start_ns")
    post_provider_status_check_end_ns = first_nonzero_int(freshness_doc, "post_provider_status_check_end_ns")
    target_status_check_ns = first_status_check_start_ns or post_provider_status_check_start_ns
    target_status_at_first_check = freshness_doc.get("target_signature_status_before_bundle")
    target_status_after_bundle_ready = freshness_doc.get("target_signature_status_after_bundle_ready") or target_status_at_first_check
    target_status_after_provider_sim = freshness_doc.get("target_signature_status_after_provider_sim")
    freshness_race_classification = classify_freshness_race(
        freshness_doc,
        final_blocker,
        bundle_ready_ns,
        projection_complete_ns,
        first_status_check_start_ns,
        provider_sim_start_ns,
        provider_sim_end_ns,
        provider_sim_failure,
    )

    row_doc = {
        "record_type": "runafbr1f_freshness_race_row",
        "candidate_id": candidate_name,
        "timestamp_basis": first_text(probe_row, "timestamp_basis") or first_text(selected_doc, "selected_timestamp_basis") or "unknown",
        "target_signature": target_signature,
        "target_pool": target_pool,
        "target_mint_in": target_mint_in,
        "target_mint_out": target_mint_out,
        "target_seen_afxdp_unix_ns": target_seen_ns,
        "target_seen_afxdp_host_mono_ns": target_seen_ns,
        "target_seen_laserstream_host_mono_ns": first_nonzero_int(probe_row, "laserstream_seen_ns", "laserstream_host_mono_recv_ns"),
        "source_delta_us": first_value(probe_row, "laserstream_minus_afxdp_us"),
        "source_delta_basis": "laserstream_minus_afxdp",
        "source_matched_afxdp_laserstream": as_bool(first_value(probe_row, "matched_afxdp_laserstream")),
        "source_first_seen_source": first_text(probe_row, "first_seen_source"),
        "source_first_seen_ns": first_nonzero_int(probe_row, "first_seen_ns"),
        "target_tx_bytes_available_unix_ns": target_tx_bytes_available_ns,
        "target_entity_linked_unix_ns": target_entity_linked_ns,
        "projection_complete_unix_ns": projection_complete_ns,
        "quote_complete_unix_ns": quote_complete_ns,
        "pm_reserve_complete_unix_ns": pm_reserve_complete_ns,
        "bundle_envelope_built_unix_ns": bundle_ready_ns,
        "first_status_check_start_ns": first_status_check_start_ns,
        "first_status_check_end_ns": first_status_check_end_ns,
        "provider_sim_start_unix_ns": provider_sim_start_ns,
        "provider_sim_end_unix_ns": provider_sim_end_ns,
        "post_provider_status_check_start_ns": post_provider_status_check_start_ns,
        "post_provider_status_check_end_ns": post_provider_status_check_end_ns,
        "target_status_check_unix_ns": target_status_check_ns,
        "target_already_processed": final_blocker == "blocked_target_already_processed" or as_bool(freshness_doc.get("target_already_landed")),
        "target_status_at_first_check": target_status_at_first_check,
        "target_status_after_bundle_ready": target_status_after_bundle_ready,
        "target_status_after_provider_sim": target_status_after_provider_sim,
        "target_confirmation_status": target_confirmation_status,
        "target_landed_slot_if_known": target_status_at_first_check.get("slot") if isinstance(target_status_at_first_check, dict) else None,
        "local_current_slot_at_first_check": first_nonzero_int(freshness_doc, "local_current_slot_at_first_check"),
        "target_recent_blockhash_valid": freshness_doc.get("target_recent_blockhash_valid"),
        "total_target_seen_to_bundle_ready_us": delta_us(target_seen_ns, bundle_ready_ns),
        "target_seen_to_projection_us": delta_us(target_seen_ns, projection_complete_ns),
        "projection_to_bundle_us": delta_us(projection_complete_ns, bundle_ready_ns),
        "bundle_ready_to_first_status_check_us": delta_us(bundle_ready_ns, first_status_check_start_ns),
        "bundle_to_provider_sim_us": delta_us(bundle_ready_ns, provider_sim_start_ns),
        "provider_sim_latency_us": delta_us(provider_sim_start_ns, provider_sim_end_ns),
        "final_blocker": final_blocker,
        "provider_sim_failure_classification": provider_sim_failure,
        "freshness_race_classification": freshness_race_classification,
        "target_tx_bytes_available": as_bool(first_value(selected_doc, "selected_target_tx_bytes_available")) or as_bool(probe_row.get("target_tx_bytes_available")),
        "target_tx_bytes_source": first_text(selected_doc, "selected_target_tx_bytes_source") or first_text(probe_row, "target_tx_bytes_source"),
        "target_tx_decodes_locally": as_bool(first_value(selected_doc, "selected_target_tx_decodes_locally")),
        "target_entity_id": as_int(first_value(probe_row, "target_entity_id", "target_entity"), 0),
        "target_relationship_source": first_text(probe_row, "target_relationship_source"),
        "target_direction": first_text(probe_row, "target_direction"),
        "target_amount_in_raw": as_int(first_value(probe_row, "target_amount_in", "target_amount_in_raw"), 0),
        "pre_target_pool_state_available": as_bool(first_value(selected_doc, "pool_state_before_available")) or as_bool(first_value(autopsy, "pre_target_pool_state_present")),
        "projected_post_target_pool_state_available": as_bool(first_value(selected_doc, "projected_pool_state_after_computed")) or as_bool(first_value(autopsy, "projected_post_target_pool_state_present")),
        "projected_expected_out_raw": as_int(first_value(selected_doc, "selected_expected_out_raw", "expected_out_raw"), 0),
        "projected_min_out_raw": as_int(first_value(selected_doc, "selected_min_out_raw", "min_out_raw"), 0),
        "projected_net_ev_after_tip_raw": as_int(first_value(selected_doc, "selected_net_ev_after_tip", "net_ev_after_tip"), 0),
        "selected_pm_reserve_ok": as_bool(first_value(selected_doc, "selected_pm_reserve_ok")),
        "selected_pm_reserve_created": as_bool(first_value(selected_doc, "selected_pm_reserve_created")),
        "selected_pm_reserve_released": as_bool(first_value(selected_doc, "selected_pm_reserve_released")),
        "selected_pm_pending_after": as_int(first_value(selected_doc, "selected_pm_pending_after"), 0),
        "bundle_envelope_built": as_bool(first_value(selected_doc, "bundle_envelope_built")),
        "bundle_tx_count": as_int(first_value(selected_doc, "bundle_tx_count"), 0),
        "target_tx_first": as_bool(first_value(selected_doc, "target_tx_first")),
        "our_backrun_tx_second": as_bool(first_value(selected_doc, "our_backrun_tx_second")),
        "tip_present": as_bool(first_value(selected_doc, "tip_present")),
        "simulate_bundle_available": freshness_doc.get("simulate_bundle_available"),
        "simulate_bundle_pass": freshness_doc.get("simulateBundle_pass"),
        "simulate_bundle_probe": freshness_doc.get("simulate_bundle_probe"),
        "simulate_bundle_error_code": freshness_doc.get("simulate_bundle_error_code"),
        "simulate_bundle_error_message": freshness_doc.get("simulate_bundle_error_message"),
        "freshness_selection_result": freshness_doc.get("selection_result"),
        "freshness_blockers": freshness_doc.get("blockers"),
        "probe_row_path": str((candidate_dir / candidate_name / "runls11_bundle_probe_rows.jsonl")) if candidate_dir.exists() else None,
        "candidate_dir": str((candidate_dir / candidate_name)) if candidate_dir.exists() else None,
    }
rows_out.append(row_doc)
final_blocker_counts[final_blocker] += 1

out_rows_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows_out),
    encoding="utf-8",
)

def collect_latency(rows: list[dict[str, Any]], key: str) -> list[int]:
    return [row[key] for row in rows if isinstance(row.get(key), int)]


bundle_ready_latencies = collect_latency(rows_out, "total_target_seen_to_bundle_ready_us")
projection_latencies = collect_latency(rows_out, "target_seen_to_projection_us")
projection_to_bundle_latencies = collect_latency(rows_out, "projection_to_bundle_us")
bundle_to_status_check_latencies = collect_latency(rows_out, "bundle_ready_to_first_status_check_us")
provider_sim_latencies = collect_latency(rows_out, "provider_sim_latency_us")
freshness_race_counts = Counter(str(row.get("freshness_race_classification") or "STATUS_UNKNOWN") for row in rows_out)

report = {
    "record_type": "runafbr1f_backrun_freshness_race_report",
    "run_name": "RunAFBR1F",
    "source_target": "afxdp",
    "candidate_count": len(rows_out),
    "target_fresh_bundle_eligible_count": final_blocker_counts.get("afbr_target_fresh_bundle_eligible_no_send", 0),
    "target_already_processed_count": final_blocker_counts.get("blocked_target_already_processed", 0),
    "provider_sim_failed_count": final_blocker_counts.get("blocked_provider_simulation_failed", 0),
    "blockhash_invalid_count": final_blocker_counts.get("blocked_blockhash_invalid", 0),
    "no_target_bytes_count": final_blocker_counts.get("blocked_no_target_bytes", 0),
    "bundle_ready_count": sum(1 for row in rows_out if row.get("bundle_envelope_built")),
    "pm_ok_count": sum(1 for row in rows_out if row.get("selected_pm_reserve_ok")),
    "ev_positive_count": sum(1 for row in rows_out if as_int(row.get("projected_net_ev_after_tip_raw"), 0) > 0),
    "final_blocker_counts": dict(final_blocker_counts),
    "freshness_race_classification_counts": dict(freshness_race_counts),
    "provider_sim_failure_classification_counts": dict(provider_sim_failure_counts),
    "target_seen_to_bundle_ready_us_p50": int(statistics.median(bundle_ready_latencies)) if bundle_ready_latencies else None,
    "target_seen_to_bundle_ready_us_p90": percentile(bundle_ready_latencies, 90),
    "target_seen_to_bundle_ready_us_p99": percentile(bundle_ready_latencies, 99),
    "target_seen_to_bundle_ready_us_min": min(bundle_ready_latencies) if bundle_ready_latencies else None,
    "target_seen_to_bundle_ready_us_max": max(bundle_ready_latencies) if bundle_ready_latencies else None,
    "target_seen_to_projection_us_p50": int(statistics.median(projection_latencies)) if projection_latencies else None,
    "target_seen_to_projection_us_p90": percentile(projection_latencies, 90),
    "target_seen_to_projection_us_p99": percentile(projection_latencies, 99),
    "projection_to_bundle_us_p50": int(statistics.median(projection_to_bundle_latencies)) if projection_to_bundle_latencies else None,
    "projection_to_bundle_us_p90": percentile(projection_to_bundle_latencies, 90),
    "projection_to_bundle_us_p99": percentile(projection_to_bundle_latencies, 99),
    "bundle_ready_to_first_status_check_us_p50": int(statistics.median(bundle_to_status_check_latencies)) if bundle_to_status_check_latencies else None,
    "bundle_ready_to_first_status_check_us_p90": percentile(bundle_to_status_check_latencies, 90),
    "bundle_ready_to_first_status_check_us_p99": percentile(bundle_to_status_check_latencies, 99),
    "provider_sim_latency_us_p50": int(statistics.median(provider_sim_latencies)) if provider_sim_latencies else None,
    "provider_sim_latency_us_p90": percentile(provider_sim_latencies, 90),
    "provider_sim_latency_us_p99": percentile(provider_sim_latencies, 99),
    "timestamp_basis": rows_out[0].get("timestamp_basis") if rows_out else "unknown",
    "summary_status": summary.get("status"),
    "summary_selection_result": summary.get("selection_result"),
    "rows_path": str(out_rows_path),
    "source_summary_path": str(summary_path),
    "source_autopsy_rows_path": str(autopsy_rows_path) if autopsy_rows_path.exists() else None,
}

out_json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"candidate_count={report['candidate_count']}",
    f"target_fresh_bundle_eligible_count={report['target_fresh_bundle_eligible_count']}",
    f"target_already_processed_count={report['target_already_processed_count']}",
    f"provider_sim_failed_count={report['provider_sim_failed_count']}",
    f"bundle_ready_count={report['bundle_ready_count']}",
    f"pm_ok_count={report['pm_ok_count']}",
    f"ev_positive_count={report['ev_positive_count']}",
    f"final_blocker_counts={json.dumps(report['final_blocker_counts'], sort_keys=True)}",
    f"freshness_race_classification_counts={json.dumps(report['freshness_race_classification_counts'], sort_keys=True)}",
    f"provider_sim_failure_classification_counts={json.dumps(report['provider_sim_failure_classification_counts'], sort_keys=True)}",
    f"target_seen_to_bundle_ready_us_p50={report['target_seen_to_bundle_ready_us_p50']}",
    f"target_seen_to_bundle_ready_us_p90={report['target_seen_to_bundle_ready_us_p90']}",
    f"target_seen_to_bundle_ready_us_p99={report['target_seen_to_bundle_ready_us_p99']}",
    f"target_seen_to_bundle_ready_us_min={report['target_seen_to_bundle_ready_us_min']}",
    f"target_seen_to_bundle_ready_us_max={report['target_seen_to_bundle_ready_us_max']}",
    f"target_seen_to_projection_us_p50={report['target_seen_to_projection_us_p50']}",
    f"target_seen_to_projection_us_p90={report['target_seen_to_projection_us_p90']}",
    f"target_seen_to_projection_us_p99={report['target_seen_to_projection_us_p99']}",
    f"projection_to_bundle_us_p50={report['projection_to_bundle_us_p50']}",
    f"projection_to_bundle_us_p90={report['projection_to_bundle_us_p90']}",
    f"projection_to_bundle_us_p99={report['projection_to_bundle_us_p99']}",
    f"bundle_ready_to_first_status_check_us_p50={report['bundle_ready_to_first_status_check_us_p50']}",
    f"bundle_ready_to_first_status_check_us_p90={report['bundle_ready_to_first_status_check_us_p90']}",
    f"bundle_ready_to_first_status_check_us_p99={report['bundle_ready_to_first_status_check_us_p99']}",
    f"provider_sim_latency_us_p50={report['provider_sim_latency_us_p50']}",
    f"provider_sim_latency_us_p90={report['provider_sim_latency_us_p90']}",
    f"provider_sim_latency_us_p99={report['provider_sim_latency_us_p99']}",
    f"timestamp_basis={report['timestamp_basis']}",
    f"rows_path={out_rows_path}",
    f"report_path={out_json_path}",
]
out_summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "AFBR1F freshness race report written"
echo "runafbr1f_report_path=$OUT_JSON_PATH"
echo "runafbr1f_rows_path=$OUT_ROWS_PATH"
echo "runafbr1f_summary_path=$OUT_SUMMARY_PATH"
finish

#!/usr/bin/env bash
# 21_runafbr1t_afxdp_target_entity_linker.sh — classify and link AF_XDP Backrun target entities

source "$(dirname "$0")/lib.sh"
script_header "21 AFBR1T — AF_XDP Target Entity Linker"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUNAFBR1T_STEP:-21_runafbr1t_afxdp_target_entity_linker}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
ROWS_PATH="$RESULT_DIR/runafbr1t_target_entity_linker_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runafbr1t_target_entity_linker_summary.json"

AFBR1_STEP="${SHADOW_RUNAFBR1T_AFBR1_STEP:-21_runafbr1_afxdp_backrun_projection_bundle_readiness}"
AFBR1_INNER_DIR="${SHADOW_RUNAFBR1T_AFBR1_INNER_DIR:-$RESULTS_ROOT/$AFBR1_STEP/window1/20_g12_r0_observe_only}"
SHOCK_RESEARCH_PATH="${SHADOW_RUNAFBR1T_SHOCK_RESEARCH_PATH:-$AFBR1_INNER_DIR/shock_research.jsonl}"
AUTOPSY_ROWS_PATH="${SHADOW_RUNAFBR1T_AUTOPSY_ROWS_PATH:-$AFBR1_INNER_DIR/runafbr1_projection_autopsy_rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$ROWS_PATH" "$SUMMARY_JSON"

python3 - "$SHOCK_RESEARCH_PATH" "$AUTOPSY_ROWS_PATH" "$ROWS_PATH" "$SUMMARY_JSON" "$SUMMARY_FILE" <<'PY'
from __future__ import annotations

import base64
import hashlib
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

shock_research_path = Path(sys.argv[1])
autopsy_rows_path = Path(sys.argv[2])
rows_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
summary_txt_path = Path(sys.argv[5])

BUNDLE_ROUTES = {"jito_bundle", "harmonic_bundle"}
KNOWN_PROGRAM_HINTS = {
    "raydium": "supported",
    "orca": "supported",
    "meteora": "supported",
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


def text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "on"}


def as_int(value: Any) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def first_value(row: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in row and row.get(key) not in (None, ""):
            return row.get(key)
    return None


def row_key(row: dict[str, Any]) -> tuple[str, str, str, str, str, int]:
    return (
        text(first_value(row, "target_signature_hex", "signature_hex")),
        text(first_value(row, "our_signature_hex")),
        text(first_value(row, "pool_pubkey_hex", "pool")),
        text(first_value(row, "mint_in_hex", "mint_in")),
        text(first_value(row, "mint_out_hex", "mint_out")),
        as_int(first_value(row, "slot", "ingress_slot")),
    )


def is_afxdp_row(row: dict[str, Any]) -> bool:
    return lower(row.get("signal_source")) == "afxdp" or as_int(row.get("signal_source_id")) == 4


def is_backrun_row(row: dict[str, Any]) -> bool:
    return (
        lower(row.get("owner_mode")) == "backrun"
        or lower(row.get("selected_owner")) == "backrun"
        or as_int(row.get("owner_mode_id")) == 1
        or as_int(row.get("selected_owner_id")) == 1
    )


def is_bundle_row(row: dict[str, Any]) -> bool:
    route_family = lower(first_value(row, "route_family_label", "route_family"))
    return route_family in BUNDLE_ROUTES or route_family == "0"


def decodes_base64(value: Any) -> tuple[bool, int]:
    raw = text(value)
    if not raw:
        return False, 0
    try:
        decoded = base64.b64decode(raw, validate=True)
    except Exception:
        return False, 0
    return bool(decoded), len(decoded)


def stable_target_entity_id(
    signature: str,
    pool: str,
    mint_in: str,
    mint_out: str,
    direction: str,
    amount_in_raw: int,
    instruction_index: int,
) -> str:
    payload = "|".join(
        [
            signature.lower(),
            pool.lower(),
            mint_in.lower(),
            mint_out.lower(),
            direction.lower(),
            str(amount_in_raw),
            str(instruction_index),
        ]
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()[:32]


def classify_row(row: dict[str, Any], autopsy: dict[str, Any] | None) -> dict[str, Any]:
    candidate_id = text(first_value(row, "candidate_id", "our_signature_hex", "bundle_payload_sha256_hex"))
    ingress_unix_ns = as_int(first_value(row, "ingress_unix_ns", "unix_ns"))
    target_signature = text(first_value(row, "target_signature_hex", "signature_hex"))
    target_tx_bytes_available = as_bool(first_value(row, "target_tx_bytes_available")) or bool(text(row.get("target_tx_base64")))
    target_tx_bytes_source = text(first_value(row, "target_tx_bytes_source", "target_signal_source"))
    target_tx_decodes_locally, target_tx_decoded_len_bytes = decodes_base64(row.get("target_tx_base64"))
    target_pool = text(first_value(row, "target_pool_pubkey_hex", "pool_pubkey_hex", "target_pool", "pool"))
    target_venue = text(first_value(row, "target_venue", "venue", "target_venue_label", "venue_label"))
    target_mint_in = text(first_value(row, "target_mint_in_hex", "mint_in_hex", "target_mint_in", "mint_in"))
    target_mint_out = text(first_value(row, "target_mint_out_hex", "mint_out_hex", "target_mint_out", "mint_out"))
    target_direction = text(first_value(row, "target_direction"))
    target_amount_in_raw = as_int(first_value(row, "target_amount_in", "target_gross_amount_in", "trade_size_lamports"))
    target_amount_out_raw_if_decodable = as_int(first_value(row, "target_amount_out_if_known"))
    target_program_ids = row.get("target_program_ids_b58") if isinstance(row.get("target_program_ids_b58"), list) else []
    target_program_id = text(target_program_ids[0] if target_program_ids else "")
    target_instruction_index = as_int(first_value(row, "target_instruction_index"))
    target_swap_decoder = text(first_value(row, "target_swap_decoder"))
    baseline_in = as_int(first_value(row, "quote_baseline_reserve_in", "baseline_reserve_in", "pool_reserve_in"))
    baseline_out = as_int(first_value(row, "quote_baseline_reserve_out", "baseline_reserve_out", "pool_reserve_out"))
    projected_in = as_int(first_value(row, "quote_projected_reserve_in", "projected_reserve_in"))
    projected_out = as_int(first_value(row, "quote_projected_reserve_out", "projected_reserve_out"))
    pre_target_pool_state_available = baseline_in > 0 and baseline_out > 0
    target_expected_pool_delta = {
        "reserve_in_delta_raw": projected_in - baseline_in if pre_target_pool_state_available and projected_in > 0 else 0,
        "reserve_out_delta_raw": projected_out - baseline_out if pre_target_pool_state_available and projected_out > 0 else 0,
    }
    route_family = lower(first_value(row, "route_family", "route_family_label"))
    venue_hint = lower(target_venue)
    target_relationship_source = ""
    first_target_entity_blocker = "none"
    live_track_scope = is_bundle_row(row) and is_afxdp_row(row) and is_backrun_row(row)

    if not target_tx_bytes_available:
        first_target_entity_blocker = "target_tx_bytes_missing"
    elif not target_tx_decodes_locally:
        first_target_entity_blocker = "target_tx_decode_failed"
    elif not target_program_id and target_program_ids:
        first_target_entity_blocker = "unsupported_target_program"
    elif not target_pool:
        first_target_entity_blocker = "pool_not_resolved"
    elif not target_mint_in or not target_mint_out:
        first_target_entity_blocker = "mint_pair_not_resolved"
    elif target_amount_in_raw <= 0:
        first_target_entity_blocker = "amount_in_missing"
    elif not target_direction:
        first_target_entity_blocker = "direction_missing"
    elif not pre_target_pool_state_available:
        first_target_entity_blocker = "pool_state_missing"
    elif autopsy and lower(autopsy.get("projection_blocker")) == "blocked_target_pool_mismatch":
        first_target_entity_blocker = "target_relationship_ambiguous"
    elif target_program_id:
        target_relationship_source = "decoded_afxdp_tx"
    elif target_tx_bytes_source:
        target_relationship_source = "target_tx_cache_join"

    if first_target_entity_blocker == "none":
        if venue_hint and any(key in venue_hint for key in KNOWN_PROGRAM_HINTS):
            target_swap_decoder = target_swap_decoder or target_venue
        target_relationship_source = target_relationship_source or ("decoded_afxdp_tx" if target_tx_bytes_available else "")
        target_entity_id = stable_target_entity_id(
            target_signature,
            target_pool,
            target_mint_in,
            target_mint_out,
            target_direction,
            target_amount_in_raw,
            target_instruction_index,
        )
    else:
        target_entity_id = ""
        if first_target_entity_blocker == "unsupported_target_program" and venue_hint and any(key in venue_hint for key in KNOWN_PROGRAM_HINTS):
            first_target_entity_blocker = "pool_not_resolved"
        if first_target_entity_blocker == "target_tx_bytes_missing" and target_tx_bytes_source:
            target_relationship_source = target_tx_bytes_source

    return {
        "record_type": "runafbr1t_target_entity_linker",
        "candidate_id": candidate_id,
        "ingress_unix_ns": ingress_unix_ns,
        "target_signature": target_signature,
        "target_tx_bytes_available": target_tx_bytes_available,
        "target_tx_bytes_source": target_tx_bytes_source,
        "target_tx_decodes_locally": target_tx_decodes_locally,
        "target_tx_decoded_len_bytes": target_tx_decoded_len_bytes,
        "target_tx_cache_key": target_signature.lower(),
        "target_entity_id": target_entity_id,
        "target_relationship_source": target_relationship_source or "missing",
        "target_pool": target_pool,
        "target_venue": target_venue,
        "target_mint_in": target_mint_in,
        "target_mint_out": target_mint_out,
        "target_direction": target_direction,
        "target_amount_in_raw": target_amount_in_raw,
        "target_amount_out_raw_if_decodable": target_amount_out_raw_if_decodable,
        "target_program_id": target_program_id,
        "target_instruction_index": target_instruction_index,
        "target_swap_decoder": target_swap_decoder,
        "pre_target_pool_state_available": pre_target_pool_state_available,
        "target_expected_pool_delta": target_expected_pool_delta,
        "first_target_entity_blocker": first_target_entity_blocker,
        "route_family": route_family,
        "is_live_track_scope": live_track_scope,
    }


shock_rows = [row for row in load_jsonl(shock_research_path) if is_afxdp_row(row) and is_backrun_row(row) and is_bundle_row(row)]
autopsy_map = {row_key(row): row for row in load_jsonl(autopsy_rows_path)}
linked_rows = [classify_row(row, autopsy_map.get(row_key(row))) for row in shock_rows]

rows_path.write_text(
    "".join(json.dumps(row, sort_keys=True) + "\n" for row in linked_rows),
    encoding="utf-8",
)

live_scope_rows = [row for row in linked_rows if as_bool(row.get("is_live_track_scope"))]
target_entity_rows = [row for row in live_scope_rows if text(row.get("target_entity_id"))]
blocker_counts = Counter(text(row.get("first_target_entity_blocker")) for row in live_scope_rows if text(row.get("first_target_entity_blocker")))
relationship_source_counts = Counter(text(row.get("target_relationship_source")) for row in live_scope_rows if text(row.get("target_relationship_source")))

summary = {
    "status": "runafbr1t_target_entity_linker_complete",
    "shock_afxdp_backrun_bundle_rows": len(shock_rows),
    "shock_afxdp_backrun_bundle_target_entity_rows": len(target_entity_rows),
    "target_bytes_visible_at_projection_count": sum(1 for row in live_scope_rows if as_bool(row.get("target_tx_bytes_available"))),
    "target_bytes_missing_count": sum(1 for row in live_scope_rows if text(row.get("first_target_entity_blocker")) == "target_tx_bytes_missing"),
    "target_tx_decode_failed_count": sum(1 for row in live_scope_rows if text(row.get("first_target_entity_blocker")) == "target_tx_decode_failed"),
    "pre_target_pool_state_rows": sum(1 for row in live_scope_rows if as_bool(row.get("pre_target_pool_state_available"))),
    "pre_target_pool_state_missing_count": sum(1 for row in live_scope_rows if text(row.get("first_target_entity_blocker")) == "pool_state_missing"),
    "blocked_target_entity_missing_count": sum(1 for row in live_scope_rows if text(row.get("first_target_entity_blocker")) != "none"),
    "first_target_entity_blocker_counts": dict(blocker_counts),
    "target_relationship_source_counts": dict(relationship_source_counts),
}

summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
summary_lines = [
    f"status={summary['status']}",
    f"shock_afxdp_backrun_bundle_rows={summary['shock_afxdp_backrun_bundle_rows']}",
    f"shock_afxdp_backrun_bundle_target_entity_rows={summary['shock_afxdp_backrun_bundle_target_entity_rows']}",
    f"target_bytes_visible_at_projection_count={summary['target_bytes_visible_at_projection_count']}",
    f"target_bytes_missing_count={summary['target_bytes_missing_count']}",
    f"target_tx_decode_failed_count={summary['target_tx_decode_failed_count']}",
    f"pre_target_pool_state_rows={summary['pre_target_pool_state_rows']}",
    f"pre_target_pool_state_missing_count={summary['pre_target_pool_state_missing_count']}",
    f"blocked_target_entity_missing_count={summary['blocked_target_entity_missing_count']}",
    "first_target_entity_blocker_counts=" + json.dumps(summary["first_target_entity_blocker_counts"], sort_keys=True),
    "target_relationship_source_counts=" + json.dumps(summary["target_relationship_source_counts"], sort_keys=True),
]
summary_txt_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

info "AFBR1T rows written to $ROWS_PATH"
info "AFBR1T summary written to $SUMMARY_JSON"
finish

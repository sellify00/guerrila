#!/usr/bin/env bash
# 21_runls14_hybrid_backrun_join_proof.sh — AF_XDP target trigger + LaserStream enrichment proof

source "$(dirname "$0")/lib.sh"
script_header "21 — RunLS14 Hybrid Backrun Join Proof"

assert_root
require_env SHADOW_PACK_DIR

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_RUNLS14_WINDOW_SECONDS:-60}}"
STEP="21_runls14_hybrid_backrun_join_proof"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
WINDOW_RESULT_DIR="$OUTER_RESULT_DIR/window1"
INNER_STEP_DIR="$WINDOW_RESULT_DIR/20_g12_r0_observe_only"
INNER_SUMMARY_FILE="$INNER_STEP_DIR/summary.txt"
SHOCK_RESEARCH_PATH="$INNER_STEP_DIR/shock_research.jsonl"
SIGNATURE_FIRST_SEEN_PATH="$INNER_STEP_DIR/laserstream_vs_afxdp_signature_first_seen.jsonl"
MATCHED_STAGE_PATH="$INNER_STEP_DIR/laserstream_vs_afxdp_matched_signature_stage_deltas.jsonl"
NEAR_READY_PATH="$INNER_STEP_DIR/runls7_near_trade_ready_stage_deltas.jsonl"
JOIN_ROWS_PATH="$INNER_STEP_DIR/runls14_hybrid_backrun_join_rows.jsonl"
SUMMARY_JSON_PATH="$INNER_STEP_DIR/runls14_hybrid_backrun_join_summary.json"
PROBE_ROWS_PATH="$INNER_STEP_DIR/runls11_bundle_probe_rows.jsonl"
SELECTED_JSON_PATH="$INNER_STEP_DIR/runls11_selected_backrun_bundle.json"
PAYLOAD_JSON_PATH="$INNER_STEP_DIR/runls11_bundle_payload_base64.json"
PM_REPORT_JSON_PATH="$INNER_STEP_DIR/runls11_pm_reserve_report.json"
FRESHNESS_JSON_PATH="$INNER_STEP_DIR/runls11e_target_freshness_bundle_eligibility.json"
OUTER_SUMMARY_FILE="$OUTER_RESULT_DIR/summary.txt"

mkdir -p "$WINDOW_RESULT_DIR"

info "Running bounded dual-source observe-only window for AF_XDP + LaserStream Backrun join proof"
export SHADOW_RESULTS_DIR="$WINDOW_RESULT_DIR"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-laserstream}"
export SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="true"
export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-2}"
export SHADOW_RUNLS11_EXPORT_PATH="$PROBE_ROWS_PATH"
unset SHADOW_RUNLS11_ROUTE_FAMILY SHADOW_RUNLS11_POOL_PUBKEY SHADOW_RUNLS11_MINT_IN SHADOW_RUNLS11_MINT_OUT

bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"

if [[ ! -f "$INNER_SUMMARY_FILE" ]]; then
    fail "Inner RunLS14 summary missing: $INNER_SUMMARY_FILE"
    finish
    exit 1
fi
if ! grep -q '^run_status=completed$' "$INNER_SUMMARY_FILE"; then
    fail "Inner G12 window did not complete cleanly"
    finish
    exit 1
fi
if [[ ! -f "$SHOCK_RESEARCH_PATH" ]]; then
    fail "shock_research.jsonl missing: $SHOCK_RESEARCH_PATH"
    finish
    exit 1
fi
if [[ ! -f "$SIGNATURE_FIRST_SEEN_PATH" ]]; then
    fail "signature first-seen artifact missing: $SIGNATURE_FIRST_SEEN_PATH"
    finish
    exit 1
fi

python3 - "$SHOCK_RESEARCH_PATH" "$SIGNATURE_FIRST_SEEN_PATH" "$MATCHED_STAGE_PATH" \
    "$NEAR_READY_PATH" "$JOIN_ROWS_PATH" "$SUMMARY_JSON_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
from typing import Any

shock_path = Path(sys.argv[1])
signature_first_seen_path = Path(sys.argv[2])
matched_stage_path = Path(sys.argv[3])
near_ready_path = Path(sys.argv[4])
rows_path = Path(sys.argv[5])
summary_path = Path(sys.argv[6])

bundle_routes = {"jito_bundle", "harmonic_bundle"}
allowed_backrun_routes = {"jito_bundle", "harmonic_bundle", "helius_sender_dual"}
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def as_int(value: Any, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def b58decode(value: str) -> bytes | None:
    num = 0
    for char in value:
        try:
            digit = BASE58_ALPHABET.index(char)
        except ValueError:
            return None
        num = num * 58 + digit
    data = num.to_bytes((num.bit_length() + 7) // 8, "big") if num else b""
    pad = 0
    for char in value:
        if char == BASE58_ALPHABET[0]:
            pad += 1
        else:
            break
    return b"\x00" * pad + data


def signature_hex(value: Any) -> str:
    if not isinstance(value, str) or not value:
        return ""
    if len(value) == 128 and all(ch in "0123456789abcdefABCDEF" for ch in value):
        return value.lower()
    decoded = b58decode(value)
    if decoded is None:
        return ""
    return decoded.hex()


def present_ns(value: Any) -> bool:
    return as_int(value, 0) > 0


def delta_us(later: Any, earlier: Any) -> int | None:
    left = as_int(later, 0)
    right = as_int(earlier, 0)
    if left <= 0 or right <= 0:
        return None
    return int((left - right) / 1000)


def percentile(values: list[int], pct: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    index = int(round((len(ordered) - 1) * pct))
    return ordered[max(0, min(index, len(ordered) - 1))]


signature_rows = {
    row.get("signature"): row
    for row in load_jsonl(signature_first_seen_path)
    if row.get("record_type") == "signature_source_first_seen" and row.get("signature")
}
matched_rows = {
    row.get("signature"): row
    for row in load_jsonl(matched_stage_path)
    if row.get("record_type") == "matched_signature_stage_delta" and row.get("signature")
}
near_ready_rows = {
    row.get("signature"): row
    for row in load_jsonl(near_ready_path)
    if row.get("record_type") == "matched_signature_near_trade_ready_delta" and row.get("signature")
}

shock_by_sig: dict[str, list[dict[str, Any]]] = defaultdict(list)
laserstream_backrun_rows: list[dict[str, Any]] = []
afxdp_backrun_rows: list[dict[str, Any]] = []
for row in load_jsonl(shock_path):
    if row.get("record_type") != "rollout_shadow":
        continue
    sig = row.get("signature_hex")
    if sig:
        shock_by_sig[str(sig)].append(row)
    if row.get("owner_mode") != "backrun":
        continue
    if row.get("signal_source") == "laserstream":
        laserstream_backrun_rows.append(row)
    elif row.get("signal_source") == "afxdp":
        afxdp_backrun_rows.append(row)

join_rows: list[dict[str, Any]] = []
join_blockers = Counter()
by_route = Counter()
by_venue = Counter()
by_pool = Counter()
enrichment_after_target_us: list[int] = []
candidate_after_target_us: list[int] = []
quote_after_target_us: list[int] = []

for signature, sigrow in signature_rows.items():
    sig_hex = signature_hex(signature)
    related_shock_rows = shock_by_sig.get(sig_hex, []) if sig_hex else []
    has_backrun_context = (
        sigrow.get("canonical_owner_mode") == "backrun"
        or sigrow.get("owner_mode_if_candidate") == "backrun"
        or any(row.get("owner_mode") == "backrun" for row in related_shock_rows)
    )
    if not has_backrun_context:
        continue
    if not bool(sigrow.get("seen_by_afxdp")) or not bool(sigrow.get("seen_by_laserstream")):
        continue

    route = sigrow.get("route_family") or sigrow.get("canonical_route_family")
    near = near_ready_rows.get(signature, {})
    matched = matched_rows.get(signature, {})
    route = route or near.get("route_family") or matched.get("route_family") or "unknown"
    venue = sigrow.get("venue") or matched.get("venue") or near.get("venue") or "unknown"
    pool = sigrow.get("pool") or matched.get("pool") or near.get("pool") or "unknown"
    afxdp_raw_ns = sigrow.get("afxdp_host_mono_recv_ns") or sigrow.get("afxdp_seen_ns")
    laserstream_raw_ns = sigrow.get("laserstream_host_mono_recv_ns") or sigrow.get("laserstream_seen_ns")
    afxdp_candidate_ns = sigrow.get("afxdp_candidate_emit_ns")
    laserstream_candidate_ns = sigrow.get("laserstream_candidate_emit_ns")
    laserstream_quote_ns = sigrow.get("laserstream_quote_ready_ns")
    raw_join_delta = delta_us(laserstream_raw_ns, afxdp_raw_ns)
    candidate_join_delta = delta_us(laserstream_candidate_ns, afxdp_raw_ns)
    quote_join_delta = delta_us(laserstream_quote_ns, afxdp_raw_ns)

    target_rows = [
        row for row in related_shock_rows
        if row.get("signal_source") == "afxdp" and row.get("owner_mode") == "backrun"
    ]
    enrichment_rows = [
        row for row in related_shock_rows
        if row.get("signal_source") == "laserstream" and row.get("owner_mode") == "backrun"
    ]
    laser_candidates = [
        row for row in enrichment_rows
        if row.get("route_family_label") in allowed_backrun_routes
    ]
    if route == "unknown" and laser_candidates:
        route = str(laser_candidates[0].get("route_family_label") or "unknown")
    laser_near_ready = [
        row for row in laser_candidates
        if as_int(row.get("quote_expected_out_lamports")) > 0
        and as_int(row.get("min_out_lamports")) > 0
        and as_int(row.get("retained_ev_lamports_lcb")) > 0
        and bool(row.get("would_build"))
        and bool(row.get("would_sign"))
        and as_int(row.get("reserved_notional_lamports")) > 0
    ]

    blockers: list[str] = []
    if not present_ns(afxdp_raw_ns):
        blockers.append("missing_afxdp_target_seen_ns")
    if not target_rows:
        blockers.append("missing_afxdp_backrun_candidate")
    if not present_ns(laserstream_raw_ns):
        blockers.append("missing_laserstream_enrichment_seen_ns")
    if not enrichment_rows:
        blockers.append("missing_laserstream_backrun_enrichment")
    if not laser_near_ready:
        blockers.append("missing_laserstream_near_trade_ready_enrichment")
    if route not in bundle_routes:
        blockers.append("route_not_bundle_ready")

    if raw_join_delta is not None:
        enrichment_after_target_us.append(raw_join_delta)
    if candidate_join_delta is not None:
        candidate_after_target_us.append(candidate_join_delta)
    if quote_join_delta is not None:
        quote_after_target_us.append(quote_join_delta)

    for blocker in blockers:
        join_blockers[blocker] += 1
    by_route[str(route)] += 1
    by_venue[str(venue)] += 1
    by_pool[str(pool)] += 1

    join_rows.append({
        "record_type": "runls14_hybrid_backrun_join_row",
        "signature": signature,
        "signature_hex": sig_hex or None,
        "slot": sigrow.get("slot"),
        "owner_mode": "backrun",
        "venue": venue,
        "pool": pool,
        "mint_in": sigrow.get("mint_in") or matched.get("mint_in") or near.get("mint_in"),
        "mint_out": sigrow.get("mint_out") or matched.get("mint_out") or near.get("mint_out"),
        "route_family": route,
        "source_first": sigrow.get("first_seen_source"),
        "afxdp_target_seen_ns": afxdp_raw_ns,
        "laserstream_enrichment_seen_ns": laserstream_raw_ns,
        "laserstream_after_afxdp_target_us": raw_join_delta,
        "laserstream_candidate_after_afxdp_target_us": candidate_join_delta,
        "laserstream_quote_after_afxdp_target_us": quote_join_delta,
        "afxdp_candidate_emit_ns": afxdp_candidate_ns,
        "laserstream_candidate_emit_ns": laserstream_candidate_ns,
        "laserstream_quote_ready_ns": laserstream_quote_ns,
        "afxdp_backrun_candidate_seen": bool(target_rows),
        "laserstream_backrun_enrichment_seen": bool(enrichment_rows),
        "laserstream_near_trade_ready": bool(laser_near_ready),
        "bundle_route_ready": route in bundle_routes,
        "target_tx_bytes_source_required": "afxdp",
        "bundle_builder_ready_to_rerun_ls11e": not blockers,
        "blockers": blockers,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    })

rows_path.write_text(
    "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in join_rows),
    encoding="utf-8",
)

bundle_ready_join_count = sum(1 for row in join_rows if row["bundle_builder_ready_to_rerun_ls11e"])
matched_backrun_join_count = len(join_rows)
selection_result = (
    "hybrid_backrun_join_ready_for_ls11e"
    if bundle_ready_join_count > 0
    else (
        "blocked_no_matched_hybrid_backrun_join"
        if matched_backrun_join_count == 0
        else "blocked_hybrid_join_incomplete"
    )
)

summary = {
    "record_type": "runls14_hybrid_backrun_join_summary",
    "run_name": "RunLS14",
    "strategy": "backrun",
    "architecture": "hybrid",
    "target_trigger_source": "afxdp",
    "enrichment_source": "laserstream_processed",
    "selection_result": selection_result,
    "matched_hybrid_backrun_join_count": matched_backrun_join_count,
    "hybrid_bundle_ready_join_count": bundle_ready_join_count,
    "laserstream_backrun_seen": len(laserstream_backrun_rows),
    "afxdp_backrun_seen": len(afxdp_backrun_rows),
    "join_blockers": dict(join_blockers),
    "by_route": dict(by_route),
    "by_venue": dict(by_venue),
    "top_pools": dict(by_pool.most_common(16)),
    "laserstream_after_afxdp_target_us": {
        "median": int(median(enrichment_after_target_us)) if enrichment_after_target_us else None,
        "p90": percentile(enrichment_after_target_us, 0.90),
        "p99": percentile(enrichment_after_target_us, 0.99),
    },
    "laserstream_candidate_after_afxdp_target_us": {
        "median": int(median(candidate_after_target_us)) if candidate_after_target_us else None,
        "p90": percentile(candidate_after_target_us, 0.90),
        "p99": percentile(candidate_after_target_us, 0.99),
    },
    "laserstream_quote_after_afxdp_target_us": {
        "median": int(median(quote_after_target_us)) if quote_after_target_us else None,
        "p90": percentile(quote_after_target_us, 0.90),
        "p99": percentile(quote_after_target_us, 0.99),
    },
    "requires_next_gate": "RunLS11E on AF_XDP-captured target bundle artifact",
    "processed_laserstream_live_source_promoted": False,
    "sendBundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

python3 - "$PROBE_ROWS_PATH" "$SHOCK_RESEARCH_PATH" "$SUMMARY_JSON_PATH" \
    "$SELECTED_JSON_PATH" "$PAYLOAD_JSON_PATH" "$PM_REPORT_JSON_PATH" <<'PY'
from __future__ import annotations

import base64
import json
import sys
from pathlib import Path
from typing import Any

probe_rows_path = Path(sys.argv[1])
shock_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
selected_path = Path(sys.argv[4])
payload_path = Path(sys.argv[5])
pm_report_path = Path(sys.argv[6])

bundle_routes = {"jito_bundle", "harmonic_bundle"}


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


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
        decoded = base64.b64decode(value)
    except Exception:  # noqa: BLE001
        return False, 0
    return bool(decoded), len(decoded)


def selected_pm_fields(shock_rows: list[dict[str, Any]], signature: str) -> dict[str, Any]:
    fields = {
        "selected_pm_reserve_created": False,
        "selected_pm_reserve_released": False,
        "selected_pm_reserve_id": None,
        "selected_pm_release_reason": None,
        "selected_pm_pending_after": None,
    }
    if not signature:
        return fields
    for row in shock_rows:
        if row.get("signature_hex") != signature:
            continue
        if row.get("record_type") == "rollout_shadow":
            fields["selected_pm_reserve_created"] = as_int(row.get("reserved_notional_lamports")) > 0
        elif row.get("record_type") == "reconcile_action":
            fields["selected_pm_reserve_released"] = True
            fields["selected_pm_release_reason"] = row.get("terminal_reason")
            fields["selected_pm_pending_after"] = 0
    return fields


summary = load_json(summary_path)
probe_rows = [
    row for row in load_jsonl(probe_rows_path)
    if row.get("record_type") == "runls11_bundle_probe"
]
shock_rows = load_jsonl(shock_path)

strict_rows: list[dict[str, Any]] = []
blockers: dict[str, int] = {
    "blocked_no_afxdp_target_bytes": 0,
    "blocked_no_laserstream_enrichment": 0,
    "blocked_target_relationship_missing": 0,
    "blocked_bundle_shape_invalid": 0,
    "blocked_pm_reserve": 0,
}

for row in probe_rows:
    row_blockers: list[str] = []
    if row.get("signal_source") != "laserstream" or row.get("owner_mode") != "backrun":
        row_blockers.append("blocked_no_laserstream_enrichment")
    target_source = row.get("target_signal_source")
    if target_source != "afxdp" or as_int(row.get("target_signal_source_id")) != 4:
        row_blockers.append("blocked_no_afxdp_target_bytes")
    if not row.get("target_tx_bytes_available") or row.get("target_tx_bytes_source") != "afxdp":
        row_blockers.append("blocked_no_afxdp_target_bytes")
    if not row.get("target_relationship_recorded"):
        row_blockers.append("blocked_target_relationship_missing")
    if row.get("target_pool_pubkey_hex") != row.get("pool_pubkey_hex"):
        row_blockers.append("blocked_target_relationship_missing")
    if row.get("target_mint_in_hex") != row.get("mint_in_hex"):
        row_blockers.append("blocked_target_relationship_missing")
    if row.get("target_mint_out_hex") != row.get("mint_out_hex"):
        row_blockers.append("blocked_target_relationship_missing")
    if row.get("route_family") not in bundle_routes:
        row_blockers.append("blocked_bundle_shape_invalid")
    if not row.get("bundle_payload_base64"):
        row_blockers.append("blocked_bundle_shape_invalid")
    if as_int(row.get("tip_lamports")) <= 0:
        row_blockers.append("blocked_bundle_shape_invalid")
    if as_int(row.get("expected_out_raw")) <= 0 or as_int(row.get("min_out_raw")) <= 0:
        row_blockers.append("blocked_bundle_shape_invalid")
    if as_int(row.get("net_ev_after_tip_lamports")) <= 0:
        row_blockers.append("blocked_bundle_shape_invalid")
    if not row.get("pm_reserve_ok") or as_int(row.get("reserved_notional_lamports")) <= 0:
        row_blockers.append("blocked_pm_reserve")
    if row_blockers:
        for blocker in set(row_blockers):
            blockers[blocker] = blockers.get(blocker, 0) + 1
    else:
        strict_rows.append(row)

if not probe_rows:
    blockers["blocked_no_laserstream_enrichment"] = 1

strict_rows.sort(
    key=lambda row: (
        as_int(row.get("net_ev_after_tip_lamports")),
        as_int(row.get("expected_out_raw")),
        as_int(row.get("tip_lamports")) * -1,
    ),
    reverse=True,
)

selected = strict_rows[0] if strict_rows else None
target_decode_ok = False
target_decode_len = 0
our_payload = selected.get("bundle_payload_base64") if selected else None
target_payload = selected.get("target_tx_base64") if selected else None
if target_payload:
    target_decode_ok, target_decode_len = decodes_base64(target_payload)
pm_fields = selected_pm_fields(shock_rows, str(selected.get("our_signature_hex") or "")) if selected else {
    "selected_pm_reserve_created": False,
    "selected_pm_reserve_released": False,
    "selected_pm_reserve_id": None,
    "selected_pm_release_reason": None,
    "selected_pm_pending_after": None,
}

if selected:
    txs_base64 = [target_payload, our_payload]
    selected_doc = {
        "record_type": "runls14_hybrid_selected_backrun_bundle",
        "selection_result": "selected_hybrid_afxdp_target_laserstream_enrichment",
        "source_target": "afxdp",
        "source_enrichment": "laserstream_processed",
        "provider": "jito_via_helius",
        "selected_owner_mode": "backrun",
        "selected_route_family": selected.get("route_family"),
        "selected_pool": selected.get("pool_pubkey_hex"),
        "selected_mint_in": selected.get("mint_in_hex"),
        "selected_mint_out": selected.get("mint_out_hex"),
        "selected_venue": selected.get("venue"),
        "selected_slot": selected.get("slot"),
        "selected_target_signature": selected.get("target_signature_hex"),
        "selected_target_tx_bytes_available": True,
        "selected_target_tx_bytes_source": "afxdp_target_tx_cache",
        "selected_target_tx_encoding": selected.get("target_tx_encoding"),
        "selected_target_tx_decodes_locally": target_decode_ok,
        "selected_target_tx_decoded_len_bytes": target_decode_len,
        "selected_target_tx_message_hash": selected.get("target_tx_message_hash"),
        "selected_target_pool": selected.get("target_pool_pubkey_hex"),
        "selected_target_pool_matches": selected.get("target_pool_pubkey_hex") == selected.get("pool_pubkey_hex"),
        "selected_target_mint_in": selected.get("target_mint_in_hex"),
        "selected_target_mint_in_matches": selected.get("target_mint_in_hex") == selected.get("mint_in_hex"),
        "selected_target_mint_out": selected.get("target_mint_out_hex"),
        "selected_target_mint_out_matches": selected.get("target_mint_out_hex") == selected.get("mint_out_hex"),
        "selected_target_amount_in": selected.get("target_amount_in"),
        "selected_target_account_keys_b58": selected.get("target_account_keys_b58", []),
        "selected_target_program_ids_b58": selected.get("target_program_ids_b58", []),
        "selected_target_relationship_recorded": True,
        "laserstream_enrichment_joined": True,
        "selected_backrun_signature": selected.get("our_signature_hex"),
        "selected_backrun_tx_template": selected.get("template_id"),
        "selected_expected_out_raw": as_int(selected.get("expected_out_raw")),
        "selected_min_out_raw": as_int(selected.get("min_out_raw")),
        "selected_net_ev_after_tip": as_int(selected.get("net_ev_after_tip_lamports")),
        "selected_tip_lamports": as_int(selected.get("tip_lamports")),
        "selected_pm_reserve_amount": as_int(selected.get("reserved_notional_lamports")),
        "selected_pm_reserve_ok": True,
        "selected_pm_reserve_created": pm_fields["selected_pm_reserve_created"],
        "selected_pm_reserve_released": pm_fields["selected_pm_reserve_released"],
        "selected_pm_reserve_id": pm_fields["selected_pm_reserve_id"],
        "selected_pm_release_reason": pm_fields["selected_pm_release_reason"],
        "selected_pm_pending_after": pm_fields["selected_pm_pending_after"],
        "bundle_envelope_built": bool(our_payload),
        "bundle_tx_count": 2,
        "target_tx_first": True,
        "our_backrun_tx_second": True,
        "tip_present": as_int(selected.get("tip_lamports")) > 0,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "record_type": "runls14_hybrid_bundle_payload",
        "bundle_payload_valid": bool(our_payload and target_payload),
        "selected_route_family": selected.get("route_family"),
        "provider": "jito_via_helius",
        "bundle_tx_count": 2,
        "bundle_tx_count_max_allowed": 5,
        "target_tx_required": True,
        "target_tx_bytes_available": True,
        "target_tx_first": True,
        "our_backrun_tx_present": bool(our_payload),
        "our_backrun_tx_second": True,
        "tip_present": as_int(selected.get("tip_lamports")) > 0,
        "min_out_positive": as_int(selected.get("min_out_raw")) > 0,
        "expected_out_positive": as_int(selected.get("expected_out_raw")) > 0,
        "net_ev_positive_after_tip": as_int(selected.get("net_ev_after_tip_lamports")) > 0,
        "estimated_tip_lamports": as_int(selected.get("tip_lamports")),
        "estimated_priority_fee_lamports": 0,
        "bundle_expiry_policy": "no_submit_hybrid_readiness_only",
        "send_bundle_disabled_in_mode": True,
        "sendBundle_called": False,
        "txs_base64": txs_base64,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "record_type": "runls14_hybrid_pm_reserve_report",
        "selection_result": selected_doc["selection_result"],
        "selected_pm_reserve_ok": True,
        "selected_reserved_notional_lamports": as_int(selected.get("reserved_notional_lamports")),
        "selected_pm_reserve_created": pm_fields["selected_pm_reserve_created"],
        "selected_pm_reserve_released": pm_fields["selected_pm_reserve_released"],
        "selected_pm_reserve_id": pm_fields["selected_pm_reserve_id"],
        "selected_pm_release_reason": pm_fields["selected_pm_release_reason"],
        "selected_pm_pending_after": pm_fields["selected_pm_pending_after"],
        "pm_reserve_reconciled": bool(
            pm_fields["selected_pm_reserve_created"]
            and pm_fields["selected_pm_reserve_released"]
            and pm_fields["selected_pm_pending_after"] == 0
        ),
    }
else:
    selected_doc = {
        "record_type": "runls14_hybrid_selected_backrun_bundle",
        "selection_result": "blocked_hybrid_join_incomplete",
        "source_target": "afxdp",
        "source_enrichment": "laserstream_processed",
        "provider": "jito_via_helius",
        "selected_target_tx_bytes_available": False,
        "selected_pm_reserve_ok": False,
        "bundle_envelope_built": False,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "record_type": "runls14_hybrid_bundle_payload",
        "bundle_payload_valid": False,
        "bundle_tx_count": 0,
        "txs_base64": [],
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "record_type": "runls14_hybrid_pm_reserve_report",
        "selection_result": "blocked_hybrid_join_incomplete",
        "selected_pm_reserve_ok": False,
        "pm_reserve_reconciled": False,
    }

selected_path.write_text(json.dumps(selected_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
payload_path.write_text(json.dumps(payload_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
pm_report_path.write_text(json.dumps(pm_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")

strict_ready = bool(
    selected
    and target_decode_ok
    and selected_doc.get("selected_target_pool_matches")
    and selected_doc.get("selected_target_mint_in_matches")
    and selected_doc.get("selected_target_mint_out_matches")
    and selected_doc.get("selected_pm_reserve_ok")
    and selected_doc.get("selected_expected_out_raw", 0) > 0
    and selected_doc.get("selected_min_out_raw", 0) > 0
    and selected_doc.get("selected_net_ev_after_tip", 0) > 0
)

if not strict_ready:
    nonzero = {key: value for key, value in blockers.items() if value}
    if nonzero:
        strict_result = max(nonzero.items(), key=lambda item: item[1])[0]
    else:
        strict_result = "blocked_hybrid_join_incomplete"
else:
    strict_result = "hybrid_join_artifact_ready"

runls14_result = "hybrid_backrun_join_ready_for_ls11e" if strict_ready else strict_result
summary.update({
    "selection_result": runls14_result,
    "hybrid_backrun_join_ready_for_ls11e": strict_ready,
    "strict_selection_result": strict_result,
    "hybrid_strict_probe_row_count": len(strict_rows),
    "hybrid_probe_row_count": len(probe_rows),
    "hybrid_strict_blockers": {key: value for key, value in blockers.items() if value},
    "source_target": "afxdp",
    "target_tx_bytes_source": "afxdp_target_tx_cache" if strict_ready else None,
    "source_enrichment": "laserstream_processed",
    "laserstream_enrichment_available": bool(probe_rows),
    "selected_artifact_path": str(selected_path),
    "payload_artifact_path": str(payload_path),
    "pm_report_path": str(pm_report_path),
    "provider": "jito_via_helius",
    "target_tx_bytes_available": bool(strict_ready),
    "selected_target_tx_bytes_available": bool(strict_ready),
    "selected_target_tx_decodes_locally": target_decode_ok,
    "selected_target_pool_matches": bool(selected_doc.get("selected_target_pool_matches")),
    "selected_target_mint_in_matches": bool(selected_doc.get("selected_target_mint_in_matches")),
    "selected_target_mint_out_matches": bool(selected_doc.get("selected_target_mint_out_matches")),
    "selected_pm_reserve_ok": bool(selected_doc.get("selected_pm_reserve_ok")),
    "selected_expected_out_raw": selected_doc.get("selected_expected_out_raw", 0),
    "selected_min_out_raw": selected_doc.get("selected_min_out_raw", 0),
    "selected_net_ev_after_tip": selected_doc.get("selected_net_ev_after_tip", 0),
    "bundle_tx_count": payload_doc.get("bundle_tx_count", 0),
    "target_tx_first": bool(payload_doc.get("target_tx_first")),
    "our_backrun_tx_second": bool(payload_doc.get("our_backrun_tx_second")),
    "tip_present": bool(payload_doc.get("tip_present")),
    "sendBundle_called": False,
    "wire_send": 0,
    "live_landed": 0,
})
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

if [[ ! -f "$SUMMARY_JSON_PATH" ]]; then
    fail "RunLS14 summary missing: $SUMMARY_JSON_PATH"
    finish
    exit 1
fi
if [[ ! -s "$JOIN_ROWS_PATH" ]]; then
    warn "RunLS14 join rows are empty: $JOIN_ROWS_PATH"
fi

readarray -t RUNLS14_FIELDS < <(python3 - "$SUMMARY_JSON_PATH" <<'PY'
import json
import sys
doc = json.load(open(sys.argv[1], "r", encoding="utf-8"))
print(doc.get("selection_result", ""))
print(str(bool(doc.get("hybrid_backrun_join_ready_for_ls11e"))).lower())
print(doc.get("matched_hybrid_backrun_join_count", 0))
print(doc.get("hybrid_bundle_ready_join_count", 0))
print(doc.get("laserstream_backrun_seen", 0))
print(doc.get("afxdp_backrun_seen", 0))
PY
)

SELECTION_RESULT="${RUNLS14_FIELDS[0]:-}"
HYBRID_READY_FOR_LS11E="${RUNLS14_FIELDS[1]:-false}"
MATCHED_JOIN_COUNT="${RUNLS14_FIELDS[2]:-0}"
BUNDLE_READY_JOIN_COUNT="${RUNLS14_FIELDS[3]:-0}"
LASERSTREAM_BACKRUN_SEEN="${RUNLS14_FIELDS[4]:-0}"
AFXDP_BACKRUN_SEEN="${RUNLS14_FIELDS[5]:-0}"

{
    echo "inner_result_dir=$INNER_STEP_DIR"
    echo "selection_result=$SELECTION_RESULT"
    echo "hybrid_backrun_join_ready_for_ls11e=$HYBRID_READY_FOR_LS11E"
    echo "matched_hybrid_backrun_join_count=$MATCHED_JOIN_COUNT"
    echo "hybrid_bundle_ready_join_count=$BUNDLE_READY_JOIN_COUNT"
    echo "laserstream_backrun_seen=$LASERSTREAM_BACKRUN_SEEN"
    echo "afxdp_backrun_seen=$AFXDP_BACKRUN_SEEN"
    echo "summary_json=$SUMMARY_JSON_PATH"
    echo "join_rows_jsonl=$JOIN_ROWS_PATH"
    echo "selected_artifact_json=$SELECTED_JSON_PATH"
    echo "bundle_payload_json=$PAYLOAD_JSON_PATH"
    echo "pm_report_json=$PM_REPORT_JSON_PATH"
} > "$OUTER_SUMMARY_FILE"

if [[ "$HYBRID_READY_FOR_LS11E" == "true" ]]; then
    pass "RunLS14 found a hybrid Backrun join candidate ready for LS11E no-submit freshness gate"
else
    fail "RunLS14 blocked safely: $SELECTION_RESULT"
fi

finish
[[ "$HYBRID_READY_FOR_LS11E" == "true" ]]

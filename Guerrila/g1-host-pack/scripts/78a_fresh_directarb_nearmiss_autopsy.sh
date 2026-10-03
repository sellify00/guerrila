#!/usr/bin/env bash
# 78A - autopsy fresh strict DirectArb near-miss rows from completed Run78 windows

source "$(dirname "$0")/lib.sh"
script_header "78A - Fresh DirectArb Near-Miss Autopsy"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78A_STEP:-epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78a_fresh_directarb_nearmiss_autopsy.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

SOURCE_STEPS_CSV="${SHADOW_RUN78A_SOURCE_STEPS:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout,epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout_rerun1}"
SOURCE_ROOTS_CSV="${SHADOW_RUN78A_SOURCE_ROOTS:-/dev/results,/home/ubuntu/Guerrila/g1-host-pack/results}"
PM_AUDIT_STEP="${SHADOW_RUN78A_PM_AUDIT_STEP:-epoch8_auto_strict_directarb_private_egress_run78pm_pm_core_balance_visibility_audit}"
PM_AUDIT_ROWS_PATH="${SHADOW_RUN78A_PM_AUDIT_ROWS_PATH:-$RESULTS_ROOT/$PM_AUDIT_STEP/rows.jsonl}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$SOURCE_STEPS_CSV" "$SOURCE_ROOTS_CSV" "$PM_AUDIT_ROWS_PATH" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

source_steps = [item.strip() for item in sys.argv[1].split(",") if item.strip()]
source_roots = [Path(item.strip()) for item in sys.argv[2].split(",") if item.strip()]
pm_audit_rows_path = Path(sys.argv[3])
summary_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
rows_csv_path = Path(sys.argv[7])

alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def b58decode(text: str) -> bytes:
    value = 0
    for char in text.strip():
        value = value * 58 + alphabet.index(char)
    raw = value.to_bytes((value.bit_length() + 7) // 8, "big") if value else b""
    pad = len(text) - len(text.lstrip("1"))
    return (b"\x00" * pad) + raw


def b58hex(text: str) -> str:
    try:
        return b58decode(text).hex()
    except Exception:
        return ""


def parse_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def normalize_route_family(value):
    raw = str(value or "").strip()
    mapping = {
        "0": "jito_bundle",
        "1": "harmonic_bundle",
        "2": "public_rpc_scout",
        "3": "tpu_quic_public",
        "4": "helius_sender_dual",
        "255": "drop",
    }
    return mapping.get(raw, raw)


def find_source_dir(step: str) -> Path | None:
    for root in source_roots:
        candidate = root / step
        if candidate.exists():
            return candidate
    return None


def shock_identity_matches(row, signature_hex: str, pool_hex: str, mint_in_hex: str, mint_out_hex: str) -> bool:
    if signature_hex and str(row.get("signature_hex") or "").lower() != signature_hex.lower():
        return False
    if pool_hex and str(row.get("pool_pubkey_hex") or "").lower() != pool_hex.lower():
        return False
    if mint_in_hex and str(row.get("mint_in_hex") or "").lower() != mint_in_hex.lower():
        return False
    if mint_out_hex and str(row.get("mint_out_hex") or "").lower() != mint_out_hex.lower():
        return False
    return True


def derive_pm_blocker(row) -> tuple[str, str]:
    reserve_ok = as_bool(row.get("pm_reserve_can_lock_exact_input"))
    required_raw = as_int(row.get("exact_input_required_raw"))
    available_raw = as_int(row.get("current_input_balance_raw"))
    inventory_ready = as_bool(row.get("inventory_ready"))
    approved_held_input = as_bool(row.get("approved_held_input"))
    if reserve_ok:
        return "none", "proven"
    if not inventory_ready or not approved_held_input:
        return "inventory_fence_missing_mint", "inferred_from_inventory_gate"
    if available_raw <= 0:
        return "wrong_token_account_or_zero_balance", "inferred_from_zero_balance_snapshot"
    if required_raw > 0 and available_raw < required_raw:
        return "insufficient_input_balance", "proven_from_exact_input_vs_balance"
    return "pm_schema_gap", "fail_closed"


def derive_quote_blocker(row) -> str:
    if not as_bool(row.get("quote_numeric_present")):
        return "quote_numeric_missing"
    if not as_bool(row.get("min_out_positive")):
        return "blocked_min_out_invalid"
    if as_int(row.get("min_out_raw")) <= 0:
        return "blocked_min_out_invalid"
    return "none"


def derive_ev_math_blocker(row) -> str:
    if not as_bool(row.get("candidate_ev_math_available")):
        return str(row.get("candidate_ev_math_basis") or "ev_math_unavailable")
    if not as_bool(row.get("candidate_ev_positive")):
        return "ev_not_positive"
    if not as_bool(row.get("candidate_meets_configured_ev_floor")):
        return "configured_ev_floor_not_met"
    return "none"


def best_shadow_markout(rows, route_label: str):
    candidates = []
    for row in rows:
        if normalize_route_family(row.get("route_family_label") or row.get("route_family")) != route_label:
            continue
        value = row.get("net_markout_after_fee_lamports")
        if value is None:
            continue
        candidates.append(as_int(value))
    return max(candidates) if candidates else None


resolved_sources = []
all_rows = []
malformed_totals = {"rejections": 0, "shock": 0}
pm_audit_rows, _ = parse_jsonl(pm_audit_rows_path)
pm_audit_by_key = {
    (str(row.get("source_step") or ""), str(row.get("signature") or ""), str(row.get("pool") or "")): row
    for row in pm_audit_rows
}

for step in source_steps:
    source_dir = find_source_dir(step)
    if source_dir is None:
        resolved_sources.append({"step": step, "status": "missing"})
        continue

    rejection_path = source_dir / "run78c_selector_rejections.jsonl"
    shock_path = source_dir / "_inner" / "20_g12_r0_observe_only" / "shock_research.jsonl"
    rejection_rows, rejection_bad = parse_jsonl(rejection_path)
    shock_rows, shock_bad = parse_jsonl(shock_path)
    malformed_totals["rejections"] += rejection_bad
    malformed_totals["shock"] += shock_bad

    shock_by_signature = defaultdict(list)
    for shock_row in shock_rows:
        signature_hex = str(shock_row.get("signature_hex") or "").lower()
        if signature_hex:
            shock_by_signature[signature_hex].append(shock_row)

    afxdp_directarb_rows = [
        row
        for row in rejection_rows
        if str(row.get("source") or "") == "afxdp"
        and str(row.get("owner_mode") or "") == "directarb"
    ]

    resolved_sources.append(
        {
            "step": step,
            "status": "ok",
            "source_dir": str(source_dir),
            "afxdp_directarb_rows": len(afxdp_directarb_rows),
            "rejection_rows": len(rejection_rows),
            "shock_rows": len(shock_rows),
            "malformed_rejection_rows": rejection_bad,
            "malformed_shock_rows": shock_bad,
        }
    )

    for row in afxdp_directarb_rows:
        signature_hex = b58hex(str(row.get("signature") or ""))
        pool_hex = b58hex(str(row.get("pool") or ""))
        mint_in_hex = b58hex(str(row.get("mint_in") or ""))
        mint_out_hex = b58hex(str(row.get("mint_out") or ""))
        matched_shock_rows = [
            shock_row
            for shock_row in shock_by_signature.get(signature_hex, [])
            if shock_identity_matches(shock_row, signature_hex, pool_hex, mint_in_hex, mint_out_hex)
        ]
        private_directarb_rows = [
            shock_row
            for shock_row in matched_shock_rows
            if normalize_route_family(shock_row.get("route_family_label") or shock_row.get("route_family"))
            in {"helius_sender_dual", "jito_bundle"}
            and str(shock_row.get("record_type") or "") == "rollout_shadow"
            and str(shock_row.get("owner_mode") or "") == "directarb"
        ]
        private_shadow_owner_modes = sorted(
            {
                str(shock_row.get("owner_mode") or "")
                for shock_row in matched_shock_rows
                if normalize_route_family(shock_row.get("route_family_label") or shock_row.get("route_family"))
                in {"helius_sender_dual", "jito_bundle"}
            }
        )

        if private_directarb_rows:
            private_route_ev_after_fee_tip = max(
                as_int(shock_row.get("retained_ev_lamports_lcb")) - as_int(shock_row.get("tip_lamports")) - 5000
                for shock_row in private_directarb_rows
            )
            private_route_ev_proven = True
            private_route_ev_proof_status = "directarb_private_shadow"
        else:
            private_route_ev_after_fee_tip = None
            private_route_ev_proven = False
            if any(
                normalize_route_family(shock_row.get("route_family_label") or shock_row.get("route_family"))
                in {"helius_sender_dual", "jito_bundle"}
                for shock_row in matched_shock_rows
            ):
                private_route_ev_proof_status = "owner_mode_mismatch_shadow_only"
            else:
                private_route_ev_proof_status = "no_private_route_shadow"

        pm_audit = pm_audit_by_key.get((step, str(row.get("signature") or ""), str(row.get("pool") or "")), {})
        pm_reserve_blocker = str(pm_audit.get("pm_reserve_blocker") or derive_pm_blocker(row)[0])
        pm_reserve_blocker_proof_status = (
            "audit_visible_balance_classification"
            if pm_audit
            else derive_pm_blocker(row)[1]
        )
        input_account_resolution_blocker = str(
            pm_audit.get("input_account_resolution_blocker") or "none"
        )

        row_out = {
            "source_step": step,
            "signature": str(row.get("signature") or ""),
            "pool": str(row.get("pool") or ""),
            "venue": str(row.get("venue") or row.get("venue_name") or row.get("venue_label") or ""),
            "mint_in": str(row.get("mint_in") or ""),
            "mint_out": str(row.get("mint_out") or ""),
            "route_family": normalize_route_family(row.get("route")),
            "owner_mode": str(row.get("owner_mode") or ""),
            "quote_request_id": as_int(row.get("quote_request_id")),
            "template_id": as_int(row.get("template_id")),
            "ingress_unix_ns": as_int(row.get("ingress_unix_ns")),
            "quote_expected_out_raw": as_int(row.get("expected_out_raw")),
            "min_out_raw": as_int(row.get("min_out_raw")),
            "net_ev_before_fee_tip": as_int(row.get("candidate_retained_ev_raw")),
            "net_ev_after_fee_tip": as_int(row.get("candidate_net_after_fee_raw")),
            "tip_lamports": as_int(row.get("policy_tip_raw")),
            "base_fee": as_int(row.get("base_fee_raw")),
            "private_route_ev_after_fee_tip": private_route_ev_after_fee_tip,
            "private_route_ev_after_fee_tip_proven": private_route_ev_proven,
            "private_route_ev_proof_status": private_route_ev_proof_status,
            "best_jito_bundle_shadow_markout_after_fee_lamports": best_shadow_markout(
                matched_shock_rows, "jito_bundle"
            ),
            "best_tpu_shadow_markout_after_fee_lamports": best_shadow_markout(
                matched_shock_rows, "tpu_quic_public"
            ),
            "tpu_route_ev_after_fee_tip": as_int(row.get("candidate_net_after_fee_raw")),
            "pm_required_input_raw": as_int(row.get("exact_input_required_raw")),
            "pm_available_input_raw": as_int(
                row.get("pm_available_input_raw")
                if row.get("pm_available_input_raw") is not None
                else row.get("current_input_balance_raw")
            ),
            "selected_input_token_account": str(row.get("selected_input_token_account") or ""),
            "pm_reserved_input_account": str(
                row.get("pm_reserved_input_account") or row.get("selected_input_token_account") or ""
            ),
            "selected_input_account_balance_raw": as_int(
                row.get("selected_input_account_balance_raw")
            ),
            "template_input_token_account": str(row.get("template_input_token_account") or ""),
            "configured_wsol_token_account": str(row.get("configured_wsol_token_account") or ""),
            "input_account_resolution_method": str(
                row.get("input_account_resolution_method") or row.get("fallback_resolution_method") or ""
            ),
            "template_pm_account_match": as_bool(
                row.get("template_pm_account_match")
            ) or (
                str(row.get("template_input_token_account") or "") != ""
                and str(row.get("template_input_token_account") or "")
                == str(row.get("pm_reserved_input_account") or row.get("selected_input_token_account") or "")
            ),
            "pm_available_fee_balance": None,
            "pm_available_tip_balance": None,
            "pm_reserve_blocker": pm_reserve_blocker,
            "pm_reserve_blocker_proof_status": pm_reserve_blocker_proof_status,
            "input_account_resolution_blocker": input_account_resolution_blocker,
            "quote_blocker": derive_quote_blocker(row),
            "ev_math_blocker": derive_ev_math_blocker(row),
            "candidate_ev_math_basis": str(row.get("candidate_ev_math_basis") or ""),
            "projection_basis": str(row.get("projection_basis") or ""),
            "projection_consistent_with_observed_tx": as_bool(
                row.get("projection_consistent_with_observed_tx")
            ),
            "pm_reserve_can_lock_exact_input": as_bool(row.get("pm_reserve_can_lock_exact_input")),
            "inventory_ready": as_bool(row.get("inventory_ready")),
            "output_account_ready": as_bool(row.get("output_account_ready")),
            "template_ready": as_bool(row.get("template_ready")),
            "quote_numeric_present": as_bool(row.get("quote_numeric_present")),
            "min_out_positive": as_bool(row.get("min_out_positive")),
            "candidate_ev_math_available": as_bool(row.get("candidate_ev_math_available")),
            "candidate_ev_positive": as_bool(row.get("candidate_ev_positive")),
            "private_shadow_owner_modes": private_shadow_owner_modes,
            "matched_shadow_rows": len(matched_shock_rows),
            "first_failed": str(row.get("first_failed_gate") or ""),
        }
        all_rows.append(row_out)

fieldnames = [
    "source_step",
    "signature",
    "pool",
    "venue",
    "mint_in",
    "mint_out",
    "route_family",
    "owner_mode",
    "quote_request_id",
    "template_id",
    "ingress_unix_ns",
    "quote_expected_out_raw",
    "min_out_raw",
    "net_ev_before_fee_tip",
    "net_ev_after_fee_tip",
    "tip_lamports",
    "base_fee",
    "private_route_ev_after_fee_tip",
    "private_route_ev_after_fee_tip_proven",
    "private_route_ev_proof_status",
    "best_jito_bundle_shadow_markout_after_fee_lamports",
    "best_tpu_shadow_markout_after_fee_lamports",
    "tpu_route_ev_after_fee_tip",
    "pm_required_input_raw",
    "pm_available_input_raw",
    "selected_input_token_account",
    "pm_reserved_input_account",
    "selected_input_account_balance_raw",
    "template_input_token_account",
    "configured_wsol_token_account",
    "input_account_resolution_method",
    "template_pm_account_match",
    "pm_available_fee_balance",
    "pm_available_tip_balance",
    "pm_reserve_blocker",
    "pm_reserve_blocker_proof_status",
    "input_account_resolution_blocker",
    "quote_blocker",
    "ev_math_blocker",
    "candidate_ev_math_basis",
    "projection_basis",
    "projection_consistent_with_observed_tx",
    "pm_reserve_can_lock_exact_input",
    "inventory_ready",
    "output_account_ready",
    "template_ready",
    "quote_numeric_present",
    "min_out_positive",
    "candidate_ev_math_available",
    "candidate_ev_positive",
    "private_shadow_owner_modes",
    "matched_shadow_rows",
    "first_failed",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in all_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in all_rows:
        csv_row = dict(row)
        csv_row["private_shadow_owner_modes"] = ",".join(csv_row["private_shadow_owner_modes"])
        writer.writerow(csv_row)

first_failed_counts = Counter(row["first_failed"] for row in all_rows)
pm_blocker_counts = Counter(row["pm_reserve_blocker"] for row in all_rows)
input_account_resolution_blocker_counts = Counter(
    row["input_account_resolution_blocker"] for row in all_rows
)
ev_blocker_counts = Counter(row["ev_math_blocker"] for row in all_rows)
quote_blocker_counts = Counter(row["quote_blocker"] for row in all_rows)
private_ev_proof_status_counts = Counter(row["private_route_ev_proof_status"] for row in all_rows)

summary = {
    "status": "run78a_complete" if all_rows else "blocked_no_afxdp_directarb_rows",
    "source_steps_requested": source_steps,
    "source_steps_resolved": resolved_sources,
    "afxdp_directarb_row_count": len(all_rows),
    "first_failed_counts": dict(first_failed_counts),
    "pm_reserve_blocker_counts": dict(pm_blocker_counts),
    "input_account_resolution_blocker_counts": dict(input_account_resolution_blocker_counts),
    "ev_math_blocker_counts": dict(ev_blocker_counts),
    "quote_blocker_counts": dict(quote_blocker_counts),
    "private_route_ev_proof_status_counts": dict(private_ev_proof_status_counts),
    "pm_failed_near_miss_count": sum(1 for row in all_rows if row["first_failed"] == "pm_reserve_can_lock_exact_input"),
    "ev_not_positive_count": sum(1 for row in all_rows if row["first_failed"] == "ev_not_positive"),
    "ev_math_unavailable_count": sum(1 for row in all_rows if row["first_failed"] == "ev_math_unavailable"),
    "quote_numeric_missing_count": sum(1 for row in all_rows if row["first_failed"] == "quote_numeric_missing"),
    "malformed_rejection_rows": malformed_totals["rejections"],
    "malformed_shock_rows": malformed_totals["shock"],
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"afxdp_directarb_row_count={summary['afxdp_directarb_row_count']}",
    f"pm_failed_near_miss_count={summary['pm_failed_near_miss_count']}",
    f"ev_not_positive_count={summary['ev_not_positive_count']}",
    f"ev_math_unavailable_count={summary['ev_math_unavailable_count']}",
    f"quote_numeric_missing_count={summary['quote_numeric_missing_count']}",
    "first_failed_counts=" + json.dumps(summary["first_failed_counts"], separators=(",", ":")),
    "pm_reserve_blocker_counts=" + json.dumps(summary["pm_reserve_blocker_counts"], separators=(",", ":")),
    "input_account_resolution_blocker_counts="
    + json.dumps(summary["input_account_resolution_blocker_counts"], separators=(",", ":")),
    "ev_math_blocker_counts=" + json.dumps(summary["ev_math_blocker_counts"], separators=(",", ":")),
    "quote_blocker_counts=" + json.dumps(summary["quote_blocker_counts"], separators=(",", ":")),
    "private_route_ev_proof_status_counts="
    + json.dumps(summary["private_route_ev_proof_status_counts"], separators=(",", ":")),
    f"malformed_rejection_rows={summary['malformed_rejection_rows']}",
    f"malformed_shock_rows={summary['malformed_shock_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78A near-miss autopsy complete"
finish

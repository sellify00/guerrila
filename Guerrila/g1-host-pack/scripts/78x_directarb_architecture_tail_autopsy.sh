#!/usr/bin/env bash
# 78X - classify remaining architecture tails across fresh strict DirectArb Run78 batches

source "$(dirname "$0")/lib.sh"
script_header "78X - DirectArb Architecture Tail Autopsy"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN78X_STEP:-epoch8_auto_strict_directarb_architecture_tail_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78x_directarb_architecture_tail_autopsy.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"

BATCH_STEPS_CSV="${SHADOW_RUN78X_BATCH_STEPS:-epoch8_auto_strict_directarb_private_egress_run78_batch1,epoch8_auto_strict_directarb_private_egress_run78_batch2,epoch8_auto_strict_directarb_private_egress_run78_batch3}"
SOURCE_ROOTS_CSV="${SHADOW_RUN78X_SOURCE_ROOTS:-/dev/results,$RESULTS_ROOT}"
CORE_INPUT_MINTS_CSV="${SHADOW_RUN78X_CORE_INPUT_MINTS:-So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v,Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2}"
RUN78A_STEPS_CSV="${SHADOW_RUN78X_RUN78A_STEPS:-}"
RUN78PM_STEPS_CSV="${SHADOW_RUN78X_RUN78PM_STEPS:-}"
RUN78INV_STEPS_CSV="${SHADOW_RUN78X_RUN78INV_STEPS:-}"
RUN78H_STEPS_CSV="${SHADOW_RUN78X_RUN78H_STEPS:-}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$RESULTS_ROOT" "$BATCH_STEPS_CSV" "$SOURCE_ROOTS_CSV" "$CORE_INPUT_MINTS_CSV" "$RUN78A_STEPS_CSV" "$RUN78PM_STEPS_CSV" "$RUN78INV_STEPS_CSV" "$RUN78H_STEPS_CSV" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" <<'PY'
from __future__ import annotations

import csv
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

results_root = Path(sys.argv[1])
batch_steps = [item.strip() for item in sys.argv[2].split(",") if item.strip()]
source_roots = [Path(item.strip()) for item in sys.argv[3].split(",") if item.strip()]
core_input_mints = {item.strip() for item in sys.argv[4].split(",") if item.strip()}
run78a_steps = [item.strip() for item in sys.argv[5].split(",") if item.strip()]
run78pm_steps = [item.strip() for item in sys.argv[6].split(",") if item.strip()]
run78inv_steps = [item.strip() for item in sys.argv[7].split(",") if item.strip()]
run78h_steps = [item.strip() for item in sys.argv[8].split(",") if item.strip()]
summary_path = Path(sys.argv[9])
summary_json_path = Path(sys.argv[10])
rows_jsonl_path = Path(sys.argv[11])
rows_csv_path = Path(sys.argv[12])


def parse_jsonl(path: Path):
    rows = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for idx, raw in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines()):
        raw = raw.strip()
        if not raw:
            continue
        try:
            row = json.loads(raw)
            row["__row_index"] = idx
            rows.append(row)
        except Exception:
            malformed += 1
    return rows, malformed


def parse_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def batch_id_from_step(step: str) -> str:
    if "run78_" in step:
        return step.rsplit("run78_", 1)[-1]
    return step


def csv_override_value(values: list[str], index: int) -> str | None:
    if not values:
        return None
    if index < len(values):
        return values[index]
    return None


def derived_related_steps(batch_step: str, batch_id: str, kind: str) -> list[str]:
    suffix_map = {
        "run78a": [
            batch_step.replace("run78_batch", "run78a_batch"),
            batch_step.replace("run78_fresh_candidate_scout", "run78a_nearmiss_autopsy"),
            f"{batch_step}_78a_nearmiss_autopsy",
            f"epoch8_auto_strict_directarb_private_egress_run78a_{batch_id}",
        ],
        "run78pm": [
            batch_step.replace("run78_batch", "run78pm_batch"),
            batch_step.replace("run78_fresh_candidate_scout", "run78pm_pm_core_balance_visibility_audit"),
            f"{batch_step}_78pm_pm_core_balance_visibility_audit",
            f"epoch8_auto_strict_directarb_private_egress_run78pm_{batch_id}",
        ],
        "run78inv": [
            batch_step.replace("run78_batch", "run78inv_batch"),
            batch_step.replace("run78_fresh_candidate_scout", "run78inv_wsol_input_account_resolution_audit"),
            f"{batch_step}_78inv_wsol_input_account_resolution_audit",
            f"epoch8_auto_strict_directarb_private_egress_run78inv_{batch_id}",
        ],
        "run78h": [
            batch_step.replace("run78_batch", "run78h_batch"),
            batch_step.replace("run78_fresh_candidate_scout", "run78h_directarb_structural_hunt_readiness"),
            f"{batch_step}_78h_directarb_structural_hunt_readiness",
            f"epoch8_auto_strict_directarb_private_egress_run78h_{batch_id}",
        ],
    }
    seen = set()
    out = []
    for candidate in suffix_map.get(kind, []):
        candidate = candidate.strip()
        if candidate and candidate not in seen:
            seen.add(candidate)
            out.append(candidate)
    return out


def resolve_related_step(index: int, batch_step: str, kind: str) -> str | None:
    override_lists = {
        "run78a": run78a_steps,
        "run78pm": run78pm_steps,
        "run78inv": run78inv_steps,
        "run78h": run78h_steps,
    }
    override = csv_override_value(override_lists.get(kind, []), index)
    if override:
        return override
    batch_id = batch_id_from_step(batch_step)
    for candidate in derived_related_steps(batch_step, batch_id, kind):
        if (results_root / candidate).exists():
            return candidate
        if kind == "run78h" and (results_root / candidate / "summary.txt").exists():
            return candidate
        if kind != "run78h" and (results_root / candidate / "rows.jsonl").exists():
            return candidate
    candidates = derived_related_steps(batch_step, batch_id, kind)
    return candidates[0] if candidates else None


def find_source_dir(step: str) -> Path | None:
    for root in source_roots:
        candidate = root / step
        if candidate.exists():
            return candidate
    return None


def amount_in_key(row: dict) -> int:
    for key in (
        "pm_required_input_raw",
        "required_input_raw",
        "exact_input_required_raw",
        "amount_in_raw",
        "selected_input_amount_raw",
    ):
        value = safe_int(row.get(key))
        if value > 0:
            return value
    return 0


def identity_keys(row: dict, source_step: str, batch_id: str):
    signature = str(row.get("signature") or "").strip()
    pool = str(row.get("pool") or "").strip()
    mint_in = str(row.get("mint_in") or "").strip()
    mint_out = str(row.get("mint_out") or "").strip()
    amount_in = amount_in_key(row)
    row_index = safe_int(row.get("__row_index"))
    candidate_id = str(row.get("candidate_id") or "").strip()

    keys = []
    if candidate_id:
        keys.append(("candidate_id", candidate_id))
    if signature and pool and mint_in and mint_out and amount_in > 0:
        keys.append(("sig_pool_pair_amount", source_step, signature, pool, mint_in, mint_out, amount_in))
    if signature and pool and mint_in and mint_out:
        keys.append(("sig_pool_pair", source_step, signature, pool, mint_in, mint_out))
    if signature and pool and mint_in:
        keys.append(("sig_pool_mint_in", source_step, signature, pool, mint_in))
    if signature and pool:
        keys.append(("sig_pool", source_step, signature, pool))
    keys.append(("batch_row_index", batch_id, row_index))
    return keys


def build_index(rows: list[dict], source_step: str, batch_id: str):
    index: dict[tuple, list[dict]] = defaultdict(list)
    for row in rows:
        for key in identity_keys(row, source_step, batch_id):
            index[key].append(row)
    return index


def first_matching_row(index: dict[tuple, list[dict]], row: dict, source_step: str, batch_id: str):
    for key in identity_keys(row, source_step, batch_id):
        candidates = index.get(key) or []
        if candidates:
            return candidates[0]
    return {}


def classify_quote_missing_reason(row: dict) -> str:
    quote_blocker = str(row.get("quote_blocker") or "")
    if quote_blocker != "quote_numeric_missing" and str(row.get("first_failed") or "") != "quote_numeric_missing":
        return "none"
    expected = safe_int(row.get("quote_expected_out_raw"))
    min_out = safe_int(row.get("min_out_raw"))
    if expected <= 0 and min_out <= 0:
        return "non_numeric_artifact"
    if safe_int(row.get("quote_request_id")) <= 0 and safe_int(row.get("template_id")) <= 0:
        return "missing_quote_identity"
    return "quote_numeric_missing"


def classify_template_missing_reason(row: dict) -> str:
    if as_bool(row.get("template_ready")):
        return "none"
    if not as_bool(row.get("output_account_ready")):
        return "output_account_not_ready"
    if safe_int(row.get("quote_expected_out_raw")) <= 0:
        return "quote_missing_cascade"
    return "template_not_ready"


def determine_architecture_blocker(row: dict, pm_row: dict, inv_row: dict) -> str:
    pm_blocker = str(pm_row.get("pm_reserve_blocker") or "")
    inv_blocker = str(inv_row.get("input_account_resolution_blocker") or "")
    if pm_blocker == "stale_inventory_snapshot" or inv_blocker == "stale_inventory_snapshot":
        return "stale_inventory_snapshot"
    if str(row.get("first_failed") or "") == "quote_numeric_missing" or str(row.get("quote_blocker") or "") == "quote_numeric_missing":
        return "quote_numeric_missing"
    if not as_bool(row.get("template_ready")):
        return "template_not_ready"
    if str(row.get("first_failed") or "") == "ev_math_unavailable":
        return "ev_math_unavailable"
    return "none"


def classify_row(row: dict, pm_row: dict, inv_row: dict, rejection_row: dict) -> tuple[bool, str]:
    mint_in = str(row.get("mint_in") or "")
    route_family = str(row.get("route_family") or "")
    owner_mode = str(row.get("owner_mode") or rejection_row.get("owner_mode") or "")
    pm_blocker = str(pm_row.get("pm_reserve_blocker") or "")
    ev_math_blocker = str(row.get("ev_math_blocker") or "")
    quote_missing_reason = classify_quote_missing_reason(row)
    architecture_blocker = determine_architecture_blocker(row, pm_row, inv_row)
    live_track_scope_base = (
        mint_in in core_input_mints
        and route_family == "tpu_quic_public"
        and owner_mode == "directarb"
    )

    if pm_blocker == "unsupported_non_wsol_input":
        return live_track_scope_base, "OUT_OF_SCOPE_UNSUPPORTED_INPUT"
    if ev_math_blocker in {"unsupported_non_wsol_pair", "non_wsol_unverified"}:
        return live_track_scope_base, "PERMANENT_UNSUPPORTED_ROUTE_MATH"
    if quote_missing_reason == "non_numeric_artifact":
        if live_track_scope_base and str(row.get("first_failed") or "") == "quote_numeric_missing":
            return live_track_scope_base, "ARCH_FIX_REQUIRED_QUOTE_NUMERIC"
        return live_track_scope_base, "PERMANENT_NON_NUMERIC_ARTIFACT"
    if not live_track_scope_base and not as_bool(row.get("template_ready")):
        return live_track_scope_base, "OUT_OF_SCOPE_UNSUPPORTED_TEMPLATE"
    if architecture_blocker == "stale_inventory_snapshot":
        return live_track_scope_base, (
            "ARCH_FIX_REQUIRED_STALE_INVENTORY"
            if live_track_scope_base
            else "OUT_OF_SCOPE_UNSUPPORTED_INPUT"
        )
    if architecture_blocker == "quote_numeric_missing":
        return live_track_scope_base, (
            "ARCH_FIX_REQUIRED_QUOTE_NUMERIC"
            if live_track_scope_base
            else "OUT_OF_SCOPE_UNSUPPORTED_TEMPLATE"
        )
    if architecture_blocker == "template_not_ready":
        return live_track_scope_base, (
            "ARCH_FIX_REQUIRED_TEMPLATE"
            if live_track_scope_base
            else "OUT_OF_SCOPE_UNSUPPORTED_TEMPLATE"
        )
    if architecture_blocker == "ev_math_unavailable":
        return live_track_scope_base, (
            "ARCH_FIX_REQUIRED_EV_MATH"
            if live_track_scope_base
            else "PERMANENT_UNSUPPORTED_ROUTE_MATH"
        )
    return live_track_scope_base, "UNKNOWN"


row_like_h_keys = {
    "stale_inventory_snapshot",
    "quote_numeric_missing",
    "template_not_ready",
    "ev_math_unavailable",
    "template_pm_input_account_mismatch",
    "output_account_missing",
}

all_rows = []
classification_counts = Counter()
malformed_totals = Counter()
unmatched_h_counts = Counter()
summary_only_h_counts = Counter()
total_h_architecture_blockers = 0
total_h_row_like_architecture_blockers = 0
total_x_arch_fix_rows = 0

for batch_index, batch_step in enumerate(batch_steps):
    batch_id = batch_id_from_step(batch_step)
    source_dir = find_source_dir(batch_step)
    rejection_rows, malformed_rejections = parse_jsonl(
        (source_dir / "run78c_selector_rejections.jsonl") if source_dir else Path("/missing")
    )
    malformed_totals["rejections"] += malformed_rejections

    run78a_step = resolve_related_step(batch_index, batch_step, "run78a")
    run78pm_step = resolve_related_step(batch_index, batch_step, "run78pm")
    run78inv_step = resolve_related_step(batch_index, batch_step, "run78inv")
    run78h_step = resolve_related_step(batch_index, batch_step, "run78h")

    autopsy_rows_path = (results_root / run78a_step / "rows.jsonl") if run78a_step else Path("/missing")
    pm_rows_path = (results_root / run78pm_step / "rows.jsonl") if run78pm_step else Path("/missing")
    inv_rows_path = (results_root / run78inv_step / "rows.jsonl") if run78inv_step else Path("/missing")
    h_summary_path = (results_root / run78h_step / "summary.txt") if run78h_step else Path("/missing")

    autopsy_rows, malformed_autopsy = parse_jsonl(autopsy_rows_path)
    pm_rows, malformed_pm = parse_jsonl(pm_rows_path)
    inv_rows, malformed_inv = parse_jsonl(inv_rows_path)
    malformed_totals["autopsy"] += malformed_autopsy
    malformed_totals["pm"] += malformed_pm
    malformed_totals["inv"] += malformed_inv

    rejection_index = build_index(rejection_rows, batch_step, batch_id)
    pm_index = build_index(pm_rows, batch_step, batch_id)
    inv_index = build_index(inv_rows, batch_step, batch_id)

    batch_rows = []
    for row in autopsy_rows:
        first_failed = str(row.get("first_failed") or "")
        pm_row = first_matching_row(pm_index, row, batch_step, batch_id)
        inv_row = first_matching_row(inv_index, row, batch_step, batch_id)
        if first_failed not in {"quote_numeric_missing", "ev_math_unavailable"} and as_bool(row.get("template_ready")):
            pm_blocker = str((pm_row or {}).get("pm_reserve_blocker") or "")
            if pm_blocker not in {"stale_inventory_snapshot", "unsupported_non_wsol_input"}:
                continue

        rejection_row = first_matching_row(rejection_index, row, batch_step, batch_id)
        is_live_track_scope, classification = classify_row(row, pm_row or {}, inv_row or {}, rejection_row or {})
        quote_missing_reason = classify_quote_missing_reason(row)
        template_missing_reason = classify_template_missing_reason(row)
        architecture_blocker = determine_architecture_blocker(row, pm_row or {}, inv_row or {})
        classification_counts[classification] += 1

        selected_input_token_account = str(
            row.get("selected_input_token_account")
            or (rejection_row or {}).get("selected_input_token_account")
            or ""
        )
        pm_reserved_input_account = str(
            row.get("pm_reserved_input_account")
            or (rejection_row or {}).get("pm_reserved_input_account")
            or selected_input_token_account
        )
        template_input_token_account = str(
            row.get("template_input_token_account")
            or (rejection_row or {}).get("template_input_token_account")
            or ""
        )

        out_row = {
            "batch_id": batch_id,
            "source_step": batch_step,
            "run78a_step": run78a_step or "",
            "run78pm_step": run78pm_step or "",
            "run78inv_step": run78inv_step or "",
            "run78h_step": run78h_step or "",
            "signature": str(row.get("signature") or ""),
            "pool": str(row.get("pool") or ""),
            "venue": str(
                row.get("venue")
                or (rejection_row or {}).get("venue")
                or (rejection_row or {}).get("venue_name")
                or (rejection_row or {}).get("venue_label")
                or ""
            ),
            "mint_in": str(row.get("mint_in") or ""),
            "mint_out": str(row.get("mint_out") or ""),
            "route_family": str(row.get("route_family") or ""),
            "owner_mode": str(row.get("owner_mode") or (rejection_row or {}).get("owner_mode") or ""),
            "is_live_track_scope": is_live_track_scope,
            "first_failed": first_failed,
            "architecture_blocker": architecture_blocker,
            "required_input_raw": safe_int(row.get("pm_required_input_raw")),
            "selected_input_token_account": selected_input_token_account,
            "template_input_token_account": template_input_token_account,
            "pm_reserved_input_account": pm_reserved_input_account,
            "pm_available_input_raw": safe_int(
                row.get("pm_available_input_raw")
                or (pm_row or {}).get("pm_available_balance_raw")
                or (rejection_row or {}).get("pm_available_input_raw")
            ),
            "selected_input_account_balance_raw": safe_int(
                row.get("selected_input_account_balance_raw")
                or (rejection_row or {}).get("selected_input_account_balance_raw")
            ),
            "quote_expected_out_raw": safe_int(row.get("quote_expected_out_raw")),
            "min_out_raw": safe_int(row.get("min_out_raw")),
            "quote_identity_mode": (
                "ingress_identity"
                if safe_int(row.get("quote_request_id")) > 0 and safe_int(row.get("template_id")) > 0
                else "numeric_without_identity"
                if as_bool(row.get("quote_numeric_present"))
                else "missing"
            ),
            "quote_missing_reason": quote_missing_reason,
            "template_family": str(
                (rejection_row or {}).get("template_family")
                or (rejection_row or {}).get("template_kind")
                or (rejection_row or {}).get("template_name")
                or row.get("route_family")
                or ""
            ),
            "template_ready": as_bool(row.get("template_ready")),
            "template_missing_reason": template_missing_reason,
            "input_account_resolution_method": str(
                row.get("input_account_resolution_method")
                or (rejection_row or {}).get("input_account_resolution_method")
                or ""
            ),
            "template_pm_account_match": as_bool(
                row.get("template_pm_account_match")
                or (rejection_row or {}).get("template_pm_account_match")
            ) or (
                template_input_token_account != ""
                and template_input_token_account == pm_reserved_input_account
            ),
            "classification": classification,
        }
        all_rows.append(out_row)
        batch_rows.append(out_row)

    h_summary = parse_summary(h_summary_path)
    h_arch_counts = {}
    try:
        h_arch_counts = json.loads(h_summary.get("architecture_blocker_counts", "{}"))
    except Exception:
        h_arch_counts = {}
    h_architecture_blocker_count = sum(safe_int(v) for v in h_arch_counts.values())
    h_row_like_architecture_blocker_count = sum(
        safe_int(v) for k, v in h_arch_counts.items() if str(k) in row_like_h_keys
    )
    h_summary_only_architecture_blocker_count = max(
        h_architecture_blocker_count - h_row_like_architecture_blocker_count,
        0,
    )
    x_arch_fix_rows = sum(
        1 for row in batch_rows if str(row.get("classification") or "").startswith("ARCH_FIX_REQUIRED_")
    )
    unmatched = max(h_row_like_architecture_blocker_count - x_arch_fix_rows, 0)

    total_h_architecture_blockers += h_architecture_blocker_count
    total_h_row_like_architecture_blockers += h_row_like_architecture_blocker_count
    total_x_arch_fix_rows += x_arch_fix_rows
    unmatched_h_counts[batch_step] = unmatched
    summary_only_h_counts[batch_step] = h_summary_only_architecture_blocker_count

allowed_classifications = {
    "ARCH_FIX_REQUIRED_STALE_INVENTORY",
    "ARCH_FIX_REQUIRED_QUOTE_NUMERIC",
    "ARCH_FIX_REQUIRED_TEMPLATE",
    "ARCH_FIX_REQUIRED_EV_MATH",
    "OUT_OF_SCOPE_UNSUPPORTED_INPUT",
    "OUT_OF_SCOPE_UNSUPPORTED_TEMPLATE",
    "OUT_OF_SCOPE_DENY_POLICY",
    "PERMANENT_NON_NUMERIC_ARTIFACT",
    "PERMANENT_UNSUPPORTED_ROUTE_MATH",
}
unknown_architecture_tail_count = sum(
    1 for row in all_rows if row["classification"] not in allowed_classifications
)
live_track_scope_architecture_blocker_count = sum(
    1
    for row in all_rows
    if row["is_live_track_scope"] and str(row["classification"]).startswith("ARCH_FIX_REQUIRED_")
)
unmatched_78h_total = sum(unmatched_h_counts.values())

fieldnames = [
    "batch_id",
    "source_step",
    "run78a_step",
    "run78pm_step",
    "run78inv_step",
    "run78h_step",
    "signature",
    "pool",
    "venue",
    "mint_in",
    "mint_out",
    "route_family",
    "owner_mode",
    "is_live_track_scope",
    "first_failed",
    "architecture_blocker",
    "required_input_raw",
    "selected_input_token_account",
    "template_input_token_account",
    "pm_reserved_input_account",
    "pm_available_input_raw",
    "selected_input_account_balance_raw",
    "quote_expected_out_raw",
    "min_out_raw",
    "quote_identity_mode",
    "quote_missing_reason",
    "template_family",
    "template_ready",
    "template_missing_reason",
    "input_account_resolution_method",
    "template_pm_account_match",
    "classification",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in all_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", encoding="utf-8", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    for row in all_rows:
        writer.writerow(row)

summary = {
    "status": "run78x_complete" if all_rows else "blocked_no_architecture_tail_rows",
    "batch_steps": batch_steps,
    "architecture_tail_row_count": len(all_rows),
    "classification_counts": dict(classification_counts),
    "unknown_architecture_tail_count": unknown_architecture_tail_count,
    "live_track_scope_architecture_blocker_count": live_track_scope_architecture_blocker_count,
    "78h_architecture_blocker_count": total_h_row_like_architecture_blockers,
    "78h_summary_only_architecture_blocker_count": sum(summary_only_h_counts.values()),
    "78x_arch_fix_row_count": total_x_arch_fix_rows,
    "78x_unmatched_78h_blocker_count": unmatched_78h_total,
    "unmatched_78h_blockers_by_batch": dict(unmatched_h_counts),
    "summary_only_78h_blockers_by_batch": dict(summary_only_h_counts),
    "malformed_rejection_rows": malformed_totals["rejections"],
    "malformed_autopsy_rows": malformed_totals["autopsy"],
    "malformed_pm_rows": malformed_totals["pm"],
    "malformed_inv_rows": malformed_totals["inv"],
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"architecture_tail_row_count={summary['architecture_tail_row_count']}",
    "classification_counts=" + json.dumps(summary["classification_counts"], separators=(",", ":")),
    f"unknown_architecture_tail_count={summary['unknown_architecture_tail_count']}",
    f"live_track_scope_architecture_blocker_count={summary['live_track_scope_architecture_blocker_count']}",
    f"78h_architecture_blocker_count={summary['78h_architecture_blocker_count']}",
    f"78h_summary_only_architecture_blocker_count={summary['78h_summary_only_architecture_blocker_count']}",
    f"78x_arch_fix_row_count={summary['78x_arch_fix_row_count']}",
    f"78x_unmatched_78h_blocker_count={summary['78x_unmatched_78h_blocker_count']}",
    "unmatched_78h_blockers_by_batch=" + json.dumps(summary["unmatched_78h_blockers_by_batch"], separators=(",", ":")),
    "summary_only_78h_blockers_by_batch=" + json.dumps(summary["summary_only_78h_blockers_by_batch"], separators=(",", ":")),
    f"malformed_rejection_rows={summary['malformed_rejection_rows']}",
    f"malformed_autopsy_rows={summary['malformed_autopsy_rows']}",
    f"malformed_pm_rows={summary['malformed_pm_rows']}",
    f"malformed_inv_rows={summary['malformed_inv_rows']}",
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78X architecture-tail autopsy complete"
finish

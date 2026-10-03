#!/usr/bin/env bash
# 69_strict_surface_forensic_audit.sh - strict current-state forensic audit over fresh strict rows

source "$(dirname "$0")/lib.sh"
script_header "69 - Strict-Surface Forensic Audit"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN69_STEP:-69_strict_surface_forensic_audit}"
STATUS_PREFIX="${SHADOW_RUN69_STATUS_PREFIX:-run69_strict_surface_forensic_audit}"
REPORT_BASENAME="${SHADOW_RUN69_REPORT_BASENAME:-run69_strict_surface_forensic_audit}"

FORENSIC_STEP="${SHADOW_RUN69_FORENSIC_STEP:-68b_epoch2_post_ata_forensic_audit}"
RUN39_STEP="${SHADOW_RUN69_RUN39_STEP:-epoch2_canonical_monetization_run55_run39}"

FORENSIC_RESULT_DIR="$RESULTS_ROOT/$FORENSIC_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

FORENSIC_ROWS_JSONL="$FORENSIC_RESULT_DIR/epoch1_would_fire_rows.jsonl"
FORENSIC_SUMMARY_JSON="$FORENSIC_RESULT_DIR/epoch1_would_fire_forensic_summary.json"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
STRICT_ROWS_JSONL="$RESULT_DIR/strict_current_state_rows.jsonl"
DIRECTARB_STRICT_ROWS_JSONL="$RESULT_DIR/directarb_strict_rows.jsonl"
DIRECTARB_REPORT_JSON="$RESULT_DIR/directarb_strict_forensic_report.json"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STRICT_ROWS_JSONL" \
    "$DIRECTARB_STRICT_ROWS_JSONL" \
    "$DIRECTARB_REPORT_JSON"

missing=0
for required_path in \
    "$FORENSIC_ROWS_JSONL" \
    "$FORENSIC_SUMMARY_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required artifact is missing: $required_path"
        missing=1
    fi
done

if [[ "$missing" == "1" ]]; then
    finish
    exit 1
fi

python3 - \
    "$FORENSIC_ROWS_JSONL" \
    "$FORENSIC_SUMMARY_JSON" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STRICT_ROWS_JSONL" \
    "$DIRECTARB_STRICT_ROWS_JSONL" \
    "$DIRECTARB_REPORT_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}

strict_rows_input_path = Path(sys.argv[1])
forensic_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
strict_rows_path = Path(sys.argv[6])
directarb_rows_path = Path(sys.argv[7])
directarb_report_path = Path(sys.argv[8])
step = sys.argv[9]
status_prefix = sys.argv[10]


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text_value(value):
    return (value or "").strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def normalize_raw_amount(raw_value, decimals):
    if decimals is None:
        return None
    return safe_int(raw_value) / (10 ** decimals)


def equivalent_values(native_value, mint, sol_usd_equivalent):
    if native_value is None:
        return None, None
    if mint == WSOL_MINT:
        return native_value * sol_usd_equivalent, native_value
    if mint in {USDC_MINT, USDT_MINT}:
        return native_value, native_value / sol_usd_equivalent if sol_usd_equivalent else None
    return None, None


def percentile(values, pct):
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    ordered = sorted(values)
    index = (len(ordered) - 1) * pct
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return ordered[lower]
    fraction = index - lower
    return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction


def sum_known(rows, field):
    return round(sum(safe_float(row.get(field)) for row in rows if row.get(field) is not None), 9)


def count_true(rows, field):
    return sum(1 for row in rows if row.get(field))


def group_distribution(rows, key_fn):
    grouped = defaultdict(lambda: {"count": 0, "net_ev_usd": 0.0, "retained_ev_usd": 0.0})
    for row in rows:
        key = key_fn(row)
        grouped[key]["count"] += 1
        grouped[key]["net_ev_usd"] += safe_float(row.get("net_ev_usd"))
        grouped[key]["retained_ev_usd"] += safe_float(row.get("retained_ev_usd"))
    output = []
    for key, value in grouped.items():
        output.append(
            {
                "key": key,
                "count": value["count"],
                "net_ev_usd": round(value["net_ev_usd"], 9),
                "retained_ev_usd": round(value["retained_ev_usd"], 9),
            }
        )
    return sorted(output, key=lambda item: (item["net_ev_usd"], item["count"], item["key"]), reverse=True)


forensic_rows = load_jsonl(strict_rows_input_path)
forensic_summary = load_json(forensic_summary_path)
candidate_rows = load_jsonl(candidate_autopsy_path)
candidate_by_id = {
    text_value(row.get("candidate_id")): row
    for row in candidate_rows
    if text_value(row.get("candidate_id"))
}

sol_usd_equivalent = safe_float((forensic_summary.get("unit_audit") or {}).get("sol_usd_equivalent")) or 1000.0
strict_rows_input = [row for row in forensic_rows if row.get("strict_current_state")]

strict_rows = []
for row in strict_rows_input:
    candidate_id = text_value(row.get("candidate_id"))
    raw = candidate_by_id.get(candidate_id, {})
    output_decimals = row.get("output_decimals")
    input_decimals = row.get("input_decimals")
    expected_output_native = normalize_raw_amount(raw.get("quote_expected_out_lamports"), output_decimals)
    target_output_native = normalize_raw_amount(raw.get("quote_target_out_lamports"), output_decimals)
    min_output_native = normalize_raw_amount(raw.get("min_out_lamports"), output_decimals)
    adverse_native = normalize_raw_amount(
        ((raw.get("markouts") or {}).get("max_adverse_excursion_lamports")),
        input_decimals,
    )
    adverse_usd, adverse_sol = equivalent_values(adverse_native, text_value(row.get("mint_in")), sol_usd_equivalent)
    inventory_used = bool(row.get("inventory_ready"))
    ata_used = bool(row.get("output_account_ready"))
    record = {
        "row_id": candidate_id,
        "candidate_id": candidate_id,
        "slot": safe_int(row.get("slot")),
        "signature": row.get("signature"),
        "owner_mode": row.get("owner_mode"),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "route": row.get("observed_route"),
        "route_used": row.get("usable_route_for_us"),
        "source": row.get("source"),
        "input_native_size": row.get("input_native_size"),
        "input_amount_raw": safe_int(raw.get("trade_size_lamports")),
        "input_usd_equiv": row.get("input_usd_equiv"),
        "input_sol_equiv": row.get("input_sol_equiv"),
        "expected_output_raw_units": safe_int(raw.get("quote_expected_out_lamports")),
        "expected_output_native": round_or_none(expected_output_native, 9),
        "quote_target_out_raw_units": safe_int(raw.get("quote_target_out_lamports")),
        "quote_target_out_native": round_or_none(target_output_native, 9),
        "min_output_raw_units": safe_int(raw.get("min_out_lamports")),
        "min_output_native": round_or_none(min_output_native, 9),
        "slippage_bps": raw.get("quote_slippage_bps"),
        "output_decimals": output_decimals,
        "net_ev_input_native": row.get("net_after_fee_input_native"),
        "net_ev_usd": row.get("net_after_fee_usd"),
        "net_ev_sol": row.get("net_after_fee_sol"),
        "net_after_fee_raw_units": safe_int(raw.get("net_after_fee_lamports")),
        "retained_ev_input_native": row.get("retained_ev"),
        "retained_ev_usd": row.get("retained_ev_usd"),
        "retained_ev_sol": row.get("retained_ev_sol"),
        "retained_ev_raw_units": safe_int(raw.get("retained_ev_lamports") or row.get("retained_ev_raw_units")),
        "policy_tip_raw_units": safe_int(raw.get("policy_tip_lamports") or raw.get("tip_lamports")),
        "policy_tip_input_native": row.get("policy_tip"),
        "policy_tip_usd": row.get("policy_tip_usd"),
        "policy_tip_sol": row.get("policy_tip_sol"),
        "base_fee_raw_units": safe_int(raw.get("base_fee_lamports") or raw.get("estimated_public_fee_lamports") or row.get("base_fee_lamports")),
        "estimated_priority_fee_lamports": safe_int(raw.get("estimated_priority_fee_lamports")),
        "base_fee_lamports": row.get("base_fee_lamports"),
        "base_fee_input_native": row.get("base_fee_input_native"),
        "quote_source": text_value(raw.get("quote_source") or raw.get("signal_source")) or None,
        "quote_timestamp_or_slot": raw.get("quote_timestamp_or_slot") or raw.get("quote_unix_ns") or raw.get("unix_ns") or raw.get("slot"),
        "quote_request_id": raw.get("quote_request_id"),
        "template_id": raw.get("template_id"),
        "inventory_used": inventory_used,
        "inventory_balance_raw": safe_int(raw.get("input_balance_raw")),
        "inventory_input_mint": row.get("mint_in") if inventory_used else None,
        "ata_used": ata_used,
        "has_output_ata": bool(raw.get("has_output_ata")),
        "output_ata_initialized": bool(raw.get("output_ata_initialized")),
        "approved_held_input": bool(raw.get("approved_held_input") or raw.get("is_approved_held_input")),
        "input_account_ready": bool(raw.get("input_account_ready")),
        "output_account_ready": bool(raw.get("output_account_ready")),
        "template_ready": bool(raw.get("template_ready")),
        "quote_ready": bool(row.get("quote_ready")),
        "tpu_or_public_usable": row.get("usable_route_for_us") in USABLE_ROUTE_FAMILIES,
        "would_enqueue_tpu": bool(row.get("would_enqueue_tpu")),
        "duplicate_key": row.get("dedupe_key"),
        "duplicate_of_prior_event": bool(row.get("duplicate_of_prior_event")),
        "notional_bucket": row.get("notional_bucket"),
        "heuristic_ev_only": bool(row.get("heuristic_ev_only")),
        "max_adverse_excursion_input_native": round_or_none(adverse_native, 9),
        "max_adverse_excursion_usd": round_or_none(adverse_usd, 9),
        "max_adverse_excursion_sol": round_or_none(adverse_sol, 9),
        "max_adverse_excursion_raw_units": safe_int(((raw.get("markouts") or {}).get("max_adverse_excursion_lamports"))),
        "why_selected": row.get("why_selected"),
    }
    strict_rows.append(record)

strict_rows.sort(key=lambda item: (item["slot"], item["owner_mode"] or "", item["pool"] or "", item["signature"] or ""))

strict_unique_map = {}
strict_duplicate_rows = []
for row in strict_rows:
    dedupe_key = tuple(row.get("duplicate_key") or [])
    if dedupe_key and dedupe_key in strict_unique_map:
        strict_duplicate_rows.append(row)
    elif dedupe_key:
        strict_unique_map[dedupe_key] = row

strict_owner_distribution = {
    owner: {
        "count": len(scoped),
        "net_ev_usd": sum_known(scoped, "net_ev_usd"),
        "retained_ev_usd": sum_known(scoped, "retained_ev_usd"),
    }
    for owner in sorted({row["owner_mode"] for row in strict_rows})
    for scoped in [[row for row in strict_rows if row["owner_mode"] == owner]]
}

directarb_rows = [row for row in strict_rows if row.get("owner_mode") == "directarb"]
directarb_rows_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in directarb_rows) + "\n",
    encoding="utf-8",
)

directarb_unique_map = {}
directarb_duplicate_rows = []
for row in directarb_rows:
    dedupe_key = tuple(row.get("duplicate_key") or [])
    if dedupe_key and dedupe_key in directarb_unique_map:
        directarb_duplicate_rows.append(row)
    elif dedupe_key:
        directarb_unique_map[dedupe_key] = row

directarb_net_evs = [safe_float(row.get("net_ev_usd")) for row in directarb_rows if row.get("net_ev_usd") is not None]
directarb_adverse_usd = [safe_float(row.get("max_adverse_excursion_usd")) for row in directarb_rows if row.get("max_adverse_excursion_usd") is not None]
directarb_pair_distribution = group_distribution(
    directarb_rows,
    lambda row: f"{row.get('mint_in')}->{row.get('mint_out')}",
)
directarb_pool_distribution = group_distribution(directarb_rows, lambda row: row.get("pool") or "unknown_pool")
directarb_size_distribution = group_distribution(directarb_rows, lambda row: row.get("notional_bucket") or "unknown_bucket")

directarb_repeating_pool_pairs = [
    item
    for item in group_distribution(directarb_rows, lambda row: f"{row.get('pool')}::{row.get('mint_in')}->{row.get('mint_out')}")
    if item["count"] > 1
]

directarb_summary_input = forensic_summary.get("directarb") or {}
strict_vs_fixup = forensic_summary.get("strict_vs_fixup") or {}
route_discipline = forensic_summary.get("route_discipline") or {}

directarb_report = {
    "directarb_seen_total": safe_int(directarb_summary_input.get("directarb_seen")),
    "directarb_unique_total": safe_int(directarb_summary_input.get("directarb_unique")),
    "directarb_strict_rows": len(directarb_rows),
    "directarb_unique_count": len(directarb_unique_map) or len(directarb_rows),
    "directarb_duplicate_rows": len(directarb_duplicate_rows),
    "directarb_duplicate_ev_removed_usd": sum_known(directarb_duplicate_rows, "net_ev_usd"),
    "directarb_deduped_ev_usd": sum_known(list(directarb_unique_map.values()) or directarb_rows, "net_ev_usd"),
    "directarb_pool_distribution": directarb_pool_distribution,
    "directarb_mint_pair_distribution": directarb_pair_distribution,
    "directarb_size_distribution": directarb_size_distribution,
    "directarb_repeating_pool_mint_pairs": directarb_repeating_pool_pairs,
    "directarb_ev_total_usd": sum_known(directarb_rows, "net_ev_usd"),
    "directarb_retained_ev_total_usd": sum_known(directarb_rows, "retained_ev_usd"),
    "directarb_ev_median_usd": round_or_none(statistics.median(directarb_net_evs), 9) if directarb_net_evs else None,
    "directarb_ev_p90_usd": round_or_none(percentile(directarb_net_evs, 0.90), 9),
    "directarb_max_adverse_usd": round_or_none(min(directarb_adverse_usd), 9) if directarb_adverse_usd else None,
    "directarb_route_usable_count": count_true(directarb_rows, "tpu_or_public_usable"),
    "directarb_inventory_ready_count": count_true(directarb_rows, "inventory_used"),
    "directarb_output_account_ready_count": count_true(directarb_rows, "output_account_ready"),
    "directarb_template_ready_count": count_true(directarb_rows, "template_ready"),
    "directarb_rows": directarb_rows,
}

directarb_report_path.write_text(json.dumps(directarb_report, indent=2) + "\n", encoding="utf-8")
strict_rows_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in strict_rows) + "\n",
    encoding="utf-8",
)

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "forensic_rows_jsonl": str(strict_rows_input_path),
        "forensic_summary_json": str(forensic_summary_path),
        "candidate_autopsy_jsonl": str(candidate_autopsy_path),
    },
    "strict_surface": {
        "strict_row_count": len(strict_rows),
        "strict_unique_count": len(strict_unique_map) or len(strict_rows),
        "strict_duplicate_rows": len(strict_duplicate_rows),
        "strict_duplicate_ev_removed_usd": sum_known(strict_duplicate_rows, "net_ev_usd"),
        "strict_deduped_ev_usd": sum_known(list(strict_unique_map.values()) or strict_rows, "net_ev_usd"),
        "strict_ev_total_usd": sum_known(strict_rows, "net_ev_usd"),
        "strict_retained_ev_total_usd": sum_known(strict_rows, "retained_ev_usd"),
        "strict_owner_distribution": strict_owner_distribution,
        "strict_route_distribution": group_distribution(strict_rows, lambda row: row.get("route_used") or "unknown_route"),
        "heuristic_ev_only_count": count_true(strict_rows, "heuristic_ev_only"),
    },
    "directarb": directarb_report,
    "baseline_reconciliation": {
        "strict_current_state_would_fire": safe_int(strict_vs_fixup.get("strict_current_state_would_fire")),
        "strict_current_state_ev_usd": safe_float(strict_vs_fixup.get("strict_current_state_ev_usd")),
        "route_leak_count": safe_int(route_discipline.get("route_leak_count")),
        "jito_bundle_executable_count": safe_int(route_discipline.get("jito_bundle_executable_count")),
        "directarb_strict_would_fire": safe_int(directarb_summary_input.get("directarb_strict_would_fire")),
        "directarb_inventory_ready": safe_int(directarb_summary_input.get("directarb_inventory_ready")),
        "directarb_route_usable": safe_int(directarb_summary_input.get("directarb_route_usable")),
        "directarb_top_blocker": text_value(directarb_summary_input.get("directarb_top_blocker")) or "unknown",
    },
    "output_files": {
        "strict_current_state_rows_jsonl": str(strict_rows_path),
        "directarb_strict_rows_jsonl": str(directarb_rows_path),
        "directarb_strict_forensic_report_json": str(directarb_report_path),
    },
}

summary_json.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary_payload['status']}",
    f"strict_row_count={summary_payload['strict_surface']['strict_row_count']}",
    f"strict_unique_count={summary_payload['strict_surface']['strict_unique_count']}",
    f"strict_duplicate_rows={summary_payload['strict_surface']['strict_duplicate_rows']}",
    f"strict_deduped_ev_usd={summary_payload['strict_surface']['strict_deduped_ev_usd']}",
    f"strict_ev_total_usd={summary_payload['strict_surface']['strict_ev_total_usd']}",
    f"directarb_seen_total={directarb_report['directarb_seen_total']}",
    f"directarb_unique_total={directarb_report['directarb_unique_total']}",
    f"directarb_strict_rows={directarb_report['directarb_strict_rows']}",
    f"directarb_unique_count={directarb_report['directarb_unique_count']}",
    f"directarb_duplicate_rows={directarb_report['directarb_duplicate_rows']}",
    f"directarb_duplicate_ev_removed_usd={directarb_report['directarb_duplicate_ev_removed_usd']}",
    f"directarb_deduped_ev_usd={directarb_report['directarb_deduped_ev_usd']}",
    f"directarb_ev_total_usd={directarb_report['directarb_ev_total_usd']}",
    f"directarb_ev_median_usd={directarb_report['directarb_ev_median_usd']}",
    f"directarb_ev_p90_usd={directarb_report['directarb_ev_p90_usd']}",
    f"directarb_max_adverse_usd={directarb_report['directarb_max_adverse_usd']}",
    f"directarb_route_usable_count={directarb_report['directarb_route_usable_count']}",
    f"directarb_inventory_ready_count={directarb_report['directarb_inventory_ready_count']}",
    f"directarb_output_account_ready_count={directarb_report['directarb_output_account_ready_count']}",
    f"directarb_template_ready_count={directarb_report['directarb_template_ready_count']}",
    f"route_leak_count={summary_payload['baseline_reconciliation']['route_leak_count']}",
    f"jito_bundle_executable_count={summary_payload['baseline_reconciliation']['jito_bundle_executable_count']}",
    f"strict_current_state_rows_jsonl={strict_rows_path}",
    f"directarb_strict_rows_jsonl={directarb_rows_path}",
    f"directarb_strict_forensic_report_json={directarb_report_path}",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "Run69 strict-surface forensic audit written to $RESULT_DIR"
cat "$SUMMARY_FILE"
finish

#!/usr/bin/env bash
# 74_strict_directarb_live_readiness_forensic.sh - inspect fresh strict DirectArb rows for live-readiness

source "$(dirname "$0")/lib.sh"
script_header "74 - Strict DirectArb Live-Readiness Forensic"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN74_STEP:-74_strict_directarb_live_readiness_forensic}"
STATUS_PREFIX="${SHADOW_RUN74_STATUS_PREFIX:-run74_strict_directarb_live_readiness_forensic}"
REPORT_BASENAME="${SHADOW_RUN74_REPORT_BASENAME:-run74_strict_directarb_live_readiness_forensic}"
RUN69_STEP="${SHADOW_RUN74_RUN69_STEP:-epoch7b_run73b_metadata_output_ata_retry_post_run69_strict_surface}"
TOP_LIMIT="${SHADOW_RUN74_TOP_LIMIT:-20}"

RUN69_RESULT_DIR="$RESULTS_ROOT/$RUN69_STEP"
RUN69_SUMMARY_JSON="$RUN69_RESULT_DIR/${RUN69_STEP}.json"
DIRECTARB_ROWS_JSONL="$RUN69_RESULT_DIR/directarb_strict_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
TOP_ROWS_JSONL="$RESULT_DIR/top_directarb_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TOP_ROWS_JSONL"

missing=0
for required_path in \
    "$RUN69_SUMMARY_JSON" \
    "$DIRECTARB_ROWS_JSONL"
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
    "$RUN69_SUMMARY_JSON" \
    "$DIRECTARB_ROWS_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TOP_ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$TOP_LIMIT" <<'PY'
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

run69_summary_path = Path(sys.argv[1])
rows_path = Path(sys.argv[2])
summary_file_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
top_rows_jsonl_path = Path(sys.argv[5])
step = sys.argv[6]
status_prefix = sys.argv[7]
top_limit = max(1, int(sys.argv[8] or "20"))


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
    return str(value or "").strip()


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


def round_or_none(value, digits=6):
    if value is None:
        return None
    return round(value, digits)


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
        key = key_fn(row) or "unknown"
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
    return sorted(output, key=lambda item: (item["count"], item["net_ev_usd"], item["key"]), reverse=True)


def live_ready_now(row):
    return all(
        [
            bool(row.get("quote_ready")),
            bool(row.get("inventory_used")),
            bool(row.get("output_account_ready")),
            bool(row.get("template_ready")),
            bool(row.get("tpu_or_public_usable")),
            bool(row.get("would_enqueue_tpu")),
        ]
    )


rows = load_jsonl(rows_path)
run69_summary = load_json(run69_summary_path)
baseline = run69_summary.get("baseline_reconciliation") or {}

unique_map = {}
duplicate_rows = []
for row in rows:
    dedupe_key = tuple(row.get("duplicate_key") or [])
    if dedupe_key and dedupe_key in unique_map:
        duplicate_rows.append(row)
    elif dedupe_key:
        unique_map[dedupe_key] = row

unique_rows = list(unique_map.values()) or rows

route_distribution = group_distribution(rows, lambda row: row.get("route_used") or row.get("route"))
venue_distribution = group_distribution(rows, lambda row: row.get("venue"))
source_distribution = group_distribution(rows, lambda row: row.get("source"))
pair_distribution = group_distribution(rows, lambda row: f"{row.get('mint_in')}->{row.get('mint_out')}")
pool_distribution = group_distribution(rows, lambda row: row.get("pool"))
bucket_distribution = group_distribution(rows, lambda row: row.get("notional_bucket"))

net_evs = [safe_float(row.get("net_ev_usd")) for row in unique_rows if row.get("net_ev_usd") is not None]
retained_evs = [safe_float(row.get("retained_ev_usd")) for row in unique_rows if row.get("retained_ev_usd") is not None]
input_native_sizes = [safe_float(row.get("input_native_size")) for row in unique_rows if row.get("input_native_size") is not None]
input_usd_equivs = [safe_float(row.get("input_usd_equiv")) for row in unique_rows if row.get("input_usd_equiv") is not None]
input_sol_equivs = [safe_float(row.get("input_sol_equiv")) for row in unique_rows if row.get("input_sol_equiv") is not None]

top_rows = sorted(
    unique_rows,
    key=lambda row: (
        safe_float(row.get("net_ev_usd")),
        safe_float(row.get("retained_ev_usd")),
        safe_float(row.get("input_usd_equiv")),
        safe_int(row.get("slot")),
    ),
    reverse=True,
)[:top_limit]

smallest_live_ready_row = None
live_ready_rows = [row for row in unique_rows if live_ready_now(row)]
if live_ready_rows:
    smallest_live_ready_row = sorted(
        live_ready_rows,
        key=lambda row: (
            safe_float(row.get("input_usd_equiv")),
            safe_float(row.get("input_native_size")),
            -safe_float(row.get("net_ev_usd")),
            safe_int(row.get("slot")),
        ),
    )[0]

quote_numeric_preserved_count = sum(1 for row in unique_rows if safe_int(row.get("expected_output_raw_units")) > 0)
min_out_numeric_preserved_count = sum(1 for row in unique_rows if safe_int(row.get("min_output_raw_units")) > 0)

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run69_summary_json": str(run69_summary_path),
        "directarb_rows_jsonl": str(rows_path),
    },
    "headline": {
        "strict_directarb_rows": len(rows),
        "strict_directarb_unique_rows": len(unique_rows),
        "strict_directarb_duplicate_rows": len(duplicate_rows),
        "strict_directarb_deduped_net_ev_usd": sum_known(unique_rows, "net_ev_usd"),
        "strict_directarb_deduped_retained_ev_usd": sum_known(unique_rows, "retained_ev_usd"),
        "strict_directarb_total_net_ev_usd": sum_known(rows, "net_ev_usd"),
        "strict_directarb_total_retained_ev_usd": sum_known(rows, "retained_ev_usd"),
        "strict_directarb_net_ev_median_usd": round_or_none(statistics.median(net_evs), 9) if net_evs else None,
        "strict_directarb_net_ev_p90_usd": round_or_none(percentile(net_evs, 0.90), 9),
        "strict_directarb_retained_ev_median_usd": round_or_none(statistics.median(retained_evs), 9) if retained_evs else None,
        "strict_directarb_retained_ev_p90_usd": round_or_none(percentile(retained_evs, 0.90), 9),
    },
    "live_readiness": {
        "quote_ready_count": count_true(unique_rows, "quote_ready"),
        "inventory_ready_count": count_true(unique_rows, "inventory_used"),
        "approved_held_input_count": count_true(unique_rows, "approved_held_input"),
        "output_account_ready_count": count_true(unique_rows, "output_account_ready"),
        "template_ready_count": count_true(unique_rows, "template_ready"),
        "tpu_or_public_usable_count": count_true(unique_rows, "tpu_or_public_usable"),
        "would_enqueue_tpu_count": count_true(unique_rows, "would_enqueue_tpu"),
        "heuristic_ev_only_count": count_true(unique_rows, "heuristic_ev_only"),
        "quote_numeric_preserved_count": quote_numeric_preserved_count,
        "min_out_numeric_preserved_count": min_out_numeric_preserved_count,
        "numeric_quote_gap_count": sum(
            1
            for row in unique_rows
            if row.get("quote_ready")
            and safe_int(row.get("expected_output_raw_units")) == 0
            and safe_int(row.get("min_output_raw_units")) == 0
        ),
        "all_routes_tpu_quic_public": len({text_value(row.get("route_used")) for row in unique_rows}) == 1
        and all(text_value(row.get("route_used")) == "tpu_quic_public" for row in unique_rows),
        "all_rows_live_ready_now": all(live_ready_now(row) for row in unique_rows),
    },
    "distributions": {
        "routes": route_distribution,
        "venues": venue_distribution,
        "sources": source_distribution,
        "mint_pairs": pair_distribution,
        "pools": pool_distribution,
        "notional_buckets": bucket_distribution,
    },
    "size_profile": {
        "min_input_native_size": round_or_none(min(input_native_sizes), 9) if input_native_sizes else None,
        "median_input_native_size": round_or_none(statistics.median(input_native_sizes), 9) if input_native_sizes else None,
        "max_input_native_size": round_or_none(max(input_native_sizes), 9) if input_native_sizes else None,
        "min_input_usd_equiv": round_or_none(min(input_usd_equivs), 9) if input_usd_equivs else None,
        "median_input_usd_equiv": round_or_none(statistics.median(input_usd_equivs), 9) if input_usd_equivs else None,
        "max_input_usd_equiv": round_or_none(max(input_usd_equivs), 9) if input_usd_equivs else None,
        "min_input_sol_equiv": round_or_none(min(input_sol_equivs), 9) if input_sol_equivs else None,
        "median_input_sol_equiv": round_or_none(statistics.median(input_sol_equivs), 9) if input_sol_equivs else None,
        "max_input_sol_equiv": round_or_none(max(input_sol_equivs), 9) if input_sol_equivs else None,
        "smallest_meaningful_live_row": smallest_live_ready_row,
    },
    "top_rows": top_rows,
    "baseline_reconciliation": {
        "strict_current_state_would_fire": safe_int(baseline.get("strict_current_state_would_fire")),
        "strict_current_state_ev_usd": safe_float(baseline.get("strict_current_state_ev_usd")),
        "route_leak_count": safe_int(baseline.get("route_leak_count")),
        "jito_bundle_executable_count": safe_int(baseline.get("jito_bundle_executable_count")),
        "directarb_strict_would_fire": safe_int(baseline.get("directarb_strict_would_fire")),
        "directarb_inventory_ready": safe_int(baseline.get("directarb_inventory_ready")),
        "directarb_route_usable": safe_int(baseline.get("directarb_route_usable")),
        "directarb_top_blocker": text_value(baseline.get("directarb_top_blocker")) or "unknown",
    },
    "operator_notes": [
        "Strict DirectArb rows are drawn from Run69 strict current-state output, so all rows here already passed current-state readiness gates.",
        "quote_ready=true shows the route/quote gate passed, but numeric quote/min_out amounts were not preserved into the retained Run69 strict artifact when expected_output_raw_units and min_output_raw_units stay zero.",
        "heuristic_ev_only=true indicates this surface is still a no-send forensic surface, not landed profit proof.",
    ],
    "output_files": {
        "top_rows_jsonl": str(top_rows_jsonl_path),
    },
}

top_rows_jsonl_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in top_rows) + "\n",
    encoding="utf-8",
)
summary_json_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary_payload['status']}",
    f"strict_directarb_rows={summary_payload['headline']['strict_directarb_rows']}",
    f"strict_directarb_unique_rows={summary_payload['headline']['strict_directarb_unique_rows']}",
    f"strict_directarb_duplicate_rows={summary_payload['headline']['strict_directarb_duplicate_rows']}",
    f"strict_directarb_deduped_net_ev_usd={summary_payload['headline']['strict_directarb_deduped_net_ev_usd']}",
    f"strict_directarb_deduped_retained_ev_usd={summary_payload['headline']['strict_directarb_deduped_retained_ev_usd']}",
    f"strict_directarb_net_ev_median_usd={summary_payload['headline']['strict_directarb_net_ev_median_usd']}",
    f"strict_directarb_net_ev_p90_usd={summary_payload['headline']['strict_directarb_net_ev_p90_usd']}",
    f"quote_ready_count={summary_payload['live_readiness']['quote_ready_count']}",
    f"inventory_ready_count={summary_payload['live_readiness']['inventory_ready_count']}",
    f"approved_held_input_count={summary_payload['live_readiness']['approved_held_input_count']}",
    f"output_account_ready_count={summary_payload['live_readiness']['output_account_ready_count']}",
    f"template_ready_count={summary_payload['live_readiness']['template_ready_count']}",
    f"tpu_or_public_usable_count={summary_payload['live_readiness']['tpu_or_public_usable_count']}",
    f"would_enqueue_tpu_count={summary_payload['live_readiness']['would_enqueue_tpu_count']}",
    f"heuristic_ev_only_count={summary_payload['live_readiness']['heuristic_ev_only_count']}",
    f"quote_numeric_preserved_count={summary_payload['live_readiness']['quote_numeric_preserved_count']}",
    f"min_out_numeric_preserved_count={summary_payload['live_readiness']['min_out_numeric_preserved_count']}",
    f"numeric_quote_gap_count={summary_payload['live_readiness']['numeric_quote_gap_count']}",
    f"all_routes_tpu_quic_public={str(summary_payload['live_readiness']['all_routes_tpu_quic_public']).lower()}",
    f"all_rows_live_ready_now={str(summary_payload['live_readiness']['all_rows_live_ready_now']).lower()}",
    f"min_input_native_size={summary_payload['size_profile']['min_input_native_size']}",
    f"median_input_native_size={summary_payload['size_profile']['median_input_native_size']}",
    f"max_input_native_size={summary_payload['size_profile']['max_input_native_size']}",
    f"min_input_usd_equiv={summary_payload['size_profile']['min_input_usd_equiv']}",
    f"median_input_usd_equiv={summary_payload['size_profile']['median_input_usd_equiv']}",
    f"max_input_usd_equiv={summary_payload['size_profile']['max_input_usd_equiv']}",
    f"route_leak_count={summary_payload['baseline_reconciliation']['route_leak_count']}",
    f"jito_bundle_executable_count={summary_payload['baseline_reconciliation']['jito_bundle_executable_count']}",
    f"top_rows_jsonl={top_rows_jsonl_path}",
]

for index, item in enumerate(route_distribution[:5], start=1):
    summary_lines.append(f"top_route_{index}={item['key']}|count={item['count']}|net_ev_usd={item['net_ev_usd']}")
for index, item in enumerate(pair_distribution[:10], start=1):
    summary_lines.append(f"top_pair_{index}={item['key']}|count={item['count']}|net_ev_usd={item['net_ev_usd']}")
for index, item in enumerate(pool_distribution[:10], start=1):
    summary_lines.append(f"top_pool_{index}={item['key']}|count={item['count']}|net_ev_usd={item['net_ev_usd']}")
if smallest_live_ready_row:
    summary_lines.extend(
        [
            f"smallest_live_row_pool={smallest_live_ready_row.get('pool')}",
            f"smallest_live_row_mint_in={smallest_live_ready_row.get('mint_in')}",
            f"smallest_live_row_mint_out={smallest_live_ready_row.get('mint_out')}",
            f"smallest_live_row_input_native_size={smallest_live_ready_row.get('input_native_size')}",
            f"smallest_live_row_input_usd_equiv={smallest_live_ready_row.get('input_usd_equiv')}",
            f"smallest_live_row_route_used={smallest_live_ready_row.get('route_used')}",
            f"smallest_live_row_net_ev_usd={smallest_live_ready_row.get('net_ev_usd')}",
            f"smallest_live_row_retained_ev_usd={smallest_live_ready_row.get('retained_ev_usd')}",
        ]
    )

summary_file_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run74 strict DirectArb live-readiness forensic written to $RESULT_DIR"
finish

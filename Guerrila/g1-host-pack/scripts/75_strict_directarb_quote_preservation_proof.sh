#!/usr/bin/env bash
# 75_strict_directarb_quote_preservation_proof.sh - rehydrate numeric quote/min-out evidence for strict DirectArb rows

source "$(dirname "$0")/lib.sh"
script_header "75 - Strict DirectArb Quote Preservation Proof"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN75_STEP:-75_strict_directarb_quote_preservation_proof}"
STATUS_PREFIX="${SHADOW_RUN75_STATUS_PREFIX:-run75_strict_directarb_quote_preservation_proof}"
REPORT_BASENAME="${SHADOW_RUN75_REPORT_BASENAME:-run75_strict_directarb_quote_preservation_proof}"
RUN69_STEP="${SHADOW_RUN75_RUN69_STEP:-epoch7b_run73b_metadata_output_ata_retry_post_run69_strict_surface}"
RUN39_STEP="${SHADOW_RUN75_RUN39_STEP:-epoch7b_run73b_metadata_output_ata_retry_canonical_monetization_run55_run39}"
TOP_LIMIT="${SHADOW_RUN75_TOP_LIMIT:-20}"

RUN69_RESULT_DIR="$RESULTS_ROOT/$RUN69_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

RUN69_SUMMARY_JSON="$RUN69_RESULT_DIR/${RUN69_STEP}.json"
DIRECTARB_ROWS_JSONL="$RUN69_RESULT_DIR/directarb_strict_rows.jsonl"
SHOCK_RESEARCH_JSONL="$RUN39_RESULT_DIR/shock_research.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/strict_directarb_quote_rows.jsonl"
TOP_ROWS_JSONL="$RESULT_DIR/top_quote_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$TOP_ROWS_JSONL"

missing=0
for required_path in \
    "$RUN69_SUMMARY_JSON" \
    "$DIRECTARB_ROWS_JSONL" \
    "$SHOCK_RESEARCH_JSONL"
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
    "$SHOCK_RESEARCH_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
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

ALPH = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

run69_summary_path = Path(sys.argv[1])
strict_rows_path = Path(sys.argv[2])
shock_path = Path(sys.argv[3])
summary_file_path = Path(sys.argv[4])
summary_json_path = Path(sys.argv[5])
rows_jsonl_path = Path(sys.argv[6])
top_rows_jsonl_path = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
top_limit = max(1, int(sys.argv[10] or "20"))


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


def text_value(value):
    return str(value or "").strip()


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


def b58encode(raw: bytes) -> str:
    zeros = 0
    for value in raw:
        if value == 0:
            zeros += 1
        else:
            break
    number = int.from_bytes(raw, "big")
    encoded = ""
    while number:
        number, remainder = divmod(number, 58)
        encoded = ALPH[remainder] + encoded
    return ("1" * zeros) + (encoded or "1")


def hex_to_b58(value):
    text = text_value(value)
    if not text:
        return ""
    try:
        return b58encode(bytes.fromhex(text))
    except Exception:
        return ""


def normalize_raw_amount(raw_value, decimals):
    if decimals is None:
        return None
    return safe_int(raw_value) / (10 ** decimals)


def shock_candidate_id(row):
    return ":".join(
        [
            str(safe_int(row.get("unix_ns"))),
            str(safe_int(row.get("slot"))),
            hex_to_b58(row.get("signature_hex")) or text_value(row.get("signature")),
            hex_to_b58(row.get("pool_pubkey_hex")) or text_value(row.get("pool")),
            hex_to_b58(row.get("mint_in_hex")) or text_value(row.get("mint_in")),
            hex_to_b58(row.get("mint_out_hex")) or text_value(row.get("mint_out")),
        ]
    )


def ingress_identity_for_values(unix_ns, slot, pool, mint_in, mint_out):
    return ":".join(
        [
            str(safe_int(unix_ns)),
            str(safe_int(slot)),
            text_value(pool),
            text_value(mint_in),
            text_value(mint_out),
        ]
    )


def strict_ingress_identity(row):
    return ingress_identity_for_values(
        row.get("unix_ns"),
        row.get("slot"),
        row.get("pool"),
        row.get("mint_in"),
        row.get("mint_out"),
    )


def shock_ingress_identity(row):
    return ingress_identity_for_values(
        row.get("ingress_unix_ns") or row.get("unix_ns"),
        row.get("slot"),
        hex_to_b58(row.get("pool_pubkey_hex")) or text_value(row.get("pool")),
        hex_to_b58(row.get("mint_in_hex")) or text_value(row.get("mint_in")),
        hex_to_b58(row.get("mint_out_hex")) or text_value(row.get("mint_out")),
    )


def slot_pool_mint_identity_for_values(slot, pool, mint_in, mint_out):
    return ":".join(
        [
            str(safe_int(slot)),
            text_value(pool),
            text_value(mint_in),
            text_value(mint_out),
        ]
    )


def strict_slot_pool_mint_identity(row):
    return slot_pool_mint_identity_for_values(
        row.get("slot"),
        row.get("pool"),
        row.get("mint_in"),
        row.get("mint_out"),
    )


def shock_slot_pool_mint_identity(row):
    return slot_pool_mint_identity_for_values(
        row.get("slot"),
        hex_to_b58(row.get("pool_pubkey_hex")) or text_value(row.get("pool")),
        hex_to_b58(row.get("mint_in_hex")) or text_value(row.get("mint_in")),
        hex_to_b58(row.get("mint_out_hex")) or text_value(row.get("mint_out")),
    )


def quote_row_rank(row):
    if not row:
        return (-1, -1, -1, -1)
    has_numeric = int(
        all(
            safe_int(row.get(key)) > 0
            for key in ("quote_expected_out_lamports", "quote_target_out_lamports", "min_out_lamports")
        )
    )
    is_final = int(bool(row.get("would_send")) or bool(row.get("live_send_suppressed")))
    has_request = int(safe_int(row.get("quote_request_id")) > 0)
    quote_time = safe_int(row.get("unix_ns")) or safe_int(row.get("slot"))
    return (has_numeric, is_final, has_request, quote_time)


def best_raw_row(*rows):
    best = {}
    best_rank = (-1, -1, -1, -1)
    for row in rows:
        rank = quote_row_rank(row)
        if rank > best_rank:
            best = row or {}
            best_rank = rank
    return best


run69_summary = load_json(run69_summary_path)
strict_rows = load_jsonl(strict_rows_path)

shock_by_candidate = {}
shock_by_ingress_identity = {}
shock_numeric_by_slot_pool_mint = defaultdict(list)
for line in shock_path.read_text(encoding="utf-8", errors="replace").splitlines():
    line = line.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except Exception:
        continue
    if row.get("record_type") != "rollout_shadow":
        continue
    candidate_id = shock_candidate_id(row)
    if not candidate_id:
        continue
    shock_by_candidate[candidate_id] = best_raw_row(shock_by_candidate.get(candidate_id, {}), row)
    ingress_identity = shock_ingress_identity(row)
    if ingress_identity:
        shock_by_ingress_identity[ingress_identity] = best_raw_row(
            shock_by_ingress_identity.get(ingress_identity, {}),
            row,
        )
    if quote_row_rank(row)[0]:
        shock_numeric_by_slot_pool_mint[shock_slot_pool_mint_identity(row)].append(row)

quote_rows = []
for row in strict_rows:
    slot_pool_matches = shock_numeric_by_slot_pool_mint.get(strict_slot_pool_mint_identity(row), [])
    raw = best_raw_row(
        shock_by_ingress_identity.get(strict_ingress_identity(row), {}),
        shock_by_candidate.get(text_value(row.get("candidate_id")), {}),
    )
    match_mode = "none"
    match_count = 0
    if raw:
        match_mode = "ingress_identity_or_candidate_id"
        match_count = 1
    if quote_row_rank(raw)[0] == 0:
        if len(slot_pool_matches) == 1:
            raw = slot_pool_matches[0]
            match_mode = "slot_pool_mints_unique"
            match_count = 1
        elif len(slot_pool_matches) > 1:
            match_mode = "slot_pool_mints_ambiguous"
            match_count = len(slot_pool_matches)
    output_decimals = row.get("output_decimals")
    expected_out_raw = safe_int(row.get("expected_output_raw_units") or raw.get("quote_expected_out_lamports"))
    quote_target_out_raw = safe_int(row.get("quote_target_out_raw_units") or raw.get("quote_target_out_lamports"))
    min_out_raw = safe_int(row.get("min_output_raw_units") or raw.get("min_out_lamports"))
    input_amount_raw = safe_int(row.get("input_amount_raw") or raw.get("trade_size_lamports"))
    net_after_fee_raw = safe_int(
        row.get("net_after_fee_raw_units")
        or raw.get("net_after_fee_lamports")
        or raw.get("t0_retained_ev_lamports_lcb")
    )
    retained_ev_raw = safe_int(row.get("retained_ev_raw_units") or raw.get("retained_ev_lamports_lcb"))
    policy_tip_raw = safe_int(row.get("policy_tip_raw_units") or raw.get("policy_tip_lamports") or raw.get("tip_lamports"))
    base_fee_raw = safe_int(row.get("base_fee_raw_units") or raw.get("base_fee_lamports") or raw.get("estimated_public_fee_lamports") or row.get("base_fee_lamports"))
    priority_fee_raw = safe_int(row.get("estimated_priority_fee_lamports") or raw.get("estimated_priority_fee_lamports"))
    quote_row = {
        "candidate_id": row.get("candidate_id"),
        "signature": row.get("signature"),
        "slot": row.get("slot"),
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "route_used": row.get("route_used"),
        "source": row.get("source"),
        "input_amount_raw": input_amount_raw,
        "input_native_size": row.get("input_native_size"),
        "input_usd_equiv": row.get("input_usd_equiv"),
        "expected_out_raw": expected_out_raw,
        "expected_out_native": round_or_none(
            normalize_raw_amount(expected_out_raw, output_decimals),
            9,
        ),
        "quote_target_out_raw": quote_target_out_raw,
        "quote_target_out_native": round_or_none(
            normalize_raw_amount(quote_target_out_raw, output_decimals),
            9,
        ),
        "min_out_raw": min_out_raw,
        "min_out_native": round_or_none(
            normalize_raw_amount(min_out_raw, output_decimals),
            9,
        ),
        "slippage_bps": row.get("slippage_bps") if row.get("slippage_bps") is not None else raw.get("quote_slippage_bps"),
        "policy_tip_raw": policy_tip_raw,
        "base_fee_raw": base_fee_raw,
        "estimated_priority_fee_raw": priority_fee_raw,
        "net_after_fee_raw": net_after_fee_raw,
        "net_after_fee_usd": row.get("net_ev_usd"),
        "retained_ev_raw": retained_ev_raw,
        "retained_ev_usd": row.get("retained_ev_usd"),
        "quote_source": text_value(row.get("quote_source") or raw.get("quote_source") or raw.get("signal_source")) or None,
        "quote_timestamp_or_slot": row.get("quote_timestamp_or_slot") or raw.get("quote_timestamp_or_slot") or raw.get("unix_ns") or raw.get("slot") or row.get("slot"),
        "quote_request_id": row.get("quote_request_id") or raw.get("quote_request_id"),
        "template_id": row.get("template_id") or raw.get("template_id"),
        "would_enqueue_tpu": bool(row.get("would_enqueue_tpu")),
        "output_account_ready": bool(row.get("output_account_ready")),
        "inventory_ready": bool(row.get("inventory_used")),
        "template_ready": bool(row.get("template_ready")),
        "quote_ready": bool(row.get("quote_ready")),
        "approved_held_input": bool(row.get("approved_held_input")),
        "duplicate_of_prior_event": bool(row.get("duplicate_of_prior_event")),
        "rehydrated_from_shock_research": bool(raw),
        "heuristic_ev_only": bool(row.get("heuristic_ev_only")),
        "quote_match_mode": match_mode,
        "quote_match_count": match_count,
    }
    quote_row["quote_numeric_preserved"] = (
        quote_row["expected_out_raw"] > 0
        and quote_row["quote_target_out_raw"] > 0
        and quote_row["min_out_raw"] > 0
    )
    quote_rows.append(quote_row)

quote_rows.sort(key=lambda item: (safe_int(item.get("slot")), text_value(item.get("signature"))))

top_rows = sorted(
    quote_rows,
    key=lambda item: (
        safe_float(item.get("net_after_fee_usd")),
        safe_int(item.get("expected_out_raw")),
        safe_int(item.get("min_out_raw")),
        safe_int(item.get("slot")),
    ),
    reverse=True,
)[:top_limit]

routes = defaultdict(int)
venues = defaultdict(int)
pair_totals = defaultdict(lambda: {"count": 0, "net_after_fee_usd": 0.0})
for row in quote_rows:
    routes[text_value(row.get("route_used")) or "unknown"] += 1
    venues[text_value(row.get("venue")) or "unknown"] += 1
    key = f"{row.get('mint_in')}->{row.get('mint_out')}"
    pair_totals[key]["count"] += 1
    pair_totals[key]["net_after_fee_usd"] += safe_float(row.get("net_after_fee_usd"))

pair_distribution = sorted(
    [
        {"key": key, "count": value["count"], "net_after_fee_usd": round(value["net_after_fee_usd"], 9)}
        for key, value in pair_totals.items()
    ],
    key=lambda item: (item["count"], item["net_after_fee_usd"], item["key"]),
    reverse=True,
)

net_evs = [safe_float(row.get("net_after_fee_usd")) for row in quote_rows if row.get("net_after_fee_usd") is not None]

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run69_summary_json": str(run69_summary_path),
        "directarb_strict_rows_jsonl": str(strict_rows_path),
        "shock_research_jsonl": str(shock_path),
    },
    "headline": {
        "strict_directarb_rows": len(quote_rows),
        "strict_directarb_unique_rows": len([row for row in quote_rows if not row.get("duplicate_of_prior_event")]),
        "rehydrated_from_shock_count": sum(1 for row in quote_rows if row.get("rehydrated_from_shock_research")),
        "quote_numeric_preserved_count": sum(1 for row in quote_rows if row.get("quote_numeric_preserved")),
        "quote_numeric_unique_slot_pool_mint_count": sum(
            1 for row in quote_rows if row.get("quote_match_mode") == "slot_pool_mints_unique"
        ),
        "quote_numeric_ambiguous_slot_pool_mint_count": sum(
            1 for row in quote_rows if row.get("quote_match_mode") == "slot_pool_mints_ambiguous"
        ),
        "min_out_numeric_preserved_count": sum(1 for row in quote_rows if safe_int(row.get("min_out_raw")) > 0),
        "expected_out_numeric_preserved_count": sum(1 for row in quote_rows if safe_int(row.get("expected_out_raw")) > 0),
        "quote_target_out_numeric_preserved_count": sum(1 for row in quote_rows if safe_int(row.get("quote_target_out_raw")) > 0),
        "slippage_bps_preserved_count": sum(1 for row in quote_rows if row.get("slippage_bps") is not None),
        "policy_tip_raw_preserved_count": sum(1 for row in quote_rows if safe_int(row.get("policy_tip_raw")) > 0),
        "base_fee_raw_preserved_count": sum(1 for row in quote_rows if safe_int(row.get("base_fee_raw")) > 0),
        "quote_request_id_preserved_count": sum(1 for row in quote_rows if row.get("quote_request_id") not in {None, "", 0}),
        "template_id_preserved_count": sum(1 for row in quote_rows if row.get("template_id") not in {None, "", 0}),
        "all_selected_candidates_min_out_positive": all(safe_int(row.get("min_out_raw")) > 0 for row in quote_rows),
        "all_selected_candidates_tpu_quic_public": all(text_value(row.get("route_used")) == "tpu_quic_public" for row in quote_rows),
        "all_selected_candidates_afxdp": all(text_value(row.get("source")) == "afxdp" for row in quote_rows),
        "all_selected_candidates_ready": all(
            row.get("quote_ready")
            and row.get("inventory_ready")
            and row.get("output_account_ready")
            and row.get("template_ready")
            and row.get("would_enqueue_tpu")
            for row in quote_rows
        ),
        "strict_directarb_net_ev_median_usd": round_or_none(statistics.median(net_evs), 9) if net_evs else None,
        "strict_directarb_net_ev_p90_usd": round_or_none(percentile(net_evs, 0.90), 9),
    },
    "distributions": {
        "routes": dict(sorted(routes.items())),
        "venues": dict(sorted(venues.items())),
        "mint_pairs": pair_distribution[:20],
    },
    "top_rows": top_rows,
    "output_files": {
        "quote_rows_jsonl": str(rows_jsonl_path),
        "top_rows_jsonl": str(top_rows_jsonl_path),
    },
    "operator_notes": [
        "quote_request_id and template_id remain null when the upstream rollout_shadow stream does not expose them.",
        "For existing epoch7b artifacts, quote numerics are rehydrated from shock_research.jsonl rather than from candidate_autopsy.jsonl, because the older Run39 autopsy omitted those fields.",
        "Historical slot/pool/mint fallback is used only when it yields a unique quote-bearing rollout row; ambiguous historical matches are left non-numeric.",
    ],
}

rows_jsonl_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in quote_rows) + "\n",
    encoding="utf-8",
)
top_rows_jsonl_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in top_rows) + "\n",
    encoding="utf-8",
)
summary_json_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary_payload['status']}",
    f"strict_directarb_rows={summary_payload['headline']['strict_directarb_rows']}",
    f"strict_directarb_unique_rows={summary_payload['headline']['strict_directarb_unique_rows']}",
    f"rehydrated_from_shock_count={summary_payload['headline']['rehydrated_from_shock_count']}",
    f"quote_numeric_preserved_count={summary_payload['headline']['quote_numeric_preserved_count']}",
    f"quote_numeric_unique_slot_pool_mint_count={summary_payload['headline']['quote_numeric_unique_slot_pool_mint_count']}",
    f"quote_numeric_ambiguous_slot_pool_mint_count={summary_payload['headline']['quote_numeric_ambiguous_slot_pool_mint_count']}",
    f"expected_out_numeric_preserved_count={summary_payload['headline']['expected_out_numeric_preserved_count']}",
    f"quote_target_out_numeric_preserved_count={summary_payload['headline']['quote_target_out_numeric_preserved_count']}",
    f"min_out_numeric_preserved_count={summary_payload['headline']['min_out_numeric_preserved_count']}",
    f"slippage_bps_preserved_count={summary_payload['headline']['slippage_bps_preserved_count']}",
    f"policy_tip_raw_preserved_count={summary_payload['headline']['policy_tip_raw_preserved_count']}",
    f"base_fee_raw_preserved_count={summary_payload['headline']['base_fee_raw_preserved_count']}",
    f"quote_request_id_preserved_count={summary_payload['headline']['quote_request_id_preserved_count']}",
    f"template_id_preserved_count={summary_payload['headline']['template_id_preserved_count']}",
    f"all_selected_candidates_min_out_positive={str(summary_payload['headline']['all_selected_candidates_min_out_positive']).lower()}",
    f"all_selected_candidates_tpu_quic_public={str(summary_payload['headline']['all_selected_candidates_tpu_quic_public']).lower()}",
    f"all_selected_candidates_afxdp={str(summary_payload['headline']['all_selected_candidates_afxdp']).lower()}",
    f"all_selected_candidates_ready={str(summary_payload['headline']['all_selected_candidates_ready']).lower()}",
    f"strict_directarb_net_ev_median_usd={summary_payload['headline']['strict_directarb_net_ev_median_usd']}",
    f"strict_directarb_net_ev_p90_usd={summary_payload['headline']['strict_directarb_net_ev_p90_usd']}",
    f"quote_rows_jsonl={rows_jsonl_path}",
    f"top_rows_jsonl={top_rows_jsonl_path}",
]

for index, item in enumerate(pair_distribution[:10], start=1):
    summary_lines.append(f"top_pair_{index}={item['key']}|count={item['count']}|net_after_fee_usd={item['net_after_fee_usd']}")

summary_file_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run75 strict DirectArb quote preservation proof written to $RESULT_DIR"
finish

#!/usr/bin/env bash
# 62_conversion_autopsy.sh - conversion autopsy over completed Run61 artifacts

source "$(dirname "$0")/lib.sh"
script_header "62 - Conversion Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"

STEP="${SHADOW_RUN62_STEP:-62_conversion_autopsy}"
STATUS_PREFIX="${SHADOW_RUN62_STATUS_PREFIX:-run62_conversion_autopsy}"
REPORT_BASENAME="${SHADOW_RUN62_REPORT_BASENAME:-run62_conversion_autopsy}"

RUN61_STEP="${SHADOW_RUN62_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN61_SUMMARY="$RUN61_RESULT_DIR/summary.txt"

RUN39_STEP="${SHADOW_RUN62_RUN39_STEP:-${RUN61_STEP}_run55_run39}"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"
RUN39_SUMMARY="$RUN39_RESULT_DIR/summary.txt"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

missing=0
for required_path in \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY"
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
    "$RUN61_REPORT_JSON" \
    "$RUN61_SUMMARY" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" <<'PY'
import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"

USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
PRIVATE_ONLY_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}
POST_SHOCK_OWNER_MODES = {"continuation", "housekeeping", "rebalancing", "flatten", "drop"}
PUBLIC_RPC_SUPPORTED_VENUES = {"raydium_clmm"}
FOCUS_OWNER_MODES = ["directarb", "continuation", "rebalancing", "housekeeping", "backrun"]

run61_report_path = Path(sys.argv[1])
run61_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
run39_summary_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]


def load_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def load_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key] = value
    return data


def text_value(value):
    return (value or "").strip()


def split_csv(value):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


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


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def route_reason(row):
    for key in ("route_reason", "route_family_label", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def owner_mode(row):
    return text_value(row.get("selected_owner")).lower() or "drop"


def venue_name(row):
    return text_value(row.get("venue")).lower() or "unknown_venue"


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def is_afxdp_row(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def approved_held_input(row):
    return bool_field(row, "approved_held_input", "is_approved_held_input")


def input_balance_ready(row):
    return bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")


def input_account_ready(row):
    return bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")


def output_account_ready(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def template_ready(row):
    return bool_field(row, "template_ready", "would_execute_current_template")


def quote_ready(row):
    return bool_field(row, "quote_ready_or_classified", "would_admit")


def route_usable_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def route_private_only(row):
    return route_family(row) in PRIVATE_ONLY_ROUTE_FAMILIES


def positive_research(row):
    return bool(row.get("research_positive")) or safe_int(row.get("net_after_fee_lamports")) > 0


def normalized_input_units(row, field_name):
    mint_in = text_value(row.get("mint_in"))
    raw_value = safe_int(row.get(field_name))
    if mint_in == WSOL_MINT:
        return raw_value / 1_000_000_000.0, "sol"
    if mint_in in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0, "usd_stable"
    return None, "token_native"


def sol_equivalent(row, field_name, sol_usd_equivalent):
    native_value, unit_kind = normalized_input_units(row, field_name)
    if native_value is None:
        return None
    if unit_kind == "sol":
        return native_value
    if unit_kind == "usd_stable":
        return native_value / sol_usd_equivalent if sol_usd_equivalent else None
    return None


def best_horizon_name(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return text_value(markouts.get("best_horizon")) or "none"


def best_horizon_net_after_fee_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports"))


def best_horizon_markout_lamports(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_markout_lamports"))


def shadow_ready_nonroute(row):
    execution_blocker = text_value(row.get("execution_blocker")).lower()
    return (
        is_afxdp_row(row)
        and positive_research(row)
        and approved_held_input(row)
        and input_account_ready(row)
        and output_account_ready(row)
        and template_ready(row)
        and quote_ready(row)
        and execution_blocker in {"", "none"}
    )


def would_fire(row):
    return owner_mode(row) in ACTIVE_FIRE_OWNERS and shadow_ready_nonroute(row) and route_usable_for_us(row) is not None


def route_only_blocked(row):
    return shadow_ready_nonroute(row) and route_private_only(row) and route_usable_for_us(row) is None


def blocker_reason(row):
    route = route_family(row)
    if route_usable_for_us(row) is None:
        if route in PRIVATE_ONLY_ROUTE_FAMILIES:
            return "route_private_only"
        if route in {"unknown", "", "drop"}:
            return "route_not_routable"
        return f"route_{route}"
    if not is_afxdp_row(row):
        return "source_not_afxdp"
    if not approved_held_input(row):
        return "not_held_input"
    if not input_account_ready(row):
        return "input_account_not_ready"
    if not input_balance_ready(row):
        return "inventory_insufficient"
    if not output_account_ready(row):
        return "account_ata_missing"
    if not template_ready(row):
        return "template_missing"
    if not quote_ready(row):
        return "quote_missing"
    if not positive_research(row):
        return "not_positive_research"
    if owner_mode(row) not in ACTIVE_FIRE_OWNERS:
        return f"owner_{owner_mode(row)}_non_fire"
    return "none"


def bucket_label(row, sol_usd_equivalent):
    native_value, unit_kind = normalized_input_units(row, "trade_size_lamports")
    if native_value is None:
        return "unbucketed_other_input_units"
    if unit_kind == "sol":
        caps = [
            (0.005, "<=_5_usdc_or_0.005_sol_equiv"),
            (0.01, "<=_10_usdc_or_0.01_sol_equiv"),
            (0.025, "<=_25_usdc_or_0.025_sol_equiv"),
            (0.05, "<=_50_usdc_or_0.05_sol_equiv"),
            (0.1, "<=_100_usdc_or_0.1_sol_equiv"),
            (0.25, "<=_250_usdc_or_0.25_sol_equiv"),
            (0.5, "<=_500_usdc_or_0.5_sol_equiv"),
            (1.0, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2.0, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if native_value <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    if unit_kind == "usd_stable":
        caps = [
            (5, "<=_5_usdc_or_0.005_sol_equiv"),
            (10, "<=_10_usdc_or_0.01_sol_equiv"),
            (25, "<=_25_usdc_or_0.025_sol_equiv"),
            (50, "<=_50_usdc_or_0.05_sol_equiv"),
            (100, "<=_100_usdc_or_0.1_sol_equiv"),
            (250, "<=_250_usdc_or_0.25_sol_equiv"),
            (500, "<=_500_usdc_or_0.5_sol_equiv"),
            (1000, "<=_1000_usdc_or_1.0_sol_equiv"),
            (2000, "<=_2000_usdc_or_2.0_sol_equiv"),
        ]
        for cap, label in caps:
            if native_value <= cap:
                return label
        return ">_2000_usdc_or_2.0_sol_equiv"
    return "unbucketed_other_input_units"


def unique_event_key(row):
    return (
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
        safe_int(row.get("slot")),
    )


def compact_row(row, sol_usd_equivalent):
    return {
        "candidate_id": row.get("candidate_id"),
        "signature": row.get("signature"),
        "slot": safe_int(row.get("slot")),
        "owner_mode": owner_mode(row),
        "venue": venue_name(row),
        "pool": row.get("pool"),
        "source": text_value(row.get("source")).lower(),
        "route": route_family(row),
        "route_reason": route_reason(row),
        "trade_size_input_native": round_or_none(normalized_input_units(row, "trade_size_lamports")[0], 9),
        "trade_size_bucket": bucket_label(row, sol_usd_equivalent),
        "net_after_fee_sol_equivalent": round_or_none(sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent), 9),
        "best_horizon_name": best_horizon_name(row),
        "best_horizon_net_after_fee_sol_equivalent": round_or_none(
            sol_equivalent({"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)}, "net_after_fee_lamports", sol_usd_equivalent),
            9,
        ),
        "first_rejection_stage": text_value(row.get("first_rejection_stage")).lower(),
        "execution_blocker": text_value(row.get("execution_blocker")).lower(),
        "route_only_blocked": route_only_blocked(row),
        "would_fire": would_fire(row),
    }


def total_immediate_ev_sol_equivalent(scoped_rows, sol_usd_equivalent):
    return sum(
        value
        for value in (
            sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent)
            for row in scoped_rows
        )
        if value is not None
    )


def total_best_horizon_ev_sol_equivalent(scoped_rows, sol_usd_equivalent):
    return sum(
        value
        for value in (
            sol_equivalent(
                {"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)},
                "net_after_fee_lamports",
                sol_usd_equivalent,
            )
            for row in scoped_rows
        )
        if value is not None
    )


def summarize_grouped_rows(scoped_rows, key_fn, label_fn, sol_usd_equivalent, limit=10):
    grouped = defaultdict(list)
    for row in scoped_rows:
        grouped[key_fn(row)].append(row)

    items = []
    for key, grouped_rows in grouped.items():
        size_buckets = Counter(bucket_label(row, sol_usd_equivalent) for row in grouped_rows)
        venues = Counter(venue_name(row) for row in grouped_rows)
        items.append(
            {
                **label_fn(key),
                "count": len(grouped_rows),
                "route_usable_count": sum(1 for row in grouped_rows if route_usable_for_us(row) is not None),
                "positive_ev_count": sum(1 for row in grouped_rows if safe_int(row.get("net_after_fee_lamports")) > 0),
                "best_horizon_positive_count": sum(1 for row in grouped_rows if best_horizon_net_after_fee_lamports(row) > 0),
                "top_size_bucket": size_buckets.most_common(1)[0][0] if size_buckets else "none",
                "main_venue": venues.most_common(1)[0][0] if venues else "none",
                "total_immediate_ev_sol_equivalent": round_or_none(
                    total_immediate_ev_sol_equivalent(grouped_rows, sol_usd_equivalent),
                    9,
                ),
                "total_best_horizon_ev_sol_equivalent": round_or_none(
                    total_best_horizon_ev_sol_equivalent(grouped_rows, sol_usd_equivalent),
                    9,
                ),
            }
        )

    items.sort(
        key=lambda item: (
            item["count"],
            item["best_horizon_positive_count"],
            safe_float(item["total_best_horizon_ev_sol_equivalent"]),
            safe_float(item["total_immediate_ev_sol_equivalent"]),
        ),
        reverse=True,
    )
    return items[:limit]


run61_report = load_json(run61_report_path)
run61_summary = load_summary(run61_summary_path)
run39_summary = load_summary(run39_summary_path)
rows = load_jsonl(candidate_autopsy_path)

if not rows:
    raise SystemExit("Run62 conversion autopsy requires non-empty candidate_autopsy.jsonl")

sol_usd_equivalent = safe_float((run61_report.get("inputs") or {}).get("sol_usd_equivalent")) or 1000.0
approved_held_input_mints = split_csv(run39_summary.get("approved_held_input_mints"))
held_input_mints = split_csv(run39_summary.get("held_input_mints"))

directarb_rows = [row for row in rows if owner_mode(row) == "directarb"]
continuation_rows = [row for row in rows if owner_mode(row) == "continuation"]
route_unavailable_rows = [row for row in rows if route_usable_for_us(row) is None]
route_only_blocked_rows = [row for row in rows if route_only_blocked(row)]
route_only_blocked_active_fire_rows = [row for row in route_only_blocked_rows if owner_mode(row) in ACTIVE_FIRE_OWNERS]
directarb_inventory_insufficient_rows = [row for row in directarb_rows if blocker_reason(row) == "inventory_insufficient"]
directarb_not_held_input_rows = [row for row in directarb_rows if blocker_reason(row) == "not_held_input"]
rebalancing_ata_missing_rows = [
    row for row in rows if owner_mode(row) == "rebalancing" and blocker_reason(row) == "account_ata_missing"
]
continuation_actionable_rows = [
    row
    for row in continuation_rows
    if best_horizon_net_after_fee_lamports(row) > safe_int(row.get("net_after_fee_lamports"))
    and shadow_ready_nonroute(row)
]
continuation_actionable_route_usable_rows = [row for row in continuation_actionable_rows if route_usable_for_us(row) is not None]
continuation_actionable_route_only_rows = [row for row in continuation_actionable_rows if route_only_blocked(row)]

directarb_blockers = Counter(blocker_reason(row) for row in directarb_rows)
directarb_size_buckets = Counter(bucket_label(row, sol_usd_equivalent) for row in directarb_rows)
directarb_route_reasons = Counter(route_reason(row) for row in directarb_rows)
directarb_routes = Counter(route_family(row) for row in directarb_rows)
directarb_unique_events = {unique_event_key(row) for row in directarb_rows}

route_unavailable_owner_counts = Counter(owner_mode(row) for row in route_unavailable_rows)
route_unavailable_venue_counts = Counter(venue_name(row) for row in route_unavailable_rows)
route_unavailable_route_counts = Counter(route_family(row) for row in route_unavailable_rows)
route_unavailable_route_reason_counts = Counter(route_reason(row) for row in route_unavailable_rows)

continuation_best_horizon_counts = Counter(best_horizon_name(row) for row in continuation_rows)
continuation_route_reason_counts = Counter(route_reason(row) for row in continuation_actionable_rows)

public_rpc_rewritable_rows = [
    row for row in route_only_blocked_active_fire_rows if venue_name(row) in PUBLIC_RPC_SUPPORTED_VENUES
]

triage_rows = []
for scoped_owner in FOCUS_OWNER_MODES:
    scoped_rows = [row for row in rows if owner_mode(row) == scoped_owner]
    blockers = defaultdict(list)
    for row in scoped_rows:
        blockers[blocker_reason(row)].append(row)
    for blocker, blocked_rows in blockers.items():
        total_best_horizon_ev = sum(
            value
            for value in (
                sol_equivalent(
                    {"mint_in": row.get("mint_in"), "net_after_fee_lamports": best_horizon_net_after_fee_lamports(row)},
                    "net_after_fee_lamports",
                    sol_usd_equivalent,
                )
                for row in blocked_rows
            )
            if value is not None
        )
        triage_rows.append(
            {
                "owner_mode": scoped_owner,
                "blocker": blocker,
                "count": len(blocked_rows),
                "best_horizon_positive_count": sum(1 for row in blocked_rows if best_horizon_net_after_fee_lamports(row) > 0),
                "route_only_blocked_count": sum(1 for row in blocked_rows if route_only_blocked(row)),
                "route_usable_count": sum(1 for row in blocked_rows if route_usable_for_us(row) is not None),
                "total_best_horizon_ev_sol_equivalent": round_or_none(total_best_horizon_ev, 9),
            }
        )

triage_rows.sort(
    key=lambda item: (
        item["count"],
        item["best_horizon_positive_count"],
        safe_float(item["total_best_horizon_ev_sol_equivalent"]),
    ),
    reverse=True,
)

directarb_autopsy = {
    "total_rows": len(directarb_rows),
    "unique_opportunity_count": len(directarb_unique_events),
    "quote_valid_count": sum(1 for row in directarb_rows if quote_ready(row)),
    "route_usable_count": sum(1 for row in directarb_rows if route_usable_for_us(row) is not None),
    "inventory_ready_count": sum(1 for row in directarb_rows if approved_held_input(row) and input_balance_ready(row)),
    "ata_ready_count": sum(1 for row in directarb_rows if output_account_ready(row)),
    "template_ready_count": sum(1 for row in directarb_rows if template_ready(row)),
    "unit_normalized_positive_ev_count": sum(
        1
        for row in directarb_rows
        if sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent) is not None
        and sol_equivalent(row, "net_after_fee_lamports", sol_usd_equivalent) > 0
    ),
    "positive_ev_count": sum(1 for row in directarb_rows if safe_int(row.get("net_after_fee_lamports")) > 0),
    "would_fire_count": sum(1 for row in directarb_rows if would_fire(row)),
    "blocker_counts": dict(directarb_blockers),
    "route_counts": dict(directarb_routes),
    "route_reason_counts": dict(directarb_route_reasons),
    "size_bucket_counts": dict(directarb_size_buckets),
    "optimal_size_bucket_by_count": directarb_size_buckets.most_common(1)[0][0] if directarb_size_buckets else "none",
    "top_route_only_candidates": [
        compact_row(row, sol_usd_equivalent)
        for row in sorted(
            [row for row in directarb_rows if route_only_blocked(row)],
            key=lambda row: (
                best_horizon_net_after_fee_lamports(row),
                safe_int(row.get("net_after_fee_lamports")),
            ),
            reverse=True,
        )[:15]
    ],
}

inventory_context = {
    "approved_held_input_mints": approved_held_input_mints,
    "held_input_mints": held_input_mints,
    "approved_but_not_held_input_mints": [
        mint for mint in approved_held_input_mints if mint not in set(held_input_mints)
    ],
}

directarb_inventory_conversion = {
    "inventory_insufficient_count": len(directarb_inventory_insufficient_rows),
    "inventory_insufficient_top_input_mints": summarize_grouped_rows(
        directarb_inventory_insufficient_rows,
        key_fn=lambda row: text_value(row.get("mint_in")) or "unknown_mint",
        label_fn=lambda mint_in: {"mint_in": mint_in},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "inventory_insufficient_top_pairs": summarize_grouped_rows(
        directarb_inventory_insufficient_rows,
        key_fn=lambda row: (
            text_value(row.get("mint_in")) or "unknown_mint",
            text_value(row.get("mint_out")) or "unknown_mint",
        ),
        label_fn=lambda pair: {"mint_in": pair[0], "mint_out": pair[1]},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "inventory_insufficient_top_pools": summarize_grouped_rows(
        directarb_inventory_insufficient_rows,
        key_fn=lambda row: text_value(row.get("pool")) or "unknown_pool",
        label_fn=lambda pool: {"pool": pool},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "not_held_input_count": len(directarb_not_held_input_rows),
    "not_held_input_top_input_mints": summarize_grouped_rows(
        directarb_not_held_input_rows,
        key_fn=lambda row: text_value(row.get("mint_in")) or "unknown_mint",
        label_fn=lambda mint_in: {"mint_in": mint_in},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "not_held_input_top_pairs": summarize_grouped_rows(
        directarb_not_held_input_rows,
        key_fn=lambda row: (
            text_value(row.get("mint_in")) or "unknown_mint",
            text_value(row.get("mint_out")) or "unknown_mint",
        ),
        label_fn=lambda pair: {"mint_in": pair[0], "mint_out": pair[1]},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "not_held_input_top_pools": summarize_grouped_rows(
        directarb_not_held_input_rows,
        key_fn=lambda row: text_value(row.get("pool")) or "unknown_pool",
        label_fn=lambda pool: {"pool": pool},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
}

route_unavailable_autopsy = {
    "total_rows": len(route_unavailable_rows),
    "private_route_only_count": sum(1 for row in route_unavailable_rows if route_private_only(row)),
    "route_only_blocked_count": len(route_only_blocked_rows),
    "route_only_blocked_active_fire_count": len(route_only_blocked_active_fire_rows),
    "owner_mode_counts": dict(route_unavailable_owner_counts),
    "venue_counts": dict(route_unavailable_venue_counts),
    "route_label_counts": dict(route_unavailable_route_counts),
    "route_reason_counts": dict(route_unavailable_route_reason_counts),
    "main_owner_mode": route_unavailable_owner_counts.most_common(1)[0][0] if route_unavailable_owner_counts else "none",
    "main_venue": route_unavailable_venue_counts.most_common(1)[0][0] if route_unavailable_venue_counts else "none",
    "main_route_label": route_unavailable_route_counts.most_common(1)[0][0] if route_unavailable_route_counts else "none",
    "main_route_reason": route_unavailable_route_reason_counts.most_common(1)[0][0] if route_unavailable_route_reason_counts else "none",
    "top_route_only_candidates": [
        compact_row(row, sol_usd_equivalent)
        for row in sorted(
            route_only_blocked_active_fire_rows,
            key=lambda row: (
                best_horizon_net_after_fee_lamports(row),
                safe_int(row.get("net_after_fee_lamports")),
            ),
            reverse=True,
        )[:20]
    ],
}

continuation_monetization = {
    "total_rows": len(continuation_rows),
    "best_horizon_positive_count": sum(1 for row in continuation_rows if best_horizon_net_after_fee_lamports(row) > 0),
    "best_horizon_beats_immediate_count": sum(
        1 for row in continuation_rows if best_horizon_net_after_fee_lamports(row) > safe_int(row.get("net_after_fee_lamports"))
    ),
    "route_usable_count": sum(1 for row in continuation_rows if route_usable_for_us(row) is not None),
    "deployable_count": sum(1 for row in continuation_rows if would_fire(row)),
    "actionable_entry_rule_candidates": len(continuation_actionable_rows),
    "actionable_route_usable_count": len(continuation_actionable_route_usable_rows),
    "actionable_route_only_blocked_count": len(continuation_actionable_route_only_rows),
    "best_horizon_counts": dict(continuation_best_horizon_counts),
    "route_reason_counts": dict(continuation_route_reason_counts),
    "top_actionable_candidates": [
        compact_row(row, sol_usd_equivalent)
        for row in sorted(
            continuation_actionable_rows,
            key=lambda row: (
                best_horizon_net_after_fee_lamports(row) - safe_int(row.get("net_after_fee_lamports")),
                best_horizon_net_after_fee_lamports(row),
            ),
            reverse=True,
        )[:20]
    ],
}

route_conversion = {
    "tpu_quic_public_rewritable_count": len(route_only_blocked_rows),
    "tpu_quic_public_rewritable_active_fire_count": len(route_only_blocked_active_fire_rows),
    "public_rpc_rewritable_count": len(public_rpc_rewritable_rows),
    "tpu_rewritable_by_owner_mode": dict(Counter(owner_mode(row) for row in route_only_blocked_active_fire_rows)),
    "tpu_rewritable_by_venue": dict(Counter(venue_name(row) for row in route_only_blocked_active_fire_rows)),
    "public_rpc_rewritable_by_venue": dict(Counter(venue_name(row) for row in public_rpc_rewritable_rows)),
    "top_tpu_rewritable_candidates": [
        compact_row(row, sol_usd_equivalent)
        for row in sorted(
            route_only_blocked_active_fire_rows,
            key=lambda row: (
                best_horizon_net_after_fee_lamports(row),
                safe_int(row.get("net_after_fee_lamports")),
            ),
            reverse=True,
        )[:20]
    ],
}

rebalancing_ata_conversion = {
    "account_ata_missing_count": len(rebalancing_ata_missing_rows),
    "top_output_mints": summarize_grouped_rows(
        rebalancing_ata_missing_rows,
        key_fn=lambda row: text_value(row.get("mint_out")) or "unknown_mint",
        label_fn=lambda mint_out: {"mint_out": mint_out},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "top_pairs": summarize_grouped_rows(
        rebalancing_ata_missing_rows,
        key_fn=lambda row: (
            text_value(row.get("mint_in")) or "unknown_mint",
            text_value(row.get("mint_out")) or "unknown_mint",
        ),
        label_fn=lambda pair: {"mint_in": pair[0], "mint_out": pair[1]},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
    "top_pools": summarize_grouped_rows(
        rebalancing_ata_missing_rows,
        key_fn=lambda row: text_value(row.get("pool")) or "unknown_pool",
        label_fn=lambda pool: {"pool": pool},
        sol_usd_equivalent=sol_usd_equivalent,
        limit=10,
    ),
}

blocker_triage = {
    "focus_owner_modes": FOCUS_OWNER_MODES,
    "top_items": triage_rows[:20],
}

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run61_summary": str(run61_summary_path),
        "run39_candidate_autopsy": str(candidate_autopsy_path),
        "run39_summary": str(run39_summary_path),
        "sol_usd_equivalent": sol_usd_equivalent,
        "run61_status": run61_summary.get("status") or run61_report.get("status"),
        "run39_status": run39_summary.get("status"),
    },
    "inventory_context": inventory_context,
    "directarb_autopsy": directarb_autopsy,
    "directarb_inventory_conversion": directarb_inventory_conversion,
    "route_unavailable_autopsy": route_unavailable_autopsy,
    "continuation_monetization": continuation_monetization,
    "route_conversion": route_conversion,
    "rebalancing_ata_conversion": rebalancing_ata_conversion,
    "blocker_triage": blocker_triage,
}

summary_lines = [
    f"status={payload['status']}",
    f"run61_status={payload['inputs']['run61_status']}",
    f"run39_status={payload['inputs']['run39_status']}",
    f"directarb_total_rows={directarb_autopsy['total_rows']}",
    f"directarb_unique_opportunity_count={directarb_autopsy['unique_opportunity_count']}",
    f"directarb_quote_valid_count={directarb_autopsy['quote_valid_count']}",
    f"directarb_route_usable_count={directarb_autopsy['route_usable_count']}",
    f"directarb_inventory_ready_count={directarb_autopsy['inventory_ready_count']}",
    f"directarb_ata_ready_count={directarb_autopsy['ata_ready_count']}",
    f"directarb_template_ready_count={directarb_autopsy['template_ready_count']}",
    f"directarb_unit_normalized_positive_ev_count={directarb_autopsy['unit_normalized_positive_ev_count']}",
    f"directarb_positive_ev_count={directarb_autopsy['positive_ev_count']}",
    f"directarb_would_fire_count={directarb_autopsy['would_fire_count']}",
    f"directarb_main_blocker={directarb_blockers.most_common(1)[0][0] if directarb_blockers else 'none'}",
    f"directarb_optimal_size_bucket_by_count={directarb_autopsy['optimal_size_bucket_by_count']}",
    f"inventory_context_approved_held_input_mint_count={len(inventory_context['approved_held_input_mints'])}",
    f"inventory_context_current_held_input_mint_count={len(inventory_context['held_input_mints'])}",
    f"directarb_inventory_insufficient_count={directarb_inventory_conversion['inventory_insufficient_count']}",
    f"directarb_inventory_insufficient_top_input_mint={directarb_inventory_conversion['inventory_insufficient_top_input_mints'][0]['mint_in'] if directarb_inventory_conversion['inventory_insufficient_top_input_mints'] else 'none'}",
    f"directarb_inventory_insufficient_top_pool={directarb_inventory_conversion['inventory_insufficient_top_pools'][0]['pool'] if directarb_inventory_conversion['inventory_insufficient_top_pools'] else 'none'}",
    f"directarb_not_held_input_count={directarb_inventory_conversion['not_held_input_count']}",
    f"directarb_not_held_input_top_input_mint={directarb_inventory_conversion['not_held_input_top_input_mints'][0]['mint_in'] if directarb_inventory_conversion['not_held_input_top_input_mints'] else 'none'}",
    f"route_unavailable_total_rows={route_unavailable_autopsy['total_rows']}",
    f"route_unavailable_private_route_only_count={route_unavailable_autopsy['private_route_only_count']}",
    f"route_only_blocked_count={route_unavailable_autopsy['route_only_blocked_count']}",
    f"route_only_blocked_active_fire_count={route_unavailable_autopsy['route_only_blocked_active_fire_count']}",
    f"route_unavailable_main_owner_mode={route_unavailable_autopsy['main_owner_mode']}",
    f"route_unavailable_main_venue={route_unavailable_autopsy['main_venue']}",
    f"route_unavailable_main_route_label={route_unavailable_autopsy['main_route_label']}",
    f"route_unavailable_main_route_reason={route_unavailable_autopsy['main_route_reason']}",
    f"continuation_total_rows={continuation_monetization['total_rows']}",
    f"continuation_best_horizon_positive_count={continuation_monetization['best_horizon_positive_count']}",
    f"continuation_best_horizon_beats_immediate_count={continuation_monetization['best_horizon_beats_immediate_count']}",
    f"continuation_route_usable_count={continuation_monetization['route_usable_count']}",
    f"continuation_deployable_count={continuation_monetization['deployable_count']}",
    f"continuation_actionable_entry_rule_candidates={continuation_monetization['actionable_entry_rule_candidates']}",
    f"continuation_actionable_route_usable_count={continuation_monetization['actionable_route_usable_count']}",
    f"continuation_actionable_route_only_blocked_count={continuation_monetization['actionable_route_only_blocked_count']}",
    f"tpu_quic_public_rewritable_count={route_conversion['tpu_quic_public_rewritable_count']}",
    f"tpu_quic_public_rewritable_active_fire_count={route_conversion['tpu_quic_public_rewritable_active_fire_count']}",
    f"public_rpc_rewritable_count={route_conversion['public_rpc_rewritable_count']}",
    f"rebalancing_account_ata_missing_count={rebalancing_ata_conversion['account_ata_missing_count']}",
    f"rebalancing_account_ata_top_output_mint={rebalancing_ata_conversion['top_output_mints'][0]['mint_out'] if rebalancing_ata_conversion['top_output_mints'] else 'none'}",
]

for horizon, count in sorted(continuation_monetization["best_horizon_counts"].items()):
    summary_lines.append(f"continuation_best_horizon_{horizon}_count={count}")

for owner, count in sorted(route_conversion["tpu_rewritable_by_owner_mode"].items()):
    summary_lines.append(f"tpu_rewritable_owner_{owner}_count={count}")

for index, item in enumerate(blocker_triage["top_items"][:10], start=1):
    summary_lines.append(
        "triage_{}_owner_mode={} blocker={} count={} best_horizon_positive_count={} total_best_horizon_ev_sol_equivalent={}".format(
            index,
            item["owner_mode"],
            item["blocker"],
            item["count"],
            item["best_horizon_positive_count"],
            item["total_best_horizon_ev_sol_equivalent"],
        )
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run62 conversion autopsy written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

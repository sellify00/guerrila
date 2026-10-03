#!/usr/bin/env bash
# 54_all_venue_current_scorecard_execution_autopsy.sh - rank nearest executable current-scorecard rows across all supported venues

source "$(dirname "$0")/lib.sh"
script_header "54 - All-Venue Execution Autopsy"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
DEFAULT_SCORECARD_SEARCH_ROOT="$RESULTS_ROOT"
if [[ -d "/home/ubuntu/shadow-host-audit/dpdk-runs" ]]; then
    DEFAULT_SCORECARD_SEARCH_ROOT="/home/ubuntu/shadow-host-audit/dpdk-runs"
fi

STEP="${SHADOW_RUN54_STEP:-54_all_venue_current_scorecard_execution_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run54_all_venue_current_scorecard_execution_autopsy.json"
RUN54_SCORECARD_PATH="${SHADOW_RUN54_SCORECARD_PATH:-}"
RUN54_SCORECARD_SEARCH_ROOT="${SHADOW_RUN54_SCORECARD_SEARCH_ROOT:-$DEFAULT_SCORECARD_SEARCH_ROOT}"
SUPPORTED_VENUES="${SHADOW_RUN54_SUPPORTED_VENUES:-raydium_clmm,raydium_cpmm,raydium_v4,meteora_damm_v2,pumpfun_amm}"
PUBLIC_SUPPORTED_VENUES="${SHADOW_RUN54_PUBLIC_SUPPORTED_VENUES:-raydium_clmm}"
RUN54_SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RUN54_MAX_LIVE_TRADE_INPUT_SOL="${SHADOW_RUN54_MAX_LIVE_TRADE_INPUT_SOL:-0.05}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

python3 - \
    "$RUN54_SCORECARD_PATH" \
    "$RUN54_SCORECARD_SEARCH_ROOT" \
    "$RESULT_DIR" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$SUPPORTED_VENUES" \
    "$PUBLIC_SUPPORTED_VENUES" \
    "$RUN54_SIGNER_TOKEN_INVENTORY_PATH" \
    "$RUN54_MAX_LIVE_TRADE_INPUT_SOL" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

explicit_scorecard = sys.argv[1].strip()
search_root = Path(sys.argv[2])
result_dir = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
supported_venues = [item.strip() for item in sys.argv[6].split(",") if item.strip()]
public_supported_venues = {item.strip() for item in sys.argv[7].split(",") if item.strip()}
inventory_path = Path(sys.argv[8]) if sys.argv[8].strip() else None
max_live_trade_input_sol = float(sys.argv[9] or "0.05")
max_tiny_lamports = int(round(max_live_trade_input_sol * 1_000_000_000))

WSOL_MINT = "So11111111111111111111111111111111111111112"
TPU_CONVERTIBLE_ROUTE_FAMILIES = {"jito_bundle", "harmonic_bundle", "tpu_quic_public"}
PUBLIC_ROUTE_FAMILIES = {"public_rpc_scout", "public", "public_rpc", "tpu_quic_public"}


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def text_value(value) -> str:
    return (value or "").strip()


def classify_tiny_cap_blocker(row: dict, max_tiny_lamports: int) -> str:
    proof_filter_result = text_value(row.get("proof_filter_result"))
    if bool(row.get("would_admit")) or proof_filter_result == "no_quote":
        return ""
    trade_size_lamports = safe_int(row.get("trade_size_lamports"))
    if max_tiny_lamports > 0 and trade_size_lamports > max_tiny_lamports:
        return "trade_size_exceeds_max_tiny"
    if proof_filter_result and proof_filter_result != "not_applicable":
        return f"proof_filter:{proof_filter_result}"
    micro_market_result = text_value(row.get("micro_market_result"))
    if micro_market_result and micro_market_result not in {"not_applicable", "not_evaluated"}:
        return f"micro_market:{micro_market_result}"
    execution_blocker = text_value(row.get("execution_blocker"))
    if execution_blocker and execution_blocker != "none":
        return execution_blocker
    r1_outcome = text_value(row.get("r1_outcome"))
    if r1_outcome:
        return f"r1_outcome:{r1_outcome}"
    drop_reason = text_value(row.get("drop_reason"))
    if drop_reason:
        return f"drop_reason:{drop_reason}"
    return "would_admit_false"


def load_inventory(path: Path | None):
    if not path or not path.exists():
        return {}
    raw = load_json(path)
    inventory = {}
    for entry in raw.get("accounts", []):
        mint = entry.get("mint_pubkey")
        if mint:
            inventory[mint] = entry
    return inventory


def overlay_inventory_truth(row: dict, inventory: dict) -> dict:
    enriched = dict(row)
    mint_in = enriched.get("mint_in") or ""
    mint_out = enriched.get("mint_out") or ""
    input_entry = inventory.get(mint_in)
    output_entry = inventory.get(mint_out)

    required_input_raw = safe_int(enriched.get("trade_size_lamports"))
    native_sol_balance_raw = safe_int(enriched.get("native_sol_balance_raw"))
    native_sol_wrap_possible = bool(
        mint_in == WSOL_MINT and native_sol_balance_raw >= required_input_raw and required_input_raw > 0
    )

    if input_entry:
        input_amount_raw = safe_int(input_entry.get("amount_raw"))
        input_initialized = bool(input_entry.get("initialized"))
        enriched["has_input_ata"] = True
        enriched["input_ata_initialized"] = input_initialized
        enriched["input_balance_raw"] = input_amount_raw
        enriched["input_balance_sufficient"] = bool(input_initialized and input_amount_raw >= required_input_raw)
        enriched["spl_input_balance_sufficient"] = enriched["input_balance_sufficient"]
        enriched["inventory_input_token_program"] = input_entry.get("token_program_id")

    if output_entry:
        output_amount_raw = safe_int(output_entry.get("amount_raw"))
        output_initialized = bool(output_entry.get("initialized"))
        enriched["has_output_ata"] = True
        enriched["output_ata_initialized"] = output_initialized
        enriched["output_balance_raw"] = output_amount_raw
        enriched["inventory_output_token_program"] = output_entry.get("token_program_id")

    if enriched.get("would_admit"):
        output_ready_current_template = bool(
            mint_out == WSOL_MINT or (enriched.get("has_output_ata") and enriched.get("output_ata_initialized"))
        )
        spl_input_balance_sufficient = bool(enriched.get("input_balance_sufficient"))
        if not output_ready_current_template:
            enriched["execution_blocker"] = (
                "missing_output_ata" if not enriched.get("has_output_ata") else "output_ata_uninitialized"
            )
        elif not spl_input_balance_sufficient:
            if mint_in == WSOL_MINT and native_sol_wrap_possible:
                enriched["execution_blocker"] = "wsol_ata_balance_insufficient"
            elif mint_in == WSOL_MINT:
                enriched["execution_blocker"] = "native_sol_wrap_insufficient"
            else:
                enriched["execution_blocker"] = "spl_input_balance_insufficient"
        else:
            enriched["execution_blocker"] = "none"

    return enriched


def discover_scorecard():
    if explicit_scorecard:
        return Path(explicit_scorecard)
    if not search_root.exists():
        return None
    current_candidate = search_root / "39_broad_surface_discovery_current" / "surface_scorecard.json"
    if current_candidate.exists():
        return current_candidate
    candidates = sorted(
        (
            path
            for path in search_root.rglob("surface_scorecard.json")
            if path.is_file() and result_dir not in path.parents
        ),
        key=lambda path: path.stat().st_mtime_ns,
        reverse=True,
    )
    return candidates[0] if candidates else None


def classify_target_route_family(row: dict) -> str:
    route = (row.get("route") or "").strip().lower()
    route_reason = (row.get("route_reason") or "").strip().lower()
    route_family_label = (row.get("route_family_label") or "").strip().lower()
    route_tokens = [token for token in (route, route_reason, route_family_label) if token]
    joined = " ".join(route_tokens)
    if "tpu" in joined or "public" in joined:
        return "tpu_quic_public"
    if "jito" in joined:
        return "jito_bundle"
    if "harmonic" in joined:
        return "harmonic_bundle"
    for token in route_tokens:
        if token:
            return token
    return "unknown"


def build_candidate(row: dict) -> dict:
    venue = (row.get("venue") or "").strip().lower()
    proof_filter_result = text_value(row.get("proof_filter_result"))
    template_result = text_value(row.get("template_executability_result"))
    micro_market_result = text_value(row.get("micro_market_result"))
    r1_outcome = text_value(row.get("r1_outcome"))
    drop_reason = text_value(row.get("drop_reason"))
    execution_blocker = text_value(row.get("execution_blocker"))
    mint_out = row.get("mint_out") or ""
    target_route_family = classify_target_route_family(row)
    input_account_ready = bool(row.get("has_input_ata") and row.get("input_ata_initialized"))
    input_balance_ok = bool(input_account_ready and row.get("input_balance_sufficient"))
    approved_held_input = bool(row.get("is_approved_held_input"))
    held_input = bool(approved_held_input and input_balance_ok)
    positive_ev = safe_int(row.get("net_after_fee_lamports")) > 0
    output_account_ready = bool(
        mint_out == WSOL_MINT or (row.get("has_output_ata") and row.get("output_ata_initialized"))
    )
    output_account_fixable = bool(
        not output_account_ready
        and (
            not row.get("has_output_ata")
            or (row.get("execution_blocker") or "").strip() in {"missing_output_ata", "output_ata_uninitialized"}
        )
    )
    template_ready = template_result in {"passed", "not_applicable"}
    quote_ready_or_classified = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
    tiny_cap_safe = bool(row.get("would_admit")) or proof_filter_result == "no_quote"
    tiny_cap_blocker = classify_tiny_cap_blocker(row, max_tiny_lamports)
    source = (row.get("source") or "").strip().lower()
    source_current = bool(source)
    public_convertible = venue in public_supported_venues
    tpu_convertible = target_route_family in TPU_CONVERTIBLE_ROUTE_FAMILIES
    public_ready = public_convertible and target_route_family in PUBLIC_ROUTE_FAMILIES and bool(row.get("would_admit"))
    tpu_ready = bool(row.get("route_tpu_ready"))

    if not held_input:
        first_blocker = "held_input"
    elif not positive_ev:
        first_blocker = "positive_ev"
    elif not input_balance_ok:
        first_blocker = "input_balance_ok"
    elif not output_account_ready:
        first_blocker = "output_account_ok"
    elif not template_ready:
        first_blocker = "template_ready"
    elif not tiny_cap_safe:
        first_blocker = "tiny_cap_safe"
    elif not public_convertible and not tpu_convertible:
        first_blocker = "route_convertible"
    else:
        first_blocker = "ready_path_open"

    distance_vector = [
        0 if input_balance_ok else 1,
        0 if (output_account_ready or output_account_fixable) else 1,
        0 if template_ready else 1,
        0 if tiny_cap_safe else 1,
        0 if (public_convertible or tpu_convertible) else 1,
        0 if positive_ev else 1,
        0 if held_input else 1,
        0 if source_current else 1,
    ]

    rank_key = (
        distance_vector[0],
        distance_vector[1],
        distance_vector[2],
        distance_vector[3],
        distance_vector[4],
        distance_vector[5],
        distance_vector[6],
        distance_vector[7],
        0 if output_account_ready else 1,
        0 if public_ready or tpu_ready else 1,
        -safe_int(row.get("net_after_fee_lamports")),
        -safe_int(row.get("retained_ev_lamports")),
        -safe_int(row.get("slot")),
        str(row.get("pool") or ""),
    )

    candidate = dict(row)
    candidate.update(
        {
            "venue": venue,
            "source": source,
            "held_input": held_input,
            "positive_ev": positive_ev,
            "input_balance_ok": input_balance_ok,
            "output_account_ready": output_account_ready,
            "output_account_fixable": output_account_fixable,
            "template_ready": template_ready,
            "quote_ready_or_classified": quote_ready_or_classified,
            "tiny_cap_safe": tiny_cap_safe,
            "tiny_cap_blocker": tiny_cap_blocker,
            "proof_filter_result": proof_filter_result,
            "micro_market_result": micro_market_result,
            "r1_outcome": r1_outcome,
            "drop_reason": drop_reason,
            "execution_blocker": execution_blocker,
            "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
            "max_tiny_lamports": max_tiny_lamports,
            "public_convertible": public_convertible,
            "tpu_convertible": tpu_convertible,
            "public_ready": public_ready,
            "tpu_ready": tpu_ready,
            "target_route_family": target_route_family,
            "first_blocker": first_blocker,
            "distance_vector": distance_vector,
            "distance_score": sum(distance_vector),
            "rank_key": rank_key,
        }
    )
    return candidate


def compact_candidate(row: dict | None):
    if not row:
        return None
    return {
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": row.get("mint_in"),
        "mint_out": row.get("mint_out"),
        "source": row.get("source"),
        "route": row.get("route"),
        "target_route_family": row.get("target_route_family"),
        "net_after_fee_lamports": safe_int(row.get("net_after_fee_lamports")),
        "retained_ev_lamports": safe_int(row.get("retained_ev_lamports")),
        "held_input": bool(row.get("held_input")),
        "positive_ev": bool(row.get("positive_ev")),
        "input_balance_ok": bool(row.get("input_balance_ok")),
        "output_account_ok": bool(row.get("output_account_ready")),
        "output_account_fixable": bool(row.get("output_account_fixable")),
        "template_ready": bool(row.get("template_ready")),
        "tiny_cap_safe": bool(row.get("tiny_cap_safe")),
        "tiny_cap_blocker": row.get("tiny_cap_blocker"),
        "proof_filter_result": row.get("proof_filter_result"),
        "micro_market_result": row.get("micro_market_result"),
        "r1_outcome": row.get("r1_outcome"),
        "drop_reason": row.get("drop_reason"),
        "execution_blocker": row.get("execution_blocker"),
        "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "max_tiny_lamports": safe_int(row.get("max_tiny_lamports")),
        "public_convertible": bool(row.get("public_convertible")),
        "tpu_convertible": bool(row.get("tpu_convertible")),
        "public_ready": bool(row.get("public_ready")),
        "tpu_ready": bool(row.get("tpu_ready")),
        "first_blocker": row.get("first_blocker"),
        "distance_score": row.get("distance_score"),
    }


scorecard_path = discover_scorecard()
scorecard_path_str = str(scorecard_path) if scorecard_path else ""
explicit_scorecard_path = str(Path(explicit_scorecard).resolve()) if explicit_scorecard else ""
resolved_scorecard_path = str(scorecard_path.resolve()) if scorecard_path and scorecard_path.exists() else ""
current_scorecard_loaded = bool(
    explicit_scorecard_path and resolved_scorecard_path and explicit_scorecard_path == resolved_scorecard_path
)
stale_scorecard_used = bool(
    scorecard_path_str and not current_scorecard_loaded and "current" not in scorecard_path_str.lower()
)

if scorecard_path is None or not scorecard_path.exists():
    payload = {
        "status": "blocked_missing_scorecard",
        "scorecard_path": scorecard_path_str,
        "current_scorecard_loaded": current_scorecard_loaded,
        "stale_scorecard_used": stale_scorecard_used,
    }
    summary_lines = [
        "status=blocked_missing_scorecard",
        f"scorecard_path={scorecard_path_str}",
        f"current_scorecard_loaded={'true' if current_scorecard_loaded else 'false'}",
        f"stale_scorecard_used={'true' if stale_scorecard_used else 'false'}",
    ]
    summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
    summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    raise SystemExit(2)

scorecard = load_json(scorecard_path)
inventory = load_inventory(inventory_path)
inventory_overlay_used = bool(inventory)
rows = [
    build_candidate(overlay_inventory_truth(row, inventory))
    for row in list(scorecard.get("r1_surface_observations") or [])
    if (row.get("venue") or "").strip().lower() in supported_venues
]

venue_rows = {venue: [] for venue in supported_venues}
for row in rows:
    venue_rows.setdefault(row.get("venue") or "", []).append(row)

venue_autopsies = {}
for venue in supported_venues:
    current_rows = list(venue_rows.get(venue) or [])
    held_rows = [row for row in current_rows if row.get("held_input")]
    positive_rows = [row for row in held_rows if row.get("positive_ev")]
    input_balance_rows = [row for row in positive_rows if row.get("input_balance_ok")]
    output_rows = [row for row in input_balance_rows if row.get("output_account_ready")]
    template_rows = [row for row in output_rows if row.get("template_ready")]
    tiny_rows = [row for row in template_rows if row.get("tiny_cap_safe")]
    public_rows = [row for row in tiny_rows if row.get("public_convertible")]
    tpu_rows = [row for row in tiny_rows if row.get("tpu_convertible")]

    first_blocker_counts = Counter(row.get("first_blocker") or "unknown" for row in current_rows)
    nearest_candidate = min(current_rows, key=lambda item: item.get("rank_key")) if current_rows else None
    venue_autopsies[venue] = {
        "observed": len(current_rows),
        "held_input": len(held_rows),
        "positive_ev": len(positive_rows),
        "input_balance_ok": len(input_balance_rows),
        "output_account_ok": len(output_rows),
        "template_ready": len(template_rows),
        "tiny_cap_safe": len(tiny_rows),
        "public_convertible": len(public_rows),
        "tpu_convertible": len(tpu_rows),
        "first_blocker": nearest_candidate.get("first_blocker") if nearest_candidate else "no_observed_rows",
        "first_blocker_counts": dict(first_blocker_counts),
        "nearest_candidate": compact_candidate(nearest_candidate),
    }

nearest_overall = min(rows, key=lambda item: item.get("rank_key")) if rows else None
status = "run54_all_venue_autopsy_complete" if rows else "run54_no_supported_venue_rows"

payload = {
    "status": status,
    "scorecard_path": scorecard_path_str,
    "current_scorecard_loaded": current_scorecard_loaded,
    "stale_scorecard_used": stale_scorecard_used,
    "inventory_overlay_path": str(inventory_path) if inventory_path else "",
    "inventory_overlay_used": inventory_overlay_used,
    "supported_venues": supported_venues,
    "public_supported_venues": sorted(public_supported_venues),
    "venue_autopsy": venue_autopsies,
    "selected_next_target": compact_candidate(nearest_overall),
}

summary_lines = [
    f"status={status}",
    f"scorecard_path={scorecard_path_str}",
    f"current_scorecard_loaded={'true' if current_scorecard_loaded else 'false'}",
    f"stale_scorecard_used={'true' if stale_scorecard_used else 'false'}",
    f"inventory_overlay_path={inventory_path if inventory_path else ''}",
    f"inventory_overlay_used={'true' if inventory_overlay_used else 'false'}",
    f"supported_venue_count={len(supported_venues)}",
    f"selected_venue={(nearest_overall or {}).get('venue', '')}",
    f"selected_pool={(nearest_overall or {}).get('pool', '')}",
    f"selected_mint_in={(nearest_overall or {}).get('mint_in', '')}",
    f"selected_mint_out={(nearest_overall or {}).get('mint_out', '')}",
    f"selected_source={(nearest_overall or {}).get('source', '')}",
    f"selected_route={(nearest_overall or {}).get('route', '')}",
    f"selected_first_blocker={(nearest_overall or {}).get('first_blocker', '')}",
    f"selected_distance_score={(nearest_overall or {}).get('distance_score', '')}",
    f"selected_tiny_cap_safe={'true' if (nearest_overall or {}).get('tiny_cap_safe') else 'false'}",
    f"selected_tiny_cap_blocker={(nearest_overall or {}).get('tiny_cap_blocker', '')}",
    f"selected_proof_filter_result={(nearest_overall or {}).get('proof_filter_result', '')}",
    f"selected_micro_market_result={(nearest_overall or {}).get('micro_market_result', '')}",
    f"selected_r1_outcome={(nearest_overall or {}).get('r1_outcome', '')}",
    f"selected_drop_reason={(nearest_overall or {}).get('drop_reason', '')}",
    f"selected_execution_blocker={(nearest_overall or {}).get('execution_blocker', '')}",
    f"selected_trade_size_lamports={(nearest_overall or {}).get('trade_size_lamports', '')}",
    f"selected_max_tiny_lamports={(nearest_overall or {}).get('max_tiny_lamports', '')}",
    f"selected_public_convertible={'true' if (nearest_overall or {}).get('public_convertible') else 'false'}",
    f"selected_tpu_convertible={'true' if (nearest_overall or {}).get('tpu_convertible') else 'false'}",
    f"selected_output_account_fixable={'true' if (nearest_overall or {}).get('output_account_fixable') else 'false'}",
]

for venue in supported_venues:
    prefix = f"venue_{venue}"
    venue_summary = venue_autopsies[venue]
    summary_lines.extend(
        [
            f"{prefix}_observed={venue_summary['observed']}",
            f"{prefix}_held_input={venue_summary['held_input']}",
            f"{prefix}_positive_ev={venue_summary['positive_ev']}",
            f"{prefix}_input_balance_ok={venue_summary['input_balance_ok']}",
            f"{prefix}_output_account_ok={venue_summary['output_account_ok']}",
            f"{prefix}_template_ready={venue_summary['template_ready']}",
            f"{prefix}_tiny_cap_safe={venue_summary['tiny_cap_safe']}",
            f"{prefix}_public_convertible={venue_summary['public_convertible']}",
            f"{prefix}_tpu_convertible={venue_summary['tpu_convertible']}",
            f"{prefix}_first_blocker={venue_summary['first_blocker']}",
        ]
    )

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

pass "Run54 all-venue execution autopsy complete"
finish
exit 0

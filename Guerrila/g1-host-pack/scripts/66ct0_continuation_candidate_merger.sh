#!/usr/bin/env bash
# 66ct0_continuation_candidate_merger.sh - merge continuation habitats with fresh rerouted impulse rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CT0 - Continuation Candidate Merger"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]] && export SHADOW_RESULTS_DIR="$PACK_DIR/results"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66CT0_STEP:-66ct0_continuation_candidate_merger}"
WATCHLIST_JSON="${SHADOW_RUN66CT0_WATCHLIST_JSON:-$REPO_ROOT/continuation/watchlist/latest_watchlist.json}"
ACTIVE_DISCOVERY_JSON="${SHADOW_RUN66CT0_ACTIVE_DISCOVERY_JSON:-$REPO_ROOT/continuation/active_discovery/latest_active_continuation_discovery.json}"
RUN80D_ROWS="${SHADOW_RUN66CT0_RUN80D_ROWS:-$REPO_ROOT/continuation/run80d/rows.jsonl}"
PERSISTENCE_JSON="${SHADOW_RUN66CT0_PERSISTENCE_JSON:-$REPO_ROOT/continuation/studies/hourly_persistence/study_summary.json}"
ROUTE_ROWS="${SHADOW_RUN66CT0_ROUTE_ROWS:-}"
CANDIDATE_AUTOPSY_ROWS="${SHADOW_RUN66CT0_CANDIDATE_AUTOPSY_ROWS:-}"
TEMPLATE_ROWS="${SHADOW_RUN66CT0_TEMPLATE_ROWS:-}"
RULE_ROWS="${SHADOW_RUN66CT0_RULE_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$ROUTE_ROWS" ]] || ROUTE_ROWS="$(latest_file route_to_continuation_rows.jsonl)"
[[ -n "$CANDIDATE_AUTOPSY_ROWS" ]] || CANDIDATE_AUTOPSY_ROWS="$(latest_file candidate_autopsy.jsonl)"
[[ -n "$TEMPLATE_ROWS" ]] || TEMPLATE_ROWS="$(latest_file run66rt_rebalancing_template_nosend_rows.jsonl)"
[[ -n "$RULE_ROWS" ]] || RULE_ROWS="$(latest_file run66_rebalancing_rule_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/continuation_candidate_manifest.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ct0_continuation_candidate_merger.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$WATCHLIST_JSON" "$ACTIVE_DISCOVERY_JSON" "$RUN80D_ROWS" "$PERSISTENCE_JSON" "$ROUTE_ROWS" "$CANDIDATE_AUTOPSY_ROWS" "$TEMPLATE_ROWS" "$RULE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

watchlist_arg, active_arg, run80d_arg, persistence_arg, route_arg, candidate_autopsy_arg, template_arg, rule_arg, result_dir_arg, manifest_arg, rows_out_arg, summary_arg, summary_txt_arg = sys.argv[1:14]
watchlist_path = Path(watchlist_arg) if watchlist_arg else None
active_path = Path(active_arg) if active_arg else None
run80d_path = Path(run80d_arg) if run80d_arg else None
persistence_path = Path(persistence_arg) if persistence_arg else None
route_path = Path(route_arg) if route_arg else None
candidate_autopsy_path = Path(candidate_autopsy_arg) if candidate_autopsy_arg else None
template_path = Path(template_arg) if template_arg else None
rule_path = Path(rule_arg) if rule_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)

def load_json(path):
    if not path or not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8", errors="replace"))

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def text(value):
    return "" if value is None else str(value).strip()

def lower(value):
    return text(value).lower()

def num(value):
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None

def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}

def first_text(row, *keys):
    for key in keys:
        value = row.get(key)
        if text(value):
            return text(value)
    return ""

def first_num(row, *keys):
    for key in keys:
        value = num(row.get(key))
        if value is not None:
            return value
    return None

def nested_num(row, *path):
    value = row
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    if isinstance(value, dict):
        for key in (
            "net_after_fee_lamports",
            "net_after_fee_at_horizon",
            "net_after_cost_lamports",
            "markout_lamports",
            "lamports",
            "value",
        ):
            parsed = num(value.get(key))
            if parsed is not None:
                return parsed
        return None
    return num(value)

def nested_bool(row, *path):
    value = row
    for key in path:
        if not isinstance(value, dict):
            return False
        value = value.get(key)
    return boolish(value)

def by_candidate(rows):
    out = {}
    for row in rows:
        cid = first_text(row, "candidate_id", "row_id")
        if cid:
            out[cid] = row
    return out

def merge(*rows):
    out = {}
    for row in rows:
        if row:
            for key, value in row.items():
                if value not in (None, ""):
                    out[key] = value
    return out

def pool_key(value):
    return text(value)

def pair_text(row):
    return first_text(row, "pair", "mint_pair") or f"{first_text(row, 'mint_in')}->{first_text(row, 'mint_out')}"

def row_pool(row):
    return first_text(row, "pool", "pool_address", "pair_address", "address")

def watchlist_rows(doc):
    if not isinstance(doc, dict):
        return []
    for key in ("watchlist", "candidates", "top_candidates", "rows"):
        value = doc.get(key)
        if isinstance(value, list):
            return value
    return []

def active_rows(doc):
    if not isinstance(doc, dict):
        return []
    for key in ("top_candidates", "candidates", "rows", "watchlist"):
        value = doc.get(key)
        if isinstance(value, list):
            return value
    return []

template_by_candidate = by_candidate(load_jsonl(template_path))
rule_by_candidate = by_candidate(load_jsonl(rule_path))
route_rows = load_jsonl(route_path)
candidate_autopsy_rows = load_jsonl(candidate_autopsy_path)
watch_doc = load_json(watchlist_path) or {}
active_doc = load_json(active_path) or {}
run80d_rows = load_jsonl(run80d_path)
persistence_doc = load_json(persistence_path) or {}

habitat_by_pool = {}
for source_name, rows in (
    ("watchlist", watchlist_rows(watch_doc)),
    ("active_discovery", active_rows(active_doc)),
    ("run80d", run80d_rows),
):
    for idx, row in enumerate(rows, start=1):
        pool = row_pool(row)
        if not pool:
            continue
        entry = habitat_by_pool.setdefault(pool, {})
        score = first_num(row, "continuation_live_score", "continuation_score") or first_num(row.get("scores", {}) if isinstance(row.get("scores"), dict) else {}, "continuation_live_score")
        deployable = first_num(row.get("scores", {}) if isinstance(row.get("scores"), dict) else {}, "continuation_deployable_score")
        rank = first_num(row, "rank") or idx
        entry.update({
            f"{source_name}_present": True,
            f"{source_name}_rank": rank,
            f"{source_name}_score": score,
            f"{source_name}_deployable_score": deployable,
            f"{source_name}_best_side": first_text(row, "best_side", "side") or first_text((row.get("continuation_live") or {}) if isinstance(row.get("continuation_live"), dict) else {}, "best_side"),
            f"{source_name}_pair": pair_text(row),
            f"{source_name}_venue": first_text(row, "venue"),
        })

for row in run80d_rows:
    pool = row_pool(row)
    if pool:
        habitat_by_pool.setdefault(pool, {})["habitat_score"] = first_num(row.get("scores", {}) if isinstance(row.get("scores"), dict) else {}, "habitat_score")

out = []
seen_pools = set()
seen_candidates = set()
for route in route_rows:
    cid = first_text(route, "candidate_id", "row_id")
    if cid:
        seen_candidates.add(cid)
    row = merge(rule_by_candidate.get(cid), template_by_candidate.get(cid), route)
    pool = row_pool(row)
    seen_pools.add(pool)
    habitat = habitat_by_pool.get(pool, {})
    execution_ready = boolish(row.get("execution_plumbing_ready")) or first_text(row, "execution_decision", "decision") in {"REBALANCING_SENDER_READY_NO_SEND", "REBALANCING_BUNDLE_READY_NO_SUBMIT"}
    has_habitat = any(habitat.get(f"{source}_present") for source in ("watchlist", "active_discovery", "run80d"))
    if execution_ready and has_habitat:
        decision = "CONTINUATION_PRIORITY_TEST"
    elif execution_ready or boolish(row.get("from_false_restoration_reject")) or first_text(row, "recommended_route") == "continuation_structural_certificate":
        decision = "CONTINUATION_FRESH_IMPULSE"
    else:
        decision = "CONTINUATION_HABITAT_WATCH" if has_habitat else "CONTINUATION_NO_OVERLAP_CONTEXT_ONLY"
    out.append({
        "candidate_id": cid,
        "original_impulse_id": first_text(row, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature") or cid,
        "slot": first_num(row, "slot"),
        "candidate_ingress_unix_ns": first_num(row, "candidate_ingress_unix_ns", "unix_ns"),
        "pool": pool,
        "venue": first_text(row, "venue"),
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "mint_pair": pair_text(row),
        "best_side": first_text(row, "best_side", "direction"),
        "source": "66rg6_false_restoration_route",
        "source_owner_mode_before_reroute": first_text(row, "owner_mode_selected"),
        "habitat_score": habitat.get("habitat_score"),
        "watchlist_rank": habitat.get("watchlist_rank"),
        "watchlist_score": habitat.get("watchlist_score"),
        "active_discovery_score": habitat.get("active_discovery_score"),
        "run80d_score": habitat.get("run80d_score"),
        "same_direction_continuation_pressure_score": first_num(row, "same_direction_continuation_pressure_score", "continuation_pressure_score"),
        "same_direction_followthrough_score": first_num(row, "same_direction_followthrough_score"),
        "opposite_direction_absorption_score": first_num(row, "opposite_direction_absorption_score", "absorption_score"),
        "from_false_restoration_reject": True,
        "false_restoration_reason": first_text(row, "false_restoration_reason"),
        "recommended_route": first_text(row, "recommended_route") or "continuation_structural_certificate",
        "execution_ready_from_66rg6": execution_ready,
        "execution_decision": first_text(row, "execution_decision", "decision"),
        "route_specific_ev_after_cost_raw": first_num(row, "route_specific_ev_after_cost_raw"),
        "expected_out_raw": first_num(row, "expected_out_raw", "quote_expected_out_raw"),
        "min_out_raw": first_num(row, "min_out_raw"),
        "best_horizon": first_text(row, "best_horizon"),
        "entry_rule_present": boolish(row.get("entry_rule_present")),
        "exit_rule_present": boolish(row.get("exit_rule_present")),
        "entry_trigger": first_text(row, "entry_trigger"),
        "exit_trigger": first_text(row, "exit_trigger"),
        "max_hold_horizon": first_text(row, "max_hold_horizon"),
        "quote_ready": boolish(row.get("quote_ready")),
        "numeric_quote_min_out": boolish(row.get("numeric_quote_min_out")),
        "route_usable": boolish(row.get("route_usable")),
        "template_ready": boolish(row.get("template_ready")),
        "actual_markout_at_50ms": first_num(row, "actual_markout_at_50ms"),
        "actual_markout_at_100ms": first_num(row, "actual_markout_at_100ms"),
        "actual_markout_at_250ms": first_num(row, "actual_markout_at_250ms"),
        "actual_markout_at_500ms": first_num(row, "actual_markout_at_500ms"),
        "actual_markout_at_1s": first_num(row, "actual_markout_at_1s"),
        "actual_markout_at_2s": first_num(row, "actual_markout_at_2s"),
        "actual_markout_at_5s": first_num(row, "actual_markout_at_5s"),
        "actual_markout_at_10s": first_num(row, "actual_markout_at_10s"),
        "actual_markout_at_30s": first_num(row, "actual_markout_at_30s"),
        "next_same_pool_event_1_markout": first_num(row, "next_same_pool_event_1_markout"),
        "next_same_pool_event_2_markout": first_num(row, "next_same_pool_event_2_markout"),
        "next_same_pool_event_3_markout": first_num(row, "next_same_pool_event_3_markout"),
        "max_adverse_excursion": first_num(row, "max_adverse_excursion", "max_adverse_excursion_bps"),
        "max_adverse_excursion_bounded": boolish(row.get("max_adverse_excursion_bounded")),
        "actual_continuation_positive": boolish(row.get("actual_continuation_positive")),
        "actual_toxic": boolish(row.get("actual_toxic")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })

for direct in candidate_autopsy_rows:
    selected_owner = lower(first_text(direct, "selected_owner", "owner_mode_selected", "owner_mode"))
    if selected_owner != "continuation":
        continue
    cid = first_text(direct, "candidate_id", "row_id")
    if not cid or cid in seen_candidates:
        continue
    seen_candidates.add(cid)
    row = merge(rule_by_candidate.get(cid), template_by_candidate.get(cid), direct)
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    pool = row_pool(row)
    seen_pools.add(pool)
    habitat = habitat_by_pool.get(pool, {})
    execution_blocked = boolish(row.get("execution_blocked"))
    execution_blocker = first_text(row, "execution_blocker")
    best_horizon = first_text(markouts, "best_horizon") or first_text(row, "best_horizon")
    best_net = first_num(row, "route_specific_ev_after_cost_raw", "best_horizon_net_after_fee_lamports")
    if best_net is None:
        best_net = nested_num(row, "markouts", "best_horizon_net_after_fee_lamports")
    max_adv = first_num(row, "max_adverse_excursion", "max_adverse_excursion_bps")
    if max_adv is None:
        max_adv = nested_num(row, "markouts", "max_adverse_excursion_bps")
    pressure = first_num(row, "same_direction_continuation_pressure_score", "same_direction_followthrough_score", "continuation_score", "owner_mode_score_continuation")
    has_habitat = any(habitat.get(f"{source}_present") for source in ("watchlist", "active_discovery", "run80d"))
    execution_ready = not execution_blocked
    if execution_ready and has_habitat:
        decision = "CONTINUATION_PRIORITY_TEST"
    elif pressure is not None and pressure > 0:
        decision = "CONTINUATION_FRESH_IMPULSE"
    else:
        decision = "CONTINUATION_HABITAT_WATCH" if has_habitat else "CONTINUATION_NO_OVERLAP_CONTEXT_ONLY"
    output_ready = boolish(row.get("output_account_ready")) or boolish(row.get("has_output_ata"))
    input_ready = boolish(row.get("input_account_ready")) or boolish(row.get("has_input_ata"))
    out.append({
        "candidate_id": cid,
        "original_impulse_id": first_text(row, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature") or cid,
        "slot": first_num(row, "slot"),
        "candidate_ingress_unix_ns": first_num(row, "candidate_ingress_unix_ns", "unix_ns"),
        "pool": pool,
        "venue": first_text(row, "venue"),
        "mint_in": first_text(row, "mint_in"),
        "mint_out": first_text(row, "mint_out"),
        "mint_pair": pair_text(row),
        "best_side": first_text(row, "best_side", "direction"),
        "source": "fresh_runtime_owner_continuation",
        "source_owner_mode_before_reroute": "",
        "owner_mode_selected": "continuation",
        "owner_mode_match_continuation": True,
        "owner_mode_score_continuation": pressure,
        "owner_mode_score_source": "fresh_runtime",
        "runner_up_owner": first_text(row, "runner_up_owner"),
        "fallback_owner": first_text(row, "fallback_owner"),
        "habitat_score": habitat.get("habitat_score"),
        "watchlist_rank": habitat.get("watchlist_rank"),
        "watchlist_score": habitat.get("watchlist_score"),
        "active_discovery_score": habitat.get("active_discovery_score"),
        "run80d_score": habitat.get("run80d_score"),
        "same_direction_continuation_pressure_score": pressure,
        "same_direction_followthrough_score": (
            1.0 if nested_bool(row, "markouts", "follow_through") else first_num(row, "same_direction_followthrough_score")
        ),
        "opposite_direction_absorption_score": first_num(row, "opposite_direction_absorption_score", "absorption_score") or nested_num(row, "markouts", "absorption"),
        "from_false_restoration_reject": False,
        "false_restoration_reason": first_text(row, "false_restoration_reason"),
        "recommended_route": "continuation_structural_certificate",
        "execution_ready_from_66rg6": False,
        "execution_ready_from_fresh_owner": execution_ready,
        "execution_blocked": execution_blocked,
        "execution_blocker": execution_blocker,
        "execution_decision": "CONTINUATION_DIRECT_OWNER_EXECUTION_READY" if execution_ready else f"BLOCKED_{execution_blocker or 'execution_blocked'}",
        "route_specific_ev_after_cost_raw": best_net,
        "expected_out_raw": first_num(row, "expected_out_raw", "quote_expected_out_raw"),
        "min_out_raw": first_num(row, "min_out_raw", "quote_min_out_raw"),
        "best_horizon": best_horizon,
        "entry_rule_present": boolish(row.get("entry_rule_present")),
        "exit_rule_present": boolish(row.get("exit_rule_present")),
        "entry_trigger": first_text(row, "entry_trigger"),
        "exit_trigger": first_text(row, "exit_trigger"),
        "max_hold_horizon": first_text(row, "max_hold_horizon") or best_horizon,
        "quote_ready": boolish(row.get("quote_ready")),
        "numeric_quote_min_out": boolish(row.get("numeric_quote_min_out")),
        "route_usable": boolish(row.get("route_usable")) or boolish(row.get("route_supported")),
        "input_account_ready": input_ready,
        "output_account_ready": output_ready,
        "has_input_ata": boolish(row.get("has_input_ata")),
        "has_output_ata": boolish(row.get("has_output_ata")),
        "input_balance_ok": boolish(row.get("input_balance_ok")),
        "input_balance_sufficient": boolish(row.get("input_balance_sufficient")),
        "template_ready": boolish(row.get("template_ready")) or execution_ready,
        "account_or_inline_ata_ready": input_ready and output_ready,
        "actual_markout_at_50ms": nested_num(row, "markouts", "time_horizons", "50ms"),
        "actual_markout_at_100ms": nested_num(row, "markouts", "time_horizons", "100ms"),
        "actual_markout_at_250ms": nested_num(row, "markouts", "time_horizons", "250ms"),
        "actual_markout_at_500ms": nested_num(row, "markouts", "time_horizons", "500ms"),
        "actual_markout_at_1s": nested_num(row, "markouts", "time_horizons", "1s"),
        "actual_markout_at_2s": nested_num(row, "markouts", "time_horizons", "2s"),
        "actual_markout_at_5s": nested_num(row, "markouts", "time_horizons", "5s"),
        "actual_markout_at_10s": nested_num(row, "markouts", "time_horizons", "10s"),
        "actual_markout_at_30s": nested_num(row, "markouts", "time_horizons", "30s"),
        "next_same_pool_event_1_markout": nested_num(row, "markouts", "event_horizons", "next_same_pool_event_1"),
        "next_same_pool_event_2_markout": nested_num(row, "markouts", "event_horizons", "next_same_pool_event_2"),
        "next_same_pool_event_3_markout": nested_num(row, "markouts", "event_horizons", "next_same_pool_event_3"),
        "max_adverse_excursion": max_adv,
        "max_adverse_excursion_bounded": max_adv is not None and max_adv <= 50,
        "actual_continuation_positive": best_net is not None and best_net > 0,
        "actual_toxic": boolish(row.get("actual_toxic")) or nested_bool(row, "markouts", "reversal"),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })

for pool, habitat in habitat_by_pool.items():
    if pool in seen_pools:
        continue
    out.append({
        "candidate_id": f"habitat:{pool}",
        "original_impulse_id": "",
        "slot": None,
        "candidate_ingress_unix_ns": None,
        "pool": pool,
        "venue": habitat.get("watchlist_venue") or habitat.get("active_discovery_venue") or habitat.get("run80d_venue"),
        "mint_in": "",
        "mint_out": "",
        "mint_pair": habitat.get("watchlist_pair") or habitat.get("active_discovery_pair") or habitat.get("run80d_pair") or "",
        "best_side": habitat.get("watchlist_best_side") or habitat.get("active_discovery_best_side") or habitat.get("run80d_best_side") or "",
        "source": "continuation_habitat_context",
        "source_owner_mode_before_reroute": "",
        "habitat_score": habitat.get("habitat_score"),
        "watchlist_rank": habitat.get("watchlist_rank"),
        "watchlist_score": habitat.get("watchlist_score"),
        "active_discovery_score": habitat.get("active_discovery_score"),
        "run80d_score": habitat.get("run80d_score"),
        "same_direction_continuation_pressure_score": None,
        "from_false_restoration_reject": False,
        "execution_ready_from_66rg6": False,
        "decision": "CONTINUATION_HABITAT_WATCH",
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })

out.sort(key=lambda row: (
    row["decision"] == "CONTINUATION_PRIORITY_TEST",
    row["decision"] == "CONTINUATION_FRESH_IMPULSE",
    boolish(row.get("execution_ready_from_66rg6")),
    row.get("watchlist_score") or row.get("active_discovery_score") or row.get("run80d_score") or 0,
), reverse=True)
counts = Counter(row["decision"] for row in out)
manifest = {
    "watchlist_json": str(watchlist_path or ""),
    "active_discovery_json": str(active_path or ""),
    "run80d_rows": str(run80d_path or ""),
    "persistence_json": str(persistence_path or ""),
    "route_rows": str(route_path or ""),
    "candidate_autopsy_rows": str(candidate_autopsy_path or ""),
    "template_rows": str(template_path or ""),
    "rule_rows": str(rule_path or ""),
    "result_dir": str(result_dir),
    "broad_hunt": False,
    "live_send": False,
    "sendBundle": False,
}
summary = {
    "status": "run66ct0_continuation_candidate_merger_complete",
    "row_count": len(out),
    "route_rows_loaded": len(route_rows),
    "candidate_autopsy_rows_loaded": len(candidate_autopsy_rows),
    "direct_continuation_rows_loaded": sum(
        1 for row in candidate_autopsy_rows
        if lower(first_text(row, "selected_owner", "owner_mode_selected", "owner_mode")) == "continuation"
    ),
    "direct_continuation_rows": sum(1 for row in out if row["source"] == "fresh_runtime_owner_continuation"),
    "habitat_pool_count": len(habitat_by_pool),
    "decision_counts": dict(counts),
    "fresh_impulse_rows": sum(
        1 for row in out
        if row["from_false_restoration_reject"] or row["source"] == "fresh_runtime_owner_continuation"
    ),
    "execution_ready_from_66rg6_rows": sum(1 for row in out if row["execution_ready_from_66rg6"]),
    "habitat_context_rows": sum(1 for row in out if row["source"] == "continuation_habitat_context"),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, dict) else v}" for k, v in summary.items()]
summary_txt_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66CT0 continuation candidate merger written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

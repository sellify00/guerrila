#!/usr/bin/env bash
# 66ctp_continuation_selected_private_replay_nosend.sh - selected Continuation candidate fresh quote replay, no-send

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTP - Continuation Selected Private Replay No-Send"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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
STEP="${SHADOW_RUN66CTP_STEP:-66ctp_continuation_selected_private_replay_nosend}"
SOURCE_ROWS="${SHADOW_RUN66CTP_SOURCE_ROWS:-}"
ENRICH_ROWS="${SHADOW_RUN66CTP_ENRICH_ROWS:-}"
MAX_ROWS="${SHADOW_RUN66CTP_MAX_ROWS:-2}"
QUOTE_API_URL="${SHADOW_RUN66CTP_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN66CTP_ENTRY_SLIPPAGE_BPS:-50}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN66CTP_EXIT_SLIPPAGE_BPS:-50}"
QUOTE_TIMEOUT_SECS="${SHADOW_RUN66CTP_QUOTE_TIMEOUT_SECS:-20}"
MAX_ENTRY_DRIFT_BPS="${SHADOW_RUN66CTP_MAX_ENTRY_DRIFT_BPS:-2500}"
MODE="${SHADOW_RUN66CTP_MODE:-strict_identity_replay}"
THESIS_SIGNAL_SOURCE="${SHADOW_RUN66CTP_THESIS_SIGNAL_SOURCE:-carried_proposal_fields}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

append_enrich_default() {
    local path="$1"
    [[ -n "$path" ]] || return 0
    if [[ -n "$ENRICH_ROWS" ]]; then
        ENRICH_ROWS="$ENRICH_ROWS:$path"
    else
        ENRICH_ROWS="$path"
    fi
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named proposal_eligible_continuation_rows.jsonl)"
if [[ -z "$ENRICH_ROWS" ]]; then
    append_enrich_default "$(latest_named run66q2_rebalancing_exact_quote_route_ev_rows.jsonl)"
    append_enrich_default "$(latest_named run66q_rebalancing_quote_minout_rows.jsonl)"
fi

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/selected_replay_rows.jsonl"
CHOICE_JSON="$RESULT_DIR/selected_row_choice.json"
SUMMARY_JSON="$RESULT_DIR/run66ctp_continuation_selected_private_replay_nosend.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$CHOICE_JSON" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$ENRICH_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$CHOICE_JSON" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$MAX_ROWS" "$QUOTE_API_URL" "$ENTRY_SLIPPAGE_BPS" "$EXIT_SLIPPAGE_BPS" "$QUOTE_TIMEOUT_SECS" "$MAX_ENTRY_DRIFT_BPS" "$MODE" "$THESIS_SIGNAL_SOURCE" <<'PY'
import json
import math
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

(
    source_arg,
    enrich_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    choice_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    max_rows_arg,
    quote_api_url,
    entry_slippage_arg,
    exit_slippage_arg,
    timeout_arg,
    max_entry_drift_arg,
    mode_arg,
    thesis_signal_source_arg,
) = sys.argv[1:18]

source_path = Path(source_arg) if source_arg else None
enrich_paths = [Path(p) for p in enrich_arg.replace(",", ":").split(":") if p.strip()]
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
choice_path = Path(choice_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
max_rows = max(1, int(max_rows_arg or "2"))
entry_slippage_bps = int(float(entry_slippage_arg or 50))
exit_slippage_bps = int(float(exit_slippage_arg or 50))
timeout_secs = float(timeout_arg or 20)
max_entry_drift_bps = float(max_entry_drift_arg or 2500)
mode = ("" if mode_arg is None else str(mode_arg).strip()) or "strict_identity_replay"
if mode not in {"strict_identity_replay", "fresh_thesis_revalidation"}:
    mode = "strict_identity_replay"
thesis_signal_source = ("" if thesis_signal_source_arg is None else str(thesis_signal_source_arg).strip()) or "carried_proposal_fields"
if thesis_signal_source not in {"carried_proposal_fields", "fresh_runtime_recomputed"}:
    thesis_signal_source = "carried_proposal_fields"

WSOL_MINT = "So11111111111111111111111111111111111111112"
CORE_MINT_DECIMALS = {
    WSOL_MINT: 9,
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": 6,
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": 6,
}


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        rows.append(json.loads(line))
    return rows


def text(value):
    return "" if value is None else str(value).strip()


def lower(value):
    return text(value).lower()


def boolish(value):
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "ready"}


def as_int(value, default=0):
    if value is None or value == "":
        return default
    try:
        if isinstance(value, float) and math.isnan(value):
            return default
        return int(float(value))
    except (TypeError, ValueError, OverflowError):
        return default


def as_float(value, default=None):
    if value is None or value == "":
        return default
    try:
        value = float(value)
        return default if math.isnan(value) else value
    except (TypeError, ValueError, OverflowError):
        return default


def first_int(row, *keys):
    for key in keys:
        value = as_int(row.get(key), None)
        if value is not None and value > 0:
            return value
    return 0


def candidate_id(row):
    return text(row.get("candidate_id"))


def by_candidate(rows):
    out = {}
    for row in rows:
        cid = candidate_id(row)
        if cid:
            out[cid] = {**out.get(cid, {}), **row}
    return out


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_float(row, *keys):
    for key in keys:
        value = as_float(row.get(key), None)
        if value is not None:
            return value
    return None


def infer_direction(mint_in, mint_out):
    mint_in = text(mint_in)
    mint_out = text(mint_out)
    if mint_in == WSOL_MINT and mint_out and mint_out != WSOL_MINT:
        return "buy"
    if mint_out == WSOL_MINT and mint_in and mint_in != WSOL_MINT:
        return "sell"
    return ""


def original_impulse_id(row):
    explicit = first_text(row, "original_impulse_id", "impulse_id", "signature_or_impulse_id", "signature")
    if explicit:
        return explicit
    cid = candidate_id(row)
    parts = cid.split(":")
    if len(parts) >= 3 and parts[2]:
        return parts[2]
    return cid


def quote(input_mint, output_mint, amount, slippage_bps):
    if not input_mint or not output_mint or amount <= 0:
        return {
            "ok": False,
            "error": "missing_quote_input",
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": amount,
        }
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(int(amount)),
        "slippageBps": str(int(slippage_bps)),
        "swapMode": "ExactIn",
        "restrictIntermediateTokens": "true",
    }
    url = quote_api_url + ("&" if "?" in quote_api_url else "?") + urllib.parse.urlencode(params)
    request = urllib.request.Request(url, headers={"User-Agent": "shadow-66ctp-nosend/1.0"})
    started = time.time()
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=timeout_secs, context=context) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:1000]
        return {
            "ok": False,
            "error": f"http_{exc.code}",
            "body": body,
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": amount,
            "latency_ms": round((time.time() - started) * 1000, 3),
        }
    except Exception as exc:
        return {
            "ok": False,
            "error": type(exc).__name__,
            "body": str(exc)[:1000],
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": amount,
            "latency_ms": round((time.time() - started) * 1000, 3),
        }
    out_amount = as_int(payload.get("outAmount"), 0)
    min_out = as_int(payload.get("otherAmountThreshold"), 0)
    return {
        "ok": out_amount > 0 and min_out > 0,
        "error": text(payload.get("error") or payload.get("message")) or None,
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": amount,
        "outAmount": out_amount,
        "otherAmountThreshold": min_out,
        "priceImpactPct": payload.get("priceImpactPct"),
        "routePlanCount": len(payload.get("routePlan") or []),
        "latency_ms": round((time.time() - started) * 1000, 3),
    }


def drift_bps(fresh_out, artifact_out):
    if artifact_out <= 0:
        return None
    return round(((fresh_out - artifact_out) / artifact_out) * 10000.0, 6)


def horizon_rank(value):
    value = text(value)
    order = {
        "next_same_pool_event_1": 1,
        "50ms": 2,
        "100ms": 3,
        "next_same_pool_event_2": 4,
        "250ms": 5,
        "500ms": 6,
        "next_same_pool_event_3": 7,
        "1s": 8,
        "2s": 9,
        "5s": 10,
        "10s": 11,
        "30s": 12,
    }
    return order.get(value, 99)


def route_rank(row):
    no_new_ata = not (
        boolish(row.get("inline_ata_creatable"))
        or boolish(row.get("ata_created"))
        or boolish(row.get("create_ata_instruction_present"))
    )
    account_score = 2 if boolish(row.get("account_ready")) else (1 if boolish(row.get("account_or_inline_ata_ready")) else 0)
    sender_score = 1 if boolish(row.get("helius_sender_no_send_ready")) else 0
    adverse = as_float(row.get("max_adverse_excursion"), 10**9)
    adverse_abs = abs(min(0.0, adverse if adverse is not None else 10**9))
    ev = as_float(row.get("route_specific_ev_after_cost_raw"), 0.0) or 0.0
    return (
        account_score,
        sender_score,
        -adverse_abs,
        -horizon_rank(row.get("best_later_horizon") or row.get("best_horizon")),
        ev,
        1 if no_new_ata else 0,
    )


def first_replay_blocker(row, evaluation_mode):
    if evaluation_mode == "fresh_thesis_revalidation":
        checks = [
            ("PM_owner_lock_failed", row["pm_owner_lock_ready"] and row["pm_reserve_ok"] and row["pm_release_on_no_send_ok"] and row["pm_pending_after"] == 0),
            ("fresh_runtime_signal_incomplete", (not row["thesis_signal_live_grade"]) or row["runtime_pressure_signal_present"]),
            ("thesis_decayed", row["pressure_still_valid"]),
            ("adverse_excursion_too_high", row["max_adverse_excursion_bounded"]),
            ("account_or_template_drift", row["account_ready_or_inline_ata_ready"] and row["template_ready"]),
            ("exit_rule_failed", row["entry_trigger_ready"] and row["confirmation_condition_ready"] and row["entry_delay_or_event_gate_ready"] and row["hold_horizon_defined"] and row["exit_trigger_ready"] and row["stop_loss_defined"]),
            ("egress_no_send_failed", row["egress_no_send_ready"]),
            ("entry_quote_negative", row["fresh_entry_positive"] and row["route_cost_conversion_ok"]),
            ("exit_not_ready", row["fresh_exit_positive"]),
            ("fresh_net_negative_after_cost", row["fresh_thesis_net_after_all_costs_positive"]),
        ]
    else:
        checks = [
            ("PM_owner_lock_failed", row["pm_owner_lock_ready"] and row["pm_reserve_ok"] and row["pm_release_on_no_send_ok"] and row["pm_pending_after"] == 0),
            ("markout_not_persistent", row["best_later_net_positive"] and row["actual_continuation_positive"]),
            ("adverse_excursion_too_high", row["max_adverse_excursion_bounded"]),
            ("account_or_template_drift", row["account_ready_or_inline_ata_ready"] and row["template_ready"]),
            ("exit_rule_failed", row["entry_trigger_ready"] and row["confirmation_condition_ready"] and row["entry_delay_or_event_gate_ready"] and row["hold_horizon_defined"] and row["exit_trigger_ready"] and row["stop_loss_defined"]),
            ("egress_no_send_failed", row["egress_no_send_ready"]),
            ("fresh_quote_drifted", row["fresh_entry_positive"] and row["fresh_exit_positive"] and row["strict_identity_replay_ready"]),
        ]
    for blocker, ok in checks:
        if not ok:
            return blocker
    return "none"


source_rows = load_jsonl(source_path)
if max_rows:
    source_rows = source_rows[:max_rows]

enrich_by_id = {}
for path in enrich_paths:
    for cid, row in by_candidate(load_jsonl(path)).items():
        enrich_by_id[cid] = {**enrich_by_id.get(cid, {}), **row}

replay_rows = []
replay_unix_ms = int(time.time() * 1000)
for index, source_row in enumerate(source_rows):
    cid = candidate_id(source_row)
    row = {**enrich_by_id.get(cid, {}), **source_row}
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    input_amount = first_int(
        row,
        "candidate_trade_size_lamports",
        "trade_size_lamports",
        "pm_owner_lock_notional_raw",
        "pm_reserved_notional_raw",
        "entry_amount_raw",
        "amount_raw",
    )
    cost_lamports = (
        first_int(row, "base_fee_raw", "base_fee_lamports")
        + first_int(row, "estimated_priority_fee_raw", "estimated_priority_fee_lamports")
        + first_int(row, "policy_tip_raw", "policy_tip_lamports", "tip_lamports")
    )

    entry_quote = quote(mint_in, mint_out, input_amount, entry_slippage_bps)
    exit_input_amount = as_int(entry_quote.get("otherAmountThreshold"), 0)
    exit_quote = quote(mint_out, mint_in, exit_input_amount, exit_slippage_bps) if exit_input_amount > 0 else {
        "ok": False,
        "error": "entry_min_out_missing",
        "inputMint": mint_out,
        "outputMint": mint_in,
        "amount": exit_input_amount,
    }

    cost_quote = None
    if mint_in == WSOL_MINT:
        cost_input_raw = cost_lamports
        cost_conversion_ok = True
    elif cost_lamports > 0:
        cost_quote = quote(WSOL_MINT, mint_in, cost_lamports, entry_slippage_bps)
        cost_input_raw = as_int(cost_quote.get("outAmount"), 0)
        cost_conversion_ok = bool(cost_quote.get("ok")) and cost_input_raw > 0
    else:
        cost_input_raw = 0
        cost_conversion_ok = True

    fresh_exit_out_raw = as_int(exit_quote.get("outAmount"), 0)
    fresh_roundtrip_net = fresh_exit_out_raw - input_amount - cost_input_raw if input_amount > 0 else None
    artifact_min_out = first_int(row, "min_out_raw", "min_out_lamports")
    artifact_expected_out = first_int(row, "expected_out_raw", "quote_expected_out_raw", "quote_expected_out_lamports")
    entry_out_drift = drift_bps(as_int(entry_quote.get("outAmount"), 0), artifact_expected_out)
    entry_min_drift = drift_bps(as_int(entry_quote.get("otherAmountThreshold"), 0), artifact_min_out)
    entry_drift_ok = entry_min_drift is not None and abs(entry_min_drift) <= max_entry_drift_bps
    original_direction = first_text(row, "original_direction", "direction", "best_side") or infer_direction(mint_in, mint_out)
    fresh_direction = first_text(row, "fresh_direction") or original_direction or infer_direction(mint_in, mint_out)
    original_pressure_score = first_float(
        row,
        "original_continuation_pressure_score",
        "same_direction_continuation_pressure_score",
        "continuation_pressure_score",
        "same_direction_followthrough_score",
    )
    runtime_pressure_score = first_float(row, "fresh_continuation_pressure_score")
    runtime_followthrough_score = first_float(row, "fresh_same_direction_followthrough_score")
    runtime_absorption_score = first_float(row, "fresh_absorption_score")
    runtime_toxicity_score = first_float(row, "fresh_opposite_direction_toxicity")
    route_lag_conflict = boolish(row.get("fresh_route_lag_conflict"))
    liquidity_migration_signal = boolish(row.get("fresh_liquidity_migration_signal"))
    runtime_pressure_signal_present = all(
        value is not None for value in (runtime_pressure_score, runtime_followthrough_score, runtime_absorption_score, runtime_toxicity_score)
    )
    runtime_pressure_still_valid = row.get("fresh_runtime_pressure_still_valid")
    if runtime_pressure_still_valid is None and thesis_signal_source == "fresh_runtime_recomputed":
        runtime_pressure_still_valid = (
            runtime_pressure_signal_present
            and runtime_pressure_score >= 0.5
            and runtime_followthrough_score >= 0.5
            and runtime_absorption_score <= 0.5
            and runtime_toxicity_score <= 0.5
            and not route_lag_conflict
            and not liquidity_migration_signal
        )
    else:
        runtime_pressure_still_valid = boolish(runtime_pressure_still_valid)
    fresh_pressure_score = runtime_pressure_score
    if fresh_pressure_score is None and thesis_signal_source != "fresh_runtime_recomputed":
        fresh_pressure_score = first_float(
            row,
            "same_direction_continuation_pressure_score",
            "continuation_pressure_score",
            "same_direction_followthrough_score",
        )
    followthrough_score_now = runtime_followthrough_score
    if followthrough_score_now is None and thesis_signal_source != "fresh_runtime_recomputed":
        followthrough_score_now = first_float(
            row,
            "same_direction_followthrough_score",
            "same_direction_continuation_pressure_score",
            "continuation_pressure_score",
        )
    best_later_net_positive = boolish(row.get("best_later_net_positive"))
    actual_continuation_positive = boolish(row.get("actual_continuation_positive"))
    same_direction_followthrough_positive = followthrough_score_now is not None and followthrough_score_now >= 0.5
    same_direction_still_valid = not original_direction or not fresh_direction or original_direction == fresh_direction
    if thesis_signal_source == "fresh_runtime_recomputed":
        pressure_still_valid = runtime_pressure_still_valid and same_direction_still_valid
    else:
        pressure_still_valid = (
            fresh_pressure_score is not None
            and fresh_pressure_score > 0
            and same_direction_still_valid
            and (
                same_direction_followthrough_positive
                or actual_continuation_positive
                or best_later_net_positive
                or bool(text(row.get("best_later_horizon") or row.get("best_horizon")))
            )
        )
    fresh_entry_positive = bool(entry_quote.get("ok")) and as_int(entry_quote.get("otherAmountThreshold"), 0) > 0
    fresh_exit_positive = bool(exit_quote.get("ok")) and as_int(exit_quote.get("otherAmountThreshold"), 0) > 0
    strict_identity_ready = (
        fresh_entry_positive
        and fresh_exit_positive
        and cost_conversion_ok
        and entry_drift_ok
        and fresh_roundtrip_net is not None
        and fresh_roundtrip_net > 0
    )
    fresh_thesis_positive = (
        fresh_entry_positive
        and fresh_exit_positive
        and cost_conversion_ok
        and fresh_roundtrip_net is not None
        and fresh_roundtrip_net > 0
    )
    if input_amount <= 0:
        fresh_quote_failure_reason = "missing_input_amount"
    elif not entry_quote.get("ok"):
        fresh_quote_failure_reason = "entry_quote_failed"
    elif not exit_quote.get("ok"):
        fresh_quote_failure_reason = "exit_quote_failed"
    elif not cost_conversion_ok:
        fresh_quote_failure_reason = "route_cost_conversion_failed"
    elif fresh_roundtrip_net is None or fresh_roundtrip_net <= 0:
        fresh_quote_failure_reason = "roundtrip_negative_after_route_cost"
    elif mode == "strict_identity_replay" and not entry_drift_ok:
        fresh_quote_failure_reason = "entry_identity_drift_exceeds_limit"
    elif mode == "fresh_thesis_revalidation" and not entry_drift_ok:
        fresh_quote_failure_reason = "entry_identity_drift_informational"
    else:
        fresh_quote_failure_reason = "none"

    hold_horizon_value = text(row.get("best_later_horizon") or row.get("best_horizon") or "unknown")
    entry_gate_ready = bool(text(row.get("best_later_horizon") or row.get("best_horizon")))
    confirmation_condition_ready = (
        runtime_pressure_signal_present if thesis_signal_source == "fresh_runtime_recomputed"
        else fresh_pressure_score is not None
    )

    item = {
        **row,
        "evaluation_mode": mode,
        "thesis_signal_source": thesis_signal_source,
        "thesis_signal_live_grade": thesis_signal_source == "fresh_runtime_recomputed",
        "replay_rank_input_order": index + 1,
        "replay_time_unix_ms": replay_unix_ms,
        "candidate_id": cid,
        "original_impulse_id": original_impulse_id(row),
        "pool": text(row.get("pool")),
        "venue": text(row.get("venue")),
        "mint_pair": first_text(row, "mint_pair", "pair") or f"{mint_in}->{mint_out}",
        "mint_in": mint_in,
        "mint_out": mint_out,
        "original_direction": original_direction,
        "fresh_direction": fresh_direction,
        "owner_mode": "continuation",
        "entry_trigger": "same_direction_followthrough_after_impulse",
        "entry_trigger_ready": True,
        "confirmation_condition": "continuation_pressure_measured_and_positive",
        "confirmation_condition_ready": confirmation_condition_ready,
        "entry_delay_or_event_gate": hold_horizon_value or "next_same_pool_event",
        "entry_delay_or_event_gate_ready": entry_gate_ready,
        "hold_horizon": hold_horizon_value,
        "hold_horizon_defined": entry_gate_ready,
        "exit_trigger": "exit_on_continuation_horizon_or_stop_loss",
        "exit_trigger_ready": True,
        "exit_latency_budget_ms": first_int(row, "exit_latency_budget_ms", "exit_latency_ms") or as_int(exit_quote.get("latency_ms"), 0),
        "stop_loss": f"max_adverse_excursion_bps<={row.get('max_adverse_limit_bps', row.get('max_adverse_excursion_limit_bps', 50))}",
        "stop_loss_defined": bool(as_float(row.get("max_adverse_excursion"), None) is not None),
        "max_adverse_excursion": as_float(row.get("max_adverse_excursion")),
        "max_adverse_excursion_bounded": boolish(row.get("max_adverse_excursion_bounded")),
        "input_amount_raw": input_amount,
        "input_amount_source": (
            "candidate_trade_size_lamports" if first_int(row, "candidate_trade_size_lamports") else
            "trade_size_lamports" if first_int(row, "trade_size_lamports") else
            "pm_owner_lock_notional_raw" if first_int(row, "pm_owner_lock_notional_raw") else
            "pm_reserved_notional_raw" if first_int(row, "pm_reserved_notional_raw") else
            "entry_amount_raw" if first_int(row, "entry_amount_raw") else
            "amount_raw" if first_int(row, "amount_raw") else
            "missing"
        ),
        "entry_latency_ms": as_float(entry_quote.get("latency_ms")),
        "fresh_entry_quote": entry_quote,
        "fresh_entry_quote_ok": bool(entry_quote.get("ok")),
        "fresh_entry_positive": fresh_entry_positive,
        "entry_expected_out_raw": as_int(entry_quote.get("outAmount"), 0),
        "entry_min_out_raw": as_int(entry_quote.get("otherAmountThreshold"), 0),
        "entry_min_out_positive": as_int(entry_quote.get("otherAmountThreshold"), 0) > 0,
        "artifact_expected_out_raw": artifact_expected_out,
        "artifact_min_out_raw": artifact_min_out,
        "fresh_entry_out_vs_artifact_expected_bps": entry_out_drift,
        "fresh_entry_min_vs_artifact_min_bps": entry_min_drift,
        "fresh_entry_drift_within_limit": entry_drift_ok,
        "entry_identity_drift_exceeds_limit": not entry_drift_ok,
        "entry_identity_state": "ENTRY_IDENTITY_PRESERVED" if entry_drift_ok else "ENTRY_IDENTITY_DRIFT",
        "fresh_quote_failure_reason": fresh_quote_failure_reason,
        "fresh_exit_quote": exit_quote,
        "fresh_exit_quote_ok": bool(exit_quote.get("ok")),
        "fresh_exit_positive": fresh_exit_positive,
        "exit_input_raw": exit_input_amount,
        "exit_expected_out_raw": fresh_exit_out_raw,
        "exit_min_out_raw": as_int(exit_quote.get("otherAmountThreshold"), 0),
        "exit_min_out_positive": as_int(exit_quote.get("otherAmountThreshold"), 0) > 0,
        "exit_state": "EXIT_READY" if fresh_exit_positive else "EXIT_NOT_READY",
        "route_cost_lamports": cost_lamports,
        "route_cost_input_raw": cost_input_raw,
        "route_cost_conversion_quote": cost_quote,
        "route_cost_conversion_ok": cost_conversion_ok,
        "fresh_roundtrip_net_after_all_costs_raw": fresh_roundtrip_net,
        "fresh_thesis_net_after_all_costs": fresh_roundtrip_net,
        "fresh_thesis_net_after_all_costs_positive": fresh_thesis_positive,
        "strict_identity_replay_ready": strict_identity_ready,
        "net_ev_after_all_costs_positive": strict_identity_ready,
        "original_continuation_pressure_score": original_pressure_score,
        "fresh_same_direction_followthrough_score": runtime_followthrough_score,
        "fresh_continuation_pressure_score": fresh_pressure_score,
        "fresh_absorption_score": runtime_absorption_score,
        "fresh_opposite_direction_toxicity": runtime_toxicity_score,
        "fresh_route_lag_conflict": route_lag_conflict,
        "fresh_liquidity_migration_signal": liquidity_migration_signal,
        "fresh_runtime_pressure_still_valid": runtime_pressure_still_valid,
        "runtime_pressure_signal_present": runtime_pressure_signal_present,
        "continuation_pressure_score_now": fresh_pressure_score,
        "same_direction_followthrough_score_now": followthrough_score_now,
        "same_direction_followthrough_positive": same_direction_followthrough_positive,
        "best_later_net_positive": best_later_net_positive,
        "actual_continuation_positive": actual_continuation_positive,
        "same_direction_still_valid": same_direction_still_valid,
        "pressure_still_valid": pressure_still_valid,
        "thesis_state": "THESIS_STILL_VALID" if pressure_still_valid else "THESIS_DECAYED",
        "pm_owner_lock_ready": boolish(row.get("pm_owner_lock_ready")),
        "pm_reserve_ok": boolish(row.get("pm_owner_lock_ready")),
        "pm_release_on_no_send_ok": True,
        "pm_pending_after": 0,
        "account_ready_or_inline_ata_ready": boolish(row.get("account_or_inline_ata_ready")),
        "template_ready": boolish(row.get("template_ready")),
        "helius_sender_no_send_accept": boolish(row.get("helius_sender_no_send_ready")) and not boolish(row.get("send_transaction_called")),
        "bundle_no_submit_pass": boolish(row.get("bundle_no_submit_ready")) and not boolish(row.get("sendBundle_called")),
        "egress_no_send_ready": (boolish(row.get("helius_sender_no_send_ready")) and not boolish(row.get("send_transaction_called"))) or (boolish(row.get("bundle_no_submit_ready")) and not boolish(row.get("sendBundle_called"))),
        "send_transaction_called": False,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
        "fresh_quote_api_url": quote_api_url,
        "entry_slippage_bps": entry_slippage_bps,
        "exit_slippage_bps": exit_slippage_bps,
    }
    item["fresh_thesis_ready"] = (
        pressure_still_valid
        and item["fresh_entry_positive"]
        and item["fresh_exit_positive"]
        and item["fresh_thesis_net_after_all_costs_positive"]
        and item["max_adverse_excursion_bounded"]
        and item["pm_owner_lock_ready"]
        and item["account_ready_or_inline_ata_ready"]
        and item["template_ready"]
        and item["egress_no_send_ready"]
    )
    item["first_replay_blocker"] = first_replay_blocker(item, mode)
    item["first_fresh_thesis_blocker"] = first_replay_blocker(item, "fresh_thesis_revalidation")
    item["selected_private_replay_ready"] = (
        item["fresh_thesis_ready"] and item["first_replay_blocker"] == "none"
        if mode == "fresh_thesis_revalidation"
        else item["strict_identity_replay_ready"] and item["first_replay_blocker"] == "none"
    )
    item["decision"] = (
        "CONTINUATION_FRESH_THESIS_READY_NO_SEND"
        if item["fresh_thesis_ready"] and mode == "fresh_thesis_revalidation" and thesis_signal_source == "fresh_runtime_recomputed"
        else "CONTINUATION_FRESH_THESIS_READY_NO_SEND_CARRIED_SIGNAL"
        if item["fresh_thesis_ready"] and mode == "fresh_thesis_revalidation"
        else "CONTINUATION_SELECTED_REPLAY_READY_NO_SEND"
        if item["strict_identity_replay_ready"] and mode == "strict_identity_replay" and item["first_replay_blocker"] == "none"
        else item["first_replay_blocker"]
    )
    replay_rows.append(item)

replay_rows.sort(key=lambda row: (int(row["selected_private_replay_ready"]),) + route_rank(row), reverse=True)
selected_ready = next((row for row in replay_rows if row["selected_private_replay_ready"]), None)
preferred = replay_rows[0] if replay_rows else None
selected = selected_ready or preferred

if selected_ready:
    if mode == "fresh_thesis_revalidation":
        final_status = (
            "CONTINUATION_FRESH_THESIS_READY_NO_SEND"
            if thesis_signal_source == "fresh_runtime_recomputed"
            else "CONTINUATION_FRESH_THESIS_READY_NO_SEND_CARRIED_SIGNAL"
        )
    else:
        final_status = "CONTINUATION_SELECTED_REPLAY_READY_NO_SEND"
else:
    blocker = text((selected or {}).get("first_replay_blocker")) or ("thesis_decayed" if mode == "fresh_thesis_revalidation" else "fresh_quote_drifted")
    if mode == "fresh_thesis_revalidation":
        final_status = {
            "fresh_runtime_signal_incomplete": "PARK_THESIS_DECAYED",
            "thesis_decayed": "PARK_THESIS_DECAYED",
            "entry_quote_negative": "PARK_ENTRY_QUOTE_NEGATIVE",
            "fresh_net_negative_after_cost": "PARK_FRESH_NET_NEGATIVE_AFTER_COST",
            "exit_not_ready": "PARK_EXIT_NOT_READY",
            "adverse_excursion_too_high": "PARK_ADVERSE_EXCURSION",
            "PM_owner_lock_failed": "PARK_PM",
            "account_or_template_drift": "PARK_ACCOUNT_OR_TEMPLATE",
            "exit_rule_failed": "PARK_EXIT_NOT_READY",
            "egress_no_send_failed": "PARK_EXIT_NOT_READY",
        }.get(blocker, "PARK_ENTRY_QUOTE_NEGATIVE")
    else:
        final_status = {
            "fresh_quote_drifted": "PARK_SELECTED_REPLAY_FRESH_QUOTE_DRIFTED",
            "markout_not_persistent": "PARK_SELECTED_REPLAY_MARKOUT_NOT_PERSISTENT",
            "adverse_excursion_too_high": "PARK_SELECTED_REPLAY_ADVERSE_EXCURSION",
            "PM_owner_lock_failed": "PARK_SELECTED_REPLAY_PM",
            "exit_rule_failed": "PARK_SELECTED_REPLAY_EXIT_RULE",
            "egress_no_send_failed": "PARK_SELECTED_REPLAY_EGRESS",
            "account_or_template_drift": "PARK_SELECTED_REPLAY_ACCOUNT_TEMPLATE",
        }.get(blocker, "PARK_SELECTED_REPLAY_FRESH_QUOTE_DRIFTED")

decision_counts = Counter(row["first_replay_blocker"] for row in replay_rows)
proposal_grade_fresh_ready_rows = sum(1 for row in replay_rows if row["fresh_thesis_ready"] and row["thesis_signal_source"] == "fresh_runtime_recomputed")
proposal_grade_blocker = (
    "fresh_runtime_pressure_recompute_missing"
    if mode == "fresh_thesis_revalidation" and thesis_signal_source != "fresh_runtime_recomputed"
    else "none"
    if mode != "fresh_thesis_revalidation" or proposal_grade_fresh_ready_rows > 0
    else text((selected or preferred or {}).get("first_fresh_thesis_blocker")) or "thesis_decayed"
)
summary = {
    "status": final_status,
    "evaluation_mode": mode,
    "thesis_signal_source": thesis_signal_source,
    "thesis_signal_live_grade": thesis_signal_source == "fresh_runtime_recomputed",
    "source_rows": str(source_path or ""),
    "enrich_rows": [str(path) for path in enrich_paths],
    "rows_seen": len(replay_rows),
    "selected_private_replay_ready_rows": sum(1 for row in replay_rows if row["selected_private_replay_ready"]),
    "strict_identity_replay_ready_rows": sum(1 for row in replay_rows if row["strict_identity_replay_ready"]),
    "fresh_thesis_ready_rows": sum(1 for row in replay_rows if row["fresh_thesis_ready"]),
    "proposal_grade_fresh_thesis_ready_rows": proposal_grade_fresh_ready_rows,
    "entry_identity_drift_rows": sum(1 for row in replay_rows if row["entry_identity_state"] == "ENTRY_IDENTITY_DRIFT"),
    "thesis_still_valid_rows": sum(1 for row in replay_rows if row["thesis_state"] == "THESIS_STILL_VALID"),
    "thesis_decayed_rows": sum(1 for row in replay_rows if row["thesis_state"] == "THESIS_DECAYED"),
    "exit_ready_rows": sum(1 for row in replay_rows if row["exit_state"] == "EXIT_READY"),
    "exit_not_ready_rows": sum(1 for row in replay_rows if row["exit_state"] == "EXIT_NOT_READY"),
    "preferred_candidate_id": text((preferred or {}).get("candidate_id")),
    "selected_candidate_id": text((selected or {}).get("candidate_id")),
    "preferred_first_replay_blocker": text((preferred or {}).get("first_replay_blocker")),
    "proposal_grade_blocker": proposal_grade_blocker,
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
choice = {
    "final_verdict": final_status,
    "evaluation_mode": mode,
    "thesis_signal_source": thesis_signal_source,
    "selected_candidate": selected,
    "preferred_candidate": preferred,
    "selection_priority": [
        "existing account ready before inline ATA",
        "Helius Sender no-send before bundle no-submit",
        "lower max adverse excursion",
        "clearer/lower hold horizon",
        "higher net EV after all costs",
        "no Token-2022 exception",
        "no acquisition or new ATA",
    ],
    "live_approval": False,
}
verdict = {
    "final_verdict": final_status,
    "evaluation_mode": mode,
    "thesis_signal_source": thesis_signal_source,
    "thesis_signal_live_grade": thesis_signal_source == "fresh_runtime_recomputed",
    "selected_candidate_id": text((selected or {}).get("candidate_id")),
    "selected_private_replay_ready_rows": summary["selected_private_replay_ready_rows"],
    "strict_identity_replay_ready_rows": summary["strict_identity_replay_ready_rows"],
    "fresh_thesis_ready_rows": summary["fresh_thesis_ready_rows"],
    "proposal_grade_fresh_thesis_ready_rows": summary["proposal_grade_fresh_thesis_ready_rows"],
    "proposal_grade_blocker": summary["proposal_grade_blocker"],
    "first_replay_blocker": text((selected or preferred or {}).get("first_replay_blocker")),
    "first_fresh_thesis_blocker": text((selected or preferred or {}).get("first_fresh_thesis_blocker")),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "evaluation_mode": mode,
    "thesis_signal_source": thesis_signal_source,
    "source_rows": str(source_path or ""),
    "enrich_rows": [str(path) for path in enrich_paths],
    "result_dir": str(result_dir),
    "max_rows": max_rows,
    "quote_api_url": quote_api_url,
    "entry_slippage_bps": entry_slippage_bps,
    "exit_slippage_bps": exit_slippage_bps,
    "max_entry_drift_bps": max_entry_drift_bps,
    "live_send": False,
    "sendBundle": False,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in replay_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
choice_path.write_text(json.dumps(choice, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTP continuation selected private replay written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

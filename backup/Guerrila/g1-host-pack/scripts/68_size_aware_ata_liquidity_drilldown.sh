#!/usr/bin/env bash
# 68_size_aware_ata_liquidity_drilldown.sh - size-aware ATA liquidity drilldown from Epoch1/Run66/Run67 artifacts

source "$(dirname "$0")/lib.sh"
script_header "68 - Size-Aware ATA Liquidity Drilldown"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN68_STEP:-68_size_aware_ata_liquidity_drilldown}"
STATUS_PREFIX="${SHADOW_RUN68_STATUS_PREFIX:-run68_size_aware_ata_liquidity_drilldown}"
REPORT_BASENAME="${SHADOW_RUN68_REPORT_BASENAME:-run68_size_aware_ata_liquidity_drilldown}"

RUN66_STEP="${SHADOW_RUN68_RUN66_STEP:-66_epoch1_monetization_forensic_audit}"
RUN67_STEP="${SHADOW_RUN68_RUN67_STEP:-67_recurring_ata_precreation_planner}"

RUN66_RESULT_DIR="$RESULTS_ROOT/$RUN66_STEP"
RUN67_RESULT_DIR="$RESULTS_ROOT/$RUN67_STEP"

RUN66_WOULD_FIRE_JSONL="$RUN66_RESULT_DIR/epoch1_would_fire_rows.jsonl"
RUN66_FORENSIC_SUMMARY_JSON="$RUN66_RESULT_DIR/epoch1_would_fire_forensic_summary.json"
RUN67_POLICY_AUDIT_JSON="$RUN67_RESULT_DIR/run67_mint_policy_audit.json"
RUN67_REPORT_JSON="$RUN67_RESULT_DIR/run67_recurring_ata_precreation_planner.json"

RPC_URL="${SHADOW_RUN68_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN68_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

MIN_RECURRENCE_COUNT="${SHADOW_RUN68_MIN_RECURRENCE_COUNT:-2}"
MIN_EXPECTED_UNLOCK_USD="${SHADOW_RUN68_MIN_EXPECTED_UNLOCK_USD:-1.0}"
MIN_MEANINGFUL_TRADE_USDC="${SHADOW_RUN68_MIN_MEANINGFUL_TRADE_USDC:-0.10}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN68_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN68_MIN_EXIT_VALUE_RATIO:-0.50}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN68_ENTRY_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN68_EXIT_SLIPPAGE_BPS:-0}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
SELECTED_JSON="$RESULT_DIR/run68_selected_ata_candidates.json"
BUCKETS_JSON="$RESULT_DIR/run68_bucketed_candidates.json"
DETAIL_JSON="$RESULT_DIR/run68_mint_liquidity_drilldown.json"
TASKS_FILE="$RESULT_DIR/run68_operator_tasks.txt"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$SELECTED_JSON" \
    "$BUCKETS_JSON" \
    "$DETAIL_JSON" \
    "$TASKS_FILE"

missing=0
for required_path in \
    "$RUN66_WOULD_FIRE_JSONL" \
    "$RUN66_FORENSIC_SUMMARY_JSON" \
    "$RUN67_POLICY_AUDIT_JSON" \
    "$RUN67_REPORT_JSON"
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
    "$RUN66_WOULD_FIRE_JSONL" \
    "$RUN66_FORENSIC_SUMMARY_JSON" \
    "$RUN67_POLICY_AUDIT_JSON" \
    "$RUN67_REPORT_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$SELECTED_JSON" \
    "$BUCKETS_JSON" \
    "$DETAIL_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_RECURRENCE_COUNT" \
    "$MIN_EXPECTED_UNLOCK_USD" \
    "$MIN_MEANINGFUL_TRADE_USDC" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$ENTRY_SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" \
    "$PACK_DIR" <<'PY'
import json
import math
import ssl
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
OWNER_PRIORITY = {
    "directarb": 0,
    "backrun": 1,
    "continuation": 2,
    "rebalancing": 3,
    "housekeeping": 4,
    "flatten": 5,
    "drop": 6,
}
BUCKET_PRIORITY = {
    "A_immediately_safe": 0,
    "B_metadata_only_maybe_safe": 1,
    "C_liquid_but_low_recurrence": 2,
    "D_recurrent_but_illiquid": 3,
    "E_toxic_or_denied": 4,
}
BUCKET_CAP_USD = {
    "lte_5_usdc_or_0_005_sol_equiv": 5.0,
    "lte_10_usdc_or_0_01_sol_equiv": 10.0,
    "lte_25_usdc_or_0_025_sol_equiv": 25.0,
    "lte_50_usdc_or_0_05_sol_equiv": 50.0,
    "lte_100_usdc_or_0_1_sol_equiv": 100.0,
    "lte_250_usdc_or_0_25_sol_equiv": 250.0,
    "lte_500_usdc_or_0_5_sol_equiv": 500.0,
    "lte_1000_usdc_or_1_0_sol_equiv": 1000.0,
    "lte_2000_usdc_or_2_0_sol_equiv": 2000.0,
    "gt_2000_usdc_or_2_0_sol_equiv": 2500.0,
}

would_fire_rows_path = Path(sys.argv[1])
forensic_summary_path = Path(sys.argv[2])
policy_audit_path = Path(sys.argv[3])
run67_report_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
selected_json = Path(sys.argv[7])
buckets_json = Path(sys.argv[8])
detail_json = Path(sys.argv[9])
tasks_file = Path(sys.argv[10])
step = sys.argv[11]
status_prefix = sys.argv[12]
rpc_url = sys.argv[13]
quote_api_url = sys.argv[14]
min_recurrence_count = int(sys.argv[15] or "2")
min_expected_unlock_usd = float(sys.argv[16] or "1.0")
min_meaningful_trade_usdc = float(sys.argv[17] or "0.10")
min_exit_value_usdc = float(sys.argv[18] or "0.10")
min_exit_value_ratio = float(sys.argv[19] or "0.50")
entry_slippage_bps = int(sys.argv[20] or "100")
exit_slippage_bps = int(sys.argv[21] or "0")
pack_dir = Path(sys.argv[22])


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text_value(value):
    return str(value or "").strip()


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = (len(ordered) - 1) * fraction
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return ordered[lower]
    weight = index - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def placeholder_url(url):
    return not url or "CHANGE_ME" in url or "${" in url


def http_json(url):
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def quote_exact_in(input_mint, output_mint, amount_raw, slippage_bps):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, int(amount_raw))),
        "slippageBps": str(slippage_bps),
    }
    request_url = quote_api_url + "?" + urllib.parse.urlencode(params)
    attempts = 3
    delay_seconds = 1.0
    for attempt in range(1, attempts + 1):
        try:
            return http_json(request_url), None
        except urllib.error.HTTPError as exc:  # noqa: PERF203
            body = exc.read().decode("utf-8", errors="replace")
            try:
                error_payload = json.loads(body)
            except Exception:  # noqa: BLE001
                error_payload = {"error": body, "errorCode": f"http_{exc.code}"}
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            error_payload["attempts"] = attempt
            return None, error_payload
        except Exception as exc:  # noqa: BLE001
            if attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            return None, {"error": str(exc), "errorCode": "request_failed", "attempts": attempt}
    return None, {"error": "quote_retry_exhausted", "errorCode": "quote_retry_exhausted", "attempts": attempts}


def route_labels(quote):
    labels = []
    for item in quote.get("routePlan") or []:
        if not isinstance(item, dict):
            continue
        swap_info = item.get("swapInfo") or {}
        label = text_value(swap_info.get("label"))
        if label:
            labels.append(label)
    return labels


def source_usdc_for_raw(mint_in, raw_amount, row_context):
    raw_amount = max(1, int(raw_amount))
    if mint_in == USDC_MINT:
        return raw_amount / 1_000_000.0
    if mint_in == USDT_MINT:
        return raw_amount / 1_000_000.0
    if mint_in == WSOL_MINT:
        return (raw_amount / 1_000_000_000.0) * sol_usd_equivalent
    usd_per_raw = row_context.get("source_usd_per_raw")
    if usd_per_raw:
        return raw_amount * usd_per_raw
    return None


def source_native_for_raw(mint_in, raw_amount, row_context):
    raw_amount = max(1, int(raw_amount))
    decimals = row_context.get("input_decimals")
    if isinstance(decimals, int) and 0 <= decimals <= 18:
        return raw_amount / (10 ** decimals)
    return None


def target_raw_for_usd(row_context, target_usd):
    mint_in = row_context["mint_in"]
    if mint_in in {USDC_MINT, USDT_MINT}:
        return max(1, math.ceil(target_usd * 1_000_000))
    if mint_in == WSOL_MINT and sol_usd_equivalent > 0:
        return max(1, math.ceil((target_usd / sol_usd_equivalent) * 1_000_000_000))
    usd_per_raw = row_context.get("source_usd_per_raw")
    if usd_per_raw and usd_per_raw > 0:
        return max(1, math.ceil(target_usd / usd_per_raw))
    return None


def raw_from_row(row):
    native_size = safe_float(row.get("input_native_size"))
    decimals = row.get("input_decimals")
    if native_size <= 0 or not isinstance(decimals, int):
        return None
    return max(1, int(round(native_size * (10 ** decimals))))


def bool_pass(value):
    return bool(value)


def evaluate_roundtrip(row_context, source_input_raw, probe_name):
    result = {
        "probe_name": probe_name,
        "source_mint": row_context["mint_in"],
        "target_mint": row_context["mint_out"],
        "input_raw": int(source_input_raw) if source_input_raw is not None else None,
        "input_native": None,
        "input_usdc": None,
        "entry_quote_ok": False,
        "entry_quote_error": None,
        "entry_route_labels": [],
        "candidate_output_raw": None,
        "candidate_output_native": None,
        "exit_quote_ok": False,
        "exit_quote_error": None,
        "exit_route_labels": [],
        "exit_out_raw": None,
        "exit_value_usdc": None,
        "exit_value_ratio": None,
        "exit_loss_pct": None,
        "exit_value_non_dust": False,
        "exit_ratio_ok": False,
        "liquidity_pass": False,
        "verdict": "probe_not_run",
    }
    if source_input_raw is None or source_input_raw <= 0:
        result["verdict"] = "input_size_unavailable"
        return result
    result["input_native"] = round_or_none(source_native_for_raw(row_context["mint_in"], source_input_raw, row_context), 9)
    input_usdc = source_usdc_for_raw(row_context["mint_in"], source_input_raw, row_context)
    result["input_usdc"] = round_or_none(input_usdc, 9)
    if placeholder_url(quote_api_url):
        result["verdict"] = "quote_api_unconfigured"
        return result

    entry_quote, entry_error = quote_exact_in(row_context["mint_in"], row_context["mint_out"], source_input_raw, entry_slippage_bps)
    if entry_quote is None:
        result["entry_quote_error"] = entry_error
        result["verdict"] = "entry_quote_missing"
        return result
    out_raw = safe_int(entry_quote.get("outAmount"))
    target_decimals = row_context.get("output_decimals")
    out_native = (out_raw / (10 ** target_decimals)) if isinstance(target_decimals, int) and 0 <= target_decimals <= 18 else None
    result["entry_quote_ok"] = True
    result["entry_route_labels"] = route_labels(entry_quote)
    result["candidate_output_raw"] = out_raw
    result["candidate_output_native"] = round_or_none(out_native, 9)

    exit_quote, exit_error = quote_exact_in(row_context["mint_out"], row_context["mint_in"], out_raw, exit_slippage_bps)
    if exit_quote is None:
        result["exit_quote_error"] = exit_error
        result["verdict"] = "exit_quote_missing"
        return result
    exit_out_raw = safe_int(exit_quote.get("outAmount"))
    exit_usdc = source_usdc_for_raw(row_context["mint_in"], exit_out_raw, row_context)
    exit_ratio = (exit_usdc / input_usdc) if input_usdc and exit_usdc is not None else None
    exit_loss_pct = (1.0 - exit_ratio) * 100.0 if exit_ratio is not None else None
    exit_non_dust = (exit_usdc or 0.0) >= min_exit_value_usdc and exit_out_raw > 1
    exit_ratio_ok = exit_ratio is not None and exit_ratio >= min_exit_value_ratio

    result["exit_quote_ok"] = True
    result["exit_route_labels"] = route_labels(exit_quote)
    result["exit_out_raw"] = exit_out_raw
    result["exit_value_usdc"] = round_or_none(exit_usdc, 9)
    result["exit_value_ratio"] = round_or_none(exit_ratio, 9)
    result["exit_loss_pct"] = round_or_none(exit_loss_pct, 6)
    result["exit_value_non_dust"] = exit_non_dust
    result["exit_ratio_ok"] = exit_ratio_ok
    result["liquidity_pass"] = exit_non_dust and exit_ratio_ok
    if result["liquidity_pass"]:
        result["verdict"] = "liquid_roundtrip_ok"
    elif not exit_non_dust:
        result["verdict"] = "roundtrip_dust"
    else:
        result["verdict"] = "roundtrip_ratio_below_threshold"
    return result


def evaluate_unit_exit_probe(mint_out, policy, output_decimals):
    probe_raw = 10 ** output_decimals if isinstance(output_decimals, int) and 0 <= output_decimals <= 18 else 1
    result = {
        "probe_name": "unit_exit_probe",
        "probe_input_amount_raw": probe_raw,
        "probe_input_native": round_or_none(probe_raw / (10 ** output_decimals), 9) if isinstance(output_decimals, int) and 0 <= output_decimals <= 18 else None,
        "probe_target_mint": None,
        "probe_quote_ok": False,
        "probe_exit_status": "not_checked",
        "probe_exit_value_usdc": None,
        "probe_exit_value_sol": None,
        "probe_route_labels": [],
        "probe_error": None,
    }
    if not policy.get("policy_ok"):
        result["probe_exit_status"] = "not_checked_policy_blocked"
        return result
    if placeholder_url(quote_api_url):
        result["probe_exit_status"] = "quote_api_unconfigured"
        return result
    if mint_out == USDC_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_output"
        result["probe_exit_value_usdc"] = round_or_none(probe_raw / 1_000_000.0, 6)
        result["probe_exit_value_sol"] = round_or_none((probe_raw / 1_000_000.0) / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        return result
    if mint_out == USDT_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_like_output"
        result["probe_exit_value_usdc"] = round_or_none(probe_raw / 1_000_000.0, 6)
        result["probe_exit_value_sol"] = round_or_none((probe_raw / 1_000_000.0) / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        return result
    if mint_out == WSOL_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_like_output"
        result["probe_exit_value_sol"] = round_or_none(probe_raw / 1_000_000_000.0, 9)
        result["probe_exit_value_usdc"] = round_or_none((probe_raw / 1_000_000_000.0) * sol_usd_equivalent, 6)
        return result

    quote, error = quote_exact_in(mint_out, USDC_MINT, probe_raw, exit_slippage_bps)
    if quote is not None:
        out_raw = safe_int(quote.get("outAmount"))
        exit_usdc = out_raw / 1_000_000.0
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_value_usdc"] = round_or_none(exit_usdc, 6)
        result["probe_exit_value_sol"] = round_or_none(exit_usdc / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        result["probe_route_labels"] = route_labels(quote)
        result["probe_exit_status"] = "unit_probe_exit_quote_ok" if exit_usdc >= min_exit_value_usdc and out_raw > 1 else "unit_probe_exit_quote_dust"
        return result

    fallback_quote, fallback_error = quote_exact_in(mint_out, WSOL_MINT, probe_raw, exit_slippage_bps)
    if fallback_quote is not None:
        out_raw = safe_int(fallback_quote.get("outAmount"))
        exit_sol = out_raw / 1_000_000_000.0
        exit_usdc = exit_sol * sol_usd_equivalent
        result["probe_target_mint"] = WSOL_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_value_usdc"] = round_or_none(exit_usdc, 6)
        result["probe_exit_value_sol"] = round_or_none(exit_sol, 9)
        result["probe_route_labels"] = route_labels(fallback_quote)
        result["probe_exit_status"] = "unit_probe_exit_quote_ok" if exit_usdc >= min_exit_value_usdc and out_raw > 1 else "unit_probe_exit_quote_dust"
        return result

    result["probe_target_mint"] = USDC_MINT
    result["probe_exit_status"] = "unit_probe_exit_quote_missing"
    result["probe_error"] = fallback_error or error
    return result


def recurrence_score(unique_event_count, row_count):
    return min(100, unique_event_count * 30 + min(row_count, 5) * 8)


def liquidity_score(unit_probe, min_viable_probe, bucket_probe, candidate_probe):
    score = 0
    if text_value(unit_probe.get("probe_exit_status")) in {"stable_output", "stable_like_output", "unit_probe_exit_quote_ok"}:
        score += 15
    if min_viable_probe.get("liquidity_pass"):
        score += 25
    if bucket_probe.get("liquidity_pass"):
        score += 25
    if candidate_probe.get("liquidity_pass"):
        score += 35
    return min(score, 100)


def bucket_cap_usd(label):
    return BUCKET_CAP_USD.get(label)


def candidate_sort_key(item):
    return (
        BUCKET_PRIORITY.get(item.get("classification_bucket"), 99),
        0 if item.get("precreate_allowed") else 1,
        OWNER_PRIORITY.get((item.get("owner_modes_affected") or ["zzz"])[0], 99),
        -safe_float(item.get("expected_unlock_ev_usd")),
        -safe_int(item.get("unique_event_count")),
        item.get("mint_out") or "",
    )


would_fire_rows = load_jsonl(would_fire_rows_path)
forensic_summary = load_json(forensic_summary_path)
policy_audit_rows = load_json(policy_audit_path)
run67_report = load_json(run67_report_path)

sol_usd_equivalent = safe_float((forensic_summary.get("unit_audit") or {}).get("sol_usd_equivalent"), 1000.0) or 1000.0
policy_lookup = {text_value(item.get("mint")): item for item in policy_audit_rows if text_value(item.get("mint"))}

ata_fixup_rows = [
    row
    for row in would_fire_rows
    if text_value(row.get("fixup_type")) in {"ata", "ata_template"}
    and text_value(row.get("usable_route_for_us")) in {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
]

grouped = defaultdict(list)
for row in ata_fixup_rows:
    mint_out = text_value(row.get("mint_out"))
    if mint_out:
        grouped[mint_out].append(row)

drilldown_items = []
for mint_out, rows in grouped.items():
    rows_sorted = sorted(rows, key=lambda row: (-safe_float(row.get("net_after_fee_usd")), -safe_float(row.get("input_usd_equiv")), safe_int(row.get("slot"))))
    representative_row = rows_sorted[0]
    row_count = len(rows)
    unique_events = {
        (
            safe_int(row.get("slot")),
            text_value(row.get("signature")),
            text_value(row.get("pool")),
            text_value(row.get("mint_in")),
            text_value(row.get("mint_out")),
            text_value(row.get("owner_mode")),
            text_value(row.get("usable_route_for_us")),
            text_value(row.get("notional_bucket")),
        )
        for row in rows
    }
    unique_event_count = len(unique_events)
    expected_unlock_ev_usd = sum(safe_float(row.get("net_after_fee_usd")) for row in rows)
    strict_ev_usd = sum(safe_float(row.get("net_after_fee_usd")) for row in rows if row.get("strict_current_state"))
    strict_row_count = sum(1 for row in rows if row.get("strict_current_state"))
    owner_mode_counter = Counter(text_value(row.get("owner_mode")).lower() for row in rows if text_value(row.get("owner_mode")))
    bucket_counter = Counter(text_value(row.get("notional_bucket")) for row in rows if text_value(row.get("notional_bucket")))
    dominant_bucket = bucket_counter.most_common(1)[0][0] if bucket_counter else None
    dominant_bucket_rows = [row for row in rows if text_value(row.get("notional_bucket")) == dominant_bucket] or rows
    dominant_bucket_input_usd_values = [safe_float(row.get("input_usd_equiv")) for row in dominant_bucket_rows if safe_float(row.get("input_usd_equiv")) > 0]
    dominant_bucket_representative_usd = percentile(dominant_bucket_input_usd_values, 0.5) if dominant_bucket_input_usd_values else bucket_cap_usd(dominant_bucket)
    representative_raw = raw_from_row(representative_row)
    representative_input_usd = safe_float(representative_row.get("input_usd_equiv"))
    source_usd_per_raw = (representative_input_usd / representative_raw) if representative_raw and representative_input_usd > 0 else None
    representative_context = {
        "mint_in": text_value(representative_row.get("mint_in")),
        "mint_out": mint_out,
        "input_decimals": representative_row.get("input_decimals"),
        "output_decimals": representative_row.get("output_decimals"),
        "source_usd_per_raw": source_usd_per_raw,
    }
    min_viable_raw = target_raw_for_usd(representative_context, min_meaningful_trade_usdc)
    bucket_probe_raw = target_raw_for_usd(representative_context, dominant_bucket_representative_usd) if dominant_bucket_representative_usd else None
    policy = policy_lookup.get(mint_out) or {}

    unit_probe = evaluate_unit_exit_probe(mint_out, policy, representative_row.get("output_decimals"))
    candidate_probe = evaluate_roundtrip(representative_context, representative_raw, "candidate_sized_probe")
    min_viable_probe = evaluate_roundtrip(representative_context, min_viable_raw, "minimum_viable_probe")
    bucket_probe = evaluate_roundtrip(representative_context, bucket_probe_raw, "bucket_sized_probe")

    recurrence_pass = unique_event_count >= min_recurrence_count
    expected_unlock_pass = expected_unlock_ev_usd > min_expected_unlock_usd
    route_usable_pass = row_count > 0
    size_aware_exit_pass = any(
        probe.get("liquidity_pass")
        for probe in (candidate_probe, min_viable_probe, bucket_probe)
    )
    metadata_maybe_safe = (
        text_value(policy.get("policy_blocker")) == "extensions_not_understood"
        and bool(policy.get("metadata_only_token2022_eligible"))
        and not policy.get("freeze_authority")
        and not policy.get("mint_authority")
    )

    if policy.get("policy_ok") and size_aware_exit_pass and recurrence_pass and expected_unlock_pass and route_usable_pass:
        classification_bucket = "A_immediately_safe"
        precreate_allowed = True
    elif metadata_maybe_safe and size_aware_exit_pass:
        classification_bucket = "B_metadata_only_maybe_safe"
        precreate_allowed = False
    elif policy.get("policy_ok") and size_aware_exit_pass:
        classification_bucket = "C_liquid_but_low_recurrence"
        precreate_allowed = False
    elif (policy.get("policy_ok") or metadata_maybe_safe) and (recurrence_pass or expected_unlock_pass):
        classification_bucket = "D_recurrent_but_illiquid"
        precreate_allowed = False
    else:
        classification_bucket = "E_toxic_or_denied"
        precreate_allowed = False

    item = {
        "mint_out": mint_out,
        "token_program": policy.get("token_program"),
        "extensions": policy.get("extensions") or [],
        "freeze_authority": policy.get("freeze_authority"),
        "mint_authority": policy.get("mint_authority"),
        "owner_modes_affected": [mode for mode, _count in sorted(owner_mode_counter.items(), key=lambda kv: OWNER_PRIORITY.get(kv[0], 99))],
        "owner_mode_breakdown": [
            {
                "owner_mode": mode,
                "row_count": count,
                "expected_unlock_ev_usd": round_or_none(sum(safe_float(row.get("net_after_fee_usd")) for row in rows if text_value(row.get("owner_mode")).lower() == mode), 6),
            }
            for mode, count in sorted(owner_mode_counter.items(), key=lambda kv: OWNER_PRIORITY.get(kv[0], 99))
        ],
        "row_count": row_count,
        "unique_event_count": unique_event_count,
        "expected_unlock_ev_usd": round_or_none(expected_unlock_ev_usd, 6),
        "strict_current_row_count": strict_row_count,
        "strict_current_ev_usd": round_or_none(strict_ev_usd, 6),
        "fixup_delta_rows": row_count - strict_row_count,
        "fixup_delta_ev_usd": round_or_none(expected_unlock_ev_usd - strict_ev_usd, 6),
        "expected_notional_bucket": dominant_bucket,
        "notional_bucket_breakdown": [{"bucket": bucket, "count": count} for bucket, count in bucket_counter.most_common()],
        "unit_exit_probe": unit_probe,
        "candidate_sized_exit_quote": candidate_probe,
        "minimum_viable_exit_quote": min_viable_probe,
        "bucket_sized_exit_quote": bucket_probe,
        "candidate_sized_exit_loss_pct": candidate_probe.get("exit_loss_pct"),
        "minimum_viable_exit_loss_pct": min_viable_probe.get("exit_loss_pct"),
        "bucket_sized_exit_loss_pct": bucket_probe.get("exit_loss_pct"),
        "recurrence_score": recurrence_score(unique_event_count, row_count),
        "liquidity_score": liquidity_score(unit_probe, min_viable_probe, bucket_probe, candidate_probe),
        "policy_verdict": text_value(policy.get("policy_blocker")) or "none",
        "policy_pass": bool(policy.get("policy_ok")),
        "metadata_only_maybe_safe": metadata_maybe_safe,
        "recurrence_pass": recurrence_pass,
        "expected_unlock_pass": expected_unlock_pass,
        "route_usable_by_us": route_usable_pass,
        "size_aware_exit_pass": size_aware_exit_pass,
        "classification_bucket": classification_bucket,
        "precreate_allowed": precreate_allowed,
        "representative_row": {
            "owner_mode": text_value(representative_row.get("owner_mode")),
            "pool": text_value(representative_row.get("pool")) or None,
            "venue": text_value(representative_row.get("venue")) or None,
            "slot": safe_int(representative_row.get("slot")),
            "signature": text_value(representative_row.get("signature")) or None,
            "candidate_input_usd_equiv": round_or_none(representative_input_usd, 6),
            "candidate_input_native_size": round_or_none(safe_float(representative_row.get("input_native_size")), 9),
            "candidate_input_raw_derived": representative_raw,
        },
    }
    drilldown_items.append(item)

bucketed_candidates = defaultdict(list)
for item in sorted(drilldown_items, key=candidate_sort_key):
    bucketed_candidates[item["classification_bucket"]].append(item)

selected_candidates = bucketed_candidates["A_immediately_safe"]
payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run66_would_fire_rows_jsonl": str(would_fire_rows_path),
        "run66_forensic_summary_json": str(forensic_summary_path),
        "run67_mint_policy_audit_json": str(policy_audit_path),
        "run67_report_json": str(run67_report_path),
        "rpc_url": rpc_url or None,
        "quote_api_url": quote_api_url or None,
    },
    "filters": {
        "min_recurrence_count": min_recurrence_count,
        "min_expected_unlock_usd": min_expected_unlock_usd,
        "min_meaningful_trade_usdc": min_meaningful_trade_usdc,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "entry_slippage_bps": entry_slippage_bps,
        "exit_slippage_bps": exit_slippage_bps,
        "note": "Candidate-sized and bucket-sized exit quotes are reconstructed from current quotes because Epoch1 tape does not preserve output raw amounts.",
    },
    "summary": {
        "ata_fixup_row_count": len(ata_fixup_rows),
        "mint_candidate_count": len(drilldown_items),
        "selected_candidate_count": len(selected_candidates),
        "bucket_counts": {bucket: len(items) for bucket, items in sorted(bucketed_candidates.items(), key=lambda kv: BUCKET_PRIORITY.get(kv[0], 99))},
    },
    "selected_candidates": selected_candidates,
    "bucketed_candidates": {bucket: items for bucket, items in sorted(bucketed_candidates.items(), key=lambda kv: BUCKET_PRIORITY.get(kv[0], 99))},
    "mint_liquidity_drilldown": sorted(drilldown_items, key=candidate_sort_key),
}

summary_lines = [
    f"status={payload['status']}",
    f"ata_fixup_row_count={payload['summary']['ata_fixup_row_count']}",
    f"mint_candidate_count={payload['summary']['mint_candidate_count']}",
    f"selected_candidate_count={payload['summary']['selected_candidate_count']}",
]
for bucket, count in payload["summary"]["bucket_counts"].items():
    summary_lines.append(f"{bucket}_count={count}")

for index, item in enumerate(selected_candidates[:10], start=1):
    summary_lines.append(f"selected_{index}_mint_out={item.get('mint_out')}")
    summary_lines.append(f"selected_{index}_owner_modes={','.join(item.get('owner_modes_affected') or [])}")
    summary_lines.append(f"selected_{index}_expected_unlock_ev_usd={item.get('expected_unlock_ev_usd')}")
    summary_lines.append(f"selected_{index}_liquidity_score={item.get('liquidity_score')}")
    summary_lines.append(f"selected_{index}_recurrence_score={item.get('recurrence_score')}")

task_lines = [
    "# Run68 operator summary (generated, not auto-executed)",
    "",
    f"# selected_candidate_count={len(selected_candidates)}",
    f"# A_immediately_safe_count={len(bucketed_candidates['A_immediately_safe'])}",
    f"# B_metadata_only_maybe_safe_count={len(bucketed_candidates['B_metadata_only_maybe_safe'])}",
    f"# C_liquid_but_low_recurrence_count={len(bucketed_candidates['C_liquid_but_low_recurrence'])}",
    f"# D_recurrent_but_illiquid_count={len(bucketed_candidates['D_recurrent_but_illiquid'])}",
    f"# E_toxic_or_denied_count={len(bucketed_candidates['E_toxic_or_denied'])}",
    "",
    "# No ATA execution is recommended unless a mint lands in bucket A_immediately_safe.",
]
for item in selected_candidates:
    task_lines.append(
        "# candidate_A mint={} owner_modes={} expected_unlock_ev_usd={} candidate_exit_value_usdc={} candidate_exit_ratio={} bucket={}".format(
            item.get("mint_out"),
            ",".join(item.get("owner_modes_affected") or []),
            item.get("expected_unlock_ev_usd"),
            (item.get("candidate_sized_exit_quote") or {}).get("exit_value_usdc"),
            (item.get("candidate_sized_exit_quote") or {}).get("exit_value_ratio"),
            item.get("classification_bucket"),
        )
    )
task_lines += [
    "",
    "# If bucket A appears and is approved, precreate only those ATAs, refresh signer inventory, then rerun a fresh epoch and the Run66 forensic split.",
    f"#   {pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'}",
]

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
selected_json.write_text(json.dumps(selected_candidates, indent=2) + "\n", encoding="utf-8")
buckets_json.write_text(json.dumps(payload["bucketed_candidates"], indent=2) + "\n", encoding="utf-8")
detail_json.write_text(json.dumps(payload["mint_liquidity_drilldown"], indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
PY

pass "Run68 ATA liquidity drilldown written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

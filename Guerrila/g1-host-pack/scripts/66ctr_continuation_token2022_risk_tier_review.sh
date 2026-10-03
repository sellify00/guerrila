#!/usr/bin/env bash
# 66ctr_continuation_token2022_risk_tier_review.sh - bounded metadata-only Token-2022 Continuation risk-tier review

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "66CTR - Continuation Token-2022 Risk-Tier Review"

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
STEP="${SHADOW_RUN66CTR_STEP:-66ctr_continuation_token2022_risk_tier_review}"
SOURCE_ROWS="${SHADOW_RUN66CTR_SOURCE_ROWS:-}"
MAX_ROWS="${SHADOW_RUN66CTR_MAX_ROWS:-0}"
QUOTE_API_URL="${SHADOW_RUN66CTR_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
ENTRY_SLIPPAGE_BPS="${SHADOW_RUN66CTR_ENTRY_SLIPPAGE_BPS:-50}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN66CTR_EXIT_SLIPPAGE_BPS:-50}"
MIN_EXIT_WSOL_RAW="${SHADOW_RUN66CTR_MIN_EXIT_WSOL_RAW:-10000}"
MIN_EXIT_USDC_RAW="${SHADOW_RUN66CTR_MIN_EXIT_USDC_RAW:-1000}"

latest_named() {
    find "$RESULTS_ROOT" -type f -name run66ctt_continuation_token2022_policy_exception_review_rows.jsonl -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_named)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/run66ctr_continuation_token2022_risk_tier_review_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/run66ctr_continuation_token2022_risk_tier_review.json"
FINAL_VERDICT="$RESULT_DIR/final_verdict.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

mkdir -p "$RESULT_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT"

python3 - "$SOURCE_ROWS" "$RESULT_DIR" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$FINAL_VERDICT" "$SUMMARY_TXT" "$MAX_ROWS" "$QUOTE_API_URL" "$ENTRY_SLIPPAGE_BPS" "$EXIT_SLIPPAGE_BPS" "$MIN_EXIT_WSOL_RAW" "$MIN_EXIT_USDC_RAW" <<'PY'
import json
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

(
    source_arg,
    result_dir_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    verdict_arg,
    summary_txt_arg,
    max_rows_arg,
    quote_api_url,
    entry_slippage_arg,
    exit_slippage_arg,
    min_exit_wsol_arg,
    min_exit_usdc_arg,
) = sys.argv[1:14]

source_path = Path(source_arg) if source_arg else None
result_dir = Path(result_dir_arg)
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
verdict_path = Path(verdict_arg)
summary_txt_path = Path(summary_txt_arg)
max_rows = max(0, int(max_rows_arg or "0"))
entry_slippage_bps = int(float(entry_slippage_arg or 50))
exit_slippage_bps = int(float(exit_slippage_arg or 50))
min_exit_wsol_raw = int(float(min_exit_wsol_arg or 10000))
min_exit_usdc_raw = int(float(min_exit_usdc_arg or 1000))

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
TOKEN2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ALLOWED_EXTENSIONS = {"metadatapointer", "tokenmetadata"}
DENY_EXTENSIONS = {
    "transferhook",
    "permanentdelegate",
    "nontransferable",
    "defaultaccountstate",
    "confidentialtransfermint",
    "confidentialtransferfeeamountextension",
}


def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
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
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value, default=0):
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def first_text(row, *keys):
    for key in keys:
        value = text(row.get(key))
        if value:
            return value
    return ""


def first_int(row, *keys):
    for key in keys:
        value = safe_int(row.get(key), None)
        if value is not None and value > 0:
            return value
    return 0


def as_extensions(value):
    if isinstance(value, list):
        return [text(v) for v in value if text(v)]
    if isinstance(value, str):
        raw = value.strip()
        if not raw:
            return []
        if raw.startswith("["):
            try:
                parsed = json.loads(raw)
            except Exception:
                parsed = []
            return as_extensions(parsed)
        return [part.strip() for part in raw.replace(";", ",").split(",") if part.strip()]
    return []


def normalize_extension(value):
    return lower(value).replace("_", "").replace("-", "").replace(" ", "")


def http_json(url):
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "shadow-66ctr/1.0"})
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=25, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:
        return None, {"error": str(exc)}


def quote_exact_in(input_mint, output_mint, amount_raw, slippage_bps):
    if not input_mint or not output_mint or amount_raw <= 0:
        return None, {"error": "missing_quote_input"}
    params = urllib.parse.urlencode(
        {
            "inputMint": input_mint,
            "outputMint": output_mint,
            "amount": str(int(amount_raw)),
            "slippageBps": str(int(slippage_bps)),
            "swapMode": "ExactIn",
            "restrictIntermediateTokens": "true",
        }
    )
    return http_json(f"{quote_api_url}?{params}")


rows = [row for row in load_jsonl(source_path) if text(row.get("review_decision")) == "REVIEW_ONLY_METADATA_TOKEN2022"]
if max_rows > 0:
    rows = rows[:max_rows]

out = []
for index, row in enumerate(rows, start=1):
    candidate_id = first_text(row, "candidate_id") or f"token2022_review_{index}"
    token_program = first_text(row, "token_program")
    mint_authority = first_text(row, "mint_authority")
    freeze_authority = first_text(row, "freeze_authority")
    extensions = as_extensions(row.get("token2022_extensions") if "token2022_extensions" in row else row.get("extensions"))
    normalized_extensions = {normalize_extension(item) for item in extensions if normalize_extension(item)}
    allowed_metadata_only = bool(
        token_program == TOKEN2022 and normalized_extensions and normalized_extensions.issubset(ALLOWED_EXTENSIONS)
    )
    dangerous_extensions = sorted(item for item in normalized_extensions if item in DENY_EXTENSIONS or item not in ALLOWED_EXTENSIONS)

    mint_in = first_text(row, "mint_in")
    mint_out = first_text(row, "mint_out")
    input_amount_raw = first_int(
        row,
        "candidate_trade_size_lamports",
        "trade_size_lamports",
        "pm_owner_lock_notional_raw",
        "pm_reserved_notional_raw",
        "entry_amount_raw",
        "amount_raw",
    )
    fresh_entry_quote, fresh_entry_quote_error = quote_exact_in(mint_in, mint_out, input_amount_raw, entry_slippage_bps)
    entry_quote_out_raw = safe_int((fresh_entry_quote or {}).get("outAmount"))
    entry_min_out_raw = safe_int((fresh_entry_quote or {}).get("otherAmountThreshold"))
    fresh_entry_positive = entry_quote_out_raw > 0 and entry_min_out_raw > 0
    fresh_exit_quote, fresh_exit_quote_error = quote_exact_in(mint_out, mint_in, entry_min_out_raw, exit_slippage_bps) if entry_min_out_raw > 0 else (None, {"error": "entry_min_out_missing"})
    exit_quote_out_raw = safe_int((fresh_exit_quote or {}).get("outAmount"))
    exit_min_out_raw = safe_int((fresh_exit_quote or {}).get("otherAmountThreshold"))
    fresh_exit_positive = exit_quote_out_raw > 0 and exit_min_out_raw > 0

    if mint_in == WSOL_MINT:
        exit_non_dust = exit_min_out_raw >= min_exit_wsol_raw
        dust_threshold_basis = "wsol"
    elif mint_in == USDC_MINT:
        exit_non_dust = exit_min_out_raw >= min_exit_usdc_raw
        dust_threshold_basis = "usdc"
    else:
        exit_non_dust = exit_min_out_raw > 0
        dust_threshold_basis = "entry_mint_roundtrip"

    fee_cost = safe_int(row.get("pm_lock_fee_raw") or row.get("pm_reserved_fee_raw"))
    tip_cost = safe_int(row.get("pm_lock_tip_raw") or row.get("pm_reserved_tip_raw"))
    route_cost_input_raw = fee_cost + tip_cost
    fresh_net_after_all_costs = exit_quote_out_raw - input_amount_raw - route_cost_input_raw if fresh_exit_positive and input_amount_raw > 0 else None

    pm_owner_lock_ready = boolish(row.get("pm_owner_lock_ready"))
    account_or_inline_ata_ready = boolish(row.get("account_or_inline_ata_ready"))
    template_ready = boolish(row.get("template_ready"))
    egress_no_send_ready = boolish(row.get("egress_no_send_ready")) or boolish(row.get("sender_possible")) or boolish(row.get("jito_bundle_possible"))
    exit_route_exists = fresh_exit_positive and egress_no_send_ready
    live_allowed = False

    if freeze_authority:
        decision = "DENY_FREEZE_AUTHORITY"
    elif mint_authority:
        decision = "DENY_AUTHORITY_RISK"
    elif token_program != TOKEN2022 or not allowed_metadata_only or dangerous_extensions:
        decision = "DENY_EXTENSION_RISK"
    elif not pm_owner_lock_ready or not account_or_inline_ata_ready or not template_ready or not exit_route_exists or not exit_non_dust:
        decision = "DENY_EXIT_RISK"
    elif fresh_net_after_all_costs is None or fresh_net_after_all_costs <= 0:
        decision = "DENY_NEGATIVE_AFTER_COST"
    else:
        decision = "CONTINUATION_R1_REVIEW_READY_NO_SEND"

    out.append(
        {
            **row,
            "candidate_id": candidate_id,
            "risk_tier": "CONTINUATION_R1_METADATA_ONLY_TOKEN2022",
            "allowed_metadata_only_extensions": allowed_metadata_only,
            "normalized_extensions": sorted(normalized_extensions),
            "dangerous_extensions": dangerous_extensions,
            "fresh_entry_quote": fresh_entry_quote or {"ok": False, "error": fresh_entry_quote_error or {"error": "entry_quote_missing"}},
            "fresh_entry_positive": fresh_entry_positive,
            "fresh_exit_quote": fresh_exit_quote or {"ok": False, "error": fresh_exit_quote_error or {"error": "exit_quote_missing"}},
            "fresh_exit_positive": fresh_exit_positive,
            "entry_min_out_positive": entry_min_out_raw > 0,
            "exit_min_out_positive": exit_min_out_raw > 0,
            "entry_expected_out_raw": entry_quote_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "exit_expected_out_raw": exit_quote_out_raw,
            "exit_min_out_raw": exit_min_out_raw,
            "exit_route_exists": exit_route_exists,
            "exit_non_dust": exit_non_dust,
            "dust_threshold_basis": dust_threshold_basis,
            "route_cost_input_raw": route_cost_input_raw,
            "fresh_net_after_all_costs": fresh_net_after_all_costs,
            "pm_owner_lock_ready": pm_owner_lock_ready,
            "account_or_inline_ata_ready": account_or_inline_ata_ready,
            "template_ready": template_ready,
            "egress_no_send_ready": egress_no_send_ready,
            "review_decision": decision,
            "live_allowed": live_allowed,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

decision_counts = Counter(row["review_decision"] for row in out)
summary = {
    "status": "CONTINUATION_R1_REVIEW_READY_NO_SEND" if decision_counts.get("CONTINUATION_R1_REVIEW_READY_NO_SEND", 0) > 0 else "PARK_CONTINUATION_R1_TOKEN2022_REVIEW",
    "source_rows": str(source_path or ""),
    "rows_seen": len(out),
    "review_ready_no_send_rows": decision_counts.get("CONTINUATION_R1_REVIEW_READY_NO_SEND", 0),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
verdict = {
    "final_verdict": summary["status"],
    "rows_seen": len(out),
    "review_ready_no_send_rows": summary["review_ready_no_send_rows"],
    "decision_counts": dict(decision_counts),
    "live_approval": False,
    "hard_stops_preserved": {
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    },
}
manifest = {
    "source_rows": str(source_path or ""),
    "result_dir": str(result_dir),
    "risk_tier": "CONTINUATION_R1_METADATA_ONLY_TOKEN2022",
    "live_send": False,
    "sendBundle": False,
}

with rows_out_path.open("w", encoding="utf-8") as fh:
    for row in out:
        fh.write(json.dumps(row, sort_keys=True) + "\n")
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
verdict_path.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{k}={json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v}"
        for k, v in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Run66CTR continuation Token-2022 risk-tier review written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

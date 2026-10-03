#!/usr/bin/env bash
# 66ci_continuation_held_input_inventory_planner.sh - rank continuation held-input inventory bands; no acquisition

source "$(dirname "$0")/lib.sh"
script_header "66CI - Continuation Held-Input Inventory Planner"

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
STEP="${SHADOW_RUN66CI_STEP:-66ci_continuation_held_input_inventory_planner}"
STATUS_PREFIX="${SHADOW_RUN66CI_STATUS_PREFIX:-run66ci_continuation_held_input_inventory_planner}"
REPORT_BASENAME="${SHADOW_RUN66CI_REPORT_BASENAME:-run66ci_continuation_held_input_inventory_planner}"
READINESS_GLOB="${SHADOW_RUN66CI_READINESS_GLOB:-66u_repeatability_chain_20260524_3x5m_retry3*_66c_readiness/run66c_continuation_readiness_rows.jsonl}"
AUTOPSY_GLOB="${SHADOW_RUN66CI_AUTOPSY_GLOB:-66u_repeatability_chain_20260524_3x5m_retry3*_run65_run55_run39/candidate_autopsy.jsonl}"
RPC_URL="${SHADOW_RUN66CI_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN66CI_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
QUOTE_SLIPPAGE_BPS="${SHADOW_RUN66CI_QUOTE_SLIPPAGE_BPS:-50}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN66CI_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN66CI_MIN_EXIT_VALUE_RATIO:-0.50}"
MAX_INVENTORY_AT_RISK_USDC="${SHADOW_RUN66CI_MAX_INVENTORY_AT_RISK_USDC:-100.0}"
INVENTORY_TTL_SECONDS="${SHADOW_RUN66CI_INVENTORY_TTL_SECONDS:-300}"
SOL_USD_EQUIVALENT="${SHADOW_RUN66CI_SOL_USD_EQUIVALENT:-1000.0}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66ci_continuation_held_input_inventory_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - \
    "$RESULTS_ROOT" \
    "$READINESS_GLOB" \
    "$AUTOPSY_GLOB" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$QUOTE_SLIPPAGE_BPS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$MAX_INVENTORY_AT_RISK_USDC" \
    "$INVENTORY_TTL_SECONDS" \
    "$SOL_USD_EQUIVALENT" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKENKEG_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPo5uXXEHRsyiSScWZ9Do"
ALPH = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

results_root = Path(sys.argv[1])
readiness_glob = sys.argv[2]
autopsy_glob = sys.argv[3]
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
rows_jsonl = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
rpc_url = sys.argv[9]
quote_api_url = sys.argv[10]
quote_slippage_bps = int(sys.argv[11] or "50")
min_exit_value_usdc = float(sys.argv[12] or "0.10")
min_exit_value_ratio = float(sys.argv[13] or "0.50")
max_inventory_at_risk_usdc = float(sys.argv[14] or "100.0")
inventory_ttl_seconds = int(sys.argv[15] or "300")
sol_usd_equivalent = float(sys.argv[16] or "1000.0")


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            pass
    return rows


def write_jsonl(path: Path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
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


def b58encode(raw: bytes):
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


def http_json(url: str, payload=None, headers=None, timeout=20):
    final_headers = {"Accept": "application/json"}
    if headers:
        final_headers.update(headers)
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        final_headers.setdefault("Content-Type", "application/json")
    request = urllib.request.Request(url, data=data, headers=final_headers)
    with urllib.request.urlopen(request, timeout=timeout, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc_call(method, params):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    return http_json(rpc_url, payload, headers={"Content-Type": "application/json"})


quote_cache = {}


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int):
    amount_raw = safe_int(amount_raw)
    if amount_raw <= 0:
        return {"ok": False, "reason": "amount_zero", "out_amount": 0, "value_usdc": None, "route_plan_labels": []}
    key = (input_mint, output_mint, amount_raw)
    if key in quote_cache:
        return quote_cache[key]
    if input_mint == output_mint:
        value_usdc = amount_raw / 1_000_000 if input_mint in {USDC_MINT, USDT_MINT} else None
        out = {"ok": value_usdc is not None, "reason": "same_mint", "out_amount": amount_raw, "value_usdc": value_usdc, "route_plan_labels": []}
        quote_cache[key] = out
        return out
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(amount_raw),
        "slippageBps": str(quote_slippage_bps),
        "restrictIntermediateTokens": "true",
    }
    url = quote_api_url + "?" + urllib.parse.urlencode(params)
    delay = 0.5
    for attempt in range(1, 4):
        try:
            payload = http_json(url, timeout=20)
            out_amount = safe_int(payload.get("outAmount"))
            value_usdc = out_amount / 1_000_000 if output_mint in {USDC_MINT, USDT_MINT} else None
            out = {
                "ok": out_amount > 0,
                "reason": "quote_ok" if out_amount > 0 else "quote_zero",
                "out_amount": out_amount,
                "value_usdc": value_usdc,
                "route_plan_labels": [
                    text((item.get("swapInfo") or {}).get("label"))
                    for item in payload.get("routePlan") or []
                    if text((item.get("swapInfo") or {}).get("label"))
                ],
            }
            quote_cache[key] = out
            return out
        except urllib.error.HTTPError as exc:
            if exc.code == 429 and attempt < 3:
                time.sleep(delay)
                delay *= 2
                continue
            out = {"ok": False, "reason": f"quote_http_{exc.code}", "out_amount": 0, "value_usdc": None, "route_plan_labels": []}
            quote_cache[key] = out
            return out
        except Exception as exc:
            if attempt < 3:
                time.sleep(delay)
                delay *= 2
                continue
            out = {"ok": False, "reason": f"quote_error:{type(exc).__name__}", "out_amount": 0, "value_usdc": None, "route_plan_labels": []}
            quote_cache[key] = out
            return out
    out = {"ok": False, "reason": "quote_retry_exhausted", "out_amount": 0, "value_usdc": None, "route_plan_labels": []}
    quote_cache[key] = out
    return out


def choose_entry_quote_for_target(output_mint: str, target_raw: int, cap_usdc: float):
    cap_raw = int(max(0.0, cap_usdc) * 1_000_000)
    if target_raw <= 0 or cap_raw <= 0:
        return None, {"reason": "target_or_cap_zero"}
    probe_raw = min(cap_raw, 1_000_000)
    quote = quote_exact_in(USDC_MINT, output_mint, probe_raw)
    if not quote.get("ok"):
        return None, quote
    if safe_int(quote.get("out_amount")) < target_raw:
        high = probe_raw
        last_quote = quote
        while high < cap_raw:
            high = min(cap_raw, max(high + 1, high * 2))
            last_quote = quote_exact_in(USDC_MINT, output_mint, high)
            if not last_quote.get("ok"):
                break
            if safe_int(last_quote.get("out_amount")) >= target_raw:
                break
        if not last_quote.get("ok") or safe_int(last_quote.get("out_amount")) < target_raw:
            return last_quote, {"reason": "entry_cap_does_not_reach_target", "cap_raw": cap_raw, "cap_usdc": cap_usdc}
        low = max(1, high // 2)
    else:
        high = probe_raw
        low = 1
        last_quote = quote
    chosen = last_quote
    chosen_raw = high
    while low <= high:
        mid = (low + high) // 2
        mid_quote = quote_exact_in(USDC_MINT, output_mint, mid)
        if not mid_quote.get("ok"):
            low = mid + 1
            continue
        if safe_int(mid_quote.get("out_amount")) >= target_raw:
            chosen = mid_quote
            chosen_raw = mid
            high = mid - 1
        else:
            low = mid + 1
    chosen = dict(chosen)
    chosen["chosen_input_raw"] = chosen_raw
    chosen["chosen_input_usdc"] = chosen_raw / 1_000_000
    return chosen, None


def decode_mint_account(mint: str, account):
    if not account:
        return {"mint": mint, "metadata_available": False, "token_program": "", "decimals": None}
    raw_payload = ((account.get("data") or [None])[0]) if isinstance(account.get("data"), list) else None
    raw = base64.b64decode(raw_payload) if raw_payload else b""
    mint_authority = None
    freeze_authority = None
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_authority = b58encode(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_authority = b58encode(raw[50:82])
    return {
        "mint": mint,
        "metadata_available": True,
        "token_program": account.get("owner"),
        "decimals": raw[44] if len(raw) >= 45 else None,
        "mint_authority": mint_authority,
        "freeze_authority": freeze_authority,
    }


def fetch_mint_metadata(mints):
    result = {}
    mints = [mint for mint in sorted(set(mints)) if mint]
    for i in range(0, len(mints), 100):
        batch = mints[i : i + 100]
        try:
            payload = rpc_call("getMultipleAccounts", [batch, {"encoding": "base64"}])
            accounts = ((payload.get("result") or {}).get("value")) or []
        except Exception:
            accounts = [None] * len(batch)
        for mint, account in zip(batch, accounts):
            result[mint] = decode_mint_account(mint, account)
    return result


def policy_for(metadata):
    program = text(metadata.get("token_program"))
    if not metadata.get("metadata_available"):
        return "unknown_metadata_retry"
    if program == TOKEN2022_PROGRAM:
        return "token2022_requires_explicit_allow"
    if program != TOKENKEG_PROGRAM:
        return "unknown_token_program"
    if metadata.get("mint_authority") or metadata.get("freeze_authority"):
        return "authority_risk"
    return "pass"


def value_usdc_from_raw(mint: str, amount_raw: int):
    amount_raw = safe_int(amount_raw)
    if amount_raw <= 0:
        return None
    if mint in {USDC_MINT, USDT_MINT}:
        return amount_raw / 1_000_000
    if mint == WSOL_MINT:
        return amount_raw / 1_000_000_000 * sol_usd_equivalent
    quote = quote_exact_in(mint, USDC_MINT, amount_raw)
    return quote.get("value_usdc") if quote.get("ok") else None


readiness_paths = sorted(results_root.glob(readiness_glob))
autopsy_paths = sorted(results_root.glob(autopsy_glob))
autopsy_by_candidate = {}
for path in autopsy_paths:
    for row in load_jsonl(path):
        cid = text(row.get("candidate_id"))
        if cid:
            autopsy_by_candidate[cid] = row

source_rows = []
for path in readiness_paths:
    sample = path.parent.name.removesuffix("_66c_readiness")
    for row in load_jsonl(path):
        if row.get("synthetic_inventory_overlay_would_fire") and not row.get("baseline_would_fire"):
            item = dict(row)
            item["sample"] = sample
            item["source_readiness_rows"] = str(path)
            source_rows.append(item)

groups = defaultdict(list)
for row in source_rows:
    groups[text(row.get("mint_in"))].append(row)

metadata_by_mint = fetch_mint_metadata(groups.keys())
out = []
for mint, rows in groups.items():
    metadata = metadata_by_mint.get(mint) or {}
    policy_verdict = policy_for(metadata)
    matched_values = [safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw")) for row in rows]
    required_raw_values = []
    estimate_sources = Counter()
    for row in rows:
        autopsy = autopsy_by_candidate.get(text(row.get("candidate_id"))) or {}
        raw = (
            safe_int(autopsy.get("trade_size_lamports"))
            or safe_int(autopsy.get("trade_size_raw"))
            or safe_int(autopsy.get("amount_in_raw"))
            or safe_int(row.get("target_out_raw"))
            or safe_int(row.get("expected_out_raw"))
        )
        if safe_int(autopsy.get("trade_size_lamports")) > 0:
            estimate_sources["trade_size_lamports"] += 1
        elif safe_int(autopsy.get("trade_size_raw")) > 0:
            estimate_sources["trade_size_raw"] += 1
        elif safe_int(autopsy.get("amount_in_raw")) > 0:
            estimate_sources["amount_in_raw"] += 1
        else:
            estimate_sources["quote_output_fallback"] += 1
        if raw > 0:
            required_raw_values.append(raw)

    required_raw = max(required_raw_values) if required_raw_values else 0
    exit_quote = quote_exact_in(mint, USDC_MINT, required_raw)
    exit_value_usdc = exit_quote.get("value_usdc") if exit_quote.get("ok") else None
    entry_quote, entry_error = choose_entry_quote_for_target(mint, required_raw, max_inventory_at_risk_usdc)
    entry_quote_ok = bool(entry_quote and entry_quote.get("ok") and entry_quote.get("chosen_input_usdc") is not None)
    required_inventory_usdc = entry_quote.get("chosen_input_usdc") if entry_quote_ok else exit_value_usdc
    exit_quote_ok = bool(exit_quote.get("ok") and exit_value_usdc is not None and exit_value_usdc >= min_exit_value_usdc)
    exit_value_ratio = (
        exit_value_usdc / required_inventory_usdc
        if exit_value_usdc is not None and required_inventory_usdc and required_inventory_usdc > 0
        else None
    )
    exit_loss_pct = (
        (required_inventory_usdc - exit_value_usdc) / required_inventory_usdc * 100.0
        if exit_value_usdc is not None and required_inventory_usdc and required_inventory_usdc > 0
        else None
    )
    total_ev_raw = sum(matched_values)
    total_ev_usdc = value_usdc_from_raw(mint, total_ev_raw)
    roundtrip_loss_usdc = (
        required_inventory_usdc - exit_value_usdc
        if exit_value_usdc is not None and required_inventory_usdc is not None
        else None
    )
    net_after_carry = (
        total_ev_usdc - max(roundtrip_loss_usdc or 0.0, 0.0)
        if total_ev_usdc is not None
        else None
    )
    route_stability_status = "route_usable_exact_rows" if all(row.get("route_usable") for row in rows) else "route_unstable"
    signed_swap_simulation_passed = False
    auto_flatten_available = exit_quote_ok
    max_at_risk = required_inventory_usdc if required_inventory_usdc is not None else exit_value_usdc

    if policy_verdict != "pass":
        decision = "REJECT_POLICY"
    elif not exit_quote_ok or exit_value_ratio is None or exit_value_ratio < min_exit_value_ratio:
        decision = "REJECT_DUST_EXIT"
    elif not entry_quote_ok:
        decision = "RETRY_MORE_EVIDENCE"
    elif route_stability_status == "route_unstable":
        decision = "REJECT_ROUTE_UNSTABLE"
    elif max_at_risk is not None and max_at_risk > max_inventory_at_risk_usdc:
        decision = "REJECT_EXPOSURE_TOO_HIGH"
    elif net_after_carry is None:
        decision = "RETRY_MORE_EVIDENCE"
    elif net_after_carry <= 0:
        decision = "REJECT_LOW_EV"
    else:
        decision = "INVENTORY_SIM_REVIEW"

    out.append({
        "mint": mint,
        "symbol": "",
        "token_program": metadata.get("token_program"),
        "decimals": metadata.get("decimals"),
        "mint_authority": metadata.get("mint_authority"),
        "freeze_authority": metadata.get("freeze_authority"),
        "policy_verdict": policy_verdict,
        "row_count": len(rows),
        "exact_positive_row_count": len(rows),
        "synthetic_inventory_would_fire_delta": len(rows),
        "matched_rollout_positive_count": len(rows),
        "total_net_ev_after_fee_tip_raw": total_ev_raw,
        "total_net_ev_after_fee_tip_usdc": round_or_none(total_ev_usdc),
        "median_net_ev_after_fee_tip_raw": sorted(matched_values)[len(matched_values) // 2] if matched_values else 0,
        "p90_net_ev_after_fee_tip_raw": sorted(matched_values)[int((len(matched_values) - 1) * 0.9)] if matched_values else 0,
        "required_inventory_raw": required_raw,
        "required_inventory_raw_source_counts": dict(estimate_sources),
        "required_inventory_usdc": round_or_none(required_inventory_usdc),
        "exit_value_usdc": round_or_none(exit_value_usdc),
        "exit_loss_pct": round_or_none(exit_loss_pct),
        "exit_value_ratio": round_or_none(exit_value_ratio),
        "entry_quote_ok": entry_quote_ok,
        "entry_quote_reason": (entry_error or {}).get("reason") if entry_error else (entry_quote or {}).get("reason"),
        "entry_route_plan_labels": (entry_quote or {}).get("route_plan_labels") or [],
        "exit_quote_ok": exit_quote_ok,
        "exit_quote_reason": exit_quote.get("reason"),
        "exit_route_plan_labels": exit_quote.get("route_plan_labels") or [],
        "signed_swap_simulation_passed": signed_swap_simulation_passed,
        "route_stability_status": route_stability_status,
        "inventory_ttl_seconds": inventory_ttl_seconds,
        "auto_flatten_available": auto_flatten_available,
        "max_inventory_at_risk_usdc": round_or_none(max_at_risk),
        "roundtrip_loss_usdc": round_or_none(roundtrip_loss_usdc),
        "net_after_inventory_carry_cost_usdc": round_or_none(net_after_carry),
        "expected_conversion_delta": len(rows),
        "sample_count": len({row.get("sample") for row in rows}),
        "sample_names": sorted({row.get("sample") for row in rows}),
        "decision": decision,
        "acquisition_allowed": False,
        "live_send_allowed": False,
        "wire_send": 0,
        "live_landed": 0,
    })

out.sort(
    key=lambda row: (
        row["decision"] == "INVENTORY_SIM_REVIEW",
        safe_float(row.get("net_after_inventory_carry_cost_usdc")),
        safe_int(row.get("synthetic_inventory_would_fire_delta")),
        -safe_float(row.get("max_inventory_at_risk_usdc")),
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, out)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_readiness_glob": readiness_glob,
    "source_autopsy_glob": autopsy_glob,
    "source_readiness_file_count": len(readiness_paths),
    "source_autopsy_file_count": len(autopsy_paths),
    "source_inventory_only_rows": len(source_rows),
    "mint_group_count": len(out),
    "decision_counts": dict(Counter(row["decision"] for row in out)),
    "inventory_sim_review_count": sum(1 for row in out if row["decision"] == "INVENTORY_SIM_REVIEW"),
    "inventory_sim_review_expected_delta": sum(row["expected_conversion_delta"] for row in out if row["decision"] == "INVENTORY_SIM_REVIEW"),
    "acquisition_allowed": False,
    "live_send_allowed": False,
    "wire_send": 0,
    "live_landed": 0,
    "top_items": out[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_readiness_file_count={summary['source_readiness_file_count']}",
    f"source_autopsy_file_count={summary['source_autopsy_file_count']}",
    f"source_inventory_only_rows={summary['source_inventory_only_rows']}",
    f"mint_group_count={summary['mint_group_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"inventory_sim_review_count={summary['inventory_sim_review_count']}",
    f"inventory_sim_review_expected_delta={summary['inventory_sim_review_expected_delta']}",
    "acquisition_allowed=false",
    "live_send_allowed=false",
    "wire_send=0",
    "live_landed=0",
]
if out:
    top = out[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_expected_conversion_delta={top['expected_conversion_delta']}",
        f"top_required_inventory_usdc={top['required_inventory_usdc']}",
        f"top_exit_value_usdc={top['exit_value_usdc']}",
        f"top_net_after_inventory_carry_cost_usdc={top['net_after_inventory_carry_cost_usdc']}",
        f"top_policy_verdict={top['policy_verdict']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66CI continuation held-input inventory planner written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

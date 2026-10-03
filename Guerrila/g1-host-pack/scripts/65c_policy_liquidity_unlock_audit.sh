#!/usr/bin/env bash
# 65c_policy_liquidity_unlock_audit.sh - strict decision audit for Run65A/65B unlock rows

source "$(dirname "$0")/lib.sh"
script_header "65C - Policy/Liquidity Unlock Audit"

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
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN65C_STEP:-65c_policy_liquidity_unlock_audit}"
STATUS_PREFIX="${SHADOW_RUN65C_STATUS_PREFIX:-run65c_policy_liquidity_unlock_audit}"
REPORT_BASENAME="${SHADOW_RUN65C_REPORT_BASENAME:-run65c_policy_liquidity_unlock_audit}"

RUN65AB_STEP="${SHADOW_RUN65C_RUN65AB_STEP:-65ab_epoch_unlock_planner_20260524_5m}"
RUN65AB_DIR="$RESULTS_ROOT/$RUN65AB_STEP"
RUN65AB_JSON="$RUN65AB_DIR/run65ab_epoch_unlock_planner.json"
RUN65A_ROWS="$RUN65AB_DIR/run65a_account_ata_unlock_rows.jsonl"
RUN65B_ROWS="$RUN65AB_DIR/run65b_exit_qualified_held_input_overlay_rows.jsonl"

RPC_URL="${SHADOW_RUN65C_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
QUOTE_API_URL="${SHADOW_RUN65C_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN65C_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN65C_MIN_EXIT_VALUE_RATIO:-0.50}"
MIN_ENTRY_VALUE_USDC="${SHADOW_RUN65C_MIN_ENTRY_VALUE_USDC:-1.00}"
MIN_STRICT_EV_USD="${SHADOW_RUN65C_MIN_STRICT_EV_USD:-0.0}"
LOW_EV_NOTE_USD="${SHADOW_RUN65C_LOW_EV_NOTE_USD:-0.01}"
QUOTE_SLIPPAGE_BPS="${SHADOW_RUN65C_QUOTE_SLIPPAGE_BPS:-50}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
RUN65A_AUDIT_ROWS="$RESULT_DIR/run65a_policy_liquidity_audit_rows.jsonl"
RUN65A_AUDIT_SUMMARY="$RESULT_DIR/run65a_policy_liquidity_audit_summary.json"
RUN65B_AUDIT_ROWS="$RESULT_DIR/run65b_strict_overlay_audit_rows.jsonl"
RUN65B_AUDIT_SUMMARY="$RESULT_DIR/run65b_strict_overlay_audit_summary.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$RUN65A_AUDIT_ROWS" "$RUN65A_AUDIT_SUMMARY" "$RUN65B_AUDIT_ROWS" "$RUN65B_AUDIT_SUMMARY"

missing=0
for required_path in "$RUN65AB_JSON" "$RUN65A_ROWS" "$RUN65B_ROWS"; do
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
    "$RUN65AB_JSON" \
    "$RUN65A_ROWS" \
    "$RUN65B_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$RUN65A_AUDIT_ROWS" \
    "$RUN65A_AUDIT_SUMMARY" \
    "$RUN65B_AUDIT_ROWS" \
    "$RUN65B_AUDIT_SUMMARY" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" \
    "$MIN_ENTRY_VALUE_USDC" \
    "$MIN_STRICT_EV_USD" \
    "$LOW_EV_NOTE_USD" \
    "$QUOTE_SLIPPAGE_BPS" <<'PY'
import base64
import json
import sys
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
POLICY_PASS = {"ok", "metadata_only_token2022_allowed"}

run65ab_json = Path(sys.argv[1])
run65a_rows_path = Path(sys.argv[2])
run65b_rows_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
run65a_audit_rows_path = Path(sys.argv[6])
run65a_audit_summary_path = Path(sys.argv[7])
run65b_audit_rows_path = Path(sys.argv[8])
run65b_audit_summary_path = Path(sys.argv[9])
step = sys.argv[10]
status_prefix = sys.argv[11]
rpc_url = sys.argv[12]
quote_api_url = sys.argv[13]
min_exit_value_usdc = float(sys.argv[14] or "0.10")
min_exit_value_ratio = float(sys.argv[15] or "0.50")
min_entry_value_usdc = float(sys.argv[16] or "1.00")
min_strict_ev_usd = float(sys.argv[17] or "0.0")
low_ev_note_usd = float(sys.argv[18] or "0.01")
quote_slippage_bps = int(sys.argv[19] or "50")


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


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


def text(value):
    return (value or "").strip() if isinstance(value, str) else str(value or "").strip()


def b58encode(raw):
    if not raw:
        return None
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, rem = divmod(n, 58)
        out = BASE58_ALPHABET[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + (out or "1")


def rpc_call(method, params):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(rpc_url, data=payload, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        data = json.loads(resp.read().decode())
    if data.get("error"):
        raise RuntimeError(data["error"])
    return data.get("result")


def decode_mint_account(mint, account):
    if not account:
        return {"mint": mint, "metadata_available": False, "decimals": None}
    raw_payload = ((account.get("data") or [None])[0]) if isinstance(account.get("data"), list) else None
    raw = base64.b64decode(raw_payload) if raw_payload else b""
    return {
        "mint": mint,
        "metadata_available": True,
        "token_program": account.get("owner"),
        "decimals": raw[44] if len(raw) >= 45 else None,
        "mint_authority": b58encode(raw[4:36]) if len(raw) >= 82 and int.from_bytes(raw[0:4], "little") != 0 else None,
        "freeze_authority": b58encode(raw[50:82]) if len(raw) >= 82 and int.from_bytes(raw[46:50], "little") != 0 else None,
    }


def fetch_mint_metadata(mints):
    result = {}
    mints = [mint for mint in sorted(set(mints)) if mint]
    for i in range(0, len(mints), 100):
        batch = mints[i:i + 100]
        accounts = rpc_call("getMultipleAccounts", [batch, {"encoding": "base64"}]).get("value") or []
        for mint, account in zip(batch, accounts):
            result[mint] = decode_mint_account(mint, account)
    return result


quote_cache = {}


def quote(input_mint, output_mint, amount_raw):
    amount_raw = safe_int(amount_raw)
    if amount_raw <= 0:
        return {"ok": False, "reason": "amount_zero", "out_amount": 0, "value_usdc": None}
    key = (input_mint, output_mint, amount_raw)
    if key in quote_cache:
        return quote_cache[key]
    if input_mint == output_mint:
        value_usdc = None
        if output_mint in {USDC_MINT, USDT_MINT}:
            value_usdc = amount_raw / 1_000_000
        out = {"ok": value_usdc is not None, "reason": "same_mint", "out_amount": amount_raw, "value_usdc": value_usdc}
        quote_cache[key] = out
        return out
    qs = urllib.parse.urlencode({
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(amount_raw),
        "slippageBps": str(quote_slippage_bps),
        "restrictIntermediateTokens": "true",
    })
    try:
        with urllib.request.urlopen(f"{quote_api_url}?{qs}", timeout=10) as resp:
            payload = json.loads(resp.read().decode())
        out_amount = safe_int(payload.get("outAmount"))
        value_usdc = out_amount / 1_000_000 if output_mint in {USDC_MINT, USDT_MINT} else None
        out = {
            "ok": out_amount > 0,
            "reason": "quote_ok" if out_amount > 0 else "quote_zero",
            "out_amount": out_amount,
            "value_usdc": value_usdc,
            "route_plan_labels": [text((item.get("swapInfo") or {}).get("label")) for item in payload.get("routePlan") or []],
        }
    except Exception as exc:
        out = {"ok": False, "reason": f"quote_error:{type(exc).__name__}", "out_amount": 0, "value_usdc": None, "route_plan_labels": []}
    quote_cache[key] = out
    return out


run65ab = load_json(run65ab_json)
run65a_rows = load_jsonl(run65a_rows_path)
run65b_rows = load_jsonl(run65b_rows_path)
sol_usd = safe_float(((run65ab.get("run65a") or {}).get("top_items") or [{}])[0].get("sol_usd_equivalent"), 0.0) or 1000.0

mints = {text(row.get("mint")) for row in run65a_rows + run65b_rows if text(row.get("mint"))}
metadata_by_mint = fetch_mint_metadata(mints)

run65a_audit = []
for row in run65a_rows:
    mint = text(row.get("mint"))
    policy_verdict = text(row.get("policy_verdict"))
    policy_ok = policy_verdict in POLICY_PASS
    exit_value = safe_float(row.get("exit_quote_value"))
    exit_quote_ok = bool(row.get("exit_quote_ok")) and exit_value >= min_exit_value_usdc
    if not policy_ok:
        decision = "REJECT_POLICY"
        ata_decision = "PARK_NO_PRECREATE"
    elif not exit_quote_ok:
        decision = "REJECT_DUST_EXIT" if exit_value and exit_value < min_exit_value_usdc else "RETRY_LATER"
        ata_decision = "PARK_NO_PRECREATE"
    elif safe_float(row.get("missed_ev_sol_equivalent")) <= 0:
        decision = "REJECT_LOW_EV"
        ata_decision = "PARK_NO_PRECREATE"
    else:
        decision = "PLAN_ONLY"
        ata_decision = "PLAN_ONLY_NO_CREATE"
    item = dict(row)
    item.update({
        "decision": decision,
        "ata_decision": ata_decision,
        "policy_pass": policy_ok,
        "exit_value_usdc": row.get("exit_quote_value"),
        "liquidity_pass": exit_quote_ok,
        "precreate_action_allowed_now": False,
        "audit_note": "No ATA creation from Run65A until conservative policy and full-band exit liquidity pass.",
    })
    run65a_audit.append(item)

run65a_audit.sort(key=lambda item: (item.get("decision") == "PLAN_ONLY", safe_float(item.get("missed_ev_sol_equivalent")), safe_int(item.get("frequency"))), reverse=True)

run65b_audit = []
for row in run65b_rows:
    mint = text(row.get("mint"))
    metadata = metadata_by_mint.get(mint) or {}
    policy_verdict = text(row.get("policy_verdict"))
    policy_ok = policy_verdict in POLICY_PASS
    required_raw = safe_int(row.get("required_input_raw_p90")) or safe_int(row.get("required_input_raw_max"))
    entry_quote = quote(USDC_MINT, mint, int(min_entry_value_usdc * 1_000_000))
    exit_quote = quote(mint, USDC_MINT, required_raw)
    entry_out_raw = safe_int(entry_quote.get("out_amount"))
    required_acquisition_usdc = (required_raw / entry_out_raw * min_entry_value_usdc) if entry_out_raw > 0 else None
    exit_value_usdc = exit_quote.get("value_usdc")
    if exit_value_usdc is None:
        exit_value_usdc = row.get("exit_quote_value")
    exit_value_usdc = safe_float(exit_value_usdc)
    exit_value_ratio = (exit_value_usdc / required_acquisition_usdc) if required_acquisition_usdc and required_acquisition_usdc > 0 else None
    exit_loss_pct = ((required_acquisition_usdc - exit_value_usdc) / required_acquisition_usdc * 100.0) if required_acquisition_usdc and required_acquisition_usdc > 0 else None
    strict_ev_sol = safe_float(row.get("missed_ev_sol_equivalent"))
    strict_ev_usd = strict_ev_sol * sol_usd
    route_stability_status = "unproven_quote_only"
    swap_simulation_status = "not_run_no_signed_swap"
    entry_quote_ok = bool(entry_quote.get("ok")) or bool(row.get("entry_quote_ok"))
    exit_quote_ok = bool(exit_quote.get("ok")) and exit_value_usdc >= min_exit_value_usdc
    if not policy_ok:
        decision = "REJECT_POLICY"
    elif not exit_quote_ok:
        decision = "REJECT_DUST_EXIT" if exit_value_usdc < min_exit_value_usdc else "RETRY_LATER"
    elif not entry_quote_ok or required_acquisition_usdc is None:
        decision = "RETRY_LATER"
    elif exit_value_ratio is not None and exit_value_ratio < min_exit_value_ratio:
        decision = "REJECT_DUST_EXIT"
    elif safe_int(row.get("expected_would_fire_delta")) <= 0 or strict_ev_usd <= min_strict_ev_usd:
        decision = "REJECT_LOW_EV"
    elif route_stability_status == "drifted_and_sim_failed":
        decision = "REJECT_ROUTE_UNSTABLE"
    else:
        decision = "SIMULATE_ONLY"
    classification = "entry_exit_liquid_but_low_ev" if decision == "SIMULATE_ONLY" and strict_ev_usd < low_ev_note_usd else "overlay_safe_for_sim_only" if decision == "SIMULATE_ONLY" else decision.lower()
    run65b_audit.append({
        "mint": mint,
        "token_program": row.get("token_program") or metadata.get("token_program"),
        "decimals": metadata.get("decimals"),
        "owner_mode_split": {text(row.get("owner_mode")): safe_int(row.get("frequency"))},
        "owner_mode": row.get("owner_mode"),
        "pool": row.get("pool"),
        "venue": row.get("venue"),
        "route_family": row.get("route_family"),
        "row_count": safe_int(row.get("frequency")),
        "fresh_epoch_frequency": safe_int(row.get("frequency")),
        "expected_would_fire_delta": safe_int(row.get("expected_would_fire_delta")),
        "strict_ev_sol_equivalent": round_or_none(strict_ev_sol, 9),
        "strict_ev_usd_equivalent": round_or_none(strict_ev_usd, 9),
        "entry_quote_ok": entry_quote_ok,
        "entry_quote_reason": entry_quote.get("reason") or row.get("entry_quote_reason"),
        "entry_route_plan_labels": entry_quote.get("route_plan_labels") or [],
        "exit_quote_ok": exit_quote_ok,
        "exit_quote_reason": exit_quote.get("reason") or row.get("exit_quote_reason"),
        "exit_value_usdc": round_or_none(exit_value_usdc, 9),
        "exit_loss_pct": round_or_none(exit_loss_pct, 9),
        "exit_value_ratio": round_or_none(exit_value_ratio, 9),
        "required_acquisition_raw": required_raw,
        "required_acquisition_usdc": round_or_none(required_acquisition_usdc, 9),
        "route_stability_status": route_stability_status,
        "swap_simulation_status": swap_simulation_status,
        "policy_verdict": policy_verdict,
        "policy_pass": policy_ok,
        "decision": decision,
        "classification": classification,
        "candidate_for_one_mint_plan": decision == "SIMULATE_ONLY",
        "candidate_for_one_mint_simulate": decision == "SIMULATE_ONLY",
        "sample_candidate_id": row.get("sample_candidate_id"),
    })

run65b_audit.sort(key=lambda item: (item["decision"] == "SIMULATE_ONLY", item["strict_ev_usd_equivalent"] or 0, item["expected_would_fire_delta"]), reverse=True)

write_jsonl(run65a_audit_rows_path, run65a_audit)
write_jsonl(run65b_audit_rows_path, run65b_audit)

run65a_summary = {
    "status": "run65a_policy_liquidity_audit_complete",
    "source_run65ab_json": str(run65ab_json),
    "row_count": len(run65a_audit),
    "decision_counts": dict(Counter(row["decision"] for row in run65a_audit)),
    "precreate_action_allowed_now_count": sum(1 for row in run65a_audit if row["precreate_action_allowed_now"]),
    "top_items": run65a_audit[:20],
}
run65b_summary = {
    "status": "run65b_strict_overlay_audit_complete",
    "source_run65ab_json": str(run65ab_json),
    "row_count": len(run65b_audit),
    "decision_counts": dict(Counter(row["decision"] for row in run65b_audit)),
    "classification_counts": dict(Counter(row["classification"] for row in run65b_audit)),
    "candidate_for_one_mint_simulate_count": sum(1 for row in run65b_audit if row["candidate_for_one_mint_simulate"]),
    "simulate_only_expected_would_fire_delta": sum(row["expected_would_fire_delta"] for row in run65b_audit if row["decision"] == "SIMULATE_ONLY"),
    "simulate_only_strict_ev_sol_equivalent": round_or_none(sum(row["strict_ev_sol_equivalent"] or 0.0 for row in run65b_audit if row["decision"] == "SIMULATE_ONLY"), 9),
    "top_items": run65b_audit[:20],
}
payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "run65a": run65a_summary,
    "run65b": run65b_summary,
    "policy": {
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_exit_value_ratio": min_exit_value_ratio,
        "min_entry_value_usdc": min_entry_value_usdc,
        "min_strict_ev_usd": min_strict_ev_usd,
        "low_ev_note_usd": low_ev_note_usd,
        "quote_slippage_bps": quote_slippage_bps,
    },
}

run65a_audit_summary_path.write_text(json.dumps(run65a_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
run65b_audit_summary_path.write_text(json.dumps(run65b_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={payload['status']}",
    f"source_run65ab_json={run65ab_json}",
    f"run65a_row_count={run65a_summary['row_count']}",
    f"run65a_decision_counts={json.dumps(run65a_summary['decision_counts'], sort_keys=True)}",
    f"run65a_precreate_action_allowed_now_count={run65a_summary['precreate_action_allowed_now_count']}",
    f"run65b_row_count={run65b_summary['row_count']}",
    f"run65b_decision_counts={json.dumps(run65b_summary['decision_counts'], sort_keys=True)}",
    f"run65b_classification_counts={json.dumps(run65b_summary['classification_counts'], sort_keys=True)}",
    f"run65b_candidate_for_one_mint_simulate_count={run65b_summary['candidate_for_one_mint_simulate_count']}",
    f"run65b_simulate_only_expected_would_fire_delta={run65b_summary['simulate_only_expected_would_fire_delta']}",
    f"run65b_simulate_only_strict_ev_sol_equivalent={run65b_summary['simulate_only_strict_ev_sol_equivalent']}",
]
if run65a_audit:
    top = run65a_audit[0]
    lines.extend([
        f"run65a_top_mint={top['mint']}",
        f"run65a_top_decision={top['decision']}",
        f"run65a_top_ata_decision={top['ata_decision']}",
        f"run65a_top_policy_verdict={top['policy_verdict']}",
        f"run65a_top_exit_value_usdc={top['exit_value_usdc']}",
    ])
if run65b_audit:
    top = run65b_audit[0]
    lines.extend([
        f"run65b_top_mint={top['mint']}",
        f"run65b_top_decision={top['decision']}",
        f"run65b_top_classification={top['classification']}",
        f"run65b_top_expected_would_fire_delta={top['expected_would_fire_delta']}",
        f"run65b_top_strict_ev_usd_equivalent={top['strict_ev_usd_equivalent']}",
        f"run65b_top_required_acquisition_usdc={top['required_acquisition_usdc']}",
        f"run65b_top_exit_value_usdc={top['exit_value_usdc']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65C policy/liquidity unlock audit written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

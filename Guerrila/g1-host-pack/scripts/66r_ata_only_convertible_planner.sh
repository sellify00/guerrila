#!/usr/bin/env bash
# 66r_ata_only_convertible_planner.sh - rank exact-positive ATA-only convertible rebalancing mints, no action

source "$(dirname "$0")/lib.sh"
script_header "66R-ATA - ATA-Only Convertible Planner"

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
STEP="${SHADOW_RUN66R_ATA_STEP:-66r_ata_only_convertible_planner}"
STATUS_PREFIX="${SHADOW_RUN66R_ATA_STATUS_PREFIX:-run66r_ata_only_convertible_planner}"
REPORT_BASENAME="${SHADOW_RUN66R_ATA_REPORT_BASENAME:-run66r_ata_only_convertible_planner}"
RUN66R_STEP="${SHADOW_RUN66R_ATA_SOURCE_STEP:-66r_rebalancing_exact_positive_readiness_overlay_20260524_5m_bridge2}"
RUN66R_ROWS="${SHADOW_RUN66R_ATA_SOURCE_ROWS:-$RESULTS_ROOT/$RUN66R_STEP/run66r_rebalancing_exact_positive_readiness_overlay_rows.jsonl}"

RPC_URL="${SHADOW_RUN66R_ATA_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
QUOTE_API_URL="${SHADOW_RUN66R_ATA_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN66R_ATA_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN66R_ATA_MIN_RECURRENCE_COUNT:-2}"
MIN_FRESH_EPOCH_SEEN_COUNT="${SHADOW_RUN66R_ATA_MIN_FRESH_EPOCH_SEEN_COUNT:-1}"
QUOTE_SLIPPAGE_BPS="${SHADOW_RUN66R_ATA_QUOTE_SLIPPAGE_BPS:-50}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN66R_ATA_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN66R_ATA_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN66R_ATA_ALLOW_MINT_AUTHORITY:-false}"
DENYLIST_MINTS="${SHADOW_RUN66R_ATA_DENYLIST_MINTS:-}"
ATA_ACCOUNT_SIZE="${SHADOW_RUN66R_ATA_ACCOUNT_SIZE:-165}"
ATA_CREATE_FEE_ESTIMATE_LAMPORTS="${SHADOW_RUN66R_ATA_CREATE_FEE_ESTIMATE_LAMPORTS:-5000}"
UNUSUALLY_STRONG_ROW_EV_LAMPORTS="${SHADOW_RUN66R_ATA_UNUSUALLY_STRONG_ROW_EV_LAMPORTS:-5000000}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66r_ata_only_convertible_planner_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN66R_ROWS" ]]; then
    fail "Missing Run66R overlay rows: $RUN66R_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RUN66R_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_RECURRENCE_COUNT" \
    "$MIN_FRESH_EPOCH_SEEN_COUNT" \
    "$QUOTE_SLIPPAGE_BPS" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$DENYLIST_MINTS" \
    "$ATA_ACCOUNT_SIZE" \
    "$ATA_CREATE_FEE_ESTIMATE_LAMPORTS" \
    "$UNUSUALLY_STRONG_ROW_EV_LAMPORTS" <<'PY'
import base64
import json
import math
import statistics
import sys
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
METADATA_ONLY_EXTENSIONS = {"MetadataPointer", "TokenMetadata"}
TOKEN2022_EXTENSION_NAMES = {
    2: "TransferFeeConfig",
    7: "MintCloseAuthority",
    10: "ImmutableOwner",
    12: "DefaultAccountState",
    13: "TransferHook",
    14: "ConfidentialTransferMint",
    15: "MetadataPointer",
    18: "GroupPointer",
    19: "GroupMemberPointer",
    20: "TokenMetadata",
}

source_rows_path = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
rpc_url = sys.argv[7]
quote_api_url = sys.argv[8]
min_exit_value_usdc = float(sys.argv[9] or "0.10")
min_recurrence_count = int(sys.argv[10] or "2")
min_fresh_epoch_seen_count = int(sys.argv[11] or "1")
quote_slippage_bps = int(sys.argv[12] or "50")
allow_metadata_only_token2022 = sys.argv[13].lower() == "true"
allow_freeze_authority = sys.argv[14].lower() == "true"
allow_mint_authority = sys.argv[15].lower() == "true"
denylist_mints = {mint.strip() for mint in (sys.argv[16] or "").replace(",", " ").split() if mint.strip()}
ata_account_size = int(sys.argv[17] or "165")
ata_create_fee_estimate_lamports = int(sys.argv[18] or "5000")
unusually_strong_row_ev_lamports = int(sys.argv[19] or "5000000")


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            pass
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
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
    return round(float(value), digits)


def percentile(values, pct):
    values = sorted(safe_int(v) for v in values)
    if not values:
        return 0
    if len(values) == 1:
        return values[0]
    pos = (len(values) - 1) * pct
    lo = math.floor(pos)
    hi = math.ceil(pos)
    if lo == hi:
        return values[lo]
    return int(values[lo] + (values[hi] - values[lo]) * (pos - lo))


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


def fetch_ata_rent_lamports():
    try:
        return safe_int(rpc_call("getMinimumBalanceForRentExemption", [ata_account_size]))
    except Exception:
        return 2039280


def decode_mint_account(mint, account):
    if not account:
        return {"mint": mint, "metadata_available": False, "token_program": None, "decimals": None, "extensions": [], "mint_authority": None, "freeze_authority": None}
    owner = account.get("owner")
    raw_payload = ((account.get("data") or [None])[0]) if isinstance(account.get("data"), list) else None
    raw = base64.b64decode(raw_payload) if raw_payload else b""
    decimals = raw[44] if len(raw) >= 45 else None
    mint_auth = None
    freeze_auth = None
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_auth = b58encode(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_auth = b58encode(raw[50:82])
    extensions = []
    cursor = 82
    while owner == TOKEN2022_PROGRAM and cursor + 4 <= len(raw):
        ext_type = int.from_bytes(raw[cursor:cursor + 2], "little")
        ext_len = int.from_bytes(raw[cursor + 2:cursor + 4], "little")
        cursor += 4
        if ext_type == 0 and ext_len == 0:
            break
        extensions.append(TOKEN2022_EXTENSION_NAMES.get(ext_type, f"extension_{ext_type}"))
        cursor += max(0, ext_len)
    return {"mint": mint, "metadata_available": True, "token_program": owner, "decimals": decimals, "extensions": sorted(set(extensions)), "mint_authority": mint_auth, "freeze_authority": freeze_auth}


def fetch_metadata(mints):
    mints = [mint for mint in sorted(set(mints)) if mint]
    out = {}
    for i in range(0, len(mints), 100):
        batch = mints[i:i + 100]
        accounts = rpc_call("getMultipleAccounts", [batch, {"encoding": "base64"}]).get("value") or []
        for mint, account in zip(batch, accounts):
            out[mint] = decode_mint_account(mint, account)
    return out


def policy_for_mint(metadata):
    if not metadata.get("metadata_available"):
        return "metadata_unavailable", False
    if metadata.get("mint") in denylist_mints:
        return "denylisted", False
    if metadata.get("freeze_authority") and not allow_freeze_authority:
        return "freeze_authority_present", False
    if metadata.get("mint_authority") and not allow_mint_authority:
        return "mint_authority_present", False
    token_program = metadata.get("token_program")
    extensions = set(metadata.get("extensions") or [])
    if token_program == SPL_TOKEN_PROGRAM:
        return "pass", True
    if token_program == TOKEN2022_PROGRAM:
        if extensions.issubset(METADATA_ONLY_EXTENSIONS) and allow_metadata_only_token2022:
            return "pass", True
        if extensions.issubset(METADATA_ONLY_EXTENSIONS):
            return "metadata_only_token2022_requires_explicit_allow", False
        return "unsupported_token2022_extensions", False
    return "unsupported_token_program", False


quote_cache = {}


def quote(input_mint, output_mint, amount_raw):
    amount_raw = safe_int(amount_raw)
    if amount_raw <= 0:
        return {"ok": False, "reason": "amount_zero", "out_amount": 0, "value_usdc": None}
    key = (input_mint, output_mint, amount_raw)
    if key in quote_cache:
        return quote_cache[key]
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
        value = out_amount / 1_000_000 if output_mint in {USDC_MINT, USDT_MINT} else None
        result = {"ok": out_amount > 0, "reason": "quote_ok" if out_amount > 0 else "quote_zero", "out_amount": out_amount, "value_usdc": value}
    except Exception as exc:
        result = {"ok": False, "reason": f"quote_error:{type(exc).__name__}", "out_amount": 0, "value_usdc": None}
    quote_cache[key] = result
    return result


def convertible(row):
    return (
        bool(row.get("exact_quote_match"))
        and bool(row.get("matched_rollout_net_ev_after_fee_tip_positive"))
        and not bool(row.get("baseline_would_fire"))
        and bool(row.get("synthetic_ata_overlay_would_fire"))
        and text(row.get("baseline_blocker")) == "account_ata_missing"
    )


source_rows = load_jsonl(source_rows_path)
convertible_rows = [row for row in source_rows if convertible(row)]
groups = {}
for row in convertible_rows:
    mint = text(row.get("mint_out"))
    if not mint:
        continue
    groups.setdefault(mint, []).append(row)

metadata = fetch_metadata(groups.keys()) if groups else {}
ata_rent_lamports = fetch_ata_rent_lamports()
ata_total_cost_lamports = ata_rent_lamports + ata_create_fee_estimate_lamports

items = []
for mint, rows in groups.items():
    meta = metadata.get(mint) or {"mint": mint, "metadata_available": False, "extensions": []}
    policy_verdict, policy_pass = policy_for_mint(meta)
    denylist_status = "denylisted" if mint in denylist_mints else "not_denylisted"
    ev_values = [safe_int(row.get("matched_rollout_net_ev_after_fee_tip_raw")) for row in rows]
    total_ev = sum(ev_values)
    median_ev = int(statistics.median(ev_values)) if ev_values else 0
    p90_ev = percentile(ev_values, 0.90)
    best_ev = max(ev_values or [0])
    min_out_values = [safe_int(row.get("min_out_raw")) for row in rows]
    expected_values = [safe_int(row.get("expected_out_raw")) for row in rows]
    full_band_amount = max(max(min_out_values or [0]), max(expected_values or [0]))
    exposure_amount = sum(v for v in expected_values if v > 0)
    decimals = meta.get("decimals")
    unit_amount = 10 ** decimals if isinstance(decimals, int) and 0 <= decimals <= 18 else 0
    unit_exit = quote(mint, USDC_MINT, unit_amount) if unit_amount else {"ok": False, "value_usdc": None, "reason": "no_unit_amount"}
    full_exit = quote(mint, USDC_MINT, full_band_amount) if full_band_amount else {"ok": False, "value_usdc": None, "reason": "no_full_band_amount"}
    exposure_exit = quote(mint, USDC_MINT, exposure_amount) if exposure_amount else {"ok": False, "value_usdc": None, "reason": "no_exposure_amount"}
    unit_value = safe_float(unit_exit.get("value_usdc"))
    full_value = safe_float(full_exit.get("value_usdc"))
    exposure_value = safe_float(exposure_exit.get("value_usdc"))
    exit_quote_ok = bool(full_exit.get("ok"))
    exit_dust = not (exit_quote_ok and full_value >= min_exit_value_usdc)
    exact_positive_count = sum(1 for row in rows if row.get("exact_quote_match"))
    matched_positive_count = sum(1 for row in rows if row.get("matched_rollout_net_ev_after_fee_tip_positive"))
    synthetic_delta = len(rows)
    total_ev_after_rent = total_ev - ata_total_cost_lamports
    best_row_after_rent = best_ev - ata_total_cost_lamports
    estimated_break_even_rows = math.ceil(ata_total_cost_lamports / median_ev) if median_ev > 0 else None
    recurrence_count = len(rows)
    fresh_epoch_seen_count = 1 if rows else 0
    recurring_or_strong = (
        recurrence_count >= min_recurrence_count
        or fresh_epoch_seen_count >= min_fresh_epoch_seen_count + 1
        or best_ev >= unusually_strong_row_ev_lamports
    )

    if not policy_pass:
        decision = "REJECT_POLICY"
    elif not exit_quote_ok:
        decision = "RETRY_LATER"
    elif exit_dust:
        decision = "REJECT_DUST_EXIT"
    elif synthetic_delta <= 0 or exact_positive_count <= 0:
        decision = "PARK"
    elif total_ev_after_rent <= 0 and best_row_after_rent <= 0:
        decision = "REJECT_LOW_EV_AFTER_RENT"
    elif not recurring_or_strong:
        decision = "REJECT_ONE_OFF"
    else:
        decision = "PRECREATE_REVIEW"

    items.append({
        "mint": mint,
        "token_program": meta.get("token_program"),
        "decimals": decimals,
        "extensions": meta.get("extensions") or [],
        "mint_authority": meta.get("mint_authority"),
        "freeze_authority": meta.get("freeze_authority"),
        "policy_verdict": policy_verdict,
        "policy_pass": policy_pass,
        "denylist_status": denylist_status,
        "row_count": len(rows),
        "exact_positive_row_count": exact_positive_count,
        "synthetic_ata_would_fire_delta": synthetic_delta,
        "matched_rollout_positive_count": matched_positive_count,
        "total_net_ev_after_fee_tip": total_ev,
        "median_net_ev_after_fee_tip": median_ev,
        "p90_net_ev_after_fee_tip": p90_ev,
        "best_row_ev": best_ev,
        "total_net_ev_after_ata_cost_lamports": total_ev_after_rent,
        "best_row_ev_after_ata_cost_lamports": best_row_after_rent,
        "unit_exit_probe_value_usdc": round_or_none(unit_value, 9),
        "unit_exit_probe_reason": unit_exit.get("reason"),
        "full_band_exit_probe_value_usdc": round_or_none(full_value, 9),
        "full_band_exit_probe_reason": full_exit.get("reason"),
        "exit_quote_ok": exit_quote_ok,
        "exit_dust": exit_dust,
        "expected_output_exposure_usdc": round_or_none(exposure_value, 9),
        "expected_output_exposure_reason": exposure_exit.get("reason"),
        "unit_exit_probe_raw": unit_amount,
        "full_band_exit_probe_raw": full_band_amount,
        "expected_output_exposure_raw": exposure_amount,
        "ata_create_rent_lamports": ata_rent_lamports,
        "ata_create_fee_estimate_lamports": ata_create_fee_estimate_lamports,
        "estimated_break_even_rows": estimated_break_even_rows,
        "recurrence_count": recurrence_count,
        "fresh_epoch_seen_count": fresh_epoch_seen_count,
        "sample_candidate_id": text(rows[0].get("candidate_id")) if rows else None,
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
    })

decision_rank = {
    "PRECREATE_REVIEW": 6,
    "RETRY_LATER": 5,
    "REJECT_LOW_EV_AFTER_RENT": 4,
    "REJECT_ONE_OFF": 3,
    "PARK": 2,
    "REJECT_DUST_EXIT": 1,
    "REJECT_POLICY": 0,
}
items.sort(
    key=lambda item: (
        decision_rank.get(item["decision"], -1),
        item["synthetic_ata_would_fire_delta"],
        item["total_net_ev_after_ata_cost_lamports"],
        item["best_row_ev"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, items)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run66r_rows": str(source_rows_path),
    "source_exact_positive_overlay_rows": len(source_rows),
    "ata_only_convertible_rows": len(convertible_rows),
    "ata_only_convertible_group_count": len(items),
    "precreate_review_group_count": sum(1 for item in items if item["decision"] == "PRECREATE_REVIEW"),
    "precreate_review_row_delta": sum(item["synthetic_ata_would_fire_delta"] for item in items if item["decision"] == "PRECREATE_REVIEW"),
    "decision_counts": dict(Counter(item["decision"] for item in items)),
    "total_synthetic_ata_would_fire_delta": sum(item["synthetic_ata_would_fire_delta"] for item in items),
    "total_net_ev_after_fee_tip": sum(item["total_net_ev_after_fee_tip"] for item in items),
    "total_net_ev_after_ata_cost_lamports": sum(item["total_net_ev_after_ata_cost_lamports"] for item in items),
    "ata_create_rent_lamports": ata_rent_lamports,
    "ata_create_fee_estimate_lamports": ata_create_fee_estimate_lamports,
    "min_exit_value_usdc": min_exit_value_usdc,
    "wire_send": 0,
    "live_landed": 0,
    "top_items": items[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_run66r_rows={source_rows_path}",
    f"source_exact_positive_overlay_rows={summary['source_exact_positive_overlay_rows']}",
    f"ata_only_convertible_rows={summary['ata_only_convertible_rows']}",
    f"ata_only_convertible_group_count={summary['ata_only_convertible_group_count']}",
    f"precreate_review_group_count={summary['precreate_review_group_count']}",
    f"precreate_review_row_delta={summary['precreate_review_row_delta']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"total_synthetic_ata_would_fire_delta={summary['total_synthetic_ata_would_fire_delta']}",
    f"total_net_ev_after_fee_tip={summary['total_net_ev_after_fee_tip']}",
    f"total_net_ev_after_ata_cost_lamports={summary['total_net_ev_after_ata_cost_lamports']}",
    f"ata_create_rent_lamports={summary['ata_create_rent_lamports']}",
    f"ata_create_fee_estimate_lamports={summary['ata_create_fee_estimate_lamports']}",
    "wire_send=0",
    "live_landed=0",
]
if items:
    top = items[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_policy_verdict={top['policy_verdict']}",
        f"top_denylist_status={top['denylist_status']}",
        f"top_row_count={top['row_count']}",
        f"top_synthetic_ata_would_fire_delta={top['synthetic_ata_would_fire_delta']}",
        f"top_exact_positive_row_count={top['exact_positive_row_count']}",
        f"top_matched_rollout_positive_count={top['matched_rollout_positive_count']}",
        f"top_total_net_ev_after_fee_tip={top['total_net_ev_after_fee_tip']}",
        f"top_total_net_ev_after_ata_cost_lamports={top['total_net_ev_after_ata_cost_lamports']}",
        f"top_median_net_ev_after_fee_tip={top['median_net_ev_after_fee_tip']}",
        f"top_p90_net_ev_after_fee_tip={top['p90_net_ev_after_fee_tip']}",
        f"top_best_row_ev={top['best_row_ev']}",
        f"top_unit_exit_probe_value_usdc={top['unit_exit_probe_value_usdc']}",
        f"top_full_band_exit_probe_value_usdc={top['full_band_exit_probe_value_usdc']}",
        f"top_expected_output_exposure_usdc={top['expected_output_exposure_usdc']}",
        f"top_exit_quote_ok={str(top['exit_quote_ok']).lower()}",
        f"top_exit_dust={str(top['exit_dust']).lower()}",
        f"top_estimated_break_even_rows={top['estimated_break_even_rows']}",
        f"top_recurrence_count={top['recurrence_count']}",
        f"top_fresh_epoch_seen_count={top['fresh_epoch_seen_count']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66R ATA-only convertible planner written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

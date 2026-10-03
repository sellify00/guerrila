#!/usr/bin/env bash
# 65ab_epoch_unlock_planner.sh - blocker-ranked ATA and held-input unlock planner for a Run65 epoch

source "$(dirname "$0")/lib.sh"
script_header "65AB - Epoch Unlock Planner"

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
STEP="${SHADOW_RUN65AB_STEP:-65ab_epoch_unlock_planner}"
STATUS_PREFIX="${SHADOW_RUN65AB_STATUS_PREFIX:-run65ab_epoch_unlock_planner}"
REPORT_BASENAME="${SHADOW_RUN65AB_REPORT_BASENAME:-run65ab_epoch_unlock_planner}"

RUN65_STEP="${SHADOW_RUN65AB_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"

RPC_URL="${SHADOW_RUN65AB_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
QUOTE_API_URL="${SHADOW_RUN65AB_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN65AB_MIN_RECURRENCE_COUNT:-2}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN65AB_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_ENTRY_VALUE_USDC="${SHADOW_RUN65AB_MIN_ENTRY_VALUE_USDC:-1.00}"
MAX_POLICY_MINTS="${SHADOW_RUN65AB_MAX_POLICY_MINTS:-80}"
QUOTE_SLIPPAGE_BPS="${SHADOW_RUN65AB_QUOTE_SLIPPAGE_BPS:-50}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN65AB_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN65AB_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN65AB_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
DENYLIST_MINTS="${SHADOW_RUN65AB_DENYLIST_MINTS:-}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
RUN65A_ROWS="$RESULT_DIR/run65a_account_ata_unlock_rows.jsonl"
RUN65A_SUMMARY="$RESULT_DIR/run65a_account_ata_unlock_summary.json"
RUN65B_ROWS="$RESULT_DIR/run65b_exit_qualified_held_input_overlay_rows.jsonl"
RUN65B_SUMMARY="$RESULT_DIR/run65b_exit_qualified_held_input_overlay_summary.json"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$RUN65A_ROWS" "$RUN65A_SUMMARY" "$RUN65B_ROWS" "$RUN65B_SUMMARY"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - \
    "$RUN65_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$RUN65A_ROWS" \
    "$RUN65A_SUMMARY" \
    "$RUN65B_ROWS" \
    "$RUN65B_SUMMARY" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_RECURRENCE_COUNT" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_ENTRY_VALUE_USDC" \
    "$MAX_POLICY_MINTS" \
    "$QUOTE_SLIPPAGE_BPS" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$DENYLIST_MINTS" <<'PY'
import base64
import json
import math
import statistics
import sys
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
TOKEN2022_EXTENSION_NAMES = {
    1: "transferFeeConfig",
    3: "mintCloseAuthority",
    4: "confidentialTransferMint",
    6: "defaultAccountState",
    9: "nonTransferable",
    10: "interestBearingConfig",
    12: "permanentDelegate",
    14: "transferHook",
    16: "confidentialTransferFeeConfig",
    18: "metadataPointer",
    19: "tokenMetadata",
}
METADATA_ONLY_EXTENSIONS = {"metadataPointer", "tokenMetadata"}

run65_json = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
run65a_rows_path = Path(sys.argv[4])
run65a_summary_path = Path(sys.argv[5])
run65b_rows_path = Path(sys.argv[6])
run65b_summary_path = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
rpc_url = sys.argv[10]
quote_api_url = sys.argv[11]
min_recurrence_count = int(sys.argv[12] or "2")
min_exit_value_usdc = float(sys.argv[13] or "0.10")
min_entry_value_usdc = float(sys.argv[14] or "1.00")
max_policy_mints = int(sys.argv[15] or "80")
quote_slippage_bps = int(sys.argv[16] or "50")
allow_freeze_authority = (sys.argv[17] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[18] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[19] or "").lower() in {"1", "true", "yes", "on"}
denylist_mints = {item.strip() for item in (sys.argv[20] or "").split(",") if item.strip()}


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


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


def percentile(values, q):
    if not values:
        return 0
    values = sorted(values)
    index = int(math.ceil(q * len(values))) - 1
    return values[max(0, min(index, len(values) - 1))]


run65 = load_json(run65_json)
run65_inputs = run65.get("inputs") or {}
candidate_path = Path(run65_inputs.get("run39_candidate_autopsy") or "")
if not candidate_path.exists():
    raise SystemExit(f"candidate_autopsy missing: {candidate_path}")
rows = load_jsonl(candidate_path)
sol_usd_equivalent = safe_float(((load_json(Path(run65_inputs.get("run61_json") or run65_json)).get("inputs") or {}).get("sol_usd_equivalent")), 1000.0) or 1000.0


def markout_best_raw(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    value = safe_int(markouts.get("best_horizon_net_after_fee_lamports"))
    if value > 0:
        return value
    return max(0, safe_int(row.get("net_after_fee_lamports")))


def markout_best_horizon(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return text(markouts.get("best_horizon")) or "none"


def normalize_raw(raw, mint, decimals=None):
    raw = safe_int(raw)
    if mint == WSOL_MINT:
        return raw / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return raw / 1_000_000
    if isinstance(decimals, int) and 0 <= decimals <= 18:
        return raw / (10 ** decimals)
    return None


def sol_equiv_known(raw, mint):
    if mint == WSOL_MINT:
        return safe_int(raw) / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return (safe_int(raw) / 1_000_000) / sol_usd_equivalent
    return None


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
        return {
            "mint": mint,
            "metadata_available": False,
            "token_program": None,
            "decimals": None,
            "extensions": [],
            "mint_authority": None,
            "freeze_authority": None,
        }
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
    return {
        "mint": mint,
        "metadata_available": True,
        "token_program": owner,
        "decimals": decimals,
        "extensions": sorted(set(extensions)),
        "mint_authority": mint_auth,
        "freeze_authority": freeze_auth,
    }


def fetch_mint_metadata(mints):
    mints = [mint for mint in sorted(set(mints)) if mint]
    result = {}
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
        return {"ok": False, "reason": "amount_zero"}
    key = (input_mint, output_mint, amount_raw)
    if key in quote_cache:
        return quote_cache[key]
    if input_mint == output_mint:
        if output_mint in {USDC_MINT, USDT_MINT}:
            value_usdc = amount_raw / 1_000_000
        elif output_mint == WSOL_MINT:
            value_usdc = amount_raw / 1_000_000_000 * sol_usd_equivalent
        else:
            value_usdc = None
        out = {"ok": value_usdc is not None, "out_amount": amount_raw, "exit_value_usdc": value_usdc, "reason": "same_mint"}
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
        if output_mint in {USDC_MINT, USDT_MINT}:
            value_usdc = out_amount / 1_000_000
        elif output_mint == WSOL_MINT:
            value_usdc = out_amount / 1_000_000_000 * sol_usd_equivalent
        else:
            value_usdc = None
        out = {"ok": out_amount > 0, "out_amount": out_amount, "exit_value_usdc": value_usdc, "reason": "quote_ok" if out_amount > 0 else "quote_zero"}
    except Exception as exc:
        out = {"ok": False, "out_amount": 0, "exit_value_usdc": None, "reason": f"quote_error:{type(exc).__name__}"}
    quote_cache[key] = out
    return out


def policy_for_mint(mint, metadata):
    if mint in denylist_mints:
        return "denylisted", False
    if not metadata.get("metadata_available"):
        return "metadata_unavailable", False
    token_program = metadata.get("token_program")
    extensions = set(metadata.get("extensions") or [])
    if metadata.get("freeze_authority") and not allow_freeze_authority:
        return "freeze_authority_present", False
    if metadata.get("mint_authority") and not allow_mint_authority:
        return "mint_authority_present", False
    if token_program == SPL_TOKEN_PROGRAM:
        return "ok", True
    if token_program == TOKEN2022_PROGRAM:
        if extensions.issubset(METADATA_ONLY_EXTENSIONS) and allow_metadata_only_token2022:
            return "metadata_only_token2022_allowed", True
        if extensions.issubset(METADATA_ONLY_EXTENSIONS):
            return "metadata_only_token2022_requires_explicit_allow", False
        return "unsupported_token2022_extensions", False
    return "unsupported_token_program", False


def owner(row):
    return text(row.get("selected_owner")).lower() or "drop"


def route_family(row):
    explicit = text(row.get("target_route_family")).lower()
    if explicit:
        return "public_rpc_scout" if explicit in {"public", "public_rpc"} else explicit
    tokens = f"{text(row.get('route')).lower()} {text(row.get('route_reason')).lower()}"
    if "jito" in tokens:
        return "jito_bundle"
    if "harmonic" in tokens:
        return "harmonic_bundle"
    if "tpu" in tokens:
        return "tpu_quic_public"
    return text(row.get("route")).lower() or "unknown"


def row_sol_ev(row, metadata_by_mint=None):
    raw = markout_best_raw(row)
    mint = text(row.get("mint_in"))
    known = sol_equiv_known(raw, mint)
    if known is not None:
        return known
    metadata = (metadata_by_mint or {}).get(mint) or {}
    exit_quote = quote(mint, USDC_MINT, raw) if raw > 0 else {"ok": False}
    if exit_quote.get("exit_value_usdc") is not None:
        return exit_quote["exit_value_usdc"] / sol_usd_equivalent
    return 0.0


ata_groups = {}
held_groups = {}

for row in rows:
    row_owner = owner(row)
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    first_stage = text(row.get("first_rejection_stage")).lower()
    blocker = text(row.get("execution_blocker")).lower()
    positive = bool(row.get("positive_ev")) or markout_best_raw(row) > 0
    if not positive:
        continue
    if first_stage == "output_account_not_ready" or blocker in {"missing_output_ata", "output_ata_uninitialized"}:
        key = (mint_out, row_owner, text(row.get("pool")), text(row.get("venue")), route_family(row))
        group = ata_groups.setdefault(key, {"rows": [], "trade_sizes": []})
        group["rows"].append(row)
        group["trade_sizes"].append(safe_int(row.get("trade_size_lamports")))
    if first_stage == "not_held_input" or not bool(row.get("approved_held_input")) or not bool(row.get("input_balance_ok")):
        key = (mint_in, row_owner, text(row.get("pool")), text(row.get("venue")), route_family(row))
        group = held_groups.setdefault(key, {"rows": [], "trade_sizes": []})
        group["rows"].append(row)
        group["trade_sizes"].append(safe_int(row.get("trade_size_lamports")))

rank_seed = Counter()
for key, group in ata_groups.items():
    rank_seed[key[0]] += len(group["rows"])
for key, group in held_groups.items():
    rank_seed[key[0]] += len(group["rows"])
top_mints = [mint for mint, _ in rank_seed.most_common(max_policy_mints)]
metadata_by_mint = fetch_mint_metadata(top_mints)


def group_sol_ev(group):
    return sum(row_sol_ev(row, metadata_by_mint) for row in group["rows"])


def quote_probe_amount(metadata, value_usdc=None):
    mint = metadata.get("mint")
    decimals = metadata.get("decimals")
    if mint == USDC_MINT:
        return int((value_usdc or min_exit_value_usdc) * 1_000_000)
    if mint == USDT_MINT:
        return int((value_usdc or min_exit_value_usdc) * 1_000_000)
    if mint == WSOL_MINT:
        return int(((value_usdc or min_exit_value_usdc) / sol_usd_equivalent) * 1_000_000_000)
    if isinstance(decimals, int) and 0 <= decimals <= 18:
        return 10 ** decimals
    return 0


ata_items = []
for key, group in ata_groups.items():
    mint, row_owner, pool, venue, route = key
    metadata = metadata_by_mint.get(mint) or {"mint": mint, "metadata_available": False}
    policy_verdict, policy_ok = policy_for_mint(mint, metadata)
    exit_probe_raw = quote_probe_amount(metadata)
    exit_quote = quote(mint, USDC_MINT, exit_probe_raw) if exit_probe_raw else {"ok": False, "exit_value_usdc": None, "reason": "no_probe_amount"}
    exit_quote_ok = bool(exit_quote.get("ok")) and safe_float(exit_quote.get("exit_value_usdc")) >= min_exit_value_usdc
    missed_ev = group_sol_ev(group)
    ata_items.append({
        "mint": mint,
        "owner_mode": row_owner,
        "pool": pool,
        "venue": venue,
        "route_family": route,
        "frequency": len(group["rows"]),
        "missed_ev_sol_equivalent": round(missed_ev, 9),
        "policy_verdict": policy_verdict,
        "token_program": metadata.get("token_program"),
        "extensions": metadata.get("extensions") or [],
        "freeze_authority": metadata.get("freeze_authority"),
        "mint_authority": metadata.get("mint_authority"),
        "exit_quote_ok": exit_quote_ok,
        "exit_quote_value": exit_quote.get("exit_value_usdc"),
        "exit_quote_reason": exit_quote.get("reason"),
        "precreate_allowed": bool(policy_ok and exit_quote_ok),
        "expected_would_fire_delta": len(group["rows"]),
        "best_horizon_counts": dict(Counter(markout_best_horizon(row) for row in group["rows"])),
        "sample_candidate_id": text(group["rows"][0].get("candidate_id")),
    })

ata_items.sort(key=lambda item: (item["precreate_allowed"], item["missed_ev_sol_equivalent"], item["frequency"]), reverse=True)

held_items = []
for key, group in held_groups.items():
    mint, row_owner, pool, venue, route = key
    metadata = metadata_by_mint.get(mint) or {"mint": mint, "metadata_available": False}
    policy_verdict, policy_ok = policy_for_mint(mint, metadata)
    sizes = [value for value in group["trade_sizes"] if value > 0]
    required_raw_p50 = int(statistics.median(sizes)) if sizes else 0
    required_raw_p90 = percentile(sizes, 0.90)
    required_raw_max = max(sizes) if sizes else 0
    exit_quote = quote(mint, USDC_MINT, required_raw_p90) if required_raw_p90 else {"ok": False, "exit_value_usdc": None, "reason": "no_required_amount"}
    entry_quote = quote(USDC_MINT, mint, int(min_entry_value_usdc * 1_000_000))
    exit_quote_ok = bool(exit_quote.get("ok")) and safe_float(exit_quote.get("exit_value_usdc")) >= min_exit_value_usdc
    entry_quote_ok = bool(entry_quote.get("ok"))
    missed_ev = group_sol_ev(group)
    recurrence_ok = len(group["rows"]) >= min_recurrence_count
    overlay_allowed = bool(policy_ok and recurrence_ok and entry_quote_ok and exit_quote_ok)
    held_items.append({
        "mint": mint,
        "owner_mode": row_owner,
        "pool": pool,
        "venue": venue,
        "route_family": route,
        "frequency": len(group["rows"]),
        "missed_ev_sol_equivalent": round(missed_ev, 9),
        "policy_verdict": policy_verdict,
        "token_program": metadata.get("token_program"),
        "extensions": metadata.get("extensions") or [],
        "freeze_authority": metadata.get("freeze_authority"),
        "mint_authority": metadata.get("mint_authority"),
        "entry_quote_ok": entry_quote_ok,
        "entry_quote_reason": entry_quote.get("reason"),
        "exit_quote_ok": exit_quote_ok,
        "exit_quote_value": exit_quote.get("exit_value_usdc"),
        "exit_quote_reason": exit_quote.get("reason"),
        "required_input_raw_p50": required_raw_p50,
        "required_input_raw_p90": required_raw_p90,
        "required_input_raw_max": required_raw_max,
        "recurrence_ok": recurrence_ok,
        "held_input_overlay_allowed": overlay_allowed,
        "expected_would_fire_delta": len(group["rows"]) if overlay_allowed else 0,
        "best_horizon_counts": dict(Counter(markout_best_horizon(row) for row in group["rows"])),
        "sample_candidate_id": text(group["rows"][0].get("candidate_id")),
    })

held_items.sort(key=lambda item: (item["held_input_overlay_allowed"], item["missed_ev_sol_equivalent"], item["frequency"]), reverse=True)


def write_jsonl(path, items):
    with Path(path).open("w", encoding="utf-8") as fh:
        for item in items:
            fh.write(json.dumps(item, sort_keys=True) + "\n")


write_jsonl(run65a_rows_path, ata_items)
write_jsonl(run65b_rows_path, held_items)

ata_summary = {
    "status": "run65a_account_ata_unlock_planner_complete",
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "account_ata_group_count": len(ata_items),
    "account_ata_row_count": sum(item["frequency"] for item in ata_items),
    "precreate_allowed_group_count": sum(1 for item in ata_items if item["precreate_allowed"]),
    "precreate_allowed_row_delta": sum(item["expected_would_fire_delta"] for item in ata_items if item["precreate_allowed"]),
    "precreate_allowed_missed_ev_sol_equivalent": round(sum(item["missed_ev_sol_equivalent"] for item in ata_items if item["precreate_allowed"]), 9),
    "top_items": ata_items[:20],
}
held_summary = {
    "status": "run65b_exit_qualified_held_input_overlay_complete",
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "held_input_group_count": len(held_items),
    "held_input_row_count": sum(item["frequency"] for item in held_items),
    "held_input_overlay_allowed_group_count": sum(1 for item in held_items if item["held_input_overlay_allowed"]),
    "held_input_overlay_expected_would_fire_delta": sum(item["expected_would_fire_delta"] for item in held_items),
    "held_input_overlay_allowed_missed_ev_sol_equivalent": round(sum(item["missed_ev_sol_equivalent"] for item in held_items if item["held_input_overlay_allowed"]), 9),
    "top_items": held_items[:20],
}
payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run65_json": str(run65_json),
    "candidate_autopsy_path": str(candidate_path),
    "policy": {
        "min_recurrence_count": min_recurrence_count,
        "min_exit_value_usdc": min_exit_value_usdc,
        "min_entry_value_usdc": min_entry_value_usdc,
        "max_policy_mints": max_policy_mints,
        "allow_freeze_authority": allow_freeze_authority,
        "allow_mint_authority": allow_mint_authority,
        "allow_metadata_only_token2022": allow_metadata_only_token2022,
        "denylist_mint_count": len(denylist_mints),
    },
    "run65a": ata_summary,
    "run65b": held_summary,
}

run65a_summary_path.write_text(json.dumps(ata_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
run65b_summary_path.write_text(json.dumps(held_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={payload['status']}",
    f"source_run65_json={run65_json}",
    f"candidate_autopsy_path={candidate_path}",
    f"run65a_account_ata_group_count={ata_summary['account_ata_group_count']}",
    f"run65a_account_ata_row_count={ata_summary['account_ata_row_count']}",
    f"run65a_precreate_allowed_group_count={ata_summary['precreate_allowed_group_count']}",
    f"run65a_precreate_allowed_row_delta={ata_summary['precreate_allowed_row_delta']}",
    f"run65a_precreate_allowed_missed_ev_sol_equivalent={ata_summary['precreate_allowed_missed_ev_sol_equivalent']}",
    f"run65b_held_input_group_count={held_summary['held_input_group_count']}",
    f"run65b_held_input_row_count={held_summary['held_input_row_count']}",
    f"run65b_overlay_allowed_group_count={held_summary['held_input_overlay_allowed_group_count']}",
    f"run65b_overlay_expected_would_fire_delta={held_summary['held_input_overlay_expected_would_fire_delta']}",
    f"run65b_overlay_allowed_missed_ev_sol_equivalent={held_summary['held_input_overlay_allowed_missed_ev_sol_equivalent']}",
]
if ata_items:
    top = ata_items[0]
    lines.extend([
        f"run65a_top_mint={top['mint']}",
        f"run65a_top_owner_mode={top['owner_mode']}",
        f"run65a_top_pool={top['pool']}",
        f"run65a_top_frequency={top['frequency']}",
        f"run65a_top_missed_ev_sol_equivalent={top['missed_ev_sol_equivalent']}",
        f"run65a_top_policy_verdict={top['policy_verdict']}",
        f"run65a_top_precreate_allowed={str(top['precreate_allowed']).lower()}",
    ])
if held_items:
    top = held_items[0]
    lines.extend([
        f"run65b_top_mint={top['mint']}",
        f"run65b_top_owner_mode={top['owner_mode']}",
        f"run65b_top_pool={top['pool']}",
        f"run65b_top_frequency={top['frequency']}",
        f"run65b_top_missed_ev_sol_equivalent={top['missed_ev_sol_equivalent']}",
        f"run65b_top_policy_verdict={top['policy_verdict']}",
        f"run65b_top_overlay_allowed={str(top['held_input_overlay_allowed']).lower()}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65AB unlock planner written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

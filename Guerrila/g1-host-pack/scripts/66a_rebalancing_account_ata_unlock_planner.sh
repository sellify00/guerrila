#!/usr/bin/env bash
# 66a_rebalancing_account_ata_unlock_planner.sh - conservative ATA/account planner for rebalancing rows

source "$(dirname "$0")/lib.sh"
script_header "66A - Rebalancing Account/ATA Unlock Planner"

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
STEP="${SHADOW_RUN66A_STEP:-66a_rebalancing_account_ata_unlock_planner}"
STATUS_PREFIX="${SHADOW_RUN66A_STATUS_PREFIX:-run66a_rebalancing_account_ata_unlock_planner}"
REPORT_BASENAME="${SHADOW_RUN66A_REPORT_BASENAME:-run66a_rebalancing_account_ata_unlock_planner}"
RUN65_STEP="${SHADOW_RUN66A_RUN65_STEP:-65_full_shadow_hybrid_private_epoch_20260524_5m_valuation}"
RUN66Q_STEP="${SHADOW_RUN66A_RUN66Q_STEP:-66q_rebalancing_quote_minout_preservation_20260524_5m}"
RUN66Q2_STEP="${SHADOW_RUN66A_RUN66Q2_STEP:-}"
RUN65_JSON="$RESULTS_ROOT/$RUN65_STEP/run65_full_potential_continuous_shard_simulation.json"
RUN66Q_ROWS="$RESULTS_ROOT/$RUN66Q_STEP/run66q_rebalancing_quote_minout_rows.jsonl"
RUN66Q2_ROWS="${SHADOW_RUN66A_RUN66Q2_ROWS:-}"
if [[ -z "$RUN66Q2_ROWS" && -n "$RUN66Q2_STEP" ]]; then
    RUN66Q2_ROWS="$RESULTS_ROOT/$RUN66Q2_STEP/run66q2_rebalancing_exact_quote_route_ev_rows.jsonl"
fi

RPC_URL="${SHADOW_RUN66A_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
QUOTE_API_URL="${SHADOW_RUN66A_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN66A_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN66A_MIN_RECURRENCE_COUNT:-2}"
QUOTE_SLIPPAGE_BPS="${SHADOW_RUN66A_QUOTE_SLIPPAGE_BPS:-50}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN66A_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN66A_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN66A_ALLOW_MINT_AUTHORITY:-false}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66a_rebalancing_account_ata_unlock_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN65_JSON" ]]; then
    fail "Missing Run65 JSON: $RUN65_JSON"
    finish
    exit 1
fi

python3 - \
    "$RUN65_JSON" \
    "$RUN66Q_ROWS" \
    "$RUN66Q2_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_RECURRENCE_COUNT" \
    "$QUOTE_SLIPPAGE_BPS" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" <<'PY'
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

run65_json = Path(sys.argv[1])
run66q_rows_path = Path(sys.argv[2])
run66q2_rows_arg = sys.argv[3]
run66q2_rows_path = Path(run66q2_rows_arg) if run66q2_rows_arg else None
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
rows_jsonl = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
rpc_url = sys.argv[9]
quote_api_url = sys.argv[10]
min_exit_value_usdc = float(sys.argv[11] or "0.10")
min_recurrence_count = int(sys.argv[12] or "2")
quote_slippage_bps = int(sys.argv[13] or "50")
allow_metadata_only_token2022 = sys.argv[14].lower() == "true"
allow_freeze_authority = sys.argv[15].lower() == "true"
allow_mint_authority = sys.argv[16].lower() == "true"


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    if not Path(path).exists():
        return rows
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
    return round(value, digits)


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


def markouts(row):
    value = row.get("markouts")
    return value if isinstance(value, dict) else {}


def best_horizon_raw(row):
    return safe_int(markouts(row).get("best_horizon_net_after_fee_lamports"))


def row_sol(raw, mint):
    if mint == WSOL_MINT:
        return safe_int(raw) / 1_000_000_000
    if mint in {USDC_MINT, USDT_MINT}:
        return safe_int(raw) / 1_000_000 / 1000.0
    return 0.0


run65 = load_json(run65_json)
candidate_path = Path((run65.get("inputs") or {}).get("run39_candidate_autopsy") or "")
if not candidate_path.exists():
    raise SystemExit(f"candidate_autopsy missing: {candidate_path}")
candidate_rows = load_jsonl(candidate_path)
qrows = {text(row.get("candidate_id")): row for row in load_jsonl(run66q_rows_path)}
q2rows = {text(row.get("candidate_id")): row for row in load_jsonl(run66q2_rows_path)} if run66q2_rows_path and run66q2_rows_path.exists() else {}

groups = {}
for row in candidate_rows:
    if text(row.get("selected_owner")).lower() != "rebalancing":
        continue
    if not (text(row.get("first_rejection_stage")).lower() == "output_account_not_ready" or text(row.get("execution_blocker")).lower() in {"missing_output_ata", "output_ata_uninitialized"} or not bool(row.get("output_account_ready"))):
        continue
    mint = text(row.get("mint_out"))
    key = (mint, text(row.get("pool")), text(row.get("venue")), text(row.get("target_route_family") or row.get("route")))
    groups.setdefault(key, []).append(row)

metadata = fetch_metadata([key[0] for key in groups])
items = []
for (mint, pool, venue, route), rows in groups.items():
    meta = metadata.get(mint) or {"mint": mint, "metadata_available": False, "extensions": []}
    policy_verdict, policy_pass = policy_for_mint(meta)
    candidate_ids = [text(row.get("candidate_id")) for row in rows]
    q_for_group = [qrows[cid] for cid in candidate_ids if cid in qrows]
    q2_for_group = [q2rows[cid] for cid in candidate_ids if cid in q2rows]
    quote_positive_rows = sum(1 for q in q2_for_group if q.get("quote_numeric_preserved"))
    net_ev_positive_rows = sum(1 for q in q2_for_group if q.get("net_ev_after_fee_tip_positive"))
    quote_overlay_delta = sum(1 for q in q2_for_group if q.get("quote_numeric_preserved") and q.get("net_ev_after_fee_tip_positive"))
    full_band_amount = max([safe_int(q.get("min_out_raw") or q.get("expected_out_raw")) for q in q_for_group] or [0])
    decimals = meta.get("decimals")
    unit_amount = 10 ** decimals if isinstance(decimals, int) and 0 <= decimals <= 18 else 0
    unit_exit = quote(mint, USDC_MINT, unit_amount) if unit_amount else {"ok": False, "value_usdc": None, "reason": "no_unit_amount"}
    full_exit = quote(mint, USDC_MINT, full_band_amount) if full_band_amount else {"ok": False, "value_usdc": None, "reason": "no_full_band_amount"}
    unit_value = safe_float(unit_exit.get("value_usdc"))
    full_value = safe_float(full_exit.get("value_usdc"))
    unit_exit_non_dust = bool(unit_exit.get("ok")) and unit_value >= min_exit_value_usdc
    full_band_exit_non_dust = bool(full_exit.get("ok")) and full_value >= min_exit_value_usdc
    exit_non_dust = full_band_exit_non_dust
    strict_ev = sum(row_sol(safe_int(row.get("net_after_fee_lamports")), text(row.get("mint_in"))) for row in rows)
    later_ev = sum(row_sol(best_horizon_raw(row), text(row.get("mint_in"))) for row in rows)
    recurrence = len(rows) >= min_recurrence_count
    if policy_pass and exit_non_dust and recurrence and (strict_ev > 0 or later_ev > 0):
        decision = "PRECREATE_ALLOWED"
    elif not policy_pass:
        decision = "REJECT_POLICY"
    elif full_band_amount == 0:
        decision = "RETRY_LATER"
    elif not exit_non_dust:
        decision = "REJECT_DUST_EXIT"
    else:
        decision = "PARK_NO_PRECREATE"
    if decision == "PRECREATE_ALLOWED" and q2rows and net_ev_positive_rows > 0:
        ata_review_decision = "ONE_ATA_PLAN_REVIEW"
    elif decision == "PRECREATE_ALLOWED":
        ata_review_decision = "PARK_SIM_ONLY"
    else:
        ata_review_decision = decision
    items.append({
        "mint": mint,
        "token_program": meta.get("token_program"),
        "extensions": meta.get("extensions") or [],
        "freeze_authority": meta.get("freeze_authority"),
        "mint_authority": meta.get("mint_authority"),
        "owner_mode": "rebalancing",
        "pool": pool,
        "venue": venue,
        "route_family": route,
        "row_count": len(rows),
        "strict_ev_sol_equivalent": round_or_none(strict_ev, 9),
        "later_horizon_ev_sol_equivalent": round_or_none(later_ev, 9),
        "exit_quote_ok": bool(exit_non_dust),
        "unit_exit_non_dust": unit_exit_non_dust,
        "full_band_exit_non_dust": full_band_exit_non_dust,
        "unit_exit_probe_value_usdc": round_or_none(unit_value, 9),
        "unit_exit_probe_reason": unit_exit.get("reason"),
        "full_band_exit_probe_value_usdc": round_or_none(full_value, 9),
        "full_band_exit_probe_reason": full_exit.get("reason"),
        "full_band_probe_raw": full_band_amount,
        "policy_verdict": policy_verdict,
        "policy_pass": policy_pass,
        "precreate_allowed": decision == "PRECREATE_ALLOWED",
        "expected_would_fire_delta": len(rows) if decision == "PRECREATE_ALLOWED" else 0,
        "precreate_candidate_quote_positive_rows": quote_positive_rows,
        "precreate_candidate_net_ev_after_fee_tip_positive_rows": net_ev_positive_rows,
        "precreate_candidate_expected_would_fire_delta_after_quote_overlay": quote_overlay_delta if decision == "PRECREATE_ALLOWED" else 0,
        "ata_review_decision": ata_review_decision,
        "recurrence": recurrence,
        "decision": decision,
        "sample_candidate_id": candidate_ids[0] if candidate_ids else None,
    })

items.sort(key=lambda item: (item["precreate_allowed"], item["later_horizon_ev_sol_equivalent"] or 0, item["row_count"]), reverse=True)
write_jsonl(rows_jsonl, items)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run65_json": str(run65_json),
    "source_run66q_rows": str(run66q_rows_path),
    "source_run66q2_rows": str(run66q2_rows_path) if run66q2_rows_path else "",
    "candidate_autopsy_path": str(candidate_path),
    "rebalancing_ata_group_count": len(items),
    "rebalancing_ata_row_count": sum(item["row_count"] for item in items),
    "precreate_allowed_group_count": sum(1 for item in items if item["precreate_allowed"]),
    "precreate_allowed_row_delta": sum(item["expected_would_fire_delta"] for item in items),
    "precreate_candidate_quote_positive_rows": sum(item["precreate_candidate_quote_positive_rows"] for item in items if item["precreate_allowed"]),
    "precreate_candidate_net_ev_after_fee_tip_positive_rows": sum(item["precreate_candidate_net_ev_after_fee_tip_positive_rows"] for item in items if item["precreate_allowed"]),
    "precreate_candidate_expected_would_fire_delta_after_quote_overlay": sum(item["precreate_candidate_expected_would_fire_delta_after_quote_overlay"] for item in items if item["precreate_allowed"]),
    "ata_review_decision_counts": dict(Counter(item["ata_review_decision"] for item in items)),
    "decision_counts": dict(Counter(item["decision"] for item in items)),
    "top_items": items[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_run65_json={run65_json}",
    f"source_run66q_rows={run66q_rows_path}",
    f"source_run66q2_rows={run66q2_rows_path if run66q2_rows_path else ''}",
    f"rebalancing_ata_group_count={summary['rebalancing_ata_group_count']}",
    f"rebalancing_ata_row_count={summary['rebalancing_ata_row_count']}",
    f"precreate_allowed_group_count={summary['precreate_allowed_group_count']}",
    f"precreate_allowed_row_delta={summary['precreate_allowed_row_delta']}",
    f"precreate_candidate_quote_positive_rows={summary['precreate_candidate_quote_positive_rows']}",
    f"precreate_candidate_net_ev_after_fee_tip_positive_rows={summary['precreate_candidate_net_ev_after_fee_tip_positive_rows']}",
    f"precreate_candidate_expected_would_fire_delta_after_quote_overlay={summary['precreate_candidate_expected_would_fire_delta_after_quote_overlay']}",
    f"ata_review_decision_counts={json.dumps(summary['ata_review_decision_counts'], sort_keys=True)}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
]
if items:
    top = items[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_ata_review_decision={top['ata_review_decision']}",
        f"top_policy_verdict={top['policy_verdict']}",
        f"top_precreate_candidate_quote_positive_rows={top['precreate_candidate_quote_positive_rows']}",
        f"top_precreate_candidate_net_ev_after_fee_tip_positive_rows={top['precreate_candidate_net_ev_after_fee_tip_positive_rows']}",
        f"top_precreate_candidate_expected_would_fire_delta_after_quote_overlay={top['precreate_candidate_expected_would_fire_delta_after_quote_overlay']}",
        f"top_unit_exit_probe_value_usdc={top['unit_exit_probe_value_usdc']}",
        f"top_full_band_exit_probe_value_usdc={top['full_band_exit_probe_value_usdc']}",
        f"top_later_horizon_ev_sol_equivalent={top['later_horizon_ev_sol_equivalent']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66A rebalancing ATA unlock planner written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

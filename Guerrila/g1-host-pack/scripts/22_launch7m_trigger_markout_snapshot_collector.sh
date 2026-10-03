#!/usr/bin/env bash
# 22_launch7m_trigger_markout_snapshot_collector.sh — capture short-horizon markout snapshots for Launch Continuation V2 trigger rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch7M — Trigger Markout Snapshot Collector"

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
STEP="${SHADOW_LAUNCH7M_STEP:-22_launch7m_trigger_markout_snapshot_collector}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launch7m_trigger_markout_snapshot_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launch7m_trigger_markout_snapshot_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PRESSURE_ROWS_PATH="${SHADOW_LAUNCH7M_PRESSURE_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
GATE_ROWS_PATH="${SHADOW_LAUNCH7M_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
CENSUS_ROWS_PATH="${SHADOW_LAUNCH7M_CENSUS_ROWS_PATH:-$RESULTS_ROOT/22_launch0_new_mint_pool_census/launch0_new_mint_pool_census_rows.jsonl}"
RPC_URL="${SHADOW_LAUNCH7M_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

TRIGGER_MODE="${SHADOW_LAUNCH7M_TRIGGER_MODE:-B}"
HORIZONS_SECONDS="${SHADOW_LAUNCH7M_HORIZONS_SECONDS:-1,2,5,15,30}"
MAX_ROWS="${SHADOW_LAUNCH7M_MAX_ROWS:-25}"
MAX_SPEND_SOL="${SHADOW_LAUNCH7M_MAX_SPEND_SOL:-0.003}"
SLIPPAGE_BPS="${SHADOW_LAUNCH7M_SLIPPAGE_BPS:-100}"
ENTRY_TIP_LAMPORTS="${SHADOW_LAUNCH7M_ENTRY_TIP_LAMPORTS:-200000}"
EXIT_TIP_LAMPORTS="${SHADOW_LAUNCH7M_EXIT_TIP_LAMPORTS:-200000}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCH7M_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCH7M_PRIORITY_FEE_LAMPORTS:-0}"

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PRESSURE_ROWS_PATH" ]] || PRESSURE_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"
[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$CENSUS_ROWS_PATH" ]] || CENSUS_ROWS_PATH="$(latest_rows_for_step launch0_new_mint_pool_census_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PRESSURE_ROWS_PATH" "$GATE_ROWS_PATH" "$CENSUS_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$RPC_URL" "$TRIGGER_MODE" "$HORIZONS_SECONDS" "$MAX_ROWS" "$MAX_SPEND_SOL" "$SLIPPAGE_BPS" "$ENTRY_TIP_LAMPORTS" "$EXIT_TIP_LAMPORTS" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import time
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

(
    pressure_rows_arg,
    gate_rows_arg,
    census_rows_arg,
    rows_out_arg,
    summary_json_arg,
    summary_txt_arg,
    rpc_url,
    trigger_mode_arg,
    horizons_arg,
    max_rows_arg,
    max_spend_sol_arg,
    slippage_bps_arg,
    entry_tip_arg,
    exit_tip_arg,
    base_fee_arg,
    priority_fee_arg,
) = sys.argv[1:17]

pressure_rows_path = Path(pressure_rows_arg) if pressure_rows_arg else Path("")
gate_rows_path = Path(gate_rows_arg) if gate_rows_arg else Path("")
census_rows_path = Path(census_rows_arg) if census_rows_arg else Path("")
rows_out_path = Path(rows_out_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
trigger_mode = str(trigger_mode_arg or "B").strip().upper()
horizons = sorted({int(part.strip()) for part in str(horizons_arg or "1,2,5,15,30").split(",") if part.strip()})
max_rows = max(0, int(float(max_rows_arg or 25)))
max_spend_sol = float(max_spend_sol_arg or 0.003)
slippage_bps = int(float(slippage_bps_arg or 100))
entry_tip_lamports = int(float(entry_tip_arg or 200000))
exit_tip_lamports = int(float(exit_tip_arg or 200000))
base_fee_lamports = int(float(base_fee_arg or 5000))
priority_fee_lamports = int(float(priority_fee_arg or 0))

WSOL_MINT = "So11111111111111111111111111111111111111112"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def safe_float(value: Any, default: float = 0.0) -> float:
    try:
        if value in (None, ""):
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def lamports_to_sol(value: int) -> float:
    return safe_int(value) / 1_000_000_000.0


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if str(path) in {"", "."} or not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def http_post_json(url: str, payload: dict[str, Any]):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        context = ssl.create_default_context() if url.startswith("https://") else None
        with urllib.request.urlopen(request, timeout=30, context=context) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return None, json.loads(body)
        except Exception:
            return None, {"error": body, "http_status": exc.code}
    except Exception as exc:  # noqa: BLE001
        return None, {"error": str(exc)}


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def b58encode(raw: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    zeros = 0
    for byte in raw:
        if byte == 0:
            zeros += 1
        else:
            break
    num = int.from_bytes(raw, "big")
    chars: list[str] = []
    while num > 0:
        num, rem = divmod(num, 58)
        chars.append(alphabet[rem])
    return ("1" * zeros) + "".join(reversed(chars or ["1"]))


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    if error is not None:
        return {}, 0, text(error.get("message") if isinstance(error, dict) else error) or "rpc_error"
    result = (response or {}).get("result", response) or {}
    context = result.get("context") or {}
    slot = safe_int(context.get("slot"))
    values = result.get("value") or []
    accounts: dict[str, dict[str, Any]] = {}
    for pubkey, value in zip(pubkeys, values):
        if not value:
            accounts[pubkey] = {"exists": False, "data_raw": b""}
            continue
        data_field = value.get("data") if isinstance(value, dict) else None
        raw = b""
        if isinstance(data_field, list) and data_field:
            try:
                raw = base64.b64decode(str(data_field[0]), validate=False)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return accounts, slot, None


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) < PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pool_state_missing"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pumpfun_pool_decode_failed"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return 0
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


def load_pumpfun_state(pool: str, launch_mint: str) -> tuple[dict[str, Any], str | None]:
    accounts, slot, error = get_multiple_accounts([pool])
    if error:
        return {}, "pool_state_missing"
    pool_doc, pool_error = decode_pumpfun_pool((accounts.get(pool) or {}).get("data_raw", b""))
    if pool_error or not pool_doc:
        return {}, pool_error or "pumpfun_pool_decode_failed"
    if launch_mint not in {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} or WSOL_MINT not in {pool_doc.get("base_mint"), pool_doc.get("quote_mint")}:
        return {}, "pool_mint_set_mismatch"
    vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
    if vault_error:
        return {}, "vault_state_missing"
    base_reserve_raw = decode_vault_balance((vaults.get(pool_doc["base_vault"]) or {}).get("data_raw", b""))
    quote_reserve_raw = decode_vault_balance((vaults.get(pool_doc["quote_vault"]) or {}).get("data_raw", b""))
    if pool_doc["base_mint"] == WSOL_MINT:
        core_reserve_raw = base_reserve_raw
        launch_reserve_raw = quote_reserve_raw
    else:
        core_reserve_raw = quote_reserve_raw
        launch_reserve_raw = base_reserve_raw
    return {
        "slot": slot,
        "pool": pool,
        "core_reserve_raw": core_reserve_raw,
        "launch_reserve_raw": launch_reserve_raw,
        "base_mint": pool_doc["base_mint"],
        "quote_mint": pool_doc["quote_mint"],
    }, None


def constant_product_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0:
        return 0
    denominator = reserve_in + amount_in
    if denominator <= 0:
        return 0
    return (amount_in * reserve_out) // denominator


def min_out_from_expected(expected_out: int, slip_bps: int) -> int:
    if expected_out <= 0:
        return 0
    bounded = min(max(slip_bps, 0), 10_000)
    return max(0, (expected_out * (10_000 - bounded)) // 10_000)


def trigger_b_passes(row: dict[str, Any]) -> tuple[bool, str]:
    if text(row.get("pair_class")) != "core_launch_pair":
        return False, "unsupported_pair_class"
    if boolish(row.get("creator_sell_detected")):
        return False, "creator_sell_detected"
    if boolish(row.get("insider_cluster_sell_seen")):
        return False, "insider_cluster_sell_seen"
    if safe_int(row.get("buy_count_1s")) < 1:
        return False, "buy_count_1s_below_threshold"
    if safe_int(row.get("unique_buyers_2s")) < 1:
        return False, "unique_buyers_2s_below_threshold"
    if safe_float(row.get("net_buy_pressure")) <= 0:
        return False, "sell_pressure_dominant"
    if boolish(row.get("first_sell_seen")):
        return False, "sell_seen_before_entry"
    return True, "none"


pressure_rows = load_jsonl(pressure_rows_path)
gate_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(gate_rows_path) if text(row.get("launch_group_id"))}
census_by_group: dict[str, list[dict[str, Any]]] = defaultdict(list)
for row in load_jsonl(census_rows_path):
    group_id = text(row.get("launch_group_id"))
    if group_id:
        census_by_group[group_id].append(row)
for rows in census_by_group.values():
    rows.sort(key=lambda item: (safe_int(item.get("target_seen_preprocessed_ns")), text(item.get("target_signature"))))

selected: list[dict[str, Any]] = []
for row in pressure_rows:
    passed, _ = trigger_b_passes(row)
    if not passed:
        continue
    selected.append(row)
selected.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), safe_int(row.get("time_since_launch_ms")), text(row.get("launch_group_id"))))
if max_rows > 0:
    selected = selected[:max_rows]

planned_spend_lamports = sol_to_lamports(max_spend_sol)
cycle_cost_lamports = entry_tip_lamports + exit_tip_lamports + (2 * base_fee_lamports) + (2 * priority_fee_lamports)

rows: list[dict[str, Any]] = []
candidate_blockers: Counter[str] = Counter()
selected_by_group: dict[str, dict[str, Any]] = {}

for row in selected:
    group_id = text(row.get("launch_group_id"))
    gate_row = gate_by_group.get(group_id, {})
    venue = text(row.get("venue")) or text(gate_row.get("venue"))
    blocker = "none"
    planned_inventory_token_out_raw = safe_int(gate_row.get("entry_min_out_raw")) or safe_int(gate_row.get("entry_expected_out_raw"))
    if venue != "pumpfun_amm":
        blocker = "route_family_unsupported"
    elif not boolish(gate_row.get("mint_policy_pass")):
        blocker = text(gate_row.get("mint_policy_blocker")) or "policy_failed"
    elif not boolish(gate_row.get("r1_exit_nonzero_size_band_ready")):
        blocker = text(gate_row.get("first_gate_blocker")) or "exit_quote_expected_out_zero"
    elif planned_inventory_token_out_raw <= 0:
        blocker = "planned_inventory_token_zero"

    trigger_events = census_by_group.get(group_id, [])
    first_sell_after_trigger_ms = None
    if trigger_events:
        trigger_seen_ns = max(safe_int(event.get("target_seen_preprocessed_ns")) for event in trigger_events)
        for event in trigger_events:
            if safe_int(event.get("target_seen_preprocessed_ns")) <= trigger_seen_ns:
                continue
            if text(event.get("direction_class")) == "sell_launch_to_core":
                first_sell_after_trigger_ms = max(0, (safe_int(event.get("target_seen_preprocessed_ns")) - trigger_seen_ns) // 1_000_000)
                break

    selected_by_group[group_id] = {
        "pressure_row": row,
        "gate_row": gate_row,
        "blocker": blocker,
        "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
        "first_sell_after_trigger_ms": first_sell_after_trigger_ms,
        "snapshots": {},
    }
    candidate_blockers[blocker] += 1

start = time.monotonic()
horizon_states: dict[int, dict[str, dict[str, Any]]] = {}

for horizon in horizons:
    wait_for = start + horizon - time.monotonic()
    if wait_for > 0:
        time.sleep(wait_for)
    states_for_horizon: dict[str, dict[str, Any]] = {}
    for group_id, candidate in selected_by_group.items():
        gate_row = candidate["gate_row"]
        pressure_row = candidate["pressure_row"]
        pool = text(pressure_row.get("pool")) or text(gate_row.get("pool"))
        launch_mint = text(pressure_row.get("launch_mint")) or text(gate_row.get("launch_mint"))
        planned_inventory_token_out_raw = safe_int(candidate["planned_inventory_token_out_raw"])
        if candidate["blocker"] != "none":
            states_for_horizon[group_id] = {"snapshot_status": candidate["blocker"]}
            continue
        state, state_error = load_pumpfun_state(pool, launch_mint)
        if state_error:
            states_for_horizon[group_id] = {"snapshot_status": state_error}
            continue
        exit_expected_out_raw = constant_product_out(safe_int(state.get("launch_reserve_raw")), safe_int(state.get("core_reserve_raw")), planned_inventory_token_out_raw)
        exit_min_out_raw = min_out_from_expected(exit_expected_out_raw, slippage_bps)
        after_cost_lamports = exit_expected_out_raw - planned_spend_lamports - cycle_cost_lamports
        return_bps = ((exit_expected_out_raw - planned_spend_lamports) / planned_spend_lamports * 10_000.0) if planned_spend_lamports > 0 else None
        after_cost_return_bps = (after_cost_lamports / planned_spend_lamports * 10_000.0) if planned_spend_lamports > 0 else None
        states_for_horizon[group_id] = {
            "snapshot_status": "ok",
            "snapshot_slot": safe_int(state.get("slot")),
            "snapshot_core_reserve_raw": safe_int(state.get("core_reserve_raw")),
            "snapshot_launch_reserve_raw": safe_int(state.get("launch_reserve_raw")),
            "exit_expected_out_raw": exit_expected_out_raw,
            "exit_min_out_raw": exit_min_out_raw,
            "exit_quote_available": exit_expected_out_raw > 0 and exit_min_out_raw > 0,
            "return_bps": round(return_bps, 6) if return_bps is not None else None,
            "after_cost_return_bps": round(after_cost_return_bps, 6) if after_cost_return_bps is not None else None,
            "after_cost_lamports": after_cost_lamports,
        }
    horizon_states[horizon] = states_for_horizon


def pick_snapshot(group_id: str, horizon: int) -> dict[str, Any]:
    return (horizon_states.get(horizon) or {}).get(group_id) or {}


for group_id, candidate in selected_by_group.items():
    pressure_row = candidate["pressure_row"]
    gate_row = candidate["gate_row"]
    planned_inventory_token_out_raw = safe_int(candidate["planned_inventory_token_out_raw"])
    entry_expected_out_raw = safe_int(gate_row.get("entry_expected_out_raw"))
    entry_min_out_raw = safe_int(gate_row.get("entry_min_out_raw"))
    trigger_exit_expected_out_raw = safe_int(gate_row.get("exit_expected_out_raw"))
    trigger_exit_min_out_raw = safe_int(gate_row.get("exit_min_out_raw"))
    snapshot_rows = {h: pick_snapshot(group_id, h) for h in horizons}
    available_horizons = [h for h, snap in snapshot_rows.items() if boolish(snap.get("exit_quote_available"))]
    returns_5s = snapshot_rows.get(5, {}).get("return_bps")
    returns_15s = snapshot_rows.get(15, {}).get("return_bps")
    returns_30s = snapshot_rows.get(30, {}).get("return_bps")
    after_cost_positive = any(safe_int(snap.get("after_cost_lamports")) > 0 for snap in snapshot_rows.values())
    max_snapshot_exit = max((safe_int(snap.get("exit_expected_out_raw")) for snap in snapshot_rows.values()), default=0)
    first_sell_after_trigger_ms = candidate.get("first_sell_after_trigger_ms")
    max_price_before_first_sell = None
    if first_sell_after_trigger_ms is not None:
        eligible = [safe_int(snapshot_rows[h].get("exit_expected_out_raw")) for h in horizons if h * 1000 <= safe_int(first_sell_after_trigger_ms) and safe_int(snapshot_rows[h].get("exit_expected_out_raw")) > 0]
        if eligible:
            max_price_before_first_sell = max(eligible)

    rows.append(
        {
            "record_type": "launch7m_trigger_markout_snapshot_row",
            "launch_group_id": group_id,
            "candidate_id": text(pressure_row.get("candidate_id")) or text(gate_row.get("candidate_id")),
            "trigger_mode": trigger_mode,
            "trigger_reference_ns": safe_int(pressure_row.get("latest_seen_preprocessed_ns")) or safe_int(pressure_row.get("target_seen_preprocessed_ns")),
            "trigger_reference_source": "launch1_latest_group_row",
            "venue": text(pressure_row.get("venue")) or text(gate_row.get("venue")),
            "pool": text(pressure_row.get("pool")) or text(gate_row.get("pool")),
            "launch_mint": text(pressure_row.get("launch_mint")) or text(gate_row.get("launch_mint")),
            "core_mint": text(pressure_row.get("core_mint")) or text(gate_row.get("core_mint")),
            "latest_signature": text(pressure_row.get("latest_signature")) or text(gate_row.get("latest_signature")),
            "buy_pressure_score": safe_float(pressure_row.get("buy_pressure_score")),
            "buy_count_1s": safe_int(pressure_row.get("buy_count_1s")),
            "unique_buyers_2s": safe_int(pressure_row.get("unique_buyers_2s")),
            "net_buy_pressure": safe_float(pressure_row.get("net_buy_pressure")),
            "first_sell_seen_before_entry": boolish(pressure_row.get("first_sell_seen")),
            "policy_pass": boolish(gate_row.get("mint_policy_pass")),
            "exit_ladder_pass": boolish(gate_row.get("r1_exit_nonzero_size_band_ready")),
            "planned_spend_lamports": planned_spend_lamports,
            "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
            "entry_expected_out_raw": entry_expected_out_raw,
            "entry_min_out_raw": entry_min_out_raw,
            "price_at_trigger": trigger_exit_expected_out_raw,
            "price_at_first_sell": None,
            "max_price_before_first_sell": max_price_before_first_sell,
            "max_snapshot_exit_expected_out_raw": max_snapshot_exit,
            "first_sell_after_trigger_ms": first_sell_after_trigger_ms,
            "max_adverse_excursion_bps": min(
                [safe_float(snap.get("after_cost_return_bps")) for snap in snapshot_rows.values() if snap.get("after_cost_return_bps") is not None],
                default=None,
            ),
            "after_cost_positive": after_cost_positive,
            "collector_blocker": candidate["blocker"],
            "exit_quote_available_at_each_horizon": {str(h): boolish(snapshot_rows[h].get("exit_quote_available")) for h in horizons},
            "return_1s": snapshot_rows.get(1, {}).get("return_bps"),
            "return_2s": snapshot_rows.get(2, {}).get("return_bps"),
            "return_5s": returns_5s,
            "return_15s": returns_15s,
            "return_30s": returns_30s,
            "after_cost_return_1s": snapshot_rows.get(1, {}).get("after_cost_return_bps"),
            "after_cost_return_2s": snapshot_rows.get(2, {}).get("after_cost_return_bps"),
            "after_cost_return_5s": snapshot_rows.get(5, {}).get("after_cost_return_bps"),
            "after_cost_return_15s": snapshot_rows.get(15, {}).get("after_cost_return_bps"),
            "after_cost_return_30s": snapshot_rows.get(30, {}).get("after_cost_return_bps"),
            "price_after_1s": safe_int(snapshot_rows.get(1, {}).get("exit_expected_out_raw")),
            "price_after_2s": safe_int(snapshot_rows.get(2, {}).get("exit_expected_out_raw")),
            "price_after_5s": safe_int(snapshot_rows.get(5, {}).get("exit_expected_out_raw")),
            "price_after_15s": safe_int(snapshot_rows.get(15, {}).get("exit_expected_out_raw")),
            "price_after_30s": safe_int(snapshot_rows.get(30, {}).get("exit_expected_out_raw")),
            "wire_send": 0,
            "live_landed": 0,
            "sendBundle_called": False,
        }
    )

rows.sort(key=lambda row: (-safe_float(row.get("buy_pressure_score")), text(row.get("launch_group_id"))))
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")


def median_of(key: str):
    values = [safe_float(row.get(key)) for row in rows if row.get(key) is not None]
    if not values:
        return None
    values.sort()
    n = len(values)
    mid = n // 2
    if n % 2 == 1:
        return round(values[mid], 6)
    return round((values[mid - 1] + values[mid]) / 2.0, 6)


def p75_of(key: str):
    values = [safe_float(row.get(key)) for row in rows if row.get(key) is not None]
    if not values:
        return None
    values.sort()
    idx = min(len(values) - 1, max(0, int(round(0.75 * (len(values) - 1)))))
    return round(values[idx], 6)


summary = {
    "status": "launch7m_trigger_markout_snapshot_complete" if rows else "launch7m_trigger_markout_snapshot_no_rows",
    "trigger_mode": trigger_mode,
    "trigger_B_rows": len(rows),
    "trigger_B_markout_available_rows": sum(1 for row in rows if any(bool(v) for v in (row.get("exit_quote_available_at_each_horizon") or {}).values())),
    "median_return_5s": median_of("return_5s"),
    "median_return_15s": median_of("return_15s"),
    "median_return_30s": median_of("return_30s"),
    "p75_return_5s": p75_of("return_5s"),
    "p75_return_15s": p75_of("return_15s"),
    "max_adverse_excursion_median": median_of("max_adverse_excursion_bps"),
    "first_sell_before_entry_count": sum(1 for row in rows if boolish(row.get("first_sell_seen_before_entry"))),
    "exit_available_5s_count": sum(1 for row in rows if safe_int(row.get("price_after_5s")) > 0),
    "exit_available_15s_count": sum(1 for row in rows if safe_int(row.get("price_after_15s")) > 0),
    "trigger_B_positive_after_cost_candidate_count": sum(1 for row in rows if boolish(row.get("after_cost_positive"))),
    "blocker_counts": dict(candidate_blockers),
    "wire_send": 0,
    "live_landed": 0,
    "sendBundle_called": False,
}
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "Collected Launch trigger markout snapshots into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

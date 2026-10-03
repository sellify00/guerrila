#!/usr/bin/env bash
# 21_lsprebrinv_r1_candidate_factory.sh — continuous no-send R1 candidate factory over preprocessed Backrun rows

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "21 LS-PRE-BR-INV-R1F — Candidate Factory"

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

WINDOW_SECONDS="${1:-${SHADOW_LSPREBRINV_FACTORY_WINDOW_SECONDS:-300}}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_LSPREBRINV_FACTORY_STEP:-21_lsprebrinv_r1_candidate_factory}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INPUT_MANIFEST="$RESULT_DIR/input_manifest.json"
ROWS_OUT="$RESULT_DIR/runlsprebrinv_r1_candidate_factory_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/runlsprebrinv_r1_candidate_factory_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"
MERGED_DIR="$RESULT_DIR/projection_merged"
MERGED_ROWS="$MERGED_DIR/runlsprebr0_projection_quote_all_rows.jsonl"
MERGED_SUMMARY="$MERGED_DIR/runlsprebr0_projection_quote_all_summary.json"

MAX_FACTORY_ROWS="${SHADOW_LSPREBRINV_FACTORY_MAX_ROWS:-50}"
MAX_CANDIDATES="${SHADOW_LSPREBRINV_FACTORY_MAX_CANDIDATES:-10}"
SIZE_LIST="${SHADOW_LSPREBRINV_FACTORY_SIZE_LIST:-0.005,0.01,0.025,0.05,0.1}"
SLIPPAGE_BPS="${SHADOW_LSPREBRINV_FACTORY_SLIPPAGE_BPS:-100}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LSPREBRINV_FACTORY_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LSPREBRINV_FACTORY_PRIORITY_FEE_LAMPORTS:-0}"
TIP_LAMPORTS="${SHADOW_LSPREBRINV_FACTORY_TIP_LAMPORTS:-0}"
RPC_URL="${SHADOW_LSPREBRINV_FACTORY_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"
LIVE_AUTOPSY_JSON_PATH="${SHADOW_LSPREBRINV_FACTORY_LIVE_AUTOPSY_JSON_PATH:-$RESULTS_ROOT/21_lsprebrinv_r1_live_pnl_autopsy/runlsprebrinv_r1_live_pnl_autopsy.json}"

mkdir -p "$RESULT_DIR" "$MERGED_DIR"
rm -f "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$MERGED_ROWS" "$MERGED_SUMMARY"
[[ -f "$LIVE_AUTOPSY_JSON_PATH" ]] || LIVE_AUTOPSY_JSON_PATH="$(find "$RESULTS_ROOT" -type f -name 'runlsprebrinv_r1_live_pnl_autopsy.json' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-)"

RUNLS13A_DIR="$RESULTS_ROOT/21_runls13a_preprocessed_backrun_target_census"
RUNLS13A_ROWS="$RUNLS13A_DIR/runls13a_preprocessed_backrun_target_census_rows.jsonl"
RUNLS13ALT_DIR="$RESULTS_ROOT/21_runls13alt_preprocessed_alt_resolution_audit"
RUNLS13ALT_ROWS="$RUNLS13ALT_DIR/runls13alt_preprocessed_alt_resolution_audit_rows.jsonl"
RUNLS13B_DIR="$RESULTS_ROOT/21_runls13b_preprocessed_backrun_selector"
RUNLS13B_ROWS="$RUNLS13B_DIR/runls13b_preprocessed_backrun_selector_rows.jsonl"
RUNLSPREBR0_DIR="$RESULTS_ROOT/21_runlsprebr0_preprocessed_projection_quote"
RUNLSPREBR0_ROWS="$RUNLSPREBR0_DIR/runlsprebr0_preprocessed_projection_quote_rows.jsonl"
RUNLSPREBR0M_DIR="$RESULTS_ROOT/21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote"
RUNLSPREBR0M_ROWS="$RUNLSPREBR0M_DIR/runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_rows.jsonl"
LSPREBRPF_DIR="$RESULTS_ROOT/21_lsprebrpf_pumpfun_amm_direct_route_probe"
LSPREBRPF_ROWS="$LSPREBRPF_DIR/runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl"

export SHADOW_RESULTS_DIR="$RESULTS_ROOT"
export SHADOW_RUNLS13A_RESULT_DIR="$RUNLS13A_DIR"
export SHADOW_RUNLS13U_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLS13U_INPUT_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLS13ALT_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLS13ALT_INPUT_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLS13B_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLS13B_INPUT_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLS13B_ALT_AUDIT_ROWS_PATH="$RUNLS13ALT_ROWS"
export SHADOW_RUNLSPREBR0_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLSPREBR0_SELECTOR_ROWS_PATH="$RUNLS13B_ROWS"
export SHADOW_RUNLSPREBR0_LS13A_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLSPREBR0_ALT_AUDIT_ROWS_PATH="$RUNLS13ALT_ROWS"
export SHADOW_RUNLSPREBR0M_RESULTS_ROOT="$RESULTS_ROOT"
export SHADOW_RUNLSPREBR0M_SELECTOR_ROWS_PATH="$RUNLS13B_ROWS"
export SHADOW_RUNLSPREBR0M_LS13A_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_RUNLSPREBR0M_ALT_AUDIT_ROWS_PATH="$RUNLS13ALT_ROWS"

export SHADOW_LSPREBR_INPUT_MODE="${SHADOW_LSPREBR_INPUT_MODE:-all}"
export SHADOW_RUNLSPREBR0_MAX_ROWS="$MAX_FACTORY_ROWS"
export SHADOW_RUNLSPREBR0M_MAX_ROWS="$MAX_FACTORY_ROWS"
export SHADOW_LSPREBRPF_MAX_ROWS="$MAX_FACTORY_ROWS"
export SHADOW_LSPREBRINV_MAX_ROWS="$MAX_FACTORY_ROWS"
bash "$PACK_DIR/scripts/21_runls13a_preprocessed_backrun_target_census.sh" "$WINDOW_SECONDS" || true
bash "$PACK_DIR/scripts/21_runls13u_preprocessed_unsupported_program_census.sh"
bash "$PACK_DIR/scripts/21_runls13alt_preprocessed_alt_resolution_audit.sh"
bash "$PACK_DIR/scripts/21_runls13b_preprocessed_backrun_selector.sh"
bash "$PACK_DIR/scripts/21_runlsprebr0_preprocessed_projection_quote.sh"
bash "$PACK_DIR/scripts/21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote.sh"

python3 - "$RESULTS_ROOT" "$MERGED_ROWS" "$MERGED_SUMMARY" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

results_root = Path(sys.argv[1])
merged_rows_path = Path(sys.argv[2])
merged_summary_path = Path(sys.argv[3])

paths = [
    results_root / "21_runlsprebr0_preprocessed_projection_quote" / "runlsprebr0_preprocessed_projection_quote_rows.jsonl",
    results_root / "21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote" / "runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_rows.jsonl",
]

rows = []
for path in paths:
    if not path.exists():
        continue
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))

merged_rows_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")

summary = {
    "status": "projection_merged",
    "rows_seen": len(rows),
    "source_paths": [str(path) for path in paths if path.exists()],
    "venue_counts": {},
}
venue_counts = {}
for row in rows:
    venue = str(row.get("venue") or "").strip()
    if not venue:
        continue
    venue_counts[venue] = venue_counts.get(venue, 0) + 1
summary["venue_counts"] = venue_counts
merged_summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

export SHADOW_LSPREBR2_PREBR0_ROWS_PATH="$MERGED_ROWS"
bash "$PACK_DIR/scripts/21_lsprebr2_preprocessed_backrun_executable_shape_filter.sh"
export SHADOW_LSPREBR3_PREBR2_ROWS_PATH="$RESULTS_ROOT/21_lsprebr2_preprocessed_backrun_executable_shape_filter/runlsprebr2_preprocessed_backrun_executable_shape_filter_rows.jsonl"
bash "$PACK_DIR/scripts/21_lsprebr3_preprocessed_backrun_direction_inventory_taxonomy.sh"

export SHADOW_LSPREBRPF_PREBR0_ROWS_PATH="$MERGED_ROWS"
export SHADOW_LSPREBRPF_LS13A_ROWS_PATH="$RUNLS13A_ROWS"
export SHADOW_LSPREBRPF_ALT_AUDIT_ROWS_PATH="$RUNLS13ALT_ROWS"
bash "$PACK_DIR/scripts/21_lsprebrpf_pumpfun_amm_direct_route_probe.sh"
export SHADOW_LSPREBRINV_DIRECT_ROWS_PATH="$LSPREBRPF_ROWS"
bash "$PACK_DIR/scripts/21_lsprebrinv_tiny_inventory_band_sim.sh"

python3 - "$RESULTS_ROOT" "$LIVE_AUTOPSY_JSON_PATH" "$INPUT_MANIFEST" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$MAX_CANDIDATES" "$SIZE_LIST" "$SLIPPAGE_BPS" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" "$TIP_LAMPORTS" "$RPC_URL" "$WINDOW_SECONDS" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    results_root_arg,
    live_autopsy_arg,
    manifest_arg,
    rows_out_arg,
    summary_arg,
    summary_txt_arg,
    max_candidates_arg,
    size_list_arg,
    slippage_bps_arg,
    base_tx_fee_lamports_arg,
    priority_fee_lamports_arg,
    tip_lamports_arg,
    rpc_url_arg,
    window_seconds_arg,
) = sys.argv[1:15]

results_root = Path(results_root_arg)
live_autopsy_path = Path(live_autopsy_arg) if live_autopsy_arg else Path("")
manifest_path = Path(manifest_arg)
rows_out_path = Path(rows_out_arg)
summary_path = Path(summary_arg)
summary_txt_path = Path(summary_txt_arg)
max_candidates = max(1, int(max_candidates_arg or "10"))
size_list = [item.strip() for item in str(size_list_arg or "").split(",") if item.strip()]
slippage_bps = max(0, int(float(slippage_bps_arg or "100")))
base_tx_fee_lamports = max(0, int(float(base_tx_fee_lamports_arg or "5000")))
priority_fee_lamports = max(0, int(float(priority_fee_lamports_arg or "0")))
tip_lamports = max(0, int(float(tip_lamports_arg or "0")))
rpc_url = str(rpc_url_arg or "").strip()
window_seconds = max(1, int(float(window_seconds_arg or "300")))

WSOL_MINT = "So11111111111111111111111111111111111111112"
PUMPFUN_AMM_ACCOUNT_LEN = 245
PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR = bytes([241, 154, 109, 4, 17, 177, 109, 188])
PUMPFUN_BASE_MINT_OFFSET = 8 + 35
PUMPFUN_QUOTE_MINT_OFFSET = 8 + 67
PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET = 8 + 131
PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET = 8 + 163
SPL_TOKEN_ACCOUNT_LEN = 165
TOKEN_AMOUNT_OFFSET = 64


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists() or not path.is_file():
        return []
    rows = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if raw:
            rows.append(json.loads(raw))
    return rows


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists() or not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return {}


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return text(value).lower() in {"1", "true", "yes", "y", "on", "ready", "pass", "passed"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


def sol_to_lamports(value: float) -> int:
    return max(0, int(round(float(value) * 1_000_000_000.0)))


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
    except Exception as exc:
        return None, {"error": str(exc)}


def rpc(method: str, params=None):
    if not rpc_url or "CHANGE_ME" in rpc_url or "${" in rpc_url:
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_post_json(rpc_url, payload)


def get_multiple_accounts(pubkeys: list[str]) -> tuple[dict[str, dict[str, Any]], int, str | None]:
    response, error = rpc("getMultipleAccounts", [pubkeys, {"encoding": "base64", "commitment": "processed"}])
    if error is not None:
        return {}, 0, "pool_state_missing"
    result = (response or {}).get("result") or {}
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
                raw = base64.b64decode(str(data_field[0]), validate=True)
            except Exception:
                raw = b""
        accounts[pubkey] = {
            "exists": True,
            "data_raw": raw,
            "lamports": safe_int(value.get("lamports")),
            "owner": text(value.get("owner")),
        }
    return accounts, slot, None


def b58encode(data: bytes) -> str:
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    num = int.from_bytes(data, "big")
    out = ""
    while num:
        num, rem = divmod(num, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    if pad or out:
        return alphabet[0] * pad + out
    return alphabet[0]


def decode_pumpfun_pool(raw: bytes) -> tuple[dict[str, Any] | None, str | None]:
    if len(raw) != PUMPFUN_AMM_ACCOUNT_LEN:
        return None, "pool_state_missing"
    if raw[:8] != PUMPFUN_AMM_ACCOUNT_DISCRIMINATOR:
        return None, "pool_decode_failed"
    return {
        "base_mint": b58encode(raw[PUMPFUN_BASE_MINT_OFFSET:PUMPFUN_BASE_MINT_OFFSET + 32]),
        "quote_mint": b58encode(raw[PUMPFUN_QUOTE_MINT_OFFSET:PUMPFUN_QUOTE_MINT_OFFSET + 32]),
        "base_vault": b58encode(raw[PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_BASE_TOKEN_ACCOUNT_OFFSET + 32]),
        "quote_vault": b58encode(raw[PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET:PUMPFUN_POOL_QUOTE_TOKEN_ACCOUNT_OFFSET + 32]),
    }, None


def decode_vault_balance(raw: bytes) -> int | None:
    if len(raw) < SPL_TOKEN_ACCOUNT_LEN:
        return None
    return int.from_bytes(raw[TOKEN_AMOUNT_OFFSET:TOKEN_AMOUNT_OFFSET + 8], "little")


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
    return max(0, (expected_out * max(0, 10_000 - slip_bps)) // 10_000)


def constant_product_out_with_fee(reserve_in: int, reserve_out: int, amount_in: int, fee_num: int, fee_den: int) -> int:
    if amount_in <= 0 or reserve_in <= 0 or reserve_out <= 0 or fee_den <= 0:
        return 0
    fee = ((amount_in * fee_num) + fee_den - 1) // fee_den
    amount_less_fee = max(0, amount_in - fee)
    denominator = reserve_in + amount_less_fee
    if denominator <= 0:
        return 0
    return (amount_less_fee * reserve_out) // denominator


direct_rows_path = results_root / "21_lsprebrpf_pumpfun_amm_direct_route_probe" / "runlsprebrpf_pumpfun_amm_direct_route_probe_rows.jsonl"
sim_rows_path = results_root / "21_lsprebrinv_tiny_inventory_band_sim" / "runlsprebrinv_tiny_inventory_band_sim_rows.jsonl"
prebr2_summary_path = results_root / "21_lsprebr2_preprocessed_backrun_executable_shape_filter" / "runlsprebr2_preprocessed_backrun_executable_shape_filter_summary.json"
prebr0_summary_path = results_root / "21_runlsprebr0_preprocessed_projection_quote" / "runlsprebr0_preprocessed_projection_quote_summary.json"
prebr0m_summary_path = results_root / "21_runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote" / "runlsprebr0m_preprocessed_meteora_damm_v2_projection_quote_summary.json"
ls13a_summary_path = results_root / "21_runls13a_preprocessed_backrun_target_census" / "runls13a_preprocessed_backrun_target_census_summary.json"
ls13b_summary_path = results_root / "21_runls13b_preprocessed_backrun_selector" / "runls13b_preprocessed_backrun_selector_summary.json"
merged_summary_path = results_root / "21_lsprebrinv_r1_candidate_factory" / "projection_merged" / "runlsprebr0_projection_quote_all_summary.json"
direct_summary_path = results_root / "21_lsprebrpf_pumpfun_amm_direct_route_probe" / "runlsprebrpf_pumpfun_amm_direct_route_probe_summary.json"
sim_summary_path = results_root / "21_lsprebrinv_tiny_inventory_band_sim" / "runlsprebrinv_tiny_inventory_band_sim_summary.json"

direct_rows = load_jsonl(direct_rows_path)
sim_rows = load_jsonl(sim_rows_path)
prebr2_summary = load_json(prebr2_summary_path)
prebr0_summary = load_json(prebr0_summary_path)
prebr0m_summary = load_json(prebr0m_summary_path)
ls13a_summary = load_json(ls13a_summary_path)
ls13b_summary = load_json(ls13b_summary_path)
merged_summary = load_json(merged_summary_path)
direct_summary = load_json(direct_summary_path)
sim_summary = load_json(sim_summary_path)
live_autopsy = load_json(live_autopsy_path)

hot_sender_tip_lamports = safe_int(live_autopsy.get("sell_sender_tip_lamports"), 200_000)
hot_sender_base_fee_lamports = safe_int(live_autopsy.get("sell_base_fee_lamports"), 5_000)
exit_variable_cost_lamports = safe_int(live_autopsy.get("exit_pumpfun_fee_lamports")) + safe_int(live_autopsy.get("exit_execution_shortfall_lamports"))
cheap_buy_base_fee_lamports = safe_int(live_autopsy.get("buy_base_fee_lamports"), 5_000)
cheap_buy_variable_cost_lamports = safe_int(live_autopsy.get("entry_pumpfun_fee_lamports")) + safe_int(live_autopsy.get("entry_execution_shortfall_lamports"))
pump_account_setup_lamports = safe_int(live_autopsy.get("pump_account_setup_lamports"), 1_844_400)

sim_by_candidate = {text(row.get("candidate_id")): row for row in sim_rows if text(row.get("candidate_id"))}

ready_rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()
size_ready_rows = 0
proposal_ready_rows = 0
ata_precreate_only_ready_rows = 0
one_mint_live_ready_rows = 0
inventory_sized_unwind_ev_positive_rows = 0
size_consistent_candidate_rows = 0

rows_scored = 0
for row in direct_rows:
    rows_scored += 1
    candidate_id = text(row.get("candidate_id"))
    sim_row = sim_by_candidate.get(candidate_id, {})
    exact_size_sol = float((sim_row or row).get("max_spend_sol") or 0.005)
    candidate_mint = text((sim_row or row).get("candidate_mint") or row.get("our_input_mint"))
    pool = text((sim_row or row).get("pool") or row.get("pool"))
    venue = text((sim_row or row).get("venue") or row.get("venue"))
    policy_pass = boolish((sim_row or row).get("policy_pass"))
    direct_entry_quote_ok = boolish((sim_row or row).get("direct_entry_quote_ok"))
    direct_exit_quote_ok = boolish((sim_row or row).get("direct_exit_quote_ok"))
    exit_non_dust = boolish((sim_row or row).get("exit_non_dust"))
    auto_flatten_available = boolish((sim_row or row).get("auto_flatten_available"))
    pm_reserve_inventory_ok = boolish((sim_row or row).get("pm_reserve_inventory_ok"))
    pm_release_ok = boolish((sim_row or row).get("pm_release_ok"))
    entry_output_account_exists = boolish((sim_row or row).get("entry_output_account_ready"))
    current_loss_lamports = safe_int((sim_row or row).get("immediate_roundtrip_loss_lamports"))

    size_decision = "SIZE_POLICY_FAIL"
    size_first_blocker = "not_pumpfun_candidate"
    best_ready_size_sol = "none"
    exact_entry_expected_out_raw = 0
    exact_entry_min_out_raw = 0
    exact_exit_expected_out_raw = 0
    exact_exit_min_out_raw = 0
    size_ladder_has_ready_size = False

    if venue == "pumpfun_amm" and candidate_mint and pool:
        accounts, _, state_error = get_multiple_accounts([pool])
        pool_account = accounts.get(pool, {})
        pool_doc, pool_error = decode_pumpfun_pool(pool_account.get("data_raw", b"")) if not state_error else (None, state_error)
        if pool_doc and {pool_doc.get("base_mint"), pool_doc.get("quote_mint")} == {candidate_mint, WSOL_MINT}:
            vaults, _, vault_error = get_multiple_accounts([pool_doc["base_vault"], pool_doc["quote_vault"]])
            if not vault_error:
                current_base_reserve_raw = decode_vault_balance(vaults.get(pool_doc["base_vault"], {}).get("data_raw", b"")) or 0
                current_quote_reserve_raw = decode_vault_balance(vaults.get(pool_doc["quote_vault"], {}).get("data_raw", b"")) or 0
                if pool_doc.get("base_mint") == WSOL_MINT and pool_doc.get("quote_mint") == candidate_mint:
                    entry_reserve_in_raw = current_base_reserve_raw
                    entry_reserve_out_raw = current_quote_reserve_raw
                    exit_reverse = True
                else:
                    entry_reserve_in_raw = current_quote_reserve_raw
                    entry_reserve_out_raw = current_base_reserve_raw
                    exit_reverse = False

                for size_text in size_list:
                    spend_lamports = sol_to_lamports(float(size_text))
                    entry_expected_out_raw = constant_product_out(entry_reserve_in_raw, entry_reserve_out_raw, spend_lamports)
                    entry_min_out_raw = min_out_from_expected(entry_expected_out_raw, slippage_bps)
                    if exit_reverse:
                        post_base_reserve_raw = current_base_reserve_raw + spend_lamports
                        post_quote_reserve_raw = max(0, current_quote_reserve_raw - entry_expected_out_raw)
                        exit_expected_out_raw = constant_product_out(post_quote_reserve_raw, post_base_reserve_raw, entry_min_out_raw)
                    else:
                        post_quote_reserve_raw = current_quote_reserve_raw + spend_lamports
                        post_base_reserve_raw = max(0, current_base_reserve_raw - entry_expected_out_raw)
                        exit_expected_out_raw = constant_product_out(post_base_reserve_raw, post_quote_reserve_raw, entry_min_out_raw)
                    exit_min_out_raw = min_out_from_expected(exit_expected_out_raw, slippage_bps)
                    if abs(float(size_text) - exact_size_sol) <= 1e-12:
                        exact_entry_expected_out_raw = entry_expected_out_raw
                        exact_entry_min_out_raw = entry_min_out_raw
                        exact_exit_expected_out_raw = exit_expected_out_raw
                        exact_exit_min_out_raw = exit_min_out_raw
                        if entry_expected_out_raw <= 0 or entry_min_out_raw <= 0:
                            size_decision = "SIZE_EXIT_ZERO"
                            size_first_blocker = "entry_quote_non_positive"
                        elif exit_expected_out_raw <= 0:
                            size_decision = "SIZE_EXIT_ZERO"
                            size_first_blocker = "exit_quote_expected_out_zero"
                        elif exit_min_out_raw <= 0 or exit_expected_out_raw <= 1:
                            size_decision = "SIZE_EXIT_DUST"
                            size_first_blocker = "exit_quote_dust"
                        else:
                            size_decision = "SIZE_EXIT_READY"
                            size_first_blocker = "none"
                    if entry_expected_out_raw > 0 and entry_min_out_raw > 0 and exit_expected_out_raw > 1 and exit_min_out_raw > 0:
                        size_ladder_has_ready_size = True
                        if best_ready_size_sol == "none":
                            best_ready_size_sol = size_text
        else:
            size_decision = "SIZE_POLICY_FAIL"
            size_first_blocker = pool_error or state_error or "pool_state_missing"

    if size_ladder_has_ready_size:
        size_ready_rows += 1

    planned_inventory_token_out_raw = exact_entry_min_out_raw or exact_entry_expected_out_raw
    projected_unwind_input_raw = safe_int(row.get("our_input_raw"))
    held_inventory_raw = max(
        safe_int(row.get("pumpfun_direct_input_actual_balance_raw")),
        safe_int(sim_row.get("exit_input_actual_balance_raw")),
        safe_int(sim_row.get("exit_input_balance_raw")),
    )
    projected_unwind_input_lte_planned_inventory = projected_unwind_input_raw > 0 and planned_inventory_token_out_raw > 0 and projected_unwind_input_raw <= planned_inventory_token_out_raw
    projected_unwind_input_lte_held_inventory = projected_unwind_input_raw > 0 and held_inventory_raw > 0 and projected_unwind_input_raw <= held_inventory_raw
    trade_fee_numerator = safe_int(row.get("trade_fee_numerator"))
    trade_fee_denominator = max(1, safe_int(row.get("trade_fee_denominator"), 1_000_000_000))
    mint_in = text(row.get("mint_in"))
    mint_out = text(row.get("mint_out"))
    if mint_in == WSOL_MINT:
        baseline_our_reserve_in = safe_int(row.get("pre_target_reserve_out_raw"))
        baseline_our_reserve_out = safe_int(row.get("pre_target_reserve_in_raw"))
        projected_our_reserve_in = safe_int(row.get("projected_reserve_out_raw"))
        projected_our_reserve_out = safe_int(row.get("projected_reserve_in_raw"))
    elif mint_out == WSOL_MINT:
        baseline_our_reserve_in = safe_int(row.get("pre_target_reserve_in_raw"))
        baseline_our_reserve_out = safe_int(row.get("pre_target_reserve_out_raw"))
        projected_our_reserve_in = safe_int(row.get("projected_reserve_in_raw"))
        projected_our_reserve_out = safe_int(row.get("projected_reserve_out_raw"))
    else:
        baseline_our_reserve_in = 0
        baseline_our_reserve_out = 0
        projected_our_reserve_in = 0
        projected_our_reserve_out = 0
    target_sized_unwind_ev_lamports = safe_int(row.get("net_ev_after_tip_raw")) + hot_sender_tip_lamports + hot_sender_base_fee_lamports
    hot_sender_exit_expected_wsol_raw = constant_product_out_with_fee(
        projected_our_reserve_in,
        projected_our_reserve_out,
        planned_inventory_token_out_raw,
        trade_fee_numerator,
        trade_fee_denominator,
    )
    hot_sender_exit_min_wsol_raw = min_out_from_expected(hot_sender_exit_expected_wsol_raw, slippage_bps)
    inventory_counterfactual_pre_target_wsol_raw = constant_product_out_with_fee(
        baseline_our_reserve_in,
        baseline_our_reserve_out,
        planned_inventory_token_out_raw,
        trade_fee_numerator,
        trade_fee_denominator,
    )
    inventory_sized_unwind_ev_lamports = (
        hot_sender_exit_expected_wsol_raw
        - inventory_counterfactual_pre_target_wsol_raw
        - hot_sender_tip_lamports
        - hot_sender_base_fee_lamports
    )
    per_cycle_cost_lamports = cheap_buy_base_fee_lamports + cheap_buy_variable_cost_lamports + hot_sender_tip_lamports + hot_sender_base_fee_lamports + exit_variable_cost_lamports
    setup_cost_lamports = 0 if entry_output_account_exists else pump_account_setup_lamports
    size_consistent_net_after_mode_c_costs = inventory_sized_unwind_ev_lamports - per_cycle_cost_lamports - setup_cost_lamports - 10_000
    size_consistency_pass = projected_unwind_input_lte_planned_inventory or projected_unwind_input_lte_held_inventory
    if inventory_sized_unwind_ev_lamports > 0:
        inventory_sized_unwind_ev_positive_rows += 1
    if size_consistency_pass and hot_sender_exit_expected_wsol_raw > 0 and hot_sender_exit_min_wsol_raw > 0 and size_consistent_net_after_mode_c_costs > 0:
        size_consistent_candidate_rows += 1

    if not policy_pass:
        decision = "PARK_POLICY"
        first_blocker = text((sim_row or row).get("policy_blocker")) or "policy_failed"
    elif not boolish(row.get("pumpfun_direct_route_exists")):
        decision = "PARK_DIRECT_ROUTE_STALE"
        first_blocker = text(row.get("first_direct_route_blocker")) or "direct_route_not_ready"
    elif not direct_entry_quote_ok:
        decision = "PARK_DIRECT_ENTRY_QUOTE_MISSING"
        first_blocker = "entry_quote_non_positive"
    elif not direct_exit_quote_ok:
        decision = "PARK_DIRECT_EXIT_QUOTE_MISSING"
        first_blocker = "exit_quote_non_positive"
    elif not exit_non_dust:
        decision = "PARK_EXIT_DUST"
        first_blocker = "exit_non_dust_false"
    elif not auto_flatten_available:
        decision = "PARK_AUTO_FLATTEN_UNAVAILABLE"
        first_blocker = "auto_flatten_unavailable"
    elif not pm_reserve_inventory_ok or not pm_release_ok:
        decision = "PARK_PM"
        first_blocker = "pm_reserve_or_release_failed"
    elif not size_consistency_pass:
        decision = "PARK_PROJECTED_EV_SIZE_INCONSISTENT"
        first_blocker = "projected_unwind_input_exceeds_available_inventory"
    elif hot_sender_exit_expected_wsol_raw <= 0 or hot_sender_exit_min_wsol_raw <= 0:
        decision = "PARK_INVENTORY_SIZED_EXIT_ZERO"
        first_blocker = "inventory_sized_exit_quote_zero"
    elif size_consistent_net_after_mode_c_costs <= 0:
        decision = "PARK_MODE_C_NEGATIVE"
        first_blocker = "size_consistent_net_after_mode_c_nonpositive"
    elif size_decision != "SIZE_EXIT_READY":
        decision = "PARK_EXIT_SIZE_BAND_NOT_READY"
        first_blocker = size_first_blocker
    elif not entry_output_account_exists:
        decision = "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY"
        first_blocker = "entry_output_account_missing_requires_precreate"
    else:
        decision = "R1_PROPOSAL_READY_ONE_MINT_LIVE"
        first_blocker = "none"

    decision_counts[decision] += 1
    if decision.startswith("R1_PROPOSAL_READY"):
        proposal_ready_rows += 1
    if decision == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY":
        ata_precreate_only_ready_rows += 1
    if decision == "R1_PROPOSAL_READY_ONE_MINT_LIVE":
        one_mint_live_ready_rows += 1

    ready_rows.append(
        {
            "record_type": "runlsprebrinv_r1_candidate_factory_row",
            "candidate_id": candidate_id,
            "mint": candidate_mint,
            "pool": pool,
            "venue": venue,
            "policy_pass": policy_pass,
            "direct_entry_quote_ok": direct_entry_quote_ok,
            "direct_exit_quote_ok": direct_exit_quote_ok,
            "exit_non_dust": exit_non_dust,
            "auto_flatten_available": auto_flatten_available,
            "pm_reserve_ok": pm_reserve_inventory_ok,
            "pm_release_ok": pm_release_ok,
            "entry_output_account_exists": entry_output_account_exists,
            "size_ladder_has_ready_size": size_ladder_has_ready_size,
            "best_ready_size_sol": best_ready_size_sol,
            "size_ladder_exact_decision": size_decision,
            "entry_expected_out_raw": exact_entry_expected_out_raw,
            "entry_min_out_raw": exact_entry_min_out_raw,
            "exit_expected_out_raw": exact_exit_expected_out_raw,
            "exit_min_out_raw": exact_exit_min_out_raw,
            "target_sized_unwind_ev_lamports": target_sized_unwind_ev_lamports,
            "inventory_sized_unwind_ev_lamports": inventory_sized_unwind_ev_lamports,
            "projected_unwind_input_raw": projected_unwind_input_raw,
            "planned_inventory_token_out_raw": planned_inventory_token_out_raw,
            "projected_unwind_input_lte_planned_inventory": projected_unwind_input_lte_planned_inventory,
            "projected_unwind_input_lte_held_inventory": projected_unwind_input_lte_held_inventory,
            "hot_sender_exit_expected_wsol_raw": hot_sender_exit_expected_wsol_raw,
            "hot_sender_exit_min_wsol_raw": hot_sender_exit_min_wsol_raw,
            "size_consistent_net_after_mode_c_costs": size_consistent_net_after_mode_c_costs,
            "immediate_roundtrip_loss_lamports": current_loss_lamports,
            "decision": decision,
            "first_candidate_blocker": first_blocker,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )

ready_rows = sorted(
    ready_rows,
    key=lambda row: (
        0 if text(row.get("decision")) == "R1_PROPOSAL_READY_ONE_MINT_LIVE" else
        1 if text(row.get("decision")) == "R1_PROPOSAL_READY_ATA_PRECREATE_ONLY" else
        2,
        -safe_int(row.get("size_consistent_net_after_mode_c_costs")),
        safe_int(row.get("immediate_roundtrip_loss_lamports")),
    )
)[:max_candidates]

rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in ready_rows), encoding="utf-8")

summary = {
    "status": (
        "R1_FACTORY_CANDIDATES_READY"
        if size_consistent_candidate_rows > 0
        else ("PARK_SIZE_CONSISTENT_FILTER_ZERO" if rows_scored > 0 else (max(decision_counts, key=decision_counts.get) if decision_counts else "PARK_NO_ROWS"))
    ),
    "window_seconds": window_seconds,
    "filter_mode": text(ls13a_summary.get("filter_mode")) or "unknown",
    "rows_seen": rows_scored,
    "rows_emitted": len(ready_rows),
    "raw_preprocessed_tx_count": safe_int(ls13a_summary.get("preprocessed_raw_tx_seen_count")),
    "target_not_landed_at_first_check_count": safe_int(ls13a_summary.get("target_not_landed_at_first_check_count")),
    "preprocessed_supported_venue_count": safe_int(ls13a_summary.get("preprocessed_supported_venue_count")),
    "preprocessed_backrun_shape_count": safe_int(ls13a_summary.get("preprocessed_backrun_shape_count")),
    "target_entity_ready_count": safe_int(ls13b_summary.get("target_entity_ready_count")),
    "projection_rows_merged_count": safe_int(merged_summary.get("rows_seen")),
    "pumpfun_projection_rows_count": safe_int(prebr0_summary.get("rows_seen")),
    "meteora_projection_rows_count": safe_int(prebr0m_summary.get("rows_seen")),
    "pumpfun_projected_ev_positive_count": safe_int(prebr0_summary.get("projected_ev_after_tip_positive_count")),
    "meteora_projected_ev_positive_count": safe_int(prebr0m_summary.get("projected_ev_after_tip_positive_count")),
    "source_proposal_ready_rows": proposal_ready_rows,
    "source_ata_precreate_only_ready_rows": ata_precreate_only_ready_rows,
    "source_one_mint_live_ready_rows": one_mint_live_ready_rows,
    "size_ready_rows": size_ready_rows,
    "pumpfun_direct_route_exists_rows": safe_int(direct_summary.get("pumpfun_direct_route_exists_rows")),
    "pumpfun_direct_quote_available_rows": safe_int(direct_summary.get("pumpfun_direct_quote_available_rows")),
    "pumpfun_direct_min_out_positive_rows": safe_int(direct_summary.get("pumpfun_direct_min_out_positive_rows")),
    "sim_ready_rows": safe_int(sim_summary.get("sim_ready_rows")),
    "policy_pass_rows": safe_int(sim_summary.get("policy_pass_rows")),
    "direct_entry_quote_ok_rows": safe_int(sim_summary.get("direct_entry_quote_ok_rows")),
    "direct_exit_quote_ok_rows": safe_int(sim_summary.get("direct_exit_quote_ok_rows")),
    "exit_non_dust_rows": safe_int(sim_summary.get("exit_non_dust_rows")),
    "auto_flatten_available_rows": safe_int(sim_summary.get("auto_flatten_available_rows")),
    "pm_reserve_inventory_ok_rows": safe_int(sim_summary.get("pm_reserve_inventory_ok_rows")),
    "inventory_sized_unwind_ev_positive_rows": inventory_sized_unwind_ev_positive_rows,
    "size_consistent_candidate_rows": size_consistent_candidate_rows,
    "source_decision_counts": dict(decision_counts),
    "projected_ev_positive_held_input_rows": safe_int(prebr2_summary.get("projected_ev_positive_held_input_rows")),
    "projected_ev_positive_core_input_rows": safe_int(prebr2_summary.get("projected_ev_positive_core_input_rows")),
    "projected_ev_positive_unheld_token_rows": safe_int(prebr2_summary.get("projected_ev_positive_unheld_token_rows")),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
manifest = {
    "direct_rows_path": str(direct_rows_path),
    "sim_rows_path": str(sim_rows_path),
    "ls13a_summary_path": str(ls13a_summary_path),
    "ls13b_summary_path": str(ls13b_summary_path),
    "merged_summary_path": str(merged_summary_path),
    "prebr0_summary_path": str(prebr0_summary_path),
    "prebr0m_summary_path": str(prebr0m_summary_path),
    "prebr2_summary_path": str(prebr2_summary_path),
    "direct_summary_path": str(direct_summary_path),
    "sim_summary_path": str(sim_summary_path),
    "size_list": size_list,
    "slippage_bps": slippage_bps,
    "base_tx_fee_lamports": base_tx_fee_lamports,
    "priority_fee_lamports": priority_fee_lamports,
    "tip_lamports": tip_lamports,
    "live_autopsy_json_path": str(live_autopsy_path),
    "rpc_url": rpc_url,
    "live_send": False,
    "sendBundle": False,
}

manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
summary_txt_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, (dict, list)) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
PY

pass "RunLSPREBRINV candidate factory written to $SUMMARY_TXT"
cat "$SUMMARY_TXT"
finish

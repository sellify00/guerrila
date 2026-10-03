#!/usr/bin/env bash
# 22_launchpm_v2_pm_readiness_autopsy.sh — Launch Continuation V2 PM readiness autopsy over trigger-B candidates

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 LaunchPM — V2 PM Readiness Autopsy"

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
STEP="${SHADOW_LAUNCHPM_STEP:-22_launchpm_v2_pm_readiness_autopsy}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
ROWS_OUT="$RESULT_DIR/launchpm_v2_pm_readiness_autopsy_rows.jsonl"
SUMMARY_JSON="$RESULT_DIR/launchpm_v2_pm_readiness_autopsy_summary.json"
SUMMARY_TXT="$RESULT_DIR/summary.txt"

PRESSURE_ROWS_PATH="${SHADOW_LAUNCHPM_PRESSURE_ROWS_PATH:-$RESULTS_ROOT/22_launch1_preprocessed_buy_pressure_score/launch1_preprocessed_buy_pressure_score_rows.jsonl}"
GATE_ROWS_PATH="${SHADOW_LAUNCHPM_GATE_ROWS_PATH:-$RESULTS_ROOT/22_launch2_policy_exit_gate/launch2_policy_exit_gate_rows.jsonl}"
SIM_ROWS_PATH="${SHADOW_LAUNCHPM_SIM_ROWS_PATH:-$RESULTS_ROOT/22_launch3_tiny_inventory_sim/launch3_tiny_inventory_sim_rows.jsonl}"
SIGNER_KEYPAIR_PATH="${SHADOW_LAUNCHPM_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_LAUNCHPM_RPC_URL:-${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

TRIGGER_MODE="${SHADOW_LAUNCHPM_TRIGGER_MODE:-B}"
OWNER_MODE="${SHADOW_LAUNCHPM_OWNER_MODE:-launch_continuation}"
RISK_TIER="${SHADOW_LAUNCHPM_RISK_TIER:-LAUNCH_R1}"
MAX_SPEND_SOL="${SHADOW_LAUNCHPM_MAX_SPEND_SOL:-0.003}"
MAX_LOSS_SOL="${SHADOW_LAUNCHPM_MAX_LOSS_SOL:-0.0015}"
BASE_TX_FEE_LAMPORTS="${SHADOW_LAUNCHPM_BASE_TX_FEE_LAMPORTS:-5000}"
PRIORITY_FEE_LAMPORTS="${SHADOW_LAUNCHPM_PRIORITY_FEE_LAMPORTS:-0}"
ENTRY_TIP_LAMPORTS="${SHADOW_LAUNCHPM_ENTRY_TIP_LAMPORTS:-200000}"
EXIT_TIP_LAMPORTS="${SHADOW_LAUNCHPM_EXIT_TIP_LAMPORTS:-200000}"
ALLOW_INLINE_ACCOUNT_CREATE="${SHADOW_LAUNCHPM_ALLOW_INLINE_ACCOUNT_CREATE:-true}"

if [[ ! -r "$SIGNER_KEYPAIR_PATH" ]] && [[ "$(id -u)" != "0" ]] && command -v sudo >/dev/null 2>&1; then
    exec sudo -E bash "$0" "$@"
fi

latest_rows_for_step() {
    local pattern="$1"
    find "$RESULTS_ROOT" -type f -name "$pattern" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -f "$PRESSURE_ROWS_PATH" ]] || PRESSURE_ROWS_PATH="$(latest_rows_for_step launch1_preprocessed_buy_pressure_score_rows.jsonl)"
[[ -f "$GATE_ROWS_PATH" ]] || GATE_ROWS_PATH="$(latest_rows_for_step launch2_policy_exit_gate_rows.jsonl)"
[[ -f "$SIM_ROWS_PATH" ]] || SIM_ROWS_PATH="$(latest_rows_for_step launch3_tiny_inventory_sim_rows.jsonl)"

mkdir -p "$RESULT_DIR"
rm -f "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT"

python3 - "$PRESSURE_ROWS_PATH" "$GATE_ROWS_PATH" "$SIM_ROWS_PATH" "$ROWS_OUT" "$SUMMARY_JSON" "$SUMMARY_TXT" "$SIGNER_KEYPAIR_PATH" "$RPC_URL" "$TRIGGER_MODE" "$OWNER_MODE" "$RISK_TIER" "$MAX_SPEND_SOL" "$MAX_LOSS_SOL" "$BASE_TX_FEE_LAMPORTS" "$PRIORITY_FEE_LAMPORTS" "$ENTRY_TIP_LAMPORTS" "$EXIT_TIP_LAMPORTS" "$ALLOW_INLINE_ACCOUNT_CREATE" <<'PY'
from __future__ import annotations

import json
import ssl
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

(
    pressure_rows_arg,
    gate_rows_arg,
    sim_rows_arg,
    rows_out_arg,
    summary_json_arg,
    summary_txt_arg,
    signer_keypair_arg,
    rpc_url,
    trigger_mode_arg,
    owner_mode_arg,
    risk_tier_arg,
    max_spend_sol_arg,
    max_loss_sol_arg,
    base_fee_arg,
    priority_fee_arg,
    entry_tip_arg,
    exit_tip_arg,
    allow_inline_account_create_arg,
) = sys.argv[1:19]

pressure_rows_path = Path(pressure_rows_arg) if pressure_rows_arg else Path("")
gate_rows_path = Path(gate_rows_arg) if gate_rows_arg else Path("")
sim_rows_path = Path(sim_rows_arg) if sim_rows_arg else Path("")
rows_out_path = Path(rows_out_arg)
summary_json_path = Path(summary_json_arg)
summary_txt_path = Path(summary_txt_arg)
signer_keypair_path = Path(signer_keypair_arg) if signer_keypair_arg else Path("")
trigger_mode = str(trigger_mode_arg or "B").strip().upper()
owner_mode = str(owner_mode_arg or "launch_continuation").strip() or "launch_continuation"
risk_tier = str(risk_tier_arg or "LAUNCH_R1").strip() or "LAUNCH_R1"
max_spend_sol = float(max_spend_sol_arg or 0.003)
max_loss_sol = float(max_loss_sol_arg or 0.0015)
base_fee_lamports = int(float(base_fee_arg or 5000))
priority_fee_lamports = int(float(priority_fee_arg or 0))
entry_tip_lamports = int(float(entry_tip_arg or 200000))
exit_tip_lamports = int(float(exit_tip_arg or 200000))
allow_inline_account_create = str(allow_inline_account_create_arg or "true").strip().lower() in {"1", "true", "yes", "y", "on"}

TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
WSOL_MINT = "So11111111111111111111111111111111111111112"


def text(value: Any) -> str:
    return "" if value is None else str(value).strip()


def lower(value: Any) -> str:
    return text(value).lower()


def boolish(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return lower(value) in {"1", "true", "yes", "y", "on", "pass", "passed", "ready"}


def safe_int(value: Any, default: int = 0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(float(value))
    except (TypeError, ValueError):
        return default


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


def read_signer_pubkey(path: Path) -> str:
    if not path.exists():
        return ""
    try:
        raw = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        if isinstance(raw, list) and len(raw) >= 64:
            return b58encode(bytes(int(item) & 0xFF for item in raw[32:64]))
        if isinstance(raw, list) and len(raw) == 32:
            return b58encode(bytes(int(item) & 0xFF for item in raw))
    except Exception:
        return ""
    return ""


def get_balance(pubkey: str) -> int:
    if not pubkey:
        return 0
    response, error = rpc("getBalance", [pubkey, {"commitment": "processed"}])
    if error is not None:
        return 0
    value = (response or {}).get("result", response)
    if isinstance(value, dict):
        value = value.get("value", value)
    return safe_int(value)


def get_token_accounts_by_owner(owner: str, mint: str) -> list[dict[str, Any]]:
    if not owner or not mint:
        return []
    response, error = rpc(
        "getTokenAccountsByOwner",
        [owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "processed"}],
    )
    if error is not None:
        return []
    value = (response or {}).get("result", response)
    rows = value.get("value") if isinstance(value, dict) else []
    out: list[dict[str, Any]] = []
    for row in rows or []:
        parsed = (((row or {}).get("account") or {}).get("data") or {}).get("parsed") or {}
        info = parsed.get("info") if isinstance(parsed, dict) else {}
        token_amount = (info or {}).get("tokenAmount") if isinstance(info, dict) else {}
        out.append(
            {
                "pubkey": text((row or {}).get("pubkey")),
                "amount_raw": safe_int((token_amount or {}).get("amount")),
            }
        )
    return out


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
    if safe_int(row.get("first_sell_seen")):
        return False, "sell_seen_before_entry"
    if safe_int(row.get("net_buy_pressure")) <= 0:
        return False, "sell_pressure_dominant"
    return True, "none"


pressure_rows = load_jsonl(pressure_rows_path)
gate_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(gate_rows_path) if text(row.get("launch_group_id"))}
sim_by_group = {text(row.get("launch_group_id")): row for row in load_jsonl(sim_rows_path) if text(row.get("launch_group_id"))}

owner_pubkey = read_signer_pubkey(signer_keypair_path)
owner_native_balance_raw = get_balance(owner_pubkey)
required_input_lamports = sol_to_lamports(max_spend_sol)
required_fee_lamports = (2 * base_fee_lamports) + (2 * priority_fee_lamports)
required_tip_lamports = entry_tip_lamports + exit_tip_lamports

rows: list[dict[str, Any]] = []
decision_counts: Counter[str] = Counter()

for pressure_row in pressure_rows:
    trigger_pass, trigger_blocker = trigger_b_passes(pressure_row)
    if not trigger_pass:
        continue

    group_id = text(pressure_row.get("launch_group_id"))
    gate_row = gate_by_group.get(group_id, {})
    sim_row = sim_by_group.get(group_id, {})
    mint = text(pressure_row.get("launch_mint")) or text(gate_row.get("launch_mint")) or text(sim_row.get("launch_mint"))
    pool = text(pressure_row.get("pool")) or text(gate_row.get("pool")) or text(sim_row.get("pool"))
    creator = text(pressure_row.get("signer"))
    token_accounts = get_token_accounts_by_owner(owner_pubkey, mint)
    output_account_ready = bool(token_accounts)
    inline_account_required = not output_account_ready
    native_sol_balance_lamports = owner_native_balance_raw
    wsol_balance_lamports = sum(safe_int(item.get("amount_raw")) for item in get_token_accounts_by_owner(owner_pubkey, WSOL_MINT))
    pm_owner_lock_ready = bool(owner_mode)
    pm_candidate_id_present = bool(text(pressure_row.get("candidate_id")))
    pm_conflict_domain = f"{owner_mode}:{mint}:{pool}" if (owner_mode and (mint or pool)) else ""
    pm_conflict_domain_present = bool(pm_conflict_domain)
    pm_reserve_input_ok = native_sol_balance_lamports >= required_input_lamports
    pm_reserve_fee_ok = native_sol_balance_lamports >= required_fee_lamports
    pm_reserve_tip_ok = native_sol_balance_lamports >= required_tip_lamports
    pm_reserve_rent_ok = native_sol_balance_lamports >= (safe_int(sim_row.get("ata_rent_lamports")) if inline_account_required else 0)
    pm_release_on_no_send_ok = True
    pm_pending_after = 0
    pm_account_surface_ok = output_account_ready or (allow_inline_account_create and inline_account_required and pm_reserve_rent_ok)

    if not pm_owner_lock_ready:
        first_pm_blocker = "PM_OWNER_MODE_MISSING"
    elif not pm_candidate_id_present:
        first_pm_blocker = "PM_CANDIDATE_ID_MISSING"
    elif not pm_conflict_domain_present:
        first_pm_blocker = "PM_CONFLICT_DOMAIN_MISSING"
    elif not pm_reserve_input_ok:
        first_pm_blocker = "PM_INPUT_BUDGET_MISSING"
    elif not pm_reserve_fee_ok or not pm_reserve_tip_ok:
        first_pm_blocker = "PM_FEE_TIP_BUDGET_MISSING"
    elif not pm_reserve_rent_ok:
        first_pm_blocker = "PM_RENT_BUDGET_MISSING"
    elif not pm_account_surface_ok:
        first_pm_blocker = "PM_ACCOUNT_SURFACE_MISSING"
    elif not pm_release_on_no_send_ok:
        first_pm_blocker = "PM_RELEASE_NOT_PROVEN"
    else:
        first_pm_blocker = "none"

    row = {
        "record_type": "launchpm_v2_pm_readiness_autopsy_row",
        "launch_group_id": group_id,
        "candidate_id": text(pressure_row.get("candidate_id")) or text(gate_row.get("candidate_id")) or text(sim_row.get("candidate_id")),
        "mint": mint,
        "pool": pool,
        "creator": creator,
        "owner_mode": owner_mode,
        "risk_tier": risk_tier,
        "signer_pubkey": owner_pubkey,
        "max_spend_sol": max_spend_sol,
        "max_loss_sol": max_loss_sol,
        "native_sol_balance_lamports": native_sol_balance_lamports,
        "wsol_balance_lamports": wsol_balance_lamports,
        "required_inventory_lamports": required_input_lamports,
        "required_fee_lamports": required_fee_lamports,
        "required_tip_lamports": required_tip_lamports,
        "required_rent_lamports": safe_int(sim_row.get("ata_rent_lamports")) if inline_account_required else 0,
        "pm_owner_lock_ready": pm_owner_lock_ready,
        "pm_owner_lock_owner_mode": owner_mode,
        "pm_candidate_id_present": pm_candidate_id_present,
        "pm_conflict_domain_present": pm_conflict_domain_present,
        "pm_conflict_domain": pm_conflict_domain,
        "pm_reserve_input_ok": pm_reserve_input_ok,
        "pm_reserve_fee_ok": pm_reserve_fee_ok,
        "pm_reserve_tip_ok": pm_reserve_tip_ok,
        "pm_reserve_rent_ok": pm_reserve_rent_ok,
        "pm_release_on_no_send_ok": pm_release_on_no_send_ok,
        "pm_pending_after": pm_pending_after,
        "pm_account_surface_ok": pm_account_surface_ok,
        "inline_account_creation_allowed": allow_inline_account_create,
        "output_account_ready": output_account_ready,
        "inline_account_required": inline_account_required,
        "target_tx_bytes_available": boolish(pressure_row.get("target_tx_bytes_available")),
        "target_not_landed_at_first_check": boolish(pressure_row.get("target_not_landed_at_first_check")),
        "exit_ladder_pass": boolish(gate_row.get("r1_exit_nonzero_size_band_ready")),
        "policy_pass": boolish(gate_row.get("mint_policy_pass")),
        "direct_entry_quote_ok": boolish(gate_row.get("direct_entry_quote_ok")),
        "direct_exit_quote_ok": boolish(gate_row.get("direct_exit_quote_ok")),
        "sim_decision": text(sim_row.get("sim_decision")),
        "first_pm_blocker": first_pm_blocker,
        "wire_send": 0,
        "live_landed": 0,
        "sendBundle_called": False,
    }
    rows.append(row)
    decision_counts[first_pm_blocker] += 1

rows.sort(key=lambda row: (0 if text(row.get("first_pm_blocker")) == "none" else 1, text(row.get("first_pm_blocker")), text(row.get("candidate_id"))))
rows_out_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")
summary = {
    "status": "launchpm_v2_pm_readiness_autopsy_complete" if rows else "launchpm_v2_pm_readiness_autopsy_no_rows",
    "trigger_mode": trigger_mode,
    "rows_seen": len(rows),
    "pm_ready_count": sum(1 for row in rows if text(row.get("first_pm_blocker")) == "none"),
    "input_budget_ready_count": sum(1 for row in rows if boolish(row.get("pm_reserve_input_ok"))),
    "fee_budget_ready_count": sum(1 for row in rows if boolish(row.get("pm_reserve_fee_ok"))),
    "tip_budget_ready_count": sum(1 for row in rows if boolish(row.get("pm_reserve_tip_ok"))),
    "rent_budget_ready_count": sum(1 for row in rows if boolish(row.get("pm_reserve_rent_ok"))),
    "pm_account_surface_ready_count": sum(1 for row in rows if boolish(row.get("pm_account_surface_ok"))),
    "output_account_ready_count": sum(1 for row in rows if boolish(row.get("output_account_ready"))),
    "inline_account_required_count": sum(1 for row in rows if boolish(row.get("inline_account_required"))),
    "inline_account_creation_allowed": allow_inline_account_create,
    "first_blocker_counts": dict(decision_counts),
    "owner_mode": owner_mode,
    "risk_tier": risk_tier,
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

pass "Computed Launch V2 PM readiness autopsy into $RESULT_DIR"
cat "$SUMMARY_TXT"
finish

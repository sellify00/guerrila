#!/usr/bin/env bash
# 78D_PRELIVE - selected candidate notional, reserve, and live-risk sanity gate

set -euo pipefail

source "$(dirname "$0")/lib.sh"
script_header "78D_PRELIVE - Selected Candidate Risk Gate"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-/home/ubuntu/shadow-host-audit/dpdk-runs}"
SOURCE_STEP="${SHADOW_RUN78D_SOURCE_STEP:-epoch8_auto_strict_directarb_quote_identity_run78c_cleanup_confirmation_retry3}"
SOURCE_SUMMARY_PATH="${SHADOW_RUN78D_SOURCE_SUMMARY_PATH:-$RESULTS_ROOT/$SOURCE_STEP/summary.txt}"
STEP="${SHADOW_RUN78D_STEP:-epoch8_auto_strict_directarb_quote_identity_run78d_prelive_selected_candidate_risk_gate}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
REPORT_JSON="$RESULT_DIR/run78d_prelive_report.json"
INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
APPROVED_LIVE_CAP_SOL="${SHADOW_RUN78D_APPROVED_LIVE_CAP_SOL:-0.1}"
SOL_USD_EQUIVALENT="${SHADOW_RUN78D_SOL_USD_EQUIVALENT:-${SHADOW_RUN78C_SOL_USD_EQUIVALENT:-}}"
RPC_URL_OVERRIDE="${SHADOW_RUN78D_RPC_URL:-}"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$REPORT_JSON"

if [[ ! -f "$SOURCE_SUMMARY_PATH" ]]; then
    fail "Missing source summary: $SOURCE_SUMMARY_PATH"
    {
        echo "status=blocked_missing_source_summary"
        echo "candidate_live_eligible=false"
        echo "blocker=blocked_missing_source_summary"
        echo "source_summary_path=$SOURCE_SUMMARY_PATH"
    } > "$SUMMARY_FILE"
    finish
    exit 1
fi

if [[ ! -f "$INVENTORY_PATH" ]]; then
    fail "Missing signer inventory snapshot: $INVENTORY_PATH"
    {
        echo "status=blocked_missing_signer_inventory"
        echo "candidate_live_eligible=false"
        echo "blocker=blocked_missing_signer_inventory"
        echo "source_summary_path=$SOURCE_SUMMARY_PATH"
        echo "signer_inventory_path=$INVENTORY_PATH"
    } > "$SUMMARY_FILE"
    finish
    exit 1
fi

export SOURCE_SUMMARY_PATH SUMMARY_FILE REPORT_JSON INVENTORY_PATH APPROVED_LIVE_CAP_SOL SOL_USD_EQUIVALENT RPC_URL_OVERRIDE

python3 - <<'PY'
import json
import math
import os
import urllib.request
from decimal import Decimal, InvalidOperation
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"


def read_kv(path: Path):
    result = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        if not raw_line or "=" not in raw_line:
            continue
        key, value = raw_line.split("=", 1)
        result[key.strip()] = value.strip()
    return result


def safe_int(value, default=0):
    try:
        if value is None or value == "":
            return default
        return int(str(value).strip())
    except (TypeError, ValueError):
        return default


def safe_decimal(value):
    try:
        if value is None or value == "":
            return None
        return Decimal(str(value).strip())
    except (InvalidOperation, ValueError):
        return None


def to_bool(value):
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def json_rpc(url: str, method: str, params):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        body = json.loads(response.read().decode())
    if "error" in body:
        raise RuntimeError(f"{method} RPC error: {body['error']}")
    return body["result"]


def normalize_amount(raw_value: int, decimals: int):
    scale = Decimal(10) ** Decimal(decimals)
    return Decimal(raw_value) / scale


def decimal_to_str(value):
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        if not math.isfinite(value):
            return ""
        text = f"{value:.12f}".rstrip("0").rstrip(".")
        return text or "0"
    if isinstance(value, Decimal):
        text = format(value.normalize(), "f")
        if "." in text:
            text = text.rstrip("0").rstrip(".")
        return text or "0"
    return str(value)


def json_ready(value):
    if isinstance(value, Decimal):
        return float(value)
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, dict):
        return {key: json_ready(item) for key, item in value.items()}
    if isinstance(value, list):
        return [json_ready(item) for item in value]
    return value


def append_summary(path: Path, rows):
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        for key, value in rows:
            handle.write(f"{key}={decimal_to_str(value)}\n")


summary_path = Path(os.environ["SOURCE_SUMMARY_PATH"])
summary = read_kv(summary_path)
target_manifest_value = (
    summary.get("run78c_target_selection_manifest")
    or summary.get("selector_manifest_path")
    or ""
).strip()
if not target_manifest_value:
    raise SystemExit("missing run78c target selection manifest path in source summary")
target_manifest_path = Path(target_manifest_value)
if not target_manifest_path.exists():
    raise SystemExit(f"missing target manifest: {target_manifest_path}")

target_lines = [line for line in target_manifest_path.read_text(encoding="utf-8").splitlines() if line.strip()]
if not target_lines:
    raise SystemExit(f"empty target manifest: {target_manifest_path}")
target = json.loads(target_lines[0])

inventory_path = Path(os.environ["INVENTORY_PATH"])
inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
inventory_accounts = inventory.get("accounts") or []
inventory_slot = safe_int(inventory.get("slot"))
inventory_rpc_url = str(inventory.get("rpc_url") or "").strip()
rpc_url = str(os.environ.get("RPC_URL_OVERRIDE") or "").strip() or inventory_rpc_url
if not rpc_url:
    raise SystemExit("missing RPC URL override and inventory rpc_url")

approved_live_cap_sol = safe_decimal(os.environ.get("APPROVED_LIVE_CAP_SOL")) or Decimal("0.1")
sol_usd_equivalent = safe_decimal(os.environ.get("SOL_USD_EQUIVALENT"))

mint_in = str(target.get("mint_in") or summary.get("run78c_selected_mint_in") or "")
mint_out = str(target.get("mint_out") or summary.get("run78c_selected_mint_out") or "")
ingress_signature = str(target.get("ingress_signature") or summary.get("run78c_selected_signature") or "")
expected_out_raw = safe_int(target.get("expected_out_raw"))
min_out_raw = safe_int(target.get("min_out_raw"))
route = str(target.get("route") or "")
source = str(target.get("source") or "")

mint_in_info = json_rpc(rpc_url, "getAccountInfo", [mint_in, {"encoding": "jsonParsed", "commitment": "confirmed"}])
mint_out_info = json_rpc(rpc_url, "getAccountInfo", [mint_out, {"encoding": "jsonParsed", "commitment": "confirmed"}])
mint_in_decimals = safe_int((((mint_in_info or {}).get("value") or {}).get("data") or {}).get("parsed", {}).get("info", {}).get("decimals"))
mint_out_decimals = safe_int((((mint_out_info or {}).get("value") or {}).get("data") or {}).get("parsed", {}).get("info", {}).get("decimals"))

tx_result = json_rpc(
    rpc_url,
    "getTransaction",
    [
        ingress_signature,
        {
            "encoding": "jsonParsed",
            "maxSupportedTransactionVersion": 0,
            "commitment": "confirmed",
        },
    ],
)

observed_input_amount_raw = 0
observed_output_amount_raw = 0
observed_input_decimals = mint_in_decimals
observed_output_decimals = mint_out_decimals

for inner in ((tx_result or {}).get("meta") or {}).get("innerInstructions") or []:
    for instruction in inner.get("instructions") or []:
        parsed = instruction.get("parsed")
        if not isinstance(parsed, dict):
            continue
        info = parsed.get("info") or {}
        token_amount = info.get("tokenAmount") or {}
        amount = safe_int(token_amount.get("amount"))
        decimals = safe_int(token_amount.get("decimals"))
        mint = str(info.get("mint") or "")
        if mint == mint_in and amount > observed_input_amount_raw:
            observed_input_amount_raw = amount
            observed_input_decimals = decimals or mint_in_decimals
        if mint == mint_out and amount > observed_output_amount_raw:
            observed_output_amount_raw = amount
            observed_output_decimals = decimals or mint_out_decimals

input_native_amount = normalize_amount(observed_input_amount_raw, observed_input_decimals) if observed_input_amount_raw > 0 else None
observed_output_native = normalize_amount(observed_output_amount_raw, observed_output_decimals) if observed_output_amount_raw > 0 else None
expected_out_native = normalize_amount(expected_out_raw, mint_out_decimals) if expected_out_raw > 0 else None
min_out_native = normalize_amount(min_out_raw, mint_out_decimals) if min_out_raw > 0 else None

input_sol_equiv = None
if mint_in == WSOL_MINT and input_native_amount is not None:
    input_sol_equiv = input_native_amount
elif mint_out == WSOL_MINT and observed_output_native is not None:
    input_sol_equiv = observed_output_native
elif mint_out == WSOL_MINT and expected_out_native is not None:
    input_sol_equiv = expected_out_native

input_usd_equiv = None
if input_sol_equiv is not None and sol_usd_equivalent is not None:
    input_usd_equiv = input_sol_equiv * sol_usd_equivalent

net_after_fee_raw = safe_int(target.get("candidate_net_after_fee_raw"))
policy_tip_raw = safe_int(target.get("policy_tip_raw"))
base_fee_raw = safe_int(target.get("base_fee_raw"))
net_after_fee_sol = safe_decimal(target.get("candidate_net_after_fee_sol_equiv"))
if net_after_fee_sol is None:
    net_after_fee_sol = Decimal(net_after_fee_raw) / Decimal(1_000_000_000)
net_after_fee_usd = None
if net_after_fee_sol is not None and sol_usd_equivalent is not None:
    net_after_fee_usd = net_after_fee_sol * sol_usd_equivalent

max_loss_if_fail_raw = policy_tip_raw + base_fee_raw
max_loss_if_fail_sol = Decimal(max_loss_if_fail_raw) / Decimal(1_000_000_000)
max_loss_if_fail_usd = None
if sol_usd_equivalent is not None:
    max_loss_if_fail_usd = max_loss_if_fail_sol * sol_usd_equivalent

input_inventory_entry = next((item for item in inventory_accounts if item.get("mint_pubkey") == mint_in), None)
output_inventory_entry = next((item for item in inventory_accounts if item.get("mint_pubkey") == mint_out), None)
input_inventory_token_account = (input_inventory_entry or {}).get("token_account_pubkey") or ""
output_inventory_token_account = (output_inventory_entry or {}).get("token_account_pubkey") or ""
current_input_balance_raw = safe_int((input_inventory_entry or {}).get("amount_raw"))
current_input_balance_decimals = mint_in_decimals
current_input_balance_slot = None
if input_inventory_token_account:
    current_input_balance_result = json_rpc(
        rpc_url,
        "getTokenAccountBalance",
        [input_inventory_token_account, {"commitment": "confirmed"}],
    )
    current_input_balance_raw = safe_int((current_input_balance_result.get("value") or {}).get("amount"))
    current_input_balance_decimals = safe_int((current_input_balance_result.get("value") or {}).get("decimals")) or mint_in_decimals
    current_input_balance_slot = safe_int((current_input_balance_result.get("context") or {}).get("slot"))

current_output_balance_raw = safe_int((output_inventory_entry or {}).get("amount_raw"))
current_output_balance_slot = None
if output_inventory_token_account:
    current_output_balance_result = json_rpc(
        rpc_url,
        "getTokenAccountBalance",
        [output_inventory_token_account, {"commitment": "confirmed"}],
    )
    current_output_balance_raw = safe_int((current_output_balance_result.get("value") or {}).get("amount"))
    current_output_balance_slot = safe_int((current_output_balance_result.get("context") or {}).get("slot"))

pm_reserve_can_lock_exact_input = observed_input_amount_raw > 0 and current_input_balance_raw >= observed_input_amount_raw
exit_liquidation_route_exists = mint_out == WSOL_MINT
exit_liquidation_route_basis = "output_is_wsol" if exit_liquidation_route_exists else "unverified"
expected_gt_min = expected_out_raw > min_out_raw > 0
candidate_ev_positive = to_bool(target.get("candidate_ev_positive"))
candidate_meets_ev_floor = to_bool(target.get("candidate_meets_configured_ev_floor"))
quote_projection_ratio = None
if observed_output_native is not None and observed_output_native > 0 and expected_out_native is not None:
    quote_projection_ratio = expected_out_native / observed_output_native
projection_consistent = quote_projection_ratio is not None and Decimal("0.5") <= quote_projection_ratio <= Decimal("2")

candidate_input_notional = input_sol_equiv
input_notional_within_cap = candidate_input_notional is not None and candidate_input_notional <= approved_live_cap_sol
projected_output_within_cap = min_out_native is not None and min_out_native <= approved_live_cap_sol

required_checks = {
    "route_is_tpu_quic_public": route == "tpu_quic_public",
    "source_is_afxdp": source == "afxdp",
    "template_ready": to_bool(target.get("template_ready")),
    "inventory_ready": to_bool(target.get("inventory_ready")),
    "approved_held_input": to_bool(target.get("approved_held_input")),
    "output_account_ready": to_bool(target.get("output_account_ready")),
    "min_out_positive": min_out_raw > 0,
    "expected_out_gt_min_out": expected_gt_min,
    "candidate_ev_positive": candidate_ev_positive,
    "candidate_meets_configured_ev_floor": candidate_meets_ev_floor,
    "pm_reserve_can_lock_exact_input": pm_reserve_can_lock_exact_input,
    "exit_liquidation_route_exists": exit_liquidation_route_exists,
    "input_notional_within_cap": input_notional_within_cap,
    "projected_output_within_cap": projected_output_within_cap,
    "projection_consistent_with_observed_tx": projection_consistent,
}

status = "passed_prelive_gate"
blocker = ""
candidate_live_eligible = True

if (min_out_native is not None and min_out_native > approved_live_cap_sol) or not input_notional_within_cap or not projection_consistent:
    status = "blocked_live_notional_too_large_or_unverified"
    blocker = "blocked_live_notional_too_large_or_unverified"
    candidate_live_eligible = False
elif not route == "tpu_quic_public":
    status = "blocked_live_route_not_tpu_quic_public"
    blocker = status
    candidate_live_eligible = False
elif not source == "afxdp":
    status = "blocked_live_source_not_afxdp"
    blocker = status
    candidate_live_eligible = False
elif not pm_reserve_can_lock_exact_input:
    status = "blocked_live_pm_reserve_cannot_lock_exact_input"
    blocker = status
    candidate_live_eligible = False
elif not exit_liquidation_route_exists:
    status = "blocked_live_exit_route_missing"
    blocker = status
    candidate_live_eligible = False
elif not candidate_ev_positive:
    status = "blocked_live_ev_not_positive"
    blocker = status
    candidate_live_eligible = False
elif not candidate_meets_ev_floor:
    status = "blocked_live_configured_ev_floor"
    blocker = status
    candidate_live_eligible = False
else:
    for key, ok in required_checks.items():
        if not ok:
            status = f"blocked_live_{key}"
            blocker = status
            candidate_live_eligible = False
            break

report = {
    "status": status,
    "candidate_live_eligible": candidate_live_eligible,
    "blocker": blocker,
    "approved_live_notional_cap_sol": approved_live_cap_sol,
    "sol_usd_equivalent": sol_usd_equivalent,
    "source_summary_path": str(summary_path),
    "target_manifest_path": str(target_manifest_path),
    "signer_inventory_path": str(inventory_path),
    "rpc_url": rpc_url,
    "inventory_snapshot_slot": inventory_slot,
    "run78c_selected_candidate_id": summary.get("run78c_selected_candidate_id", ""),
    "selected_pool": target.get("pool", ""),
    "selected_signature": ingress_signature,
    "selected_shadow_signature": target.get("shadow_signature", ""),
    "route": route,
    "source": source,
    "template_id": target.get("template_id"),
    "mint_in": mint_in,
    "mint_in_decimals": mint_in_decimals,
    "input_amount_raw": observed_input_amount_raw,
    "input_amount_basis": "observed_ingress_transaction_transfer_checked",
    "input_native_amount": input_native_amount,
    "input_sol_equiv": input_sol_equiv,
    "input_usd_equiv": input_usd_equiv,
    "mint_out": mint_out,
    "mint_out_decimals": mint_out_decimals,
    "expected_out_raw": expected_out_raw,
    "expected_out_native": expected_out_native,
    "min_out_raw": min_out_raw,
    "min_out_native": min_out_native,
    "observed_output_raw": observed_output_amount_raw,
    "observed_output_native": observed_output_native,
    "quote_projection_ratio_vs_observed_output": quote_projection_ratio,
    "projection_consistent_with_observed_tx": projection_consistent,
    "candidate_retained_ev_raw": safe_int(target.get("candidate_retained_ev_raw")),
    "candidate_retained_ev_sol_equiv": safe_decimal(target.get("candidate_retained_ev_sol_equiv")),
    "candidate_retained_ev_usd_equiv": safe_decimal(target.get("candidate_retained_ev_usd_equiv")),
    "policy_tip_raw": policy_tip_raw,
    "base_fee_raw": base_fee_raw,
    "net_after_fee_raw": net_after_fee_raw,
    "net_after_fee_sol": net_after_fee_sol,
    "net_after_fee_usd": net_after_fee_usd,
    "max_loss_if_fail_raw": max_loss_if_fail_raw,
    "max_loss_if_fail_sol": max_loss_if_fail_sol,
    "max_loss_if_fail_usd": max_loss_if_fail_usd,
    "candidate_ev_positive": candidate_ev_positive,
    "candidate_meets_configured_ev_floor": candidate_meets_ev_floor,
    "inventory_ready": to_bool(target.get("inventory_ready")),
    "approved_held_input": to_bool(target.get("approved_held_input")),
    "output_account_ready": to_bool(target.get("output_account_ready")),
    "template_ready": to_bool(target.get("template_ready")),
    "pm_reserve_can_lock_exact_input": pm_reserve_can_lock_exact_input,
    "current_input_balance_raw": current_input_balance_raw,
    "current_input_balance_decimals": current_input_balance_decimals,
    "current_input_balance_slot": current_input_balance_slot,
    "current_output_balance_raw": current_output_balance_raw,
    "current_output_balance_slot": current_output_balance_slot,
    "input_inventory_token_account": input_inventory_token_account,
    "output_inventory_token_account": output_inventory_token_account,
    "exit_liquidation_route_exists": exit_liquidation_route_exists,
    "exit_liquidation_route_basis": exit_liquidation_route_basis,
    "required_checks": required_checks,
}

report_path = Path(os.environ["REPORT_JSON"])
report_path.write_text(
    json.dumps(json_ready(report), indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

summary_rows = [
    ("status", status),
    ("candidate_live_eligible", "true" if candidate_live_eligible else "false"),
    ("blocker", blocker),
    ("approved_live_notional_cap_sol", approved_live_cap_sol),
    ("source_summary_path", summary_path),
    ("target_manifest_path", target_manifest_path),
    ("signer_inventory_path", inventory_path),
    ("selected_pool", target.get("pool", "")),
    ("selected_signature", ingress_signature),
    ("selected_shadow_signature", target.get("shadow_signature", "")),
    ("route", route),
    ("source", source),
    ("template_id", target.get("template_id", "")),
    ("mint_in", mint_in),
    ("mint_in_decimals", mint_in_decimals),
    ("input_amount_raw", observed_input_amount_raw),
    ("input_amount_basis", "observed_ingress_transaction_transfer_checked"),
    ("input_native_amount", input_native_amount),
    ("input_usd_equiv", input_usd_equiv),
    ("input_sol_equiv", input_sol_equiv),
    ("candidate_input_notional_sol", candidate_input_notional),
    ("mint_out", mint_out),
    ("mint_out_decimals", mint_out_decimals),
    ("expected_out_raw", expected_out_raw),
    ("expected_out_native", expected_out_native),
    ("candidate_expected_out_native", expected_out_native),
    ("min_out_raw", min_out_raw),
    ("min_out_native", min_out_native),
    ("candidate_min_out_native", min_out_native),
    ("observed_output_raw", observed_output_amount_raw),
    ("observed_output_native", observed_output_native),
    ("quote_projection_ratio_vs_observed_output", quote_projection_ratio),
    ("projection_consistent_with_observed_tx", "true" if projection_consistent else "false"),
    ("policy_tip_raw", policy_tip_raw),
    ("base_fee_raw", base_fee_raw),
    ("net_after_fee_raw", net_after_fee_raw),
    ("net_after_fee_sol", net_after_fee_sol),
    ("net_after_fee_usd", net_after_fee_usd),
    ("max_loss_if_fail_raw", max_loss_if_fail_raw),
    ("max_loss_if_fail_sol", max_loss_if_fail_sol),
    ("max_loss_if_fail_usd", max_loss_if_fail_usd),
    ("candidate_ev_positive", "true" if candidate_ev_positive else "false"),
    ("candidate_meets_configured_ev_floor", "true" if candidate_meets_ev_floor else "false"),
    ("inventory_ready", "true" if to_bool(target.get("inventory_ready")) else "false"),
    ("approved_held_input", "true" if to_bool(target.get("approved_held_input")) else "false"),
    ("output_account_ready", "true" if to_bool(target.get("output_account_ready")) else "false"),
    ("template_ready", "true" if to_bool(target.get("template_ready")) else "false"),
    ("pm_reserve_can_lock_exact_input", "true" if pm_reserve_can_lock_exact_input else "false"),
    ("current_input_balance_raw", current_input_balance_raw),
    ("current_input_balance_slot", current_input_balance_slot or ""),
    ("current_output_balance_raw", current_output_balance_raw),
    ("current_output_balance_slot", current_output_balance_slot or ""),
    ("exit_liquidation_route_exists", "true" if exit_liquidation_route_exists else "false"),
    ("exit_liquidation_route_basis", exit_liquidation_route_basis),
    ("run78d_prelive_report_path", report_path),
]

for key, ok in required_checks.items():
    summary_rows.append((key, "true" if ok else "false"))

append_summary(Path(os.environ["SUMMARY_FILE"]), summary_rows)
PY

info "Run78D pre-live gate wrote $SUMMARY_FILE"
info "Run78D pre-live report wrote $REPORT_JSON"
finish

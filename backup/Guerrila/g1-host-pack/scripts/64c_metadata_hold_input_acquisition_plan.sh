#!/usr/bin/env bash
# 64c_metadata_hold_input_acquisition_plan.sh - dry-run acquisition planning for metadata-only Token-2022 held-input prep

source "$(dirname "$0")/lib.sh"
script_header "64C - Metadata Held-Input Acquisition Plan"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN64C_STEP:-64c_metadata_hold_input_acquisition_plan}"
STATUS_PREFIX="${SHADOW_RUN64C_STATUS_PREFIX:-run64c_metadata_hold_input_acquisition_plan}"
REPORT_BASENAME="${SHADOW_RUN64C_REPORT_BASENAME:-run64c_hold_input_acquisition_plan}"

RUN61_STEP="${SHADOW_RUN64C_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN63_STEP="${SHADOW_RUN64C_RUN63_STEP:-63_directarb_conversion_pass}"
RUN64B_STEP="${SHADOW_RUN64C_RUN64B_STEP:-64b_metadata_only_token2022_conversion_prep}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN63_RESULT_DIR="$RESULTS_ROOT/$RUN63_STEP"
RUN64B_RESULT_DIR="$RESULTS_ROOT/$RUN64B_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN63_REPORT_JSON="$RUN63_RESULT_DIR/run63_directarb_conversion_pass.json"
RUN64B_REPORT_JSON="$RUN64B_RESULT_DIR/run64b_metadata_only_token2022_conversion_prep.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RUN64C_QUOTE_API_URL="${SHADOW_RUN64C_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
RUN64C_PER_MINT_USDC_CAP="${SHADOW_RUN64C_PER_MINT_USDC_CAP:-5.0}"
RUN64C_TOTAL_USDC_CAP="${SHADOW_RUN64C_TOTAL_USDC_CAP:-25.0}"
RUN64C_SLIPPAGE_BPS="${SHADOW_RUN64C_SLIPPAGE_BPS:-100}"
RUN64C_HELD_INPUT_COUNT="${SHADOW_RUN64C_HELD_INPUT_COUNT:-5}"
RUN64C_ATA_COUNT="${SHADOW_RUN64C_ATA_COUNT:-5}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/$REPORT_BASENAME.json"
TASKS_FILE="$RESULT_DIR/run64c_operator_tasks.sh"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TASKS_FILE"

missing=0
for required_path in \
    "$RUN61_REPORT_JSON" \
    "$RUN63_REPORT_JSON" \
    "$RUN64B_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH"
do
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
    "$RUN61_REPORT_JSON" \
    "$RUN63_REPORT_JSON" \
    "$RUN64B_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RUN64C_QUOTE_API_URL" \
    "$RUN64C_PER_MINT_USDC_CAP" \
    "$RUN64C_TOTAL_USDC_CAP" \
    "$RUN64C_SLIPPAGE_BPS" \
    "$RUN64C_HELD_INPUT_COUNT" \
    "$RUN64C_ATA_COUNT" \
    "$PACK_DIR" <<'PY'
import json
import math
import ssl
import stat
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
WSOL_MINT = "So11111111111111111111111111111111111111112"
SOURCE_ASSETS = {
    USDC_MINT: {"symbol": "USDC", "decimals": 6},
    WSOL_MINT: {"symbol": "WSOL", "decimals": 9},
}

run61_report_path = Path(sys.argv[1])
run63_report_path = Path(sys.argv[2])
run64b_report_path = Path(sys.argv[3])
inventory_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
tasks_file = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
quote_api_url = sys.argv[10]
per_mint_usdc_cap = float(sys.argv[11] or "5.0")
total_usdc_cap = float(sys.argv[12] or "25.0")
slippage_bps = int(sys.argv[13] or "100")
held_input_count = int(sys.argv[14] or "5")
ata_count = int(sys.argv[15] or "5")
pack_dir = Path(sys.argv[16])


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def text_value(value):
    return str(value or "").strip()


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def rpc_quote(url: str, params: dict):
    request_url = url + "?" + urllib.parse.urlencode(params)
    request = urllib.request.Request(request_url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def source_native_from_raw(source_mint: str, raw_amount: int, sol_usd_equivalent: float):
    if source_mint == USDC_MINT:
        native = raw_amount / 1_000_000.0
        return native, native
    if source_mint == WSOL_MINT:
        native = raw_amount / 1_000_000_000.0
        return native, native * sol_usd_equivalent
    return None, None


def output_native_from_raw(raw_amount: int, decimals: int):
    return raw_amount / (10 ** decimals)


def max_input_raw_for_source(source_mint: str, sol_usd_equivalent: float):
    if source_mint == USDC_MINT:
        return int(per_mint_usdc_cap * 1_000_000)
    if source_mint == WSOL_MINT:
        return int((per_mint_usdc_cap / sol_usd_equivalent) * 1_000_000_000)
    return 0


def quote_exact_in(source_mint: str, output_mint: str, input_raw: int):
    params = {
        "inputMint": source_mint,
        "outputMint": output_mint,
        "amount": str(max(1, input_raw)),
        "slippageBps": str(slippage_bps),
    }
    attempts = 3
    delay_seconds = 1.0
    for attempt in range(1, attempts + 1):
        try:
            return rpc_quote(quote_api_url, params), None
        except urllib.error.HTTPError as exc:  # noqa: PERF203
            body = exc.read().decode("utf-8", errors="replace")
            try:
                error_payload = json.loads(body)
            except Exception:  # noqa: BLE001
                error_payload = {"error": body, "errorCode": f"http_{exc.code}"}
            if exc.code == 429 and attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            error_payload["attempts"] = attempt
            return None, error_payload
        except Exception as exc:  # noqa: BLE001
            return None, {"error": str(exc), "errorCode": "request_failed", "attempts": attempt}
    return None, {"error": "quote_retry_exhausted", "errorCode": "quote_retry_exhausted", "attempts": attempts}


def choose_quote_for_target(output_mint: str, output_decimals: int, target_output_raw: int, sol_usd_equivalent: float, source_balances_raw: dict):
    plans = []
    for source_mint, source_meta in SOURCE_ASSETS.items():
        balance_raw = source_balances_raw.get(source_mint, 0)
        if balance_raw <= 0:
            continue
        max_input_raw = min(balance_raw, max_input_raw_for_source(source_mint, sol_usd_equivalent))
        if max_input_raw <= 0:
            continue

        probe_input_raw = 1_000_000 if source_mint == USDC_MINT else 10_000_000
        probe_input_raw = min(max_input_raw, max(1, probe_input_raw))
        probe_quote, probe_error = quote_exact_in(source_mint, output_mint, probe_input_raw)
        if probe_quote is None:
            plans.append(
                {
                    "source_mint": source_mint,
                    "source_symbol": source_meta["symbol"],
                    "route_provider": "jupiter_quote_api_exact_in",
                    "error": probe_error,
                    "plan_status": "blocked_no_route",
                }
            )
            continue

        probe_out_raw = int(probe_quote.get("outAmount") or 0)
        if probe_out_raw <= 0:
            plans.append(
                {
                    "source_mint": source_mint,
                    "source_symbol": source_meta["symbol"],
                    "route_provider": "jupiter_quote_api_exact_in",
                    "error": {"error": "zero_out_amount", "errorCode": "zero_out_amount"},
                    "plan_status": "blocked_zero_output",
                }
            )
            continue

        estimated_input_raw = max(1, math.ceil(probe_input_raw * target_output_raw / probe_out_raw * 1.02))
        high = min(max_input_raw, max(probe_input_raw, estimated_input_raw))
        best_quote = None
        best_input_raw = None
        low = 1
        previous_high = None

        while True:
            quote, error = quote_exact_in(source_mint, output_mint, high)
            if quote is None:
                break
            out_raw = int(quote.get("outAmount") or 0)
            if out_raw >= target_output_raw:
                best_quote = quote
                best_input_raw = high
                previous_high = high
                break
            if high >= max_input_raw:
                previous_high = high
                break
            low = high + 1
            high = min(max_input_raw, max(high + 1, high * 2))

        if best_quote is None:
            plans.append(
                {
                    "source_mint": source_mint,
                    "source_symbol": source_meta["symbol"],
                    "route_provider": "jupiter_quote_api_exact_in",
                    "plan_status": "blocked_target_not_reached_within_cap",
                    "per_mint_usdc_cap": per_mint_usdc_cap,
                    "source_balance_raw": balance_raw,
                    "max_input_raw": max_input_raw,
                }
            )
            continue

        lo = low
        hi = best_input_raw
        while lo < hi:
            mid = (lo + hi) // 2
            quote, error = quote_exact_in(source_mint, output_mint, mid)
            if quote is None:
                lo = mid + 1
                continue
            if int(quote.get("outAmount") or 0) >= target_output_raw:
                hi = mid
                best_quote = quote
                best_input_raw = mid
            else:
                lo = mid + 1

        expected_output_raw = int(best_quote.get("outAmount") or 0)
        min_output_raw = int(best_quote.get("otherAmountThreshold") or 0)
        input_native, spend_usdc_equiv = source_native_from_raw(source_mint, best_input_raw, sol_usd_equivalent)
        route_labels = [
            text_value((step.get("swapInfo") or {}).get("label"))
            for step in (best_quote.get("routePlan") or [])
            if text_value((step.get("swapInfo") or {}).get("label"))
        ]
        plans.append(
            {
                "source_mint": source_mint,
                "source_symbol": source_meta["symbol"],
                "source_decimals": source_meta["decimals"],
                "source_balance_raw": balance_raw,
                "source_balance_native": round_or_none(balance_raw / (10 ** source_meta["decimals"]), 9),
                "route_provider": "jupiter_quote_api_exact_in",
                "route_labels": route_labels,
                "swap_mode": "ExactIn",
                "execution_path_status": "dry_run_only_no_approved_live_transaction",
                "input_amount_raw": best_input_raw,
                "input_amount_native": round_or_none(input_native, 9),
                "spend_usdc_equivalent": round_or_none(spend_usdc_equiv, 9),
                "expected_output_raw": expected_output_raw,
                "expected_output_native": round_or_none(output_native_from_raw(expected_output_raw, output_decimals), 9),
                "min_output_raw": min_output_raw,
                "min_output_native": round_or_none(output_native_from_raw(min_output_raw, output_decimals), 9),
                "quote_response": best_quote,
                "plan_status": "planned",
            }
        )

    successful = [plan for plan in plans if plan.get("plan_status") == "planned"]
    if not successful:
        return None, plans
    chosen = sorted(
        successful,
        key=lambda item: (
            item.get("spend_usdc_equivalent") if item.get("spend_usdc_equivalent") is not None else float("inf"),
            item.get("source_symbol"),
        ),
    )[0]
    return chosen, plans


run61_report = load_json(run61_report_path)
run63_report = load_json(run63_report_path)
run64b_report = load_json(run64b_report_path)
inventory_payload = load_json(inventory_path)

sol_usd_equivalent = float((run61_report.get("inputs") or {}).get("sol_usd_equivalent") or 1000.0)
held_input_candidates = (run64b_report.get("held_input_plan") or {}).get("selected_mints") or []
held_input_candidates = held_input_candidates[:held_input_count]
ata_candidates = (run64b_report.get("ata_plan") or {}).get("selected_mints") or []
ata_candidates = ata_candidates[:ata_count]

inventory_accounts = {
    text_value(entry.get("mint_pubkey")): entry
    for entry in (inventory_payload.get("accounts") or [])
    if entry.get("mint_pubkey")
}
source_balances_raw = {
    mint: int((inventory_accounts.get(mint) or {}).get("amount_raw") or 0)
    for mint in SOURCE_ASSETS
}

plan_items = []
total_planned_spend_usdc = 0.0
for item in held_input_candidates:
    mint = text_value(item.get("mint"))
    decimals = int(item.get("decimals") or 0)
    target_native = float(item.get("required_band_input_native_p90") or item.get("required_band_input_native_max") or 0.0)
    target_output_raw = int(round(target_native * (10 ** decimals)))
    current_balance_raw = int((inventory_accounts.get(mint) or {}).get("amount_raw") or 0)
    chosen_quote, all_quote_attempts = choose_quote_for_target(
        mint,
        decimals,
        target_output_raw,
        sol_usd_equivalent,
        source_balances_raw,
    )
    policy_verdict = "ok" if item.get("policy_ok") else text_value(item.get("policy_blocker")) or "unknown"
    if chosen_quote:
        total_planned_spend_usdc += chosen_quote.get("spend_usdc_equivalent") or 0.0
        chosen_quote["quote_url_dry_run"] = quote_api_url
        chosen_quote["dry_run_quote_command"] = (
            "python3 - <<'PY'\n"
            "import json, ssl, urllib.parse, urllib.request\n"
            f"params = {{'inputMint': '{chosen_quote['source_mint']}', 'outputMint': '{mint}', "
            f"'amount': '{chosen_quote['input_amount_raw']}', 'slippageBps': '{slippage_bps}'}}\n"
            f"url = '{quote_api_url}' + '?' + urllib.parse.urlencode(params)\n"
            "req = urllib.request.Request(url, headers={'Accept': 'application/json'})\n"
            "with urllib.request.urlopen(req, timeout=30, context=ssl.create_default_context()) as resp:\n"
            "    print(json.dumps(json.loads(resp.read().decode('utf-8')), indent=2))\n"
            "PY"
        )
        chosen_quote["transaction_command_dry_run"] = (
            f"echo DRY_RUN acquire inputMint={chosen_quote['source_mint']} outputMint={mint} "
            f"amountRaw={chosen_quote['input_amount_raw']} expectedOutRaw={chosen_quote['expected_output_raw']} "
            f"minOutRaw={chosen_quote['min_output_raw']} slippageBps={slippage_bps} provider=jupiter_quote_api_exact_in"
        )
        abort_conditions = [
            "policy_verdict != ok",
            "rpc_policy_snapshot_stable != true",
            "quote route unavailable",
            f"spend_usdc_equivalent > {per_mint_usdc_cap}",
            f"aggregate_spend_usdc_equivalent > {total_usdc_cap}",
            f"current signer balance for source asset < {chosen_quote['input_amount_native']}",
            f"min_output_raw < {target_output_raw}",
        ]
    else:
        abort_conditions = [
            "policy_verdict != ok",
            "no bounded acquisition route found within per-mint cap",
        ]
    plan_items.append(
        {
            "mint_pubkey": mint,
            "token_program": item.get("token_program"),
            "decimals": decimals,
            "extension_list": item.get("extensions") or [],
            "policy_verdict": policy_verdict,
            "route_status_classification": (
                "quote_backed_subset_ready_for_operator_review" if chosen_quote else "metadata_policy_safe_but_no_acquisition_route"
            ),
            "rpc_policy_snapshot_stable": bool(item.get("rpc_policy_snapshot_stable")),
            "current_signer_balance_raw": current_balance_raw,
            "current_signer_balance_native": round_or_none(output_native_from_raw(current_balance_raw, decimals), 9),
            "target_held_input_band_native": round_or_none(target_native, 9),
            "exact_acquisition_amount_raw": target_output_raw,
            "exact_acquisition_amount_native": round_or_none(target_native, 9),
            "source_asset": chosen_quote.get("source_symbol") if chosen_quote else None,
            "source_mint": chosen_quote.get("source_mint") if chosen_quote else None,
            "execution_path_status": chosen_quote.get("execution_path_status") if chosen_quote else "no_bounded_live_transaction_path",
            "max_spend_raw": chosen_quote.get("input_amount_raw") if chosen_quote else None,
            "max_spend_native": chosen_quote.get("input_amount_native") if chosen_quote else None,
            "max_spend_usdc_equivalent": chosen_quote.get("spend_usdc_equivalent") if chosen_quote else None,
            "max_slippage_bps": slippage_bps,
            "expected_output_raw": chosen_quote.get("expected_output_raw") if chosen_quote else None,
            "expected_output_native": chosen_quote.get("expected_output_native") if chosen_quote else None,
            "min_output_raw": chosen_quote.get("min_output_raw") if chosen_quote else None,
            "min_output_native": chosen_quote.get("min_output_native") if chosen_quote else None,
            "route_provider_tool": chosen_quote.get("route_provider") if chosen_quote else None,
            "route_labels": chosen_quote.get("route_labels") if chosen_quote else [],
            "transaction_command": chosen_quote.get("transaction_command_dry_run") if chosen_quote else None,
            "dry_run_quote_command": chosen_quote.get("dry_run_quote_command") if chosen_quote else None,
            "failure_abort_condition": abort_conditions,
            "quote_attempts": all_quote_attempts,
        }
    )

selected_ata_plan = []
for item in ata_candidates:
    mint = text_value(item.get("mint"))
    inventory_entry = inventory_accounts.get(mint) or {}
    selected_ata_plan.append(
        {
            "mint_pubkey": mint,
            "token_program": item.get("token_program"),
            "count": item.get("count"),
            "current_signer_balance_raw": int(inventory_entry.get("amount_raw") or 0),
            "ata_exists_in_inventory": bool(inventory_entry),
            "create_ata_command_dry_run": f"echo DRY_RUN create_output_ata mint={mint}",
            "failure_abort_condition": [
                "policy_verdict != ok",
                "ATA already exists and is initialized",
            ],
        }
    )

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run63_report_json": str(run63_report_path),
        "run64b_report_json": str(run64b_report_path),
        "inventory_path": str(inventory_path),
        "quote_api_url": quote_api_url,
        "per_mint_usdc_cap": per_mint_usdc_cap,
        "total_usdc_cap": total_usdc_cap,
        "slippage_bps": slippage_bps,
        "held_input_count": held_input_count,
        "ata_count": ata_count,
    },
    "inventory_snapshot": {
        "owner_pubkey": inventory_payload.get("owner_pubkey"),
        "source_asset_balances": {
            SOURCE_ASSETS[mint]["symbol"]: {
                "mint_pubkey": mint,
                "amount_raw": amount_raw,
                "amount_native": round_or_none(amount_raw / (10 ** SOURCE_ASSETS[mint]["decimals"]), 9),
            }
            for mint, amount_raw in source_balances_raw.items()
        },
    },
    "held_input_acquisition_plan": {
        "aggregate_planned_spend_usdc_equivalent": round_or_none(total_planned_spend_usdc, 9),
        "within_total_cap": total_planned_spend_usdc <= total_usdc_cap,
        "items": plan_items,
    },
    "ata_prep_plan": {
        "items": selected_ata_plan,
    },
}

summary_lines = [
    f"status={payload['status']}",
    f"source_usdc_balance={payload['inventory_snapshot']['source_asset_balances']['USDC']['amount_native']}",
    f"source_wsol_balance={payload['inventory_snapshot']['source_asset_balances']['WSOL']['amount_native']}",
    f"aggregate_planned_spend_usdc_equivalent={payload['held_input_acquisition_plan']['aggregate_planned_spend_usdc_equivalent']}",
    f"within_total_cap={str(payload['held_input_acquisition_plan']['within_total_cap']).lower()}",
]

for index, item in enumerate(plan_items, start=1):
    summary_lines.append(f"held_input_mint_{index}={item['mint_pubkey']}")
    summary_lines.append(f"held_input_mint_{index}_policy_verdict={item['policy_verdict']}")
    summary_lines.append(f"held_input_mint_{index}_target_native={item['target_held_input_band_native']}")
    summary_lines.append(f"held_input_mint_{index}_source_asset={item['source_asset'] or 'none'}")
    summary_lines.append(f"held_input_mint_{index}_max_spend_native={item['max_spend_native'] if item['max_spend_native'] is not None else 'none'}")
    summary_lines.append(f"held_input_mint_{index}_max_spend_usdc_equivalent={item['max_spend_usdc_equivalent'] if item['max_spend_usdc_equivalent'] is not None else 'none'}")
    summary_lines.append(f"held_input_mint_{index}_expected_output_native={item['expected_output_native'] if item['expected_output_native'] is not None else 'none'}")
    summary_lines.append(f"held_input_mint_{index}_min_output_native={item['min_output_native'] if item['min_output_native'] is not None else 'none'}")

task_lines = [
    "#!/usr/bin/env bash",
    "# Run64C operator tasks (generated, dry-run only)",
    "set -euo pipefail",
    "",
    f"# per_mint_usdc_cap={per_mint_usdc_cap}",
    f"# total_usdc_cap={total_usdc_cap}",
    f"# slippage_bps={slippage_bps}",
    "# This file is dry-run only. Do not execute live acquisition until an explicit operator path is approved.",
    "",
    "# 1. Refresh inventory before any future live prep.",
    f"#    {pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'}",
    "",
    "# 2. Dry-run held-input acquisition review:",
]

for item in plan_items:
    task_lines.append(f"#    mint_pubkey={item['mint_pubkey']}")
    task_lines.append(f"#    token_program={item['token_program']}")
    task_lines.append(f"#    extension_list={item['extension_list']}")
    task_lines.append(f"#    policy_verdict={item['policy_verdict']}")
    task_lines.append(f"#    route_status_classification={item['route_status_classification']}")
    task_lines.append(f"#    rpc_policy_snapshot_stable={str(item['rpc_policy_snapshot_stable']).lower()}")
    task_lines.append(f"#    source_asset={item['source_asset'] or 'none'}")
    task_lines.append(f"#    source_mint={item['source_mint'] or 'none'}")
    task_lines.append(f"#    execution_path_status={item['execution_path_status']}")
    task_lines.append(f"#    target_held_input_band_native={item['target_held_input_band_native']}")
    task_lines.append(f"#    exact_acquisition_amount_native={item['exact_acquisition_amount_native']}")
    task_lines.append(f"#    exact_acquisition_amount_raw={item['exact_acquisition_amount_raw']}")
    task_lines.append(f"#    max_spend_native={item['max_spend_native'] if item['max_spend_native'] is not None else 'none'}")
    task_lines.append(f"#    max_spend_usdc_equivalent={item['max_spend_usdc_equivalent'] if item['max_spend_usdc_equivalent'] is not None else 'none'}")
    task_lines.append(f"#    max_slippage_bps={item['max_slippage_bps']}")
    task_lines.append(f"#    expected_output_native={item['expected_output_native'] if item['expected_output_native'] is not None else 'none'}")
    task_lines.append(f"#    expected_output_raw={item['expected_output_raw'] if item['expected_output_raw'] is not None else 'none'}")
    task_lines.append(f"#    min_output_native={item['min_output_native'] if item['min_output_native'] is not None else 'none'}")
    task_lines.append(f"#    min_output_raw={item['min_output_raw'] if item['min_output_raw'] is not None else 'none'}")
    task_lines.append(f"#    provider={item['route_provider_tool'] or 'none'}")
    task_lines.append(f"#    route_labels={item['route_labels']}")
    if item["dry_run_quote_command"]:
        task_lines.append(f"#    exact_dry_run_quote_command_start")
        for line in item["dry_run_quote_command"].splitlines():
            task_lines.append(f"#    {line}")
        task_lines.append(f"#    exact_dry_run_quote_command_end")
    if item["transaction_command"]:
        task_lines.append(f"#    exact_transaction_command_dry_run={item['transaction_command']}")
    else:
        task_lines.append(f"#    exact_transaction_command_dry_run=none")
    task_lines.append("#    exact_transaction_command_live=not_available_no_approved_execution_path")
    for condition in item["failure_abort_condition"]:
        task_lines.append(f"#    abort_if={condition}")
    task_lines.append("")

task_lines += [
    "",
    "# 3. Dry-run ATA creation review:",
]
for item in selected_ata_plan:
    task_lines.append(f"#    mint={item['mint_pubkey']} ata_exists_in_inventory={str(item['ata_exists_in_inventory']).lower()}")
    task_lines.append(f"#    {item['create_ata_command_dry_run']}")
task_lines += [
    "",
    "# 4. After explicit approval and execution, rerun the bounded no-send stack:",
    f"#    sudo env SHADOW_RESULTS_DIR={run61_report_path.parent.parent} {pack_dir / 'scripts' / '61_post_shock_conversion.sh'}",
    f"#    sudo env SHADOW_RESULTS_DIR={run63_report_path.parent.parent} {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    f"#    sudo env SHADOW_RESULTS_DIR={run63_report_path.parent.parent} SHADOW_RUN63_ALLOW_METADATA_ONLY_TOKEN2022=true {pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}",
]

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

pass "Run64C acquisition plan written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

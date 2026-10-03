#!/usr/bin/env bash
# 65b_sim2_signed_swap_simulation.sh - signed Jupiter swap simulation for top 65B mints; no send/no acquisition

source "$(dirname "$0")/lib.sh"
script_header "65B-SIM2 - Signed Swap Simulation (No Send)"

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
STEP="${SHADOW_RUN65B_SIM2_STEP:-65b_sim2_signed_swap_simulation}"
STATUS_PREFIX="${SHADOW_RUN65B_SIM2_STATUS_PREFIX:-run65b_sim2_signed_swap_simulation}"
REPORT_BASENAME="${SHADOW_RUN65B_SIM2_REPORT_BASENAME:-run65b_sim2_signed_swap_simulation}"
RUN65B_SIM1_STEP="${SHADOW_RUN65B_SIM2_SOURCE_STEP:-65b_sim1_top_mint_simulation_plan_20260524_5m}"
SOURCE_ROWS="$RESULTS_ROOT/$RUN65B_SIM1_STEP/run65b_sim1_top_mint_simulation_plan_rows.jsonl"
SIGNER_KEYPAIR_PATH="${SHADOW_RUN65B_SIM2_SIGNER_KEYPAIR_PATH:-/etc/shadow-engine/worker-signer.json}"
RPC_URL="${SHADOW_RUN65B_SIM2_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN65B_SIM2_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"
SWAP_API_URL="${SHADOW_RUN65B_SIM2_SWAP_API_URL:-https://lite-api.jup.ag/swap/v1/swap}"
TOP_N="${SHADOW_RUN65B_SIM2_TOP_N:-3}"
MAX_INPUT_USDC="${SHADOW_RUN65B_SIM2_MAX_INPUT_USDC:-100.0}"
SLIPPAGE_BPS="${SHADOW_RUN65B_SIM2_SLIPPAGE_BPS:-100}"
EXIT_SLIPPAGE_BPS="${SHADOW_RUN65B_SIM2_EXIT_SLIPPAGE_BPS:-0}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN65B_SIM2_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_EXIT_VALUE_RATIO="${SHADOW_RUN65B_SIM2_MIN_EXIT_VALUE_RATIO:-0.50}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run65b_sim2_signed_swap_simulation_rows.jsonl"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$SOURCE_ROWS" ]]; then
    fail "Missing Run65B-SIM1 rows: $SOURCE_ROWS"
    finish
    exit 1
fi

TMP_SIGNER=""
SIGNER_STAGED="false"
cleanup() {
    if [[ -n "$TMP_SIGNER" && -f "$TMP_SIGNER" ]]; then
        if command -v shred >/dev/null 2>&1; then
            shred -u "$TMP_SIGNER" || rm -f "$TMP_SIGNER"
        else
            rm -f "$TMP_SIGNER"
        fi
    fi
}
trap cleanup EXIT

TMP_SIGNER="$(mktemp)"
if sudo -n cat "$SIGNER_KEYPAIR_PATH" >"$TMP_SIGNER" 2>/dev/null; then
    chmod 600 "$TMP_SIGNER"
    SIGNER_STAGED="true"
else
    rm -f "$TMP_SIGNER"
    TMP_SIGNER=""
fi

python3 - \
    "$SOURCE_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$TOP_N" \
    "$TMP_SIGNER" \
    "$SIGNER_STAGED" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$SWAP_API_URL" \
    "$MAX_INPUT_USDC" \
    "$SLIPPAGE_BPS" \
    "$EXIT_SLIPPAGE_BPS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_EXIT_VALUE_RATIO" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

rows_path = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
top_n = max(1, int(sys.argv[7] or "3"))
tmp_signer_path = Path(sys.argv[8]) if sys.argv[8] else None
signer_staged = (sys.argv[9] or "").lower() in {"1", "true", "yes", "on"}
rpc_url = sys.argv[10]
quote_api_url = sys.argv[11]
swap_api_url = sys.argv[12]
max_input_usdc = float(sys.argv[13] or "100.0")
slippage_bps = int(sys.argv[14] or "100")
exit_slippage_bps = int(sys.argv[15] or "0")
min_exit_value_usdc = float(sys.argv[16] or "0.10")
min_exit_value_ratio = float(sys.argv[17] or "0.50")


def load_jsonl(path: Path):
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


def write_jsonl(path: Path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return str(value or "").strip()


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def safe_int(value, default=0):
    try:
        return int(value or 0)
    except Exception:
        return default


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def http_json(url: str, payload=None, headers=None):
    final_headers = {"Accept": "application/json"}
    if headers:
        final_headers.update(headers)
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        final_headers.setdefault("Content-Type", "application/json")
    request = urllib.request.Request(url, data=data, headers=final_headers)
    with urllib.request.urlopen(request, timeout=45, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    return http_json(rpc_url, payload)


def quote_exact_in(input_mint: str, output_mint: str, amount_raw: int, bps: int):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, amount_raw)),
        "slippageBps": str(max(0, bps)),
    }
    url = quote_api_url + "?" + urllib.parse.urlencode(params)
    delay_seconds = 1.0
    for attempt in range(1, 4):
        try:
            return http_json(url), None
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            try:
                payload = json.loads(body)
            except Exception:
                payload = {"error": body}
            payload["http_status"] = exc.code
            payload["attempts"] = attempt
            if exc.code == 429 and attempt < 3:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            return None, payload
        except Exception as exc:
            if attempt < 3:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            return None, {"error": str(exc), "errorCode": "request_failed", "attempts": attempt}
    return None, {"error": "quote_retry_exhausted", "errorCode": "quote_retry_exhausted", "attempts": 3}


def build_swap_transaction(quote_response: dict, user_pubkey: str):
    body = {
        "userPublicKey": user_pubkey,
        "quoteResponse": quote_response,
        "wrapAndUnwrapSol": False,
        "dynamicComputeUnitLimit": True,
    }
    return http_json(swap_api_url, body, headers={"Content-Type": "application/json"})


def simulate_raw_tx(tx_bytes: bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
        "simulateTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "commitment": "confirmed",
                "sigVerify": True,
            },
        ],
    )


def extract_route_labels(quote_response: dict):
    return [
        text((step.get("swapInfo") or {}).get("label"))
        for step in (quote_response.get("routePlan") or [])
        if text((step.get("swapInfo") or {}).get("label"))
    ]


def simulation_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    value = result.get("value") if isinstance(result, dict) else None
    if not isinstance(value, dict):
        return False, value
    return value.get("err") is None, value


deps_ok = True
deps_error = None
owner_pubkey = None
owner_keypair = None
if signer_staged:
    try:
        from solders.keypair import Keypair
        from solders.transaction import VersionedTransaction

        owner_keypair = Keypair.from_json(tmp_signer_path.read_text(encoding="utf-8"))
        owner_pubkey = str(owner_keypair.pubkey())
    except Exception as exc:  # noqa: BLE001
        deps_ok = False
        deps_error = str(exc)

source = [row for row in load_jsonl(rows_path) if text(row.get("decision")) in {"PLAN_ONLY_SIGNED_SWAP_SIM_NOT_RUN", "SIMULATE_ONE_MINT_READY"}]
source.sort(
    key=lambda item: (
        safe_float(item.get("strict_ev_sol_equivalent")),
        safe_int(item.get("expected_would_fire_delta")),
    ),
    reverse=True,
)

out = []
for rank, row in enumerate(source[:top_n], start=1):
    mint = text(row.get("mint"))
    required_acquisition_usdc = safe_float(row.get("required_acquisition_usdc"))
    entry_amount_raw = int(round(required_acquisition_usdc * 1_000_000))
    first_blocker = None
    entry_quote = None
    entry_quote_error = None
    exit_quote = None
    exit_quote_error = None
    swap_prepare_error = None
    simulate_error = None
    swap_response = None
    simulation_response = None
    route_labels = []
    exit_route_labels = []
    signed_swap_simulation_status = "not_run"
    swap_tx_built = False
    swap_tx_signed_for_simulation = False
    simulate_transaction_called = False
    simulate_transaction_ok = False
    min_out_raw = 0
    expected_out_raw = 0
    exit_out_raw = 0
    exit_value_usdc = None
    exit_value_ratio = None

    if not signer_staged:
        first_blocker = "blocked_signer_unavailable"
        signed_swap_simulation_status = "blocked_signer_unavailable"
    elif not deps_ok:
        first_blocker = "blocked_missing_python_deps"
        signed_swap_simulation_status = "blocked_missing_python_deps"
    elif not mint:
        first_blocker = "blocked_missing_mint"
        signed_swap_simulation_status = "blocked_missing_mint"
    elif entry_amount_raw <= 0:
        first_blocker = "blocked_entry_amount_invalid"
        signed_swap_simulation_status = "blocked_entry_amount_invalid"
    elif required_acquisition_usdc > max_input_usdc:
        first_blocker = "blocked_input_cap"
        signed_swap_simulation_status = "blocked_input_cap"
    else:
        entry_quote, entry_quote_error = quote_exact_in(USDC_MINT, mint, entry_amount_raw, slippage_bps)
        expected_out_raw = safe_int((entry_quote or {}).get("outAmount"))
        min_out_raw = safe_int((entry_quote or {}).get("otherAmountThreshold"))
        route_labels = extract_route_labels(entry_quote or {})
        if entry_quote is None:
            first_blocker = "blocked_entry_quote_failed"
            signed_swap_simulation_status = "failed"
        elif min_out_raw <= 0:
            first_blocker = "blocked_min_out_invalid"
            signed_swap_simulation_status = "failed"
        else:
            exit_quote, exit_quote_error = quote_exact_in(mint, USDC_MINT, expected_out_raw, exit_slippage_bps)
            exit_out_raw = safe_int((exit_quote or {}).get("outAmount"))
            exit_value_usdc = exit_out_raw / 1_000_000.0 if exit_quote else None
            exit_value_ratio = (
                exit_value_usdc / required_acquisition_usdc
                if exit_value_usdc is not None and required_acquisition_usdc > 0
                else None
            )
            exit_route_labels = extract_route_labels(exit_quote or {})
            if exit_quote is None:
                first_blocker = "blocked_exit_quote_failed"
                signed_swap_simulation_status = "failed"
            elif exit_value_usdc is None or exit_value_usdc < min_exit_value_usdc:
                first_blocker = "blocked_exit_dust"
                signed_swap_simulation_status = "failed"
            elif exit_value_ratio is None or exit_value_ratio < min_exit_value_ratio:
                first_blocker = "blocked_exit_value_ratio"
                signed_swap_simulation_status = "failed"
            else:
                try:
                    swap_response = build_swap_transaction(entry_quote, owner_pubkey)
                    swap_tx_b64 = text((swap_response or {}).get("swapTransaction"))
                    swap_tx_built = bool(swap_tx_b64)
                    if not swap_tx_built:
                        raise RuntimeError("swapTransaction_missing")
                    unsigned_swap_tx = VersionedTransaction.from_bytes(base64.b64decode(swap_tx_b64))
                    signed_swap_tx = VersionedTransaction(unsigned_swap_tx.message, [owner_keypair])
                    swap_tx_signed_for_simulation = True
                    simulate_transaction_called = True
                    simulation_response = simulate_raw_tx(bytes(signed_swap_tx))
                    simulate_transaction_ok, sim_value = simulation_ok(simulation_response)
                    if simulate_transaction_ok:
                        signed_swap_simulation_status = "passed"
                    else:
                        signed_swap_simulation_status = "failed"
                        simulate_error = (sim_value or {}).get("err") if isinstance(sim_value, dict) else sim_value
                        first_blocker = "blocked_swap_simulation_failed"
                except Exception as exc:  # noqa: BLE001
                    signed_swap_simulation_status = "failed"
                    swap_prepare_error = str(exc)
                    first_blocker = "blocked_swap_prepare_failed" if not simulate_transaction_called else "blocked_swap_simulation_failed"

    gates = {
        "entry_quote_ok": entry_quote is not None,
        "min_out_positive": min_out_raw > 0,
        "exit_quote_ok": exit_quote is not None,
        "exit_value_non_dust": exit_value_usdc is not None and exit_value_usdc >= min_exit_value_usdc,
        "exit_value_ratio_ok": exit_value_ratio is not None and exit_value_ratio >= min_exit_value_ratio,
        "swap_tx_built": swap_tx_built,
        "swap_tx_signed_for_simulation": swap_tx_signed_for_simulation,
        "simulate_transaction_called": simulate_transaction_called,
        "simulate_transaction_ok": simulate_transaction_ok,
    }
    if signed_swap_simulation_status == "passed":
        decision = "SIMULATE_ONE_MINT_READY"
    elif first_blocker in {"blocked_exit_dust", "blocked_exit_value_ratio"}:
        decision = "REJECT_DUST_EXIT"
    elif first_blocker in {"blocked_swap_prepare_failed", "blocked_swap_simulation_failed"}:
        decision = "REJECT_ROUTE_UNSTABLE"
    elif first_blocker in {"blocked_input_cap"}:
        decision = "REJECT_LOW_EV"
    elif first_blocker == "blocked_missing_mint":
        decision = "REJECT_POLICY"
    else:
        decision = "RETRY_LATER"

    out.append({
        "rank": rank,
        "mint": mint,
        "source_decision": row.get("decision"),
        "required_acquisition_usdc": row.get("required_acquisition_usdc"),
        "entry_amount_raw": entry_amount_raw,
        "exit_value_usdc_source": row.get("exit_value_usdc"),
        "expected_would_fire_delta": safe_int(row.get("expected_would_fire_delta")),
        "strict_ev_sol_equivalent": row.get("strict_ev_sol_equivalent"),
        "policy_verdict": row.get("policy_verdict"),
        "recurrence_count": safe_int(row.get("recurrence_count")),
        "entry_quote_ok": entry_quote is not None,
        "entry_quote_error": entry_quote_error,
        "entry_route_plan_labels": route_labels,
        "expected_out_raw": expected_out_raw,
        "min_out_raw": min_out_raw,
        "exit_quote_ok": exit_quote is not None,
        "exit_quote_error": exit_quote_error,
        "exit_route_plan_labels": exit_route_labels,
        "exit_out_raw": exit_out_raw,
        "exit_value_usdc": round_or_none(exit_value_usdc, 9) if exit_value_usdc is not None else None,
        "exit_value_ratio": round_or_none(exit_value_ratio, 9) if exit_value_ratio is not None else None,
        "swap_tx_built": swap_tx_built,
        "swap_tx_signed_for_simulation": swap_tx_signed_for_simulation,
        "simulate_transaction_called": simulate_transaction_called,
        "simulate_transaction_ok": simulate_transaction_ok,
        "simulate_transaction_error": simulate_error,
        "swap_prepare_error": swap_prepare_error,
        "signed_swap_simulation_status": signed_swap_simulation_status,
        "gate_summary": gates,
        "first_blocker": first_blocker,
        "decision": decision,
        "execute_mode_allowed": False,
        "acquisition_allowed": False,
        "live_send_allowed": False,
        "send_transaction_called": False,
        "wire_send": 0,
        "live_landed": 0,
    })

write_jsonl(rows_jsonl, out)
decision_counts = Counter(row["decision"] for row in out)
blocker_counts = Counter(text(row.get("first_blocker")) or "none" for row in out)
summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_rows": str(rows_path),
    "top_n": top_n,
    "row_count": len(out),
    "signer_staged": signer_staged,
    "python_deps_ok": deps_ok,
    "python_deps_error": deps_error,
    "swap_tx_built_count": sum(1 for row in out if row["swap_tx_built"]),
    "swap_tx_signed_for_simulation_count": sum(1 for row in out if row["swap_tx_signed_for_simulation"]),
    "simulate_transaction_called_count": sum(1 for row in out if row["simulate_transaction_called"]),
    "signed_swap_simulation_passed_count": sum(1 for row in out if row["signed_swap_simulation_status"] == "passed"),
    "simulation_ready_count": sum(1 for row in out if row["decision"] == "SIMULATE_ONE_MINT_READY"),
    "decision_counts": dict(decision_counts),
    "first_blocker_counts": dict(blocker_counts),
    "execute_mode_allowed": False,
    "acquisition_allowed": False,
    "live_send_allowed": False,
    "send_transaction_called": False,
    "wire_send": 0,
    "live_landed": 0,
    "top_items": out,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_rows={rows_path}",
    f"row_count={summary['row_count']}",
    f"signer_staged={str(signer_staged).lower()}",
    f"python_deps_ok={str(deps_ok).lower()}",
    f"swap_tx_built_count={summary['swap_tx_built_count']}",
    f"swap_tx_signed_for_simulation_count={summary['swap_tx_signed_for_simulation_count']}",
    f"simulate_transaction_called_count={summary['simulate_transaction_called_count']}",
    f"signed_swap_simulation_passed_count={summary['signed_swap_simulation_passed_count']}",
    f"simulation_ready_count={summary['simulation_ready_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"first_blocker_counts={json.dumps(summary['first_blocker_counts'], sort_keys=True)}",
    "execute_mode_allowed=false",
    "acquisition_allowed=false",
    "live_send_allowed=false",
    "send_transaction_called=false",
    "wire_send=0",
    "live_landed=0",
]
if out:
    top = out[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_first_blocker={top['first_blocker']}",
        f"top_signed_swap_simulation_status={top['signed_swap_simulation_status']}",
        f"top_required_acquisition_usdc={top['required_acquisition_usdc']}",
        f"top_exit_value_usdc={top['exit_value_usdc']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run65B-SIM2 signed swap simulation written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

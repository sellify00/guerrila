#!/usr/bin/env bash
# 67_recurring_ata_precreation_planner.sh - rank recurring output ATA precreation targets from Run66

source "$(dirname "$0")/lib.sh"
script_header "67 - Recurring ATA Precreation Planner"

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
STEP="${SHADOW_RUN67_STEP:-67_recurring_ata_precreation_planner}"
STATUS_PREFIX="${SHADOW_RUN67_STATUS_PREFIX:-run67_recurring_ata_precreation_planner}"
REPORT_BASENAME="${SHADOW_RUN67_REPORT_BASENAME:-run67_recurring_ata_precreation_planner}"

RUN66_STEP="${SHADOW_RUN67_RUN66_STEP:-66_epoch1_monetization_forensic_audit}"
RUN66_RESULT_DIR="$RESULTS_ROOT/$RUN66_STEP"
RUN66_WOULD_FIRE_JSONL="$RUN66_RESULT_DIR/epoch1_would_fire_rows.jsonl"
RUN66_FORENSIC_SUMMARY_JSON="$RUN66_RESULT_DIR/epoch1_would_fire_forensic_summary.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
RPC_URL="${SHADOW_RUN67_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
QUOTE_API_URL="${SHADOW_RUN67_QUOTE_API_URL:-https://lite-api.jup.ag/swap/v1/quote}"

TARGET_TOTAL_CANDIDATES="${SHADOW_RUN67_TARGET_TOTAL_CANDIDATES:-20}"
TARGET_PER_OWNER_MODE="${SHADOW_RUN67_TARGET_PER_OWNER_MODE:-5}"
MIN_RECURRENCE_COUNT="${SHADOW_RUN67_MIN_RECURRENCE_COUNT:-2}"
MIN_EXPECTED_UNLOCK_USD="${SHADOW_RUN67_MIN_EXPECTED_UNLOCK_USD:-1.0}"
MIN_EXIT_PROBE_VALUE_USDC="${SHADOW_RUN67_MIN_EXIT_PROBE_VALUE_USDC:-0.10}"
EXIT_PROBE_SLIPPAGE_BPS="${SHADOW_RUN67_EXIT_PROBE_SLIPPAGE_BPS:-0}"
ALLOW_FREEZE_AUTHORITY="${SHADOW_RUN67_ALLOW_FREEZE_AUTHORITY:-false}"
ALLOW_MINT_AUTHORITY="${SHADOW_RUN67_ALLOW_MINT_AUTHORITY:-false}"
ALLOW_METADATA_ONLY_TOKEN2022="${SHADOW_RUN67_ALLOW_METADATA_ONLY_TOKEN2022:-false}"
ALLOW_FREEZE_AUTHORITY_MINTS="${SHADOW_RUN67_ALLOW_FREEZE_AUTHORITY_MINTS:-}"
ALLOW_MINT_AUTHORITY_MINTS="${SHADOW_RUN67_ALLOW_MINT_AUTHORITY_MINTS:-}"
ALLOWED_EXTENSIONS="${SHADOW_RUN67_ALLOWED_EXTENSIONS:-}"
DENYLIST_MINTS="${SHADOW_RUN67_DENYLIST_MINTS:-}"
MAX_DECIMALS="${SHADOW_RUN67_MAX_DECIMALS:-12}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
OWNER_MODE_PLAN_JSON="$RESULT_DIR/run67_owner_mode_ata_plan.json"
SELECTED_PLAN_JSON="$RESULT_DIR/run67_selected_ata_precreation_plan.json"
POLICY_AUDIT_JSON="$RESULT_DIR/run67_mint_policy_audit.json"
TASKS_FILE="$RESULT_DIR/run67_operator_tasks.sh"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$OWNER_MODE_PLAN_JSON" \
    "$SELECTED_PLAN_JSON" \
    "$POLICY_AUDIT_JSON" \
    "$TASKS_FILE"

missing=0
for required_path in \
    "$RUN66_WOULD_FIRE_JSONL" \
    "$RUN66_FORENSIC_SUMMARY_JSON" \
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
    "$RUN66_WOULD_FIRE_JSONL" \
    "$RUN66_FORENSIC_SUMMARY_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$OWNER_MODE_PLAN_JSON" \
    "$SELECTED_PLAN_JSON" \
    "$POLICY_AUDIT_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$QUOTE_API_URL" \
    "$TARGET_TOTAL_CANDIDATES" \
    "$TARGET_PER_OWNER_MODE" \
    "$MIN_RECURRENCE_COUNT" \
    "$MIN_EXPECTED_UNLOCK_USD" \
    "$MIN_EXIT_PROBE_VALUE_USDC" \
    "$EXIT_PROBE_SLIPPAGE_BPS" \
    "$ALLOW_FREEZE_AUTHORITY" \
    "$ALLOW_MINT_AUTHORITY" \
    "$ALLOW_METADATA_ONLY_TOKEN2022" \
    "$ALLOW_FREEZE_AUTHORITY_MINTS" \
    "$ALLOW_MINT_AUTHORITY_MINTS" \
    "$ALLOWED_EXTENSIONS" \
    "$DENYLIST_MINTS" \
    "$MAX_DECIMALS" \
    "$PACK_DIR" <<'PY'
import json
import ssl
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
SPL_TOKEN_PROGRAMS = {
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    TOKEN2022_PROGRAM,
}
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
OWNER_PRIORITY = {
    "directarb": 0,
    "backrun": 1,
    "continuation": 2,
    "rebalancing": 3,
    "housekeeping": 4,
    "flatten": 5,
    "drop": 6,
}

would_fire_rows_path = Path(sys.argv[1])
forensic_summary_path = Path(sys.argv[2])
inventory_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
owner_mode_plan_path = Path(sys.argv[6])
selected_plan_path = Path(sys.argv[7])
policy_audit_path = Path(sys.argv[8])
tasks_file = Path(sys.argv[9])
step = sys.argv[10]
status_prefix = sys.argv[11]
rpc_url = sys.argv[12]
quote_api_url = sys.argv[13]
target_total_candidates = int(sys.argv[14] or "20")
target_per_owner_mode = int(sys.argv[15] or "5")
min_recurrence_count = int(sys.argv[16] or "2")
min_expected_unlock_usd = float(sys.argv[17] or "1.0")
min_exit_probe_value_usdc = float(sys.argv[18] or "0.10")
exit_probe_slippage_bps = int(sys.argv[19] or "0")
allow_freeze_authority = (sys.argv[20] or "").lower() in {"1", "true", "yes", "on"}
allow_mint_authority = (sys.argv[21] or "").lower() in {"1", "true", "yes", "on"}
allow_metadata_only_token2022 = (sys.argv[22] or "").lower() in {"1", "true", "yes", "on"}
allow_freeze_authority_mints = sys.argv[23]
allow_mint_authority_mints = sys.argv[24]
allowed_extensions = sys.argv[25]
denylist_mints = sys.argv[26]
max_decimals = int(sys.argv[27] or "12")
pack_dir = Path(sys.argv[28])


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path):
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def text_value(value):
    return str(value or "").strip()


def split_csv(value):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


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


def placeholder_url(url):
    return not url or "CHANGE_ME" in url or "${" in url


def http_json(url):
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def rpc(url, method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def extension_names(value):
    if isinstance(value, list):
        names = []
        for item in value:
            if isinstance(item, str):
                names.append(item)
            elif isinstance(item, dict):
                names.append(
                    text_value(item.get("extension"))
                    or text_value(item.get("type"))
                    or text_value(item.get("name"))
                )
        return [name for name in names if name]
    if isinstance(value, dict):
        return [
            name
            for name in (
                text_value(value.get("extension")),
                text_value(value.get("type")),
                text_value(value.get("name")),
            )
            if name
        ]
    if isinstance(value, str):
        return [value]
    return []


def extension_detail_lookup(value):
    lookup = {}
    if not isinstance(value, list):
        return lookup
    for item in value:
        if not isinstance(item, dict):
            continue
        name = text_value(item.get("extension")) or text_value(item.get("type")) or text_value(item.get("name"))
        if name and name not in lookup:
            lookup[name] = item.get("state")
    return lookup


def normalize_default_account_state(value):
    text = text_value(value)
    return text.lower() if text else None


def metadata_policy_snapshot(metadata):
    return {
        "initialized": bool(metadata.get("initialized")),
        "token_program": metadata.get("token_program"),
        "decimals": metadata.get("decimals"),
        "freeze_authority": metadata.get("freeze_authority"),
        "mint_authority": metadata.get("mint_authority"),
        "extensions": sorted(metadata.get("extensions") or []),
        "default_account_state": normalize_default_account_state(metadata.get("default_account_state")),
    }


def fetch_mint_metadata(mint_pubkey):
    base = {
        "mint": mint_pubkey,
        "rpc_audit_ok": False,
        "rpc_policy_snapshot_stable": False,
        "initialized": False,
        "token_program": None,
        "decimals": None,
        "freeze_authority": None,
        "mint_authority": None,
        "extensions": [],
        "extensions_detail": [],
        "default_account_state": None,
        "rpc_error": None,
    }
    if mint_pubkey == WSOL_MINT:
        base.update(
            {
                "rpc_audit_ok": True,
                "rpc_policy_snapshot_stable": True,
                "initialized": True,
                "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "decimals": 9,
            }
        )
        return base
    if mint_pubkey in {USDC_MINT, USDT_MINT}:
        base.update(
            {
                "rpc_audit_ok": True,
                "rpc_policy_snapshot_stable": True,
                "initialized": True,
                "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "decimals": 6,
            }
        )
        return base
    if placeholder_url(rpc_url):
        base["rpc_error"] = "rpc_unconfigured"
        return base

    def parse_response(response):
        if isinstance(response, list):
            response = response[0] if response else {}
        if not isinstance(response, dict):
            raise ValueError("invalid_rpc_shape")
        if "error" in response:
            raise ValueError(str(response["error"]))
        result_blob = response.get("result") or {}
        if isinstance(result_blob, list):
            result_blob = result_blob[0] if result_blob else {}
        if not isinstance(result_blob, dict):
            raise ValueError("invalid_result_shape")
        value = result_blob.get("value") or {}
        if not isinstance(value, dict) or not value:
            raise ValueError("mint_not_found")
        data_blob = value.get("data") or {}
        if isinstance(data_blob, list):
            data_blob = data_blob[0] if data_blob else {}
        if not isinstance(data_blob, dict):
            data_blob = {}
        parsed = data_blob.get("parsed") or {}
        if isinstance(parsed, list):
            parsed = parsed[0] if parsed else {}
        if not isinstance(parsed, dict):
            parsed = {}
        info = parsed.get("info") or {}
        if isinstance(info, list):
            info = info[0] if info else {}
        if not isinstance(info, dict):
            info = {}
        extensions_detail = info.get("extensions") or parsed.get("extensions") or []
        extension_state = extension_detail_lookup(extensions_detail)
        default_account_state = info.get("defaultAccountState")
        if default_account_state is None and isinstance(extension_state.get("defaultAccountState"), dict):
            default_account_state = extension_state["defaultAccountState"].get("state")
        return {
            "rpc_audit_ok": True,
            "initialized": bool(info.get("isInitialized", False)),
            "token_program": text_value(value.get("owner")) or None,
            "decimals": info.get("decimals"),
            "freeze_authority": text_value(info.get("freezeAuthority")) or None,
            "mint_authority": text_value(info.get("mintAuthority")) or None,
            "extensions": extension_names(extensions_detail),
            "extensions_detail": extensions_detail,
            "default_account_state": default_account_state,
        }

    try:
        first = parse_response(
            rpc(
                rpc_url,
                "getAccountInfo",
                [mint_pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}],
            )
        )
        second = parse_response(
            rpc(
                rpc_url,
                "getAccountInfo",
                [mint_pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}],
            )
        )
        base.update(first)
        base["rpc_policy_snapshot_stable"] = metadata_policy_snapshot(first) == metadata_policy_snapshot(second)
    except Exception as exc:  # noqa: BLE001
        base["rpc_error"] = str(exc)
    return base


def audit_mint_policy(mint_pubkey, metadata, count_lookup=None):
    denylisted = mint_pubkey in denylist_mint_set
    token_program = metadata.get("token_program")
    decimals = metadata.get("decimals")
    freeze_authority = metadata.get("freeze_authority")
    mint_authority = metadata.get("mint_authority")
    extensions = metadata.get("extensions") or []
    default_account_state = normalize_default_account_state(metadata.get("default_account_state"))
    token_program_allowed = token_program in SPL_TOKEN_PROGRAMS
    freeze_authority_ok = (not freeze_authority) or allow_freeze_authority or mint_pubkey in allow_freeze_authority_mint_set
    mint_authority_ok = (not mint_authority) or allow_mint_authority or mint_pubkey in allow_mint_authority_mint_set
    decimals_sane = isinstance(decimals, int) and 0 <= decimals <= max_decimals
    initialized = bool(metadata.get("initialized"))
    rpc_audit_ok = bool(metadata.get("rpc_audit_ok"))
    rpc_policy_snapshot_stable = bool(metadata.get("rpc_policy_snapshot_stable"))
    metadata_only_extension_subset_ok = bool(extensions) and set(extensions).issubset({"metadataPointer", "tokenMetadata"})
    default_account_state_ok = default_account_state not in {"frozen"}
    metadata_only_token2022_eligible = (
        rpc_audit_ok
        and rpc_policy_snapshot_stable
        and initialized
        and token_program == TOKEN2022_PROGRAM
        and token_program_allowed
        and not denylisted
        and freeze_authority_ok
        and mint_authority_ok
        and decimals_sane
        and metadata_only_extension_subset_ok
        and default_account_state_ok
    )
    extensions_understood = (
        (not extensions)
        or all(ext in allowed_extension_set for ext in extensions)
        or (allow_metadata_only_token2022 and metadata_only_token2022_eligible)
    )
    if not rpc_audit_ok:
        policy_blocker = "rpc_audit_unavailable"
    elif not rpc_policy_snapshot_stable:
        policy_blocker = "rpc_policy_snapshot_unstable"
    elif denylisted:
        policy_blocker = "denylisted"
    elif not initialized:
        policy_blocker = "mint_uninitialized"
    elif not token_program_allowed:
        policy_blocker = "token_program_not_allowed"
    elif not freeze_authority_ok:
        policy_blocker = "freeze_authority_present"
    elif not mint_authority_ok:
        policy_blocker = "mint_authority_present"
    elif not decimals_sane:
        policy_blocker = "decimals_not_sane"
    elif not extensions_understood:
        policy_blocker = "extensions_not_understood"
    else:
        policy_blocker = "none"
    return {
        "mint": mint_pubkey,
        "count": (count_lookup or {}).get(mint_pubkey, 0),
        "rpc_audit_ok": rpc_audit_ok,
        "rpc_policy_snapshot_stable": rpc_policy_snapshot_stable,
        "initialized": initialized,
        "token_program": token_program,
        "decimals": decimals,
        "freeze_authority": freeze_authority,
        "mint_authority": mint_authority,
        "extensions": extensions,
        "default_account_state": default_account_state,
        "denylisted": denylisted,
        "token_program_allowed": token_program_allowed,
        "freeze_authority_ok": freeze_authority_ok,
        "mint_authority_ok": mint_authority_ok,
        "decimals_sane": decimals_sane,
        "extensions_understood": extensions_understood,
        "metadata_only_extension_subset_ok": metadata_only_extension_subset_ok,
        "default_account_state_ok": default_account_state_ok,
        "metadata_only_token2022_eligible": metadata_only_token2022_eligible,
        "metadata_only_token2022_policy_enabled": allow_metadata_only_token2022,
        "policy_ok": policy_blocker == "none",
        "policy_blocker": policy_blocker,
        "rpc_error": metadata.get("rpc_error"),
    }


def quote_exact_in(input_mint, output_mint, amount_raw, slippage_bps):
    params = {
        "inputMint": input_mint,
        "outputMint": output_mint,
        "amount": str(max(1, amount_raw)),
        "slippageBps": str(slippage_bps),
    }
    request_url = quote_api_url + "?" + urllib.parse.urlencode(params)
    attempts = 3
    delay_seconds = 1.0
    for attempt in range(1, attempts + 1):
        try:
            return http_json(request_url), None
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
            if attempt < attempts:
                time.sleep(delay_seconds)
                delay_seconds *= 2
                continue
            return None, {"error": str(exc), "errorCode": "request_failed", "attempts": attempt}
    return None, {"error": "quote_retry_exhausted", "errorCode": "quote_retry_exhausted", "attempts": attempts}


def route_labels(quote):
    labels = []
    for item in quote.get("routePlan") or []:
        if not isinstance(item, dict):
            continue
        swap_info = item.get("swapInfo") or {}
        label = text_value(swap_info.get("label"))
        if label:
            labels.append(label)
    return labels


def decimals_to_divisor(decimals):
    if isinstance(decimals, int) and 0 <= decimals <= max_decimals:
        return 10 ** decimals
    return None


def unit_probe_amount_raw(metadata):
    decimals = metadata.get("decimals")
    divisor = decimals_to_divisor(decimals)
    return divisor if divisor is not None else 1


def exit_probe_for_mint(mint_pubkey, metadata, policy):
    probe_raw = unit_probe_amount_raw(metadata)
    decimals = metadata.get("decimals")
    probe_native = (probe_raw / (10 ** decimals)) if isinstance(decimals, int) and 0 <= decimals <= max_decimals else None
    result = {
        "probe_method": "one_native_unit_probe",
        "probe_note": "Epoch1/Run66 rows do not preserve output raw amounts, so Run67 uses a 1-native-unit exit probe per mint.",
        "probe_input_amount_raw": probe_raw,
        "probe_input_native": round_or_none(probe_native, 9),
        "probe_target_mint": None,
        "probe_quote_ok": False,
        "probe_exit_status": "not_checked",
        "probe_exit_value_usdc": None,
        "probe_exit_value_sol": None,
        "probe_route_labels": [],
        "probe_error": None,
    }
    if not policy.get("policy_ok"):
        result["probe_exit_status"] = "not_checked_policy_blocked"
        return result
    if placeholder_url(quote_api_url):
        result["probe_exit_status"] = "quote_api_unconfigured"
        return result
    if mint_pubkey == USDC_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_output"
        result["probe_exit_value_usdc"] = round_or_none(probe_raw / 1_000_000.0, 6)
        result["probe_exit_value_sol"] = round_or_none((probe_raw / 1_000_000.0) / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        return result
    if mint_pubkey == USDT_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_like_output"
        result["probe_exit_value_usdc"] = round_or_none(probe_raw / 1_000_000.0, 6)
        result["probe_exit_value_sol"] = round_or_none((probe_raw / 1_000_000.0) / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        return result
    if mint_pubkey == WSOL_MINT:
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_status"] = "stable_like_output"
        result["probe_exit_value_sol"] = round_or_none(probe_raw / 1_000_000_000.0, 9)
        result["probe_exit_value_usdc"] = round_or_none((probe_raw / 1_000_000_000.0) * sol_usd_equivalent, 6)
        return result

    quote, error = quote_exact_in(mint_pubkey, USDC_MINT, probe_raw, exit_probe_slippage_bps)
    if quote is not None:
        out_raw = safe_int(quote.get("outAmount"))
        exit_usdc = out_raw / 1_000_000.0
        result["probe_target_mint"] = USDC_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_value_usdc"] = round_or_none(exit_usdc, 6)
        result["probe_exit_value_sol"] = round_or_none(exit_usdc / sol_usd_equivalent, 9) if sol_usd_equivalent else None
        result["probe_route_labels"] = route_labels(quote)
        result["probe_exit_status"] = "unit_probe_exit_quote_ok" if exit_usdc >= min_exit_probe_value_usdc and out_raw > 1 else "unit_probe_exit_quote_dust"
        return result

    fallback_quote, fallback_error = quote_exact_in(mint_pubkey, WSOL_MINT, probe_raw, exit_probe_slippage_bps)
    if fallback_quote is not None:
        out_raw = safe_int(fallback_quote.get("outAmount"))
        exit_sol = out_raw / 1_000_000_000.0
        exit_usdc = exit_sol * sol_usd_equivalent
        result["probe_target_mint"] = WSOL_MINT
        result["probe_quote_ok"] = True
        result["probe_exit_value_usdc"] = round_or_none(exit_usdc, 6)
        result["probe_exit_value_sol"] = round_or_none(exit_sol, 9)
        result["probe_route_labels"] = route_labels(fallback_quote)
        result["probe_exit_status"] = "unit_probe_exit_quote_ok" if exit_usdc >= min_exit_probe_value_usdc and out_raw > 1 else "unit_probe_exit_quote_dust"
        return result

    result["probe_target_mint"] = USDC_MINT
    result["probe_exit_status"] = "unit_probe_exit_quote_missing"
    result["probe_error"] = fallback_error or error
    return result


def candidate_sort_key(item):
    return (
        0 if item.get("selected_for_precreation") else 1,
        OWNER_PRIORITY.get(item.get("owner_mode"), 99),
        -safe_float(item.get("expected_unlock_ev_usd")),
        -safe_int(item.get("count")),
        item.get("mint_out") or "",
    )


def compact_top_pools(counter, limit=5):
    return [{"pool": pool, "count": count} for pool, count in counter.most_common(limit)]


def compact_top_venues(counter, limit=5):
    return [{"venue": venue, "count": count} for venue, count in counter.most_common(limit)]


would_fire_rows = load_jsonl(would_fire_rows_path)
forensic_summary = load_json(forensic_summary_path)
inventory_payload = load_json(inventory_path)

sol_usd_equivalent = safe_float((forensic_summary.get("unit_audit") or {}).get("sol_usd_equivalent"), 1000.0) or 1000.0
strict_current_rows = sum(1 for row in would_fire_rows if row.get("strict_current_state"))
strict_current_ev_usd = sum(safe_float(row.get("net_after_fee_usd")) for row in would_fire_rows if row.get("strict_current_state"))
inventory_accounts = inventory_payload.get("accounts") or []
existing_ata_mints = {
    text_value(entry.get("mint_pubkey"))
    for entry in inventory_accounts
    if text_value(entry.get("mint_pubkey"))
}
owner_pubkey = text_value(inventory_payload.get("owner_pubkey")) or None

ata_fixup_rows = [
    row
    for row in would_fire_rows
    if text_value(row.get("fixup_type")) in {"ata", "ata_template"}
    and text_value(row.get("usable_route_for_us")) in USABLE_ROUTE_FAMILIES
]

mint_occurrence_counter = Counter(text_value(row.get("mint_out")) for row in ata_fixup_rows if text_value(row.get("mint_out")))
unique_mints = sorted(mint_occurrence_counter)
allow_freeze_authority_mint_set = set(split_csv(allow_freeze_authority_mints))
allow_mint_authority_mint_set = set(split_csv(allow_mint_authority_mints))
allowed_extension_set = set(split_csv(allowed_extensions))
denylist_mint_set = set(split_csv(denylist_mints))

mint_metadata_lookup = {mint: fetch_mint_metadata(mint) for mint in unique_mints}
mint_policy_lookup = {
    mint: audit_mint_policy(mint, mint_metadata_lookup.get(mint) or {}, mint_occurrence_counter)
    for mint in unique_mints
}
exit_probe_lookup = {
    mint: exit_probe_for_mint(mint, mint_metadata_lookup.get(mint) or {}, mint_policy_lookup.get(mint) or {})
    for mint in unique_mints
}

aggregates = {}
for row in ata_fixup_rows:
    owner_mode = text_value(row.get("owner_mode")).lower() or "unknown"
    mint_out = text_value(row.get("mint_out"))
    if not mint_out:
        continue
    key = (owner_mode, mint_out)
    item = aggregates.get(key)
    if item is None:
        item = {
            "owner_mode": owner_mode,
            "mint_out": mint_out,
            "count": 0,
            "expected_unlock_rows": 0,
            "expected_unlock_ev_usd": 0.0,
            "strict_rows_current": 0,
            "strict_ev_usd_current": 0.0,
            "usable_route_count": 0,
            "would_enqueue_tpu_count": 0,
            "route_families": Counter(),
            "pools": Counter(),
            "venues": Counter(),
            "rows": [],
        }
        aggregates[key] = item
    item["count"] += 1
    item["expected_unlock_rows"] += 1
    item["expected_unlock_ev_usd"] += safe_float(row.get("net_after_fee_usd"))
    if row.get("strict_current_state"):
        item["strict_rows_current"] += 1
        item["strict_ev_usd_current"] += safe_float(row.get("net_after_fee_usd"))
    if text_value(row.get("usable_route_for_us")) in USABLE_ROUTE_FAMILIES:
        item["usable_route_count"] += 1
    if row.get("would_enqueue_tpu"):
        item["would_enqueue_tpu_count"] += 1
    route_family = text_value(row.get("usable_route_for_us")) or text_value(row.get("observed_route")) or "unknown"
    item["route_families"][route_family] += 1
    pool = text_value(row.get("pool")) or "unknown"
    venue = text_value(row.get("venue")) or "unknown"
    item["pools"][pool] += 1
    item["venues"][venue] += 1
    item["rows"].append(
        {
            "slot": safe_int(row.get("slot")),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "venue": row.get("venue"),
            "usable_route_for_us": row.get("usable_route_for_us"),
            "net_after_fee_usd": round_or_none(safe_float(row.get("net_after_fee_usd")), 6),
            "input_usd_equiv": round_or_none(safe_float(row.get("input_usd_equiv")), 6),
            "notional_bucket": row.get("notional_bucket"),
        }
    )

candidate_items = []
for item in aggregates.values():
    mint_out = item["mint_out"]
    policy = mint_policy_lookup.get(mint_out) or {}
    exit_probe = exit_probe_lookup.get(mint_out) or {}
    recurrence_count = mint_occurrence_counter.get(mint_out, 0)
    ata_exists_in_inventory = mint_out in existing_ata_mints
    core_gate_ok = (
        policy.get("policy_ok")
        and not ata_exists_in_inventory
        and item["count"] >= min_recurrence_count
        and item["expected_unlock_ev_usd"] >= min_expected_unlock_usd
        and item["usable_route_count"] > 0
    )
    liquidity_probe_ok = text_value(exit_probe.get("probe_exit_status")) in {
        "stable_output",
        "stable_like_output",
        "unit_probe_exit_quote_ok",
    }
    top_pool = item["pools"].most_common(1)[0] if item["pools"] else (None, 0)
    top_venue = item["venues"].most_common(1)[0] if item["venues"] else (None, 0)
    candidate_items.append(
        {
            "owner_mode": item["owner_mode"],
            "mint_out": mint_out,
            "count": item["count"],
            "recurrence_across_candidate_rows": recurrence_count,
            "expected_unlock_rows": item["expected_unlock_rows"],
            "expected_unlock_ev_usd": round_or_none(item["expected_unlock_ev_usd"], 6),
            "strict_rows_current": item["strict_rows_current"],
            "strict_ev_usd_current": round_or_none(item["strict_ev_usd_current"], 6),
            "usable_route_count": item["usable_route_count"],
            "would_enqueue_tpu_count": item["would_enqueue_tpu_count"],
            "top_pool": top_pool[0],
            "top_pool_count": top_pool[1],
            "top_venue": top_venue[0],
            "top_venue_count": top_venue[1],
            "pool_breakdown": compact_top_pools(item["pools"]),
            "venue_breakdown": compact_top_venues(item["venues"]),
            "route_families": [{"route_family": route, "count": count} for route, count in item["route_families"].most_common()],
            "ata_exists_in_inventory": ata_exists_in_inventory,
            "policy_audit": policy,
            "exit_probe": exit_probe,
            "core_precreate_gate_ok": core_gate_ok,
            "liquidity_probe_ok": liquidity_probe_ok,
            "selected_for_precreation": core_gate_ok and liquidity_probe_ok,
            "selection_reason": "selected"
            if core_gate_ok and liquidity_probe_ok
            else ("blocked_liquidity_probe" if core_gate_ok else "blocked_core_gate"),
        }
    )

owner_mode_candidates = defaultdict(list)
for item in sorted(candidate_items, key=candidate_sort_key):
    owner_mode_candidates[item["owner_mode"]].append(item)

selected_candidates = []
for owner_mode, items in sorted(owner_mode_candidates.items(), key=lambda kv: OWNER_PRIORITY.get(kv[0], 99)):
    chosen_for_owner = 0
    for item in items:
        if not item.get("selected_for_precreation"):
            continue
        if chosen_for_owner >= target_per_owner_mode or len(selected_candidates) >= target_total_candidates:
            break
        selected_candidates.append(item)
        chosen_for_owner += 1
    if len(selected_candidates) >= target_total_candidates:
        break

selected_keys = {(item["owner_mode"], item["mint_out"]) for item in selected_candidates}
for item in candidate_items:
    item["selected_in_top_plan"] = (item["owner_mode"], item["mint_out"]) in selected_keys

selected_mint_plan = {}
for item in selected_candidates:
    mint_out = item["mint_out"]
    plan_item = selected_mint_plan.get(mint_out)
    if plan_item is None:
        plan_item = {
            "mint_out": mint_out,
            "owner_modes": [],
            "owner_mode_breakdown": [],
            "expected_unlock_rows": 0,
            "expected_unlock_ev_usd": 0.0,
            "policy_audit": item.get("policy_audit") or {},
            "exit_probe": item.get("exit_probe") or {},
            "ata_exists_in_inventory": bool(item.get("ata_exists_in_inventory")),
            "core_precreate_gate_ok": bool(item.get("core_precreate_gate_ok")),
            "liquidity_probe_ok": bool(item.get("liquidity_probe_ok")),
            "selected_for_precreation": True,
        }
        selected_mint_plan[mint_out] = plan_item
    plan_item["owner_modes"].append(item.get("owner_mode"))
    plan_item["owner_mode_breakdown"].append(
        {
            "owner_mode": item.get("owner_mode"),
            "count": item.get("count"),
            "expected_unlock_rows": item.get("expected_unlock_rows"),
            "expected_unlock_ev_usd": item.get("expected_unlock_ev_usd"),
            "top_pool": item.get("top_pool"),
            "top_pool_count": item.get("top_pool_count"),
        }
    )
    plan_item["expected_unlock_rows"] += safe_int(item.get("expected_unlock_rows"))
    plan_item["expected_unlock_ev_usd"] += safe_float(item.get("expected_unlock_ev_usd"))

selected_unique_candidates = sorted(
    [
        {
            **item,
            "owner_modes": sorted(item["owner_modes"], key=lambda value: OWNER_PRIORITY.get(value, 99)),
            "expected_unlock_ev_usd": round_or_none(item["expected_unlock_ev_usd"], 6),
        }
        for item in selected_mint_plan.values()
    ],
    key=lambda item: (-safe_float(item.get("expected_unlock_ev_usd")), -safe_int(item.get("expected_unlock_rows")), item.get("mint_out") or ""),
)

owner_mode_projection = {}
for owner_mode, items in owner_mode_candidates.items():
    selected = [item for item in items if item.get("selected_in_top_plan")]
    owner_mode_projection[owner_mode] = {
        "selected_count": len(selected),
        "projected_strict_unlock_rows": sum(safe_int(item.get("expected_unlock_rows")) for item in selected),
        "projected_strict_unlock_ev_usd": round_or_none(sum(safe_float(item.get("expected_unlock_ev_usd")) for item in selected), 6),
    }

projection_rows = strict_current_rows + sum(safe_int(item.get("expected_unlock_rows")) for item in selected_candidates)
projection_ev_usd = strict_current_ev_usd + sum(safe_float(item.get("expected_unlock_ev_usd")) for item in selected_candidates)

owner_mode_plan = {
    owner_mode: {
        "all_candidates": items,
        "selected_candidates": [item for item in items if item.get("selected_in_top_plan")],
    }
    for owner_mode, items in sorted(owner_mode_candidates.items(), key=lambda kv: OWNER_PRIORITY.get(kv[0], 99))
}

policy_audit_rows = []
for mint in unique_mints:
    payload = dict(mint_policy_lookup.get(mint) or {})
    payload["exit_probe"] = exit_probe_lookup.get(mint) or {}
    payload["ata_exists_in_inventory"] = mint in existing_ata_mints
    payload["owner_modes"] = sorted({item["owner_mode"] for item in candidate_items if item["mint_out"] == mint})
    payload["expected_unlock_ev_usd"] = round_or_none(
        sum(safe_float(item.get("expected_unlock_ev_usd")) for item in candidate_items if item["mint_out"] == mint),
        6,
    )
    payload["expected_unlock_rows"] = sum(safe_int(item.get("expected_unlock_rows")) for item in candidate_items if item["mint_out"] == mint)
    policy_audit_rows.append(payload)
policy_audit_rows.sort(key=lambda item: (0 if item.get("policy_ok") else 1, -safe_float(item.get("expected_unlock_ev_usd")), item.get("mint") or ""))

selected_projection_by_owner = {
    owner_mode: owner_mode_projection.get(owner_mode) or {
        "selected_count": 0,
        "projected_strict_unlock_rows": 0,
        "projected_strict_unlock_ev_usd": 0.0,
    }
    for owner_mode in sorted(owner_mode_candidates, key=lambda value: OWNER_PRIORITY.get(value, 99))
}

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run66_would_fire_rows_jsonl": str(would_fire_rows_path),
        "run66_forensic_summary_json": str(forensic_summary_path),
        "inventory_path": str(inventory_path),
        "owner_pubkey": owner_pubkey,
        "rpc_url": rpc_url or None,
        "quote_api_url": quote_api_url or None,
    },
    "filters": {
        "min_recurrence_count": min_recurrence_count,
        "min_expected_unlock_usd": min_expected_unlock_usd,
        "min_exit_probe_value_usdc": min_exit_probe_value_usdc,
        "exit_probe_slippage_bps": exit_probe_slippage_bps,
        "target_total_candidates": target_total_candidates,
        "target_per_owner_mode": target_per_owner_mode,
        "selection_note": "Run67 uses a unit-sized exit probe because Run66 does not preserve output raw amounts.",
    },
    "current_state": {
        "would_fire_rows": len(would_fire_rows),
        "ata_fixup_rows": len(ata_fixup_rows),
        "strict_current_rows": strict_current_rows,
        "strict_current_ev_usd": round_or_none(strict_current_ev_usd, 6),
        "existing_inventory_mint_count": len(existing_ata_mints),
    },
    "selected_projection": {
        "selected_candidate_count": len(selected_unique_candidates),
        "selected_owner_mode_entry_count": len(selected_candidates),
        "projected_strict_rows_if_selected_atas_precreated": projection_rows,
        "projected_strict_ev_usd_if_selected_atas_precreated": round_or_none(projection_ev_usd, 6),
        "projected_unlock_rows_from_selected_atas": projection_rows - strict_current_rows,
        "projected_unlock_ev_usd_from_selected_atas": round_or_none(projection_ev_usd - strict_current_ev_usd, 6),
        "by_owner_mode": selected_projection_by_owner,
    },
    "selected_candidates": selected_unique_candidates,
    "selected_owner_mode_entries": selected_candidates,
    "owner_mode_plan": owner_mode_plan,
    "mint_policy_audit": policy_audit_rows,
}

summary_lines = [
    f"status={payload['status']}",
    f"would_fire_rows={payload['current_state']['would_fire_rows']}",
    f"ata_fixup_rows={payload['current_state']['ata_fixup_rows']}",
    f"strict_current_rows={payload['current_state']['strict_current_rows']}",
    f"strict_current_ev_usd={payload['current_state']['strict_current_ev_usd']}",
    f"selected_candidate_count={payload['selected_projection']['selected_candidate_count']}",
    f"projected_strict_rows_if_selected_atas_precreated={payload['selected_projection']['projected_strict_rows_if_selected_atas_precreated']}",
    f"projected_strict_ev_usd_if_selected_atas_precreated={payload['selected_projection']['projected_strict_ev_usd_if_selected_atas_precreated']}",
    f"projected_unlock_rows_from_selected_atas={payload['selected_projection']['projected_unlock_rows_from_selected_atas']}",
    f"projected_unlock_ev_usd_from_selected_atas={payload['selected_projection']['projected_unlock_ev_usd_from_selected_atas']}",
    f"policy_ok_mint_count={sum(1 for item in policy_audit_rows if item.get('policy_ok'))}",
    f"selected_with_liquidity_probe_ok_count={len(selected_unique_candidates)}",
    f"selected_owner_mode_entry_count={len(selected_candidates)}",
]

for owner_mode, projection in selected_projection_by_owner.items():
    summary_lines.append(f"{owner_mode}_selected_count={projection['selected_count']}")
    summary_lines.append(f"{owner_mode}_projected_unlock_rows={projection['projected_strict_unlock_rows']}")
    summary_lines.append(f"{owner_mode}_projected_unlock_ev_usd={projection['projected_strict_unlock_ev_usd']}")

for index, item in enumerate(selected_unique_candidates[:10], start=1):
    summary_lines.append(f"selected_{index}_owner_modes={','.join(item.get('owner_modes') or [])}")
    summary_lines.append(f"selected_{index}_mint_out={item.get('mint_out')}")
    summary_lines.append(f"selected_{index}_count={item.get('expected_unlock_rows')}")
    summary_lines.append(f"selected_{index}_expected_unlock_ev_usd={item.get('expected_unlock_ev_usd')}")
    summary_lines.append(f"selected_{index}_top_pool={(item.get('owner_mode_breakdown') or [{}])[0].get('top_pool')}")
    summary_lines.append(f"selected_{index}_exit_probe_status={(item.get('exit_probe') or {}).get('probe_exit_status')}")

rerun_epoch_name = "epoch2"
rerun_step = f"{rerun_epoch_name}_canonical_monetization"
forensic_compare_step = "67_epoch2_post_ata_forensic_audit"

task_lines = [
    "#!/usr/bin/env bash",
    "# Run67 operator tasks (generated, not auto-executed)",
    "set -euo pipefail",
    "",
    f"# strict_current_rows={payload['current_state']['strict_current_rows']}",
    f"# strict_current_ev_usd={payload['current_state']['strict_current_ev_usd']}",
    f"# projected_strict_rows_if_selected_atas_precreated={payload['selected_projection']['projected_strict_rows_if_selected_atas_precreated']}",
    f"# projected_strict_ev_usd_if_selected_atas_precreated={payload['selected_projection']['projected_strict_ev_usd_if_selected_atas_precreated']}",
    "",
    "# 1. Precreate only the selected recurring output ATAs below after reviewing token policy and the unit-sized exit probe.",
]
for item in selected_unique_candidates:
    exit_probe = item.get("exit_probe") or {}
    policy = item.get("policy_audit") or {}
    task_lines.append(
        "#    owner_modes={} mint={} rows={} expected_unlock_ev_usd={} token_program={} policy_blocker={} exit_probe_status={} exit_probe_value_usdc={}".format(
            ",".join(item.get("owner_modes") or []),
            item.get("mint_out"),
            item.get("expected_unlock_rows"),
            item.get("expected_unlock_ev_usd"),
            policy.get("token_program") or "unknown",
            policy.get("policy_blocker") or "none",
            exit_probe.get("probe_exit_status") or "unknown",
            exit_probe.get("probe_exit_value_usdc") if exit_probe.get("probe_exit_value_usdc") is not None else "none",
        )
    )
    task_lines.append(
        "#    create_output_ata mint={} owner_modes={} recurrence={} expected_unlock_ev_usd={}".format(
            item.get("mint_out"),
            ",".join(item.get("owner_modes") or []),
            item.get("expected_unlock_rows"),
            item.get("expected_unlock_ev_usd"),
        )
    )
task_lines += [
    "",
    "# 2. Refresh signer inventory immediately after ATA creation.",
    f"#    {pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'}",
    "",
    "# 3. Rerun a fresh canonical epoch only after the ATA surface is updated.",
    f"#    sudo env SHADOW_EPOCH_NAME={rerun_epoch_name} SHADOW_EPOCH_WINDOW_SECONDS=900 {pack_dir / 'scripts' / 'epoch_canonical_monetization.sh'}",
    "",
    "# 4. Re-run the forensic split on the fresh epoch to compare strict vs fixup conversion.",
    "#    This reuses the Run66 forensic logic with a new results step name.",
    (
        f"#    sudo env SHADOW_RUN66_RUN61_STEP={rerun_step} "
        f"SHADOW_RUN66_RUN62_STEP={rerun_step}_run62 "
        f"SHADOW_RUN66_RUN39_STEP={rerun_step}_run55_run39 "
        f"SHADOW_RUN66_STEP={forensic_compare_step} "
        f"SHADOW_RUN66_STATUS_PREFIX={forensic_compare_step} "
        f"SHADOW_RUN66_REPORT_BASENAME={forensic_compare_step} "
        f"{pack_dir / 'scripts' / '66_epoch1_monetization_forensic_audit.sh'}"
    ),
]

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
owner_mode_plan_path.write_text(json.dumps(owner_mode_plan, indent=2) + "\n", encoding="utf-8")
selected_plan_path.write_text(json.dumps(selected_unique_candidates, indent=2) + "\n", encoding="utf-8")
policy_audit_path.write_text(json.dumps(policy_audit_rows, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

pass "Run67 ATA planner written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

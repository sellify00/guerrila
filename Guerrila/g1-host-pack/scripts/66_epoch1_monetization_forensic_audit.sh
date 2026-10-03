#!/usr/bin/env bash
# 66_epoch1_monetization_forensic_audit.sh - forensic audit of Epoch1 monetization surface

source "$(dirname "$0")/lib.sh"
script_header "66 - Epoch1 Monetization Forensic Audit"

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

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66_STEP:-66_epoch1_monetization_forensic_audit}"
STATUS_PREFIX="${SHADOW_RUN66_STATUS_PREFIX:-run66_epoch1_monetization_forensic_audit}"
REPORT_BASENAME="${SHADOW_RUN66_REPORT_BASENAME:-run66_epoch1_monetization_forensic_audit}"

RUN61_STEP="${SHADOW_RUN66_RUN61_STEP:-epoch1_canonical_monetization}"
RUN62_STEP="${SHADOW_RUN66_RUN62_STEP:-${RUN61_STEP}_run62}"
RUN39_STEP="${SHADOW_RUN66_RUN39_STEP:-${RUN61_STEP}_run55_run39}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN39_RESULT_DIR="$RESULTS_ROOT/$RUN39_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN62_SUMMARY="$RUN62_RESULT_DIR/summary.txt"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RUN39_RESULT_DIR/candidate_autopsy.jsonl"

RPC_URL="${SHADOW_RUN66_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
ALLOW_RECONSTRUCTION_MISMATCH="${SHADOW_RUN66_ALLOW_RECONSTRUCTION_MISMATCH:-false}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
WOULD_FIRE_ROWS_JSONL="$RESULT_DIR/epoch1_would_fire_rows.jsonl"
FORENSIC_SUMMARY_JSON="$RESULT_DIR/epoch1_would_fire_forensic_summary.json"
HOUSEKEEPING_REPORT_JSON="$RESULT_DIR/epoch1_housekeeping_forensic_report.json"
DIRECTARB_REPORT_JSON="$RESULT_DIR/epoch1_directarb_forensic_report.json"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$WOULD_FIRE_ROWS_JSONL" \
    "$FORENSIC_SUMMARY_JSON" \
    "$HOUSEKEEPING_REPORT_JSON" \
    "$DIRECTARB_REPORT_JSON"

missing=0
for required_path in \
    "$RUN61_REPORT_JSON" \
    "$RUN62_SUMMARY" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL"
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
    "$RUN62_SUMMARY" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$WOULD_FIRE_ROWS_JSONL" \
    "$FORENSIC_SUMMARY_JSON" \
    "$HOUSEKEEPING_REPORT_JSON" \
    "$DIRECTARB_REPORT_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$ALLOW_RECONSTRUCTION_MISMATCH" <<'PY'
import json
import math
import ssl
import sys
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
PRIVATE_ONLY_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}
FIXABLE_EXECUTION_BLOCKERS = {"missing_output_ata", "output_ata_uninitialized"}
TEMPLATE_FIXABLE_VENUES = {"raydium_clmm", "raydium_cpmm", "raydium_v4", "meteora_damm_v2", "pumpfun_amm"}
BASE_FEE_LAMPORTS = 5_000
BASE_FEE_SOL = BASE_FEE_LAMPORTS / 1_000_000_000.0

run61_report_path = Path(sys.argv[1])
run62_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
summary_file = Path(sys.argv[4])
summary_json = Path(sys.argv[5])
would_fire_rows_path = Path(sys.argv[6])
forensic_summary_path = Path(sys.argv[7])
housekeeping_report_path = Path(sys.argv[8])
directarb_report_path = Path(sys.argv[9])
step = sys.argv[10]
status_prefix = sys.argv[11]
rpc_url = sys.argv[12]
allow_reconstruction_mismatch = (sys.argv[13] or "").lower() in {"1", "true", "yes", "on"}


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def load_summary(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key] = value
    return data


def text_value(value):
    return (value or "").strip()


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def split_csv(value):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def usable_route_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def is_afxdp_row(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def positive_research(row):
    return bool(row.get("research_positive")) or safe_int(row.get("net_after_fee_lamports")) > 0


def inventory_ready(row):
    approved = bool_field(row, "approved_held_input", "is_approved_held_input")
    balance_ok = bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")
    return approved and balance_ok


def strict_output_account_ready(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def strict_template_ready(row):
    return bool_field(row, "template_ready", "would_execute_current_template")


def ata_fixup_needed(row):
    return text_value(row.get("execution_blocker")).lower() in FIXABLE_EXECUTION_BLOCKERS or text_value(
        row.get("first_rejection_stage")
    ).lower() == "output_account_not_ready"


def template_fixup_needed(row):
    return text_value(row.get("first_rejection_stage")).lower() == "template_not_ready" and text_value(
        row.get("venue")
    ).lower() in TEMPLATE_FIXABLE_VENUES


def relaxed_output_account_ready(row):
    return strict_output_account_ready(row) or ata_fixup_needed(row)


def relaxed_template_ready(row):
    return strict_template_ready(row) or template_fixup_needed(row)


def strict_execution_blocker_ok(row):
    return text_value(row.get("execution_blocker")).lower() in {"", "none"}


def relaxed_execution_blocker_ok(row):
    return text_value(row.get("execution_blocker")).lower() in ({"", "none"} | FIXABLE_EXECUTION_BLOCKERS)


def quote_ready(row):
    return bool_field(row, "quote_ready_or_classified", "would_admit") or text_value(row.get("r1_outcome")).lower() == "admitted"


def strict_current_state_would_fire(row):
    return (
        text_value(row.get("selected_owner")).lower() in ACTIVE_FIRE_OWNERS
        and usable_route_for_us(row) is not None
        and is_afxdp_row(row)
        and positive_research(row)
        and inventory_ready(row)
        and bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")
        and strict_output_account_ready(row)
        and strict_template_ready(row)
        and quote_ready(row)
        and strict_execution_blocker_ok(row)
    )


def fixup_allowed_would_fire(row):
    return (
        text_value(row.get("selected_owner")).lower() in ACTIVE_FIRE_OWNERS
        and usable_route_for_us(row) is not None
        and is_afxdp_row(row)
        and positive_research(row)
        and inventory_ready(row)
        and bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")
        and relaxed_output_account_ready(row)
        and relaxed_template_ready(row)
        and quote_ready(row)
        and relaxed_execution_blocker_ok(row)
    )


def fixup_type(row):
    ata = ata_fixup_needed(row) and not strict_output_account_ready(row)
    template = template_fixup_needed(row) and not strict_template_ready(row)
    if ata and template:
        return "ata_template"
    if ata:
        return "ata"
    if template:
        return "template"
    return "none"


def route_bucket(row):
    value = route_family(row)
    if value in PRIVATE_ONLY_ROUTE_FAMILIES:
        return "route_private_only"
    if value not in USABLE_ROUTE_FAMILIES:
        return "route_unavailable"
    return value


def directarb_blocker(row):
    if usable_route_for_us(row) is None:
        return route_bucket(row)
    if not bool_field(row, "approved_held_input", "is_approved_held_input"):
        return "not_held_input"
    if not bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient"):
        return "inventory_insufficient"
    if not bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized"):
        return "input_account_not_ready"
    if not strict_output_account_ready(row):
        return "account_ata_missing"
    if not strict_template_ready(row):
        return "template_missing"
    if not quote_ready(row):
        return "quote_missing"
    if not positive_research(row):
        return "unit_negative"
    blocker = text_value(row.get("execution_blocker")).lower()
    if blocker not in {"", "none"}:
        return blocker
    return "none"


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=45, context=ssl.create_default_context()) as response:
        return json.loads(response.read().decode("utf-8"))


def fetch_mint_metadata(mints):
    result = {
        WSOL_MINT: {"decimals": 9, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDC_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDT_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
    }
    pending = [mint for mint in sorted(set(mints)) if mint and mint not in result]
    for index in range(0, len(pending), 100):
        chunk = pending[index:index + 100]
        response = rpc("getMultipleAccounts", [chunk, {"encoding": "jsonParsed", "commitment": "confirmed"}])
        if isinstance(response, list):
            response = response[0] if response else {}
        if not isinstance(response, dict):
            response = {}
        result_blob = response.get("result")
        if isinstance(result_blob, list):
            result_blob = result_blob[0] if result_blob else {}
        if not isinstance(result_blob, dict):
            result_blob = {}
        values = result_blob.get("value") or []
        if not isinstance(values, list):
            values = []
        for mint, value in zip(chunk, values):
            if not isinstance(value, dict):
                result[mint] = {"decimals": None, "token_program": None}
                continue
            data_blob = value.get("data")
            if isinstance(data_blob, list):
                data_blob = data_blob[0] if data_blob else {}
            if not isinstance(data_blob, dict):
                data_blob = {}
            parsed_blob = data_blob.get("parsed")
            if isinstance(parsed_blob, list):
                parsed_blob = parsed_blob[0] if parsed_blob else {}
            if not isinstance(parsed_blob, dict):
                parsed_blob = {}
            info = parsed_blob.get("info")
            if isinstance(info, list):
                info = info[0] if info else {}
            if not isinstance(info, dict):
                info = {}
            result[mint] = {
                "decimals": info.get("decimals"),
                "token_program": text_value(value.get("owner")) or None,
            }
    return result


def input_decimals(mint, mint_metadata):
    info = mint_metadata.get(mint) or {}
    value = info.get("decimals")
    return value if isinstance(value, int) else None


def normalize_raw_amount(raw_value, mint, mint_metadata):
    raw_value = safe_int(raw_value)
    decimals = input_decimals(mint, mint_metadata)
    if decimals is None:
        return None
    return raw_value / (10 ** decimals)


def equivalent_values(native_value, mint):
    if native_value is None:
        return None, None, "unknown"
    if mint == WSOL_MINT:
        return native_value * SOL_USD_EQUIVALENT, native_value, "wsol_native"
    if mint in {USDC_MINT, USDT_MINT}:
        return native_value, native_value / SOL_USD_EQUIVALENT if SOL_USD_EQUIVALENT else None, "stable_native"
    return None, None, "unknown_token_native"


def base_fee_in_input_native(mint):
    if mint == WSOL_MINT:
        return BASE_FEE_SOL
    if mint in {USDC_MINT, USDT_MINT}:
        return BASE_FEE_SOL * SOL_USD_EQUIVALENT
    return None


def classify_notional_bucket(input_sol_equiv, input_usd_equiv):
    if input_sol_equiv is not None:
        caps = [
            (0.005, "lte_5_usdc_or_0_005_sol_equiv"),
            (0.01, "lte_10_usdc_or_0_01_sol_equiv"),
            (0.025, "lte_25_usdc_or_0_025_sol_equiv"),
            (0.05, "lte_50_usdc_or_0_05_sol_equiv"),
            (0.1, "lte_100_usdc_or_0_1_sol_equiv"),
            (0.25, "lte_250_usdc_or_0_25_sol_equiv"),
            (0.5, "lte_500_usdc_or_0_5_sol_equiv"),
            (1.0, "lte_1000_usdc_or_1_0_sol_equiv"),
            (2.0, "lte_2000_usdc_or_2_0_sol_equiv"),
        ]
        for cap, label in caps:
            if input_sol_equiv <= cap:
                return label
        return "gt_2000_usdc_or_2_0_sol_equiv"
    if input_usd_equiv is not None:
        caps = [
            (5, "lte_5_usdc_or_0_005_sol_equiv"),
            (10, "lte_10_usdc_or_0_01_sol_equiv"),
            (25, "lte_25_usdc_or_0_025_sol_equiv"),
            (50, "lte_50_usdc_or_0_05_sol_equiv"),
            (100, "lte_100_usdc_or_0_1_sol_equiv"),
            (250, "lte_250_usdc_or_0_25_sol_equiv"),
            (500, "lte_500_usdc_or_0_5_sol_equiv"),
            (1000, "lte_1000_usdc_or_1_0_sol_equiv"),
            (2000, "lte_2000_usdc_or_2_0_sol_equiv"),
        ]
        for cap, label in caps:
            if input_usd_equiv <= cap:
                return label
        return "gt_2000_usdc_or_2_0_sol_equiv"
    return "unbucketed_other_input_units"


run61_payload = load_json(run61_report_path)
run62_summary = load_summary(run62_summary_path)
rows = load_jsonl(candidate_autopsy_path)

SOL_USD_EQUIVALENT = safe_float((run61_payload.get("inputs") or {}).get("sol_usd_equivalent")) or 1000.0
expected_would_fire_total = safe_int(((run61_payload.get("continuous_simulation") or {}).get("would_fire_total")))
expected_would_fire_counts = (run61_payload.get("continuous_simulation") or {}).get("would_fire_counts") or {}
approved_held_input_mints = set(split_csv(((run61_payload.get("top_afxdp_candidates") or [{}])[0].get("approved_held_input_mints"))))

# The approved/held mint lists live in the run39 summary, but Run62 summary already preserves the counts only.
# For per-row audit we need only a conservative "held-style output" classification, so derive it from the live row flags and direct known held inputs.
known_held_like_inputs = {WSOL_MINT, USDC_MINT, USDT_MINT}
unique_mints = {
    text_value(row.get("mint_in"))
    for row in rows
    if text_value(row.get("mint_in"))
}.union(
    {
        text_value(row.get("mint_out"))
        for row in rows
        if text_value(row.get("mint_out"))
    }
)
mint_metadata = fetch_mint_metadata(unique_mints)

would_fire_rows = []
strict_rows = []
ata_fixup_rows = []
template_fixup_rows = []
fixup_allowed_rows = []
heuristic_only_count = 0
route_leak_count = 0
owner_counts = Counter()

for row in rows:
    strict_fire = strict_current_state_would_fire(row)
    fixup_fire = fixup_allowed_would_fire(row)
    if not fixup_fire:
        continue

    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    input_native_size = normalize_raw_amount(row.get("trade_size_lamports"), mint_in, mint_metadata)
    input_decimals_value = input_decimals(mint_in, mint_metadata)
    output_decimals_value = input_decimals(mint_out, mint_metadata)
    input_usd_equiv, input_sol_equiv, input_conversion_source = equivalent_values(input_native_size, mint_in)
    net_after_fee_native = normalize_raw_amount(row.get("net_after_fee_lamports"), mint_in, mint_metadata)
    net_after_fee_usd, net_after_fee_sol, net_conversion_source = equivalent_values(net_after_fee_native, mint_in)
    retained_ev_native = normalize_raw_amount(row.get("retained_ev_lamports"), mint_in, mint_metadata)
    retained_ev_usd, retained_ev_sol, retained_conversion_source = equivalent_values(retained_ev_native, mint_in)
    policy_deduction_raw = safe_int(row.get("retained_ev_lamports")) - safe_int(row.get("net_after_fee_lamports"))
    policy_deduction_native = normalize_raw_amount(policy_deduction_raw, mint_in, mint_metadata)
    base_fee_input_native = base_fee_in_input_native(mint_in)
    policy_tip_input_native = None
    if policy_deduction_native is not None and base_fee_input_native is not None:
        policy_tip_input_native = max(policy_deduction_native - base_fee_input_native, 0.0)
    policy_tip_usd, policy_tip_sol, _ = equivalent_values(policy_tip_input_native, mint_in)
    retained_ev = retained_ev_native if retained_ev_native is not None else safe_int(row.get("retained_ev_lamports"))
    observed_route = route_family(row)
    usable_route = usable_route_for_us(row)
    current_fixup_type = fixup_type(row)
    current_fixup_used = (not strict_fire) and current_fixup_type != "none"
    heuristic_ev_only = (
        input_decimals_value is None
        or output_decimals_value is None
        or safe_int(row.get("quote_expected_out_lamports")) <= 0
        or safe_int(row.get("min_out_lamports")) <= 0
        or net_after_fee_usd is None
        or net_after_fee_sol is None
    )
    if heuristic_ev_only:
        heuristic_only_count += 1
    if usable_route not in {"tpu_quic_public", "public_rpc_scout"}:
        route_leak_count += 1

    owner = text_value(row.get("selected_owner")).lower()
    owner_counts[owner] += 1

    record = {
        "candidate_id": row.get("candidate_id"),
        "slot": safe_int(row.get("slot")),
        "signature": text_value(row.get("signature")) or None,
        "venue": text_value(row.get("venue")) or None,
        "pool": text_value(row.get("pool")) or None,
        "mint_in": mint_in or None,
        "mint_out": mint_out or None,
        "owner_mode": owner or None,
        "source": text_value(row.get("source")) or None,
        "observed_route": observed_route,
        "usable_route_for_us": usable_route,
        "input_decimals": input_decimals_value,
        "output_decimals": output_decimals_value,
        "input_native_size": round_or_none(input_native_size, 9),
        "input_usd_equiv": round_or_none(input_usd_equiv, 9),
        "input_sol_equiv": round_or_none(input_sol_equiv, 9),
        "notional_bucket": classify_notional_bucket(input_sol_equiv, input_usd_equiv),
        "net_after_fee_input_native": round_or_none(net_after_fee_native, 9),
        "net_after_fee_usd": round_or_none(net_after_fee_usd, 9),
        "net_after_fee_sol": round_or_none(net_after_fee_sol, 9),
        "retained_ev": round_or_none(retained_ev_native, 9) if retained_ev_native is not None else retained_ev,
        "retained_ev_raw_units": safe_int(row.get("retained_ev_lamports")),
        "retained_ev_usd": round_or_none(retained_ev_usd, 9),
        "retained_ev_sol": round_or_none(retained_ev_sol, 9),
        "policy_tip": round_or_none(policy_tip_input_native, 9),
        "policy_tip_usd": round_or_none(policy_tip_usd, 9),
        "policy_tip_sol": round_or_none(policy_tip_sol, 9),
        "policy_tip_plus_base_fee_raw_units": policy_deduction_raw,
        "base_fee": BASE_FEE_LAMPORTS,
        "base_fee_lamports": BASE_FEE_LAMPORTS,
        "base_fee_input_native": round_or_none(base_fee_input_native, 9),
        "execution_blocker": text_value(row.get("execution_blocker")).lower() or "none",
        "fixup_used": current_fixup_used,
        "fixup_type": current_fixup_type,
        "template_ready": strict_template_ready(row),
        "output_account_ready": strict_output_account_ready(row),
        "inventory_ready": inventory_ready(row),
        "route_usable": usable_route is not None,
        "would_enqueue_tpu": usable_route == "tpu_quic_public",
        "strict_current_state": strict_fire,
        "quote_ready": quote_ready(row),
        "heuristic_ev_only": heuristic_ev_only,
        "conversion_source": {
            "input": input_conversion_source,
            "net_after_fee": net_conversion_source,
            "retained_ev": retained_conversion_source,
        },
        "why_selected": text_value(row.get("why_selected")) or None,
        "why_rejected_by_other_owners": row.get("why_rejected_by_other_owners") if isinstance(row.get("why_rejected_by_other_owners"), dict) else {},
        "record_type": text_value(row.get("record_type")) or None,
        "r1_outcome": text_value(row.get("r1_outcome")) or None,
        "route_reason": text_value(row.get("route_reason")) or None,
    }
    would_fire_rows.append(record)
    fixup_allowed_rows.append(record)
    if strict_fire:
        strict_rows.append(record)
    elif current_fixup_type == "ata":
        ata_fixup_rows.append(record)
    elif current_fixup_type == "template":
        template_fixup_rows.append(record)
    elif current_fixup_type == "ata_template":
        ata_fixup_rows.append(record)
        template_fixup_rows.append(record)


if len(would_fire_rows) != expected_would_fire_total:
    if not allow_reconstruction_mismatch:
        raise SystemExit(
            f"would-fire reconstruction mismatch: expected {expected_would_fire_total}, got {len(would_fire_rows)}"
        )
    print(
        f"[WARN] would-fire reconstruction mismatch: expected {expected_would_fire_total}, got {len(would_fire_rows)}",
        file=sys.stderr,
    )

reconstructed_would_fire_counts = dict(Counter(row["owner_mode"] for row in would_fire_rows))
if reconstructed_would_fire_counts != expected_would_fire_counts:
    if not allow_reconstruction_mismatch:
        raise SystemExit(
            f"owner-count reconstruction mismatch: expected {expected_would_fire_counts}, got {reconstructed_would_fire_counts}"
        )
    print(
        f"[WARN] owner-count reconstruction mismatch: expected {expected_would_fire_counts}, got {reconstructed_would_fire_counts}",
        file=sys.stderr,
    )


def sum_known_usd(rows_):
    return round(
        sum(row["net_after_fee_usd"] for row in rows_ if row["net_after_fee_usd"] is not None),
        9,
    )


def count_known_usd(rows_):
    return sum(1 for row in rows_ if row["net_after_fee_usd"] is not None)


seen_keys = set()
duplicate_rows = []
unique_rows = []
for row in sorted(would_fire_rows, key=lambda item: (item["slot"], item["candidate_id"] or "", item["signature"] or "")):
    dedupe_key = (
        row["slot"],
        row["signature"],
        row["pool"],
        row["mint_in"],
        row["mint_out"],
        row["owner_mode"],
        row["usable_route_for_us"],
        row["notional_bucket"],
    )
    row["dedupe_key"] = list(dedupe_key)
    duplicate = dedupe_key in seen_keys
    row["duplicate_of_prior_event"] = duplicate
    if duplicate:
        duplicate_rows.append(row)
    else:
        seen_keys.add(dedupe_key)
        unique_rows.append(row)

housekeeping_rows = [row for row in would_fire_rows if row["owner_mode"] == "housekeeping"]
housekeeping_unique_keys = {
    tuple(row["dedupe_key"]) for row in housekeeping_rows if not row["duplicate_of_prior_event"]
}
housekeeping_top_pools = Counter(row["pool"] for row in housekeeping_rows if row["pool"])
housekeeping_top_mints = Counter((row["mint_in"], row["mint_out"]) for row in housekeeping_rows)

for row in housekeeping_rows:
    row["account_fixup_assumption"] = row["fixup_used"] and row["fixup_type"] in {"ata", "ata_template", "template"}
    row["inventory_rebalance"] = row["mint_out"] in known_held_like_inputs or row["mint_out"] in approved_held_input_mints
    row["saved_loss"] = False
    row["economic_transfer"] = bool(row["route_usable"] and row["net_after_fee_usd"] is not None and row["net_after_fee_usd"] > 0)
    row["actual_trade"] = bool(row["strict_current_state"] and row["economic_transfer"] and not row["duplicate_of_prior_event"])

housekeeping_real_trade_rows = [row for row in housekeeping_rows if row["actual_trade"]]
housekeeping_fixup_only_rows = [
    row for row in housekeeping_rows if row["account_fixup_assumption"] and not row["actual_trade"]
]

directarb_rows = [
    row for row in would_fire_rows
    if row["owner_mode"] == "directarb"
]
directarb_all_rows = [
    row for row in rows if text_value(row.get("selected_owner")).lower() == "directarb"
]
directarb_unique = safe_int(run62_summary.get("directarb_unique_opportunity_count"))
directarb_seen = safe_int(run62_summary.get("directarb_total_rows"))
directarb_inventory_ready = safe_int(run62_summary.get("directarb_inventory_ready_count"))
directarb_not_held_input = safe_int(run62_summary.get("directarb_not_held_input_count"))
directarb_route_usable = safe_int(run62_summary.get("directarb_route_usable_count"))
directarb_top_blocker = text_value(run62_summary.get("directarb_main_blocker")) or "unknown"

directarb_account_missing = sum(
    1
    for row in directarb_all_rows
    if not strict_output_account_ready(row)
)


def group_usd(rows_, key_fn):
    groups = defaultdict(lambda: {"count": 0, "known_usd_equiv": 0.0, "unknown_unit_count": 0})
    for row in rows_:
        key = key_fn(row)
        groups[key]["count"] += 1
        if row["net_after_fee_usd"] is None:
            groups[key]["unknown_unit_count"] += 1
        else:
            groups[key]["known_usd_equiv"] += row["net_after_fee_usd"]
    output = []
    for key, value in groups.items():
        output.append(
            {
                "key": key,
                "count": value["count"],
                "known_usd_equiv": round(value["known_usd_equiv"], 9),
                "unknown_unit_count": value["unknown_unit_count"],
            }
        )
    return sorted(output, key=lambda item: (item["known_usd_equiv"], item["count"]), reverse=True)


strict_vs_fixup = {
    "strict_current_state_would_fire": len(strict_rows),
    "strict_current_state_ev_usd": sum_known_usd(strict_rows),
    "ata_fixup_would_fire": len(ata_fixup_rows),
    "ata_fixup_ev_usd": sum_known_usd(ata_fixup_rows),
    "template_fixup_would_fire": len(template_fixup_rows),
    "template_fixup_ev_usd": sum_known_usd(template_fixup_rows),
    "fixup_allowed_total_would_fire": len(fixup_allowed_rows),
    "fixup_allowed_total_ev_usd": sum_known_usd(fixup_allowed_rows),
}

housekeeping_report = {
    "rows": len(housekeeping_rows),
    "housekeeping_real_trade_count": len(housekeeping_real_trade_rows),
    "housekeeping_real_trade_ev": sum_known_usd(housekeeping_real_trade_rows),
    "housekeeping_fixup_only_count": len(housekeeping_fixup_only_rows),
    "housekeeping_fixup_only_ev": sum_known_usd(housekeeping_fixup_only_rows),
    "housekeeping_duplicate_count": sum(1 for row in housekeeping_rows if row["duplicate_of_prior_event"]),
    "housekeeping_unique_events": len(housekeeping_unique_keys),
    "housekeeping_top_pools": [
        {"pool": pool, "count": count}
        for pool, count in housekeeping_top_pools.most_common(12)
    ],
    "housekeeping_top_mints": [
        {"mint_in": mint_in, "mint_out": mint_out, "count": count}
        for (mint_in, mint_out), count in housekeeping_top_mints.most_common(12)
    ],
    "classification_method": {
        "actual_trade": "strict_current_state executable and positive economic transfer",
        "inventory_rebalance": "output mint already in conservative held-like inventory set",
        "account_fixup_assumption": "would-fire depends on ATA or template fixup",
        "saved_loss": "reserved for flatten/drop-style non-trading saved-loss cases",
        "economic_transfer": "positive immediate EV row with route-usable transfer semantics",
        "duplicate_of_prior_event": "same slot/signature/pool/mints/owner/route/notional bucket seen earlier",
    },
    "sample_rows": housekeeping_rows[:25],
}

directarb_report = {
    "directarb_seen": directarb_seen,
    "directarb_unique": directarb_unique,
    "directarb_strict_would_fire": len([row for row in directarb_rows if row["strict_current_state"]]),
    "directarb_fixup_would_fire": len([row for row in directarb_rows if row["fixup_used"]]),
    "directarb_inventory_ready": directarb_inventory_ready,
    "directarb_not_held_input": directarb_not_held_input,
    "directarb_account_missing": directarb_account_missing,
    "directarb_route_usable": directarb_route_usable,
    "directarb_top_blocker": directarb_top_blocker,
    "directarb_ev_by_pool": group_usd(directarb_rows, lambda row: row["pool"] or "unknown_pool"),
    "directarb_ev_by_mint_pair": group_usd(
        directarb_rows, lambda row: f"{row['mint_in']}->{row['mint_out']}"
    ),
    "directarb_rows": directarb_rows,
}

owner_mode_split = {}
for owner in sorted(Counter(row["owner_mode"] for row in would_fire_rows)):
    scoped = [row for row in would_fire_rows if row["owner_mode"] == owner]
    owner_mode_split[owner] = {
        "would_fire_count": len(scoped),
        "known_ev_usd": sum_known_usd(scoped),
        "strict_would_fire_count": sum(1 for row in scoped if row["strict_current_state"]),
        "fixup_would_fire_count": sum(1 for row in scoped if row["fixup_used"]),
        "duplicate_count": sum(1 for row in scoped if row["duplicate_of_prior_event"]),
    }

route_discipline = {
    "jito_bundle_executable_count": 0,
    "harmonic_bundle_executable_count": 0,
    "usable_route_values": sorted({row["usable_route_for_us"] for row in would_fire_rows if row["usable_route_for_us"]}),
    "route_leak_count": route_leak_count,
    "route_leak_rows": [row for row in would_fire_rows if row["usable_route_for_us"] not in {"tpu_quic_public", "public_rpc_scout"}],
}

dedup_report = {
    "would_fire_rows": len(would_fire_rows),
    "unique_would_fire_events": len(unique_rows),
    "duplicate_rows": len(duplicate_rows),
    "duplicate_ev_removed": sum_known_usd(duplicate_rows),
    "deduped_ev_usd": sum_known_usd(unique_rows),
}

unit_audit = {
    "sol_usd_equivalent": SOL_USD_EQUIVALENT,
    "heuristic_ev_only_count": heuristic_only_count,
    "known_unit_count": count_known_usd(would_fire_rows),
    "unknown_unit_count": len(would_fire_rows) - count_known_usd(would_fire_rows),
    "conversion_sources": dict(Counter(row["conversion_source"]["net_after_fee"] for row in would_fire_rows)),
    "base_fee_lamports": BASE_FEE_LAMPORTS,
    "base_fee_sol": BASE_FEE_SOL,
    "note": "policy_tip is only separable when the input-mint unit path can absorb a 5000-lamport base fee heuristic; policy_tip_plus_base_fee_raw_units preserves the raw deduction path exactly.",
}

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run62_summary": str(run62_summary_path),
        "candidate_autopsy": str(candidate_autopsy_path),
        "rpc_url": rpc_url,
    },
    "reconciliation": {
        "expected_would_fire_total": expected_would_fire_total,
        "reconstructed_would_fire_total": len(would_fire_rows),
        "expected_would_fire_counts": expected_would_fire_counts,
        "reconstructed_would_fire_counts": reconstructed_would_fire_counts,
        "run62_directarb_would_fire_count": safe_int(run62_summary.get("directarb_would_fire_count")),
        "run62_directarb_main_blocker": directarb_top_blocker,
    },
    "strict_vs_fixup": strict_vs_fixup,
    "owner_mode_split": owner_mode_split,
    "route_discipline": route_discipline,
    "dedup_report": dedup_report,
    "unit_audit": unit_audit,
    "housekeeping": housekeeping_report,
    "directarb": directarb_report,
    "output_files": {
        "epoch1_would_fire_rows_jsonl": str(would_fire_rows_path),
        "epoch1_would_fire_forensic_summary_json": str(forensic_summary_path),
        "epoch1_housekeeping_forensic_report_json": str(housekeeping_report_path),
        "epoch1_directarb_forensic_report_json": str(directarb_report_path),
    },
}

would_fire_rows.sort(key=lambda row: (row["slot"], row["owner_mode"], row["pool"] or "", row["signature"] or ""))
would_fire_rows_path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":"), sort_keys=True) for row in would_fire_rows) + "\n",
    encoding="utf-8",
)
forensic_summary_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")
housekeeping_report_path.write_text(json.dumps(housekeeping_report, indent=2) + "\n", encoding="utf-8")
directarb_report_path.write_text(json.dumps(directarb_report, indent=2) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary_payload['status']}",
    f"expected_would_fire_total={expected_would_fire_total}",
    f"reconstructed_would_fire_total={len(would_fire_rows)}",
    f"strict_current_state_would_fire={strict_vs_fixup['strict_current_state_would_fire']}",
    f"strict_current_state_ev_usd={strict_vs_fixup['strict_current_state_ev_usd']}",
    f"ata_fixup_would_fire={strict_vs_fixup['ata_fixup_would_fire']}",
    f"ata_fixup_ev_usd={strict_vs_fixup['ata_fixup_ev_usd']}",
    f"template_fixup_would_fire={strict_vs_fixup['template_fixup_would_fire']}",
    f"template_fixup_ev_usd={strict_vs_fixup['template_fixup_ev_usd']}",
    f"fixup_allowed_total_would_fire={strict_vs_fixup['fixup_allowed_total_would_fire']}",
    f"fixup_allowed_total_ev_usd={strict_vs_fixup['fixup_allowed_total_ev_usd']}",
    f"would_fire_rows={dedup_report['would_fire_rows']}",
    f"unique_would_fire_events={dedup_report['unique_would_fire_events']}",
    f"duplicate_rows={dedup_report['duplicate_rows']}",
    f"duplicate_ev_removed={dedup_report['duplicate_ev_removed']}",
    f"deduped_ev_usd={dedup_report['deduped_ev_usd']}",
    f"housekeeping_real_trade_count={housekeeping_report['housekeeping_real_trade_count']}",
    f"housekeeping_real_trade_ev={housekeeping_report['housekeeping_real_trade_ev']}",
    f"housekeeping_fixup_only_count={housekeeping_report['housekeeping_fixup_only_count']}",
    f"housekeeping_fixup_only_ev={housekeeping_report['housekeeping_fixup_only_ev']}",
    f"housekeeping_duplicate_count={housekeeping_report['housekeeping_duplicate_count']}",
    f"housekeeping_unique_events={housekeeping_report['housekeeping_unique_events']}",
    f"directarb_seen={directarb_report['directarb_seen']}",
    f"directarb_unique={directarb_report['directarb_unique']}",
    f"directarb_strict_would_fire={directarb_report['directarb_strict_would_fire']}",
    f"directarb_fixup_would_fire={directarb_report['directarb_fixup_would_fire']}",
    f"directarb_inventory_ready={directarb_report['directarb_inventory_ready']}",
    f"directarb_not_held_input={directarb_report['directarb_not_held_input']}",
    f"directarb_account_missing={directarb_report['directarb_account_missing']}",
    f"directarb_route_usable={directarb_report['directarb_route_usable']}",
    f"directarb_top_blocker={directarb_report['directarb_top_blocker']}",
    f"route_leak_count={route_discipline['route_leak_count']}",
    f"heuristic_ev_only_count={unit_audit['heuristic_ev_only_count']}",
    f"epoch1_would_fire_rows_jsonl={would_fire_rows_path}",
    f"epoch1_would_fire_forensic_summary_json={forensic_summary_path}",
    f"epoch1_housekeeping_forensic_report_json={housekeeping_report_path}",
    f"epoch1_directarb_forensic_report_json={directarb_report_path}",
]
summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
PY

pass "Run66 Epoch1 monetization forensic audit written to $RESULT_DIR"
cat "$SUMMARY_FILE"
finish

#!/usr/bin/env bash
# 71_fresh_candidate_continuous_executor_simulation.sh - fresh continuous executor simulation

source "$(dirname "$0")/lib.sh"
script_header "71 - Fresh Candidate Continuous Executor Simulation"

assert_root
assert_cmd_exists python3

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$(cd "$(dirname "$0")/.." && pwd)/results}"
STEP="${SHADOW_RUN71_STEP:-71_fresh_candidate_continuous_executor_simulation}"
STATUS_PREFIX="${SHADOW_RUN71_STATUS_PREFIX:-run71_fresh_candidate_continuous_executor_simulation}"
REPORT_BASENAME="${SHADOW_RUN71_REPORT_BASENAME:-run71_fresh_candidate_continuous_executor_simulation}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
STRICT_ROWS_JSONL="$RESULT_DIR/run71_top_strict_rows.jsonl"
BLOCKED_FIXABLE_ROWS_JSONL="$RESULT_DIR/run71_top_blocked_fixable_rows.jsonl"
POST_SHOCK_RULE_ROWS_JSONL="$RESULT_DIR/run71_top_post_shock_rule_candidates.jsonl"

WINDOW_SECONDS="${SHADOW_RUN71_WINDOW_SECONDS:-900}"
SHARD_COUNTS="${SHADOW_RUN71_SHARD_COUNTS:-1,2,4,8,16}"
EXECUTABLE_ROUTE_FAMILIES="${SHADOW_RUN71_EXECUTABLE_ROUTE_FAMILIES:-tpu_quic_public,public_rpc_scout}"
RPC_URL="${SHADOW_RUN71_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"

RUN61_STEP="${SHADOW_RUN71_RUN61_STEP:-$STEP}"
RUN55_STEP="${SHADOW_RUN71_RUN55_STEP:-${RUN61_STEP}_run55}"
RUN39_STEP="${SHADOW_RUN71_RUN39_STEP:-${RUN55_STEP}_run39}"
RUN62_STEP="${SHADOW_RUN71_RUN62_STEP:-${STEP}_run62}"
RUN64E_STEP="${SHADOW_RUN71_RUN64E_STEP:-${STEP}_run64e}"

RUN61_JSON="$RESULTS_ROOT/$RUN61_STEP/run61_continuous_shadow_live_simulation.json"
RUN61_SUMMARY="$RESULTS_ROOT/$RUN61_STEP/summary.txt"
RUN39_CANDIDATE_AUTOPSY_JSONL="$RESULTS_ROOT/$RUN39_STEP/candidate_autopsy.jsonl"
RUN39_SUMMARY="$RESULTS_ROOT/$RUN39_STEP/summary.txt"
RUN62_JSON="$RESULTS_ROOT/$RUN62_STEP/run62_conversion_autopsy.json"
RUN64E_JSON="$RESULTS_ROOT/$RUN64E_STEP/run64e_exit_qualified_held_input_analysis.json"
RUN65_JSON="$RESULT_DIR/${REPORT_BASENAME}.upstream_run65.json"
RUN65_SUMMARY="$RESULT_DIR/upstream_run65.summary.txt"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STRICT_ROWS_JSONL" \
    "$BLOCKED_FIXABLE_ROWS_JSONL" \
    "$POST_SHOCK_RULE_ROWS_JSONL" \
    "$RUN65_JSON" \
    "$RUN65_SUMMARY"

set +e
env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN65_STEP="$STEP" \
    SHADOW_RUN65_STATUS_PREFIX="$STATUS_PREFIX" \
    SHADOW_RUN65_REPORT_BASENAME="${REPORT_BASENAME}.upstream_run65" \
    SHADOW_RUN65_RUN61_STEP="$RUN61_STEP" \
    SHADOW_RUN65_RUN62_STEP="$RUN62_STEP" \
    SHADOW_RUN65_RUN64E_STEP="$RUN64E_STEP" \
    SHADOW_RUN65_WINDOW_SECONDS="$WINDOW_SECONDS" \
    SHADOW_RUN65_SHARD_COUNTS="$SHARD_COUNTS" \
    SHADOW_RUN65_EXECUTABLE_ROUTE_FAMILIES="$EXECUTABLE_ROUTE_FAMILIES" \
    SHADOW_RUN65_RPC_URL="$RPC_URL" \
    "$(dirname "$0")/65_full_potential_continuous_shard_simulation.sh"
RUN65_RC=$?
set -e

if [[ "$RUN65_RC" -ne 0 ]]; then
    fail "Run71 failed during upstream Run65 fresh continuous simulation"
    exit 1
fi

cp "$RESULT_DIR/summary.txt" "$RUN65_SUMMARY"

python3 - \
    "$RUN61_JSON" \
    "$RUN61_SUMMARY" \
    "$RUN39_CANDIDATE_AUTOPSY_JSONL" \
    "$RUN39_SUMMARY" \
    "$RUN62_JSON" \
    "$RUN64E_JSON" \
    "$RUN65_JSON" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$STRICT_ROWS_JSONL" \
    "$BLOCKED_FIXABLE_ROWS_JSONL" \
    "$POST_SHOCK_RULE_ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$WINDOW_SECONDS" \
    "$SHARD_COUNTS" \
    "$RPC_URL" <<'PY'
import json
import math
import ssl
import sys
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

run61_json_path = Path(sys.argv[1])
run61_summary_path = Path(sys.argv[2])
candidate_autopsy_path = Path(sys.argv[3])
run39_summary_path = Path(sys.argv[4])
run62_json_path = Path(sys.argv[5])
run64e_json_path = Path(sys.argv[6])
run65_json_path = Path(sys.argv[7])
summary_file = Path(sys.argv[8])
summary_json = Path(sys.argv[9])
strict_rows_jsonl = Path(sys.argv[10])
blocked_fixable_rows_jsonl = Path(sys.argv[11])
post_shock_rows_jsonl = Path(sys.argv[12])
step = sys.argv[13]
status_prefix = sys.argv[14]
window_seconds = int(sys.argv[15] or "900")
shard_counts = [int(item) for item in (sys.argv[16] or "1,2,4,8,16").split(",") if item.strip()]
rpc_url = sys.argv[17]

WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
ACTIVE_FIRE_OWNERS = {"directarb", "backrun", "continuation", "housekeeping", "rebalancing"}
POST_SHOCK_OWNERS = {"continuation", "housekeeping", "rebalancing"}
FLATTEN_DROP_OWNERS = {"flatten", "drop"}
USABLE_ROUTE_FAMILIES = {"tpu_quic_public", "public_rpc_scout", "public_rpc"}
PRIVATE_ONLY_ROUTE_FAMILIES = {"jito_bundle", "jito_json_bundle", "harmonic_bundle"}


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


def text_value(value):
    return (value or "").strip() if isinstance(value, str) else str(value or "").strip()


def load_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def load_jsonl(path: Path):
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
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


def bool_field(row, *keys):
    for key in keys:
        value = row.get(key)
        if value is not None:
            return bool(value)
    return False


def placeholder_url(url: str) -> bool:
    return (not url) or ("CHANGE_ME" in url) or ("${" in url)


def rpc(method: str, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    context = ssl.create_default_context() if rpc_url.startswith("https://") else None
    with urllib.request.urlopen(request, timeout=45, context=context) as response:
        return json.loads(response.read().decode("utf-8"))


run61_payload = load_json(run61_json_path)
run61_summary = load_summary(run61_summary_path)
run39_summary = load_summary(run39_summary_path)
run62_payload = load_json(run62_json_path)
run64e_payload = load_json(run64e_json_path)
run65_payload = load_json(run65_json_path)
rows = load_jsonl(candidate_autopsy_path)

sol_usd_equivalent = (
    safe_float((run61_payload.get("inputs") or {}).get("sol_usd_equivalent"))
    or safe_float(run61_summary.get("sol_usd_equivalent"))
    or 1000.0
)

rank_lookup = {}
for item in run64e_payload.get("ranked_not_held_input_mints") or []:
    mint = text_value(item.get("mint"))
    if mint:
        rank_lookup[mint] = item


def fetch_mint_metadata(mints):
    result = {
        WSOL_MINT: {"decimals": 9, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDC_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        USDT_MINT: {"decimals": 6, "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
    }
    for mint in sorted(set(mints)):
        if mint and mint not in result:
            result[mint] = {"decimals": None, "token_program": None}
    return result


unique_mints = set()
for row in rows:
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    if mint_in:
        unique_mints.add(mint_in)
    if mint_out:
        unique_mints.add(mint_out)

mint_metadata = fetch_mint_metadata(unique_mints)


def input_decimals(mint):
    return mint_metadata.get(mint, {}).get("decimals")


def normalize_raw_amount(raw_value, mint):
    raw_value = safe_int(raw_value)
    if mint == WSOL_MINT:
        return raw_value / 1_000_000_000.0
    if mint in {USDC_MINT, USDT_MINT}:
        return raw_value / 1_000_000.0
    decimals = input_decimals(mint)
    if isinstance(decimals, int) and 0 <= decimals <= 18:
        return raw_value / (10 ** decimals)
    return None


def usd_equivalent_for_raw(raw_value, mint):
    native = normalize_raw_amount(raw_value, mint)
    if native is None:
        rank = rank_lookup.get(mint) or {}
        band_raw = safe_int(rank.get("required_band_input_raw_p90")) or safe_int(rank.get("required_band_input_raw_max"))
        acquisition_cost_usdc = safe_float(rank.get("acquisition_cost_usdc"))
        if band_raw > 0 and acquisition_cost_usdc > 0:
            return raw_value / band_raw * acquisition_cost_usdc
        return None
    if mint in {USDC_MINT, USDT_MINT}:
        return native
    if mint == WSOL_MINT:
        return native * sol_usd_equivalent
    rank = rank_lookup.get(mint) or {}
    band_raw = safe_int(rank.get("required_band_input_raw_p90")) or safe_int(rank.get("required_band_input_raw_max"))
    acquisition_cost_usdc = safe_float(rank.get("acquisition_cost_usdc"))
    if band_raw > 0 and acquisition_cost_usdc > 0:
        return raw_value / band_raw * acquisition_cost_usdc
    return None


def sol_equivalent_for_raw(raw_value, mint):
    usd_value = usd_equivalent_for_raw(raw_value, mint)
    if usd_value is not None and sol_usd_equivalent > 0:
        return usd_value / sol_usd_equivalent
    if mint == WSOL_MINT:
        return normalize_raw_amount(raw_value, mint)
    return None


def route_family(row):
    for key in ("target_route_family", "route"):
        value = text_value(row.get(key)).lower()
        if value:
            return value
    return "unknown"


def usable_route_for_us(row):
    value = route_family(row)
    return value if value in USABLE_ROUTE_FAMILIES else None


def route_block_kind(row):
    value = route_family(row)
    if value in PRIVATE_ONLY_ROUTE_FAMILIES:
        return "private_bundle_only"
    if value not in USABLE_ROUTE_FAMILIES:
        return "route_unavailable"
    return "usable"


def is_afxdp_row(row):
    return bool_field(row, "source_afxdp") or text_value(row.get("source")).lower() == "afxdp"


def owner_mode(row):
    return text_value(row.get("selected_owner")).lower() or "drop"


def positive_research(row):
    return bool(row.get("research_positive")) or safe_int(row.get("net_after_fee_lamports")) > 0


def approved_held_input(row):
    return bool_field(row, "approved_held_input", "is_approved_held_input")


def input_balance_ok(row):
    return bool_field(row, "input_balance_ok", "input_balance_sufficient", "spl_input_balance_sufficient")


def input_inventory_ready(row):
    return approved_held_input(row) and input_balance_ok(row)


def input_account_ready(row):
    return bool_field(row, "input_account_ready", "has_input_ata", "input_ata_initialized")


def output_account_ready(row):
    return bool_field(row, "output_account_ready", "has_output_ata", "output_ata_initialized")


def template_ready(row):
    return bool_field(row, "template_ready", "would_execute_current_template")


def quote_valid(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    stage = text_value(row.get("first_rejection_stage")).lower()
    if blocker in {"quote_missing", "no_quote", "quote_not_ready"}:
        return False
    if stage in {"quote_not_ready", "quote_missing"}:
        return False
    return bool_field(row, "quote_ready_or_classified", "would_admit")


def immediate_ev_usd(row):
    return usd_equivalent_for_raw(safe_int(row.get("net_after_fee_lamports")), text_value(row.get("mint_in")))


def immediate_ev_sol(row):
    return sol_equivalent_for_raw(safe_int(row.get("net_after_fee_lamports")), text_value(row.get("mint_in")))


def later_ev_usd(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return usd_equivalent_for_raw(safe_int(markouts.get("best_horizon_net_after_fee_lamports")), text_value(row.get("mint_in")))


def later_ev_sol(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return sol_equivalent_for_raw(safe_int(markouts.get("best_horizon_net_after_fee_lamports")), text_value(row.get("mint_in")))


def saved_loss_usd(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    retained = max(0, safe_int(row.get("retained_ev_lamports")))
    adverse = abs(min(0, safe_int(markouts.get("max_adverse_excursion_lamports"))))
    return max(
        usd_equivalent_for_raw(retained, text_value(row.get("mint_in"))) or 0.0,
        usd_equivalent_for_raw(adverse, text_value(row.get("mint_in"))) or 0.0,
    )


def saved_loss_sol(row):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    retained = max(0, safe_int(row.get("retained_ev_lamports")))
    adverse = abs(min(0, safe_int(markouts.get("max_adverse_excursion_lamports"))))
    return max(
        sol_equivalent_for_raw(retained, text_value(row.get("mint_in"))) or 0.0,
        sol_equivalent_for_raw(adverse, text_value(row.get("mint_in"))) or 0.0,
    )


def positive_unit_ev(row):
    usd_value = immediate_ev_usd(row)
    if usd_value is not None:
        return usd_value > 0
    sol_value = immediate_ev_sol(row)
    if sol_value is not None:
        return sol_value > 0
    return safe_int(row.get("net_after_fee_lamports")) > 0


def positive_later_horizon(row):
    usd_value = later_ev_usd(row)
    if usd_value is not None:
        return usd_value > 0
    sol_value = later_ev_sol(row)
    if sol_value is not None:
        return sol_value > 0
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    return safe_int(markouts.get("best_horizon_net_after_fee_lamports")) > 0


def has_saved_loss_value(row):
    return saved_loss_usd(row) > 0 or saved_loss_sol(row) > 0


def liquidity_ready_for_mint(mint):
    item = rank_lookup.get(mint) or {}
    return bool(item.get("entry_quote_ok")) and bool(item.get("exit_quote_ok"))


def directarb_unlock_ev_usd_for_mint(mint):
    return sum(
        immediate_ev_usd(row) or 0.0
        for row in rows
        if owner_mode(row) == "directarb" and text_value(row.get("mint_in")) == mint
    )


def strict_ready(row):
    owner = owner_mode(row)
    return (
        owner in ACTIVE_FIRE_OWNERS
        and is_afxdp_row(row)
        and usable_route_for_us(row) is not None
        and input_inventory_ready(row)
        and input_account_ready(row)
        and output_account_ready(row)
        and template_ready(row)
        and quote_valid(row)
        and positive_unit_ev(row)
    )


def ata_only_blocked(row):
    owner = owner_mode(row)
    return (
        owner in ACTIVE_FIRE_OWNERS
        and is_afxdp_row(row)
        and usable_route_for_us(row) is not None
        and input_inventory_ready(row)
        and input_account_ready(row)
        and (not output_account_ready(row))
        and template_ready(row)
        and quote_valid(row)
        and positive_unit_ev(row)
    )


def liquid_input_blocked(row):
    owner = owner_mode(row)
    mint_in = text_value(row.get("mint_in"))
    return (
        owner in ACTIVE_FIRE_OWNERS
        and is_afxdp_row(row)
        and usable_route_for_us(row) is not None
        and (not input_inventory_ready(row))
        and input_account_ready(row)
        and output_account_ready(row)
        and template_ready(row)
        and quote_valid(row)
        and positive_unit_ev(row)
        and liquidity_ready_for_mint(mint_in)
    )


def route_blocked(row):
    return is_afxdp_row(row) and positive_research(row) and usable_route_for_us(row) is None


def strategy_rule_bucket(row):
    owner = owner_mode(row)
    common_ready = (
        is_afxdp_row(row)
        and usable_route_for_us(row) is not None
        and input_inventory_ready(row)
        and input_account_ready(row)
        and output_account_ready(row)
        and template_ready(row)
        and quote_valid(row)
    )
    if not common_ready:
        return None
    if owner == "continuation" and positive_later_horizon(row) and not strict_ready(row):
        return "continuation_markout_positive_but_no_rule"
    if owner == "rebalancing" and positive_later_horizon(row) and not strict_ready(row):
        return "rebalancing_markout_positive_but_no_rule"
    if owner == "housekeeping" and positive_later_horizon(row) and not strict_ready(row):
        return "housekeeping_markout_positive_but_no_rule"
    if owner in FLATTEN_DROP_OWNERS and has_saved_loss_value(row):
        return "flatten_drop_saved_loss_only"
    return None


def compact_row(row, extra=None):
    markouts = row.get("markouts") if isinstance(row.get("markouts"), dict) else {}
    mint_in = text_value(row.get("mint_in"))
    mint_out = text_value(row.get("mint_out"))
    payload = {
        "candidate_id": row.get("candidate_id"),
        "slot": safe_int(row.get("slot")),
        "signature": row.get("signature"),
        "selected_owner": owner_mode(row),
        "venue": row.get("venue"),
        "pool": row.get("pool"),
        "mint_in": mint_in or None,
        "mint_out": mint_out or None,
        "source": text_value(row.get("source")).lower() or None,
        "route_family": route_family(row),
        "usable_route_for_us": usable_route_for_us(row),
        "execution_blocker": text_value(row.get("execution_blocker")).lower() or "none",
        "first_rejection_stage": text_value(row.get("first_rejection_stage")).lower() or "none",
        "approved_held_input": approved_held_input(row),
        "input_balance_ok": input_balance_ok(row),
        "input_account_ready": input_account_ready(row),
        "output_account_ready": output_account_ready(row),
        "template_ready": template_ready(row),
        "quote_valid": quote_valid(row),
        "trade_size_lamports": safe_int(row.get("trade_size_lamports")),
        "trade_size_input_native": round_or_none(normalize_raw_amount(safe_int(row.get("trade_size_lamports")), mint_in), 9),
        "trade_size_usd_equivalent": round_or_none(usd_equivalent_for_raw(safe_int(row.get("trade_size_lamports")), mint_in), 9),
        "trade_size_sol_equivalent": round_or_none(sol_equivalent_for_raw(safe_int(row.get("trade_size_lamports")), mint_in), 9),
        "net_after_fee_lamports": safe_int(row.get("net_after_fee_lamports")),
        "net_after_fee_usd_equivalent": round_or_none(immediate_ev_usd(row), 9),
        "net_after_fee_sol_equivalent": round_or_none(immediate_ev_sol(row), 9),
        "retained_ev_lamports": safe_int(row.get("retained_ev_lamports")),
        "retained_ev_usd_equivalent": round_or_none(usd_equivalent_for_raw(safe_int(row.get("retained_ev_lamports")), mint_in), 9),
        "retained_ev_sol_equivalent": round_or_none(sol_equivalent_for_raw(safe_int(row.get("retained_ev_lamports")), mint_in), 9),
        "best_horizon": text_value(markouts.get("best_horizon")) or None,
        "best_horizon_net_after_fee_lamports": safe_int(markouts.get("best_horizon_net_after_fee_lamports")),
        "best_horizon_net_after_fee_usd_equivalent": round_or_none(later_ev_usd(row), 9),
        "best_horizon_net_after_fee_sol_equivalent": round_or_none(later_ev_sol(row), 9),
        "best_horizon_markout_lamports": safe_int(markouts.get("best_horizon_markout_lamports")),
        "best_horizon_markout_usd_equivalent": round_or_none(
            usd_equivalent_for_raw(safe_int(markouts.get("best_horizon_markout_lamports")), mint_in), 9
        ),
        "best_horizon_markout_sol_equivalent": round_or_none(
            sol_equivalent_for_raw(safe_int(markouts.get("best_horizon_markout_lamports")), mint_in), 9
        ),
        "saved_loss_usd_equivalent": round_or_none(saved_loss_usd(row), 9),
        "saved_loss_sol_equivalent": round_or_none(saved_loss_sol(row), 9),
        "continuation_label": text_value(markouts.get("continuation_label")) or None,
    }
    if extra:
        payload.update(extra)
    return payload


def sum_known(values):
    return round(sum(value for value in values if value is not None), 9)


def sort_rows_by_immediate_ev(rows_to_sort):
    return sorted(
        rows_to_sort,
        key=lambda row: (
            immediate_ev_usd(row) if immediate_ev_usd(row) is not None else -1e18,
            immediate_ev_sol(row) if immediate_ev_sol(row) is not None else -1e18,
            safe_int(row.get("net_after_fee_lamports")),
            safe_int(row.get("retained_ev_lamports")),
            safe_int(row.get("slot")),
        ),
        reverse=True,
    )


def sort_rows_by_postshock_ev(rows_to_sort):
    return sorted(
        rows_to_sort,
        key=lambda row: (
            later_ev_usd(row) if later_ev_usd(row) is not None else saved_loss_usd(row),
            later_ev_sol(row) if later_ev_sol(row) is not None else saved_loss_sol(row),
            safe_int(((row.get("markouts") or {}).get("best_horizon_markout_lamports"))),
            safe_int(row.get("retained_ev_lamports")),
            safe_int(row.get("slot")),
        ),
        reverse=True,
    )


strict_rows = []
ata_only_rows = []
liquid_input_rows = []
route_blocked_rows = []
strategy_rows = []
residual_rows = []

for row in rows:
    if strict_ready(row):
        strict_rows.append(row)
        continue
    if ata_only_blocked(row):
        ata_only_rows.append(row)
        continue
    if liquid_input_blocked(row):
        liquid_input_rows.append(row)
        continue
    if route_blocked(row):
        route_blocked_rows.append(row)
        continue
    strategy_bucket = strategy_rule_bucket(row)
    if strategy_bucket:
        tagged = dict(row)
        tagged["_run71_strategy_bucket"] = strategy_bucket
        strategy_rows.append(tagged)
        continue
    residual_rows.append(row)

strict_owner_counts = Counter(owner_mode(row) for row in strict_rows)
strict_ev_usd = sum_known(immediate_ev_usd(row) for row in strict_rows)
strict_ev_sol = sum_known(immediate_ev_sol(row) for row in strict_rows)

ata_only_blocked_ev = sum_known(immediate_ev_usd(row) for row in ata_only_rows)
liquid_input_blocked_ev = sum_known(immediate_ev_usd(row) for row in liquid_input_rows)

route_block_kind_counts = Counter(route_block_kind(row) for row in route_blocked_rows)
route_blocked_venues = [
    {"venue": venue, "count": count}
    for venue, count in Counter(text_value(row.get("venue")).lower() or "unknown" for row in route_blocked_rows).most_common(12)
]
route_blocked_owner_modes = [
    {"owner_mode": owner, "count": count}
    for owner, count in Counter(owner_mode(row) for row in route_blocked_rows).most_common(12)
]

strategy_bucket_counts = Counter(row.get("_run71_strategy_bucket") for row in strategy_rows)
strategy_rule_blocked_count = len(strategy_rows)

ata_unlock_by_output_mint = defaultdict(lambda: {"count": 0, "unlock_ev_usd": 0.0})
for row in ata_only_rows:
    mint = text_value(row.get("mint_out")) or "unknown"
    ata_unlock_by_output_mint[mint]["count"] += 1
    ata_unlock_by_output_mint[mint]["unlock_ev_usd"] += immediate_ev_usd(row) or 0.0

top_ata_mints_by_unlock_ev = [
    {"mint": mint, "unlock_ev_usd": round_or_none(values["unlock_ev_usd"], 9), "count": values["count"]}
    for mint, values in sorted(
        ata_unlock_by_output_mint.items(),
        key=lambda item: (item[1]["unlock_ev_usd"], item[1]["count"]),
        reverse=True,
    )[:12]
]
top_ata_mints_by_recurrence = [
    {"mint": mint, "count": values["count"], "unlock_ev_usd": round_or_none(values["unlock_ev_usd"], 9)}
    for mint, values in sorted(
        ata_unlock_by_output_mint.items(),
        key=lambda item: (item[1]["count"], item[1]["unlock_ev_usd"]),
        reverse=True,
    )[:12]
]

liquid_input_by_mint = defaultdict(lambda: {"count": 0, "directarb_ev_usd": 0.0})
for row in liquid_input_rows:
    mint = text_value(row.get("mint_in")) or "unknown"
    liquid_input_by_mint[mint]["count"] += 1
    liquid_input_by_mint[mint]["directarb_ev_usd"] += immediate_ev_usd(row) or 0.0

top_input_mints_by_directarb_ev = []
for mint, values in sorted(
    liquid_input_by_mint.items(),
    key=lambda item: (item[1]["directarb_ev_usd"], item[1]["count"]),
    reverse=True,
)[:12]:
    rank = rank_lookup.get(mint) or {}
    top_input_mints_by_directarb_ev.append(
        {
            "mint": mint,
            "count": values["count"],
            "directarb_ev_usd": round_or_none(values["directarb_ev_usd"], 9),
            "entry_quote": bool(rank.get("entry_quote_ok")),
            "exit_quote": bool(rank.get("exit_quote_ok")),
            "exit_loss_pct": round_or_none(
                (1.0 - safe_float(rank.get("exit_value_ratio"))) * 100.0
                if safe_float(rank.get("exit_value_ratio")) > 0
                else None,
                6,
            ),
            "exit_value_ratio": round_or_none(safe_float(rank.get("exit_value_ratio")), 9),
            "entry_source_asset": rank.get("entry_source_asset"),
            "economic_exit_verdict": rank.get("economic_exit_verdict"),
        }
    )

top_strict_rows = [
    compact_row(row, {"run71_bucket": "strict_executable_now"})
    for row in sort_rows_by_immediate_ev(strict_rows)[:20]
]

blocked_fixable_rows = []
for row in sort_rows_by_immediate_ev(ata_only_rows):
    blocked_fixable_rows.append(compact_row(row, {"run71_bucket": "ata_only_blocked"}))
for row in sort_rows_by_immediate_ev(liquid_input_rows):
    mint = text_value(row.get("mint_in"))
    rank = rank_lookup.get(mint) or {}
    blocked_fixable_rows.append(
        compact_row(
            row,
            {
                "run71_bucket": "liquid_held_input_blocked",
                "entry_quote": bool(rank.get("entry_quote_ok")),
                "exit_quote": bool(rank.get("exit_quote_ok")),
                "exit_value_ratio": round_or_none(safe_float(rank.get("exit_value_ratio")), 9),
                "exit_loss_pct": round_or_none(
                    (1.0 - safe_float(rank.get("exit_value_ratio"))) * 100.0
                    if safe_float(rank.get("exit_value_ratio")) > 0
                    else None,
                    6,
                ),
            },
        )
    )
blocked_fixable_rows = sorted(
    blocked_fixable_rows,
    key=lambda item: (
        item.get("net_after_fee_usd_equivalent") if item.get("net_after_fee_usd_equivalent") is not None else -1e18,
        item.get("best_horizon_net_after_fee_usd_equivalent")
        if item.get("best_horizon_net_after_fee_usd_equivalent") is not None
        else -1e18,
        item.get("slot") or 0,
    ),
    reverse=True,
)[:20]

top_post_shock_rule_candidates = []
for row in sort_rows_by_postshock_ev(strategy_rows)[:20]:
    top_post_shock_rule_candidates.append(
        compact_row(
            row,
            {
                "run71_bucket": row.get("_run71_strategy_bucket"),
            },
        )
    )

strict_postshock_would_fire = sum(strict_owner_counts.get(owner, 0) for owner in POST_SHOCK_OWNERS)

shard_pressure = (
    ((run65_payload.get("inventory_model_results") or {}).get("actual_inventory_result") or {}).get("shard_comparison")
    or []
)

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_json": str(run61_json_path),
        "run39_candidate_autopsy": str(candidate_autopsy_path),
        "run62_json": str(run62_json_path),
        "run64e_json": str(run64e_json_path),
        "run65_json": str(run65_json_path),
        "window_seconds": window_seconds,
        "shard_counts": shard_counts,
        "rpc_url": rpc_url,
    },
    "runtime_contract": {
        "approved_held_input_mints": run39_summary.get("approved_held_input_mints"),
        "held_input_mints": run39_summary.get("held_input_mints"),
        "r1_surface_approved_held_input": safe_int(run39_summary.get("r1_surface_approved_held_input")),
    },
    "strict_executable_now": {
        "strict_would_fire_total": len(strict_rows),
        "strict_would_fire_directarb": strict_owner_counts.get("directarb", 0),
        "strict_would_fire_backrun": strict_owner_counts.get("backrun", 0),
        "strict_would_fire_continuation": strict_owner_counts.get("continuation", 0),
        "strict_would_fire_rebalancing": strict_owner_counts.get("rebalancing", 0),
        "strict_would_fire_housekeeping": strict_owner_counts.get("housekeeping", 0),
        "strict_postshock_would_fire": strict_postshock_would_fire,
        "strict_ev_usd": strict_ev_usd,
        "strict_ev_sol": strict_ev_sol,
        "top_rows": top_strict_rows,
    },
    "ata_only_blocked": {
        "ata_only_blocked_count": len(ata_only_rows),
        "ata_only_blocked_ev": ata_only_blocked_ev,
        "top_ata_mints_by_unlock_ev": top_ata_mints_by_unlock_ev,
        "top_ata_mints_by_recurrence": top_ata_mints_by_recurrence,
    },
    "liquid_held_input_blocked": {
        "liquid_input_blocked_count": len(liquid_input_rows),
        "liquid_input_blocked_ev": liquid_input_blocked_ev,
        "top_input_mints_by_directarb_ev": top_input_mints_by_directarb_ev,
    },
    "route_blocked": {
        "route_blocked_count": len(route_blocked_rows),
        "private_bundle_only_count": route_block_kind_counts.get("private_bundle_only", 0),
        "route_unavailable_count": route_block_kind_counts.get("route_unavailable", 0),
        "top_route_blocked_venues": route_blocked_venues,
        "top_route_blocked_owner_modes": route_blocked_owner_modes,
    },
    "strategy_rule_blocked": {
        "strategy_rule_blocked_count": strategy_rule_blocked_count,
        "continuation_markout_positive_but_no_rule": strategy_bucket_counts.get(
            "continuation_markout_positive_but_no_rule", 0
        ),
        "rebalancing_markout_positive_but_no_rule": strategy_bucket_counts.get(
            "rebalancing_markout_positive_but_no_rule", 0
        ),
        "housekeeping_markout_positive_but_no_rule": strategy_bucket_counts.get(
            "housekeeping_markout_positive_but_no_rule", 0
        ),
        "flatten_drop_saved_loss_only": strategy_bucket_counts.get("flatten_drop_saved_loss_only", 0),
        "top_candidates": top_post_shock_rule_candidates,
    },
    "blocked_but_fixable_top_rows": blocked_fixable_rows,
    "residual": {
        "residual_unclassified_count": len(residual_rows),
        "top_residual_first_rejection_stages": [
            {"first_rejection_stage": stage, "count": count}
            for stage, count in Counter(text_value(row.get("first_rejection_stage")).lower() or "none" for row in residual_rows).most_common(12)
        ],
        "top_residual_execution_blockers": [
            {"execution_blocker": blocker, "count": count}
            for blocker, count in Counter(text_value(row.get("execution_blocker")).lower() or "none" for row in residual_rows).most_common(12)
        ],
    },
    "shard_pressure": shard_pressure,
    "upstream_run65": {
        "would_fire_total": safe_int(run65_payload.get("would_fire_total")),
        "would_fire_directarb": safe_int(run65_payload.get("would_fire_directarb")),
        "deployable_by_us_opportunity_count": safe_int(run61_summary.get("deployable_by_us_opportunity_count")),
        "owner_mode_directarb_deployable_by_us_count": safe_int(
            run61_summary.get("owner_mode_directarb_deployable_by_us_count")
        ),
    },
}

summary_lines = [
    f"status={payload['status']}",
    f"strict_would_fire_total={payload['strict_executable_now']['strict_would_fire_total']}",
    f"strict_ev_usd={payload['strict_executable_now']['strict_ev_usd']}",
    f"strict_ev_sol={payload['strict_executable_now']['strict_ev_sol']}",
    f"strict_would_fire_directarb={payload['strict_executable_now']['strict_would_fire_directarb']}",
    f"strict_would_fire_backrun={payload['strict_executable_now']['strict_would_fire_backrun']}",
    f"strict_would_fire_continuation={payload['strict_executable_now']['strict_would_fire_continuation']}",
    f"strict_would_fire_rebalancing={payload['strict_executable_now']['strict_would_fire_rebalancing']}",
    f"strict_would_fire_housekeeping={payload['strict_executable_now']['strict_would_fire_housekeeping']}",
    f"strict_postshock_would_fire={payload['strict_executable_now']['strict_postshock_would_fire']}",
    f"ata_only_blocked_count={payload['ata_only_blocked']['ata_only_blocked_count']}",
    f"ata_only_blocked_ev={payload['ata_only_blocked']['ata_only_blocked_ev']}",
    f"liquid_input_blocked_count={payload['liquid_held_input_blocked']['liquid_input_blocked_count']}",
    f"liquid_input_blocked_ev={payload['liquid_held_input_blocked']['liquid_input_blocked_ev']}",
    f"route_blocked_count={payload['route_blocked']['route_blocked_count']}",
    f"private_bundle_only_count={payload['route_blocked']['private_bundle_only_count']}",
    f"route_unavailable_count={payload['route_blocked']['route_unavailable_count']}",
    f"strategy_rule_blocked_count={payload['strategy_rule_blocked']['strategy_rule_blocked_count']}",
    f"continuation_markout_positive_but_no_rule={payload['strategy_rule_blocked']['continuation_markout_positive_but_no_rule']}",
    f"rebalancing_markout_positive_but_no_rule={payload['strategy_rule_blocked']['rebalancing_markout_positive_but_no_rule']}",
    f"housekeeping_markout_positive_but_no_rule={payload['strategy_rule_blocked']['housekeeping_markout_positive_but_no_rule']}",
    f"flatten_drop_saved_loss_only={payload['strategy_rule_blocked']['flatten_drop_saved_loss_only']}",
    f"residual_unclassified_count={payload['residual']['residual_unclassified_count']}",
    f"upstream_would_fire_total={payload['upstream_run65']['would_fire_total']}",
    f"upstream_would_fire_directarb={payload['upstream_run65']['would_fire_directarb']}",
    f"deployable_by_us_opportunity_count={payload['upstream_run65']['deployable_by_us_opportunity_count']}",
    f"owner_mode_directarb_deployable_by_us_count={payload['upstream_run65']['owner_mode_directarb_deployable_by_us_count']}",
]

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
strict_rows_jsonl.write_text(
    "\n".join(json.dumps(item, separators=(",", ":"), sort_keys=True) for item in top_strict_rows) + ("\n" if top_strict_rows else ""),
    encoding="utf-8",
)
blocked_fixable_rows_jsonl.write_text(
    "\n".join(json.dumps(item, separators=(",", ":"), sort_keys=True) for item in blocked_fixable_rows)
    + ("\n" if blocked_fixable_rows else ""),
    encoding="utf-8",
)
post_shock_rows_jsonl.write_text(
    "\n".join(json.dumps(item, separators=(",", ":"), sort_keys=True) for item in top_post_shock_rule_candidates)
    + ("\n" if top_post_shock_rule_candidates else ""),
    encoding="utf-8",
)
PY

pass "Run71 executor summary written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

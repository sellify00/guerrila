#!/usr/bin/env bash
# 64_minimal_live_prep_conversion_validation.sh - prepare the minimal live-prep Run64 validation lane

source "$(dirname "$0")/lib.sh"
script_header "64 - Minimal Live-Prep Conversion Validation"

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
STEP="${SHADOW_RUN64_STEP:-64_minimal_live_prep_conversion_validation}"
STATUS_PREFIX="${SHADOW_RUN64_STATUS_PREFIX:-run64_minimal_live_prep_conversion_validation}"
REPORT_BASENAME="${SHADOW_RUN64_REPORT_BASENAME:-run64_minimal_live_prep_conversion_validation}"

RUN61_STEP="${SHADOW_RUN64_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN62_STEP="${SHADOW_RUN64_RUN62_STEP:-62_conversion_autopsy}"
RUN63_STEP="${SHADOW_RUN64_RUN63_STEP:-63_directarb_conversion_pass}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN63_RESULT_DIR="$RESULTS_ROOT/$RUN63_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN62_REPORT_JSON="$RUN62_RESULT_DIR/run62_conversion_autopsy.json"
RUN63_REPORT_JSON="$RUN63_RESULT_DIR/run63_directarb_conversion_pass.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
REFRESH_INVENTORY_SCRIPT="${SHADOW_RUN64_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"
RPC_URL="${SHADOW_RUN64_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-}}}}"

WSOL_TOTAL_BAND_SOL="${SHADOW_RUN64_WSOL_TOTAL_BAND_SOL:-1.0}"
REFRESH_INVENTORY_FIRST="${SHADOW_RUN64_REFRESH_INVENTORY_FIRST:-true}"
EXECUTE_RERUNS="${SHADOW_RUN64_EXECUTE_RERUNS:-false}"
PREP_CONFIRMED="${SHADOW_RUN64_PREP_CONFIRMED:-false}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
TASKS_FILE="$RESULT_DIR/run64_operator_tasks.sh"
RERUN_LOG="$RESULT_DIR/rerun.log"
LOCAL_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TASKS_FILE" "$RERUN_LOG" "$LOCAL_AGAVE_PREFLIGHT_FILE"

missing=0
for required_path in \
    "$RUN61_REPORT_JSON" \
    "$RUN62_REPORT_JSON" \
    "$RUN63_REPORT_JSON"
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

inventory_refresh_status="not_requested"
if value_is_true "$REFRESH_INVENTORY_FIRST"; then
    if [[ -x "$REFRESH_INVENTORY_SCRIPT" ]]; then
        if "$REFRESH_INVENTORY_SCRIPT" >"$RESULT_DIR/refresh_inventory.stdout" 2>"$RESULT_DIR/refresh_inventory.stderr"; then
            inventory_refresh_status="ok"
        else
            inventory_refresh_status="failed"
            warn "Signer inventory refresh failed; continuing with the current inventory snapshot if present"
        fi
    else
        inventory_refresh_status="script_missing"
        warn "Inventory refresh script is missing or not executable: $REFRESH_INVENTORY_SCRIPT"
    fi
fi

python3 - \
    "$RUN61_REPORT_JSON" \
    "$RUN62_REPORT_JSON" \
    "$RUN63_REPORT_JSON" \
    "$SIGNER_TOKEN_INVENTORY_PATH" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$TASKS_FILE" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$WSOL_TOTAL_BAND_SOL" \
    "$inventory_refresh_status" \
    "$PACK_DIR" <<'PY'
import json
import math
import stat
import sys
from pathlib import Path

WSOL_MINT = "So11111111111111111111111111111111111111112"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"

run61_report_path = Path(sys.argv[1])
run62_report_path = Path(sys.argv[2])
run63_report_path = Path(sys.argv[3])
inventory_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
tasks_file = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
rpc_url = sys.argv[10]
target_wsol_total_band_sol = float(sys.argv[11] or "1.0")
inventory_refresh_status = sys.argv[12]
pack_dir = Path(sys.argv[13])


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def text_value(value):
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


def load_inventory(path: Path):
    if not path.exists():
        return {}
    return load_json(path)


def current_wsol_balance_sol(inventory_payload: dict):
    accounts = inventory_payload.get("accounts") or []
    for entry in accounts:
        if text_value(entry.get("mint_pubkey")) == WSOL_MINT:
            return safe_int(entry.get("amount_raw")) / 1_000_000_000.0
    return 0.0


def split_csv(value: str):
    return [item.strip() for item in text_value(value).split(",") if item.strip()]


run61_report = load_json(run61_report_path)
run62_report = load_json(run62_report_path)
run63_report = load_json(run63_report_path)
inventory_payload = load_inventory(inventory_path)

selected_directarb_pools = run63_report.get("intervention", {}).get("selected_directarb_pools") or []
selected_ata_mints = run63_report.get("intervention", {}).get("selected_rebalancing_ata_mints") or []
policy_blocked_ata_mints = run63_report.get("intervention", {}).get("policy_blocked_rebalancing_ata_mints") or []
optional_held_audit = run63_report.get("intervention", {}).get("approved_but_not_held_mint_policy_audit") or []

current_wsol_sol = current_wsol_balance_sol(inventory_payload)
additional_wsol_needed_sol = max(target_wsol_total_band_sol - current_wsol_sol, 0.0)

pool_total = sum(safe_int(item.get("count")) for item in selected_directarb_pools) or 1
directarb_pool_allocations = []
for item in selected_directarb_pools:
    pool_count = safe_int(item.get("count"))
    share = pool_count / pool_total
    directarb_pool_allocations.append(
        {
            "pool": item.get("pool"),
            "count": pool_count,
            "mint_in": item.get("mint_in"),
            "dominant_pair": item.get("dominant_pair"),
            "target_share_of_wsol_band": round_or_none(share, 6),
            "target_wsol_band_sol": round_or_none(target_wsol_total_band_sol * share, 9),
        }
    )

metadata_only_token2022_blocked = [
    item
    for item in policy_blocked_ata_mints
    if item.get("policy_blocker") == "extensions_not_understood"
    and item.get("token_program") == TOKEN2022_PROGRAM
    and set(item.get("extensions") or []) == {"metadataPointer", "tokenMetadata"}
]

optional_held_lookup = {item.get("mint"): item for item in optional_held_audit if item.get("mint")}
ray_audit = optional_held_lookup.get("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R")
usdt_audit = optional_held_lookup.get("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB")

owner_pubkey = text_value(inventory_payload.get("owner_pubkey")) or None
rerun_commands = [
    f"sudo env SHADOW_RESULTS_DIR={run61_report_path.parent.parent} {pack_dir / 'scripts' / '61_post_shock_conversion.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run62_report_path.parent.parent} {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run63_report_path.parent.parent} {pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}",
]

task_lines = [
    "#!/usr/bin/env bash",
    "# Run64 operator tasks (generated, not auto-executed)",
    "set -euo pipefail",
    "",
    f"# inventory_refresh_status={inventory_refresh_status}",
    f"# current_wsol_balance_sol={round_or_none(current_wsol_sol, 9)}",
    f"# target_wsol_total_band_sol={round_or_none(target_wsol_total_band_sol, 9)}",
    f"# additional_wsol_needed_sol={round_or_none(additional_wsol_needed_sol, 9)}",
    "",
    "# 1. Ensure the signer inventory is fresh before live prep.",
    f"#    {pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'}",
    "",
    "# 2. Top up WSOL conservatively until the total working WSOL band reaches the target above.",
]
for item in directarb_pool_allocations:
    pair = item.get("dominant_pair") or {}
    task_lines.append(
        "#    pool={} target_share_of_wsol_band={} target_wsol_band_sol={} dominant_pair={} -> {}".format(
            item.get("pool"),
            item.get("target_share_of_wsol_band"),
            item.get("target_wsol_band_sol"),
            pair.get("mint_in"),
            pair.get("mint_out"),
        )
    )
task_lines += [
    "#    No automatic funding command is emitted here; confirm the signer, fee payer, and WSOL handling path first.",
    "",
    "# 3. Precreate output ATAs only for the current policy-cleared mints below.",
]
for item in selected_ata_mints:
    task_lines.append(f"#    create_output_ata mint={item.get('mint')} count={item.get('count')}")
task_lines += [
    "",
    "# 4. Do not precreate the blocked Token-2022 metadata-only mints until extension policy is explicitly widened.",
]
for item in metadata_only_token2022_blocked:
    task_lines.append(f"#    blocked_token2022_metadata_only mint={item.get('mint')} count={item.get('count')}")
task_lines += [
    "",
    "# 5. After live prep is complete, rerun the validation stack:",
]
task_lines.extend([f"#    {command}" for command in rerun_commands])
task_lines.append("")

payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "inputs": {
        "run61_report_json": str(run61_report_path),
        "run62_report_json": str(run62_report_path),
        "run63_report_json": str(run63_report_path),
        "inventory_path": str(inventory_path),
        "inventory_present": inventory_path.exists(),
        "inventory_refresh_status": inventory_refresh_status,
        "rpc_url": rpc_url or None,
    },
    "inventory_snapshot": {
        "owner_pubkey": owner_pubkey,
        "current_wsol_balance_sol": round_or_none(current_wsol_sol, 9),
        "target_wsol_total_band_sol": round_or_none(target_wsol_total_band_sol, 9),
        "additional_wsol_needed_sol": round_or_none(additional_wsol_needed_sol, 9),
        "held_input_mints": sorted(text_value(entry.get("mint_pubkey")) for entry in inventory_payload.get("accounts") or [] if entry.get("mint_pubkey")),
    },
    "wsol_band_plan": {
        "selected_directarb_pool_count": len(directarb_pool_allocations),
        "allocations": directarb_pool_allocations,
    },
    "ata_plan": {
        "policy_cleared_mints": selected_ata_mints,
        "policy_cleared_count": len(selected_ata_mints),
        "policy_blocked_metadata_only_token2022_mints": metadata_only_token2022_blocked,
        "policy_blocked_metadata_only_count": len(metadata_only_token2022_blocked),
    },
    "optional_held_input_policy": {
        "ray": ray_audit,
        "usdt": usdt_audit,
    },
    "rerun_plan": {
        "execute_reruns_default": False,
        "commands": rerun_commands,
    },
}

summary_lines = [
    f"status={payload['status']}",
    f"inventory_present={str(payload['inputs']['inventory_present']).lower()}",
    f"inventory_refresh_status={inventory_refresh_status}",
    f"current_wsol_balance_sol={payload['inventory_snapshot']['current_wsol_balance_sol']}",
    f"target_wsol_total_band_sol={payload['inventory_snapshot']['target_wsol_total_band_sol']}",
    f"additional_wsol_needed_sol={payload['inventory_snapshot']['additional_wsol_needed_sol']}",
    f"selected_directarb_pool_count={payload['wsol_band_plan']['selected_directarb_pool_count']}",
    f"policy_cleared_ata_count={payload['ata_plan']['policy_cleared_count']}",
    f"policy_blocked_metadata_only_count={payload['ata_plan']['policy_blocked_metadata_only_count']}",
    f"ray_policy_ok={str(bool(ray_audit and ray_audit.get('policy_ok'))).lower()}",
    f"usdt_policy_ok={str(bool(usdt_audit and usdt_audit.get('policy_ok'))).lower()}",
    f"rerun_plan_execute_default=false",
]

for index, item in enumerate(directarb_pool_allocations, start=1):
    summary_lines.append(f"wsol_pool_{index}={item.get('pool')}")
    summary_lines.append(f"wsol_pool_{index}_target_band_sol={item.get('target_wsol_band_sol')}")

for index, item in enumerate(selected_ata_mints, start=1):
    summary_lines.append(f"policy_cleared_ata_mint_{index}={item.get('mint')}")

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

if value_is_true "$EXECUTE_RERUNS"; then
    if ! value_is_true "$PREP_CONFIRMED"; then
        fail "Refusing reruns because SHADOW_RUN64_PREP_CONFIRMED is not true"
        finish
        exit 1
    fi
    assert_root
    if ! require_local_agave_preflight "$LOCAL_AGAVE_PREFLIGHT_FILE"; then
        finish
        exit 1
    fi
    {
        echo "=== rerunning Run61 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/61_post_shock_conversion.sh"
        echo "=== rerunning Run62 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/62_conversion_autopsy.sh"
        echo "=== rerunning Run63 ==="
        env SHADOW_RESULTS_DIR="$RESULTS_ROOT" "$(dirname "$0")/63_directarb_conversion_pass.sh"
    } | tee "$RERUN_LOG"
fi

pass "Run64 live-prep plan written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

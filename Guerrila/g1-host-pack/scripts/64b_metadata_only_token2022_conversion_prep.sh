#!/usr/bin/env bash
# 64b_metadata_only_token2022_conversion_prep.sh - plan the metadata-only Token-2022 conversion prep lane

source "$(dirname "$0")/lib.sh"
script_header "64B - Metadata-Only Token-2022 Conversion Prep"

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
STEP="${SHADOW_RUN64B_STEP:-64b_metadata_only_token2022_conversion_prep}"
STATUS_PREFIX="${SHADOW_RUN64B_STATUS_PREFIX:-run64b_metadata_only_token2022_conversion_prep}"
REPORT_BASENAME="${SHADOW_RUN64B_REPORT_BASENAME:-run64b_metadata_only_token2022_conversion_prep}"

RUN61_STEP="${SHADOW_RUN64B_RUN61_STEP:-61_continuous_shadow_live_simulation}"
RUN62_STEP="${SHADOW_RUN64B_RUN62_STEP:-62_conversion_autopsy}"
RUN63_STEP="${SHADOW_RUN64B_RUN63_STEP:-63_directarb_conversion_pass}"

RUN61_RESULT_DIR="$RESULTS_ROOT/$RUN61_STEP"
RUN62_RESULT_DIR="$RESULTS_ROOT/$RUN62_STEP"
RUN63_RESULT_DIR="$RESULTS_ROOT/$RUN63_STEP"

RUN61_REPORT_JSON="$RUN61_RESULT_DIR/run61_continuous_shadow_live_simulation.json"
RUN62_REPORT_JSON="$RUN62_RESULT_DIR/run62_conversion_autopsy.json"
RUN63_REPORT_JSON="$RUN63_RESULT_DIR/run63_directarb_conversion_pass.json"

SIGNER_TOKEN_INVENTORY_PATH="${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}"
REFRESH_INVENTORY_SCRIPT="${SHADOW_RUN64B_REFRESH_INVENTORY_SCRIPT:-$(dirname "$0")/refresh_signer_token_inventory.sh}"
REFRESH_INVENTORY_FIRST="${SHADOW_RUN64B_REFRESH_INVENTORY_FIRST:-true}"
TARGET_HELD_INPUT_MINT_COUNT="${SHADOW_RUN64B_TARGET_HELD_INPUT_MINT_COUNT:-5}"
TARGET_ATA_MINT_COUNT="${SHADOW_RUN64B_TARGET_ATA_MINT_COUNT:-5}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
TASKS_FILE="$RESULT_DIR/run64b_operator_tasks.sh"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TASKS_FILE"

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
    "$TARGET_HELD_INPUT_MINT_COUNT" \
    "$TARGET_ATA_MINT_COUNT" \
    "$inventory_refresh_status" \
    "$PACK_DIR" <<'PY'
import json
import stat
import sys
from pathlib import Path

run61_report_path = Path(sys.argv[1])
run62_report_path = Path(sys.argv[2])
run63_report_path = Path(sys.argv[3])
inventory_path = Path(sys.argv[4])
summary_file = Path(sys.argv[5])
summary_json = Path(sys.argv[6])
tasks_file = Path(sys.argv[7])
step = sys.argv[8]
status_prefix = sys.argv[9]
target_held_input_mint_count = int(sys.argv[10] or "5")
target_ata_mint_count = int(sys.argv[11] or "5")
inventory_refresh_status = sys.argv[12]
pack_dir = Path(sys.argv[13])


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def text_value(value):
    return str(value or "").strip()


def round_or_none(value, digits=9):
    if value is None:
        return None
    return round(value, digits)


def load_inventory(path: Path):
    if not path.exists():
        return {}
    return load_json(path)


run61_report = load_json(run61_report_path)
run62_report = load_json(run62_report_path)
run63_report = load_json(run63_report_path)
inventory_payload = load_inventory(inventory_path)

held_input_mints = sorted(
    text_value(entry.get("mint_pubkey"))
    for entry in inventory_payload.get("accounts") or []
    if entry.get("mint_pubkey")
)
held_input_mint_set = set(held_input_mints)
owner_pubkey = text_value(inventory_payload.get("owner_pubkey")) or None

followup_candidates = run63_report.get("followup_analysis", {}).get("not_held_input_followup_candidates") or []
followup_lookup = {item.get("mint"): item for item in followup_candidates if item.get("mint")}
selected_metadata_policy_not_held = run63_report.get("intervention", {}).get("selected_metadata_policy_not_held_input_mints") or []
selected_metadata_policy_atas = run63_report.get("intervention", {}).get("selected_metadata_policy_rebalancing_ata_mints") or []
metadata_safe_atas = run63_report.get("intervention", {}).get("metadata_only_token2022_safe_rebalancing_ata_mints") or []
metadata_safe_ata_lookup = {item.get("mint"): item for item in metadata_safe_atas if item.get("mint")}

selected_held_input_candidates = [
    followup_lookup[mint]
    for mint in selected_metadata_policy_not_held[:target_held_input_mint_count]
    if mint in followup_lookup
]
selected_ata_candidates = [
    metadata_safe_ata_lookup[mint]
    for mint in selected_metadata_policy_atas[:target_ata_mint_count]
    if mint in metadata_safe_ata_lookup
]

rerun_commands = [
    f"sudo env SHADOW_RESULTS_DIR={run61_report_path.parent.parent} {pack_dir / 'scripts' / '61_post_shock_conversion.sh'}",
    f"sudo env SHADOW_RESULTS_DIR={run62_report_path.parent.parent} {pack_dir / 'scripts' / '62_conversion_autopsy.sh'}",
    (
        f"sudo env SHADOW_RESULTS_DIR={run63_report_path.parent.parent} "
        f"SHADOW_RUN63_ALLOW_METADATA_ONLY_TOKEN2022=true "
        f"{pack_dir / 'scripts' / '63_directarb_conversion_pass.sh'}"
    ),
]

allow_rule = {
    "token_program": "Token-2022",
    "allowed_extensions_subset": ["metadataPointer", "tokenMetadata"],
    "freeze_authority": "null",
    "mint_authority": "null",
    "transfer_hook": "blocked",
    "permanent_delegate": "blocked",
    "non_transferable": "blocked",
    "default_account_state_frozen": "blocked",
    "denylisted": "blocked",
    "decimals_sane": True,
    "rpc_policy_snapshot_stable_across_two_reads": True,
}

task_lines = [
    "#!/usr/bin/env bash",
    "# Run64B operator tasks (generated, not auto-executed)",
    "set -euo pipefail",
    "",
    f"# inventory_refresh_status={inventory_refresh_status}",
    "# 1. Keep the metadata-only Token-2022 allow rule narrow and fail-closed.",
]
for key, value in allow_rule.items():
    task_lines.append(f"#    allow_rule.{key}={value}")
task_lines += [
    "",
    "# 2. Refresh signer inventory before any bounded metadata-only prep.",
    f"#    {pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'}",
    "",
    "# 3. Prepare bounded held-input bands only for the selected metadata-only mints below.",
]
for item in selected_held_input_candidates:
    task_lines.append(
        "#    held_input mint={} count={} p50={} p90={} max={} would_fire_if_held={} ata_present_in_inventory={}".format(
            item.get("mint"),
            item.get("count"),
            item.get("required_band_input_native_p50"),
            item.get("required_band_input_native_p90"),
            item.get("required_band_input_native_max"),
            item.get("directarb_would_fire_if_held_count"),
            str(item.get("mint") in held_input_mint_set).lower(),
        )
    )
task_lines += [
    "#    No automatic acquisition command is emitted here; confirm bounded sizing and operator path first.",
    "",
    "# 4. Precreate output ATAs for the selected metadata-only shortlist if they are still absent.",
]
for item in selected_ata_candidates:
    task_lines.append(
        "#    create_output_ata mint={} count={} ata_present_in_inventory={}".format(
            item.get("mint"),
            item.get("count"),
            str(item.get("mint") in held_input_mint_set).lower(),
        )
    )
task_lines += [
    "",
    "# 5. After the bounded prep is complete, rerun the validation stack with the metadata-only policy lane enabled.",
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
        "target_held_input_mint_count": target_held_input_mint_count,
        "target_ata_mint_count": target_ata_mint_count,
    },
    "allow_rule": allow_rule,
    "current_window": {
        "deployable_by_us_opportunity_count": run61_report.get("deployable_by_us_opportunity_count"),
        "would_fire_total": run61_report.get("would_fire_total"),
        "directarb_not_held_input_count": run62_report.get("directarb_inventory_conversion", {}).get("not_held_input_count"),
        "rebalancing_account_ata_missing_count": run62_report.get("rebalancing_ata_conversion", {}).get("account_ata_missing_count"),
    },
    "modeled_metadata_lane": {
        "directarb_would_fire_if_policy_safe_held": run63_report.get("deltas", {}).get("policy_safe_not_held_input_directarb_would_fire_count"),
        "directarb_would_fire_if_metadata_policy_held": run63_report.get("deltas", {}).get("metadata_policy_not_held_input_directarb_would_fire_count"),
        "rebalancing_ata_unlocked_if_metadata_only_allowed": run63_report.get("deltas", {}).get("rebalancing_ata_unlocked_if_metadata_only_allowed"),
        "metadata_only_token2022_safe_count": run63_report.get("followup_analysis", {}).get("metadata_only_token2022_safe_count"),
    },
    "inventory_snapshot": {
        "owner_pubkey": owner_pubkey,
        "held_input_mints": held_input_mints,
    },
    "held_input_plan": {
        "selected_count": len(selected_held_input_candidates),
        "selected_mints": selected_held_input_candidates,
    },
    "ata_plan": {
        "selected_count": len(selected_ata_candidates),
        "selected_mints": selected_ata_candidates,
    },
    "rerun_plan": {
        "commands": rerun_commands,
    },
}

summary_lines = [
    f"status={payload['status']}",
    f"inventory_present={str(payload['inputs']['inventory_present']).lower()}",
    f"inventory_refresh_status={inventory_refresh_status}",
    f"metadata_only_token2022_safe_count={payload['modeled_metadata_lane']['metadata_only_token2022_safe_count']}",
    f"directarb_would_fire_if_policy_safe_held={payload['modeled_metadata_lane']['directarb_would_fire_if_policy_safe_held']}",
    f"directarb_would_fire_if_metadata_policy_held={payload['modeled_metadata_lane']['directarb_would_fire_if_metadata_policy_held']}",
    f"rebalancing_ata_unlocked_if_metadata_only_allowed={payload['modeled_metadata_lane']['rebalancing_ata_unlocked_if_metadata_only_allowed']}",
    f"selected_metadata_held_input_count={payload['held_input_plan']['selected_count']}",
    f"selected_metadata_ata_count={payload['ata_plan']['selected_count']}",
]

for index, item in enumerate(selected_held_input_candidates, start=1):
    summary_lines.append(f"metadata_held_input_mint_{index}={item.get('mint')}")
    summary_lines.append(f"metadata_held_input_mint_{index}_count={item.get('count')}")
    summary_lines.append(f"metadata_held_input_mint_{index}_required_band_input_native_p90={item.get('required_band_input_native_p90')}")
    summary_lines.append(f"metadata_held_input_mint_{index}_directarb_would_fire_if_held={item.get('directarb_would_fire_if_held_count')}")

for index, item in enumerate(selected_ata_candidates, start=1):
    summary_lines.append(f"metadata_ata_mint_{index}={item.get('mint')}")
    summary_lines.append(f"metadata_ata_mint_{index}_count={item.get('count')}")

summary_file.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file.chmod(tasks_file.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY

pass "Run64B metadata-only prep plan written to $SUMMARY_FILE"
tail -n 120 "$SUMMARY_FILE" || true
finish

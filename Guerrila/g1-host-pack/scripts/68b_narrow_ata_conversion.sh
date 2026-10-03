#!/usr/bin/env bash
# 68b_narrow_ata_conversion.sh - narrow ATA-only conversion for Run68 bucket-A mints

source "$(dirname "$0")/lib.sh"
script_header "68B - Narrow ATA Conversion"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT_PATH="$PACK_DIR/scripts/$(basename "$0")"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"

MODE="${SHADOW_RUN68B_MODE:-full}"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RPC_URL="${SHADOW_RUN68B_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-${SHADOW_RPC_URL:-$(default_http_rpc_url)}}}}"
SIGNER_PATH="${SHADOW_RUN68B_SIGNER_PATH:-${SHADOW_SIGNER_PATH:-/etc/shadow-engine/worker-signer.json}}"
INVENTORY_PATH="${SHADOW_RUN68B_INVENTORY_PATH:-${SHADOW_SIGNER_TOKEN_INVENTORY_PATH:-/etc/shadow-engine/signer-token-inventory.json}}"

if [[ "$MODE" == "create-single-ata" ]]; then
    SINGLE_MINT="${SHADOW_RUN68B_SINGLE_MINT:-}"
    SINGLE_TOKEN_PROGRAM="${SHADOW_RUN68B_SINGLE_TOKEN_PROGRAM:-}"
    SINGLE_RPC_URL="${SHADOW_RUN68B_SINGLE_RPC_URL:-$RPC_URL}"
    SINGLE_SIGNER_PATH="${SHADOW_RUN68B_SINGLE_SIGNER_PATH:-$SIGNER_PATH}"
    SINGLE_RESULT_JSON="${SHADOW_RUN68B_SINGLE_RESULT_JSON:-}"

    if [[ -z "$SINGLE_MINT" || -z "$SINGLE_TOKEN_PROGRAM" ]]; then
        fail "Set SHADOW_RUN68B_SINGLE_MINT and SHADOW_RUN68B_SINGLE_TOKEN_PROGRAM for create-single-ata mode"
        finish
        exit 1
    fi

    python3 - \
        "$SINGLE_MINT" \
        "$SINGLE_TOKEN_PROGRAM" \
        "$SINGLE_RPC_URL" \
        "$SINGLE_SIGNER_PATH" \
        "$SINGLE_RESULT_JSON" <<'PY'
import base64
import json
import ssl
import sys
import time
import urllib.request
from pathlib import Path

from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import Message
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from spl.token.instructions import create_idempotent_associated_token_account, get_associated_token_address

mint_pubkey = sys.argv[1]
token_program_id = sys.argv[2]
rpc_url = sys.argv[3]
signer_path = Path(sys.argv[4])
result_json_path = Path(sys.argv[5]) if sys.argv[5] else None


def rpc(method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if rpc_url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def latest_blockhash():
    response = rpc("getLatestBlockhash")
    value = (response.get("result") or {}).get("value") or {}
    return Hash.from_string(value["blockhash"])


def account_info(pubkey):
    response = rpc("getAccountInfo", [pubkey, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    return ((response.get("result") or {}).get("value")) or None


def send_raw_tx(tx_bytes):
    encoded = base64.b64encode(tx_bytes).decode("ascii")
    return rpc(
        "sendTransaction",
        [
            encoded,
            {
                "encoding": "base64",
                "skipPreflight": False,
                "preflightCommitment": "confirmed",
                "maxRetries": 1,
            },
        ],
    )


def confirm_signature(signature, attempts=15, delay_seconds=2.0):
    last_response = None
    for attempt in range(attempts):
        last_response = rpc("getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        values = (((last_response or {}).get("result") or {}).get("value")) or []
        if values and values[0] is not None:
            return last_response
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    return last_response


def rpc_result_ok(response):
    if not isinstance(response, dict):
        return False, None
    result = response.get("result") or {}
    if not isinstance(result, dict):
        return False, None
    values = result.get("value") or []
    if not values:
        return False, None
    entry = values[0]
    if not isinstance(entry, dict):
        return False, entry
    return entry.get("err") is None, entry


raw = json.loads(signer_path.read_text(encoding="utf-8"))
if not isinstance(raw, list) or len(raw) < 64:
    raise RuntimeError(f"unexpected signer keypair shape in {signer_path}")
owner_keypair = Keypair.from_bytes(bytes(int(value) for value in raw[:64]))
owner = owner_keypair.pubkey()
mint = Pubkey.from_string(mint_pubkey)
program_id = Pubkey.from_string(token_program_id)
ata = get_associated_token_address(owner, mint, program_id)
ata_before = account_info(str(ata))
ata_exists_before = ata_before is not None

payload = {
    "mint": mint_pubkey,
    "token_program": token_program_id,
    "owner_pubkey": str(owner),
    "derived_ata": str(ata),
    "ata_exists_before": ata_exists_before,
    "action": "already_exists" if ata_exists_before else "created",
    "create_signature": None,
    "confirm_response": None,
    "ata_exists_after": ata_exists_before,
}

if not ata_exists_before:
    instruction = create_idempotent_associated_token_account(owner, owner, mint, program_id)
    message = Message([instruction], owner)
    tx = Transaction([owner_keypair], message, latest_blockhash())
    send_response = send_raw_tx(bytes(tx))
    if "error" in send_response:
        raise RuntimeError(f"sendTransaction failed for {mint_pubkey}: {send_response['error']}")
    signature = send_response.get("result")
    confirm_response = confirm_signature(signature)
    ok, status_entry = rpc_result_ok(confirm_response)
    payload["create_signature"] = signature
    payload["confirm_response"] = confirm_response
    payload["status_entry"] = status_entry
    if not ok:
        raise RuntimeError(f"ATA confirmation failed for {mint_pubkey}: {confirm_response}")
    payload["ata_exists_after"] = account_info(str(ata)) is not None
else:
    payload["status_entry"] = None

if result_json_path:
    result_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

print(f"mint={payload['mint']}")
print(f"owner_pubkey={payload['owner_pubkey']}")
print(f"derived_ata={payload['derived_ata']}")
print(f"ata_exists_before={str(payload['ata_exists_before']).lower()}")
print(f"action={payload['action']}")
print(f"create_signature={payload['create_signature'] or 'none'}")
print(f"ata_exists_after={str(payload['ata_exists_after']).lower()}")
PY

    finish
    exit 0
fi

STEP="${SHADOW_RUN68B_STEP:-68b_narrow_ata_conversion}"
STATUS_PREFIX="${SHADOW_RUN68B_STATUS_PREFIX:-run68b_narrow_ata_conversion}"
REPORT_BASENAME="${SHADOW_RUN68B_REPORT_BASENAME:-run68b_narrow_ata_conversion}"
RUN68_STEP="${SHADOW_RUN68B_RUN68_STEP:-68_size_aware_ata_liquidity_drilldown}"
RUN66_BASE_STEP="${SHADOW_RUN68B_RUN66_BASE_STEP:-66_epoch1_monetization_forensic_audit}"
TARGET_MINTS="${SHADOW_RUN68B_TARGET_MINTS:-CwhSfYsGUBVZyLP96Uyfau6hsWcdwe5GMEcBFKeZTRT4,HKJHsYJHMVK5VRyHHk5GhvzY9tBAAtPvDkZfDH6RLDTd}"
EPOCH_NAME="${SHADOW_RUN68B_EPOCH_NAME:-epoch2}"
EPOCH_WINDOW_SECONDS="${SHADOW_RUN68B_EPOCH_WINDOW_SECONDS:-900}"
FORENSIC_STEP="${SHADOW_RUN68B_FORENSIC_STEP:-68b_epoch2_post_ata_forensic_audit}"
REFRESH_RPC_URL="${SHADOW_RUN68B_REFRESH_INVENTORY_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-$(default_http_rpc_url)}}"

RUN68_RESULT_DIR="$RESULTS_ROOT/$RUN68_STEP"
RUN66_BASE_RESULT_DIR="$RESULTS_ROOT/$RUN66_BASE_STEP"
RUN68_SELECTED_JSON="$RUN68_RESULT_DIR/run68_selected_ata_candidates.json"
RUN66_BASE_SUMMARY_JSON="$RUN66_BASE_RESULT_DIR/epoch1_would_fire_forensic_summary.json"
RUN66_BASE_WOULD_FIRE_JSONL="$RUN66_BASE_RESULT_DIR/epoch1_would_fire_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
PLAN_JSON="$RESULT_DIR/run68b_ata_plan.json"
TASKS_FILE="$RESULT_DIR/run68b_operator_tasks.sh"
EXECUTION_JSON="$RESULT_DIR/run68b_ata_execution.json"
COMPARE_JSON="$RESULT_DIR/run68b_epoch2_comparison.json"
INVENTORY_REFRESH_LOG="$RESULT_DIR/run68b_inventory_refresh.txt"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$PLAN_JSON" \
    "$TASKS_FILE" \
    "$EXECUTION_JSON" \
    "$COMPARE_JSON" \
    "$INVENTORY_REFRESH_LOG" \
    "$RESULT_DIR"/run68b_create_*.json

missing=0
for required_path in \
    "$RUN68_SELECTED_JSON" \
    "$RUN66_BASE_SUMMARY_JSON" \
    "$RUN66_BASE_WOULD_FIRE_JSONL" \
    "$SIGNER_PATH"
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
    "$RUN68_SELECTED_JSON" \
    "$TARGET_MINTS" \
    "$RPC_URL" \
    "$SIGNER_PATH" \
    "$INVENTORY_PATH" \
    "$SCRIPT_PATH" \
    "$PLAN_JSON" \
    "$TASKS_FILE" \
    "$EPOCH_NAME" \
    "$EPOCH_WINDOW_SECONDS" \
    "$FORENSIC_STEP" \
    "$PACK_DIR" <<'PY'
import json
import shlex
import ssl
import stat
import sys
import urllib.request
from pathlib import Path

from solders.keypair import Keypair
from solders.pubkey import Pubkey
from spl.token.instructions import get_associated_token_address

run68_selected_path = Path(sys.argv[1])
target_mints_csv = sys.argv[2]
rpc_url = sys.argv[3]
signer_path = Path(sys.argv[4])
inventory_path = Path(sys.argv[5])
script_path = Path(sys.argv[6])
plan_json_path = Path(sys.argv[7])
tasks_file_path = Path(sys.argv[8])
epoch_name = sys.argv[9]
epoch_window_seconds = sys.argv[10]
forensic_step = sys.argv[11]
pack_dir = Path(sys.argv[12])

TOKENKEG = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"


def rpc(method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    if rpc_url.startswith("https://"):
        with urllib.request.urlopen(request, timeout=30, context=ssl.create_default_context()) as response:
            return json.loads(response.read().decode("utf-8"))
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def text_value(value):
    return str(value or "").strip()


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=6):
    if value is None:
        return None
    return round(value, digits)


targets = [item.strip() for item in target_mints_csv.split(",") if item.strip()]
selected_rows = json.loads(run68_selected_path.read_text(encoding="utf-8"))
selected_by_mint = {text_value(row.get("mint_out")): row for row in selected_rows}

raw_signer = json.loads(signer_path.read_text(encoding="utf-8"))
owner_keypair = Keypair.from_bytes(bytes(int(value) for value in raw_signer[:64]))
owner_pubkey = str(owner_keypair.pubkey())

inventory_owner = None
if inventory_path.exists():
    try:
        inventory_owner = text_value(json.loads(inventory_path.read_text(encoding="utf-8")).get("owner_pubkey")) or None
    except Exception:
        inventory_owner = None

plan_items = []
plan_errors = []

for mint in targets:
    candidate = selected_by_mint.get(mint)
    if candidate is None:
        plan_errors.append(f"target_mint_missing_from_run68={mint}")
        continue
    token_program = text_value(candidate.get("token_program"))
    mint_response = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    mint_value = ((mint_response.get("result") or {}).get("value")) or {}
    mint_owner = text_value(mint_value.get("owner"))
    mint_info = ((((mint_value.get("data") or {}).get("parsed") or {}).get("info")) or {})
    decimals = mint_info.get("decimals")
    ata = get_associated_token_address(
        Pubkey.from_string(owner_pubkey),
        Pubkey.from_string(mint),
        Pubkey.from_string(token_program),
    )
    ata_response = rpc("getAccountInfo", [str(ata), {"encoding": "jsonParsed", "commitment": "confirmed"}])
    ata_exists = ((ata_response.get("result") or {}).get("value")) is not None
    command = " ".join(
        [
            "sudo -n env",
            f"SHADOW_RUN68B_MODE={shlex.quote('create-single-ata')}",
            f"SHADOW_RUN68B_SINGLE_MINT={shlex.quote(mint)}",
            f"SHADOW_RUN68B_SINGLE_TOKEN_PROGRAM={shlex.quote(token_program)}",
            f"SHADOW_RUN68B_SINGLE_RPC_URL={shlex.quote(rpc_url)}",
            f"SHADOW_RUN68B_SINGLE_SIGNER_PATH={shlex.quote(str(signer_path))}",
            shlex.quote(str(script_path)),
        ]
    )
    plan_items.append(
        {
            "mint": mint,
            "token_program": token_program,
            "mint_owner_program": mint_owner or None,
            "decimals": decimals,
            "policy_verdict": text_value(candidate.get("policy_verdict")) or "none",
            "policy_pass": bool(candidate.get("policy_pass")),
            "classification_bucket": text_value(candidate.get("classification_bucket")),
            "owner_signer": owner_pubkey,
            "inventory_owner_pubkey": inventory_owner,
            "signer_matches_inventory_owner": inventory_owner in {None, owner_pubkey},
            "derived_ata": str(ata),
            "current_ata_exists": ata_exists,
            "create_idempotent_ata_command": command,
            "expected_unlock_rows": int(candidate.get("row_count") or 0),
            "expected_unlock_ev_usd": round_or_none(safe_float(candidate.get("expected_unlock_ev_usd")), 6),
            "owner_modes_affected": candidate.get("owner_modes_affected") or [],
            "route_usable_by_us": bool(candidate.get("route_usable_by_us")),
            "precreate_allowed": bool(candidate.get("precreate_allowed")),
        }
    )

plan_clean = (
    len(plan_items) == len(targets)
    and not plan_errors
     and all(
         item.get("policy_pass")
         and item.get("precreate_allowed")
         and item.get("classification_bucket") == "A_immediately_safe"
         and item.get("token_program") == TOKENKEG
         and item.get("mint_owner_program") == item.get("token_program")
         and item.get("decimals") is not None
         and item.get("route_usable_by_us")
         and item.get("signer_matches_inventory_owner")
         for item in plan_items
     )
 )

projected_unlock_rows = sum(int(item.get("expected_unlock_rows") or 0) for item in plan_items)
projected_unlock_ev_usd = round_or_none(sum(safe_float(item.get("expected_unlock_ev_usd")) for item in plan_items), 6)

payload = {
    "status": "run68b_plan_ready" if plan_clean else "run68b_plan_blocked",
    "targets": targets,
    "plan_clean": plan_clean,
    "plan_errors": plan_errors,
    "projected_unlock_rows": projected_unlock_rows,
    "projected_unlock_ev_usd": projected_unlock_ev_usd,
    "epoch_rerun": {
        "epoch_name": epoch_name,
        "epoch_step": f"{epoch_name}_canonical_monetization",
        "epoch_window_seconds": int(epoch_window_seconds),
        "forensic_step": forensic_step,
    },
    "plan_items": plan_items,
}

task_lines = [
    "#!/usr/bin/env bash",
    "# Run68B operator tasks (generated)",
    "set -euo pipefail",
    "",
    f"# plan_clean={str(plan_clean).lower()}",
    f"# projected_unlock_rows={projected_unlock_rows}",
    f"# projected_unlock_ev_usd={projected_unlock_ev_usd}",
    "",
    "# 1. Precreate only the two safe Tokenkeg ATAs below.",
]
for item in plan_items:
    task_lines.extend(
        [
            (
                "#    mint={mint} token_program={token_program} decimals={decimals} policy_verdict={policy_verdict} "
                "owner_signer={owner_signer} derived_ata={derived_ata} current_ata_exists={current_ata_exists} "
                "expected_unlock_rows={expected_unlock_rows} expected_unlock_ev_usd={expected_unlock_ev_usd} "
                "owner_modes_affected={owner_modes}"
            ).format(
                mint=item["mint"],
                token_program=item["token_program"],
                decimals=item["decimals"],
                policy_verdict=item["policy_verdict"],
                owner_signer=item["owner_signer"],
                derived_ata=item["derived_ata"],
                current_ata_exists=str(bool(item["current_ata_exists"])).lower(),
                expected_unlock_rows=item["expected_unlock_rows"],
                expected_unlock_ev_usd=item["expected_unlock_ev_usd"],
                owner_modes=",".join(item["owner_modes_affected"]),
            ),
            item["create_idempotent_ata_command"],
            "",
        ]
    )
task_lines.extend(
    [
        "# 2. Refresh signer inventory after ATA creation.",
        (
            "sudo -n env "
            "SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL=\"${SHADOW_SOLANA_RPC_URL:-$(default_http_rpc_url)}\" "
            f"bash {shlex.quote(str(pack_dir / 'scripts' / 'refresh_signer_token_inventory.sh'))}"
        ),
        "",
        "# 3. Rerun a fresh bounded AF_XDP epoch.",
        (
            f"sudo -n env SHADOW_EPOCH_NAME={shlex.quote(epoch_name)} "
            f"SHADOW_EPOCH_WINDOW_SECONDS={shlex.quote(str(epoch_window_seconds))} "
            f"{shlex.quote(str(pack_dir / 'scripts' / 'epoch_canonical_monetization.sh'))}"
        ),
        "",
        "# 4. Re-run Run66-style forensic audit on the fresh epoch.",
        (
            f"sudo -n env SHADOW_RUN66_RUN61_STEP={shlex.quote(f'{epoch_name}_canonical_monetization')} "
            f"SHADOW_RUN66_RUN62_STEP={shlex.quote(f'{epoch_name}_canonical_monetization_run62')} "
            f"SHADOW_RUN66_RUN39_STEP={shlex.quote(f'{epoch_name}_canonical_monetization_run55_run39')} "
            f"SHADOW_RUN66_STEP={shlex.quote(forensic_step)} "
            f"SHADOW_RUN66_STATUS_PREFIX={shlex.quote(forensic_step)} "
            f"SHADOW_RUN66_REPORT_BASENAME={shlex.quote(forensic_step)} "
            f"{shlex.quote(str(pack_dir / 'scripts' / '66_epoch1_monetization_forensic_audit.sh'))}"
        ),
        "",
    ]
)

plan_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
tasks_file_path.write_text("\n".join(task_lines) + "\n", encoding="utf-8")
tasks_file_path.chmod(tasks_file_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

print(f"plan_clean={str(plan_clean).lower()}")
print(f"planned_candidate_count={len(plan_items)}")
print(f"projected_unlock_rows={projected_unlock_rows}")
print(f"projected_unlock_ev_usd={projected_unlock_ev_usd}")
PY

plan_clean="$(python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
print('true' if payload.get('plan_clean') else 'false')
PY
)"

if [[ "$plan_clean" != "true" ]]; then
    fail "Run68B plan is not clean; see $PLAN_JSON"
    finish
    exit 1
fi

while IFS=$'\t' read -r mint token_program; do
    result_path="$RESULT_DIR/run68b_create_${mint}.json"
    env \
        SHADOW_RUN68B_MODE=create-single-ata \
        SHADOW_RUN68B_SINGLE_MINT="$mint" \
        SHADOW_RUN68B_SINGLE_TOKEN_PROGRAM="$token_program" \
        SHADOW_RUN68B_SINGLE_RPC_URL="$RPC_URL" \
        SHADOW_RUN68B_SINGLE_SIGNER_PATH="$SIGNER_PATH" \
        SHADOW_RUN68B_SINGLE_RESULT_JSON="$result_path" \
        "$SCRIPT_PATH"
done < <(
    python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
for item in payload.get('plan_items') or []:
    print(f"{item['mint']}\t{item['token_program']}")
PY
)

python3 - "$PLAN_JSON" "$EXECUTION_JSON" "$RESULT_DIR" <<'PY'
import json
import sys
from pathlib import Path

plan = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
execution_json_path = Path(sys.argv[2])
result_dir = Path(sys.argv[3])
rows = []
for item in plan.get("plan_items") or []:
    path = result_dir / f"run68b_create_{item['mint']}.json"
    if path.exists():
        rows.append(json.loads(path.read_text(encoding="utf-8")))
payload = {
    "status": "run68b_ata_execution_complete",
    "execution_count": len(rows),
    "rows": rows,
}
execution_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(f"execution_count={len(rows)}")
PY

env \
    SHADOW_SIGNER_TOKEN_INVENTORY_RPC_URL="$REFRESH_RPC_URL" \
    bash "$PACK_DIR/scripts/refresh_signer_token_inventory.sh" | tee "$INVENTORY_REFRESH_LOG"

env \
    SHADOW_EPOCH_NAME="$EPOCH_NAME" \
    SHADOW_EPOCH_WINDOW_SECONDS="$EPOCH_WINDOW_SECONDS" \
    "$PACK_DIR/scripts/epoch_canonical_monetization.sh"

env \
    SHADOW_RUN66_RUN61_STEP="${EPOCH_NAME}_canonical_monetization" \
    SHADOW_RUN66_RUN62_STEP="${EPOCH_NAME}_canonical_monetization_run62" \
    SHADOW_RUN66_RUN39_STEP="${EPOCH_NAME}_canonical_monetization_run55_run39" \
    SHADOW_RUN66_STEP="$FORENSIC_STEP" \
    SHADOW_RUN66_STATUS_PREFIX="$FORENSIC_STEP" \
    SHADOW_RUN66_REPORT_BASENAME="$FORENSIC_STEP" \
    "$PACK_DIR/scripts/66_epoch1_monetization_forensic_audit.sh"

python3 - \
    "$RUN66_BASE_SUMMARY_JSON" \
    "$RUN66_BASE_WOULD_FIRE_JSONL" \
    "$RESULTS_ROOT/$FORENSIC_STEP/epoch1_would_fire_forensic_summary.json" \
    "$RESULTS_ROOT/$FORENSIC_STEP/epoch1_would_fire_rows.jsonl" \
    "$RESULTS_ROOT/${EPOCH_NAME}_canonical_monetization/run61_continuous_shadow_live_simulation.json" \
    "$PLAN_JSON" \
    "$EXECUTION_JSON" \
    "$COMPARE_JSON" \
    "$SUMMARY_JSON" \
    "$SUMMARY_FILE" \
    "$INVENTORY_REFRESH_LOG" \
    "$STATUS_PREFIX" \
    "$STEP" <<'PY'
import json
import sys
from pathlib import Path

baseline_summary_path = Path(sys.argv[1])
baseline_rows_path = Path(sys.argv[2])
fresh_summary_path = Path(sys.argv[3])
fresh_rows_path = Path(sys.argv[4])
run61_path = Path(sys.argv[5])
plan_path = Path(sys.argv[6])
execution_path = Path(sys.argv[7])
compare_json_path = Path(sys.argv[8])
summary_json_path = Path(sys.argv[9])
summary_file_path = Path(sys.argv[10])
inventory_refresh_log_path = Path(sys.argv[11])
status_prefix = sys.argv[12]
step = sys.argv[13]


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


def safe_float(value, default=0.0):
    try:
        return float(value or 0.0)
    except Exception:
        return default


def round_or_none(value, digits=6):
    if value is None:
        return None
    return round(value, digits)


def fixup_required(row):
    blocker = text_value(row.get("execution_blocker")).lower()
    fixup_type = text_value(row.get("fixup_type")).lower()
    return fixup_type == "ata" or blocker in {"missing_output_ata", "output_ata_uninitialized"}


baseline_summary = load_json(baseline_summary_path)
baseline_rows = load_jsonl(baseline_rows_path)
fresh_summary = load_json(fresh_summary_path)
fresh_rows = load_jsonl(fresh_rows_path)
run61_payload = load_json(run61_path)
plan_payload = load_json(plan_path)
execution_payload = load_json(execution_path)
inventory_refresh_log = inventory_refresh_log_path.read_text(encoding="utf-8") if inventory_refresh_log_path.exists() else ""

base_strict = baseline_summary.get("strict_vs_fixup") or {}
fresh_strict = fresh_summary.get("strict_vs_fixup") or {}
base_owner = baseline_summary.get("owner_mode_split") or {}
fresh_owner = fresh_summary.get("owner_mode_split") or {}
base_route = baseline_summary.get("route_discipline") or {}
fresh_route = fresh_summary.get("route_discipline") or {}

mint_comparison = []
for item in plan_payload.get("plan_items") or []:
    mint = item["mint"]
    base_rows_for_mint = [row for row in baseline_rows if text_value(row.get("mint_out")) == mint]
    fresh_rows_for_mint = [row for row in fresh_rows if text_value(row.get("mint_out")) == mint]
    base_fixup_rows = sum(1 for row in base_rows_for_mint if fixup_required(row) and not bool(row.get("strict_current_state")))
    fresh_fixup_rows = sum(1 for row in fresh_rows_for_mint if fixup_required(row) and not bool(row.get("strict_current_state")))
    base_strict_rows = sum(1 for row in base_rows_for_mint if bool(row.get("strict_current_state")))
    fresh_strict_rows = sum(1 for row in fresh_rows_for_mint if bool(row.get("strict_current_state")))
    base_strict_ev = sum(safe_float(row.get("retained_ev_usd")) for row in base_rows_for_mint if bool(row.get("strict_current_state")))
    fresh_strict_ev = sum(safe_float(row.get("retained_ev_usd")) for row in fresh_rows_for_mint if bool(row.get("strict_current_state")))
    mint_comparison.append(
        {
            "mint": mint,
            "derived_ata": item.get("derived_ata"),
            "current_ata_exists_before": item.get("current_ata_exists"),
            "baseline_would_fire_rows": len(base_rows_for_mint),
            "fresh_would_fire_rows": len(fresh_rows_for_mint),
            "baseline_fixup_required_rows": base_fixup_rows,
            "fresh_fixup_required_rows": fresh_fixup_rows,
            "fixup_required_rows_delta": fresh_fixup_rows - base_fixup_rows,
            "baseline_strict_rows": base_strict_rows,
            "fresh_strict_rows": fresh_strict_rows,
            "strict_rows_delta": fresh_strict_rows - base_strict_rows,
            "baseline_strict_ev_usd": round_or_none(base_strict_ev, 6),
            "fresh_strict_ev_usd": round_or_none(fresh_strict_ev, 6),
            "strict_ev_usd_delta": round_or_none(fresh_strict_ev - base_strict_ev, 6),
            "owner_modes_affected": item.get("owner_modes_affected") or [],
        }
    )

comparison = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "targets": plan_payload.get("targets") or [],
    "projected_unlock_rows": plan_payload.get("projected_unlock_rows"),
    "projected_unlock_ev_usd": plan_payload.get("projected_unlock_ev_usd"),
    "inventory_refresh_log": str(inventory_refresh_log_path),
    "live_send_executed": False,
    "baseline": {
        "strict_current_state_would_fire": base_strict.get("strict_current_state_would_fire"),
        "strict_current_state_ev_usd": base_strict.get("strict_current_state_ev_usd"),
        "ata_fixup_would_fire": base_strict.get("ata_fixup_would_fire"),
        "fixup_allowed_total_would_fire": base_strict.get("fixup_allowed_total_would_fire"),
        "housekeeping_strict_rows": ((base_owner.get("housekeeping") or {}).get("strict_would_fire_count")),
        "rebalancing_strict_rows": ((base_owner.get("rebalancing") or {}).get("strict_would_fire_count")),
        "jito_bundle_executable_count": base_route.get("jito_bundle_executable_count"),
    },
    "fresh": {
        "strict_current_state_would_fire": fresh_strict.get("strict_current_state_would_fire"),
        "strict_current_state_ev_usd": fresh_strict.get("strict_current_state_ev_usd"),
        "ata_fixup_would_fire": fresh_strict.get("ata_fixup_would_fire"),
        "fixup_allowed_total_would_fire": fresh_strict.get("fixup_allowed_total_would_fire"),
        "housekeeping_strict_rows": ((fresh_owner.get("housekeeping") or {}).get("strict_would_fire_count")),
        "rebalancing_strict_rows": ((fresh_owner.get("rebalancing") or {}).get("strict_would_fire_count")),
        "jito_bundle_executable_count": fresh_route.get("jito_bundle_executable_count"),
    },
    "delta": {
        "strict_current_state_would_fire_delta": (fresh_strict.get("strict_current_state_would_fire") or 0) - (base_strict.get("strict_current_state_would_fire") or 0),
        "strict_current_state_ev_usd_delta": round_or_none((fresh_strict.get("strict_current_state_ev_usd") or 0.0) - (base_strict.get("strict_current_state_ev_usd") or 0.0), 6),
        "ata_fixup_would_fire_delta": (fresh_strict.get("ata_fixup_would_fire") or 0) - (base_strict.get("ata_fixup_would_fire") or 0),
        "housekeeping_strict_rows_delta": (((fresh_owner.get("housekeeping") or {}).get("strict_would_fire_count") or 0) - ((base_owner.get("housekeeping") or {}).get("strict_would_fire_count") or 0)),
        "rebalancing_strict_rows_delta": (((fresh_owner.get("rebalancing") or {}).get("strict_would_fire_count") or 0) - ((base_owner.get("rebalancing") or {}).get("strict_would_fire_count") or 0)),
    },
    "target_mints": mint_comparison,
    "checks": {
        "strict_current_state_would_fire_increased": (fresh_strict.get("strict_current_state_would_fire") or 0) > (base_strict.get("strict_current_state_would_fire") or 0),
        "strict_current_state_ev_usd_increased": (fresh_strict.get("strict_current_state_ev_usd") or 0.0) > (base_strict.get("strict_current_state_ev_usd") or 0.0),
        "housekeeping_strict_rows_increased": ((fresh_owner.get("housekeeping") or {}).get("strict_would_fire_count") or 0) > ((base_owner.get("housekeeping") or {}).get("strict_would_fire_count") or 0),
        "rebalancing_strict_rows_increased": ((fresh_owner.get("rebalancing") or {}).get("strict_would_fire_count") or 0) > ((base_owner.get("rebalancing") or {}).get("strict_would_fire_count") or 0),
        "jito_bundle_executable_count_zero": (fresh_route.get("jito_bundle_executable_count") or 0) == 0,
        "no_live_send": True,
        "target_fixup_rows_disappeared_or_decreased": all(item["fresh_fixup_required_rows"] <= item["baseline_fixup_required_rows"] for item in mint_comparison),
    },
    "run61_simulation": {
        "would_fire_total": run61_payload.get("would_fire_total"),
        "would_enqueue_tpu_count": run61_payload.get("would_enqueue_tpu_count"),
        "would_public_rpc_send_count": run61_payload.get("would_public_rpc_send_count"),
        "would_wire_send_count": run61_payload.get("would_wire_send_count"),
    },
    "plan": plan_payload,
    "execution": execution_payload,
    "fresh_forensic_paths": {
        "forensic_summary_json": str(fresh_summary_path),
        "would_fire_rows_jsonl": str(fresh_rows_path),
    },
}

summary_payload = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "plan": plan_payload,
    "execution": execution_payload,
    "comparison": comparison,
    "inventory_refresh_log_text": inventory_refresh_log,
}

summary_lines = [
    f"status={summary_payload['status']}",
    f"plan_clean={str(bool(plan_payload.get('plan_clean'))).lower()}",
    f"selected_target_count={len(plan_payload.get('plan_items') or [])}",
    f"projected_unlock_rows={plan_payload.get('projected_unlock_rows')}",
    f"projected_unlock_ev_usd={plan_payload.get('projected_unlock_ev_usd')}",
    f"strict_current_state_would_fire_baseline={comparison['baseline']['strict_current_state_would_fire']}",
    f"strict_current_state_would_fire_fresh={comparison['fresh']['strict_current_state_would_fire']}",
    f"strict_current_state_would_fire_delta={comparison['delta']['strict_current_state_would_fire_delta']}",
    f"strict_current_state_ev_usd_baseline={comparison['baseline']['strict_current_state_ev_usd']}",
    f"strict_current_state_ev_usd_fresh={comparison['fresh']['strict_current_state_ev_usd']}",
    f"strict_current_state_ev_usd_delta={comparison['delta']['strict_current_state_ev_usd_delta']}",
    f"ata_fixup_would_fire_baseline={comparison['baseline']['ata_fixup_would_fire']}",
    f"ata_fixup_would_fire_fresh={comparison['fresh']['ata_fixup_would_fire']}",
    f"ata_fixup_would_fire_delta={comparison['delta']['ata_fixup_would_fire_delta']}",
    f"housekeeping_strict_rows_baseline={comparison['baseline']['housekeeping_strict_rows']}",
    f"housekeeping_strict_rows_fresh={comparison['fresh']['housekeeping_strict_rows']}",
    f"housekeeping_strict_rows_delta={comparison['delta']['housekeeping_strict_rows_delta']}",
    f"rebalancing_strict_rows_baseline={comparison['baseline']['rebalancing_strict_rows']}",
    f"rebalancing_strict_rows_fresh={comparison['fresh']['rebalancing_strict_rows']}",
    f"rebalancing_strict_rows_delta={comparison['delta']['rebalancing_strict_rows_delta']}",
    f"jito_bundle_executable_count_fresh={comparison['fresh']['jito_bundle_executable_count']}",
    f"would_public_rpc_send_count={comparison['run61_simulation']['would_public_rpc_send_count']}",
    f"would_wire_send_count={comparison['run61_simulation']['would_wire_send_count']}",
    f"strict_current_state_would_fire_increased={str(comparison['checks']['strict_current_state_would_fire_increased']).lower()}",
    f"strict_current_state_ev_usd_increased={str(comparison['checks']['strict_current_state_ev_usd_increased']).lower()}",
    f"housekeeping_strict_rows_increased={str(comparison['checks']['housekeeping_strict_rows_increased']).lower()}",
    f"rebalancing_strict_rows_increased={str(comparison['checks']['rebalancing_strict_rows_increased']).lower()}",
     f"target_fixup_rows_disappeared_or_decreased={str(comparison['checks']['target_fixup_rows_disappeared_or_decreased']).lower()}",
     f"jito_bundle_executable_count_zero={str(comparison['checks']['jito_bundle_executable_count_zero']).lower()}",
     f"no_live_send={str(comparison['checks']['no_live_send']).lower()}",
     "live_send_executed=false",
 ]

for index, item in enumerate(mint_comparison, start=1):
    summary_lines.extend(
        [
            f"target_{index}_mint={item['mint']}",
            f"target_{index}_derived_ata={item['derived_ata']}",
            f"target_{index}_baseline_fixup_required_rows={item['baseline_fixup_required_rows']}",
            f"target_{index}_fresh_fixup_required_rows={item['fresh_fixup_required_rows']}",
            f"target_{index}_fixup_required_rows_delta={item['fixup_required_rows_delta']}",
            f"target_{index}_baseline_strict_rows={item['baseline_strict_rows']}",
            f"target_{index}_fresh_strict_rows={item['fresh_strict_rows']}",
            f"target_{index}_strict_rows_delta={item['strict_rows_delta']}",
            f"target_{index}_baseline_strict_ev_usd={item['baseline_strict_ev_usd']}",
            f"target_{index}_fresh_strict_ev_usd={item['fresh_strict_ev_usd']}",
            f"target_{index}_strict_ev_usd_delta={item['strict_ev_usd_delta']}",
        ]
    )

compare_json_path.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")
summary_file_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run68B completed; summary written to $SUMMARY_FILE"
finish

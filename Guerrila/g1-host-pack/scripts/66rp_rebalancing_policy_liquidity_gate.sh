#!/usr/bin/env bash
# 66rp_rebalancing_policy_liquidity_gate.sh - policy and liquidity gate for rebalancing rows

source "$(dirname "$0")/lib.sh"
script_header "66RP - Rebalancing Policy/Liquidity Gate"

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
STEP="${SHADOW_RUN66RP_STEP:-66rp_rebalancing_policy_liquidity_gate}"
STATUS_PREFIX="${SHADOW_RUN66RP_STATUS_PREFIX:-run66rp_rebalancing_policy_liquidity_gate}"
REPORT_BASENAME="${SHADOW_RUN66RP_REPORT_BASENAME:-run66rp_rebalancing_policy_liquidity_gate}"
SOURCE_ROWS="${SHADOW_RUN66RP_SOURCE_ROWS:-}"
ATA_ROWS="${SHADOW_RUN66RP_ATA_ROWS:-}"

latest_file() {
    find "$RESULTS_ROOT" -type f -name "$1" -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 1 | cut -d' ' -f2-
}

[[ -n "$SOURCE_ROWS" ]] || SOURCE_ROWS="$(latest_file run66ri_rebalancing_inline_ata_lifecycle_rows.jsonl)"
[[ -n "$ATA_ROWS" ]] || ATA_ROWS="$(latest_file run66a_rebalancing_account_ata_unlock_rows.jsonl)"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66rp_rebalancing_policy_liquidity_gate_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

python3 - "$SOURCE_ROWS" "$ATA_ROWS" "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$STEP" "$STATUS_PREFIX" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

source_arg, ata_arg, summary_file, summary_json, rows_jsonl, step, status_prefix = sys.argv[1:8]
source_path = Path(source_arg) if source_arg else None
ata_path = Path(ata_arg) if ata_arg else None
summary_file = Path(summary_file)
summary_json = Path(summary_json)
rows_jsonl = Path(rows_jsonl)

TOKEN2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
METADATA_ONLY = {"MetadataPointer", "TokenMetadata"}
DANGEROUS_EXTENSIONS = {"TransferHook", "PermanentDelegate", "NonTransferable", "DefaultAccountState"}

def load_jsonl(path):
    if not path or not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows

def write_jsonl(path, rows):
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

def text(value):
    return str(value or "").strip()

def boolish(value):
    return bool(value)

def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def as_extensions(value):
    if isinstance(value, list):
        return {text(v) for v in value if text(v)}
    if isinstance(value, str):
        raw = value.strip()
        if raw.startswith("["):
            try:
                parsed = json.loads(raw)
                return as_extensions(parsed)
            except Exception:
                pass
        return {part.strip() for part in raw.replace(";", ",").split(",") if part.strip()}
    return set()

def positive_roundtrip(row):
    for key in ("roundtrip_net_after_all_costs", "net_ev_with_ata_burned", "net_ev_with_ata_recovered", "base_net_ev"):
        if safe_int(row.get(key)) > 0:
            return True
    return False

def decision_for(row):
    extensions = as_extensions(row.get("extensions"))
    token_program = text(row.get("token_program"))
    freeze_authority = text(row.get("freeze_authority"))
    mint_authority = text(row.get("mint_authority"))
    policy_verdict = text(row.get("policy_verdict"))
    if freeze_authority or "freeze_authority" in policy_verdict:
        return "DENY_FREEZE_AUTHORITY"
    if extensions & DANGEROUS_EXTENSIONS:
        return "DENY_TRANSFER_HOOK" if "TransferHook" in extensions else "DENY_AUTHORITY_RISK"
    if mint_authority or "mint_authority" in policy_verdict:
        return "DENY_AUTHORITY_RISK"
    if boolish(row.get("hard_policy_risk")):
        return "DENY_AUTHORITY_RISK"
    if token_program == TOKEN2022 or "token2022" in policy_verdict:
        if extensions.issubset(METADATA_ONLY):
            return "REVIEW_METADATA_ONLY_TOKEN2022"
        return "DENY_TRANSFER_HOOK" if "TransferHook" in extensions else "DENY_AUTHORITY_RISK"
    if boolish(row.get("exit_dust")) or boolish(row.get("dust_after_exit")):
        return "DENY_EXIT_DUST"
    if not positive_roundtrip(row):
        return "DENY_ROUNDTRIP_NEGATIVE"
    if boolish(row.get("policy_actionable_now")) or policy_verdict in {"pass", "POLICY_CLEAN_READY", ""}:
        return "POLICY_CLEAN_READY"
    if boolish(row.get("explicit_allow_required")):
        return "REVIEW_METADATA_ONLY_TOKEN2022"
    return "DENY_AUTHORITY_RISK" if policy_verdict else "POLICY_CLEAN_READY"

ata_by_mint = {text(row.get("mint")): row for row in load_jsonl(ata_path) if text(row.get("mint"))}
out = []
for row in load_jsonl(source_path):
    mint = text(row.get("mint_out") or row.get("mint"))
    ata = ata_by_mint.get(mint, {})
    merged = dict(row)
    for key in ("token_program", "extensions", "freeze_authority", "mint_authority", "policy_verdict", "policy_pass", "full_band_exit_probe_value_usdc", "unit_exit_probe_value_usdc", "exit_quote_ok"):
        if key not in merged or merged.get(key) in (None, "", []):
            if key in ata:
                merged[key] = ata.get(key)
    decision = decision_for(merged)
    item = dict(merged)
    item.update({
        "mint": mint,
        "output_token_policy_pass": decision == "POLICY_CLEAN_READY",
        "roundtrip_positive_after_all_costs": positive_roundtrip(merged),
        "exit_non_dust": not boolish(merged.get("exit_dust")) and not boolish(merged.get("dust_after_exit")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
        "send_transaction_called": False,
        "sendBundle_called": False,
    })
    out.append(item)

out.sort(key=lambda row: (row["decision"] == "POLICY_CLEAN_READY", safe_int(row.get("base_net_ev"))), reverse=True)
write_jsonl(rows_jsonl, out)
decision_counts = Counter(row["decision"] for row in out)
summary = {
    "status": f"{status_prefix}_complete" if source_path and source_path.exists() else f"{status_prefix}_missing_source",
    "step": step,
    "source_rows": str(source_path) if source_path else "",
    "source_ata_rows": str(ata_path) if ata_path else "",
    "row_count": len(out),
    "policy_clean_ready_count": decision_counts.get("POLICY_CLEAN_READY", 0),
    "review_metadata_only_token2022_count": decision_counts.get("REVIEW_METADATA_ONLY_TOKEN2022", 0),
    "deny_freeze_authority_count": decision_counts.get("DENY_FREEZE_AUTHORITY", 0),
    "deny_roundtrip_negative_count": decision_counts.get("DENY_ROUNDTRIP_NEGATIVE", 0),
    "decision_counts": dict(decision_counts),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = [
    f"status={summary['status']}",
    f"source_rows={summary['source_rows']}",
    f"source_ata_rows={summary['source_ata_rows']}",
    f"row_count={summary['row_count']}",
    f"policy_clean_ready_count={summary['policy_clean_ready_count']}",
    f"review_metadata_only_token2022_count={summary['review_metadata_only_token2022_count']}",
    f"deny_freeze_authority_count={summary['deny_freeze_authority_count']}",
    f"deny_roundtrip_negative_count={summary['deny_roundtrip_negative_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    "wire_send=0",
    "live_landed=0",
    "send_transaction_called=false",
    "sendBundle_called=false",
]
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66RP policy/liquidity gate written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

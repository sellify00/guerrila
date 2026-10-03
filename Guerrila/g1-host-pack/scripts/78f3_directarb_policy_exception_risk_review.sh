#!/usr/bin/env bash
# 78F3 - research-only risk review for policy-exception DirectArb flatten rows.

source "$(dirname "$0")/lib.sh"
script_header "78F3 - DirectArb Policy Exception Risk Review"

assert_cmd_exists python3

EPOCH_DIR=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --epoch)
            EPOCH_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$EPOCH_DIR" ]]; then
    EPOCH_DIR="${SHADOW_RUN78F3_EPOCH_DIR:-/dev/results/epoch_directarb_cap1_runtime_requote_nosend_20260527_030601}"
fi
if [[ ! -d "$EPOCH_DIR" ]]; then
    fail "Epoch directory not found: $EPOCH_DIR"
    exit 1
fi
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$EPOCH_DIR/78f3_directarb_policy_exception_risk_review"
fi

mkdir -p "$OUTPUT_DIR"
SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
REPORT_JSON="$OUTPUT_DIR/run78f3_directarb_policy_exception_risk_review.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"
rm -f "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV"

DEFAULT_ROWS="${SHADOW_RUN78F3_DEFAULT_ROWS:-$EPOCH_DIR/78f_directarb_exit_flatten_route_certificate/rows.jsonl}"
WHATIF_ROWS="${SHADOW_RUN78F3_WHATIF_ROWS:-$EPOCH_DIR/78f_directarb_exit_flatten_route_certificate_policy_whatif/rows.jsonl}"
RPC_URL="${SHADOW_RUN78F3_RPC_URL:-${SHADOW_LOCAL_AGAVE_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}}"

python3 - "$EPOCH_DIR" "$OUTPUT_DIR" "$SUMMARY_FILE" "$REPORT_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$DEFAULT_ROWS" "$WHATIF_ROWS" "$RPC_URL" <<'PY'
from __future__ import annotations

import csv
import json
import ssl
import sys
import urllib.request
from collections import Counter
from pathlib import Path

epoch_dir = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
rows_jsonl_path = Path(sys.argv[5])
rows_csv_path = Path(sys.argv[6])
default_rows_path = Path(sys.argv[7])
whatif_rows_path = Path(sys.argv[8])
rpc_url = sys.argv[9]

TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"


def parse_jsonl(path: Path) -> tuple[list[dict], int]:
    rows: list[dict] = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed += 1
    return rows, malformed


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def text(value) -> str:
    return str(value or "").strip()


def row_key(row: dict) -> tuple[str, str, str, str]:
    return (
        text(row.get("candidate_id")),
        text(row.get("signature")),
        text(row.get("pool")),
        text(row.get("mint_out")),
    )


def placeholder_url(url: str) -> bool:
    return not url or "CHANGE_ME" in url or "${" in url


def rpc(method: str, params=None) -> tuple[dict | None, dict | None]:
    if placeholder_url(rpc_url):
        return None, {"error": "rpc_unconfigured"}
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    request = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        if rpc_url.startswith("https://"):
            with urllib.request.urlopen(request, timeout=20, context=ssl.create_default_context()) as response:
                body = json.loads(response.read().decode("utf-8"))
        else:
            with urllib.request.urlopen(request, timeout=20) as response:
                body = json.loads(response.read().decode("utf-8"))
        if "error" in body:
            return None, body["error"]
        return body.get("result"), None
    except Exception as exc:
        return None, {"error": str(exc)}


def extension_names(value) -> list[str]:
    if not isinstance(value, list):
        return []
    names: list[str] = []
    for item in value:
        if isinstance(item, str):
            names.append(item)
        elif isinstance(item, dict):
            names.append(text(item.get("extension") or item.get("type") or item.get("name")))
    return [name for name in names if name]


def extension_state_lookup(value) -> dict[str, object]:
    lookup: dict[str, object] = {}
    if not isinstance(value, list):
        return lookup
    for item in value:
        if not isinstance(item, dict):
            continue
        name = text(item.get("extension") or item.get("type") or item.get("name"))
        if name:
            lookup[name] = item.get("state")
    return lookup


def fetch_mint_metadata(mint: str) -> dict:
    result, error = rpc("getAccountInfo", [mint, {"encoding": "jsonParsed", "commitment": "confirmed"}])
    base = {
        "mint": mint,
        "token_program": "",
        "decimals": None,
        "freeze_authority": "",
        "mint_authority": "",
        "extensions": [],
        "default_account_state": "",
        "transfer_hook": False,
        "permanent_delegate": False,
        "non_transferable": False,
        "rpc_error": json.dumps(error, sort_keys=True) if error else "",
    }
    if error or not result:
        return base
    value = result.get("value") or {}
    parsed = ((value.get("data") or {}).get("parsed")) or {}
    info = parsed.get("info") or {}
    extensions_detail = info.get("extensions") or parsed.get("extensions") or []
    extension_lookup = extension_state_lookup(extensions_detail)
    extensions = extension_names(extensions_detail)
    default_account_state = text(info.get("defaultAccountState"))
    default_state_payload = extension_lookup.get("defaultAccountState")
    if not default_account_state and isinstance(default_state_payload, dict):
        default_account_state = text(default_state_payload.get("state"))
    permanent_delegate = bool(info.get("permanentDelegate")) or "permanentDelegate" in set(extensions)
    transfer_hook = bool(info.get("transferHook")) or "transferHook" in set(extensions)
    non_transferable = "nonTransferable" in set(extensions)
    base.update(
        {
            "token_program": text(value.get("owner")),
            "decimals": info.get("decimals"),
            "freeze_authority": text(info.get("freezeAuthority")),
            "mint_authority": text(info.get("mintAuthority")),
            "extensions": extensions,
            "default_account_state": default_account_state,
            "transfer_hook": transfer_hook,
            "permanent_delegate": permanent_delegate,
            "non_transferable": non_transferable,
        }
    )
    return base


default_rows, default_bad = parse_jsonl(default_rows_path)
whatif_rows, whatif_bad = parse_jsonl(whatif_rows_path)
default_by_key = {row_key(row): row for row in default_rows}
metadata_cache: dict[str, dict] = {}

review_targets = [
    row
    for row in whatif_rows
    if row.get("decision") in {"DIRECTARB_EXIT_READY", "DIRECTARB_FLATTEN_READY_NO_SEND"}
    and not truth(default_by_key.get(row_key(row), {}).get("output_token_policy_pass"))
]

out_rows: list[dict] = []
for row in review_targets:
    default = default_by_key.get(row_key(row), {})
    mint = text(row.get("mint_out"))
    meta = metadata_cache.setdefault(mint, fetch_mint_metadata(mint))
    extensions = meta.get("extensions") or []
    extension_set = set(extensions)
    transfer_hook = bool(meta.get("transfer_hook"))
    permanent_delegate = bool(meta.get("permanent_delegate"))
    non_transferable = bool(meta.get("non_transferable"))
    default_account_state = text(meta.get("default_account_state"))
    freeze_authority = text(meta.get("freeze_authority"))
    mint_authority = text(meta.get("mint_authority"))
    token_program = text(meta.get("token_program") or row.get("output_token_program"))
    roundtrip_net = safe_int(row.get("roundtrip_net_after_all_costs"))

    if truth(row.get("exit_dust")) or not truth(row.get("exit_route_exists")):
        decision = "DENY_DUST_OR_SLIPPAGE"
    elif roundtrip_net <= 0:
        decision = "DENY_ROUNDTRIP_NEGATIVE"
    elif transfer_hook:
        decision = "DENY_TRANSFER_HOOK"
    elif permanent_delegate or non_transferable or default_account_state.lower() == "frozen" or mint_authority:
        decision = "DENY_AUTHORITY_RISK"
    elif freeze_authority:
        decision = "DENY_FREEZE_AUTHORITY"
    elif token_program == TOKEN2022_PROGRAM and set(extensions).issubset({"metadataPointer", "tokenMetadata"}):
        decision = "REVIEW_METADATA_ONLY_TOKEN2022"
    elif token_program == TOKEN2022_PROGRAM:
        decision = "DENY_AUTHORITY_RISK"
    else:
        decision = "DENY_AUTHORITY_RISK"

    out_rows.append(
        {
            "candidate_id": row.get("candidate_id"),
            "signature": row.get("signature"),
            "pool": row.get("pool"),
            "mint_out": mint,
            "token_program": token_program,
            "extensions": "|".join(extensions),
            "freeze_authority": freeze_authority,
            "mint_authority": mint_authority,
            "transfer_hook": transfer_hook,
            "permanent_delegate": permanent_delegate,
            "non_transferable": non_transferable,
            "default_account_state": default_account_state,
            "default_policy_blocker": default.get("output_token_policy_blocker", ""),
            "exit_route": row.get("exit_route_family"),
            "exit_value_usd": row.get("exit_value_usd"),
            "exit_slippage_bps": row.get("exit_slippage_bps"),
            "roundtrip_net_after_all_costs": row.get("roundtrip_net_after_all_costs"),
            "roundtrip_net_after_all_costs_usd": row.get("roundtrip_net_after_all_costs_usd"),
            "max_hold_ttl_ms": row.get("max_hold_ttl_ms"),
            "immediate_flatten_possible": truth(row.get("auto_flatten_possible")),
            "policy_whatif_decision": row.get("decision"),
            "decision": decision,
            "wire_send": 0,
            "live_landed": 0,
            "send_transaction_called": False,
            "sendBundle_called": False,
        }
    )


def write_jsonl(path: Path, payload: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in payload),
        encoding="utf-8",
    )


write_jsonl(rows_jsonl_path, out_rows)
fieldnames = list(out_rows[0].keys()) if out_rows else ["candidate_id", "decision"]
with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

decision_counts = Counter(row["decision"] for row in out_rows)
blocker_counts = Counter(str(row.get("default_policy_blocker") or "none") for row in out_rows)
report = {
    "status": "run78f3_complete" if out_rows else "blocked_no_policy_exception_rows",
    "epoch_dir": str(epoch_dir),
    "default_rows_path": str(default_rows_path),
    "whatif_rows_path": str(whatif_rows_path),
    "rpc_url": rpc_url,
    "default_rows_loaded": len(default_rows),
    "whatif_rows_loaded": len(whatif_rows),
    "policy_exception_review_count": len(out_rows),
    "malformed_default_rows": default_bad,
    "malformed_whatif_rows": whatif_bad,
    "decision_counts": dict(decision_counts),
    "default_policy_blocker_counts": dict(blocker_counts),
    "review_metadata_only_token2022_count": decision_counts.get("REVIEW_METADATA_ONLY_TOKEN2022", 0),
    "deny_freeze_authority_count": decision_counts.get("DENY_FREEZE_AUTHORITY", 0),
    "deny_transfer_hook_count": decision_counts.get("DENY_TRANSFER_HOOK", 0),
    "deny_authority_risk_count": decision_counts.get("DENY_AUTHORITY_RISK", 0),
    "deny_dust_or_slippage_count": decision_counts.get("DENY_DUST_OR_SLIPPAGE", 0),
    "deny_roundtrip_negative_count": decision_counts.get("DENY_ROUNDTRIP_NEGATIVE", 0),
    "wire_send": 0,
    "live_landed": 0,
    "send_transaction_called": False,
    "sendBundle_called": False,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = []
for key, value in report.items():
    if isinstance(value, dict):
        value = json.dumps(value, sort_keys=True)
    elif isinstance(value, bool):
        value = "true" if value else "false"
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
PY

pass "Run78F3 policy exception risk review complete"
finish

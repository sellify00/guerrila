#!/usr/bin/env bash
# 66t_token2022_explicit_allow_research.sh - policy/liquidity/rent audit for ATA-only convertible mints, no action

source "$(dirname "$0")/lib.sh"
script_header "66T - Token-2022 Explicit-Allow Research"

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
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66T_STEP:-66t_token2022_explicit_allow_research}"
STATUS_PREFIX="${SHADOW_RUN66T_STATUS_PREFIX:-run66t_token2022_explicit_allow_research}"
REPORT_BASENAME="${SHADOW_RUN66T_REPORT_BASENAME:-run66t_token2022_explicit_allow_research}"
RUN66R_ATA_STEP="${SHADOW_RUN66T_SOURCE_STEP:-66r_ata_only_convertible_planner_20260524_5m_bridge2}"
RUN66R_ATA_ROWS="${SHADOW_RUN66T_SOURCE_ROWS:-$RESULTS_ROOT/$RUN66R_ATA_STEP/run66r_ata_only_convertible_planner_rows.jsonl}"

RPC_URL="${SHADOW_RUN66T_RPC_URL:-${SHADOW_SOLANA_RPC_URL:-${SHADOW_BOOTSTRAP_RPC_URL:-$(default_http_rpc_url)}}}"
DENYLIST_MINTS="${SHADOW_RUN66T_DENYLIST_MINTS:-}"
MIN_EXIT_VALUE_USDC="${SHADOW_RUN66T_MIN_EXIT_VALUE_USDC:-0.10}"
MIN_FRESH_EPOCH_SEEN_COUNT="${SHADOW_RUN66T_MIN_FRESH_EPOCH_SEEN_COUNT:-2}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
ROWS_JSONL="$RESULT_DIR/run66t_token2022_explicit_allow_research_rows.jsonl"

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL"

if [[ ! -f "$RUN66R_ATA_ROWS" ]]; then
    fail "Missing Run66R-ATA rows: $RUN66R_ATA_ROWS"
    finish
    exit 1
fi

python3 - \
    "$RUN66R_ATA_ROWS" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$ROWS_JSONL" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$RPC_URL" \
    "$DENYLIST_MINTS" \
    "$MIN_EXIT_VALUE_USDC" \
    "$MIN_FRESH_EPOCH_SEEN_COUNT" <<'PY'
import base64
import json
import sys
import urllib.request
from collections import Counter
from pathlib import Path

SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
EXTENSION_NAMES = {
    0: "Uninitialized",
    1: "TransferFeeConfig",
    2: "TransferFeeAmount",
    3: "MintCloseAuthority",
    4: "ConfidentialTransferMint",
    5: "ConfidentialTransferAccount",
    6: "DefaultAccountState",
    7: "ImmutableOwner",
    8: "MemoTransfer",
    9: "NonTransferable",
    10: "InterestBearingConfig",
    11: "CpiGuard",
    12: "PermanentDelegate",
    13: "NonTransferableAccount",
    14: "TransferHook",
    15: "TransferHookAccount",
    16: "ConfidentialTransferFeeConfig",
    17: "ConfidentialTransferFeeAmount",
    18: "MetadataPointer",
    19: "TokenMetadata",
    20: "GroupPointer",
    21: "TokenGroup",
    22: "GroupMemberPointer",
    23: "TokenGroupMember",
    24: "ConfidentialMintBurn",
    25: "ScaledUiAmount",
    26: "Pausable",
    27: "PausableAccount",
}
METADATA_FAMILY = {"MetadataPointer", "TokenMetadata"}
HARD_RISK_EXTENSIONS = {
    "TransferFeeConfig",
    "MintCloseAuthority",
    "ConfidentialTransferMint",
    "DefaultAccountState",
    "NonTransferable",
    "InterestBearingConfig",
    "PermanentDelegate",
    "TransferHook",
    "ConfidentialTransferFeeConfig",
    "GroupPointer",
    "TokenGroup",
    "GroupMemberPointer",
    "TokenGroupMember",
    "ConfidentialMintBurn",
    "ScaledUiAmount",
    "Pausable",
}

source_rows_path = Path(sys.argv[1])
summary_file = Path(sys.argv[2])
summary_json = Path(sys.argv[3])
rows_jsonl = Path(sys.argv[4])
step = sys.argv[5]
status_prefix = sys.argv[6]
rpc_url = sys.argv[7]
denylist_mints = {mint.strip() for mint in (sys.argv[8] or "").replace(",", " ").split() if mint.strip()}
min_exit_value_usdc = float(sys.argv[9] or "0.10")
min_fresh_epoch_seen_count = int(sys.argv[10] or "2")


def load_jsonl(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            pass
    return rows


def write_jsonl(path, rows):
    with Path(path).open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def text(value):
    return str(value or "").strip()


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
    return round(float(value), digits)


def b58encode(raw):
    if not raw:
        return None
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, rem = divmod(n, 58)
        out = BASE58_ALPHABET[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + (out or "1")


def rpc_call(method, params):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(rpc_url, data=payload, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        data = json.loads(resp.read().decode())
    if data.get("error"):
        raise RuntimeError(data["error"])
    return data.get("result")


def parse_tlv_extensions(raw, owner):
    if owner != TOKEN2022_PROGRAM or len(raw) <= 82:
        return []
    starts = []
    if len(raw) > 83 and raw[82] in {0, 1, 2}:
        starts.append(83)
    starts.append(82)
    best = []
    for start in starts:
        cursor = start
        exts = []
        ok = True
        while cursor + 4 <= len(raw):
            ext_type = int.from_bytes(raw[cursor:cursor + 2], "little")
            ext_len = int.from_bytes(raw[cursor + 2:cursor + 4], "little")
            cursor += 4
            if ext_type == 0 and ext_len == 0:
                break
            if ext_type not in EXTENSION_NAMES or cursor + ext_len > len(raw):
                ok = False
                break
            exts.append(EXTENSION_NAMES[ext_type])
            cursor += ext_len
        if ok and len(exts) >= len(best):
            best = exts
    return sorted(set(best))


def decode_mint_account(mint, account):
    if not account:
        return {"mint": mint, "metadata_available": False, "token_program": None, "decimals": None, "extensions": [], "mint_authority": None, "freeze_authority": None}
    owner = account.get("owner")
    raw_payload = ((account.get("data") or [None])[0]) if isinstance(account.get("data"), list) else None
    raw = base64.b64decode(raw_payload) if raw_payload else b""
    decimals = raw[44] if len(raw) >= 45 else None
    mint_auth = None
    freeze_auth = None
    if len(raw) >= 82:
        if int.from_bytes(raw[0:4], "little") != 0:
            mint_auth = b58encode(raw[4:36])
        if int.from_bytes(raw[46:50], "little") != 0:
            freeze_auth = b58encode(raw[50:82])
    extensions = parse_tlv_extensions(raw, owner)
    return {"mint": mint, "metadata_available": True, "token_program": owner, "decimals": decimals, "extensions": extensions, "mint_authority": mint_auth, "freeze_authority": freeze_auth}


def fetch_base64_metadata(mints):
    out = {}
    for i in range(0, len(mints), 100):
        batch = mints[i:i + 100]
        accounts = rpc_call("getMultipleAccounts", [batch, {"encoding": "base64"}]).get("value") or []
        for mint, account in zip(batch, accounts):
            out[mint] = decode_mint_account(mint, account)
    return out


def normalize_ext_name(value):
    value = text(value)
    if not value:
        return ""
    return value[:1].upper() + value[1:]


def fetch_parsed_extensions(mints):
    out = {}
    try:
        for i in range(0, len(mints), 100):
            batch = mints[i:i + 100]
            accounts = rpc_call("getMultipleAccounts", [batch, {"encoding": "jsonParsed"}]).get("value") or []
            for mint, account in zip(batch, accounts):
                details = {}
                try:
                    info = (((account or {}).get("data") or {}).get("parsed") or {}).get("info") or {}
                    exts = []
                    for ext in info.get("extensions") or []:
                        if isinstance(ext, str):
                            exts.append(normalize_ext_name(ext))
                        elif isinstance(ext, dict):
                            ext_type = normalize_ext_name(ext.get("extension") or ext.get("extensionType") or ext.get("type"))
                            if ext_type:
                                exts.append(ext_type)
                                details[ext_type] = ext.get("state") or ext.get("value") or ext
                    for ext in info.get("extensionTypes") or []:
                        ext_name = normalize_ext_name(ext)
                        if ext_name:
                            exts.append(ext_name)
                    out[mint] = {"parsed_extensions": sorted(set(exts)), "parsed_extension_details": details}
                except Exception:
                    out[mint] = {"parsed_extensions": [], "parsed_extension_details": {}}
    except Exception:
        return {}
    return out


def combined_metadata(mints):
    base = fetch_base64_metadata(mints)
    parsed = fetch_parsed_extensions(mints)
    for mint, meta in base.items():
        parsed_meta = parsed.get(mint) or {}
        parsed_exts = parsed_meta.get("parsed_extensions") or []
        exts = sorted(set((meta.get("extensions") or []) + parsed_exts))
        meta["extensions"] = exts
        meta["parsed_extension_details"] = parsed_meta.get("parsed_extension_details") or {}
    return base


def explicit_allow_analysis(meta):
    token_program = meta.get("token_program")
    extensions = set(meta.get("extensions") or [])
    denylisted = meta.get("mint") in denylist_mints
    risk_exts = sorted(extensions & HARD_RISK_EXTENSIONS)
    authority_risks = []
    if meta.get("mint_authority"):
        authority_risks.append("mint_authority_present")
    if meta.get("freeze_authority"):
        authority_risks.append("freeze_authority_present")
    if denylisted:
        return False, "denylisted", risk_exts, authority_risks
    if token_program == SPL_TOKEN_PROGRAM:
        if authority_risks:
            return False, "spl_token_authority_risk", risk_exts, authority_risks
        return False, "spl_token_policy_pass_no_explicit_allow_needed", risk_exts, authority_risks
    if token_program != TOKEN2022_PROGRAM:
        return False, "unsupported_token_program", risk_exts, authority_risks
    if risk_exts:
        return False, "hard_token2022_extension_risk", risk_exts, authority_risks
    if authority_risks:
        return False, "token2022_authority_risk", risk_exts, authority_risks
    if extensions.issubset(METADATA_FAMILY):
        return True, "metadata_only_token2022_no_risky_extensions", risk_exts, authority_risks
    if not extensions:
        return True, "token2022_no_detected_extensions_requires_explicit_allow", risk_exts, authority_risks
    return False, "unclassified_token2022_extension_set", risk_exts, authority_risks


source_rows = load_jsonl(source_rows_path)
mints = sorted({text(row.get("mint")) for row in source_rows if text(row.get("mint"))})
metadata = combined_metadata(mints) if mints else {}

items = []
for row in source_rows:
    mint = text(row.get("mint"))
    if not mint:
        continue
    meta = metadata.get(mint) or {"mint": mint, "metadata_available": False, "extensions": []}
    meta["mint"] = mint
    explicit_possible, explicit_reason, risk_exts, authority_risks = explicit_allow_analysis(meta)
    extensions = set(meta.get("extensions") or [])
    exit_quote_unit = row.get("unit_exit_probe_value_usdc")
    exit_quote_full_band = row.get("full_band_exit_probe_value_usdc")
    exit_dust = bool(row.get("exit_dust")) or safe_float(exit_quote_full_band) < min_exit_value_usdc
    rent_adjusted_ev = safe_int(row.get("total_net_ev_after_ata_cost_lamports"))
    denylist_status = "denylisted" if mint in denylist_mints else text(row.get("denylist_status") or "not_denylisted")
    policy_verdict = text(row.get("policy_verdict"))
    if risk_exts or denylist_status == "denylisted" or text(meta.get("token_program")) not in {SPL_TOKEN_PROGRAM, TOKEN2022_PROGRAM}:
        decision = "REJECT_EXTENSION_RISK"
    elif authority_risks:
        decision = "REJECT_AUTHORITY_RISK"
    elif exit_dust:
        decision = "REJECT_DUST_EXIT"
    elif rent_adjusted_ev <= 0:
        decision = "REJECT_RENT_NEGATIVE"
    elif safe_int(row.get("fresh_epoch_seen_count")) < min_fresh_epoch_seen_count:
        decision = "RETRY_LONGER_EPOCH"
    elif explicit_possible:
        decision = "EXPLICIT_ALLOW_CANDIDATE"
    else:
        decision = "PARK"
    item = {
        "mint": mint,
        "token_program": meta.get("token_program"),
        "extensions": sorted(extensions),
        "metadata_pointer": "MetadataPointer" in extensions,
        "metadata_pointer_detail": (meta.get("parsed_extension_details") or {}).get("MetadataPointer"),
        "token_metadata": "TokenMetadata" in extensions,
        "token_metadata_detail": (meta.get("parsed_extension_details") or {}).get("TokenMetadata"),
        "transfer_hook": "TransferHook" in extensions,
        "transfer_hook_detail": (meta.get("parsed_extension_details") or {}).get("TransferHook"),
        "permanent_delegate": "PermanentDelegate" in extensions,
        "permanent_delegate_detail": (meta.get("parsed_extension_details") or {}).get("PermanentDelegate"),
        "non_transferable": "NonTransferable" in extensions,
        "default_account_state": "DefaultAccountState" in extensions,
        "default_account_state_detail": (meta.get("parsed_extension_details") or {}).get("DefaultAccountState"),
        "mint_authority": meta.get("mint_authority"),
        "freeze_authority": meta.get("freeze_authority"),
        "authority_risks": authority_risks,
        "decimals": meta.get("decimals"),
        "denylist_status": denylist_status,
        "policy_verdict": policy_verdict,
        "explicit_allow_possible": explicit_possible,
        "explicit_allow_reason": explicit_reason,
        "extension_risks": risk_exts,
        "exit_quote_unit": exit_quote_unit,
        "exit_quote_full_band": exit_quote_full_band,
        "exit_dust": exit_dust,
        "rent_adjusted_ev": rent_adjusted_ev,
        "row_count": safe_int(row.get("row_count")),
        "synthetic_ata_would_fire_delta": safe_int(row.get("synthetic_ata_would_fire_delta")),
        "total_net_ev_after_fee_tip": safe_int(row.get("total_net_ev_after_fee_tip")),
        "best_row_ev": safe_int(row.get("best_row_ev")),
        "estimated_break_even_rows": row.get("estimated_break_even_rows"),
        "fresh_epoch_seen_count": safe_int(row.get("fresh_epoch_seen_count")),
        "decision": decision,
        "wire_send": 0,
        "live_landed": 0,
    }
    items.append(item)

decision_rank = {
    "EXPLICIT_ALLOW_CANDIDATE": 6,
    "RETRY_LONGER_EPOCH": 5,
    "PARK": 4,
    "REJECT_RENT_NEGATIVE": 3,
    "REJECT_DUST_EXIT": 2,
    "REJECT_AUTHORITY_RISK": 1,
    "REJECT_EXTENSION_RISK": 0,
}
items.sort(
    key=lambda item: (
        decision_rank.get(item["decision"], -1),
        item["synthetic_ata_would_fire_delta"],
        item["rent_adjusted_ev"],
        item["best_row_ev"],
    ),
    reverse=True,
)
write_jsonl(rows_jsonl, items)

summary = {
    "status": f"{status_prefix}_complete",
    "step": step,
    "source_run66r_ata_rows": str(source_rows_path),
    "mint_group_count": len(items),
    "decision_counts": dict(Counter(item["decision"] for item in items)),
    "explicit_allow_possible_count": sum(1 for item in items if item["explicit_allow_possible"]),
    "explicit_allow_candidate_count": sum(1 for item in items if item["decision"] == "EXPLICIT_ALLOW_CANDIDATE"),
    "retry_longer_epoch_count": sum(1 for item in items if item["decision"] == "RETRY_LONGER_EPOCH"),
    "reject_extension_risk_count": sum(1 for item in items if item["decision"] == "REJECT_EXTENSION_RISK"),
    "reject_authority_risk_count": sum(1 for item in items if item["decision"] == "REJECT_AUTHORITY_RISK"),
    "reject_dust_exit_count": sum(1 for item in items if item["decision"] == "REJECT_DUST_EXIT"),
    "reject_rent_negative_count": sum(1 for item in items if item["decision"] == "REJECT_RENT_NEGATIVE"),
    "metadata_only_or_no_risk_token2022_count": sum(1 for item in items if item["explicit_allow_possible"]),
    "wire_send": 0,
    "live_landed": 0,
    "top_items": items[:20],
}
summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    f"status={summary['status']}",
    f"source_run66r_ata_rows={source_rows_path}",
    f"mint_group_count={summary['mint_group_count']}",
    f"decision_counts={json.dumps(summary['decision_counts'], sort_keys=True)}",
    f"explicit_allow_possible_count={summary['explicit_allow_possible_count']}",
    f"explicit_allow_candidate_count={summary['explicit_allow_candidate_count']}",
    f"retry_longer_epoch_count={summary['retry_longer_epoch_count']}",
    f"reject_extension_risk_count={summary['reject_extension_risk_count']}",
    f"reject_authority_risk_count={summary['reject_authority_risk_count']}",
    f"reject_dust_exit_count={summary['reject_dust_exit_count']}",
    f"reject_rent_negative_count={summary['reject_rent_negative_count']}",
    "wire_send=0",
    "live_landed=0",
]
if items:
    top = items[0]
    lines.extend([
        f"top_mint={top['mint']}",
        f"top_decision={top['decision']}",
        f"top_policy_verdict={top['policy_verdict']}",
        f"top_explicit_allow_possible={str(top['explicit_allow_possible']).lower()}",
        f"top_explicit_allow_reason={top['explicit_allow_reason']}",
        f"top_token_program={top['token_program']}",
        f"top_extensions={json.dumps(top['extensions'], sort_keys=True)}",
        f"top_authority_risks={json.dumps(top['authority_risks'], sort_keys=True)}",
        f"top_exit_quote_unit={top['exit_quote_unit']}",
        f"top_exit_quote_full_band={top['exit_quote_full_band']}",
        f"top_exit_dust={str(top['exit_dust']).lower()}",
        f"top_rent_adjusted_ev={top['rent_adjusted_ev']}",
        f"top_row_count={top['row_count']}",
        f"top_synthetic_ata_would_fire_delta={top['synthetic_ata_would_fire_delta']}",
    ])
summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

pass "Run66T Token-2022 explicit-allow research written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

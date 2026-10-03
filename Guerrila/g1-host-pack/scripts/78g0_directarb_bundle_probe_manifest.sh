#!/usr/bin/env bash
# 78G0 - materialize DirectArb bundle probe rows into a 78G candidate manifest

source "$(dirname "$0")/lib.sh"
script_header "78G0 - DirectArb Bundle Probe Manifest"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
RESULT_ROOT="${SHADOW_RUN78G0_RESULT_ROOT:-}"

if [[ -z "$RESULT_ROOT" ]]; then
    RESULT_ROOT="$(ls -td "$RESULTS_ROOT"/epoch8_auto_directarb_jito_bundle_roundtrip_nosubmit_fresh_selector* 2>/dev/null | head -n1 || true)"
fi

if [[ -z "$RESULT_ROOT" || ! -d "$RESULT_ROOT" ]]; then
    fail "Missing 78G0 result root; set SHADOW_RUN78G0_RESULT_ROOT"
    finish
    exit 1
fi

PROBE_ROWS="${SHADOW_RUN78G0_PROBE_ROWS:-$RESULT_ROOT/_inner/20_g12_r0_observe_only/run78g_directarb_bundle_probe_rows.jsonl}"
OUTPUT_PATH="${SHADOW_RUN78G0_OUTPUT_PATH:-$RESULT_ROOT/78g_bundle_candidate_manifest.jsonl}"
REJECTION_PATH="${SHADOW_RUN78G0_REJECTION_PATH:-$RESULT_ROOT/78g_bundle_candidate_rejections.jsonl}"
SUMMARY_PATH="${SHADOW_RUN78G0_SUMMARY_PATH:-$RESULT_ROOT/78g0_directarb_bundle_probe_manifest_summary.txt}"
FLATTEN_SAFETY_BPS="${SHADOW_RUN78G0_FLATTEN_SAFETY_BPS:-${SHADOW_RUN78G_FLATTEN_SAFETY_BPS:-0}}"

python3 - \
    "$PROBE_ROWS" \
    "$OUTPUT_PATH" \
    "$REJECTION_PATH" \
    "$SUMMARY_PATH" \
    "$FLATTEN_SAFETY_BPS" <<'PY'
from __future__ import annotations

import json
import re
import sys
from collections import Counter
from pathlib import Path

probe_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
rejection_path = Path(sys.argv[3])
summary_path = Path(sys.argv[4])
flatten_safety_bps = max(0, min(5000, int(sys.argv[5] or "0")))

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def text(value) -> str:
    return str(value or "").strip()


def safe_int(value, default=0) -> int:
    try:
        if value in (None, ""):
            return default
        return int(str(value))
    except Exception:
        return default


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def base58_encode(raw: bytes) -> str:
    if not raw:
        return ""
    number = int.from_bytes(raw, "big")
    out = ""
    while number:
        number, rem = divmod(number, 58)
        out = BASE58_ALPHABET[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + (out or "1")


def b58_from_hex(value: str) -> str:
    raw = bytes.fromhex(value) if value and re.fullmatch(r"[0-9a-fA-F]+", value) else b""
    return base58_encode(raw) if raw else ""


rows = []
malformed_rows = 0
if probe_path.exists():
    for raw in probe_path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            malformed_rows += 1

manifest = []
rejections = []

for index, row in enumerate(rows, 1):
    reasons = []
    if text(row.get("record_type")) != "runls11_bundle_probe":
        reasons.append("NOT_BUNDLE_PROBE_ROW")
    if text(row.get("signal_source")) != "afxdp":
        reasons.append("NOT_AFXDP")
    if text(row.get("route_family")) != "jito_bundle":
        reasons.append("NOT_JITO_BUNDLE_ROUTE")

    owner_mode = text(row.get("owner_mode")).lower()
    owner_mode_id = safe_int(row.get("owner_mode_id"), -1)
    if owner_mode_id == -1 and owner_mode and owner_mode != "directarb":
        reasons.append("NOT_DIRECTARB_OWNER_MODE")
    if owner_mode_id not in (-1, 0):
        reasons.append("NOT_DIRECTARB_OWNER_MODE_ID")

    entry_min_out_raw = safe_int(row.get("min_out_raw"))
    if entry_min_out_raw <= 0:
        reasons.append("MISSING_QUOTE")
    if not as_bool(row.get("pm_reserve_ok")):
        reasons.append("MISSING_PM_ACCOUNT")

    if reasons:
        rejections.append(
            {
                "row_index": index,
                "reasons": reasons,
                "first_reason": reasons[0],
                "record_type": text(row.get("record_type")),
                "signal_source": text(row.get("signal_source")),
                "route_family": text(row.get("route_family")),
                "owner_mode": text(row.get("owner_mode")),
                "owner_mode_id": row.get("owner_mode_id"),
            }
        )
        continue

    flatten_safety_margin_raw = (entry_min_out_raw * flatten_safety_bps) // 10000
    flatten_input_raw = max(1, entry_min_out_raw - flatten_safety_margin_raw)
    mint_out = b58_from_hex(text(row.get("mint_out_hex")))
    signature = b58_from_hex(text(row.get("target_signature_hex") or row.get("our_signature_hex")))

    manifest.append(
        {
            "candidate_id": signature or f"bundle_probe_{index}",
            "pool": b58_from_hex(text(row.get("pool_pubkey_hex") or row.get("target_pool_pubkey_hex"))),
            "venue": text(row.get("venue") or row.get("target_venue")),
            "mint_in": b58_from_hex(text(row.get("mint_in_hex"))),
            "mint_out": mint_out,
            "entry_amount_raw": safe_int(row.get("trade_size_lamports") or row.get("target_amount_in")),
            "entry_expected_out_raw": safe_int(row.get("expected_out_raw")),
            "entry_min_out_raw": entry_min_out_raw,
            "flatten_input_raw": flatten_input_raw,
            "flatten_input_lte_entry_min_out": flatten_input_raw <= entry_min_out_raw,
            "exit_route": "PENDING_78G_RUNTIME_QUOTE",
            "exit_min_out_raw": 0,
            "roundtrip_net_after_all_costs": "",
            "policy_verdict": "PENDING_78G_EVALUATION",
            "output_token_account": "",
            "output_account_preexisting": "",
            "output_account_created_by_entry_expected": "",
            "pm_entry_account": "",
            "pm_flatten_account": "",
            "route_family": "jito_bundle",
            "source_mode": "bundle_probe",
            "pm_reserve_ok": as_bool(row.get("pm_reserve_ok")),
            "net_ev_after_tip_lamports": safe_int(row.get("net_ev_after_tip_lamports")),
            "template_id": safe_int(row.get("template_id")),
        }
    )

manifest.sort(
    key=lambda row: (
        bool(row.get("pm_reserve_ok")),
        safe_int(row.get("net_ev_after_tip_lamports")),
        safe_int(row.get("entry_expected_out_raw")),
    ),
    reverse=True,
)

output_path.parent.mkdir(parents=True, exist_ok=True)
rejection_path.parent.mkdir(parents=True, exist_ok=True)
summary_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in manifest),
    encoding="utf-8",
)
rejection_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in rejections),
    encoding="utf-8",
)

rejection_counts = Counter(item["first_reason"] for item in rejections)
status = "manifest_ready" if manifest else "blocked_no_materialized_bundle_probe_rows"
summary = {
    "status": status,
    "probe_rows": len(rows),
    "manifest_rows": len(manifest),
    "malformed_probe_rows": malformed_rows,
    "rejection_rows": len(rejections),
    "rejection_counts": dict(rejection_counts),
    "output_path": str(output_path),
    "rejection_path": str(rejection_path),
}
summary_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, sort_keys=True) if isinstance(value, dict) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
print(summary_path.read_text(encoding="utf-8"), end="")
PY

manifest_rows="$(wc -l < "$OUTPUT_PATH" 2>/dev/null || echo 0)"
if [[ "$manifest_rows" -gt 0 ]]; then
    pass "78G0 materialized $manifest_rows DirectArb bundle candidate row(s)"
else
    warn "78G0 materialized zero rows; see $SUMMARY_PATH and $REJECTION_PATH"
fi
finish

#!/usr/bin/env bash
# 78C - report original-size cap required for EV-positive DirectArb rows.

source "$(dirname "$0")/lib.sh"
script_header "78C - DirectArb Required Cap Requote Report"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
SOURCE_DIR=""
OUTPUT_DIR=""
CAP_TOO_HIGH_SOL="${SHADOW_RUN78C_CAP_TOO_HIGH_SOL:-1.0}"
CURRENT_APPROVED_CAP_SOL="${SHADOW_RUN78C_APPROVED_LIVE_CAP_SOL:-}"
SOL_USD_EQUIVALENT="${SHADOW_RUN78C_SOL_USD_EQUIVALENT:-${SHADOW_RUN78_SOL_USD_EQUIVALENT:-}}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source)
            SOURCE_DIR="${2:-}"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        --cap-too-high-sol)
            CAP_TOO_HIGH_SOL="${2:-}"
            shift 2
            ;;
        --current-approved-cap-sol)
            CURRENT_APPROVED_CAP_SOL="${2:-}"
            shift 2
            ;;
        *)
            fail "Unknown argument: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$SOURCE_DIR" ]]; then
    SOURCE_DIR="${SHADOW_RUN78C_SOURCE_DIR:-$RESULTS_ROOT/epoch8_auto_strict_directarb_ev_positive_input_readiness_split}"
fi
if [[ ! -d "$SOURCE_DIR" ]]; then
    fail "Source directory not found: $SOURCE_DIR"
    exit 1
fi

if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$SOURCE_DIR/78c_directarb_required_cap_requote"
fi

SUMMARY_FILE="$OUTPUT_DIR/summary.txt"
SUMMARY_JSON="$OUTPUT_DIR/run78c_directarb_required_cap_requote.json"
ROWS_JSONL="$OUTPUT_DIR/rows.jsonl"
ROWS_CSV="$OUTPUT_DIR/rows.csv"

mkdir -p "$OUTPUT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

python3 - "$SOURCE_DIR" "$OUTPUT_DIR" "$CAP_TOO_HIGH_SOL" "$CURRENT_APPROVED_CAP_SOL" "$SOL_USD_EQUIVALENT" "$PACK_DIR" <<'PY'
from __future__ import annotations

import csv
import json
import os
import re
import sys
from collections import Counter
from decimal import Decimal, InvalidOperation
from pathlib import Path

source_dir = Path(sys.argv[1])
output_dir = Path(sys.argv[2])
cap_too_high_sol_arg = sys.argv[3]
current_approved_cap_sol_arg = sys.argv[4]
sol_usd_arg = sys.argv[5]
pack_dir = Path(sys.argv[6])

summary_path = output_dir / "summary.txt"
summary_json_path = output_dir / "run78c_directarb_required_cap_requote.json"
rows_jsonl_path = output_dir / "rows.jsonl"
rows_csv_path = output_dir / "rows.csv"

WSOL_MINT = "So11111111111111111111111111111111111111112"
CORE_INPUT_MINTS = {
    WSOL_MINT,
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2",
}


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


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        return payload if isinstance(payload, dict) else {}
    except Exception:
        return {}


def safe_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def truth(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def dec_or_none(value) -> Decimal | None:
    if value in (None, ""):
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def dec_to_str(value: Decimal | None) -> str | None:
    if value is None:
        return None
    normalized = value.normalize()
    if normalized == normalized.to_integral():
        return str(normalized.quantize(Decimal("1")))
    return format(normalized, "f")


def raw_to_sol(raw: int) -> Decimal:
    return Decimal(raw) / Decimal(1_000_000_000)


def raw_to_native(raw: int, mint: str) -> Decimal | None:
    if mint == WSOL_MINT:
        return raw_to_sol(raw)
    if mint in {
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "Es9vMFrzaCERmJfrF4H2FYD6oGnq3xD3xZg8tT9NtW2",
    }:
        return Decimal(raw) / Decimal(1_000_000)
    return None


def identity_parts(row: dict) -> tuple[str, str, str, str]:
    identity = str(row.get("identity") or "")
    if identity.count("|") >= 3:
        sig, pool, mint_in, mint_out = identity.split("|", 3)
        return sig, pool, mint_in, mint_out
    return "", "", "", ""


def key4(row: dict) -> tuple[str, str, str, str]:
    identity_sig, identity_pool, identity_mint_in, identity_mint_out = identity_parts(row)
    return (
        str(row.get("signature") or identity_sig or ""),
        str(row.get("pool") or identity_pool or ""),
        str(row.get("mint_in") or identity_mint_in or ""),
        str(row.get("mint_out") or identity_mint_out or ""),
    )


def first_present(row: dict, keys: tuple[str, ...], default=""):
    for key in keys:
        value = row.get(key)
        if value not in (None, ""):
            return value
    return default


def parse_dotenv_sender(repo_root: Path) -> dict:
    dotenv = repo_root / ".env"
    out = {
        "helius_sender_url": "",
        "tip_accounts": [],
        "tip_accounts_source": "",
    }
    if not dotenv.exists():
        return out
    text = dotenv.read_text(encoding="utf-8", errors="replace")
    match = re.search(r"(?m)^HELIUS_SENDER_URL=(.+)$", text)
    if match:
        out["helius_sender_url"] = match.group(1).strip().strip('"').strip("'")
    tip_match = re.search(r'(?ms)^tip_accounts\s*=\s*"([^"]*)"', text)
    if tip_match:
        accounts = [
            item.strip().strip(",")
            for item in re.split(r"[\s,]+", tip_match.group(1))
            if item.strip().strip(",")
        ]
        out["tip_accounts"] = list(dict.fromkeys(accounts))
        out["tip_accounts_source"] = str(dotenv)
    return out


def parse_config_sender(pack_dir: Path) -> dict:
    path = pack_dir / "config.guerrila.yaml"
    out = {
        "helius_sender_url": "",
        "tip_accounts": [],
        "tip_accounts_source": "",
    }
    if not path.exists():
        return out
    stack: list[tuple[int, str]] = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        while stack and stack[-1][0] >= indent:
            stack.pop()
        if stripped.endswith(":") and not stripped.startswith("- "):
            key = stripped[:-1]
            prefix = stack[-1][1] + "." if stack else ""
            stack.append((indent, prefix + key))
            continue
        current = stack[-1][1] if stack else ""
        if current == "helius_sender" and stripped.startswith("endpoint:"):
            value = stripped.split(":", 1)[1].strip().strip('"').strip("'")
            if value and "${" not in value:
                out["helius_sender_url"] = value
        elif current == "helius_sender.tip_accounts" and stripped.startswith("- "):
            out["tip_accounts"].append(stripped[2:].strip().strip('"').strip("'"))
    out["tip_accounts"] = list(dict.fromkeys([item for item in out["tip_accounts"] if item]))
    if out["tip_accounts"]:
        out["tip_accounts_source"] = str(path)
    return out


def infer_epoch_root(source_dir: Path) -> Path:
    if source_dir.name == "epoch8_auto_strict_directarb_ev_positive_input_readiness_split":
        return source_dir.parent
    return source_dir.parent


repo_root = pack_dir.parent
sender_cfg = parse_dotenv_sender(repo_root)
config_sender_cfg = parse_config_sender(pack_dir)
helius_sender_url = (
    os.environ.get("SHADOW_HELIUS_SENDER_URL")
    or os.environ.get("HELIUS_SENDER_URL")
    or os.environ.get("SHADOW_HELIUS_SENDER_ENDPOINT")
    or sender_cfg["helius_sender_url"]
    or config_sender_cfg["helius_sender_url"]
    or "https://sender.helius-rpc.com/fast"
)
tip_accounts = sender_cfg["tip_accounts"] or config_sender_cfg["tip_accounts"]
tip_accounts_count = len(tip_accounts)

split_rows_path = source_dir / "rows.jsonl"
split_report_path = source_dir / "run78d_directarb_ev_positive_input_readiness_split.json"
split_report = load_json(split_report_path)
epoch_root = infer_epoch_root(source_dir)

run78a_rows_path = Path(
    ((split_report.get("run78a_nearmiss") or {}).get("source_path") or "")
)
if not str(run78a_rows_path):
    run78a_rows_path = epoch_root / "epoch8_auto_strict_directarb_private_egress_run78a_nearmiss_autopsy" / "rows.jsonl"

raw_selector_rows_path = Path(
    ((split_report.get("raw_selector") or {}).get("source_path") or "")
)
if not str(raw_selector_rows_path):
    raw_selector_rows_path = epoch_root / "78_fresh_strict_directarb_candidate_scout_nosend" / "run78c_selector_rejections.jsonl"

split_rows, split_bad = parse_jsonl(split_rows_path)
run78a_rows, run78a_bad = parse_jsonl(run78a_rows_path)
raw_rows, raw_bad = parse_jsonl(raw_selector_rows_path)
run78a_by_key = {key4(row): row for row in run78a_rows}
raw_by_key = {key4(row): row for row in raw_rows}

cap_rows = [
    row
    for row in split_rows
    if str(row.get("scope") or "") == "run78a_nearmiss"
    and truth(row.get("ev_positive"))
    and str(row.get("first_blocker") or "") == "input_notional_within_cap"
    and not truth(row.get("unsupported_input"))
    and truth(row.get("existing_input_ready"))
    and truth(row.get("pm_reserve_ok"))
    and truth(row.get("template_ready"))
    and truth(row.get("template_pm_account_match"))
]

if not cap_rows:
    for row in run78a_rows:
        if (
            str(row.get("first_failed") or "") == "input_notional_within_cap"
            and str(row.get("mint_in") or "") in CORE_INPUT_MINTS
            and truth(row.get("candidate_ev_positive"))
            and truth(row.get("pm_reserve_can_lock_exact_input"))
            and truth(row.get("template_ready"))
            and truth(row.get("template_pm_account_match"))
        ):
            cap_rows.append(
                {
                    "scope": "run78a_nearmiss",
                    "signature": row.get("signature"),
                    "pool": row.get("pool"),
                    "mint_in": row.get("mint_in"),
                    "mint_out": row.get("mint_out"),
                    "first_blocker": "input_notional_within_cap",
                    "ev_positive": True,
                    "unsupported_input": False,
                    "existing_input_ready": True,
                    "pm_reserve_ok": True,
                    "template_ready": True,
                    "template_pm_account_match": True,
                }
            )

cap_too_high_sol = dec_or_none(cap_too_high_sol_arg) or Decimal("1.0")
fallback_current_cap_sol = dec_or_none(current_approved_cap_sol_arg) or Decimal("0.01")
sol_usd = dec_or_none(sol_usd_arg)

out_rows: list[dict] = []
for index, split_row in enumerate(cap_rows, start=1):
    key = key4(split_row)
    autopsy = run78a_by_key.get(key, {})
    raw = raw_by_key.get(key, {})

    _, identity_pool, identity_mint_in, identity_mint_out = identity_parts(split_row)
    mint_in = str(split_row.get("mint_in") or autopsy.get("mint_in") or identity_mint_in or "")
    mint_out = str(split_row.get("mint_out") or autopsy.get("mint_out") or identity_mint_out or "")
    original_input_raw = safe_int(
        first_present(
            autopsy,
            ("pm_required_input_raw",),
            first_present(split_row, ("exact_input_required_raw",), first_present(raw, ("exact_input_required_raw", "input_amount_raw"), 0)),
        )
    )
    original_input_native = raw_to_native(original_input_raw, mint_in)
    original_input_sol = raw_to_sol(original_input_raw) if mint_in == WSOL_MINT else dec_or_none(raw.get("input_notional_sol_equiv"))
    if original_input_sol is None:
        original_input_sol = dec_or_none(split_row.get("input_notional_sol_equiv")) or Decimal(0)
    original_input_usd = original_input_sol * sol_usd if sol_usd is not None else None

    current_cap = (
        dec_or_none(raw.get("approved_live_cap_sol"))
        or dec_or_none(autopsy.get("approved_live_cap_sol"))
        or fallback_current_cap_sol
    )
    required_cap = original_input_sol
    quote_expected_out_raw = safe_int(autopsy.get("quote_expected_out_raw") or raw.get("expected_out_raw"))
    min_out_raw = safe_int(autopsy.get("min_out_raw") or raw.get("min_out_raw"))
    min_out_positive = min_out_raw > 0 or truth(autopsy.get("min_out_positive")) or truth(split_row.get("min_out_positive"))
    base_fee = safe_int(autopsy.get("base_fee") or raw.get("base_fee_raw"))
    tip = safe_int(autopsy.get("tip_lamports") or raw.get("policy_tip_raw"))
    net_ev_after_fee_tip = safe_int(autopsy.get("net_ev_after_fee_tip") or raw.get("candidate_net_after_fee_raw"))
    net_ev_positive = net_ev_after_fee_tip > 0
    pm_available_input_raw = safe_int(
        autopsy.get("pm_available_input_raw")
        or split_row.get("pm_available_input_raw")
        or raw.get("pm_available_input_raw")
        or raw.get("current_input_balance_raw")
    )
    pm_reserve_ok_at_original_size = (
        truth(autopsy.get("pm_reserve_can_lock_exact_input"))
        or truth(split_row.get("pm_reserve_ok"))
        or truth(raw.get("pm_reserve_can_lock_exact_input"))
    ) and pm_available_input_raw >= original_input_raw > 0
    template_ready = truth(autopsy.get("template_ready")) or truth(split_row.get("template_ready")) or truth(raw.get("template_ready"))
    template_pm_account_match = (
        truth(autopsy.get("template_pm_account_match"))
        or truth(split_row.get("template_pm_account_match"))
        or truth(raw.get("template_pm_account_match"))
    )
    existing_input_ready = truth(split_row.get("existing_input_ready")) or str(
        autopsy.get("input_account_resolution_blocker") or raw.get("input_account_resolution_blocker") or ""
    ) in {"", "none"}
    unsupported = mint_in not in CORE_INPUT_MINTS
    quote_missing = quote_expected_out_raw <= 0 or min_out_raw <= 0
    cap_required_too_high = required_cap > cap_too_high_sol

    helius_sender_dual_no_send_possible = bool(helius_sender_url) and tip_accounts_count > 0 and template_ready
    helius_sender_dual_no_send_basis = (
        "configuration_tip_accounts_and_template_ready"
        if helius_sender_dual_no_send_possible
        else "missing_sender_url_or_tip_accounts_or_template"
    )
    private_route_ev_after_fee_tip = autopsy.get("private_route_ev_after_fee_tip")
    private_route_ev_proven = truth(autopsy.get("private_route_ev_after_fee_tip_proven"))

    if unsupported:
        decision = "OUT_OF_SCOPE"
    elif quote_missing:
        decision = "QUOTE_DIVERGED"
    elif not net_ev_positive:
        decision = "NEGATIVE_AT_ORIGINAL_SIZE"
    elif not pm_reserve_ok_at_original_size:
        decision = "PM_FAIL_AT_ORIGINAL_SIZE"
    elif not template_ready or not template_pm_account_match or not existing_input_ready:
        decision = "TEMPLATE_OR_ACCOUNT_FAIL"
    elif cap_required_too_high:
        decision = "CAP_REQUIRED_TOO_HIGH"
    else:
        decision = "READY_IF_CAP_APPROVED"

    full_acceptance_ready = (
        decision == "READY_IF_CAP_APPROVED"
        and helius_sender_dual_no_send_possible
        and min_out_positive
        and net_ev_positive
        and pm_reserve_ok_at_original_size
        and template_pm_account_match
    )

    row_out = {
        "candidate_id": f"run78c_cap_original_{index}",
        "signature": str(split_row.get("signature") or autopsy.get("signature") or ""),
        "pool": str(split_row.get("pool") or autopsy.get("pool") or identity_pool or ""),
        "venue": str(autopsy.get("venue") or raw.get("venue") or raw.get("venue_name") or ""),
        "mint_in": mint_in,
        "mint_out": mint_out,
        "source": str(raw.get("source") or split_row.get("source") or "afxdp"),
        "route_family": str(autopsy.get("route_family") or split_row.get("route") or raw.get("route") or ""),
        "original_input_raw": original_input_raw,
        "original_input_native": dec_to_str(original_input_native),
        "original_input_sol_equivalent": dec_to_str(original_input_sol),
        "original_input_usd_equivalent": dec_to_str(original_input_usd),
        "current_max_live_trade_input_sol": dec_to_str(current_cap),
        "current_max_notional_sol_per_entity": dec_to_str(current_cap),
        "current_max_position_sol_per_mint": dec_to_str(current_cap),
        "current_cap_basis": "run78_selector_approved_live_cap_sol",
        "required_max_live_trade_input_sol": dec_to_str(required_cap),
        "required_max_notional_sol_per_entity": dec_to_str(required_cap),
        "required_max_position_sol_per_mint": dec_to_str(required_cap),
        "required_wsol_balance_raw": original_input_raw if mint_in == WSOL_MINT else 0,
        "required_fee_balance_raw": base_fee,
        "required_tip_balance_raw": tip,
        "quote_expected_out_raw": quote_expected_out_raw,
        "min_out_raw": min_out_raw,
        "min_out_positive": min_out_positive,
        "route_specific_fee_tip": base_fee + tip,
        "net_ev_after_fee_tip": net_ev_after_fee_tip,
        "net_ev_positive": net_ev_positive,
        "pm_reserve_ok_at_original_size": pm_reserve_ok_at_original_size,
        "template_ready": template_ready,
        "template_pm_account_match": template_pm_account_match,
        "helius_sender_dual_no_send_possible": helius_sender_dual_no_send_possible,
        "helius_sender_dual_no_send_basis": helius_sender_dual_no_send_basis,
        "helius_sender_url_present": bool(helius_sender_url),
        "helius_sender_tip_accounts_count": tip_accounts_count,
        "helius_sender_tip_accounts_source": sender_cfg["tip_accounts_source"] or config_sender_cfg["tip_accounts_source"],
        "private_route_ev_after_fee_tip": private_route_ev_after_fee_tip,
        "private_route_ev_after_fee_tip_proven": private_route_ev_proven,
        "private_route_ev_proof_status": str(autopsy.get("private_route_ev_proof_status") or ""),
        "quote_refresh_status": "ORIGINAL_EPOCH_QUOTE_REUSED_NO_EXTERNAL_REQUOTE_ADAPTER",
        "quote_basis": "run78a_original_size_epoch_quote",
        "decision": decision,
        "full_acceptance_ready": full_acceptance_ready,
    }
    out_rows.append(row_out)

fieldnames = [
    "candidate_id",
    "signature",
    "pool",
    "venue",
    "mint_in",
    "mint_out",
    "source",
    "route_family",
    "original_input_raw",
    "original_input_native",
    "original_input_sol_equivalent",
    "original_input_usd_equivalent",
    "current_max_live_trade_input_sol",
    "current_max_notional_sol_per_entity",
    "current_max_position_sol_per_mint",
    "current_cap_basis",
    "required_max_live_trade_input_sol",
    "required_max_notional_sol_per_entity",
    "required_max_position_sol_per_mint",
    "required_wsol_balance_raw",
    "required_fee_balance_raw",
    "required_tip_balance_raw",
    "quote_expected_out_raw",
    "min_out_raw",
    "min_out_positive",
    "route_specific_fee_tip",
    "net_ev_after_fee_tip",
    "net_ev_positive",
    "pm_reserve_ok_at_original_size",
    "template_ready",
    "template_pm_account_match",
    "helius_sender_dual_no_send_possible",
    "helius_sender_dual_no_send_basis",
    "helius_sender_url_present",
    "helius_sender_tip_accounts_count",
    "private_route_ev_after_fee_tip",
    "private_route_ev_after_fee_tip_proven",
    "private_route_ev_proof_status",
    "quote_refresh_status",
    "quote_basis",
    "decision",
    "full_acceptance_ready",
]

with rows_jsonl_path.open("w", encoding="utf-8") as handle:
    for row in out_rows:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")

with rows_csv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(out_rows)

required_caps = [dec_or_none(row["required_max_live_trade_input_sol"]) for row in out_rows]
required_caps = [item for item in required_caps if item is not None]
required_caps_sorted = sorted(required_caps)


def percentile(values: list[Decimal], frac: Decimal) -> Decimal | None:
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    pos = frac * Decimal(len(values) - 1)
    lo = int(pos)
    hi = min(lo + 1, len(values) - 1)
    weight = pos - Decimal(lo)
    return values[lo] * (Decimal(1) - weight) + values[hi] * weight


top_rows = sorted(
    out_rows,
    key=lambda row: (
        dec_or_none(row["required_max_live_trade_input_sol"]) or Decimal(0),
        safe_int(row["net_ev_after_fee_tip"]),
    ),
    reverse=True,
)[:7]
top_rows_compact = [
    {
        "candidate_id": row["candidate_id"],
        "signature_prefix": str(row["signature"])[:12],
        "pool": row["pool"],
        "mint_out": row["mint_out"],
        "required_cap_sol": row["required_max_live_trade_input_sol"],
        "net_ev_after_fee_tip": row["net_ev_after_fee_tip"],
        "decision": row["decision"],
        "helius_sender_dual_no_send_possible": row["helius_sender_dual_no_send_possible"],
        "private_route_ev_proof_status": row["private_route_ev_proof_status"],
    }
    for row in top_rows
]

decision_counts = Counter(row["decision"] for row in out_rows)
quote_refresh_counts = Counter(row["quote_refresh_status"] for row in out_rows)
summary = {
    "status": "run78c_complete" if out_rows else "blocked_no_cap_blocked_ev_positive_aligned_rows",
    "source_dir": str(source_dir),
    "output_dir": str(output_dir),
    "source_split_rows_path": str(split_rows_path),
    "run78a_rows_path": str(run78a_rows_path),
    "raw_selector_rows_path": str(raw_selector_rows_path),
    "source_cap_blocked_candidate_count": len(cap_rows),
    "ready_if_cap_approved_count": sum(1 for row in out_rows if row["decision"] == "READY_IF_CAP_APPROVED"),
    "full_acceptance_ready_count": sum(1 for row in out_rows if row["full_acceptance_ready"]),
    "required_cap_min_sol": dec_to_str(required_caps_sorted[0] if required_caps_sorted else None),
    "required_cap_median_sol": dec_to_str(percentile(required_caps_sorted, Decimal("0.5"))),
    "required_cap_max_sol": dec_to_str(required_caps_sorted[-1] if required_caps_sorted else None),
    "net_ev_positive_at_original_size_count": sum(1 for row in out_rows if row["net_ev_positive"]),
    "pm_reserve_ok_at_original_size_count": sum(1 for row in out_rows if row["pm_reserve_ok_at_original_size"]),
    "helius_sender_dual_no_send_possible_count": sum(
        1 for row in out_rows if row["helius_sender_dual_no_send_possible"]
    ),
    "template_pm_account_match_count": sum(1 for row in out_rows if row["template_pm_account_match"]),
    "min_out_positive_count": sum(1 for row in out_rows if row["min_out_positive"]),
    "decision_counts": dict(decision_counts),
    "quote_refresh_status_counts": dict(quote_refresh_counts),
    "quote_refresh_note": "No external DirectArb requote adapter is available in this script layer; rows preserve the original epoch quote at the original input size.",
    "cap_too_high_sol": dec_to_str(cap_too_high_sol),
    "helius_sender_url_present": bool(helius_sender_url),
    "helius_sender_tip_accounts_count": tip_accounts_count,
    "malformed_split_rows": split_bad,
    "malformed_run78a_rows": run78a_bad,
    "malformed_raw_selector_rows": raw_bad,
    "top_rows_by_required_cap_and_ev": top_rows_compact,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}

summary_lines = [
    f"status={summary['status']}",
    f"source_cap_blocked_candidate_count={summary['source_cap_blocked_candidate_count']}",
    f"ready_if_cap_approved_count={summary['ready_if_cap_approved_count']}",
    f"full_acceptance_ready_count={summary['full_acceptance_ready_count']}",
    f"required_cap_min_sol={summary['required_cap_min_sol']}",
    f"required_cap_median_sol={summary['required_cap_median_sol']}",
    f"required_cap_max_sol={summary['required_cap_max_sol']}",
    f"net_ev_positive_at_original_size_count={summary['net_ev_positive_at_original_size_count']}",
    f"pm_reserve_ok_at_original_size_count={summary['pm_reserve_ok_at_original_size_count']}",
    f"helius_sender_dual_no_send_possible_count={summary['helius_sender_dual_no_send_possible_count']}",
    f"template_pm_account_match_count={summary['template_pm_account_match_count']}",
    f"min_out_positive_count={summary['min_out_positive_count']}",
    "decision_counts=" + json.dumps(summary["decision_counts"], separators=(",", ":"), sort_keys=True),
    "quote_refresh_status_counts=" + json.dumps(summary["quote_refresh_status_counts"], separators=(",", ":"), sort_keys=True),
    f"quote_refresh_note={summary['quote_refresh_note']}",
    f"cap_too_high_sol={summary['cap_too_high_sol']}",
    f"helius_sender_url_present={str(summary['helius_sender_url_present']).lower()}",
    f"helius_sender_tip_accounts_count={summary['helius_sender_tip_accounts_count']}",
    "top_rows_by_required_cap_and_ev="
    + json.dumps(summary["top_rows_by_required_cap_and_ev"], separators=(",", ":"), sort_keys=True),
    f"rows_jsonl={summary['rows_jsonl']}",
    f"rows_csv={summary['rows_csv']}",
    f"report_json={summary_json_path}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run78C required cap report complete"
finish

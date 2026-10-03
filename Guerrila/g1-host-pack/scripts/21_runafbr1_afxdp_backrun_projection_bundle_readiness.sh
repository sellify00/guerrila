#!/usr/bin/env bash
# 21_runafbr1_afxdp_backrun_projection_bundle_readiness.sh — AF_XDP-only Backrun projection proof

source "$(dirname "$0")/lib.sh"
script_header "21 — RunAFBR1 AF_XDP Backrun Projection Bundle Readiness"

assert_root
require_env SHADOW_PACK_DIR

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINDOW_SECONDS="${1:-${SHADOW_RUNAFBR1_WINDOW_SECONDS:-60}}"
STEP="21_runafbr1_afxdp_backrun_projection_bundle_readiness"
OUTER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
WINDOW_RESULT_DIR="$OUTER_RESULT_DIR/window1"
INNER_STEP_DIR="$WINDOW_RESULT_DIR/20_g12_r0_observe_only"
INNER_SUMMARY_FILE="$INNER_STEP_DIR/summary.txt"
PROBE_ROWS_PATH="$INNER_STEP_DIR/runafbr1_bundle_probe_rows.jsonl"
HOT_PROBE_ROWS_PATH="${SHADOW_RUNAFBR1_HOT_PROBE_ROWS_PATH:-/dev/shm/runafbr1_hot_bundle_probe_rows_$$.jsonl}"
STOP_FILE="$INNER_STEP_DIR/runafbr1_stop_after_fresh_candidate"
SUMMARY_JSON_PATH="$INNER_STEP_DIR/runafbr1_summary.json"
REJECTION_ROWS_PATH="$INNER_STEP_DIR/runafbr1_candidate_rejections.jsonl"
AUTOPSY_ROWS_PATH="$INNER_STEP_DIR/runafbr1_projection_autopsy_rows.jsonl"
SELECTED_JSON_PATH="$INNER_STEP_DIR/runafbr1_selected_backrun_bundle.json"
PAYLOAD_JSON_PATH="$INNER_STEP_DIR/runafbr1_bundle_payload_base64.json"
PM_REPORT_JSON_PATH="$INNER_STEP_DIR/runafbr1_pm_reserve_report.json"
FRESHNESS_JSON_PATH="$INNER_STEP_DIR/runafbr1_target_freshness_bundle_eligibility.json"
OUTER_SUMMARY_FILE="$OUTER_RESULT_DIR/summary.txt"
PI_SUBREASON_ROWS_PATH="$INNER_STEP_DIR/runls5c_process_intent_subreason_rows.jsonl"

rm -rf "$OUTER_RESULT_DIR"
rm -f "$HOT_PROBE_ROWS_PATH"
mkdir -p "$INNER_STEP_DIR"

info "Running bounded AF_XDP-only Backrun projection proof with inline no-submit freshness gate"
export SHADOW_RESULTS_DIR="$WINDOW_RESULT_DIR"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-laserstream}"
export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP:-true}"
export SHADOW_POST_WINDOW_DRAIN_SECONDS="${SHADOW_POST_WINDOW_DRAIN_SECONDS:-0}"
export SHADOW_G12_R0_POLL_SECONDS="${SHADOW_G12_R0_POLL_SECONDS:-1}"
export SHADOW_G12_R0_STOP_FILE="$STOP_FILE"
export SHADOW_RUNLS11_EXPORT_PATH="$PROBE_ROWS_PATH"
export SHADOW_RUNLS11_HOT_PROBE_PATH="$HOT_PROBE_ROWS_PATH"
export SHADOW_RUNLS11_SIGNAL_SOURCE="afxdp"
unset SHADOW_RUNLS11_ROUTE_FAMILY SHADOW_RUNLS11_POOL_PUBKEY SHADOW_RUNLS11_MINT_IN SHADOW_RUNLS11_MINT_OUT

python3 - "$PACK_DIR" "$INNER_STEP_DIR" "$PROBE_ROWS_PATH" "$STOP_FILE" "$SUMMARY_JSON_PATH" \
    "$REJECTION_ROWS_PATH" "$AUTOPSY_ROWS_PATH" "$SELECTED_JSON_PATH" "$PAYLOAD_JSON_PATH" "$PM_REPORT_JSON_PATH" \
    "$FRESHNESS_JSON_PATH" "$WINDOW_SECONDS" "$HOT_PROBE_ROWS_PATH" <<'PY' &
from __future__ import annotations

import base64
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

pack_dir = Path(sys.argv[1])
inner_dir = Path(sys.argv[2])
probe_rows_path = Path(sys.argv[3])
stop_file = Path(sys.argv[4])
summary_path = Path(sys.argv[5])
rejection_rows_path = Path(sys.argv[6])
autopsy_rows_path = Path(sys.argv[7])
selected_path = Path(sys.argv[8])
payload_path = Path(sys.argv[9])
pm_report_path = Path(sys.argv[10])
freshness_path = Path(sys.argv[11])
window_seconds = int(sys.argv[12])
hot_probe_rows_path = Path(sys.argv[13])

bundle_routes = {"jito_bundle", "harmonic_bundle"}
ls11e_script = pack_dir / "scripts" / "21_runls11e_target_freshness_bundle_eligibility.sh"
shock_research_path = inner_dir / "shock_research.jsonl"
source_arbiter_path = inner_dir / "laserstream_vs_afxdp_first_seen.jsonl"
pi_subreason_rows_path = inner_dir / "runls5c_process_intent_subreason_rows.jsonl"
start = time.monotonic()
deadline = start + window_seconds + 3
monitor_poll_seconds = float(os.environ.get("SHADOW_RUNAFBR1_MONITOR_POLL_SECONDS", "0.25"))
stop_after_first_strict_candidate = os.environ.get(
    "SHADOW_RUNAFBR1_STOP_AFTER_FIRST_STRICT_CANDIDATE",
    "false",
).strip().lower() in {"1", "true", "yes", "on"}
inline_bundle_ready_probe = os.environ.get(
    "SHADOW_RUNAFBR1_INLINE_BUNDLE_READY_PROBE",
    "false",
).strip().lower() in {"1", "true", "yes", "on"}
seen_keys: set[str] = set()
status_counts: dict[str, int] = {}
target_seen = 0
strict_seen = 0
shock_summary: dict[str, Any] | None = None
autopsy_keys: set[str] = set()
autopsy_blocker_counts: dict[str, int] = {}
autopsy_rows_path.write_text("", encoding="utf-8")
shock_rows_cache: list[dict[str, Any]] = []
shock_rows_cache_mtime_ns: int | None = None
source_arbiter_rows_cache: list[dict[str, Any]] = []
source_arbiter_rows_cache_mtime_ns: int | None = None

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def is_placeholder(value: str | None) -> bool:
    return not value or "CHANGE_ME" in value or "${" in value


def add_api_key(url: str, api_key: str) -> str:
    if is_placeholder(url) or not api_key:
        return url
    parsed = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    if not any(key.lower() == "api-key" for key, _ in query):
        query.append(("api-key", api_key))
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, urllib.parse.urlencode(query), parsed.fragment)
    )


def b58encode(data: bytes) -> str:
    num = int.from_bytes(data, "big")
    encoded = ""
    while num:
        num, rem = divmod(num, 58)
        encoded = BASE58_ALPHABET[rem] + encoded
    pad = 0
    for byte in data:
        if byte == 0:
            pad += 1
        else:
            break
    return BASE58_ALPHABET[0] * pad + (encoded or BASE58_ALPHABET[0])


def hex_signature_to_b58(value: str) -> str:
    value = value.strip()
    try:
        if len(value) == 128 and all(ch in "0123456789abcdefABCDEF" for ch in value):
            return b58encode(bytes.fromhex(value))
    except ValueError:
        pass
    return value


def rpc_call(url: str, method: str, params: Any) -> dict[str, Any]:
    if is_placeholder(url):
        return {"reachable": False, "probe": "placeholder", "method_available": False, "result": None}
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(
        url,
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as response:
            text = response.read().decode("utf-8", errors="replace")
            status = response.status
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8", errors="replace")
        status = exc.code
    except Exception as exc:  # noqa: BLE001
        return {
            "reachable": False,
            "probe": str(exc),
            "method_available": False,
            "result": None,
        }
    try:
        payload = json.loads(text) if text else {}
    except json.JSONDecodeError:
        payload = {"raw": text[:240]}
    err = payload.get("error") if isinstance(payload, dict) else None
    err_msg = str(err.get("message", "")) if isinstance(err, dict) else ""
    method_missing = isinstance(err, dict) and (
        err.get("code") == -32601 or "method not found" in err_msg.lower()
    )
    auth_failed = status in {401, 403} or "api key" in err_msg.lower() or "unauthorized" in err_msg.lower()
    return {
        "reachable": status < 500 and not auth_failed,
        "probe": f"http_{status}",
        "method_available": not method_missing and not auth_failed,
        "auth_failed": auth_failed,
        "error_code": err.get("code") if isinstance(err, dict) else None,
        "error_message": err_msg[:240] if err_msg else None,
        "result": payload.get("result") if isinstance(payload, dict) else None,
    }


def inline_bundle_ready_status_probe(row: dict[str, Any]) -> dict[str, Any]:
    target_signature = hex_signature_to_b58(str(row.get("target_signature_hex") or ""))
    endpoint = add_api_key(
        os.environ.get("SHADOW_HELIUS_BUNDLE_RPC_URL")
        or os.environ.get("SHADOW_SOLANA_RPC_URL")
        or os.environ.get("SHADOW_RPC_URL")
        or os.environ.get("SHADOW_PUBLIC_RPC_URL")
        or "",
        os.environ.get("SHADOW_HELIUS_BUNDLE_API_KEY", ""),
    )
    start_ns = time.monotonic_ns() if target_signature else None
    signature_probe = rpc_call(
        endpoint,
        "getSignatureStatuses",
        [[target_signature], {"searchTransactionHistory": True}],
    ) if target_signature else {"reachable": False, "probe": "missing_target_signature", "result": None}
    local_slot_probe = rpc_call(
        os.environ.get("SHADOW_LOCAL_AGAVE_RPC_URL", "http://127.0.0.1:8899"),
        "getSlot",
        [{"commitment": "processed"}],
    ) if target_signature else {"reachable": False, "probe": "missing_target_signature", "result": None}
    end_ns = time.monotonic_ns() if target_signature else None
    status_value = None
    try:
        status_value = signature_probe.get("result", {}).get("value", [None])[0]
    except Exception:  # noqa: BLE001
        status_value = None
    local_slot = None
    try:
        local_slot = local_slot_probe.get("result")
    except Exception:  # noqa: BLE001
        local_slot = None
    monitor_seen_ns = as_int(row.get("monitor_seen_ns") or row.get("_monitor_seen_ns"))
    bundle_ready_ns = as_int(row.get("current_trade_ready_ns")) or as_int(row.get("current_candidate_emit_ns"))
    bundle_probe_export_ns = as_int(row.get("bundle_probe_export_ns"))
    bundle_ready_to_status_start_us = None
    if start_ns is not None and bundle_ready_ns > 0 and start_ns >= bundle_ready_ns:
        bundle_ready_to_status_start_us = (start_ns - bundle_ready_ns) // 1_000
    bundle_probe_export_to_status_start_us = None
    if start_ns is not None and bundle_probe_export_ns > 0 and start_ns >= bundle_probe_export_ns:
        bundle_probe_export_to_status_start_us = (start_ns - bundle_probe_export_ns) // 1_000
    bundle_probe_export_to_monitor_seen_us = None
    if monitor_seen_ns > 0 and bundle_probe_export_ns > 0 and monitor_seen_ns >= bundle_probe_export_ns:
        bundle_probe_export_to_monitor_seen_us = (monitor_seen_ns - bundle_probe_export_ns) // 1_000
    monitor_seen_to_status_start_us = None
    if start_ns is not None and monitor_seen_ns > 0 and start_ns >= monitor_seen_ns:
        monitor_seen_to_status_start_us = (start_ns - monitor_seen_ns) // 1_000
    return {
        "inline_bundle_ready_probe": True,
        "target_signature_status_before_bundle": status_value,
        "target_signature_status_after_bundle_ready": status_value,
        "target_signature_status_probe": signature_probe.get("probe"),
        "target_signature_status_reachable": bool(signature_probe.get("reachable")),
        "first_status_check_start_ns": start_ns,
        "first_status_check_end_ns": end_ns,
        "first_status_check_latency_us": ((end_ns - start_ns) // 1_000) if start_ns is not None and end_ns is not None else None,
        "monitor_seen_ns": monitor_seen_ns or None,
        "bundle_ready_to_status_start_us": bundle_ready_to_status_start_us,
        "bundle_probe_export_ns": bundle_probe_export_ns or None,
        "bundle_probe_export_to_status_start_us": bundle_probe_export_to_status_start_us,
        "bundle_probe_export_to_monitor_seen_us": bundle_probe_export_to_monitor_seen_us,
        "monitor_seen_to_status_start_us": monitor_seen_to_status_start_us,
        "local_current_slot_at_first_check": local_slot,
        "local_current_slot_probe": local_slot_probe.get("probe"),
        "local_current_slot_reachable": bool(local_slot_probe.get("reachable")),
        "target_already_landed": status_value is not None,
    }


def as_int(value: Any, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def decodes_base64(value: Any) -> tuple[bool, int]:
    if not isinstance(value, str) or not value:
        return False, 0
    try:
        data = base64.b64decode(value, validate=True)
    except Exception:  # noqa: BLE001
        return False, 0
    return bool(data), len(data)


def load_probe_rows() -> list[dict[str, Any]]:
    if not probe_rows_path.exists():
        return []
    rows: list[dict[str, Any]] = []
    try:
        lines = probe_rows_path.read_text(encoding="utf-8", errors="replace").splitlines()
    except FileNotFoundError:
        return rows
    for raw in lines:
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("record_type") == "runls11_bundle_probe":
            if not (inline_bundle_ready_probe and probe_row_self_contained(row)):
                row = enrich_probe_row_from_shock_research(row)
                row = enrich_probe_row_from_source_arbiter(row)
            rows.append(row)
    return rows


def load_hot_probe_rows() -> list[dict[str, Any]]:
    if not hot_probe_rows_path.exists():
        return []
    rows: list[dict[str, Any]] = []
    try:
        lines = hot_probe_rows_path.read_text(encoding="utf-8", errors="replace").splitlines()
    except FileNotFoundError:
        return rows
    for raw in lines:
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("record_type") == "runls11_hot_bundle_probe":
            rows.append(row)
    return rows


def load_shock_research_rows() -> list[dict[str, Any]]:
    global shock_rows_cache, shock_rows_cache_mtime_ns
    if not shock_research_path.exists():
        shock_rows_cache = []
        shock_rows_cache_mtime_ns = None
        return []
    try:
        stat = shock_research_path.stat()
    except FileNotFoundError:
        shock_rows_cache = []
        shock_rows_cache_mtime_ns = None
        return []
    if shock_rows_cache_mtime_ns == stat.st_mtime_ns:
        return shock_rows_cache
    rows: list[dict[str, Any]] = []
    for raw in shock_research_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    shock_rows_cache = rows
    shock_rows_cache_mtime_ns = stat.st_mtime_ns
    return shock_rows_cache


def load_source_arbiter_rows() -> list[dict[str, Any]]:
    global source_arbiter_rows_cache, source_arbiter_rows_cache_mtime_ns
    if not source_arbiter_path.exists():
        source_arbiter_rows_cache = []
        source_arbiter_rows_cache_mtime_ns = None
        return []
    try:
        stat = source_arbiter_path.stat()
    except FileNotFoundError:
        source_arbiter_rows_cache = []
        source_arbiter_rows_cache_mtime_ns = None
        return []
    if source_arbiter_rows_cache_mtime_ns == stat.st_mtime_ns:
        return source_arbiter_rows_cache
    rows: list[dict[str, Any]] = []
    for raw in source_arbiter_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("record_type") == "source_arbiter":
            rows.append(row)
    source_arbiter_rows_cache = rows
    source_arbiter_rows_cache_mtime_ns = stat.st_mtime_ns
    return source_arbiter_rows_cache


def hex_signature_to_base58(signature_hex: str | None) -> str | None:
    if not signature_hex:
        return None
    try:
        data = bytes.fromhex(signature_hex)
    except ValueError:
        return None
    if not data:
        return None
    value = int.from_bytes(data, "big")
    encoded = ""
    while value:
        value, remainder = divmod(value, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    leading_zeroes = 0
    for byte in data:
        if byte == 0:
            leading_zeroes += 1
        else:
            break
    return ("1" * leading_zeroes) + (encoded or "1")


def merge_prefer_probe_fields(shock_row: dict[str, Any], probe_row: dict[str, Any]) -> dict[str, Any]:
    merged = dict(shock_row)
    for key, value in probe_row.items():
        if value not in (None, ""):
            merged[key] = value
    return merged


def timing_field_count(row: dict[str, Any]) -> int:
    return sum(
        1
        for key in (
            "timestamp_basis",
            "afxdp_seen_ns",
            "first_seen_ns",
            "current_normalized_record_ns",
            "current_worker_seen_ns",
            "current_route_ready_ns",
            "current_quote_ready_ns",
            "current_pm_ready_ns",
            "current_candidate_emit_ns",
            "current_trade_ready_ns",
            "trade_ready_delta_us",
        )
        if row.get(key) not in (None, "")
    )


def source_preference(row: dict[str, Any]) -> int:
    source = text_lower(first_value(row, "signal_source", "source_target", "source"))
    if source == "afxdp":
        return 2
    if source == "laserstream":
        return 1
    return 0


def enrich_probe_row_from_shock_research(probe_row: dict[str, Any]) -> dict[str, Any]:
    our_sig = first_text(probe_row, "our_signature_hex", "backrun_signature", "signature_hex")
    target_sig = first_text(probe_row, "target_signature_hex", "target_signature")
    pool = first_text(probe_row, "pool_pubkey_hex", "pool")
    target_pool = first_text(probe_row, "target_pool_pubkey_hex", "target_pool")
    amount_in = as_int(first_value(probe_row, "target_amount_in", "target_amount_in_raw"))
    best_match: dict[str, Any] | None = None
    best_score: tuple[int, int, int] | None = None
    for row in load_shock_research_rows():
        if not is_backrun_row(row) or not is_bundle_row(row):
            continue
        score = 0
        row_our_sig = first_text(row, "our_signature_hex", "backrun_signature", "signature_hex")
        row_target_sig = first_text(row, "target_signature_hex", "target_signature", "signature_hex")
        row_amount_in = as_int(first_value(row, "target_amount_in", "target_amount_in_raw"))
        if our_sig and row_our_sig == our_sig:
            score = max(score, 3)
        if target_sig and row_target_sig == target_sig:
            if amount_in <= 0 or row_amount_in <= 0 or row_amount_in == amount_in:
                score = max(score, 2)
        if pool and target_pool:
            if first_text(row, "pool_pubkey_hex", "pool") == pool and first_text(row, "target_pool_pubkey_hex", "target_pool", "pool_pubkey_hex", "pool") == target_pool:
                if amount_in <= 0 or row_amount_in <= 0 or row_amount_in == amount_in:
                    score = max(score, 1)
        if score <= 0:
            continue
        row_score = (score, source_preference(row), timing_field_count(row))
        if best_score is None or row_score > best_score:
            best_match = row
            best_score = row_score
    if best_match is None:
        return probe_row
    return merge_prefer_probe_fields(best_match, probe_row)


def enrich_probe_row_from_source_arbiter(probe_row: dict[str, Any]) -> dict[str, Any]:
    our_sig_b58 = hex_signature_to_base58(first_text(probe_row, "our_signature_hex", "backrun_signature", "signature_hex"))
    target_sig_b58 = hex_signature_to_base58(first_text(probe_row, "target_signature_hex", "target_signature"))
    best_match: dict[str, Any] | None = None
    best_score: tuple[int, int] | None = None
    for row in load_source_arbiter_rows():
        if text_lower(row.get("owner_mode")) != "backrun":
            continue
        if text_lower(first_value(row, "route", "route_family_label")) not in bundle_routes:
            continue
        signature_b58 = first_text(row, "signature")
        if not signature_b58:
            continue
        score = 0
        if our_sig_b58 and signature_b58 == our_sig_b58:
            score = max(score, 2)
        if target_sig_b58 and signature_b58 == target_sig_b58:
            score = max(score, 1)
        if score <= 0:
            continue
        row_score = (score, timing_field_count(row))
        if best_score is None or row_score > best_score:
            best_match = row
            best_score = row_score
    if best_match is None:
        return probe_row
    return merge_prefer_probe_fields(best_match, probe_row)


def add_count(status: str) -> None:
    status_counts[status] = status_counts.get(status, 0) + 1


def text_lower(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip().lower()


def bool_value(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return text_lower(value) in {"1", "true", "yes", "y", "ok"}


def first_value(row: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in row and row.get(key) is not None:
            return row.get(key)
    return None


def first_int(row: dict[str, Any], *keys: str) -> int:
    return as_int(first_value(row, *keys))


def first_text(row: dict[str, Any], *keys: str) -> str | None:
    value = first_value(row, *keys)
    if value is None:
        return None
    text = str(value)
    return text if text else None


def probe_key(row: dict[str, Any]) -> str:
    return str(
        row.get("bundle_probe_export_ns")
        or row.get("our_signature_hex")
        or row.get("bundle_payload_sha256_hex")
        or row.get("target_signature_hex")
        or json.dumps(row, sort_keys=True)[:256]
    )


def append_rejection(row: dict[str, Any], status: str, extra: dict[str, Any] | None = None) -> None:
    add_count(status)
    payload = {
        "record_type": "runafbr1_candidate_rejection",
        "status": status,
        "target_signature": row.get("target_signature_hex"),
        "backrun_signature": row.get("our_signature_hex"),
        "slot": row.get("slot"),
        "source_target": "afxdp",
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    if extra:
        payload.update(extra)
    with rejection_rows_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(payload, sort_keys=True) + "\n")


def append_summary_rejection(status: str, extra: dict[str, Any]) -> None:
    add_count(status)
    payload = {
        "record_type": "runafbr1_candidate_rejection_summary",
        "status": status,
        "source_target": "afxdp",
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload.update(extra)
    with rejection_rows_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(payload, sort_keys=True) + "\n")


def is_afxdp_row(row: dict[str, Any]) -> bool:
    return text_lower(row.get("signal_source")) == "afxdp" or as_int(row.get("signal_source_id")) == 4


def is_backrun_row(row: dict[str, Any]) -> bool:
    return (
        text_lower(row.get("owner_mode")) == "backrun"
        or text_lower(row.get("selected_owner")) == "backrun"
        or as_int(row.get("owner_mode_id")) == 1
        or as_int(row.get("selected_owner_id")) == 1
    )


def is_bundle_row(row: dict[str, Any]) -> bool:
    return text_lower(row.get("route_family_label")) in bundle_routes or text_lower(row.get("route_family")) in {"0", "jito_bundle", "harmonic_bundle"}


def is_zeroed_telemetry_row(row: dict[str, Any]) -> bool:
    return (
        row.get("record_type") == "rollout_shadow"
        and not bool_value(row.get("would_build"))
        and not bool_value(row.get("would_sign"))
        and not bool_value(row.get("would_send"))
        and first_int(row, "trade_size_lamports") == 0
        and first_int(row, "route_base_cu_limit") == 0
        and first_int(row, "quote_request_id", "ingress_quote_request_id") == 0
        and first_int(row, "quote_expected_out_lamports", "expected_out_raw") == 0
        and first_int(row, "reserved_notional_lamports") == 0
    )


def target_tx_bytes_present(row: dict[str, Any]) -> bool:
    if bool_value(row.get("target_tx_bytes_available")):
        return True
    ok, _ = decodes_base64(row.get("target_tx_base64"))
    return ok


def probe_row_self_contained(row: dict[str, Any]) -> bool:
    return (
        target_tx_bytes_present(row)
        and first_int(row, "target_entity_id") > 0
        and first_int(row, "quote_baseline_reserve_in") > 0
        and first_int(row, "quote_baseline_reserve_out") > 0
        and first_int(row, "quote_projected_reserve_in") > 0
        and first_int(row, "quote_projected_reserve_out") > 0
        and first_int(row, "expected_out_raw", "quote_expected_out_lamports") > 0
        and first_int(row, "min_out_raw", "min_out_lamports") > 0
        and first_int(row, "net_ev_after_tip_lamports", "retained_ev_lamports_lcb") > 0
        and first_int(row, "current_trade_ready_ns", "current_candidate_emit_ns") > 0
        and first_int(row, "bundle_probe_export_ns") > 0
    )


def hot_inline_candidate(row: dict[str, Any]) -> bool:
    hot_row = row.get("record_type") == "runls11_hot_bundle_probe"
    target_payload_ok = bool(row.get("target_tx_base64")) or (
        hot_row and bool_value(row.get("target_tx_bytes_available"))
    )
    our_payload_ok = bool(row.get("bundle_payload_base64")) or hot_row
    return (
        inline_bundle_ready_probe
        and is_afxdp_row(row)
        and row.get("target_signal_source") == "afxdp"
        and as_int(row.get("target_signal_source_id")) == 4
        and bool_value(row.get("target_tx_bytes_available"))
        and target_payload_ok
        and our_payload_ok
        and bool_value(row.get("target_relationship_recorded"))
        and row.get("target_pool_pubkey_hex") == row.get("pool_pubkey_hex")
        and row.get("target_mint_in_hex") == row.get("mint_in_hex")
        and row.get("target_mint_out_hex") == row.get("mint_out_hex")
        and as_int(row.get("target_amount_in")) > 0
        and as_int(row.get("trade_size_lamports")) > 0
        and as_int(row.get("expected_out_raw")) > 0
        and as_int(row.get("min_out_raw")) > 0
        and as_int(row.get("net_ev_after_tip_lamports")) > 0
        and text_lower(row.get("route_family")) in bundle_routes
        and as_int(row.get("tip_lamports")) > 0
        and bool_value(row.get("pm_reserve_ok"))
        and as_int(row.get("reserved_notional_lamports")) > 0
        and as_int(row.get("current_trade_ready_ns") or row.get("current_candidate_emit_ns")) > 0
        and as_int(row.get("bundle_probe_export_ns")) > 0
    )


def probe_identity_matches(row: dict[str, Any], hot_row: dict[str, Any]) -> bool:
    row_export_ns = as_int(row.get("bundle_probe_export_ns"))
    hot_export_ns = as_int(hot_row.get("bundle_probe_export_ns"))
    if row_export_ns > 0 and hot_export_ns > 0:
        return row_export_ns == hot_export_ns
    return first_text(row, "target_signature_hex") == first_text(hot_row, "target_signature_hex")


def wait_for_matching_full_probe_row(hot_row: dict[str, Any]) -> dict[str, Any] | None:
    deadline_ns = time.monotonic_ns() + int(500_000_000)
    while True:
        for row in load_probe_rows():
            if probe_identity_matches(row, hot_row):
                return row
        if time.monotonic_ns() >= deadline_ns:
            return None
        time.sleep(0.001)


def projection_autopsy(row: dict[str, Any]) -> dict[str, Any]:
    target_pool = first_text(row, "target_pool_pubkey_hex", "target_pool")
    pool = first_text(row, "pool_pubkey_hex", "pool")
    target_mint_in = first_text(row, "target_mint_in_hex", "target_mint_in")
    mint_in = first_text(row, "mint_in_hex", "mint_in")
    target_mint_out = first_text(row, "target_mint_out_hex", "target_mint_out")
    mint_out = first_text(row, "mint_out_hex", "mint_out")

    expected_out_raw = first_int(row, "expected_out_raw", "quote_expected_out_lamports")
    min_out_raw = first_int(row, "min_out_raw", "min_out_lamports")
    tip_lamports = first_int(row, "tip_lamports")
    net_ev_after_tip = first_int(
        row,
        "net_ev_after_tip_lamports",
        "retained_ev_lamports_lcb",
        "t0_retained_ev_lamports_lcb",
        "net_markout_after_fee_lamports",
    )
    baseline_in = first_int(row, "quote_baseline_reserve_in", "baseline_reserve_in", "pool_reserve_in")
    baseline_out = first_int(row, "quote_baseline_reserve_out", "baseline_reserve_out", "pool_reserve_out")
    projected_in = first_int(row, "quote_projected_reserve_in", "projected_reserve_in")
    projected_out = first_int(row, "quote_projected_reserve_out", "projected_reserve_out")
    projection_latency_us = first_int(row, "projection_latency_us")
    target_entity_id = first_int(row, "target_entity_id")
    target_direction = first_text(row, "target_direction")
    target_amount_in_raw = first_int(row, "target_amount_in", "target_gross_amount_in")
    target_relationship_source = first_text(
        row,
        "target_relationship_source",
        "target_entity_source",
        "target_source",
        "target_tx_bytes_source",
    )

    target_entity_present = target_entity_id > 0
    target_tx_present = target_tx_bytes_present(row)
    target_pool_matches = bool(target_pool and pool and target_pool == pool)
    target_mint_in_matches = bool(target_mint_in and mint_in and target_mint_in == mint_in)
    target_mint_out_matches = bool(target_mint_out and mint_out and target_mint_out == mint_out)
    pre_target_pool_state_present = baseline_in > 0 and baseline_out > 0
    projected_post_target_pool_state_present = projected_in > 0 and projected_out > 0
    pm_reserve_sim_ok = bool_value(row.get("pm_reserve_ok")) or first_int(row, "reserved_notional_lamports") > 0
    target_expected_pool_delta = {
        "reserve_in_delta_raw": projected_in - baseline_in if pre_target_pool_state_present and projected_post_target_pool_state_present else 0,
        "reserve_out_delta_raw": projected_out - baseline_out if pre_target_pool_state_present and projected_post_target_pool_state_present else 0,
    }

    if not target_entity_present:
        first_blocker = "blocked_target_entity_missing"
    elif not target_tx_present:
        first_blocker = "blocked_no_target_bytes"
    elif not target_pool_matches:
        first_blocker = "blocked_target_pool_mismatch"
    elif not target_mint_in_matches or not target_mint_out_matches:
        first_blocker = "blocked_target_mint_mismatch"
    elif not pre_target_pool_state_present:
        first_blocker = "blocked_pool_state_missing"
    elif not projected_post_target_pool_state_present:
        first_blocker = "blocked_projection_failed"
    elif expected_out_raw <= 0:
        first_blocker = "blocked_projected_quote_missing"
    elif min_out_raw <= 0:
        first_blocker = "blocked_min_out_invalid"
    elif net_ev_after_tip <= 0:
        first_blocker = "blocked_ev_not_positive"
    elif not pm_reserve_sim_ok:
        first_blocker = "blocked_pm_reserve"
    else:
        first_blocker = "none"

    bundle_candidate_ready = (
        target_entity_present
        and target_tx_present
        and target_pool_matches
        and target_mint_in_matches
        and target_mint_out_matches
        and pre_target_pool_state_present
        and projected_post_target_pool_state_present
        and expected_out_raw > 0
        and min_out_raw > 0
        and net_ev_after_tip > 0
        and pm_reserve_sim_ok
    )

    return {
        "target_entity_present": target_entity_present,
        "target_tx_bytes_present": target_tx_present,
        "target_tx_bytes_available": target_tx_present,
        "target_pool_matches": target_pool_matches,
        "target_mint_in_matches": target_mint_in_matches,
        "target_mint_out_matches": target_mint_out_matches,
        "pre_target_pool_state_present": pre_target_pool_state_present,
        "projected_post_target_pool_state_present": projected_post_target_pool_state_present,
        "expected_out_raw": expected_out_raw,
        "min_out_raw": min_out_raw,
        "tip_lamports": tip_lamports,
        "net_ev_after_tip": net_ev_after_tip,
        "pm_reserve_sim_ok": pm_reserve_sim_ok,
        "bundle_candidate_ready": bundle_candidate_ready,
        "first_projection_blocker": first_blocker,
        "projection_blocker": first_blocker,
        "target_entity_id": target_entity_id,
        "target_relationship_source": target_relationship_source or ("target_entity_id" if target_entity_present else ""),
        "projection_source": "af_xdp_local",
        "projection_latency_us": projection_latency_us,
        "target_signature": first_text(row, "target_signature_hex", "signature_hex"),
        "backrun_signature": first_text(row, "our_signature_hex"),
        "slot": row.get("slot"),
        "signal_source": first_text(row, "signal_source"),
        "signal_source_id": row.get("signal_source_id"),
        "owner_mode": first_text(row, "owner_mode", "selected_owner"),
        "owner_mode_id": first_value(row, "owner_mode_id", "selected_owner_id"),
        "route_family": first_text(row, "route_family_label", "route_family"),
        "venue": first_text(row, "venue", "venue_label"),
        "venue_id": row.get("venue_id"),
        "pool": pool,
        "target_pool": target_pool,
        "mint_in": mint_in,
        "target_mint_in": target_mint_in,
        "mint_out": mint_out,
        "target_mint_out": target_mint_out,
        "target_direction": target_direction,
        "target_amount_in": target_amount_in_raw,
        "target_amount_in_raw": target_amount_in_raw,
        "target_expected_pool_delta": target_expected_pool_delta,
        "pre_target_reserve_in": baseline_in,
        "pre_target_reserve_out": baseline_out,
        "projected_post_target_reserve_in": projected_in,
        "projected_post_target_reserve_out": projected_out,
        "reserved_notional_lamports": first_int(row, "reserved_notional_lamports"),
        "trade_size_lamports": first_int(row, "trade_size_lamports"),
        "quote_target_out_lamports": first_int(row, "quote_target_out_lamports"),
        "quote_request_id": first_int(row, "quote_request_id", "ingress_quote_request_id"),
        "drop_reason": row.get("drop_reason"),
        "kill_reason": row.get("kill_reason"),
        "t0_exhaust_candidate": bool_value(row.get("t0_exhaust_candidate") or row.get("exhaust_candidate")),
        "t0_exhaust_win": bool_value(row.get("t0_exhaust_win") or row.get("exhaust_win")),
    }


def append_projection_autopsy(
    row: dict[str, Any],
    source_artifact: str,
    source_row_index: int,
) -> None:
    autopsy = projection_autopsy(row)
    key = "|".join(
        str(value or "")
        for value in [
            source_artifact,
            source_row_index,
            autopsy.get("target_signature"),
            autopsy.get("backrun_signature"),
            autopsy.get("pool"),
            autopsy.get("expected_out_raw"),
            autopsy.get("min_out_raw"),
            autopsy.get("first_projection_blocker"),
        ]
    )
    if key in autopsy_keys:
        return
    autopsy_keys.add(key)
    blocker = str(autopsy["first_projection_blocker"])
    autopsy_blocker_counts[blocker] = autopsy_blocker_counts.get(blocker, 0) + 1
    payload = {
        "record_type": "runafbr1_projection_autopsy",
        "source_artifact": source_artifact,
        "source_row_index": source_row_index,
        **autopsy,
    }
    with autopsy_rows_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(payload, sort_keys=True) + "\n")


def summarize_shock_research() -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    if shock_research_path.exists():
        for raw in shock_research_path.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                rows.append(json.loads(raw))
            except json.JSONDecodeError:
                continue

    afxdp_rows = [row for row in rows if is_afxdp_row(row)]
    afxdp_backrun_rows = [row for row in afxdp_rows if is_backrun_row(row)]
    afxdp_backrun_bundle_rows = [
        row
        for row in afxdp_backrun_rows
        if is_bundle_row(row) and not is_zeroed_telemetry_row(row)
    ]
    for idx, row in enumerate(afxdp_backrun_bundle_rows):
        append_projection_autopsy(row, "shock_research", idx)
    positive_expected_rows = [
        row for row in afxdp_backrun_bundle_rows if as_int(row.get("quote_expected_out_lamports")) > 0
    ]
    positive_min_out_rows = [
        row for row in afxdp_backrun_bundle_rows if as_int(row.get("min_out_lamports")) > 0
    ]
    positive_ev_rows = [
        row for row in afxdp_backrun_bundle_rows if as_int(row.get("retained_ev_lamports_lcb")) > 0
    ]
    strict_rows = [
        row
        for row in afxdp_backrun_bundle_rows
        if as_int(row.get("quote_expected_out_lamports")) > 0
        and as_int(row.get("min_out_lamports")) > 0
        and as_int(row.get("retained_ev_lamports_lcb")) > 0
    ]
    pm_ok_rows = [
        row
        for row in afxdp_backrun_bundle_rows
        if row.get("pm_reserve_ok") is True or text_lower(row.get("pm_reserve_ok")) == "true"
    ]
    target_entity_rows = [row for row in afxdp_backrun_bundle_rows if as_int(row.get("target_entity_id")) > 0]

    if not afxdp_rows:
        suggested_status = "blocked_no_target_bytes"
    elif not afxdp_backrun_rows:
        suggested_status = "blocked_pool_not_identified"
    elif not afxdp_backrun_bundle_rows:
        suggested_status = "blocked_bundle_shape_invalid"
    elif not positive_expected_rows:
        suggested_status = "blocked_projected_quote_missing"
    elif not positive_min_out_rows:
        suggested_status = "blocked_min_out_invalid"
    elif not positive_ev_rows:
        suggested_status = "blocked_ev_not_positive"
    elif not pm_ok_rows:
        suggested_status = "blocked_pm_reserve"
    else:
        suggested_status = "blocked_bundle_shape_invalid"

    max_value = lambda key: max((as_int(row.get(key)) for row in afxdp_backrun_bundle_rows), default=0)
    return {
        "shock_research_path": str(shock_research_path),
        "shock_research_rows": len(rows),
        "shock_afxdp_rows": len(afxdp_rows),
        "shock_afxdp_backrun_rows": len(afxdp_backrun_rows),
        "shock_afxdp_backrun_bundle_rows": len(afxdp_backrun_bundle_rows),
        "shock_afxdp_backrun_bundle_positive_expected_out_rows": len(positive_expected_rows),
        "shock_afxdp_backrun_bundle_positive_min_out_rows": len(positive_min_out_rows),
        "shock_afxdp_backrun_bundle_positive_ev_rows": len(positive_ev_rows),
        "shock_afxdp_backrun_bundle_strict_rows": len(strict_rows),
        "shock_afxdp_backrun_bundle_pm_ok_rows": len(pm_ok_rows),
        "shock_afxdp_backrun_bundle_target_entity_rows": len(target_entity_rows),
        "shock_afxdp_backrun_bundle_max_expected_out": max_value("quote_expected_out_lamports"),
        "shock_afxdp_backrun_bundle_max_min_out": max_value("min_out_lamports"),
        "shock_afxdp_backrun_bundle_max_net_ev": max_value("retained_ev_lamports_lcb"),
        "suggested_status": suggested_status,
    }


def normalize_pi_subreason(subreason: str) -> str:
    mapping = {
        "reject_pi_quote_missing": "blocked_projected_quote_missing",
        "reject_pi_min_out_missing": "blocked_min_out_invalid",
        "reject_pi_pool_state_stale": "blocked_pool_state_missing",
        "reject_pi_mint_meta_missing": "blocked_mint_meta_missing",
        "reject_pi_no_send_sink_disabled": "blocked_no_send_sink_disabled",
        "reject_pi_source_allowed_observe_only": "blocked_observe_only_source",
        "reject_pi_live_disabled_but_research_allowed": "blocked_live_disabled_but_research_allowed",
    }
    return mapping.get(subreason, "blocked_process_intent")


def summarize_process_intent_subreasons() -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    if pi_subreason_rows_path.exists():
        for raw in pi_subreason_rows_path.read_text(encoding="utf-8", errors="replace").splitlines():
            line = raw.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if text_lower(row.get("source")) != "afxdp":
                continue
            if text_lower(row.get("owner_mode")) != "backrun":
                continue
            rows.append(row)

    counts: dict[str, int] = {}
    normalized_counts: dict[str, int] = {}
    for row in rows:
        subreason = text_lower(row.get("first_subreason")) or "unknown"
        counts[subreason] = counts.get(subreason, 0) + 1
        normalized = normalize_pi_subreason(subreason)
        normalized_counts[normalized] = normalized_counts.get(normalized, 0) + 1

    top_subreason = "none"
    top_count = 0
    for key, value in counts.items():
        if value > top_count:
            top_subreason = key
            top_count = value

    return {
        "pi_rows": len(rows),
        "pi_subreason_counts": counts,
        "pi_normalized_blocker_counts": normalized_counts,
        "pi_top_subreason": top_subreason,
        "pi_top_normalized_blocker": normalize_pi_subreason(top_subreason)
        if rows
        else "none",
    }


def projection_rejection(row: dict[str, Any]) -> str | None:
    if not is_afxdp_row(row):
        return "blocked_no_target_bytes"
    if (
        row.get("target_signal_source") != "afxdp"
        or as_int(row.get("target_signal_source_id")) != 4
        or not row.get("target_tx_bytes_available")
        or row.get("target_tx_bytes_source") != "afxdp"
        or not row.get("target_tx_base64")
    ):
        return "blocked_no_target_bytes"
    target_ok, _ = decodes_base64(row.get("target_tx_base64"))
    our_ok, _ = decodes_base64(row.get("bundle_payload_base64"))
    if not target_ok:
        return "blocked_target_decode_failed"
    if not row.get("target_relationship_recorded"):
        return "blocked_pool_not_identified"
    if row.get("target_pool_pubkey_hex") != row.get("pool_pubkey_hex"):
        return "blocked_pool_not_identified"
    if row.get("target_mint_in_hex") != row.get("mint_in_hex") or row.get("target_mint_out_hex") != row.get("mint_out_hex"):
        return "blocked_pool_not_identified"
    if as_int(row.get("target_amount_in")) <= 0:
        return "blocked_target_decode_failed"
    if as_int(row.get("trade_size_lamports")) <= 0:
        return "blocked_projected_quote_missing"
    if as_int(row.get("expected_out_raw")) <= 0:
        return "blocked_projected_quote_missing"
    if as_int(row.get("min_out_raw")) <= 0:
        return "blocked_min_out_invalid"
    if as_int(row.get("net_ev_after_tip_lamports")) <= 0:
        return "blocked_ev_not_positive"
    if row.get("route_family") not in bundle_routes or not our_ok or as_int(row.get("tip_lamports")) <= 0:
        return "blocked_bundle_shape_invalid"
    if not row.get("pm_reserve_ok") or as_int(row.get("reserved_notional_lamports")) <= 0:
        return "blocked_pm_reserve"
    return None


def write_json(path: Path, doc: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def provider_reports_already_processed(doc: dict[str, Any]) -> bool:
    summary = doc.get("simulate_bundle_summary")
    if summary is None:
        return False
    return "already been processed" in json.dumps(summary, sort_keys=True).lower()


def normalize_freshness_status(doc: dict[str, Any]) -> str:
    result = str(doc.get("selection_result") or "")
    blockers = list(doc.get("blockers") or [])
    if result == "target_fresh_bundle_eligible":
        return "afbr_target_fresh_bundle_eligible_no_send"
    if result == "blocked_target_already_landed" or doc.get("target_already_landed") or provider_reports_already_processed(doc):
        return "blocked_target_already_processed"
    if (
        result in {"blocked_target_recent_blockhash_invalid", "blocked_target_blockhash_invalid"}
        or doc.get("target_recent_blockhash_valid") is False
    ):
        return "blocked_blockhash_invalid"
    if result == "blocked_simulate_bundle_failed" or "blocked_simulate_bundle_failed" in blockers:
        return "blocked_provider_simulation_failed"
    if any(item in blockers for item in ["blocked_target_tx_bytes_missing", "blocked_target_tx_decode_failed"]):
        return "blocked_no_target_bytes"
    return "blocked_bundle_shape_invalid"


def build_candidate_docs(row: dict[str, Any], candidate_dir: Path) -> tuple[Path, Path, Path]:
    target_payload = str(row.get("target_tx_base64") or "")
    our_payload = str(row.get("bundle_payload_base64") or "")
    target_ok, target_len = decodes_base64(target_payload)
    txs_base64 = [target_payload, our_payload]
    pm_fields = {
        "selected_pm_reserve_created": True,
        "selected_pm_reserve_released": True,
        "selected_pm_reserve_id": None,
        "selected_pm_release_reason": "no_send_probe_released",
        "selected_pm_pending_after": 0,
    }
    candidate_selected = candidate_dir / "runls11_selected_backrun_bundle.json"
    candidate_payload = candidate_dir / "runls11_bundle_payload_base64.json"
    candidate_pm = candidate_dir / "runls11_pm_reserve_report.json"
    candidate_probe = candidate_dir / "runls11_bundle_probe_rows.jsonl"
    selected_doc = {
        "record_type": "runafbr1_selected_backrun_bundle",
        "selection_result": "selected_afxdp_local_projection",
        "source_target": "afxdp",
        "source_enrichment": "afxdp_local_projection",
        "provider": "jito_via_helius",
        "selected_owner_mode": "backrun",
        "selected_route_family": row.get("route_family"),
        "selected_pool": row.get("pool_pubkey_hex"),
        "selected_mint_in": row.get("mint_in_hex"),
        "selected_mint_out": row.get("mint_out_hex"),
        "selected_venue": row.get("venue"),
        "selected_slot": row.get("slot"),
        "selected_target_signature": row.get("target_signature_hex"),
        "selected_target_tx_bytes_available": True,
        "selected_target_tx_bytes_source": "afxdp_target_tx_cache",
        "selected_target_tx_encoding": row.get("target_tx_encoding"),
        "selected_target_tx_decodes_locally": target_ok,
        "selected_target_tx_decoded_len_bytes": target_len,
        "selected_target_tx_message_hash": row.get("target_tx_message_hash"),
        "selected_timestamp_basis": row.get("timestamp_basis"),
        "selected_afxdp_seen_ns": row.get("afxdp_seen_ns"),
        "selected_first_seen_ns": row.get("first_seen_ns"),
        "selected_current_normalized_record_ns": row.get("current_normalized_record_ns"),
        "selected_current_worker_seen_ns": row.get("current_worker_seen_ns"),
        "selected_current_route_ready_ns": row.get("current_route_ready_ns"),
        "selected_current_quote_ready_ns": row.get("current_quote_ready_ns"),
        "selected_current_pm_ready_ns": row.get("current_pm_ready_ns"),
        "selected_current_candidate_emit_ns": row.get("current_candidate_emit_ns"),
        "selected_current_trade_ready_ns": row.get("current_trade_ready_ns"),
        "selected_decode_delta_us": row.get("decode_delta_us"),
        "selected_normalized_record_delta_us": row.get("normalized_record_delta_us"),
        "selected_worker_seen_delta_us": row.get("worker_seen_delta_us"),
        "selected_route_ready_delta_us": row.get("route_ready_delta_us"),
        "selected_quote_ready_delta_us": row.get("quote_ready_delta_us"),
        "selected_pm_ready_delta_us": row.get("pm_ready_delta_us"),
        "selected_candidate_emit_delta_us": row.get("candidate_emit_delta_us"),
        "selected_trade_ready_delta_us": row.get("trade_ready_delta_us"),
        "selected_target_pool": row.get("target_pool_pubkey_hex"),
        "selected_target_pool_matches": row.get("target_pool_pubkey_hex") == row.get("pool_pubkey_hex"),
        "selected_target_mint_in": row.get("target_mint_in_hex"),
        "selected_target_mint_in_matches": row.get("target_mint_in_hex") == row.get("mint_in_hex"),
        "selected_target_mint_out": row.get("target_mint_out_hex"),
        "selected_target_mint_out_matches": row.get("target_mint_out_hex") == row.get("mint_out_hex"),
        "selected_target_amount_in": row.get("target_amount_in"),
        "target_pool_identified": True,
        "target_direction_decoded": bool(row.get("target_direction")),
        "target_amount_decoded": as_int(row.get("target_amount_in")) > 0,
        "pool_state_before_available": True,
        "projected_pool_state_after_computed": True,
        "backrun_quote_computed_from_projected_state": True,
        "laserstream_required_for_critical_path": False,
        "selected_backrun_signature": row.get("our_signature_hex"),
        "selected_backrun_tx_template": row.get("template_id"),
        "selected_expected_out_raw": as_int(row.get("expected_out_raw")),
        "selected_min_out_raw": as_int(row.get("min_out_raw")),
        "selected_net_ev_after_tip": as_int(row.get("net_ev_after_tip_lamports")),
        "selected_tip_lamports": as_int(row.get("tip_lamports")),
        "selected_pm_reserve_amount": as_int(row.get("reserved_notional_lamports")),
        "selected_pm_reserve_ok": True,
        **pm_fields,
        "bundle_envelope_built": bool(our_payload and target_payload),
        "bundle_tx_count": 2,
        "target_tx_first": True,
        "our_backrun_tx_second": True,
        "tip_present": as_int(row.get("tip_lamports")) > 0,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    payload_doc = {
        "record_type": "runafbr1_bundle_payload",
        "bundle_payload_valid": bool(our_payload and target_payload),
        "selected_route_family": row.get("route_family"),
        "provider": "jito_via_helius",
        "bundle_tx_count": 2,
        "bundle_tx_count_max_allowed": 5,
        "target_tx_required": True,
        "target_tx_bytes_available": True,
        "target_tx_first": True,
        "our_backrun_tx_present": bool(our_payload),
        "our_backrun_tx_second": True,
        "tip_present": as_int(row.get("tip_lamports")) > 0,
        "min_out_positive": as_int(row.get("min_out_raw")) > 0,
        "expected_out_positive": as_int(row.get("expected_out_raw")) > 0,
        "net_ev_positive_after_tip": as_int(row.get("net_ev_after_tip_lamports")) > 0,
        "estimated_tip_lamports": as_int(row.get("tip_lamports")),
        "estimated_priority_fee_lamports": 0,
        "bundle_expiry_policy": "runafbr1_no_submit_readiness_only",
        "send_bundle_disabled_in_mode": True,
        "sendBundle_called": False,
        "txs_base64": txs_base64,
        "wire_send": 0,
        "live_landed": 0,
    }
    pm_doc = {
        "record_type": "runafbr1_pm_reserve_report",
        "selection_result": selected_doc["selection_result"],
        "selected_pm_reserve_ok": True,
        "selected_reserved_notional_lamports": as_int(row.get("reserved_notional_lamports")),
        **pm_fields,
        "pm_reserve_reconciled": True,
    }
    candidate_dir.mkdir(parents=True, exist_ok=True)
    write_json(candidate_selected, selected_doc)
    write_json(candidate_payload, payload_doc)
    write_json(candidate_pm, pm_doc)
    candidate_probe.write_text(json.dumps(row, sort_keys=True) + "\n", encoding="utf-8")
    return candidate_selected, candidate_payload, candidate_pm


def run_freshness_check(
    row: dict[str, Any],
    index: int,
    pre_inline_doc: dict[str, Any] | None = None,
) -> tuple[str, dict[str, Any], Path]:
    candidate_dir = inner_dir / "runafbr1_candidates" / f"candidate_{index:04d}"
    inline_doc: dict[str, Any] = dict(pre_inline_doc or {})
    if inline_bundle_ready_probe and not inline_doc:
        row["monitor_seen_ns"] = time.monotonic_ns()
        inline_doc = inline_bundle_ready_status_probe(row)
    if inline_doc:
        inline_doc.setdefault("inline_probe_order", "before_candidate_artifact_build")
        inline_doc.setdefault("inline_probe_before_candidate_artifacts", True)
    candidate_selected, candidate_payload, _ = build_candidate_docs(row, candidate_dir)
    subprocess.run(
        [
            "bash",
            str(ls11e_script),
            str(candidate_dir),
            str(candidate_selected),
            str(candidate_payload),
            str(candidate_dir / "runls11_bundle_probe_rows.jsonl"),
        ],
        cwd=str(pack_dir),
        env=os.environ.copy(),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
        timeout=25,
    )
    candidate_freshness = candidate_dir / "runls11e_target_freshness_bundle_eligibility.json"
    if not candidate_freshness.exists():
        return "blocked_provider_simulation_failed", inline_doc, candidate_dir
    doc = json.loads(candidate_freshness.read_text(encoding="utf-8"))
    if inline_doc:
        doc["runls11e_target_signature_status_before_bundle"] = doc.get("target_signature_status_before_bundle")
        doc["runls11e_first_status_check_start_ns"] = doc.get("first_status_check_start_ns")
        doc["runls11e_first_status_check_end_ns"] = doc.get("first_status_check_end_ns")
        doc["runls11e_first_status_check_latency_us"] = doc.get("first_status_check_latency_us")
        doc["runls11e_bundle_ready_to_status_start_us"] = doc.get("bundle_ready_to_status_start_us")
        doc.update(inline_doc)
    return normalize_freshness_status(doc), doc, candidate_dir


def copy_candidate_artifacts(candidate_dir: Path, freshness_doc: dict[str, Any], status: str) -> dict[str, Any]:
    shutil.copyfile(candidate_dir / "runls11_selected_backrun_bundle.json", selected_path)
    shutil.copyfile(candidate_dir / "runls11_bundle_payload_base64.json", payload_path)
    shutil.copyfile(candidate_dir / "runls11_pm_reserve_report.json", pm_report_path)
    normalized = {
        **freshness_doc,
        "record_type": "runafbr1_target_freshness_bundle_eligibility",
        "status": status,
        "selection_result": status,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
    }
    if status == "blocked_target_already_processed":
        normalized["target_already_landed"] = True
    write_json(freshness_path, normalized)
    return normalized


def write_summary(status: str, freshness_doc: dict[str, Any] | None = None) -> None:
    global shock_summary
    selected_doc: dict[str, Any] = {}
    if selected_path.exists():
        selected_doc = json.loads(selected_path.read_text(encoding="utf-8"))
    freshness_doc = freshness_doc or {}
    if shock_summary is None:
        shock_summary = summarize_shock_research()
    pi_summary = summarize_process_intent_subreasons()
    effective_status = status
    if (
        status == "blocked_projected_quote_missing"
        and not selected_doc
        and pi_summary["pi_rows"] > 0
    ):
        effective_status = str(pi_summary["pi_top_normalized_blocker"])
    summary = {
        "record_type": "runafbr1_summary",
        "run_name": "RunAFBR1",
        "status": effective_status,
        "selection_result": effective_status,
        "source_target": "afxdp",
        "target_tx_bytes_source": "afxdp_target_tx_cache" if selected_doc else None,
        "target_candidate_seen": target_seen > 0,
        "strict_projection_candidate_seen": strict_seen > 0,
        "target_tx_bytes_available": bool(selected_doc.get("selected_target_tx_bytes_available")),
        "target_tx_decodes_locally": bool(selected_doc.get("selected_target_tx_decodes_locally")),
        "target_already_landed": bool(freshness_doc.get("target_already_landed")) if freshness_doc else None,
        "target_recent_blockhash_valid": bool(freshness_doc.get("target_recent_blockhash_valid")) if freshness_doc else None,
        "target_pool_identified": bool(selected_doc.get("target_pool_identified")),
        "target_direction_decoded": bool(selected_doc.get("target_direction_decoded")),
        "target_amount_decoded": bool(selected_doc.get("target_amount_decoded")),
        "pool_state_before_available": bool(selected_doc.get("pool_state_before_available")),
        "projected_pool_state_after_computed": bool(selected_doc.get("projected_pool_state_after_computed")),
        "backrun_quote_computed_from_projected_state": bool(selected_doc.get("backrun_quote_computed_from_projected_state")),
        "expected_out_raw": selected_doc.get("selected_expected_out_raw", 0),
        "min_out_raw": selected_doc.get("selected_min_out_raw", 0),
        "net_ev_after_tip": selected_doc.get("selected_net_ev_after_tip", 0),
        "selected_pm_reserve_ok": bool(selected_doc.get("selected_pm_reserve_ok")),
        "selected_pm_reserve_created": bool(selected_doc.get("selected_pm_reserve_created")),
        "selected_pm_reserve_released": bool(selected_doc.get("selected_pm_reserve_released")),
        "selected_pm_pending_after": selected_doc.get("selected_pm_pending_after"),
        "bundle_tx_count": selected_doc.get("bundle_tx_count", 0),
        "target_tx_first": bool(selected_doc.get("target_tx_first")),
        "our_backrun_tx_second": bool(selected_doc.get("our_backrun_tx_second")),
        "tip_present": bool(selected_doc.get("tip_present")),
        "provider": "jito_via_helius",
        "provider_auth_material_present": bool(
            os.environ.get("SHADOW_HELIUS_BUNDLE_API_KEY")
            or "api-key=" in os.environ.get("SHADOW_HELIUS_BUNDLE_RPC_URL", "").lower()
            or "api-key=" in os.environ.get("SHADOW_SOLANA_RPC_URL", "").lower()
        ),
        "simulate_bundle_available": bool(freshness_doc.get("simulate_bundle_available")) if freshness_doc else None,
        "simulateBundle_pass": bool(freshness_doc.get("simulateBundle_pass")) if freshness_doc else None,
        "sendBundle_called": False,
        "wire_send": 0,
        "live_landed": 0,
        "candidate_seen_count": target_seen,
        "strict_candidate_seen_count": strict_seen,
        "candidate_rejection_counts": status_counts,
        "projection_autopsy_rows_path": str(autopsy_rows_path),
        "projection_autopsy_row_count": len(autopsy_keys),
        "projection_autopsy_blocker_counts": autopsy_blocker_counts,
        "bundle_probe_rows_seen": len(seen_keys),
        "no_bundle_probe_upstream_summary": shock_summary,
        "pi_subreason_rows_path": str(pi_subreason_rows_path) if pi_subreason_rows_path.exists() else None,
        "pi_subreason_row_count": pi_summary["pi_rows"],
        "pi_subreason_counts": pi_summary["pi_subreason_counts"],
        "pi_normalized_blocker_counts": pi_summary["pi_normalized_blocker_counts"],
        "pi_top_subreason": pi_summary["pi_top_subreason"],
        "pi_top_normalized_blocker": pi_summary["pi_top_normalized_blocker"],
        "elapsed_seconds": round(time.monotonic() - start, 3),
        "selected_artifact_path": str(selected_path) if selected_path.exists() else None,
        "payload_artifact_path": str(payload_path) if payload_path.exists() else None,
        "pm_report_path": str(pm_report_path) if pm_report_path.exists() else None,
        "freshness_artifact_path": str(freshness_path) if freshness_path.exists() else None,
        "laserstream_required_for_critical_path": False,
    }
    write_json(summary_path, summary)


index = 0
while time.monotonic() < deadline:
    for hot_row in load_hot_probe_rows():
        key = probe_key(hot_row)
        if key in seen_keys:
            continue
        seen_keys.add(key)
        if hot_row.get("signal_source") == "afxdp":
            target_seen += 1
        if not hot_inline_candidate(hot_row):
            continue
        strict_seen += 1
        index += 1
        hot_row["monitor_seen_ns"] = time.monotonic_ns()
        inline_doc = inline_bundle_ready_status_probe(hot_row)
        inline_doc["inline_probe_order"] = "hot_probe_before_full_artifact"
        inline_doc["inline_probe_before_candidate_artifacts"] = True
        inline_doc["hot_probe_path"] = str(hot_probe_rows_path)
        full_row = wait_for_matching_full_probe_row(hot_row)
        if full_row is None:
            append_rejection(hot_row, "blocked_no_target_bytes", {
                "hot_probe_full_artifact_missing": True,
                "bundle_probe_export_to_status_start_us": inline_doc.get("bundle_probe_export_to_status_start_us"),
                "target_already_landed": inline_doc.get("target_already_landed"),
            })
            continue
        seen_keys.add(probe_key(full_row))
        status, freshness_doc, candidate_dir = run_freshness_check(full_row, index, inline_doc)
        append_projection_autopsy(full_row, "runls11_bundle_probe", len(seen_keys))
        normalized_doc = copy_candidate_artifacts(candidate_dir, freshness_doc, status)
        if status == "afbr_target_fresh_bundle_eligible_no_send":
            write_summary(status, normalized_doc)
            stop_file.write_text(status + "\n", encoding="utf-8")
            sys.exit(0)
        append_rejection(full_row, status, {
            "target_already_landed": normalized_doc.get("target_already_landed"),
            "target_recent_blockhash_valid": normalized_doc.get("target_recent_blockhash_valid"),
            "simulateBundle_pass": normalized_doc.get("simulateBundle_pass"),
            "inline_hot_probe": True,
            "hot_probe_path": str(hot_probe_rows_path),
        })
        if stop_after_first_strict_candidate:
            write_summary(status, normalized_doc)
            stop_file.write_text(status + "\n", encoding="utf-8")
            sys.exit(0)
    for row in load_probe_rows():
        key = probe_key(row)
        if key in seen_keys:
            continue
        seen_keys.add(key)
        if row.get("signal_source") == "afxdp":
            target_seen += 1
        if hot_inline_candidate(row):
            strict_seen += 1
            index += 1
            status, freshness_doc, candidate_dir = run_freshness_check(row, index)
            append_projection_autopsy(row, "runls11_bundle_probe", len(seen_keys))
            normalized_doc = copy_candidate_artifacts(candidate_dir, freshness_doc, status)
            if status == "afbr_target_fresh_bundle_eligible_no_send":
                write_summary(status, normalized_doc)
                stop_file.write_text(status + "\n", encoding="utf-8")
                sys.exit(0)
            append_rejection(row, status, {
                "target_already_landed": normalized_doc.get("target_already_landed"),
                "target_recent_blockhash_valid": normalized_doc.get("target_recent_blockhash_valid"),
                "simulateBundle_pass": normalized_doc.get("simulateBundle_pass"),
                "inline_hot_probe": True,
            })
            if stop_after_first_strict_candidate:
                write_summary(status, normalized_doc)
                stop_file.write_text(status + "\n", encoding="utf-8")
                sys.exit(0)
            continue
        append_projection_autopsy(row, "runls11_bundle_probe", len(seen_keys))
        status = projection_rejection(row)
        if status:
            append_rejection(row, status)
            continue
        strict_seen += 1
        index += 1
        status, freshness_doc, candidate_dir = run_freshness_check(row, index)
        normalized_doc = copy_candidate_artifacts(candidate_dir, freshness_doc, status)
        if status == "afbr_target_fresh_bundle_eligible_no_send":
            write_summary(status, normalized_doc)
            stop_file.write_text(status + "\n", encoding="utf-8")
            sys.exit(0)
        append_rejection(row, status, {
            "target_already_landed": normalized_doc.get("target_already_landed"),
            "target_recent_blockhash_valid": normalized_doc.get("target_recent_blockhash_valid"),
            "simulateBundle_pass": normalized_doc.get("simulateBundle_pass"),
        })
        if stop_after_first_strict_candidate:
            write_summary(status, normalized_doc)
            stop_file.write_text(status + "\n", encoding="utf-8")
            sys.exit(0)
    time.sleep(monitor_poll_seconds)

if strict_seen > 0:
    if status_counts.get("blocked_target_already_processed"):
        final_status = "blocked_target_already_processed"
    elif status_counts.get("blocked_blockhash_invalid"):
        final_status = "blocked_blockhash_invalid"
    elif status_counts.get("blocked_provider_simulation_failed"):
        final_status = "blocked_provider_simulation_failed"
    else:
        final_status = "blocked_projected_quote_missing"
elif target_seen > 0:
    final_status = max(status_counts.items(), key=lambda item: item[1])[0] if status_counts else "blocked_projected_quote_missing"
else:
    shock_summary = summarize_shock_research()
    final_status = str(shock_summary.get("suggested_status") or "blocked_no_target_bytes")
    append_summary_rejection(final_status, shock_summary)
write_summary(final_status)
sys.exit(1)
PY
MONITOR_PID=$!
if value_is_true "${SHADOW_RUNAFBR1_MONITOR_REALTIME:-false}"; then
    renice -n "${SHADOW_RUNAFBR1_MONITOR_NICE:--20}" -p "$MONITOR_PID" >/dev/null 2>&1 || true
    if command -v chrt >/dev/null 2>&1; then
        chrt -f -p "${SHADOW_RUNAFBR1_MONITOR_RT_PRIORITY:-20}" "$MONITOR_PID" >/dev/null 2>&1 || true
    fi
fi

bash "$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"
WINDOW_RC=$?
wait "$MONITOR_PID" || MONITOR_RC=$?
MONITOR_RC="${MONITOR_RC:-0}"

if [[ "$WINDOW_RC" -ne 0 ]]; then
    fail "RunAFBR1 inner observe-only window failed"
fi
if [[ ! -f "$SUMMARY_JSON_PATH" ]]; then
    fail "RunAFBR1 summary missing: $SUMMARY_JSON_PATH"
    finish
    exit 1
fi

readarray -t AFBR_FIELDS < <(python3 - "$SUMMARY_JSON_PATH" <<'PY'
import json
import sys
from pathlib import Path

doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for key in [
    "status",
    "target_candidate_seen",
    "strict_projection_candidate_seen",
    "target_tx_bytes_available",
    "target_recent_blockhash_valid",
    "simulateBundle_pass",
    "wire_send",
    "live_landed",
]:
    print(doc.get(key))
PY
)

STATUS="${AFBR_FIELDS[0]:-unknown}"
TARGET_SEEN="${AFBR_FIELDS[1]:-false}"
STRICT_SEEN="${AFBR_FIELDS[2]:-false}"
TARGET_BYTES="${AFBR_FIELDS[3]:-false}"
BLOCKHASH_VALID="${AFBR_FIELDS[4]:-}"
SIM_PASS="${AFBR_FIELDS[5]:-}"
WIRE_SEND="${AFBR_FIELDS[6]:-0}"
LIVE_LANDED="${AFBR_FIELDS[7]:-0}"

{
    echo "runafbr1_summary_path=$SUMMARY_JSON_PATH"
    echo "runafbr1_rejection_rows_path=$REJECTION_ROWS_PATH"
    echo "runafbr1_projection_autopsy_rows_path=$AUTOPSY_ROWS_PATH"
    echo "runafbr1_selected_artifact_path=$SELECTED_JSON_PATH"
    echo "runafbr1_payload_artifact_path=$PAYLOAD_JSON_PATH"
    echo "runafbr1_pm_report_path=$PM_REPORT_JSON_PATH"
    echo "runafbr1_freshness_artifact_path=$FRESHNESS_JSON_PATH"
    echo "status=$STATUS"
    echo "target_candidate_seen=$TARGET_SEEN"
    echo "strict_projection_candidate_seen=$STRICT_SEEN"
    echo "target_tx_bytes_available=$TARGET_BYTES"
    echo "target_recent_blockhash_valid=$BLOCKHASH_VALID"
    echo "simulateBundle_pass=$SIM_PASS"
    echo "sendBundle_called=false"
    echo "wire_send=$WIRE_SEND"
    echo "live_landed=$LIVE_LANDED"
    echo "inner_summary_path=$INNER_SUMMARY_FILE"
} | tee "$OUTER_SUMMARY_FILE"

if [[ "$WIRE_SEND" != "0" || "$LIVE_LANDED" != "0" ]]; then
    fail "RunAFBR1 observed live activity wire_send=$WIRE_SEND live_landed=$LIVE_LANDED"
fi

if [[ "$STATUS" == "afbr_target_fresh_bundle_eligible_no_send" ]]; then
    pass "RunAFBR1 found a fresh AF_XDP-only Backrun bundle candidate"
else
    fail "RunAFBR1 blocked safely: $STATUS"
fi

finish
[[ "$STATUS" == "afbr_target_fresh_bundle_eligible_no_send" && "$WIRE_SEND" == "0" && "$LIVE_LANDED" == "0" ]]

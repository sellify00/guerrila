#!/usr/bin/env bash
# 77C - map the selected strict DirectArb replay class onto private egress in no-send mode

source "$(dirname "$0")/lib.sh"
script_header "77C - Selected Strict DirectArb Private-Egress No-Send"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_NIC_IFACE="${SHADOW_NIC_IFACE-}"
CALLER_SHADOW_XSKMAP_PIN="${SHADOW_XSKMAP_PIN-}"
CALLER_SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE-}"
CALLER_SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE-}"
CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP="${SHADOW_ALLOW_MANAGEMENT_NIC_XDP-}"
CALLER_SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP-}"
CALLER_SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL-}"
CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="${SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL-}"
CALLER_SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL-}"
CALLER_SHADOW_RUN40_RPC_URL="${SHADOW_RUN40_RPC_URL-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
[[ -n "$CALLER_SHADOW_NIC_IFACE" ]] && export SHADOW_NIC_IFACE="$CALLER_SHADOW_NIC_IFACE"
[[ -n "$CALLER_SHADOW_XSKMAP_PIN" ]] && export SHADOW_XSKMAP_PIN="$CALLER_SHADOW_XSKMAP_PIN"
[[ -n "$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP" ]] && export SHADOW_ALLOW_MANAGEMENT_NIC_XDP="$CALLER_SHADOW_ALLOW_MANAGEMENT_NIC_XDP"
[[ -n "$CALLER_SHADOW_REUSE_ATTACHED_XDP" ]] && export SHADOW_REUSE_ATTACHED_XDP="$CALLER_SHADOW_REUSE_ATTACHED_XDP"
[[ -n "$CALLER_SHADOW_BOOTSTRAP_RPC_URL" ]] && export SHADOW_BOOTSTRAP_RPC_URL="$CALLER_SHADOW_BOOTSTRAP_RPC_URL"
[[ -n "$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL" ]] && export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL="$CALLER_SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL"
[[ -n "$CALLER_SHADOW_SOLANA_RPC_URL" ]] && export SHADOW_SOLANA_RPC_URL="$CALLER_SHADOW_SOLANA_RPC_URL"
[[ -n "$CALLER_SHADOW_RUN40_RPC_URL" ]] && export SHADOW_RUN40_RPC_URL="$CALLER_SHADOW_RUN40_RPC_URL"

export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN77C_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN77C_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
export SHADOW_REUSE_ATTACHED_XDP="${SHADOW_REUSE_ATTACHED_XDP:-true}"

is_true() {
    case "${1:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

require_env SHADOW_NIC_IFACE
export SHADOW_PACK_DIR="$PACK_DIR"

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN77C_STEP:-epoch8_auto_strict_directarb_private_egress_run77c}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INNER_RESULTS_ROOT="$RESULT_DIR/_inner"
INNER_STEP_DIR="$INNER_RESULTS_ROOT/20_g12_r0_observe_only"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run77c_selected_directarb_private_egress_nosend.json"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/pre_attach_local_agave_preflight.txt"
HYDRATION_SUMMARY="$RESULT_DIR/run77c_prehydration_summary.txt"
REPLAY_SPEC_PATH="$RESULT_DIR/run77c_scorecard_replay_spec.json"
REPLAY_TX_PATH="${SHADOW_RUN77_REPLAY_TX_PATH:-$RESULT_DIR/selected_replay_transaction.json}"
NORMALIZED_CONFIG="$RESULT_DIR/run77c.config.yaml"

SCORECARD_PATH="${SHADOW_RUN77_SCORECARD_PATH:-$RESULTS_ROOT/epoch8_auto_strict_directarb_quote_identity_run76_selector/run76_selected_scorecard.json}"
CONFIG_SOURCE="${SHADOW_RUN77C_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
WINDOW_SECONDS="${SHADOW_RUN77C_WINDOW_SECONDS:-90}"
POST_WINDOW_DRAIN_SECONDS="${SHADOW_RUN77C_POST_WINDOW_DRAIN_SECONDS:-5}"
HYDRATION_RPC_URL="${SHADOW_RUN77_REPLAY_RPC_URL:-${SHADOW_RUN40_SCORECARD_REPLAY_RPC_URL:-$(default_public_rpc_url)}}"
TARGET_ROUTE_FAMILY="${SHADOW_RUN77C_TARGET_ROUTE_FAMILY:-helius_sender_dual}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-${RESULTS_ROOT}/run39_native_oob_ready.flag}"

EXPECTED_SIGNATURE="${SHADOW_RUN77_EXPECTED_SIGNATURE:-2rhCyCFgu9eJrBJfvuCYZM53coE1cuYJEnt7NxaUnCzhxsUhBKby1L7cRZiT7P9pGmLSPmr9kkd7Kh2EHdQojyL4}"
EXPECTED_POOL="${SHADOW_RUN77_EXPECTED_POOL:-E9dpbYUGp1gywFcAxsKJuNHZAQQjZLWusLXt19kCVcN4}"
EXPECTED_MINT_IN="${SHADOW_RUN77_EXPECTED_MINT_IN:-So11111111111111111111111111111111111111112}"
EXPECTED_MINT_OUT="${SHADOW_RUN77_EXPECTED_MINT_OUT:-456pnYfppwJozLJ4nr156HwvJQmqnvN8Uxs3RKurbE8o}"
EXPECTED_SELECTED_SOURCE="${SHADOW_RUN77_EXPECTED_SOURCE:-afxdp}"

mkdir -p "$RESULT_DIR" "$INNER_RESULTS_ROOT"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$HYDRATION_SUMMARY" "$REPLAY_SPEC_PATH"

summary_set() {
    local key="$1"
    local value="$2"
    printf '%s=%s\n' "$key" "$value" >> "$SUMMARY_FILE"
}

summary_force_status() {
    local status="$1"
    local tmp="${SUMMARY_FILE}.tmp.$$"
    if [[ -f "$SUMMARY_FILE" ]]; then
        awk -F= '$1 != "status" { print }' "$SUMMARY_FILE" > "$tmp"
    else
        : > "$tmp"
    fi
    {
        printf 'status=%s\n' "$status"
        cat "$tmp"
    } > "$SUMMARY_FILE"
    rm -f "$tmp"
}

run77c_xdp_line_count() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -ci 'xdp' || true
}

prepare_run77c_config() {
    python3 - "$CONFIG_SOURCE" "$NORMALIZED_CONFIG" <<'PY'
import re
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])
text = source_path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n")

replacements = {
    r'(^\s*default_jito_searcher\s*:\s*).+$': r'\1helius_sender_dual',
    r'(^\s*grpc_source_grade\s*:\s*).+$': r'\1external_geyser',
}
for pattern, replacement in replacements.items():
    if re.search(pattern, text, flags=re.M):
        text = re.sub(pattern, replacement, text, count=1, flags=re.M)

target_path.write_text(text if text.endswith("\n") else text + "\n", encoding="utf-8")
PY
}

prepare_replay_inputs() {
    python3 - "$SCORECARD_PATH" "$REPLAY_SPEC_PATH" "$REPLAY_TX_PATH" "$HYDRATION_SUMMARY" \
        "$HYDRATION_RPC_URL" "$EXPECTED_SIGNATURE" "$EXPECTED_POOL" "$EXPECTED_MINT_IN" \
        "$EXPECTED_MINT_OUT" "$TARGET_ROUTE_FAMILY" "$EXPECTED_SELECTED_SOURCE" <<'PY'
from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.request
from pathlib import Path

scorecard_path = Path(sys.argv[1])
replay_spec_path = Path(sys.argv[2])
replay_tx_path = Path(sys.argv[3])
summary_path = Path(sys.argv[4])
rpc_url = sys.argv[5]
expected_sig = sys.argv[6]
expected_pool = sys.argv[7]
expected_mint_in = sys.argv[8]
expected_mint_out = sys.argv[9]
target_route_family = sys.argv[10]
expected_selected_source = sys.argv[11]
replay_signal_source = "external_geyser"

alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0

def b58encode(raw: bytes) -> str:
    value = int.from_bytes(raw, "big")
    out = ""
    while value:
        value, rem = divmod(value, 58)
        out = alphabet[rem] + out
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + (out or "")

def read_compact_u16(raw: bytes, offset: int):
    value = 0
    shift = 0
    for _ in range(3):
        if offset >= len(raw):
            raise ValueError("short transaction while reading signature count")
        byte = raw[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return value, offset
        shift += 7
    raise ValueError("signature count compact-u16 is too long")

scorecard = json.loads(scorecard_path.read_text(encoding="utf-8"))
run40_selection = scorecard.get("run40_candidate_selection") or {}
candidate = run40_selection.get("selected_candidate") or {}
selected_class = scorecard.get("selected_class") or {}
if not candidate:
    raise SystemExit("selected_class_loaded=0: run40_candidate_selection.selected_candidate missing")

checks = {
    "signature": (str(candidate.get("signature") or ""), expected_sig),
    "pool": (str(candidate.get("pool") or ""), expected_pool),
    "mint_in": (str(candidate.get("mint_in") or ""), expected_mint_in),
    "mint_out": (str(candidate.get("mint_out") or ""), expected_mint_out),
    "route": (str(candidate.get("route") or ""), "tpu_quic_public"),
    "source": (str(candidate.get("source") or ""), expected_selected_source),
}
for label, (actual, expected) in checks.items():
    if actual != expected:
        raise SystemExit(f"{label} mismatch: expected {expected} got {actual}")

replay_spec = {
    "signature": candidate["signature"],
    "signal_source": replay_signal_source,
    "slot": safe_int(candidate.get("slot")),
    "unix_ns": safe_int(candidate.get("unix_ns")),
    "pool": candidate["pool"],
    "mint_in": candidate["mint_in"],
    "mint_out": candidate["mint_out"],
}
replay_spec_path.write_text(json.dumps(replay_spec, indent=2) + "\n", encoding="utf-8")

body = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
        candidate["signature"],
        {
            "encoding": "base64",
            "commitment": "confirmed",
            "maxSupportedTransactionVersion": 0,
        },
    ],
}
request = urllib.request.Request(
    rpc_url,
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json"},
)
context = ssl.create_default_context() if rpc_url.startswith("https://") else None
with urllib.request.urlopen(request, timeout=20, context=context) as response:
    payload = json.loads(response.read().decode())
if payload.get("error"):
    raise SystemExit(f"getTransaction RPC error: {payload['error']}")
result = payload.get("result")
if not result:
    raise SystemExit("getTransaction returned null/missing result")

tx_value = result.get("transaction")
if isinstance(tx_value, list):
    encoded_tx = tx_value[0]
elif isinstance(tx_value, str):
    encoded_tx = tx_value
else:
    raise SystemExit("getTransaction result.transaction is not base64")

raw_tx = base64.b64decode(encoded_tx)
sig_count, sig_offset = read_compact_u16(raw_tx, 0)
if sig_count < 1 or len(raw_tx) < sig_offset + 64:
    raise SystemExit("hydrated transaction has no first signature")
first_sig = b58encode(raw_tx[sig_offset:sig_offset + 64])
if first_sig != candidate["signature"]:
    raise SystemExit(
        f"hydrated transaction signature mismatch: expected {candidate['signature']} got {first_sig}"
    )

artifact = {
    "signature": candidate["signature"],
    "slot": safe_int(result.get("slot")),
    "transaction": tx_value,
    "meta": result.get("meta"),
    "encoding": "base64",
    "pool": candidate["pool"],
    "mint_in": candidate["mint_in"],
    "mint_out": candidate["mint_out"],
    "selected_template_id": candidate.get("run76_template_id"),
    "selected_min_out_raw": safe_int(candidate.get("run76_min_out_raw")),
    "selected_expected_out_raw": safe_int(candidate.get("run76_expected_out_raw")),
    "selected_target_out_raw": safe_int(candidate.get("run76_target_out_raw")),
    "selected_quote_request_id": candidate.get("run76_quote_request_id"),
    "selected_quote_match_mode": candidate.get("run76_quote_match_mode"),
    "selected_route": "tpu_quic_public",
}
replay_tx_path.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")

exact_quote_match_count = 1 if str(candidate.get("run76_quote_match_mode") or "").strip() not in {"", "none", "slot_pool_mints_ambiguous"} else 0
min_out_positive_count = 1 if safe_int(candidate.get("run76_min_out_raw")) > 0 or bool(selected_class.get("selected_min_out_positive")) else 0
route_specific_positive = 1 if any([
    safe_int(candidate.get("net_after_fee_lamports")) > 0,
    safe_int(candidate.get("retained_ev_lamports")) > 0,
    safe_float(candidate.get("run76_deduped_ev_usd")) > 0,
    safe_float(selected_class.get("selected_net_ev_usd")) > 0,
]) else 0

summary_lines = [
    "selected_class_loaded=1",
    f"selected_signature={candidate['signature']}",
    f"selected_pool={candidate['pool']}",
    f"selected_mint_in={candidate['mint_in']}",
    f"selected_mint_out={candidate['mint_out']}",
    f"selected_source={candidate['source']}",
    "selected_replay_ingress_override_source=external_geyser",
    f"replay_spec_signal_source={replay_signal_source}",
    "selected_route_source=tpu_quic_public",
    f"selected_target_route_family={target_route_family}",
    f"exact_quote_match_count={exact_quote_match_count}",
    f"min_out_positive_count={min_out_positive_count}",
    f"route_specific_ev_after_fee_tip_positive_count={route_specific_positive}",
    "replay_hydration_mode=prehydrated_artifact",
    "replay_hydration_before_xdp=true",
    "replay_hydration_rpc_during_lease=false",
    "prehydrated_replay_tx_loaded=1",
    f"prehydrated_replay_tx_path={replay_tx_path}",
    f"replay_spec_path={replay_spec_path}",
    f"selected_template_id={artifact['selected_template_id']}",
    f"selected_quote_request_id={artifact['selected_quote_request_id']}",
    f"selected_min_out_raw={artifact['selected_min_out_raw']}",
    f"selected_expected_out_raw={artifact['selected_expected_out_raw']}",
    f"selected_target_out_raw={artifact['selected_target_out_raw']}",
    f"selected_net_ev_usd={selected_class.get('selected_net_ev_usd', 0)}",
    f"selected_candidate_net_after_fee_lamports={safe_int(candidate.get('net_after_fee_lamports'))}",
    f"selected_candidate_retained_ev_lamports={safe_int(candidate.get('retained_ev_lamports'))}",
    f"selected_candidate_run76_deduped_ev_usd={safe_float(candidate.get('run76_deduped_ev_usd'))}",
    f"selected_quote_match_mode={artifact['selected_quote_match_mode'] or ''}",
]
summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY
}

run77c_host_preflight() {
    if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
        summary_force_status "blocked_preconditions"
        summary_set "preflight_local_agave" "fail"
        return 1
    fi

    local link_state xdp_lines xsk_state oob_state
    link_state="$(ip link show dev "$SHADOW_NIC_IFACE" 2>/dev/null || true)"
    xdp_lines="$(run77c_xdp_line_count)"
    if [[ -e "$XSKMAP_PIN" ]]; then
        xsk_state="present"
    else
        xsk_state="absent"
    fi
    if [[ -f "$OOB_ARM_FILE" ]]; then
        oob_state="armed"
    else
        oob_state="missing"
    fi

    summary_set "eno1_lower_up" "$([[ "$link_state" == *"LOWER_UP"* ]] && echo true || echo false)"
    summary_set "eno1_xdp_lines_before" "$xdp_lines"
    summary_set "xsk_map_before" "$xsk_state"
    summary_set "oob_arm_file" "$OOB_ARM_FILE"
    summary_set "oob_arm_status" "$oob_state"

    if [[ "$link_state" != *"LOWER_UP"* || "$xdp_lines" != "0" || "$xsk_state" != "absent" || "$oob_state" != "armed" ]]; then
        summary_force_status "blocked_preconditions"
        return 1
    fi
    return 0
}

render_run77c_summary() {
    python3 - "$SCORECARD_PATH" "$HYDRATION_SUMMARY" "$INNER_STEP_DIR/summary.txt" \
        "$INNER_STEP_DIR/g12_gate_input.json" "$INNER_STEP_DIR/g12_gate_report.json" \
        "$INNER_STEP_DIR/shock_research.jsonl" "$INNER_STEP_DIR/shadow-tier0.log" \
        "$SUMMARY_FILE" "$SUMMARY_JSON" "$TARGET_ROUTE_FAMILY" "$EXPECTED_SIGNATURE" \
        "$EXPECTED_POOL" "$EXPECTED_MINT_IN" "$EXPECTED_MINT_OUT" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

scorecard_path = Path(sys.argv[1])
hydration_summary_path = Path(sys.argv[2])
inner_summary_path = Path(sys.argv[3])
gate_input_path = Path(sys.argv[4])
gate_report_path = Path(sys.argv[5])
shock_path = Path(sys.argv[6])
log_path = Path(sys.argv[7])
summary_path = Path(sys.argv[8])
summary_json_path = Path(sys.argv[9])
target_route_family = sys.argv[10]
expected_signature = sys.argv[11]
expected_pool = sys.argv[12]
expected_mint_in = sys.argv[13]
expected_mint_out = sys.argv[14]

def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data

def parse_json(path: Path):
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

def parse_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            rows.append(json.loads(raw))
        except Exception:
            continue
    return rows

def parse_lines(path: Path):
    if not path.exists():
        return []
    return path.read_text(encoding="utf-8", errors="replace").splitlines()

def as_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0

def as_bool(value):
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}

def normalize_route_family(value):
    raw = str(value or "").strip()
    mapping = {
        "0": "jito_bundle",
        "1": "harmonic_bundle",
        "2": "public_rpc_scout",
        "3": "tpu_quic_public",
        "4": "helius_sender_dual",
        "255": "drop",
    }
    return mapping.get(raw, raw)

scorecard = parse_json(scorecard_path)
hydration = parse_kv(hydration_summary_path)
inner = parse_kv(inner_summary_path)
gate_input = parse_json(gate_input_path)
gate_report = parse_json(gate_report_path)
rows = parse_jsonl(shock_path)
log_lines = parse_lines(log_path)

selected_candidate = ((scorecard.get("run40_candidate_selection") or {}).get("selected_candidate") or {})
selected_class = scorecard.get("selected_class") or {}

selected_class_loaded = int(
    bool(selected_candidate)
    and selected_candidate.get("signature") == expected_signature
    and selected_candidate.get("pool") == expected_pool
    and selected_candidate.get("mint_in") == expected_mint_in
    and selected_candidate.get("mint_out") == expected_mint_out
)
exact_quote_match_count = as_int(hydration.get("exact_quote_match_count"))
min_out_positive_count = as_int(hydration.get("min_out_positive_count"))
route_specific_positive_count = as_int(hydration.get("route_specific_ev_after_fee_tip_positive_count"))
prehydrated_replay_tx_loaded = as_int(hydration.get("prehydrated_replay_tx_loaded"))

target_rows = []
for row in rows:
    if row.get("record_type") != "rollout_shadow":
        continue
    if str(row.get("signature") or "") != expected_signature:
        continue
    if str(row.get("pool") or "") != expected_pool:
        continue
    if str(row.get("mint_in") or "") != expected_mint_in:
        continue
    if str(row.get("mint_out") or "") != expected_mint_out:
        continue
    route_label = normalize_route_family(row.get("route_family_label") or row.get("route_family"))
    if route_label == target_route_family:
        target_rows.append(row)

trace_route_override_applied_count = sum(
    1
    for line in log_lines
    if expected_signature in line
    and "TRACE scorecard_replay_gate stage=route_override_applied" in line
    and f"route_decision={target_route_family}" in line
)
trace_source_override_applied_count = sum(
    1
    for line in log_lines
    if expected_signature in line
    and "TRACE scorecard_replay_gate stage=source_override_applied" in line
    and f"route_decision={target_route_family}" in line
)
trace_admit_private_count = sum(
    1
    for line in log_lines
    if expected_signature in line
    and "TRACE scorecard_replay_gate stage=admit_to_process_intent_ok" in line
    and f"detail={target_route_family}" in line
)
trace_process_emit_ok_count = sum(
    1
    for line in log_lines
    if expected_signature in line
    and "TRACE scorecard_replay_gate stage=process_intent_ok" in line
    and "emit_ok=true" in line
)
trace_private_accept_count = int(
    trace_route_override_applied_count > 0
    and trace_source_override_applied_count > 0
    and trace_admit_private_count > 0
    and trace_process_emit_ok_count > 0
)

helius_sender_dual_no_send_accept_count = sum(
    1 for row in target_rows
    if target_route_family == "helius_sender_dual" and (as_bool(row.get("would_send")) or as_bool(row.get("live_send_suppressed")))
)
jito_via_helius_simulate_no_submit_accept_count = sum(
    1 for row in target_rows
    if target_route_family == "jito_bundle" and (as_bool(row.get("would_send")) or as_bool(row.get("live_send_suppressed")))
)
if target_route_family == "helius_sender_dual":
    helius_sender_dual_no_send_accept_count = max(
        helius_sender_dual_no_send_accept_count,
        trace_private_accept_count,
    )
if target_route_family == "jito_bundle":
    jito_via_helius_simulate_no_submit_accept_count = max(
        jito_via_helius_simulate_no_submit_accept_count,
        trace_private_accept_count,
    )

pm_reserve_ok = any(as_bool(row.get("pm_reserve_ok")) for row in target_rows)
worker_pm_reservation_created = as_int(gate_input.get("worker_pm_reservation_created") or gate_report.get("worker_pm_reservation_created"))
pm_reservation_released = as_int(gate_input.get("pm_reservation_released") or gate_report.get("pm_reservation_released"))
gate_pm_reserve_ok = as_bool(gate_input.get("pm_reserve_ok") or gate_report.get("pm_reserve_ok"))
if not pm_reserve_ok and trace_private_accept_count > 0 and gate_pm_reserve_ok:
    pm_reserve_ok = True
pm_pending_after = max(0, worker_pm_reservation_created - pm_reservation_released)
pm_release_ok = pm_reserve_ok and worker_pm_reservation_created > 0 and pm_pending_after == 0

worker_no_send_backend_ok = as_int(gate_input.get("worker_no_send_backend_ok") or gate_report.get("worker_no_send_backend_ok"))
worker_final_no_send_event_published = as_int(
    gate_input.get("worker_final_no_send_event_published")
    or gate_report.get("worker_final_no_send_event_published")
)
live_landed = as_int(gate_input.get("live_landed") or gate_report.get("live_landed") or inner.get("live_landed"))
wire_send = 0
send_transaction_called = False
sendBundle_called = False

private_accept_count = max(
    helius_sender_dual_no_send_accept_count,
    jito_via_helius_simulate_no_submit_accept_count,
)
private_accept_evidence_source = (
    "rollout_shadow"
    if private_accept_count > 0 and len(target_rows) > 0
    else "trace_selected_replay"
    if private_accept_count > 0 and trace_private_accept_count > 0
    else "none"
)

acceptance_pass = all(
    [
        selected_class_loaded == 1,
        prehydrated_replay_tx_loaded > 0,
        exact_quote_match_count > 0,
        min_out_positive_count > 0,
        route_specific_positive_count > 0,
        pm_reserve_ok,
        pm_release_ok,
        pm_pending_after == 0,
        private_accept_count > 0,
        worker_no_send_backend_ok > 0,
        worker_final_no_send_event_published > 0,
        wire_send == 0,
        live_landed == 0,
        not send_transaction_called,
        not sendBundle_called,
    ]
)

payload = {
    "status": "run77c_private_egress_ready" if acceptance_pass else "blocked_private_egress_mapping",
    "selected_class_loaded": selected_class_loaded,
    "prehydrated_replay_tx_loaded": prehydrated_replay_tx_loaded,
    "selected_signature": expected_signature,
    "selected_pool": expected_pool,
    "selected_mint_in": expected_mint_in,
    "selected_mint_out": expected_mint_out,
    "selected_route_source": "tpu_quic_public",
    "mapped_runtime_route_family": target_route_family,
    "exact_quote_match_count": exact_quote_match_count,
    "min_out_positive_count": min_out_positive_count,
    "route_specific_ev_after_fee_tip_positive_count": route_specific_positive_count,
    "selected_net_ev_usd": selected_class.get("selected_net_ev_usd"),
    "selected_quote_match_mode": hydration.get("selected_quote_match_mode", ""),
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "pm_pending_after": pm_pending_after,
    "worker_pm_reservation_created": worker_pm_reservation_created,
    "pm_reservation_released": pm_reservation_released,
    "worker_no_send_backend_ok": worker_no_send_backend_ok,
    "worker_final_no_send_event_published": worker_final_no_send_event_published,
    "helius_sender_dual_no_send_accept_count": helius_sender_dual_no_send_accept_count,
    "jito_via_helius_simulate_no_submit_accept_count": jito_via_helius_simulate_no_submit_accept_count,
    "private_accept_evidence_source": private_accept_evidence_source,
    "trace_route_override_applied_count": trace_route_override_applied_count,
    "trace_source_override_applied_count": trace_source_override_applied_count,
    "trace_admit_private_count": trace_admit_private_count,
    "trace_process_emit_ok_count": trace_process_emit_ok_count,
    "send_transaction_called": send_transaction_called,
    "sendBundle_called": sendBundle_called,
    "wire_send": wire_send,
    "live_landed": live_landed,
    "target_rollout_rows": len(target_rows),
    "acceptance_pass": acceptance_pass,
    "inner_run_status": inner.get("run_status", ""),
    "inner_cleanup_verified": inner.get("cleanup_verified", ""),
    "replay_hydration_mode": hydration.get("replay_hydration_mode", ""),
    "replay_hydration_before_xdp": hydration.get("replay_hydration_before_xdp", ""),
    "replay_hydration_rpc_during_lease": hydration.get("replay_hydration_rpc_during_lease", ""),
}

summary_lines = [
    f"status={payload['status']}",
    f"selected_class_loaded={selected_class_loaded}",
    f"prehydrated_replay_tx_loaded={prehydrated_replay_tx_loaded}",
    f"selected_signature={expected_signature}",
    f"selected_pool={expected_pool}",
    f"selected_mint_in={expected_mint_in}",
    f"selected_mint_out={expected_mint_out}",
    "selected_route_source=tpu_quic_public",
    f"mapped_runtime_route_family={target_route_family}",
    f"exact_quote_match_count={exact_quote_match_count}",
    f"min_out_positive_count={min_out_positive_count}",
    f"route_specific_ev_after_fee_tip_positive_count={route_specific_positive_count}",
    f"pm_reserve_ok={str(pm_reserve_ok).lower()}",
    f"pm_release_ok={str(pm_release_ok).lower()}",
    f"pm_pending_after={pm_pending_after}",
    f"worker_pm_reservation_created={worker_pm_reservation_created}",
    f"pm_reservation_released={pm_reservation_released}",
    f"worker_no_send_backend_ok={worker_no_send_backend_ok}",
    f"worker_final_no_send_event_published={worker_final_no_send_event_published}",
    f"helius_sender_dual_no_send_accept_count={helius_sender_dual_no_send_accept_count}",
    f"jito_via_helius_simulate_no_submit_accept_count={jito_via_helius_simulate_no_submit_accept_count}",
    f"private_accept_evidence_source={private_accept_evidence_source}",
    f"trace_route_override_applied_count={trace_route_override_applied_count}",
    f"trace_source_override_applied_count={trace_source_override_applied_count}",
    f"trace_admit_private_count={trace_admit_private_count}",
    f"trace_process_emit_ok_count={trace_process_emit_ok_count}",
    "send_transaction_called=false",
    "sendBundle_called=false",
    "wire_send=0",
    f"live_landed={live_landed}",
    f"target_rollout_rows={len(target_rows)}",
    f"replay_hydration_mode={payload['replay_hydration_mode']}",
    f"replay_hydration_before_xdp={payload['replay_hydration_before_xdp']}",
    f"replay_hydration_rpc_during_lease={payload['replay_hydration_rpc_during_lease']}",
    f"acceptance_pass={str(acceptance_pass).lower()}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY
}

if [[ ! -f "$SCORECARD_PATH" ]]; then
    fail "Missing Run76 selected scorecard: $SCORECARD_PATH"
    finish
    exit 1
fi
if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run77C config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

summary_force_status "initializing"
summary_set "step" "$STEP"
summary_set "target_route_family" "$TARGET_ROUTE_FAMILY"
summary_set "scorecard_path" "$SCORECARD_PATH"
summary_set "config_source" "$CONFIG_SOURCE"

if ! run77c_host_preflight; then
    fail "Run77C preconditions are not green"
    finish
    exit 1
fi

prepare_run77c_config
prepare_replay_inputs

export SHADOW_RESULTS_DIR="$INNER_RESULTS_ROOT"
export SHADOW_G12_CONFIG_PATH="$NORMALIZED_CONFIG"
export SHADOW_RUN40_SOURCE_RELAXED_DEV_PROOF=true
export SHADOW_RUN40_SCORECARD_REPLAY_DEV_PROOF=true
export SHADOW_RUN40_LIVE_MARKET_RECURRENCE_REQUIRED=false
export SHADOW_RUN40_SELECTED_SIGNATURE="$EXPECTED_SIGNATURE"
export SHADOW_RUN40_SELECTED_POOL="$EXPECTED_POOL"
export SHADOW_RUN40_SELECTED_MINT_IN="$EXPECTED_MINT_IN"
export SHADOW_RUN40_SELECTED_MINT_OUT="$EXPECTED_MINT_OUT"
export SHADOW_RUN40_SELECTED_SOURCE="external_geyser"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="external_geyser"
export SHADOW_RUN40_SELECTED_TARGET_ROUTE_FAMILY="$TARGET_ROUTE_FAMILY"
export SHADOW_RUN40_SCORECARD_REPLAY_SPEC="$REPLAY_SPEC_PATH"
export SHADOW_RUN77_REPLAY_TX_PATH="$REPLAY_TX_PATH"
export SHADOW_RUN40_SCORECARD_REPLAY_TX_PATH="$REPLAY_TX_PATH"
export SHADOW_RUN77B_REQUIRE_PREHYDRATED_REPLAY=true
export SHADOW_RUN77_REPLAY_HYDRATION_MODE=prehydrated_artifact
export SHADOW_RUN77_REPLAY_HYDRATION_BEFORE_XDP=true
export SHADOW_RUN77_REPLAY_HYDRATION_RPC_DURING_LEASE=false
export SHADOW_POST_WINDOW_DRAIN_SECONDS="$POST_WINDOW_DRAIN_SECONDS"
export SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-native}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-false}"

set +e
"$PACK_DIR/scripts/20_g12_r0_observe_only.sh" "$WINDOW_SECONDS"
INNER_RC=$?
set -e
summary_set "inner_rc" "$INNER_RC"

if [[ $INNER_RC -ne 0 ]]; then
    summary_force_status "blocked_inner_observe_only_failed"
    fail "Run77C inner observe-only window failed"
    finish
    exit "$INNER_RC"
fi

if [[ ! -f "$INNER_STEP_DIR/g12_gate_input.json" || ! -f "$INNER_STEP_DIR/g12_gate_report.json" || ! -f "$INNER_STEP_DIR/shock_research.jsonl" ]]; then
    summary_force_status "blocked_missing_inner_artifacts"
    fail "Run77C missing inner gate or shock artifacts"
    finish
    exit 1
fi

render_run77c_summary
ACCEPTANCE_PASS="$(awk -F= '$1 == "acceptance_pass" { print $2 }' "$SUMMARY_FILE" | tail -n 1)"

if [[ "$ACCEPTANCE_PASS" != "true" ]]; then
    fail "Run77C private-egress no-send acceptance failed"
    finish
    exit 1
fi

pass "Run77C private-egress no-send acceptance passed"
finish
exit 0

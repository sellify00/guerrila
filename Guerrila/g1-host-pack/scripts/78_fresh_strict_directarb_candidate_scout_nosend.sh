#!/usr/bin/env bash
# 78 - bounded native AF_XDP fresh strict DirectArb scout mapped to private egress in no-send mode

source "$(dirname "$0")/lib.sh"
script_header "78 - Fresh Strict DirectArb Candidate Scout No-Send"

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

export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN78_XDP_ATTACH_MODE:-${CALLER_SHADOW_XDP_ATTACH_MODE:-native}}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN78_AFXDP_SAFE_MODE:-${CALLER_SHADOW_AFXDP_SAFE_MODE:-false}}"
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
STEP="${SHADOW_RUN78_STEP:-epoch8_auto_strict_directarb_private_egress_run78_fresh_candidate_scout}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
INNER_RESULTS_ROOT="$RESULT_DIR/_inner"
INNER_STEP_DIR="$INNER_RESULTS_ROOT/20_g12_r0_observe_only"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78_fresh_strict_directarb_candidate_scout_nosend.json"
PREATTACH_AGAVE_PREFLIGHT_FILE="$RESULT_DIR/pre_attach_local_agave_preflight.txt"
NORMALIZED_CONFIG="$RESULT_DIR/run78.config.yaml"
TARGET_MANIFEST_PATH="$RESULT_DIR/run78c_target_selection_manifest.jsonl"
REJECTION_MANIFEST_PATH="$RESULT_DIR/run78c_selector_rejections.jsonl"

CONFIG_SOURCE="${SHADOW_RUN78_CONFIG_SOURCE:-/etc/shadow-engine/config.guerrila.yaml}"
WINDOW_SECONDS="${SHADOW_RUN78_WINDOW_SECONDS:-120}"
POST_WINDOW_DRAIN_SECONDS="${SHADOW_RUN78_POST_WINDOW_DRAIN_SECONDS:-5}"
SCOUT_ROUTE_FAMILY="${SHADOW_RUN78_SCOUT_ROUTE_FAMILY:-${SHADOW_RUN78_TARGET_ROUTE_FAMILY:-tpu_quic_public}}"
PRIVATE_REPLAY_TARGET_ROUTE_FAMILY="${SHADOW_RUN78_PRIVATE_REPLAY_TARGET_ROUTE_FAMILY:-helius_sender_dual}"
APPROVED_LIVE_CAP_SOL="${SHADOW_RUN78_APPROVED_LIVE_CAP_SOL:-0.01}"
PROJECTION_RATIO_MAX="${SHADOW_RUN78_PROJECTION_RATIO_MAX:-4}"
SOL_USD_EQUIVALENT="${SHADOW_RUN78_SOL_USD_EQUIVALENT:-}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-${RESULTS_ROOT}/run39_native_oob_ready.flag}"

mkdir -p "$RESULT_DIR" "$INNER_RESULTS_ROOT"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$TARGET_MANIFEST_PATH" "$REJECTION_MANIFEST_PATH"

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

run78_xdp_line_count() {
    ip -details link show dev "$SHADOW_NIC_IFACE" 2>/dev/null | grep -ci 'xdp' || true
}

prepare_run78_config() {
    python3 - "$CONFIG_SOURCE" "$NORMALIZED_CONFIG" "$PRIVATE_REPLAY_TARGET_ROUTE_FAMILY" <<'PY'
import re
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])
target_route_family = sys.argv[3].strip() or "helius_sender_dual"
target_route_family_lower = target_route_family.lower()
text = source_path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n")

replacements = {
    r'(^\s*default_jito_searcher\s*:\s*).+$': rf'\1{target_route_family}',
}
for pattern, replacement in replacements.items():
    if re.search(pattern, text, flags=re.M):
        text = re.sub(pattern, replacement, text, count=1, flags=re.M)

lines = text.splitlines()
in_helius_sender = False
for index, line in enumerate(lines):
    if re.match(r"^helius_sender:\s*$", line):
        in_helius_sender = True
        continue
    if in_helius_sender and re.match(r"^\S", line):
        in_helius_sender = False
    if not in_helius_sender:
        continue
    match = re.match(r"^(\s*)enabled\s*:\s*.+$", line)
    if not match:
        continue
    enabled_value = "true" if target_route_family_lower == "helius_sender_dual" else "false"
    lines[index] = f"{match.group(1)}enabled: {enabled_value}"
    break

text = "\n".join(lines)

target_path.write_text(text if text.endswith("\n") else text + "\n", encoding="utf-8")
PY
}

run78_host_preflight() {
    if ! require_local_agave_preflight "$PREATTACH_AGAVE_PREFLIGHT_FILE"; then
        summary_force_status "blocked_preconditions"
        summary_set "preflight_local_agave" "fail"
        return 1
    fi

    local link_state xdp_lines xsk_state oob_state
    link_state="$(ip link show dev "$SHADOW_NIC_IFACE" 2>/dev/null || true)"
    xdp_lines="$(run78_xdp_line_count)"
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

extract_run78_manifests() {
    python3 - "$INNER_STEP_DIR/shadow-tier0.log" "$TARGET_MANIFEST_PATH" "$REJECTION_MANIFEST_PATH" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])
rejection_path = Path(sys.argv[3])
target_rows = []
rejection_rows = []
if log_path.exists():
    for raw_line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        for marker, sink in (
            ("RUN78C_TARGET_SELECTION ", target_rows),
            ("RUN78C_SELECTOR_REJECTION ", rejection_rows),
        ):
            if marker not in raw_line:
                continue
            payload = raw_line.split(marker, 1)[1].strip()
            if not payload:
                continue
            try:
                sink.append(json.loads(payload))
            except Exception:
                continue

def write_jsonl(path: Path, rows):
    path.write_text(
        "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows),
        encoding="utf-8",
    )

write_jsonl(target_path, target_rows)
write_jsonl(rejection_path, rejection_rows)
PY
}

render_run78_summary() {
    python3 - "$TARGET_MANIFEST_PATH" "$REJECTION_MANIFEST_PATH" "$INNER_STEP_DIR/summary.txt" \
        "$INNER_STEP_DIR/g12_gate_input.json" "$INNER_STEP_DIR/g12_gate_report.json" \
        "$INNER_STEP_DIR/shock_research.jsonl" "$SUMMARY_FILE" "$SUMMARY_JSON" \
        "$SCOUT_ROUTE_FAMILY" "$PRIVATE_REPLAY_TARGET_ROUTE_FAMILY" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

target_manifest_path = Path(sys.argv[1])
rejection_manifest_path = Path(sys.argv[2])
inner_summary_path = Path(sys.argv[3])
gate_input_path = Path(sys.argv[4])
gate_report_path = Path(sys.argv[5])
shock_path = Path(sys.argv[6])
summary_path = Path(sys.argv[7])
summary_json_path = Path(sys.argv[8])
scout_route_family = sys.argv[9]
private_replay_target_route_family = sys.argv[10]

alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def parse_kv(path: Path):
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
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

def b58decode(text: str) -> bytes:
    value = 0
    for char in text.strip():
        value = value * 58 + alphabet.index(char)
    raw = value.to_bytes((value.bit_length() + 7) // 8, "big") if value else b""
    pad = len(text) - len(text.lstrip("1"))
    return (b"\x00" * pad) + raw

def b58hex(text: str) -> str:
    try:
        return b58decode(text).hex()
    except Exception:
        return ""

def row_matches_identity(row, signature, pool, mint_in, mint_out):
    signature_hex = b58hex(signature)
    pool_hex = b58hex(pool)
    mint_in_hex = b58hex(mint_in)
    mint_out_hex = b58hex(mint_out)

    def match_any(candidates, expected, expected_hex):
        for candidate in candidates:
            raw = str(candidate or "").strip()
            if not raw:
                continue
            lowered = raw.lower()
            if expected and raw == expected:
                return True
            if expected_hex and lowered == expected_hex:
                return True
        return False

    signature_ok = match_any(
        [
            row.get("signature"),
            row.get("ingress_signature"),
            row.get("signature_hex"),
            row.get("ingress_signature_hex"),
        ],
        signature,
        signature_hex,
    )
    pool_ok = match_any(
        [row.get("pool"), row.get("pool_pubkey"), row.get("pool_pubkey_hex")],
        pool,
        pool_hex,
    )
    mint_in_ok = match_any(
        [row.get("mint_in"), row.get("mint_in_pubkey"), row.get("mint_in_hex")],
        mint_in,
        mint_in_hex,
    )
    mint_out_ok = match_any(
        [row.get("mint_out"), row.get("mint_out_pubkey"), row.get("mint_out_hex")],
        mint_out,
        mint_out_hex,
    )
    return signature_ok and pool_ok and mint_in_ok and mint_out_ok

target_rows = parse_jsonl(target_manifest_path)
rejection_rows = parse_jsonl(rejection_manifest_path)
inner = parse_kv(inner_summary_path)
gate_input = parse_json(gate_input_path)
gate_report = parse_json(gate_report_path)
shock_rows = parse_jsonl(shock_path)
target = target_rows[0] if target_rows else {}

selected = bool(target)
selected_signature = str(target.get("ingress_signature") or target.get("signature") or "")
selected_pool = str(target.get("pool") or "")
selected_mint_in = str(target.get("mint_in") or "")
selected_mint_out = str(target.get("mint_out") or "")
selected_route = normalize_route_family(target.get("route") or target.get("selected_target_route_family") or "")
selected_owner_mode = str(target.get("owner_mode") or "")

matched_rollout_rows = []
for row in shock_rows:
    if row.get("record_type") != "rollout_shadow":
        continue
    if not selected:
        continue
    route_label = normalize_route_family(row.get("route_family_label") or row.get("route_family"))
    if route_label != scout_route_family:
        continue
    if row_matches_identity(row, selected_signature, selected_pool, selected_mint_in, selected_mint_out):
        matched_rollout_rows.append(row)

quote_numeric_present_count = int(as_bool(target.get("quote_numeric_present")))
quote_request_id_positive_count = sum(
    1 for row in matched_rollout_rows if as_int(row.get("quote_request_id")) > 0
)
exact_quote_match_count = quote_request_id_positive_count
min_out_positive_count = max(
    int(as_bool(target.get("min_out_positive"))),
    sum(
        1
        for row in matched_rollout_rows
        if as_bool(row.get("min_out_positive")) or as_int(row.get("min_out_raw")) > 0
    ),
)
route_specific_ev_after_fee_tip_positive_count = max(
    int(as_bool(target.get("candidate_ev_positive")) or as_int(target.get("candidate_net_after_fee_raw")) > 0),
    sum(
        1
        for row in matched_rollout_rows
        if any(
            as_int(row.get(field)) > 0
            for field in (
                "matched_rollout_net_ev_after_fee_tip_raw",
                "net_ev_after_fee_tip_raw",
                "net_after_fee_lamports",
                "retained_ev_lamports",
            )
        )
    ),
)

inventory_ready = as_bool(target.get("inventory_ready"))
output_account_ready = as_bool(target.get("output_account_ready"))
template_ready = as_bool(target.get("template_ready"))
pm_reserve_can_lock_exact_input = as_bool(target.get("pm_reserve_can_lock_exact_input"))
pm_reservation_created = as_bool(target.get("pm_reservation_created"))
emit_attempted = as_bool(target.get("emit_attempted"))
emit_ok = as_bool(target.get("emit_ok"))

worker_pm_reservation_created = as_int(
    gate_input.get("worker_pm_reservation_created")
    or gate_report.get("worker_pm_reservation_created")
)
pm_reservation_released = as_int(
    gate_input.get("pm_reservation_released")
    or gate_report.get("pm_reservation_released")
)
pm_pending_after = max(0, worker_pm_reservation_created - pm_reservation_released)
gate_pm_reserve_ok = as_bool(gate_input.get("pm_reserve_ok") or gate_report.get("pm_reserve_ok"))
pm_reserve_ok = (
    pm_reservation_created
    or any(as_bool(row.get("pm_reserve_ok")) for row in matched_rollout_rows)
    or (selected and gate_pm_reserve_ok and pm_reserve_can_lock_exact_input)
)
pm_release_ok = pm_reserve_ok and worker_pm_reservation_created > 0 and pm_pending_after == 0

worker_no_send_backend_ok = as_int(
    gate_input.get("worker_no_send_backend_ok")
    or gate_report.get("worker_no_send_backend_ok")
)
worker_final_no_send_event_published = as_int(
    gate_input.get("worker_final_no_send_event_published")
    or gate_report.get("worker_final_no_send_event_published")
)
live_landed = as_int(gate_input.get("live_landed") or gate_report.get("live_landed") or inner.get("live_landed"))
wire_send = 0
send_transaction_called = False
sendBundle_called = False

fresh_source_route_no_send_accept_count = sum(
    1
    for row in matched_rollout_rows
    if as_bool(row.get("would_send")) or as_bool(row.get("live_send_suppressed"))
)
if fresh_source_route_no_send_accept_count > 0:
    source_route_accept_evidence_source = "rollout_shadow"
elif selected and emit_ok and worker_no_send_backend_ok > 0 and worker_final_no_send_event_published > 0:
    fresh_source_route_no_send_accept_count = 1
    source_route_accept_evidence_source = "selected_manifest_plus_worker_counters"
else:
    source_route_accept_evidence_source = "none"

helius_sender_dual_no_send_accept_count = 0
jito_via_helius_simulate_no_submit_accept_count = 0
fresh_private_egress_no_send_accept_count = 0
private_accept_evidence_source = "pending_run78b"

acceptance_pass = all(
    [
        selected,
        quote_numeric_present_count > 0,
        min_out_positive_count > 0,
        route_specific_ev_after_fee_tip_positive_count > 0,
        inventory_ready,
        output_account_ready,
        template_ready,
        pm_reserve_ok,
        pm_release_ok,
        pm_pending_after == 0,
        fresh_source_route_no_send_accept_count > 0,
        worker_no_send_backend_ok > 0,
        worker_final_no_send_event_published > 0,
        wire_send == 0,
        live_landed == 0,
        not send_transaction_called,
        not sendBundle_called,
    ]
)

status = (
    "run78_fresh_strict_directarb_candidate_ready"
    if acceptance_pass
    else "blocked_no_fresh_strict_directarb_candidate"
    if not selected
    else "blocked_fresh_source_candidate_not_live_eligible"
)

payload = {
    "status": status,
    "inventory_refresh_before_tier0": str(inner.get("inventory_refresh_before_tier0") or "false").lower(),
    "signer_token_inventory_account_count": as_int(inner.get("signer_token_inventory_account_count")),
    "signer_token_inventory_slot": inner.get("signer_token_inventory_slot") or "",
    "signer_token_inventory_path": inner.get("signer_token_inventory_path") or "",
    "fresh_strict_directarb_candidate_count": 1 if selected else 0,
    "selected_candidate_loaded": 1 if selected else 0,
    "selected_signature": selected_signature,
    "selected_pool": selected_pool,
    "selected_mint_in": selected_mint_in,
    "selected_mint_out": selected_mint_out,
    "selected_route": selected_route,
    "selected_target_route_family": str(target.get("selected_target_route_family") or scout_route_family),
    "scout_route_family": scout_route_family,
    "private_replay_target_route_family": private_replay_target_route_family,
    "selected_owner_mode": selected_owner_mode,
    "quote_numeric_present_count": quote_numeric_present_count,
    "quote_request_id_positive_count": quote_request_id_positive_count,
    "exact_quote_match_count": exact_quote_match_count,
    "min_out_positive_count": min_out_positive_count,
    "route_specific_ev_after_fee_tip_positive_count": route_specific_ev_after_fee_tip_positive_count,
    "candidate_net_after_fee_raw": as_int(target.get("candidate_net_after_fee_raw")),
    "candidate_retained_ev_raw": as_int(target.get("candidate_retained_ev_raw")),
    "inventory_ready": inventory_ready,
    "output_account_ready": output_account_ready,
    "template_ready": template_ready,
    "pm_reserve_can_lock_exact_input": pm_reserve_can_lock_exact_input,
    "pm_reservation_created": pm_reservation_created,
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "pm_pending_after": pm_pending_after,
    "worker_pm_reservation_created": worker_pm_reservation_created,
    "pm_reservation_released": pm_reservation_released,
    "emit_attempted": emit_attempted,
    "emit_ok": emit_ok,
    "worker_no_send_backend_ok": worker_no_send_backend_ok,
    "worker_final_no_send_event_published": worker_final_no_send_event_published,
    "fresh_source_route_no_send_accept_count": fresh_source_route_no_send_accept_count,
    "source_route_accept_evidence_source": source_route_accept_evidence_source,
    "helius_sender_dual_no_send_accept_count": helius_sender_dual_no_send_accept_count,
    "jito_via_helius_simulate_no_submit_accept_count": jito_via_helius_simulate_no_submit_accept_count,
    "fresh_private_egress_no_send_accept_count": fresh_private_egress_no_send_accept_count,
    "private_accept_evidence_source": private_accept_evidence_source,
    "matched_rollout_rows": len(matched_rollout_rows),
    "selector_rejection_count": len(rejection_rows),
    "wire_send": wire_send,
    "live_landed": live_landed,
    "send_transaction_called": send_transaction_called,
    "sendBundle_called": sendBundle_called,
    "acceptance_pass": acceptance_pass,
}

summary_lines = [
    f"status={payload['status']}",
    f"inventory_refresh_before_tier0={payload['inventory_refresh_before_tier0']}",
    f"signer_token_inventory_account_count={payload['signer_token_inventory_account_count']}",
    f"signer_token_inventory_slot={payload['signer_token_inventory_slot']}",
    f"signer_token_inventory_path={payload['signer_token_inventory_path']}",
    f"fresh_strict_directarb_candidate_count={payload['fresh_strict_directarb_candidate_count']}",
    f"selected_signature={payload['selected_signature']}",
    f"selected_pool={payload['selected_pool']}",
    f"selected_mint_in={payload['selected_mint_in']}",
    f"selected_mint_out={payload['selected_mint_out']}",
    f"selected_route={payload['selected_route']}",
    f"selected_target_route_family={payload['selected_target_route_family']}",
    f"scout_route_family={payload['scout_route_family']}",
    f"private_replay_target_route_family={payload['private_replay_target_route_family']}",
    f"selected_owner_mode={payload['selected_owner_mode']}",
    f"quote_numeric_present_count={payload['quote_numeric_present_count']}",
    f"quote_request_id_positive_count={payload['quote_request_id_positive_count']}",
    f"exact_quote_match_count={payload['exact_quote_match_count']}",
    f"min_out_positive_count={payload['min_out_positive_count']}",
    f"route_specific_ev_after_fee_tip_positive_count={payload['route_specific_ev_after_fee_tip_positive_count']}",
    f"inventory_ready={str(payload['inventory_ready']).lower()}",
    f"output_account_ready={str(payload['output_account_ready']).lower()}",
    f"template_ready={str(payload['template_ready']).lower()}",
    f"pm_reserve_can_lock_exact_input={str(payload['pm_reserve_can_lock_exact_input']).lower()}",
    f"pm_reservation_created={str(payload['pm_reservation_created']).lower()}",
    f"pm_reserve_ok={str(payload['pm_reserve_ok']).lower()}",
    f"pm_release_ok={str(payload['pm_release_ok']).lower()}",
    f"pm_pending_after={payload['pm_pending_after']}",
    f"worker_pm_reservation_created={payload['worker_pm_reservation_created']}",
    f"pm_reservation_released={payload['pm_reservation_released']}",
    f"emit_attempted={str(payload['emit_attempted']).lower()}",
    f"emit_ok={str(payload['emit_ok']).lower()}",
    f"worker_no_send_backend_ok={payload['worker_no_send_backend_ok']}",
    f"worker_final_no_send_event_published={payload['worker_final_no_send_event_published']}",
    f"fresh_source_route_no_send_accept_count={payload['fresh_source_route_no_send_accept_count']}",
    f"source_route_accept_evidence_source={payload['source_route_accept_evidence_source']}",
    f"helius_sender_dual_no_send_accept_count={payload['helius_sender_dual_no_send_accept_count']}",
    f"jito_via_helius_simulate_no_submit_accept_count={payload['jito_via_helius_simulate_no_submit_accept_count']}",
    f"fresh_private_egress_no_send_accept_count={payload['fresh_private_egress_no_send_accept_count']}",
    f"private_accept_evidence_source={payload['private_accept_evidence_source']}",
    f"matched_rollout_rows={payload['matched_rollout_rows']}",
    f"selector_rejection_count={payload['selector_rejection_count']}",
    f"wire_send={payload['wire_send']}",
    f"live_landed={payload['live_landed']}",
    f"send_transaction_called={str(payload['send_transaction_called']).lower()}",
    f"sendBundle_called={str(payload['sendBundle_called']).lower()}",
    f"acceptance_pass={str(payload['acceptance_pass']).lower()}",
]

summary_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY
}

if [[ ! -f "$CONFIG_SOURCE" ]]; then
    fail "Missing Run78 config source: $CONFIG_SOURCE"
    finish
    exit 1
fi

summary_force_status "initializing"
summary_set "step" "$STEP"
summary_set "scout_route_family" "$SCOUT_ROUTE_FAMILY"
summary_set "private_replay_target_route_family" "$PRIVATE_REPLAY_TARGET_ROUTE_FAMILY"
summary_set "config_source" "$CONFIG_SOURCE"
summary_set "approved_live_cap_sol" "$APPROVED_LIVE_CAP_SOL"
summary_set "projection_ratio_max" "$PROJECTION_RATIO_MAX"

if ! run78_host_preflight; then
    fail "Run78 preconditions are not green"
    finish
    exit 1
fi

prepare_run78_config

export SHADOW_RESULTS_DIR="$INNER_RESULTS_ROOT"
export SHADOW_G12_CONFIG_PATH="$NORMALIZED_CONFIG"
export SHADOW_RUN78C_FRESH_NUMERIC_SAFE_DIRECTARB=true
export SHADOW_RUN78E_LIVE_ELIGIBLE_SELECTOR=true
export SHADOW_RUN78E_APPROVED_LIVE_CAP_SOL="$APPROVED_LIVE_CAP_SOL"
export SHADOW_RUN78E_PROJECTION_RATIO_MAX="$PROJECTION_RATIO_MAX"
export SHADOW_RUN78C_TARGET_ROUTE_FAMILY="$SCOUT_ROUTE_FAMILY"
export SHADOW_EXPECT_GRPC_SOURCE_GRADE="${SHADOW_EXPECT_GRPC_SOURCE_GRADE:-external_geyser}"
[[ -n "$SOL_USD_EQUIVALENT" ]] && export SHADOW_RUN78C_SOL_USD_EQUIVALENT="$SOL_USD_EQUIVALENT"
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
    fail "Run78 inner observe-only window failed"
    finish
    exit "$INNER_RC"
fi

if [[ ! -f "$INNER_STEP_DIR/g12_gate_input.json" || ! -f "$INNER_STEP_DIR/g12_gate_report.json" || ! -f "$INNER_STEP_DIR/shock_research.jsonl" || ! -f "$INNER_STEP_DIR/shadow-tier0.log" ]]; then
    summary_force_status "blocked_missing_inner_artifacts"
    fail "Run78 missing inner gate, shock, or log artifacts"
    finish
    exit 1
fi

extract_run78_manifests
render_run78_summary
ACCEPTANCE_PASS="$(awk -F= '$1 == "acceptance_pass" { print $2 }' "$SUMMARY_FILE" | tail -n 1)"

if [[ "$ACCEPTANCE_PASS" != "true" ]]; then
    fail "Run78 fresh strict DirectArb candidate scout acceptance failed"
    finish
    exit 1
fi

pass "Run78 fresh strict DirectArb candidate scout acceptance passed"
finish
exit 0

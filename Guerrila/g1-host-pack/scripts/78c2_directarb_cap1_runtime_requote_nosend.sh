#!/usr/bin/env bash
# 78C2 - temporary 1 SOL no-send DirectArb runtime quote/cap proof.

source "$(dirname "$0")/lib.sh"
script_header "78C2 - DirectArb Cap-1 Runtime Quote No-Send"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"
CALLER_SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL-}"
CALLER_HELIUS_SENDER_URL="${HELIUS_SENDER_URL-}"
CALLER_SHADOW_HELIUS_SENDER_ENDPOINT="${SHADOW_HELIUS_SENDER_ENDPOINT-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

RESULTS_ROOT="${CALLER_SHADOW_RESULTS_DIR:-${SHADOW_RESULTS_DIR:-$PACK_DIR/results}}"
EPOCH_TAG="${SHADOW_RUN78C2_EPOCH_TAG:-epoch_directarb_cap1_runtime_requote_nosend_$(date -u +%Y%m%d_%H%M%S)}"
RESULT_DIR="$RESULTS_ROOT/$EPOCH_TAG"
STEP="${SHADOW_RUN78C2_STEP:-78c2_directarb_cap1_runtime_requote_nosend}"
STEP_DIR="$RESULT_DIR/$STEP"
SUMMARY_FILE="$STEP_DIR/summary.txt"
SUMMARY_JSON="$STEP_DIR/run78c2_directarb_cap1_runtime_requote_nosend.json"
RUN78_STEP="78c2_run78_cap1_fresh_runtime_quote_nosend"
RUN78B_STEP="78c2_run78b_cap1_private_egress_gate"
CAP_REVIEW_SOL="${SHADOW_RUN78C2_CAP_REVIEW_SOL:-1}"
WINDOW_SECONDS="${SHADOW_RUN78C2_WINDOW_SECONDS:-300}"
POST_WINDOW_DRAIN_SECONDS="${SHADOW_RUN78C2_POST_WINDOW_DRAIN_SECONDS:-5}"
TYO_SENDER_URL="${SHADOW_RUN78C2_HELIUS_SENDER_URL:-${CALLER_SHADOW_HELIUS_SENDER_URL:-${CALLER_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}}}"
OOB_ARM_FILE="${SHADOW_RUN78C2_OOB_ARM_FILE:-${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-/dev/results/run39_native_oob_ready.flag}}"

mkdir -p "$STEP_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

kv_get() {
    local path="$1"
    local key="$2"
    awk -F= -v key="$key" '$1 == key { value = substr($0, length($1) + 2) } END { print value }' "$path" 2>/dev/null
}

write_initial_summary() {
    {
        echo "status=initializing"
        echo "result_dir=$RESULT_DIR"
        echo "cap_review_sol=$CAP_REVIEW_SOL"
        echo "window_seconds=$WINDOW_SECONDS"
        echo "helius_sender_url=$TYO_SENDER_URL"
        echo "oob_arm_file=$OOB_ARM_FILE"
        echo "run78_step=$RUN78_STEP"
        echo "run78b_step=$RUN78B_STEP"
        echo "no_send=true"
        echo "disable_live_send=true"
        echo "disable_sendbundle=true"
    } > "$SUMMARY_FILE"
}

render_summary() {
    local run78_summary="$1"
    local run78b_summary="$2"
    local run78_rc="$3"
    local run78b_rc="$4"
    python3 - "$SUMMARY_FILE" "$SUMMARY_JSON" "$run78_summary" "$run78b_summary" \
        "$run78_rc" "$run78b_rc" "$CAP_REVIEW_SOL" "$WINDOW_SECONDS" "$TYO_SENDER_URL" \
        "$RESULT_DIR" "$RUN78_STEP" "$RUN78B_STEP" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
run78_summary_path = Path(sys.argv[3])
run78b_summary_path = Path(sys.argv[4])
run78_rc = int(sys.argv[5])
run78b_rc = int(sys.argv[6])
cap_review_sol = sys.argv[7]
window_seconds = sys.argv[8]
helius_sender_url = sys.argv[9]
result_dir = Path(sys.argv[10])
run78_step = sys.argv[11]
run78b_step = sys.argv[12]


def parse_kv(path: Path) -> dict[str, str]:
    data: dict[str, str] = {}
    if not path.exists():
        return data
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def as_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def as_bool(value) -> bool:
    if isinstance(value, bool):
        return value
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


run78 = parse_kv(run78_summary_path)
run78b = parse_kv(run78b_summary_path)
fresh_runtime_quote = (
    as_int(run78.get("fresh_strict_directarb_candidate_count")) > 0
    and as_int(run78.get("quote_numeric_present_count")) > 0
    and as_int(run78.get("exact_quote_match_count")) > 0
)
min_out_positive = as_int(run78.get("min_out_positive_count")) > 0
net_ev_after_fee_tip_positive = as_int(run78.get("route_specific_ev_after_fee_tip_positive_count")) > 0
pm_reserve_ok = as_bool(run78.get("pm_reserve_ok"))
pm_release_ok = as_bool(run78.get("pm_release_ok"))
template_pm_account_match = as_bool(run78.get("template_ready")) and pm_reserve_ok
private_sender_config_ready = bool(helius_sender_url) and "tyo-sender.helius-rpc.com/fast" in helius_sender_url
helius_sender_dual_no_send_accept = (
    as_int(run78.get("helius_sender_dual_no_send_accept_count")) > 0
    or (
        fresh_runtime_quote
        and min_out_positive
        and net_ev_after_fee_tip_positive
        and pm_reserve_ok
        and pm_release_ok
        and private_sender_config_ready
        and as_bool(run78.get("acceptance_pass"))
    )
    or as_int(run78b.get("helius_sender_dual_no_send_accept")) > 0
)
send_transaction_called = as_bool(run78.get("send_transaction_called")) or as_bool(
    run78b.get("send_transaction_called")
)
send_bundle_called = as_bool(run78.get("sendBundle_called")) or as_bool(run78b.get("sendBundle_called"))
wire_send = max(as_int(run78.get("wire_send")), as_int(run78b.get("wire_send")))
live_landed = max(as_int(run78.get("live_landed")), as_int(run78b.get("live_landed")))
acceptance_pass = all(
    [
        cap_review_sol == "1",
        fresh_runtime_quote,
        min_out_positive,
        net_ev_after_fee_tip_positive,
        pm_reserve_ok,
        pm_release_ok,
        template_pm_account_match,
        helius_sender_dual_no_send_accept,
        not send_transaction_called,
        not send_bundle_called,
        wire_send == 0,
        live_landed == 0,
    ]
)

if acceptance_pass:
    status = "run78c2_cap1_runtime_quote_nosend_ready"
elif run78_rc != 0 and as_int(run78.get("fresh_strict_directarb_candidate_count")) <= 0:
    status = "blocked_no_fresh_runtime_candidate_at_cap1"
elif not fresh_runtime_quote:
    status = "blocked_fresh_runtime_quote"
elif not net_ev_after_fee_tip_positive:
    status = "blocked_runtime_ev_not_positive"
elif not pm_reserve_ok:
    status = "blocked_pm_reserve"
elif not helius_sender_dual_no_send_accept:
    status = "blocked_private_sender_no_send_accept"
elif send_transaction_called or send_bundle_called or wire_send != 0 or live_landed != 0:
    status = "blocked_send_side_effect_observed"
else:
    status = "blocked_runtime_gate"

report = {
    "status": status,
    "result_dir": str(result_dir),
    "cap_review_sol": cap_review_sol,
    "window_seconds": int(window_seconds),
    "helius_sender_url": helius_sender_url,
    "helius_sender_tyo_forced": "tyo-sender.helius-rpc.com/fast" in helius_sender_url,
    "run78_rc": run78_rc,
    "run78b_rc": run78b_rc,
    "run78_summary_path": str(run78_summary_path),
    "run78b_summary_path": str(run78b_summary_path),
    "fresh_runtime_quote": fresh_runtime_quote,
    "fresh_strict_directarb_candidate_count": as_int(run78.get("fresh_strict_directarb_candidate_count")),
    "min_out_positive": min_out_positive,
    "net_ev_after_fee_tip_positive": net_ev_after_fee_tip_positive,
    "pm_reserve_ok": pm_reserve_ok,
    "pm_release_ok": pm_release_ok,
    "template_pm_account_match": template_pm_account_match,
    "helius_sender_dual_no_send_accept": bool(helius_sender_dual_no_send_accept),
    "helius_sender_dual_no_send_accept_count": max(
        as_int(run78.get("helius_sender_dual_no_send_accept_count")),
        as_int(run78b.get("helius_sender_dual_no_send_accept")),
        int(bool(helius_sender_dual_no_send_accept)),
    ),
    "private_sender_config_ready": private_sender_config_ready,
    "send_transaction_called": send_transaction_called,
    "sendBundle_called": send_bundle_called,
    "wire_send": wire_send,
    "live_landed": live_landed,
    "acceptance_pass": acceptance_pass,
    "selected_signature": run78.get("selected_signature", ""),
    "selected_pool": run78.get("selected_pool", ""),
    "selected_mint_in": run78.get("selected_mint_in", ""),
    "selected_mint_out": run78.get("selected_mint_out", ""),
    "selected_route": run78.get("selected_route", ""),
    "selected_target_route_family": run78.get("selected_target_route_family", ""),
    "run78_step": run78_step,
    "run78b_step": run78b_step,
}

lines = [
    f"status={report['status']}",
    f"result_dir={report['result_dir']}",
    f"cap_review_sol={report['cap_review_sol']}",
    f"window_seconds={report['window_seconds']}",
    f"helius_sender_url={report['helius_sender_url']}",
    f"helius_sender_tyo_forced={str(report['helius_sender_tyo_forced']).lower()}",
    f"run78_rc={report['run78_rc']}",
    f"run78b_rc={report['run78b_rc']}",
    f"fresh_runtime_quote={str(report['fresh_runtime_quote']).lower()}",
    f"fresh_strict_directarb_candidate_count={report['fresh_strict_directarb_candidate_count']}",
    f"min_out_positive={str(report['min_out_positive']).lower()}",
    f"net_ev_after_fee_tip_positive={str(report['net_ev_after_fee_tip_positive']).lower()}",
    f"pm_reserve_ok={str(report['pm_reserve_ok']).lower()}",
    f"pm_release_ok={str(report['pm_release_ok']).lower()}",
    f"template_pm_account_match={str(report['template_pm_account_match']).lower()}",
    f"helius_sender_dual_no_send_accept={str(report['helius_sender_dual_no_send_accept']).lower()}",
    f"helius_sender_dual_no_send_accept_count={report['helius_sender_dual_no_send_accept_count']}",
    f"private_sender_config_ready={str(report['private_sender_config_ready']).lower()}",
    f"send_transaction_called={str(report['send_transaction_called']).lower()}",
    f"sendBundle_called={str(report['sendBundle_called']).lower()}",
    f"wire_send={report['wire_send']}",
    f"live_landed={report['live_landed']}",
    f"acceptance_pass={str(report['acceptance_pass']).lower()}",
    f"selected_signature={report['selected_signature']}",
    f"selected_pool={report['selected_pool']}",
    f"selected_mint_in={report['selected_mint_in']}",
    f"selected_mint_out={report['selected_mint_out']}",
    f"selected_route={report['selected_route']}",
    f"selected_target_route_family={report['selected_target_route_family']}",
    f"run78_summary_path={report['run78_summary_path']}",
    f"run78b_summary_path={report['run78b_summary_path']}",
]
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("\n".join(lines))
PY
}

write_initial_summary

export SHADOW_RESULTS_DIR="$RESULT_DIR"
export SHADOW_RUN78_STEP="$RUN78_STEP"
export SHADOW_RUN78B_STEP="$RUN78B_STEP"
export SHADOW_RUN78B_RUN78_STEP="$RUN78_STEP"
export SHADOW_RUN78_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN78_POST_WINDOW_DRAIN_SECONDS="$POST_WINDOW_DRAIN_SECONDS"
export SHADOW_RUN78_APPROVED_LIVE_CAP_SOL="$CAP_REVIEW_SOL"
export SHADOW_RUN78_PRIVATE_REPLAY_TARGET_ROUTE_FAMILY="helius_sender_dual"
export SHADOW_RUN78_SCOUT_ROUTE_FAMILY="tpu_quic_public"
export SHADOW_XDP_ATTACH_MODE="native"
export SHADOW_AFXDP_SAFE_MODE="false"
export SHADOW_RUN78_XDP_ATTACH_MODE="native"
export SHADOW_RUN78_AFXDP_SAFE_MODE="false"
export SHADOW_REQUIRE_AFXDP_ZERO_COPY="true"
export SHADOW_AFXDP_BIND_MODE="zero_copy"
export SHADOW_NO_SEND=true
export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
export SHADOW_ENABLE_HELIUS_SENDER_DUAL=true
export SHADOW_ENABLE_JITO_VIA_HELIUS=true
export SHADOW_HELIUS_SENDER_URL="$TYO_SENDER_URL"
export HELIUS_SENDER_URL="$TYO_SENDER_URL"
export SHADOW_HELIUS_SENDER_ENDPOINT="$TYO_SENDER_URL"
export SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$OOB_ARM_FILE"

set +e
sudo -E bash "$PACK_DIR/scripts/78_fresh_strict_directarb_candidate_scout_nosend.sh"
RUN78_RC=$?
set -e

RUN78_SUMMARY="$RESULT_DIR/$RUN78_STEP/summary.txt"
set +e
bash "$PACK_DIR/scripts/78b_fresh_directarb_private_egress_nosend.sh"
RUN78B_RC=$?
set -e
RUN78B_SUMMARY="$RESULT_DIR/$RUN78B_STEP/summary.txt"

render_summary "$RUN78_SUMMARY" "$RUN78B_SUMMARY" "$RUN78_RC" "$RUN78B_RC"

ACCEPTANCE_PASS="$(kv_get "$SUMMARY_FILE" acceptance_pass)"
if [[ "$ACCEPTANCE_PASS" != "true" ]]; then
    fail "Run78C2 cap-1 runtime no-send proof did not pass"
    finish
    exit 1
fi

pass "Run78C2 cap-1 runtime no-send proof passed"
finish
exit 0

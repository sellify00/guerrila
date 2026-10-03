#!/usr/bin/env bash
# 82_public_rpc_bounded_window.sh - standalone bounded public-RPC scout window
#
# This wrapper avoids the older observe-only Run39/55/61 stack and drives the
# existing R1-PUBLIC-A lane directly through Run29. By default it uses the
# loopback blackhole endpoint so we can observe repeated public-scout routing
# and would-send activity without a real chain send.

set -euo pipefail

source "$(dirname "$0")/lib.sh"
script_header "82 - Public RPC Bounded Window"

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RUN29_SCRIPT="$PACK_DIR/scripts/29_clmm_publicrpc_blackhole_proof.sh"
BASE_TEMPLATE="${SHADOW_RUN82_BASE_TEMPLATE:-$PACK_DIR/config.r1-public-a.yaml}"

WINDOW_SECONDS_RAW="${1:-${SHADOW_RUN82_WINDOW_SECONDS:-60}}"
WINDOW_SECONDS="$(printf '%s\n' "$WINDOW_SECONDS_RAW" | head -n 1 | tr -d '\r')"
if [[ ! "$WINDOW_SECONDS" =~ ^[0-9]+$ ]]; then
    fail "WINDOW_SECONDS must be an integer (got '$WINDOW_SECONDS_RAW')"
    exit 1
fi

STEP="${SHADOW_RUN82_STEP:-82_public_rpc_bounded_window}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
INNER_STEP="${SHADOW_RUN82_INNER_STEP:-${STEP}_inner}"
INNER_RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$INNER_STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
INNER_SUMMARY_FILE="$INNER_RESULT_DIR/summary.txt"
INNER_TIER0_LOG="$INNER_RESULT_DIR/shadow-tier0.log"
CONFIG_TEMPLATE_TMP="$RESULT_DIR/config.r1-public-window.yaml"
ROUTE_MANIFEST="$RESULT_DIR/public_rpc_route_manifest.jsonl"
PROCESS_MANIFEST="$RESULT_DIR/public_rpc_process_manifest.jsonl"
CLASSIFICATION_JSON="$RESULT_DIR/classification.json"

PUBLIC_MODE="${SHADOW_RUN82_PUBLIC_MODE:-blackhole}"
BLACKHOLE_ENDPOINT="${SHADOW_RUN82_BLACKHOLE_PUBLIC_RPC_ENDPOINT:-https://127.0.0.1:1}"
REAL_PUBLIC_RPC_ENDPOINT="${SHADOW_RUN82_PUBLIC_RPC_ENDPOINT:-${SHADOW_PUBLIC_RPC_URL:-$(default_public_rpc_url)}}"
ALLOW_ONLY_VENUE_ID="${SHADOW_RUN82_ALLOW_ONLY_VENUE_ID:-raydium_clmm}"
MAX_SCOUT_ATTEMPTS="${SHADOW_RUN82_MAX_SCOUT_ATTEMPTS_PER_WINDOW:-64}"
POLL_SECONDS="${SHADOW_RUN82_POLL_SECONDS:-5}"
NIC_IFACE="${SHADOW_NIC_IFACE:-eno1}"
XSKMAP_PIN="${SHADOW_XSKMAP_PIN:-/sys/fs/bpf/shadow_xsks_map}"
NIC_BASELINE_COMBINED_QUEUES="${SHADOW_NIC_BASELINE_COMBINED_QUEUES:-16}"
LOCAL_AGAVE_SERVICE="${SHADOW_RUN82_LOCAL_AGAVE_SERVICE:-${SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE:-sh1-agave-no-geyser-2.service}}"
LOCAL_AGAVE_RPC_URL="${SHADOW_LOCAL_AGAVE_RPC_URL:-http://127.0.0.1:8899}"
CLEANUP_TIMEOUT_SECONDS="${SHADOW_RUN82_CLEANUP_TIMEOUT_SECONDS:-600}"

case "$PUBLIC_MODE" in
    blackhole)
        PUBLIC_RPC_ENDPOINT="$BLACKHOLE_ENDPOINT"
        EXPECT_BLACKHOLE_ENDPOINT="true"
        ;;
    real)
        PUBLIC_RPC_ENDPOINT="$REAL_PUBLIC_RPC_ENDPOINT"
        EXPECT_BLACKHOLE_ENDPOINT="false"
        MAX_SCOUT_ATTEMPTS=1
        ;;
    *)
        fail "Unsupported SHADOW_RUN82_PUBLIC_MODE='$PUBLIC_MODE' (expected blackhole or real)"
        exit 1
        ;;
esac

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$CONFIG_TEMPLATE_TMP" \
    "$ROUTE_MANIFEST" \
    "$PROCESS_MANIFEST" \
    "$CLASSIFICATION_JSON"

python3 - "$BASE_TEMPLATE" "$CONFIG_TEMPLATE_TMP" "$MAX_SCOUT_ATTEMPTS" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
max_scout_attempts = sys.argv[3]

text = src.read_text(encoding="utf-8")
text, count = re.subn(
    r"(^\s*max_scout_attempts_per_window:\s*)\d+\s*$",
    rf"\g<1>{max_scout_attempts}",
    text,
    count=1,
    flags=re.M,
)
if count != 1:
    raise SystemExit("Failed to rewrite max_scout_attempts_per_window in public template")
dst.write_text(text, encoding="utf-8")
PY

export SHADOW_RUN29C_STEP="$INNER_STEP"
export SHADOW_RUN29C_CONFIG_TEMPLATE="$CONFIG_TEMPLATE_TMP"
export SHADOW_RUN29C_PUBLIC_RPC_ENDPOINT="$PUBLIC_RPC_ENDPOINT"
export SHADOW_RUN29C_EXPECT_BLACKHOLE_ENDPOINT="$EXPECT_BLACKHOLE_ENDPOINT"
export SHADOW_RUN29C_VALIDATE_PUBLIC_SCOUT_PROFILE=false
export SHADOW_RUN29C_REQUIRE_TARGET_PUBLIC_SCOUT=false
export SHADOW_RUN29C_ONE_SHOT_MODE=false
export SHADOW_RUN29C_MODE=strict-proof
export SHADOW_RUN29C_POLL_SECONDS="$POLL_SECONDS"
export SHADOW_RUN29C_ALLOW_ONLY_VENUE_ID="$ALLOW_ONLY_VENUE_ID"
export SHADOW_XDP_ATTACH_MODE="${SHADOW_XDP_ATTACH_MODE:-native}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_AFXDP_SAFE_MODE:-false}"
export SHADOW_TRACE_PUBLIC_RPC_SCOUT_EMIT=true

set +e
"$RUN29_SCRIPT" "$WINDOW_SECONDS"
inner_rc=$?
set -e

if [[ ! -f "$INNER_SUMMARY_FILE" ]]; then
    fail "Run29 inner summary missing: $INNER_SUMMARY_FILE"
    exit 1
fi

python3 - "$INNER_TIER0_LOG" "$ROUTE_MANIFEST" "$PROCESS_MANIFEST" "$CLASSIFICATION_JSON" <<'PY'
import json
import re
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
route_path = Path(sys.argv[2])
process_path = Path(sys.argv[3])
classification_path = Path(sys.argv[4])

route_re = re.compile(
    r"TRACE public_rpc_scout_clmm_census stage=route actionable=(?P<actionable>\d+) "
    r"decision=(?P<decision>\S+) reason=(?P<reason>\S+) pool=(?P<pool>\S+) "
    r"mint_in=(?P<mint_in>\S+) mint_out=(?P<mint_out>\S+) signature=(?P<signature>\S+) "
    r"signal_source=(?P<signal_source>\S+) slot=(?P<slot>\d+)"
)
process_ok_re = re.compile(
    r"TRACE public_rpc_scout_clmm_census stage=process_intent outcome=ok "
    r"pool=(?P<pool>\S+) mint_in=(?P<mint_in>\S+) mint_out=(?P<mint_out>\S+) signature=(?P<signature>\S+)"
)
process_err_re = re.compile(
    r"TRACE public_rpc_scout_clmm_census stage=process_intent outcome=err "
    r"error_stage=(?P<error_stage>\S+) error_reason=(?P<error_reason>\S+) "
    r"pool=(?P<pool>\S+) mint_in=(?P<mint_in>\S+) mint_out=(?P<mint_out>\S+) signature=(?P<signature>\S+)"
)

route_rows = []
process_rows = []
if log_path.exists():
    for raw in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = route_re.search(raw)
        if match:
            row = match.groupdict()
            row["actionable"] = int(row["actionable"])
            row["slot"] = int(row["slot"])
            route_rows.append(row)
            continue
        match = process_ok_re.search(raw)
        if match:
            row = match.groupdict()
            row["outcome"] = "ok"
            process_rows.append(row)
            continue
        match = process_err_re.search(raw)
        if match:
            row = match.groupdict()
            row["outcome"] = "err"
            process_rows.append(row)

with route_path.open("w", encoding="utf-8") as fh:
    for row in route_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")

with process_path.open("w", encoding="utf-8") as fh:
    for row in process_rows:
        fh.write(json.dumps(row, sort_keys=True) + "\n")

classification = {
    "trace_route_rows": len(route_rows),
    "trace_route_actionable_rows": sum(1 for row in route_rows if row["actionable"] == 1),
    "trace_route_public_rpc_rows": sum(1 for row in route_rows if row["decision"] == "public_rpc_scout"),
    "trace_route_jito_rows": sum(1 for row in route_rows if row["decision"] == "jito_bundle"),
    "trace_route_drop_rows": sum(1 for row in route_rows if row["decision"] == "drop"),
    "trace_route_unique_signatures": len({row["signature"] for row in route_rows}),
    "trace_route_unique_pools": len({row["pool"] for row in route_rows}),
    "trace_process_rows": len(process_rows),
    "trace_process_ok_rows": sum(1 for row in process_rows if row["outcome"] == "ok"),
    "trace_process_err_rows": sum(1 for row in process_rows if row["outcome"] == "err"),
}
classification_path.write_text(json.dumps(classification, indent=2, sort_keys=True), encoding="utf-8")
PY

summary_value() {
    local key="$1"
    sed -n "s/^${key}=//p" "$INNER_SUMMARY_FILE" | tail -n 1
}

cleanup_verified="false"
cleanup_restart_performed="false"
agave_service_after_cleanup="unknown"
agave_health_after_cleanup="unavailable"
eno1_xdp_lines_after_cleanup=""
xsk_map_after_cleanup="unknown"
eno1_combined_after_cleanup=""
udp_8000_8025_count_after_cleanup="0"

reconcile_host_cleanup() {
    sudo pkill -f 'shadow-tier0' 2>/dev/null || true
    sudo pkill -f 'shadow-oracle' 2>/dev/null || true
    sudo pkill -f 'shadow-egress-tpu' 2>/dev/null || true
    sudo ip link set dev "$NIC_IFACE" xdp off 2>/dev/null || true
    sudo rm -f "$XSKMAP_PIN" 2>/dev/null || true
    sudo ethtool -L "$NIC_IFACE" combined "$NIC_BASELINE_COMBINED_QUEUES" 2>/dev/null || true
    sudo systemctl restart "$LOCAL_AGAVE_SERVICE" 2>/dev/null || true
    cleanup_restart_performed="true"

    local deadline payload
    deadline=$((SECONDS + CLEANUP_TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        eno1_xdp_lines_after_cleanup="$(ip -details link show dev "$NIC_IFACE" 2>/dev/null | grep -c 'prog/xdp' || true)"
        if [[ -e "$XSKMAP_PIN" ]]; then
            xsk_map_after_cleanup="present"
        else
            xsk_map_after_cleanup="absent"
        fi
        eno1_combined_after_cleanup="$(ethtool -l "$NIC_IFACE" 2>/dev/null | awk '/Current hardware settings/{flag=1;next} flag && /Combined:/{print $2; exit}')"
        agave_service_after_cleanup="$(systemctl is-active "$LOCAL_AGAVE_SERVICE" 2>/dev/null || true)"
        udp_8000_8025_count_after_cleanup="$(ss -H -u -lpn 2>/dev/null | awk '{print $5}' | grep -Ec ':80(0[0-9]|1[0-9]|2[0-5])$' || true)"
        payload="$(curl -fsS -X POST -H 'Content-Type: application/json' --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$LOCAL_AGAVE_RPC_URL" 2>/dev/null || true)"
        if printf '%s' "$payload" | grep -q '"result":"ok"'; then
            agave_health_after_cleanup="ok"
        elif [[ -n "$payload" ]]; then
            agave_health_after_cleanup="not_ok"
        else
            agave_health_after_cleanup="unavailable"
        fi
        if [[ "$eno1_xdp_lines_after_cleanup" == "0" ]] \
            && [[ "$xsk_map_after_cleanup" == "absent" ]] \
            && [[ "$agave_service_after_cleanup" == "active" ]] \
            && [[ "$agave_health_after_cleanup" == "ok" ]]; then
            cleanup_verified="true"
            return 0
        fi
        sleep 5
    done
    return 0
}

worker_route_hits="$(summary_value worker_route_resolved_public_rpc_scout)"
worker_emit_attempt="$(summary_value worker_public_rpc_scout_emit_attempt)"
worker_emit_ok="$(summary_value worker_public_rpc_scout_emit_ok)"
public_enqueued="$(summary_value public_rpc_scout_enqueued)"
transport_attempt="$(summary_value public_rpc_transport_attempt)"
transport_failure="$(summary_value public_rpc_transport_failure)"
transport_rejected="$(summary_value public_rpc_transport_rejected)"
nack_recorded="$(summary_value public_rpc_nack_recorded)"
pm_created="$(summary_value worker_pm_reservation_created)"
pm_released="$(summary_value pm_reservation_released)"
public_scout_gates_green="$(summary_value public_scout_gates_green)"
rollout_public_no_send="$(summary_value rollout_public_rpc_no_send_would_sends)"
r1_attempt="$(summary_value run38_target_public_scout_clmm_r1_attempt)"
reject_proof_filter="$(summary_value run38_target_public_scout_clmm_reject_proof_filter)"
reject_micro_market="$(summary_value run38_target_public_scout_clmm_reject_micro_market)"
reject_inventory="$(summary_value run38_target_public_scout_clmm_reject_inventory)"
reject_template="$(summary_value run38_target_public_scout_clmm_reject_template_executability)"
reject_latch="$(summary_value run38_target_public_scout_clmm_reject_attempt_latch)"
admit_count="$(summary_value run38_target_public_scout_clmm_admit)"
process_intent_count="$(summary_value run38_target_public_scout_clmm_process_intent)"
target_emit_attempt="$(summary_value run38_target_public_scout_clmm_emit_attempt)"
target_transport_attempt="$(summary_value run38_target_public_scout_clmm_transport_attempt)"
raw_outcome="$(summary_value raw_outcome)"
inner_outcome="$(summary_value outcome)"

reconcile_host_cleanup

normalize_int() {
    local value="${1:-0}"
    if [[ -z "$value" ]]; then
        printf '0'
    else
        printf '%s' "$value"
    fi
}

route_hits_i="$(normalize_int "$worker_route_hits")"
emit_ok_i="$(normalize_int "$worker_emit_ok")"
transport_attempt_i="$(normalize_int "$transport_attempt")"
rollout_public_no_send_i="$(normalize_int "$rollout_public_no_send")"
pm_created_i="$(normalize_int "$pm_created")"
reject_inventory_i="$(normalize_int "$reject_inventory")"
reject_proof_filter_i="$(normalize_int "$reject_proof_filter")"
reject_micro_market_i="$(normalize_int "$reject_micro_market")"
reject_template_i="$(normalize_int "$reject_template")"
reject_latch_i="$(normalize_int "$reject_latch")"

status="blocked_unknown"
acceptable_outcome="false"
if [[ "$public_scout_gates_green" != "true" ]]; then
    status="blocked_public_scout_gates_not_green"
elif (( transport_attempt_i > 0 || emit_ok_i > 0 || rollout_public_no_send_i > 0 || pm_created_i > 0 )); then
    if [[ "$EXPECT_BLACKHOLE_ENDPOINT" == "true" ]]; then
        status="passed_public_rpc_blackhole_window"
    else
        status="passed_public_rpc_window"
    fi
    acceptable_outcome="true"
elif (( route_hits_i == 0 )); then
    status="blocked_no_public_rpc_candidate"
    acceptable_outcome="true"
elif (( reject_inventory_i > 0 )); then
    status="blocked_candidate_inventory"
    acceptable_outcome="true"
elif (( reject_proof_filter_i > 0 )); then
    status="blocked_candidate_proof_filter"
    acceptable_outcome="true"
elif (( reject_micro_market_i > 0 )); then
    status="blocked_candidate_micro_market"
    acceptable_outcome="true"
elif (( reject_template_i > 0 )); then
    status="blocked_candidate_template"
    acceptable_outcome="true"
elif (( reject_latch_i > 0 )); then
    status="blocked_candidate_attempt_latch"
    acceptable_outcome="true"
else
    status="blocked_candidate_no_emit"
    acceptable_outcome="true"
fi

classification_trace_route_rows="$(python3 - "$CLASSIFICATION_JSON" <<'PY'
import json, sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("trace_route_rows", 0))
PY
)"
classification_trace_route_public_rpc_rows="$(python3 - "$CLASSIFICATION_JSON" <<'PY'
import json, sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("trace_route_public_rpc_rows", 0))
PY
)"
classification_trace_route_jito_rows="$(python3 - "$CLASSIFICATION_JSON" <<'PY'
import json, sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("trace_route_jito_rows", 0))
PY
)"
classification_trace_process_rows="$(python3 - "$CLASSIFICATION_JSON" <<'PY'
import json, sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("trace_process_rows", 0))
PY
)"

{
    echo "status=$status"
    echo "acceptable_outcome=$acceptable_outcome"
    echo "public_mode=$PUBLIC_MODE"
    echo "window_seconds=$WINDOW_SECONDS"
    echo "public_rpc_endpoint=$PUBLIC_RPC_ENDPOINT"
    echo "max_scout_attempts_per_window=$MAX_SCOUT_ATTEMPTS"
    echo "allow_only_venue_id=$ALLOW_ONLY_VENUE_ID"
    echo "inner_rc=$inner_rc"
    echo "inner_raw_outcome=$raw_outcome"
    echo "inner_outcome=$inner_outcome"
    echo "inner_summary_path=$INNER_SUMMARY_FILE"
    echo "inner_tier0_log=$INNER_TIER0_LOG"
    echo "route_manifest_path=$ROUTE_MANIFEST"
    echo "process_manifest_path=$PROCESS_MANIFEST"
    echo "classification_json=$CLASSIFICATION_JSON"
    echo "worker_route_resolved_public_rpc_scout=$worker_route_hits"
    echo "worker_public_rpc_scout_emit_attempt=$worker_emit_attempt"
    echo "worker_public_rpc_scout_emit_ok=$worker_emit_ok"
    echo "public_rpc_scout_enqueued=$public_enqueued"
    echo "public_rpc_transport_attempt=$transport_attempt"
    echo "public_rpc_transport_failure=$transport_failure"
    echo "public_rpc_transport_rejected=$transport_rejected"
    echo "public_rpc_nack_recorded=$nack_recorded"
    echo "worker_pm_reservation_created=$pm_created"
    echo "pm_reservation_released=$pm_released"
    echo "public_scout_gates_green=$public_scout_gates_green"
    echo "rollout_public_rpc_no_send_would_sends=$rollout_public_no_send"
    echo "run38_target_public_scout_clmm_r1_attempt=$r1_attempt"
    echo "run38_target_public_scout_clmm_reject_proof_filter=$reject_proof_filter"
    echo "run38_target_public_scout_clmm_reject_micro_market=$reject_micro_market"
    echo "run38_target_public_scout_clmm_reject_inventory=$reject_inventory"
    echo "run38_target_public_scout_clmm_reject_template_executability=$reject_template"
    echo "run38_target_public_scout_clmm_reject_attempt_latch=$reject_latch"
    echo "run38_target_public_scout_clmm_admit=$admit_count"
    echo "run38_target_public_scout_clmm_process_intent=$process_intent_count"
    echo "run38_target_public_scout_clmm_emit_attempt=$target_emit_attempt"
    echo "run38_target_public_scout_clmm_transport_attempt=$target_transport_attempt"
    echo "trace_route_rows=$classification_trace_route_rows"
    echo "trace_route_public_rpc_rows=$classification_trace_route_public_rpc_rows"
    echo "trace_route_jito_rows=$classification_trace_route_jito_rows"
    echo "trace_process_rows=$classification_trace_process_rows"
    echo "cleanup_verified=$cleanup_verified"
    echo "cleanup_restart_performed=$cleanup_restart_performed"
    echo "agave_service_after_cleanup=$agave_service_after_cleanup"
    echo "agave_health_after_cleanup=$agave_health_after_cleanup"
    echo "eno1_xdp_lines_after_cleanup=$eno1_xdp_lines_after_cleanup"
    echo "xsk_map_after_cleanup=$xsk_map_after_cleanup"
    echo "eno1_combined_after_cleanup=$eno1_combined_after_cleanup"
    echo "udp_8000_8025_count_after_cleanup=$udp_8000_8025_count_after_cleanup"
} | tee "$SUMMARY_FILE"

if [[ "$acceptable_outcome" != "true" ]]; then
    fail "$status"
    exit 1
fi

finish

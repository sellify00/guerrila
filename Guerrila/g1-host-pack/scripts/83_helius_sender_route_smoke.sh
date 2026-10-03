#!/usr/bin/env bash
# 83_helius_sender_route_smoke.sh — Sender route mux / no-send smoke
#
# Non-live validation only. Confirms:
#   - Sender is explicitly selected for plain private flow
#   - bundle-only flow still stays on Jito
#   - tip account boot selection works
#   - Sender-shaped tx body / transport wiring is valid
#   - no-send PM ledger path still works

source "$(dirname "$0")/lib.sh"
script_header "83 — Helius Sender Route Smoke"

STEP="83_helius_sender_route_smoke"
assert_cmd_exists cargo || exit 1
assert_cmd_exists grep || exit 1

WORKSPACE_ROOT="${SHADOW_WORKSPACE_ROOT:-$REPO_ROOT_DIR}"
HOST_AUDIT_ROOT="${SHADOW_HOST_AUDIT_DIR:-$HOME/shadow-host-audit}"
RUN_ID="${SHADOW_RUNSENDER0_ID:-runsender0_$(date -u +%Y%m%dT%H%M%SZ)}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$HOST_AUDIT_ROOT/dpdk-runs/$RUN_ID/$STEP}"
mkdir -p "$RESULT_DIR"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
CHECK_CONFIG_LOG="$RESULT_DIR/check_config.log"

cd "$WORKSPACE_ROOT"

run_check() {
    local name="$1"
    shift
    local log_file="$RESULT_DIR/${name}.log"
    info "Running $name"
    if "$@" >"$log_file" 2>&1; then
        pass "$name"
        echo "${name}=pass" >>"$SUMMARY_FILE"
    else
        fail "$name"
        echo "${name}=fail" >>"$SUMMARY_FILE"
        echo "${name}_log=$log_file" >>"$SUMMARY_FILE"
    fi
}

: >"$SUMMARY_FILE"
echo "step=$STEP" >>"$SUMMARY_FILE"
echo "workspace_root=$WORKSPACE_ROOT" >>"$SUMMARY_FILE"
echo "result_dir=$RESULT_DIR" >>"$SUMMARY_FILE"

run_check config_sender_selected \
    grep -q 'default_jito_searcher: helius_sender_dual' config.guerrila.yaml
run_check hostpack_config_sender_selected \
    grep -q 'default_jito_searcher: helius_sender_dual' g1-host-pack/config.guerrila.yaml

run_check shadow_core_sender_routing \
    cargo test -p shadow-core helius_sender -- --nocapture
run_check shadow_tier0_sender_tip_boot \
    cargo test -p shadow-tier0 helius_sender -- --nocapture
run_check shadow_tier0_no_send_pm_ledger \
    cargo test -p shadow-tier0 sh7_a_controlled_supported_candidate_reaches_final_no_send_pm_ledger -- --nocapture
run_check shadow_egress_sender_body \
    cargo test -p shadow-egress-jito sender_transaction_body_uses_skip_preflight_and_base64 -- --nocapture
run_check shadow_egress_sender_transport_hint \
    cargo test -p shadow-egress-jito transport_hint -- --nocapture
run_check shadow_egress_sender_transport_nack \
    cargo test -p shadow-egress-jito selected_transport_is_unavailable -- --nocapture

info "Running config check"
if cargo run -p shadow-tier0 --bin shadow-tier0 -- --check-config config.guerrila.yaml >"$CHECK_CONFIG_LOG" 2>&1; then
    if grep -q 'HELIUS_SENDER_DUAL_EXPLICIT=true' "$CHECK_CONFIG_LOG"; then
        pass "check_config_sender_explicit"
        echo "check_config_sender_explicit=pass" >>"$SUMMARY_FILE"
    else
        fail "check_config_sender_explicit"
        echo "check_config_sender_explicit=fail" >>"$SUMMARY_FILE"
    fi
else
    fail "check_config_sender_explicit"
    echo "check_config_sender_explicit=fail" >>"$SUMMARY_FILE"
fi

echo "check_config_log=$CHECK_CONFIG_LOG" >>"$SUMMARY_FILE"

if [[ $FAILURES -eq 0 ]]; then
    pass "RunSender0 smoke passed"
    echo "status=passed" >>"$SUMMARY_FILE"
    exit 0
fi

fail "RunSender0 smoke failed"
echo "status=failed" >>"$SUMMARY_FILE"
exit 1

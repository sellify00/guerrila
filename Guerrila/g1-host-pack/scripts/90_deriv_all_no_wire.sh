#!/usr/bin/env bash
# 90_deriv_all_no_wire.sh — run full no-wire derivatives research funnel
set -euo pipefail

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_crlf_safe "$SCRIPT_DIR/lib.sh"
script_header "90 Deriv — Full No-Wire Funnel"

bash "$SCRIPT_DIR/90_deriv0_market_universe_probe.sh"
bash "$SCRIPT_DIR/90_deriv1_fee_funding_cost_model.sh"
bash "$SCRIPT_DIR/90_deriv2_onchain_signal_to_perp_mapper.sh"
bash "$SCRIPT_DIR/90_deriv3_postshock_perp_paper_runner.sh"
bash "$SCRIPT_DIR/90_deriv4_basis_fair_value_scanner.sh"
bash "$SCRIPT_DIR/90_deriv6_perp_impulse_watch.sh"
bash "$SCRIPT_DIR/90_deriv7_rolling_impulse_trigger_replay.sh"
bash "$SCRIPT_DIR/90_deriv5_money_gate.sh"

pass "Derivatives no-wire funnel complete"
finish

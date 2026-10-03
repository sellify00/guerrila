#!/usr/bin/env bash
# 66u_run_repeatability_chain.sh - run fresh no-send epochs and aggregate ATA repeatability

source "$(dirname "$0")/lib.sh"
script_header "66U - Fresh Repeatability Chain"

assert_root
assert_cmd_exists date

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CALLER_SHADOW_RESULTS_DIR="${SHADOW_RESULTS_DIR-}"

source_text_file() {
    local path="$1"
    [[ -f "$path" ]] || return 0
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_text_file "$PACK_DIR/env.g1.sh"
source_text_file "$PACK_DIR/env.r1-afxdp-eno1-safe.sh"

[[ -n "$CALLER_SHADOW_RESULTS_DIR" ]] && export SHADOW_RESULTS_DIR="$CALLER_SHADOW_RESULTS_DIR"
if [[ "${SHADOW_RESULTS_DIR:-}" == "/dev/results" ]]; then
    export SHADOW_RESULTS_DIR="$PACK_DIR/results"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN66U_CHAIN_STEP:-66u_repeatability_chain_$(date -u +%Y%m%d_%H%M%S)}"
ACTIVE_CONFIG="${SHADOW_REPEATABILITY_ACTIVE_CONFIG_PATH:-${SHADOW_RUN65_ACTIVE_CONFIG_PATH:-/etc/shadow-engine/config.guerrila.yaml}}"
EPOCH_COUNT="${SHADOW_REPEATABILITY_EPOCH_COUNT:-3}"
WINDOW_SECONDS="${SHADOW_REPEATABILITY_WINDOW_SECONDS:-300}"
SLEEP_BETWEEN_SECONDS="${SHADOW_REPEATABILITY_SLEEP_BETWEEN_SECONDS:-0}"
SHARD_COUNTS="${SHADOW_REPEATABILITY_SHARD_COUNTS:-1,2,4,8,16}"
EXECUTABLE_ROUTE_FAMILIES="${SHADOW_REPEATABILITY_EXECUTABLE_ROUTE_FAMILIES:-tpu_quic_public,public_rpc_scout,jito_bundle,jito_json_bundle,harmonic_bundle}"
ALLOW_PRIVATE_ROUTE_SIMULATION="${SHADOW_REPEATABILITY_ALLOW_PRIVATE_ROUTE_SIMULATION:-true}"
ALLOW_LASERSTREAM_PRIVATE_SIMULATION="${SHADOW_REPEATABILITY_ALLOW_LASERSTREAM_PRIVATE_SIMULATION:-true}"
MIN_EPOCHS="${SHADOW_REPEATABILITY_MIN_EPOCHS:-$EPOCH_COUNT}"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE"

config_value() {
    local key="$1"
    local path="$2"
    [[ -f "$path" ]] || return 0
    awk -v key="$key" '
        $0 ~ "^[[:space:]]*" key ":[[:space:]]*" {
            sub(/^[^:]*:[[:space:]]*/, "", $0)
            gsub(/["\r]/, "", $0)
            print $0
            exit
        }
    ' "$path" | xargs
}

infer_source_grade() {
    local endpoint="${1,,}"
    case "$endpoint" in
        *laserstream*) printf '%s\n' "laserstream" ;;
        *) printf '%s\n' "${SHADOW_GRPC_SOURCE_GRADE:-external_geyser}" ;;
    esac
}

CONFIG_GEYSER_ENDPOINT="$(config_value geyser_endpoint "$ACTIVE_CONFIG")"
CONFIG_GRPC_FALLBACK="$(config_value grpc_fallback "$ACTIVE_CONFIG")"
CONFIG_GRPC_TOKEN="$(config_value grpc_fallback_token "$ACTIVE_CONFIG")"
CONFIG_GRPC_SOURCE_GRADE="$(config_value grpc_source_grade "$ACTIVE_CONFIG")"

if [[ -z "${SHADOW_GEYSER_ENDPOINT:-}" || "${SHADOW_GEYSER_ENDPOINT:-}" == *CHANGE_ME* ]]; then
    export SHADOW_GEYSER_ENDPOINT="${CONFIG_GRPC_FALLBACK:-$CONFIG_GEYSER_ENDPOINT}"
fi
if [[ -z "${SHADOW_GEYSER_TOKEN:-}" || "${SHADOW_GEYSER_TOKEN:-}" == *CHANGE_ME* ]]; then
    export SHADOW_GEYSER_TOKEN="$CONFIG_GRPC_TOKEN"
fi
if [[ -z "${SHADOW_GRPC_SOURCE_GRADE:-}" || "${SHADOW_GRPC_SOURCE_GRADE:-}" == "external_geyser" ]]; then
    export SHADOW_GRPC_SOURCE_GRADE="${CONFIG_GRPC_SOURCE_GRADE:-$(infer_source_grade "${SHADOW_GEYSER_ENDPOINT:-}")}"
fi

if [[ "${SHADOW_GEYSER_ENDPOINT:-}" == *laserstream* ]]; then
    info "Using LaserStream endpoint from active config for repeatability windows: $SHADOW_GEYSER_ENDPOINT"
else
    warn "Repeatability chain did not resolve a LaserStream endpoint from $ACTIVE_CONFIG"
fi

source_66t_steps=()

run_step() {
    info "$*"
    "$@"
}

for epoch_index in $(seq 1 "$EPOCH_COUNT"); do
    epoch_tag="${STEP}_e${epoch_index}"
    run65_step="${epoch_tag}_run65"
    run66q_step="${epoch_tag}_66q"
    run66x_step="${epoch_tag}_66x"
    run66q2_step="${epoch_tag}_66q2"
    run66rule_step="${epoch_tag}_66_rule"
    run66overlay_step="${epoch_tag}_66r_overlay"
    run66ata_step="${epoch_tag}_66r_ata"
    run66t_step="${epoch_tag}_66t"

    info "Starting no-send repeatability epoch $epoch_index/$EPOCH_COUNT: $epoch_tag"

    set +e
    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN65_STEP="$run65_step" \
        SHADOW_RUN65_WINDOW_SECONDS="$WINDOW_SECONDS" \
        SHADOW_RUN65_SHARD_COUNTS="$SHARD_COUNTS" \
        SHADOW_RUN65_EXECUTABLE_ROUTE_FAMILIES="$EXECUTABLE_ROUTE_FAMILIES" \
        SHADOW_RUN65_ALLOW_PRIVATE_ROUTE_SIMULATION="$ALLOW_PRIVATE_ROUTE_SIMULATION" \
        SHADOW_RUN65_ALLOW_LASERSTREAM_PRIVATE_SIMULATION="$ALLOW_LASERSTREAM_PRIVATE_SIMULATION" \
        "$PACK_DIR/scripts/65_full_potential_continuous_shard_simulation.sh"
    run65_rc=$?
    set -e

    run65_json="$RESULTS_ROOT/$run65_step/run65_full_potential_continuous_shard_simulation.json"
    run65_summary="$RESULTS_ROOT/$run65_step/summary.txt"
    if [[ "$run65_rc" -ne 0 ]]; then
        warn "Run65 returned rc=$run65_rc; accepting only if fresh no-send artifacts are complete"
    fi
    if [[ ! -f "$run65_json" || ! -f "$run65_summary" ]]; then
        fail "Run65 artifact missing after rc=$run65_rc: $run65_json"
        exit 1
    fi
    would_wire_send_count="$(awk -F= '$1 == "would_wire_send_count" { print $2; found=1 } END { if (!found) print "missing" }' "$run65_summary")"
    blocked_private_route_count="$(awk -F= '$1 == "blocked_private_route_count" { print $2; found=1 } END { if (!found) print "missing" }' "$run65_summary")"
    if [[ "$would_wire_send_count" != "0" ]]; then
        fail "Run65 no-send invariant failed: would_wire_send_count=$would_wire_send_count"
        exit 1
    fi
    if [[ "$blocked_private_route_count" != "0" ]]; then
        fail "Run65 private-route simulation invariant failed: blocked_private_route_count=$blocked_private_route_count"
        exit 1
    fi

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66Q_STEP="$run66q_step" \
        SHADOW_RUN66Q_RUN65_STEP="$run65_step" \
        "$PACK_DIR/scripts/66q_rebalancing_quote_minout_preservation.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66X_STEP="$run66x_step" \
        SHADOW_RUN66X_RUN65_STEP="$run65_step" \
        "$PACK_DIR/scripts/66x_exact_quote_identity_bridge_audit.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66Q2_STEP="$run66q2_step" \
        SHADOW_RUN66Q2_RUN66Q_STEP="$run66q_step" \
        SHADOW_RUN66Q2_RUN66X_STEP="$run66x_step" \
        "$PACK_DIR/scripts/66q2_rebalancing_exact_quote_route_ev.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66R_STEP="$run66rule_step" \
        SHADOW_RUN66R_RUN65_STEP="$run65_step" \
        SHADOW_RUN66R_RUN66Q_STEP="$run66q_step" \
        "$PACK_DIR/scripts/66_rebalancing_rule_proof.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66R_OVERLAY_STEP="$run66overlay_step" \
        SHADOW_RUN66R_OVERLAY_RUN66Q2_STEP="$run66q2_step" \
        SHADOW_RUN66R_OVERLAY_RUN66_STEP="$run66rule_step" \
        SHADOW_RUN66R_OVERLAY_RUN66A_STEP="${epoch_tag}_66a_not_run" \
        "$PACK_DIR/scripts/66r_rebalancing_exact_positive_readiness_overlay.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66R_ATA_STEP="$run66ata_step" \
        SHADOW_RUN66R_ATA_SOURCE_STEP="$run66overlay_step" \
        "$PACK_DIR/scripts/66r_ata_only_convertible_planner.sh"

    env \
        SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
        SHADOW_RUN66T_STEP="$run66t_step" \
        SHADOW_RUN66T_SOURCE_STEP="$run66ata_step" \
        "$PACK_DIR/scripts/66t_token2022_explicit_allow_research.sh"

    source_66t_steps+=("$run66t_step")

    if [[ "$epoch_index" -lt "$EPOCH_COUNT" && "$SLEEP_BETWEEN_SECONDS" -gt 0 ]]; then
        info "Sleeping $SLEEP_BETWEEN_SECONDS seconds before the next market moment"
        sleep "$SLEEP_BETWEEN_SECONDS"
    fi
done

source_steps_csv="$(IFS=,; echo "${source_66t_steps[*]}")"
aggregate_step="${STEP}_66u_aggregate"

env \
    SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
    SHADOW_RUN66U_STEP="$aggregate_step" \
    SHADOW_RUN66U_SOURCE_STEPS="$source_steps_csv" \
    SHADOW_RUN66U_MIN_EPOCHS="$MIN_EPOCHS" \
    bash "$PACK_DIR/scripts/66u_ata_repeatability_aggregate.sh"

{
    echo "status=run66u_repeatability_chain_complete"
    echo "step=$STEP"
    echo "epoch_count=$EPOCH_COUNT"
    echo "window_seconds=$WINDOW_SECONDS"
    echo "source_66t_steps=$source_steps_csv"
    echo "aggregate_step=$aggregate_step"
    echo "wire_send=0"
    echo "live_landed=0"
} > "$SUMMARY_FILE"

pass "Run66U repeatability chain written to $SUMMARY_FILE"
cat "$SUMMARY_FILE"
finish

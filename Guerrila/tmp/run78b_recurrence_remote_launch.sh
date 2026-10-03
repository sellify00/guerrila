#!/usr/bin/env bash
set -euo pipefail

cd /home/ubuntu/Guerrila
mkdir -p /home/ubuntu/shadow-host-audit/dpdk-runs/run78b_recurrence_logs

exec sudo -E env \
  SHADOW_RESULTS_DIR=/home/ubuntu/shadow-host-audit/dpdk-runs \
  SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF=true \
  SHADOW_RUN78_WINDOW_SECONDS=60 \
  SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY=true \
  SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE=1 \
  SHADOW_RUN78A_STEP=epoch8_auto_strict_directarb_quote_identity_run78b_recurrence_gated_diagnostic_retry1 \
  SHADOW_XDP_ATTACH_MODE=native \
  SHADOW_AFXDP_SAFE_MODE=false \
  SHADOW_RUN78_TPU_QUIC_WORKERS=0 \
  bash g1-host-pack/scripts/78a_fresh_selected_numeric_safe_directarb_tpu_tiny_live.sh \
  > /home/ubuntu/shadow-host-audit/dpdk-runs/run78b_recurrence_logs/run78b-recurrence-retry1.log 2>&1

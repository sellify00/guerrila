#!/usr/bin/env bash
set -euo pipefail

STEP="${1:?step required}"
INNER_STEP="${2:?inner step required}"
LOG_PATH="${3:?log path required}"

sudo pkill -f '__SHADOW_SAMPLE_BEGIN__' 2>/dev/null || true
mkdir -p "$(dirname "$LOG_PATH")"
cd /home/ubuntu/Guerrila

nohup sudo -E env \
  SHADOW_RESULTS_DIR=/home/ubuntu/shadow-host-audit/dpdk-runs \
  SHADOW_RUN78E_STEP="$STEP" \
  SHADOW_RUN78A_INNER_STEP="$INNER_STEP" \
  SHADOW_RUN78_WINDOW_SECONDS=60 \
  SHADOW_RUN78_TPU_QUIC_WORKERS=2 \
  SHADOW_RUN78_XDP_ATTACH_MODE=native \
  SHADOW_AFXDP_BIND_MODE=zero_copy \
  SHADOW_RUN78_AFXDP_SAFE_MODE=false \
  SHADOW_RUN78E_APPROVED_LIVE_CAP_SOL=0.1 \
  SHADOW_RUN78E_PROJECTION_RATIO_MAX=4 \
  SHADOW_RUN78A_REQUIRE_EGRESS_POLL_SUMMARY=true \
  SHADOW_RUN78A_STOP_AFTER_RING_ENQUEUE=1 \
  SHADOW_RUN78A_RESTORE_COMBINED=16 \
  SHADOW_RUN78_DIAGNOSTIC_RING_HANDOFF=true \
  bash g1-host-pack/scripts/78e_fresh_live_eligible_directarb_tpu_nosend.sh \
  >"$LOG_PATH" 2>&1 < /dev/null &

printf 'PID=%s\n' "$!"
printf 'LOG=%s\n' "$LOG_PATH"

#!/usr/bin/env bash
# R1-AFXDP-ENO1-SAFE — guarded eno1 AF_XDP observe/no-send overrides.
#
# Usage:
#   source g1-host-pack/env.g1.sh
#   source g1-host-pack/env.r1-afxdp-eno1-safe.sh

set -a

SHADOW_NIC_IFACE="eno1"
SHADOW_NIC_QUEUE=0
SHADOW_XDP_ATTACH_MODE="xdpgeneric"
SHADOW_AFXDP_SAFE_MODE="true"
SHADOW_ALLOW_MANAGEMENT_NIC_XDP="true"

set +a

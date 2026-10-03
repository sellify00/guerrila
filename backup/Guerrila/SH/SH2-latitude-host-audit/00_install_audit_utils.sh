#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PACKAGES=(
    ethtool
    numactl
    pciutils
    jq
    curl
    chrony
)

require_sudo

info "Installing SH2 audit utilities"
sudo apt-get update
sudo apt-get install -y "${PACKAGES[@]}"
pass "Audit utilities installed"

#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PACKAGES=(
    ufw
    fail2ban
    chrony
)

require_sudo

info "Installing baseline security packages"
sudo apt-get update
sudo apt-get install -y "${PACKAGES[@]}"

info "Applying default firewall posture"
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow OpenSSH
sudo ufw --force enable

info "Enabling chrony"
sudo systemctl enable --now chrony
timedatectl

pass "Baseline security applied without opening Solana ports or public RPC"

#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PACKAGES=(
    build-essential
    clang
    libclang-18-dev
    llvm
    cmake
    pkg-config
    libssl-dev
    libudev-dev
    protobuf-compiler
    git
    curl
    python3
    ca-certificates
)

require_cmd bash

if ! command -v sudo >/dev/null 2>&1 || ! sudo -n true >/dev/null 2>&1; then
    fail "Passwordless sudo is unavailable. Install these packages manually first:"
    printf '  %s\n' "${PACKAGES[@]}" >&2
    exit 1
fi

info "Installing SH1 build dependencies"
sudo apt-get update
sudo apt-get install -y "${PACKAGES[@]}"
pass "Build dependencies installed"

#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_cmd git
require_cmd cargo
require_cmd rustc
ensure_base_dirs
setup_native_build_env

if [[ ! -d "$SH1_AGAVE_REPO_DIR/.git" ]]; then
    info "Cloning Agave into $SH1_AGAVE_REPO_DIR"
    git clone https://github.com/anza-xyz/agave.git "$SH1_AGAVE_REPO_DIR"
fi

info "Updating Agave checkout to $SH1_AGAVE_GIT_REF"
git -C "$SH1_AGAVE_REPO_DIR" fetch --tags --prune
git -C "$SH1_AGAVE_REPO_DIR" checkout "$SH1_AGAVE_GIT_REF"

info "Building Agave validator + CLI binaries"
(
    cd "$SH1_AGAVE_REPO_DIR"
    cargo build --release \
        --bin agave-validator \
        --bin solana \
        --bin solana-keygen \
        --bin solana-test-validator
)

"$(agave_bin agave-validator)" --version
"$(agave_bin solana)" --version
"$(agave_bin solana-test-validator)" --version
pass "Agave binaries built"

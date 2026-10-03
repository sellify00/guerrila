#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ensure_base_dirs

KEYGEN_BIN="$(agave_bin solana-keygen || true)"
if [[ -z "$KEYGEN_BIN" ]]; then
    fail "solana-keygen not found. Run 10_build_agave.sh first."
    exit 1
fi

if [[ ! -f "$SH1_IDENTITY_PATH" ]]; then
    info "Creating validator identity at $SH1_IDENTITY_PATH"
    "$KEYGEN_BIN" new --no-bip39-passphrase --silent -o "$SH1_IDENTITY_PATH"
else
    info "Validator identity already exists at $SH1_IDENTITY_PATH"
fi

pass "Lab directories and identity are ready"

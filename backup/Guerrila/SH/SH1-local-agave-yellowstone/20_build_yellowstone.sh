#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_cmd git
require_cmd cargo
require_cmd rustc
require_cmd pkg-config
setup_native_build_env

if ! command -v protoc >/dev/null 2>&1; then
    fail "Missing protoc. Run 00_install_build_deps.sh or add protobuf-compiler to PATH before building Yellowstone."
    exit 1
fi

ensure_base_dirs

if [[ ! -d "$SH1_YELLOWSTONE_REPO_DIR/.git" ]]; then
    info "Cloning Yellowstone into $SH1_YELLOWSTONE_REPO_DIR"
    git clone https://github.com/rpcpool/yellowstone-grpc.git "$SH1_YELLOWSTONE_REPO_DIR"
fi

info "Updating Yellowstone checkout to $SH1_YELLOWSTONE_GIT_REF"
git -C "$SH1_YELLOWSTONE_REPO_DIR" fetch --tags --prune
git -C "$SH1_YELLOWSTONE_REPO_DIR" checkout "$SH1_YELLOWSTONE_GIT_REF"

info "Building Yellowstone workspace"
(
    cd "$SH1_YELLOWSTONE_REPO_DIR"
    cargo build --release --workspace
)

PLUGIN_PATH="$(yellowstone_plugin_path || true)"
if [[ -z "$PLUGIN_PATH" ]]; then
    fail "Yellowstone plugin .so was not produced under target/release"
    exit 1
fi

info "Writing Yellowstone config to $SH1_YELLOWSTONE_CONFIG_PATH"
bash "$(dirname "${BASH_SOURCE[0]}")/50_write_yellowstone_config.sh"

info "Running Yellowstone config-check"
(
    cd "$SH1_YELLOWSTONE_REPO_DIR"
    cargo run --release --bin config-check -- --config "$SH1_YELLOWSTONE_CONFIG_PATH"
)

pass "Yellowstone plugin built: $PLUGIN_PATH"

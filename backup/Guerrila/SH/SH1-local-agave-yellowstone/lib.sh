#!/usr/bin/env bash

set -euo pipefail

SH1_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SH1_DIR}/env.sh" ]]; then
    # shellcheck disable=SC1091
    source "${SH1_DIR}/env.sh"
elif [[ -f "${SH1_DIR}/env.sh.example" ]]; then
    # shellcheck disable=SC1091
    source "${SH1_DIR}/env.sh.example"
fi

: "${SH1_ROOT:=${SH1_DIR}/lab}"
: "${SH1_CLUSTER:=devnet}"
: "${SH1_RPC_BIND_ADDRESS:=127.0.0.1}"
: "${SH1_RPC_PORT:=8899}"
: "${SH1_DYNAMIC_PORT_RANGE:=8000-8025}"
: "${SH1_IDENTITY_PATH:=${SH1_ROOT}/validator-keypair.json}"
: "${SH1_LEDGER_DIR:=${SH1_ROOT}/ledger}"
: "${SH1_SNAPSHOTS_DIR:=${SH1_LEDGER_DIR}/snapshots}"
: "${SH1_ACCOUNTS_DIR:=${SH1_ROOT}/accounts}"
: "${SH1_LOG_DIR:=${SH1_ROOT}/logs}"
: "${SH1_TOOLING_DIR:=${SH1_ROOT}/tooling}"
: "${SH1_MAXIMUM_FULL_SNAPSHOTS_TO_RETAIN:=2}"
: "${SH1_MAXIMUM_INCREMENTAL_SNAPSHOTS_TO_RETAIN:=4}"
: "${SH1_RAYON_GLOBAL_THREADS:=}"
: "${SH1_REPLAY_TRANSACTIONS_THREADS:=}"
: "${SH1_AGAVE_REPO_DIR:=${SH1_ROOT}/agave}"
: "${SH1_AGAVE_GIT_REF:=master}"
: "${SH1_YELLOWSTONE_REPO_DIR:=${SH1_ROOT}/yellowstone-grpc}"
: "${SH1_YELLOWSTONE_GIT_REF:=master}"
: "${SH1_YELLOWSTONE_CONFIG_PATH:=${SH1_ROOT}/yellowstone-config.json}"
: "${SH1_YELLOWSTONE_GRPC_ADDRESS:=127.0.0.1:10000}"
: "${SH1_YELLOWSTONE_PROMETHEUS_ADDRESS:=127.0.0.1:8999}"
: "${SH1_YELLOWSTONE_X_TOKEN:=}"
: "${SH1_SOLANA_METRICS_CONFIG:=}"
: "${SH1_LIBCLANG_PATH:=}"

info() {
    printf '[sh1] %s\n' "$*"
}

fail() {
    printf '[sh1] ERROR: %s\n' "$*" >&2
}

pass() {
    printf '[sh1] OK: %s\n' "$*"
}

require_cmd() {
    local name="$1"
    if ! command -v "$name" >/dev/null 2>&1; then
        fail "Missing required command: $name"
        exit 1
    fi
}

require_file() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        fail "Missing file: $path"
        exit 1
    fi
}

require_dir() {
    local path="$1"
    if [[ ! -d "$path" ]]; then
        fail "Missing directory: $path"
        exit 1
    fi
}

ensure_base_dirs() {
    mkdir -p "$SH1_ROOT" "$SH1_LEDGER_DIR" "$SH1_SNAPSHOTS_DIR" "$SH1_ACCOUNTS_DIR" "$SH1_LOG_DIR" "$SH1_TOOLING_DIR"
}

agave_bin() {
    local name="$1"
    local local_path="${SH1_AGAVE_REPO_DIR}/target/release/${name}"
    if [[ -x "$local_path" ]]; then
        printf '%s\n' "$local_path"
        return 0
    fi
    if command -v "$name" >/dev/null 2>&1; then
        command -v "$name"
        return 0
    fi
    return 1
}

yellowstone_plugin_path() {
    local local_path="${SH1_YELLOWSTONE_REPO_DIR}/target/release/libyellowstone_grpc_geyser.so"
    if [[ -f "$local_path" ]]; then
        printf '%s\n' "$local_path"
        return 0
    fi
    if [[ -d "${SH1_YELLOWSTONE_REPO_DIR}/target/release" ]]; then
        find "${SH1_YELLOWSTONE_REPO_DIR}/target/release" -maxdepth 1 -name 'libyellowstone_grpc_geyser*.so' -print -quit
        return $?
    fi
    return 1
}

yellowstone_client_manifest() {
    printf '%s\n' "${SH1_YELLOWSTONE_REPO_DIR}/examples/rust/Cargo.toml"
}

resolve_libclang_path() {
    local candidate

    if [[ -n "$SH1_LIBCLANG_PATH" ]]; then
        printf '%s\n' "$SH1_LIBCLANG_PATH"
        return 0
    fi

    for candidate in \
        /usr/lib/llvm-18/lib \
        /usr/lib/llvm-17/lib \
        /usr/lib/llvm-16/lib \
        /usr/lib/llvm-15/lib \
        /usr/lib/x86_64-linux-gnu
    do
        if [[ -e "${candidate}/libclang.so" ]] || [[ -n "$(find "$candidate" -maxdepth 1 -name 'libclang*.so*' -print -quit 2>/dev/null)" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done

    return 1
}

setup_native_build_env() {
    local resolved

    resolved="$(resolve_libclang_path || true)"
    if [[ -n "$resolved" ]]; then
        export LIBCLANG_PATH="$resolved"
    fi
}

set_cluster_defaults() {
    case "$SH1_CLUSTER" in
        devnet)
            SH1_CLUSTER_RPC_URL="https://api.devnet.solana.com"
            SH1_EXPECTED_GENESIS="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG"
            SH1_ENTRYPOINTS=(
                "entrypoint.devnet.solana.com:8001"
                "entrypoint2.devnet.solana.com:8001"
                "entrypoint3.devnet.solana.com:8001"
                "entrypoint4.devnet.solana.com:8001"
                "entrypoint5.devnet.solana.com:8001"
            )
            SH1_KNOWN_VALIDATORS=(
                "dv1ZAGvdsz5hHLwWXsVnM94hWf1pjbKVau1QVkaMJ92"
                "dv2eQHeP4RFrJZ6UeiZWoc3XTtmtZCUKxxCApCDcRNV"
                "dv4ACNkpYPcE3aKmYDqZm9G5EB3J4MRoeE7WNDRBVJB"
                "dv3qDFk1DTF36Z62bNvrCXe9sKATA6xvVy6A798xxAS"
            )
            ;;
        testnet)
            SH1_CLUSTER_RPC_URL="https://api.testnet.solana.com"
            SH1_EXPECTED_GENESIS="4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY"
            SH1_ENTRYPOINTS=(
                "entrypoint.testnet.solana.com:8001"
                "entrypoint2.testnet.solana.com:8001"
                "entrypoint3.testnet.solana.com:8001"
            )
            SH1_KNOWN_VALIDATORS=(
                "5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on"
                "dDzy5SR3AXdYWVqbDEkVFdvSPCtS9ihF5kJkHCtXoFs"
                "Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN"
                "eoKpUABi59aT4rR9HGS3LcMecfut9x7zJyodWWP43YQ"
                "9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv"
            )
            ;;
        mainnet|mainnet-beta)
            SH1_CLUSTER="mainnet-beta"
            SH1_CLUSTER_RPC_URL="https://api.mainnet-beta.solana.com"
            SH1_EXPECTED_GENESIS="5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
            SH1_ENTRYPOINTS=(
                "entrypoint.mainnet-beta.solana.com:8001"
                "entrypoint2.mainnet-beta.solana.com:8001"
                "entrypoint3.mainnet-beta.solana.com:8001"
                "entrypoint4.mainnet-beta.solana.com:8001"
                "entrypoint5.mainnet-beta.solana.com:8001"
            )
            SH1_KNOWN_VALIDATORS=(
                "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2"
                "GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ"
                "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ"
                "CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S"
            )
            ;;
        *)
            fail "Unsupported SH1_CLUSTER=${SH1_CLUSTER}; use devnet, testnet, or mainnet-beta"
            exit 1
            ;;
    esac
}

append_cluster_args() {
    local -n args_ref="$1"
    local validator entrypoint

    for validator in "${SH1_KNOWN_VALIDATORS[@]}"; do
        args_ref+=(--known-validator "$validator")
    done
    args_ref+=(--only-known-rpc)

    for entrypoint in "${SH1_ENTRYPOINTS[@]}"; do
        args_ref+=(--entrypoint "$entrypoint")
    done
    args_ref+=(--expected-genesis-hash "$SH1_EXPECTED_GENESIS")
}

build_validator_args() {
    local target_name="$1"
    local -n args_ref="$target_name"
    local log_file="$2"
    local rayon_global_threads replay_transactions_threads

    set_cluster_defaults
    replay_transactions_threads="$(
        if [[ "${SH1_REPLAY_TRANSACTIONS_THREADS:-}" =~ ^[0-9]+$ ]]; then
            printf '%s\n' "$SH1_REPLAY_TRANSACTIONS_THREADS"
        else
            nproc --all 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1\n'
        fi
    )"
    rayon_global_threads="$(
        if [[ "${SH1_RAYON_GLOBAL_THREADS:-}" =~ ^[0-9]+$ ]]; then
            printf '%s\n' "$SH1_RAYON_GLOBAL_THREADS"
        else
            nproc --all 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1\n'
        fi
    )"
    if (( replay_transactions_threads < 1 )); then
        replay_transactions_threads=1
    elif (( replay_transactions_threads > 36 )); then
        replay_transactions_threads=36
    fi
    if (( rayon_global_threads < 1 )); then
        rayon_global_threads=1
    elif (( rayon_global_threads > 36 )); then
        rayon_global_threads=36
    fi
    args_ref=(
        --identity "$SH1_IDENTITY_PATH"
        --no-voting
        --full-rpc-api
        --private-rpc
        --ledger "$SH1_LEDGER_DIR"
        --snapshots "$SH1_SNAPSHOTS_DIR"
        --accounts "$SH1_ACCOUNTS_DIR"
        --log "$log_file"
        --rpc-port "$SH1_RPC_PORT"
        --rpc-bind-address "$SH1_RPC_BIND_ADDRESS"
        --dynamic-port-range "$SH1_DYNAMIC_PORT_RANGE"
        --wal-recovery-mode skip_any_corrupted_record
        --limit-ledger-size
        --maximum-full-snapshots-to-retain "$SH1_MAXIMUM_FULL_SNAPSHOTS_TO_RETAIN"
        --maximum-incremental-snapshots-to-retain "$SH1_MAXIMUM_INCREMENTAL_SNAPSHOTS_TO_RETAIN"
        --replay-transactions-threads "$replay_transactions_threads"
        --rayon-global-threads "$rayon_global_threads"
    )
    append_cluster_args "$target_name"
}

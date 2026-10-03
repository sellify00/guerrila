#!/usr/bin/env bash

set -euo pipefail

SH2_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SH2_DIR}/env.sh" ]]; then
    # shellcheck disable=SC1091
    source "${SH2_DIR}/env.sh"
elif [[ -f "${SH2_DIR}/env.sh.example" ]]; then
    # shellcheck disable=SC1091
    source "${SH2_DIR}/env.sh.example"
fi

: "${SH2_AUDIT_DIR:=${HOME}/shadow-host-audit}"
: "${SH2_HOST_TAG:=shadow-solana-tokyo-sh2}"
: "${SH2_IFACE:=}"

info() {
    printf '[sh2] %s\n' "$*"
}

fail() {
    printf '[sh2] ERROR: %s\n' "$*" >&2
}

pass() {
    printf '[sh2] OK: %s\n' "$*"
}

require_cmd() {
    local name="$1"
    if ! command -v "$name" >/dev/null 2>&1; then
        fail "Missing required command: $name"
        exit 1
    fi
}

ensure_audit_dir() {
    mkdir -p "$SH2_AUDIT_DIR"
}

require_sudo() {
    if ! command -v sudo >/dev/null 2>&1 || ! sudo -n true >/dev/null 2>&1; then
        fail "Passwordless sudo is unavailable."
        exit 1
    fi
}

resolve_iface() {
    local candidate="${1:-${SH2_IFACE}}"
    if [[ -z "$candidate" ]]; then
        fail "No interface supplied. Export SH2_IFACE or pass the interface as the first argument."
        exit 1
    fi
    printf '%s\n' "$candidate"
}

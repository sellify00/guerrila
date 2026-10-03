#!/usr/bin/env bash
# local_b002_runtime_env.sh — local Windows/WSL runtime wiring for B002 frozen lane.
#
# This file intentionally uses the local repo .env for current RPC/LaserStream
# credentials and only uses the Latitude backup for signer/key files. Do not
# source the backed-up /etc/shadow-engine/env.g1.local.sh here; that host
# subscription may be stale/suspended after migration.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/.." && pwd)"

# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"

LOCAL_SHADOW_ETC="${SHADOW_LOCAL_BACKUP_ETC_DIR:-$HOME/.shadow-engine-latitude15/etc/shadow-engine}"
if [[ ! -d "$LOCAL_SHADOW_ETC" ]]; then
    printf 'LOCAL_B002_NOT_READY: missing staged key/env directory: %s\n' "$LOCAL_SHADOW_ETC" >&2
    return 1 2>/dev/null || exit 1
fi

export SHADOW_ETC_DIR="$LOCAL_SHADOW_ETC"
export SHADOW_SIGNER_PATH="$LOCAL_SHADOW_ETC/worker-signer.json"
export SHADOW_LAUNCH5_SIGNER_KEYPAIR_PATH="$SHADOW_SIGNER_PATH"
export SHADOW_DEPLOYER_KEYPAIR_PATH="$LOCAL_SHADOW_ETC/shadow-guard-deployer.json"
export SHADOW_SIGNER_TOKEN_INVENTORY_PATH="$LOCAL_SHADOW_ETC/signer-token-inventory.json"

export SHADOW_RUNLS13_BINARY="${SHADOW_RUNLS13_BINARY:-$REPO_ROOT/target/release-guerrila/runls13-preprocessed-probe}"
if [[ ! -x "$SHADOW_RUNLS13_BINARY" && -x "$PACK_DIR/artifacts/release-guerrila/runls13-preprocessed-probe" ]]; then
    export SHADOW_RUNLS13_BINARY="$PACK_DIR/artifacts/release-guerrila/runls13-preprocessed-probe"
fi

export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-${HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}}"

# Frozen B002 lane identity. Keep this separate from R4/BREATHE lab settings.
export SHADOW_RISK_TIER="${SHADOW_RISK_TIER:-R3_COLD_CAPITAL_PRESS}"
export SHADOW_R2_MAX_SPEND_SOL="${SHADOW_R2_MAX_SPEND_SOL:-0.01}"
export SHADOW_R2_MAX_LOSS_SOL="${SHADOW_R2_MAX_LOSS_SOL:-0.015}"
export SHADOW_R2_MAX_CASH_AT_RISK_SOL="${SHADOW_R2_MAX_CASH_AT_RISK_SOL:-0.025}"
export SHADOW_R2_MAX_ATTEMPTS="${SHADOW_R2_MAX_ATTEMPTS:-3}"
export SHADOW_LAUNCH_STOP_AFTER_SUCCESS="${SHADOW_LAUNCH_STOP_AFTER_SUCCESS:-false}"
export SHADOW_LAUNCH_HOLD_V2_ENABLED="false"
export SHADOW_LAUNCH_HUNTER_MODE="false"
export SHADOW_LAUNCH_BATCH002_SIMILARITY_REQUIRED="false"
export SHADOW_LAUNCH_ENTRY_EDGE_ENABLED="false"

if [[ ! -f "$SHADOW_SIGNER_PATH" ]]; then
    printf 'LOCAL_B002_NOT_READY: signer missing: %s\n' "$SHADOW_SIGNER_PATH" >&2
    return 1 2>/dev/null || exit 1
fi
if [[ ! -f "$SHADOW_SIGNER_TOKEN_INVENTORY_PATH" ]]; then
    printf 'LOCAL_B002_NOT_READY: signer inventory missing: %s\n' "$SHADOW_SIGNER_TOKEN_INVENTORY_PATH" >&2
    return 1 2>/dev/null || exit 1
fi
if [[ ! -x "$SHADOW_RUNLS13_BINARY" ]]; then
    printf 'LOCAL_B002_NOT_READY: RunLS13 binary missing/non-executable: %s\n' "$SHADOW_RUNLS13_BINARY" >&2
    return 1 2>/dev/null || exit 1
fi

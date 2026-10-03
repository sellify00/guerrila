#!/usr/bin/env bash
# local_b002_live.sh — live B002 frozen clone from local WSL runtime.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/local_b002_runtime_env.sh"

WINDOW_SECONDS="${1:-3600}"
CONFIRM="${SHADOW_LAUNCH_EXECUTE_CONFIRM:-EXECUTE_B002_LOCAL_FROZEN_001}"
export SHADOW_LAUNCH_EXECUTE_CONFIRM="$CONFIRM"
export SHADOW_R2_EXECUTE_CONFIRM="$CONFIRM"
export SHADOW_R2_EXECUTE_CONFIRM_REQUIRED="$CONFIRM"
export SHADOW_LAUNCH_B002_SKIP_REPLAY_PREFLIGHT="${SHADOW_LAUNCH_B002_SKIP_REPLAY_PREFLIGHT:-false}"

exec "$SCRIPT_DIR/22_launch_b002_live_clone_runner.sh" "$WINDOW_SECONDS"


#!/usr/bin/env bash

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ensure_base_dirs

PLUGIN_PATH="$(yellowstone_plugin_path || true)"
if [[ -z "$PLUGIN_PATH" ]]; then
    fail "Yellowstone plugin .so not found. Run 20_build_yellowstone.sh first."
    exit 1
fi

X_TOKEN_JSON="null"
if [[ -n "$SH1_YELLOWSTONE_X_TOKEN" ]]; then
    X_TOKEN_JSON="\"${SH1_YELLOWSTONE_X_TOKEN}\""
fi

mkdir -p "$(dirname "$SH1_YELLOWSTONE_CONFIG_PATH")"
cat >"$SH1_YELLOWSTONE_CONFIG_PATH" <<EOF
{
  "libpath": "${PLUGIN_PATH}",
  "log": {
    "level": "info"
  },
  "grpc": {
    "address": "${SH1_YELLOWSTONE_GRPC_ADDRESS}",
    "x_token": ${X_TOKEN_JSON},
    "filter_limits": {
      "slots": {
        "max": 1
      },
      "transactions": {
        "max": 1,
        "any": false,
        "account_include_max": 16,
        "account_exclude_max": 16,
        "account_required_max": 16
      }
    }
  },
  "prometheus": {
    "address": "${SH1_YELLOWSTONE_PROMETHEUS_ADDRESS}"
  }
}
EOF

pass "Yellowstone config written to $SH1_YELLOWSTONE_CONFIG_PATH"

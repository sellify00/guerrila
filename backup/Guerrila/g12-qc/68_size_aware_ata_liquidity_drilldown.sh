#!/usr/bin/env bash
# Thin mirror wrapper for the canonical Run68 implementation.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/../g1-host-pack/scripts/68_size_aware_ata_liquidity_drilldown.sh" "$@"

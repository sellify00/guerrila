#!/usr/bin/env bash
# 22_launch15_sell_quality_markout_backtest.sh — compatibility wrapper for Launch15 chaos/sell-quality backtest

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/22_launch15_chaos_detector_backtest.sh" "$@"

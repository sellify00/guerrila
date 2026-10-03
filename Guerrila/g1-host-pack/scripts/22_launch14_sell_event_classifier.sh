#!/usr/bin/env bash
# 22_launch14_sell_event_classifier.sh — compatibility wrapper for Launch14 sell-quality classification

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/22_launch14_chaos_catastrophe_detector.sh" "$@"

#!/usr/bin/env bash
# 22_launch2_bonding_curve_entry_exit_ladder.sh — compatibility entrypoint for bonding-curve entry/exit size ladder gating

exec bash "$(dirname "$0")/22_launch2_policy_exit_gate.sh" "$@"

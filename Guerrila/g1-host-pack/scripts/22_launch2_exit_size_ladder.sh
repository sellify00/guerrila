#!/usr/bin/env bash
# 22_launch2_exit_size_ladder.sh — compatibility entrypoint for launch-follow exit-size gating

exec bash "$(dirname "$0")/22_launch2_policy_exit_gate.sh" "$@"

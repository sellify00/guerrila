#!/usr/bin/env bash
# 22_launch3_policy_creator_risk.sh — compatibility entrypoint for launch-follow policy/creator-risk plus tiny-band sim

exec bash "$(dirname "$0")/22_launch3_tiny_inventory_sim.sh" "$@"

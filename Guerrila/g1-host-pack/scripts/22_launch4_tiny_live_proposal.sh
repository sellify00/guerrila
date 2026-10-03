#!/usr/bin/env bash
# 22_launch4_tiny_live_proposal.sh — compatibility entrypoint for launch-follow tiny live proposal packaging

exec bash "$(dirname "$0")/22_launch4_one_mint_live_proposal.sh" "$@"

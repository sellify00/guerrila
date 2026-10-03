#!/usr/bin/env bash
# 22_launch0_preprocessed_birth_census.sh — compatibility entrypoint for preprocessed launch birth / first-buy census

exec bash "$(dirname "$0")/22_launch0_new_mint_pool_census.sh" "$@"

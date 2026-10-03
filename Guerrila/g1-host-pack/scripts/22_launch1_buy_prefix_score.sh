#!/usr/bin/env bash
# 22_launch1_buy_prefix_score.sh — compatibility entrypoint for buy-prefix scoring

exec bash "$(dirname "$0")/22_launch1_preprocessed_buy_pressure_score.sh" "$@"

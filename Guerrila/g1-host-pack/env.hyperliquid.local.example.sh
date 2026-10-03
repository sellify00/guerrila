#!/usr/bin/env bash
# Copy to g1-host-pack/env.hyperliquid.local.sh or
# /etc/shadow-engine/env.hyperliquid.local.sh and fill locally.
# Do not commit the local file.

# Funded/master Hyperliquid account address. This is the account whose margin,
# positions, and open orders are checked through the Info endpoint.
export SHADOW_HYPERLIQUID_ACCOUNT_ADDRESS=""

# API wallet address + private key. The API wallet signs actions for the master
# account after it is approved in Hyperliquid.
export SHADOW_HYPERLIQUID_API_WALLET_ADDRESS=""
export SHADOW_HYPERLIQUID_SECRET_KEY=""

# Keep empty until you intentionally want the readiness gate to pass its
# confirmation check. This runner still does not submit exchange actions.
export SHADOW_DERIV19_EXECUTE_CONFIRM=""

# Optional readiness tuning.
export SHADOW_DERIV19_TINY_NOTIONAL_USD="25"
export SHADOW_DERIV19_MIN_ACCOUNT_VALUE_USD="25"

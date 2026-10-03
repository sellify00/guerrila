#!/usr/bin/env bash
# G1 Host-Run Pack — Environment Variables
#
# Fill in every value below before running any validation script.
# Then:  source env.g1.sh

set -a  # auto-export all variables

load_repo_dotenv_var() {
  local key="$1"
  local dotenv_path="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.env}"
  [[ -f "$dotenv_path" ]] || return 0
  awk -F= -v k="$key" '$1 == k { value = substr($0, length($1) + 2); sub(/\r$/, "", value); print value; exit }' "$dotenv_path"
}

HELIUS_RPC_FROM_DOTENV="${HELIUS_RPC_URL:-$(load_repo_dotenv_var HELIUS_RPC_URL)}"
HELIUS_GATEKEEPER_FROM_DOTENV="${HELIUS_GATEKEEPER:-$(load_repo_dotenv_var HELIUS_GATEKEEPER)}"

# ── State / RPC endpoint split ────────────────────────────────────
# Triton gRPC stays reserved for live state/account streaming only.
SHADOW_GEYSER_ENDPOINT="https://CHANGE_ME.triton.one"
SHADOW_GEYSER_TOKEN="CHANGE_ME"
SHADOW_GRPC_FALLBACK_WORKERS="1"

# HTTP RPC used by shadow-state for bootstrap / hydration / targeted reads.
# When the repository .env defines HELIUS_RPC_URL, default to that so Triton
# stays gRPC-only unless the operator overrides these explicitly.
SHADOW_RPC_URL="${SHADOW_RPC_URL:-${HELIUS_RPC_FROM_DOTENV:-https://CHANGE_ME.rpc}}"
SHADOW_BOOTSTRAP_RPC_URL="${SHADOW_BOOTSTRAP_RPC_URL:-${HELIUS_RPC_FROM_DOTENV:-${SHADOW_RPC_URL}}}"

# Local Agave gating for AF_XDP/DPDK/scout runs. These guards fail closed
# before starting ingest when the host-local validator is absent, stale, or
# not listening on the expected dynamic Solana UDP range.
SHADOW_REQUIRE_LOCAL_AGAVE_PREFLIGHT="true"
SHADOW_LOCAL_AGAVE_SERVICE_NAMES="sh1-agave-no-geyser.service sh1-agave-no-geyser-2.service sh1-agave-no-geyser"
SHADOW_LOCAL_AGAVE_RPC_URL="http://127.0.0.1:8899"
# Leave blank to inherit the post-override bootstrap RPC from env.g1.local.sh.
SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL=""
SHADOW_LOCAL_AGAVE_MAX_PROCESSED_SLOT_LAG="2048"
SHADOW_LOCAL_AGAVE_MAX_FINALIZED_SLOT_LAG="4096"
SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MIN="8000"
SHADOW_LOCAL_AGAVE_DYNAMIC_PORT_MAX="8025"
SHADOW_LOCAL_AGAVE_REQUIRE_DYNAMIC_PORTS="true"
SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE="sh1-agave-no-geyser-2.service"
SHADOW_LOCAL_AGAVE_REQUIRE_LIMIT_LEDGER_SIZE="true"
SHADOW_LOCAL_AGAVE_MIN_LEDGER_FREE_GIB="100"
SHADOW_LOCAL_AGAVE_MIN_ACCOUNTS_FREE_GIB="100"

# HTTP RPC used by PM for blockhash / signature status polling.
SHADOW_SOLANA_RPC_URL="${SHADOW_SOLANA_RPC_URL:-${HELIUS_RPC_FROM_DOTENV:-https://CHANGE_ME.rpc}}"

# HTTP RPC used by explicit public_rpc sendTransaction.
SHADOW_PUBLIC_RPC_URL="${SHADOW_PUBLIC_RPC_URL:-${HELIUS_GATEKEEPER_FROM_DOTENV:-${HELIUS_RPC_FROM_DOTENV:-${SHADOW_SOLANA_RPC_URL}}}}"

# ── Bundle status endpoints (PM evidence plane) ──────────────────
SHADOW_JITO_BUNDLE_STATUS_URL="https://mainnet.block-engine.jito.wtf/api/v1/bundles"
SHADOW_HARMONIC_BUNDLE_STATUS_URL="https://CHANGE_ME"

# ── Credential paths (must exist on the host) ────────────────────
SHADOW_ETC_DIR="/etc/shadow-engine"
SHADOW_JITO_AUTH_PATH="${SHADOW_ETC_DIR}/jito-auth-keypair.json"
SHADOW_HARMONIC_AUTH_PATH="${SHADOW_ETC_DIR}/harmonic-auth-keypair.json"
SHADOW_SIGNER_PATH="${SHADOW_ETC_DIR}/worker-signer.json"
SHADOW_DEPLOYER_KEYPAIR_PATH="${SHADOW_ETC_DIR}/shadow-guard-deployer.json"
SHADOW_ALLOWLIST_PATH="${SHADOW_ETC_DIR}/allowlist.bin"

# ── NIC / AF_XDP ─────────────────────────────────────────────────
SHADOW_NIC_IFACE="enp65s0f0"        # ConnectX-4+ or E810/X710
SHADOW_NIC_QUEUE=0                   # AF_XDP queue index
SHADOW_XSKMAP_PIN="/sys/fs/bpf/shadow_xsks_map"
SHADOW_XDP_ATTACH_MODE="native"     # native | xdpgeneric

# ── Core pinning (adjust to match host NUMA topology) ────────────
SHADOW_MONEY_CORES="4-7"            # money worker + nonce cores
SHADOW_INGRESS_CORES="8-9"          # ingress / AF_XDP cores
SHADOW_EGRESS_CORES="10-11"         # egress proxy cores
SHADOW_UTILITY_CORES="12-15"        # control / PM / telemetry
SHADOW_LOCAL_AGAVE_CPU_AFFINITY="0-3,16-47"  # leave engine lanes isolated from local validator replay/RPC
SHADOW_SHADOW_STATE_CPU_AFFINITY="${SHADOW_UTILITY_CORES}"  # long-lived state service stays off validator cores
SHADOW_RUN29C_TIER0_CPU_AFFINITY="${SHADOW_MONEY_CORES},${SHADOW_INGRESS_CORES},${SHADOW_EGRESS_CORES}"
SHADOW_RUN29C_ORACLE_CPU_AFFINITY="${SHADOW_UTILITY_CORES}"

# ── Hugepages (number of 2 MB pages) ─────────────────────────────
SHADOW_HUGEPAGES=4096

# ── Paths ─────────────────────────────────────────────────────────
SHADOW_PACK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SHADOW_RESULTS_DIR="${SHADOW_PACK_DIR}/results"
SHADOW_PM_SHOCK_RESEARCH_EXPORT_PATH="/home/ubuntu/shadow-host-audit/shock-research/shock_research.jsonl"

# ── G12 rollout helpers ───────────────────────────────────────────
SHADOW_G12_R0_WINDOW_SECONDS=1800
SHADOW_G12_R0_READINESS_TIMEOUT_SECONDS=60
SHADOW_G12_R0_POLL_SECONDS=10
SHADOW_GUARD_SMOKE_OK="0"

# Load host-local secrets/endpoint overrides when present.
if [[ -f "${SHADOW_ETC_DIR}/env.g1.local.sh" ]]; then
  if [[ -r "${SHADOW_ETC_DIR}/env.g1.local.sh" ]]; then
    # shellcheck disable=SC1091
    source "${SHADOW_ETC_DIR}/env.g1.local.sh"
  elif command -v sudo >/dev/null 2>&1 && sudo -n test -r "${SHADOW_ETC_DIR}/env.g1.local.sh" 2>/dev/null; then
    # shellcheck disable=SC1090
    source <(sudo -n cat "${SHADOW_ETC_DIR}/env.g1.local.sh")
  else
    printf '%s\n' "WARN env.g1.local.sh exists but is not readable: ${SHADOW_ETC_DIR}/env.g1.local.sh" >&2
  fi
fi

set +a

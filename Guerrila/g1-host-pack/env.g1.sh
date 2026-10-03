#!/usr/bin/env bash
# G1 Host-Run Pack — Environment Variables
#
# Fill in every value below before running any validation script.
# Then:  source env.g1.sh

set -a  # auto-export all variables

load_repo_dotenv_var() {
  local key="$1"
  local repo_root=""
  if [[ -n "${SHADOW_REPO_ROOT_DIR:-}" ]]; then
    repo_root="$SHADOW_REPO_ROOT_DIR"
  elif [[ -n "${REPO_ROOT_DIR:-}" ]]; then
    repo_root="$REPO_ROOT_DIR"
  elif [[ -n "${PACK_DIR:-}" ]]; then
    repo_root="$(cd "$PACK_DIR/.." && pwd)"
  elif [[ "${BASH_SOURCE[0]}" == "/dev/stdin" && -n "${PWD:-}" && -f "${PWD}/.env" ]]; then
    repo_root="$PWD"
  else
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
  local dotenv_path="${2:-$repo_root/.env}"
  [[ -f "$dotenv_path" ]] || return 0
  awk -F= -v k="$key" '$1 == k { value = substr($0, length($1) + 2); sub(/\r$/, "", value); print value; exit }' "$dotenv_path"
}

HELIUS_RPC_FROM_DOTENV="${HELIUS_RPC_URL:-$(load_repo_dotenv_var HELIUS_RPC_URL)}"
HELIUS_BUNDLE_RPC_FROM_DOTENV="${HELIUS_BUNDLE_RPC_URL:-$(load_repo_dotenv_var HELIUS_BUNDLE_RPC_URL)}"
if [[ -z "$HELIUS_BUNDLE_RPC_FROM_DOTENV" ]]; then
  HELIUS_BUNDLE_RPC_FROM_DOTENV="$HELIUS_RPC_FROM_DOTENV"
fi
HELIUS_HARMONIC_BUNDLE_RPC_FROM_DOTENV="${HELIUS_HARMONIC_BUNDLE_RPC_URL:-$(load_repo_dotenv_var HELIUS_HARMONIC_BUNDLE_RPC_URL)}"
HELIUS_GATEKEEPER_FROM_DOTENV="${HELIUS_GATEKEEPER_URL:-${HELIUS_GATEKEEPER:-$(load_repo_dotenv_var HELIUS_GATEKEEPER_URL)}}"
if [[ -z "$HELIUS_GATEKEEPER_FROM_DOTENV" ]]; then
  HELIUS_GATEKEEPER_FROM_DOTENV="$(load_repo_dotenv_var HELIUS_GATEKEEPER)"
fi
LASERSTREAM_GRPC_URL_FROM_DOTENV="${HELIUS_LASERSTREAM_URL:-${LASERSTREAM_GRPC_URL:-$(load_repo_dotenv_var HELIUS_LASERSTREAM_URL)}}"
if [[ -z "$LASERSTREAM_GRPC_URL_FROM_DOTENV" ]]; then
  LASERSTREAM_GRPC_URL_FROM_DOTENV="$(load_repo_dotenv_var LASERSTREAM_GRPC_URL)"
fi
LASERSTREAM_X_TOKEN_FROM_DOTENV="${HELIUS_LASERSTREAM_API_KEY:-${LASERSTREAM_X_TOKEN:-$(load_repo_dotenv_var HELIUS_LASERSTREAM_API_KEY)}}"
if [[ -z "$LASERSTREAM_X_TOKEN_FROM_DOTENV" ]]; then
  LASERSTREAM_X_TOKEN_FROM_DOTENV="$(load_repo_dotenv_var LASERSTREAM_X_TOKEN)"
fi

infer_grpc_source_grade() {
  local endpoint="$1"
  case "${endpoint,,}" in
    *laserstream*)
      printf '%s\n' "laserstream"
      ;;
    *)
      printf '%s\n' "external_geyser"
      ;;
  esac
}

# ── State / RPC endpoint split ────────────────────────────────────
# LaserStream / Yellowstone-style gRPC is continuity/enrichment ingress only.
# It must not replace AF_XDP as the hot private-grade live trigger.
SHADOW_GEYSER_ENDPOINT="${SHADOW_GEYSER_ENDPOINT:-${LASERSTREAM_GRPC_URL_FROM_DOTENV:-https://CHANGE_ME.triton.one}}"
SHADOW_GEYSER_TOKEN="${SHADOW_GEYSER_TOKEN:-${LASERSTREAM_X_TOKEN_FROM_DOTENV:-CHANGE_ME}}"
SHADOW_GRPC_FALLBACK_WORKERS="1"
SHADOW_GRPC_SOURCE_GRADE="${SHADOW_GRPC_SOURCE_GRADE:-$(infer_grpc_source_grade "$SHADOW_GEYSER_ENDPOINT")}"

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

# HTTP RPC used by Helius-mediated Jito bundle methods in no-send/readiness
# checks. This is intentionally separate from Helius Sender's single-tx API key.
SHADOW_HELIUS_BUNDLE_RPC_URL="${SHADOW_HELIUS_BUNDLE_RPC_URL:-${HELIUS_BUNDLE_RPC_FROM_DOTENV:-${SHADOW_SOLANA_RPC_URL}}}"
SHADOW_HELIUS_BUNDLE_API_KEY="${SHADOW_HELIUS_BUNDLE_API_KEY:-}"
SHADOW_HELIUS_HARMONIC_BUNDLE_RPC_URL="${SHADOW_HELIUS_HARMONIC_BUNDLE_RPC_URL:-${HELIUS_HARMONIC_BUNDLE_RPC_FROM_DOTENV:-}}"
SHADOW_HELIUS_HARMONIC_BUNDLE_API_KEY="${SHADOW_HELIUS_HARMONIC_BUNDLE_API_KEY:-}"

# Helius Sender is the default plain private single-tx egress lane. True
# bundle-only tactics still stay on Jito via route-aware dispatch.
SHADOW_HELIUS_SENDER_ENABLED="${SHADOW_HELIUS_SENDER_ENABLED:-true}"
SHADOW_HELIUS_SENDER_ENDPOINT="${SHADOW_HELIUS_SENDER_ENDPOINT:-https://sender.helius-rpc.com/fast}"
SHADOW_HELIUS_SENDER_TIMEOUT_MS="${SHADOW_HELIUS_SENDER_TIMEOUT_MS:-3000}"
SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS="${SHADOW_HELIUS_SENDER_MIN_TIP_LAMPORTS:-200000}"
SHADOW_HELIUS_SENDER_API_KEY="${SHADOW_HELIUS_SENDER_API_KEY:-}"

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
SHADOW_NIC_IFACE="eno1"             # .15 live Solana feed / Agave interface
SHADOW_NIC_QUEUE=0                   # AF_XDP queue index
SHADOW_XSKMAP_PIN="/sys/fs/bpf/shadow_xsks_map"
SHADOW_XDP_ATTACH_MODE="native"     # native | xdpgeneric
SHADOW_NIC_BASELINE_COMBINED_QUEUES="16"  # Agave-owned steady-state queue count on .15

# ── Core pinning (EPYC 9254 / .15 whole-core partition) ──────────
# SMT siblings on .15 are separated by +24. Keep Shadow and Agave on
# disjoint physical cores rather than splitting only logical siblings.
SHADOW_NETWORK_IRQ_CORES="8-9,32-33"  # NIC IRQ / softirq / AF_XDP ingest island
SHADOW_MONEY_CORES="10-11,34-35"      # tier0 hot-path workers
SHADOW_INGRESS_CORES="${SHADOW_NETWORK_IRQ_CORES}"  # ingress workers stay with NIC island
SHADOW_EGRESS_CORES="12,36"           # egress / sender workers
SHADOW_UTILITY_CORES="13,37"          # oracle / state / PM / telemetry
SHADOW_LOCAL_AGAVE_CPU_AFFINITY="0-7,14-31,38-47"  # whole-core Agave partition
SHADOW_SHADOW_STATE_CPU_AFFINITY="${SHADOW_UTILITY_CORES}"  # long-lived state service stays off validator cores
SHADOW_RUN29C_TIER0_CPU_AFFINITY="${SHADOW_MONEY_CORES},${SHADOW_EGRESS_CORES}"
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
SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED="${SHADOW_LASERSTREAM_BACKRUN_NO_SEND_ENABLED:-false}"

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

# Repair derived endpoint vars after host-local overrides and /dev/stdin sourcing.
if [[ -n "${HELIUS_LASERSTREAM_URL:-}" ]]; then
  LASERSTREAM_GRPC_URL_FROM_DOTENV="${HELIUS_LASERSTREAM_URL}"
elif [[ -n "${LASERSTREAM_GRPC_URL:-}" ]]; then
  LASERSTREAM_GRPC_URL_FROM_DOTENV="${LASERSTREAM_GRPC_URL}"
fi
if [[ -n "${HELIUS_LASERSTREAM_API_KEY:-}" ]]; then
  LASERSTREAM_X_TOKEN_FROM_DOTENV="${HELIUS_LASERSTREAM_API_KEY}"
elif [[ -n "${LASERSTREAM_X_TOKEN:-}" ]]; then
  LASERSTREAM_X_TOKEN_FROM_DOTENV="${LASERSTREAM_X_TOKEN}"
fi
if [[ -z "${SHADOW_GEYSER_ENDPOINT:-}" || "${SHADOW_GEYSER_ENDPOINT}" == *CHANGE_ME* ]]; then
  SHADOW_GEYSER_ENDPOINT="${LASERSTREAM_GRPC_URL_FROM_DOTENV:-https://CHANGE_ME.triton.one}"
fi
if [[ -z "${SHADOW_GEYSER_TOKEN:-}" || "${SHADOW_GEYSER_TOKEN}" == "CHANGE_ME" ]]; then
  SHADOW_GEYSER_TOKEN="${LASERSTREAM_X_TOKEN_FROM_DOTENV:-CHANGE_ME}"
fi
if [[ -z "${SHADOW_RPC_URL:-}" || "${SHADOW_RPC_URL}" == *CHANGE_ME* ]]; then
  SHADOW_RPC_URL="${HELIUS_RPC_FROM_DOTENV:-https://CHANGE_ME.rpc}"
fi
if [[ -z "${SHADOW_BOOTSTRAP_RPC_URL:-}" || "${SHADOW_BOOTSTRAP_RPC_URL}" == *CHANGE_ME* ]]; then
  SHADOW_BOOTSTRAP_RPC_URL="${HELIUS_RPC_FROM_DOTENV:-${SHADOW_RPC_URL}}"
fi
if [[ -z "${SHADOW_SOLANA_RPC_URL:-}" || "${SHADOW_SOLANA_RPC_URL}" == *CHANGE_ME* ]]; then
  SHADOW_SOLANA_RPC_URL="${HELIUS_RPC_FROM_DOTENV:-https://CHANGE_ME.rpc}"
fi
if [[ -z "${SHADOW_PUBLIC_RPC_URL:-}" || "${SHADOW_PUBLIC_RPC_URL}" == *CHANGE_ME* ]]; then
  SHADOW_PUBLIC_RPC_URL="${HELIUS_GATEKEEPER_FROM_DOTENV:-${HELIUS_RPC_FROM_DOTENV:-${SHADOW_SOLANA_RPC_URL}}}"
fi
if [[ -z "${SHADOW_GRPC_SOURCE_GRADE:-}" || "${SHADOW_GRPC_SOURCE_GRADE}" == "external_geyser" && "${SHADOW_GEYSER_ENDPOINT:-}" == *laserstream* ]]; then
  SHADOW_GRPC_SOURCE_GRADE="$(infer_grpc_source_grade "$SHADOW_GEYSER_ENDPOINT")"
fi

set +a

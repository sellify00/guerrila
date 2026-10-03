#!/usr/bin/env bash
# 90_deriv19_hyperliquid_tiny_live_readiness.sh
#
# No-submit Hyperliquid tiny-live readiness gate for the filtered derivatives
# lane. It verifies account/market/order-shape readiness and refuses to submit
# exchange actions. A later runner can consume the ready row only after explicit
# credentials, SDK/signing, and execute confirmation are present.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv19 - Hyperliquid Tiny Live Readiness"
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required" >&2
  exit 127
fi

if [[ -f "${PACK_DIR}/env.g1.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/env.g1.sh"
fi

CALLER_SHADOW_DERIV19_EXECUTE_CONFIRM="${SHADOW_DERIV19_EXECUTE_CONFIRM:-}"
CALLER_SHADOW_DERIV19_SOURCE_ROOT="${SHADOW_DERIV19_SOURCE_ROOT:-}"

for env_file in \
  "/etc/shadow-engine/env.hyperliquid.local.sh" \
  "${PACK_DIR}/env.hyperliquid.local.sh"; do
  if [[ -f "${env_file}" ]]; then
    # shellcheck source=/dev/null
    source "${env_file}"
  fi
done

if [[ -n "${CALLER_SHADOW_DERIV19_EXECUTE_CONFIRM}" ]]; then
  export SHADOW_DERIV19_EXECUTE_CONFIRM="${CALLER_SHADOW_DERIV19_EXECUTE_CONFIRM}"
fi
if [[ -n "${CALLER_SHADOW_DERIV19_SOURCE_ROOT}" ]]; then
  export SHADOW_DERIV19_SOURCE_ROOT="${CALLER_SHADOW_DERIV19_SOURCE_ROOT}"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-${PACK_DIR}/results}"
mkdir -p "${RESULTS_ROOT}"

SOURCE_ROOT="${1:-${SHADOW_DERIV19_SOURCE_ROOT:-}}"
if [[ -z "${SOURCE_ROOT}" && -f "${RESULTS_ROOT}/90_deriv18_filtered_size_ladder_repeatability_watch_latest_root.txt" ]]; then
  SOURCE_ROOT="$(cat "${RESULTS_ROOT}/90_deriv18_filtered_size_ladder_repeatability_watch_latest_root.txt")"
fi
if [[ -z "${SOURCE_ROOT}" ]]; then
  echo "missing Deriv18 source root; pass it as arg or set SHADOW_DERIV19_SOURCE_ROOT" >&2
  exit 2
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV19_OUT_DIR:-${RESULTS_ROOT}/90_deriv19_hyperliquid_tiny_live_readiness_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv19_hyperliquid_tiny_live_readiness_latest_root.txt"

PYTHON_BIN="${SHADOW_DERIV_PYTHON:-}"
if [[ -z "${PYTHON_BIN}" && -x "${PACK_DIR}/.venv-deriv/bin/python" ]]; then
  PYTHON_BIN="${PACK_DIR}/.venv-deriv/bin/python"
fi
if [[ -z "${PYTHON_BIN}" ]]; then
  PYTHON_BIN="python3"
fi

"${PYTHON_BIN}" "${PACK_DIR}/scripts/derivatives_research_tools.py" hyperliquid-live-readiness \
  --source-root "${SOURCE_ROOT}" \
  --out-dir "${OUT_DIR}" \
  --tiny-notional-usd "${SHADOW_DERIV19_TINY_NOTIONAL_USD:-25}" \
  --min-source-notional-usd "${SHADOW_DERIV19_MIN_SOURCE_NOTIONAL_USD:-100}" \
  --min-source-net-bps "${SHADOW_DERIV19_MIN_SOURCE_NET_BPS:-5}" \
  --max-slippage-bps "${SHADOW_DERIV19_MAX_SLIPPAGE_BPS:-10}" \
  --min-account-value-usd "${SHADOW_DERIV19_MIN_ACCOUNT_VALUE_USD:-25}" \
  --max-open-positions "${SHADOW_DERIV19_MAX_OPEN_POSITIONS:-0}" \
  --max-open-orders "${SHADOW_DERIV19_MAX_OPEN_ORDERS:-0}" \
  --execute-confirm "${SHADOW_DERIV19_EXECUTE_CONFIRM:-}" \
  --required-execute-confirm "${SHADOW_DERIV19_REQUIRED_EXECUTE_CONFIRM:-EXECUTE_DERIV19_TINY_LIVE_001}"

SUMMARY_JSON="${OUT_DIR}/summary.json"
if [[ ! -f "${SUMMARY_JSON}" ]]; then
  echo "missing summary: ${SUMMARY_JSON}" >&2
  exit 1
fi

wire_send="$("${PYTHON_BIN}" - "${SUMMARY_JSON}" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    print(json.load(f).get("wire_send", ""))
PY
)"
live_landed="$("${PYTHON_BIN}" - "${SUMMARY_JSON}" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    print(json.load(f).get("live_landed", ""))
PY
)"

if [[ "${wire_send}" != "0" || "${live_landed}" != "0" ]]; then
  echo "no-wire invariant failed: wire_send=${wire_send} live_landed=${live_landed}" >&2
  exit 2
fi

cat "${OUT_DIR}/summary.txt"

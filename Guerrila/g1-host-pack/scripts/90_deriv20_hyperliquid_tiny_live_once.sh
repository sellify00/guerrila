#!/usr/bin/env bash
# 90_deriv20_hyperliquid_tiny_live_once.sh
#
# One-shot tiny Hyperliquid live runner. It consumes a DERIV19 readiness row,
# rechecks account/book state, submits only with an exact execute-confirm, then
# monitors and reduce-only flattens the position.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv20 - Hyperliquid Tiny Live Once"
fi

if [[ -f "${PACK_DIR}/env.g1.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/env.g1.sh"
fi

CALLER_SHADOW_DERIV20_EXECUTE_CONFIRM="${SHADOW_DERIV20_EXECUTE_CONFIRM:-}"
CALLER_SHADOW_DERIV20_READINESS_ROOT="${SHADOW_DERIV20_READINESS_ROOT:-}"

for env_file in \
  "/etc/shadow-engine/env.hyperliquid.local.sh" \
  "${PACK_DIR}/env.hyperliquid.local.sh"; do
  if [[ -f "${env_file}" ]]; then
    # shellcheck source=/dev/null
    source "${env_file}"
  fi
done

if [[ -n "${CALLER_SHADOW_DERIV20_EXECUTE_CONFIRM}" ]]; then
  export SHADOW_DERIV20_EXECUTE_CONFIRM="${CALLER_SHADOW_DERIV20_EXECUTE_CONFIRM}"
fi
if [[ -n "${CALLER_SHADOW_DERIV20_READINESS_ROOT}" ]]; then
  export SHADOW_DERIV20_READINESS_ROOT="${CALLER_SHADOW_DERIV20_READINESS_ROOT}"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-${PACK_DIR}/results}"
mkdir -p "${RESULTS_ROOT}"

READINESS_ROOT="${1:-${SHADOW_DERIV20_READINESS_ROOT:-}}"
if [[ -z "${READINESS_ROOT}" && -f "${RESULTS_ROOT}/90_deriv19_hyperliquid_tiny_live_readiness_latest_root.txt" ]]; then
  READINESS_ROOT="$(cat "${RESULTS_ROOT}/90_deriv19_hyperliquid_tiny_live_readiness_latest_root.txt")"
fi
if [[ -z "${READINESS_ROOT}" ]]; then
  echo "missing Deriv19 readiness root; pass it as arg or set SHADOW_DERIV20_READINESS_ROOT" >&2
  exit 2
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV20_OUT_DIR:-${RESULTS_ROOT}/90_deriv20_hyperliquid_tiny_live_once_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv20_hyperliquid_tiny_live_once_latest_root.txt"

PYTHON_BIN="${SHADOW_DERIV_PYTHON:-}"
if [[ -z "${PYTHON_BIN}" && -x "${PACK_DIR}/.venv-deriv/bin/python" ]]; then
  PYTHON_BIN="${PACK_DIR}/.venv-deriv/bin/python"
fi
if [[ -z "${PYTHON_BIN}" ]]; then
  PYTHON_BIN="python3"
fi

"${PYTHON_BIN}" "${PACK_DIR}/scripts/derivatives_research_tools.py" hyperliquid-tiny-live-once \
  --readiness-root "${READINESS_ROOT}" \
  --out-dir "${OUT_DIR}" \
  --notional-usd "${SHADOW_DERIV20_NOTIONAL_USD:-0}" \
  --min-usdc-available "${SHADOW_DERIV20_MIN_USDC_AVAILABLE:-25}" \
  --min-usdc-to-notional-ratio "${SHADOW_DERIV20_MIN_USDC_TO_NOTIONAL_RATIO:-1.0}" \
  --max-open-positions "${SHADOW_DERIV20_MAX_OPEN_POSITIONS:-0}" \
  --max-open-orders "${SHADOW_DERIV20_MAX_OPEN_ORDERS:-0}" \
  --max-readiness-age-seconds "${SHADOW_DERIV20_MAX_READINESS_AGE_SECONDS:-60}" \
  --max-signal-entry-age-seconds "${SHADOW_DERIV20_MAX_SIGNAL_ENTRY_AGE_SECONDS:-90}" \
  --max-source-last-seen-age-seconds "${SHADOW_DERIV20_MAX_SOURCE_LAST_SEEN_AGE_SECONDS:-90}" \
  --min-live-trigger-bps "${SHADOW_DERIV20_MIN_LIVE_TRIGGER_BPS:-70}" \
  --max-live-followthrough-reversal-bps "${SHADOW_DERIV20_MAX_LIVE_FOLLOWTHROUGH_REVERSAL_BPS:-15}" \
  --max-entry-spread-bps "${SHADOW_DERIV20_MAX_ENTRY_SPREAD_BPS:-12}" \
  --max-entry-book-drag-bps "${SHADOW_DERIV20_MAX_ENTRY_BOOK_DRAG_BPS:-8}" \
  --min-depth-multiple "${SHADOW_DERIV20_MIN_DEPTH_MULTIPLE:-3}" \
  --max-slippage-bps "${SHADOW_DERIV20_MAX_SLIPPAGE_BPS:-10}" \
  --max-hold-seconds "${SHADOW_DERIV20_MAX_HOLD_SECONDS:-300}" \
  --poll-seconds "${SHADOW_DERIV20_POLL_SECONDS:-2}" \
  --adverse-stop-bps "${SHADOW_DERIV20_ADVERSE_STOP_BPS:-25}" \
  --decay-exit-fraction "${SHADOW_DERIV20_DECAY_EXIT_FRACTION:-0.30}" \
  --min-peak-for-decay-bps "${SHADOW_DERIV20_MIN_PEAK_FOR_DECAY_BPS:-35}" \
  --max-close-attempts "${SHADOW_DERIV20_MAX_CLOSE_ATTEMPTS:-5}" \
  --execute-confirm "${SHADOW_DERIV20_EXECUTE_CONFIRM:-}" \
  --required-execute-confirm "${SHADOW_DERIV20_REQUIRED_EXECUTE_CONFIRM:-EXECUTE_DERIV20_TINY_LIVE_ONCE_001}"

SUMMARY_JSON="${OUT_DIR}/summary.json"
if [[ ! -f "${SUMMARY_JSON}" ]]; then
  echo "missing summary: ${SUMMARY_JSON}" >&2
  exit 1
fi

cat "${OUT_DIR}/summary.txt"

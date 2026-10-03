#!/usr/bin/env bash
# 90_deriv22_hyperliquid_fresh_trigger_watch.sh
#
# Live-shaped no-wire Hyperliquid trigger watch. Unlike Deriv17, this does not
# wait for a completed forward markout before emitting readiness; it identifies
# the current impulse and writes a Deriv20-compatible ready row immediately.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv22 - Hyperliquid Fresh Trigger Watch"
fi

if [[ -f "${PACK_DIR}/env.g1.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/env.g1.sh"
fi

for env_file in \
  "/etc/shadow-engine/env.hyperliquid.local.sh" \
  "${PACK_DIR}/env.hyperliquid.local.sh"; do
  if [[ -f "${env_file}" ]]; then
    # shellcheck source=/dev/null
    source "${env_file}"
  fi
done

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-${PACK_DIR}/results}"
mkdir -p "${RESULTS_ROOT}"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV22_OUT_DIR:-${RESULTS_ROOT}/90_deriv22_hyperliquid_fresh_trigger_watch_${RUN_ID}}"
COST_ROWS="${SHADOW_DERIV22_COST_ROWS:-${RESULTS_ROOT}/90_deriv1_fee_funding_cost_model/rows.jsonl}"

mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv22_hyperliquid_fresh_trigger_watch_latest_root.txt"

PYTHON_BIN="${SHADOW_DERIV_PYTHON:-}"
if [[ -z "${PYTHON_BIN}" && -x "${PACK_DIR}/.venv-deriv/bin/python" ]]; then
  PYTHON_BIN="${PACK_DIR}/.venv-deriv/bin/python"
fi
if [[ -z "${PYTHON_BIN}" ]]; then
  PYTHON_BIN="python3"
fi

args=()
if [[ -f "${COST_ROWS}" ]]; then
  args+=(--cost-rows "${COST_ROWS}")
fi

"${PYTHON_BIN}" "${PACK_DIR}/scripts/derivatives_research_tools.py" hyperliquid-fresh-trigger-watch \
  --out-dir "${OUT_DIR}" \
  "${args[@]}" \
  --duration-seconds "${SHADOW_DERIV22_DURATION_SECONDS:-900}" \
  --interval-seconds "${SHADOW_DERIV22_INTERVAL_SECONDS:-5}" \
  --trigger-window-seconds "${SHADOW_DERIV22_TRIGGER_WINDOW_SECONDS:-60}" \
  --min-window-fraction "${SHADOW_DERIV22_MIN_WINDOW_FRACTION:-0.75}" \
  --notional-usd "${SHADOW_DERIV22_NOTIONAL_USD:-25}" \
  --min-trigger-bps "${SHADOW_DERIV22_MIN_TRIGGER_BPS:-70}" \
  --min-projected-edge-bps "${SHADOW_DERIV22_MIN_PROJECTED_EDGE_BPS:-30}" \
  --default-required-edge-bps "${SHADOW_DERIV22_DEFAULT_REQUIRED_EDGE_BPS:-36.125}" \
  --min-day-notional-volume "${SHADOW_DERIV22_MIN_DAY_NOTIONAL_VOLUME:-1000000}" \
  --min-usdc-available "${SHADOW_DERIV22_MIN_USDC_AVAILABLE:-25}" \
  --max-open-positions "${SHADOW_DERIV22_MAX_OPEN_POSITIONS:-0}" \
  --max-open-orders "${SHADOW_DERIV22_MAX_OPEN_ORDERS:-0}" \
  --max-entry-spread-bps "${SHADOW_DERIV22_MAX_ENTRY_SPREAD_BPS:-12}" \
  --max-entry-book-drag-bps "${SHADOW_DERIV22_MAX_ENTRY_BOOK_DRAG_BPS:-8}" \
  --min-depth-multiple "${SHADOW_DERIV22_MIN_DEPTH_MULTIPLE:-3}" \
  --stop-after-ready

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

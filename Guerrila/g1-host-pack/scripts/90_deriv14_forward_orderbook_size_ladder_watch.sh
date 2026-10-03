#!/usr/bin/env bash
# 90_deriv14_forward_orderbook_size_ladder_watch.sh
#
# No-wire forward size-ladder paper watch for listed-perp impulse continuation.
# Each live-detected trigger records executable entry and exit book fills for a
# notional ladder, so size viability is measured in-window on both sides.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv14 - Forward Orderbook Size Ladder Watch"
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required" >&2
  exit 127
fi

if [[ -f "${PACK_DIR}/env.g1.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/env.g1.sh"
fi

RESULTS_ROOT="${SHADOW_RESULTS_DIR:-${PACK_DIR}/results}"
mkdir -p "${RESULTS_ROOT}"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${SHADOW_DERIV14_OUT_DIR:-${RESULTS_ROOT}/90_deriv14_forward_orderbook_size_ladder_watch_${RUN_ID}}"
COST_ROWS="${SHADOW_DERIV14_COST_ROWS:-${RESULTS_ROOT}/90_deriv1_fee_funding_cost_model/rows.jsonl}"

mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv14_forward_orderbook_size_ladder_watch_latest_root.txt"

args=()
if [[ -f "${COST_ROWS}" ]]; then
  args+=(--cost-rows "${COST_ROWS}")
fi

python3 "${PACK_DIR}/scripts/derivatives_research_tools.py" forward-orderbook-size-ladder-watch \
  --out-dir "${OUT_DIR}" \
  "${args[@]}" \
  --duration-seconds "${SHADOW_DERIV14_DURATION_SECONDS:-900}" \
  --interval-seconds "${SHADOW_DERIV_IMPULSE_INTERVAL_SECONDS:-5}" \
  --trigger-window-seconds "${SHADOW_DERIV_TRIGGER_WINDOW_SECONDS:-60}" \
  --min-trigger-bps "${SHADOW_DERIV_MIN_TRIGGER_BPS:-35}" \
  --min-net-edge-bps "${SHADOW_DERIV_MIN_NET_EDGE_BPS:-10}" \
  --min-day-notional-volume "${SHADOW_DERIV_MIN_DAY_NOTIONAL_VOLUME:-1000000}" \
  --notional-usd "${SHADOW_DERIV14_NOTIONAL_USD:-25,100,250,500}" \
  --min-open-notional-usd "${SHADOW_DERIV14_MIN_OPEN_NOTIONAL_USD:-25}" \
  --already-modeled-spread-bps "${SHADOW_DERIV_ALREADY_MODELED_SPREAD_BPS:-2}" \
  --min-net-after-orderbook-bps "${SHADOW_DERIV14_MIN_NET_AFTER_ORDERBOOK_BPS:-5}" \
  --max-spread-bps "${SHADOW_DERIV14_MAX_ENTRY_SPREAD_BPS:-20}" \
  --max-entry-book-drag-bps "${SHADOW_DERIV14_MAX_ENTRY_BOOK_DRAG_BPS:-15}" \
  --max-exit-spread-bps "${SHADOW_DERIV14_MAX_EXIT_SPREAD_BPS:-25}" \
  --max-exit-book-drag-bps "${SHADOW_DERIV14_MAX_EXIT_BOOK_DRAG_BPS:-20}" \
  --min-depth-multiple "${SHADOW_DERIV14_MIN_DEPTH_MULTIPLE:-3}" \
  --adverse-stop-bps "${SHADOW_DERIV_ADVERSE_STOP_BPS:-25}" \
  --decay-exit-fraction "${SHADOW_DERIV_DECAY_EXIT_FRACTION:-0.30}" \
  --max-hold-seconds "${SHADOW_DERIV_MAX_HOLD_SECONDS:-900}"

SUMMARY_JSON="${OUT_DIR}/summary.json"
if [[ ! -f "${SUMMARY_JSON}" ]]; then
  echo "missing summary: ${SUMMARY_JSON}" >&2
  exit 1
fi

wire_send="$(python3 - "${SUMMARY_JSON}" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    print(json.load(f).get("wire_send", ""))
PY
)"
live_landed="$(python3 - "${SUMMARY_JSON}" <<'PY'
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

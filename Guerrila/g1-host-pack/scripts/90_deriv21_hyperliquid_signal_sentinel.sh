#!/usr/bin/env bash
# 90_deriv21_hyperliquid_signal_sentinel.sh
#
# Continuous no-wire sentinel for the derivatives impulse lane. Each cycle runs
# Deriv17, then Deriv19 readiness on the fresh root. It stops when a ready
# Hyperliquid candidate appears, or after the configured cycle count.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -f "${PACK_DIR}/scripts/lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${PACK_DIR}/scripts/lib.sh"
fi

if declare -f script_header >/dev/null 2>&1; then
  script_header "90 Deriv21 - Hyperliquid Signal Sentinel"
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
OUT_DIR="${SHADOW_DERIV21_OUT_DIR:-${RESULTS_ROOT}/90_deriv21_hyperliquid_signal_sentinel_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
printf '%s\n' "${OUT_DIR}" > "${RESULTS_ROOT}/90_deriv21_hyperliquid_signal_sentinel_latest_root.txt"

PYTHON_BIN="${SHADOW_DERIV_PYTHON:-}"
if [[ -z "${PYTHON_BIN}" && -x "${PACK_DIR}/.venv-deriv/bin/python" ]]; then
  PYTHON_BIN="${PACK_DIR}/.venv-deriv/bin/python"
fi
if [[ -z "${PYTHON_BIN}" ]]; then
  PYTHON_BIN="python3"
fi

cycles="${SHADOW_DERIV21_CYCLES:-24}"
window_seconds="${SHADOW_DERIV21_WINDOW_SECONDS:-900}"
sleep_seconds="${SHADOW_DERIV21_SLEEP_SECONDS:-30}"

cycles_jsonl="${OUT_DIR}/cycles.jsonl"
: > "${cycles_jsonl}"

latest_deriv17_root=""
latest_deriv19_root=""
ready_cycle=0
ready_symbol=""
ready_side=""
ready_net_bps=0

json_get() {
  local path="$1"
  local key="$2"
  "${PYTHON_BIN}" - "$path" "$key" <<'PY'
import json, sys
path, key = sys.argv[1], sys.argv[2]
try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    print("")
    raise SystemExit(0)
value = data
for part in key.split("."):
    if isinstance(value, dict):
        value = value.get(part, "")
    else:
        value = ""
        break
if isinstance(value, (dict, list)):
    print(json.dumps(value, sort_keys=True))
else:
    print(value)
PY
}

for cycle in $(seq 1 "${cycles}"); do
  echo "cycle=${cycle}/${cycles} window_seconds=${window_seconds}"
  export SHADOW_DERIV17_DURATION_SECONDS="${window_seconds}"
  bash "${PACK_DIR}/scripts/90_deriv17_filtered_forward_size_ladder_watch.sh"
  latest_deriv17_root="$(cat "${RESULTS_ROOT}/90_deriv17_filtered_forward_size_ladder_watch_latest_root.txt")"

  export SHADOW_DERIV19_EXECUTE_CONFIRM="EXECUTE_DERIV19_TINY_LIVE_001"
  bash "${PACK_DIR}/scripts/90_deriv19_hyperliquid_tiny_live_readiness.sh" "${latest_deriv17_root}"
  latest_deriv19_root="$(cat "${RESULTS_ROOT}/90_deriv19_hyperliquid_tiny_live_readiness_latest_root.txt")"

  d17_summary="${latest_deriv17_root}/summary.json"
  d19_summary="${latest_deriv19_root}/summary.json"
  d17_ready="$(json_get "${d17_summary}" "size_ladder_ready_rows")"
  d19_ready="$(json_get "${d19_summary}" "readiness_ready")"
  d19_symbol="$(json_get "${d19_summary}" "selected_symbol")"
  d19_side="$(json_get "${d19_summary}" "selected_side")"
  d19_net="$(json_get "${d19_summary}" "selected_source_net_after_orderbook_bps")"
  d19_wire="$(json_get "${d19_summary}" "wire_send")"
  d19_landed="$(json_get "${d19_summary}" "live_landed")"

  "${PYTHON_BIN}" - "${cycles_jsonl}" \
    "${cycle}" "${latest_deriv17_root}" "${latest_deriv19_root}" \
    "${d17_ready}" "${d19_ready}" "${d19_symbol}" "${d19_side}" "${d19_net}" \
    "${d19_wire}" "${d19_landed}" <<'PY'
import json, sys, time
path = sys.argv[1]
row = {
    "record_type": "deriv21_hyperliquid_signal_sentinel_cycle",
    "cycle": int(sys.argv[2]),
    "deriv17_root": sys.argv[3],
    "deriv19_root": sys.argv[4],
    "size_ladder_ready_rows": int(float(sys.argv[5] or 0)),
    "readiness_ready": str(sys.argv[6]).lower() == "true",
    "selected_symbol": sys.argv[7],
    "selected_side": sys.argv[8],
    "selected_source_net_after_orderbook_bps": float(sys.argv[9] or 0),
    "wire_send": int(float(sys.argv[10] or 0)),
    "live_landed": int(float(sys.argv[11] or 0)),
    "cycle_time": int(time.time()),
}
with open(path, "a", encoding="utf-8") as f:
    f.write(json.dumps(row, sort_keys=True) + "\n")
PY

  if [[ "${d19_ready}" == "True" || "${d19_ready}" == "true" ]]; then
    ready_cycle="${cycle}"
    ready_symbol="${d19_symbol}"
    ready_side="${d19_side}"
    ready_net_bps="${d19_net}"
    break
  fi

  if [[ "${cycle}" != "${cycles}" ]]; then
    sleep "${sleep_seconds}"
  fi
done

status="PARK_DERIV21_HYPERLIQUID_SIGNAL_SENTINEL_NO_READY"
if [[ "${ready_cycle}" != "0" ]]; then
  status="DERIV21_HYPERLIQUID_SIGNAL_READY"
fi

"${PYTHON_BIN}" - "${OUT_DIR}" "${status}" "${cycles}" "${ready_cycle}" \
  "${latest_deriv17_root}" "${latest_deriv19_root}" "${ready_symbol}" "${ready_side}" "${ready_net_bps}" <<'PY'
import json, pathlib, sys
out_dir = pathlib.Path(sys.argv[1])
summary = {
    "status": sys.argv[2],
    "configured_cycles": int(float(sys.argv[3] or 0)),
    "ready_cycle": int(float(sys.argv[4] or 0)),
    "latest_deriv17_root": sys.argv[5],
    "latest_deriv19_root": sys.argv[6],
    "ready_symbol": sys.argv[7],
    "ready_side": sys.argv[8],
    "ready_net_after_orderbook_bps": float(sys.argv[9] or 0),
    "exchange_submit_attempted": False,
    "no_wire": True,
    "wire_send": 0,
    "live_landed": 0,
}
(out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
lines = []
for key, value in summary.items():
    if isinstance(value, (dict, list)):
        value = json.dumps(value, sort_keys=True)
    lines.append(f"{key}={value}")
(out_dir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

cat "${OUT_DIR}/summary.txt"

#!/usr/bin/env bash
# 78G3 - fresh AF_XDP DirectArb-B Jito bundle no-submit epoch.

source "$(dirname "$0")/lib.sh"
script_header "78G3 - Fresh DirectArb-B AF_XDP Jito Bundle No-Submit"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
EPOCH_TAG="${SHADOW_RUN78G3_EPOCH_TAG:-epoch_directarb_b_fresh_$(date -u +%Y%m%d_%H%M%S)}"
RESULT_DIR="$RESULTS_ROOT/$EPOCH_TAG"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run78g3_directarb_b_fresh_epoch_nosubmit.json"

WINDOW_SECONDS="${SHADOW_RUN78G3_WINDOW_SECONDS:-300}"
POST_WINDOW_DRAIN_SECONDS="${SHADOW_RUN78G3_POST_WINDOW_DRAIN_SECONDS:-5}"
CAP_REVIEW_SOL="${SHADOW_RUN78G3_CAP_REVIEW_SOL:-1}"
RUN78_STEP="78g3_run78_cap1_fresh_jito_bundle_nosend"
C3_STEP="78c3_directarb_cap1_nearmiss_autopsy"
F_STEP="78f_directarb_exit_flatten_route_certificate"
MANIFEST_DIR="$RESULT_DIR/78g3_directarb_b_manifest"
MANIFEST_PATH="$MANIFEST_DIR/manifest.jsonl"
MANIFEST_SUMMARY="$MANIFEST_DIR/summary.txt"
G2_STEP="78g3_directarb_b_manifest_batch_nosubmit"

mkdir -p "$RESULT_DIR" "$MANIFEST_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON"

kv_get() {
    local path="$1"
    local key="$2"
    awk -F= -v key="$key" '$1 == key { value = substr($0, length($1) + 2) } END { print value }' "$path" 2>/dev/null
}

write_status() {
    local status="$1"
    {
        echo "status=$status"
        echo "result_dir=$RESULT_DIR"
        echo "window_seconds=$WINDOW_SECONDS"
        echo "cap_review_sol=$CAP_REVIEW_SOL"
        echo "run78_step=$RUN78_STEP"
        echo "c3_step=$C3_STEP"
        echo "f_step=$F_STEP"
        echo "manifest_path=$MANIFEST_PATH"
        echo "g2_step=$G2_STEP"
        echo "no_send=true"
        echo "disable_live_send=true"
        echo "disable_sendbundle=true"
    } > "$SUMMARY_FILE"
}

write_status "initializing"

export SHADOW_RESULTS_DIR="$RESULT_DIR"
export SHADOW_RUN78_STEP="$RUN78_STEP"
export SHADOW_RUN78_WINDOW_SECONDS="$WINDOW_SECONDS"
export SHADOW_RUN78_POST_WINDOW_DRAIN_SECONDS="$POST_WINDOW_DRAIN_SECONDS"
export SHADOW_RUN78_APPROVED_LIVE_CAP_SOL="$CAP_REVIEW_SOL"
export SHADOW_RUN78_SCOUT_ROUTE_FAMILY="jito_bundle"
export SHADOW_RUN78_PRIVATE_REPLAY_TARGET_ROUTE_FAMILY="jito_bundle"
export SHADOW_XDP_ATTACH_MODE="${SHADOW_RUN78G3_XDP_ATTACH_MODE:-native}"
export SHADOW_AFXDP_SAFE_MODE="${SHADOW_RUN78G3_AFXDP_SAFE_MODE:-false}"
export SHADOW_RUN78_XDP_ATTACH_MODE="${SHADOW_RUN78G3_XDP_ATTACH_MODE:-native}"
export SHADOW_RUN78_AFXDP_SAFE_MODE="${SHADOW_RUN78G3_AFXDP_SAFE_MODE:-false}"
export SHADOW_REQUIRE_AFXDP_ZERO_COPY="true"
export SHADOW_AFXDP_BIND_MODE="zero_copy"
export SHADOW_NO_SEND=true
export SHADOW_DISABLE_LIVE_SEND=true
export SHADOW_DISABLE_SENDBUNDLE=true
export SHADOW_ENABLE_JITO_VIA_HELIUS=true
export SHADOW_ENABLE_HELIUS_SENDER_DUAL=true
export SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}"

set +e
sudo -E bash "$PACK_DIR/scripts/78_fresh_strict_directarb_candidate_scout_nosend.sh"
RUN78_RC=$?
set -e

RUN78_SUMMARY="$RESULT_DIR/$RUN78_STEP/summary.txt"

set +e
SHADOW_RUN78C3_RUN78_STEP="$RUN78_STEP" \
    bash "$PACK_DIR/scripts/78c3_directarb_cap1_nearmiss_autopsy.sh" --epoch "$RESULT_DIR"
C3_RC=$?
set -e

set +e
SHADOW_RUN78F_RUN78_STEP="$RUN78_STEP" \
    bash "$PACK_DIR/scripts/78f_directarb_exit_flatten_route_certificate.sh" --epoch "$RESULT_DIR"
F_RC=$?
set -e

F_ROWS="$RESULT_DIR/$F_STEP/rows.jsonl"
C3_ROWS="$RESULT_DIR/$C3_STEP/rows.jsonl"
SOURCE_ROWS="$F_ROWS"
if [[ ! -s "$SOURCE_ROWS" ]]; then
    SOURCE_ROWS="$C3_ROWS"
fi

MANIFEST_RC=0
if [[ -s "$SOURCE_ROWS" ]]; then
    SHADOW_RUN78G_REPLAY_SOURCE_ROWS="$SOURCE_ROWS" \
    SHADOW_RUN78G_REPLAY_OUTPUT_PATH="$MANIFEST_PATH" \
    SHADOW_RUN78G_REPLAY_SUMMARY_PATH="$MANIFEST_SUMMARY" \
        bash "$PACK_DIR/scripts/78g_replay_directarb_bundle_manifest_from_78f.sh" || MANIFEST_RC=$?
else
    MANIFEST_RC=1
fi

G2_RC=0
if [[ -s "$MANIFEST_PATH" ]]; then
    SHADOW_RUN78G2_MANIFEST_PATH="$MANIFEST_PATH" \
    SHADOW_RUN78G2_STEP="$G2_STEP" \
        sudo -E bash "$PACK_DIR/scripts/78g2_directarb_bundle_manifest_batch_nosubmit.sh" || G2_RC=$?
else
    G2_RC=1
fi

G2_SUMMARY="$RESULT_DIR/$G2_STEP/summary.txt"
C3_SUMMARY="$RESULT_DIR/$C3_STEP/summary.txt"
F_SUMMARY="$RESULT_DIR/$F_STEP/summary.txt"

python3 - \
    "$SUMMARY_FILE" "$SUMMARY_JSON" "$RESULT_DIR" \
    "$RUN78_SUMMARY" "$C3_SUMMARY" "$F_SUMMARY" "$MANIFEST_SUMMARY" "$G2_SUMMARY" \
    "$RUN78_RC" "$C3_RC" "$F_RC" "$MANIFEST_RC" "$G2_RC" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
result_dir = Path(sys.argv[3])
paths = {
    "run78": Path(sys.argv[4]),
    "c3": Path(sys.argv[5]),
    "f": Path(sys.argv[6]),
    "manifest": Path(sys.argv[7]),
    "g2": Path(sys.argv[8]),
}
rcs = {
    "run78_rc": int(sys.argv[9]),
    "c3_rc": int(sys.argv[10]),
    "f_rc": int(sys.argv[11]),
    "manifest_rc": int(sys.argv[12]),
    "g2_rc": int(sys.argv[13]),
}


def kv(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    if not path.exists():
        return out
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        out[key.strip()] = value.strip()
    return out


def as_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


run78 = kv(paths["run78"])
c3 = kv(paths["c3"])
f = kv(paths["f"])
manifest = kv(paths["manifest"])
g2 = kv(paths["g2"])

wire_send = max(as_int(run78.get("wire_send")), as_int(g2.get("wire_send")))
live_landed = max(as_int(run78.get("live_landed")), as_int(g2.get("live_landed")))
send_bundle_called = str(g2.get("sendBundle_called", "false")).lower() == "true" or str(run78.get("sendBundle_called", "false")).lower() == "true"
would_send = as_int(g2.get("would_send_positive_count"))
default_would_send = as_int(g2.get("default_policy_would_send_positive_count"))

if wire_send or live_landed or send_bundle_called:
    status = "blocked_send_side_effect_observed"
elif would_send > 0:
    status = "directarb_b_would_send_positive_found"
elif as_int(manifest.get("manifest_rows")) <= 0:
    status = "blocked_no_directarb_b_manifest_rows"
else:
    status = "blocked_no_directarb_b_would_send_positive"

report = {
    "status": status,
    "result_dir": str(result_dir),
    **rcs,
    "run78_summary_path": str(paths["run78"]),
    "c3_summary_path": str(paths["c3"]),
    "f_summary_path": str(paths["f"]),
    "manifest_summary_path": str(paths["manifest"]),
    "g2_summary_path": str(paths["g2"]),
    "afxdp_directarb_rows": as_int(run78.get("afxdp_directarb_rows")),
    "fresh_strict_directarb_candidate_count": as_int(run78.get("fresh_strict_directarb_candidate_count")),
    "ev_positive_total": as_int(c3.get("ev_positive_total")),
    "would_pass_if_exit_route_ignored": as_int(c3.get("would_pass_if_exit_route_ignored")),
    "exit_route_exists_count": as_int(f.get("exit_route_exists_count")),
    "roundtrip_positive_count": as_int(f.get("roundtrip_positive_count")),
    "directarb_exit_ready_count": as_int(f.get("directarb_exit_ready_count")),
    "directarb_flatten_ready_no_send_count": as_int(f.get("directarb_flatten_ready_no_send_count")),
    "manifest_rows": as_int(manifest.get("manifest_rows")),
    "g2_evaluated_rows": as_int(g2.get("evaluated_rows")),
    "g2_decision_counts": g2.get("decision_counts", "{}"),
    "would_send_positive_count": would_send,
    "default_policy_would_send_positive_count": default_would_send,
    "positive_built_sim_pass_count": as_int(g2.get("positive_built_sim_pass_count")),
    "wire_send": wire_send,
    "live_landed": live_landed,
    "sendBundle_called": send_bundle_called,
}

lines = []
for key, value in report.items():
    if isinstance(value, bool):
        value = str(value).lower()
    lines.append(f"{key}={value}")
summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY

STATUS="$(kv_get "$SUMMARY_FILE" status)"
if [[ "$STATUS" == "directarb_b_would_send_positive_found" ]]; then
    pass "78G3 found DirectArb-B would-send positive row(s)"
else
    warn "78G3 completed without DirectArb-B would-send positive row"
fi
finish

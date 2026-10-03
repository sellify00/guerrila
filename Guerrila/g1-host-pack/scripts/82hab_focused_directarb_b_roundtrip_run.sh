#!/usr/bin/env bash
# 82HAB - run focused no-submit DirectArb-B windows from 81HAB selections.

source "$(dirname "$0")/lib.sh"
script_header "82HAB - Focused DirectArb-B Roundtrip No-Submit"

assert_root
assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN82HAB_STEP:-82hab_focused_directarb_b_roundtrip_$(date -u +%Y%m%d_%H%M%S)}"
RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/run82hab_focused_directarb_b_roundtrip_run.json"
ROWS_JSONL="$RESULT_DIR/rows.jsonl"
ROWS_CSV="$RESULT_DIR/rows.csv"
SELECTED_HABITATS_JSON="${SHADOW_RUN82HAB_SELECTED_HABITATS_JSON:-$RESULTS_ROOT/81hab_directarb_roundtrip_poolbook_selector/selected_habitats.json}"
TOP_N="${SHADOW_RUN82HAB_TOP_N:-3}"
WINDOW_SECONDS="${SHADOW_RUN82HAB_WINDOW_SECONDS:-180}"
CAP_REVIEW_SOL="${SHADOW_RUN82HAB_CAP_REVIEW_SOL:-1}"
OOB_ARM_FILE="${SHADOW_RUN39_NATIVE_OOB_ARM_FILE:-$PACK_DIR/results/run39_native_oob_ready.flag}"

if [[ ! -f "$SELECTED_HABITATS_JSON" ]]; then
    fail "Selected habitat manifest missing: $SELECTED_HABITATS_JSON"
    finish
    exit 1
fi

mkdir -p "$RESULT_DIR"
rm -f "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV"

readarray -t SELECTED_LINES < <(python3 - "$SELECTED_HABITATS_JSON" "$TOP_N" <<'PY'
from __future__ import annotations
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
top_n = max(1, int(sys.argv[2] or "3"))
rows = json.loads(path.read_text(encoding="utf-8")) if path.exists() else []
if not isinstance(rows, list):
    rows = []
for idx, row in enumerate(rows[:top_n], 1):
    print("\t".join([
        str(idx),
        str(row.get("selected_pool") or row.get("pool") or ""),
        str(row.get("selected_mint_in") or row.get("mint_in") or ""),
        str(row.get("selected_mint_out") or row.get("mint_out") or ""),
        str(row.get("selection_label") or ""),
        str(row.get("source") or ""),
        str(row.get("route_family") or ""),
    ]))
PY
)

if [[ "${#SELECTED_LINES[@]}" -le 0 ]]; then
    {
        echo "status=blocked_no_selected_habitats"
        echo "selected_habitats_json=$SELECTED_HABITATS_JSON"
        echo "wire_send=0"
        echo "live_landed=0"
        echo "sendBundle_called=false"
    } > "$SUMMARY_FILE"
    cat "$SUMMARY_FILE"
    finish
    exit 0
fi

window_rows=()
for line in "${SELECTED_LINES[@]}"; do
    IFS=$'\t' read -r idx pool mint_in mint_out selection_label source route_family <<< "$line"
    if [[ -z "$pool" || -z "$mint_in" || -z "$mint_out" ]]; then
        warn "Skipping malformed selected habitat line: $line"
        continue
    fi

    short_pool="$(echo "$pool" | cut -c1-8)"
    window_step="${STEP}_w${idx}_${short_pool}"
    window_root="$RESULT_DIR/$window_step"

    info "Focused DirectArb-B window $idx pool=$pool mint_out=$mint_out"
    set +e
    env \
        SHADOW_RESULTS_DIR="$RESULT_DIR" \
        SHADOW_RUN78G3_EPOCH_TAG="$window_step" \
        SHADOW_RUN78G3_WINDOW_SECONDS="$WINDOW_SECONDS" \
        SHADOW_RUN78G3_CAP_REVIEW_SOL="$CAP_REVIEW_SOL" \
        SHADOW_RUN78G3_XDP_ATTACH_MODE="native" \
        SHADOW_RUN78G3_AFXDP_SAFE_MODE="false" \
        SHADOW_OWNER_MODE_FILTER="directarb" \
        SHADOW_RUN40_ALLOW_ONLY_POOL="$pool" \
        SHADOW_RUN40_ALLOW_ONLY_MINT_IN="$mint_in" \
        SHADOW_RUN40_ALLOW_ONLY_MINT_OUT="$mint_out" \
        SHADOW_RUN40_SELECTED_POOL="$pool" \
        SHADOW_RUN40_SELECTED_MINT_IN="$mint_in" \
        SHADOW_RUN40_SELECTED_MINT_OUT="$mint_out" \
        SHADOW_RUN78_SCOUT_ROUTE_FAMILY="jito_bundle" \
        SHADOW_RUN78_PRIVATE_REPLAY_TARGET_ROUTE_FAMILY="jito_bundle" \
        SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$OOB_ARM_FILE" \
        SHADOW_REQUIRE_AFXDP_ZERO_COPY="true" \
        SHADOW_XDP_ATTACH_MODE="native" \
        SHADOW_AFXDP_BIND_MODE="zero_copy" \
        SHADOW_NO_SEND="true" \
        SHADOW_DISABLE_LIVE_SEND="true" \
        SHADOW_DISABLE_SENDBUNDLE="true" \
        SHADOW_ENABLE_JITO_VIA_HELIUS="true" \
        SHADOW_ENABLE_HELIUS_SENDER_DUAL="true" \
        SHADOW_HELIUS_SENDER_URL="${SHADOW_HELIUS_SENDER_URL:-http://tyo-sender.helius-rpc.com/fast}" \
        bash "$PACK_DIR/scripts/78g3_directarb_b_fresh_epoch_nosubmit.sh" >"$RESULT_DIR/${window_step}.log" 2>&1
    rc=$?
    set -e

    hab_summary=""
    if [[ -d "$window_root" ]]; then
        hab_output="$window_root/78hab_directarb_roundtrip_habitat_leaderboard"
        set +e
        bash "$PACK_DIR/scripts/78hab_directarb_roundtrip_habitat_leaderboard.sh" \
            --epoch "$window_root" \
            --output "$hab_output" \
            --top 10 >>"$RESULT_DIR/${window_step}.log" 2>&1
        hab_rc=$?
        set -e
        hab_summary="$hab_output/summary.txt"
    else
        hab_rc=1
    fi

    window_rows+=("$(python3 - "$idx" "$pool" "$mint_in" "$mint_out" "$selection_label" "$source" "$route_family" "$window_step" "$window_root" "$rc" "$hab_rc" "$hab_summary" <<'PY'
from __future__ import annotations
import json
import sys
from pathlib import Path

idx, pool, mint_in, mint_out, selection_label, source, route_family, window_step, window_root = sys.argv[1:10]
rc = int(sys.argv[10])
hab_rc = int(sys.argv[11])
hab_summary = Path(sys.argv[12]) if sys.argv[12] else Path("")
window_root_path = Path(window_root)


def kv(path: Path) -> dict[str, str]:
    out = {}
    if not path.exists():
        return out
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in raw:
            continue
        k, v = raw.split("=", 1)
        out[k.strip()] = v.strip()
    return out


def as_int(value) -> int:
    try:
        return int(value or 0)
    except Exception:
        return 0


def truth(value) -> bool:
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}

g3 = kv(window_root_path / "summary.txt")
hab = kv(hab_summary)
row = {
    "idx": int(idx),
    "pool": pool,
    "mint_in": mint_in,
    "mint_out": mint_out,
    "selection_label": selection_label,
    "source_from_selector": source,
    "route_from_selector": route_family,
    "window_step": window_step,
    "window_root": str(window_root_path),
    "run78g3_rc": rc,
    "habitat_rc": hab_rc,
    "run78g3_status": g3.get("status", ""),
    "afxdp_directarb_rows": as_int(g3.get("afxdp_directarb_rows")),
    "ev_positive_total": as_int(g3.get("ev_positive_total")),
    "exit_route_exists_count": as_int(g3.get("exit_route_exists_count")),
    "roundtrip_positive_count": as_int(g3.get("roundtrip_positive_count")) or as_int(hab.get("roundtrip_positive_count")),
    "manifest_rows": as_int(g3.get("manifest_rows")),
    "g2_evaluated_rows": as_int(g3.get("g2_evaluated_rows")),
    "would_send_positive_count": as_int(g3.get("would_send_positive_count")) or as_int(hab.get("would_send_count")),
    "positive_built_sim_pass_count": as_int(g3.get("positive_built_sim_pass_count")) or as_int(hab.get("simulate_pass_count")),
    "habitat_entry_ev_positive_count": as_int(hab.get("entry_ev_positive_count")),
    "habitat_count": as_int(hab.get("habitat_count")),
    "top_blocker_counts": hab.get("top_blocker_counts", "{}"),
    "wire_send": max(as_int(g3.get("wire_send")), as_int(hab.get("wire_send"))),
    "live_landed": max(as_int(g3.get("live_landed")), as_int(hab.get("live_landed"))),
    "sendBundle_called": truth(g3.get("sendBundle_called")) or truth(hab.get("sendBundle_called")),
}
print(json.dumps(row, separators=(",", ":"), sort_keys=True))
PY
)")
done

python3 - "$SUMMARY_FILE" "$SUMMARY_JSON" "$ROWS_JSONL" "$ROWS_CSV" "$RESULT_DIR" "${window_rows[@]}" <<'PY'
from __future__ import annotations
import csv
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
summary_json_path = Path(sys.argv[2])
rows_jsonl_path = Path(sys.argv[3])
rows_csv_path = Path(sys.argv[4])
result_dir = Path(sys.argv[5])
rows = [json.loads(raw) for raw in sys.argv[6:] if raw.strip()]

wire_send = max([int(row.get("wire_send") or 0) for row in rows] or [0])
live_landed = max([int(row.get("live_landed") or 0) for row in rows] or [0])
send_bundle_called = any(bool(row.get("sendBundle_called")) for row in rows)
would_send = sum(int(row.get("would_send_positive_count") or 0) for row in rows)
roundtrip_positive = sum(int(row.get("roundtrip_positive_count") or 0) for row in rows)

if wire_send or live_landed or send_bundle_called:
    status = "blocked_send_side_effect_observed"
elif would_send > 0:
    status = "focused_directarb_b_would_send_positive_found"
elif roundtrip_positive > 0:
    status = "focused_directarb_b_roundtrip_positive_blocked"
else:
    status = "focused_directarb_b_no_roundtrip_positive"

rows_jsonl_path.write_text(
    "".join(json.dumps(row, separators=(",", ":"), sort_keys=True) + "\n" for row in rows),
    encoding="utf-8",
)
csv_fields = [
    "idx",
    "pool",
    "mint_out",
    "selection_label",
    "run78g3_status",
    "afxdp_directarb_rows",
    "ev_positive_total",
    "exit_route_exists_count",
    "roundtrip_positive_count",
    "manifest_rows",
    "g2_evaluated_rows",
    "would_send_positive_count",
    "positive_built_sim_pass_count",
    "top_blocker_counts",
    "wire_send",
    "live_landed",
    "sendBundle_called",
    "window_root",
]
with rows_csv_path.open("w", encoding="utf-8", newline="") as fh:
    writer = csv.DictWriter(fh, fieldnames=csv_fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(rows)

summary = {
    "status": status,
    "result_dir": str(result_dir),
    "focused_window_count": len(rows),
    "afxdp_directarb_rows": sum(int(row.get("afxdp_directarb_rows") or 0) for row in rows),
    "ev_positive_total": sum(int(row.get("ev_positive_total") or 0) for row in rows),
    "exit_route_exists_count": sum(int(row.get("exit_route_exists_count") or 0) for row in rows),
    "roundtrip_positive_count": roundtrip_positive,
    "manifest_rows": sum(int(row.get("manifest_rows") or 0) for row in rows),
    "g2_evaluated_rows": sum(int(row.get("g2_evaluated_rows") or 0) for row in rows),
    "would_send_positive_count": would_send,
    "positive_built_sim_pass_count": sum(int(row.get("positive_built_sim_pass_count") or 0) for row in rows),
    "wire_send": wire_send,
    "live_landed": live_landed,
    "sendBundle_called": send_bundle_called,
    "rows_jsonl": str(rows_jsonl_path),
    "rows_csv": str(rows_csv_path),
}
summary_path.write_text(
    "\n".join(
        f"{key}={json.dumps(value, separators=(',', ':')) if isinstance(value, (dict, list)) else str(value).lower() if isinstance(value, bool) else value}"
        for key, value in summary.items()
    )
    + "\n",
    encoding="utf-8",
)
summary_json_path.write_text(json.dumps({"summary": summary, "rows": rows}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(summary_path.read_text(encoding="utf-8"), end="")
PY

STATUS="$(awk -F= '$1=="status"{print $2}' "$SUMMARY_FILE" | tail -n1)"
if [[ "$STATUS" == "focused_directarb_b_would_send_positive_found" ]]; then
    pass "82HAB found focused DirectArb-B would-send positive row(s)"
else
    warn "82HAB completed without DirectArb-B would-send positive row"
fi
finish

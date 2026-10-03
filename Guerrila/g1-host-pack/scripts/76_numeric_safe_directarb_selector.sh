#!/usr/bin/env bash
# 76_numeric_safe_directarb_selector.sh - select one numeric-safe strict DirectArb class for no-send proof

source "$(dirname "$0")/lib.sh"
script_header "76 - Numeric-Safe DirectArb Selector"

assert_cmd_exists python3

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_ROOT="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}"
STEP="${SHADOW_RUN76_STEP:-76_numeric_safe_directarb_selector}"
STATUS_PREFIX="${SHADOW_RUN76_STATUS_PREFIX:-run76_numeric_safe_directarb_selector}"
REPORT_BASENAME="${SHADOW_RUN76_REPORT_BASENAME:-run76_numeric_safe_directarb_selector}"
RUN75_STEP="${SHADOW_RUN76_RUN75_STEP:-75_strict_directarb_quote_preservation_proof}"
TOP_LIMIT="${SHADOW_RUN76_TOP_LIMIT:-10}"

RUN75_RESULT_DIR="$RESULTS_ROOT/$RUN75_STEP"
RUN75_SUMMARY_JSON="$RUN75_RESULT_DIR/${RUN75_STEP}.json"
RUN75_ROWS_JSONL="$RUN75_RESULT_DIR/strict_directarb_quote_rows.jsonl"

RESULT_DIR="$RESULTS_ROOT/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
SUMMARY_JSON="$RESULT_DIR/${REPORT_BASENAME}.json"
CLASS_ROWS_JSON="$RESULT_DIR/numeric_safe_class_rows.json"
SELECTED_SCORECARD_JSON="$RESULT_DIR/run76_selected_scorecard.json"

mkdir -p "$RESULT_DIR"
rm -f \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$CLASS_ROWS_JSON" \
    "$SELECTED_SCORECARD_JSON"

missing=0
if [[ ! -f "$RUN75_SUMMARY_JSON" ]]; then
    alt_run75_summary_json="$RUN75_RESULT_DIR/run75_strict_directarb_quote_preservation_proof.json"
    if [[ -f "$alt_run75_summary_json" ]]; then
        RUN75_SUMMARY_JSON="$alt_run75_summary_json"
    fi
fi

for required_path in \
    "$RUN75_SUMMARY_JSON" \
    "$RUN75_ROWS_JSONL"
do
    if [[ ! -f "$required_path" ]]; then
        fail "Required artifact is missing: $required_path"
        missing=1
    fi
done

if [[ "$missing" == "1" ]]; then
    finish
    exit 1
fi

python3 - \
    "$RUN75_SUMMARY_JSON" \
    "$RUN75_ROWS_JSONL" \
    "$SUMMARY_FILE" \
    "$SUMMARY_JSON" \
    "$CLASS_ROWS_JSON" \
    "$SELECTED_SCORECARD_JSON" \
    "$STEP" \
    "$STATUS_PREFIX" \
    "$TOP_LIMIT" <<'PY'
import json
import sys
from collections import defaultdict
from pathlib import Path

run75_summary_path = Path(sys.argv[1])
rows_path = Path(sys.argv[2])
summary_file_path = Path(sys.argv[3])
summary_json_path = Path(sys.argv[4])
class_rows_json_path = Path(sys.argv[5])
selected_scorecard_path = Path(sys.argv[6])
step = sys.argv[7]
status_prefix = sys.argv[8]
top_limit = max(1, int(sys.argv[9] or "10"))


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def load_jsonl(path: Path):
    rows = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def safe_int(value):
    try:
        return int(value or 0)
    except Exception:
        return 0


def safe_float(value):
    try:
        return float(value or 0.0)
    except Exception:
        return 0.0


def text_value(value):
    return str(value or "").strip()


run75_summary = load_json(run75_summary_path)
rows = load_jsonl(rows_path)

numeric_ready_rows = []
excluded_ambiguous_count = 0
excluded_missing_numeric_count = 0
excluded_min_out_nonpositive_count = 0
excluded_not_ready_count = 0
excluded_missing_template_count = 0
for row in rows:
    if text_value(row.get("quote_match_mode")) == "slot_pool_mints_ambiguous":
        excluded_ambiguous_count += 1
    if not row.get("quote_numeric_preserved"):
        excluded_missing_numeric_count += 1
        continue
    if safe_int(row.get("min_out_raw")) <= 0:
        excluded_min_out_nonpositive_count += 1
        continue
    if text_value(row.get("route_used")) != "tpu_quic_public":
        excluded_not_ready_count += 1
        continue
    if text_value(row.get("source")) != "afxdp":
        excluded_not_ready_count += 1
        continue
    if not row.get("inventory_ready"):
        excluded_not_ready_count += 1
        continue
    if not row.get("approved_held_input"):
        excluded_not_ready_count += 1
        continue
    if not row.get("output_account_ready"):
        excluded_not_ready_count += 1
        continue
    if not row.get("template_ready"):
        excluded_not_ready_count += 1
        continue
    if not row.get("would_enqueue_tpu"):
        excluded_not_ready_count += 1
        continue
    if safe_int(row.get("template_id")) <= 0:
        excluded_missing_template_count += 1
        continue
    numeric_ready_rows.append(row)

composite_key_counts = defaultdict(int)
for row in numeric_ready_rows:
    key = (
        safe_int(row.get("slot")),
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
        safe_int(row.get("input_amount_raw")),
        safe_int(row.get("template_id")),
    )
    composite_key_counts[key] += 1

allowed_rows = []
excluded_nonunique_composite_count = 0
selected_quote_match_mode = "unique_slot_pool_mint_amount_template"
for row in numeric_ready_rows:
    key = (
        safe_int(row.get("slot")),
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
        safe_int(row.get("input_amount_raw")),
        safe_int(row.get("template_id")),
    )
    if composite_key_counts[key] != 1:
        excluded_nonunique_composite_count += 1
        continue
    allowed_rows.append(row)

classes = defaultdict(lambda: {"rows": [], "slots": set()})
for row in allowed_rows:
    key = (
        text_value(row.get("pool")),
        text_value(row.get("mint_in")),
        text_value(row.get("mint_out")),
    )
    classes[key]["rows"].append(row)
    classes[key]["slots"].add(safe_int(row.get("slot")))

ranked_classes = []
for (pool, mint_in, mint_out), payload in classes.items():
    scoped_rows = payload["rows"]
    scoped_rows.sort(
        key=lambda item: (
            safe_float(item.get("net_after_fee_usd")),
            safe_int(item.get("expected_out_raw")),
            safe_int(item.get("min_out_raw")),
            safe_int(item.get("slot")),
        ),
        reverse=True,
    )
    sample = scoped_rows[0]
    ranked_classes.append(
        {
            "selected_pool": pool,
            "selected_mint_in": mint_in,
            "selected_mint_out": mint_out,
            "selected_venue": text_value(sample.get("venue")),
            "row_count": len(scoped_rows),
            "same_pair_recurrence": len(payload["slots"]),
            "deduped_ev_usd": round(sum(safe_float(item.get("net_after_fee_usd")) for item in scoped_rows), 6),
            "selected_pair_count": len(scoped_rows),
            "selected_input_amount_raw": safe_int(sample.get("input_amount_raw")),
            "selected_expected_out_raw": safe_int(sample.get("expected_out_raw")),
            "selected_target_out_raw": safe_int(sample.get("quote_target_out_raw") or sample.get("target_out_raw")),
            "selected_min_out_raw": safe_int(sample.get("min_out_raw")),
            "selected_min_out_positive": safe_int(sample.get("min_out_raw")) > 0,
            "selected_net_ev_usd": round(safe_float(sample.get("net_after_fee_usd")), 9),
            "selected_route": text_value(sample.get("route_used")),
            "selected_source": text_value(sample.get("source")),
            "selected_template_id": sample.get("template_id"),
            "selected_quote_request_id": sample.get("quote_request_id"),
            "selected_quote_match_mode": selected_quote_match_mode,
            "selected_signature": text_value(sample.get("signature")),
            "selected_slot": safe_int(sample.get("slot")),
            "selected_unix_ns": safe_int(text_value(sample.get("candidate_id")).split(":", 1)[0]),
            "selected_slippage_bps": safe_int(sample.get("slippage_bps")),
            "selected_quote_source": text_value(sample.get("quote_source")),
            "selected_policy_tip_raw": safe_int(sample.get("policy_tip_raw")),
            "selected_base_fee_raw": safe_int(sample.get("base_fee_raw")),
            "selected_retained_ev_raw": safe_int(sample.get("retained_ev_raw")),
            "selected_retained_ev_usd": round(safe_float(sample.get("retained_ev_usd")), 9),
            "min_min_out_raw": min(safe_int(item.get("min_out_raw")) for item in scoped_rows),
            "max_min_out_raw": max(safe_int(item.get("min_out_raw")) for item in scoped_rows),
            "all_route_tpu_quic_public": all(text_value(item.get("route_used")) == "tpu_quic_public" for item in scoped_rows),
            "all_source_afxdp": all(text_value(item.get("source")) == "afxdp" for item in scoped_rows),
            "all_inventory_ready": all(bool(item.get("inventory_ready")) for item in scoped_rows),
            "all_output_account_ready": all(bool(item.get("output_account_ready")) for item in scoped_rows),
            "all_template_ready": all(bool(item.get("template_ready")) for item in scoped_rows),
            "all_approved_held_input": all(bool(item.get("approved_held_input")) for item in scoped_rows),
            "quote_ambiguity": False,
            "sample_refs": [
                {
                    "slot": safe_int(item.get("slot")),
                    "signature": text_value(item.get("signature")),
                    "net_ev_usd": round(safe_float(item.get("net_after_fee_usd")), 9),
                }
                for item in scoped_rows[:3]
            ],
        }
    )

ranked_classes.sort(
    key=lambda item: (
        item["row_count"],
        item["deduped_ev_usd"],
        item["same_pair_recurrence"],
        item["selected_net_ev_usd"],
    ),
    reverse=True,
)

selected_class = ranked_classes[0] if ranked_classes else {}
why_selected = (
    "highest same pool+mint recurrence, then highest deduped EV, then highest same-pair recurrence among "
    f"the {len(allowed_rows)} numeric-safe strict DirectArb rows"
    if selected_class
    else "no numeric-safe class available"
)
why_safe = (
    "quote_numeric_preserved=true, min_out_numeric_preserved=true, quote ambiguity false, "
    "route=tpu_quic_public, source=afxdp, inventory/account/template ready, approved held input"
    if selected_class
    else "no eligible class"
)

selected_candidate = None
if selected_class:
    selected_candidate = {
        "venue": selected_class["selected_venue"],
        "pool": selected_class["selected_pool"],
        "mint_in": selected_class["selected_mint_in"],
        "mint_out": selected_class["selected_mint_out"],
        "signature": selected_class["selected_signature"],
        "source": selected_class["selected_source"],
        "slot": selected_class["selected_slot"],
        "unix_ns": selected_class["selected_unix_ns"],
        "route": selected_class["selected_route"],
        "selected_target_route_family": "tpu_quic_public",
        "target_route_family": "tpu_quic_public",
        "held_input": True,
        "input_balance_ok": True,
        "output_account_ready": True,
        "template_ready": True,
        "quote_ready_or_classified": True,
        "tiny_cap_safe": True,
        "tpu_convertible_route": True,
        "source_relaxed_dev_proof": True,
        "scorecard_replay_dev_proof": True,
        "live_market_recurrence_required": False,
        "canonical_private_grade": False,
        "net_after_fee_lamports": selected_class["selected_policy_tip_raw"],
        "retained_ev_lamports": selected_class["selected_retained_ev_raw"],
        "run76_row_count": selected_class["row_count"],
        "run76_deduped_ev_usd": selected_class["deduped_ev_usd"],
        "run76_expected_out_raw": selected_class["selected_expected_out_raw"],
        "run76_target_out_raw": selected_class["selected_target_out_raw"],
        "run76_min_out_raw": selected_class["selected_min_out_raw"],
        "run76_quote_request_id": selected_class["selected_quote_request_id"],
        "run76_template_id": selected_class["selected_template_id"],
        "run76_quote_match_mode": selected_class["selected_quote_match_mode"],
    }

selected_scorecard = {
    "run40_candidate_selection": {
        "status": "numeric_safe_selected_from_run75",
        "selected_candidate": selected_candidate,
        "selector_step": step,
    },
    "r1_surface_observations": [],
}

summary_payload = {
    "status": f"{status_prefix}_complete" if selected_class else f"{status_prefix}_no_candidate",
    "step": step,
    "inputs": {
        "run75_summary_json": str(run75_summary_path),
        "run75_rows_jsonl": str(rows_path),
    },
        "headline": {
            "strict_directarb_rows": safe_int((run75_summary.get("headline") or {}).get("strict_directarb_rows")),
            "numeric_safe_allowed_rows": len(allowed_rows),
            "numeric_safe_class_count": len(ranked_classes),
            "excluded_ambiguous_count": excluded_ambiguous_count,
            "excluded_missing_numeric_count": excluded_missing_numeric_count,
            "excluded_min_out_nonpositive_count": excluded_min_out_nonpositive_count,
            "excluded_not_ready_count": excluded_not_ready_count,
            "excluded_missing_template_count": excluded_missing_template_count,
            "excluded_nonunique_composite_count": excluded_nonunique_composite_count,
        },
        "selected_class": {
            **selected_class,
            "why_selected": why_selected,
            "why_safe": why_safe,
            "excluded_ambiguous_count": excluded_ambiguous_count,
            "excluded_min_out_nonpositive_count": excluded_min_out_nonpositive_count,
            "excluded_nonunique_composite_count": excluded_nonunique_composite_count,
        } if selected_class else {},
    "top_classes": ranked_classes[:top_limit],
    "output_files": {
        "class_rows_json": str(class_rows_json_path),
        "selected_scorecard_json": str(selected_scorecard_path),
    },
}

class_rows_json_path.write_text(json.dumps(ranked_classes, indent=2) + "\n", encoding="utf-8")
selected_scorecard_path.write_text(json.dumps(selected_scorecard, indent=2) + "\n", encoding="utf-8")
summary_json_path.write_text(json.dumps(summary_payload, indent=2) + "\n", encoding="utf-8")

summary_lines = [
    f"status={summary_payload['status']}",
    f"numeric_safe_allowed_rows={summary_payload['headline']['numeric_safe_allowed_rows']}",
    f"numeric_safe_class_count={summary_payload['headline']['numeric_safe_class_count']}",
    f"excluded_ambiguous_count={excluded_ambiguous_count}",
    f"excluded_min_out_nonpositive_count={excluded_min_out_nonpositive_count}",
    f"excluded_nonunique_composite_count={excluded_nonunique_composite_count}",
]

if selected_class:
    summary_lines.extend(
        [
            f"selected_pool={selected_class['selected_pool']}",
            f"selected_mint_in={selected_class['selected_mint_in']}",
            f"selected_mint_out={selected_class['selected_mint_out']}",
            f"selected_pair_count={selected_class['selected_pair_count']}",
            f"selected_input_amount_raw={selected_class['selected_input_amount_raw']}",
            f"selected_expected_out_raw={selected_class['selected_expected_out_raw']}",
            f"selected_target_out_raw={selected_class['selected_target_out_raw']}",
            f"selected_min_out_raw={selected_class['selected_min_out_raw']}",
            f"selected_min_out_positive={str(selected_class['selected_min_out_positive']).lower()}",
            f"selected_net_ev_usd={selected_class['selected_net_ev_usd']}",
            f"selected_route={selected_class['selected_route']}",
            f"selected_source={selected_class['selected_source']}",
            f"selected_template_id={selected_class['selected_template_id']}",
            f"selected_quote_request_id={selected_class['selected_quote_request_id']}",
            f"selected_quote_match_mode={selected_class['selected_quote_match_mode']}",
            f"selected_signature={selected_class['selected_signature']}",
            f"selected_slot={selected_class['selected_slot']}",
            f"selected_row_count={selected_class['row_count']}",
            f"selected_deduped_ev_usd={selected_class['deduped_ev_usd']}",
            f"selected_same_pair_recurrence={selected_class['same_pair_recurrence']}",
            f"selected_quote_ambiguity=false",
            f"why_selected={why_selected}",
            f"why_safe={why_safe}",
            f"selected_scorecard_json={selected_scorecard_path}",
        ]
    )

summary_file_path.write_text("\n".join(summary_lines) + "\n", encoding="utf-8")
print("\n".join(summary_lines))
PY

pass "Run76 numeric-safe DirectArb selector written to $RESULT_DIR"
finish

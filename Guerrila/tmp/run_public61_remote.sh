#!/usr/bin/env bash
set -euo pipefail

STEP="${1:?step required}"

PACK_DIR='/home/ubuntu/Guerrila/g1-host-pack'
source "$PACK_DIR/scripts/lib.sh"

GATE="$(default_public_rpc_url)"
RESULTS_ROOT='/home/ubuntu/shadow-host-audit/dpdk-runs'
RESULT_DIR="$RESULTS_ROOT/$STEP"
CONFIG_TMP="$RESULT_DIR/config.public_scout.yaml"
ARM_FILE="$RESULTS_ROOT/${STEP}_oob_arm.flag"

mkdir -p "$RESULT_DIR"

python3 - /home/ubuntu/Guerrila/config.guerrila.yaml "$CONFIG_TMP" "$GATE" <<'PY'
import re
import sys
from pathlib import Path

src_path = Path(sys.argv[1])
dst_path = Path(sys.argv[2])
public_rpc_endpoint = sys.argv[3]
text = src_path.read_text(encoding="utf-8")


def replace_in_block(text, block_name, child_indent, replacements):
    lines = text.splitlines()
    inside = False
    block_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not inside:
            if stripped == f"{block_name}:":
                inside = True
                block_indent = indent
            continue
        if stripped and indent <= block_indent:
            break
        for key, value in replacements.items():
            expected_prefix = " " * child_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"


def replace_in_nested_block(text, parent, child, child_key_indent, replacements):
    lines = text.splitlines()
    inside = False
    in_child = False
    parent_indent = 0
    child_indent = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))
        if not inside:
            if stripped == f"{parent}:":
                inside = True
                parent_indent = indent
            continue
        if stripped and indent <= parent_indent:
            break
        if not in_child:
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        if stripped and indent <= child_indent:
            in_child = False
            if stripped == f"{child}:":
                in_child = True
                child_indent = indent
            continue
        for key, value in replacements.items():
            expected_prefix = " " * child_key_indent + f"{key}:"
            if line.startswith(expected_prefix):
                lines[idx] = f"{expected_prefix} {value}"
    return "\n".join(lines) + "\n"


def replace_line(text, pattern, replacement):
    if not re.search(pattern, text, flags=re.M):
        return text
    return re.sub(pattern, replacement, text, count=1, flags=re.M)


text = replace_in_block(
    text,
    "public_rpc",
    2,
    {
        "enabled": "true",
        "endpoint": f"\"{public_rpc_endpoint}\"",
    },
)
text = replace_in_nested_block(
    text,
    "r1",
    "public_scout",
    4,
    {
        "enabled": "true",
        "mode": "auto_admission",
    },
)
text = replace_in_nested_block(
    text,
    "r1",
    "live_admission",
    4,
    {
        "max_live_trade_input_sol": "0.05",
        "max_position_sol_per_mint": "0.1",
        "max_notional_sol_per_entity": "0.1",
        "max_new_positions_per_window": "1",
        "min_retained_ev_lamports": "1",
        "min_net_after_fee_lamports": "1",
    },
)
text = replace_in_nested_block(
    text,
    "submit",
    "routing",
    4,
    {
        "default_jito_searcher": "public_rpc_scout",
        "enable_harmonic_lane": "false",
    },
)
text = replace_in_block(
    text,
    "rollout",
    2,
    {
        "mode": "tiny_live",
        "no_send_backend": "false",
        "cap_rung": "r1_tiny",
        "max_live_trade_input_sol": "0.05",
        "max_position_sol_per_mint": "0.1",
        "max_notional_sol_per_entity": "0.1",
    },
)
text = replace_line(
    text,
    r'(^\s*shock_research_export_path:\s*).+$',
    r'\1""',
)

dst_path.write_text(text, encoding="utf-8")
print(dst_path)
PY

: > "$ARM_FILE"

cd /home/ubuntu/Guerrila
sudo -E env \
  SHADOW_RESULTS_DIR="$RESULTS_ROOT" \
  SHADOW_RUN61_STEP="$STEP" \
  SHADOW_RUN61_WINDOW_SECONDS=60 \
  SHADOW_RUN39_CONFIG_SRC="$CONFIG_TMP" \
  SHADOW_RUN39_NATIVE_OOB_ARM_FILE="$ARM_FILE" \
  SHADOW_RUN39_NATIVE_WAIT_POLL_SECONDS=10 \
  SHADOW_RUN55_PUBLIC_RPC_ENDPOINT="$GATE" \
  SHADOW_RUN55_ENABLE_LIVE_SEND=false \
  bash g1-host-pack/scripts/61_post_shock_conversion.sh

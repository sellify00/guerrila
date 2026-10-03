#!/usr/bin/env bash
# 21_shadowguard_preflight.sh — G12 ShadowGuard mainnet ops preflight
#
# Verifies the configured ShadowGuard program id is present on mainnet, checks
# the configured guard vault pubkeys are discoverable, and records the explicit
# operator abort/pass smoke acknowledgement required before any tiny-live send.
#
# Usage:
#   source env.g1.sh
#   sudo ./g1-host-pack/scripts/21_shadowguard_preflight.sh
source "$(dirname "$0")/lib.sh"
script_header "21 — ShadowGuard Mainnet Preflight"

assert_root
require_env SHADOW_DEPLOYER_KEYPAIR_PATH
require_env SHADOW_GUARD_SMOKE_OK

CONFIG="/etc/shadow-engine/config.guerrila.yaml"
STEP="21_shadowguard_preflight"
PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
MAINNET_URL="${SHADOW_RUN21_MAINNET_URL:-${SHADOW_SOLANA_RPC_URL:-$(default_http_rpc_url)}}"
ARTIFACT_DIR="${PACK_DIR}/artifacts/release-guerrila"
TEMPLATE_PROOF_BIN="$ARTIFACT_DIR/shadowguard-template-proof"

mkdir -p "$RESULT_DIR"

require_file() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        fail "Missing file: $path"
        exit 1
    fi
}

yaml_string() {
    local key="$1"
    sed -n "s/^[[:space:]]*$key:[[:space:]]*\"\([^\"]*\)\"[[:space:]]*$/\1/p" "$CONFIG" | head -1
}

known_non_guard_program_id() {
    local value="$1"
    case "$value" in
        "" \
        | "GUard1111111111111111111111111111111111111" \
        | "AddressLookupTab1e1111111111111111111111111" \
        | "11111111111111111111111111111111" \
        | "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" \
        | "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" \
        | "SysvarRent111111111111111111111111111111111" \
        | "SysvarC1ock11111111111111111111111111111111")
            return 0
            ;;
    esac
    [[ -n "${SWAP_PROGRAM_ID:-}" && "$value" == "$SWAP_PROGRAM_ID" ]]
}

known_non_guard_vault_account() {
    local value="$1"
    case "$value" in
        "" \
        | "GVin11111111111111111111111111111111111111111" \
        | "GVout1111111111111111111111111111111111111111" \
        | "AddressLookupTab1e1111111111111111111111111" \
        | "11111111111111111111111111111111" \
        | "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" \
        | "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" \
        | "SysvarRent111111111111111111111111111111111" \
        | "SysvarC1ock11111111111111111111111111111111")
            return 0
            ;;
    esac
    [[ -n "${SWAP_PROGRAM_ID:-}" && "$value" == "$SWAP_PROGRAM_ID" ]]
}

require_file "$CONFIG"
assert_cmd_exists solana

SWAP_PROGRAM_ID="$(yaml_string swap_program_id)"
PROGRAM_ID="$(yaml_string shadow_guard_program_id)"
VAULT_IN="$(yaml_string guard_vault_in_pubkey)"
VAULT_OUT="$(yaml_string guard_vault_out_pubkey)"

if known_non_guard_program_id "$PROGRAM_ID"; then
    fail "execution.shadow_guard_program_id is missing, a placeholder, a system program, or equal to the swap program: ${PROGRAM_ID:-<empty>}"
    exit 1
fi

if known_non_guard_vault_account "$VAULT_IN"; then
    fail "execution.guard_vault_in_pubkey is missing or still a known placeholder/program/sysvar: ${VAULT_IN:-<empty>}"
    exit 1
fi

if known_non_guard_vault_account "$VAULT_OUT"; then
    fail "execution.guard_vault_out_pubkey is missing or still a known placeholder/program/sysvar: ${VAULT_OUT:-<empty>}"
    exit 1
fi

if [[ ! -f "$SHADOW_DEPLOYER_KEYPAIR_PATH" ]]; then
    fail "Deployer keypair is missing: $SHADOW_DEPLOYER_KEYPAIR_PATH"
    exit 1
fi
DEPLOYER_OWNER="$(stat -c '%U:%G' "$SHADOW_DEPLOYER_KEYPAIR_PATH")"
DEPLOYER_MODE="$(stat -c '%a' "$SHADOW_DEPLOYER_KEYPAIR_PATH")"
if [[ "${DEPLOYER_MODE: -2}" != "00" ]]; then
    fail "Deployer keypair permissions are too broad: owner=$DEPLOYER_OWNER mode=$DEPLOYER_MODE"
    exit 1
fi
pass "Deployer keypair is staged with restricted permissions: owner=$DEPLOYER_OWNER mode=$DEPLOYER_MODE"

info "Checking Solana CLI cluster target"
solana config get >"$RESULT_DIR/solana_config.txt" 2>"$RESULT_DIR/solana_config.stderr" || true

info "Checking deployer keypair balance"
DEPLOYER_BALANCE="$(solana balance --keypair "$SHADOW_DEPLOYER_KEYPAIR_PATH" --url "$MAINNET_URL" 2>"$RESULT_DIR/deployer_balance.stderr" || true)"
echo "$DEPLOYER_BALANCE" > "$RESULT_DIR/deployer_balance.txt"
if ! grep -Eq '[1-9][0-9]*(\.[0-9]+)? SOL|0\.[0-9]*[1-9][0-9]* SOL' "$RESULT_DIR/deployer_balance.txt"; then
    fail "Deployer keypair does not show a positive SOL balance on mainnet"
fi

info "Checking ShadowGuard program deployment on mainnet"
if solana --keypair "$SHADOW_DEPLOYER_KEYPAIR_PATH" program show "$PROGRAM_ID" --url "$MAINNET_URL" >"$RESULT_DIR/program_show.txt" 2>"$RESULT_DIR/program_show.stderr"; then
    pass "ShadowGuard program is deployed on mainnet: $PROGRAM_ID"
    shadow_guard_mainnet_deployed=true
else
    fail "ShadowGuard program is not discoverable on mainnet: $PROGRAM_ID"
    shadow_guard_mainnet_deployed=false
fi

shadow_guard_program_id_present=true

run_template_proof() {
    if [[ -x "$TEMPLATE_PROOF_BIN" ]]; then
        "$TEMPLATE_PROOF_BIN" --config "$CONFIG"
    elif command -v cargo &>/dev/null; then
        (
            cd "$PACK_DIR/.."
            cargo run --quiet -p shadow-tier0 --bin shadowguard-template-proof -- --config "$CONFIG"
        )
    else
        return 127
    fi
}

if run_template_proof >"$RESULT_DIR/template_proof.txt" 2>"$RESULT_DIR/template_proof.stderr"; then
    shadow_guard_template_program_id_matches="$(sed -n 's/^shadow_guard_template_program_id_matches=//p' "$RESULT_DIR/template_proof.txt" | head -1)"
    shadow_guard_jito_template_program_id_matches="$(sed -n 's/^shadow_guard_jito_template_program_id_matches=//p' "$RESULT_DIR/template_proof.txt" | head -1)"
    shadow_guard_harmonic_template_program_id_matches="$(sed -n 's/^shadow_guard_harmonic_template_program_id_matches=//p' "$RESULT_DIR/template_proof.txt" | head -1)"
    lookup_table_placeholder_present="$(sed -n 's/^lookup_table_placeholder_present=//p' "$RESULT_DIR/template_proof.txt" | head -1)"

    if [[ "$shadow_guard_template_program_id_matches" == "true" ]]; then
        pass "Jito and Harmonic templates reference the configured ShadowGuard program id"
    else
        fail "Template proof did not confirm the configured ShadowGuard program id in both templates"
    fi

    if [[ "$lookup_table_placeholder_present" == "true" ]]; then
        fail "AddressLookupTable placeholder is still present in a generated execution template"
    else
        pass "No AddressLookupTable placeholder appears in generated execution templates"
    fi
else
    fail "Unable to run ShadowGuard template proof helper"
    shadow_guard_template_program_id_matches=false
    shadow_guard_jito_template_program_id_matches=false
    shadow_guard_harmonic_template_program_id_matches=false
    lookup_table_placeholder_present=true
fi

if [[ -n "$VAULT_IN" ]]; then
    if solana --keypair "$SHADOW_DEPLOYER_KEYPAIR_PATH" account "$VAULT_IN" --url "$MAINNET_URL" >"$RESULT_DIR/guard_vault_in.txt" 2>"$RESULT_DIR/guard_vault_in.stderr"; then
        pass "Guard vault in account is discoverable"
    else
        fail "Guard vault in account is not discoverable: $VAULT_IN"
    fi
else
    fail "execution.guard_vault_in_pubkey is missing from $CONFIG"
fi

if [[ -n "$VAULT_OUT" ]]; then
    if solana --keypair "$SHADOW_DEPLOYER_KEYPAIR_PATH" account "$VAULT_OUT" --url "$MAINNET_URL" >"$RESULT_DIR/guard_vault_out.txt" 2>"$RESULT_DIR/guard_vault_out.stderr"; then
        pass "Guard vault out account is discoverable"
    else
        fail "Guard vault out account is not discoverable: $VAULT_OUT"
    fi
else
    fail "execution.guard_vault_out_pubkey is missing from $CONFIG"
fi

if [[ "$SHADOW_GUARD_SMOKE_OK" == "1" ]]; then
    pass "Operator-confirmed guard abort/pass smoke completed"
    guard_vault_layout_smoke=true
else
    fail "Set SHADOW_GUARD_SMOKE_OK=1 only after the guard abort/pass smoke succeeds on the configured vault layout"
    guard_vault_layout_smoke=false
fi

{
    echo "shadow_guard_program_id_present=$shadow_guard_program_id_present"
    echo "shadow_guard_mainnet_deployed=$shadow_guard_mainnet_deployed"
    echo "shadow_guard_template_program_id_matches=$shadow_guard_template_program_id_matches"
    echo "shadow_guard_jito_template_program_id_matches=$shadow_guard_jito_template_program_id_matches"
    echo "shadow_guard_harmonic_template_program_id_matches=$shadow_guard_harmonic_template_program_id_matches"
    echo "guard_vault_layout_smoke=$guard_vault_layout_smoke"
    echo "lookup_table_placeholder_present=$lookup_table_placeholder_present"
    echo "program_id=$PROGRAM_ID"
    echo "deployer_keypair=$SHADOW_DEPLOYER_KEYPAIR_PATH"
    echo "deployer_keypair_owner=$DEPLOYER_OWNER"
    echo "deployer_keypair_mode=$DEPLOYER_MODE"
} | tee "$SUMMARY_FILE"

finish

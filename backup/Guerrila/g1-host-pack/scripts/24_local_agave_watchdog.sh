#!/usr/bin/env bash
# 24_local_agave_watchdog.sh — keep the local SH1 Agave source present for AF_XDP mode

source "$(dirname "$0")/lib.sh"
script_header "24 — Local Agave Watchdog"

assert_root

PACK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck disable=SC1091
source "$PACK_DIR/env.g1.sh"

STEP="${SHADOW_LOCAL_AGAVE_WATCHDOG_STEP:-24_local_agave_watchdog}"
RESULT_DIR="${SHADOW_RESULTS_DIR:-$PACK_DIR/results}/$STEP"
SUMMARY_FILE="$RESULT_DIR/summary.txt"
PREFLIGHT_FILE="$RESULT_DIR/local_agave_preflight.txt"
PREFERRED_SERVICE="${SHADOW_LOCAL_AGAVE_WATCHDOG_SERVICE:-sh1-agave-no-geyser-2.service}"
REQUIRE_LIMIT_LEDGER_SIZE="${SHADOW_LOCAL_AGAVE_REQUIRE_LIMIT_LEDGER_SIZE:-true}"
MIN_LEDGER_FREE_GIB="${SHADOW_LOCAL_AGAVE_MIN_LEDGER_FREE_GIB:-100}"
MIN_ACCOUNTS_FREE_GIB="${SHADOW_LOCAL_AGAVE_MIN_ACCOUNTS_FREE_GIB:-100}"
LOCAL_AGAVE_CPU_AFFINITY="${SHADOW_LOCAL_AGAVE_CPU_AFFINITY:-}"
SHADOW_STATE_CPU_AFFINITY="${SHADOW_SHADOW_STATE_CPU_AFFINITY:-}"

mkdir -p "$RESULT_DIR"

assert_cmd_exists systemctl
assert_cmd_exists pgrep
assert_cmd_exists sed
assert_cmd_exists awk
assert_cmd_exists df
assert_cmd_exists taskset

declare -a VALIDATOR_CMDLINE=()
validator_pid=""
validator_limit_ledger_size="unknown"
validator_maximum_full_snapshots_to_retain="agave_default_2"
validator_maximum_incremental_snapshots_to_retain="agave_default_4"
validator_ledger_dir="unknown"
validator_accounts_dir="unknown"
ledger_available_gib="unknown"
accounts_available_gib="unknown"
storage_guard_status="pass"
storage_guard_reasons="none"
validator_cpu_affinity_current="unknown"
shadow_state_pid=""
shadow_state_cpu_affinity_current="unknown"

load_validator_runtime_state() {
    validator_pid="$(pgrep -fo 'agave-validator|solana-validator' 2>/dev/null || true)"
    VALIDATOR_CMDLINE=()
    if [[ -n "$validator_pid" && -r "/proc/$validator_pid/cmdline" ]]; then
        mapfile -d '' -t VALIDATOR_CMDLINE <"/proc/$validator_pid/cmdline"
    fi
}

cmdline_has_flag() {
    local expected_flag="$1"
    local arg
    for arg in "${VALIDATOR_CMDLINE[@]}"; do
        if [[ "$arg" == "$expected_flag" ]]; then
            return 0
        fi
    done
    return 1
}

cmdline_get_flag_value() {
    local expected_flag="$1"
    local idx
    for ((idx = 0; idx < ${#VALIDATOR_CMDLINE[@]}; idx++)); do
        if [[ "${VALIDATOR_CMDLINE[$idx]}" == "$expected_flag" ]] && ((idx + 1 < ${#VALIDATOR_CMDLINE[@]})); then
            printf '%s\n' "${VALIDATOR_CMDLINE[$((idx + 1))]}"
            return 0
        fi
    done
    return 1
}

path_available_gib() {
    local target_path="$1"
    local available_kib
    available_kib="$(df -Pk "$target_path" 2>/dev/null | awk 'NR==2 {print $4}')"
    if [[ ! "$available_kib" =~ ^[0-9]+$ ]]; then
        return 1
    fi
    printf '%s\n' "$((available_kib / 1024 / 1024))"
}

taskset_current_affinity() {
    local target_pid="$1"
    taskset -pc "$target_pid" 2>/dev/null | awk -F': ' 'END {print $2}'
}

apply_cpu_affinity() {
    local label="$1"
    local target_pid="$2"
    local target_affinity="$3"
    local current_affinity=""

    [[ -n "$target_pid" ]] || return 0
    [[ -n "$target_affinity" ]] || return 0

    current_affinity="$(taskset_current_affinity "$target_pid" || true)"
    if [[ "$current_affinity" != "$target_affinity" ]]; then
        info "Pinning $label pid=$target_pid to CPU(s) $target_affinity"
        taskset -pc "$target_affinity" "$target_pid" >/dev/null
        current_affinity="$(taskset_current_affinity "$target_pid" || true)"
    fi

    printf '%s\n' "${current_affinity:-unknown}"
}

evaluate_storage_guard() {
    local reasons=()

    load_validator_runtime_state
    if [[ -z "$validator_pid" || ${#VALIDATOR_CMDLINE[@]} -eq 0 ]]; then
        storage_guard_status="unknown"
        storage_guard_reasons="validator_cmdline_unavailable"
        return 0
    fi

    if cmdline_has_flag --limit-ledger-size; then
        validator_limit_ledger_size="present"
    else
        validator_limit_ledger_size="missing"
        if value_is_true "$REQUIRE_LIMIT_LEDGER_SIZE"; then
            reasons+=("limit_ledger_size_missing")
        fi
    fi

    validator_maximum_full_snapshots_to_retain="$(cmdline_get_flag_value --maximum-full-snapshots-to-retain || printf '%s' "$validator_maximum_full_snapshots_to_retain")"
    validator_maximum_incremental_snapshots_to_retain="$(cmdline_get_flag_value --maximum-incremental-snapshots-to-retain || printf '%s' "$validator_maximum_incremental_snapshots_to_retain")"
    validator_ledger_dir="$(cmdline_get_flag_value --ledger || printf '%s' "$validator_ledger_dir")"
    validator_accounts_dir="$(cmdline_get_flag_value --accounts || printf '%s' "$validator_accounts_dir")"

    if [[ "$validator_ledger_dir" != "unknown" ]]; then
        ledger_available_gib="$(path_available_gib "$validator_ledger_dir" || printf '%s' "unknown")"
        if [[ "$ledger_available_gib" =~ ^[0-9]+$ ]] && ((ledger_available_gib < MIN_LEDGER_FREE_GIB)); then
            reasons+=("ledger_free_space_below_${MIN_LEDGER_FREE_GIB}gib")
        fi
    else
        reasons+=("ledger_path_unavailable")
    fi

    if [[ "$validator_accounts_dir" != "unknown" ]]; then
        accounts_available_gib="$(path_available_gib "$validator_accounts_dir" || printf '%s' "unknown")"
        if [[ "$accounts_available_gib" =~ ^[0-9]+$ ]] && ((accounts_available_gib < MIN_ACCOUNTS_FREE_GIB)); then
            reasons+=("accounts_free_space_below_${MIN_ACCOUNTS_FREE_GIB}gib")
        fi
    else
        reasons+=("accounts_path_unavailable")
    fi

    if ((${#reasons[@]} > 0)); then
        storage_guard_status="fail"
        storage_guard_reasons="$(IFS=,; echo "${reasons[*]}")"
        return 1
    fi

    storage_guard_status="pass"
    storage_guard_reasons="none"
    return 0
}

write_summary() {
    local status_value="$1"
    {
        echo "status=$status_value"
        echo "action=$action"
        echo "active_unit=${active_unit:-none}"
        echo "validator_pid=${validator_pid:-none}"
        echo "validator_limit_ledger_size=$validator_limit_ledger_size"
        echo "validator_maximum_full_snapshots_to_retain=$validator_maximum_full_snapshots_to_retain"
        echo "validator_maximum_incremental_snapshots_to_retain=$validator_maximum_incremental_snapshots_to_retain"
        echo "validator_ledger_dir=$validator_ledger_dir"
        echo "validator_accounts_dir=$validator_accounts_dir"
        echo "ledger_available_gib=$ledger_available_gib"
        echo "accounts_available_gib=$accounts_available_gib"
        echo "minimum_ledger_free_gib=$MIN_LEDGER_FREE_GIB"
        echo "minimum_accounts_free_gib=$MIN_ACCOUNTS_FREE_GIB"
        echo "storage_guard_status=$storage_guard_status"
        echo "storage_guard_reasons=$storage_guard_reasons"
        echo "validator_cpu_affinity_target=${LOCAL_AGAVE_CPU_AFFINITY:-none}"
        echo "validator_cpu_affinity_current=$validator_cpu_affinity_current"
        echo "shadow_state_pid=${shadow_state_pid:-none}"
        echo "shadow_state_cpu_affinity_target=${SHADOW_STATE_CPU_AFFINITY:-none}"
        echo "shadow_state_cpu_affinity_current=$shadow_state_cpu_affinity_current"
        if [[ -f "$PREFLIGHT_FILE" ]]; then
            cat "$PREFLIGHT_FILE"
        fi
    } >"$SUMMARY_FILE"
}

active_unit="$(find_local_agave_active_unit || true)"
validator_process_count="$(pgrep -fc 'agave-validator|solana-validator' 2>/dev/null || true)"
action="observe_only"

if [[ -z "$active_unit" && "${validator_process_count:-0}" == "0" ]]; then
    if systemctl list-unit-files --no-pager | awk '{print $1}' | grep -Fxq "$PREFERRED_SERVICE"; then
        info "No active local Agave unit/process detected; starting $PREFERRED_SERVICE"
        systemctl start "$PREFERRED_SERVICE"
        action="started_preferred_service"
    else
        fail "No active local Agave unit/process and preferred service $PREFERRED_SERVICE is not installed"
        {
            echo "status=fail"
            echo "action=missing_preferred_service"
            echo "preferred_service=$PREFERRED_SERVICE"
        } >"$SUMMARY_FILE"
        finish
        exit 1
    fi
fi

active_unit="$(find_local_agave_active_unit || true)"
load_validator_runtime_state
if [[ -n "$validator_pid" ]]; then
    validator_cpu_affinity_current="$(apply_cpu_affinity "local Agave validator" "$validator_pid" "$LOCAL_AGAVE_CPU_AFFINITY")"
fi
shadow_state_pid="$(pgrep -xo shadow-state 2>/dev/null || true)"
if [[ -n "$shadow_state_pid" ]]; then
    shadow_state_cpu_affinity_current="$(apply_cpu_affinity "shadow-state" "$shadow_state_pid" "$SHADOW_STATE_CPU_AFFINITY")"
fi
if require_local_agave_preflight "$PREFLIGHT_FILE"; then
    preflight_status="pass"
else
    preflight_status="fail"
fi

if ! evaluate_storage_guard; then
    fail "Local Agave storage guard failed: $storage_guard_reasons"
    write_summary "fail"
    finish
    exit 1
fi

if [[ "$preflight_status" == "pass" ]]; then
    write_summary "pass"
    finish
    exit 0
fi

failure_reasons="$(sed -n 's/^failure_reasons=//p' "$PREFLIGHT_FILE" | head -n 1)"
status_label="bootstrap_in_progress"
if [[ "$failure_reasons" == *"validator_not_running"* ]]; then
    status_label="validator_unavailable"
fi

warn "Local Agave preflight not yet green: ${failure_reasons:-unknown}"
write_summary "$status_label"

FAILURES=0
pass "Watchdog leaving validator running while bootstrap/catch-up continues"
exit 0

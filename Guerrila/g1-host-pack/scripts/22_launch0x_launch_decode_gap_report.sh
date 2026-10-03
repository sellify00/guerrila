#!/usr/bin/env bash
# 22_launch0x_launch_decode_gap_report.sh — canonical Launch decode/admission gap report

source_crlf_safe() {
    local path="$1"
    # shellcheck disable=SC1090
    source /dev/stdin <<<"$(tr -d '\r' < "$path")"
}

source_crlf_safe "$(dirname "$0")/lib.sh"
script_header "22 Launch0X — Launch Decode Gap Report"

"$(dirname "$0")/22_launch0i_launch_decode_gap_report.sh"

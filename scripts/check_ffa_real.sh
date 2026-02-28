#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <log-file>" >&2
    exit 2
fi

log_file="$1"
if [[ ! -f "$log_file" ]]; then
    echo "ERROR: log file not found: $log_file" >&2
    exit 2
fi

has_bypass=0
has_real_resp=0
has_bad=0

if rg -a -q "\\[BYPASS\\] SUCCESS" "$log_file"; then
    has_bypass=1
fi

if rg -a -q "EVT\\] tx x0=0x84000070 x1=0x80010000 x3=0xaaaa" "$log_file"; then
    has_real_resp=1
fi

if rg -a -q "Kernel panic|!!! PANIC !!!|unexpected SP exit|SP Idle transition failed|EVT\\] tx x0=0x00000000" "$log_file"; then
    has_bad=1
fi

echo "check: bypass_success=$has_bypass real_direct_resp=$has_real_resp bad_signals=$has_bad"

if [[ $has_bypass -eq 1 && $has_real_resp -eq 1 && $has_bad -eq 0 ]]; then
    echo "PASS: REAL DIRECT_REQ path looks healthy"
    exit 0
fi

echo "FAIL: REAL DIRECT_REQ path still suspicious"
exit 1

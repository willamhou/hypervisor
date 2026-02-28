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
has_e2e_pass=0
has_bad=0

if rg -a -q "\\[BYPASS\\] SUCCESS" "$log_file"; then
    has_bypass=1
fi

# Compatibility: old diagnostic format from SPMC side.
if rg -a -q "EVT\\] tx x0=0x84000070 x1=0x80010000 x3=0xaaaa" "$log_file"; then
    has_real_resp=1
fi

# Current format: Linux ffa_test module raw/real DIRECT_REQ diagnostics.
if rg -a -q "\\[REAL\\] Result: x0=0x84000070 x1=0x80010000 x3=0xaaaa" "$log_file"; then
    has_real_resp=1
fi

# End-to-end success in latest flow.
# Prefer summary line, but also accept per-SP pass patterns.
if rg -a -q "ffa_test: +Results: +8/8 PASS" "$log_file" \
    || {
        rg -a -q "\\[PASS\\] DIRECT_REQ to SP 0x8001 returns success" "$log_file" \
        && rg -a -q "\\[PASS\\] DIRECT_REQ to SP 0x8002 returns success" "$log_file"
    }; then
    has_e2e_pass=1
fi

if rg -a -q "Kernel panic|!!! PANIC !!!|unexpected SP exit|SP Idle transition failed|EVT\\] tx x0=0x00000000|DIRECT_REQ to SP 0x8001: ret=-95|DIRECT_REQ to SP 0x8002: ret=-95|Results: [0-9]+/[0-9]+ FAIL|\\[FAIL\\]" "$log_file"; then
    has_bad=1
fi

echo "check: bypass_success=$has_bypass real_direct_resp=$has_real_resp e2e_pass=$has_e2e_pass bad_signals=$has_bad"

if [[ $has_bypass -eq 1 && $has_real_resp -eq 1 && $has_e2e_pass -eq 1 && $has_bad -eq 0 ]]; then
    echo "PASS: REAL DIRECT_REQ path looks healthy"
    exit 0
fi

echo "FAIL: REAL DIRECT_REQ path still suspicious"
exit 1

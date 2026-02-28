#!/usr/bin/env bash
set -euo pipefail

# Unified regression checker for the three integration test logs:
# 1) run-spmc
# 2) run-tfa-linux-ffa
# 3) run-pkvm-ffa-test
#
# Usage:
#   ./scripts/check_test_suite.sh
#   ./scripts/check_test_suite.sh <spmc-log> <tfa-linux-ffa-log> <pkvm-ffa-log>

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_SPMC_LOG="${ROOT_DIR}/test_run_spmc.log"
DEFAULT_TFA_LOG="${ROOT_DIR}/test_run_tfa_linux_ffa.log"
DEFAULT_PKVM_LOG="${ROOT_DIR}/test_run_pkvm_ffa.log"

if [[ $# -ne 0 && $# -ne 3 ]]; then
    echo "Usage: $0 [<spmc-log> <tfa-linux-ffa-log> <pkvm-ffa-log>]" >&2
    exit 2
fi

if [[ $# -eq 3 ]]; then
    SPMC_LOG="$1"
    TFA_LOG="$2"
    PKVM_LOG="$3"
else
    SPMC_LOG="$DEFAULT_SPMC_LOG"
    TFA_LOG="$DEFAULT_TFA_LOG"
    PKVM_LOG="$DEFAULT_PKVM_LOG"
fi

for log in "$SPMC_LOG" "$TFA_LOG" "$PKVM_LOG"; do
    if [[ ! -f "$log" ]]; then
        echo "[FAIL] log not found: $log"
        exit 2
    fi
done

overall_ok=0

check_spmc() {
    local log_file="$1"
    local has_all_complete=0
    local pass_count=0
    local has_bad=0

    if rg -a -q "All tests complete\\." "$log_file"; then
        has_all_complete=1
    fi

    pass_count="$(rg -a -n "Test [0-9]+: .*PASS" "$log_file" -S | wc -l | tr -d ' ')"

    if rg -a -q "Kernel panic|!!! PANIC !!!|unexpected SP exit|SP Idle transition failed|Test [0-9]+: .*FAIL|\\[FAIL\\]" "$log_file"; then
        has_bad=1
    fi

    echo "spmc: pass_count=${pass_count} all_complete=${has_all_complete} bad=${has_bad}"
    if [[ "$pass_count" -ge 13 && "$has_all_complete" -eq 1 && "$has_bad" -eq 0 ]]; then
        echo "[PASS] run-spmc log looks healthy"
        return 0
    fi

    echo "[FAIL] run-spmc log is suspicious"
    return 1
}

check_tfa_linux_ffa() {
    local log_file="$1"
    local summary_total=0
    local has_nonzero_failed=0
    local has_bad=0

    summary_total="$(rg -a -n "Results: [0-9]+ passed, [0-9]+ failed" "$log_file" -S | wc -l | tr -d ' ')"

    if rg -a -q "Results: [0-9]+ passed, [1-9][0-9]* failed|\\[FAIL\\]|Kernel panic|!!! PANIC !!!|unexpected SP exit|SP Idle transition failed" "$log_file"; then
        has_nonzero_failed=1
        has_bad=1
    fi

    # Defensive: require at least one explicit PASSED line and one summary line.
    if ! rg -a -q "PASSED \\(|Results: [0-9]+ passed, [0-9]+ failed" "$log_file"; then
        has_bad=1
    fi

    echo "tfa-linux-ffa: summaries=${summary_total} nonzero_failed=${has_nonzero_failed} bad=${has_bad}"
    if [[ "$summary_total" -ge 1 && "$has_nonzero_failed" -eq 0 && "$has_bad" -eq 0 ]]; then
        echo "[PASS] run-tfa-linux-ffa log looks healthy"
        return 0
    fi

    echo "[FAIL] run-tfa-linux-ffa log is suspicious"
    return 1
}

check_pkvm_ffa() {
    local log_file="$1"
    if "${ROOT_DIR}/scripts/check_ffa_real.sh" "$log_file"; then
        echo "[PASS] run-pkvm-ffa-test log looks healthy"
        return 0
    fi

    echo "[FAIL] run-pkvm-ffa-test log is suspicious"
    return 1
}

echo "== Unified Test Suite Check =="
echo "spmc_log=${SPMC_LOG}"
echo "tfa_log=${TFA_LOG}"
echo "pkvm_log=${PKVM_LOG}"
echo

if check_spmc "$SPMC_LOG"; then
    :
else
    overall_ok=1
fi

if check_tfa_linux_ffa "$TFA_LOG"; then
    :
else
    overall_ok=1
fi

if check_pkvm_ffa "$PKVM_LOG"; then
    :
else
    overall_ok=1
fi

echo
if [[ "$overall_ok" -eq 0 ]]; then
    echo "PASS: all test suites look healthy"
    exit 0
fi

echo "FAIL: one or more test suites look suspicious"
exit 1

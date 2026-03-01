//! Global state tests
//!
//! Tests PendingCpuOn atomics and UartRxRing lock-free ring buffer.

use hypervisor::global::{PendingCpuOn, UartRxRing};

pub fn run_global_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Global State Test\n");
    hypervisor::log_info!("========================================\n\n");

    // === PendingCpuOn tests ===

    // Test 1: Fresh PendingCpuOn — take returns None
    hypervisor::log_info!("[GLOBAL] Test 1: PendingCpuOn empty...\n");
    let pending = PendingCpuOn::new();
    if pending.take().is_some() {
        hypervisor::log_info!("[GLOBAL] FAILED: should be None\n");
        return;
    }
    hypervisor::log_info!("[GLOBAL] Test 1 PASSED\n\n");

    // Test 2: Request then take
    hypervisor::log_info!("[GLOBAL] Test 2: PendingCpuOn request+take...\n");
    pending.request(2, 0x4800_0000, 0xDEAD);
    match pending.take() {
        Some((target, entry, ctx)) => {
            if target != 2 || entry != 0x4800_0000 || ctx != 0xDEAD {
                hypervisor::log_info!("[GLOBAL] FAILED: wrong values\n");
                return;
            }
        }
        None => {
            hypervisor::log_info!("[GLOBAL] FAILED: should be Some\n");
            return;
        }
    }
    hypervisor::log_info!("[GLOBAL] Test 2 PASSED\n\n");

    // Test 3: Second take returns None (consumed)
    hypervisor::log_info!("[GLOBAL] Test 3: PendingCpuOn consumed...\n");
    if pending.take().is_some() {
        hypervisor::log_info!("[GLOBAL] FAILED: should be None after take\n");
        return;
    }
    hypervisor::log_info!("[GLOBAL] Test 3 PASSED\n\n");

    // === UartRxRing tests ===

    // Test 4: Empty ring — pop returns None
    hypervisor::log_info!("[GLOBAL] Test 4: UartRxRing empty...\n");
    let ring = UartRxRing::new();
    if ring.pop().is_some() {
        hypervisor::log_info!("[GLOBAL] FAILED: should be None\n");
        return;
    }
    hypervisor::log_info!("[GLOBAL] Test 4 PASSED\n\n");

    // Test 5: Push and pop
    hypervisor::log_info!("[GLOBAL] Test 5: UartRxRing push+pop...\n");
    ring.push(b'A');
    ring.push(b'B');
    ring.push(b'C');
    let a = ring.pop();
    let b = ring.pop();
    let c = ring.pop();
    let d = ring.pop();
    if a != Some(b'A') || b != Some(b'B') || c != Some(b'C') || d.is_some() {
        hypervisor::log_info!("[GLOBAL] FAILED: push/pop mismatch\n");
        return;
    }
    hypervisor::log_info!("[GLOBAL] Test 5 PASSED\n\n");

    // Test 6: Ring full — drops overflow
    hypervisor::log_info!("[GLOBAL] Test 6: UartRxRing overflow...\n");
    let ring2 = UartRxRing::new();
    // Ring size is 64, capacity is 63 (sentinel slot)
    for i in 0..63u8 {
        ring2.push(i);
    }
    ring2.push(0xFF); // Should be dropped (full)
                      // Drain and verify
    let mut last = 0u8;
    let mut count = 0u32;
    while let Some(ch) = ring2.pop() {
        last = ch;
        count += 1;
    }
    if count != 63 || last != 62 {
        hypervisor::log_info!(
            "[GLOBAL] FAILED: expected 63 items, last=62, got count={} last={}\n",
            count,
            last
        );
        return;
    }
    hypervisor::log_info!("[GLOBAL] Test 6 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Global State Test PASSED (6 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}

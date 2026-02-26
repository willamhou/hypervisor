//! NetRxRing SPSC ring buffer tests

use hypervisor::vswitch::{NetRxRing, MAX_FRAME_SIZE};

pub fn run_net_rx_ring_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  NetRxRing Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Test 1: Empty ring take() -> None
    hypervisor::log_info!("[NETRX] Test 1: Empty take...\n");
    let ring = NetRxRing::new();
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let result = ring.take(&mut buf);
    assert_eq_test(result.is_none(), true, "empty ring should return None");
    hypervisor::log_info!("[NETRX] Test 1 PASSED\n\n");

    // Test 2: Store + take round-trip
    hypervisor::log_info!("[NETRX] Test 2: Store + take...\n");
    let frame = [0xAA; 64];
    let stored = ring.store(&frame);
    assert_eq_test(stored, true, "store should succeed");
    let len = ring.take(&mut buf);
    assert_eq_test(len, Some(64), "take should return 64 bytes");
    assert_eq_test(buf[0], 0xAA, "first byte should be 0xAA");
    assert_eq_test(buf[63], 0xAA, "last byte should be 0xAA");
    hypervisor::log_info!("[NETRX] Test 2 PASSED\n\n");

    // Test 3: Take empties ring
    hypervisor::log_info!("[NETRX] Test 3: Take empties...\n");
    let result = ring.take(&mut buf);
    assert_eq_test(result.is_none(), true, "ring should be empty after take");
    hypervisor::log_info!("[NETRX] Test 3 PASSED\n\n");

    // Test 4: Fill 8 frames -> all succeed
    hypervisor::log_info!("[NETRX] Test 4: Fill 8...\n");
    let frame = [0xBB; 100];
    for _ in 0..8 {
        let ok = ring.store(&frame);
        assert_eq_test(ok, true, "store within capacity should succeed");
    }
    hypervisor::log_info!("[NETRX] Test 4 PASSED\n\n");

    // Test 5: 9th frame -> store() returns false (full)
    hypervisor::log_info!("[NETRX] Test 5: Overflow...\n");
    let ok = ring.store(&frame);
    assert_eq_test(ok, false, "store on full ring should fail");
    hypervisor::log_info!("[NETRX] Test 5 PASSED\n\n");

    // Test 6: Take 1 + store 1 -> succeeds (wraparound)
    hypervisor::log_info!("[NETRX] Test 6: Wraparound...\n");
    let len = ring.take(&mut buf);
    assert_eq_test(len, Some(100), "take should return 100");
    let ok = ring.store(&frame);
    assert_eq_test(ok, true, "store after take should succeed");
    hypervisor::log_info!("[NETRX] Test 6 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  NetRxRing Test PASSED (8 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}

fn assert_eq_test<T: PartialEq + core::fmt::Debug>(a: T, b: T, msg: &str) {
    if a != b {
        hypervisor::log_info!("[NETRX] ASSERTION FAILED: {}\n", msg);
        panic!("test assertion failed");
    }
}

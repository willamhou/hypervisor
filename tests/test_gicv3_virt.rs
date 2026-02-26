//! GICv3 Virtual Interface Tests
//!
//! Tests for the GICv3 List Register management and virtual interrupt injection.

use hypervisor::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;

/// Test GICv3 virtual interface functionality
pub fn run_gicv3_virt_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  GICv3 Virtual Interface Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Check if GICv3 is available
    if !GicV3VirtualInterface::is_available() {
        hypervisor::log_info!("[GICv3 VIRT] GICv3 not available, skipping test\n");
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] GICv3 is available\n");

    // Test 1: Read VTR to get LR count
    hypervisor::log_info!("[GICv3 VIRT] Test 1: Reading VTR...\n");
    let num_lrs = GicV3VirtualInterface::num_list_registers();
    hypervisor::log_info!("[GICv3 VIRT] Number of List Registers: {}\n", num_lrs);

    if num_lrs < 4 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Expected at least 4 LRs\n");
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] Test 1 PASSED\n\n");

    // Test 2: Build and verify LR value
    hypervisor::log_info!("[GICv3 VIRT] Test 2: Building LR value...\n");
    let test_lr = GicV3VirtualInterface::build_lr(27, 0xA0);

    // Verify fields
    let state = GicV3VirtualInterface::get_lr_state(test_lr);
    let intid = GicV3VirtualInterface::get_lr_intid(test_lr);
    let priority = GicV3VirtualInterface::get_lr_priority(test_lr);

    if state != GicV3VirtualInterface::LR_STATE_PENDING {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: State should be Pending\n");
        return;
    }
    if intid != 27 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: INTID should be 27\n");
        return;
    }
    if priority != 0xA0 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Priority should be 0xA0\n");
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] LR fields verified: state=Pending, intid=27, priority=0xA0\n");
    hypervisor::log_info!("[GICv3 VIRT] Test 2 PASSED\n\n");

    // Test 3: Write and read LR
    hypervisor::log_info!("[GICv3 VIRT] Test 3: Write/Read LR...\n");
    GicV3VirtualInterface::write_lr(0, test_lr);
    let read_back = GicV3VirtualInterface::read_lr(0);

    if read_back != test_lr {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: LR read-back mismatch\n");
        hypervisor::log_info!("[GICv3 VIRT] Expected: {:#018x}\n", test_lr);
        hypervisor::log_info!("[GICv3 VIRT] Got: {:#018x}\n", read_back);
        // Clear LR before returning
        GicV3VirtualInterface::write_lr(0, 0);
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] LR write/read verified\n");

    // Clear the LR
    GicV3VirtualInterface::write_lr(0, 0);
    let cleared = GicV3VirtualInterface::read_lr(0);
    if GicV3VirtualInterface::get_lr_state(cleared) != GicV3VirtualInterface::LR_STATE_INVALID {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: LR not cleared properly\n");
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] LR cleared successfully\n");
    hypervisor::log_info!("[GICv3 VIRT] Test 3 PASSED\n\n");

    // Test 4: Find free LR
    hypervisor::log_info!("[GICv3 VIRT] Test 4: Find free LR...\n");
    let free_lr = GicV3VirtualInterface::find_free_lr();
    if free_lr.is_none() {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: No free LR found\n");
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] Found free LR at index: {}\n", free_lr.unwrap());
    hypervisor::log_info!("[GICv3 VIRT] Test 4 PASSED\n\n");

    // Test 5: Pending count
    hypervisor::log_info!("[GICv3 VIRT] Test 5: Pending count...\n");

    // Should be 0 initially
    let count = GicV3VirtualInterface::pending_count();
    if count != 0 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Expected 0 pending, got {}\n", count);
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] Initial pending count: 0\n");

    // Inject one interrupt
    if GicV3VirtualInterface::inject_interrupt(27, 0xA0).is_err() {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Failed to inject interrupt\n");
        return;
    }

    let count = GicV3VirtualInterface::pending_count();
    if count != 1 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Expected 1 pending, got {}\n", count);
        GicV3VirtualInterface::clear_interrupt(27);
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] After inject: pending count = 1\n");

    // Clear and verify
    GicV3VirtualInterface::clear_interrupt(27);
    let count = GicV3VirtualInterface::pending_count();
    if count != 0 {
        hypervisor::log_info!("[GICv3 VIRT] ERROR: Expected 0 after clear, got {}\n", count);
        return;
    }
    hypervisor::log_info!("[GICv3 VIRT] After clear: pending count = 0\n");
    hypervisor::log_info!("[GICv3 VIRT] Test 5 PASSED\n\n");

    // Test 6: Check ARMv8.4 features
    hypervisor::log_info!("[GICv3 VIRT] Test 6: ARMv8.4 feature check...\n");
    if GicV3VirtualInterface::has_armv8_4_features() {
        hypervisor::log_info!("[GICv3 VIRT] ARMv8.4+ features available (nested virt supported)\n");
    } else {
        hypervisor::log_info!("[GICv3 VIRT] ARMv8.4+ features not available\n");
    }
    hypervisor::log_info!("[GICv3 VIRT] Test 6 PASSED (informational)\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  GICv3 Virtual Interface Test PASSED\n");
    hypervisor::log_info!("========================================\n\n");
}

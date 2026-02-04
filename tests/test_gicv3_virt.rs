//! GICv3 Virtual Interface Tests
//!
//! Tests for the GICv3 List Register management and virtual interrupt injection.

use hypervisor::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
use hypervisor::uart_puts;

/// Test GICv3 virtual interface functionality
pub fn run_gicv3_virt_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  GICv3 Virtual Interface Test\n");
    uart_puts(b"========================================\n\n");

    // Check if GICv3 is available
    if !GicV3VirtualInterface::is_available() {
        uart_puts(b"[GICv3 VIRT] GICv3 not available, skipping test\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] GICv3 is available\n");

    // Test 1: Read VTR to get LR count
    uart_puts(b"[GICv3 VIRT] Test 1: Reading VTR...\n");
    let num_lrs = GicV3VirtualInterface::num_list_registers();
    uart_puts(b"[GICv3 VIRT] Number of List Registers: ");
    print_num(num_lrs);
    uart_puts(b"\n");

    if num_lrs < 4 {
        uart_puts(b"[GICv3 VIRT] ERROR: Expected at least 4 LRs\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] Test 1 PASSED\n\n");

    // Test 2: Build and verify LR value
    uart_puts(b"[GICv3 VIRT] Test 2: Building LR value...\n");
    let test_lr = GicV3VirtualInterface::build_lr(27, 0xA0);

    // Verify fields
    let state = GicV3VirtualInterface::get_lr_state(test_lr);
    let intid = GicV3VirtualInterface::get_lr_intid(test_lr);
    let priority = GicV3VirtualInterface::get_lr_priority(test_lr);

    if state != GicV3VirtualInterface::LR_STATE_PENDING {
        uart_puts(b"[GICv3 VIRT] ERROR: State should be Pending\n");
        return;
    }
    if intid != 27 {
        uart_puts(b"[GICv3 VIRT] ERROR: INTID should be 27\n");
        return;
    }
    if priority != 0xA0 {
        uart_puts(b"[GICv3 VIRT] ERROR: Priority should be 0xA0\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] LR fields verified: state=Pending, intid=27, priority=0xA0\n");
    uart_puts(b"[GICv3 VIRT] Test 2 PASSED\n\n");

    // Test 3: Write and read LR
    uart_puts(b"[GICv3 VIRT] Test 3: Write/Read LR...\n");
    GicV3VirtualInterface::write_lr(0, test_lr);
    let read_back = GicV3VirtualInterface::read_lr(0);

    if read_back != test_lr {
        uart_puts(b"[GICv3 VIRT] ERROR: LR read-back mismatch\n");
        uart_puts(b"[GICv3 VIRT] Expected: ");
        print_hex(test_lr);
        uart_puts(b"\n[GICv3 VIRT] Got: ");
        print_hex(read_back);
        uart_puts(b"\n");
        // Clear LR before returning
        GicV3VirtualInterface::write_lr(0, 0);
        return;
    }
    uart_puts(b"[GICv3 VIRT] LR write/read verified\n");

    // Clear the LR
    GicV3VirtualInterface::write_lr(0, 0);
    let cleared = GicV3VirtualInterface::read_lr(0);
    if GicV3VirtualInterface::get_lr_state(cleared) != GicV3VirtualInterface::LR_STATE_INVALID {
        uart_puts(b"[GICv3 VIRT] ERROR: LR not cleared properly\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] LR cleared successfully\n");
    uart_puts(b"[GICv3 VIRT] Test 3 PASSED\n\n");

    // Test 4: Find free LR
    uart_puts(b"[GICv3 VIRT] Test 4: Find free LR...\n");
    let free_lr = GicV3VirtualInterface::find_free_lr();
    if free_lr.is_none() {
        uart_puts(b"[GICv3 VIRT] ERROR: No free LR found\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] Found free LR at index: ");
    print_num(free_lr.unwrap() as u32);
    uart_puts(b"\n");
    uart_puts(b"[GICv3 VIRT] Test 4 PASSED\n\n");

    // Test 5: Pending count
    uart_puts(b"[GICv3 VIRT] Test 5: Pending count...\n");

    // Should be 0 initially
    let count = GicV3VirtualInterface::pending_count();
    if count != 0 {
        uart_puts(b"[GICv3 VIRT] ERROR: Expected 0 pending, got ");
        print_num(count as u32);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] Initial pending count: 0\n");

    // Inject one interrupt
    if GicV3VirtualInterface::inject_interrupt(27, 0xA0).is_err() {
        uart_puts(b"[GICv3 VIRT] ERROR: Failed to inject interrupt\n");
        return;
    }

    let count = GicV3VirtualInterface::pending_count();
    if count != 1 {
        uart_puts(b"[GICv3 VIRT] ERROR: Expected 1 pending, got ");
        print_num(count as u32);
        uart_puts(b"\n");
        GicV3VirtualInterface::clear_interrupt(27);
        return;
    }
    uart_puts(b"[GICv3 VIRT] After inject: pending count = 1\n");

    // Clear and verify
    GicV3VirtualInterface::clear_interrupt(27);
    let count = GicV3VirtualInterface::pending_count();
    if count != 0 {
        uart_puts(b"[GICv3 VIRT] ERROR: Expected 0 after clear, got ");
        print_num(count as u32);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GICv3 VIRT] After clear: pending count = 0\n");
    uart_puts(b"[GICv3 VIRT] Test 5 PASSED\n\n");

    // Test 6: Check ARMv8.4 features
    uart_puts(b"[GICv3 VIRT] Test 6: ARMv8.4 feature check...\n");
    if GicV3VirtualInterface::has_armv8_4_features() {
        uart_puts(b"[GICv3 VIRT] ARMv8.4+ features available (nested virt supported)\n");
    } else {
        uart_puts(b"[GICv3 VIRT] ARMv8.4+ features not available\n");
    }
    uart_puts(b"[GICv3 VIRT] Test 6 PASSED (informational)\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  GICv3 Virtual Interface Test PASSED\n");
    uart_puts(b"========================================\n\n");
}

/// Helper to print a number
fn print_num(n: u32) {
    if n >= 10 {
        print_num(n / 10);
    }
    let digit = (b'0' + (n % 10) as u8) as u8;
    uart_puts(&[digit]);
}

/// Helper to print hex value
fn print_hex(n: u64) {
    uart_puts(b"0x");
    for i in (0..16).rev() {
        let nibble = ((n >> (i * 4)) & 0xF) as u8;
        let c = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        uart_puts(&[c]);
    }
}

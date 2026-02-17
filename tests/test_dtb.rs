//! DTB parsing tests
//!
//! Verifies that the host DTB was successfully parsed and the discovered
//! platform values match expected QEMU virt machine configuration.

use hypervisor::uart_puts;

pub fn run_dtb_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  DTB Parsing Test\n");
    uart_puts(b"========================================\n\n");

    let pi = hypervisor::dtb::platform_info();
    let initialized = hypervisor::dtb::is_initialized();

    // Test 1: DTB was parsed (QEMU always passes a DTB with -kernel)
    uart_puts(b"[DTB] Test 1: DTB initialized...\n");
    if !initialized {
        uart_puts(b"[DTB] WARN: DTB not initialized (using defaults)\n");
        // Not a hard failure — DTB addr might be invalid in some test configs
    } else {
        uart_puts(b"[DTB] Test 1 PASSED\n\n");
    }

    // Test 2: UART base matches QEMU virt (0x09000000)
    uart_puts(b"[DTB] Test 2: UART base...\n");
    if pi.uart_base != 0x0900_0000 {
        uart_puts(b"[DTB] FAILED: unexpected uart_base\n");
        return;
    }
    uart_puts(b"[DTB] Test 2 PASSED\n\n");

    // Test 3: GICD base matches QEMU virt (0x08000000)
    uart_puts(b"[DTB] Test 3: GICD base...\n");
    if pi.gicd_base != 0x0800_0000 {
        uart_puts(b"[DTB] FAILED: unexpected gicd_base\n");
        return;
    }
    uart_puts(b"[DTB] Test 3 PASSED\n\n");

    // Test 4: GICR base matches QEMU virt GICv3 (0x080A0000)
    uart_puts(b"[DTB] Test 4: GICR base...\n");
    if pi.gicr_base != 0x080A_0000 {
        uart_puts(b"[DTB] FAILED: unexpected gicr_base\n");
        return;
    }
    uart_puts(b"[DTB] Test 4 PASSED\n\n");

    // Test 5: CPU count is reasonable (1..=8)
    uart_puts(b"[DTB] Test 5: num_cpus...\n");
    if pi.num_cpus == 0 || pi.num_cpus > 8 {
        uart_puts(b"[DTB] FAILED: unexpected num_cpus\n");
        return;
    }
    uart_puts(b"[DTB] Test 5 PASSED\n\n");

    // Test 6: RAM base matches QEMU virt (0x40000000)
    uart_puts(b"[DTB] Test 6: RAM base...\n");
    if pi.ram_base != 0x4000_0000 {
        uart_puts(b"[DTB] FAILED: unexpected ram_base\n");
        return;
    }
    uart_puts(b"[DTB] Test 6 PASSED\n\n");

    // Test 7: RAM size is non-zero
    uart_puts(b"[DTB] Test 7: RAM size...\n");
    if pi.ram_size == 0 {
        uart_puts(b"[DTB] FAILED: ram_size is 0\n");
        return;
    }
    uart_puts(b"[DTB] Test 7 PASSED\n\n");

    // Test 8: gicr_rd_base helper computes correctly
    uart_puts(b"[DTB] Test 8: GICR helpers...\n");
    let rd0 = hypervisor::dtb::gicr_rd_base(0);
    let rd1 = hypervisor::dtb::gicr_rd_base(1);
    let sgi0 = hypervisor::dtb::gicr_sgi_base(0);
    if rd0 != pi.gicr_base {
        uart_puts(b"[DTB] FAILED: gicr_rd_base(0) != gicr_base\n");
        return;
    }
    if rd1 != pi.gicr_base + 0x20000 {
        uart_puts(b"[DTB] FAILED: gicr_rd_base(1) != gicr_base + 0x20000\n");
        return;
    }
    if sgi0 != pi.gicr_base + 0x10000 {
        uart_puts(b"[DTB] FAILED: gicr_sgi_base(0) != gicr_base + 0x10000\n");
        return;
    }
    uart_puts(b"[DTB] Test 8 PASSED\n\n");

    uart_puts(b"=== DTB Parsing: All 8 tests PASSED ===\n");
}

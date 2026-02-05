//! Test for guest_loader module
//!
//! Verifies GuestConfig creation and default values.

use hypervisor::guest_loader::GuestConfig;
use hypervisor::uart_puts;

/// Test GuestConfig default values
pub fn run_test() {
    uart_puts(b"\n[TEST] Guest Loader Test\n");
    uart_puts(b"[TEST] ========================\n");

    // Test zephyr_default configuration
    let config = GuestConfig::zephyr_default();

    // Verify load address
    uart_puts(b"[TEST] Checking load_addr... ");
    if config.load_addr == 0x4800_0000 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    // Verify memory size (128MB)
    uart_puts(b"[TEST] Checking mem_size... ");
    if config.mem_size == 128 * 1024 * 1024 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    // Verify entry point
    uart_puts(b"[TEST] Checking entry_point... ");
    if config.entry_point == 0x4800_0000 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    uart_puts(b"[TEST] Guest Loader Test PASSED\n\n");
}

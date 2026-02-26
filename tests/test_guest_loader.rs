//! Test for guest_loader module
//!
//! Verifies GuestConfig creation and default values.

use hypervisor::guest_loader::GuestConfig;

/// Test GuestConfig default values
pub fn run_test() {
    hypervisor::log_info!("\n[TEST] Guest Loader Test\n");
    hypervisor::log_info!("[TEST] ========================\n");

    // Test zephyr_default configuration
    let config = GuestConfig::zephyr_default();

    // Verify load address
    hypervisor::log_info!("[TEST] Checking load_addr... ");
    if config.load_addr == 0x4800_0000 {
        hypervisor::log_info!("PASS\n");
    } else {
        hypervisor::log_info!("FAIL\n");
        return;
    }

    // Verify memory size (128MB)
    hypervisor::log_info!("[TEST] Checking mem_size... ");
    if config.mem_size == 128 * 1024 * 1024 {
        hypervisor::log_info!("PASS\n");
    } else {
        hypervisor::log_info!("FAIL\n");
        return;
    }

    // Verify entry point
    hypervisor::log_info!("[TEST] Checking entry_point... ");
    if config.entry_point == 0x4800_0000 {
        hypervisor::log_info!("PASS\n");
    } else {
        hypervisor::log_info!("FAIL\n");
        return;
    }

    hypervisor::log_info!("[TEST] Guest Loader Test PASSED\n\n");
}

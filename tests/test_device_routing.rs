//! DeviceManager routing tests
//!
//! Tests device registration, MMIO address routing, and accessor methods.

use hypervisor::devices::gic::VirtualGicd;
use hypervisor::devices::pl011::VirtualUart;
use hypervisor::devices::{Device, DeviceManager};
use hypervisor::uart_puts;

pub fn run_device_routing_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Device Manager Routing Test\n");
    uart_puts(b"========================================\n\n");

    let mut dm = DeviceManager::new();

    // Test 1: Empty manager returns 0 for reads
    uart_puts(b"[DEVMGR] Test 1: Empty read...\n");
    let result = dm.handle_mmio(0x0900_0000, 0, 4, false);
    if result != Some(0) {
        uart_puts(b"[DEVMGR] FAILED: empty read should return Some(0)\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 1 PASSED\n\n");

    // Test 2: Register UART, read hits
    uart_puts(b"[DEVMGR] Test 2: Register + route UART...\n");
    let uart = VirtualUart::new();
    dm.register_device(Device::Uart(uart));
    // Read UART Flag Register (offset 0x18) — should return something (TX empty bit)
    let result = dm.handle_mmio(0x0900_0018, 0, 4, false);
    if result.is_none() {
        uart_puts(b"[DEVMGR] FAILED: UART read returned None\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 2 PASSED\n\n");

    // Test 3: Miss address returns 0
    uart_puts(b"[DEVMGR] Test 3: Miss address...\n");
    let result = dm.handle_mmio(0x1234_0000, 0, 4, false);
    if result != Some(0) {
        uart_puts(b"[DEVMGR] FAILED: miss should return Some(0)\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 3 PASSED\n\n");

    // Test 4: Register GICD, route_spi works
    uart_puts(b"[DEVMGR] Test 4: GICD route_spi...\n");
    let gicd = VirtualGicd::new();
    dm.register_device(Device::Gicd(gicd));
    let target = dm.route_spi(48);
    if target != 0 {
        // Default IROUTER is 0 -> vCPU 0
        uart_puts(b"[DEVMGR] FAILED: route_spi should be 0\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 4 PASSED\n\n");

    // Test 5: uart_mut accessor
    uart_puts(b"[DEVMGR] Test 5: uart_mut accessor...\n");
    if dm.uart_mut().is_none() {
        uart_puts(b"[DEVMGR] FAILED: uart_mut should find UART\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 5 PASSED\n\n");

    // Test 6: Reset clears all devices
    uart_puts(b"[DEVMGR] Test 6: Reset...\n");
    dm.reset();
    if dm.uart_mut().is_some() {
        uart_puts(b"[DEVMGR] FAILED: uart_mut should be None after reset\n");
        return;
    }
    uart_puts(b"[DEVMGR] Test 6 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Device Manager Routing Test PASSED (6 assertions)\n");
    uart_puts(b"========================================\n\n");
}

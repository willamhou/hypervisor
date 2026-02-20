//! Multi-VM device isolation tests
//!
//! Verifies that DEVICES[0] and DEVICES[1] are independent and
//! can register/route devices without cross-contamination.

use hypervisor::devices::gic::VirtualGicd;
use hypervisor::devices::pl011::VirtualUart;
use hypervisor::devices::Device;
use hypervisor::global::DEVICES;
use hypervisor::uart_puts;

pub fn run_multi_vm_devices_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Multi-VM Device Isolation Test\n");
    uart_puts(b"========================================\n\n");

    // Reset both device managers
    DEVICES[0].reset();
    DEVICES[1].reset();

    // Test 1: Register UART to VM 0, verify VM 1 has no UART
    // Read UARTFR (offset 0x18) — a registered UART returns non-zero (TX FIFO
    // empty flag = 0x80), while an empty DeviceManager returns 0 for unknown
    // addresses.
    uart_puts(b"[MV-DEV] Test 1: Device registration isolation...\n");
    DEVICES[0].register_device(Device::Uart(VirtualUart::new()));
    let uart_fr_0 = DEVICES[0]
        .handle_mmio(0x0900_0018, 0, 4, false)
        .unwrap_or(0);
    let uart_fr_1 = DEVICES[1]
        .handle_mmio(0x0900_0018, 0, 4, false)
        .unwrap_or(0);
    if uart_fr_0 == 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 0 UARTFR should be non-zero\n");
        return;
    }
    if uart_fr_1 != 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 1 should return 0 (no UART registered)\n");
        return;
    }
    uart_puts(b"[MV-DEV] Test 1 PASSED\n\n");

    // Test 2: Register GICD to VM 1, verify independent MMIO routing
    // Read GICD_CTLR (offset 0x000) — returns non-zero (ARE_NS=0x10 forced on)
    // when registered, 0 when not.
    uart_puts(b"[MV-DEV] Test 2: Independent GICD registration...\n");
    DEVICES[1].register_device(Device::Gicd(VirtualGicd::new()));
    let vm1_gicd = DEVICES[1]
        .handle_mmio(0x0800_0000, 0, 4, false)
        .unwrap_or(0);
    let vm0_gicd = DEVICES[0]
        .handle_mmio(0x0800_0000, 0, 4, false)
        .unwrap_or(0);
    if vm1_gicd == 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 1 GICD_CTLR should be non-zero\n");
        return;
    }
    if vm0_gicd != 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 0 should return 0 (no GICD registered)\n");
        return;
    }
    uart_puts(b"[MV-DEV] Test 2 PASSED\n\n");

    // Test 3: Cross-check — VM 0 has UART, VM 1 has GICD, no cross-leak
    uart_puts(b"[MV-DEV] Test 3: No cross-leak...\n");
    let vm0_uart_fr = DEVICES[0]
        .handle_mmio(0x0900_0018, 0, 4, false)
        .unwrap_or(0);
    let vm1_uart_fr = DEVICES[1]
        .handle_mmio(0x0900_0018, 0, 4, false)
        .unwrap_or(0);
    if vm0_uart_fr == 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 0 UARTFR should be non-zero\n");
        return;
    }
    if vm1_uart_fr != 0 {
        uart_puts(b"[MV-DEV] FAILED: VM 1 should return 0 for UART\n");
        return;
    }
    uart_puts(b"[MV-DEV] Test 3 PASSED\n\n");

    // Clean up — restore device state for subsequent tests
    DEVICES[0].reset();
    DEVICES[1].reset();

    uart_puts(b"========================================\n");
    uart_puts(b"  Multi-VM Device Isolation Test PASSED (3 assertions)\n");
    uart_puts(b"========================================\n\n");
}

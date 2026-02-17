//! VMID/VTTBR encoding tests
//!
//! Verifies that Stage2Config::new_with_vmid correctly encodes
//! VMID in VTTBR_EL2 bits [63:48].

use hypervisor::arch::aarch64::mm::mmu::Stage2Config;
use hypervisor::uart_puts;

pub fn run_vmid_vttbr_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  VMID/VTTBR Encoding Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: VMID 0 — bits [63:48] should be 0
    uart_puts(b"[VMID] Test 1: VMID 0 encoding...\n");
    let config0 = Stage2Config::new_with_vmid(0x4100_0000, 0);
    let vmid_bits = config0.vttbr >> 48;
    if vmid_bits != 0 {
        uart_puts(b"[VMID] FAILED: expected VMID 0, got ");
        hypervisor::uart_put_hex(vmid_bits);
        uart_puts(b"\n");
        return;
    }
    // Verify page table base is preserved
    let base = config0.vttbr & 0x0000_FFFF_FFFF_FFFE;
    if base != 0x4100_0000 {
        uart_puts(b"[VMID] FAILED: base address corrupted\n");
        return;
    }
    uart_puts(b"[VMID] Test 1 PASSED\n\n");

    // Test 2: VMID 1 — bits [63:48] should be 1
    uart_puts(b"[VMID] Test 2: VMID 1 encoding...\n");
    let config1 = Stage2Config::new_with_vmid(0x6100_0000, 1);
    let vmid_bits = config1.vttbr >> 48;
    if vmid_bits != 1 {
        uart_puts(b"[VMID] FAILED: expected VMID 1, got ");
        hypervisor::uart_put_hex(vmid_bits);
        uart_puts(b"\n");
        return;
    }
    // Verify page table base is preserved
    let base = config1.vttbr & 0x0000_FFFF_FFFF_FFFE;
    if base != 0x6100_0000 {
        uart_puts(b"[VMID] FAILED: base address corrupted\n");
        return;
    }
    uart_puts(b"[VMID] Test 2 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  VMID/VTTBR Encoding Test PASSED (2 assertions)\n");
    uart_puts(b"========================================\n\n");
}

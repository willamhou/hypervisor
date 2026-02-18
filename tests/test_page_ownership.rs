//! Test page ownership tracking via Stage-2 PTE SW bits

use hypervisor::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};

pub fn run_page_ownership_test() {
    hypervisor::uart_puts(b"\n=== Test: Page Ownership (SW bits) ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    // Create a mapper and map a 2MB region
    let mut mapper = DynamicIdentityMapper::new();
    mapper.map_region(0x5000_0000, 0x0020_0000, MemoryAttribute::Normal).unwrap();

    // Test 1: Default SW bits should be 0 (OWNED)
    {
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0) {
            hypervisor::uart_puts(b"  [PASS] Default SW bits = 0 (OWNED)\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Default SW bits\n");
            fail += 1;
        }
    }

    // Test 2: Write SHARED_OWNED (0b01) and read back
    {
        mapper.write_sw_bits(0x5000_0000, 0b01).unwrap();
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0b01) {
            hypervisor::uart_puts(b"  [PASS] Write/read SHARED_OWNED\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Write/read SHARED_OWNED\n");
            fail += 1;
        }
    }

    // Test 3: Write back to OWNED (0b00) and verify
    {
        mapper.write_sw_bits(0x5000_0000, 0b00).unwrap();
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0b00) {
            hypervisor::uart_puts(b"  [PASS] Restore to OWNED\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Restore to OWNED\n");
            fail += 1;
        }
    }

    // Test 4: Unmapped IPA returns None
    {
        let bits = mapper.read_sw_bits(0x9000_0000);
        if bits.is_none() {
            hypervisor::uart_puts(b"  [PASS] Unmapped IPA returns None\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Unmapped IPA should be None\n");
            fail += 1;
        }
    }

    hypervisor::uart_puts(b"  Results: ");
    hypervisor::uart_put_u64(pass);
    hypervisor::uart_puts(b" passed, ");
    hypervisor::uart_put_u64(fail);
    hypervisor::uart_puts(b" failed\n");
    assert!(fail == 0, "Page ownership tests failed");
}

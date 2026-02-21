//! Unit tests for SecureStage2Config.

use hypervisor::secure_stage2::SecureStage2Config;

pub fn run_tests() {
    crate::uart_puts(b"  test_secure_stage2...\n");
    let mut pass = 0u32;

    // Test 1: VSTTBR contains page table address (masked to valid bits)
    let config = SecureStage2Config::new(0x1000_0000);
    assert_eq!(config.vsttbr & 0x0000_FFFF_FFFF_F000, 0x1000_0000);
    pass += 1;

    // Test 2: VSTCR has T0SZ=16 (48-bit IPA)
    assert_eq!(config.vstcr & 0x3F, 16);
    pass += 1;

    // Test 3: Different addresses produce different VSTTBR
    let config2 = SecureStage2Config::new(0x2000_0000);
    assert_ne!(config.vsttbr, config2.vsttbr);
    pass += 1;

    // Test 4: new_from_vsttbr preserves the value exactly
    let config3 = SecureStage2Config::new_from_vsttbr(0xABCD_0000);
    assert_eq!(config3.vsttbr, 0xABCD_0000);
    pass += 1;

    crate::uart_puts(b"    ");
    crate::print_u32(pass);
    crate::uart_puts(b" assertions passed\n");
}

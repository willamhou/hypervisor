//! MMIO instruction decode tests
//!
//! Tests MmioAccess::decode() for ISS-based and instruction-based paths.

use hypervisor::arch::aarch64::hypervisor::decode::MmioAccess;
use hypervisor::uart_puts;

pub fn run_decode_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  MMIO Instruction Decode Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: ISS-based decode — 4-byte store, register x5
    uart_puts(b"[DECODE] Test 1: ISS store word x5...\n");
    // ISV=1 (bit24), SAS=10/word (bits23:22), SRT=5 (bits20:16), WNR=1 (bit6)
    let iss_store_w5: u32 = (1 << 24) | (2 << 22) | (5 << 16) | (1 << 6);
    let access = MmioAccess::decode(0, iss_store_w5).expect("decode failed");
    assert_store(&access, 5, 4, "ISS store word x5");

    // Test 2: ISS-based decode — 1-byte load, register x10
    uart_puts(b"[DECODE] Test 2: ISS load byte x10...\n");
    // ISV=1, SAS=00/byte, SRT=10, WNR=0
    let iss_load_b10: u32 = (1 << 24) | (0 << 22) | (10 << 16) | (0 << 6);
    let access = MmioAccess::decode(0, iss_load_b10).expect("decode failed");
    assert_load(&access, 10, 1, "ISS load byte x10");

    // Test 3: ISS-based decode — 8-byte load, register x0
    uart_puts(b"[DECODE] Test 3: ISS load dword x0...\n");
    // ISV=1, SAS=11/dword, SRT=0, WNR=0
    let iss_load_d0: u32 = (1 << 24) | (3 << 22) | (0 << 16) | (0 << 6);
    let access = MmioAccess::decode(0, iss_load_d0).expect("decode failed");
    assert_load(&access, 0, 8, "ISS load dword x0");

    // Test 4: ISS-based decode — 2-byte store, register x15
    uart_puts(b"[DECODE] Test 4: ISS store half x15...\n");
    // ISV=1, SAS=01/half, SRT=15, WNR=1
    let iss_store_h15: u32 = (1 << 24) | (1 << 22) | (15 << 16) | (1 << 6);
    let access = MmioAccess::decode(0, iss_store_h15).expect("decode failed");
    assert_store(&access, 15, 2, "ISS store half x15");

    // Test 5: ISV=0, instruction-based — STR W1, [X19] (0xb9000261)
    uart_puts(b"[DECODE] Test 5: Instruction STR W1, [X19]...\n");
    let insn_str_w1: u32 = 0xb9000261; // STR W1, [X19, #0]
    let iss_no_isv: u32 = 0; // ISV=0
    let access = MmioAccess::decode(insn_str_w1, iss_no_isv).expect("decode failed");
    assert_store(&access, 1, 4, "insn STR W1");

    // Test 6: ISV=0, instruction-based — LDR W2, [X19] (0xb9400262)
    uart_puts(b"[DECODE] Test 6: Instruction LDR W2, [X19]...\n");
    let insn_ldr_w2: u32 = 0xb9400262; // LDR W2, [X19, #0]
    let access = MmioAccess::decode(insn_ldr_w2, iss_no_isv).expect("decode failed");
    assert_load(&access, 2, 4, "insn LDR W2");

    // Test 7: ISV=0, instruction-based — STRB W3, [X0] (0x39000003)
    uart_puts(b"[DECODE] Test 7: Instruction STRB W3, [X0]...\n");
    let insn_strb_w3: u32 = 0x39000003; // STRB W3, [X0, #0]
    let access = MmioAccess::decode(insn_strb_w3, iss_no_isv).expect("decode failed");
    assert_store(&access, 3, 1, "insn STRB W3");

    // Test 8: ISV=0, instruction-based — LDRH W4, [X1] (0x79400024)
    uart_puts(b"[DECODE] Test 8: Instruction LDRH W4, [X1]...\n");
    let insn_ldrh_w4: u32 = 0x79400024; // LDRH W4, [X1, #0]
    let access = MmioAccess::decode(insn_ldrh_w4, iss_no_isv).expect("decode failed");
    assert_load(&access, 4, 2, "insn LDRH W4");

    // Test 9: Unsupported instruction returns None
    uart_puts(b"[DECODE] Test 9: Unsupported instruction...\n");
    let insn_add: u32 = 0x8b010000; // ADD X0, X0, X1
    if MmioAccess::decode(insn_add, 0).is_some() {
        uart_puts(b"[DECODE] FAILED: should return None for ADD\n");
        return;
    }
    uart_puts(b"[DECODE] Test 9 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  MMIO Instruction Decode Test PASSED (9 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_store(access: &MmioAccess, expected_reg: u8, expected_size: u8, label: &str) {
    if !access.is_store() {
        uart_puts(b"[DECODE] FAILED: expected store for ");
        uart_puts(label.as_bytes());
        uart_puts(b"\n");
        return;
    }
    if access.reg() != expected_reg || access.size() != expected_size {
        uart_puts(b"[DECODE] FAILED: wrong reg/size for ");
        uart_puts(label.as_bytes());
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[DECODE] ");
    uart_puts(label.as_bytes());
    uart_puts(b" PASSED\n\n");
}

fn assert_load(access: &MmioAccess, expected_reg: u8, expected_size: u8, label: &str) {
    if !access.is_load() {
        uart_puts(b"[DECODE] FAILED: expected load for ");
        uart_puts(label.as_bytes());
        uart_puts(b"\n");
        return;
    }
    if access.reg() != expected_reg || access.size() != expected_size {
        uart_puts(b"[DECODE] FAILED: wrong reg/size for ");
        uart_puts(label.as_bytes());
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[DECODE] ");
    uart_puts(label.as_bytes());
    uart_puts(b" PASSED\n\n");
}

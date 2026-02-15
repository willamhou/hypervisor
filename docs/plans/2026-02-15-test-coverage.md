# Test Coverage Expansion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expand test coverage from 12 wired tests (40 assertions) to ~19 tests (~75+ assertions), covering MMIO decode, GICD/GICR emulation, global state, and fixing dead tests.

**Architecture:** All tests run at EL2 on bare metal (QEMU). No `std`, no `#[test]` harness. Tests are Rust functions called sequentially from `src/main.rs`, outputting to UART. Each test function follows the pattern: print header, run assertions, print PASSED/FAILED.

**Tech Stack:** Rust no_std, QEMU aarch64-virt, UART output for test reporting. Build via `make` and run via `make run`.

---

## Prioritized Test Tasks

Tests are ordered by impact: pure-logic units first (no hardware), then integration tests.

---

### Task 1: MMIO Instruction Decode Tests (`test_decode.rs`)

Tests `MmioAccess::decode()` — pure logic, no hardware dependency. Covers ISS-based and instruction-based decode paths.

**Files:**
- Create: `tests/test_decode.rs`
- Modify: `tests/mod.rs` (add module + re-export)
- Modify: `src/main.rs` (wire into test sequence)

**Step 1: Create the test file**

```rust
// tests/test_decode.rs
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

    // Test 4: ISS-based decode — 2-byte store, register x15, sign-extend
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
```

**Step 2: Wire into test harness**

Add to `tests/mod.rs`:
- `pub mod test_decode;`
- `pub use test_decode::run_decode_test;`

Add to `src/main.rs` after `run_simple_guest_test()`:
- `tests::run_decode_test();`

**Step 3: Build and verify**

Run: `make clean && make run`
Expected: All existing 12 tests pass, plus "MMIO Instruction Decode Test PASSED (9 assertions)"

**Step 4: Commit**

```
feat: add MMIO instruction decode tests (9 assertions)
```

---

### Task 2: GICD Emulation Tests (`test_gicd.rs`)

Tests `VirtualGicd` shadow state: CTLR, TYPER, ISENABLER/ICENABLER set/clear semantics, IROUTER write/read, PIDR2. Pure logic (no physical GICD write-through during test — only shadow state is verified).

**Files:**
- Create: `tests/test_gicd.rs`
- Modify: `tests/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create the test file**

```rust
// tests/test_gicd.rs
//! Virtual GICD emulation tests
//!
//! Tests VirtualGicd shadow state read/write semantics without touching
//! physical GICD hardware. Write-through to physical GICD will occur
//! but is harmless since we're already at EL2 with GICD accessible.

use hypervisor::devices::gic::VirtualGicd;
use hypervisor::devices::MmioDevice;
use hypervisor::uart_puts;

pub fn run_gicd_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Virtual GICD Emulation Test\n");
    uart_puts(b"========================================\n\n");

    let mut gicd = VirtualGicd::new();

    // Test 1: CTLR reads back with ARE_NS forced on
    uart_puts(b"[GICD] Test 1: CTLR ARE_NS forced...\n");
    let ctlr = gicd.read(0x000, 4).unwrap();
    if ctlr & (1 << 4) == 0 {
        uart_puts(b"[GICD] FAILED: ARE_NS not set\n");
        return;
    }
    uart_puts(b"[GICD] Test 1 PASSED\n\n");

    // Test 2: CTLR write preserves ARE_NS
    uart_puts(b"[GICD] Test 2: CTLR write preserves ARE_NS...\n");
    gicd.write(0x000, 0x01, 4); // EnableGrp1NS only
    let ctlr = gicd.read(0x000, 4).unwrap();
    if ctlr != 0x11 { // EnableGrp1NS | ARE_NS
        uart_puts(b"[GICD] FAILED: CTLR should be 0x11\n");
        return;
    }
    uart_puts(b"[GICD] Test 2 PASSED\n\n");

    // Test 3: TYPER reports correct CPUNumber
    uart_puts(b"[GICD] Test 3: TYPER CPUNumber...\n");
    let typer = gicd.read(0x004, 4).unwrap() as u32;
    let cpu_num = (typer >> 5) & 0x7;
    // Default SMP_CPUS is 4, so CPUNumber = 3
    if cpu_num != 3 {
        uart_puts(b"[GICD] FAILED: CPUNumber should be 3, got ");
        hypervisor::uart_put_u64(cpu_num as u64);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GICD] Test 3 PASSED\n\n");

    // Test 4: ISENABLER/ICENABLER set/clear semantics
    uart_puts(b"[GICD] Test 4: ISENABLER/ICENABLER...\n");
    // ISENABLER[1] at offset 0x104 covers INTIDs 32-63
    gicd.write(0x104, 0x0001_0002, 4); // Set bits 1 and 16 (INTID 33, 48)
    let enabled = gicd.read(0x104, 4).unwrap();
    if enabled != 0x0001_0002 {
        uart_puts(b"[GICD] FAILED: ISENABLER readback\n");
        return;
    }
    // ICENABLER[1] at offset 0x184 — clear bit 1 (INTID 33)
    gicd.write(0x184, 0x0000_0002, 4);
    let enabled = gicd.read(0x104, 4).unwrap();
    if enabled != 0x0001_0000 {
        uart_puts(b"[GICD] FAILED: ICENABLER clear\n");
        return;
    }
    uart_puts(b"[GICD] Test 4 PASSED\n\n");

    // Test 5: IROUTER write and route_spi
    uart_puts(b"[GICD] Test 5: IROUTER + route_spi...\n");
    // SPI 48 (INTID 48) → IROUTER index = (48-32) = 16, offset = 0x6100 + 16*8 = 0x6180
    gicd.write(0x6180, 0x02, 8); // Route to Aff0=2 (vCPU 2)
    let target = gicd.route_spi(48);
    if target != 2 {
        uart_puts(b"[GICD] FAILED: route_spi(48) should be 2\n");
        return;
    }
    // Read back IROUTER
    let irouter = gicd.read(0x6180, 8).unwrap();
    if irouter != 0x02 {
        uart_puts(b"[GICD] FAILED: IROUTER readback\n");
        return;
    }
    uart_puts(b"[GICD] Test 5 PASSED\n\n");

    // Test 6: route_spi returns 0 for SGIs/PPIs
    uart_puts(b"[GICD] Test 6: route_spi boundary...\n");
    if gicd.route_spi(15) != 0 || gicd.route_spi(31) != 0 {
        uart_puts(b"[GICD] FAILED: SGI/PPI should route to 0\n");
        return;
    }
    uart_puts(b"[GICD] Test 6 PASSED\n\n");

    // Test 7: PIDR2 reports GICv3
    uart_puts(b"[GICD] Test 7: PIDR2...\n");
    let pidr2 = gicd.read(0xFFE8, 4).unwrap();
    if pidr2 != 0x30 {
        uart_puts(b"[GICD] FAILED: PIDR2 should be 0x30\n");
        return;
    }
    uart_puts(b"[GICD] Test 7 PASSED\n\n");

    // Test 8: IIDR reports ARM implementer
    uart_puts(b"[GICD] Test 8: IIDR...\n");
    let iidr = gicd.read(0x008, 4).unwrap();
    if iidr != 0x0000_043B {
        uart_puts(b"[GICD] FAILED: IIDR should be 0x43B\n");
        return;
    }
    uart_puts(b"[GICD] Test 8 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Virtual GICD Emulation Test PASSED (8 assertions)\n");
    uart_puts(b"========================================\n\n");
}
```

**Step 2: Wire into test harness** (same pattern as Task 1)

**Step 3: Build and verify**

Run: `make clean && make run`

**Step 4: Commit**

```
feat: add GICD emulation tests (8 assertions)
```

---

### Task 3: GICR Emulation Tests (`test_gicr.rs`)

Tests `VirtualGicr` per-vCPU state: TYPER (Aff0, Last bit), WAKER transitions, ISENABLER0/ICENABLER0 set/clear, PIDR2, SGI frame routing.

**Files:**
- Create: `tests/test_gicr.rs`
- Modify: `tests/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create the test file**

```rust
// tests/test_gicr.rs
//! Virtual GICR emulation tests
//!
//! Tests VirtualGicr per-vCPU state management. All accesses go through
//! the MmioDevice trait (read/write with offset from GICR base).

use hypervisor::devices::gic::VirtualGicr;
use hypervisor::devices::MmioDevice;
use hypervisor::uart_puts;

pub fn run_gicr_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Virtual GICR Emulation Test\n");
    uart_puts(b"========================================\n\n");

    let mut gicr = VirtualGicr::new(4); // 4 vCPUs

    // GICR layout: each vCPU gets 0x20000 bytes
    // vCPU 0: offset 0x00000, vCPU 1: offset 0x20000, etc.
    // RD frame: offset +0x00000, SGI frame: offset +0x10000

    // Test 1: GICR_TYPER for vCPU 0 — Aff0=0, Last=0
    uart_puts(b"[GICR] Test 1: TYPER vCPU 0...\n");
    let typer0 = gicr.read(0x0008, 8).unwrap(); // vCPU 0 RD frame, TYPER
    let aff0 = (typer0 >> 32) & 0xFF;
    let last = (typer0 >> 4) & 1;
    if aff0 != 0 || last != 0 {
        uart_puts(b"[GICR] FAILED: vCPU 0 Aff0 or Last wrong\n");
        return;
    }
    uart_puts(b"[GICR] Test 1 PASSED\n\n");

    // Test 2: GICR_TYPER for vCPU 3 — Aff0=3, Last=1
    uart_puts(b"[GICR] Test 2: TYPER vCPU 3 (last)...\n");
    let typer3 = gicr.read(0x60008, 8).unwrap(); // vCPU 3 = 3*0x20000 + 0x0008
    let aff0 = (typer3 >> 32) & 0xFF;
    let last = (typer3 >> 4) & 1;
    if aff0 != 3 || last != 1 {
        uart_puts(b"[GICR] FAILED: vCPU 3 Aff0=");
        hypervisor::uart_put_u64(aff0);
        uart_puts(b" Last=");
        hypervisor::uart_put_u64(last);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GICR] Test 2 PASSED\n\n");

    // Test 3: WAKER — reset state has ProcessorSleep=1, ChildrenAsleep=1
    uart_puts(b"[GICR] Test 3: WAKER reset state...\n");
    let waker = gicr.read(0x0014, 4).unwrap() as u32; // vCPU 0 WAKER
    if waker != 0x06 { // bits 1+2 set
        uart_puts(b"[GICR] FAILED: WAKER reset should be 0x06\n");
        return;
    }
    uart_puts(b"[GICR] Test 3 PASSED\n\n");

    // Test 4: Clear ProcessorSleep → both bits clear
    uart_puts(b"[GICR] Test 4: WAKER clear ProcessorSleep...\n");
    gicr.write(0x0014, 0x00, 4); // Clear ProcessorSleep
    let waker = gicr.read(0x0014, 4).unwrap() as u32;
    if waker != 0x00 {
        uart_puts(b"[GICR] FAILED: WAKER should be 0 after clear\n");
        return;
    }
    uart_puts(b"[GICR] Test 4 PASSED\n\n");

    // Test 5: ISENABLER0/ICENABLER0 on vCPU 1
    uart_puts(b"[GICR] Test 5: ISENABLER0/ICENABLER0 vCPU 1...\n");
    // vCPU 1 SGI frame ISENABLER0 = 0x20000 + 0x10000 + 0x0100 = 0x30100
    gicr.write(0x30100, 0xFF00, 4); // Enable INTIDs 8-15
    let enabled = gicr.read(0x30100, 4).unwrap();
    if enabled != 0xFF00 {
        uart_puts(b"[GICR] FAILED: ISENABLER0 readback\n");
        return;
    }
    // ICENABLER0 = 0x30180 — clear bits 8-11
    gicr.write(0x30180, 0x0F00, 4);
    let enabled = gicr.read(0x30100, 4).unwrap();
    if enabled != 0xF000 {
        uart_puts(b"[GICR] FAILED: ICENABLER0 clear\n");
        return;
    }
    uart_puts(b"[GICR] Test 5 PASSED\n\n");

    // Test 6: vCPU isolation — vCPU 0 state unaffected by vCPU 1 writes
    uart_puts(b"[GICR] Test 6: vCPU isolation...\n");
    // vCPU 0 SGI frame ISENABLER0 = 0x10100
    let vcpu0_enabled = gicr.read(0x10100, 4).unwrap();
    if vcpu0_enabled != 0 {
        uart_puts(b"[GICR] FAILED: vCPU 0 ISENABLER0 should be 0\n");
        return;
    }
    uart_puts(b"[GICR] Test 6 PASSED\n\n");

    // Test 7: PIDR2 reports GICv3
    uart_puts(b"[GICR] Test 7: PIDR2...\n");
    let pidr2 = gicr.read(0xFFE8, 4).unwrap(); // vCPU 0 RD frame PIDR2
    if pidr2 != 0x30 {
        uart_puts(b"[GICR] FAILED: PIDR2 should be 0x30\n");
        return;
    }
    uart_puts(b"[GICR] Test 7 PASSED\n\n");

    // Test 8: ICFGR0 is read-only (SGIs always edge-triggered)
    uart_puts(b"[GICR] Test 8: ICFGR0 read-only...\n");
    let icfgr0_before = gicr.read(0x1_0C00, 4).unwrap(); // vCPU 0 SGI frame
    gicr.write(0x1_0C00, 0x0, 4); // Try to clear
    let icfgr0_after = gicr.read(0x1_0C00, 4).unwrap();
    if icfgr0_before != icfgr0_after {
        uart_puts(b"[GICR] FAILED: ICFGR0 should be read-only\n");
        return;
    }
    uart_puts(b"[GICR] Test 8 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Virtual GICR Emulation Test PASSED (8 assertions)\n");
    uart_puts(b"========================================\n\n");
}
```

**Step 2: Wire into test harness**

**Step 3: Build and verify**

Run: `make clean && make run`

**Step 4: Commit**

```
feat: add GICR emulation tests (8 assertions)
```

---

### Task 4: Global State Tests (`test_global.rs`)

Tests `PendingCpuOn` request/take atomics and `UartRxRing` push/pop ring buffer — pure logic, no hardware.

**Files:**
- Create: `tests/test_global.rs`
- Modify: `tests/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create the test file**

```rust
// tests/test_global.rs
//! Global state tests
//!
//! Tests PendingCpuOn atomics and UartRxRing lock-free ring buffer.

use hypervisor::global::{PendingCpuOn, UartRxRing};
use hypervisor::uart_puts;

pub fn run_global_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Global State Test\n");
    uart_puts(b"========================================\n\n");

    // === PendingCpuOn tests ===

    // Test 1: Fresh PendingCpuOn — take returns None
    uart_puts(b"[GLOBAL] Test 1: PendingCpuOn empty...\n");
    let pending = PendingCpuOn::new();
    if pending.take().is_some() {
        uart_puts(b"[GLOBAL] FAILED: should be None\n");
        return;
    }
    uart_puts(b"[GLOBAL] Test 1 PASSED\n\n");

    // Test 2: Request then take
    uart_puts(b"[GLOBAL] Test 2: PendingCpuOn request+take...\n");
    pending.request(2, 0x4800_0000, 0xDEAD);
    match pending.take() {
        Some((target, entry, ctx)) => {
            if target != 2 || entry != 0x4800_0000 || ctx != 0xDEAD {
                uart_puts(b"[GLOBAL] FAILED: wrong values\n");
                return;
            }
        }
        None => {
            uart_puts(b"[GLOBAL] FAILED: should be Some\n");
            return;
        }
    }
    uart_puts(b"[GLOBAL] Test 2 PASSED\n\n");

    // Test 3: Second take returns None (consumed)
    uart_puts(b"[GLOBAL] Test 3: PendingCpuOn consumed...\n");
    if pending.take().is_some() {
        uart_puts(b"[GLOBAL] FAILED: should be None after take\n");
        return;
    }
    uart_puts(b"[GLOBAL] Test 3 PASSED\n\n");

    // === UartRxRing tests ===

    // Test 4: Empty ring — pop returns None
    uart_puts(b"[GLOBAL] Test 4: UartRxRing empty...\n");
    let ring = UartRxRing::new();
    if ring.pop().is_some() {
        uart_puts(b"[GLOBAL] FAILED: should be None\n");
        return;
    }
    uart_puts(b"[GLOBAL] Test 4 PASSED\n\n");

    // Test 5: Push and pop
    uart_puts(b"[GLOBAL] Test 5: UartRxRing push+pop...\n");
    ring.push(b'A');
    ring.push(b'B');
    ring.push(b'C');
    let a = ring.pop();
    let b = ring.pop();
    let c = ring.pop();
    let d = ring.pop();
    if a != Some(b'A') || b != Some(b'B') || c != Some(b'C') || d.is_some() {
        uart_puts(b"[GLOBAL] FAILED: push/pop mismatch\n");
        return;
    }
    uart_puts(b"[GLOBAL] Test 5 PASSED\n\n");

    // Test 6: Ring full — drops overflow
    uart_puts(b"[GLOBAL] Test 6: UartRxRing overflow...\n");
    let ring2 = UartRxRing::new();
    // Ring size is 64, fill it (capacity = 63 due to sentinel)
    for i in 0..63u8 {
        ring2.push(i);
    }
    ring2.push(0xFF); // This should be dropped (full)
    // Drain and verify last byte
    let mut last = 0u8;
    let mut count = 0u32;
    while let Some(ch) = ring2.pop() {
        last = ch;
        count += 1;
    }
    if count != 63 || last != 62 {
        uart_puts(b"[GLOBAL] FAILED: expected 63 items, last=62, got count=");
        hypervisor::uart_put_u64(count as u64);
        uart_puts(b" last=");
        hypervisor::uart_put_u64(last as u64);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GLOBAL] Test 6 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Global State Test PASSED (6 assertions)\n");
    uart_puts(b"========================================\n\n");
}
```

**Step 2: Wire into test harness**

**Step 3: Build and verify**

Run: `make clean && make run`

**Step 4: Commit**

```
feat: add global state tests — PendingCpuOn + UartRxRing (6 assertions)
```

---

### Task 5: DynamicIdentityMapper 4KB Page Tests

The existing `test_dynamic_pagetable.rs` only tests 2MB mapping. Add 4KB `unmap_4kb_page()` coverage.

**Files:**
- Modify: `tests/test_dynamic_pagetable.rs` (add 4KB tests)

**Step 1: Add 4KB tests to existing file**

Append after the existing "Test 4: Verify VTTBR" block, before the final PASSED message:

```rust
    // Test 5: Unmap a 4KB page from a 2MB block
    uart_puts(b"[DYN PT] Test 5: Unmap 4KB page...\n");
    // First map a fresh 2MB region, then unmap a single 4KB page within it
    let result = mapper.map_region(0x3000_0000, 0x20_0000, MemoryAttribute::Normal);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to map region for 4KB test\n");
        return;
    }
    let result = mapper.unmap_4kb_page(0x3000_1000); // Unmap second page
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to unmap 4KB page\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 5 PASSED\n\n");

    // Test 6: Unmap multiple 4KB pages in same 2MB block
    uart_puts(b"[DYN PT] Test 6: Unmap multiple 4KB pages...\n");
    let result = mapper.unmap_4kb_page(0x3000_2000);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to unmap second 4KB page\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 6 PASSED\n\n");
```

**Step 2: Build and verify**

Run: `make clean && make run`

**Step 3: Commit**

```
feat: add 4KB page unmap tests to dynamic pagetable suite
```

---

### Task 6: Clean Up Dead Tests

Replace the `test_guest_irq.rs` placeholder with a real test, and wire `test_guest_interrupt.rs` into the test harness.

**Files:**
- Modify: `tests/test_guest_irq.rs` (replace placeholder)
- Modify: `tests/mod.rs` (remove `#[allow(dead_code)]`, add re-exports)
- Modify: `src/main.rs` (wire both tests)

**Step 1: Replace test_guest_irq.rs with a real test**

The existing `test_guest_interrupt.rs` already creates a guest with IRQ injection. The `test_guest_irq.rs` placeholder should become a simpler test that verifies the PENDING_SGIS/PENDING_SPIS atomic bit operations without running a guest.

```rust
// tests/test_guest_irq.rs
//! Interrupt queueing tests
//!
//! Tests PENDING_SGIS and PENDING_SPIS atomic bitmask operations
//! used for cross-vCPU interrupt delivery.

use core::sync::atomic::Ordering;
use hypervisor::global::{PENDING_SGIS, PENDING_SPIS, MAX_VCPUS};
use hypervisor::uart_puts;

pub fn run_irq_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Interrupt Queue Test\n");
    uart_puts(b"========================================\n\n");

    // Reset state
    for i in 0..MAX_VCPUS {
        PENDING_SGIS[i].store(0, Ordering::Relaxed);
        PENDING_SPIS[i].store(0, Ordering::Relaxed);
    }

    // Test 1: Queue SGI 1 to vCPU 2
    uart_puts(b"[IRQ Q] Test 1: Queue SGI...\n");
    PENDING_SGIS[2].fetch_or(1 << 1, Ordering::Release);
    let pending = PENDING_SGIS[2].load(Ordering::Acquire);
    if pending != 0x02 {
        uart_puts(b"[IRQ Q] FAILED: SGI bit not set\n");
        return;
    }
    uart_puts(b"[IRQ Q] Test 1 PASSED\n\n");

    // Test 2: Queue multiple SGIs
    uart_puts(b"[IRQ Q] Test 2: Multiple SGIs...\n");
    PENDING_SGIS[2].fetch_or(1 << 0, Ordering::Release); // SGI 0
    PENDING_SGIS[2].fetch_or(1 << 7, Ordering::Release); // SGI 7
    let pending = PENDING_SGIS[2].load(Ordering::Acquire);
    if pending != 0x83 { // bits 0,1,7
        uart_puts(b"[IRQ Q] FAILED: expected 0x83\n");
        return;
    }
    uart_puts(b"[IRQ Q] Test 2 PASSED\n\n");

    // Test 3: Consume SGIs via swap
    uart_puts(b"[IRQ Q] Test 3: Consume SGIs...\n");
    let consumed = PENDING_SGIS[2].swap(0, Ordering::AcqRel);
    if consumed != 0x83 {
        uart_puts(b"[IRQ Q] FAILED: swap should return 0x83\n");
        return;
    }
    let after = PENDING_SGIS[2].load(Ordering::Acquire);
    if after != 0 {
        uart_puts(b"[IRQ Q] FAILED: should be 0 after swap\n");
        return;
    }
    uart_puts(b"[IRQ Q] Test 3 PASSED\n\n");

    // Test 4: Queue SPI — bit encoding (INTID 48 = bit 16)
    uart_puts(b"[IRQ Q] Test 4: Queue SPI...\n");
    let spi_bit = 48u32 - 32; // bit 16
    PENDING_SPIS[0].fetch_or(1 << spi_bit, Ordering::Release);
    let pending = PENDING_SPIS[0].load(Ordering::Acquire);
    if pending != (1 << 16) {
        uart_puts(b"[IRQ Q] FAILED: SPI bit not set\n");
        return;
    }
    uart_puts(b"[IRQ Q] Test 4 PASSED\n\n");

    // Test 5: vCPU isolation — vCPU 1 unaffected
    uart_puts(b"[IRQ Q] Test 5: vCPU isolation...\n");
    let vcpu1_sgis = PENDING_SGIS[1].load(Ordering::Acquire);
    let vcpu1_spis = PENDING_SPIS[1].load(Ordering::Acquire);
    if vcpu1_sgis != 0 || vcpu1_spis != 0 {
        uart_puts(b"[IRQ Q] FAILED: vCPU 1 should have no pending\n");
        return;
    }
    uart_puts(b"[IRQ Q] Test 5 PASSED\n\n");

    // Clean up
    for i in 0..MAX_VCPUS {
        PENDING_SGIS[i].store(0, Ordering::Relaxed);
        PENDING_SPIS[i].store(0, Ordering::Relaxed);
    }

    uart_puts(b"========================================\n");
    uart_puts(b"  Interrupt Queue Test PASSED (5 assertions)\n");
    uart_puts(b"========================================\n\n");
}
```

**Step 2: Wire test_guest_irq into harness**

In `tests/mod.rs`:
- Remove `#[allow(dead_code)]` from `pub mod test_guest_irq;`
- Add `pub use test_guest_irq::run_irq_test;`

In `src/main.rs`:
- Add `tests::run_irq_test();`

**Step 3: Wire test_guest_interrupt into harness**

In `tests/mod.rs`:
- Remove `#[allow(dead_code)]` from `pub mod test_guest_interrupt;`

In `src/main.rs`:
- Add `tests::run_guest_interrupt_test();`

**Step 4: Build and verify**

Run: `make clean && make run`

**Step 5: Commit**

```
feat: replace IRQ placeholder with interrupt queue tests, wire guest interrupt test
```

---

### Task 7: DeviceManager Routing Tests (`test_device_routing.rs`)

Tests DeviceManager registration, MMIO routing (hit/miss), UART accessor, and reset.

**Files:**
- Create: `tests/test_device_routing.rs`
- Modify: `tests/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create the test file**

```rust
// tests/test_device_routing.rs
//! DeviceManager routing tests
//!
//! Tests device registration, MMIO address routing, and accessor methods.

use hypervisor::devices::{DeviceManager, Device, MmioDevice};
use hypervisor::devices::gic::VirtualGicd;
use hypervisor::devices::pl011::VirtualUart;
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
    // Read UART Flag Register (offset 0x18) — should return TXFE bit (TX FIFO empty)
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
    if target != 0 { // Default IROUTER is 0 → vCPU 0
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
```

**Step 2: Wire into test harness**

**Step 3: Build and verify**

Run: `make clean && make run`

**Step 4: Commit**

```
feat: add DeviceManager routing tests (6 assertions)
```

---

### Task 8: Final Wiring + Update Assertion Count

Update CLAUDE.md and DEVELOPMENT_PLAN.md with new test count.

**Files:**
- Modify: `CLAUDE.md` (update test count in Tests section)
- Modify: `DEVELOPMENT_PLAN.md` (update test count in overview + mark Option E progress)

**Step 1: Update counts**

After all tasks: 12 original + 9 (decode) + 8 (GICD) + 8 (GICR) + 6 (global) + 2 (4KB PT) + 5 (IRQ queue) + 6 (device routing) + 1 (guest interrupt wired) = **57+ assertions** across **19 test suites**.

Update the relevant lines in CLAUDE.md and DEVELOPMENT_PLAN.md.

**Step 2: Build final verification**

Run: `make clean && make run`
Verify all tests pass with new count.

**Step 3: Commit**

```
docs: update test counts (19 suites, ~57 assertions)
```

---

## Execution Order Summary

| Task | Test File | Assertions | Dependencies |
|------|-----------|------------|--------------|
| 1 | test_decode.rs | 9 | None (pure logic) |
| 2 | test_gicd.rs | 8 | None (pure logic, GICD write-through harmless) |
| 3 | test_gicr.rs | 8 | None (pure logic) |
| 4 | test_global.rs | 6 | None (pure logic) |
| 5 | test_dynamic_pagetable.rs | +2 | Heap initialized (already is) |
| 6 | test_guest_irq.rs | 5+1 | Globals reset between tests |
| 7 | test_device_routing.rs | 6 | Heap initialized |
| 8 | (docs update) | — | All tests passing |

**Total new assertions: ~45**
**Total test suites: 19 (was 12)**
**Total assertions: ~85 (was 40)**

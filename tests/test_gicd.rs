//! Virtual GICD emulation tests
//!
//! Tests VirtualGicd shadow state read/write semantics. Write-through to
//! physical GICD occurs but is harmless at EL2.

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
        uart_puts(b"[GICD] FAILED: CTLR should be 0x11, got 0x");
        hypervisor::uart_put_hex(ctlr);
        uart_puts(b"\n");
        return;
    }
    uart_puts(b"[GICD] Test 2 PASSED\n\n");

    // Test 3: TYPER reports correct CPUNumber
    uart_puts(b"[GICD] Test 3: TYPER CPUNumber...\n");
    let typer = gicd.read(0x004, 4).unwrap() as u32;
    let cpu_num = (typer >> 5) & 0x7;
    // SMP_CPUS is 4, so CPUNumber = 3
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
    // SPI 48 (INTID 48) -> IROUTER index = 16, offset = 0x6100 + 16*8 = 0x6180
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

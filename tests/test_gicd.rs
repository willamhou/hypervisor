//! Virtual GICD emulation tests
//!
//! Tests VirtualGicd shadow state read/write semantics. Write-through to
//! physical GICD occurs but is harmless at EL2.

use hypervisor::devices::gic::VirtualGicd;
use hypervisor::devices::MmioDevice;

pub fn run_gicd_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Virtual GICD Emulation Test\n");
    hypervisor::log_info!("========================================\n\n");

    let mut gicd = VirtualGicd::new();

    // Test 1: CTLR reads back with ARE_NS forced on
    hypervisor::log_info!("[GICD] Test 1: CTLR ARE_NS forced...\n");
    let ctlr = gicd.read(0x000, 4).unwrap();
    if ctlr & (1 << 4) == 0 {
        hypervisor::log_info!("[GICD] FAILED: ARE_NS not set\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 1 PASSED\n\n");

    // Test 2: CTLR write preserves ARE_NS
    hypervisor::log_info!("[GICD] Test 2: CTLR write preserves ARE_NS...\n");
    gicd.write(0x000, 0x01, 4); // EnableGrp1NS only
    let ctlr = gicd.read(0x000, 4).unwrap();
    if ctlr != 0x11 {
        // EnableGrp1NS | ARE_NS
        hypervisor::log_info!("[GICD] FAILED: CTLR should be 0x11, got {:#018x}\n", ctlr);
        return;
    }
    hypervisor::log_info!("[GICD] Test 2 PASSED\n\n");

    // Test 3: TYPER reports correct CPUNumber
    hypervisor::log_info!("[GICD] Test 3: TYPER CPUNumber...\n");
    let typer = gicd.read(0x004, 4).unwrap() as u32;
    let cpu_num = (typer >> 5) & 0x7;
    // SMP_CPUS is 4, so CPUNumber = 3
    if cpu_num != 3 {
        hypervisor::log_info!("[GICD] FAILED: CPUNumber should be 3, got {}\n", cpu_num);
        return;
    }
    hypervisor::log_info!("[GICD] Test 3 PASSED\n\n");

    // Test 4: ISENABLER/ICENABLER set/clear semantics
    hypervisor::log_info!("[GICD] Test 4: ISENABLER/ICENABLER...\n");
    // ISENABLER[1] at offset 0x104 covers INTIDs 32-63
    gicd.write(0x104, 0x0001_0002, 4); // Set bits 1 and 16 (INTID 33, 48)
    let enabled = gicd.read(0x104, 4).unwrap();
    if enabled != 0x0001_0002 {
        hypervisor::log_info!("[GICD] FAILED: ISENABLER readback\n");
        return;
    }
    // ICENABLER[1] at offset 0x184 — clear bit 1 (INTID 33)
    gicd.write(0x184, 0x0000_0002, 4);
    let enabled = gicd.read(0x104, 4).unwrap();
    if enabled != 0x0001_0000 {
        hypervisor::log_info!("[GICD] FAILED: ICENABLER clear\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 4 PASSED\n\n");

    // Test 5: IROUTER write and route_spi
    hypervisor::log_info!("[GICD] Test 5: IROUTER + route_spi...\n");
    // SPI 48 (INTID 48) -> IROUTER index = 16, offset = 0x6100 + 16*8 = 0x6180
    gicd.write(0x6180, 0x02, 8); // Route to Aff0=2 (vCPU 2)
    let target = gicd.route_spi(48);
    if target != 2 {
        hypervisor::log_info!("[GICD] FAILED: route_spi(48) should be 2\n");
        return;
    }
    // Read back IROUTER
    let irouter = gicd.read(0x6180, 8).unwrap();
    if irouter != 0x02 {
        hypervisor::log_info!("[GICD] FAILED: IROUTER readback\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 5 PASSED\n\n");

    // Test 6: route_spi returns 0 for SGIs/PPIs
    hypervisor::log_info!("[GICD] Test 6: route_spi boundary...\n");
    if gicd.route_spi(15) != 0 || gicd.route_spi(31) != 0 {
        hypervisor::log_info!("[GICD] FAILED: SGI/PPI should route to 0\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 6 PASSED\n\n");

    // Test 7: PIDR2 reports GICv3
    hypervisor::log_info!("[GICD] Test 7: PIDR2...\n");
    let pidr2 = gicd.read(0xFFE8, 4).unwrap();
    if pidr2 != 0x30 {
        hypervisor::log_info!("[GICD] FAILED: PIDR2 should be 0x30\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 7 PASSED\n\n");

    // Test 8: IIDR reports ARM implementer
    hypervisor::log_info!("[GICD] Test 8: IIDR...\n");
    let iidr = gicd.read(0x008, 4).unwrap();
    if iidr != 0x0000_043B {
        hypervisor::log_info!("[GICD] FAILED: IIDR should be 0x43B\n");
        return;
    }
    hypervisor::log_info!("[GICD] Test 8 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Virtual GICD Emulation Test PASSED (8 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}

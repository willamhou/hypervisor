//! Virtual GICR emulation tests
//!
//! Tests VirtualGicr per-vCPU state management. All accesses go through
//! the MmioDevice trait (read/write with offset from GICR base).

use hypervisor::devices::gic::VirtualGicr;
use hypervisor::devices::MmioDevice;

pub fn run_gicr_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Virtual GICR Emulation Test\n");
    hypervisor::log_info!("========================================\n\n");

    let mut gicr = VirtualGicr::new(4); // 4 vCPUs

    // GICR layout: each vCPU gets 0x20000 bytes (128KB)
    // vCPU 0: offset 0x00000, vCPU 1: offset 0x20000, etc.
    // RD frame: offset +0x00000, SGI frame: offset +0x10000

    // Test 1: GICR_TYPER for vCPU 0 — Aff0=0, Last=0
    hypervisor::log_info!("[GICR] Test 1: TYPER vCPU 0...\n");
    let typer0 = gicr.read(0x0008, 8).unwrap(); // vCPU 0 RD frame, TYPER
    let aff0 = (typer0 >> 32) & 0xFF;
    let last = (typer0 >> 4) & 1;
    if aff0 != 0 || last != 0 {
        hypervisor::log_info!("[GICR] FAILED: vCPU 0 Aff0 or Last wrong\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 1 PASSED\n\n");

    // Test 2: GICR_TYPER for vCPU 3 — Aff0=3, Last=1
    hypervisor::log_info!("[GICR] Test 2: TYPER vCPU 3 (last)...\n");
    let typer3 = gicr.read(0x60008, 8).unwrap(); // vCPU 3 = 3*0x20000 + 0x0008
    let aff0 = (typer3 >> 32) & 0xFF;
    let last = (typer3 >> 4) & 1;
    if aff0 != 3 || last != 1 {
        hypervisor::log_info!("[GICR] FAILED: vCPU 3 Aff0={} Last={}\n", aff0, last);
        return;
    }
    hypervisor::log_info!("[GICR] Test 2 PASSED\n\n");

    // Test 3: WAKER — reset state has ProcessorSleep=1, ChildrenAsleep=1
    hypervisor::log_info!("[GICR] Test 3: WAKER reset state...\n");
    let waker = gicr.read(0x0014, 4).unwrap() as u32; // vCPU 0 WAKER
    if waker != 0x06 {
        // bits 1+2 set
        hypervisor::log_info!("[GICR] FAILED: WAKER reset should be 0x06, got {:#018x}\n", waker as u64);
        return;
    }
    hypervisor::log_info!("[GICR] Test 3 PASSED\n\n");

    // Test 4: Clear ProcessorSleep → both bits clear
    hypervisor::log_info!("[GICR] Test 4: WAKER clear ProcessorSleep...\n");
    gicr.write(0x0014, 0x00, 4); // Clear ProcessorSleep
    let waker = gicr.read(0x0014, 4).unwrap() as u32;
    if waker != 0x00 {
        hypervisor::log_info!("[GICR] FAILED: WAKER should be 0 after clear\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 4 PASSED\n\n");

    // Test 5: ISENABLER0/ICENABLER0 on vCPU 1
    hypervisor::log_info!("[GICR] Test 5: ISENABLER0/ICENABLER0 vCPU 1...\n");
    // vCPU 1 SGI frame ISENABLER0 = 0x20000 + 0x10000 + 0x0100 = 0x30100
    gicr.write(0x30100, 0xFF00, 4); // Enable INTIDs 8-15
    let enabled = gicr.read(0x30100, 4).unwrap();
    if enabled != 0xFF00 {
        hypervisor::log_info!("[GICR] FAILED: ISENABLER0 readback\n");
        return;
    }
    // ICENABLER0 = 0x30180 — clear bits 8-11
    gicr.write(0x30180, 0x0F00, 4);
    let enabled = gicr.read(0x30100, 4).unwrap();
    if enabled != 0xF000 {
        hypervisor::log_info!("[GICR] FAILED: ICENABLER0 clear\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 5 PASSED\n\n");

    // Test 6: vCPU isolation — vCPU 0 state unaffected by vCPU 1 writes
    hypervisor::log_info!("[GICR] Test 6: vCPU isolation...\n");
    // vCPU 0 SGI frame ISENABLER0 = 0x10100
    let vcpu0_enabled = gicr.read(0x10100, 4).unwrap();
    if vcpu0_enabled != 0 {
        hypervisor::log_info!("[GICR] FAILED: vCPU 0 ISENABLER0 should be 0\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 6 PASSED\n\n");

    // Test 7: PIDR2 reports GICv3
    hypervisor::log_info!("[GICR] Test 7: PIDR2...\n");
    let pidr2 = gicr.read(0xFFE8, 4).unwrap(); // vCPU 0 RD frame PIDR2
    if pidr2 != 0x30 {
        hypervisor::log_info!("[GICR] FAILED: PIDR2 should be 0x30\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 7 PASSED\n\n");

    // Test 8: ICFGR0 is read-only (SGIs always edge-triggered)
    hypervisor::log_info!("[GICR] Test 8: ICFGR0 read-only...\n");
    // vCPU 0 SGI frame ICFGR0 = 0x10000 + 0x0C00 = 0x10C00
    let icfgr0_before = gicr.read(0x10C00, 4).unwrap();
    gicr.write(0x10C00, 0x0, 4); // Try to clear
    let icfgr0_after = gicr.read(0x10C00, 4).unwrap();
    if icfgr0_before != icfgr0_after {
        hypervisor::log_info!("[GICR] FAILED: ICFGR0 should be read-only\n");
        return;
    }
    hypervisor::log_info!("[GICR] Test 8 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Virtual GICR Emulation Test PASSED (8 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use hypervisor::arch::aarch64::hypervisor::exception;
use hypervisor::uart_puts;

// Include test module
mod tests {
    include!("../tests/mod.rs");
}

/// Simple function to write a string to UART using inline assembly
#[inline(never)]
fn uart_puts_local(s: &[u8]) {
    uart_puts(s);
}

/// Rust entry point called from boot.S
/// `dtb_addr` is the host DTB address passed by QEMU in x0, preserved by boot.S in x20.
#[no_mangle]
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    // Initialize log module first (all static, no deps)
    hypervisor::log::init();

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  ARM64 Hypervisor - Sprint 2.4\n");
    hypervisor::log_info!("  API Documentation\n");
    hypervisor::log_info!("========================================\n\n");
    hypervisor::log_info!("[INIT] Initializing at EL2...\n");

    // Parse host DTB (before heap init — fdt crate does zero-copy parsing)
    hypervisor::log_info!("[INIT] Parsing host DTB at {:#018x}...\n", dtb_addr);
    hypervisor::dtb::init(dtb_addr);
    if hypervisor::dtb::is_initialized() {
        let pi = hypervisor::dtb::platform_info();
        hypervisor::log_info!("[INIT] DTB: cpus={} ram={:#018x}+{:#018x} uart={:#018x}\n",
            pi.num_cpus, pi.ram_base, pi.ram_size, pi.uart_base);
        hypervisor::log_info!("[INIT] DTB: gicd={:#018x} gicr={:#018x}\n",
            pi.gicd_base, pi.gicr_base);
    } else {
        hypervisor::log_info!("[INIT] DTB: parse failed, using defaults\n");
    }

    // Initialize exception handling
    hypervisor::log_info!("[INIT] Setting up exception vector table...\n");
    exception::init();
    hypervisor::log_info!("[INIT] Exception handling initialized\n");

    // Initialize GIC - try GICv3 first, fall back to GICv2
    hypervisor::arch::aarch64::peripherals::gicv3::init();

    // Initialize FF-A proxy (probe for real SPMC at EL3)
    #[cfg(feature = "linux_guest")]
    hypervisor::ffa::proxy::init();

    // Initialize timer
    hypervisor::log_info!("[INIT] Configuring timer...\n");
    hypervisor::arch::aarch64::peripherals::timer::init_hypervisor_timer();
    hypervisor::arch::aarch64::peripherals::timer::print_timer_info();

    // Check current exception level
    let current_el: u64;
    unsafe {
        core::arch::asm!(
            "mrs {el}, CurrentEL",
            el = out(reg) current_el,
            options(nostack, nomem),
        );
    }
    let el = (current_el >> 2) & 0x3;
    hypervisor::log_info!("[INIT] Current EL: EL{}\n", el);

    // Initialize heap
    hypervisor::log_info!("[INIT] Initializing heap...\n");
    unsafe {
        hypervisor::mm::heap::init();
    }
    hypervisor::log_info!("[INIT] Heap initialized (16MB at 0x41000000)\n\n");

    // Run the DTB parsing test (validates DTB init above)
    tests::run_dtb_test();

    // Run the allocator test
    tests::run_allocator_test();

    // Run the heap test
    tests::run_heap_test();

    // Run the dynamic page table test
    tests::run_dynamic_pt_test();

    // Run the multi-vCPU test
    tests::run_multi_vcpu_test();

    // Run the scheduler test
    tests::run_scheduler_test();

    // Run the VM scheduler integration test
    tests::run_vm_scheduler_test();

    // Run the MMIO device emulation test
    tests::run_mmio_test();

    // Run the GICv3 virtual interface test
    tests::run_gicv3_virt_test();

    // Run the complete interrupt injection test (with guest exception vector)
    tests::run_complete_interrupt_test();

    // Run the original guest test (hypercall)
    tests::run_guest_test();

    // Run the guest loader test
    tests::run_guest_loader_test();

    // Run the simple guest test
    tests::run_simple_guest_test();

    // Run the MMIO instruction decode test
    tests::run_decode_test();

    // Run the GICD emulation test
    tests::run_gicd_test();

    // Run the GICR emulation test
    tests::run_gicr_test();

    // Run the global state test
    tests::run_global_test();

    // Run the interrupt queue test
    tests::run_irq_test();

    // Run the device manager routing test
    tests::run_device_routing_test();

    // Run multi-VM tests
    tests::run_vm_state_isolation_test();
    tests::run_vmid_vttbr_test();
    tests::run_multi_vm_devices_test();
    tests::run_vm_activate_test();

    // Run the NetRxRing test
    tests::run_net_rx_ring_test();

    // Run the VSwitch test
    tests::run_vswitch_test();

    // Run the VirtioNet device test
    tests::run_virtio_net_test();

    // Run the page ownership test
    tests::run_page_ownership_test();

    // Run the PL031 RTC test
    tests::run_pl031_test();

    // Run the FF-A proxy test
    tests::run_ffa_test();

    // Run the SPMC handler dispatch test
    tests::run_spmc_handler_test();

    // Run the SP context state machine test
    tests::run_sp_context_test();

    // Run the Secure Stage-2 config test
    tests::run_secure_stage2_test();

    // Run the log module test
    tests::run_log_test();

    // Run the guest interrupt injection test (LAST before guest boot — blocks forever)
    // Skip when booting guests since it never returns.
    #[cfg(not(any(feature = "linux_guest", feature = "guest")))]
    tests::run_guest_interrupt_test();

    // Check if we should boot a Zephyr guest
    #[cfg(feature = "guest")]
    {
        use hypervisor::guest_loader::{run_guest, GuestConfig};

        hypervisor::log_info!("\n[INIT] Booting Zephyr guest VM...\n");

        let config = GuestConfig::zephyr_default();
        match run_guest(&config) {
            Ok(()) => {
                hypervisor::log_info!("[INIT] Guest exited normally\n");
            }
            Err(e) => {
                if e == "WFI" {
                    hypervisor::log_info!("[INIT] Guest completed and is idle\n");
                } else {
                    hypervisor::log_info!("[INIT] Guest error: {}\n", e);
                }
            }
        }
    }

    // Check if we should boot multiple VMs
    #[cfg(feature = "multi_vm")]
    {
        hypervisor::log_info!("\n[INIT] Booting multi-VM mode...\n");

        match hypervisor::guest_loader::run_multi_vm_guests() {
            Ok(()) => {
                hypervisor::log_info!("[INIT] Multi-VM exited normally\n");
            }
            Err(e) => {
                hypervisor::log_info!("[INIT] Multi-VM error: {}\n", e);
            }
        }
    }

    // Check if we should boot a Linux guest (single VM)
    #[cfg(all(feature = "linux_guest", not(feature = "multi_vm")))]
    {
        use hypervisor::guest_loader::{run_guest, GuestConfig};

        hypervisor::log_info!("\n[INIT] Booting Linux guest VM...\n");

        let config = GuestConfig::linux_default();
        match run_guest(&config) {
            Ok(()) => {
                hypervisor::log_info!("[INIT] Linux guest exited normally\n");
            }
            Err(e) => {
                hypervisor::log_info!("[INIT] Linux guest error: {}\n", e);
            }
        }
    }

    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("All Sprints Complete (2.1-2.4)\n");
    hypervisor::log_info!("========================================\n");

    // Halt - we'll implement proper VM execution later
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

/// S-EL2 SPMC entry point called from boot_sel2.S.
/// SPMD passes: x0=TOS_FW_CONFIG, x1=HW_CONFIG, x4=core_id
#[cfg(feature = "sel2")]
#[no_mangle]
pub extern "C" fn rust_main_sel2(
    manifest_addr: usize,
    hw_config_addr: usize,
    _core_id: usize,
) -> ! {
    // 1. Install exception vectors FIRST (before any memory access that could fault)
    exception::init();

    // Initialize log module (all static, no deps)
    hypervisor::log::init();

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  ARM64 SPMC - S-EL2\n");
    hypervisor::log_info!("========================================\n\n");

    let current_el: u64;
    unsafe {
        core::arch::asm!("mrs {}, CurrentEL", out(reg) current_el);
    }
    let el = (current_el >> 2) & 0x3;
    hypervisor::log_info!("[SPMC] Running at EL{}\n", el);

    // 3. Parse SPMC manifest (TOS_FW_CONFIG in x0)
    hypervisor::log_info!("[SPMC] Manifest at {:#018x}\n", manifest_addr);
    hypervisor::manifest::init(manifest_addr);
    let mi = hypervisor::manifest::manifest_info();
    hypervisor::log_info!("[SPMC] spmc_id={:#06x} version={}.{}\n",
        mi.spmc_id, mi.maj_ver, mi.min_ver);

    // 4. Parse hardware DTB (HW_CONFIG in x1)
    hypervisor::log_info!("[SPMC] HW config at {:#018x}\n", hw_config_addr);
    if hw_config_addr != 0 {
        hypervisor::dtb::init(hw_config_addr);
    } else {
        hypervisor::log_info!("[SPMC] No HW config DTB, using QEMU virt defaults\n");
    }

    // 4.5. Enable S-EL2 Stage-1 MMU (identity map with NS=1 for Non-Secure DRAM)
    // Must be before GIC init (Device mapping needed) and before any NWd RXTX access.
    hypervisor::sel2_mmu::init_sel2_stage1();
    hypervisor::log_info!("[SPMC] S-EL2 Stage-1 MMU enabled (NS DRAM mapped)\n");

    // 5. Initialize GIC
    hypervisor::arch::aarch64::peripherals::gicv3::init();
    hypervisor::log_info!("[SPMC] GIC initialized\n");

    // 5.1. Enable PPIs in GICR for physical delivery at S-EL2:
    //   - PPI 26 (CNTHP timer): preemption watchdog for SP execution
    //   - PPI 29 (Secure Physical Timer): virtual interrupt injection to SPs
    //
    // Both must be Secure Group 1 (IGROUPR0 bit=0, IGRPMODR0 bit=1).
    // NS Group 1 interrupts route to EL3 as FIQ in the Secure world,
    // so ICC_IAR1_EL1 at S-EL2 would never see them.
    {
        let gicr_sgi_base = hypervisor::dtb::gicr_sgi_base(0);
        unsafe {
            let ppi_mask: u32 = (1 << 26) | (1 << 29);

            // GICR_IGROUPR0: clear bits 26,29 → NOT Non-secure Group 1
            let igroupr0 = (gicr_sgi_base + 0x0080) as *mut u32;
            let val = core::ptr::read_volatile(igroupr0);
            core::ptr::write_volatile(igroupr0, val & !ppi_mask);

            // GICR_IGRPMODR0: set bits 26,29 → Secure Group 1
            let igrpmodr0 = (gicr_sgi_base + 0x0D00) as *mut u32;
            let val = core::ptr::read_volatile(igrpmodr0);
            core::ptr::write_volatile(igrpmodr0, val | ppi_mask);

            // GICR_ISENABLER0: enable both PPIs
            let isenabler0 = (gicr_sgi_base + 0x0100) as *mut u32;
            core::ptr::write_volatile(isenabler0, ppi_mask);

            core::arch::asm!("isb");
        }
    }

    // 5.5. Initialize secure heap (for page table allocation)
    hypervisor::log_info!("[SPMC] Initializing secure heap\n");
    unsafe {
        hypervisor::mm::heap::init_at(
            hypervisor::platform::SECURE_HEAP_START,
            hypervisor::platform::SECURE_HEAP_SIZE,
        );
    }

    // 5.5b. Enable S-EL1 access to physical timer/counter (CNTHCTL_EL2)
    unsafe {
        let mut cnthctl: u64;
        core::arch::asm!("mrs {}, cnthctl_el2", out(reg) cnthctl);
        cnthctl |= hypervisor::arch::aarch64::defs::CNTHCTL_EL1PCTEN
            | hypervisor::arch::aarch64::defs::CNTHCTL_EL1PCEN;
        core::arch::asm!("msr cnthctl_el2, {}", "isb", in(reg) cnthctl);
    }

    // 5.6. Build Secure Stage-2 for SP1
    hypervisor::log_info!("[SPMC] Building Secure Stage-2 for SP1\n");
    let mapper = hypervisor::secure_stage2::build_sp_stage2(
        hypervisor::platform::SP1_LOAD_ADDR,
        hypervisor::platform::SP1_MEM_SIZE,
    )
    .expect("Failed to build SP Stage-2");
    let s2_config = hypervisor::secure_stage2::SecureStage2Config::new(mapper.l0_addr());
    s2_config.install();

    // Enable Secure Stage-2 by setting HCR_EL2.VM
    unsafe {
        let hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr | hypervisor::arch::aarch64::defs::HCR_VM,
        );
    }

    // 5.7. Parse SP package header and create SP context
    // BL2 loads the raw SP package (SPKG) to SP1_LOAD_ADDR.  Layout:
    //   offset 0x00: magic "SPKG" (4B)
    //   offset 0x04: version      (4B LE)
    //   offset 0x08: pm_offset    (4B LE)  — manifest DTB
    //   offset 0x0C: pm_size      (4B LE)
    //   offset 0x10: img_offset   (4B LE)  — SP binary
    //   offset 0x14: img_size     (4B LE)
    let pkg_base = hypervisor::platform::SP1_LOAD_ADDR;
    let img_offset = unsafe {
        let ptr = pkg_base as *const u32;
        // Read img_offset at offset 0x10 (index 4)
        core::ptr::read_volatile(ptr.add(4)) as u64
    };
    let sp1_entry = pkg_base + img_offset;

    hypervisor::log_info!("[SPMC] SP1 package at {:#018x}, img_offset={:#018x}, entry={:#018x}\n",
        pkg_base, img_offset, sp1_entry);

    // SP1 UUID from sp_manifest.dts (byte-swapped by sp_mk_generator.py)
    let sp1_uuid: [u32; 4] = [0x12345678, 0x12345678, 0x12345678, 0x12345678];
    let mut sp1 = hypervisor::sp_context::SpContext::new(
        hypervisor::platform::SP1_PARTITION_ID,
        sp1_entry,
        hypervisor::platform::SP1_STACK_TOP,
        sp1_uuid,
    );
    sp1.set_vsttbr(s2_config.vsttbr);

    // Clear EL1 system registers left by TF-A (SCTLR_EL1.M=1 would fault
    // because TF-A's Stage-1 page tables don't map SP load address).
    // Also clear VBAR_EL1 so stale TF-A exception handlers don't trigger.
    unsafe {
        core::arch::asm!(
            "msr sctlr_el1, xzr",
            "msr tcr_el1, xzr",
            "msr ttbr0_el1, xzr",
            "msr vbar_el1, xzr",
            "isb",
        );
    }

    // ERET to SP1 — SP runs, prints hello, calls FFA_MSG_WAIT, traps back
    {
        use hypervisor::arch::aarch64::enter_guest;
        use hypervisor::arch::aarch64::regs::VcpuContext;
        let _exit = unsafe { enter_guest(sp1.vcpu_ctx_mut() as *mut VcpuContext) };
    }

    // Save SP1's EL1 sysregs after initial boot (VBAR_EL1 set by SP during startup)
    sp1.save_el1_state();

    // SP trapped back — verify it called FFA_MSG_WAIT
    let (x0, _, _, _, _, _, _, _) = sp1.get_args();
    if x0 == hypervisor::ffa::FFA_MSG_WAIT {
        hypervisor::log_info!("[SPMC] SP1 booted, now Idle (FFA_MSG_WAIT received)\n");
        sp1.transition_to(hypervisor::sp_context::SpState::Idle)
            .expect("SP1 transition failed");
    } else {
        hypervisor::log_warn!("[SPMC] WARNING: SP1 did not call FFA_MSG_WAIT, x0={:#018x}\n", x0);
    }

    // Store SP1 context globally for dispatch
    hypervisor::sp_context::register_sp(sp1);

    // 5.8. Boot SP2 (if present at SP2_LOAD_ADDR)
    {
        let sp2_pkg_base = hypervisor::platform::SP2_LOAD_ADDR;
        let sp2_magic = unsafe { core::ptr::read_volatile(sp2_pkg_base as *const u32) };
        if sp2_magic == 0x474B5053 {
            // "SPKG" magic found
            hypervisor::log_info!("[SPMC] SP2 package found at {:#018x}\n", sp2_pkg_base);

            // Build Secure Stage-2 for SP2
            let mapper2 = hypervisor::secure_stage2::build_sp_stage2(
                hypervisor::platform::SP2_LOAD_ADDR,
                hypervisor::platform::SP2_MEM_SIZE,
            )
            .expect("Failed to build SP2 Stage-2");
            let s2_config2 =
                hypervisor::secure_stage2::SecureStage2Config::new(mapper2.l0_addr());

            // Parse SPKG header for SP2
            let sp2_img_offset = unsafe {
                let ptr = sp2_pkg_base as *const u32;
                core::ptr::read_volatile(ptr.add(4)) as u64
            };
            let sp2_entry = sp2_pkg_base + sp2_img_offset;

            hypervisor::log_info!("[SPMC] SP2 entry={:#018x}\n", sp2_entry);

            // SP2 UUID from sp_manifest.dts (byte-swapped)
            let sp2_uuid: [u32; 4] = [0xAABBCCDD, 0xAABBCCDD, 0xAABBCCDD, 0xAABBCCDD];
            let mut sp2 = hypervisor::sp_context::SpContext::new(
                hypervisor::platform::SP2_PARTITION_ID,
                sp2_entry,
                hypervisor::platform::SP2_STACK_TOP,
                sp2_uuid,
            );
            sp2.set_vsttbr(s2_config2.vsttbr);

            // Register INTID 29 (Secure Physical Timer PPI) for vIRQ injection
            sp2.set_owned_intids([29, 0, 0, 0]);

            // Install SP2's Stage-2, clear EL1 state, ERET to SP2
            s2_config2.install();
            unsafe {
                core::arch::asm!(
                    "msr sctlr_el1, xzr",
                    "msr tcr_el1, xzr",
                    "msr ttbr0_el1, xzr",
                    "msr vbar_el1, xzr",
                    "isb",
                );
            }

            {
                use hypervisor::arch::aarch64::enter_guest;
                use hypervisor::arch::aarch64::regs::VcpuContext;
                let _exit = unsafe { enter_guest(sp2.vcpu_ctx_mut() as *mut VcpuContext) };
            }

            // Save SP2's EL1 sysregs after initial boot
            sp2.save_el1_state();

            // Verify SP2 called FFA_MSG_WAIT
            let (x0, _, _, _, _, _, _, _) = sp2.get_args();
            if x0 == hypervisor::ffa::FFA_MSG_WAIT {
                hypervisor::log_info!("[SPMC] SP2 booted, now Idle (FFA_MSG_WAIT received)\n");
                sp2.transition_to(hypervisor::sp_context::SpState::Idle)
                    .expect("SP2 transition failed");
            } else {
                hypervisor::log_warn!("[SPMC] WARNING: SP2 did not call FFA_MSG_WAIT, x0={:#018x}\n", x0);
            }

            hypervisor::sp_context::register_sp(sp2);
        } else {
            hypervisor::log_info!("[SPMC] No SP2 package found (single-SP mode)\n");
        }
    }

    // 5.9. Register secondary entry point with SPMD
    {
        extern "C" {
            fn secondary_entry_sel2();
        }
        let ep = secondary_entry_sel2 as *const () as usize as u64;
        hypervisor::log_info!("[SPMC] Registering secondary EP at {:#018x}\n", ep);

        let result = hypervisor::ffa::smc_forward::forward_smc(
            hypervisor::ffa::FFA_SECONDARY_EP_REGISTER,
            ep,
            0,
            0,
            0,
            0,
            0,
            0,
        );
        if result.x0 == hypervisor::ffa::FFA_SUCCESS_32
            || result.x0 == hypervisor::ffa::FFA_SUCCESS_64
        {
            hypervisor::log_info!("[SPMC] Secondary EP registered with SPMD\n");
        } else {
            hypervisor::log_warn!("[SPMC] WARNING: FFA_SECONDARY_EP_REGISTER failed, x0={:#018x}\n", result.x0);
        }
    }

    // 6. Signal SPMD: init complete, receive first NWd request
    // Note: NWd RXTX is managed by the SPMC event loop (SPMD forwards
    // FFA_RXTX_MAP from NWd to SPMC, not handled by SPMD itself).
    hypervisor::log_info!("[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT\n");
    let first_req = hypervisor::manifest::signal_spmc_ready();

    // 7. Enter SPMC event loop (does not return)
    hypervisor::spmc_handler::run_event_loop(first_req);
}

/// Secondary CPU S-EL2 entry point (warm-boot via SPMD).
///
/// Called from boot_sel2.S:secondary_entry_sel2 after SPMD routes
/// PSCI CPU_ON to our registered secondary EP.
///
/// SPMD is per-CPU: when NS-EL2 on this CPU issues an FF-A SMC, SPMD
/// context-switches THIS CPU into S-EL2 and resumes our code here.
/// We must run an event loop (like the primary) to handle those requests.
#[cfg(feature = "sel2")]
#[no_mangle]
pub extern "C" fn rust_main_sel2_secondary(
    _manifest_addr: usize,
    _hw_config_addr: usize,
    core_id: usize,
) -> ! {
    // 1. Install exception vectors (for fault diagnosis)
    exception::init();

    // 1b. Enable Secure Stage-2 (HCR_EL2.VM) — exception::init() doesn't set VM.
    // Primary CPU sets this during SP Stage-2 setup, but secondaries need it too
    // for dispatch_to_sp() to work (ERET to S-EL1 requires Stage-2 enabled).
    unsafe {
        let hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr | hypervisor::arch::aarch64::defs::HCR_VM,
        );
    }

    // 1c. Don't trap FP/SIMD/debug from S-EL1 to S-EL2.
    // TF-A warm-boot path may leave CPTR_EL2/MDCR_EL2 in a different state
    // than the primary CPU. Clear trap bits to match primary CPU behavior.
    unsafe {
        use hypervisor::arch::aarch64::defs::*;
        core::arch::asm!(
            "mrs x0, cptr_el2",
            "bic x0, x0, {cptr_tz}",
            "bic x0, x0, {cptr_tfp}",
            "bic x0, x0, {cptr_tsm}",
            "bic x0, x0, {cptr_tcpac}",
            "msr cptr_el2, x0",
            "msr mdcr_el2, xzr",
            "isb",
            cptr_tz = const CPTR_TZ,
            cptr_tfp = const CPTR_TFP,
            cptr_tsm = const CPTR_TSM,
            cptr_tcpac = const CPTR_TCPAC,
            out("x0") _,
            options(nostack),
        );
    }

    // 2. Install S-EL2 Stage-1 MMU (reuse primary's page tables)
    hypervisor::sel2_mmu::install_sel2_stage1_secondary();

    // 2b. Enable per-CPU GIC PPIs for this secondary CPU.
    // Primary CPU enables PPIs 26+29 on GICR0 during init, but each CPU
    // has its own GICR. Without this, CNTHP timer (PPI 26) never fires on
    // secondary CPUs, so dispatch_to_sp() hangs if the SP gets stuck.
    {
        let gicr_sgi_base = hypervisor::dtb::gicr_sgi_base(core_id);
        unsafe {
            let ppi_mask: u32 = (1 << 26) | (1 << 29);

            // GICR_IGROUPR0: clear bits → NOT Non-secure Group 1
            let igroupr0 = (gicr_sgi_base + 0x0080) as *mut u32;
            let val = core::ptr::read_volatile(igroupr0);
            core::ptr::write_volatile(igroupr0, val & !ppi_mask);

            // GICR_IGRPMODR0: set bits → Secure Group 1
            let igrpmodr0 = (gicr_sgi_base + 0x0D00) as *mut u32;
            let val = core::ptr::read_volatile(igrpmodr0);
            core::ptr::write_volatile(igrpmodr0, val | ppi_mask);

            // GICR_ISENABLER0: enable both PPIs
            let isenabler0 = (gicr_sgi_base + 0x0100) as *mut u32;
            core::ptr::write_volatile(isenabler0, ppi_mask);
        }
    }

    hypervisor::log_info!("[SPMC] Secondary CPU {} warm-boot, signaling FFA_MSG_WAIT\n", core_id);

    // 3. Signal SPMD: secondary init complete, receive first FF-A request.
    // FFA_MSG_WAIT tells SPMD this CPU's S-EL2 init is done.
    // When NS-EL2 later issues an FF-A SMC on this CPU, SPMD re-enters
    // S-EL2 here and the return value contains the FF-A request.
    let first_request = hypervisor::ffa::smc_forward::forward_smc8(
        hypervisor::ffa::FFA_MSG_WAIT,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    );

    // 4. Enter per-CPU event loop (same as primary CPU).
    // SPMD routes FF-A SMCs per-CPU — each physical CPU runs its own
    // event loop independently. Without this, NS-EL2 callers on this
    // CPU would never get a response and hang forever.
    hypervisor::spmc_handler::run_event_loop(first_request);
}

/// Secondary pCPU entry point (called from boot.S after PSCI CPU_ON start).
///
/// Sets up EL2 state (VBAR, HCR, Stage-2, GIC) then enters an idle loop
/// waiting for guest PSCI CPU_ON requests.
#[cfg(feature = "multi_pcpu")]
#[no_mangle]
pub extern "C" fn rust_main_secondary(cpu_id: usize) -> ! {
    use core::sync::atomic::Ordering;
    use hypervisor::arch::aarch64::defs::*;
    use hypervisor::arch::aarch64::hypervisor::exception;
    use hypervisor::arch::aarch64::peripherals::gicv3;

    // Early debug: write 'S' directly to UART via assembly
    // This verifies the CPU actually entered rust_main_secondary
    unsafe {
        core::arch::asm!(
            "mov x1, #0x09000000",
            "mov w2, #0x53",  // 'S'
            "strb w2, [x1]",
            out("x1") _,
            out("x2") _,
            options(nostack),
        );
    }

    hypervisor::log_info!("[SMP] pCPU {} started\n", cpu_id);

    // 1. Set VBAR_EL2 (same exception vectors as primary)
    exception::init();

    // 2. Set VTTBR_EL2 / VTCR_EL2 (shared Stage-2 from primary)
    let vttbr = hypervisor::global::SHARED_VTTBR.load(Ordering::Acquire);
    let vtcr = hypervisor::global::SHARED_VTCR.load(Ordering::Acquire);
    unsafe {
        core::arch::asm!(
            "msr vtcr_el2, {vtcr}",
            "msr vttbr_el2, {vttbr}",
            "isb",
            vtcr = in(reg) vtcr,
            vttbr = in(reg) vttbr,
            options(nostack, nomem),
        );
    }

    // 3. HCR_EL2 is set by exception::init(). Enable Stage-2 and clear TWI.
    unsafe {
        let mut hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        hcr |= HCR_VM; // Enable Stage-2
        hcr &= !HCR_TWI; // Don't trap WFI (multi-pCPU: WFI passthrough)
        core::arch::asm!("msr hcr_el2, {}", "isb", in(reg) hcr);
    }

    // 4. Configure CPTR_EL2 / MDCR_EL2 (don't trap FP/SIMD/debug)
    unsafe {
        core::arch::asm!(
            "mrs x0, cptr_el2",
            "bic x0, x0, {cptr_tz}",
            "bic x0, x0, {cptr_tfp}",
            "bic x0, x0, {cptr_tsm}",
            "bic x0, x0, {cptr_tcpac}",
            "msr cptr_el2, x0",
            "msr mdcr_el2, xzr",
            "isb",
            cptr_tz = const CPTR_TZ,
            cptr_tfp = const CPTR_TFP,
            cptr_tsm = const CPTR_TSM,
            cptr_tcpac = const CPTR_TCPAC,
            out("x0") _,
            options(nostack),
        );
    }

    // 5. Initialize per-pCPU GIC (system register interface + virtual interface)
    gicv3::init();

    // 6. Set PerCpuContext
    unsafe {
        (*hypervisor::percpu::this_cpu()).vcpu_id = cpu_id;
    }

    hypervisor::log_info!("[SMP] pCPU {} ready, waiting for CPU_ON\n", cpu_id);

    // 7. Idle loop: WFE until PSCI CPU_ON sets our request
    loop {
        unsafe { core::arch::asm!("wfe") };
        if let Some((entry, ctx)) = hypervisor::global::PENDING_CPU_ON_PER_VCPU[cpu_id].take() {
            hypervisor::log_info!("[SMP] pCPU {} got CPU_ON, entering guest\n", cpu_id);
            secondary_enter_guest(cpu_id, entry, ctx);
        }
    }
}

/// Set up vCPU and enter guest loop for a secondary pCPU.
/// Returns if the vCPU terminates (CPU_OFF/SYSTEM_OFF/SYSTEM_RESET),
/// allowing the pCPU to return to the idle loop for potential reuse.
#[cfg(feature = "multi_pcpu")]
fn secondary_enter_guest(cpu_id: usize, entry: u64, ctx_id: u64) {
    use core::sync::atomic::Ordering;
    use hypervisor::arch::aarch64::defs::*;
    use hypervisor::platform;
    use hypervisor::vcpu::Vcpu;

    // Wake this CPU's GICR
    if cpu_id < platform::num_cpus() {
        let rd_base = hypervisor::dtb::gicr_rd_base(cpu_id);
        let waker_addr = (rd_base + platform::GICR_WAKER_OFF) as *mut u32;
        unsafe {
            let mut waker = core::ptr::read_volatile(waker_addr);
            waker &= !(1 << 1); // Clear ProcessorSleep
            core::ptr::write_volatile(waker_addr, waker);
            loop {
                let w = core::ptr::read_volatile(waker_addr);
                if w & (1 << 2) == 0 {
                    break;
                }
            }
        }
    }

    // Create vCPU
    let mut vcpu = Vcpu::new(cpu_id, entry, 0);
    vcpu.context_mut().gp_regs.x0 = ctx_id;
    vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;
    vcpu.arch_state_mut().sctlr_el1 = 0x30D0_0800;
    vcpu.arch_state_mut().cpacr_el1 = 3 << 20;
    vcpu.arch_state_mut().init_for_vcpu(cpu_id);

    // Mark vCPU online (current_vcpu_id() uses MPIDR in multi_pcpu mode)
    hypervisor::global::vm_state(0)
        .vcpu_online_mask
        .fetch_or(1 << cpu_id, Ordering::Release);

    // Reset exception counters for this pCPU
    hypervisor::arch::aarch64::hypervisor::exception::reset_exception_counters();

    hypervisor::log_info!("[SMP] vCPU {} entering guest at {:#018x}\n", cpu_id, entry);

    // Run loop: inject pending, enter guest, handle exit.
    // Uses shared inject_pending_sgis/spis helpers (with re-queue on LR full).
    loop {
        // Ensure PPI 27 (virtual timer) stays enabled at the physical GICR
        hypervisor::vm::ensure_vtimer_enabled(cpu_id);

        // Inject pending SGIs and SPIs (shared with run_vcpu)
        hypervisor::vm::inject_pending_sgis(&mut vcpu);
        hypervisor::vm::inject_pending_spis(&mut vcpu);

        // Enter guest
        match vcpu.run() {
            Ok(()) => {
                // Check for terminal PSCI exits (CPU_OFF, SYSTEM_OFF, SYSTEM_RESET)
                if hypervisor::global::vm_state(0).terminal_exit[cpu_id]
                    .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    hypervisor::log_info!("[SMP] vCPU {} terminal exit\n", cpu_id);
                    hypervisor::global::vm_state(0)
                        .vcpu_online_mask
                        .fetch_and(!(1 << cpu_id), Ordering::Release);
                    // Return to idle loop — pCPU can be reused for future CPU_ON
                    break;
                }
                // Normal exit — loop back, re-enter guest
            }
            Err("WFI") => {
                // WFI: execute real WFI — pCPU idles until next interrupt
                unsafe { core::arch::asm!("wfi") };
            }
            Err(_) => {
                // Other exit — loop back
            }
        }
    }
    // Returns to idle loop in rust_main_secondary for potential CPU_ON reuse
}

/// Panic handler - required for no_std
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    uart_puts_local(b"\n!!! PANIC !!!\n");
    if let Some(location) = info.location() {
        uart_puts_local(b"  at ");
        uart_puts_local(location.file().as_bytes());
        uart_puts_local(b":");
        print_u32(location.line());
        uart_puts_local(b"\n");
    }
    if let Some(msg) = info.message().as_str() {
        uart_puts_local(b"  ");
        uart_puts_local(msg.as_bytes());
        uart_puts_local(b"\n");
    }

    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

/// Print a u32 value in decimal
fn print_u32(mut val: u32) {
    if val == 0 {
        uart_puts_local(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        uart_puts_local(&[buf[i]]);
    }
}

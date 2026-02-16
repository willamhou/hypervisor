//! Guest Loader Module
//!
//! This module provides configuration and boot logic for loading
//! real ELF binaries as guests.

use crate::vm::Vm;
use crate::uart_puts;
use crate::uart_put_hex;
use crate::platform;
use crate::arch::aarch64::defs::*;

/// Guest type for different kernel formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestType {
    /// Zephyr RTOS (ELF or raw binary)
    Zephyr,
    /// Linux kernel (ARM64 Image format)
    Linux,
}

/// Guest configuration
///
/// Defines memory layout and entry point for a guest VM.
pub struct GuestConfig {
    /// Guest type
    pub guest_type: GuestType,
    /// Guest code load address (where QEMU loads the kernel)
    pub load_addr: u64,
    /// Guest memory size in bytes
    pub mem_size: u64,
    /// Entry point address (usually equals load_addr)
    pub entry_point: u64,
    /// DTB (device tree blob) address for Linux
    pub dtb_addr: u64,
}

impl GuestConfig {
    /// Default configuration for Zephyr RTOS on qemu_cortex_a53
    pub fn zephyr_default() -> Self {
        let load_addr = platform::GUEST_LOAD_ADDR;

        // Read entry point from ELF header
        let entry_point = unsafe {
            let elf_header = load_addr as *const u8;
            let magic = core::slice::from_raw_parts(elf_header, 4);

            // Debug: print first 8 bytes at load address
            uart_puts(b"[GUEST] First 8 bytes at load addr: ");
            for i in 0..8 {
                let byte = *elf_header.add(i);
                let hex_chars = b"0123456789abcdef";
                uart_puts(&[hex_chars[(byte >> 4) as usize], hex_chars[(byte & 0xf) as usize], b' ']);
            }
            uart_puts(b"\n");

            if magic == [0x7F, b'E', b'L', b'F'] {
                // Valid ELF, read e_entry at offset 0x18
                let e_entry_ptr = (load_addr + 0x18) as *const u64;
                let entry = core::ptr::read_volatile(e_entry_ptr);
                uart_puts(b"[GUEST] ELF detected, e_entry = 0x");
                uart_put_hex(entry);
                uart_puts(b"\n");
                entry
            } else {
                // Not an ELF - QEMU loaded raw segments
                uart_puts(b"[GUEST] No ELF magic - checking for branch instruction\n");

                let first_instr = core::ptr::read_volatile(load_addr as *const u32);
                uart_puts(b"[GUEST] First instruction: 0x");
                uart_put_hex(first_instr as u64);
                uart_puts(b"\n");

                if (first_instr >> 26) == 0b000101 {
                    // B imm26 - unconditional branch
                    let imm26 = first_instr & 0x03FF_FFFF;
                    let offset = if imm26 & 0x0200_0000 != 0 {
                        ((imm26 | 0xFC00_0000) as i32) * 4
                    } else {
                        (imm26 as i32) * 4
                    };
                    let target = (load_addr as i64 + offset as i64) as u64;
                    uart_puts(b"[GUEST] Branch to offset ");
                    uart_put_hex(offset as u64);
                    uart_puts(b", target = 0x");
                    uart_put_hex(target);
                    uart_puts(b"\n");
                    target
                } else {
                    uart_puts(b"[GUEST] Using load address as entry\n");
                    load_addr
                }
            }
        };

        Self {
            guest_type: GuestType::Zephyr,
            load_addr,
            mem_size: platform::ZEPHYR_MEM_SIZE,
            entry_point,
            dtb_addr: 0, // Zephyr doesn't need DTB
        }
    }

    /// Default configuration for Linux kernel on QEMU virt
    pub fn linux_default() -> Self {
        let dtb_addr = platform::LINUX_DTB_ADDR;
        let mem_start = platform::GUEST_RAM_BASE;
        let kernel_addr = platform::GUEST_LOAD_ADDR;

        // Parse ARM64 Image header to find entry point
        let entry_point = unsafe {
            let header = kernel_addr as *const u8;

            // Debug: print header
            uart_puts(b"[LINUX] First 64 bytes of Image header:\n");
            for row in 0..4 {
                uart_puts(b"  ");
                for col in 0..16 {
                    let byte = *header.add(row * 16 + col);
                    let hex_chars = b"0123456789abcdef";
                    uart_puts(&[hex_chars[(byte >> 4) as usize], hex_chars[(byte & 0xf) as usize], b' ']);
                }
                uart_puts(b"\n");
            }

            // Check for ARM64 magic at offset 0x38
            let magic = core::ptr::read_volatile((kernel_addr + 0x38) as *const u32);
            if magic == 0x644d5241 { // "ARMd" little-endian
                uart_puts(b"[LINUX] ARM64 Image format detected\n");

                let text_offset = core::ptr::read_volatile((kernel_addr + 0x08) as *const u64);
                uart_puts(b"[LINUX] text_offset = 0x");
                uart_put_hex(text_offset);
                uart_puts(b"\n");

                if text_offset != 0 && text_offset < 0x100000 {
                    kernel_addr + text_offset
                } else {
                    kernel_addr
                }
            } else {
                uart_puts(b"[LINUX] WARNING: No ARM64 magic, using kernel address\n");
                kernel_addr
            }
        };

        uart_puts(b"[LINUX] Entry point: 0x");
        uart_put_hex(entry_point);
        uart_puts(b"\n");

        // Stage-2 mapping must cover from GUEST_RAM_BASE through the end of
        // the DTB-declared memory region (GUEST_LOAD_ADDR + LINUX_MEM_SIZE).
        // The DTB says memory starts at GUEST_LOAD_ADDR (0x48000000), but the
        // Stage-2 mapping starts from GUEST_RAM_BASE (0x40000000) to also cover
        // the DTB itself (at 0x47000000).
        let stage2_size = (kernel_addr - mem_start) + platform::LINUX_MEM_SIZE;

        Self {
            guest_type: GuestType::Linux,
            load_addr: mem_start,
            mem_size: stage2_size,
            entry_point,
            dtb_addr,
        }
    }

    /// Configuration for Linux VM 1 (multi-VM mode)
    ///
    /// Uses a separate memory region from VM 0 so both can run simultaneously.
    #[cfg(feature = "multi_vm")]
    pub fn linux_vm1() -> Self {
        let dtb_addr = platform::VM1_LINUX_DTB_ADDR;
        let mem_start = platform::VM1_GUEST_LOAD_ADDR - 0x0800_0000; // 0x60000000
        let kernel_addr = platform::VM1_GUEST_LOAD_ADDR;

        // Parse ARM64 Image header to find entry point
        let entry_point = unsafe {
            let magic = core::ptr::read_volatile((kernel_addr + 0x38) as *const u32);
            if magic == 0x644d5241 {
                let text_offset = core::ptr::read_volatile((kernel_addr + 0x08) as *const u64);
                if text_offset != 0 && text_offset < 0x100000 {
                    kernel_addr + text_offset
                } else {
                    kernel_addr
                }
            } else {
                kernel_addr
            }
        };

        // Stage-2 size: from mem_start through kernel + VM1 mem size
        let stage2_size = (kernel_addr - mem_start) + platform::VM1_LINUX_MEM_SIZE;

        Self {
            guest_type: GuestType::Linux,
            load_addr: mem_start,
            mem_size: stage2_size,
            entry_point,
            dtb_addr,
        }
    }
}

/// Boot a guest VM with the given configuration
pub fn run_guest(config: &GuestConfig) -> Result<(), &'static str> {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Guest VM Boot\n");
    uart_puts(b"========================================\n");

    uart_puts(b"[GUEST] Load address: 0x");
    uart_put_hex(config.load_addr);
    uart_puts(b"\n");

    uart_puts(b"[GUEST] Memory size: ");
    uart_put_hex(config.mem_size);
    uart_puts(b" bytes\n");

    uart_puts(b"[GUEST] Entry point: 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"\n\n");

    // Create VM
    uart_puts(b"[GUEST] Creating VM...\n");
    let mut vm = Vm::new(0);

    // Initialize memory mapping for guest
    uart_puts(b"[GUEST] Initializing Stage-2 memory...\n");
    vm.init_memory(config.load_addr, config.mem_size);

    // Create vCPU with guest entry point
    let guest_sp = config.load_addr + config.mem_size - platform::GUEST_STACK_RESERVE;

    uart_puts(b"[GUEST] Creating vCPU...\n");
    uart_puts(b"[GUEST] Stack pointer: 0x");
    uart_put_hex(guest_sp);
    uart_puts(b"\n");

    match vm.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = config.entry_point;
            vcpu.context_mut().sp = guest_sp;

            // Set up Linux boot protocol if this is a Linux guest
            if config.guest_type == GuestType::Linux {
                uart_puts(b"[GUEST] Setting up Linux boot protocol...\n");
                uart_puts(b"[GUEST] x0 (DTB) = 0x");
                uart_put_hex(config.dtb_addr);
                uart_puts(b"\n");

                // Linux ARM64 boot protocol:
                // x0 = physical address of device tree blob (DTB)
                // x1-x3 = 0 (reserved)
                vcpu.context_mut().gp_regs.x0 = config.dtb_addr;
                vcpu.context_mut().gp_regs.x1 = 0;
                vcpu.context_mut().gp_regs.x2 = 0;
                vcpu.context_mut().gp_regs.x3 = 0;
            }
        }
        Err(e) => {
            uart_puts(b"[GUEST] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return Err(e);
        }
    }

    // Initialize guest timer access
    uart_puts(b"[GUEST] Configuring virtual timer for guest...\n");
    crate::arch::aarch64::peripherals::timer::init_guest_timer();

    // Initialize EL1 system registers to clean state for Linux boot
    if config.guest_type == GuestType::Linux {
        uart_puts(b"[GUEST] Initializing EL1/EL2 registers...\n");

        // Set initial EL1 state in vCPU 0's arch_state (restored on guest entry)
        if let Some(vcpu) = vm.vcpu_mut(0) {
            let arch = vcpu.arch_state_mut();
            arch.sctlr_el1 = 0x30D0_0800; // RES1, MMU off, caches off
            arch.cpacr_el1 = 3 << 20;      // FP/SIMD access enabled
            // All other EL1 regs default to 0 (from VcpuArchState::new)
        }

        // Configure EL2 registers (not per-vCPU)
        unsafe {
            core::arch::asm!(
                // Ensure CPTR_EL2 does NOT trap FP/SIMD/SVE/SME to EL2
                "mrs x0, cptr_el2",
                "bic x0, x0, {cptr_tz}",
                "bic x0, x0, {cptr_tfp}",
                "bic x0, x0, {cptr_tsm}",
                "bic x0, x0, {cptr_tcpac}",
                "msr cptr_el2, x0",
                // Clear MDCR_EL2
                "msr mdcr_el2, xzr",
                // Set VPIDR_EL2 from real hardware value
                "mrs x0, midr_el1",
                "msr vpidr_el2, x0",
                // VMPIDR_EL2 is now set per-vCPU by VcpuArchState::restore()
                "isb",
                cptr_tz = const CPTR_TZ,
                cptr_tfp = const CPTR_TFP,
                cptr_tsm = const CPTR_TSM,
                cptr_tcpac = const CPTR_TCPAC,
                out("x0") _,
                options(nostack),
            );
        }
        uart_puts(b"[GUEST] EL1/EL2 registers initialized\n");
    }

    // For Linux guests: configure WFI/WFE trapping.
    // Single-pCPU: keep TWI set (trap WFI for cooperative scheduling), clear TWE.
    // Multi-pCPU: clear both TWI and TWE (WFI passthrough — real idle on pCPU).
    if config.guest_type == GuestType::Linux {
        unsafe {
            #[cfg(not(feature = "multi_pcpu"))]
            core::arch::asm!(
                "mrs x0, hcr_el2",
                "bic x0, x0, {twe}",
                "msr hcr_el2, x0",
                "isb",
                twe = const HCR_TWE,
                out("x0") _,
                options(nostack),
            );
            #[cfg(feature = "multi_pcpu")]
            core::arch::asm!(
                "mrs x0, hcr_el2",
                "bic x0, x0, {twe}",
                "bic x0, x0, {twi}",
                "msr hcr_el2, x0",
                "isb",
                twe = const HCR_TWE,
                twi = const HCR_TWI,
                out("x0") _,
                options(nostack),
            );
        }
    }

    // Attach virtio-blk device (backed by in-memory disk image loaded by QEMU)
    if config.guest_type == GuestType::Linux {
        crate::global::DEVICES[0].attach_virtio_blk(
            platform::VIRTIO_DISK_ADDR,
            platform::VIRTIO_DISK_SIZE,
        );
    }

    // Enable physical UART RX interrupt (INTID 33) so the hypervisor
    // can deliver keyboard input to the guest via VirtualUart.
    if config.guest_type == GuestType::Linux {
        enable_physical_uart_irq();
    }

    // Reset exception counters so Linux exceptions are clearly visible
    if config.guest_type == GuestType::Linux {
        crate::arch::aarch64::hypervisor::exception::reset_exception_counters();
    }

    // Enter guest
    uart_puts(b"[GUEST] Entering guest at 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"...\n");
    uart_puts(b"========================================\n\n");

    // Run VM - use SMP scheduling for Linux, single vCPU for others
    #[cfg(not(feature = "multi_pcpu"))]
    let result = if config.guest_type == GuestType::Linux {
        vm.run_smp()
    } else {
        vm.run()
    };
    #[cfg(feature = "multi_pcpu")]
    let result = if config.guest_type == GuestType::Linux {
        // Wake secondary pCPUs now that all setup is complete.
        // They will enter rust_main_secondary() and wait for PSCI CPU_ON.
        wake_secondary_pcpus();
        vm.run_vcpu(0)
    } else {
        vm.run()
    };

    // Debug: check UART state after guest exits
    uart_puts(b"\n[GUEST] Guest exited, checking UART state...\n");
    unsafe {
        let uart_base = platform::UART_BASE;
        let uartfr = core::ptr::read_volatile((uart_base + 0x18) as *const u32);
        uart_puts(b"[GUEST] UART FR: 0x");
        let fr_bytes = [
            b"0123456789abcdef"[((uartfr >> 12) & 0xF) as usize],
            b"0123456789abcdef"[((uartfr >> 8) & 0xF) as usize],
            b"0123456789abcdef"[((uartfr >> 4) & 0xF) as usize],
            b"0123456789abcdef"[(uartfr & 0xF) as usize],
        ];
        uart_puts(&fr_bytes);
        uart_puts(b"\n");

        uart_puts(b"[GUEST] Test output after guest: OK\n");
    }

    result
}

/// Enable physical UART RX interrupt (INTID 33 = SPI 1).
///
/// Configures:
/// 1. GICD: enable INTID 33, set priority, route to PE 0
/// 2. Physical PL011: enable RX interrupt in UARTIMSC
fn enable_physical_uart_irq() {
    use crate::uart_puts;
    const GICD_BASE: u64 = 0x0800_0000;
    const UART_BASE: u64 = 0x0900_0000;
    const INTID: u32 = 33; // SPI 1

    unsafe {
        // GICD_ISENABLER1: enable INTID 33 (bit 1 of word 1)
        let isenabler1 = (GICD_BASE + 0x104) as *mut u32;
        core::ptr::write_volatile(isenabler1, 1 << (INTID - 32));

        // GICD_IPRIORITYR8: set priority for INTID 33
        // INTID 33 is byte 1 of IPRIORITYR[8] (offset 0x420 + 1)
        let ipriorityr = (GICD_BASE + 0x421) as *mut u8;
        core::ptr::write_volatile(ipriorityr, 0xA0); // medium priority

        // GICD_IROUTER33: route to PE 0 (Aff0=0)
        let irouter = (GICD_BASE + 0x6100 + (INTID as u64 - 32) * 8) as *mut u64;
        core::ptr::write_volatile(irouter, 0); // Aff0=0 → PE 0

        // Enable RX interrupt in physical PL011 UARTIMSC (bit 4 = RXIM)
        let uartimsc = (UART_BASE + 0x038) as *mut u32;
        let current = core::ptr::read_volatile(uartimsc as *const u32);
        core::ptr::write_volatile(uartimsc, current | (1 << 4));
    }

    uart_puts(b"[GUEST] Physical UART RX interrupt enabled (INTID 33)\n");
}

/// Wake secondary pCPUs via real PSCI CPU_ON SMC calls to QEMU firmware.
///
/// QEMU virt machine keeps secondary CPUs powered off at boot.
/// We issue SMC PSCI_CPU_ON(target_mpidr, entry_point, context_id=0)
/// to QEMU's built-in PSCI firmware, which starts each CPU at
/// `secondary_entry` in boot.S (EL2, MMU off).
#[cfg(feature = "multi_pcpu")]
fn wake_secondary_pcpus() {
    use crate::uart_puts;
    use crate::uart_put_hex;
    use crate::platform::SMP_CPUS;

    // PSCI CPU_ON (64-bit): function_id=0xC4000003
    const PSCI_CPU_ON_64: u64 = 0xC400_0003;
    const PSCI_SUCCESS: u64 = 0;

    extern "C" {
        // secondary_entry symbol from boot.S — the address where
        // QEMU firmware will start secondary CPUs
        fn secondary_entry();
    }

    let entry_addr = secondary_entry as *const () as usize as u64;
    uart_puts(b"[SMP] Waking secondary pCPUs via PSCI CPU_ON...\n");
    uart_puts(b"[SMP] secondary_entry = 0x");
    uart_put_hex(entry_addr);
    uart_puts(b"\n");

    for cpu_id in 1..SMP_CPUS {
        let target_mpidr = cpu_id as u64; // Aff0 = cpu_id

        let ret: u64;
        unsafe {
            // SMC #0 with:
            //   x0 = PSCI_CPU_ON_64 (function ID)
            //   x1 = target CPU MPIDR
            //   x2 = entry point address
            //   x3 = context_id (unused, 0)
            // Returns result in x0
            core::arch::asm!(
                "mov x0, {func_id}",
                "mov x1, {target}",
                "mov x2, {entry}",
                "mov x3, #0",
                "smc #0",
                "mov {ret}, x0",
                func_id = in(reg) PSCI_CPU_ON_64,
                target = in(reg) target_mpidr,
                entry = in(reg) entry_addr,
                ret = out(reg) ret,
                out("x0") _,
                out("x1") _,
                out("x2") _,
                out("x3") _,
                options(nostack, nomem),
            );
        }

        uart_puts(b"[SMP] PSCI CPU_ON for pCPU ");
        uart_puts(&[b'0' + cpu_id as u8]);
        if ret == PSCI_SUCCESS {
            uart_puts(b" -> SUCCESS\n");
        } else {
            uart_puts(b" -> FAILED (0x");
            uart_put_hex(ret);
            uart_puts(b")\n");
        }
    }
    uart_puts(b"[SMP] All secondary pCPUs signaled\n");
}

/// Boot two Linux VMs time-sliced on a single pCPU.
///
/// VM 0: standard Linux config (0x48000000)
/// VM 1: separate memory region (0x68000000)
/// Both share the same physical CPU via round-robin scheduling.
#[cfg(feature = "multi_vm")]
pub fn run_multi_vm_guests() -> Result<(), &'static str> {
    use crate::vm::{Vm, run_multi_vm};
    use crate::arch::aarch64::defs::*;

    uart_puts(b"\n========================================\n");
    uart_puts(b"  Multi-VM Boot (2 Linux VMs)\n");
    uart_puts(b"========================================\n\n");

    // --- VM 0 setup ---
    let config0 = GuestConfig::linux_default();
    uart_puts(b"[MULTI-VM] VM 0: entry=0x");
    uart_put_hex(config0.entry_point);
    uart_puts(b" dtb=0x");
    uart_put_hex(config0.dtb_addr);
    uart_puts(b"\n");

    let mut vm0 = Vm::new(0);
    vm0.init_memory(config0.load_addr, config0.mem_size);

    let guest_sp0 = config0.load_addr + config0.mem_size - platform::GUEST_STACK_RESERVE;
    match vm0.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = config0.entry_point;
            vcpu.context_mut().sp = guest_sp0;
            vcpu.context_mut().gp_regs.x0 = config0.dtb_addr;
            vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;
        }
        Err(e) => return Err(e),
    }
    if let Some(vcpu) = vm0.vcpu_mut(0) {
        vcpu.arch_state_mut().sctlr_el1 = 0x30D0_0800;
        vcpu.arch_state_mut().cpacr_el1 = 3 << 20;
    }

    // Attach virtio-blk to VM 0
    crate::global::DEVICES[0].attach_virtio_blk(
        platform::VIRTIO_DISK_ADDR,
        platform::VIRTIO_DISK_SIZE,
    );

    // --- VM 1 setup ---
    let config1 = GuestConfig::linux_vm1();
    uart_puts(b"[MULTI-VM] VM 1: entry=0x");
    uart_put_hex(config1.entry_point);
    uart_puts(b" dtb=0x");
    uart_put_hex(config1.dtb_addr);
    uart_puts(b"\n");

    // Save VM 0's Stage-2 before VM 1 creates its own
    let vm0_vttbr = vm0.vttbr();

    let mut vm1 = Vm::new(1);
    vm1.init_memory(config1.load_addr, config1.mem_size);

    let guest_sp1 = config1.load_addr + config1.mem_size - platform::GUEST_STACK_RESERVE;
    match vm1.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = config1.entry_point;
            vcpu.context_mut().sp = guest_sp1;
            vcpu.context_mut().gp_regs.x0 = config1.dtb_addr;
            vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;
        }
        Err(e) => return Err(e),
    }
    if let Some(vcpu) = vm1.vcpu_mut(0) {
        vcpu.arch_state_mut().sctlr_el1 = 0x30D0_0800;
        vcpu.arch_state_mut().cpacr_el1 = 3 << 20;
    }

    // Attach virtio-blk to VM 1 (different disk image address)
    crate::global::DEVICES[1].attach_virtio_blk(
        platform::VM1_VIRTIO_DISK_ADDR,
        platform::VIRTIO_DISK_SIZE,
    );

    // Restore VM 0's Stage-2 as active (run_multi_vm will switch as needed)
    unsafe {
        core::arch::asm!(
            "msr vttbr_el2, {vttbr}",
            "isb",
            vttbr = in(reg) vm0_vttbr,
            options(nostack, nomem),
        );
    }

    // Configure EL2 registers (shared: CPTR, MDCR, VPIDR)
    unsafe {
        core::arch::asm!(
            "mrs x0, cptr_el2",
            "bic x0, x0, {cptr_tz}",
            "bic x0, x0, {cptr_tfp}",
            "bic x0, x0, {cptr_tsm}",
            "bic x0, x0, {cptr_tcpac}",
            "msr cptr_el2, x0",
            "msr mdcr_el2, xzr",
            "mrs x0, midr_el1",
            "msr vpidr_el2, x0",
            "isb",
            cptr_tz = const CPTR_TZ,
            cptr_tfp = const CPTR_TFP,
            cptr_tsm = const CPTR_TSM,
            cptr_tcpac = const CPTR_TCPAC,
            out("x0") _,
            options(nostack),
        );
    }

    // Configure WFI trapping: trap WFI for cooperative scheduling (single-pCPU)
    unsafe {
        core::arch::asm!(
            "mrs x0, hcr_el2",
            "bic x0, x0, {twe}",
            "msr hcr_el2, x0",
            "isb",
            twe = const HCR_TWE,
            out("x0") _,
            options(nostack),
        );
    }

    // Init guest timer
    crate::arch::aarch64::peripherals::timer::init_guest_timer();

    // Enable physical UART RX interrupt
    enable_physical_uart_irq();

    // Reset exception counters
    crate::arch::aarch64::hypervisor::exception::reset_exception_counters();

    uart_puts(b"[MULTI-VM] Starting round-robin scheduler...\n");
    uart_puts(b"========================================\n\n");

    // Run both VMs with the two-level scheduler
    let mut vms = [vm0, vm1];
    run_multi_vm(&mut vms);

    uart_puts(b"\n[MULTI-VM] All VMs finished\n");
    Ok(())
}

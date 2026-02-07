//! Guest Loader Module
//!
//! This module provides configuration and boot logic for loading
//! real ELF binaries as guests.

use crate::vm::Vm;
use crate::uart_puts;
use crate::uart_put_hex;

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
    ///
    /// - Load address: 0x4800_0000 (offset to avoid DTB at 0x40000000)
    /// - Memory size: 128MB
    /// - Entry point: Read from ELF header at runtime
    ///
    /// Note: Zephyr is built with hypervisor_guest.overlay to link at 0x48000000.
    pub fn zephyr_default() -> Self {
        let load_addr: u64 = 0x4800_0000;

        // Read entry point from ELF header
        // ELF64 header: e_entry is at offset 0x18 (24 bytes)
        let entry_point = unsafe {
            let elf_header = load_addr as *const u8;
            // Check ELF magic: 0x7F 'E' 'L' 'F'
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
                // Check if first instruction is a branch (B imm26)
                // B instruction encoding: 000101 | imm26
                // 0x14xxxxxx = unconditional branch
                uart_puts(b"[GUEST] No ELF magic - checking for branch instruction\n");

                let first_instr = core::ptr::read_volatile(load_addr as *const u32);
                uart_puts(b"[GUEST] First instruction: 0x");
                uart_put_hex(first_instr as u64);
                uart_puts(b"\n");

                if (first_instr >> 26) == 0b000101 {
                    // B imm26 - unconditional branch
                    // imm26 is signed, in units of 4 bytes
                    let imm26 = first_instr & 0x03FF_FFFF;
                    // Sign extend from 26 bits
                    let offset = if imm26 & 0x0200_0000 != 0 {
                        // Negative offset
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
                    // Not a branch, use load address
                    uart_puts(b"[GUEST] Using load address as entry\n");
                    load_addr
                }
            }
        };

        Self {
            guest_type: GuestType::Zephyr,
            load_addr,
            mem_size: 128 * 1024 * 1024, // 128MB
            entry_point,
            dtb_addr: 0, // Zephyr doesn't need DTB
        }
    }

    /// Default configuration for Linux kernel on QEMU virt
    ///
    /// - Memory start: 0x4000_0000 (QEMU virt default RAM start)
    /// - DTB address: 0x4700_0000 (device tree blob, loaded by QEMU)
    /// - Kernel address: 0x4800_0000 (kernel Image)
    /// - Memory size: 512MB (from 0x40000000)
    /// - Entry point: Determined from ARM64 Image header
    ///
    /// Linux ARM64 Image header format:
    /// - Offset 0x00: MZ magic (for UEFI) or branch instruction
    /// - Offset 0x08: text_offset (kernel offset from load address)
    /// - Offset 0x38: "ARMd" magic
    pub fn linux_default() -> Self {
        // DTB is loaded by QEMU at 0x47000000 (before kernel)
        let dtb_addr: u64 = 0x4700_0000;
        // Memory mapping starts from QEMU's RAM base
        let mem_start: u64 = 0x4000_0000;
        // Kernel is loaded at 0x48000000
        let kernel_addr: u64 = 0x4800_0000;

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

                // Read text_offset at offset 0x08
                let text_offset = core::ptr::read_volatile((kernel_addr + 0x08) as *const u64);
                uart_puts(b"[LINUX] text_offset = 0x");
                uart_put_hex(text_offset);
                uart_puts(b"\n");

                // Entry point = kernel_addr + text_offset (if text_offset != 0)
                // For modern kernels, text_offset might be 0, meaning entry at kernel_addr
                if text_offset != 0 && text_offset < 0x100000 {
                    kernel_addr + text_offset
                } else {
                    // Use kernel address directly
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

        Self {
            guest_type: GuestType::Linux,
            load_addr: mem_start,  // Memory mapping starts from QEMU RAM base
            mem_size: 512 * 1024 * 1024, // 512MB (includes DTB + kernel + runtime)
            entry_point,
            dtb_addr,
        }
    }
}

/// Boot a guest VM with the given configuration
///
/// # Arguments
/// * `config` - Guest configuration (memory layout, entry point)
///
/// # Returns
/// * `Ok(())` - Guest exited normally
/// * `Err(msg)` - Error occurred
///
/// # Example
/// ```rust,ignore
/// let config = GuestConfig::zephyr_default();
/// run_guest(&config)?;
/// ```
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
    // Stack pointer at end of guest memory region
    let guest_sp = config.load_addr + config.mem_size - 0x1000; // Leave 4KB at top

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
                // x1 = 0 (reserved)
                // x2 = 0 (reserved)
                // x3 = 0 (reserved)
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

    // Initialize guest timer access (allows guest to use virtual timer)
    uart_puts(b"[GUEST] Configuring virtual timer for guest...\n");
    crate::arch::aarch64::peripherals::timer::init_guest_timer();

    // Initialize EL1 system registers to clean state for Linux boot
    if config.guest_type == GuestType::Linux {
        uart_puts(b"[GUEST] Initializing EL1 system registers...\n");
        unsafe {
            // Linux ARM64 boot protocol requires:
            // - MMU off (SCTLR_EL1.M = 0)
            // - D-cache off (SCTLR_EL1.C = 0)
            // - All EL1 system registers at known/clean state
            //
            // CRITICAL: SCTLR_EL1 has RES1 (reserved-as-1) bits that MUST be set,
            // or behavior is UNPREDICTABLE. RES1 bits for ARMv8-A:
            // Bit 29 (LSMAOE), Bit 28 (nTLSMD), Bit 23 (SPAN),
            // Bit 22 (EIS), Bit 20 (TSCXT), Bit 11 (EOS)
            // = 0x30D00800
            core::arch::asm!(
                // Set SCTLR_EL1 to RES1 value (MMU off, caches off, RES1 bits set)
                "mov x0, #0x0800",
                "movk x0, #0x30D0, lsl #16",
                "msr sctlr_el1, x0",
                // Zero out translation registers
                "msr tcr_el1, xzr",
                "msr ttbr0_el1, xzr",
                "msr ttbr1_el1, xzr",
                "msr mair_el1, xzr",
                // Zero out VBAR_EL1 (kernel will set its own)
                "msr vbar_el1, xzr",
                // Enable FP/SIMD access at EL1 (CPACR_EL1.FPEN = 0b11)
                // Without this, any FP instruction at EL1 traps to EL1
                "mov x0, #(3 << 20)",
                "msr cpacr_el1, x0",
                // Zero out other EL1 registers
                "msr contextidr_el1, xzr",
                "msr mdscr_el1, xzr",
                "msr tpidr_el1, xzr",
                "msr tpidrro_el0, xzr",
                "msr tpidr_el0, xzr",
                // Ensure CPTR_EL2 does NOT trap FP/SIMD/SVE/SME to EL2
                // TZ  (bit 8)  = 0: don't trap SVE to EL2
                // TFP (bit 10) = 0: don't trap FP to EL2
                // TSM (bit 12) = 0: don't trap SME to EL2
                // TCPAC (bit 20) = 0: don't trap CPACR_EL1 access
                "mrs x0, cptr_el2",
                "bic x0, x0, #(1 << 8)",
                "bic x0, x0, #(1 << 10)",
                "bic x0, x0, #(1 << 12)",
                "bic x0, x0, #(1 << 20)",
                "msr cptr_el2, x0",
                // Clear MDCR_EL2 to not trap debug/PMU register accesses
                // TDA (bit 8) = 0: don't trap debug registers
                // TPM (bit 11) = 0: don't trap PMU registers
                "msr mdcr_el2, xzr",
                // Set VPIDR_EL2 and VMPIDR_EL2 from real hardware values
                // When HCR_EL2.VM=1, guest reads of MIDR_EL1/MPIDR_EL1
                // return VPIDR_EL2/VMPIDR_EL2 instead. If these are 0,
                // the kernel misidentifies the CPU.
                "mrs x0, midr_el1",
                "msr vpidr_el2, x0",
                "mrs x0, mpidr_el1",
                "msr vmpidr_el2, x0",
                // Synchronize
                "isb",
                out("x0") _,
                options(nostack),
            );
        }
        uart_puts(b"[GUEST] EL1 registers initialized\n");
    }

    // For Linux guests: clear TWI/TWE in HCR_EL2 so WFI/WFE execute natively
    // instead of trapping to EL2 (which creates massive overhead).
    // Timer interrupts still route to EL2 via IMO bit.
    if config.guest_type == GuestType::Linux {
        unsafe {
            core::arch::asm!(
                "mrs x0, hcr_el2",
                "bic x0, x0, #(1 << 13)",  // Clear TWI: don't trap WFI
                "bic x0, x0, #(1 << 14)",  // Clear TWE: don't trap WFE
                "msr hcr_el2, x0",
                "isb",
                out("x0") _,
                options(nostack),
            );
        }
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

    // Run VM
    let result = vm.run();

    // Debug: check UART state after guest exits
    uart_puts(b"\n[GUEST] Guest exited, checking UART state...\n");
    unsafe {
        let uart_base = 0x09000000usize;
        // Read UART Flag Register (UARTFR) at offset 0x18
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

        // Write a test character to verify UART still works
        uart_puts(b"[GUEST] Test output after guest: OK\n");
    }

    result
}

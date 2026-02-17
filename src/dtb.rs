//! Host DTB (Device Tree Blob) runtime parsing
//!
//! Parses the host DTB passed by QEMU in x0 at boot to discover
//! platform hardware: UART, GIC, RAM, CPU count. Replaces hardcoded
//! constants from `platform.rs` with runtime-discovered values.
//!
//! The `fdt` crate does zero-copy parsing — no heap allocation needed.
//! This module must be initialized before heap init since DTB may
//! describe the memory layout.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Runtime-discovered platform information from host DTB.
///
/// Fields are initialized with QEMU virt defaults so everything works
/// even if DTB parsing fails (e.g., test mode, invalid DTB address).
pub struct PlatformInfo {
    /// UART (PL011) base address
    pub uart_base: u64,
    /// GIC distributor base address
    pub gicd_base: u64,
    /// GIC redistributor base address (first frame)
    pub gicr_base: u64,
    /// GIC redistributor region size (total)
    pub gicr_size: u64,
    /// Number of CPUs discovered from /cpus node
    pub num_cpus: usize,
    /// RAM base address
    pub ram_base: u64,
    /// RAM size in bytes
    pub ram_size: u64,
}

struct PlatformInfoCell {
    inner: UnsafeCell<PlatformInfo>,
    initialized: AtomicBool,
}

// Safety: Written once during single-threaded boot, read-only after.
unsafe impl Sync for PlatformInfoCell {}

/// Global platform info with QEMU virt defaults.
static PLATFORM_INFO: PlatformInfoCell = PlatformInfoCell {
    inner: UnsafeCell::new(PlatformInfo {
        uart_base: 0x0900_0000,
        gicd_base: 0x0800_0000,
        gicr_base: 0x080A_0000,
        gicr_size: 0,
        num_cpus: 4,
        ram_base: 0x4000_0000,
        ram_size: 0x4000_0000, // 1GB default
    }),
    initialized: AtomicBool::new(false),
};

/// Initialize platform info from host DTB. Called once from rust_main.
///
/// If the DTB address is invalid or parsing fails, the QEMU virt defaults
/// are retained — all existing behavior is preserved.
pub fn init(dtb_addr: usize) {
    if let Some(info) = parse_host_dtb(dtb_addr) {
        unsafe { *PLATFORM_INFO.inner.get() = info; }
        PLATFORM_INFO.initialized.store(true, Ordering::Release);
    }
}

/// Returns true if DTB was successfully parsed.
pub fn is_initialized() -> bool {
    PLATFORM_INFO.initialized.load(Ordering::Acquire)
}

/// Get platform info. Always available — returns defaults if DTB parsing failed.
pub fn platform_info() -> &'static PlatformInfo {
    unsafe { &*PLATFORM_INFO.inner.get() }
}

/// Compute GICR RD base for a given CPU ID.
/// GICv3 redistributor frames are 0x20000 (128KB) apart.
pub fn gicr_rd_base(cpu_id: usize) -> u64 {
    platform_info().gicr_base + (cpu_id as u64) * 0x20000
}

/// Compute GICR SGI frame base for a given CPU ID.
/// SGI frame is at RD base + 0x10000 (64KB offset).
pub fn gicr_sgi_base(cpu_id: usize) -> u64 {
    gicr_rd_base(cpu_id) + 0x10000
}

/// Validate that the given address plausibly points to a valid FDT.
fn validate_dtb_address(addr: usize) -> bool {
    if addr == 0 {
        return false;
    }
    // QEMU virt RAM range
    if addr < 0x4000_0000 || addr >= 0x8000_0000 {
        return false;
    }
    // Check FDT magic (0xD00DFEED big-endian)
    let magic = unsafe { core::ptr::read_volatile(addr as *const u32) };
    u32::from_be(magic) == 0xD00D_FEED
}

/// Parse the host DTB and extract platform information.
fn parse_host_dtb(dtb_addr: usize) -> Option<PlatformInfo> {
    if !validate_dtb_address(dtb_addr) {
        return None;
    }

    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8).ok()? };

    let mut info = PlatformInfo {
        uart_base: 0x0900_0000,
        gicd_base: 0x0800_0000,
        gicr_base: 0x080A_0000,
        gicr_size: 0,
        num_cpus: 4,
        ram_base: 0x4000_0000,
        ram_size: 0,
    };

    // 1. Parse /memory node
    let memory = fdt.memory();
    if let Some(region) = memory.regions().next() {
        info.ram_base = region.starting_address as u64;
        if let Some(size) = region.size {
            info.ram_size = size as u64;
        }
    }

    // 2. Parse UART (arm,pl011)
    if let Some(uart_node) = fdt.find_compatible(&["arm,pl011"]) {
        if let Some(mut regs) = uart_node.reg() {
            if let Some(reg) = regs.next() {
                info.uart_base = reg.starting_address as u64;
            }
        }
    }

    // 3. Parse GIC (arm,gic-v3)
    // reg = <GICD_base GICD_size GICR_base GICR_size>
    if let Some(gic_node) = fdt.find_compatible(&["arm,gic-v3"]) {
        if let Some(mut regs) = gic_node.reg() {
            if let Some(gicd_reg) = regs.next() {
                info.gicd_base = gicd_reg.starting_address as u64;
            }
            if let Some(gicr_reg) = regs.next() {
                info.gicr_base = gicr_reg.starting_address as u64;
                if let Some(size) = gicr_reg.size {
                    info.gicr_size = size as u64;
                }
            }
        }
    }

    // 4. Count CPUs
    let cpu_count = fdt.cpus().count();
    if cpu_count > 0 {
        info.num_cpus = cpu_count;
    }

    Some(info)
}

//! Platform/Board Constants (QEMU virt machine)
//!
//! All board-specific addresses and sizes live here so they can be
//! changed in one place when targeting a different platform.

use crate::arch::aarch64::defs::BLOCK_SIZE_2MB;

// ── UART (PL011) ─────────────────────────────────────────────────────
pub const UART_BASE: usize = 0x0900_0000;
pub const UART_SIZE: u64 = 0x1000;

// ── GIC ──────────────────────────────────────────────────────────────
pub const GICD_BASE: u64 = 0x0800_0000;
pub const GICD_SIZE: u64 = 0x1_0000;
pub const GICC_BASE: u64 = 0x0801_0000;
pub const GIC_REGION_BASE: u64 = 0x0800_0000;
/// 16MB covers GICD + GICR (8 x 2MB blocks: 0x0800_0000 - 0x0900_0000)
pub const GIC_REGION_SIZE: u64 = 8 * BLOCK_SIZE_2MB;

// ── Guest memory layout ──────────────────────────────────────────────
pub const GUEST_RAM_BASE: u64 = 0x4000_0000;
pub const GUEST_LOAD_ADDR: u64 = 0x4800_0000;
pub const LINUX_DTB_ADDR: u64 = 0x4700_0000;
pub const LINUX_MEM_SIZE: u64 = 512 * 1024 * 1024;
pub const ZEPHYR_MEM_SIZE: u64 = 128 * 1024 * 1024;
pub const GUEST_STACK_RESERVE: u64 = 0x1000;

// ── GICR redistributor frames ────────────────────────────────────────
/// GICR 0 RD base
pub const GICR0_RD_BASE: u64 = 0x080A_0000;
/// GICR 1 RD base
pub const GICR1_RD_BASE: u64 = 0x080C_0000;
/// GICR 0 SGI frame: RD base + 0x10000
pub const GICR0_SGI_BASE: u64 = 0x080B_0000;
/// GICR 1 SGI frame: RD base + 0x10000
pub const GICR1_SGI_BASE: u64 = 0x080D_0000;
/// GICR_WAKER offset from RD base
pub const GICR_WAKER_OFF: u64 = 0x014;
/// GICR_IGROUPR0 offset within SGI frame (interrupt group)
pub const GICR_IGROUPR0_OFF: u64 = 0x080;
/// GICR_ISENABLER0 offset within SGI frame (write-1-to-enable)
pub const GICR_ISENABLER0_OFF: u64 = 0x100;
/// GICR_ISPENDR0 offset within SGI frame
pub const GICR_ISPENDR0_OFF: u64 = 0x200;
/// GICR_ICPENDR0 offset within SGI frame
pub const GICR_ICPENDR0_OFF: u64 = 0x280;

// ── Heap ─────────────────────────────────────────────────────────────
pub const HEAP_START: u64 = 0x4100_0000;
pub const HEAP_SIZE: u64 = 0x100_0000; // 16MB

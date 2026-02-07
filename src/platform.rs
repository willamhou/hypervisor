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

// ── Heap ─────────────────────────────────────────────────────────────
pub const HEAP_START: u64 = 0x4100_0000;
pub const HEAP_SIZE: u64 = 0x100_0000; // 16MB

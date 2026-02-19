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
pub const LINUX_MEM_SIZE: u64 = 1024 * 1024 * 1024;
pub const ZEPHYR_MEM_SIZE: u64 = 128 * 1024 * 1024;
pub const GUEST_STACK_RESERVE: u64 = 0x1000;

// ── Virtio-blk disk image ───────────────────────────────────────────
/// Disk image load address (loaded by QEMU -device loader)
pub const VIRTIO_DISK_ADDR: u64 = 0x5800_0000;
/// Disk image size (2MB default — overridden if image is smaller/larger)
pub const VIRTIO_DISK_SIZE: u64 = 2 * 1024 * 1024;

// ── Virtio-MMIO slot layout ───────────────────────────────────────
/// Base address of the first virtio-mmio transport (QEMU virt convention)
pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
/// Stride between virtio-mmio transports
pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
/// First SPI INTID for virtio devices (SPI 16 = INTID 48)
pub const VIRTIO_SPI_BASE: u32 = 48;

/// Compute (base_addr, intid) for virtio-mmio slot N.
/// Slot 0: virtio-blk (0x0a000000, INTID 48)
/// Slot 1: virtio-net (0x0a000200, INTID 49)
pub const fn virtio_slot(n: usize) -> (u64, u32) {
    (
        VIRTIO_MMIO_BASE + (n as u64) * VIRTIO_MMIO_STRIDE,
        VIRTIO_SPI_BASE + n as u32,
    )
}

// ── SMP ──────────────────────────────────────────────────────────────
/// Maximum CPUs supported (compile-time capacity for array sizing)
pub const MAX_SMP_CPUS: usize = 8;
/// Default CPU count (used when DTB is not available)
pub const SMP_CPUS: usize = 4;
/// Runtime CPU count from DTB (falls back to SMP_CPUS default)
pub fn num_cpus() -> usize {
    crate::dtb::platform_info().num_cpus
}

// ── GICR redistributor offsets ───────────────────────────────────────
// Per-CPU GICR bases are now computed at runtime from DTB:
//   crate::dtb::gicr_rd_base(cpu_id)  → RD frame
//   crate::dtb::gicr_sgi_base(cpu_id) → SGI frame
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

// ── VM 1 memory layout (multi-VM mode) ──────────────────────────────
pub const VM1_GUEST_LOAD_ADDR: u64 = 0x6800_0000;
pub const VM1_LINUX_DTB_ADDR: u64 = 0x6700_0000;
pub const VM1_LINUX_MEM_SIZE: u64 = 256 * 1024 * 1024;
pub const VM1_VIRTIO_DISK_ADDR: u64 = 0x7800_0000;

// ── Heap ─────────────────────────────────────────────────────────────
pub const HEAP_START: u64 = 0x4100_0000;
pub const HEAP_SIZE: u64 = 0x100_0000; // 16MB

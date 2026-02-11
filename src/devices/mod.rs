//! Device Emulation Framework
//!
//! This module provides the infrastructure for emulating hardware devices
//! that guests interact with via MMIO (Memory-Mapped I/O).
//!
//! # Architecture
//!
//! ```text
//! Guest Memory Access (Stage-2 Fault)
//!              │
//!              ▼
//! ┌────────────────────────┐
//! │   Data Abort Handler   │  (ESR_EL2.EC = 0x24/0x25)
//! │   exception.rs         │
//! └────────────────────────┘
//!              │
//!              ▼
//! ┌────────────────────────┐
//! │    DeviceManager       │  Routes by address range
//! │    handle_mmio()       │
//! └────────────────────────┘
//!              │
//!    ┌─────────┼─────────┐
//!    ▼         ▼         ▼
//! ┌──────┐ ┌───────┐ ┌───────┐
//! │ UART │ │ GICD  │ │ (new) │
//! │PL011 │ │       │ │       │
//! └──────┘ └───────┘ └───────┘
//!  0x0900   0x0800
//!  _0000    _0000
//! ```
//!
//! # Trap-and-Emulate Flow
//!
//! 1. Guest accesses MMIO address (e.g., `str x0, [x1]` to UART)
//! 2. Stage-2 translation fails (no mapping or Device memory type)
//! 3. Hardware traps to EL2 with Data Abort
//! 4. Hypervisor decodes the faulting instruction
//! 5. `DeviceManager::handle_mmio()` routes to appropriate device
//! 6. Device emulates the read/write
//! 7. Hypervisor updates guest registers and resumes
//!
//! # Adding a New Device
//!
//! 1. Create a new module (e.g., `src/devices/rtc.rs`)
//! 2. Implement the [`MmioDevice`] trait
//! 3. Add the device to [`DeviceManager`]
//! 4. Map the MMIO region in Stage-2 tables (or leave unmapped for trap)
//!
//! ## Example: Custom Device
//!
//! ```rust,ignore
//! use hypervisor::devices::MmioDevice;
//!
//! /// Simple counter device
//! pub struct CounterDevice {
//!     base: u64,
//!     count: u32,
//! }
//!
//! impl CounterDevice {
//!     const REG_COUNT: u64 = 0x00;  // Read: current count
//!     const REG_INCR: u64 = 0x04;   // Write: increment by value
//!     const REG_RESET: u64 = 0x08;  // Write: reset to 0
//!
//!     pub fn new(base: u64) -> Self {
//!         Self { base, count: 0 }
//!     }
//! }
//!
//! impl MmioDevice for CounterDevice {
//!     fn read(&mut self, offset: u64, _size: u8) -> Option<u64> {
//!         match offset {
//!             Self::REG_COUNT => Some(self.count as u64),
//!             _ => None,
//!         }
//!     }
//!
//!     fn write(&mut self, offset: u64, value: u64, _size: u8) -> bool {
//!         match offset {
//!             Self::REG_INCR => {
//!                 self.count = self.count.wrapping_add(value as u32);
//!                 true
//!             }
//!             Self::REG_RESET => {
//!                 self.count = 0;
//!                 true
//!             }
//!             _ => false,
//!         }
//!     }
//!
//!     fn base_address(&self) -> u64 { self.base }
//!     fn size(&self) -> u64 { 0x1000 }  // 4KB region
//! }
//! ```
//!
//! # Supported Devices
//!
//! | Device | Base Address | Description |
//! |--------|--------------|-------------|
//! | PL011 UART | `0x0900_0000` | Serial console I/O |
//! | GIC Distributor | `0x0800_0000` | Interrupt controller |

pub mod pl011;
pub mod gic;
pub mod virtio;

/// Trait for MMIO-accessible devices
///
/// Implement this trait to create a new emulated device that can be
/// accessed by guests through memory-mapped I/O.
///
/// # Implementation Notes
///
/// - `read()` and `write()` receive offsets relative to `base_address()`
/// - `size` parameter indicates access width (1, 2, 4, or 8 bytes)
/// - Return `None`/`false` for invalid offsets to signal access failure
/// - Device state should be updated atomically where necessary
pub trait MmioDevice {
    /// Read from device register
    /// 
    /// # Arguments
    /// * `offset` - Offset from device base address
    /// * `size` - Access size in bytes (1, 2, 4, 8)
    /// 
    /// # Returns
    /// * `Some(value)` if read succeeded
    /// * `None` if offset is invalid or read not supported
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    
    /// Write to device register
    /// 
    /// # Arguments
    /// * `offset` - Offset from device base address
    /// * `value` - Value to write
    /// * `size` - Access size in bytes (1, 2, 4, 8)
    /// 
    /// # Returns
    /// * `true` if write succeeded
    /// * `false` if offset is invalid or write not supported
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    
    /// Get device base address
    fn base_address(&self) -> u64;
    
    /// Get device size in bytes
    fn size(&self) -> u64;
    
    /// Check if address is within this device's range
    fn contains(&self, addr: u64) -> bool {
        let base = self.base_address();
        addr >= base && addr < base + self.size()
    }
}

/// MMIO Device Manager
///
/// Routes MMIO accesses to the appropriate emulated device based on
/// the physical address. Acts as a dispatcher for all device emulation.
///
/// # Thread Safety
///
/// Currently not thread-safe. Access is serialized through the
/// exception handler which runs with interrupts disabled.
///
/// # Device Routing
///
/// Addresses are checked in order:
/// 1. UART (PL011) at `0x0900_0000`
/// 2. GIC Distributor at `0x0800_0000`
/// 3. Unknown addresses return 0 for reads
pub struct DeviceManager {
    uart: pl011::VirtualUart,
    gicd: gic::VirtualGicd,
    virtio_blk: Option<virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>>,
}

/// Virtio-blk MMIO base address (first QEMU virt virtio-mmio slot)
const VIRTIO_BLK_BASE: u64 = 0x0a00_0000;
/// Virtio-blk SPI: SPI 16 = INTID 48
const VIRTIO_BLK_INTID: u32 = 48;

impl DeviceManager {
    /// Create a new device manager (no virtio-blk by default)
    pub fn new() -> Self {
        Self {
            uart: pl011::VirtualUart::new(),
            gicd: gic::VirtualGicd::new(),
            virtio_blk: None,
        }
    }

    /// Attach a virtio-blk device backed by an in-memory disk image.
    pub fn attach_virtio_blk(&mut self, disk_base: u64, disk_size: u64) {
        let blk = virtio::blk::VirtioBlk::new(disk_base, disk_size);
        let transport = virtio::mmio::VirtioMmioTransport::new(
            VIRTIO_BLK_BASE, blk, VIRTIO_BLK_INTID,
        );
        self.virtio_blk = Some(transport);
    }
    
    /// Handle MMIO access
    /// 
    /// # Arguments
    /// * `addr` - Physical address being accessed
    /// * `value` - Value to write (for stores)
    /// * `size` - Access size in bytes
    /// * `is_write` - true for store, false for load
    /// 
    /// # Returns
    /// * `Some(value)` - For loads, the value read
    /// * `None` - If access failed or is a successful store
    pub fn handle_mmio(&mut self, addr: u64, value: u64, size: u8, is_write: bool) -> Option<u64> {
        // Try UART first (most common)
        if self.uart.contains(addr) {
            let offset = addr - self.uart.base_address();
            return if is_write {
                self.uart.write(offset, value, size);
                None
            } else {
                self.uart.read(offset, size)
            };
        }

        // Try GICD
        if self.gicd.contains(addr) {
            let offset = addr - self.gicd.base_address();
            return if is_write {
                self.gicd.write(offset, value, size);
                None
            } else {
                self.gicd.read(offset, size)
            };
        }

        // Try virtio-blk
        if let Some(ref mut vblk) = self.virtio_blk {
            if vblk.contains(addr) {
                let offset = addr - vblk.base_address();
                return if is_write {
                    vblk.write(offset, value, size);
                    None
                } else {
                    vblk.read(offset, size)
                };
            }
        }

        // Unknown device — return 0 for reads, ignore writes
        if is_write { None } else { Some(0) }
    }

    /// Look up SPI routing via GICD_IROUTER
    pub fn route_spi(&self, intid: u32) -> usize {
        self.gicd.route_spi(intid)
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

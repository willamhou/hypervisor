/// Device emulation framework
/// 
/// This module provides a framework for emulating MMIO devices using
/// the trap-and-emulate approach.

pub mod pl011;
pub mod gic;

/// MMIO device trait
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

/// MMIO device manager
pub struct DeviceManager {
    uart: pl011::VirtualUart,
    gicd: gic::VirtualGicd,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            uart: pl011::VirtualUart::new(),
            gicd: gic::VirtualGicd::new(),
        }
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
            if is_write {
                self.uart.write(offset, value, size);
                None
            } else {
                self.uart.read(offset, size)
            }
        }
        // Try GICD
        else if self.gicd.contains(addr) {
            let offset = addr - self.gicd.base_address();
            if is_write {
                self.gicd.write(offset, value, size);
                None
            } else {
                self.gicd.read(offset, size)
            }
        }
        // Unknown device
        else {
            crate::uart_puts(b"[MMIO] Unknown device access at 0x");
            crate::uart_put_hex(addr);
            crate::uart_puts(b"\n");
            
            // Return 0 for reads, ignore writes
            if is_write {
                None
            } else {
                Some(0)
            }
        }
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

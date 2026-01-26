/// Virtual UART (PL011) device
/// 
/// This emulates a minimal PL011 UART for guest output.

use super::MmioDevice;

/// UART base address (same as physical UART in QEMU virt machine)
const UART_BASE: u64 = 0x09000000;
const UART_SIZE: u64 = 0x1000;

/// UART register offsets
const UARTDR: u64 = 0x000;      // Data Register
const UARTFR: u64 = 0x018;      // Flag Register
const UARTCR: u64 = 0x030;      // Control Register

/// Flag Register bits
const FR_TXFF: u32 = 1 << 5;    // Transmit FIFO full
const FR_RXFE: u32 = 1 << 4;    // Receive FIFO empty

/// Virtual UART device
pub struct VirtualUart {
    /// Control register
    cr: u32,
    /// Flag register
    fr: u32,
}

impl VirtualUart {
    /// Create a new virtual UART
    pub fn new() -> Self {
        Self {
            cr: 0,
            // Initialize with TX not full, RX empty
            fr: FR_RXFE,
        }
    }
    
    /// Write a character to the real UART
    fn output_char(&self, ch: u8) {
        unsafe {
            let uart_base = UART_BASE as usize;
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) ch as u32,
                options(nostack),
            );
        }
    }
}

impl MmioDevice for VirtualUart {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        if size != 4 {
            // UART registers are 32-bit
            return Some(0);
        }
        
        match offset {
            UARTFR => {
                // Flag register: TX never full, RX always empty
                Some(self.fr as u64)
            }
            UARTCR => {
                // Control register
                Some(self.cr as u64)
            }
            UARTDR => {
                // Data register (read): always return 0 (no input)
                Some(0)
            }
            _ => {
                // Other registers: return 0
                Some(0)
            }
        }
    }
    
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        if size != 4 && offset != UARTDR {
            // Most UART registers are 32-bit, except DR can be byte
            return false;
        }
        
        match offset {
            UARTDR => {
                // Data register: output character
                let ch = (value & 0xFF) as u8;
                self.output_char(ch);
                true
            }
            UARTCR => {
                // Control register: just save it
                self.cr = (value & 0xFFFF_FFFF) as u32;
                true
            }
            UARTFR => {
                // Flag register is read-only
                false
            }
            _ => {
                // Other registers: ignore
                true
            }
        }
    }
    
    fn base_address(&self) -> u64 {
        UART_BASE
    }
    
    fn size(&self) -> u64 {
        UART_SIZE
    }
}

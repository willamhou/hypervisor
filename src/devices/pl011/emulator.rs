/// Virtual UART (PL011) device
///
/// This emulates a PL011 UART with both input and output support.
/// Input is read from the real UART (passthrough), output goes to real UART.

use crate::devices::MmioDevice;

/// UART base address (same as physical UART in QEMU virt machine)
const UART_BASE: u64 = 0x09000000;
const UART_SIZE: u64 = 0x1000;

/// UART register offsets
const UARTDR: u64 = 0x000;      // Data Register
const UARTFR: u64 = 0x018;      // Flag Register
const UARTIBRD: u64 = 0x024;    // Integer Baud Rate
const UARTFBRD: u64 = 0x028;    // Fractional Baud Rate
const UARTLCR_H: u64 = 0x02C;   // Line Control
const UARTCR: u64 = 0x030;      // Control Register
const UARTIMSC: u64 = 0x038;    // Interrupt Mask Set/Clear
const UARTRIS: u64 = 0x03C;     // Raw Interrupt Status
const UARTMIS: u64 = 0x040;     // Masked Interrupt Status
const UARTICR: u64 = 0x044;     // Interrupt Clear

/// Flag Register bits
#[allow(dead_code)]
const FR_TXFE: u32 = 1 << 7;    // Transmit FIFO empty
#[allow(dead_code)]
const FR_RXFF: u32 = 1 << 6;    // Receive FIFO full
const FR_TXFF: u32 = 1 << 5;    // Transmit FIFO full
const FR_RXFE: u32 = 1 << 4;    // Receive FIFO empty
#[allow(dead_code)]
const FR_BUSY: u32 = 1 << 3;    // UART busy

/// Interrupt bits
const INT_RX: u32 = 1 << 4;     // Receive interrupt
#[allow(dead_code)]
const INT_TX: u32 = 1 << 5;     // Transmit interrupt

/// Virtual UART device
pub struct VirtualUart {
    /// Control register
    cr: u32,
    /// Line control register
    lcr_h: u32,
    /// Integer baud rate
    ibrd: u32,
    /// Fractional baud rate
    fbrd: u32,
    /// Interrupt mask
    imsc: u32,
    /// Raw interrupt status
    ris: u32,
}

impl VirtualUart {
    /// Create a new virtual UART
    pub fn new() -> Self {
        Self {
            cr: 0x0301,      // UART enabled, TX/RX enabled
            lcr_h: 0x60,     // 8 data bits, no parity, 1 stop bit
            ibrd: 1,         // Baud rate (doesn't matter for QEMU)
            fbrd: 0,
            imsc: 0,         // Interrupts masked
            ris: 0,          // No raw interrupts
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

    /// Read flag register from real UART
    fn read_real_fr(&self) -> u32 {
        unsafe {
            let uart_fr = (UART_BASE as usize) + 0x18;
            let fr: u32;
            core::arch::asm!(
                "ldr {val:w}, [{addr}]",
                addr = in(reg) uart_fr,
                val = out(reg) fr,
                options(nostack, readonly),
            );
            fr
        }
    }

    /// Read a character from the real UART (non-blocking)
    /// Returns Some(char) if available, None if FIFO empty
    fn read_char(&self) -> Option<u8> {
        let fr = self.read_real_fr();
        if fr & FR_RXFE != 0 {
            // Receive FIFO empty
            return None;
        }

        unsafe {
            let uart_dr = UART_BASE as usize;
            let data: u32;
            core::arch::asm!(
                "ldr {val:w}, [{addr}]",
                addr = in(reg) uart_dr,
                val = out(reg) data,
                options(nostack, readonly),
            );
            Some((data & 0xFF) as u8)
        }
    }

    /// Get current flag register value (combining real UART state)
    fn get_flags(&self) -> u32 {
        let real_fr = self.read_real_fr();
        // TX is always ready (not full), RX state from real UART
        let mut fr = FR_TXFE;  // TX FIFO empty (ready to transmit)

        // Pass through RX status from real UART
        if real_fr & FR_RXFE != 0 {
            fr |= FR_RXFE;  // RX FIFO empty
        }

        fr
    }
}

impl MmioDevice for VirtualUart {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        // Allow 1, 2, or 4 byte reads
        if size != 1 && size != 2 && size != 4 {
            return Some(0);
        }

        let value = match offset {
            UARTDR => {
                // Data register: read character from real UART
                match self.read_char() {
                    Some(ch) => {
                        // Clear RX interrupt when data is read
                        self.ris &= !INT_RX;
                        ch as u64
                    }
                    None => 0,
                }
            }
            UARTFR => {
                // Flag register: get current state
                self.get_flags() as u64
            }
            UARTCR => self.cr as u64,
            UARTLCR_H => self.lcr_h as u64,
            UARTIBRD => self.ibrd as u64,
            UARTFBRD => self.fbrd as u64,
            UARTIMSC => self.imsc as u64,
            UARTRIS => {
                // Check if RX has data and set RX interrupt
                let fr = self.read_real_fr();
                if fr & FR_RXFE == 0 {
                    self.ris |= INT_RX;
                }
                self.ris as u64
            }
            UARTMIS => {
                // Masked interrupt status = RIS & IMSC
                (self.ris & self.imsc) as u64
            }
            _ => 0,
        };

        Some(value)
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        // Allow 1, 2, or 4 byte writes
        if size != 1 && size != 2 && size != 4 {
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
                self.cr = (value & 0xFFFF) as u32;
                true
            }
            UARTLCR_H => {
                self.lcr_h = (value & 0xFF) as u32;
                true
            }
            UARTIBRD => {
                self.ibrd = (value & 0xFFFF) as u32;
                true
            }
            UARTFBRD => {
                self.fbrd = (value & 0x3F) as u32;
                true
            }
            UARTIMSC => {
                self.imsc = (value & 0x7FF) as u32;
                true
            }
            UARTICR => {
                // Interrupt clear: clear the specified interrupt bits
                self.ris &= !(value as u32);
                true
            }
            UARTFR => {
                // Flag register is read-only, ignore writes
                true
            }
            _ => {
                // Unknown register, accept but ignore
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

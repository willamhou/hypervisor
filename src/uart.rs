//! PL011 UART Driver for QEMU virt machine
//!
//! Base address: 0x0900_0000 (QEMU virt)

use core::fmt;

/// PL011 UART registers
const UART_BASE: usize = 0x0900_0000;
const UART_DR: usize = UART_BASE + 0x00; // Data Register
const UART_FR: usize = UART_BASE + 0x18; // Flag Register

/// Flag Register bits
const UART_FR_TXFF: u32 = 1 << 5; // Transmit FIFO full

/// UART device structure
pub struct Uart {
    #[allow(dead_code)]
    base: usize,
}

impl Uart {
    /// Create a new UART instance
    const fn new(base: usize) -> Self {
        Self { base }
    }

    /// Write a byte to the UART
    pub fn putc(&self, c: u8) {
        // Wait until TX FIFO is not full
        while self.read_reg(UART_FR) & UART_FR_TXFF != 0 {}

        // Write character
        self.write_reg(UART_DR, c as u32);
    }

    /// Write a string to the UART
    pub fn puts(&self, s: &str) {
        for byte in s.bytes() {
            self.putc(byte);
        }
    }

    /// Read a register
    #[inline]
    fn read_reg(&self, offset: usize) -> u32 {
        // SAFETY: `offset` is a PL011 MMIO register address; volatile read is required.
        unsafe { core::ptr::read_volatile(offset as *const u32) }
    }

    /// Write a register
    #[inline]
    fn write_reg(&self, offset: usize, value: u32) {
        // SAFETY: `offset` is a PL011 MMIO register address; volatile write is required.
        unsafe { core::ptr::write_volatile(offset as *mut u32, value) }
    }
}

/// Global UART instance
static UART: Uart = Uart::new(UART_BASE);

/// Initialize the UART
pub fn init() {
    // For QEMU virt, UART is already initialized by firmware
    // Just write a test character to verify it works
    // SAFETY: Writes newline to PL011 DR MMIO register via inline asm.
    unsafe {
        core::arch::asm!(
            "mov x9, #0x09000000",
            "mov w10, #10",      // '\n'
            "str w10, [x9]",
            out("x9") _,
            out("w10") _,
        );
    }
}

/// Print a string to the UART
pub fn print(s: &str) {
    UART.puts(s);
}

/// Print implementation for fmt::Write
impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.puts(s);
        Ok(())
    }
}

// print!/println! macros are now in src/log.rs (route through LogWriter)

/// Virtual UART (PL011) device
///
/// Full trap-and-emulate PL011 with:
/// - TX: writes directly to physical UART via inline asm
/// - RX: ring buffer filled by hypervisor when physical UART IRQ fires
/// - Linux-compatible peripheral ID registers for amba-pl011.c probe

use crate::devices::MmioDevice;

/// UART base address (same as physical UART in QEMU virt machine)
const UART_BASE: u64 = 0x09000000;
const UART_SIZE: u64 = 0x1000;

// ── Register offsets ────────────────────────────────────────────────

const UARTDR: u64 = 0x000;
const UARTRSR: u64 = 0x004;      // Receive Status / Error Clear
const UARTFR: u64 = 0x018;
const UARTILPR: u64 = 0x020;     // IrDA Low-Power Counter
const UARTIBRD: u64 = 0x024;
const UARTFBRD: u64 = 0x028;
const UARTLCR_H: u64 = 0x02C;
const UARTCR: u64 = 0x030;
const UARTIFLS: u64 = 0x034;     // Interrupt FIFO Level Select
const UARTIMSC: u64 = 0x038;
const UARTRIS: u64 = 0x03C;
const UARTMIS: u64 = 0x040;
const UARTICR: u64 = 0x044;
const UARTDMACR: u64 = 0x048;    // DMA Control

// PL011 Peripheral ID registers (read by Linux amba bus during probe)
const UART_PERIPHID0: u64 = 0xFE0;
const UART_PERIPHID1: u64 = 0xFE4;
const UART_PERIPHID2: u64 = 0xFE8;
const UART_PERIPHID3: u64 = 0xFEC;
const UART_PCELLID0: u64 = 0xFF0;
const UART_PCELLID1: u64 = 0xFF4;
const UART_PCELLID2: u64 = 0xFF8;
const UART_PCELLID3: u64 = 0xFFC;

// ── Flag Register bits ──────────────────────────────────────────────

const FR_TXFE: u32 = 1 << 7;     // Transmit FIFO empty
const FR_RXFF: u32 = 1 << 6;     // Receive FIFO full
const FR_TXFF: u32 = 1 << 5;     // Transmit FIFO full
const FR_RXFE: u32 = 1 << 4;     // Receive FIFO empty

// ── Interrupt bits ──────────────────────────────────────────────────

const INT_RX: u32 = 1 << 4;      // Receive interrupt
const INT_TX: u32 = 1 << 5;      // Transmit interrupt
const INT_RT: u32 = 1 << 6;      // Receive timeout interrupt

/// UART SPI: SPI 1 = INTID 33
const UART_SPI_INTID: u32 = 33;

// ── RX Ring Buffer ──────────────────────────────────────────────────

const RX_BUF_SIZE: usize = 64;

/// Virtual UART device with RX ring buffer and full Linux compatibility.
pub struct VirtualUart {
    // Control/config registers
    cr: u32,
    lcr_h: u32,
    ibrd: u32,
    fbrd: u32,
    ifls: u32,
    imsc: u32,
    ris: u32,
    dmacr: u32,
    // RX ring buffer
    rx_buf: [u8; RX_BUF_SIZE],
    rx_head: usize,  // next read position
    rx_tail: usize,  // next write position
}

impl VirtualUart {
    pub fn new() -> Self {
        Self {
            cr: 0x0301,       // UART enabled, TX/RX enabled
            lcr_h: 0x60,      // 8 data bits, no parity, 1 stop bit
            ibrd: 1,
            fbrd: 0,
            ifls: 0x12,       // RX 1/2, TX 1/2 (default)
            imsc: 0,
            ris: 0,
            dmacr: 0,
            rx_buf: [0; RX_BUF_SIZE],
            rx_head: 0,
            rx_tail: 0,
        }
    }

    /// Write a character to the physical UART.
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

    /// Push a received byte into the RX ring buffer.
    /// Called by the hypervisor when physical UART data is available.
    pub fn push_rx(&mut self, ch: u8) {
        let next_tail = (self.rx_tail + 1) % RX_BUF_SIZE;
        if next_tail == self.rx_head {
            return; // buffer full, drop
        }
        self.rx_buf[self.rx_tail] = ch;
        self.rx_tail = next_tail;
        // Assert RX interrupt
        self.ris |= INT_RX;
    }

    /// Pop a byte from the RX ring buffer.
    fn pop_rx(&mut self) -> Option<u8> {
        if self.rx_head == self.rx_tail {
            return None; // empty
        }
        let ch = self.rx_buf[self.rx_head];
        self.rx_head = (self.rx_head + 1) % RX_BUF_SIZE;
        // If buffer now empty, clear RX interrupt
        if self.rx_head == self.rx_tail {
            self.ris &= !INT_RX;
        }
        Some(ch)
    }

    /// Check if RX buffer has data.
    fn rx_has_data(&self) -> bool {
        self.rx_head != self.rx_tail
    }

    /// Get flag register value based on RX buffer state.
    fn get_flags(&self) -> u32 {
        let mut fr = FR_TXFE; // TX always ready
        if !self.rx_has_data() {
            fr |= FR_RXFE;
        }
        let count = (self.rx_tail + RX_BUF_SIZE - self.rx_head) % RX_BUF_SIZE;
        if count >= RX_BUF_SIZE - 1 {
            fr |= FR_RXFF;
        }
        fr
    }
}

impl MmioDevice for VirtualUart {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        if size != 1 && size != 2 && size != 4 {
            return Some(0);
        }

        let value = match offset {
            UARTDR => {
                match self.pop_rx() {
                    Some(ch) => ch as u64,
                    None => 0,
                }
            }
            UARTRSR => 0,  // No errors
            UARTFR => self.get_flags() as u64,
            UARTILPR => 0,
            UARTIBRD => self.ibrd as u64,
            UARTFBRD => self.fbrd as u64,
            UARTLCR_H => self.lcr_h as u64,
            UARTCR => self.cr as u64,
            UARTIFLS => self.ifls as u64,
            UARTIMSC => self.imsc as u64,
            UARTRIS => self.ris as u64,
            UARTMIS => (self.ris & self.imsc) as u64,
            UARTDMACR => self.dmacr as u64,

            // PL011 Peripheral ID (required for Linux amba-pl011.c probe)
            UART_PERIPHID0 => 0x11,  // Part number low
            UART_PERIPHID1 => 0x10,  // Part number high + designer
            UART_PERIPHID2 => 0x14,  // Revision + designer (r1p4)
            UART_PERIPHID3 => 0x00,
            UART_PCELLID0 => 0x0D,   // PrimeCell ID
            UART_PCELLID1 => 0xF0,
            UART_PCELLID2 => 0x05,
            UART_PCELLID3 => 0xB1,

            _ => 0,
        };

        Some(value)
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        if size != 1 && size != 2 && size != 4 {
            return false;
        }

        match offset {
            UARTDR => {
                let ch = (value & 0xFF) as u8;
                self.output_char(ch);
                // Assert TX interrupt (TX FIFO always immediately empties)
                self.ris |= INT_TX;
                true
            }
            UARTRSR => true, // Error clear — no-op
            UARTILPR => true, // IrDA — ignore
            UARTIBRD => { self.ibrd = (value & 0xFFFF) as u32; true }
            UARTFBRD => { self.fbrd = (value & 0x3F) as u32; true }
            UARTLCR_H => { self.lcr_h = (value & 0xFF) as u32; true }
            UARTCR => { self.cr = (value & 0xFFFF) as u32; true }
            UARTIFLS => { self.ifls = (value & 0x3F) as u32; true }
            UARTIMSC => { self.imsc = (value & 0x7FF) as u32; true }
            UARTICR => {
                self.ris &= !(value as u32);
                true
            }
            UARTDMACR => { self.dmacr = (value & 0x07) as u32; true }
            UARTFR => true, // Read-only, ignore writes
            _ => true,       // Unknown — accept silently
        }
    }

    fn base_address(&self) -> u64 { UART_BASE }
    fn size(&self) -> u64 { UART_SIZE }

    fn pending_irq(&self) -> Option<u32> {
        if (self.ris & self.imsc) != 0 {
            Some(UART_SPI_INTID)
        } else {
            None
        }
    }
}

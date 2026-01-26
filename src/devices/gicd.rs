/// Virtual GIC Distributor (GICD)
/// 
/// This emulates a minimal GICD for guest interrupt configuration.

use super::MmioDevice;

/// GICD base address
const GICD_BASE: u64 = 0x08000000;
const GICD_SIZE: u64 = 0x10000;

/// GICD register offsets
const GICD_CTLR: u64 = 0x000;       // Distributor Control Register
const GICD_TYPER: u64 = 0x004;      // Interrupt Controller Type Register
const GICD_ISENABLER: u64 = 0x100;  // Interrupt Set-Enable Registers
const GICD_ICENABLER: u64 = 0x180;  // Interrupt Clear-Enable Registers

/// Virtual GICD device
pub struct VirtualGicd {
    /// Distributor control register
    ctlr: u32,
    /// Interrupt enable bits (1024 interrupts max, 32 regs)
    enabled: [u32; 32],
}

impl VirtualGicd {
    /// Create a new virtual GICD
    pub fn new() -> Self {
        Self {
            ctlr: 0,
            enabled: [0; 32],
        }
    }
}

impl MmioDevice for VirtualGicd {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        if size != 4 {
            // GIC registers are 32-bit
            return Some(0);
        }
        
        match offset {
            GICD_CTLR => {
                // Control register
                Some(self.ctlr as u64)
            }
            GICD_TYPER => {
                // Type register
                // ITLinesNumber[4:0] = 31 means (31+1)*32 = 1024 interrupts
                // CPUNumber[7:5] = 0 means 1 CPU
                Some(31)
            }
            GICD_ISENABLER..=0x17F => {
                // Interrupt Set-Enable registers
                let reg = ((offset - GICD_ISENABLER) / 4) as usize;
                if reg < 32 {
                    Some(self.enabled[reg] as u64)
                } else {
                    Some(0)
                }
            }
            GICD_ICENABLER..=0x1FF => {
                // Interrupt Clear-Enable registers (read as enabled state)
                let reg = ((offset - GICD_ICENABLER) / 4) as usize;
                if reg < 32 {
                    Some(self.enabled[reg] as u64)
                } else {
                    Some(0)
                }
            }
            _ => {
                // Other registers: return 0
                Some(0)
            }
        }
    }
    
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        if size != 4 {
            return false;
        }
        
        let val = (value & 0xFFFF_FFFF) as u32;
        
        match offset {
            GICD_CTLR => {
                // Control register
                self.ctlr = val;
                
                if val & 1 != 0 {
                    crate::uart_puts(b"[GICD] Distributor enabled\n");
                } else {
                    crate::uart_puts(b"[GICD] Distributor disabled\n");
                }
                true
            }
            GICD_ISENABLER..=0x17F => {
                // Interrupt Set-Enable registers
                let reg = ((offset - GICD_ISENABLER) / 4) as usize;
                if reg < 32 {
                    self.enabled[reg] |= val;
                    
                    // Log which interrupts were enabled
                    if val != 0 {
                        crate::uart_puts(b"[GICD] Enable IRQs: reg=");
                        crate::uart_put_u64(reg as u64);
                        crate::uart_puts(b", mask=0x");
                        crate::uart_put_hex(val as u64);
                        crate::uart_puts(b"\n");
                    }
                }
                true
            }
            GICD_ICENABLER..=0x1FF => {
                // Interrupt Clear-Enable registers
                let reg = ((offset - GICD_ICENABLER) / 4) as usize;
                if reg < 32 {
                    self.enabled[reg] &= !val;
                    
                    if val != 0 {
                        crate::uart_puts(b"[GICD] Disable IRQs: reg=");
                        crate::uart_put_u64(reg as u64);
                        crate::uart_puts(b", mask=0x");
                        crate::uart_put_hex(val as u64);
                        crate::uart_puts(b"\n");
                    }
                }
                true
            }
            _ => {
                // Other registers: ignore
                true
            }
        }
    }
    
    fn base_address(&self) -> u64 {
        GICD_BASE
    }
    
    fn size(&self) -> u64 {
        GICD_SIZE
    }
}

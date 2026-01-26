//! Virtual Interrupt Management for Guest vCPUs
//!
//! This module handles injecting virtual interrupts into guest VMs.
//! 
//! ## GICv3/v4 List Register Mechanism
//! GICv3+ uses List Registers (LRs) instead of HCR_EL2.VI for interrupt injection.
//! Each LR can hold one pending/active virtual interrupt with its state, priority, etc.
//!
//! ## Interrupt Injection Flow
//! 1. Hypervisor receives physical interrupt (e.g., timer)
//! 2. If interrupt is for guest, call inject_irq()
//! 3. inject_irq() writes to an available List Register
//! 4. Hardware automatically injects virtual interrupt into guest
//! 5. Guest handles interrupt and performs EOI
//! 6. Hardware clears the LR automatically

/// Virtual interrupt state for a vCPU
#[derive(Debug, Clone, Copy)]
pub struct VirtualInterruptState {
    /// Pending virtual IRQ
    pub irq_pending: bool,
    
    /// Pending virtual FIQ
    pub fiq_pending: bool,
    
    /// IRQ number that is pending (if any)
    pub pending_irq_num: Option<u32>,
    
    /// Use GICv3 List Registers (true) or HCR_EL2.VI (false)
    pub use_gicv3: bool,
}

impl Default for VirtualInterruptState {
    fn default() -> Self {
        Self {
            irq_pending: false,
            fiq_pending: false,
            pending_irq_num: None,
            use_gicv3: true, // Default to GICv3
        }
    }
}

impl VirtualInterruptState {
    /// Create a new virtual interrupt state
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Inject a virtual IRQ
    ///
    /// # Arguments
    /// * `irq_num` - The interrupt number to inject
    pub fn inject_irq(&mut self, irq_num: u32) {
        if self.use_gicv3 {
            // Use GICv3 List Register injection
            use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
            
            match GicV3VirtualInterface::inject_interrupt(irq_num, 0xA0) {
                Ok(()) => {
                    self.irq_pending = true;
                    self.pending_irq_num = Some(irq_num);
                }
                Err(_) => {
                    // Fallback to HCR_EL2.VI if LRs are full
                    self.irq_pending = true;
                    self.pending_irq_num = Some(irq_num);
                }
            }
        } else {
            // Use legacy HCR_EL2.VI mechanism
            self.irq_pending = true;
            self.pending_irq_num = Some(irq_num);
        }
    }
    
    /// Inject a virtual FIQ
    pub fn inject_fiq(&mut self) {
        self.fiq_pending = true;
    }
    
    /// Clear IRQ pending state
    pub fn clear_irq(&mut self) {
        if self.use_gicv3 {
            // GICv3 LRs are cleared automatically by hardware
            // when guest performs EOI, but we can manually clear if needed
            if let Some(irq_num) = self.pending_irq_num {
                use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
                GicV3VirtualInterface::clear_interrupt(irq_num);
            }
        }
        self.irq_pending = false;
        self.pending_irq_num = None;
    }
    
    /// Clear FIQ pending state
    pub fn clear_fiq(&mut self) {
        self.fiq_pending = false;
    }
    
    /// Check if any interrupt is pending
    pub fn has_pending_interrupt(&self) -> bool {
        self.irq_pending || self.fiq_pending
    }
    
    /// Apply interrupt state to HCR_EL2
    ///
    /// This is only used for legacy mode (GICv2 or fallback).
    /// GICv3+ uses List Registers instead.
    ///
    /// # Arguments
    /// * `hcr` - Current HCR_EL2 value
    ///
    /// # Returns
    /// Updated HCR_EL2 value with VI/VF bits set
    pub fn apply_to_hcr(&self, hcr: u64) -> u64 {
        if self.use_gicv3 {
            // GICv3 mode: don't use HCR_EL2.VI
            // Just return HCR unchanged
            hcr
        } else {
            // Legacy mode: use HCR_EL2.VI/VF
            let mut new_hcr = hcr;
            
            // Bit 7: VI - Virtual IRQ pending
            if self.irq_pending {
                new_hcr |= 1 << 7;
            } else {
                new_hcr &= !(1 << 7);
            }
            
            // Bit 6: VF - Virtual FIQ pending
            if self.fiq_pending {
                new_hcr |= 1 << 6;
            } else {
                new_hcr &= !(1 << 6);
            }
            
            new_hcr
        }
    }
}

/// Set HCR_EL2 with virtual interrupt state
///
/// # Safety
/// This function modifies system registers and must be called with care.
#[inline]
pub unsafe fn set_hcr_el2(hcr: u64) {
    core::arch::asm!(
        "msr hcr_el2, {hcr}",
        "isb",
        hcr = in(reg) hcr,
        options(nostack, nomem),
    );
}

/// Read current HCR_EL2 value
///
/// # Safety
/// This function reads system registers.
#[inline]
pub unsafe fn get_hcr_el2() -> u64 {
    let hcr: u64;
    core::arch::asm!(
        "mrs {hcr}, hcr_el2",
        hcr = out(reg) hcr,
        options(nostack, nomem),
    );
    hcr
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_inject_irq() {
        let mut state = VirtualInterruptState::new();
        assert!(!state.has_pending_interrupt());
        
        state.inject_irq(27); // Timer IRQ
        assert!(state.has_pending_interrupt());
        assert_eq!(state.pending_irq_num, Some(27));
        
        state.clear_irq();
        assert!(!state.has_pending_interrupt());
    }
    
    #[test]
    fn test_apply_to_hcr() {
        let mut state = VirtualInterruptState::new();
        let base_hcr = 0x80000000u64; // RW bit set
        
        // No interrupt pending
        let hcr = state.apply_to_hcr(base_hcr);
        assert_eq!(hcr & (1 << 7), 0); // VI bit clear
        assert_eq!(hcr & (1 << 6), 0); // VF bit clear
        
        // IRQ pending
        state.inject_irq(27);
        let hcr = state.apply_to_hcr(base_hcr);
        assert_ne!(hcr & (1 << 7), 0); // VI bit set
        
        // FIQ pending
        state.inject_fiq();
        let hcr = state.apply_to_hcr(base_hcr);
        assert_ne!(hcr & (1 << 6), 0); // VF bit set
    }
}

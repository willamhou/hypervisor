//! Virtual CPU (vCPU) Management
//!
//! This module provides the [`Vcpu`] type which represents a virtual processor
//! that can execute guest code. Each vCPU maintains its own register context,
//! interrupt state, and execution state.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │                 Vcpu                    │
//! ├─────────────────────────────────────────┤
//! │  id: usize          - vCPU identifier   │
//! │  state: VcpuState   - Ready/Running/... │
//! ├─────────────────────────────────────────┤
//! │  VcpuContext                            │
//! │  ├─ gp_regs (x0-x30)                    │
//! │  ├─ sys_regs (ELR, SPSR, etc.)          │
//! │  ├─ pc (program counter)                │
//! │  └─ sp (stack pointer)                  │
//! ├─────────────────────────────────────────┤
//! │  VirtualInterruptState                  │
//! │  └─ pending IRQ injection via HCR_EL2   │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # State Machine
//!
//! ```text
//!                    ┌──────────────┐
//!                    │ Uninitialized│
//!                    └──────┬───────┘
//!                           │ new() / reset()
//!                           ▼
//!              ┌───────► Ready ◄───────┐
//!              │            │          │
//!              │            │ run()    │
//!              │            ▼          │
//!              │        Running ───────┘
//!              │            │    (guest exit)
//!              │            │
//!              │            ▼
//!              │        Stopped
//!              │            │
//!              └────────────┘
//!                  reset()
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use hypervisor::vcpu::Vcpu;
//!
//! // Create a vCPU with entry point and stack
//! let mut vcpu = Vcpu::new(0, 0x4000_0000, 0x4001_0000);
//!
//! // Run the guest
//! match vcpu.run() {
//!     Ok(()) => println!("Guest exited normally"),
//!     Err("WFI") => println!("Guest waiting for interrupt"),
//!     Err(e) => println!("Error: {}", e),
//! }
//!
//! // Inject an interrupt
//! vcpu.inject_irq(27); // Timer interrupt
//! ```
//!
//! # Interrupt Injection
//!
//! Virtual interrupts are injected using the GICv3 List Registers or the
//! HCR_EL2.VI bit. When `inject_irq()` is called, the interrupt is marked
//! as pending and will be delivered when the guest resumes execution with
//! interrupts enabled.

use crate::arch::aarch64::{VcpuContext, enter_guest};
use crate::vcpu_interrupt::VirtualInterruptState;

/// Virtual CPU execution state
///
/// Represents the current state of a vCPU in its lifecycle.
///
/// # State Transitions
///
/// | From | To | Trigger |
/// |------|-----|---------|
/// | `Uninitialized` | `Ready` | `new()` or `reset()` |
/// | `Ready` | `Running` | `run()` called |
/// | `Running` | `Ready` | Guest exit (HVC, WFI, etc.) |
/// | `Running` | `Stopped` | Fatal error |
/// | `Stopped` | `Ready` | `reset()` |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpuState {
    /// vCPU has not been initialized with entry point
    Uninitialized,

    /// vCPU is ready to execute guest code
    Ready,

    /// vCPU is currently executing in guest mode (EL1)
    Running,

    /// vCPU has been stopped and cannot run
    Stopped,
}

/// Virtual CPU (vCPU)
///
/// Represents a single virtual processor that can execute guest code at EL1.
/// Each vCPU maintains its own register context, allowing multiple vCPUs
/// to run independently within a VM.
///
/// # Thread Safety
///
/// A `Vcpu` is **not** thread-safe. Only one physical CPU should access
/// a given `Vcpu` at a time. The hypervisor ensures this by binding vCPUs
/// to physical CPUs during scheduling.
///
/// # Register Context
///
/// The vCPU saves and restores all guest-visible registers on entry/exit:
/// - General purpose registers (x0-x30)
/// - Stack pointer (SP_EL1)
/// - Program counter (ELR_EL2)
/// - Processor state (SPSR_EL2)
/// - System registers (SCTLR_EL1, TTBR0_EL1, etc.)
pub struct Vcpu {
    /// Unique identifier for this vCPU
    id: usize,
    
    /// Current state of the vCPU
    state: VcpuState,
    
    /// Register context for this vCPU
    context: VcpuContext,
    
    /// Virtual interrupt state for this vCPU
    virt_irq: VirtualInterruptState,
}

impl Vcpu {
    /// Create a new vCPU
    /// 
    /// # Arguments
    /// * `id` - Unique identifier for this vCPU
    /// * `entry_point` - Guest code entry point (physical address)
    /// * `stack_pointer` - Guest stack pointer
    pub fn new(id: usize, entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            id,
            state: VcpuState::Ready,
            context: VcpuContext::new(entry_point, stack_pointer),
            virt_irq: VirtualInterruptState::new(),
        }
    }
    
    /// Get vCPU ID
    pub fn id(&self) -> usize {
        self.id
    }
    
    /// Get current state
    pub fn state(&self) -> VcpuState {
        self.state
    }
    
    /// Get mutable reference to context
    pub fn context_mut(&mut self) -> &mut VcpuContext {
        &mut self.context
    }
    
    /// Get reference to context
    pub fn context(&self) -> &VcpuContext {
        &self.context
    }
    
    /// Run the vCPU
    /// 
    /// This will enter the guest and execute code until an exit occurs.
    /// 
    /// # Returns
    /// * `Ok(())` - Guest exited normally (HVC #0)
    /// * `Err("WFI")` - Guest executed WFI (waiting for interrupt)
    /// * `Err(msg)` - Other error occurred
    pub fn run(&mut self) -> Result<(), &'static str> {
        if self.state != VcpuState::Ready {
            return Err("vCPU is not in Ready state");
        }
        
        self.state = VcpuState::Running;
        
        // Apply virtual interrupt state to HCR_EL2 before entering guest
        unsafe {
            use crate::vcpu_interrupt::{get_hcr_el2, set_hcr_el2};
            let hcr = get_hcr_el2();
            let hcr_with_vi = self.virt_irq.apply_to_hcr(hcr);
            set_hcr_el2(hcr_with_vi);
        }
        
        // Enter the guest
        let result = unsafe {
            enter_guest(&mut self.context as *mut VcpuContext)
        };
        
        self.state = VcpuState::Ready;
        
        // After guest exit, check if interrupt was handled and clear VI bit if needed
        if self.virt_irq.has_pending_interrupt() {
            // Guest returned from interrupt handler, clear pending state
            self.virt_irq.clear_irq();
        }
        
        match result {
            0 => Ok(()),          // Normal exit (HVC #0)
            1 => Err("WFI"),      // Guest executed WFI
            _ => Err("Guest exit with error"),
        }
    }
    
    /// Stop the vCPU
    pub fn stop(&mut self) {
        self.state = VcpuState::Stopped;
    }
    
    /// Reset the vCPU to initial state
    pub fn reset(&mut self, entry_point: u64, stack_pointer: u64) {
        self.context = VcpuContext::new(entry_point, stack_pointer);
        self.state = VcpuState::Ready;
    }
    
    /// Inject a virtual IRQ into the guest
    ///
    /// # Arguments
    /// * `irq_num` - The interrupt number to inject
    pub fn inject_irq(&mut self, irq_num: u32) {
        self.virt_irq.inject_irq(irq_num);
    }
    
    /// Check if vCPU has pending interrupts
    pub fn has_pending_interrupt(&self) -> bool {
        self.virt_irq.has_pending_interrupt()
    }
    
    /// Clear pending IRQ
    pub fn clear_irq(&mut self) {
        self.virt_irq.clear_irq();
    }
    
    /// Get virtual interrupt state
    pub fn virt_irq(&self) -> &VirtualInterruptState {
        &self.virt_irq
    }
    
    /// Get mutable virtual interrupt state
    pub fn virt_irq_mut(&mut self) -> &mut VirtualInterruptState {
        &mut self.virt_irq
    }
}

impl core::fmt::Debug for Vcpu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vcpu")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("pc", &format_args!("0x{:016x}", self.context.pc))
            .field("sp", &format_args!("0x{:016x}", self.context.sp))
            .finish()
    }
}

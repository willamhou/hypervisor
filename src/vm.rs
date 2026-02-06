//! Virtual Machine Management
//!
//! This module provides the [`Vm`] type which represents a complete virtual machine
//! containing one or more vCPUs, Stage-2 memory mapping, and emulated devices.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                          Vm                              │
//! ├──────────────────────────────────────────────────────────┤
//! │  id: usize                    - VM identifier            │
//! │  state: VmState               - Ready/Running/Paused/... │
//! ├──────────────────────────────────────────────────────────┤
//! │  vcpus[0..MAX_VCPUS]          - Virtual processors       │
//! │  ┌────────┐ ┌────────┐ ┌────────┐                        │
//! │  │ vCPU 0 │ │ vCPU 1 │ │  ...   │                        │
//! │  └────────┘ └────────┘ └────────┘                        │
//! ├──────────────────────────────────────────────────────────┤
//! │  scheduler: Scheduler         - Round-robin vCPU sched   │
//! ├──────────────────────────────────────────────────────────┤
//! │  Stage-2 Page Tables          - IPA → PA translation     │
//! │  ┌─────────────────────────────────────────────────┐     │
//! │  │ Guest Memory (Normal)  │  MMIO (Device)         │     │
//! │  │ 0x4000_0000-0x4020_0000│  0x0900_0000 (UART)    │     │
//! │  └─────────────────────────────────────────────────┘     │
//! ├──────────────────────────────────────────────────────────┤
//! │  devices: DeviceManager       - MMIO trap-and-emulate    │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example: Creating and Running a VM
//!
//! ```rust,ignore
//! use hypervisor::vm::Vm;
//!
//! // Create a new VM
//! let mut vm = Vm::new(0);
//!
//! // Initialize memory mapping
//! vm.init_memory(0x4000_0000, 0x200000);
//!
//! // Create vCPUs
//! {
//!     let vcpu0 = vm.create_vcpu(0).unwrap();
//!     vcpu0.context_mut().pc = guest_entry;
//! }
//! vm.create_vcpu(1).unwrap();
//!
//! // Run scheduler loop
//! while let Some(vcpu_id) = vm.schedule() {
//!     match vm.run_current() {
//!         Ok(()) => {
//!             // Guest exited normally
//!             vm.mark_current_done();
//!         }
//!         Err("WFI") => {
//!             // Guest waiting for interrupt
//!             vm.block_current();
//!         }
//!         Err(e) => {
//!             // Handle error
//!             break;
//!         }
//!     }
//! }
//! ```
//!
//! # Memory Model
//!
//! The VM uses ARM Stage-2 translation to provide memory isolation:
//!
//! - **IPA (Intermediate Physical Address)**: Guest-visible physical addresses
//! - **PA (Physical Address)**: Host physical addresses
//! - **Identity Mapping**: IPA == PA for simplicity
//!
//! Guest memory accesses to unmapped MMIO regions cause Stage-2 faults,
//! which are trapped to the hypervisor for device emulation.

use crate::vcpu::Vcpu;
use crate::arch::aarch64::{MemoryAttributes, init_stage2};
use crate::devices::DeviceManager;
use crate::scheduler::Scheduler;

/// Maximum number of vCPUs per VM
///
/// This limit is chosen to balance memory usage (each vCPU requires
/// context storage) with practical multi-core guest support.
pub const MAX_VCPUS: usize = 8;

/// Virtual Machine lifecycle state
///
/// Tracks the overall state of the VM for lifecycle management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM created but no vCPUs or memory configured
    Uninitialized,

    /// VM fully configured and ready to execute
    Ready,

    /// At least one vCPU is currently executing
    Running,

    /// VM execution suspended, can be resumed
    Paused,

    /// VM terminated, requires reset to run again
    Stopped,
}

/// Virtual Machine
///
/// A `Vm` represents a complete isolated execution environment containing:
///
/// - **vCPUs**: Up to [`MAX_VCPUS`] virtual processors
/// - **Memory**: Stage-2 page tables for guest physical address translation
/// - **Devices**: Emulated MMIO devices (UART, GIC, etc.)
/// - **Scheduler**: Round-robin vCPU scheduling
///
/// # Lifecycle
///
/// 1. Create VM with `Vm::new(id)`
/// 2. Initialize memory with `init_memory(start, size)`
/// 3. Create vCPUs with `create_vcpu(id)`
/// 4. Run scheduler loop with `schedule()` + `run_current()`
/// 5. Stop with `stop()` when done
pub struct Vm {
    /// Unique identifier for this VM
    id: usize,

    /// Current state of the VM
    state: VmState,

    /// vCPUs belonging to this VM
    vcpus: [Option<Vcpu>; MAX_VCPUS],

    /// Number of active vCPUs
    vcpu_count: usize,

    /// Whether memory is initialized
    memory_initialized: bool,

    /// Device manager for MMIO emulation
    /// Note: Currently accessed via global::DEVICES for exception handler access
    #[allow(dead_code)]
    devices: DeviceManager,

    /// Scheduler for multi-vCPU execution
    scheduler: Scheduler,
}

impl Vm {
    /// Create a new VM
    ///
    /// # Arguments
    /// * `id` - Unique identifier for this VM
    pub fn new(id: usize) -> Self {
        const INIT: Option<Vcpu> = None;
        let devices = DeviceManager::new();

        // Install device manager globally for exception handler access
        crate::global::DEVICES.init(devices);

        Self {
            id,
            state: VmState::Uninitialized,
            vcpus: [INIT; MAX_VCPUS],
            vcpu_count: 0,
            memory_initialized: false,
            devices: DeviceManager::new(), // Dummy, real one is in global
            scheduler: Scheduler::new(),
        }
    }
    
    /// Get VM ID
    pub fn id(&self) -> usize {
        self.id
    }
    
    /// Get current state
    pub fn state(&self) -> VmState {
        self.state
    }
    
    /// Get number of vCPUs
    pub fn vcpu_count(&self) -> usize {
        self.vcpu_count
    }
    
    /// Initialize memory for the VM
    /// 
    /// Sets up identity mapping for guest memory regions.
    /// 
    /// # Arguments
    /// * `guest_mem_start` - Start of guest memory region
    /// * `guest_mem_size` - Size of guest memory region
    pub fn init_memory(&mut self, guest_mem_start: u64, guest_mem_size: u64) {
        use crate::uart_puts;
        use crate::arch::aarch64::mm::IdentityMapper;
        
        if self.memory_initialized {
            uart_puts(b"[VM] Memory already initialized\n");
            return;
        }
        
        uart_puts(b"[VM] Initializing memory mapping...\n");
        
        // Use a global static mapper (to avoid large stack allocation)
        static mut MAPPER: IdentityMapper = IdentityMapper::new();
        
        // Map guest memory region (identity mapping)
        // Round to 2MB boundaries
        let start_aligned = guest_mem_start & !(2 * 1024 * 1024 - 1);
        let size_aligned = ((guest_mem_size + 2 * 1024 * 1024 - 1) / (2 * 1024 * 1024)) * (2 * 1024 * 1024);
        
        uart_puts(b"[VM] Mapping region: 0x");
        print_hex(start_aligned);
        uart_puts(b" - 0x");
        print_hex(start_aligned + size_aligned);
        uart_puts(b"\n");
        
        unsafe {
            MAPPER.map_region(start_aligned, size_aligned, MemoryAttributes::NORMAL);

            // Map MMIO device regions (DEVICE memory type)
            // NOTE: UART is NOT mapped - accesses will trap for virtualization
            // This allows the hypervisor to emulate UART I/O

            // GIC Distributor: 0x08000000 - 0x08010000 (64KB)
            let gicd_base = 0x08000000u64;
            let gicd_size = 2 * 1024 * 1024;  // 2MB block
            MAPPER.map_region(gicd_base, gicd_size, MemoryAttributes::DEVICE);

            // GIC Redistributor: 0x080A0000 - 0x08100000 for GICv3
            // Note: Redistributor is at different address than Distributor
            let gicr_base = 0x080A0000u64 & !(2 * 1024 * 1024 - 1);  // Align to 2MB
            let gicr_size = 2 * 1024 * 1024;  // 2MB block
            MAPPER.map_region(gicr_base, gicr_size, MemoryAttributes::DEVICE);

            // NOTE: UART is NOT mapped - Zephyr uses Jailhouse console (HVC) instead
            // UART virtualization is deferred for now

            // Initialize Stage-2 translation
            init_stage2(&MAPPER);
        }
        
        self.memory_initialized = true;
        uart_puts(b"[VM] Memory mapping complete\n");
    }
    
    /// Create a vCPU with specified ID
    ///
    /// # Arguments
    /// * `vcpu_id` - Unique ID for the vCPU (0 to MAX_VCPUS-1)
    ///
    /// # Returns
    /// * `Ok(&mut Vcpu)` - Reference to the created vCPU
    /// * `Err(msg)` - Failed to create vCPU
    pub fn create_vcpu(&mut self, vcpu_id: usize) -> Result<&mut Vcpu, &'static str> {
        if vcpu_id >= MAX_VCPUS {
            return Err("vCPU ID out of range");
        }
        if self.vcpus[vcpu_id].is_some() {
            return Err("vCPU already exists");
        }

        let vcpu = Vcpu::new(vcpu_id, 0, 0);
        self.vcpus[vcpu_id] = Some(vcpu);
        self.vcpu_count += 1;
        self.scheduler.add_vcpu(vcpu_id);

        if self.state == VmState::Uninitialized {
            self.state = VmState::Ready;
        }

        Ok(self.vcpus[vcpu_id].as_mut().unwrap())
    }

    /// Add a vCPU to this VM
    ///
    /// # Arguments
    /// * `entry_point` - Guest code entry point
    /// * `stack_pointer` - Guest stack pointer
    ///
    /// # Returns
    /// * `Ok(vcpu_id)` - Successfully added vCPU with given ID
    /// * `Err(msg)` - Failed to add vCPU
    pub fn add_vcpu(&mut self, entry_point: u64, stack_pointer: u64)
        -> Result<usize, &'static str> {
        if self.vcpu_count >= MAX_VCPUS {
            return Err("Maximum vCPU count reached");
        }

        let vcpu_id = self.vcpu_count;
        let vcpu = Vcpu::new(vcpu_id, entry_point, stack_pointer);

        self.vcpus[vcpu_id] = Some(vcpu);
        self.vcpu_count += 1;

        if self.state == VmState::Uninitialized {
            self.state = VmState::Ready;
        }

        Ok(vcpu_id)
    }
    
    /// Get a reference to a vCPU
    pub fn vcpu(&self, vcpu_id: usize) -> Option<&Vcpu> {
        if vcpu_id < self.vcpu_count {
            self.vcpus[vcpu_id].as_ref()
        } else {
            None
        }
    }
    
    /// Get a mutable reference to a vCPU
    pub fn vcpu_mut(&mut self, vcpu_id: usize) -> Option<&mut Vcpu> {
        if vcpu_id < self.vcpu_count {
            self.vcpus[vcpu_id].as_mut()
        } else {
            None
        }
    }
    
    /// Run the VM (single vCPU for now)
    /// 
    /// This will run vCPU 0 until it exits.
    /// In a real implementation, this would schedule all vCPUs.
    pub fn run(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Ready {
            return Err("VM is not in Ready state");
        }
        
        if self.vcpu_count == 0 {
            return Err("No vCPUs configured");
        }
        
        self.state = VmState::Running;
        
        // For now, just run vCPU 0
        // In a real implementation, we would have a proper scheduler
        if let Some(vcpu) = self.vcpu_mut(0) {
            let result = vcpu.run();
            
            self.state = VmState::Ready;
            
            result
        } else {
            self.state = VmState::Ready;
            Err("vCPU 0 not found")
        }
    }
    
    /// Pause the VM
    pub fn pause(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Running {
            return Err("VM is not running");
        }
        
        self.state = VmState::Paused;
        Ok(())
    }
    
    /// Resume the VM
    pub fn resume(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Paused {
            return Err("VM is not paused");
        }
        
        self.state = VmState::Running;
        Ok(())
    }
    
    /// Stop the VM
    pub fn stop(&mut self) {
        // Stop all vCPUs
        for vcpu in self.vcpus.iter_mut().flatten() {
            vcpu.stop();
        }

        self.state = VmState::Stopped;
    }

    // ========== Scheduler Integration ==========

    /// Schedule the next vCPU to run
    ///
    /// Returns the vCPU ID that should run next, or None if no vCPU is ready.
    pub fn schedule(&mut self) -> Option<usize> {
        self.scheduler.pick_next()
    }

    /// Run the currently scheduled vCPU
    ///
    /// # Returns
    /// * `Ok(())` - Guest exited normally
    /// * `Err("WFI")` - Guest executed WFI
    /// * `Err(msg)` - Error occurred
    pub fn run_current(&mut self) -> Result<(), &'static str> {
        let vcpu_id = self.scheduler.current().ok_or("No current vCPU")?;
        let vcpu = self.vcpus[vcpu_id].as_mut().ok_or("vCPU not found")?;
        vcpu.run()
    }

    /// Mark the current vCPU as done (remove from scheduler)
    pub fn mark_current_done(&mut self) {
        if let Some(id) = self.scheduler.current() {
            self.scheduler.remove_vcpu(id);
        }
    }

    /// Yield the current vCPU (allow another vCPU to run)
    pub fn yield_current(&mut self) {
        self.scheduler.yield_current();
    }

    /// Block the current vCPU (e.g., waiting for I/O)
    pub fn block_current(&mut self) {
        self.scheduler.block_current();
    }

    /// Unblock a vCPU (make it ready to run again)
    pub fn unblock(&mut self, vcpu_id: usize) {
        self.scheduler.unblock(vcpu_id);
    }

    /// Get the currently scheduled vCPU ID
    pub fn current_vcpu(&self) -> Option<usize> {
        self.scheduler.current()
    }
}

impl core::fmt::Debug for Vm {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vm")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("vcpu_count", &self.vcpu_count)
            .finish()
    }
}

/// Helper function to print hex value
fn print_hex(value: u64) {
    use crate::uart_puts;
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];
    
    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }
    
    uart_puts(&buffer);
}

//! Virtual Machine Management
//!
//! This module provides the [`Vm`] type which represents a complete virtual machine
//! containing one or more vCPUs, Stage-2 memory mapping, and emulated devices.

use crate::vcpu::Vcpu;
use crate::arch::aarch64::{MemoryAttributes, init_stage2};
use crate::arch::aarch64::defs::*;
use crate::devices::DeviceManager;
use crate::scheduler::Scheduler;
use crate::platform;

/// Maximum number of vCPUs per VM
pub const MAX_VCPUS: usize = 8;

/// Virtual Machine lifecycle state
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

    /// Scheduler for multi-vCPU execution
    scheduler: Scheduler,
}

impl Vm {
    /// Create a new VM
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
    pub fn init_memory(&mut self, guest_mem_start: u64, guest_mem_size: u64) {
        use crate::uart_puts;
        use crate::uart_put_hex;
        use crate::arch::aarch64::mm::IdentityMapper;

        if self.memory_initialized {
            uart_puts(b"[VM] Memory already initialized\n");
            return;
        }

        uart_puts(b"[VM] Initializing memory mapping...\n");

        // Use a global static mapper (to avoid large stack allocation)
        static mut MAPPER: IdentityMapper = IdentityMapper::new();

        // Reset the mapper to clear any stale mappings from previous VM runs
        unsafe {
            MAPPER.reset();
        }

        // Map guest memory region (identity mapping)
        // Round to 2MB boundaries
        let start_aligned = guest_mem_start & !BLOCK_MASK_2MB;
        let size_aligned = ((guest_mem_size + BLOCK_SIZE_2MB - 1) / BLOCK_SIZE_2MB) * BLOCK_SIZE_2MB;

        uart_puts(b"[VM] Mapping region: 0x");
        uart_put_hex(start_aligned);
        uart_puts(b" - 0x");
        uart_put_hex(start_aligned + size_aligned);
        uart_puts(b"\n");

        unsafe {
            MAPPER.map_region(start_aligned, size_aligned, MemoryAttributes::NORMAL);

            // Map MMIO device regions (DEVICE memory type)

            // GIC region: covers distributor + redistributor
            MAPPER.map_region(platform::GIC_REGION_BASE, platform::GIC_REGION_SIZE, MemoryAttributes::DEVICE);

            // Map UART for passthrough
            MAPPER.map_region(platform::UART_BASE as u64, BLOCK_SIZE_2MB, MemoryAttributes::DEVICE);

            // Initialize Stage-2 translation
            init_stage2(&MAPPER);
        }

        self.memory_initialized = true;
        uart_puts(b"[VM] Memory mapping complete\n");
    }

    /// Create a vCPU with specified ID
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
    pub fn run(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Ready {
            return Err("VM is not in Ready state");
        }

        if self.vcpu_count == 0 {
            return Err("No vCPUs configured");
        }

        self.state = VmState::Running;

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
        for vcpu in self.vcpus.iter_mut().flatten() {
            vcpu.stop();
        }

        self.state = VmState::Stopped;
    }

    // ========== Scheduler Integration ==========

    /// Schedule the next vCPU to run
    pub fn schedule(&mut self) -> Option<usize> {
        self.scheduler.pick_next()
    }

    /// Run the currently scheduled vCPU
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

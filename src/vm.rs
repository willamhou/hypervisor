//! Virtual Machine Management
//! 
//! This module provides the VM abstraction that contains one or more vCPUs
//! and manages guest resources including memory.

use crate::vcpu::Vcpu;
use crate::arch::aarch64::mmu::{MemoryAttributes, init_stage2};
use crate::devices::DeviceManager;

/// Maximum number of vCPUs per VM
pub const MAX_VCPUS: usize = 8;

/// Virtual Machine State
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM is not initialized
    Uninitialized,
    
    /// VM is initialized and ready to run
    Ready,
    
    /// VM is currently running
    Running,
    
    /// VM is paused
    Paused,
    
    /// VM is stopped
    Stopped,
}

/// Virtual Machine
/// 
/// Represents a complete virtual machine with one or more vCPUs and memory.
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
    devices: DeviceManager,
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
        use crate::arch::aarch64::mmu::IdentityMapper;
        
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
            // UART (PL011): 0x09000000 - 0x09001000 (4KB)
            let uart_base = 0x09000000u64;
            let uart_size = 2 * 1024 * 1024;  // 2MB block
            MAPPER.map_region(uart_base, uart_size, MemoryAttributes::DEVICE);
            
            // GIC Distributor: 0x08000000 - 0x08010000 (64KB)
            let gicd_base = 0x08000000u64;
            let gicd_size = 2 * 1024 * 1024;  // 2MB block
            MAPPER.map_region(gicd_base, gicd_size, MemoryAttributes::DEVICE);
            
            // Initialize Stage-2 translation
            init_stage2(&MAPPER);
        }
        
        self.memory_initialized = true;
        uart_puts(b"[VM] Memory mapping complete\n");
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

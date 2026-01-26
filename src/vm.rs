//! Virtual Machine Management
//! 
//! This module provides the VM abstraction that contains one or more vCPUs
//! and manages guest resources.

use crate::vcpu::Vcpu;

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
/// Represents a complete virtual machine with one or more vCPUs.
pub struct Vm {
    /// Unique identifier for this VM
    id: usize,
    
    /// Current state of the VM
    state: VmState,
    
    /// vCPUs belonging to this VM
    vcpus: [Option<Vcpu>; MAX_VCPUS],
    
    /// Number of active vCPUs
    vcpu_count: usize,
}

impl Vm {
    /// Create a new VM
    /// 
    /// # Arguments
    /// * `id` - Unique identifier for this VM
    pub fn new(id: usize) -> Self {
        const INIT: Option<Vcpu> = None;
        Self {
            id,
            state: VmState::Uninitialized,
            vcpus: [INIT; MAX_VCPUS],
            vcpu_count: 0,
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

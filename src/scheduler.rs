//! Simple round-robin vCPU scheduler

use crate::vm::MAX_VCPUS;

/// Run state for a vCPU in the scheduler
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RunState {
    /// vCPU is not registered
    None,
    /// vCPU is ready to run
    Ready,
    /// vCPU is currently running
    Running,
    /// vCPU is blocked (e.g., waiting for I/O)
    Blocked,
}

/// Simple round-robin scheduler for vCPUs
pub struct Scheduler {
    /// Run state for each vCPU slot
    states: [RunState; MAX_VCPUS],
    /// Currently running vCPU (if any)
    current: Option<usize>,
    /// Next index to check in round-robin
    next_idx: usize,
}

impl Scheduler {
    /// Create a new scheduler
    pub const fn new() -> Self {
        Self {
            states: [RunState::None; MAX_VCPUS],
            current: None,
            next_idx: 0,
        }
    }

    /// Add a vCPU to the scheduler
    pub fn add_vcpu(&mut self, vcpu_id: usize) {
        if vcpu_id < MAX_VCPUS {
            self.states[vcpu_id] = RunState::Ready;
        }
    }

    /// Remove a vCPU from the scheduler
    pub fn remove_vcpu(&mut self, vcpu_id: usize) {
        if vcpu_id < MAX_VCPUS {
            self.states[vcpu_id] = RunState::None;
            if self.current == Some(vcpu_id) {
                self.current = None;
            }
        }
    }

    /// Pick the next vCPU to run (round-robin)
    ///
    /// If a vCPU is already running, returns it.
    /// Otherwise, finds the next ready vCPU starting from next_idx.
    pub fn pick_next(&mut self) -> Option<usize> {
        // If current is still running, return it
        if let Some(id) = self.current {
            if self.states[id] == RunState::Running {
                return self.current;
            }
        }

        // Find next ready vCPU
        for i in 0..MAX_VCPUS {
            let idx = (self.next_idx + i) % MAX_VCPUS;
            if self.states[idx] == RunState::Ready {
                self.current = Some(idx);
                self.states[idx] = RunState::Running;
                return Some(idx);
            }
        }

        None
    }

    /// Yield the current vCPU (put back in ready queue)
    pub fn yield_current(&mut self) {
        if let Some(id) = self.current {
            self.states[id] = RunState::Ready;
            self.current = None;
            self.next_idx = (id + 1) % MAX_VCPUS;
        }
    }

    /// Block the current vCPU (e.g., waiting for I/O)
    pub fn block_current(&mut self) {
        if let Some(id) = self.current {
            self.states[id] = RunState::Blocked;
            self.current = None;
            self.next_idx = (id + 1) % MAX_VCPUS;
        }
    }

    /// Unblock a vCPU (make it ready again)
    pub fn unblock(&mut self, vcpu_id: usize) {
        if vcpu_id < MAX_VCPUS && self.states[vcpu_id] == RunState::Blocked {
            self.states[vcpu_id] = RunState::Ready;
        }
    }

    /// Get the currently running vCPU (if any)
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Get the run state of a vCPU
    pub fn state(&self, vcpu_id: usize) -> RunState {
        if vcpu_id < MAX_VCPUS {
            self.states[vcpu_id]
        } else {
            RunState::None
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

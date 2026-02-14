use crate::platform::SMP_CPUS;

pub struct PerCpuContext {
    pub vcpu_id: usize,
    pub exception_count: u32,
}

static mut PER_CPU: [PerCpuContext; SMP_CPUS] = {
    const INIT: PerCpuContext = PerCpuContext {
        vcpu_id: 0,
        exception_count: 0,
    };
    [INIT; SMP_CPUS]
};

/// Read current physical CPU ID from MPIDR_EL1.Aff0
#[inline(always)]
pub fn current_cpu_id() -> usize {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr) };
    (mpidr & 0xFF) as usize
}

/// Get per-CPU context for current pCPU (mutable).
/// SAFETY: Each pCPU only accesses its own entry — no data races possible
/// with fixed vCPU-to-pCPU affinity.
pub fn this_cpu() -> &'static mut PerCpuContext {
    let id = current_cpu_id();
    unsafe { &mut PER_CPU[id] }
}

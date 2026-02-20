use crate::platform::MAX_SMP_CPUS;
use core::cell::UnsafeCell;

pub struct PerCpuContext {
    pub vcpu_id: usize,
    pub exception_count: u32,
}

/// Wrapper for per-CPU array with interior mutability.
/// SAFETY: Each pCPU only accesses its own entry (indexed by MPIDR.Aff0),
/// so no data races occur with fixed vCPU-to-pCPU affinity.
struct PerCpuArray(UnsafeCell<[PerCpuContext; MAX_SMP_CPUS]>);
unsafe impl Sync for PerCpuArray {}

static PER_CPU: PerCpuArray = PerCpuArray(UnsafeCell::new({
    const INIT: PerCpuContext = PerCpuContext {
        vcpu_id: 0,
        exception_count: 0,
    };
    [INIT; MAX_SMP_CPUS]
}));

/// Read current physical CPU ID from MPIDR_EL1.Aff0
#[inline(always)]
pub fn current_cpu_id() -> usize {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr) };
    (mpidr & 0xFF) as usize
}

/// Get per-CPU context for current pCPU.
///
/// Returns a raw pointer to avoid creating multiple `&'static mut` references
/// (which would be UB under Rust aliasing rules). Callers should dereference
/// the pointer locally and not hold long-lived references.
///
/// SAFETY: Each pCPU only accesses its own entry — no data races with 1:1 affinity.
#[inline]
pub fn this_cpu() -> *mut PerCpuContext {
    let id = current_cpu_id();
    unsafe { &raw mut (*PER_CPU.0.get())[id] }
}

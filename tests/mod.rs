///! Test module for hypervisor
///!
///! This module contains various integration tests for the hypervisor.

pub mod test_guest;
pub mod test_guest_irq;
pub mod test_timer;
pub mod test_mmio;
pub mod test_guest_interrupt;
pub mod test_complete_interrupt;
pub mod test_gicv3_virt;
pub mod test_allocator;
pub mod test_heap;
pub mod test_dynamic_pagetable;
pub mod test_multi_vcpu;
pub mod test_scheduler;
pub mod test_vm_scheduler;
pub mod test_guest_loader;
pub mod test_simple_guest;

// Re-export test functions for easy access
pub use test_guest::run_test as run_guest_test;
#[allow(unused_imports)]
pub use test_timer::run_timer_test;
pub use test_mmio::run_mmio_test;
pub use test_guest_interrupt::run_guest_interrupt_test;
pub use test_complete_interrupt::run_complete_interrupt_test;
pub use test_gicv3_virt::run_gicv3_virt_test;
pub use test_allocator::run_allocator_test;
pub use test_heap::run_heap_test;
pub use test_dynamic_pagetable::run_dynamic_pt_test;
pub use test_multi_vcpu::run_multi_vcpu_test;
pub use test_scheduler::run_scheduler_test;
pub use test_vm_scheduler::run_vm_scheduler_test;
pub use test_guest_loader::run_test as run_guest_loader_test;
pub use test_simple_guest::run_test as run_simple_guest_test;

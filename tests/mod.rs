///! Test module for hypervisor
///! 
///! This module contains various integration tests for the hypervisor.

pub mod test_guest;
pub mod test_guest_irq;
pub mod test_timer;
pub mod test_mmio;
pub mod test_guest_interrupt;
pub mod test_complete_interrupt;

// Re-export test functions for easy access
pub use test_guest::run_test as run_guest_test;
#[allow(unused_imports)]
pub use test_timer::run_timer_test;
pub use test_mmio::run_mmio_test;
pub use test_guest_interrupt::run_guest_interrupt_test;
pub use test_complete_interrupt::run_complete_interrupt_test;

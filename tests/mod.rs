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
pub mod test_decode;
pub mod test_gicd;
pub mod test_gicr;
pub mod test_global;
pub mod test_device_routing;
pub mod test_vm_state_isolation;
pub mod test_vmid_vttbr;
pub mod test_multi_vm_devices;
pub mod test_vm_activate;
pub mod test_dtb;
pub mod test_net_rx_ring;
pub mod test_vswitch;

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
pub use test_decode::run_decode_test;
pub use test_gicd::run_gicd_test;
pub use test_gicr::run_gicr_test;
pub use test_global::run_global_test;
pub use test_guest_irq::run_irq_test;
pub use test_device_routing::run_device_routing_test;
pub use test_vm_state_isolation::run_vm_state_isolation_test;
pub use test_vmid_vttbr::run_vmid_vttbr_test;
pub use test_multi_vm_devices::run_multi_vm_devices_test;
pub use test_vm_activate::run_vm_activate_test;
pub use test_dtb::run_dtb_test;
pub use test_net_rx_ring::run_net_rx_ring_test;
pub use test_vswitch::run_vswitch_test;

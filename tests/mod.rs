pub mod test_allocator;
pub mod test_complete_interrupt;
pub mod test_decode;
pub mod test_device_routing;
pub mod test_dtb;
pub mod test_dynamic_pagetable;
pub mod test_ffa;
pub mod test_gicd;
pub mod test_gicr;
pub mod test_gicv3_virt;
pub mod test_global;
// Test module for hypervisor
//
// This module contains various integration tests for the hypervisor.
pub mod test_guest;
pub mod test_guest_interrupt;
pub mod test_guest_irq;
pub mod test_guest_loader;
pub mod test_heap;
pub mod test_log;
pub mod test_mmio;
pub mod test_multi_vcpu;
pub mod test_multi_vm_devices;
pub mod test_net_rx_ring;
pub mod test_page_ownership;
pub mod test_pl031;
pub mod test_scheduler;
pub mod test_secure_stage2;
pub mod test_simple_guest;
pub mod test_sp_context;
pub mod test_spmc_handler;
pub mod test_timer;
pub mod test_virtio_net;
pub mod test_vm_activate;
pub mod test_vm_scheduler;
pub mod test_vm_state_isolation;
pub mod test_vmid_vttbr;
pub mod test_vswitch;

fn run_one(name: &str, f: fn()) {
    hypervisor::log_info!("[TEST REGISTRY] START: {}\n", name);
    f();
    hypervisor::log_info!("[TEST REGISTRY] END:   {}\n", name);
}

macro_rules! register_tests {
    ($( $(#[$meta:meta])* ($name:expr, $func:path) ),+ $(,)?) => {
        /// Central test registry. Add new tests here; `main.rs` should not require edits.
        pub fn run_all_tests() {
            $(
                $(#[$meta])*
                run_one($name, $func);
            )+
        }
    };
}

register_tests!(
    ("dtb", test_dtb::run_dtb_test),
    ("allocator", test_allocator::run_allocator_test),
    ("heap", test_heap::run_heap_test),
    (
        "dynamic_pagetable",
        test_dynamic_pagetable::run_dynamic_pt_test
    ),
    ("multi_vcpu", test_multi_vcpu::run_multi_vcpu_test),
    ("scheduler", test_scheduler::run_scheduler_test),
    ("vm_scheduler", test_vm_scheduler::run_vm_scheduler_test),
    ("mmio", test_mmio::run_mmio_test),
    ("gicv3_virt", test_gicv3_virt::run_gicv3_virt_test),
    (
        "complete_interrupt",
        test_complete_interrupt::run_complete_interrupt_test
    ),
    ("guest", test_guest::run_test),
    ("guest_loader", test_guest_loader::run_test),
    ("simple_guest", test_simple_guest::run_test),
    ("decode", test_decode::run_decode_test),
    ("gicd", test_gicd::run_gicd_test),
    ("gicr", test_gicr::run_gicr_test),
    ("global", test_global::run_global_test),
    ("guest_irq", test_guest_irq::run_irq_test),
    (
        "device_routing",
        test_device_routing::run_device_routing_test
    ),
    (
        "vm_state_isolation",
        test_vm_state_isolation::run_vm_state_isolation_test
    ),
    ("vmid_vttbr", test_vmid_vttbr::run_vmid_vttbr_test),
    (
        "multi_vm_devices",
        test_multi_vm_devices::run_multi_vm_devices_test
    ),
    ("vm_activate", test_vm_activate::run_vm_activate_test),
    ("net_rx_ring", test_net_rx_ring::run_net_rx_ring_test),
    ("vswitch", test_vswitch::run_vswitch_test),
    ("virtio_net", test_virtio_net::run_virtio_net_test),
    (
        "page_ownership",
        test_page_ownership::run_page_ownership_test
    ),
    ("pl031", test_pl031::run_pl031_test),
    ("ffa", test_ffa::run_ffa_test),
    ("spmc_handler", test_spmc_handler::run_tests),
    ("sp_context", test_sp_context::run_tests),
    ("secure_stage2", test_secure_stage2::run_tests),
    ("log", test_log::run_log_test),
    // Must stay last before optional guest boots; this test does not return.
    #[cfg(not(any(feature = "linux_guest", feature = "guest")))]
    (
        "guest_interrupt",
        test_guest_interrupt::run_guest_interrupt_test
    ),
);

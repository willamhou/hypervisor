#![no_std]

pub mod uart;

// Note: println! macro is exported at the crate root via #[macro_export]
// It can be used as: use hypervisor::println;

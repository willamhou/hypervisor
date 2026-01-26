#![no_std]

pub mod uart;

// Re-export commonly used items
pub use uart::println;

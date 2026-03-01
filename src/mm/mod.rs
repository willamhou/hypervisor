//! Memory management subsystem

pub mod allocator;
pub mod heap;
pub mod region_registry;

pub use allocator::BumpAllocator;

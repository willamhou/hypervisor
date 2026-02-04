//! Memory management subsystem

pub mod allocator;
pub mod heap;

pub use allocator::BumpAllocator;

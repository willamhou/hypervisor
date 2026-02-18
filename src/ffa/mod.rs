//! FF-A v1.1 Proxy Framework
//!
//! Implements a pKVM-compatible FF-A proxy at EL2. Traps guest SMC calls,
//! validates memory ownership via Stage-2 PTE SW bits, and forwards to
//! a stub SPMC (replaceable with real Secure World later).

pub mod proxy;

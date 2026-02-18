//! Stub SPMC — simulates Secure World responses for testing.
//! Replace with real SMC forwarding when integrating TF-A + Hafnium.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, Ordering};

/// Simulated secure partition info.
pub struct StubPartition {
    pub id: u16,
    #[allow(dead_code)]
    pub uuid: [u32; 4],
    pub exec_ctx_count: u16,
    pub properties: u32,
}

/// Two simulated SPs for testing.
pub static STUB_PARTITIONS: [StubPartition; 2] = [
    StubPartition {
        id: 0x8001,
        uuid: [0x12345678, 0x9ABC_DEF0, 0x1111_2222, 0x3333_4444],
        exec_ctx_count: 1,
        properties: 1, // Supports direct messaging
    },
    StubPartition {
        id: 0x8002,
        uuid: [0x87654321, 0x0FED_CBA9, 0x5555_6666, 0x7777_8888],
        exec_ctx_count: 1,
        properties: 1, // Supports direct messaging
    },
];

/// Handle count for memory sharing.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Memory share record.
pub struct MemShareRecord {
    pub handle: u64,
    pub sender_id: u16,
    #[allow(dead_code)]
    pub receiver_id: u16,
    #[allow(dead_code)]
    pub page_count: u32,
    pub active: bool,
}

/// Fixed-size array of share records (no alloc).
///
/// Uses UnsafeCell for interior mutability. Access is safe: in single-pCPU modes,
/// only one exception handler runs at a time. In multi-pCPU mode, share records
/// are accessed under the FF-A proxy dispatch (one SMC at a time per VM).
const MAX_SHARES: usize = 16;
struct ShareRecordArray(UnsafeCell<[MemShareRecord; MAX_SHARES]>);
unsafe impl Sync for ShareRecordArray {}

static SHARE_RECORDS: ShareRecordArray = ShareRecordArray(UnsafeCell::new({
    const EMPTY: MemShareRecord = MemShareRecord {
        handle: 0, sender_id: 0, receiver_id: 0, page_count: 0, active: false,
    };
    // Can't use [EMPTY; MAX_SHARES] because MemShareRecord doesn't impl Copy
    // but we can use const array init
    [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
     EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY]
}));

/// Allocate a new memory sharing handle.
pub fn alloc_handle() -> u64 {
    NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

/// Record a memory share and return the handle.
pub fn record_share(sender_id: u16, receiver_id: u16, page_count: u32) -> Option<u64> {
    let handle = alloc_handle();
    let records = unsafe { &mut *SHARE_RECORDS.0.get() };
    for record in records.iter_mut() {
        if !record.active {
            *record = MemShareRecord {
                handle, sender_id, receiver_id, page_count, active: true,
            };
            return Some(handle);
        }
    }
    None // No free slots
}

/// Reclaim a memory share by handle. Returns true if found and removed.
pub fn reclaim_share(handle: u64) -> bool {
    let records = unsafe { &mut *SHARE_RECORDS.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle {
            record.active = false;
            return true;
        }
    }
    false
}

/// Check if a partition ID is a known stub SP.
pub fn is_valid_sp(part_id: u16) -> bool {
    STUB_PARTITIONS.iter().any(|sp| sp.id == part_id)
}

/// Get partition count.
pub fn partition_count() -> usize {
    STUB_PARTITIONS.len()
}

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

/// Maximum address ranges per share record.
pub const MAX_SHARE_RANGES: usize = 4;

/// Memory share record.
pub struct MemShareRecord {
    pub handle: u64,
    pub sender_id: u16,
    pub receiver_id: u16,
    /// Address ranges: (base_ipa, page_count) per range.
    pub ranges: [(u64, u32); MAX_SHARE_RANGES],
    pub range_count: usize,
    pub total_page_count: u32,
    pub active: bool,
    /// True for MEM_LEND (S2AP=NONE), false for MEM_SHARE (S2AP=RO).
    pub is_lend: bool,
    /// Whether receiver has called FFA_MEM_RETRIEVE_REQ.
    pub retrieved: bool,
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
        handle: 0,
        sender_id: 0,
        receiver_id: 0,
        ranges: [(0, 0); MAX_SHARE_RANGES],
        range_count: 0,
        total_page_count: 0,
        active: false,
        is_lend: false,
        retrieved: false,
    };
    [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
     EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY]
}));

/// Allocate a new memory sharing handle.
pub fn alloc_handle() -> u64 {
    NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

/// Record a memory share and return the handle.
pub fn record_share(
    sender_id: u16,
    receiver_id: u16,
    ranges: &[(u64, u32)],
    total_page_count: u32,
    is_lend: bool,
) -> Option<u64> {
    let handle = alloc_handle();
    let records = unsafe { &mut *SHARE_RECORDS.0.get() };
    for record in records.iter_mut() {
        if !record.active {
            let mut stored_ranges = [(0u64, 0u32); MAX_SHARE_RANGES];
            let count = ranges.len().min(MAX_SHARE_RANGES);
            for (i, &r) in ranges.iter().take(count).enumerate() {
                stored_ranges[i] = r;
            }
            *record = MemShareRecord {
                handle,
                sender_id,
                receiver_id,
                ranges: stored_ranges,
                range_count: count,
                total_page_count,
                active: true,
                is_lend,
                retrieved: false,
            };
            return Some(handle);
        }
    }
    None // No free slots
}

/// Share record info returned by lookup.
pub struct ShareInfo {
    pub ranges: [(u64, u32); MAX_SHARE_RANGES],
    pub range_count: usize,
    pub total_page_count: u32,
    pub is_lend: bool,
}

/// Look up a share record by handle. Returns range info for reclaim.
pub fn lookup_share(handle: u64) -> Option<ShareInfo> {
    let records = unsafe { &*SHARE_RECORDS.0.get() };
    for record in records.iter() {
        if record.active && record.handle == handle {
            return Some(ShareInfo {
                ranges: record.ranges,
                range_count: record.range_count,
                total_page_count: record.total_page_count,
                is_lend: record.is_lend,
            });
        }
    }
    None
}

/// Extended share record info (includes sender/receiver/retrieved state).
pub struct ShareInfoFull {
    pub sender_id: u16,
    pub receiver_id: u16,
    pub ranges: [(u64, u32); MAX_SHARE_RANGES],
    pub range_count: usize,
    pub total_page_count: u32,
    pub is_lend: bool,
    pub retrieved: bool,
}

/// Look up a share record by handle, returning full info including sender/receiver.
pub fn lookup_share_full(handle: u64) -> Option<ShareInfoFull> {
    let records = unsafe { &*SHARE_RECORDS.0.get() };
    for record in records.iter() {
        if record.active && record.handle == handle {
            return Some(ShareInfoFull {
                sender_id: record.sender_id,
                receiver_id: record.receiver_id,
                ranges: record.ranges,
                range_count: record.range_count,
                total_page_count: record.total_page_count,
                is_lend: record.is_lend,
                retrieved: record.retrieved,
            });
        }
    }
    None
}

/// Mark a share as retrieved. Returns true if found and was not already retrieved.
pub fn mark_retrieved(handle: u64) -> bool {
    let records = unsafe { &mut *SHARE_RECORDS.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle && !record.retrieved {
            record.retrieved = true;
            return true;
        }
    }
    false
}

/// Mark a share as relinquished (not retrieved). Returns true if found and was retrieved.
pub fn mark_relinquished(handle: u64) -> bool {
    let records = unsafe { &mut *SHARE_RECORDS.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle && record.retrieved {
            record.retrieved = false;
            return true;
        }
    }
    false
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

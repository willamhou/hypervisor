//! FF-A v1.1 Proxy Framework
//!
//! Implements a pKVM-compatible FF-A proxy at EL2. Traps guest SMC calls,
//! validates memory ownership via Stage-2 PTE SW bits, and forwards to
//! a stub SPMC (replaceable with real Secure World later).

pub mod descriptors;
pub mod mailbox;
pub mod memory;
pub mod notifications;
pub mod proxy;
pub mod smc_forward;
pub mod stage2_walker;
pub mod stub_spmc;

// ── FF-A Function IDs (SMC32) ─────────────────────────────────────
pub const FFA_ERROR: u64 = 0x84000060;
pub const FFA_SUCCESS_32: u64 = 0x84000061;
pub const FFA_VERSION: u64 = 0x84000063;
pub const FFA_FEATURES: u64 = 0x84000064;
pub const FFA_RX_RELEASE: u64 = 0x84000065;
pub const FFA_RXTX_UNMAP: u64 = 0x84000067;
pub const FFA_PARTITION_INFO_GET: u64 = 0x84000068;
pub const FFA_ID_GET: u64 = 0x84000069;
pub const FFA_MSG_SEND_DIRECT_REQ_32: u64 = 0x8400006F;
pub const FFA_MSG_SEND_DIRECT_RESP_32: u64 = 0x84000070;
pub const FFA_MEM_DONATE_32: u64 = 0x84000071;
pub const FFA_MEM_LEND_32: u64 = 0x84000072;
pub const FFA_MEM_SHARE_32: u64 = 0x84000073;
pub const FFA_MEM_RETRIEVE_REQ_32: u64 = 0x84000074;
pub const FFA_MEM_RETRIEVE_RESP: u64 = 0x84000075;
pub const FFA_MEM_RELINQUISH: u64 = 0x84000076;
pub const FFA_MEM_RECLAIM: u64 = 0x84000077;
#[allow(dead_code)]
pub const FFA_MEM_FRAG_RX: u64 = 0x8400007A;
#[allow(dead_code)]
pub const FFA_MEM_FRAG_TX: u64 = 0x8400007B;
pub const FFA_INTERRUPT: u64 = 0x84000062;
pub const FFA_NOTIFICATION_BITMAP_CREATE: u64 = 0x8400007D;
pub const FFA_NOTIFICATION_BITMAP_DESTROY: u64 = 0x8400007E;
pub const FFA_NOTIFICATION_BIND: u64 = 0x8400007F;
pub const FFA_NOTIFICATION_UNBIND: u64 = 0x84000080;
pub const FFA_NOTIFICATION_SET: u64 = 0x84000081;
pub const FFA_NOTIFICATION_GET: u64 = 0x84000082;
pub const FFA_NOTIFICATION_INFO_GET_32: u64 = 0x84000083;
pub const FFA_SPM_ID_GET: u64 = 0x84000085;
pub const FFA_MSG_SEND2: u64 = 0x84000086;
pub const FFA_MSG_WAIT: u64 = 0x8400006B;
pub const FFA_RUN: u64 = 0x8400006D;

// ── FF-A Function IDs (SMC64) ─────────────────────────────────────
#[allow(dead_code)]
pub const FFA_SUCCESS_64: u64 = 0xC4000061;
pub const FFA_RXTX_MAP: u64 = 0xC4000066;
pub const FFA_MSG_SEND_DIRECT_REQ_64: u64 = 0xC400006F;
pub const FFA_MSG_SEND_DIRECT_RESP_64: u64 = 0xC4000070;
pub const FFA_MEM_DONATE_64: u64 = 0xC4000071;
pub const FFA_MEM_LEND_64: u64 = 0xC4000072;
pub const FFA_MEM_SHARE_64: u64 = 0xC4000073;
pub const FFA_MEM_RETRIEVE_REQ_64: u64 = 0xC4000074;
pub const FFA_NOTIFICATION_INFO_GET_64: u64 = 0xC4000083;

// ── FF-A Version ──────────────────────────────────────────────────
pub const FFA_VERSION_1_1: u32 = 0x00010001; // Major=1, Minor=1

// ── FF-A Error Codes (returned in x2 with FFA_ERROR in x0) ───────
pub const FFA_NOT_SUPPORTED: i32 = -1;
pub const FFA_INVALID_PARAMETERS: i32 = -2;
pub const FFA_NO_MEMORY: i32 = -3;
pub const FFA_BUSY: i32 = -4;
pub const FFA_DENIED: i32 = -6;
#[allow(dead_code)]
pub const FFA_ABORTED: i32 = -7;
#[allow(dead_code)]
pub const FFA_NO_DATA: i32 = -8;

// ── Partition IDs ─────────────────────────────────────────────────
#[allow(dead_code)]
pub const FFA_HOST_ID: u16 = 0x0000;
#[allow(dead_code)]
pub const FFA_SPMC_ID: u16 = 0x8000;

// ── SPMD Framework Message constants ──────────────────────────────
// SPMD wraps certain FF-A calls (e.g. FFA_VERSION) as DIRECT_REQ
// framework messages with FFA_FWK_MSG_BIT set in x2.
pub const FFA_FWK_MSG_BIT: u64 = 1 << 31;
pub const SPMD_FWK_MSG_FFA_VERSION_REQ: u64 = 0x8;
pub const SPMD_FWK_MSG_FFA_VERSION_RESP: u64 = 0x9;

/// Maximum number of VMs that can have FF-A partition IDs.
/// VM 0 → partition ID 1, VM 1 → partition ID 2.
pub const FFA_MAX_VMS: usize = 4;

/// Convert a VM ID to an FF-A partition ID.
pub fn vm_id_to_partition_id(vm_id: usize) -> u16 {
    (vm_id + 1) as u16
}

/// Convert an FF-A partition ID to a VM ID. Returns None for non-VM IDs.
pub fn partition_id_to_vm_id(part_id: u16) -> Option<usize> {
    if part_id >= 1 && (part_id as usize) <= FFA_MAX_VMS {
        Some((part_id - 1) as usize)
    } else {
        None
    }
}

/// Check if a partition ID is a valid VM (non-SP) partition.
pub fn is_vm_partition(part_id: u16) -> bool {
    partition_id_to_vm_id(part_id).is_some()
}

/// Check if a partition ID is a valid receiver (VM or SP).
pub fn is_valid_receiver(part_id: u16) -> bool {
    is_vm_partition(part_id) || stub_spmc::is_valid_sp(part_id)
}

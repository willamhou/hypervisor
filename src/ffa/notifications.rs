//! FF-A v1.1 Notification state management.
//!
//! Per-endpoint notification bitmaps with sender-receiver bind tracking.
//! Follows the same global state pattern as `mailbox.rs`.

use crate::ffa::{self, FFA_DENIED, FFA_INVALID_PARAMETERS, FFA_MAX_VMS};
use core::cell::UnsafeCell;

/// Maximum number of endpoints (VMs + SPs) with notification support.
const MAX_ENDPOINTS: usize = 8;

/// Maximum number of bind records per endpoint.
const MAX_BINDS: usize = 8;

/// A sender-to-notification binding record.
struct NotifBind {
    sender_id: u16,
    bitmap: u64,
    active: bool,
}

impl NotifBind {
    const fn new() -> Self {
        Self {
            sender_id: 0,
            bitmap: 0,
            active: false,
        }
    }
}

/// Per-endpoint notification state.
struct EndpointNotifState {
    enabled: bool,
    pending: u64,
    binds: [NotifBind; MAX_BINDS],
}

impl EndpointNotifState {
    const fn new() -> Self {
        Self {
            enabled: false,
            pending: 0,
            binds: [
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
                NotifBind::new(),
            ],
        }
    }
}

struct NotifStateArray(UnsafeCell<[EndpointNotifState; MAX_ENDPOINTS]>);
unsafe impl Sync for NotifStateArray {}

static NOTIF_STATE: NotifStateArray = NotifStateArray(UnsafeCell::new([
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
    EndpointNotifState::new(),
]));

/// Map partition ID to endpoint index.
/// VMs: partition ID 1..=MAX_VMS → index 0..MAX_VMS-1
/// SPs: 0x8001 → FFA_MAX_VMS, 0x8002 → FFA_MAX_VMS+1
fn endpoint_index(part_id: u16) -> Option<usize> {
    if let Some(vm_id) = ffa::partition_id_to_vm_id(part_id) {
        Some(vm_id)
    } else if part_id == 0x8001 {
        Some(FFA_MAX_VMS)
    } else if part_id == 0x8002 {
        Some(FFA_MAX_VMS + 1)
    } else {
        None
    }
}

fn get_state(part_id: u16) -> Result<&'static mut EndpointNotifState, i32> {
    let idx = endpoint_index(part_id).ok_or(FFA_INVALID_PARAMETERS)?;
    Ok(unsafe { &mut (*NOTIF_STATE.0.get())[idx] })
}

/// Create notification bitmap for an endpoint.
pub fn bitmap_create(part_id: u16) -> Result<(), i32> {
    let state = get_state(part_id)?;
    if state.enabled {
        return Err(FFA_DENIED);
    }
    state.enabled = true;
    Ok(())
}

/// Destroy notification bitmap for an endpoint.
pub fn bitmap_destroy(part_id: u16) -> Result<(), i32> {
    let state = get_state(part_id)?;
    if !state.enabled {
        return Err(FFA_DENIED);
    }
    state.enabled = false;
    state.pending = 0;
    for bind in state.binds.iter_mut() {
        *bind = NotifBind::new();
    }
    Ok(())
}

/// Bind a sender to notification IDs on a receiver.
pub fn bind(sender: u16, receiver: u16, _flags: u32, bitmap: u64) -> Result<(), i32> {
    if bitmap == 0 {
        return Err(FFA_INVALID_PARAMETERS);
    }
    let state = get_state(receiver)?;
    if !state.enabled {
        return Err(FFA_DENIED);
    }

    // Check for overlap with existing binds
    for bind in state.binds.iter() {
        if bind.active && bind.bitmap & bitmap != 0 {
            return Err(FFA_DENIED);
        }
    }

    // Find a free slot
    for bind in state.binds.iter_mut() {
        if !bind.active {
            bind.active = true;
            bind.sender_id = sender;
            bind.bitmap = bitmap;
            return Ok(());
        }
    }

    Err(ffa::FFA_NO_MEMORY)
}

/// Unbind a sender from notification IDs on a receiver.
pub fn unbind(sender: u16, receiver: u16, bitmap: u64) -> Result<(), i32> {
    let state = get_state(receiver)?;

    for bind in state.binds.iter_mut() {
        if bind.active && bind.sender_id == sender && bind.bitmap & bitmap != 0 {
            bind.bitmap &= !bitmap;
            if bind.bitmap == 0 {
                bind.active = false;
            }
            return Ok(());
        }
    }

    Err(FFA_DENIED)
}

/// Set pending notification bits on a receiver.
pub fn set(sender: u16, receiver: u16, bitmap: u64) -> Result<(), i32> {
    let state = get_state(receiver)?;
    if !state.enabled {
        return Err(FFA_DENIED);
    }

    // Validate sender has bound these notification IDs
    let mut allowed: u64 = 0;
    for bind in state.binds.iter() {
        if bind.active && bind.sender_id == sender {
            allowed |= bind.bitmap;
        }
    }
    if bitmap & !allowed != 0 {
        return Err(FFA_DENIED);
    }

    state.pending |= bitmap;
    Ok(())
}

/// Get and clear pending notification bits for a receiver.
pub fn get(receiver: u16) -> Result<u64, i32> {
    let state = get_state(receiver)?;
    if !state.enabled {
        return Err(FFA_DENIED);
    }
    let pending = state.pending;
    state.pending = 0;
    Ok(pending)
}

/// Get list of partitions with pending notifications.
/// Returns (count, array of partition IDs with pending).
pub fn info_get() -> (usize, [u16; 4]) {
    let mut ids = [0u16; 4];
    let mut count = 0usize;
    let states = unsafe { &*NOTIF_STATE.0.get() };

    // Scan VMs
    for vm_id in 0..FFA_MAX_VMS {
        if count >= 4 {
            break;
        }
        if states[vm_id].enabled && states[vm_id].pending != 0 {
            ids[count] = ffa::vm_id_to_partition_id(vm_id);
            count += 1;
        }
    }

    // Scan SPs
    for sp_idx in 0..2usize {
        if count >= 4 {
            break;
        }
        let idx = FFA_MAX_VMS + sp_idx;
        if states[idx].enabled && states[idx].pending != 0 {
            ids[count] = 0x8001 + sp_idx as u16;
            count += 1;
        }
    }

    (count, ids)
}

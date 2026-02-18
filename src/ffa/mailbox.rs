//! FF-A RXTX Mailbox management — per-VM TX/RX buffer tracking.

use core::cell::UnsafeCell;
use crate::ffa::FFA_MAX_VMS;

/// Per-VM RXTX buffer state.
pub struct FfaMailbox {
    /// Guest TX buffer IPA (guest writes, proxy reads)
    pub tx_ipa: u64,
    /// Guest RX buffer IPA (proxy writes, guest reads)
    pub rx_ipa: u64,
    /// Buffer size in pages (typically 1)
    pub page_count: u32,
    /// Whether buffers are registered
    pub mapped: bool,
    /// RX buffer ownership: true = proxy owns (can write), false = VM owns
    pub rx_held_by_proxy: bool,
}

impl FfaMailbox {
    pub const fn new() -> Self {
        Self {
            tx_ipa: 0,
            rx_ipa: 0,
            page_count: 0,
            mapped: false,
            rx_held_by_proxy: true,
        }
    }
}

/// Global per-VM mailbox state.
///
/// Access is safe: in single-pCPU modes, only one exception handler runs at a time.
/// In multi-pCPU mode, each pCPU handles its own VM's mailbox (no cross-VM access).
struct MailboxArray(UnsafeCell<[FfaMailbox; FFA_MAX_VMS]>);
unsafe impl Sync for MailboxArray {}

static MAILBOXES: MailboxArray = MailboxArray(UnsafeCell::new([
    FfaMailbox::new(),
    FfaMailbox::new(),
    FfaMailbox::new(),
    FfaMailbox::new(),
]));

/// Get the mailbox for a VM.
///
/// # Safety
/// Single-pCPU: only one exception handler runs at a time.
/// Multi-pCPU: each pCPU handles its own VM exclusively.
pub fn get_mailbox(vm_id: usize) -> &'static mut FfaMailbox {
    assert!(vm_id < FFA_MAX_VMS);
    unsafe { &mut (*MAILBOXES.0.get())[vm_id] }
}

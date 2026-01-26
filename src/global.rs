/// Global state for hypervisor
/// 
/// This module contains global state that needs to be accessed
/// from exception handlers and other low-level code.

use core::cell::UnsafeCell;
use crate::devices::DeviceManager;

/// Global device manager
/// 
/// This is accessed from exception handlers to emulate MMIO devices.
/// Safety: Only one vCPU runs at a time, so this is effectively single-threaded.
pub struct GlobalDeviceManager {
    devices: UnsafeCell<Option<DeviceManager>>,
}

unsafe impl Sync for GlobalDeviceManager {}

impl GlobalDeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: UnsafeCell::new(None),
        }
    }
    
    /// Initialize with a device manager
    pub fn init(&self, devices: DeviceManager) {
        unsafe {
            *self.devices.get() = Some(devices);
        }
    }
    
    /// Handle MMIO access
    pub fn handle_mmio(&self, addr: u64, value: u64, size: u8, is_write: bool) -> Option<u64> {
        unsafe {
            if let Some(ref mut devices) = *self.devices.get() {
                devices.handle_mmio(addr, value, size, is_write)
            } else {
                // No device manager installed
                crate::uart_puts(b"[MMIO] No device manager!\n");
                if is_write {
                    None
                } else {
                    Some(0)
                }
            }
        }
    }
}

/// Global device manager instance
pub static DEVICES: GlobalDeviceManager = GlobalDeviceManager::new();

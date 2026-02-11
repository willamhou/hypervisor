/// Global state for hypervisor
///
/// This module contains global state that needs to be accessed
/// from exception handlers and other low-level code.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    
    /// Attach a virtio-blk device to the global device manager.
    pub fn attach_virtio_blk(&self, disk_base: u64, disk_size: u64) {
        unsafe {
            if let Some(ref mut devices) = *self.devices.get() {
                devices.attach_virtio_blk(disk_base, disk_size);
            }
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

    /// Look up SPI routing via GICD_IROUTER
    pub fn route_spi(&self, intid: u32) -> usize {
        unsafe {
            if let Some(ref devices) = *self.devices.get() {
                devices.route_spi(intid)
            } else {
                0
            }
        }
    }
}

/// Global device manager instance
pub static DEVICES: GlobalDeviceManager = GlobalDeviceManager::new();

/// Pending PSCI CPU_ON request from exception handler to run loop
pub struct PendingCpuOn {
    pub requested: AtomicBool,
    pub target_cpu: AtomicU64,
    pub entry_point: AtomicU64,
    pub context_id: AtomicU64,
}

impl PendingCpuOn {
    pub const fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            target_cpu: AtomicU64::new(0),
            entry_point: AtomicU64::new(0),
            context_id: AtomicU64::new(0),
        }
    }

    /// Signal a CPU_ON request (called from exception handler)
    pub fn request(&self, target: u64, entry: u64, ctx: u64) {
        self.target_cpu.store(target, Ordering::Relaxed);
        self.entry_point.store(entry, Ordering::Relaxed);
        self.context_id.store(ctx, Ordering::Relaxed);
        // Release fence: ensure target/entry/ctx are visible before requested flag
        self.requested.store(true, Ordering::Release);
    }

    /// Take a pending CPU_ON request (called from run loop)
    pub fn take(&self) -> Option<(u64, u64, u64)> {
        // Acquire fence: if we see requested=true, target/entry/ctx are visible
        if self.requested.compare_exchange(
            true, false, Ordering::Acquire, Ordering::Relaxed,
        ).is_ok() {
            let target = self.target_cpu.load(Ordering::Relaxed);
            let entry = self.entry_point.load(Ordering::Relaxed);
            let ctx = self.context_id.load(Ordering::Relaxed);
            Some((target, entry, ctx))
        } else {
            None
        }
    }
}

/// Global pending CPU_ON request
pub static PENDING_CPU_ON: PendingCpuOn = PendingCpuOn::new();

/// Bitmask of online vCPUs (bit N = vCPU N is online)
/// vCPU 0 is online by default
pub static VCPU_ONLINE_MASK: AtomicU64 = AtomicU64::new(1);

/// Maximum number of vCPUs (must match vm::MAX_VCPUS)
pub const MAX_VCPUS: usize = 8;

/// Currently running vCPU ID (set by run_smp before each vcpu.run())
pub static CURRENT_VCPU_ID: AtomicUsize = AtomicUsize::new(0);

/// Pending SGI bitmask per vCPU (bits 0-15 = SGI 0-15)
pub static PENDING_SGIS: [AtomicU32; MAX_VCPUS] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];

/// Flag set by IRQ handler to signal preemptive vCPU exit
pub static PREEMPTION_EXIT: AtomicBool = AtomicBool::new(false);

/// Pending SPI bitmask per vCPU. Each bit represents an SPI INTID offset
/// from 32 (bit 0 = INTID 32, bit 1 = INTID 33, ..., bit 31 = INTID 63).
/// Only covers the first 32 SPIs (INTIDs 32-63), which is sufficient for
/// UART (SPI 1 = INTID 33) and virtio (SPI 16 = INTID 48).
pub static PENDING_SPIS: [AtomicU32; MAX_VCPUS] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];

/// Inject an SPI to the correct vCPU based on GICD_IROUTER.
///
/// Called from exception handler or device completion path.
/// Reads the IROUTER for the given SPI to determine the target vCPU,
/// then queues the SPI bit in PENDING_SPIS for that vCPU.
///
/// Only supports INTIDs 32-63 (first 32 SPIs).
pub fn inject_spi(intid: u32) {
    if intid < 32 || intid > 63 {
        return;
    }
    let bit = intid - 32;

    // Read IROUTER to find target vCPU
    let target = DEVICES.route_spi(intid);
    if target < MAX_VCPUS {
        PENDING_SPIS[target].fetch_or(1 << bit, Ordering::Release);
    }
}

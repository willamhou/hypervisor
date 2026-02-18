/// Global state for hypervisor
///
/// This module contains global state that needs to be accessed
/// from exception handlers and other low-level code.
///
/// Per-VM state is stored in `VM_STATE[vm_id]`. The exception handler
/// reads `CURRENT_VM_ID` to index into the correct VM's state.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use crate::devices::DeviceManager;

/// Maximum number of VMs (compile-time constant)
pub const MAX_VMS: usize = 2;

/// Maximum number of vCPUs per VM (must match vm::MAX_VCPUS)
pub const MAX_VCPUS: usize = 8;

/// Currently running VM ID (set by outer scheduler before each VM time-slice)
pub static CURRENT_VM_ID: AtomicUsize = AtomicUsize::new(0);

// ── Single-pCPU GlobalDeviceManager (UnsafeCell, no locking) ──────

#[cfg(not(feature = "multi_pcpu"))]
pub struct GlobalDeviceManager {
    devices: UnsafeCell<DeviceManager>,
    initialized: AtomicBool,
}

#[cfg(not(feature = "multi_pcpu"))]
unsafe impl Sync for GlobalDeviceManager {}

#[cfg(not(feature = "multi_pcpu"))]
impl GlobalDeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: UnsafeCell::new(DeviceManager::new()),
            initialized: AtomicBool::new(false),
        }
    }

    pub fn reset(&self) {
        unsafe { (*self.devices.get()).reset(); }
    }

    pub fn register_device(&self, dev: crate::devices::Device) {
        unsafe { (*self.devices.get()).register_device(dev); }
        self.initialized.store(true, Ordering::Relaxed);
    }

    pub fn attach_virtio_blk(&self, disk_base: u64, disk_size: u64) {
        unsafe { (*self.devices.get()).attach_virtio_blk(disk_base, disk_size); }
    }

    pub fn handle_mmio(&self, addr: u64, value: u64, size: u8, is_write: bool) -> Option<u64> {
        unsafe { (*self.devices.get()).handle_mmio(addr, value, size, is_write) }
    }

    pub fn route_spi(&self, intid: u32) -> usize {
        unsafe { (*self.devices.get()).route_spi(intid) }
    }

    pub fn uart_mut(&self) -> Option<&mut crate::devices::pl011::VirtualUart> {
        unsafe { (*self.devices.get()).uart_mut() }
    }

    pub fn attach_virtio_net(&self, vm_id: usize) {
        unsafe { (*self.devices.get()).attach_virtio_net(vm_id); }
    }

    pub fn inject_net_rx(&self, frame: &[u8]) -> bool {
        unsafe {
            if let Some(transport) = (*self.devices.get()).virtio_net_mut() {
                transport.inject_rx(frame)
            } else {
                false
            }
        }
    }
}

// ── Multi-pCPU GlobalDeviceManager (SpinLock protected) ───────────

#[cfg(feature = "multi_pcpu")]
use crate::sync::SpinLock;

#[cfg(feature = "multi_pcpu")]
use crate::devices::MmioDevice;

#[cfg(feature = "multi_pcpu")]
pub struct GlobalDeviceManager {
    devices: SpinLock<DeviceManager>,
}

#[cfg(feature = "multi_pcpu")]
impl GlobalDeviceManager {
    pub const fn new() -> Self {
        Self {
            devices: SpinLock::new(DeviceManager::new()),
        }
    }

    pub fn reset(&self) {
        self.devices.lock().reset();
    }

    pub fn register_device(&self, dev: crate::devices::Device) {
        self.devices.lock().register_device(dev);
    }

    pub fn attach_virtio_blk(&self, disk_base: u64, disk_size: u64) {
        self.devices.lock().attach_virtio_blk(disk_base, disk_size);
    }

    pub fn handle_mmio(&self, addr: u64, value: u64, size: u8, is_write: bool) -> Option<u64> {
        self.devices.lock().handle_mmio(addr, value, size, is_write)
    }

    pub fn route_spi(&self, intid: u32) -> usize {
        self.devices.lock().route_spi(intid)
    }

    /// UART RX injection — acquires the device lock.
    pub fn uart_push_rx(&self, ch: u8) {
        if let Some(uart) = self.devices.lock().uart_mut() {
            uart.push_rx(ch);
        }
    }

    /// Drain UART RX ring buffer and inject SPI 33 if needed.
    /// Single lock acquisition for the entire drain + IRQ check.
    pub fn drain_uart_rx(&self) {
        // Pop all bytes from lock-free ring first, then take one lock
        // to push them all into VirtualUart.
        let mut buf = [0u8; 64];
        let mut count = 0usize;
        while let Some(ch) = UART_RX.pop() {
            if count < buf.len() {
                buf[count] = ch;
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        let mut guard = self.devices.lock();
        if let Some(uart) = guard.uart_mut() {
            for &ch in &buf[..count] {
                uart.push_rx(ch);
            }
            if uart.pending_irq().is_some() {
                drop(guard); // Release lock before inject_spi (may re-lock)
                inject_spi(33);
            }
        }
    }

    pub fn attach_virtio_net(&self, vm_id: usize) {
        self.devices.lock().attach_virtio_net(vm_id);
    }

    pub fn inject_net_rx(&self, frame: &[u8]) -> bool {
        if let Some(transport) = self.devices.lock().virtio_net_mut() {
            transport.inject_rx(frame)
        } else {
            false
        }
    }
}

/// Per-VM device managers.
/// Exception handler indexes by CURRENT_VM_ID.
pub static DEVICES: [GlobalDeviceManager; MAX_VMS] = [
    GlobalDeviceManager::new(),
    GlobalDeviceManager::new(),
];

/// Get the current VM's device manager.
#[inline]
pub fn current_devices() -> &'static GlobalDeviceManager {
    &DEVICES[CURRENT_VM_ID.load(Ordering::Relaxed)]
}

// ── Per-VM Global State ──────────────────────────────────────────────

/// Per-VM global state — exception handler indexes by CURRENT_VM_ID.
///
/// Contains all the per-vCPU atomics that were previously flat globals
/// (PENDING_SGIS, PENDING_SPIS, TERMINAL_EXIT, etc.), now scoped per VM.
pub struct VmGlobalState {
    /// Per-vCPU pending SGI bitmask (bits 0-15 = SGI 0-15)
    pub pending_sgis: [AtomicU32; MAX_VCPUS],
    /// Per-vCPU pending SPI bitmask (bit N = INTID N+32)
    pub pending_spis: [AtomicU32; MAX_VCPUS],
    /// Per-vCPU terminal exit flag (PSCI CPU_OFF/SYSTEM_OFF/SYSTEM_RESET)
    pub terminal_exit: [AtomicBool; MAX_VCPUS],
    /// Bitmask of online vCPUs for this VM (bit N = vCPU N online)
    pub vcpu_online_mask: AtomicU64,
    /// Currently running vCPU ID within this VM
    pub current_vcpu_id: AtomicUsize,
    /// Pending PSCI CPU_ON for this VM (single-pCPU mode)
    pub pending_cpu_on: PendingCpuOn,
    /// Flag set by IRQ handler to signal preemptive vCPU exit
    pub preemption_exit: AtomicBool,
}

impl VmGlobalState {
    pub const fn new() -> Self {
        Self {
            pending_sgis: [
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
            ],
            pending_spis: [
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0),
            ],
            terminal_exit: [
                AtomicBool::new(false), AtomicBool::new(false),
                AtomicBool::new(false), AtomicBool::new(false),
                AtomicBool::new(false), AtomicBool::new(false),
                AtomicBool::new(false), AtomicBool::new(false),
            ],
            vcpu_online_mask: AtomicU64::new(0),
            current_vcpu_id: AtomicUsize::new(0),
            pending_cpu_on: PendingCpuOn::new(),
            preemption_exit: AtomicBool::new(false),
        }
    }
}

/// Global array of per-VM state.
/// VM 0 is the default — all existing single-VM code paths use VM_STATE[0].
pub static VM_STATE: [VmGlobalState; MAX_VMS] = [
    VmGlobalState::new(),
    VmGlobalState::new(),
];

/// Get the current VM's global state.
#[inline]
pub fn current_vm_state() -> &'static VmGlobalState {
    &VM_STATE[CURRENT_VM_ID.load(Ordering::Relaxed)]
}

/// Get a specific VM's global state.
#[inline]
pub fn vm_state(vm_id: usize) -> &'static VmGlobalState {
    &VM_STATE[vm_id]
}

/// Get the current vCPU ID.
/// - Single-pCPU: reads current_vm_state().current_vcpu_id.
/// - Multi-pCPU: reads MPIDR_EL1.Aff0 (1:1 affinity, vCPU N = pCPU N).
#[inline]
pub fn current_vcpu_id() -> usize {
    #[cfg(not(feature = "multi_pcpu"))]
    { current_vm_state().current_vcpu_id.load(Ordering::Relaxed) }

    #[cfg(feature = "multi_pcpu")]
    { crate::percpu::current_cpu_id() }
}

// ── PSCI CPU_ON ──────────────────────────────────────────────────────

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

/// Per-vCPU PSCI CPU_ON request (multi-pCPU mode).
/// Index = target vCPU ID. Each pCPU checks its own slot.
#[cfg(feature = "multi_pcpu")]
pub struct PerVcpuCpuOnRequest {
    pub requested: AtomicBool,
    pub entry_point: AtomicU64,
    pub context_id: AtomicU64,
}

#[cfg(feature = "multi_pcpu")]
impl PerVcpuCpuOnRequest {
    pub const fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            entry_point: AtomicU64::new(0),
            context_id: AtomicU64::new(0),
        }
    }

    /// Signal a CPU_ON request for this vCPU
    pub fn request(&self, entry: u64, ctx: u64) {
        self.entry_point.store(entry, Ordering::Relaxed);
        self.context_id.store(ctx, Ordering::Relaxed);
        self.requested.store(true, Ordering::Release);
    }

    /// Take a pending CPU_ON request
    pub fn take(&self) -> Option<(u64, u64)> {
        if self.requested.compare_exchange(
            true, false, Ordering::Acquire, Ordering::Relaxed,
        ).is_ok() {
            let entry = self.entry_point.load(Ordering::Relaxed);
            let ctx = self.context_id.load(Ordering::Relaxed);
            Some((entry, ctx))
        } else {
            None
        }
    }
}

#[cfg(feature = "multi_pcpu")]
pub static PENDING_CPU_ON_PER_VCPU: [PerVcpuCpuOnRequest; MAX_VCPUS] = [
    PerVcpuCpuOnRequest::new(), PerVcpuCpuOnRequest::new(),
    PerVcpuCpuOnRequest::new(), PerVcpuCpuOnRequest::new(),
    PerVcpuCpuOnRequest::new(), PerVcpuCpuOnRequest::new(),
    PerVcpuCpuOnRequest::new(), PerVcpuCpuOnRequest::new(),
];

/// Shared Stage-2 translation configuration (set by primary, read by secondaries).
/// VTTBR_EL2 and VTCR_EL2 must be identical on all pCPUs.
#[cfg(feature = "multi_pcpu")]
pub static SHARED_VTTBR: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "multi_pcpu")]
pub static SHARED_VTCR: AtomicU64 = AtomicU64::new(0);

/// Inject an SPI to the correct vCPU based on GICD_IROUTER.
///
/// Called from exception handler or device completion path.
/// Routes via the current VM's device manager and pending SPI state.
///
/// Only supports INTIDs 32-63 (first 32 SPIs).
pub fn inject_spi(intid: u32) {
    if intid < 32 || intid > 63 {
        return;
    }
    let bit = intid - 32;
    let vm_id = CURRENT_VM_ID.load(Ordering::Relaxed);
    let vs = &VM_STATE[vm_id];

    // Read IROUTER to find target vCPU.
    // In multi-pCPU mode, read the physical GICD_IROUTER directly (EL2 bypasses
    // Stage-2) to avoid deadlock — inject_spi() may be called from inside the
    // DEVICES lock (e.g., virtio-blk signal_interrupt → inject_spi).
    #[cfg(feature = "multi_pcpu")]
    let target = {
        let gicd_irouter_base = crate::dtb::platform_info().gicd_base + 0x6100;
        let irouter_addr = gicd_irouter_base + (intid as u64 - 32) * 8;
        let irouter = unsafe { core::ptr::read_volatile(irouter_addr as *const u64) };
        (irouter & 0xFF) as usize // Aff0 = vCPU ID
    };
    #[cfg(not(feature = "multi_pcpu"))]
    let target = DEVICES[vm_id].route_spi(intid);
    if target < MAX_VCPUS {
        vs.pending_spis[target].fetch_or(1 << bit, Ordering::Release);

        // Multi-pCPU: if target is a remote pCPU, send physical SGI to wake it.
        #[cfg(feature = "multi_pcpu")]
        {
            let current = crate::percpu::current_cpu_id();
            if target != current {
                // Send SGI 0 to target pCPU to wake it from WFI
                let val: u64 = 1u64 << target; // TargetList only, INTID=0
                unsafe {
                    core::arch::asm!(
                        "msr icc_sgi1r_el1, {val}",
                        "isb",
                        val = in(reg) val,
                        options(nostack, nomem),
                    );
                }
            }
        }
    }
}

// ── UART RX pending ring buffer ─────────────────────────────────────
// Filled by handle_irq_exception (INTID 33), drained by run loop.

const UART_RX_RING_SIZE: usize = 64;

pub struct UartRxRing {
    buf: UnsafeCell<[u8; UART_RX_RING_SIZE]>,
    head: AtomicUsize,  // read index
    tail: AtomicUsize,  // write index
}

unsafe impl Sync for UartRxRing {}

impl UartRxRing {
    pub const fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0; UART_RX_RING_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Push a byte (called from IRQ handler).
    pub fn push(&self, ch: u8) {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % UART_RX_RING_SIZE;
        if next == self.head.load(Ordering::Acquire) {
            return; // full, drop
        }
        unsafe { (*self.buf.get())[tail] = ch; }
        self.tail.store(next, Ordering::Release);
    }

    /// Pop a byte (called from run loop).
    pub fn pop(&self) -> Option<u8> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None; // empty
        }
        let ch = unsafe { (*self.buf.get())[head] };
        self.head.store((head + 1) % UART_RX_RING_SIZE, Ordering::Release);
        Some(ch)
    }
}

pub static UART_RX: UartRxRing = UartRxRing::new();

//! Virtual Machine Management
//!
//! This module provides the [`Vm`] type which represents a complete virtual machine
//! containing one or more vCPUs, Stage-2 memory mapping, and emulated devices.

use crate::vcpu::Vcpu;
#[cfg(not(feature = "linux_guest"))]
use crate::arch::aarch64::{MemoryAttributes, init_stage2};
use crate::arch::aarch64::defs::*;
use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
#[cfg(not(feature = "multi_pcpu"))]
use crate::devices::MmioDevice;
use crate::scheduler::Scheduler;
use crate::platform;
use core::sync::atomic::Ordering;

/// Maximum number of vCPUs per VM
pub const MAX_VCPUS: usize = 8;

/// Virtual Machine lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM created but no vCPUs or memory configured
    Uninitialized,

    /// VM fully configured and ready to execute
    Ready,

    /// At least one vCPU is currently executing
    Running,

    /// VM execution suspended, can be resumed
    Paused,

    /// VM terminated, requires reset to run again
    Stopped,
}

/// Virtual Machine
pub struct Vm {
    /// Unique identifier for this VM
    id: usize,

    /// Current state of the VM
    state: VmState,

    /// vCPUs belonging to this VM
    vcpus: [Option<Vcpu>; MAX_VCPUS],

    /// Number of active vCPUs
    vcpu_count: usize,

    /// Whether memory is initialized
    memory_initialized: bool,

    /// Scheduler for multi-vCPU execution
    scheduler: Scheduler,
}

impl Vm {
    /// Create a new VM
    pub fn new(id: usize) -> Self {
        const INIT: Option<Vcpu> = None;
        // Reset and register default devices into the global device manager.
        // GlobalDeviceManager uses a static DeviceManager to avoid stack overflow
        // (VirtualGicd alone is ~10KB due to irouter[988]).
        crate::global::DEVICES.reset();
        crate::global::DEVICES.register_device(crate::devices::Device::Uart(
            crate::devices::pl011::VirtualUart::new(),
        ));
        crate::global::DEVICES.register_device(crate::devices::Device::Gicd(
            crate::devices::gic::VirtualGicd::new(),
        ));
        #[cfg(feature = "linux_guest")]
        crate::global::DEVICES.register_device(crate::devices::Device::Gicr(
            crate::devices::gic::VirtualGicr::new(platform::SMP_CPUS),
        ));

        Self {
            id,
            state: VmState::Uninitialized,
            vcpus: [INIT; MAX_VCPUS],
            vcpu_count: 0,
            memory_initialized: false,
            scheduler: Scheduler::new(),
        }
    }

    /// Get VM ID
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get current state
    pub fn state(&self) -> VmState {
        self.state
    }

    /// Get number of vCPUs
    pub fn vcpu_count(&self) -> usize {
        self.vcpu_count
    }

    /// Initialize memory for the VM
    pub fn init_memory(&mut self, guest_mem_start: u64, guest_mem_size: u64) {
        use crate::uart_puts;
        use crate::uart_put_hex;

        if self.memory_initialized {
            uart_puts(b"[VM] Memory already initialized\n");
            return;
        }

        uart_puts(b"[VM] Initializing memory mapping...\n");

        // Round to 2MB boundaries
        let start_aligned = guest_mem_start & !BLOCK_MASK_2MB;
        let size_aligned = ((guest_mem_size + BLOCK_SIZE_2MB - 1) / BLOCK_SIZE_2MB) * BLOCK_SIZE_2MB;

        uart_puts(b"[VM] Mapping region: 0x");
        uart_put_hex(start_aligned);
        uart_puts(b" - 0x");
        uart_put_hex(start_aligned + size_aligned);
        uart_puts(b"\n");

        #[cfg(feature = "linux_guest")]
        self.init_memory_dynamic(start_aligned, size_aligned);

        #[cfg(not(feature = "linux_guest"))]
        self.init_memory_static(start_aligned, size_aligned);

        self.memory_initialized = true;
        uart_puts(b"[VM] Memory mapping complete\n");
    }

    /// Static mapper path for unit tests (no 4KB page support needed)
    #[cfg(not(feature = "linux_guest"))]
    fn init_memory_static(&self, start_aligned: u64, size_aligned: u64) {
        use crate::arch::aarch64::mm::IdentityMapper;
        use core::cell::UnsafeCell;

        // UnsafeCell wrapper to avoid `static mut` (Rust 2024 compatibility).
        // SAFETY: Only called from single-threaded unit test path.
        struct MapperCell(UnsafeCell<IdentityMapper>);
        unsafe impl Sync for MapperCell {}

        static MAPPER: MapperCell = MapperCell(UnsafeCell::new(IdentityMapper::new()));
        unsafe {
            let m = &mut *MAPPER.0.get();
            m.reset();
            m.map_region(start_aligned, size_aligned, MemoryAttributes::NORMAL);
            m.map_region(platform::GIC_REGION_BASE, platform::GIC_REGION_SIZE, MemoryAttributes::DEVICE);
            init_stage2(&*MAPPER.0.get());
        }
    }

    /// Dynamic mapper path for Linux guest (supports 4KB unmap for GICR trap)
    #[cfg(feature = "linux_guest")]
    fn init_memory_dynamic(&self, start_aligned: u64, size_aligned: u64) {
        use crate::uart_puts;
        use crate::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute, init_stage2_from_config};

        let mut mapper = DynamicIdentityMapper::new();

        // Map guest memory in two regions, SKIPPING the hypervisor heap.
        // The heap (HEAP_START .. HEAP_START+HEAP_SIZE) lies within the guest's
        // Stage-2 address range. If mapped as Normal, the guest could corrupt
        // page tables allocated from the heap (L3 tables for GICR trap-and-emulate).
        // By not mapping the heap region, accesses from the guest to those addresses
        // cause Stage-2 translation faults (harmless — the kernel's declared memory
        // starts at 0x48000000, well above the heap).
        let heap_start = platform::HEAP_START;
        let heap_end = heap_start + platform::HEAP_SIZE;
        let end_aligned = start_aligned + size_aligned;

        // Compute the overlap between [start_aligned, end_aligned) and [heap_start, heap_end)
        let overlap_start = if heap_start > start_aligned { heap_start } else { start_aligned };
        let overlap_end = if heap_end < end_aligned { heap_end } else { end_aligned };
        let has_overlap = overlap_start < overlap_end;

        if has_overlap {
            // Map the portion before the heap (if any)
            if start_aligned < overlap_start {
                let before_size = overlap_start - start_aligned;
                uart_puts(b"[VM] Guest region before heap: 0x");
                crate::uart_put_hex(start_aligned);
                uart_puts(b" - 0x");
                crate::uart_put_hex(overlap_start);
                uart_puts(b"\n");
                mapper.map_region(start_aligned, before_size, MemoryAttribute::Normal)
                    .expect("Failed to map guest memory before heap");
            }
            uart_puts(b"[VM] Heap gap (unmapped): 0x");
            crate::uart_put_hex(overlap_start);
            uart_puts(b" - 0x");
            crate::uart_put_hex(overlap_end);
            uart_puts(b"\n");
            // Map the portion after the heap (if any)
            if overlap_end < end_aligned {
                let after_size = end_aligned - overlap_end;
                uart_puts(b"[VM] Guest region after heap: 0x");
                crate::uart_put_hex(overlap_end);
                uart_puts(b" - 0x");
                crate::uart_put_hex(end_aligned);
                uart_puts(b"\n");
                mapper.map_region(overlap_end, after_size, MemoryAttribute::Normal)
                    .expect("Failed to map guest memory after heap");
            }
        } else {
            // Heap is outside guest range — single contiguous mapping
            mapper.map_region(start_aligned, size_aligned, MemoryAttribute::Normal)
                .expect("Failed to map guest memory");
        }

        // Map entire GIC region as DEVICE (passthrough), then selectively
        // unmap GICD and all GICR frames so guest accesses trap to EL2
        // for emulation via VirtualGicd / VirtualGicr.
        mapper.map_region(platform::GIC_REGION_BASE, platform::GIC_REGION_SIZE, MemoryAttribute::Device)
            .expect("Failed to map GIC region");

        // Unmap GICD (64KB = 16 × 4KB pages) for full trap-and-emulate.
        // Guest GICD accesses trap as Data Aborts → VirtualGicd.
        // The hypervisor still accesses physical GICD at EL2 (bypasses Stage-2).
        for page in 0..16u64 {
            let addr = platform::GICD_BASE + page * PAGE_SIZE_4KB;
            mapper.unmap_4kb_page(addr)
                .expect("Failed to unmap GICD page");
        }
        uart_puts(b"[VM] GICD unmapped (trap to EL2 via VirtualGicd)\n");

        // Unmap all GICR frames (each = 128KB = 32 × 4KB pages)
        for &base in &platform::GICR_RD_BASES {
            for page in 0..32u64 {
                let addr = base + page * PAGE_SIZE_4KB;
                mapper.unmap_4kb_page(addr)
                    .expect("Failed to unmap GICR page");
            }
        }
        uart_puts(b"[VM] All GICRs unmapped (trap to EL2 via VirtualGicr)\n");

        // UART (0x09000000) is NOT mapped — all accesses trap to VirtualUart

        // Install Stage-2 translation
        let config = mapper.config();
        init_stage2_from_config(&config);

        // Save VTTBR/VTCR for secondary pCPUs (they need the same Stage-2 config)
        #[cfg(feature = "multi_pcpu")]
        {
            let vttbr: u64;
            let vtcr: u64;
            unsafe {
                core::arch::asm!("mrs {}, vttbr_el2", out(reg) vttbr);
                core::arch::asm!("mrs {}, vtcr_el2", out(reg) vtcr);
            }
            crate::global::SHARED_VTTBR.store(vttbr, Ordering::Release);
            crate::global::SHARED_VTCR.store(vtcr, Ordering::Release);
        }

        // Keep mapper alive — page tables are heap-allocated and must not be freed.
        // Leak it intentionally (we only create one per boot).
        core::mem::forget(mapper);
    }

    /// Create a vCPU with specified ID
    pub fn create_vcpu(&mut self, vcpu_id: usize) -> Result<&mut Vcpu, &'static str> {
        if vcpu_id >= MAX_VCPUS {
            return Err("vCPU ID out of range");
        }
        if self.vcpus[vcpu_id].is_some() {
            return Err("vCPU already exists");
        }

        let vcpu = Vcpu::new(vcpu_id, 0, 0);
        self.vcpus[vcpu_id] = Some(vcpu);
        self.vcpu_count += 1;
        self.scheduler.add_vcpu(vcpu_id);

        if self.state == VmState::Uninitialized {
            self.state = VmState::Ready;
        }

        Ok(self.vcpus[vcpu_id].as_mut().unwrap())
    }

    /// Add a vCPU to this VM
    pub fn add_vcpu(&mut self, entry_point: u64, stack_pointer: u64)
        -> Result<usize, &'static str> {
        if self.vcpu_count >= MAX_VCPUS {
            return Err("Maximum vCPU count reached");
        }

        let vcpu_id = self.vcpu_count;
        let vcpu = Vcpu::new(vcpu_id, entry_point, stack_pointer);

        self.vcpus[vcpu_id] = Some(vcpu);
        self.vcpu_count += 1;

        if self.state == VmState::Uninitialized {
            self.state = VmState::Ready;
        }

        Ok(vcpu_id)
    }

    /// Get a reference to a vCPU
    pub fn vcpu(&self, vcpu_id: usize) -> Option<&Vcpu> {
        if vcpu_id < self.vcpu_count {
            self.vcpus[vcpu_id].as_ref()
        } else {
            None
        }
    }

    /// Get a mutable reference to a vCPU
    pub fn vcpu_mut(&mut self, vcpu_id: usize) -> Option<&mut Vcpu> {
        if vcpu_id < self.vcpu_count {
            self.vcpus[vcpu_id].as_mut()
        } else {
            None
        }
    }

    /// Run the VM (single vCPU for now)
    pub fn run(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Ready {
            return Err("VM is not in Ready state");
        }

        if self.vcpu_count == 0 {
            return Err("No vCPUs configured");
        }

        self.state = VmState::Running;

        if let Some(vcpu) = self.vcpu_mut(0) {
            let result = vcpu.run();

            self.state = VmState::Ready;

            result
        } else {
            self.state = VmState::Ready;
            Err("vCPU 0 not found")
        }
    }

    /// Run a single vCPU on the current pCPU (multi-pCPU mode).
    ///
    /// Each pCPU calls this with its own vcpu_id (1:1 affinity).
    /// The primary pCPU (pCPU 0) runs vCPU 0, secondary pCPUs create
    /// their own vCPUs in `secondary_enter_guest()` (main.rs).
    ///
    /// This function only returns if the vCPU exits cleanly (HVC exit).
    /// WFI executes real WFI on the physical CPU (no scheduler needed).
    #[cfg(feature = "multi_pcpu")]
    #[allow(unreachable_code)]
    pub fn run_vcpu(&mut self, vcpu_id: usize) -> Result<(), &'static str> {
        use crate::uart_puts;

        if self.state != VmState::Ready {
            return Err("VM is not in Ready state");
        }
        let vcpu = self.vcpus[vcpu_id].as_mut().ok_or("vCPU not found")?;
        let _ = vcpu; // drop borrow — re-borrow in loop

        self.state = VmState::Running;
        crate::global::CURRENT_VCPU_ID.store(vcpu_id, Ordering::Release);
        crate::global::VCPU_ONLINE_MASK.fetch_or(1 << vcpu_id, Ordering::Release);

        uart_puts(b"[VM] pCPU 0 entering run_vcpu loop for vCPU 0\n");

        loop {
            // Drain physical UART RX bytes → VirtualUart → inject SPI 33
            crate::global::DEVICES.drain_uart_rx();

            // Ensure PPI 27 (virtual timer) is enabled at the physical GICR.
            // Guest's GICR writes are trapped → shadow only → physical stays disabled.
            ensure_vtimer_enabled(vcpu_id);

            // Inject pending SGIs and SPIs
            let vcpu = self.vcpus[vcpu_id].as_mut().unwrap();
            inject_pending_sgis(vcpu);
            inject_pending_spis(vcpu);

            // Enter guest
            match vcpu.run() {
                Ok(()) => {
                    // Normal exit — IRQ handler exited to host for processing
                    // (e.g., UART RX data to drain). Loop back to re-enter.
                }
                Err("WFI") => {
                    // WFI: execute real WFI on the physical CPU.
                    // pCPU idles until next interrupt (SGI, SPI, timer).
                    unsafe { core::arch::asm!("wfi") };
                }
                Err(_) => {
                    // Other exit — loop back
                }
            }
        }

        self.state = VmState::Ready;
        Ok(())
    }

    /// Run the VM with SMP scheduling (multiple vCPUs, round-robin on single pCPU)
    #[cfg(not(feature = "multi_pcpu"))]
    pub fn run_smp(&mut self) -> Result<(), &'static str> {
        use crate::uart_puts;
        use crate::uart_put_hex;

        if self.state != VmState::Ready {
            return Err("VM is not in Ready state");
        }
        if self.vcpu_count == 0 {
            return Err("No vCPUs configured");
        }

        self.state = VmState::Running;

        loop {
            // Check for pending PSCI CPU_ON requests
            if let Some((target, entry, ctx_id)) = crate::global::PENDING_CPU_ON.take() {
                let vcpu_id = (target & 0xFF) as usize;
                if vcpu_id < MAX_VCPUS && self.vcpus[vcpu_id].is_none() {
                    uart_puts(b"[VM] Booting secondary vCPU ");
                    uart_put_hex(vcpu_id as u64);
                    uart_puts(b" at entry=0x");
                    uart_put_hex(entry);
                    uart_puts(b"\n");
                    self.boot_secondary_vcpu(vcpu_id, entry, ctx_id);
                }
            }

            // Unblock vCPUs with pending SGIs BEFORE scheduling, so the
            // scheduler can immediately pick a vCPU that has work.
            wake_pending_vcpus(&mut self.scheduler, &self.vcpus);

            // Schedule next vCPU
            let vcpu_id = match self.schedule() {
                Some(id) => id,
                None => {
                    // All vCPUs blocked (WFI). Unblock all online vCPUs so
                    // timers can fire and make progress.
                    let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
                    let mut any = false;
                    for id in 0..MAX_VCPUS {
                        if online & (1 << id) != 0 && self.vcpus[id].is_some() {
                            self.scheduler.unblock(id);
                            any = true;
                        }
                    }
                    if !any {
                        break; // No vCPUs at all
                    }
                    continue; // Retry scheduling
                }
            };

            // Set current vCPU ID so IRQ/trap handler knows who's running
            crate::global::CURRENT_VCPU_ID.store(vcpu_id, Ordering::Release);

            // Drain physical UART RX bytes into VirtualUart and inject SPI 33
            while let Some(ch) = crate::global::UART_RX.pop() {
                if let Some(uart) = crate::global::DEVICES.uart_mut() {
                    uart.push_rx(ch);
                }
            }
            if let Some(uart) = crate::global::DEVICES.uart_mut() {
                if uart.pending_irq().is_some() {
                    crate::global::inject_spi(33);
                }
            }

            // Inject pending SGIs and SPIs into this vCPU's arch_state before run
            inject_pending_sgis(self.vcpus[vcpu_id].as_mut().unwrap());
            inject_pending_spis(self.vcpus[vcpu_id].as_mut().unwrap());

            // Arm CNTHP preemption watchdog (10ms) in SMP mode.
            // This ensures preemption works even when the guest virtual timer
            // is masked (e.g., during multi_cpu_stop with IRQs disabled).
            let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
            let multi_vcpu = online != 0 && (online & (online - 1)) != 0;
            if multi_vcpu {
                ensure_cnthp_enabled();
                crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
            }

            // Run it
            let vcpu = self.vcpus[vcpu_id].as_mut().unwrap();
            let result = vcpu.run();

            match result {
                Ok(()) => {
                    // Normal exit - distinguish CPU_ON, preemption, or real termination
                    if crate::global::PENDING_CPU_ON.requested.load(Ordering::Relaxed) {
                        // CPU_ON exit: yield so we process the request at loop top
                        self.scheduler.yield_current();
                    } else if crate::global::PREEMPTION_EXIT
                        .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                    {
                        // Preemptive timer exit: yield to let other vCPUs run
                        self.scheduler.yield_current();
                    } else {
                        // Real exit (HVC #1 exit, SYSTEM_OFF, etc.) - remove from scheduler
                        self.scheduler.remove_vcpu(vcpu_id);
                    }
                }
                Err("WFI") => {
                    // WFI - block until SGI/IPI wakes this vCPU
                    self.scheduler.block_current();
                }
                Err(_) => {
                    // Other error - yield
                    self.scheduler.yield_current();
                }
            }
        }

        self.state = VmState::Ready;
        Ok(())
    }

    /// Boot a secondary vCPU via PSCI CPU_ON (single-pCPU mode only)
    #[cfg(not(feature = "multi_pcpu"))]
    fn boot_secondary_vcpu(&mut self, id: usize, entry: u64, ctx_id: u64) {
        // Wake up the target CPU's GICR so it accepts pending SGIs.
        // Without this, ICC_SGI1R_EL1 writes targeting this CPU are dropped
        // by the physical GIC because GICR_WAKER.ProcessorSleep=1.
        if id > 0 && id < platform::GICR_RD_BASES.len() {
            wake_gicr(platform::GICR_RD_BASES[id]);
        }
        let mut vcpu = Vcpu::new(id, entry, 0);
        // PSCI CPU_ON: x0 = context_id, booting into EL1h with DAIF masked
        vcpu.context_mut().gp_regs.x0 = ctx_id;
        vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;
        // SCTLR_EL1: RES1 value (MMU off, caches off) - same as primary at boot
        vcpu.arch_state_mut().sctlr_el1 = 0x30D0_0800;
        // Enable FP/SIMD access (CPACR_EL1.FPEN = 0b11)
        vcpu.arch_state_mut().cpacr_el1 = 3 << 20;
        vcpu.arch_state_mut().init_for_vcpu(id);
        self.vcpus[id] = Some(vcpu);
        self.vcpu_count += 1;
        self.scheduler.add_vcpu(id);
        crate::global::VCPU_ONLINE_MASK.fetch_or(1 << id, Ordering::Release);
        // Reset exception counters so the new vCPU gets a clean slate
        crate::arch::aarch64::hypervisor::exception::reset_exception_counters();
    }

    /// Pause the VM
    pub fn pause(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Running {
            return Err("VM is not running");
        }

        self.state = VmState::Paused;
        Ok(())
    }

    /// Resume the VM
    pub fn resume(&mut self) -> Result<(), &'static str> {
        if self.state != VmState::Paused {
            return Err("VM is not paused");
        }

        self.state = VmState::Running;
        Ok(())
    }

    /// Stop the VM
    pub fn stop(&mut self) {
        for vcpu in self.vcpus.iter_mut().flatten() {
            vcpu.stop();
        }

        self.state = VmState::Stopped;
    }

    // ========== Scheduler Integration ==========

    /// Schedule the next vCPU to run
    pub fn schedule(&mut self) -> Option<usize> {
        self.scheduler.pick_next()
    }

    /// Run the currently scheduled vCPU
    pub fn run_current(&mut self) -> Result<(), &'static str> {
        let vcpu_id = self.scheduler.current().ok_or("No current vCPU")?;
        let vcpu = self.vcpus[vcpu_id].as_mut().ok_or("vCPU not found")?;
        vcpu.run()
    }

    /// Mark the current vCPU as done (remove from scheduler)
    pub fn mark_current_done(&mut self) {
        if let Some(id) = self.scheduler.current() {
            self.scheduler.remove_vcpu(id);
        }
    }

    /// Yield the current vCPU (allow another vCPU to run)
    pub fn yield_current(&mut self) {
        self.scheduler.yield_current();
    }

    /// Block the current vCPU (e.g., waiting for I/O)
    pub fn block_current(&mut self) {
        self.scheduler.block_current();
    }

    /// Unblock a vCPU (make it ready to run again)
    pub fn unblock(&mut self, vcpu_id: usize) {
        self.scheduler.unblock(vcpu_id);
    }

    /// Get the currently scheduled vCPU ID
    pub fn current_vcpu(&self) -> Option<usize> {
        self.scheduler.current()
    }
}

/// Wake up a GICR redistributor by clearing GICR_WAKER.ProcessorSleep.
///
/// Only used in single-pCPU mode (secondary vCPUs booted via PSCI CPU_ON).
/// In multi-pCPU mode, each pCPU wakes its own GICR in secondary_enter_guest().
#[cfg(not(feature = "multi_pcpu"))]
fn wake_gicr(rd_base: u64) {
    let waker_addr = (rd_base + platform::GICR_WAKER_OFF) as *mut u32;
    unsafe {
        let mut waker = core::ptr::read_volatile(waker_addr);
        // Clear ProcessorSleep (bit 1)
        waker &= !(1 << 1);
        core::ptr::write_volatile(waker_addr, waker);
        // Wait for ChildrenAsleep (bit 2) to clear
        loop {
            let w = core::ptr::read_volatile(waker_addr);
            if w & (1 << 2) == 0 {
                break;
            }
        }
    }
}

/// Ensure SGIs (0-15) and PPI 27 (virtual timer) are enabled and Group 1
/// at the physical GICR for the given pCPU.
///
/// In multi-pCPU mode, the guest's GICR writes are trapped and only update
/// shadow state (VirtualGicr). The physical GICR never sees the guest's
/// ISENABLER0 write, so PPIs stay disabled. Without PPI 27, the virtual
/// timer can't generate a physical IRQ (WFI never wakes). Without SGIs
/// 0-15, physical IPIs between pCPUs don't fire.
///
/// This function programs the **physical** GICR SGI frame at EL2
/// (EL2 accesses bypass Stage-2 translation).
#[cfg(feature = "multi_pcpu")]
#[inline]
pub fn ensure_vtimer_enabled(cpu_id: usize) {
    // Bits to enable: SGIs 0-15 (for physical IPIs) + PPI 27 (vtimer)
    const ENABLE_MASK: u32 = 0xFFFF | (1 << 27); // bits 0-15 + bit 27

    let sgi_base = platform::GICR_RD_BASES[cpu_id] + 0x10000;
    unsafe {
        // IGROUPR0: ensure Group 1 for SGIs + PPI 27
        let igroupr0 = core::ptr::read_volatile(
            (sgi_base + platform::GICR_IGROUPR0_OFF) as *const u32,
        );
        if igroupr0 & ENABLE_MASK != ENABLE_MASK {
            core::ptr::write_volatile(
                (sgi_base + platform::GICR_IGROUPR0_OFF) as *mut u32,
                igroupr0 | ENABLE_MASK,
            );
        }
        // ISENABLER0: write-1-to-set (only sets our bits, doesn't affect others)
        core::ptr::write_volatile(
            (sgi_base + platform::GICR_ISENABLER0_OFF) as *mut u32,
            ENABLE_MASK,
        );
    }
}

/// Ensure INTID 26 (CNTHP timer PPI) is enabled and Group 1 in GICR0.
/// Only needed in single-pCPU mode (for preemptive scheduling timer).
#[cfg(not(feature = "multi_pcpu"))]
#[inline]
fn ensure_cnthp_enabled() {
    unsafe {
        let sgi_base = platform::GICR0_SGI_BASE;
        // IGROUPR0: ensure Group 1 (read-modify-write)
        let igroupr0 = core::ptr::read_volatile(
            (sgi_base + platform::GICR_IGROUPR0_OFF) as *const u32,
        );
        if igroupr0 & (1 << 26) == 0 {
            core::ptr::write_volatile(
                (sgi_base + platform::GICR_IGROUPR0_OFF) as *mut u32,
                igroupr0 | (1 << 26),
            );
        }
        // ISENABLER0: write-1-to-set (only sets bit 26, doesn't affect others)
        core::ptr::write_volatile(
            (sgi_base + platform::GICR_ISENABLER0_OFF) as *mut u32,
            1 << 26,
        );
    }
}

/// Check for pending SGIs and unblock blocked vCPUs that have work.
/// Only used in single-pCPU mode (scheduler-based scheduling).
#[cfg(not(feature = "multi_pcpu"))]
fn wake_pending_vcpus(scheduler: &mut Scheduler, vcpus: &[Option<Vcpu>; MAX_VCPUS]) {
    for id in 0..MAX_VCPUS {
        if vcpus[id].is_none() {
            continue;
        }
        if crate::global::PENDING_SGIS[id].load(Ordering::Relaxed) != 0
            || crate::global::PENDING_SPIS[id].load(Ordering::Relaxed) != 0
        {
            scheduler.unblock(id);
        }
    }
}

/// Inject pending SGIs into a vCPU's saved arch_state LRs before running.
///
/// SGIs are queued in PENDING_SGIS by the TALL1 trap handler (handle_sgi_trap)
/// when the guest writes ICC_SGI1R_EL1.
///
/// Critical: must write to `arch_state.ich_lr[]` (not hardware LRs), because
/// `vcpu.run()` calls `arch_state.restore()` which overwrites hardware LRs.
fn inject_pending_sgis(vcpu: &mut Vcpu) {
    let vcpu_id = vcpu.id();

    let all = crate::global::PENDING_SGIS[vcpu_id].swap(0, Ordering::Acquire);
    if all == 0 {
        return;
    }

    let arch = vcpu.arch_state_mut();
    for sgi in 0..16u32 {
        if all & (1 << sgi) == 0 {
            continue;
        }
        // Find a free LR slot in saved state
        let mut injected = false;
        for lr in arch.ich_lr.iter_mut() {
            if (*lr >> LR_STATE_SHIFT) & LR_STATE_MASK == 0 {
                // LR is free — write pending SGI
                *lr = (GicV3VirtualInterface::LR_STATE_PENDING << LR_STATE_SHIFT)
                    | LR_GROUP1_BIT
                    | ((IRQ_DEFAULT_PRIORITY as u64) << LR_PRIORITY_SHIFT)
                    | (sgi as u64);
                injected = true;
                break;
            }
        }
        if !injected {
            // No free LR — re-queue for next entry
            crate::global::PENDING_SGIS[vcpu_id].fetch_or(1 << sgi, Ordering::Relaxed);
        }
    }
}

/// Inject pending SPIs into a vCPU's saved arch_state LRs before running.
///
/// SPIs are queued in PENDING_SPIS by `global::inject_spi()`.
/// Bit N = SPI with INTID (N + 32).
fn inject_pending_spis(vcpu: &mut Vcpu) {
    let vcpu_id = vcpu.id();

    let all = crate::global::PENDING_SPIS[vcpu_id].swap(0, Ordering::Acquire);
    if all == 0 {
        return;
    }

    let arch = vcpu.arch_state_mut();
    for bit in 0..32u32 {
        if all & (1 << bit) == 0 {
            continue;
        }
        let intid = bit + 32; // SPI INTIDs start at 32
        let mut injected = false;
        for lr in arch.ich_lr.iter_mut() {
            if (*lr >> LR_STATE_SHIFT) & LR_STATE_MASK == 0 {
                *lr = (GicV3VirtualInterface::LR_STATE_PENDING << LR_STATE_SHIFT)
                    | LR_GROUP1_BIT
                    | ((IRQ_DEFAULT_PRIORITY as u64) << LR_PRIORITY_SHIFT)
                    | (intid as u64);
                injected = true;
                break;
            }
        }
        if !injected {
            crate::global::PENDING_SPIS[vcpu_id].fetch_or(1 << bit, Ordering::Relaxed);
        }
    }
}

impl core::fmt::Debug for Vm {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vm")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("vcpu_count", &self.vcpu_count)
            .finish()
    }
}

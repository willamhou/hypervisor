# Multi-pCPU Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Run 4 guest vCPUs on 4 physical CPUs in parallel with fixed vCPU-to-pCPU affinity.

**Architecture:** Each pCPU runs its own `run_vcpu()` loop. Shared device state is protected by per-device spinlocks. Cross-pCPU interrupt delivery uses physical SGI at EL2. Secondary pCPUs boot via WFE/SEV and wait for PSCI CPU_ON.

**Tech Stack:** Rust no_std, ARM64 assembly, QEMU virt (-smp 4)

**Design doc:** `docs/plans/2026-02-15-multi-pcpu-design.md`

---

## Task 1: SpinLock Primitive

**Files:**
- Create: `src/sync.rs`
- Modify: `src/lib.rs` (or `src/main.rs`) — add `mod sync;`

**Step 1: Implement ticket-based SpinLock**

```rust
// src/sync.rs
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

pub struct SpinLock<T> {
    next_ticket: AtomicU32,
    now_serving: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    ticket: u32,
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            next_ticket: AtomicU32::new(0),
            now_serving: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        while self.now_serving.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop(); // WFE on ARM64
        }
        SpinLockGuard { lock: self, ticket }
    }
}

impl<T> core::ops::Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { unsafe { &*self.lock.data.get() } }
}

impl<T> core::ops::DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { unsafe { &mut *self.lock.data.get() } }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.now_serving.store(self.ticket + 1, Ordering::Release);
    }
}
```

**Step 2: Build and verify**

Run: `make check`
Expected: Compiles without errors.

**Step 3: Commit**

```
feat: add ticket-based SpinLock primitive for multi-pCPU
```

---

## Task 2: Feature Flag and Cargo Config

**Files:**
- Modify: `Cargo.toml` — add `multi_pcpu` feature
- Modify: `Makefile` — add `run-linux-smp` target

**Step 1: Add feature to Cargo.toml**

In `[features]` section, add:
```toml
multi_pcpu = ["linux_guest"]
```

**Step 2: Add Makefile target**

```makefile
run-linux-smp:
	# Same as run-linux but with multi_pcpu feature
	cargo build ... --features multi_pcpu
	# QEMU flags same as run-linux (already uses -smp 4)
```

**Step 3: Verify existing build is unaffected**

Run: `make` then `make run-linux`
Expected: Both work identically to before.

**Step 4: Commit**

```
chore: add multi_pcpu feature flag and run-linux-smp target
```

---

## Task 3: Per-pCPU Context and CPU ID Helper

**Files:**
- Create: `src/percpu.rs`
- Modify: `src/main.rs` — add `mod percpu;`

**Step 1: Implement PerCpuContext**

```rust
// src/percpu.rs
use crate::platform::SMP_CPUS;

pub struct PerCpuContext {
    pub vcpu_id: usize,
    pub exception_count: u32,
}

static mut PER_CPU: [PerCpuContext; SMP_CPUS] = {
    const INIT: PerCpuContext = PerCpuContext { vcpu_id: 0, exception_count: 0 };
    [INIT; SMP_CPUS]
};

/// Read current physical CPU ID from MPIDR_EL1.Aff0
#[inline(always)]
pub fn current_cpu_id() -> usize {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr) };
    (mpidr & 0xFF) as usize
}

/// Get per-CPU context for current pCPU (mutable).
/// SAFETY: Each pCPU only accesses its own entry.
pub fn this_cpu() -> &'static mut PerCpuContext {
    let id = current_cpu_id();
    unsafe { &mut PER_CPU[id] }
}
```

**Step 2: Build and verify**

Run: `make check`

**Step 3: Commit**

```
feat: add per-pCPU context and CPU ID helper
```

---

## Task 4: Per-vCPU PENDING_CPU_ON

**Files:**
- Modify: `src/global.rs` — replace single `PendingCpuOn` with per-vCPU array

**Step 1: Add per-vCPU CPU_ON request struct**

Keep the old `PendingCpuOn` behind `#[cfg(not(feature = "multi_pcpu"))]`. Add new per-vCPU version behind `#[cfg(feature = "multi_pcpu")]`:

```rust
#[cfg(feature = "multi_pcpu")]
pub struct PerVcpuCpuOnRequest {
    pub requested: AtomicBool,
    pub entry_point: AtomicU64,
    pub context_id: AtomicU64,
}

#[cfg(feature = "multi_pcpu")]
pub static PENDING_CPU_ON_PER_VCPU: [PerVcpuCpuOnRequest; MAX_VCPUS] = ...;
```

**Step 2: Build both feature sets**

Run: `make check` and `cargo check --features multi_pcpu`

**Step 3: Commit**

```
feat: per-vCPU PENDING_CPU_ON for multi-pCPU PSCI
```

---

## Task 5: Wrap DeviceManager with Per-Device SpinLocks

**Files:**
- Modify: `src/devices/mod.rs` — wrap Device enum variants
- Modify: `src/global.rs` — change DEVICES to use SpinLock per device

This is the most invasive change. Under `multi_pcpu`, `GlobalDeviceManager` holds `SpinLock<Device>` instead of bare `Device`. The `handle_mmio()` method acquires the lock for the matched device.

**Step 1: Add SpinLock wrapping in DeviceManager**

Under `#[cfg(feature = "multi_pcpu")]`, change `DeviceManager` to hold `[Option<SpinLock<Device>>; MAX_DEVICES]`. The `handle_mmio` method locks the matched device before calling read/write.

Without the feature, keep existing `UnsafeCell` approach.

**Step 2: Update exception handler MMIO dispatch**

In `src/arch/aarch64/hypervisor/exception.rs`, the MMIO handling code accesses `DEVICES`. Under `multi_pcpu`, it calls the spinlock-guarded version.

**Step 3: Verify single-pCPU path unbroken**

Run: `make run` (unit tests) and `make run-linux`
Expected: All 43 assertions pass, Linux boots to shell.

**Step 4: Commit**

```
feat: per-device spinlocks for concurrent MMIO access
```

---

## Task 6: Secondary CPU Boot Assembly

**Files:**
- Modify: `arch/aarch64/boot.S` — add secondary CPU path
- Modify: `arch/aarch64/linker.ld` — add pcpu_stacks section

**Step 1: Modify boot.S**

Add CPU ID check at `_start`. CPU 0 takes existing primary path. CPUs 1-3 enter `secondary_wait` loop (WFE until `BOOT_READY[id]` is set), set up per-pCPU stack, then jump to `rust_main_secondary`.

Add `.bss.pcpu_stacks` section: 4 × 16KB stacks (or 3 for secondary CPUs only, primary uses existing stack).

Gate behind `#ifdef MULTI_PCPU` (set by build.rs when feature is active).

**Step 2: Update linker.ld**

Add `.bss.pcpu_stacks` to BSS section.

**Step 3: Update build.rs**

Pass `-DMULTI_PCPU` to gcc when `multi_pcpu` feature is enabled.

**Step 4: Verify primary boot unaffected**

Run: `make run` (without multi_pcpu feature)
Expected: Boots normally, no regression.

**Step 5: Commit**

```
feat: secondary pCPU boot path in assembly
```

---

## Task 7: rust_main_secondary and Primary Init Changes

**Files:**
- Modify: `src/main.rs` — add `rust_main_secondary()`, modify `rust_main()` for multi_pcpu
- Modify: `src/global.rs` — add `BOOT_READY` array

**Step 1: Add BOOT_READY signaling**

```rust
// In global.rs
#[cfg(feature = "multi_pcpu")]
pub static BOOT_READY: [AtomicBool; SMP_CPUS] = ...;
```

**Step 2: Add rust_main_secondary**

```rust
#[cfg(feature = "multi_pcpu")]
#[no_mangle]
pub extern "C" fn rust_main_secondary(cpu_id: usize) -> ! {
    // 1. Set VBAR_EL2
    exception::init();
    // 2. Set VTTBR_EL2 (shared Stage-2 from primary)
    // 3. Set HCR_EL2
    // 4. Initialize per-pCPU GIC
    // 5. WFE loop waiting for PENDING_CPU_ON_PER_VCPU[cpu_id]
    // 6. When triggered: init vCPU, enter run_vcpu(cpu_id)
}
```

**Step 3: Modify rust_main for multi_pcpu**

After heap/device/Stage-2 init, set `BOOT_READY[1..3] = true`, execute SEV, then enter `run_vcpu(0)` instead of `run_smp()`.

**Step 4: Verify with QEMU**

Run: `make run-linux-smp`
Expected: At minimum, pCPU 0 boots vCPU 0. Secondary CPUs should reach `rust_main_secondary` and wait.

**Step 5: Commit**

```
feat: secondary pCPU Rust init and primary boot changes
```

---

## Task 8: run_vcpu() Per-pCPU Loop

**Files:**
- Modify: `src/vm.rs` — add `run_vcpu()` method

**Step 1: Implement run_vcpu**

Simpler than `run_smp()` — no scheduler, no round-robin:

```rust
#[cfg(feature = "multi_pcpu")]
pub fn run_vcpu(&mut self, vcpu_id: usize) -> ! {
    let vcpu = self.vcpus[vcpu_id].as_mut().unwrap();
    loop {
        // 1. Inject pending SGIs/SPIs from atomic queues
        // 2. Enter guest (vcpu.run())
        // 3. Handle exit:
        //    - WFI: execute real WFI (pCPU idles)
        //    - HVC CPU_ON: write PENDING_CPU_ON[target], SEV
        //    - Data Abort: MMIO dispatch (with spinlocks)
        //    - SGI trap: decode, write PENDING_SGIS, send physical SGI
        //    - IRQ (physical SGI kick): check/inject pending, re-enter
    }
}
```

**Step 2: Wire exception handler for multi_pcpu**

Replace `CURRENT_VCPU_ID` reads with `current_cpu_id()` (1:1 affinity). Under multi_pcpu, SGI emulation sends physical SGI to target pCPU instead of just writing atomics.

**Step 3: Test**

Run: `make run-linux-smp`
Expected: vCPU 0 boots, kernel prints early messages. Secondary vCPUs may not boot yet (PSCI path needs wiring).

**Step 4: Commit**

```
feat: per-pCPU run_vcpu() loop
```

---

## Task 9: PSCI CPU_ON Wiring for Multi-pCPU

**Files:**
- Modify: `src/arch/aarch64/hypervisor/exception.rs` — multi_pcpu PSCI path
- Modify: `src/main.rs` — `rust_main_secondary` idle → boot transition

**Step 1: Update exception handler PSCI CPU_ON**

Under `multi_pcpu`: write to `PENDING_CPU_ON_PER_VCPU[target]` + execute `SEV` instruction.

**Step 2: Wire rust_main_secondary idle loop**

When `PENDING_CPU_ON_PER_VCPU[my_id].take()` succeeds, initialize vCPU and enter `run_vcpu()`.

**Step 3: Test**

Run: `make run-linux-smp`
Expected: `smp: Brought up 1 node, 4 CPUs` — all 4 CPUs boot.

**Step 4: Commit**

```
feat: PSCI CPU_ON across physical CPUs via WFE/SEV
```

---

## Task 10: Cross-pCPU SGI via Physical IPI

**Files:**
- Modify: `src/arch/aarch64/hypervisor/exception.rs` — SGI emulation sends physical SGI
- Modify: `src/arch/aarch64/peripherals/gicv3.rs` — add `send_physical_sgi()` helper

**Step 1: Implement send_physical_sgi**

Write ICC_SGI1R_EL1 at EL2 to send physical SGI INTID 0 to target pCPU.

**Step 2: Update SGI emulation**

When guest writes ICC_SGI1R_EL1 and target != self, after writing `PENDING_SGIS[target]`, call `send_physical_sgi(target_pcpu)`.

**Step 3: Handle physical SGI reception**

In the EL2 IRQ handler: ACK (ICC_IAR1_EL1), EOI, check PENDING_SGIS/SPIS, inject, re-enter guest.

**Step 4: Test**

Run: `make run-linux-smp`
Expected: Full boot with SGI/IPI working — no RCU stalls.

**Step 5: Commit**

```
feat: cross-pCPU SGI delivery via physical IPI
```

---

## Task 11: Cross-pCPU SPI Delivery

**Files:**
- Modify: `src/global.rs` — `inject_spi()` sends physical SGI kick when target != self
- Modify: `src/devices/virtio/mmio.rs` — ensure SPI injection works cross-pCPU

**Step 1: Update inject_spi**

After writing `PENDING_SPIS[target]`, if target pCPU != current pCPU, send physical SGI kick.

**Step 2: Test virtio-blk**

Run: `make run-linux-smp`
Expected: `virtio_blk virtio0: [vda] 4096 512-byte logical blocks`

**Step 3: Commit**

```
feat: cross-pCPU SPI delivery for virtio interrupts
```

---

## Task 12: WFI Passthrough

**Files:**
- Modify: `src/vcpu.rs` or HCR_EL2 setup — clear TWI bit under multi_pcpu

**Step 1: Clear TWI in HCR_EL2**

Under `multi_pcpu` feature, do not set TWI bit. Guest WFI executes as real hardware WFI — pCPU halts until next interrupt.

**Step 2: Test**

Run: `make run-linux-smp`
Expected: Boot completes, idle CPUs consume less cycles.

**Step 3: Commit**

```
feat: WFI passthrough on multi-pCPU (no trap needed)
```

---

## Task 13: Regression Testing

**Step 1: Verify single-pCPU mode**

Run: `make run` (unit tests, no feature flags)
Expected: All 43 assertions pass.

Run: `make run-linux` (single-pCPU Linux boot)
Expected: BusyBox shell, 4 vCPUs.

**Step 2: Verify multi-pCPU mode**

Run: `make run-linux-smp`
Expected: BusyBox shell, 4 vCPUs on 4 pCPUs. `dmesg` shows parallel CPU init.

**Step 3: Commit (if any fixes needed)**

---

## Task 14: Documentation Update

**Files:**
- Modify: `CLAUDE.md` — add multi-pCPU architecture section
- Modify: `DEVELOPMENT_PLAN.md` — update Phase 9 status

**Step 1: Update CLAUDE.md**

Add SMP/multi-pCPU section documenting the new architecture, feature flag, and build commands.

**Step 2: Update DEVELOPMENT_PLAN.md**

Mark Phase 9 complete.

**Step 3: Commit**

```
docs: document multi-pCPU support
```

---

## Execution Order and Dependencies

```
Task 1 (SpinLock) ──┐
Task 2 (Feature)  ──┼── Task 5 (Device Locks) ──┐
Task 3 (PerCpu)   ──┘                           │
                                                 │
Task 4 (Per-vCPU CPU_ON) ───────────────────────┐│
Task 6 (Boot ASM) ──── Task 7 (Secondary Init) ─┤│
                                                 ││
                       Task 8 (run_vcpu) ────────┘│
                       Task 9 (PSCI wiring) ──────┘
                       Task 10 (Physical SGI)
                       Task 11 (SPI delivery)
                       Task 12 (WFI passthrough)
                       Task 13 (Regression)
                       Task 14 (Docs)
```

Tasks 1-3 can be done in parallel. Tasks 6-7 are the boot sequence. Tasks 8-12 are sequential (each builds on the previous). Task 13-14 are final.

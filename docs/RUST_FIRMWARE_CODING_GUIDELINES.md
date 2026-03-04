# Rust Firmware Coding Guidelines

ARM64 Type-1 Hypervisor — Project-Specific Standards

> Based on [Safety-Critical Rust Coding Guidelines](https://coding-guidelines.arewesafetycriticalyet.org/) (Rust Foundation),
> [The Embedded Rust Book](https://docs.rust-embedded.org/book/),
> [High Assurance Rust](https://highassurance.rs/), and project experience.

---

## 1. Foundational Principles

### 1.1 Language Subset

```rust
#![no_std]                    // Mandatory — no standard library
#![no_main]                   // Bare-metal entry point via linker
```

- **Core crate only** (`core::`, `alloc::` with custom allocator).
- **No dynamic dispatch** — enum-dispatch over trait objects (`dyn Trait`). Vtable overhead and indirection are unacceptable at EL2.
- **No recursion** — all call graphs must be statically bounded (MISRA Rule 17.2). Stack is fixed-size (128KB per pCPU).
- **No floating-point** — `CPTR_EL3.TFP` may trap FP/SIMD from S-EL2. Avoid `f32`/`f64` in all code paths. Rust debug builds may emit NEON instructions for `read_volatile` alignment checks — build with `opt-level >= 1` for S-EL2.

### 1.2 Memory Allocation

- **Boot-time only** — all heap allocation (`Box`, `Vec`, page tables) occurs during initialization.
- **No runtime allocation** on the hot path (exception handlers, MMIO dispatch, SMC forwarding).
- **`BumpAllocator` with free-list** — deterministic, O(1) alloc/free. No fragmentation risk from bump-only; free-list handles page table tear-down.
- **Allocation failure = panic** — acceptable at boot (`alloc_page().expect("...")`), forbidden in exception paths.

### 1.3 Error Handling Strategy

| Context | Pattern | Example |
|---------|---------|---------|
| Boot init | `.expect("message")` | `alloc_page().expect("L0 table")` |
| State machine | `.expect("transition")` | `sp.transition_to(Idle).expect(...)` |
| FF-A protocol | `Result<T, i32>` | `reclaim_share() -> Result<(), i32>` |
| Exception handler | **Never panic** | Return `bool` / `SmcResult8` |
| Device emulation | `Option<u64>` / `bool` | `read() -> Option<u64>` |

**Rule**: Exception handlers (`handle_exception`, `handle_irq_exception`, `handle_fiq_exception`) must **never** call `.expect()`, `.unwrap()`, or any panicking function. A panic inside an exception handler is unrecoverable.

---

## 2. Unsafe Code Discipline

### 2.1 Minimize and Isolate

- **Every `unsafe` block must have a `// SAFETY:` comment** explaining why the operation is safe.
- Wrap unsafe operations in **safe abstractions** where possible. Consumers should not need `unsafe`.
- **Maximum unsafe block size: 5 statements**. If more, extract a helper function with a `# Safety` doc comment.

### 2.2 SAFETY Comment Format

```rust
// SAFETY: <precondition> — <why it holds>
unsafe {
    core::ptr::write_volatile(addr as *mut u32, value);
}
```

**Good examples**:
```rust
// SAFETY: addr is GICD_IGROUPR0 (0x08000080), identity-mapped and within GICD MMIO range
unsafe {
    let val = core::ptr::read_volatile(addr as *const u32);
}

// SAFETY: VTTBR_EL2 valid — set during boot via Stage2Config::install().
// Identity-mapped page tables guarantee l0_table PA == VA.
unsafe {
    let l0_entry = *(self.l0_table as *const u64).add(l0_idx);
}

// SAFETY: Single-threaded init — called once from rust_main() before any concurrent access
unsafe {
    MANIFEST = Some(manifest);
}
```

**Bad examples** (insufficient justification):
```rust
// SAFETY: we need unsafe here          ← WHY is it safe?
// SAFETY: trust me                     ← Not a justification
// SAFETY: same as above                ← Each block needs its own reasoning
```

### 2.3 Permitted Unsafe Patterns

| Pattern | Risk | Required Justification |
|---------|------|----------------------|
| `asm!("mrs/msr ...")` | LOW | Register name + expected context (EL2/S-EL2) |
| `core::ptr::read_volatile` | MEDIUM | Address validity + alignment |
| `core::ptr::write_volatile` | MEDIUM | Address validity + alignment + no concurrent writers |
| `core::ptr::read_unaligned` | MEDIUM | Bounds check before read + source validity |
| `UnsafeCell::get()` deref | HIGH | Exclusive access proof (single-pCPU / lock held / init-only) |
| `static mut` access | HIGH | Single-threaded context proof |
| Page table pointer deref | HIGH | VTTBR valid + identity-mapped + entry within table bounds |
| `core::ptr::copy_nonoverlapping` | MEDIUM | src/dst valid + no overlap + size within allocation |

### 2.4 Prohibited Unsafe Patterns

- **`core::mem::transmute`** — use `as` casts or `from_ne_bytes()` / `to_ne_bytes()`.
- **Raw pointer arithmetic beyond allocation bounds** — always validate offset before deref.
- **`static mut` in concurrent code** — use `AtomicXxx` or `SpinLock<T>` instead.

---

## 3. Inline Assembly Rules

### 3.1 General Rules

- **All assembly in `.S` files** — no `global_asm!()`. The `build.rs` cross-compiles `.S` via `aarch64-linux-gnu-gcc`.
- **`core::arch::asm!()` for system register access only** — MRS, MSR, ISB, DSB, TLB invalidation, SMC.
- **Always specify `options()`**: `nostack` (no stack use), `nomem` (no memory side effects), or document why omitted.

```rust
// GOOD: options specified
unsafe {
    asm!("mrs {}, esr_el2", out(reg) esr, options(nostack, nomem));
}

// BAD: options omitted without justification
unsafe {
    asm!("mrs {}, esr_el2", out(reg) esr);
}
```

### 3.2 Barrier Requirements

| After this operation... | Required barrier |
|------------------------|-----------------|
| MSR to system register (except DAIF) | `isb` |
| MSR to VTTBR_EL2 / TTBR0_EL2 | `isb` |
| Write to page table entry | `dsb ish` + `tlbi` + `dsb ish` + `isb` |
| Write_volatile to GICD | None (device memory) |
| Write_volatile to page table entry | `dsb ish` (before TLB invalidation) |

### 3.3 Break-Before-Make Protocol (CRITICAL)

When modifying an existing valid page table descriptor, ARM architecture (D5.7) requires:

```rust
// Step 1: Write INVALID descriptor (break)
unsafe { core::ptr::write_volatile(pte_ptr, 0u64); }

// Step 2: TLB invalidation + barrier
Self::tlbi_all();   // tlbi alle2 + dsb ish + isb

// Step 3: Write NEW descriptor (make)
unsafe { core::ptr::write_volatile(pte_ptr, new_desc); }

// Step 4: Final TLB invalidation
Self::tlbi_all();
```

**Violation consequence**: Translation faults, TLB corruption, data loss. This sequence must never be optimized or reordered.

---

## 4. SMCCC Calling Convention

### 4.1 Register Preservation Rules

Per [SMC Calling Convention](https://developer.arm.com/documentation/den0028/latest/) (DEN0028):

| Registers | Preservation | Role |
|-----------|-------------|------|
| x0-x3 | Input/Output | Function ID + args + return values |
| x4-x7 | Input/Output (FF-A 8-reg) | Additional args for FFA_DIRECT_REQ/RESP |
| **x8-x17** | **CORRUPTED** | **Caller must NOT rely on these across SMC** |
| x18 | Platform-specific | Do not use |
| x19-x30 | **Callee-saved** | Preserved across SMC calls |
| SP | Preserved | Stack pointer maintained |

### 4.2 Critical Rule: Never Store Values in x8-x17 Across SMC

```asm
@ BAD — x16/x17 are corrupted by SMC
mov     x16, x4          @ save handle_lo
mov     x17, x5          @ save handle_hi
smc     #0                @ x16, x17 now GARBAGE
cmp     x16, x0           @ UNDEFINED BEHAVIOR

@ GOOD — use callee-saved registers
mov     x24, x4          @ save handle_lo (callee-saved)
mov     x25, x5          @ save handle_hi (callee-saved)
smc     #0                @ x24, x25 preserved
cmp     x24, x0           @ correct
```

> **Lesson learned**: This caused BL33 Test 14 failure. See bugs-and-issues.md.

### 4.3 SMC Forwarding Pattern (Rust)

```rust
// SAFETY: x0-x7 contain valid FF-A arguments. x4-x17 marked as clobbered per SMCCC.
unsafe {
    core::arch::asm!(
        "smc #0",
        inout("x0") x0 => r0,
        inout("x1") x1 => r1,
        inout("x2") x2 => r2,
        inout("x3") x3 => r3,
        in("x4") x4, in("x5") x5, in("x6") x6, in("x7") x7,
        lateout("x4") _, lateout("x5") _, lateout("x6") _,
        lateout("x7") _, lateout("x8") _, lateout("x9") _,
        lateout("x10") _, lateout("x11") _, lateout("x12") _,
        lateout("x13") _, lateout("x14") _, lateout("x15") _,
        lateout("x16") _, lateout("x17") _,
        options(nomem, nostack),
    );
}
```

**Rule**: Always declare `lateout("x4")` through `lateout("x17")` as clobbered. Omitting these is a **latent correctness bug**.

### 4.4 SMC vs HVC PC Adjustment

| Instruction | ELR_EL2 points to... | Action needed |
|-------------|----------------------|---------------|
| SMC #0 | The SMC instruction itself | `context.pc += 4` after handling |
| HVC #0 | Instruction **after** HVC | No adjustment needed |
| WFI/WFE | The WFI/WFE instruction | No adjustment (re-execute or advance) |
| Data Abort | Faulting instruction | No adjustment (emulate in-place) |

---

## 5. Context Save/Restore

### 5.1 VcpuContext Layout

The `VcpuContext` struct at offset 0 maps directly to `exception.S` save/restore:

```
Offset   0: x0-x1     (16 bytes)
Offset  16: x2-x3     (16 bytes)
...
Offset 240: x30       (8 bytes)
Offset 248: sp_el1    (8 bytes)
Offset 256: elr_el1   (8 bytes)
Offset 264: spsr_el1  (8 bytes)
Offset 384: sp        (8 bytes, host)
Offset 392: pc        (8 bytes, guest ELR_EL2)
Offset 400: spsr_el2  (8 bytes, guest PSTATE)
```

**Rule**: Any modification to `VcpuContext` struct field order or size requires **simultaneous update** to `exception.S` offsets. Mismatch causes silent register corruption.

### 5.2 EL1 System Register Save/Restore

When switching between SPs or between SP and NWd:

```rust
// Save BEFORE context switch
sp.el1_state.save();    // 18 registers via MRS

// Restore AFTER context switch
sp.el1_state.restore(); // 18 registers via MSR + ISB
```

**Required registers** (SpEl1State): SCTLR_EL1, TTBR0/1_EL1, TCR_EL1, MAIR_EL1, VBAR_EL1, CPACR_EL1, CONTEXTIDR_EL1, TPIDR_EL0/EL1/TPIDRRO_EL0, PAR_EL1, CNTKCTL_EL1, AFSR0/1_EL1, ESR_EL1, FAR_EL1, AMAIR_EL1, MDSCR_EL1.

> **Lesson learned**: Missing SpEl1State save/restore caused pKVM DIRECT_REQ failures. pKVM corrupts EL1 sysregs during world switch.

### 5.3 Guest SPSR_EL2 — Never Modify

```rust
// BAD — causes guest spinlock deadlock
context.spsr_el2 &= !(1 << 7); // Clearing PSTATE.I

// GOOD — guest manages its own interrupt mask
// (do not touch SPSR_EL2 from hypervisor)
```

**Rationale**: Guest OS assumes it has exclusive control of PSTATE.I (interrupt mask). Overriding it from EL2 causes spinlock holders to be interrupted while holding locks.

---

## 6. MMIO and Device Emulation

### 6.1 Volatile Access

All MMIO register access **must** use volatile operations:

```rust
// SAFETY: addr is a valid MMIO register within the device's mapped range
unsafe {
    let val = core::ptr::read_volatile(addr as *const u32);
    core::ptr::write_volatile(addr as *mut u32, new_val);
}
```

**Never use** plain pointer dereference (`*ptr`) for MMIO — the compiler may optimize away or reorder accesses.

### 6.2 HPFAR_EL2 for Guest IPA (CRITICAL)

When Stage-2 is enabled (`HCR_EL2.VM=1`), `FAR_EL2` holds the **guest VA**, not IPA:

```rust
// CORRECT: IPA from HPFAR_EL2 + FAR_EL2
let ipa = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8 | (far_el2 & 0xFFF);

// WRONG: FAR_EL2 alone (only works when guest MMU is off)
let ipa = far_el2; // BUG — this is a VA!
```

### 6.3 Device Register Abstraction

Use the enum-dispatch pattern with `contains()` for address routing:

```rust
pub enum Device {
    Uart(VirtualUart),
    Gicd(VirtualGicd),
    // ...
}

impl Device {
    pub fn contains(&self, addr: u64) -> bool { /* base..base+size */ }
    pub fn read(&mut self, offset: u64, size: u8) -> Option<u64> { /* dispatch */ }
    pub fn write(&mut self, offset: u64, value: u64, size: u8) -> bool { /* dispatch */ }
}
```

**Rule**: Every new emulated device must implement PrimeCell ID registers (0xFE0-0xFFC) for Linux `amba` bus probe. Missing IDs cause `OF_BAD_ADDR` in dmesg.

---

## 7. Concurrency and Synchronization

### 7.1 Single-pCPU vs Multi-pCPU

| Mechanism | Single-pCPU | Multi-pCPU |
|-----------|-------------|------------|
| Device access | `UnsafeCell` (no locking) | `SpinLock<T>` (ticket lock) |
| SGI/SPI bitmasks | `AtomicU32` | `AtomicU32` |
| Per-CPU context | Global variable | `TPIDR_EL2` (hardware-banked) |
| SPI delivery | Local injection | Physical SGI 0 cross-CPU wake |

**Rule**: Code that accesses shared state must be feature-gated:
```rust
#[cfg(not(feature = "multi_pcpu"))]
// UnsafeCell access OK — single pCPU, no concurrency

#[cfg(feature = "multi_pcpu")]
// Must hold SpinLock or use atomics
```

### 7.2 Atomic Ordering

| Operation | Required Ordering | Rationale |
|-----------|------------------|-----------|
| Flag set/check (non-critical) | `Relaxed` | No ordering dependency |
| CURRENT_VM_ID store | `Release` | Pairs with `Acquire` loads in exception handler |
| SP_IRQ_PREEMPTED swap | `Acquire` | Must see SP register state written before flag |
| Lock acquire | `Acquire` | Standard lock semantics |
| Lock release | `Release` | Standard lock semantics |

**Rule**: Do not use `SeqCst` — it generates unnecessary `dmb` barriers on ARM64. `Acquire`/`Release` pairs are sufficient.

### 7.3 Deadlock Prevention

**inject_spi() must NOT acquire the DEVICES lock** — it may be called from within a DEVICES lock (e.g., virtio interrupt signaling). In multi-pCPU mode, read physical GICD_IROUTER directly (EL2 bypasses Stage-2).

**SpinLock is non-reentrant** — re-locking on the same CPU will deadlock. Structure code to avoid nested lock acquisition.

---

## 8. Feature Flags and Conditional Compilation

### 8.1 Feature Hierarchy

```
default (unit tests only)
├── guest (Zephyr)
├── linux_guest
│   ├── multi_pcpu (1:1 vCPU-to-pCPU)
│   ├── multi_vm (2 VMs time-sliced)
│   └── tfa_boot (TF-A BL33 mode)
└── sel2 (S-EL2 SPMC)
```

### 8.2 Mutual Exclusivity

- `multi_pcpu` ⊕ `multi_vm` — different scheduling models
- `sel2` ⊕ `linux_guest` — different privilege levels
- `sel2` ⊕ `guest` — different boot paths

**Rule**: Add `compile_error!` guards for invalid combinations:
```rust
#[cfg(all(feature = "multi_pcpu", feature = "multi_vm"))]
compile_error!("multi_pcpu and multi_vm are mutually exclusive");
```

### 8.3 Gating Pattern

```rust
// Module-level: entire module gated
#[cfg(feature = "sel2")]
pub mod sel2_mmu;

// Function-level: alternate implementations
#[cfg(feature = "multi_pcpu")]
fn get_context() -> *mut VcpuContext { /* TPIDR_EL2 */ }

#[cfg(not(feature = "multi_pcpu"))]
fn get_context() -> *mut VcpuContext { /* global variable */ }

// Block-level: minimal scope
let hcr = HCR_BASE
    | HCR_TSC   // SMC trap
    #[cfg(not(feature = "sel2"))]
    { | HCR_FMO }  // FIQ routing (NS-EL2 only)
    ;
```

---

## 9. Testing Standards

### 9.1 Test Structure

Each test module in `tests/` follows:

```rust
pub fn run_xxx_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Test Suite Name\n");
    uart_puts(b"========================================\n\n");

    // Test 1: Description
    uart_puts(b"[TAG] Test 1: ...\n");
    if condition {
        uart_puts(b"[TAG] Test 1 PASSED\n\n");
    } else {
        uart_puts(b"[TAG] FAILED: reason\n");
    }

    uart_puts(b"=== Test Suite: All N tests PASSED ===\n");
}
```

### 9.2 Test Categories

| Category | Location | Invocation |
|----------|----------|------------|
| Unit tests | `src/tests/` | `make run` (33 suites, ~347 assertions) |
| BL33 integration | `tfa/bl33_ffa_test/start.S` | `make run-spmc` (14 tests) |
| Linux FF-A | `guest/linux/ffa-test/` | `make run-pkvm` (kernel module) |

### 9.3 Test Rules

- **Every new FF-A handler** must have a unit test in `test_ffa.rs` or `test_spmc_handler.rs`.
- **Every new device** must have a dedicated test module with PrimeCell ID validation.
- **Update test counts in CLAUDE.md** when adding/removing assertions.
- **Feature-gated tests** must not break default builds. Guard with `#[cfg(feature = "...")]`.
- **Stale VTTBR_EL2**: Clear `VTTBR_EL2` at the end of page table tests and start of FF-A tests to avoid cross-test contamination.

---

## 10. BL33 / SP Assembly Test Code

### 10.1 BL33 Test Pattern

```asm
@ Test N: Description
    adr     x0, str_tN
    bl      uart_print          @ Print test header

    @ Setup
    ldr     x0, =FFA_FUNCTION_ID
    mov     x1, #arg1
    smc     #0                  @ FF-A call

    @ Verify x0 == expected
    ldr     x8, =EXPECTED_VALUE
    cmp     x0, x8
    b.ne    .Lfail_N

    @ PASS
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_next

.Lfail_N:
    adr     x0, str_fail
    bl      uart_print
```

### 10.2 SP Message Loop Pattern

```asm
.Lmsg_loop:
    @ Check x0 == FFA_DIRECT_REQ_32
    ldr     x8, =FFA_DIRECT_REQ_32
    cmp     x0, x8
    b.ne    .Lunknown_msg

    @ Swap source/dest in x1
    lsr     x8, x1, #16        @ x8 = source
    and     x9, x1, #0xFFFF    @ x9 = dest
    orr     x1, x8, x9, lsl #16

    @ Process command (dispatch on x3)
    @ ...

    @ Respond
    ldr     x0, =FFA_DIRECT_RESP_32
    smc     #0
    b       .Lmsg_loop
```

### 10.3 SP Rules

- **Save callee-saved registers** (x19-x30) before any SMC call from SP.
- **FFA_MSG_WAIT on unknown messages** — never hang. Go back to idle.
- **Error responses**: Always send DIRECT_RESP even on failure. Never leave the NWd waiting.
- **No infinite loops without SMC** — SP must always yield back to SPMC.

---

## 11. Known Pitfalls (Project-Specific)

These are hard-won lessons from debugging. Treat as **mandatory checklist** items:

| # | Pitfall | Consequence | Rule |
|---|---------|-------------|------|
| 1 | SMCCC x8-x17 corrupted across SMC | Silent data corruption | Use x19-x30 for values surviving SMC |
| 2 | FAR_EL2 is VA, not IPA | Wrong MMIO address | Use HPFAR_EL2 for IPA |
| 3 | Modifying guest SPSR_EL2 | Spinlock deadlock | Never touch PSTATE.I from EL2 |
| 4 | Missing ISB after MSR | Stale register value | Always `isb` after MSR |
| 5 | CPTR_EL3.TFP=1 traps FP from S-EL2 | Silent hang on `read_volatile` | CTX_INCLUDE_FPREGS=1 in TF-A |
| 6 | SPMD is per-CPU | Secondary CPUs miss events | Each CPU runs own event loop |
| 7 | SpEl1State not saved across world switch | EL1 sysreg corruption | Save/restore around enter_guest() |
| 8 | inject_spi() inside DEVICES lock | Deadlock | Read GICD_IROUTER directly |
| 9 | CNTHP timer preempts SP without owned INTIDs | Spurious FFA_INTERRUPT | Re-arm timer, return true |
| 10 | VcpuContext struct/asm offset mismatch | Silent register corruption | Update both simultaneously |
| 11 | FIQ vectors routing to sync handler | Stale ESR_EL2, hangs | FIQ/IRQ vectors → irq_exception_handler |
| 12 | HCR_EL2.VM not set on secondary CPUs | No Stage-2 translation | Set explicitly in secondary init |

---

## 12. Code Review Checklist

Before merging, verify:

- [ ] Every `unsafe` block has a `// SAFETY:` comment
- [ ] No `.unwrap()` or `.expect()` in exception handlers
- [ ] `asm!()` blocks have `options(nostack, nomem)` or documented justification
- [ ] SMC forwarding declares `lateout("x4")` through `lateout("x17")`
- [ ] Page table modifications follow break-before-make
- [ ] New device implements PrimeCell ID registers
- [ ] VcpuContext changes have matching `exception.S` updates
- [ ] Feature-gated code compiles under both feature on/off
- [ ] Test counts updated in CLAUDE.md
- [ ] No `SeqCst` atomics (use `Acquire`/`Release`)
- [ ] No `static mut` in concurrent code paths
- [ ] ISB after every MSR to system register

---

## References

- [Safety-Critical Rust Coding Guidelines](https://coding-guidelines.arewesafetycriticalyet.org/) — Rust Foundation Consortium
- [The Embedded Rust Book](https://docs.rust-embedded.org/book/) — rust-embedded WG
- [High Assurance Rust](https://highassurance.rs/) — MISRA-inspired safe subset
- [Ferrocene](https://ferrocene.dev/) — ISO 26262 / IEC 61508 qualified Rust compiler
- [SMCCC (DEN0028)](https://developer.arm.com/documentation/den0028/latest/) — SMC Calling Convention
- [FF-A (DEN0077A)](https://developer.arm.com/documentation/den0077/latest/) — Firmware Framework for Arm
- [ARM Architecture Reference Manual](https://developer.arm.com/documentation/ddi0487/latest/) — D5.7 Break-before-make

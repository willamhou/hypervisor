# ARM64 Hypervisor Architecture Reference

> **Codebase**: ~7,700 lines across 30+ source files (Rust + ARM64 Assembly)
> **Target**: QEMU `virt` machine with ARMv8 Virtualization Extensions
> **Guests**: Boots Linux 6.12 (arm64) and Zephyr RTOS

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Boot & Initialization](#2-boot--initialization)
3. [vCPU Management](#3-vcpu-management)
4. [VM Management](#4-vm-management)
5. [Stage-2 Memory Management](#5-stage-2-memory-management)
6. [Exception Handling](#6-exception-handling)
7. [GICv3 Interrupt Controller](#7-gicv3-interrupt-controller)
8. [Timer Virtualization](#8-timer-virtualization)
9. [Device Emulation Framework](#9-device-emulation-framework)
10. [Guest Boot](#10-guest-boot)
11. [Architecture Abstractions](#11-architecture-abstractions)
12. [Appendices](#appendices)

---

## 1. System Overview

### 1.1 Architecture Diagram

```
 ┌──────────────────────────────────────────────────────────────────┐
 │                         QEMU virt Machine                        │
 │  CPU: max (ARMv8+VHE)   RAM: 1GB   GIC: v3   UART: PL011      │
 └──────────────────────────────────────────────────────────────────┘
        │
        │  Hardware
 ═══════╪══════════════════════════════════════════════════════════════
        │  Software
        │
 ┌──────┴──────────────────────────────────────────────────────────┐
 │                        EL2 — Hypervisor                          │
 │                                                                  │
 │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
 │  │  boot.S  │→ │rust_main │→ │ Tests    │→ │  Guest Loader    │ │
 │  │ (entry)  │  │ (init)   │  │          │  │  (Zephyr/Linux)  │ │
 │  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘ │
 │                                                                  │
 │  ┌───────────────────────┐  ┌──────────────────────────────────┐ │
 │  │   Exception Handling  │  │        VM / vCPU Layer           │ │
 │  │  ┌────────────────┐   │  │  ┌──────┐ ┌──────┐ ┌─────────┐ │ │
 │  │  │ exception.S    │   │  │  │ Vm   │ │ Vcpu │ │Scheduler│ │ │
 │  │  │ (vector table) │   │  │  └──────┘ └──────┘ └─────────┘ │ │
 │  │  │ (save/restore) │   │  └──────────────────────────────────┘ │
 │  │  └────────┬───────┘   │                                       │
 │  │           │            │  ┌──────────────────────────────────┐ │
 │  │  ┌────────▼───────┐   │  │       Hardware Interface         │ │
 │  │  │ exception.rs   │   │  │  ┌───────┐ ┌─────┐ ┌──────────┐│ │
 │  │  │ (dispatch)     │   │  │  │GICv3  │ │Timer│ │  MMU     ││ │
 │  │  └────────┬───────┘   │  │  │(ICC/  │ │     │ │(Stage-2) ││ │
 │  │           │            │  │  │ ICH)  │ │     │ │          ││ │
 │  │  ┌────────▼───────┐   │  │  └───────┘ └─────┘ └──────────┘│ │
 │  │  │ decode.rs      │   │  └──────────────────────────────────┘ │
 │  │  │ (MMIO decode)  │   │                                       │
 │  │  └────────┬───────┘   │  ┌──────────────────────────────────┐ │
 │  │           │            │  │      Device Emulation            │ │
 │  │  ┌────────▼───────┐   │  │  ┌────────┐ ┌────────┐          │ │
 │  │  │ DeviceManager  │◄──│──│  │ PL011  │ │ GICD   │          │ │
 │  │  │ (route MMIO)   │   │  │  │ (UART) │ │(Dist.) │          │ │
 │  │  └────────────────┘   │  │  └────────┘ └────────┘          │ │
 │  └───────────────────────┘  └──────────────────────────────────┘ │
 │                                                                  │
 │  ┌──────────────────────────────────────────────────────────────┐ │
 │  │  Memory Management: BumpAllocator → GlobalHeap → PageTables │ │
 │  └──────────────────────────────────────────────────────────────┘ │
 └──────────────────────────────────────────────────────────────────┘
        │
        │  ERET / Exception
 ═══════╪══════════════════════════════════════════════════════════════
        │
 ┌──────┴──────────────────────────────────────────────────────────┐
 │                        EL1 — Guest                               │
 │                                                                  │
 │  ┌──────────────────────────────────────────────────────────────┐ │
 │  │  Linux 6.12 (arm64)  or  Zephyr RTOS  or  Test Guest Code  │ │
 │  │                                                              │ │
 │  │  Sees: Virtual CPU, Virtual GIC (ICC_* via ICH), UART,      │ │
 │  │        Identity-mapped RAM, Virtual Timer (PPI 27)           │ │
 │  └──────────────────────────────────────────────────────────────┘ │
 └──────────────────────────────────────────────────────────────────┘
```

### 1.2 Physical Memory Map

```
 Address          Size        Description
 ────────────────────────────────────────────────────────
 0x0000_0000                  (QEMU firmware ROM)
      ...
 0x0800_0000     64 KB        GIC Distributor (GICD)
 0x0801_0000                  GIC CPU Interface (GICC, GICv2)
 0x080A_0000                  GIC Redistributor (GICR, GICv3)
      ...
 0x0900_0000      4 KB        PL011 UART
      ...
 0x4000_0000                  ┌─ Hypervisor code (.text, .rodata, .data)
                              │  (loaded by QEMU -kernel)
                              │
 0x4000_4000     16 KB        │  Hypervisor stack (grows down)
      ...                     │
 0x4100_0000     16 MB        │  Hypervisor heap (BumpAllocator)
      ...                     │
 0x4200_0000                  └─ End of heap
      ...
 0x4700_0000                  Linux DTB (device tree blob)
 0x4800_0000                  ┌─ Guest code (kernel Image or Zephyr ELF)
                              │  (loaded by QEMU -device loader)
      ...                     │
 0x6800_0000                  └─ End of Linux guest (512 MB)
```

### 1.3 Module Dependency Graph

```
                        main.rs
                           │
              ┌────────────┼──────────────┐
              ▼            ▼              ▼
        guest_loader    tests/         exception::init()
              │                           │
              ▼                           ▼
            vm.rs ◄─────────────── exception.rs
              │                     │         │
     ┌────────┼────────┐           │         ▼
     ▼        ▼        ▼           │     decode.rs
  vcpu.rs  scheduler  mm/mmu.rs    │
     │                    │        │
     ▼                    ▼        ▼
  vcpu_interrupt.rs    defs.rs   global.rs ──► devices/mod.rs
     │                                          │         │
     ▼                                          ▼         ▼
  gicv3.rs ◄──────────────────────────────  pl011/     gic/
     │                                      emulator   distributor
     ▼
  timer.rs

  arch/traits.rs ◄── (implemented by gicv3, mmu, regs, timer)
  platform.rs    ◄── (used by vm, guest_loader, heap, uart, gic)
  lib.rs         ◄── (uart_puts, uart_put_hex — used everywhere)
```

### 1.4 Source File Index

| File | Lines | Purpose |
|------|------:|---------|
| `arch/aarch64/boot.S` | 44 | Entry point, stack setup, BSS clear |
| `arch/aarch64/exception.S` | 469 | Exception vector table, enter_guest, context save/restore |
| `arch/aarch64/linker.ld` | 35 | Linker script, base address 0x4000_0000 |
| `src/lib.rs` | 79 | Crate root, uart_puts/uart_put_hex utilities |
| `src/main.rs` | 170 | rust_main entry, test orchestration, guest dispatch |
| `src/platform.rs` | 30 | QEMU virt board constants (addresses, sizes) |
| `src/global.rs` | 52 | Global DeviceManager for exception handler access |
| `src/arch/mod.rs` | 12 | Architecture module root |
| `src/arch/traits.rs` | 76 | Portable trait definitions |
| `src/arch/aarch64/mod.rs` | 20 | ARM64 module root, re-exports |
| `src/arch/aarch64/defs.rs` | 94 | Named constants for system registers, PTE bits |
| `src/arch/aarch64/regs.rs` | 409 | VcpuContext, GeneralPurposeRegs, SystemRegs, ExitReason |
| `src/arch/aarch64/hypervisor/exception.rs` | 926 | Exception dispatch, PSCI, MMIO, MSR/MRS, WFI, IRQ handlers |
| `src/arch/aarch64/hypervisor/decode.rs` | 135 | Instruction decoder for MMIO emulation |
| `src/arch/aarch64/mm/mmu.rs` | 500 | Stage-2 page tables, IdentityMapper, DynamicIdentityMapper |
| `src/arch/aarch64/peripherals/timer.rs` | 182 | ARM Generic Timer, virtual timer access |
| `src/arch/aarch64/peripherals/gic.rs` | 164 | GICv2 fallback (MMIO-based distributor/CPU interface) |
| `src/arch/aarch64/peripherals/gicv3.rs` | 657 | GICv3 system registers (ICC_\*), virtual interface (ICH_\*) |
| `src/vcpu.rs` | 268 | Virtual CPU, state machine, enter_guest wrapper |
| `src/vcpu_interrupt.rs` | 210 | VirtualInterruptState, HCR_EL2.VI fallback |
| `src/vm.rs` | 303 | Virtual Machine, memory init, scheduler integration |
| `src/scheduler.rs` | 124 | Round-robin vCPU scheduler |
| `src/devices/mod.rs` | 238 | MmioDevice trait, DeviceManager router |
| `src/devices/pl011/emulator.rs` | 235 | Virtual UART (passthrough to real PL011) |
| `src/devices/gic/distributor.rs` | 144 | Virtual GIC Distributor (GICD emulation) |
| `src/mm/allocator.rs` | 51 | Bump allocator (simple, no-free) |
| `src/mm/heap.rs` | 68 | Global heap singleton |
| `src/guest_loader.rs` | 328 | GuestConfig, Linux/Zephyr boot, EL1 register init |
| `src/uart.rs` | 121 | UART driver (direct hardware access) |
| `build.rs` | 74 | Assembly compilation, libboot.a creation |

---

## 2. Boot & Initialization

### 2.1 Purpose

Bring the hypervisor from power-on to a state where it can create VMs and run guest code. The boot sequence starts in assembly at EL2 and transitions to Rust.

### 2.2 Boot Sequence

```
 Power On (QEMU -kernel loads hypervisor to 0x4000_0000)
      │
      ▼
 ┌──────────────────────────────────────────────────────────────┐
 │  _start (boot.S)                                             │
 │  1. Set SP to stack_top (0x4000_0000 + 0x4000)               │
 │  2. Clear BSS (__bss_start → __bss_end)                      │
 │  3. bl rust_main                                             │
 └──────────────────────────────────────────────────────────────┘
      │
      ▼
 ┌──────────────────────────────────────────────────────────────┐
 │  rust_main() (main.rs)                                       │
 │                                                              │
 │  1. exception::init()                                        │
 │     ├─ Write VBAR_EL2 ← &exception_vector_table             │
 │     └─ Configure HCR_EL2 (RW|SWIO|FMO|IMO|AMO|FB|BSU|      │
 │                             TWI|TWE|APK|API)                 │
 │                                                              │
 │  2. gicv3::init()                                            │
 │     ├─ ICC_SRE_EL2 = SRE | Enable                           │
 │     ├─ ICC_SRE_EL1 = SRE                                    │
 │     ├─ GicV3VirtualInterface::init()                         │
 │     │  ├─ ICH_HCR_EL2 = 1 (En)                              │
 │     │  ├─ ICH_VMCR_EL2 = VPMR=0xFF | VENG1=1                │
 │     │  └─ Clear all List Registers                           │
 │     ├─ ICC_CTLR_EL1.EOImode = 1                             │
 │     └─ GicV3SystemRegs::enable() (PMR=0xFF, IGRPEN1=1)      │
 │                                                              │
 │  3. timer::init_hypervisor_timer()                           │
 │     └─ CNTHCTL_EL2 |= EL1PCTEN | EL1PCEN                   │
 │                                                              │
 │  4. mm::heap::init()                                         │
 │     └─ BumpAllocator::new(0x4100_0000, 0x100_0000)          │
 │                                                              │
 │  5. Run test suite (12 tests)                                │
 │                                                              │
 │  6. [feature = "guest"]     → run_guest(zephyr_default)      │
 │     [feature = "linux_guest"] → run_guest(linux_default)     │
 │                                                              │
 │  7. loop { wfe }                                             │
 └──────────────────────────────────────────────────────────────┘
```

### 2.3 Exception Vector Table Layout

```
 VBAR_EL2 (2KB aligned, .text.exception section)

 Offset   Vector                        Handler
 ──────── ─────────────────────────────  ─────────────────────────
 +0x000   Sync,  Current EL, SP_EL0     → exception_handler
 +0x080   IRQ,   Current EL, SP_EL0     → exception_handler
 +0x100   FIQ,   Current EL, SP_EL0     → exception_handler
 +0x180   SError,Current EL, SP_EL0     → exception_handler

 +0x200   Sync,  Current EL, SP_ELx     → exception_handler
 +0x280   IRQ,   Current EL, SP_ELx     → exception_handler
 +0x300   FIQ,   Current EL, SP_ELx     → exception_handler
 +0x380   SError,Current EL, SP_ELx     → exception_handler

 +0x400   Sync,  Lower EL, AArch64      → exception_handler  ← Guest traps
 +0x480   IRQ,   Lower EL, AArch64      → irq_exception_handler ← Physical IRQs
 +0x500   FIQ,   Lower EL, AArch64      → exception_handler
 +0x580   SError,Lower EL, AArch64      → exception_handler

 +0x600   Sync,  Lower EL, AArch32      → exception_handler  (unsupported)
 +0x680   IRQ,   Lower EL, AArch32      → exception_handler
 +0x700   FIQ,   Lower EL, AArch32      → exception_handler
 +0x780   SError,Lower EL, AArch32      → exception_handler

 Each entry: .align 7 (128 bytes), contains a single branch instruction.
 Total: 16 entries x 128 bytes = 2048 bytes (2KB).
```

### 2.4 Key Types

**No types defined in boot code.** Boot is pure assembly.

### 2.5 Source Files

- `arch/aarch64/boot.S` — Entry point, stack, BSS clear
- `arch/aarch64/exception.S` — Vector table, enter_guest, context save/restore
- `arch/aarch64/linker.ld` — Memory layout, section placement
- `src/main.rs` — rust_main, test dispatch, guest boot
- `build.rs` — Compiles assembly, creates libboot.a

---

## 3. vCPU Management

### 3.1 Purpose

Represents a single virtual processor. Manages the guest register context, execution lifecycle, and virtual interrupt state.

### 3.2 Key Types

```rust
// src/vcpu.rs

pub enum VcpuState {
    Uninitialized,   // Not yet configured
    Ready,           // Can be scheduled
    Running,         // Executing in EL1
    Stopped,         // Terminated
}

pub struct Vcpu {
    id: usize,                          // Unique identifier (0..7)
    state: VcpuState,                   // Lifecycle state
    context: VcpuContext,               // All guest registers (repr(C))
    virt_irq: VirtualInterruptState,    // Pending virtual IRQs/FIQs
}
```

### 3.3 vCPU State Machine

```
                     ┌─────────────────┐
                     │  Uninitialized   │
                     └────────┬────────┘
                              │ new(id, entry, sp)
                              ▼
            ┌────────► ┌───────────┐ ◄──────────┐
            │          │   Ready   │             │
            │          └─────┬─────┘             │
            │                │ run()             │
            │                ▼                   │
            │          ┌───────────┐             │
            │          │  Running  │─────────────┘
            │          │  (EL1)    │  guest exit → state = Ready
            │          └─────┬─────┘
            │                │ fatal error
            │                ▼
            │          ┌───────────┐
            └──────────│  Stopped  │
              reset()  └───────────┘
```

### 3.4 VcpuContext Memory Layout (repr(C))

This layout is **shared between Rust and assembly**. The offsets are hardcoded in `exception.S`.

```
 Offset  Field               Size    Notes
 ──────  ──────────────────  ──────  ─────────────────────────
      0  gp_regs.x0           8     ┐
      8  gp_regs.x1           8     │
     16  gp_regs.x2           8     │
     24  gp_regs.x3           8     │
     ..  ...                  ...    │ GeneralPurposeRegs
    224  gp_regs.x28          8     │ (31 x 8 = 248 bytes)
    232  gp_regs.x29 (FP)     8     │
    240  gp_regs.x30 (LR)     8     ┘
    248  sys_regs.sp_el1       8     ┐
    256  sys_regs.elr_el1      8     │
    264  sys_regs.spsr_el1     8     │
    272  sys_regs.sctlr_el1    8     │
    280  sys_regs.ttbr0_el1    8     │
    288  sys_regs.ttbr1_el1    8     │ SystemRegs
    296  sys_regs.tcr_el1      8     │ (17 x 8 = 136 bytes)
    304  sys_regs.mair_el1     8     │
    312  sys_regs.vbar_el1     8     │
    320  sys_regs.contextidr   8     │
    328  sys_regs.tpidr_el1    8     │
    336  sys_regs.tpidrro_el0  8     │
    344  sys_regs.tpidr_el0    8     │
    352  sys_regs.esr_el2      8     │
    360  sys_regs.far_el2      8     │
    368  sys_regs.hcr_el2      8     │
    376  sys_regs.cntvoff_el2  8     ┘
    384  sp                    8     Guest stack pointer
    392  pc                    8     Guest program counter (ELR_EL2)
    400  spsr_el2              8     Guest PSTATE on trap

 Total: 408 bytes

 Assembly accesses (exception.S):
   [x0, #0]    → x0, x1 (stp/ldp pair)
   [x0, #248]  → sp_el1
   [x0, #256]  → elr_el1
   [x0, #264]  → spsr_el1
   [x0, #384]  → sp
   [x0, #392]  → pc (ELR_EL2)
   [x0, #400]  → spsr_el2
```

### 3.5 Public API

| Function | Signature | Description |
|----------|-----------|-------------|
| `new` | `(id: usize, entry: u64, sp: u64) -> Self` | Create Ready vCPU |
| `run` | `(&mut self) -> Result<(), &'static str>` | Enter guest, returns on exit |
| `stop` | `(&mut self)` | Transition to Stopped |
| `reset` | `(&mut self, entry: u64, sp: u64)` | Reset to Ready |
| `inject_irq` | `(&mut self, irq_num: u32)` | Queue virtual interrupt |
| `has_pending_interrupt` | `(&self) -> bool` | Check pending state |
| `clear_irq` | `(&mut self)` | Clear pending interrupt |
| `context_mut` | `(&mut self) -> &mut VcpuContext` | Access registers |

### 3.6 Implementation Notes

- `run()` calls assembly `enter_guest()` which does ERET into EL1
- Return values from `enter_guest()`: 0 = normal exit, 1 = WFI exit
- Before entering guest, `apply_to_hcr()` sets HCR_EL2.VI bit (legacy mode only; GICv3 uses List Registers)
- After exit, pending interrupts are auto-cleared (hardware handled EOI)

### 3.7 Source Files

- `src/vcpu.rs` — Vcpu struct, state machine, run/stop/reset
- `src/arch/aarch64/regs.rs` — VcpuContext, GeneralPurposeRegs, SystemRegs, ExitReason
- `src/vcpu_interrupt.rs` — VirtualInterruptState, HCR_EL2 helpers

---

## 4. VM Management

### 4.1 Purpose

Groups vCPUs, Stage-2 memory, and emulated devices into a Virtual Machine. Provides lifecycle management and scheduler integration.

### 4.2 Key Types

```rust
// src/vm.rs

pub const MAX_VCPUS: usize = 8;

pub enum VmState {
    Uninitialized,    // No vCPUs, no memory
    Ready,            // Configured, can run
    Running,          // Active execution
    Paused,           // Suspended
    Stopped,          // Terminated
}

pub struct Vm {
    id: usize,
    state: VmState,
    vcpus: [Option<Vcpu>; MAX_VCPUS],   // Up to 8 vCPUs
    vcpu_count: usize,
    memory_initialized: bool,
    scheduler: Scheduler,
}
```

### 4.3 VM State Machine

```
 ┌─────────────────┐
 │  Uninitialized   │ ◄── Vm::new(id)
 └────────┬────────┘
          │ create_vcpu() / add_vcpu()
          ▼
 ┌─────────────┐           ┌─────────────┐
 │    Ready    │ ◄────────│   Paused     │
 └──────┬──────┘ resume()  └──────▲──────┘
        │ run()                   │ pause()
        ▼                         │
 ┌─────────────┐─────────────────┘
 │   Running   │
 └──────┬──────┘
        │ guest exit → Ready
        │ stop()
        ▼
 ┌─────────────┐
 │   Stopped   │
 └─────────────┘
```

### 4.4 init_memory Flow

```
 vm.init_memory(guest_mem_start, guest_mem_size)
      │
      ├─ IdentityMapper::reset()
      │
      ├─ map_region(start_aligned, size_aligned, NORMAL)
      │    └─ Guest RAM: identity-mapped, write-back cacheable
      │
      ├─ map_region(0x0800_0000, 16MB, DEVICE)
      │    └─ GIC: distributor + redistributors
      │
      ├─ map_region(0x0900_0000, 2MB, DEVICE)
      │    └─ UART: PL011 serial console
      │
      └─ init_stage2(&mapper)
           ├─ HCR_EL2 |= VM           (enable Stage-2)
           ├─ Stage2Config::install()  (VTCR_EL2, VTTBR_EL2)
           └─ tlbi vmalls12e1is        (flush TLB)
```

### 4.5 Public API

| Function | Signature | Description |
|----------|-----------|-------------|
| `new` | `(id: usize) -> Self` | Create VM, install global DeviceManager |
| `init_memory` | `(&mut self, start: u64, size: u64)` | Setup Stage-2 page tables |
| `create_vcpu` | `(&mut self, id: usize) -> Result<&mut Vcpu>` | Create vCPU by ID |
| `add_vcpu` | `(&mut self, entry: u64, sp: u64) -> Result<usize>` | Auto-ID vCPU |
| `run` | `(&mut self) -> Result<()>` | Run vCPU 0 |
| `schedule` | `(&mut self) -> Option<usize>` | Pick next vCPU (round-robin) |
| `run_current` | `(&mut self) -> Result<()>` | Run scheduled vCPU |
| `yield_current` | `(&mut self)` | Yield to next vCPU |
| `block_current` | `(&mut self)` | Block current vCPU |
| `unblock` | `(&mut self, vcpu_id: usize)` | Wake blocked vCPU |

### 4.6 Source Files

- `src/vm.rs` — Vm struct, memory init, scheduler integration
- `src/scheduler.rs` — Round-robin Scheduler

---

## 5. Stage-2 Memory Management

### 5.1 Purpose

Implement Stage-2 address translation (IPA → PA) using ARM64 page tables. All mappings are identity-mapped (IPA == PA). Uses 2MB block descriptors (no 4KB pages).

### 5.2 Key Types

```rust
// src/arch/aarch64/mm/mmu.rs

#[repr(transparent)]
pub struct S2PageTableEntry(u64);       // 64-bit PTE

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [S2PageTableEntry; 512],   // 512 entries x 8 bytes = 4KB
}

pub struct IdentityMapper {             // Static allocation (in BSS)
    l0_table: PageTable,                // 1 table
    l1_table: PageTable,                // 1 table
    l2_tables: [PageTable; 4],          // Up to 4 L2 tables
    l2_count: usize,
}

pub struct DynamicIdentityMapper {      // Heap allocation
    l0_table: u64,                      // Address of heap-allocated table
    l1_table: u64,
    l2_tables: [u64; 4],
    l2_count: usize,
}

pub struct MemoryAttributes { bits: u64 }

// Predefined attributes:
//   NORMAL:   MemAttr=0b1111, S2AP=RW, SH=Inner, AF=1
//   DEVICE:   MemAttr=0b0000, S2AP=RW, SH=Non,   AF=1
//   READONLY: MemAttr=0b1111, S2AP=RO, SH=Inner, AF=1
```

### 5.3 Page Table Hierarchy

```
 48-bit IPA:  [47:39]   [38:30]   [29:21]    [20:0]
              L0 index  L1 index  L2 index   Block offset (2MB)

 ┌────────────────┐
 │  L0 Table      │  512 entries, each covers 512GB
 │  (VTTBR_EL2)   │  Only entry [0] used (first 512GB)
 └───────┬────────┘
         │ Table descriptor → addr of L1
         ▼
 ┌────────────────┐
 │  L1 Table      │  512 entries, each covers 1GB
 │                │  Entries [0], [1], [2] used for
 │                │  GIC (0x0-1GB), UART (0x0-1GB),
 │                │  Guest RAM (1GB-2GB)
 └───────┬────────┘
         │ Table descriptor → addr of L2
         ▼
 ┌────────────────┐
 │  L2 Table(s)   │  512 entries, each covers 2MB block
 │  (up to 4)     │  Block descriptors with attributes
 └────────────────┘

 Page Table Entry Format (Stage-2 Block Descriptor):
 ┌──────────────────────────────────────────────────────────────────┐
 │ 63   ...   48 │ 47          12 │ 11  10 │ 9  8 │ 7  6 │ 5  2 │1│0│
 │    (Upper)    │  Output Addr   │  --  AF│  SH  │ S2AP │MemAttr│T│V│
 └──────────────────────────────────────────────────────────────────┘
 V   = Valid bit (1 = entry is valid)
 T   = Table/Block (0 = block, 1 = table)
 MemAttr[3:0] = Memory type (0b1111=Normal, 0b0000=Device)
 S2AP[1:0]    = Stage-2 Access Permission (01=RO, 11=RW)
 SH[1:0]      = Shareability (00=Non, 11=Inner)
 AF           = Access Flag (must be 1)
```

### 5.4 VTCR_EL2 Configuration

```
 VTCR_EL2 = T0SZ(16) | SL0(Level0) | IRGN0(WB) | ORGN0(WB) |
            SH0(Inner) | TG0(4KB) | PS(48-bit)

 Field      Value   Meaning
 ─────────  ──────  ─────────────────────────────────
 T0SZ       16      48-bit IPA space (64 - 16 = 48)
 SL0        2       Start at Level 0
 IRGN0      0b01    Inner Write-back cacheable
 ORGN0      0b01    Outer Write-back cacheable
 SH0        0b11    Inner Shareable
 TG0        0b00    4KB granule
 PS         0b101   48-bit Physical Address space
```

### 5.5 Public API

| Function | Signature | Description |
|----------|-----------|-------------|
| `IdentityMapper::new` | `() -> Self` | Create empty mapper (const) |
| `map_region` | `(&mut self, start: u64, size: u64, attrs: MemoryAttributes)` | Map 2MB-aligned region |
| `reset` | `(&mut self)` | Clear all mappings |
| `config` | `(&self) -> Stage2Config` | Get VTCR/VTTBR values |
| `install` | `(&self)` | Write to VTCR_EL2 and VTTBR_EL2 |
| `init_stage2` | `(mapper: &IdentityMapper)` | Enable Stage-2 (HCR_EL2.VM=1, TLB flush) |

### 5.6 Implementation Notes

- `IdentityMapper` uses static allocation (in BSS) — no heap needed
- `DynamicIdentityMapper` allocates page tables from global heap
- Maximum 4 L2 tables → 4 x 512 x 2MB = 4TB coverage (more than sufficient)
- `init_stage2` sets HCR_EL2.VM=1 and flushes TLBs with `tlbi vmalls12e1is`

### 5.7 Source Files

- `src/arch/aarch64/mm/mmu.rs` — Page tables, IdentityMapper, DynamicIdentityMapper
- `src/mm/allocator.rs` — BumpAllocator (page allocation for DynamicIdentityMapper)
- `src/mm/heap.rs` — Global heap singleton (backed by BumpAllocator)

---

## 6. Exception Handling

### 6.1 Purpose

Intercept all guest traps at EL2, decode the exception cause, and dispatch to the appropriate handler. This is the core trap-and-emulate loop.

### 6.2 Exception Dispatch Flowchart

```
 Guest @ EL1 executes instruction
      │
      │ Exception / Trap / IRQ
      ▼
 ┌──────────────────────────────────────────────────────────┐
 │  exception.S: exception_handler / irq_exception_handler  │
 │  1. Save x0-x3 on stack                                  │
 │  2. Load current_vcpu_context pointer                    │
 │  3. Save x0-x30, sp_el1, elr_el1, spsr_el1              │
 │  4. Save ELR_EL2 → context.pc                            │
 │  5. Save SPSR_EL2 → context.spsr_el2                     │
 │  6. bl handle_exception / handle_irq_exception           │
 └──────────────┬─────────────────────────┬─────────────────┘
                │ returns true            │ returns false
                ▼                         ▼
        Restore context,              guest_exit:
        ERET → Guest               Restore host regs,
                                   return to enter_guest caller

 ┌──────────────────────────────────────────────────────────┐
 │  handle_exception(context) → bool                        │
 │                                                          │
 │  1. Read ESR_EL2 and FAR_EL2                             │
 │  2. Check exception loop counter (> 100 → FATAL)         │
 │  3. Extract EC from ESR_EL2[31:26]                       │
 │                                                          │
 │     EC=0x01 (WFI/WFE)                                    │
 │     ├─ handle_wfi_with_timer_injection()                 │
 │     ├─ If timer pending: inject IRQ, advance PC → true   │
 │     └─ No timer: → false (exit to host)                  │
 │                                                          │
 │     EC=0x16 (HVC)                                        │
 │     ├─ Extract HVC immediate from ESR[15:0]              │
 │     ├─ 0x4A48: Jailhouse debug console                   │
 │     ├─ x0 & 0x80000000: PSCI call                        │
 │     └─ x0 = 0/1: Custom hypercall                        │
 │                                                          │
 │     EC=0x18 (MSR/MRS trap)                               │
 │     ├─ Decode ISS: Op0, Op1, CRn, CRm, Op2, Rt          │
 │     ├─ MRS (read): emulate_mrs → write to Rt             │
 │     └─ MSR (write): read Rt → emulate_msr               │
 │                                                          │
 │     EC=0x24/0x25 (Data Abort)                            │
 │     ├─ Read FAR_EL2 (faulting address)                   │
 │     ├─ ISV=1: decode from ISS (SAS,SRT,WnR)             │
 │     ├─ ISV=0: decode instruction at context.pc           │
 │     └─ Route to DeviceManager::handle_mmio()             │
 │                                                          │
 │     EC=0x20/0x21 (Instruction Abort)                     │
 │     └─ Dump EL1 state, dump S2 page tables → false       │
 │                                                          │
 │     EC=0x07/0x09/0x19 (FP/SVE trap)                      │
 │     └─ Skip instruction (should not happen) → true       │
 │                                                          │
 │     Other EC                                              │
 │     └─ Log and exit → false                               │
 └──────────────────────────────────────────────────────────┘
```

### 6.3 IRQ Exception Handler

```
 Physical IRQ while guest running (IMO=1 → trap to EL2)
      │
      ▼
 irq_exception_handler (exception.S)
      │ same save/restore as sync handler
      ▼
 handle_irq_exception(context) → bool
      │
      ├─ Acknowledge: ICC_IAR1_EL1 → intid
      ├─ If intid >= 1020: spurious → return true
      │
      ├─ intid == 27 (Virtual Timer PPI):
      │   ├─ mask_guest_vtimer()  (CNTV_CTL_EL0.IMASK=1)
      │   ├─ inject_hw_interrupt(27, 27, 0xA0)   HW=1
      │   │      └─ Virtual EOI auto-deactivates physical
      │   ├─ DO NOT modify SPSR_EL2  ← CRITICAL RULE
      │   └─ EOIR(27)  (priority drop only, EOImode=1)
      │
      └─ Other intid:
          ├─ Log warning
          ├─ EOIR(intid)
          └─ DIR(intid)  (explicit deactivation, non-HW)
```

### 6.4 PSCI Emulation

| Function ID | Handler | Return |
|-------------|---------|--------|
| `0x84000000` | PSCI_VERSION | `0x00000002` (v0.2) |
| `0x8400000A` | PSCI_FEATURES | SUCCESS or NOT_SUPPORTED |
| `0x84000002` | CPU_OFF | Exit guest |
| `0xC4000003` | CPU_ON | SUCCESS (stub) |
| `0xC4000004` | AFFINITY_INFO | 0 (ON) |
| `0x84000006` | MIGRATE_INFO_TYPE | 2 (not supported) |
| `0x84000008` | SYSTEM_OFF | Exit guest |
| `0x84000009` | SYSTEM_RESET | Exit guest |
| `0x84000001` | CPU_SUSPEND | SUCCESS (treat as WFI) |

### 6.5 Critical Rule: Never Modify Guest SPSR_EL2

```
 !! DO NOT clear PSTATE.I (bit 7) in SPSR_EL2 !!

 Why: Guest controls its own interrupt masking.

 If guest holds a spinlock with interrupts disabled (PSTATE.I=1)
 and we force-clear I to deliver a timer IRQ:

   Guest holds spinlock → I=1
        ↓ Hypervisor clears I in SPSR_EL2
   ERET → Guest with I=0
        ↓ Timer IRQ fires immediately
   Guest IRQ handler runs
        ↓ Handler tries to acquire same spinlock
   DEADLOCK in queued_spin_lock_slowpath

 Correct behavior:
   - Virtual IRQ stays pending in List Register
   - Guest ERET with original PSTATE (I=1)
   - When guest does spin_unlock + local_irq_restore
   - Hardware delivers pending virtual IRQ automatically
```

### 6.6 Exception Loop Prevention

```
 static EXCEPTION_COUNT: AtomicU32 (reset on each successful handling)
 const MAX_CONSECUTIVE_EXCEPTIONS: u32 = 100

 On each exception:
   count = EXCEPTION_COUNT.fetch_add(1)
   if count > 100:
     Print FATAL diagnostics (ESR, FAR, PC)
     Loop { wfe }   ← Hard halt

 Resets on: WFI handled, HVC, MSR/MRS, MMIO, IRQ
```

### 6.7 Source Files

- `src/arch/aarch64/hypervisor/exception.rs` — Exception dispatch, PSCI, MSR/MRS, MMIO, WFI, IRQ
- `src/arch/aarch64/hypervisor/decode.rs` — Instruction decoder (ISV and manual decode)
- `arch/aarch64/exception.S` — Vector table, context save/restore, enter_guest

---

## 7. GICv3 Interrupt Controller

### 7.1 Purpose

Manage the GICv3 hardware for both hypervisor operation (physical interrupts) and guest virtualization (virtual interrupt injection via List Registers).

### 7.2 Key Types

```rust
// src/arch/aarch64/peripherals/gicv3.rs

pub struct GicV3SystemRegs;          // ICC_* register access (EL2 physical)
pub struct GicV3VirtualInterface;    // ICH_* register access (virtual injection)

pub const VTIMER_IRQ: u32 = 27;     // Virtual Timer PPI
pub const PTIMER_IRQ: u32 = 30;     // Physical Timer PPI
```

### 7.3 List Register Layout (64-bit)

```
 63  62  61  60  59       48  47       32  31             0
 ┌───┬───┬───┬───────────────┬────────────┬────────────────┐
 │St │HW │Grp│   Priority    │   pINTID   │     vINTID     │
 │[1:0]│   │   │    [7:0]     │   [9:0]    │    [31:0]      │
 └───┴───┴───┴───────────────┴────────────┴────────────────┘

 Field      Bits      Values
 ─────────  ────────  ──────────────────────────────────────
 State      [63:62]   00=Invalid, 01=Pending,
                      10=Active, 11=Pending+Active
 HW         [61]      0=Software, 1=Hardware-linked
 Group      [60]      0=Group0, 1=Group1
 Priority   [55:48]   0x00=highest, 0xFF=lowest
 pINTID     [41:32]   Physical INTID (when HW=1)
 vINTID     [31:0]    Virtual INTID seen by guest
```

### 7.4 HW=1 Interrupt Injection Flow

```
 Physical Timer fires (INTID 27, IMO=1 traps to EL2)
      │
      ▼
 1. Hypervisor acknowledges: ICC_IAR1_EL1 → 27
      │
      ▼
 2. Mask timer: CNTV_CTL_EL0.IMASK = 1
      │
      ▼
 3. Find free LR, write:
    ┌────────────────────────────────────────────────────────┐
    │ State=01(Pending) │ HW=1 │ Grp=1 │ Prio=0xA0 │       │
    │ pINTID=27 │ vINTID=27                                  │
    └────────────────────────────────────────────────────────┘
      │
      ▼
 4. Priority drop: ICC_EOIR1_EL1(27)
    (EOImode=1: EOIR only drops priority, does NOT deactivate)
      │
      ▼
 5. ERET → Guest resumes
      │
      ▼
 6. Guest sees pending virtual IRQ (when PSTATE.I=0)
      │
      ▼
 7. Guest IRQ handler runs, reads ICC_IAR1_EL1 → 27
      │                          (virtual, from ICH_LR)
      ▼
 8. Guest writes ICC_EOIR1_EL1(27)  (virtual EOI)
      │
      ▼
 9. Hardware auto-deactivates physical INTID 27
    because HW=1 and pINTID=27 in the LR
      │
      ▼
 10. LR State → Invalid (free for reuse)
```

### 7.5 EOImode=1 Deactivation Flow

```
 ┌──────────────────────────────────────────────────────┐
 │                   EOImode=1                          │
 │  ICC_EOIR1_EL1 → Priority Drop ONLY                 │
 │  ICC_DIR_EL1   → Deactivation (explicit)            │
 │                                                      │
 │  For HW=1 interrupts (timer):                        │
 │    Guest virtual EOI → auto-deactivates physical     │
 │    Hypervisor does NOT call DIR                       │
 │                                                      │
 │  For non-HW interrupts:                              │
 │    Hypervisor calls EOIR (priority drop)              │
 │    Then calls DIR (deactivation)                      │
 └──────────────────────────────────────────────────────┘
```

### 7.6 GIC Initialization Sequence

```
 gicv3::init()
    │
    ├─ Check ID_AA64PFR0_EL1[27:24] >= 1 (GICv3 available?)
    │   └─ If not: fall back to gic::init() (GICv2)
    │
    ├─ ICC_SRE_EL2 = SRE(bit 0) | Enable(bit 3)
    │   └─ Enable system register interface at EL2
    │   └─ Allow EL1 access to ICC_* registers
    │
    ├─ ICC_SRE_EL1 = SRE(bit 0)
    │   └─ Enable system register interface at EL1
    │
    ├─ GicV3VirtualInterface::init()
    │   ├─ ICH_HCR_EL2 = 1 (En = enable virtual GIC)
    │   ├─ ICH_VMCR_EL2 = VPMR(0xFF) | VENG1(1)
    │   └─ Clear all LRs (write 0)
    │
    ├─ ICC_CTLR_EL1 |= EOImode (bit 1)
    │   └─ Split priority drop / deactivation
    │
    └─ GicV3SystemRegs::enable()
        ├─ ICC_PMR_EL1 = 0xFF (allow all priorities)
        └─ ICC_IGRPEN1_EL1 = 1 (enable Group 1 interrupts)
```

### 7.7 Public API (GicV3SystemRegs)

| Function | Description |
|----------|-------------|
| `read_sre_el2() → u32` | Read ICC_SRE_EL2 |
| `write_sre_el2(u32)` | Write ICC_SRE_EL2 |
| `read_iar1() → u32` | Acknowledge interrupt (returns INTID) |
| `write_eoir1(u32)` | End of Interrupt (priority drop) |
| `write_dir(u32)` | Deactivate Interrupt (explicit) |
| `read_ctlr() → u32` | Read ICC_CTLR_EL1 |
| `write_ctlr(u32)` | Write ICC_CTLR_EL1 |
| `write_pmr(u32)` | Set Priority Mask |
| `write_igrpen1(bool)` | Enable/disable Group 1 |
| `enable()` | PMR=0xFF, IGRPEN1=1 |
| `disable()` | IGRPEN1=0 |

### 7.8 Public API (GicV3VirtualInterface)

| Function | Description |
|----------|-------------|
| `read_hcr() → u32` | Read ICH_HCR_EL2 |
| `write_hcr(u32)` | Write ICH_HCR_EL2 |
| `read_lr(n) → u64` | Read List Register n (0-3) |
| `write_lr(n, u64)` | Write List Register n |
| `inject_interrupt(intid, priority) → Result` | SW inject (HW=0) |
| `inject_hw_interrupt(vintid, pintid, priority) → Result` | HW inject (HW=1) |
| `clear_interrupt(intid)` | Clear LR with matching INTID |
| `pending_count() → usize` | Count pending LRs |
| `find_free_lr() → Option<usize>` | Find invalid-state LR |
| `num_list_registers() → u32` | From ICH_VTR_EL2[4:0]+1 |

### 7.9 Source Files

- `src/arch/aarch64/peripherals/gicv3.rs` — GICv3 system register interface, List Register management
- `src/arch/aarch64/peripherals/gic.rs` — GICv2 fallback (MMIO-based GICD/GICC)

---

## 8. Timer Virtualization

### 8.1 Purpose

Provide guest access to the ARM Generic Timer (virtual timer at PPI 27). Handle timer interrupts by injecting them as virtual interrupts through GICv3 List Registers.

### 8.2 CNTV_CTL_EL0 Register Layout

```
 ┌──────────────────────────────────────────────┐
 │ 63                    3     2      1      0  │
 │  ───── Reserved ─────  ISTATUS  IMASK  ENABLE│
 └──────────────────────────────────────────────┘
 ENABLE   [0]  = 1: Timer enabled
 IMASK    [1]  = 1: Interrupt masked (suppressed)
 ISTATUS  [2]  = 1: Timer condition met (read-only)

 Timer fires when: ENABLE=1 && ISTATUS=1 && IMASK=0
 Hypervisor masks: sets IMASK=1 to stop continuous firing
```

### 8.3 Timer Interrupt Lifecycle

```
 ┌────────────┐
 │ Guest arms │  CNTV_TVAL_EL0 = ticks
 │   timer    │  CNTV_CTL_EL0 = ENABLE
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ Timer      │  Counter reaches compare value
 │ fires      │  ISTATUS=1, physical IRQ 27 asserted
 └─────┬──────┘
       │ IMO=1 → trap to EL2
       ▼
 ┌────────────┐
 │ Hypervisor │  handle_irq_exception():
 │ masks      │  CNTV_CTL_EL0.IMASK = 1
 │ timer      │  (stops continuous firing)
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ Inject     │  ICH_LR: State=Pending, HW=1,
 │ virtual    │  vINTID=27, pINTID=27, Prio=0xA0
 │ interrupt  │
 └─────┬──────┘
       │ EOIR(27) = priority drop
       │ ERET → Guest
       ▼
 ┌────────────┐
 │ Guest IRQ  │  Guest sees pending vIRQ
 │ handler    │  Acknowledges: ICC_IAR1 → 27
 │ runs       │  Handles timer event
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ Guest EOI  │  ICC_EOIR1(27) → virtual EOI
 │            │  HW=1 → auto-deactivates physical
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ Guest      │  Writes new CNTV_TVAL_EL0
 │ re-arms    │  Clears IMASK (CNTV_CTL = ENABLE)
 └────────────┘
       │
       └────────→ (cycle repeats)
```

### 8.4 Timer Register Access Functions

| Function | Register | Description |
|----------|----------|-------------|
| `get_frequency()` | `CNTFRQ_EL0` | Counter frequency in Hz |
| `get_counter()` | `CNTVCT_EL0` | Current virtual counter |
| `get_ctl()` / `set_ctl()` | `CNTV_CTL_EL0` | Timer control |
| `get_cval()` / `set_cval()` | `CNTV_CVAL_EL0` | Compare value |
| `get_tval()` / `set_tval()` | `CNTV_TVAL_EL0` | Countdown value |
| `is_guest_vtimer_pending()` | `CNTV_CTL_EL0` | ENABLE && ISTATUS && !IMASK |
| `mask_guest_vtimer()` | `CNTV_CTL_EL0` | Set IMASK bit |
| `init_hypervisor_timer()` | `CNTHCTL_EL2` | Allow EL1 counter/timer access |
| `init_guest_timer()` | `CNTHCTL_EL2`, `CNTVOFF_EL2` | Guest timer access, offset=0 |

### 8.5 Source Files

- `src/arch/aarch64/peripherals/timer.rs` — Timer register access, init, pending check

---

## 9. Device Emulation Framework

### 9.1 Purpose

Emulate hardware devices that guests interact with through MMIO. Provides a trait-based framework for pluggable device emulation.

### 9.2 Key Types

```rust
// src/devices/mod.rs

pub trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;
    fn contains(&self, addr: u64) -> bool;   // default impl provided
}

pub struct DeviceManager {
    uart: pl011::VirtualUart,    // 0x0900_0000, 4KB
    gicd: gic::VirtualGicd,      // 0x0800_0000, 64KB
}
```

### 9.3 Trap-and-Emulate Data Flow

```
 Guest @ EL1: str w0, [x1]   (x1 = 0x0900_0000 = UART DR)
      │
      │ Stage-2 translation → Data Abort (MMIO region)
      ▼
 ┌──────────────────────────────────────────────────────────┐
 │  exception.S: exception_handler                          │
 │  Save context, call handle_exception(context)            │
 └───────────────────────┬──────────────────────────────────┘
                         │ EC=0x24 (Data Abort from Lower EL)
                         ▼
 ┌──────────────────────────────────────────────────────────┐
 │  handle_mmio_abort(context, FAR_EL2=0x09000000)          │
 │                                                          │
 │  ISV=1?                                                  │
 │  ├─ Yes: Decode from ESR_EL2 ISS                         │
 │  │   SAS[23:22]=size, SRT[20:16]=reg, WnR[6]=direction  │
 │  └─ No:  Decode instruction at context.pc                │
 │       └─ Pattern match on ARM64 LDR/STR encoding         │
 └───────────────────────┬──────────────────────────────────┘
                         │ MmioAccess::Store { reg=0, size=4 }
                         ▼
 ┌──────────────────────────────────────────────────────────┐
 │  global::DEVICES.handle_mmio(0x09000000, value, 4, true) │
 │                                                          │
 │  uart.contains(0x09000000)?  → YES                       │
 │  offset = 0x09000000 - 0x09000000 = 0x000 (UARTDR)      │
 │  uart.write(0x000, value, 4)                             │
 │    └─ output_char(value as u8)  → physical UART          │
 └───────────────────────┬──────────────────────────────────┘
                         │
                         ▼
 context.pc += 4         Advance past faulting instruction
 return true             Continue guest
```

### 9.4 MmioAccess Instruction Decoding

```rust
// src/arch/aarch64/hypervisor/decode.rs

pub enum MmioAccess {
    Load { reg: u8, size: u8, sign_extend: bool },
    Store { reg: u8, size: u8 },
}

// ISV=1 path (ESR_EL2 ISS fields):
//   SAS[23:22] → size: 00=1B, 01=2B, 10=4B, 11=8B
//   SRT[20:16] → register number (0-30)
//   WnR[6]     → 0=read(Load), 1=write(Store)
//   SSE[23]    → sign extend

// ISV=0 path (manual instruction decode):
//   Pattern: (insn & 0x3B000000) == 0x39000000
//   → LDR/STR with unsigned immediate offset
//   Size from insn[31:30], Rt from insn[4:0]
//   Direction from insn[22]
```

### 9.5 PL011 Virtual UART

```
 Register Map (base: 0x0900_0000):

 Offset  Name        RW   Description
 ──────  ──────────  ──   ───────────────────────────────
 0x000   UARTDR      RW   Data: write=TX to physical, read=RX from physical
 0x018   UARTFR      R    Flags: TXFE=1(always ready), RXFE=from real UART
 0x024   UARTIBRD    RW   Integer baud rate (stored, ignored by QEMU)
 0x028   UARTFBRD    RW   Fractional baud rate (stored, ignored)
 0x02C   UARTLCR_H   RW   Line control (stored)
 0x030   UARTCR      RW   Control register (default: 0x0301 = enabled)
 0x038   UARTIMSC    RW   Interrupt mask set/clear
 0x03C   UARTRIS     R    Raw interrupt status
 0x040   UARTMIS     R    Masked interrupt status (= RIS & IMSC)
 0x044   UARTICR     W    Interrupt clear

 Design: Passthrough — TX writes go directly to physical UART,
 RX reads come from physical UART. No buffering.
```

### 9.6 Virtual GIC Distributor

```
 Register Map (base: 0x0800_0000):

 Offset    Name            RW  Description
 ────────  ──────────────  ──  ──────────────────────────
 0x000     GICD_CTLR       RW  Distributor control
 0x004     GICD_TYPER       R   Type: ITLinesNumber=31 (1024 IRQs), CPUs=1
 0x100-    GICD_ISENABLER  RW  Set-enable (32 regs x 32 bits = 1024 IRQs)
 0x17F
 0x180-    GICD_ICENABLER  RW  Clear-enable (same layout)
 0x1FF
 *         (all other)      -   RAZ/WI (Read-As-Zero / Write-Ignore)

 Internal state:
   ctlr: u32           (enable/disable distributor)
   enabled: [u32; 32]  (1024 interrupt enable bits)
```

### 9.7 Global Device Access

```rust
// src/global.rs

pub struct GlobalDeviceManager {
    devices: UnsafeCell<Option<DeviceManager>>,
}

pub static DEVICES: GlobalDeviceManager = GlobalDeviceManager::new();

// Exception handler access (no &mut self needed):
//   global::DEVICES.handle_mmio(addr, value, size, is_write)
//
// Safety: Only one vCPU runs at a time → effectively single-threaded.
```

### 9.8 Source Files

- `src/devices/mod.rs` — MmioDevice trait, DeviceManager router
- `src/devices/pl011/emulator.rs` — VirtualUart (passthrough UART)
- `src/devices/gic/distributor.rs` — VirtualGicd (interrupt enable/disable)
- `src/global.rs` — GlobalDeviceManager (exception handler access)

---

## 10. Guest Boot

### 10.1 Purpose

Load and boot real operating systems (Linux, Zephyr) as guests. Handles ELF parsing, ARM64 Image header parsing, EL1 register initialization, and the Linux boot protocol.

### 10.2 Key Types

```rust
// src/guest_loader.rs

pub enum GuestType {
    Zephyr,    // Zephyr RTOS (ELF or raw binary)
    Linux,     // Linux kernel (ARM64 Image format)
}

pub struct GuestConfig {
    pub guest_type: GuestType,
    pub load_addr: u64,        // Where QEMU loaded the kernel
    pub mem_size: u64,         // Guest RAM size
    pub entry_point: u64,      // Kernel entry address
    pub dtb_addr: u64,         // Device tree blob address
}
```

### 10.3 Guest Boot Flow

```
 run_guest(config)
      │
      ├─ Create VM: Vm::new(0)
      │   └─ Install global DeviceManager
      │
      ├─ Init memory: vm.init_memory(load_addr, mem_size)
      │   └─ Stage-2 page tables, HCR_EL2.VM=1
      │
      ├─ Create vCPU: vm.create_vcpu(0)
      │   ├─ Set PC = entry_point
      │   ├─ Set SP = load_addr + mem_size - 0x1000
      │   └─ [Linux] x0 = dtb_addr, x1-x3 = 0
      │
      ├─ Init guest timer: init_guest_timer()
      │   ├─ CNTHCTL_EL2 |= EL1PCTEN
      │   └─ CNTVOFF_EL2 = 0
      │
      ├─ [Linux] Init EL1 system registers:
      │   ├─ SCTLR_EL1 = 0x30D0_0800 (RES1, MMU off)
      │   ├─ Zero: TCR, TTBR0, TTBR1, MAIR, VBAR
      │   ├─ CPACR_EL1 = 3 << 20 (FP/SIMD enabled)
      │   ├─ CPTR_EL2: clear TZ, TFP, TSM, TCPAC
      │   ├─ MDCR_EL2 = 0
      │   ├─ VPIDR_EL2 = MIDR_EL1 (real CPU ID)
      │   └─ VMPIDR_EL2 = MPIDR_EL1
      │
      ├─ [Linux] Clear TWI/TWE in HCR_EL2
      │   └─ Guest handles its own WFI (no trap)
      │
      ├─ [Linux] Reset exception counters
      │
      └─ vm.run()
          └─ vcpu.run() → enter_guest() → ERET to guest
```

### 10.4 Linux ARM64 Image Header Parsing

```
 Offset  Size  Field
 ──────  ────  ──────────────────
 0x00      4   code0 (branch instruction)
 0x04      4   code1
 0x08      8   text_offset (offset from load address)
 0x10      8   image_size
 0x18      8   flags
 0x20      8   res2
 0x28      8   res3
 0x30      8   res4
 0x38      4   magic (0x644d5241 = "ARMd" LE)
 0x3C      4   res5

 Entry point = kernel_addr + text_offset
 (if text_offset is nonzero and < 0x100000)
```

### 10.5 HCR_EL2 Configuration Differences

| Feature | Test Guests | Linux Guest |
|---------|------------|-------------|
| TWI (trap WFI) | Set | **Cleared** |
| TWE (trap WFE) | Set | **Cleared** |
| VM (Stage-2) | Set during init_memory | Set during init_memory |
| APK/API (PAC) | Set | Set |
| IMO/FMO/AMO | Set | Set |

### 10.6 Source Files

- `src/guest_loader.rs` — GuestConfig, linux_default, zephyr_default, run_guest

---

## 11. Architecture Abstractions

### 11.1 Purpose

Define portable traits that abstract hardware-specific operations, enabling potential future support for other architectures (e.g., RISC-V).

### 11.2 Trait Definitions

```rust
// src/arch/traits.rs

pub trait InterruptController {
    fn init(&mut self);
    fn enable(&mut self);
    fn disable(&mut self);
    fn acknowledge(&mut self) -> u32;
    fn eoi(&mut self, intid: u32);
    fn deactivate(&mut self, intid: u32);
    fn set_priority_mask(&mut self, mask: u8);
}

pub trait VirtualInterruptController {
    fn inject_interrupt(&mut self, intid: u32, priority: u8) -> Result<(), &'static str>;
    fn inject_hw_interrupt(&mut self, vintid: u32, pintid: u32, priority: u8)
        -> Result<(), &'static str>;
    fn clear_interrupt(&mut self, intid: u32);
    fn pending_count(&self) -> usize;
}

pub trait GuestTimer {
    fn init_hypervisor(&mut self);
    fn init_guest(&mut self);
    fn is_pending(&self) -> bool;
    fn mask(&mut self);
    fn get_frequency(&self) -> u64;
    fn get_counter(&self) -> u64;
}

pub trait Stage2Mapper {
    fn map_region(&mut self, ipa: u64, size: u64, mem_type: MemoryType)
        -> Result<(), &'static str>;
    fn reset(&mut self);
    fn install(&self);
    fn root_table_addr(&self) -> u64;
}

pub trait VcpuContextOps {
    fn new(entry: u64, sp: u64) -> Self;
    fn pc(&self) -> u64;
    fn set_pc(&mut self, val: u64);
    fn sp(&self) -> u64;
    fn set_sp(&mut self, val: u64);
    fn get_reg(&self, n: u8) -> u64;
    fn set_reg(&mut self, n: u8, val: u64);
    fn advance_pc(&mut self);
}

pub trait ExceptionInfo {
    fn is_wfi(&self) -> bool;
    fn is_hypercall(&self) -> bool;
    fn is_data_abort(&self) -> bool;
    fn is_instruction_abort(&self) -> bool;
    fn fault_address(&self) -> Option<u64>;
}

pub enum MemoryType { Normal, Device, ReadOnly }
```

### 11.3 Trait Implementations

| Trait | Implementor | Location |
|-------|-------------|----------|
| `VcpuContextOps` | `VcpuContext` | `src/arch/aarch64/regs.rs` |
| `ExceptionInfo` | `ExitReason` | `src/arch/aarch64/regs.rs` |
| `Stage2Mapper` | `DynamicIdentityMapper` | `src/arch/aarch64/mm/mmu.rs` |

### 11.4 Platform Constants

```rust
// src/platform.rs

pub const UART_BASE: usize = 0x0900_0000;
pub const UART_SIZE: u64   = 0x1000;

pub const GICD_BASE: u64   = 0x0800_0000;
pub const GICD_SIZE: u64   = 0x1_0000;
pub const GICC_BASE: u64   = 0x0801_0000;
pub const GIC_REGION_BASE: u64 = 0x0800_0000;
pub const GIC_REGION_SIZE: u64 = 8 * BLOCK_SIZE_2MB;   // 16MB

pub const GUEST_RAM_BASE: u64  = 0x4000_0000;
pub const GUEST_LOAD_ADDR: u64 = 0x4800_0000;
pub const LINUX_DTB_ADDR: u64  = 0x4700_0000;
pub const LINUX_MEM_SIZE: u64  = 512 * 1024 * 1024;    // 512MB
pub const ZEPHYR_MEM_SIZE: u64 = 128 * 1024 * 1024;    // 128MB
pub const GUEST_STACK_RESERVE: u64 = 0x1000;

pub const HEAP_START: u64 = 0x4100_0000;
pub const HEAP_SIZE: u64  = 0x100_0000;                 // 16MB
```

### 11.5 Named Constants (defs.rs)

| Category | Constants |
|----------|-----------|
| HCR_EL2 | `VM`, `SWIO`, `FMO`, `IMO`, `AMO`, `FB`, `BSU_INNER`, `TWI`, `TWE`, `RW`, `APK`, `API` |
| ESR_EL2 | `EC_SHIFT`, `EC_MASK`, `ISS_MASK`, `HVC_IMM_MASK` |
| Exception Class | `EC_UNKNOWN`, `EC_WFI_WFE`, `EC_TRAPPED_SIMD_FP`, `EC_TRAPPED_SVE`, `EC_HVC64`, `EC_MSR_MRS`, `EC_SVE_TRAP`, `EC_IABT_LOWER`, `EC_IABT_SAME`, `EC_DABT_LOWER`, `EC_DABT_SAME` |
| SPSR_EL2 | `SPSR_EL1H_DAIF_MASKED` (0x3C5), `SPSR_EL1H` (0b0101) |
| CPTR_EL2 | `CPTR_TZ`, `CPTR_TFP`, `CPTR_TSM`, `CPTR_TCPAC` |
| ICC registers | `ICC_SRE_SRE`, `ICC_SRE_ENABLE`, `ICC_CTLR_EOIMODE`, `ICC_PMR_ALLOW_ALL` |
| GICv3 LR fields | `LR_STATE_SHIFT`, `LR_STATE_MASK`, `LR_HW_BIT`, `LR_GROUP1_BIT`, `LR_PRIORITY_SHIFT`, `LR_PINTID_SHIFT`, `LR_PINTID_MASK`, `LR_VINTID_MASK`, `VTR_LISTREGS_MASK`, `GIC_SPURIOUS_INTID` |
| IRQ priority | `IRQ_DEFAULT_PRIORITY` (0xA0) |
| VTCR_EL2 | `T0SZ_48BIT`, `SL0_LEVEL0`, `IRGN0_WB`, `ORGN0_WB`, `SH0_INNER`, `TG0_4KB`, `PS_48BIT` |
| CNTHCTL_EL2 | `EL1PCTEN`, `EL1PCEN` |
| Page table | `PTE_VALID`, `PTE_TABLE`, `PTE_ADDR_MASK`, `PAGE_OFFSET_MASK`, `PT_INDEX_MASK`, `BLOCK_SIZE_2MB`, `BLOCK_MASK_2MB` |
| Instruction | `AARCH64_INSN_SIZE` (4) |

### 11.6 Source Files

- `src/arch/traits.rs` — Portable trait definitions
- `src/arch/aarch64/defs.rs` — Named constants
- `src/platform.rs` — QEMU virt board addresses and sizes

---

## Appendices

### Appendix A: HCR_EL2 Bit Reference

```
 Bit   Name   Set?  Purpose
 ────  ─────  ────  ────────────────────────────────────────────
   0   VM      *    Enable Stage-2 translation
                    (* Set by init_stage2, not by exception::init)
   1   SWIO    Y    Set/Way Invalidation Override
   3   FMO     Y    Route physical FIQ to EL2
   4   IMO     Y    Route physical IRQ to EL2
   5   AMO     Y    Route physical SError to EL2
   6   VF      -    Virtual FIQ pending (legacy, cleared in GICv3 mode)
   7   VI      -    Virtual IRQ pending (legacy, cleared in GICv3 mode)
   9   FB      Y    Force Broadcast TLB/cache maintenance
  10   BSU     Y    Barrier Shareability Upgrade = Inner Shareable
  12   DC      N    Default Cacheability — NOT SET (causes stale PTE bug)
  13   TWI     Y*   Trap WFI (*cleared for Linux guests)
  14   TWE     Y*   Trap WFE (*cleared for Linux guests)
  31   RW      Y    EL1 is AArch64
  40   APK     Y    Don't trap PAC key registers
  41   API     Y    Don't trap PAC instructions
```

### Appendix B: ESR_EL2 Exception Class Quick Reference

```
 EC     Name                Description                    Handler
 ─────  ──────────────────  ─────────────────────────────  ─────────────
 0x00   EC_UNKNOWN          Unknown/uncategorized          Log, exit
 0x01   EC_WFI_WFE          WFI/WFE trapped               Timer inject
 0x07   EC_TRAPPED_SIMD_FP  FP/SIMD access trapped        Skip insn
 0x09   EC_TRAPPED_SVE      SVE/SME access trapped         Skip insn
 0x16   EC_HVC64            HVC instruction (AArch64)      PSCI / custom
 0x18   EC_MSR_MRS          Trapped system register        Emulate R/W
 0x19   EC_SVE_TRAP         SVE trapped (CPTR_EL2.TZ)      Skip insn
 0x20   EC_IABT_LOWER       Instruction Abort from EL1     Dump, exit
 0x21   EC_IABT_SAME        Instruction Abort from EL2     Dump, exit
 0x24   EC_DABT_LOWER       Data Abort from EL1            MMIO emulate
 0x25   EC_DABT_SAME        Data Abort from EL2            Dump, exit
```

### Appendix C: List Register Encoding Reference

```
 Bit(s)    Field        Description
 ────────  ──────────── ──────────────────────────────────────────
 [63:62]   State        00=Invalid 01=Pending 10=Active 11=P+A
 [61]      HW           1=Hardware-linked (pINTID valid)
 [60]      Group        0=Group 0, 1=Group 1
 [59:56]   (Reserved)
 [55:48]   Priority     0x00=highest, 0xFF=lowest
 [47:42]   (Reserved)
 [41:32]   pINTID       Physical INTID (only when HW=1)
 [31:0]    vINTID       Virtual INTID seen by guest

 Typical values used:
   SW inject:  State=01 | Group=1 | Prio=0xA0 | vINTID
   HW inject:  State=01 | HW=1 | Group=1 | Prio=0xA0 | pINTID | vINTID
```

### Appendix D: Build System

```
 Build targets (Makefile):
   make              Build hypervisor (cargo build --target aarch64-unknown-none)
   make run          Run in QEMU (tests only, no guest)
   make run-guest    Run with Zephyr ELF  (feature: guest)
   make run-linux    Run with Linux Image (feature: linux_guest)
   make debug        Run with GDB server on port 1234
   make clippy       Run linter
   make fmt          Format code

 QEMU configuration:
   qemu-system-aarch64
     -machine virt,virtualization=on,gic-version=3
     -cpu max
     -smp 1
     -m 1G
     -nographic
     -kernel target/aarch64-unknown-none/debug/hypervisor

 Feature flags (Cargo.toml):
   default      = []           No guest (tests only)
   guest        = []           Enable Zephyr guest
   linux_guest  = []           Enable Linux guest

 Build pipeline (build.rs):
   1. aarch64-linux-gnu-gcc -c boot.S → boot.o
   2. aarch64-linux-gnu-gcc -c exception.S → exception.o
   3. aarch64-linux-gnu-ar crs libboot.a boot.o exception.o
   4. Link with --whole-archive (include all assembly symbols)
```

# Architecture

A bare-metal Type-1 hypervisor for ARM64 in Rust (`no_std`). One codebase, two personalities: an NS-EL2 hypervisor that boots Linux, or an S-EL2 SPMC that manages Secure Partitions alongside Android pKVM. ~7,700 LOC, single external dependency (`fdt`), 34 test suites.

For quick start and features, see [README.md](README.md).
For exhaustive internals, see [docs/architecture.md](docs/architecture.md).

---

## Privilege Model

The hypervisor operates in one of two compile-time modes:

```
 NS-EL2 Mode (make run-linux)          S-EL2 Mode (make run-spmc)
 ──────────────────────────────         ──────────────────────────────
                                        EL3 │ TF-A BL31 + SPMD
                                            │ (world switch, SMC relay)
                                        ────┼─────────────────────────
 EL2 │ This hypervisor                 S-EL2│ This hypervisor (SPMC)
     │ (exception handling,                 │ (FF-A dispatch, SP lifecycle,
     │  Stage-2 MMU, GIC virt)              │  Secure Stage-2)
 ────┼─────────────────────────        S-EL1│ SP1 Hello, SP2 IRQ, SP3 Relay
 EL1 │ Linux / Zephyr guest            ────┼─────────────────────────
     │                                NS-EL2│ pKVM (protected hVHE)
                                       NS-EL1│ Linux / Android guest
```

Same Rust codebase — `#[cfg(feature = "sel2")]` selects entry point, linker script, and event loop. The two modes share ~70% of code (MMU, GIC, exception handling, FF-A protocol).

---

## Boot Flow

### NS-EL2 (QEMU `-kernel`)

```
boot.S          Save DTB addr (x0→x20), set up stack, clear BSS
    │
rust_main()     Parse DTB (fdt crate, zero-copy, before heap)
    │           Install exception vectors (VBAR_EL2)
    │           Configure HCR_EL2 (trap WFI, SMC, MMIO)
    │           Init GICv3 (GICD enable, List Registers)
    │           Init FF-A proxy (probe SPMC at EL3)
    │           Init heap (BumpAllocator, 16MB at 0x41000000)
    │
    ├── make run:       Run 34 test suites, halt
    ├── make run-linux: Boot Linux (4 vCPUs, virtio-blk)
    └── make run-multi-vm: Boot 2 Linux VMs time-sliced
```

### S-EL2 (TF-A BL32)

```
BL1 → BL2 → BL31(SPMD) → BL32(us) → BL33(pKVM)

boot_sel2.S     Save manifest/HW_CONFIG/core_id from SPMD
    │
rust_main_sel2()  Parse SPMC manifest (TOS_FW_CONFIG DTB)
    │             Enable S-EL2 Stage-1 MMU (NS=1 for NWd DRAM)
    │             Init GIC, Secure Stage-2 for SPs
    │             Parse SPKG headers, ERET to SP1 at S-EL1
    │             SP1 calls FFA_MSG_WAIT → boot SP2, SP3
    │             Register secondary EP (FFA_SECONDARY_EP_REGISTER)
    │
    └── FFA_MSG_WAIT → SPMD dispatches NWd requests → loop
```

**Key insight**: DTB parsing uses the `fdt` crate (zero-copy, no allocations),
so it runs *before* the heap is initialized.

---

## Core Abstractions

```
src/
├── vm.rs              VM lifecycle, Stage-2 setup, run_smp() scheduler loop
├── vcpu.rs            State machine (Uninitialized→Ready→Running→Stopped)
├── scheduler.rs       Round-robin vCPU scheduling with block/unblock
├── devices/mod.rs     Enum-dispatch MMIO routing (see Design Decisions)
├── ffa/proxy.rs       FF-A v1.1 proxy — intercepts guest SMC at NS-EL2
├── spmc_handler.rs    S-EL2 SPMC event loop — FF-A dispatch to SPs
├── sp_context.rs      Per-SP state, INTID ownership, call stack
├── global.rs          Per-VM state arrays, UART RX ring, VSwitch
└── arch/aarch64/
    ├── exception.S    Vector table, context save/restore, enter_guest
    └── hypervisor/
        ├── exception.rs  ESR_EL2 decode → exit reason dispatch
        └── decode.rs     MMIO instruction decode (ISS + raw instruction)
```

### Exception Handling Flow

```
Guest @ EL1
  │ trap (HVC, SMC, MMIO fault, WFI, MSR/MRS)
  ▼
exception.S ─── save x0-x30, SP_EL1, ELR_EL2, SPSR_EL2
  │              (context pointer from TPIDR_EL2)
  ▼
exception.rs ── read ESR_EL2, extract EC (exception class)
  │
  ├─ WfiWfe      → return to scheduler (block vCPU)
  ├─ HvcCall     → PSCI (CPU_ON/OFF/RESET) or HF_INTERRUPT_GET
  ├─ SmcCall     → FF-A proxy or forward to EL3
  ├─ DataAbort   → HPFAR_EL2 for IPA → DeviceManager MMIO dispatch
  ├─ SysReg trap → ICC_SGI1R (IPI emulation), timer regs
  └─ IRQ         → INTID 26 (preemption), 27 (vtimer), 33 (UART)
  │
  ▼
exception.S ─── advance PC, restore context, ERET back to guest
```

**Critical detail**: For MMIO, `FAR_EL2` holds the guest *virtual* address.
The guest *physical* address (IPA) comes from `HPFAR_EL2`:
`IPA = (HPFAR_EL2 & 0xFFFFFFFFF0) << 8 | (FAR_EL2 & 0xFFF)`.

---

## Key Design Decisions

### 1. Enum-Dispatch over Trait Objects

```rust
// src/devices/mod.rs
pub enum Device {
    Uart(VirtualUart), Gicd(VirtualGicd), Gicr(VirtualGicr),
    VirtioBlk(...), VirtioNet(...), Pl031(VirtualPl031),
}
```

**Why**: In `no_std` bare-metal, trait objects (`dyn MmioDevice`) add vtable indirection and prevent inlining on the MMIO hot path. The device set is fixed at compile time — enum dispatch lets the compiler see through match arms and optimize the entire path.

**Trade-off**: Adding a device requires modifying the enum and match blocks. Acceptable with 6 device types and ~1 new type per milestone.

### 2. Bump Allocator with Free-List Recycling

**Why**: `no_std` means no global allocator. A bump allocator is the simplest correct allocator — just increment a pointer. Free-list recycling (singly-linked via first 8 bytes of freed pages) was added for Stage-2 page table teardown, where pages are allocated then freed in bulk.

**Trade-off**: Only 4KB pages can be freed. Arbitrary-size allocations are permanent. Fine because 99% of heap usage is page tables.

### 3. Identity Mapping (GPA == HPA)

Stage-2 translation maps every guest physical address to the same host physical address.

**Why**: Simplifies device emulation (MMIO addresses match hardware), avoids IPA→PA translation bugs, and works well for QEMU `virt`. virtio backends use `copy_nonoverlapping` directly between guest buffers and disk images.

**Trade-off**: Cannot overcommit memory, relocate VMs, or deduplicate pages. A production hypervisor would add an IPA→PA layer.

### 4. Compile-Time Feature Flags for Dual Mode

`sel2` and `linux_guest` use different entry points, linker scripts, and main loops — but share MMU, GIC, FF-A, and device code.

**Why**: A runtime mode switch would carry dead code and branch on every hot path. Feature flags let `cfg` eliminate the unused mode, keeping BL32 at ~240KB.

**Trade-off**: Cannot switch modes without recompiling. In practice, NS-EL2 (guest management) and S-EL2 (SP management) are fundamentally different use cases.

### 5. Single External Dependency

Only `fdt` v0.1.5 (zero-copy device tree parsing). Everything else — exceptions, MMU, GICv3, virtio, FF-A, allocator — is hand-written.

**Why**: Bare-metal firmware cannot tolerate surprise `std` dependencies in the dep tree. Every transitive dependency is a build risk. The `fdt` crate is verified `no_std` and does one thing well.

**Trade-off**: ~7,700 LOC to maintain. But every line is auditable, GDB-steppable, and has no hidden behavior.

---

## Memory Architecture

| Layer | Purpose | Implementation |
|-------|---------|----------------|
| EL2 Heap | Page tables, runtime structures | `BumpAllocator` (16MB at 0x41000000) |
| Stage-2 | Guest isolation (GPA→HPA) | `DynamicIdentityMapper` (2MB blocks + 4KB pages) |
| Secure Stage-2 | SP isolation (S-EL2 mode) | `VSTTBR_EL2/VSTCR_EL2` per-SP |

**Page ownership**: Stage-2 PTE software bits [56:55] encode ownership — `Owned(00)`, `SharedOwned(01)`, `SharedBorrowed(10)`, `Donated(11)`. Validated during FF-A memory operations. Compatible with pKVM's page ownership model.

**Heap gap**: The heap lies within the guest's physical range but is left **unmapped** in Stage-2, preventing guest corruption of hypervisor state.

---

## FF-A and Secure World

[FF-A v1.1](https://developer.arm.com/documentation/den0077) is the protocol between Normal World and Secure World:

**NS-EL2 proxy** (`src/ffa/proxy.rs`): Guest SMC calls trapped via `HCR_EL2.TSC=1`. Handles VERSION/FEATURES/RXTX locally, forwards DIRECT_REQ/MEM_SHARE to real SPMC via EL3 (or stub SPMC for testing).

**S-EL2 SPMC** (`src/spmc_handler.rs`): *Is* the SPMC. Receives requests from SPMD, dispatches DIRECT_REQ to SPs via ERET, handles SP-initiated calls (MEM_RETRIEVE, CONSOLE_LOG) through `handle_sp_exit()` loop.

**SP-to-SP calls**: `CallStack` with cycle detection. Recursive `dispatch_to_sp()` handles chain preemption (Blocked→Preempted state transition).

**Memory sharing lifecycle**:
```
Sender: MEM_SHARE(pages) → handle → PTE bits → SharedOwned
Receiver: MEM_RETRIEVE_REQ(handle) → Stage-2 map → SharedBorrowed
Receiver: MEM_RELINQUISH(handle) → Stage-2 unmap
Sender: MEM_RECLAIM(handle) → restore PTE → Owned
```

---

## Source Tree

| Subsystem | Files |
|-----------|-------|
| **Boot** | `arch/aarch64/boot.S`, `boot_sel2.S`, `linker.ld`, `linker_sel2.ld` |
| **Core** | `vm.rs`, `vcpu.rs`, `scheduler.rs`, `global.rs` |
| **Exceptions** | `arch/aarch64/exception.S`, `hypervisor/exception.rs`, `decode.rs` |
| **Memory** | `mm/allocator.rs`, `mm/heap.rs`, `mm/mmu.rs`, `sel2_mmu.rs` |
| **Devices** | `devices/{pl011,gic,pl031,virtio/}` — enum-dispatch in `mod.rs` |
| **FF-A** | `ffa/{proxy,descriptors,stage2_walker,memory,mailbox,smc_forward}.rs` |
| **SPMC** | `spmc_handler.rs`, `sp_context.rs`, `manifest.rs`, `secure_stage2.rs` |
| **Networking** | `vswitch.rs` — L2 virtual switch, MAC learning, inter-VM forwarding |
| **Platform** | `platform.rs` (constants), `dtb.rs` (runtime DTB discovery) |
| **Tests** | `tests/test_*.rs` — 34 suites, ~457 assertions (`make run`) |

---

## Further Reading

- [docs/architecture.md](docs/architecture.md) — Exhaustive internal reference: register layouts, memory maps, every handler
- [docs/GICV3_IMPLEMENTATION.md](docs/GICV3_IMPLEMENTATION.md) — GICv3 trap-and-emulate deep dive
- [docs/RUST_FIRMWARE_CODING_GUIDELINES.md](docs/RUST_FIRMWARE_CODING_GUIDELINES.md) — Bare-metal Rust coding conventions
- [docs/debugging.md](docs/debugging.md) — GDB remote debugging and QEMU tracing
- [CLAUDE.md](CLAUDE.md) — Full module/test tables, build commands, feature flag matrix

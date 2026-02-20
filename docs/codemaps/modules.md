# Modules Codemap

> Freshness: 2026-02-20 | 50+ source files across 17 directories

## Module Tree

```
src/
├── lib.rs                          crate root (no_std, global_asm, alloc)
├── main.rs                         binary entry: rust_main, test orchestration
├── vm.rs                           VM lifecycle, Stage-2 setup, SMP scheduler
├── vcpu.rs                         vCPU state machine, enter_guest wrapper
├── vcpu_interrupt.rs               legacy IRQ/FIQ pending state
├── scheduler.rs                    round-robin scheduler with block/unblock
├── global.rs                       all global statics: DEVICES, VM_STATE, UART_RX, PER_VM_VTTBR
├── guest_loader.rs                 GuestConfig, run_guest(), run_multi_vm_guests()
├── platform.rs                     board constants + DTB-backed num_cpus()
├── dtb.rs                          host DTB parsing via fdt crate
├── vswitch.rs                      L2 virtual switch: MAC learning, frame forwarding, NetRxRing
├── percpu.rs                       per-CPU context (MPIDR → PerCpuContext)
├── sync.rs                         ticket SpinLock<T>
├── uart.rs                         physical PL011 driver + fmt::Write
├── mm/
│   ├── mod.rs                      re-exports BumpAllocator
│   ├── allocator.rs                BumpAllocator (page-level)
│   └── heap.rs                     global heap init, alloc_page(), GlobalAlloc impl
├── arch/
│   ├── mod.rs                      arch dispatch (aarch64 only currently)
│   ├── traits.rs                   portable traits: Stage2Mapper, InterruptController, etc.
│   └── aarch64/
│       ├── mod.rs                  re-exports enter_guest, init_stage2, MemoryAttributes
│       ├── defs.rs                 ARM64 constants (HCR bits, ESR classes, GIC offsets)
│       ├── regs.rs                 VcpuContext, GeneralPurposeRegs, SystemRegs, ExitReason
│       ├── vcpu_arch_state.rs      VcpuArchState (42 fields): save/restore per-vCPU hw state
│       ├── hypervisor/
│       │   ├── mod.rs              re-exports exception, decode
│       │   ├── exception.rs        handle_exception, handle_irq_exception, MMIO dispatch
│       │   └── decode.rs           MmioAccess: ISS-based + instruction-based MMIO decode
│       ├── mm/
│       │   ├── mod.rs              re-exports mmu
│       │   └── mmu.rs             IdentityMapper, DynamicIdentityMapper, S2PageTableEntry
│       └── peripherals/
│           ├── mod.rs              re-exports gic, gicv3, timer
│           ├── gic.rs              GICD/GICC physical base statics, low-level helpers
│           ├── gicv3.rs            GicV3SystemRegs, GicV3VirtualInterface (LR management)
│           └── timer.rs            timer_get_count(), timer_irq_pending()
├── ffa/
│   ├── mod.rs                      FF-A v1.1 constants, types, function IDs
│   ├── proxy.rs                    FF-A proxy: SMC interception, dispatch, handle_ffa_call()
│   ├── mailbox.rs                  Per-VM RXTX mailbox (FFA_RXTX_MAP/UNMAP/RX_RELEASE)
│   ├── stub_spmc.rs                Stub SPMC: 2 SPs, echo messaging, share records, ShareInfoFull
│   ├── memory.rs                   PageOwnership enum, validate_page_for_share()
│   ├── stage2_walker.rs            Lightweight Stage-2 walker from VTTBR: SW bits, S2AP, map/unmap
│   ├── descriptors.rs              FF-A v1.1 composite memory region descriptor parsing
│   └── smc_forward.rs              forward_smc() to EL3 via smc #0, probe_spmc()
└── devices/
    ├── mod.rs                      Device enum, DeviceManager, MmioDevice trait
    ├── gic/
    │   ├── mod.rs                  re-exports VirtualGicd, VirtualGicr
    │   ├── distributor.rs          VirtualGicd: shadow state + write-through to physical GICD
    │   └── redistributor.rs        VirtualGicr: per-vCPU GICR state (8 vCPUs max)
    ├── pl011/
    │   ├── mod.rs                  re-exports VirtualUart
    │   └── emulator.rs             VirtualUart: TX passthrough, RX buffer, PeriphID
    ├── pl031.rs                    PL031 RTC emulation (counter from CNTVCT, PrimeCell ID)
    └── virtio/
        ├── mod.rs                  re-exports queue, mmio, blk, net
        ├── queue.rs                Virtqueue: descriptor table, avail/used rings
        ├── mmio.rs                 VirtioMmioTransport<T>: virtio-mmio register interface
        ├── blk.rs                  VirtioBlk: read/write/flush against in-memory disk
        └── net.rs                  VirtioNet: TX/RX queues, MAC config, VSwitch integration
```

## Key Entry Points

| Function | File | Role |
|----------|------|------|
| `_start` | `boot.S` | EL2 entry, stack setup, BSS clear |
| `secondary_entry` | `boot.S` | Secondary pCPU entry via PSCI |
| `rust_main(dtb_addr)` | `main.rs` | Primary Rust entry |
| `rust_main_secondary(cpu_id)` | `main.rs` | Secondary pCPU Rust entry |
| `handle_exception(ctx)` | `exception.rs` | Sync exception handler (called from asm) |
| `handle_irq_exception(ctx)` | `exception.rs` | IRQ handler (called from asm) |
| `enter_guest(ctx)` | `exception.S` | Context switch to EL1, ERET |

## Feature-Gated Modules

| Feature | Gated Code |
|---------|------------|
| `linux_guest` | `DynamicIdentityMapper`, GICR trap setup, `run_smp()`, virtio-blk/net registration, `ffa::proxy::init()`, Stage-2 validation in FF-A |
| `multi_pcpu` | `rust_main_secondary`, `SpinLock<DeviceManager>`, `TPIDR_EL2` context, physical SGI send, `ensure_vtimer_enabled()`, `SHARED_VTTBR/VTCR`, `PENDING_CPU_ON_PER_VCPU` |
| `multi_vm` | `run_multi_vm_guests()`, `GuestConfig::linux_vm1()`, VM1 memory partition, per-VM VSwitch ports, per-VM virtio-net |
| `guest` | Zephyr boot path in `main.rs` |

## Cross-Module Dependencies

```
                    ┌──────────┐
                    │  main.rs │
                    └────┬─────┘
            ┌────────────┼────────────┐
            ▼            ▼            ▼
     guest_loader    exception    tests/
       │    │         │    │
       ▼    ▼         ▼    ▼
      vm  platform  global  decode
       │              │
  ┌────┼────┐    ┌────┼────┐
  ▼    ▼    ▼    ▼    ▼    ▼
vcpu sched devices  sync percpu
  │         │
  ▼         ├─▶ pl011/emulator
arch_state  ├─▶ pl031
  │         ├─▶ gic/{distributor, redistributor}
  ▼         └─▶ virtio/{mmio, blk, net, queue}
gicv3
              vswitch ◀── virtio/net, global (PORT_RX)

              ffa/
              ├─▶ proxy ◀── exception (handle_smc)
              ├─▶ stub_spmc ◀── proxy
              ├─▶ memory ◀── proxy
              ├─▶ stage2_walker ◀── proxy (VTTBR_EL2, PER_VM_VTTBR)
              ├─▶ descriptors ◀── proxy (TX buffer parsing)
              ├─▶ mailbox ◀── proxy
              └─▶ smc_forward ◀── proxy, exception

dtb ◀── platform, distributor, redistributor, pl011, guest_loader
```

## Global Statics Summary

| Static | Module | Type | Scope |
|--------|--------|------|-------|
| `DEVICES` | global | `[GlobalDeviceManager; 2]` | per-VM MMIO dispatch |
| `VM_STATE` | global | `[VmGlobalState; 2]` | per-VM atomics |
| `CURRENT_VM_ID` | global | `AtomicUsize` | active VM index |
| `UART_RX` | global | `UartRxRing` | IRQ → run loop |
| `PORT_RX` | global | `[NetRxRing; MAX_PORTS]` | per-VM SPSC ring for virtio-net RX |
| `VSWITCH` | global | `UnsafeCell<VSwitch>` | L2 virtual switch with MAC table |
| `PER_VM_VTTBR` | global | `[AtomicU64; MAX_VMS]` | per-VM L0 table PA for cross-VM Stage-2 (FF-A) |
| `SHARED_VTTBR/VTCR` | global | `AtomicU64` | multi_pcpu only |
| `PENDING_CPU_ON_PER_VCPU` | global | `[PerVcpuCpuOnRequest; 8]` | multi_pcpu only |
| `PLATFORM_INFO` | dtb | `UnsafeCell<PlatformInfo>` | DTB-discovered hw addrs |
| `HEAP` | mm/heap | `UnsafeCell<Option<BumpAllocator>>` | page allocator |
| `PER_CPU` | percpu | `UnsafeCell<[PerCpuContext; 8]>` | per-CPU state |
| `MAPPER` | vm | `UnsafeCell<IdentityMapper>` | test-only static mapper |
| `EXCEPTION_COUNT` | exception | `AtomicU32` | single-pCPU only |
| `SPMC_PRESENT` | ffa/proxy | `AtomicBool` | runtime SPMC detection flag |
| `MAILBOXES` | ffa/mailbox | `[UnsafeCell<Mailbox>; MAX_VMS]` | per-VM RXTX buffers |

## Assembly Interface

```
boot.S exports:
  _start                → rust_main(x20=dtb_addr)
  secondary_entry       → rust_main_secondary(mpidr.aff0)
  pcpu_stacks           → 3×128KB BSS for pCPUs 1-3

exception.S exports:
  exception_vector_table → installed into VBAR_EL2
  enter_guest(ctx)       → save EL2 callee-saved, restore guest regs, ERET
                         → on trap: save guest regs, call handle_exception/handle_irq_exception

Rust → ASM contract:
  VcpuContext must be #[repr(C)] — field offsets used by exception.S
  TPIDR_EL2 (multi_pcpu) = pointer to current VcpuContext
```

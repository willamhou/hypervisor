# Architecture Codemap

> Freshness: 2026-02-17 | Source: 40 `.rs` files + 2 `.S` files | Single external dep: `fdt 0.1.5`

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│  Guest OS (Linux 6.12.12 / Zephyr)                    EL1      │
│  ─ Virtual ICC registers (ICH_HCR.En=1)                        │
│  ─ Stage-2 translated memory (identity map)                    │
├─────────────────────────────────────────────────────────────────┤
│  Hypervisor                                           EL2      │
│  ┌───────────┐ ┌──────────┐ ┌─────────┐ ┌──────────────────┐  │
│  │ Exception  │ │ Devices  │ │ Stage-2 │ │ GICv3 Virtual IF │  │
│  │ Handler    │ │ Manager  │ │ MMU     │ │ (List Registers) │  │
│  └─────┬─────┘ └────┬─────┘ └────┬────┘ └────────┬─────────┘  │
│        │            │            │               │             │
│  ┌─────┴────────────┴────────────┴───────────────┴──────────┐  │
│  │  VM → Scheduler → vCPU → enter_guest() → ERET           │  │
│  └──────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│  Hardware (QEMU virt)                                          │
│  ─ GICv3, PL011, Generic Timer, virtio-mmio                   │
└─────────────────────────────────────────────────────────────────┘
```

## Boot Flow

```
QEMU firmware (EL3)
  │ passes DTB addr in x0
  ▼
_start (boot.S)
  │ save x0→x20, setup stack, clear BSS
  ▼
rust_main(dtb_addr) (main.rs)
  ├─ dtb::init()           → parse host DTB
  ├─ exception::init()     → set VBAR_EL2, configure HCR_EL2
  ├─ mm::heap::init()      → bump allocator at 0x41000000
  ├─ [tests path]          → run 24 test suites, then halt
  └─ [guest path]          → guest_loader::run_guest() or run_multi_vm_guests()
       ├─ Vm::new(id)
       ├─ Vm::init_memory() → Stage-2 page tables
       ├─ register devices  → UART, GICD, GICR, virtio-blk
       ├─ Vm::add_vcpu()    → configure entry point, SP
       └─ Vm::run_smp()     → scheduling loop → ERET into guest
```

## Feature Configurations

```
make run          → (default)      → unit tests only, no guest
make run-linux    → linux_guest    → 4 vCPUs on 1 pCPU, cooperative+preemptive
make run-linux-smp→ multi_pcpu     → 4 vCPUs on 4 pCPUs, 1:1 affinity
make run-multi-vm → multi_vm       → 2 Linux VMs time-sliced on 1 pCPU

multi_pcpu ──implies──▶ linux_guest
multi_vm   ──implies──▶ linux_guest
multi_pcpu ⊥ multi_vm  (mutually exclusive)
```

## Module Dependency Graph

```
main.rs ──▶ guest_loader ──▶ vm ──▶ vcpu ──▶ arch/aarch64/regs
   │              │           │       │          vcpu_arch_state
   │              │           │       └──▶ vcpu_interrupt ──▶ gicv3
   │              │           ├──▶ scheduler
   │              │           ├──▶ devices (DeviceManager)
   │              │           ├──▶ global (VM_STATE, DEVICES)
   │              │           └──▶ arch/aarch64/mm/mmu
   │              ├──▶ platform
   │              └──▶ dtb
   └──▶ tests/mod.rs ──▶ (all test modules)

exception.rs ◀── called by exception.S (vector table)
   ├──▶ global::current_devices()  → MMIO dispatch
   ├──▶ decode::MmioAccess         → instruction decode
   ├──▶ gicv3::GicV3SystemRegs     → IAR/EOIR/DIR
   └──▶ percpu                     → per-CPU exception count
```

## Execution Modes

### Single-pCPU SMP (`linux_guest`, not `multi_pcpu`)
```
run_smp() loop:
  for each iteration:
    check PSCI CPU_ON → boot_secondary_vcpu()
    wake vCPUs with pending SGIs/SPIs
    scheduler.next() → pick vCPU (round-robin)
    inject SGIs/SPIs into arch_state LRs
    arm CNTHP timer (10ms preemption)
    vcpu.run() → enter_guest() → ERET
    handle exit: WFI→block, preemption→yield, terminal→remove
```

### Multi-pCPU (`multi_pcpu`)
```
pCPU 0: rust_main() → run_guest() → Vm::run_vcpu(0) → enter_guest() loop
pCPU 1-3: rust_main_secondary() → wait for PSCI CPU_ON → enter_guest() loop
  - TPIDR_EL2 = per-CPU context pointer
  - WFI passthrough (real hardware sleep)
  - Physical SGI 0 for cross-pCPU wake
```

### Multi-VM (`multi_vm`)
```
run_multi_vm():
  VM round-robin → for each VM:
    set CURRENT_VM_ID, install Stage-2 (VTTBR with VMID)
    run_one_iteration() → vCPU round-robin within VM
    switch to next VM
```

## Memory Map

```
0x0800_0000  GICD (64KB, 16×4KB pages) ─── trap-and-emulate
0x080A_0000  GICR frames (4 CPUs)      ─── trap-and-emulate
0x0900_0000  PL011 UART                ─── trap-and-emulate
0x0A00_0000  virtio-mmio               ─── trap-and-emulate
0x4000_0000  Hypervisor code (.text)
0x4100_0000  Heap (16MB bump alloc)    ─── unmapped in Stage-2
0x4700_0000  Guest DTB
0x4800_0000  Linux kernel Image        ─── guest RAM start
0x5400_0000  Initramfs (BusyBox)
0x5800_0000  virtio-blk disk image
0x6800_0000  VM1 memory (multi_vm)     ─── 256MB partition
```

## Interrupt Flow

```
Physical IRQ at EL2
  ▼
exception.S → handle_irq_exception()
  ├─ INTID 26: CNTHP preemption timer → set preemption_exit, return false
  ├─ INTID 27: vtimer PPI → inject HW=1 LR, return true (back to guest)
  ├─ INTID 33: UART RX → push to UART_RX ring, inject SPI 33
  └─ INTID 48: virtio-blk (should not arrive as physical IRQ)

Guest MSR ICC_SGI1R_EL1 (trapped via TALL1)
  ▼
handle_sgi_trap() → decode TargetList/Aff1/INTID → PENDING_SGIS[target] |= (1 << intid)
  └─ multi_pcpu: also send physical SGI 0 to wake remote pCPU
```

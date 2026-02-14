# SMP Scheduling

This document describes how the hypervisor schedules multiple virtual CPUs on a single physical CPU.

## Architecture

All vCPUs run on **one physical CPU** (pCPU 0) via time-multiplexed scheduling. The scheduler combines:

- **Cooperative scheduling**: Guest WFI traps (HCR_EL2.TWI=1) yield to other vCPUs
- **Preemptive scheduling**: CNTHP timer (10ms, INTID 26) forces context switches

## vCPU State Machine

```
                 add_vcpu()
    None ──────────────────► Ready
                              │  ▲
                  pick_next() │  │ yield_current() / unblock()
                              ▼  │
                            Running
                              │  ▲
                 block_current│  │ unblock()
                              ▼  │
                            Blocked
```

States (from `src/scheduler.rs`):

| State | Meaning |
|-------|---------|
| `None` | Not registered in scheduler |
| `Ready` | Eligible to run |
| `Running` | Currently executing on pCPU |
| `Blocked` | Waiting (WFI), needs SGI/SPI to wake |

## The `run_smp()` Loop

Located in `src/vm.rs:334`, this is the main scheduling loop:

```
┌──────────────────────────────────────────────┐
│ 1. Check PENDING_CPU_ON → boot_secondary()   │
│ 2. wake_pending_vcpus() (SGI/SPI → unblock)  │
│ 3. scheduler.pick_next() → round-robin       │
│    └─ None? unblock all online, retry        │
│ 4. CURRENT_VCPU_ID = vcpu_id                 │
│ 5. Drain UART_RX → VirtualUart → inject SPI33│
│ 6. inject_pending_sgis() → arch_state.ich_lr │
│ 7. inject_pending_spis() → arch_state.ich_lr │
│ 8. Arm CNTHP preemption timer (10ms)         │
│ 9. ensure_cnthp_enabled() (re-enable INTID26)│
│ 10. vcpu.run() → save/restore → ERET         │
│ 11. Handle exit:                             │
│     ├─ CPU_ON exit → yield_current()         │
│     ├─ PREEMPTION_EXIT → yield_current()     │
│     ├─ WFI → block_current()                 │
│     ├─ Error → yield_current()               │
│     └─ Real exit → remove_vcpu()             │
└──────────────────────────────────────────────┘
```

### Step Details

**Step 1 — PSCI CPU_ON**: The exception handler sets `PENDING_CPU_ON` atomics when the guest calls `HVC PSCI_CPU_ON`. The loop picks this up and calls `boot_secondary_vcpu()`.

**Step 2 — Wake blocked vCPUs**: Scans `PENDING_SGIS[id]` and `PENDING_SPIS[id]` for all vCPUs. Any with pending interrupts are moved from Blocked→Ready.

**Step 3 — Round-robin**: `Scheduler::pick_next()` scans from `next_idx` wrapping around. If no Ready vCPU found, all online vCPUs are unblocked (to allow timer interrupts to fire).

**Steps 5-7 — Interrupt injection**: SGIs/SPIs are written to `arch_state.ich_lr[]` (the saved LR array), not hardware LRs. `vcpu.run()` calls `arch_state.restore()` which writes hardware LRs.

**Step 8 — Preemption timer**: Only armed when multiple vCPUs are online (`online & (online-1) != 0`). The CNTHP timer fires INTID 26 after 10ms, which sets `PREEMPTION_EXIT=true` and returns false from the IRQ handler.

**Step 9 — Re-enable INTID 26**: Guest can disable INTID 26 via GICR_ICENABLER0 writes. `ensure_cnthp_enabled()` directly writes physical GICR0 (EL2 bypasses Stage-2) to re-enable it.

## Per-vCPU Architectural State

`VcpuArchState` (in `src/arch/aarch64/vcpu_arch_state.rs`) saves/restores everything not handled by the exception entry/exit assembly:

| Category | Registers |
|----------|-----------|
| GIC virtual interface | ICH_LR0-3_EL2, ICH_VMCR_EL2, ICH_HCR_EL2 |
| Virtual timer | CNTV_CTL_EL0, CNTV_CVAL_EL0 |
| CPU identity | VMPIDR_EL2 (Aff0 = vcpu_id) |
| EL1 system regs | SCTLR, TTBR0/1, TCR, MAIR, VBAR, CPACR, CONTEXTIDR, TPIDR, TPIDRRO, PAR, CNTKCTL, SP_EL1, ELR_EL1, SPSR_EL1, AFSR0/1, ESR_EL1, FAR_EL1, AMAIR, MDSCR |
| Stack pointer | SP_EL0 |
| PAC keys | APIA, APIB, APDA, APDB, APGA (lo+hi each) |

The `save()` and `restore()` methods use inline assembly (`mrs`/`msr`) with an `isb` barrier after restore.

### What exception.S Handles

The assembly context switch (`enter_guest` in `exception.S`) saves/restores:
- x0-x30 general-purpose registers
- SP_EL2 (hypervisor stack)
- ELR_EL2 (guest PC)
- SPSR_EL2 (guest PSTATE)

Everything else requires `VcpuArchState`.

## PSCI CPU_ON Flow

```
Guest vCPU 0: HVC PSCI_CPU_ON(target=1, entry=0x..., ctx_id=0x...)
  → handle_psci() → PENDING_CPU_ON.request(1, entry, ctx_id)
  → returns false (exit to scheduler)

run_smp() loop top:
  → PENDING_CPU_ON.take() returns Some((1, entry, ctx_id))
  → boot_secondary_vcpu(1, entry, ctx_id):
      1. wake_gicr(GICR1_RD_BASE) — clear ProcessorSleep
      2. Create Vcpu::new(1, entry, 0)
      3. Set x0=ctx_id, SPSR=EL1h+DAIF, sctlr_el1=0x30D00800 (MMU off)
      4. Set CPACR=0x300000 (FP/SIMD enabled)
      5. arch_state.init_for_vcpu(1) — VMPIDR Aff0=1, ICH_HCR=TALL1|En
      6. scheduler.add_vcpu(1) — state=Ready
      7. VCPU_ONLINE_MASK |= (1 << 1)
      8. reset_exception_counters()
```

## Cooperative Scheduling (WFI)

When guest executes WFI:
1. HCR_EL2.TWI traps to EL2
2. In SMP mode (`multi_vcpu=true`): always inject timer if pending, then return false
3. `run_smp()` calls `scheduler.block_current()` → vCPU state = Blocked
4. Blocked vCPU waits until `wake_pending_vcpus()` finds pending SGIs/SPIs

**Single vCPU mode**: WFI handling uses `handle_wfi_with_timer_injection()` which polls the virtual timer and injects periodic ticks.

## Preemptive Scheduling (CNTHP Timer)

```
Arm timer (10ms from CNTPCT_EL0)
  → CNTHP fires → IRQ trap → INTID 26
  → disarm_preemption_timer()
  → if multi_vcpu: PREEMPTION_EXIT = true, return false
  → run_smp(): yield_current() → next_idx = (current+1) % MAX_VCPUS
```

**Why CNTHP, not the guest timer?** The guest can mask its own timer (e.g., during `multi_cpu_stop` with IRQs disabled). CNTHP is a separate EL2 timer not controllable by the guest.

**Why re-enable INTID 26?** When the guest re-initializes its GIC (ICENABLER0 clears all, then re-enables only guest PPIs), INTID 26 gets disabled. `ensure_cnthp_enabled()` writes directly to physical GICR0 (EL2 bypasses Stage-2) before every vCPU entry.

## Critical Rules

1. **Never modify SPSR_EL2**: Guest controls its own PSTATE.I (interrupt mask). Overriding causes spinlock deadlocks.
2. **TWI=1, TWE=0**: WFI traps for scheduling; WFE executes natively (used in spinlocks, woken by SEV).
3. **Write to `arch_state.ich_lr[]`, not hardware**: `vcpu.run()` calls `restore()` which overwrites hardware LRs. Injecting into hardware LRs from `run_smp()` would be clobbered.
4. **SP_EL0 must be saved/restored**: Linux uses SP_EL0 for per-CPU current task pointer.

## Source Files

| File | Key Functions |
|------|--------------|
| `src/vm.rs` | `run_smp()`, `boot_secondary_vcpu()`, `inject_pending_sgis/spis()`, `wake_pending_vcpus()`, `ensure_cnthp_enabled()` |
| `src/scheduler.rs` | `Scheduler`, `pick_next()`, `yield_current()`, `block_current()`, `unblock()` |
| `src/arch/aarch64/vcpu_arch_state.rs` | `VcpuArchState`, `save()`, `restore()`, `init_for_vcpu()` |
| `src/vcpu.rs` | `Vcpu::run()` — calls arch_state save/restore around `enter_guest()` |
| `src/arch/aarch64/hypervisor/exception.rs` | `handle_psci()`, `handle_irq_exception()` (INTID 26/27) |
| `src/global.rs` | `PENDING_CPU_ON`, `VCPU_ONLINE_MASK`, `CURRENT_VCPU_ID`, `PREEMPTION_EXIT` |
| `src/arch/aarch64/peripherals/timer.rs` | `arm_preemption_timer()`, `disarm_preemption_timer()` |

# Exception Handling

This document describes how the hypervisor handles exceptions from the guest VM.

## Architecture

```
Guest @ EL1
  ↓ exception (sync, IRQ, FIQ, SError)
ARM exception vector table (arch/aarch64/exception.S)
  ↓ save context (x0-x30, SP, ELR_EL2, SPSR_EL2)
handle_exception() or handle_irq_exception()  (Rust)
  ↓ decode ESR_EL2 → ExitReason
  ↓ handle, optionally advance PC
  ↓ return true (continue) or false (exit)
exception.S: restore context
  ↓ ERET → guest resumes at ELR_EL2
```

## Exception Vector Table

Defined in `arch/aarch64/exception.S`, installed via `VBAR_EL2`:

- **Sync from Lower EL (AArch64)**: Offset 0x400 → saves context → calls `handle_exception()`
- **IRQ from Lower EL (AArch64)**: Offset 0x480 → saves context → calls `handle_irq_exception()`
- Other vectors (FIQ, SError, same-EL) → halt with error message

The assembly saves all 31 general-purpose registers plus SP, ELR_EL2, and SPSR_EL2 into a `VcpuContext` struct on the stack.

## ESR_EL2 Decoding

`ExitReason` is decoded from ESR_EL2 EC field (bits [31:26]):

| EC | ExitReason | Description |
|----|-----------|-------------|
| 0x01 | WfiWfe | WFI/WFE trapped (HCR_EL2.TWI=1) |
| 0x16 | HvcCall | HVC instruction from EL1 |
| 0x18 | TrapMsrMrs | MSR/MRS to trapped system register |
| 0x20 | InstructionAbort | Stage-2 instruction fault |
| 0x24 | DataAbort | Stage-2 data fault (MMIO) |
| other | Other(ec) | FP/SIMD trap, SVE, unknown |

## Synchronous Exception Handlers

### WFI/WFE (EC=0x01)

**SMP mode** (multiple vCPUs online):
1. Inject timer if pending via `handle_wfi_with_timer_injection()`
2. Advance PC by 4
3. Return false → `run_smp()` calls `scheduler.block_current()`

**Single vCPU mode**:
1. Check if virtual timer is pending → inject via LR
2. Check if any LR has pending interrupt → continue
3. If stuck (same PC, `MAX_CONSECUTIVE_WFI` reached) → exit
4. Inject periodic timer ticks every 100 iterations

### HVC (EC=0x16)

Dispatched by HVC immediate value:
- **HVC #0**: Standard hypercall (x0 = function ID)
  - x0=0: Print character (x1 = char)
  - x0=1: Exit guest
  - x0 with bit 31 set: PSCI call → `handle_psci()`
- **HVC #0x4A48** ("JH"): Jailhouse debug console
  - x0=8: putc (x1 = char)
  - x0=9: getc → x0 = char or -1

PC is **not** advanced — ELR_EL2 already points past the HVC instruction.

### PSCI Handling

Implements PSCI v0.2:

| Function | ID | Action |
|----------|-----|--------|
| VERSION | 0x84000000 | Returns 0x00000002 (v0.2) |
| CPU_ON | 0xC4000003 | Sets PENDING_CPU_ON atomics, returns false |
| CPU_OFF | 0x84000002 | Returns false (exit vCPU) |
| AFFINITY_INFO | 0xC4000004 | Checks VCPU_ONLINE_MASK |
| SYSTEM_OFF | 0x84000008 | Returns false |
| SYSTEM_RESET | 0x84000009 | Returns false |
| FEATURES | 0x8400000A | Returns SUCCESS for supported functions |
| CPU_SUSPEND | 0x84000001 | Treated as no-op, returns SUCCESS |

### Data Abort (EC=0x24)

MMIO trap-and-emulate path:

1. Read HPFAR_EL2 for IPA (see [HPFAR_EL2](#hpfar_el2-for-ipa))
2. Call `handle_mmio_abort(context, ipa)`
3. Decode instruction via `MmioAccess::decode(insn, iss)`
4. Route through `DEVICES.handle_mmio(addr, value, size, is_write)`
5. For loads: write device result to guest register
6. Advance PC by 4
7. Call `flush_pending_spis_to_hardware()` for immediate SPI delivery

### MSR/MRS Trap (EC=0x18)

ISS decoding (from `handle_msr_mrs_trap()`):
```
Op0 = ISS[21:20], Op2 = ISS[19:17], Op1 = ISS[16:14]
CRn = ISS[13:10], Rt = ISS[9:5], CRm = ISS[4:1]
Direction = ISS[0]  (1=read/MRS, 0=write/MSR)
```

#### Trapped MSR writes:
- **ICC_SGI1R_EL1** (S3_0_C12_C11_5): → `handle_sgi_trap()` for IPI emulation
- **MDSCR_EL1**: Passed through to hardware
- **OSLAR_EL1**: Passed through
- **OSDLR_EL1**: Ignored (don't lock)
- **PMU registers** (CRn=9): Ignored (no PMU)

#### Trapped MRS reads:
- **MDSCR_EL1**: Read from hardware
- **OSLSR_EL1**: Returns 0x8 (unlocked)
- **OSDLR_EL1**: Returns 0
- **PMU registers**: Returns 0
- **Others**: Read-As-Zero

### Other EC values

- **EC=0x07** (FP/SIMD trap): Advance PC, continue. Should not occur after CPTR_EL2 fix.
- **EC=0x19** (SVE trap): Advance PC, continue.
- **Unknown EC**: Print diagnostic, exit guest.

## IRQ Exception Handler

`handle_irq_exception()` handles physical IRQs that trapped from guest to EL2 (HCR_EL2.IMO=1):

1. Acknowledge via `ICC_IAR1_EL1` → get INTID
2. Check for spurious (INTID >= 1020) → ignore

| INTID | Source | Action |
|-------|--------|--------|
| 0-15 | SGI | If current vCPU=0: inject via LR. Else: queue in PENDING_SGIS[0]. ACK+DIR. |
| 26 | CNTHP preemption | Disarm timer. Set PREEMPTION_EXIT=true. Return false. |
| 27 | Virtual timer | Mask timer. Inject HW=1 LR (pINTID=27). Demand-driven preemption. |
| 33 | UART RX | Read all bytes from physical UART FIFO → UART_RX ring. Return false. |
| other | Unknown | Log, EOI+DIR, continue. |

### EOI Handling

EOImode=1 (ICC_CTLR_EL1 at EL2):
- `write_eoir1(intid)`: Priority drop only
- `write_dir(intid)`: Deactivation (for non-HW interrupts)
- HW=1 interrupts (INTID 27): No DIR needed — guest virtual EOI handles deactivation

## HPFAR_EL2 for IPA

When Stage-2 is enabled and guest MMU is on:
- **FAR_EL2** = guest **virtual** address (useless for MMIO routing)
- **HPFAR_EL2** = faulting IPA page frame

```rust
let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;  // IPA[47:12]
let page_offset = far_el2 & 0xFFF;                       // offset within page
let ipa = ipa_page | page_offset;
```

When guest MMU is off, FAR_EL2 == IPA (but HPFAR_EL2 still works correctly).

## HCR_EL2 Configuration

Set in `exception::init()`:

| Bit | Name | Value | Purpose |
|-----|------|-------|---------|
| RW | AArch64 | 1 | EL1 is AArch64 |
| SWIO | Set/Way Override | 1 | Cache maintenance broadcast |
| FMO | FIQ route | 1 | Physical FIQ → EL2 |
| IMO | IRQ route | 1 | Physical IRQ → EL2 |
| AMO | SError route | 1 | Physical SError → EL2 |
| FB | Force Broadcast | 1 | TLB/cache maintenance broadcast |
| BSU | Barrier Upgrade | IS | Inner Shareable barriers |
| TWI | Trap WFI | 1 | WFI → EL2 (scheduling) |
| TWE | Trap WFE | 0 | WFE native (spinlocks use SEV) |
| TEA | Trap External Abort | 1 | External aborts → EL2 |
| APK | PAC key access | 1 | Don't trap PAC key registers |
| API | PAC instructions | 1 | Don't trap PAC instructions |

**TWE=0 is critical**: Linux spinlocks use WFE/SEV. Trapping WFE would cause spinlock deadlocks since the scheduler can't deliver the SEV signal.

## Exception Loop Prevention

`EXCEPTION_COUNT` atomic tracks consecutive exceptions without reset:
- Each successful handler resets to 0
- If count exceeds `MAX_CONSECUTIVE_EXCEPTIONS` (100): halt system with diagnostic

## Critical Rules

1. **Never modify SPSR_EL2**: The guest's saved PSTATE.I controls interrupt masking. Clearing it causes spinlock deadlocks.
2. **Always use HPFAR_EL2 for IPA**: FAR_EL2 is guest VA when MMU is on.
3. **flush_pending_spis_to_hardware()**: Called after MMIO handling for immediate virtio completion delivery.
4. **HVC doesn't advance PC**: ELR_EL2 already points past the HVC instruction. All other sync exceptions advance PC by 4.

## Source Files

| File | Role |
|------|------|
| `src/arch/aarch64/hypervisor/exception.rs` | `handle_exception()`, `handle_irq_exception()`, `handle_psci()`, `handle_sgi_trap()`, `handle_mmio_abort()` |
| `arch/aarch64/exception.S` | Exception vector table, context save/restore, `enter_guest()` |
| `src/arch/aarch64/hypervisor/decode.rs` | `MmioAccess::decode()` — instruction decoding for MMIO |
| `src/arch/aarch64/regs.rs` | `VcpuContext`, `ExitReason`, `GpRegs` |
| `src/arch/aarch64/defs.rs` | ESR_EC constants, HCR_EL2 bit definitions |
| `src/global.rs` | `DEVICES`, `PENDING_CPU_ON`, `PREEMPTION_EXIT` |

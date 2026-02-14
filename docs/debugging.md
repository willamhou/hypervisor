# Debugging Guide

This guide covers debugging techniques for the ARM64 hypervisor and its Linux guest.

## GDB Setup

### Starting the Debug Server

```bash
make debug
# QEMU starts with -s -S (GDB server on port 1234, halted at boot)
```

### Connecting GDB

```bash
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) continue
```

### Useful Breakpoints

```gdb
# Hypervisor entry point
break rust_main

# Exception handling
break handle_exception
break handle_irq_exception

# SMP scheduling loop
break run_smp

# Specific exception types
break handle_mmio_abort
break handle_psci
break handle_sgi_trap

# vCPU lifecycle
break boot_secondary_vcpu
break enter_guest
```

### Examining State

```gdb
# Guest registers (from VcpuContext)
print/x context->gp_regs
print/x context->pc
print/x context->spsr_el2

# System registers
print/x context->sys_regs.esr_el2
print/x context->sys_regs.far_el2

# Read hardware registers
monitor info registers

# Memory at address
x/8gx 0x48000000
```

## QEMU Monitor Commands

Access via `Ctrl+A` then `C`:

```
# CPU registers
info registers

# Memory tree (address map)
info mtree

# TLBs
info tlb

# Interrupt state
info pic
info irq

# Guest page tables
info mem
```

## Debugging Stage-2 Translation Faults

When a guest memory access causes an unexpected fault:

### 1. Read the Fault Registers

```gdb
# At handle_exception breakpoint:
print/x context->sys_regs.esr_el2   # Exception syndrome
print/x context->sys_regs.far_el2   # Faulting address (guest VA if MMU on)

# Read HPFAR_EL2 for the real IPA
monitor info registers
# Look for HPFAR_EL2 value
```

### 2. Compute the IPA

```
IPA = (HPFAR_EL2 & 0x0000_0FFF_FFFF_FFF0) << 8 | (FAR_EL2 & 0xFFF)
```

**Critical**: When guest MMU is ON, `FAR_EL2` = guest VA, NOT IPA. Always use `HPFAR_EL2`.

### 3. Check the Page Table

Verify the IPA is mapped in Stage-2:
- If it should be MMIO-trapped: confirm the page is unmapped
- If it should be RAM: confirm the page is mapped with correct attributes
- Check for heap gap (0x41000000-0x42000000 must be unmapped)

### 4. Common Causes

| Symptom | Likely Cause |
|---------|-------------|
| Fault on GIC address (0x08xxxxxx) | GICR page not properly unmapped for trap |
| Fault on UART address (0x09xxxxxx) | UART not registered in DeviceManager |
| Fault on virtio address (0x0axxxxxx) | Virtio device not registered |
| Fault on RAM address (0x48xxxxxx-0x68xxxxxx) | Missing Stage-2 mapping or heap gap collision |

## Debugging Interrupt Injection

### Check List Registers

```gdb
# In handle_irq_exception or inject_pending_sgis:
# Read ICH_LR0-3_EL2 values from arch_state
print/x vcpu.arch_state.ich_lr[0]
print/x vcpu.arch_state.ich_lr[1]
print/x vcpu.arch_state.ich_lr[2]
print/x vcpu.arch_state.ich_lr[3]
```

### Decode LR Values

```
Bits [63:62] = State (00=free, 01=pending, 10=active, 11=pending+active)
Bit [61]     = HW (1=physical linkage)
Bit [60]     = Group1
Bits [55:48] = Priority
Bits [41:32] = pINTID (if HW=1)
Bits [31:0]  = vINTID
```

### Check ELRSR (Empty LR Status)

If all 4 LRs are occupied, new interrupts cannot be injected. Look for:
- Stale Active LRs (state=10) that weren't deactivated
- LRs stuck in Pending+Active (state=11)

### Verify Pending Atomics

```gdb
# Pending SGIs for each vCPU
print PENDING_SGIS[0].load()
print PENDING_SGIS[1].load()
print PENDING_SGIS[2].load()
print PENDING_SGIS[3].load()

# Pending SPIs
print PENDING_SPIS[0].load()
```

## Debugging Multi-vCPU Issues

### Identify Current vCPU

```gdb
print CURRENT_VCPU_ID.load()
print VCPU_ONLINE_MASK.load()
```

### Check Scheduler State

```gdb
# In run_smp():
print scheduler.states[0]  # RunState for vCPU 0
print scheduler.states[1]  # RunState for vCPU 1
print scheduler.states[2]
print scheduler.states[3]
print scheduler.current
```

### Watch PSCI CPU_ON

```gdb
break boot_secondary_vcpu
# When hit, check:
print vcpu_id
print entry_point
print context_id
```

### Preemption Timer

```gdb
# Check if CNTHP timer is enabled
break ensure_cnthp_enabled
# Verify INTID 26 is enabled in GICR
```

## Common Issues and Solutions

### "Wrong magic value 0x00000000" for virtio-mmio

**Root cause**: MMIO trap computes wrong IPA because it uses FAR_EL2 (guest VA) instead of HPFAR_EL2.

**Fix**: Use `(hpfar & 0x0FFF_FFFF_FFF0) << 8 | (far_el2 & 0xFFF)` for full IPA.

### RCU Stall / Soft Lockup

**Root cause**: SGI/IPI delivery failure — one vCPU waits forever for an IPI that never arrives.

**Diagnosis**:
1. Check `PENDING_SGIS` — are SGIs being queued?
2. Check `inject_pending_sgis()` — are they being drained?
3. Check LRs — are they being written correctly?
4. Check preemption timer — is CNTHP INTID 26 being re-enabled?

**Common fixes**:
- ICC_SGI1R_EL1 bit fields: TargetList is [15:0], NOT [23:16]; INTID is [27:24], NOT [3:0]
- Preemptive timer: guest can disable INTID 26 via GICR writes; call `ensure_cnthp_enabled()`

### Kernel Panic on Secondary CPU Boot

**Root cause**: Incorrect PSCI CPU_ON register setup.

**Checklist**:
- x0 = context_id (not garbage)
- SPSR_EL2 = EL1h with DAIF masked (0x3C5)
- sctlr_el1 = 0x30D00800 (MMU off, caches off)
- CPACR_EL1 = 0x300000 (FP/SIMD enabled)
- VMPIDR_EL2.Aff0 = vcpu_id (unique per vCPU)
- ICH_HCR_EL2 = TALL1 | En (virtual GIC enabled)
- GICR waker: ProcessorSleep cleared for the new vCPU's redistributor

### Spinlock Deadlock in Linux SMP

**Root cause**: Hypervisor modifying guest's SPSR_EL2 (clearing PSTATE.I).

**Rule**: NEVER modify SPSR_EL2 on exception return. The guest controls its own interrupt masking.

### Guest Can't Receive Interrupts

**Diagnosis**:
1. Is ICH_HCR_EL2.En set? (enables virtual interface)
2. Is ICH_VMCR_EL2.VPMR set to 0xFF? (allow all priorities)
3. Is ICH_VMCR_EL2.VENG1 set? (enable Group 1)
4. Is the LR correctly formatted? (State=Pending, Group1=1)
5. Is the guest's PSTATE.I clear? (interrupts unmasked)

### Disk Not Detected (virtio-blk)

**Checklist**:
- Kernel has `CONFIG_VIRTIO_MMIO=y` (built-in, not module)
- Kernel has `CONFIG_VIRTIO_BLK=y`
- DTB has `virtio_mmio@a000000` node with correct interrupt
- Virtio device registered in DeviceManager
- `flush_pending_spis_to_hardware()` called after MMIO write handling

## Adding Debug Output

### Hypervisor UART Output

```rust
crate::uart_puts(b"[DEBUG] Some message\n");
```

This writes directly to the physical PL011 UART. Available from any context including exception handlers.

### Hex Dump Helper

```rust
fn print_hex(val: u64) {
    let hex = b"0123456789abcdef";
    crate::uart_puts(b"0x");
    for i in (0..16).rev() {
        let nibble = ((val >> (i * 4)) & 0xF) as usize;
        crate::uart_puts(&[hex[nibble]]);
    }
}
```

## QEMU Tracing

For low-level debugging, enable QEMU's trace flags:

```bash
# Interrupt tracing
qemu-system-aarch64 ... -d int

# Guest errors
qemu-system-aarch64 ... -d guest_errors

# All exceptions
qemu-system-aarch64 ... -d int,guest_errors,exec

# To file (avoids console flooding)
qemu-system-aarch64 ... -d int -D qemu_trace.log
```

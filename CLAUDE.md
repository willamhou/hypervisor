# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ARM64 Type-1 bare-metal hypervisor written in Rust (no_std) with ARM64 assembly. Runs at EL2 (hypervisor exception level) and manages guest VMs at EL1. Educational project targeting QEMU virt machine.

## Build Commands

```bash
make              # Build hypervisor
make run          # Build and run in QEMU (exit: Ctrl+A then X)
make debug        # Build and run with GDB server on port 1234
make clean        # Clean build artifacts
make check        # Check code without building
make clippy       # Run linter
make fmt          # Format code
```

**Debug session:**
```bash
# Terminal 1
make debug

# Terminal 2
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) b rust_main
(gdb) c
```

## Architecture

### Privilege Model
- **EL2**: Hypervisor code runs here
- **EL1**: Guest code runs here
- **Stage-2 Translation**: IPA (Guest Physical) → PA (Host Physical)

### Key Abstractions

1. **VcpuContext** (`src/arch/aarch64/regs.rs`) - All guest register state (x0-x30, SP, PC, system registers)

2. **Vcpu** (`src/vcpu.rs`) - Virtual CPU with state machine (Uninitialized → Ready → Running → Stopped), manages context and virtual interrupts

3. **Vm** (`src/vm.rs`) - Manages up to 8 vCPUs, handles Stage-2 memory setup, holds device manager

4. **DeviceManager** (`src/devices/mod.rs`) - Routes MMIO accesses to emulated devices (PL011 UART, GIC Distributor)

5. **ExitReason** (`src/arch/aarch64/regs.rs`) - VM exit causes: WfiWfe, HvcCall, DataAbort, etc.

### Exception Handling Flow
```
Guest @ EL1
  ↓ trap
Exception Vector (arch/aarch64/exception.S)
  ↓ save context
handle_exception() (src/arch/aarch64/hypervisor/exception.rs)
  ↓ decode ESR_EL2, route by type
  - WFI: return exit
  - Hypercall: handle + advance PC
  - Data Abort: decode instruction → MMIO emulation
  ↓ restore context
ERET back to guest
```

### MMIO Trap-and-Emulate
1. Guest load/store to MMIO region causes Data Abort
2. Handler reads FAR_EL2 (fault address) and decodes instruction
3. DeviceManager routes to appropriate device
4. Result written to guest register, PC advanced, ERET resumes guest

### Memory Layout
- **Stage-2 Pages**: 2MB blocks (L2 descriptors)
- **Identity Mapping**: GPA == HPA
- **IPA Space**: 40-bit (1TB), PA Space: 48-bit
- **Page Attributes**: NORMAL (cached), DEVICE (uncached)

### Interrupt Injection
- **GICv3**: Uses List Registers (ICH_LR_EL2) for injection
- **Fallback**: HCR_EL2.VI bit for legacy injection
- vcpu.inject_irq(irq_num) → hardware injects into guest → guest handles + EOI

## Key Files

| Path | Purpose |
|------|---------|
| `arch/aarch64/boot.S` | Entry point, stack setup, BSS clear |
| `arch/aarch64/exception.S` | Exception vector table (2KB aligned), context save/restore |
| `src/main.rs` | rust_main entry, test orchestration |
| `src/arch/aarch64/hypervisor/exception.rs` | Exception handler, ESR_EL2 decode |
| `src/arch/aarch64/hypervisor/decode.rs` | Instruction decoder for MMIO |
| `src/arch/aarch64/mm/mmu.rs` | Stage-2 page tables, IdentityMapper |
| `src/arch/aarch64/peripherals/gicv3.rs` | GICv3 system registers, List Registers |
| `src/devices/mod.rs` | MmioDevice trait, DeviceManager |
| `src/global.rs` | Global device manager for exception handler access |

## Build System

- **build.rs**: Compiles boot.S and exception.S via aarch64-linux-gnu-gcc, creates libboot.a
- **Target**: aarch64-unknown-none (custom spec in aarch64-unknown-none.json)
- **Toolchain**: Rust nightly with rust-src, rustfmt, clippy
- **Linking**: whole-archive to include all assembly symbols

## Tests

Tests run automatically on `make run`. Located in `tests/`:
- `test_guest.rs` - Basic hypercall
- `test_timer.rs` - Timer interrupt detection
- `test_mmio.rs` - MMIO device emulation
- `test_complete_interrupt.rs` - End-to-end interrupt handling

## Technical Notes

### Exception Loop Prevention
`MAX_CONSECUTIVE_EXCEPTIONS = 100` in exception.rs prevents infinite loops; system halts if exceeded.

### Global Device Access
Exception handlers use `global::DEVICES` static to access DeviceManager without passing through assembly.

### HCR_EL2 Configuration
Key bits: RW (AArch64 EL1), TWI/TWE (trap WFI/WFE), AMO/IMO/FMO (route exceptions to EL2)

### Device Trait
```rust
pub trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;
}
```

### Emulated Devices
- **PL011 UART**: 0x09000000 (UARTDR, UARTFR, UARTCR)
- **GIC Distributor**: 0x08000000 (GICD_CTLR, GICD_TYPER, ISENABLER/ICENABLER)

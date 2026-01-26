# Sprint 1.1: vCPU Framework - Summary

## Overview
Sprint 1.1 focused on implementing the foundational vCPU (Virtual CPU) framework for the ARM64 hypervisor. This includes data structures, exception handling, and the VM/vCPU management layer.

## Completed Components

### 1. ARM64 Register Definitions (`src/arch/aarch64/regs.rs`)

#### Data Structures:
- **`GeneralPurposeRegs`**: All 31 general-purpose registers (x0-x30)
- **`SystemRegs`**: Key system registers for virtualization:
  - EL1 state: `sp_el1`, `elr_el1`, `spsr_el1`, `sctlr_el1`
  - Translation: `ttbr0_el1`, `ttbr1_el1`, `tcr_el1`, `mair_el1`
  - Exception info: `esr_el2`, `far_el2`, `hcr_el2`
  - Thread IDs and context
- **`VcpuContext`**: Complete CPU state (GP regs + system regs + PC + SP)
- **`ExitReason`**: Enum for VM exit causes (WFI/WFE, HVC, MSR/MRS traps, aborts, etc.)

#### Key Features:
- Zero-copy register context switching
- Exit reason decoding from ESR_EL2
- Default initialization for safe vCPU creation

### 2. Exception Vector Table (`arch/aarch64/exception.S`)

#### Implementation:
- **16-entry vector table** (0x800 bytes, 2KB aligned):
  - Current EL with SP_EL0/SP_ELx
  - Lower EL (AArch64) - guest exceptions
  - Lower EL (AArch32) - not supported
- **Exception handler**: Saves/restores full vCPU context
- **`enter_guest` function**: Enters guest at EL1 via ERET
- **Global context pointer**: `current_vcpu_context` for exception communication

#### Vector Table Layout:
```
VBAR_EL2 + 0x000: Sync from Current EL with SP_EL0
VBAR_EL2 + 0x080: IRQ from Current EL with SP_EL0
...
VBAR_EL2 + 0x400: Sync from Lower EL (AArch64) <- Guest traps
VBAR_EL2 + 0x480: IRQ from Lower EL (AArch64)
...
```

### 3. Exception Handling (`src/arch/aarch64/exception.rs`)

#### Functions:
- **`init()`**: Initialize EL2 exception handling
  - Load VBAR_EL2 with vector table address
  - Configure HCR_EL2 for guest trapping (WFI/WFE, interrupts, etc.)
- **`handle_exception()`**: Rust exception handler called from assembly
  - Reads ESR_EL2 and FAR_EL2
  - Decodes exit reason
  - Handles basic cases: WFI/WFE, HVC, MSR/MRS, aborts

#### Hypercalls Implemented:
- **Hypercall 0**: Print character (guest -> host UART)
- **Hypercall 1**: Exit guest

### 4. vCPU Management (`src/vcpu.rs`)

#### `Vcpu` Structure:
- ID, state (Uninitialized/Ready/Running/Stopped)
- Register context (`VcpuContext`)
- `run()` method to enter guest
- `stop()`, `reset()` methods

#### State Machine:
```
Uninitialized -> Ready -> Running -> Ready
                            |
                            v
                         Stopped
```

### 5. VM Management (`src/vm.rs`)

#### `Vm` Structure:
- ID, state, array of vCPUs (max 8)
- Methods: `add_vcpu()`, `run()`, `pause()`, `resume()`, `stop()`
- Simple scheduler (currently runs vCPU 0 only)

#### VM State Machine:
```
Uninitialized -> Ready -> Running <-> Paused
                             |
                             v
                          Stopped
```

### 6. Build System Updates

#### `build.rs`:
- Compiles `boot.S` and `exception.S` with `aarch64-linux-gnu-gcc`
- Creates `libboot.a` archive
- Links with `--whole-archive` to ensure all symbols are included

#### `.cargo/config.toml`:
- Removed `--whole-archive` from rustflags (moved to build.rs)
- Kept linker script and relocation model settings

#### `Makefile`:
- Updated to use ELF format (not raw binary)
- Added `virtualization=on` to QEMU machine type
- This ensures QEMU starts at EL2

## Architecture Decisions

### 1. Context Switching Strategy
- **Choice**: Full context save/restore on every VM exit
- **Rationale**: Simple, correct, and sufficient for MVP
- **Future**: Optimize with lazy context switching

### 2. Exception Vector Table
- **Choice**: Single vector table for all exceptions
- **Rationale**: ARM64 architecture requirement
- **Implementation**: Assembly for performance and control

### 3. VM/vCPU Separation
- **Choice**: Separate `Vm` and `Vcpu` structures
- **Rationale**: Mirrors real hardware, supports SMP guests in future
- **Current**: Single vCPU per VM, will expand later

### 4. EL2 Operation
- **Choice**: Run hypervisor at EL2, guests at EL1
- **Rationale**: Hardware-assisted virtualization, proper privilege separation
- **Configuration**: QEMU `virtualization=on` required

## Testing

### Test Output:
```
========================================
  ARM64 Hypervisor - Sprint 1.1
  vCPU Framework Test
========================================

[INIT] Initializing at EL2...
[INIT] Setting up exception vector table...
[INIT] Exception handling initialized
[INIT] Current EL: EL2

[TEST] Creating VM...
[TEST] VM created successfully
[TEST] vCPU framework is ready!

========================================
Sprint 1.1: vCPU Framework - COMPLETE
========================================
```

### Verified:
✅ Exception vector table installation  
✅ EL2 operation confirmed  
✅ HCR_EL2 configuration  
✅ VM/vCPU object creation  
✅ Module integration  

## File Structure

```
hypervisor/
├── arch/aarch64/
│   ├── boot.S              # Boot code (from Milestone 0)
│   ├── exception.S         # NEW: Exception vector table and handlers
│   └── linker.ld           # Linker script
├── src/
│   ├── arch/
│   │   └── aarch64/
│   │       ├── mod.rs      # NEW: Architecture module
│   │       ├── regs.rs     # NEW: Register definitions
│   │       └── exception.rs # NEW: Exception handling
│   ├── vcpu.rs             # NEW: vCPU management
│   ├── vm.rs               # NEW: VM management
│   ├── lib.rs              # Updated: Added new modules
│   └── main.rs             # Updated: Test vCPU framework
├── build.rs                # Updated: Compile exception.S
└── Makefile                # Updated: ELF format, EL2 boot
```

## Known Issues and Limitations

### Current Limitations:
1. **No actual guest execution yet**: VM/vCPU structures exist but no guest code runs
2. **No memory management**: Page tables not implemented
3. **Single vCPU only**: Scheduler doesn't support multiple vCPUs
4. **Basic exception handling**: Only handles a few exception types
5. **No interrupt routing**: IRQ/FIQ handling not implemented

### Next Steps (Sprint 1.2+):
1. Implement guest memory management (page tables)
2. Load and execute simple guest code
3. Handle more exception types (especially MMIO traps)
4. Implement proper interrupt routing
5. Add vCPU context switching optimizations

## Performance Considerations

### Context Switch Cost:
- Full register save: 31 GP regs + ~17 system regs = 48 registers * 8 bytes = 384 bytes
- Assembly implementation: ~50-60 instructions
- Estimated cost: ~100-200 cycles (without memory latency)

### Future Optimizations:
1. **Lazy FP/SIMD context**: Save only when guest uses FP
2. **Dirty register tracking**: Only save/restore changed registers
3. **TLB management**: Minimize TLB flushes with ASID
4. **Interrupt handling**: Fast-path for common interrupts

## Security Considerations

### Current Status:
✅ Hypervisor runs at EL2 (higher privilege than guest)  
✅ HCR_EL2 configured to trap sensitive operations  
✅ Exception vector table properly aligned and configured  
⚠️ No memory isolation yet (guest can access host memory)  
⚠️ No resource limits (guest can monopolize CPU)  

### Required for Production:
1. Stage-2 translation (guest physical -> host physical)
2. Resource quotas (CPU time, memory limits)
3. Secure boot and attestation
4. Side-channel mitigations

## Lessons Learned

### Build System:
- **Issue**: Assembly code was being linked twice (lib + bin)
- **Solution**: Moved `--whole-archive` from `.cargo/config.toml` to `build.rs`
- **Lesson**: Cargo build scripts run for both lib and bin targets

### QEMU Configuration:
- **Issue**: QEMU started at EL1 instead of EL2
- **Solution**: Added `virtualization=on` to machine type, used ELF format
- **Lesson**: Raw binary doesn't convey privilege level info to QEMU

### Exception Handling:
- **Issue**: Vector entries must be exactly 128 bytes
- **Solution**: Used `.align 7` and `.skip` to ensure correct layout
- **Lesson**: ARM64 architecture has strict alignment requirements

## Conclusion

Sprint 1.1 successfully established the vCPU framework foundation:
- ✅ Complete register context management
- ✅ ARM64 exception vector table and handling
- ✅ VM/vCPU management structures
- ✅ Build system integration
- ✅ EL2 operation verified

The framework is now ready for the next phase: implementing memory management and actually running guest code.

**Status**: ✅ **COMPLETE**  
**Next Sprint**: 1.2 - Memory Management (Page Tables, Stage-2 Translation)

# Contributing to ARM64 Hypervisor

## Development Setup

### Prerequisites

- Rust nightly toolchain
- QEMU with ARM64 support
- ARM64 cross-compilation toolchain

```bash
# Install Rust target
rustup target add aarch64-unknown-none

# Install QEMU (Ubuntu/Debian)
sudo apt install qemu-system-arm

# Install cross-compiler (for objcopy)
sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu
```

### Building

```bash
make build    # Compile the hypervisor
make run      # Build and run in QEMU
make debug    # Run with GDB server on port 1234
make clean    # Clean build artifacts
make check    # Check code without building
make clippy   # Run clippy linter
make fmt      # Format code

# Guest boot targets
make run-android      # Boot Android-configured kernel
make run-sel2         # Boot TF-A with BL32 at S-EL2
make run-tfa-linux    # Boot TF-A → hypervisor (BL33) → Linux
make run-spmc         # Boot TF-A → our SPMC (BL32) at S-EL2
make run-tfa-linux-ffa # Boot TF-A → SPMC → BL33 → Linux (FF-A)
make run-pkvm         # Boot pKVM (NS-EL2) + our SPMC (S-EL2)

# Build infrastructure (Docker-based)
make build-tfa        # Build TF-A flash.bin (Docker)
make build-tfa-bl33   # Build TF-A with PRELOADED_BL33
make build-spmc       # Build hypervisor as S-EL2 SPMC binary
make build-tfa-spmc   # Build TF-A with real SPMC + SPs
make build-pkvm-kernel # Build AOSP kernel for pKVM (Docker)
make build-tfa-pkvm   # Build TF-A flash-pkvm.bin
```

### Testing

33 test suites (~347 assertions) run automatically during `make run`. The output shows pass/fail status for each test suite. Test registry uses macro-based registration in `tests/mod.rs`.

To add a new test:

1. Create `tests/test_<name>.rs` with a `pub fn run_<name>_test()` function
2. Add `pub mod test_<name>;` to `tests/mod.rs`
3. Add entry to `register_tests!()` macro in `tests/mod.rs` (main.rs no longer needs editing)

## Project Structure

```
hypervisor/
├── src/
│   ├── main.rs              # Entry point, test runner
│   ├── lib.rs               # Library root
│   ├── vcpu.rs              # vCPU abstraction
│   ├── vm.rs                # VM management
│   ├── scheduler.rs         # vCPU scheduler
│   ├── global.rs            # Per-VM atomics, global state
│   ├── guest_loader.rs      # Linux/Zephyr boot configuration
│   ├── platform.rs          # Board constants + DTB-backed helpers
│   ├── dtb.rs               # Runtime hardware discovery from host DTB
│   ├── vswitch.rs           # L2 virtual switch with MAC learning
│   ├── sync.rs              # Ticket SpinLock<T>
│   ├── uart.rs              # Physical PL011 driver
│   ├── percpu.rs            # Per-CPU context (MPIDR → PerCpuContext)
│   ├── log.rs               # Structured logging macros
│   ├── manifest.rs          # SPMC manifest parser (TOS_FW_CONFIG DTB)
│   ├── secure_stage2.rs     # Secure Stage-2 config (VSTTBR/VSTCR)
│   ├── sel2_mmu.rs          # S-EL2 Stage-1 identity map
│   ├── sp_context.rs        # Per-SP state machine, INTID ownership
│   ├── spmc_handler.rs      # S-EL2 SPMC event loop + FF-A dispatch
│   ├── arch/
│   │   ├── traits.rs        # Portable trait definitions
│   │   └── aarch64/
│   │       ├── boot.S       # Boot code (assembly)
│   │       ├── regs.rs      # Register definitions
│   │       ├── defs.rs      # ARM64 named constants
│   │       ├── vcpu_arch_state.rs # Per-vCPU GIC/timer/sysreg state
│   │       ├── hypervisor/  # EL2 code (exception handling, decode)
│   │       ├── mm/          # Memory management, Stage-2
│   │       └── peripherals/ # GIC, Timer drivers
│   ├── devices/             # MMIO device emulation
│   │   ├── mod.rs           # DeviceManager (enum dispatch)
│   │   ├── pl011/           # UART emulation
│   │   ├── pl031.rs         # PL031 RTC emulation
│   │   ├── gic/             # GICD/GICR emulation
│   │   └── virtio/          # virtio-mmio transport
│   │       ├── blk.rs       # virtio-blk backend
│   │       ├── net.rs       # virtio-net backend
│   │       ├── mmio.rs      # virtio-mmio register interface
│   │       └── queue.rs     # Virtqueue implementation
│   ├── ffa/                 # FF-A v1.1 proxy
│   │   ├── mod.rs           # FF-A constants and types
│   │   ├── proxy.rs         # SMC interception and dispatch
│   │   ├── mailbox.rs       # RXTX mailbox management
│   │   ├── stub_spmc.rs     # Simulated Secure Partitions
│   │   ├── memory.rs        # Page ownership tracking
│   │   ├── stage2_walker.rs # Stage-2 PTE walker for SW bits
│   │   ├── descriptors.rs   # Memory region descriptor parsing
│   │   ├── notifications.rs # Per-partition notification bitmaps
│   │   └── smc_forward.rs   # SMC forwarding to EL3
│   └── mm/                  # Heap allocator
│       └── region_registry.rs # Stage-2 region registration
└── tests/                   # 33 test suites (~347 assertions)
```

The `Device` enum uses enum-dispatch (no dynamic dispatch):

```
Device ──variants──▶ VirtualUart | VirtualGicd | VirtualGicr
                   | VirtioMmioTransport<VirtioBlk>
                   | VirtioMmioTransport<VirtioNet>
                   | VirtualPl031
```

## Code Style

### Rust

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Add rustdoc comments for all public APIs
- Use `#[inline]` sparingly, only for performance-critical code

### Documentation

```rust
/// Brief description (one line)
///
/// Longer description if needed.
///
/// # Arguments
///
/// * `param` - Description of parameter
///
/// # Returns
///
/// Description of return value
///
/// # Errors
///
/// When and why this function returns an error
///
/// # Example
///
/// ```rust,ignore
/// let result = function(arg);
/// ```
pub fn function(param: Type) -> Result<T, E> { ... }
```

### Assembly

- Comment every instruction or logical block
- Use consistent register conventions:
  - `x0-x7`: Arguments and return values
  - `x8`: Indirect result location
  - `x9-x15`: Temporary (caller-saved)
  - `x16-x17`: Intra-procedure-call scratch
  - `x18`: Platform register (reserved)
  - `x19-x28`: Callee-saved
  - `x29`: Frame pointer (FP)
  - `x30`: Link register (LR)
  - `sp`: Stack pointer

## Adding Features

### New MMIO Device

1. Create `src/devices/<name>.rs`
2. Define device struct with registers
3. Implement `MmioDevice` trait
4. Add to `DeviceManager` in `src/devices/mod.rs`
5. Map MMIO region in Stage-2 tables (if needed)
6. Add test in `tests/test_<name>.rs`

### New System Register Trap

1. Add handler in `src/arch/aarch64/hypervisor/exception.rs`
2. Decode ISS field from ESR_EL2
3. Implement emulation logic
4. Update guest registers as needed
5. Advance PC past the trapped instruction

### New Architecture Code

- EL2 code: `src/arch/aarch64/hypervisor/`
- Memory management: `src/arch/aarch64/mm/`
- Peripherals: `src/arch/aarch64/peripherals/`

### Feature Flags

| Feature | Implies | Description |
|---------|---------|-------------|
| `(default)` | — | Unit tests only, no guest boot |
| `guest` | — | Zephyr guest loading |
| `linux_guest` | — | Linux guest with DynamicIdentityMapper, GICR trap, virtio |
| `multi_pcpu` | `linux_guest` | 4 vCPUs on 4 pCPUs (1:1 affinity) |
| `multi_vm` | `linux_guest` | 2 VMs time-sliced on 1 pCPU |
| `sel2` | — | S-EL2 SPMC mode (BL32) |
| `tfa_boot` | `linux_guest` | TF-A boot with real SPMC |

Note: `multi_pcpu` and `multi_vm` are mutually exclusive. `sel2` is mutually exclusive with all others.

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `refactor`: Code change without behavior change
- `test`: Adding or updating tests
- `chore`: Build, CI, tooling changes

### Examples

```
feat(vcpu): add virtual interrupt injection

Implement IRQ injection using GICv3 List Registers.
Supports edge and level triggered interrupts.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```

```
fix(mm): handle unaligned page table allocation

The bump allocator was returning unaligned addresses
for page table allocations. Fixed by rounding up.
```

## Pull Requests

1. Create feature branch from `main`
2. Write tests for new functionality
3. Ensure all tests pass (`make run`)
4. Run linter (`make clippy`)
5. Update documentation if needed
6. Submit PR with clear description

## Debugging

### GDB Debugging

```bash
# Terminal 1: Start QEMU with GDB server
make debug

# Terminal 2: Connect GDB
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) break rust_main
(gdb) continue
```

### Useful GDB Commands

```gdb
info registers              # Show all registers
x/10i $pc                   # Disassemble 10 instructions at PC
x/10x $sp                   # Examine stack
p/x $esr_el2                # Print ESR_EL2 (need to read from memory)
```

### QEMU Monitor

Press `Ctrl+A` then `C` to enter QEMU monitor:

```
info registers              # CPU registers
info mtree                  # Memory map
info qtree                  # Device tree
quit                        # Exit QEMU
```

## Resources

- [ARM Architecture Reference Manual (ARMv8-A)](https://developer.arm.com/documentation/ddi0487/latest)
- [ARM GIC Architecture Specification](https://developer.arm.com/documentation/ihi0069/latest)
- [QEMU ARM System Emulator](https://www.qemu.org/docs/master/system/arm/virt.html)

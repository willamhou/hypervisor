# Guest VM Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable hypervisor to load and run real ELF binaries (Zephyr RTOS, then Linux) as guests via QEMU's device loader.

**Architecture:** QEMU loads guest ELF to `0x4800_0000` using `-device loader`. Hypervisor creates Stage-2 identity mappings for guest RAM (128MB) and device MMIO regions. vCPU starts execution at guest entry point.

**Tech Stack:** ARM64 Stage-2 MMU, GICv3, QEMU virt machine, Zephyr RTOS

---

## Phase 1: Minimal Guest Support (UART Only)

### Task 1: Create guest_loader module skeleton

**Files:**
- Create: `src/guest_loader.rs`
- Modify: `src/lib.rs:1-12`

**Step 1: Create the guest_loader.rs file with GuestConfig struct**

Create `src/guest_loader.rs`:

```rust
//! Guest Loader Module
//!
//! This module provides configuration and boot logic for loading
//! real ELF binaries as guests.

/// Guest configuration
///
/// Defines memory layout and entry point for a guest VM.
pub struct GuestConfig {
    /// Guest code load address (where QEMU loads the ELF)
    pub load_addr: u64,
    /// Guest memory size in bytes
    pub mem_size: u64,
    /// Entry point address (usually equals load_addr)
    pub entry_point: u64,
}

impl GuestConfig {
    /// Default configuration for Zephyr RTOS on qemu_cortex_a53
    ///
    /// - Load address: 0x4800_0000
    /// - Memory size: 128MB
    /// - Entry point: 0x4800_0000
    pub const fn zephyr_default() -> Self {
        Self {
            load_addr: 0x4800_0000,
            mem_size: 128 * 1024 * 1024, // 128MB
            entry_point: 0x4800_0000,
        }
    }
}
```

**Step 2: Add module to lib.rs**

In `src/lib.rs`, add after line 11 (`pub mod scheduler;`):

```rust
pub mod guest_loader;
```

**Step 3: Verify it compiles**

Run: `cargo check --target aarch64-unknown-none`
Expected: Compilation succeeds with no errors

**Step 4: Commit**

```bash
git add src/guest_loader.rs src/lib.rs
git commit -m "$(cat <<'EOF'
feat(guest): add guest_loader module skeleton

Add GuestConfig struct for guest VM configuration.
Includes zephyr_default() for Zephyr RTOS settings.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Add extended Stage-2 memory mapping to VM

**Files:**
- Modify: `src/vm.rs:191-243`

**Step 1: Read current init_memory implementation**

Review `src/vm.rs:198-243` to understand current memory mapping logic.

**Step 2: Modify init_memory to accept optional guest region**

Replace `init_memory` method in `src/vm.rs` with version that maps:
- GIC region: `0x0800_0000` (2MB, DEVICE)
- UART region: `0x0900_0000` (2MB, DEVICE)
- Guest region: configurable start/size (NORMAL)

```rust
/// Initialize memory for the VM
///
/// Sets up identity mapping for guest memory and device regions.
///
/// # Arguments
/// * `guest_mem_start` - Start of guest memory region
/// * `guest_mem_size` - Size of guest memory region
pub fn init_memory(&mut self, guest_mem_start: u64, guest_mem_size: u64) {
    use crate::uart_puts;
    use crate::arch::aarch64::mm::IdentityMapper;

    if self.memory_initialized {
        uart_puts(b"[VM] Memory already initialized\n");
        return;
    }

    uart_puts(b"[VM] Initializing memory mapping...\n");

    // Use a global static mapper (to avoid large stack allocation)
    static mut MAPPER: IdentityMapper = IdentityMapper::new();

    // Round guest memory to 2MB boundaries
    let start_aligned = guest_mem_start & !(2 * 1024 * 1024 - 1);
    let size_aligned = ((guest_mem_size + 2 * 1024 * 1024 - 1) / (2 * 1024 * 1024)) * (2 * 1024 * 1024);

    uart_puts(b"[VM] Mapping guest memory: 0x");
    print_hex(start_aligned);
    uart_puts(b" - 0x");
    print_hex(start_aligned + size_aligned);
    uart_puts(b"\n");

    unsafe {
        // Map guest RAM (Normal memory)
        MAPPER.map_region(start_aligned, size_aligned, MemoryAttributes::NORMAL);

        // Map UART (PL011): 0x09000000 (Device memory)
        MAPPER.map_region(0x0900_0000, 2 * 1024 * 1024, MemoryAttributes::DEVICE);

        // Map GIC: 0x08000000 (Device memory for GICD + GICR)
        MAPPER.map_region(0x0800_0000, 2 * 1024 * 1024, MemoryAttributes::DEVICE);

        // Initialize Stage-2 translation
        init_stage2(&MAPPER);
    }

    self.memory_initialized = true;
    uart_puts(b"[VM] Memory mapping complete\n");
}
```

**Step 3: Verify it compiles**

Run: `cargo check --target aarch64-unknown-none`
Expected: Compilation succeeds

**Step 4: Commit**

```bash
git add src/vm.rs
git commit -m "$(cat <<'EOF'
refactor(vm): update init_memory for guest VM support

Memory mapping now includes:
- GIC region at 0x0800_0000 (Device)
- UART region at 0x0900_0000 (Device)
- Guest RAM at configurable address (Normal)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Implement run_guest function

**Files:**
- Modify: `src/guest_loader.rs`

**Step 1: Add run_guest function**

Add to `src/guest_loader.rs`:

```rust
use crate::vm::Vm;
use crate::uart_puts;
use crate::uart_put_hex;

/// Boot a guest VM with the given configuration
///
/// # Arguments
/// * `config` - Guest configuration (memory layout, entry point)
///
/// # Returns
/// * `Ok(())` - Guest exited normally
/// * `Err(msg)` - Error occurred
///
/// # Example
/// ```rust,ignore
/// let config = GuestConfig::zephyr_default();
/// run_guest(&config)?;
/// ```
pub fn run_guest(config: &GuestConfig) -> Result<(), &'static str> {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Guest VM Boot\n");
    uart_puts(b"========================================\n");

    uart_puts(b"[GUEST] Load address: 0x");
    uart_put_hex(config.load_addr);
    uart_puts(b"\n");

    uart_puts(b"[GUEST] Memory size: ");
    uart_put_hex(config.mem_size);
    uart_puts(b" bytes\n");

    uart_puts(b"[GUEST] Entry point: 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"\n\n");

    // Create VM
    uart_puts(b"[GUEST] Creating VM...\n");
    let mut vm = Vm::new(0);

    // Initialize memory mapping for guest
    uart_puts(b"[GUEST] Initializing Stage-2 memory...\n");
    vm.init_memory(config.load_addr, config.mem_size);

    // Create vCPU with guest entry point
    // Stack pointer at end of guest memory region
    let guest_sp = config.load_addr + config.mem_size - 0x1000; // Leave 4KB at top

    uart_puts(b"[GUEST] Creating vCPU...\n");
    uart_puts(b"[GUEST] Stack pointer: 0x");
    uart_put_hex(guest_sp);
    uart_puts(b"\n");

    match vm.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = config.entry_point;
            vcpu.context_mut().sp = guest_sp;
        }
        Err(e) => {
            uart_puts(b"[GUEST] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return Err(e);
        }
    }

    // Enter guest
    uart_puts(b"[GUEST] Entering guest at 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"...\n");
    uart_puts(b"========================================\n\n");

    // Run VM
    vm.run()
}
```

**Step 2: Verify it compiles**

Run: `cargo check --target aarch64-unknown-none`
Expected: Compilation succeeds

**Step 3: Commit**

```bash
git add src/guest_loader.rs
git commit -m "$(cat <<'EOF'
feat(guest): implement run_guest function

Creates VM, sets up Stage-2 memory mapping, creates vCPU,
and enters guest execution at configured entry point.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Add Makefile run-guest target

**Files:**
- Modify: `Makefile`

**Step 1: Add GUEST_ELF variable and run-guest target**

Add after the `run` target in Makefile:

```makefile
# Guest ELF path (set via environment variable)
GUEST_ELF ?=

# Run hypervisor with guest ELF
run-guest: build
ifndef GUEST_ELF
	$(error GUEST_ELF is not set. Usage: make run-guest GUEST_ELF=/path/to/zephyr.elf)
endif
	@echo "Starting QEMU with guest: $(GUEST_ELF)"
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(GUEST_ELF),addr=0x48000000
```

**Step 2: Update help target**

Add to the help target output:

```makefile
	@echo "  run-guest - Build and run with guest ELF (GUEST_ELF=/path/to/elf)"
```

**Step 3: Verify Makefile syntax**

Run: `make help`
Expected: Shows run-guest in the list

**Step 4: Commit**

```bash
git add Makefile
git commit -m "$(cat <<'EOF'
feat(build): add run-guest Makefile target

Usage: make run-guest GUEST_ELF=/path/to/zephyr.elf

Loads guest ELF to 0x48000000 using QEMU device loader.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Add guest boot path to main.rs

**Files:**
- Modify: `src/main.rs`

**Step 1: Add conditional guest boot**

Add feature flag and guest boot path. After the test runs in `rust_main`, add:

```rust
// Check if we should boot a guest
// For now, always try to boot guest at 0x48000000
#[cfg(feature = "guest")]
{
    use hypervisor::guest_loader::{GuestConfig, run_guest};

    uart_puts_local(b"\n[INIT] Booting guest VM...\n");

    let config = GuestConfig::zephyr_default();
    match run_guest(&config) {
        Ok(()) => {
            uart_puts_local(b"[INIT] Guest exited normally\n");
        }
        Err(e) => {
            uart_puts_local(b"[INIT] Guest error: ");
            uart_puts_local(e.as_bytes());
            uart_puts_local(b"\n");
        }
    }
}
```

**Step 2: Add feature to Cargo.toml**

Add to `Cargo.toml`:

```toml
[features]
default = []
guest = []
```

**Step 3: Update Makefile run-guest target**

Modify the run-guest target to enable guest feature:

```makefile
run-guest:
ifndef GUEST_ELF
	$(error GUEST_ELF is not set. Usage: make run-guest GUEST_ELF=/path/to/zephyr.elf)
endif
	@echo "Building hypervisor with guest support..."
	cargo build --target aarch64-unknown-none --features guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with guest: $(GUEST_ELF)"
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(GUEST_ELF),addr=0x48000000
```

**Step 4: Verify it compiles**

Run: `cargo build --target aarch64-unknown-none --features guest`
Expected: Compilation succeeds

**Step 5: Commit**

```bash
git add src/main.rs Cargo.toml Makefile
git commit -m "$(cat <<'EOF'
feat(main): add guest boot path with feature flag

Enable with --features guest or via make run-guest.
Boots guest at 0x48000000 with Zephyr default config.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Create test for guest_loader module

**Files:**
- Create: `tests/test_guest_loader.rs`
- Modify: `tests/mod.rs`

**Step 1: Write the test file**

Create `tests/test_guest_loader.rs`:

```rust
//! Test for guest_loader module
//!
//! Verifies GuestConfig creation and default values.

use hypervisor::guest_loader::GuestConfig;
use hypervisor::uart_puts;

/// Test GuestConfig default values
pub fn run_test() {
    uart_puts(b"\n[TEST] Guest Loader Test\n");
    uart_puts(b"[TEST] ========================\n");

    // Test zephyr_default configuration
    let config = GuestConfig::zephyr_default();

    // Verify load address
    uart_puts(b"[TEST] Checking load_addr... ");
    if config.load_addr == 0x4800_0000 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    // Verify memory size (128MB)
    uart_puts(b"[TEST] Checking mem_size... ");
    if config.mem_size == 128 * 1024 * 1024 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    // Verify entry point
    uart_puts(b"[TEST] Checking entry_point... ");
    if config.entry_point == 0x4800_0000 {
        uart_puts(b"PASS\n");
    } else {
        uart_puts(b"FAIL\n");
        return;
    }

    uart_puts(b"[TEST] Guest Loader Test PASSED\n\n");
}
```

**Step 2: Add to tests/mod.rs**

Add to `tests/mod.rs`:

```rust
pub mod test_guest_loader;
pub use test_guest_loader::run_test as run_guest_loader_test;
```

**Step 3: Add test call to main.rs**

Add before the "All Sprints Complete" message:

```rust
// Run the guest loader test
tests::run_guest_loader_test();
```

**Step 4: Run tests**

Run: `make run`
Expected: Output includes `[TEST] Guest Loader Test PASSED`

**Step 5: Commit**

```bash
git add tests/test_guest_loader.rs tests/mod.rs src/main.rs
git commit -m "$(cat <<'EOF'
test(guest): add guest_loader module tests

Verifies GuestConfig::zephyr_default() returns correct values.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Integration test with simple guest

**Files:**
- Create: `tests/test_simple_guest.rs`
- Modify: `tests/mod.rs`

**Step 1: Create simple inline guest test**

Create `tests/test_simple_guest.rs`:

```rust
//! Simple guest test using inline assembly
//!
//! Tests the guest boot path with a minimal guest that:
//! 1. Prints a character via UART
//! 2. Exits via HVC

use hypervisor::vm::Vm;
use hypervisor::uart_puts;

/// Simple guest code that writes to UART then exits
#[repr(C, align(4096))]
struct SimpleGuest {
    code: [u32; 8],
}

/// Guest code at 0x48000000 (simulated - we place it in static memory)
static SIMPLE_GUEST: SimpleGuest = SimpleGuest {
    code: [
        // Write 'Z' to UART (0x09000000)
        0xd2920000,  // mov x0, #0x9000_0000 (UART base, upper bits)
        0xf2a00000,  // movk x0, #0, lsl #16
        0xd28008a1,  // mov x1, #'Z' (0x5A)
        0xb9000001,  // str w1, [x0]

        // Exit via HVC
        0xd2800020,  // mov x0, #1 (exit hypercall)
        0xd4000002,  // hvc #0

        // Should not reach
        0xd503207f,  // wfe
        0x14000000,  // b .
    ],
};

/// Run simple guest test
pub fn run_test() {
    uart_puts(b"\n[TEST] Simple Guest Test\n");
    uart_puts(b"[TEST] ========================\n");

    let guest_addr = &SIMPLE_GUEST.code as *const _ as u64;
    uart_puts(b"[TEST] Guest code at: 0x");
    print_hex(guest_addr);
    uart_puts(b"\n");

    // Create VM
    let mut vm = Vm::new(1);

    // Initialize memory - map the region containing our guest code
    let mem_start = guest_addr & !(2 * 1024 * 1024 - 1);
    vm.init_memory(mem_start, 4 * 1024 * 1024);

    // Create vCPU
    match vm.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = guest_addr;
            vcpu.context_mut().sp = guest_addr + 0x10000; // Arbitrary stack
        }
        Err(e) => {
            uart_puts(b"[TEST] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return;
        }
    }

    // Run guest
    uart_puts(b"[TEST] Running guest (expect 'Z'): ");

    match vm.run() {
        Ok(()) => {
            uart_puts(b"\n[TEST] Simple Guest Test PASSED\n\n");
        }
        Err(e) => {
            uart_puts(b"\n[TEST] Guest error: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
        }
    }
}

fn print_hex(value: u64) {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];

    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }

    uart_puts(&buffer);
}
```

**Step 2: Add to tests/mod.rs**

```rust
pub mod test_simple_guest;
pub use test_simple_guest::run_test as run_simple_guest_test;
```

**Step 3: Add test call to main.rs**

Add after guest_loader test:

```rust
// Run the simple guest test
tests::run_simple_guest_test();
```

**Step 4: Run tests**

Run: `make run`
Expected: Output includes `[TEST] Simple Guest Test PASSED` and 'Z' printed

**Step 5: Commit**

```bash
git add tests/test_simple_guest.rs tests/mod.rs src/main.rs
git commit -m "$(cat <<'EOF'
test(guest): add simple inline guest test

Tests guest boot path with minimal guest that writes
to UART and exits via HVC.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
EOF
)"
```

---

## Phase 2: Timer Interrupt Support (Future)

### Task 8: Virtual timer configuration

**Deferred** - Implement after Phase 1 is validated with Zephyr

---

## Phase 3: PSCI Support (Future)

### Task 9: PSCI hypercall handler

**Deferred** - Implement after timer support is working

---

## Bugs Found and Fixed

During implementation, we discovered and fixed 4 critical bugs in the existing Stage-2 translation code:

### Bug 1: Missing S2AP in MemoryAttributes

**File:** `src/arch/aarch64/mm/mmu.rs`

**Problem:** `MemoryAttributes::NORMAL` didn't set S2AP (Stage-2 Access Permissions), defaulting to 0b00 (No access).

**Fix:** Added `(0b11 << 4)` for S2AP = Read-Write.

### Bug 2: Invalid VTCR_EL2.T0SZ

**File:** `src/arch/aarch64/mm/mmu.rs`

**Problem:** T0SZ=24 (1TB IPA) is invalid for SL0=1 (Level 1 start). SL0=1 supports max 512GB.

**Fix:** Changed T0SZ from 24 to 25 (512GB IPA).

### Bug 3: Incorrect L2 Table Index in map_2mb_block

**File:** `src/arch/aarch64/mm/mmu.rs`

**Problem:** Used `l1_index - 1` as L2 table index, causing different regions to overwrite each other.

**Fix:** Search `l2_tables` array for matching address from L1 entry.

### Bug 4: Wrong ARM64 Instruction Encoding

**File:** `tests/test_simple_guest.rs`

**Problem:**
- UART address: `0xd2920000` encoded `MOVZ x0, #0x9000` (no shift), result = 0x9000
- 'Z' character: `0xd28008a1` encoded #0x45 = 'E', not 'Z'

**Fix:**
- UART: `0xd2a12000` = `MOVZ x0, #0x900, LSL #16` → 0x09000000
- 'Z': `0xd2800b41` = `MOVZ x1, #0x5A` → 'Z'

---

## Validation Checklist

After completing Phase 1:

- [x] `make run` passes all tests including guest_loader_test
- [x] Simple inline guest prints 'Z' and exits cleanly
- [ ] `cargo clippy --target aarch64-unknown-none` has no warnings
- [ ] `cargo fmt --check` passes
- [ ] (Optional) Zephyr hello_world runs with `make run-guest GUEST_ELF=...`

## Quick Reference

### Memory Layout

| Region | Start | End | Size | Type |
|--------|-------|-----|------|------|
| GIC | `0x0800_0000` | `0x0A00_0000` | 2MB | Device |
| UART | `0x0900_0000` | `0x0B00_0000` | 2MB | Device |
| Hypervisor | `0x4000_0000` | `0x4200_0000` | 32MB | Normal |
| Guest | `0x4800_0000` | `0x5000_0000` | 128MB | Normal |

### Key Commands

```bash
# Build and run tests
make run

# Build with guest support
cargo build --target aarch64-unknown-none --features guest

# Run with Zephyr guest
make run-guest GUEST_ELF=/path/to/zephyr.elf

# Debug with GDB
make debug
```

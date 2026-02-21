# Sprint 4.4 Phase B: SP Boot + Direct Messaging — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot a minimal Secure Partition at S-EL1 from our SPMC at S-EL2, with NWd→SP direct messaging via FF-A.

**Architecture:** TF-A BL2 loads SP binary at 0x0e300000 (SEC_DRAM). SPMC builds Secure Stage-2 page tables (VSTTBR_EL2), creates SpContext, ERETs to S-EL1. SP boots, calls FFA_MSG_WAIT. On DIRECT_REQ from NWd, SPMC ERETs to SP; SP echoes payload via DIRECT_RESP (SMC trap back). Reuses existing `enter_guest()` assembly and `VcpuContext` struct.

**Tech Stack:** Rust (no_std), AArch64 assembly, TF-A v2.12.0, QEMU virt (secure=on, TCG)

---

## Task 1: Platform Constants for S-EL2

**Files:**
- Modify: `src/platform.rs`

**Step 1: Add secure DRAM constants**

Add at the end of `src/platform.rs`:

```rust
// ── S-EL2 SPMC / Secure Partition memory layout ────────────────────
/// SP1 load address in SEC_DRAM (loaded by TF-A BL2 from FIP)
#[cfg(feature = "sel2")]
pub const SP1_LOAD_ADDR: u64 = 0x0e30_0000;
/// SP1 memory size (1MB)
#[cfg(feature = "sel2")]
pub const SP1_MEM_SIZE: u64 = 0x10_0000;
/// SP1 stack pointer (top of SP1 region)
#[cfg(feature = "sel2")]
pub const SP1_STACK_TOP: u64 = SP1_LOAD_ADDR + SP1_MEM_SIZE;
/// SP1 partition ID
#[cfg(feature = "sel2")]
pub const SP1_PARTITION_ID: u16 = 0x8001;
/// Maximum number of Secure Partitions
#[cfg(feature = "sel2")]
pub const MAX_SPS: usize = 4;

/// Secure heap start (for S-EL2 page table allocation)
#[cfg(feature = "sel2")]
pub const SECURE_HEAP_START: u64 = 0x0e50_0000;
/// Secure heap size (~11MB, up to end of SEC_DRAM)
#[cfg(feature = "sel2")]
pub const SECURE_HEAP_SIZE: u64 = 0x0f00_0000 - SECURE_HEAP_START;

/// UART base for SP Stage-2 mapping (SP debug output)
#[cfg(feature = "sel2")]
pub const SP_UART_BASE: u64 = 0x0900_0000;
#[cfg(feature = "sel2")]
pub const SP_UART_SIZE: u64 = 0x1000;
```

**Step 2: Commit**

```bash
git add src/platform.rs
git commit -m "feat(sel2): add S-EL2 platform constants for SP memory layout"
```

---

## Task 2: SpContext — SP State Machine

**Files:**
- Create: `src/sp_context.rs`
- Modify: `src/lib.rs` (add `pub mod sp_context;`)
- Create: `tests/test_sp_context.rs`

**Step 1: Write the unit test**

Create `tests/test_sp_context.rs`:

```rust
//! Unit tests for SpContext — SP state machine and context management.

use hypervisor::sp_context::{SpContext, SpState};

pub fn run(pass: &mut usize) {
    hypervisor::uart_puts(b"  [test_sp_context] ");

    // Test 1: New SpContext has correct initial state
    let ctx = SpContext::new(0x8001, 0x0e300000, 0x0e400000);
    assert_eq!(ctx.state(), SpState::Reset);
    assert_eq!(ctx.sp_id(), 0x8001);
    assert_eq!(ctx.entry_point(), 0x0e300000);
    *pass += 3;

    // Test 2: State transitions Reset → Idle
    let mut ctx = SpContext::new(0x8001, 0x0e300000, 0x0e400000);
    assert!(ctx.transition_to(SpState::Idle).is_ok());
    assert_eq!(ctx.state(), SpState::Idle);
    *pass += 2;

    // Test 3: State transitions Idle → Running
    assert!(ctx.transition_to(SpState::Running).is_ok());
    assert_eq!(ctx.state(), SpState::Running);
    *pass += 2;

    // Test 4: State transitions Running → Idle (SP returned)
    assert!(ctx.transition_to(SpState::Idle).is_ok());
    assert_eq!(ctx.state(), SpState::Idle);
    *pass += 2;

    // Test 5: Invalid transition Reset → Running (must go through Idle)
    let mut ctx2 = SpContext::new(0x8002, 0x0e400000, 0x0e500000);
    assert!(ctx2.transition_to(SpState::Running).is_err());
    *pass += 1;

    // Test 6: VcpuContext fields are set correctly
    let ctx3 = SpContext::new(0x8001, 0x0e300000, 0x0e400000);
    assert_eq!(ctx3.vcpu_ctx().pc, 0x0e300000);
    assert_eq!(ctx3.vcpu_ctx().sp, 0x0e400000);
    assert_eq!(ctx3.vcpu_ctx().spsr_el2, 0x3C5); // EL1h, DAIF masked
    *pass += 3;

    // Test 7: set_x0_x7 and get_x0_x7
    let mut ctx4 = SpContext::new(0x8001, 0x0e300000, 0x0e400000);
    ctx4.set_args(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22);
    let (x0, x1, x2, x3, x4, x5, x6, x7) = ctx4.get_args();
    assert_eq!(x0, 0xAA);
    assert_eq!(x3, 0xDD);
    assert_eq!(x7, 0x22);
    *pass += 3;

    hypervisor::uart_puts(b"16 assertions passed\n");
}
```

**Step 2: Write the implementation**

Create `src/sp_context.rs`:

```rust
//! Secure Partition context management.
//!
//! Each SP has an `SpContext` that holds its register state (via `VcpuContext`)
//! and a state machine tracking its lifecycle.

use crate::arch::aarch64::regs::VcpuContext;
use crate::arch::aarch64::defs::SPSR_EL1H_DAIF_MASKED;

/// SP lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpState {
    /// SP has been loaded but not yet booted.
    Reset,
    /// SP has booted and is waiting for a message (called FFA_MSG_WAIT).
    Idle,
    /// SP is currently executing (SPMC ERETs to it).
    Running,
    /// SP is blocked waiting for an event.
    Blocked,
}

/// Per-SP context: register state + metadata.
pub struct SpContext {
    /// Register context passed to enter_guest() for ERET.
    ctx: VcpuContext,
    /// FF-A partition ID (e.g. 0x8001).
    id: u16,
    /// Current lifecycle state.
    state: SpState,
    /// Cold boot entry point.
    entry: u64,
    /// Secure Stage-2 VSTTBR value for this SP (set after page table creation).
    vsttbr: u64,
}

impl SpContext {
    /// Create a new SP context in Reset state.
    pub fn new(sp_id: u16, entry_point: u64, stack_top: u64) -> Self {
        let mut ctx = VcpuContext::default();
        ctx.pc = entry_point;
        ctx.sp = stack_top;
        ctx.sys_regs.sp_el1 = stack_top;
        ctx.spsr_el2 = SPSR_EL1H_DAIF_MASKED;

        Self {
            ctx,
            id: sp_id,
            state: SpState::Reset,
            entry: entry_point,
            vsttbr: 0,
        }
    }

    pub fn sp_id(&self) -> u16 {
        self.id
    }

    pub fn state(&self) -> SpState {
        self.state
    }

    pub fn entry_point(&self) -> u64 {
        self.entry
    }

    pub fn vsttbr(&self) -> u64 {
        self.vsttbr
    }

    pub fn set_vsttbr(&mut self, vsttbr: u64) {
        self.vsttbr = vsttbr;
    }

    /// Get immutable reference to the VcpuContext.
    pub fn vcpu_ctx(&self) -> &VcpuContext {
        &self.ctx
    }

    /// Get mutable reference to the VcpuContext (for enter_guest).
    pub fn vcpu_ctx_mut(&mut self) -> &mut VcpuContext {
        &mut self.ctx
    }

    /// Validate and perform a state transition.
    pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
        let valid = match (self.state, new_state) {
            (SpState::Reset, SpState::Idle) => true,      // Boot complete
            (SpState::Idle, SpState::Running) => true,     // DIRECT_REQ dispatch
            (SpState::Running, SpState::Idle) => true,     // DIRECT_RESP / MSG_WAIT
            (SpState::Running, SpState::Blocked) => true,  // SP blocked on event
            (SpState::Blocked, SpState::Running) => true,  // Unblocked
            _ => false,
        };
        if valid {
            self.state = new_state;
            Ok(())
        } else {
            Err("invalid SP state transition")
        }
    }

    /// Set x0-x7 in the context (for passing DIRECT_REQ args before ERET).
    pub fn set_args(&mut self, x0: u64, x1: u64, x2: u64, x3: u64,
                    x4: u64, x5: u64, x6: u64, x7: u64) {
        self.ctx.gp_regs.x0 = x0;
        self.ctx.gp_regs.x1 = x1;
        self.ctx.gp_regs.x2 = x2;
        self.ctx.gp_regs.x3 = x3;
        self.ctx.gp_regs.x4 = x4;
        self.ctx.gp_regs.x5 = x5;
        self.ctx.gp_regs.x6 = x6;
        self.ctx.gp_regs.x7 = x7;
    }

    /// Read x0-x7 from the context (after SP traps back with DIRECT_RESP).
    pub fn get_args(&self) -> (u64, u64, u64, u64, u64, u64, u64, u64) {
        (
            self.ctx.gp_regs.x0,
            self.ctx.gp_regs.x1,
            self.ctx.gp_regs.x2,
            self.ctx.gp_regs.x3,
            self.ctx.gp_regs.x4,
            self.ctx.gp_regs.x5,
            self.ctx.gp_regs.x6,
            self.ctx.gp_regs.x7,
        )
    }
}
```

**Step 3: Register module in lib.rs**

Add `pub mod sp_context;` to `src/lib.rs` (after `spmc_handler`).

**Step 4: Wire test into main.rs**

In `src/main.rs`, add the test call after the existing `test_spmc_handler`:

```rust
mod test_sp_context;
// ...
test_sp_context::run(&mut pass);
```

**Step 5: Build and run tests**

```bash
make clean && make run
```

Expected: `[test_sp_context] 16 assertions passed`

**Step 6: Commit**

```bash
git add src/sp_context.rs src/lib.rs tests/test_sp_context.rs src/main.rs
git commit -m "feat(sel2): add SpContext with state machine and unit tests (16 assertions)"
```

---

## Task 3: Secure Stage-2 Page Tables

**Files:**
- Create: `src/secure_stage2.rs`
- Modify: `src/lib.rs` (add `pub mod secure_stage2;`)
- Create: `tests/test_secure_stage2.rs`

**Step 1: Write the unit test**

Create `tests/test_secure_stage2.rs`:

```rust
//! Unit tests for SecureStage2Config.

use hypervisor::secure_stage2::SecureStage2Config;

pub fn run(pass: &mut usize) {
    hypervisor::uart_puts(b"  [test_secure_stage2] ");

    // Test 1: Config creation has correct VSTTBR/VSTCR
    let config = SecureStage2Config::new(0x1000_0000);
    // VSTTBR should contain the page table address
    assert_eq!(config.vsttbr & 0x0000_FFFF_FFFF_F000, 0x1000_0000);
    *pass += 1;

    // Test 2: VSTCR has expected T0SZ, SL0, granule bits
    let vstcr = config.vstcr;
    // T0SZ = 16 (bits [5:0])
    assert_eq!(vstcr & 0x3F, 16);
    *pass += 1;

    // Test 3: Different page table addresses produce different VSTTBR
    let config2 = SecureStage2Config::new(0x2000_0000);
    assert_ne!(config.vsttbr, config2.vsttbr);
    *pass += 1;

    hypervisor::uart_puts(b"3 assertions passed\n");
}
```

**Step 2: Write the implementation**

Create `src/secure_stage2.rs`:

```rust
//! Secure Stage-2 page tables for SP isolation at S-EL2.
//!
//! Uses VSTTBR_EL2/VSTCR_EL2 (not VTTBR/VTCR) to provide
//! address translation for Secure Partitions at S-EL1.
//! Reuses the same page table format as the NS Stage-2.

use crate::arch::aarch64::defs::*;
use crate::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute, PAGE_SIZE};

/// Secure Stage-2 configuration (VSTTBR_EL2 + VSTCR_EL2).
pub struct SecureStage2Config {
    pub vsttbr: u64,
    pub vstcr: u64,
}

impl SecureStage2Config {
    /// Create configuration from a page table base address.
    pub fn new(page_table_addr: u64) -> Self {
        let vstcr = VTCR_T0SZ_48BIT
            | VTCR_SL0_LEVEL0
            | VTCR_IRGN0_WB
            | VTCR_ORGN0_WB
            | VTCR_SH0_INNER
            | VTCR_TG0_4KB
            | VTCR_PS_48BIT;

        let vsttbr = page_table_addr & 0x0000_FFFF_FFFF_FFFE;

        Self { vsttbr, vstcr }
    }

    /// Install Secure Stage-2 to hardware registers.
    /// Must be called at S-EL2 before ERET to S-EL1.
    #[cfg(feature = "sel2")]
    pub fn install(&self) {
        unsafe {
            core::arch::asm!(
                "msr s3_4_c2_c6_2, {vstcr}",  // VSTCR_EL2
                "isb",
                vstcr = in(reg) self.vstcr,
                options(nostack, nomem),
            );
            core::arch::asm!(
                "msr s3_4_c2_c6_0, {vsttbr}",  // VSTTBR_EL2
                "isb",
                vsttbr = in(reg) self.vsttbr,
                options(nostack, nomem),
            );
        }
    }
}

/// Build Secure Stage-2 page tables for an SP.
///
/// Identity-maps the SP's code/data region and UART for debug output.
/// Returns the L0 table physical address (for VSTTBR_EL2).
///
/// # Arguments
/// * `sp_base` - SP code/data base address (e.g. 0x0e300000)
/// * `sp_size` - SP memory region size (e.g. 1MB)
#[cfg(feature = "sel2")]
pub fn build_sp_stage2(sp_base: u64, sp_size: u64) -> Result<DynamicIdentityMapper, &'static str> {
    let mut mapper = DynamicIdentityMapper::new();

    // Map SP code/data region (Normal memory, identity-mapped)
    mapper.map_region(sp_base, sp_size, MemoryAttribute::Normal)?;

    // Map UART for SP debug output (Device memory)
    mapper.map_region(
        crate::platform::SP_UART_BASE,
        crate::arch::aarch64::defs::BLOCK_SIZE_2MB,  // 2MB block containing UART
        MemoryAttribute::Device,
    )?;

    Ok(mapper)
}
```

Note: `VSTTBR_EL2` and `VSTCR_EL2` are not recognized by the assembler as named registers. We use their system register encoding:
- `VSTTBR_EL2` = `S3_4_C2_C6_0` (op0=3, op1=4, CRn=2, CRm=6, op2=0)
- `VSTCR_EL2` = `S3_4_C2_C6_2` (op0=3, op1=4, CRn=2, CRm=6, op2=2)

**Step 3: Register module and wire tests**

Add `pub mod secure_stage2;` to `src/lib.rs`.

Wire `test_secure_stage2::run(&mut pass)` into `src/main.rs`.

**Step 4: Build and run tests**

```bash
make clean && make run
```

Expected: `[test_secure_stage2] 3 assertions passed`

**Step 5: Commit**

```bash
git add src/secure_stage2.rs src/lib.rs tests/test_secure_stage2.rs src/main.rs
git commit -m "feat(sel2): add SecureStage2Config (VSTTBR_EL2/VSTCR_EL2) with unit tests"
```

---

## Task 4: SP Hello Binary

**Files:**
- Create: `tfa/sp_hello/start.S`
- Create: `tfa/sp_hello/linker.ld`

**Step 1: Write the SP assembly**

Create `tfa/sp_hello/start.S`:

```asm
/*
 * SP Hello — minimal Secure Partition running at S-EL1.
 *
 * Boot sequence:
 *   1. Print "[SP] Hello from S-EL1"
 *   2. Call FFA_MSG_WAIT (SMC) to signal idle to SPMC
 *   3. On wakeup (SPMC ERETs with DIRECT_REQ args in x0-x7):
 *      - Echo x3-x7 back via FFA_MSG_SEND_DIRECT_RESP
 *      - Loop back to FFA_MSG_WAIT
 *
 * Loaded at 0x0e300000 by TF-A BL2. Stack at 0x0e400000.
 */

.section .text.boot
.global _start

.equ UART_BASE,          0x09000000
.equ UARTDR,             0x000
.equ UARTFR,             0x018

.equ FFA_MSG_WAIT,       0x8400006B
.equ FFA_DIRECT_REQ_32,  0x8400006F
.equ FFA_DIRECT_RESP_32, 0x84000070
.equ FFA_ERROR,          0x84000060
.equ FFA_NOT_SUPPORTED,  0xFFFFFFFF   /* -1 as u32 */

.equ SP_STACK_TOP,       0x0e400000

_start:
    /* Set up stack */
    ldr     x0, =SP_STACK_TOP
    mov     sp, x0

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* Signal SPMC: SP init complete via FFA_MSG_WAIT */
    ldr     x0, =FFA_MSG_WAIT
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* After ERET from SPMC, x0-x7 contain the DIRECT_REQ.
     * Fall through to message loop. */

.Lmsg_loop:
    /* Check if this is a DIRECT_REQ_32 */
    ldr     x8, =FFA_DIRECT_REQ_32
    cmp     x0, x8
    b.ne    .Lunknown_msg

    /* Save source/dest from x1 for the response */
    /* x1 = (source << 16) | dest. Swap for response. */
    lsr     x8, x1, #16         /* x8 = source */
    and     x9, x1, #0xFFFF     /* x9 = dest (our SP ID) */
    orr     x1, x8, x9, lsl #16 /* x1 = (dest << 16) | source */

    /* Build DIRECT_RESP: echo x3-x7, x2=0 */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    /* x3-x7 already contain the payload — pass through */
    smc     #0

    /* After ERET from SPMC with next request */
    b       .Lmsg_loop

.Lunknown_msg:
    /* Unknown message — respond with FFA_MSG_WAIT to go idle */
    ldr     x0, =FFA_MSG_WAIT
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

/* ============ UART print subroutine ============ */
uart_print:
    ldr     x10, =UART_BASE
.Lprint_loop:
    ldrb    w11, [x0], #1
    cbz     w11, .Lprint_done
.Lprint_wait:
    ldr     w12, [x10, #UARTFR]
    tbnz    w12, #5, .Lprint_wait
    str     w11, [x10, #UARTDR]
    b       .Lprint_loop
.Lprint_done:
    ret

/* ============ String data ============ */
.section .rodata
str_banner:
    .asciz "[SP] Hello from S-EL1!\r\n"
```

**Step 2: Write the linker script**

Create `tfa/sp_hello/linker.ld`:

```ld
/* SP Hello linker script.
 * Loaded at 0x0e300000 in SEC_DRAM by TF-A BL2. */
ENTRY(_start)
SECTIONS
{
    . = 0x0e300000;
    .text : { *(.text.boot) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) }
}
```

**Step 3: Commit**

```bash
git add tfa/sp_hello/start.S tfa/sp_hello/linker.ld
git commit -m "feat(sel2): add sp_hello — minimal SP binary for S-EL1"
```

---

## Task 5: TF-A Configuration

**Files:**
- Create: `tfa/sp_hello/sp_manifest.dts`
- Modify: `tfa/sp_layout.json`
- Modify: `tfa/tb_fw_config.dts`

**Step 1: Write the SP manifest**

Create `tfa/sp_hello/sp_manifest.dts`:

```dts
/dts-v1/;

/ {
    compatible = "arm,ffa-manifest-1.0";
    ffa-version = <0x00010001>;  /* FF-A v1.1 */

    uuid = <0x12345678 0x12345678 0x12345678 0x12345678>;
    id = <0x8001>;

    execution-ctx-count = <1>;     /* Single execution context */
    exception-level = <2>;         /* S-EL1 */
    execution-state = <0>;         /* AArch64 */
    load-address = <0x0 0x0e300000>;
    entrypoint = <0x0 0x0e300000>;
    xlat-granule = <0>;            /* 4KB */
    messaging-method = <3>;        /* Direct request/response */
    managed-exit;                  /* SPMC manages preemption */
};
```

**Step 2: Update sp_layout.json**

Replace contents of `tfa/sp_layout.json`:

```json
{
    "SP1": {
        "image": "tfa/sp_hello/sp_hello.bin",
        "pm": "tfa/sp_hello/sp_manifest.dts",
        "owner": "Plat"
    }
}
```

**Step 3: Update tb_fw_config.dts**

Replace contents of `tfa/tb_fw_config.dts`:

```dts
/dts-v1/;
/ {
	secure-partitions {
		compatible = "arm,sp";

		sp1 {
			uuid = <0x12345678 0x12345678 0x12345678 0x12345678>;
			load-address = <0x0 0x0e300000>;
			owner = "Plat";
		};
	};
};
```

**Step 4: Commit**

```bash
git add tfa/sp_hello/sp_manifest.dts tfa/sp_layout.json tfa/tb_fw_config.dts
git commit -m "feat(sel2): add TF-A SP config (sp_layout.json, tb_fw_config, SP manifest)"
```

---

## Task 6: Exception Handler — sel2 SMC Passthrough

**Files:**
- Modify: `src/arch/aarch64/hypervisor/exception.rs`

**Step 1: Gate SMC handling for sel2**

In `handle_exception()`, find the `ExitReason::SmcCall` arm (around line 213). Add a `sel2` gate at the top so SP SMC traps exit to caller without being processed by the NS-EL2 handler:

```rust
ExitReason::SmcCall => {
    reset_exception_count();

    // In S-EL2 SPMC mode, SP SMC traps should exit to the caller
    // (rust_main_sel2 or spmc_handler) which handles them directly.
    #[cfg(feature = "sel2")]
    {
        context.pc += AARCH64_INSN_SIZE;
        return false;
    }

    #[cfg(not(feature = "sel2"))]
    {
        let should_continue = handle_smc(context);
        context.pc += AARCH64_INSN_SIZE;
        should_continue
    }
}
```

Also gate `ExitReason::WfiWfe` for sel2 — SP calling WFI should just return to caller:

```rust
ExitReason::WfiWfe => {
    reset_exception_count();

    #[cfg(feature = "sel2")]
    {
        context.pc += AARCH64_INSN_SIZE;
        return false;
    }

    #[cfg(not(feature = "sel2"))]
    {
        // ... existing WFI handling ...
    }
}
```

**Step 2: Build check**

```bash
make clean && make run          # NS tests still pass
cargo build --target aarch64-unknown-none --features sel2  # sel2 compiles
```

**Step 3: Commit**

```bash
git add src/arch/aarch64/hypervisor/exception.rs
git commit -m "feat(sel2): gate exception handler SMC/WFI for S-EL2 SP passthrough"
```

---

## Task 7: SP Boot Sequence in rust_main_sel2()

**Files:**
- Modify: `src/main.rs` (the `rust_main_sel2()` function, lines 258-315)

**Step 1: Add secure heap init and SP boot**

Between the GIC init (step 5, line 307) and the FFA_MSG_WAIT (step 6, line 310), insert:

```rust
    // 5.5. Initialize secure heap (for page table allocation)
    uart_puts_local(b"[SPMC] Initializing secure heap\n");
    unsafe {
        // Override heap with secure DRAM range
        hypervisor::mm::heap::init_at(
            hypervisor::platform::SECURE_HEAP_START,
            hypervisor::platform::SECURE_HEAP_SIZE,
        );
    }

    // 5.6. Build Secure Stage-2 for SP1
    uart_puts_local(b"[SPMC] Building Secure Stage-2 for SP1\n");
    let mapper = hypervisor::secure_stage2::build_sp_stage2(
        hypervisor::platform::SP1_LOAD_ADDR,
        hypervisor::platform::SP1_MEM_SIZE,
    )
    .expect("Failed to build SP Stage-2");
    let s2_config = hypervisor::secure_stage2::SecureStage2Config::new(mapper.l0_addr());
    s2_config.install();

    // Enable Secure Stage-2 by setting HCR_EL2.VM
    unsafe {
        let hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        core::arch::asm!(
            "msr hcr_el2, {}",
            "isb",
            in(reg) hcr | hypervisor::arch::aarch64::defs::HCR_VM,
        );
    }

    // 5.7. Create SP context and boot SP1
    uart_puts_local(b"[SPMC] Booting SP1 at 0x");
    hypervisor::uart_put_hex(hypervisor::platform::SP1_LOAD_ADDR);
    uart_puts_local(b"\n");
    let mut sp1 = hypervisor::sp_context::SpContext::new(
        hypervisor::platform::SP1_PARTITION_ID,
        hypervisor::platform::SP1_LOAD_ADDR,
        hypervisor::platform::SP1_STACK_TOP,
    );
    sp1.set_vsttbr(s2_config.vsttbr);

    // ERET to SP1 — SP runs, prints hello, calls FFA_MSG_WAIT, traps back
    {
        use hypervisor::arch::aarch64::enter_guest;
        let _exit = unsafe { enter_guest(sp1.vcpu_ctx_mut() as *mut _) };
    }

    // SP trapped back — verify it called FFA_MSG_WAIT
    let (x0, _, _, _, _, _, _, _) = sp1.get_args();
    if x0 == hypervisor::ffa::FFA_MSG_WAIT {
        uart_puts_local(b"[SPMC] SP1 booted, now Idle (FFA_MSG_WAIT received)\n");
        sp1.transition_to(hypervisor::sp_context::SpState::Idle)
            .expect("SP1 state transition failed");
    } else {
        uart_puts_local(b"[SPMC] WARNING: SP1 did not call FFA_MSG_WAIT, x0=0x");
        hypervisor::uart_put_hex(x0);
        uart_puts_local(b"\n");
    }

    // Store SP1 context globally for dispatch
    hypervisor::sp_context::register_sp(sp1);
```

**Step 2: Add `init_at()` to heap module**

Add to `src/mm/heap.rs`:

```rust
/// Initialize the global heap at a specific address.
/// Used by S-EL2 SPMC for secure DRAM heap.
pub unsafe fn init_at(start: u64, size: u64) {
    let alloc = super::BumpAllocator::new(start, size);
    *HEAP.allocator.get() = Some(alloc);
}
```

**Step 3: Add `l0_addr()` to DynamicIdentityMapper**

In `src/arch/aarch64/mm/mmu.rs`, add a public getter for the L0 table address:

```rust
impl DynamicIdentityMapper {
    /// Get the L0 table physical address (for VTTBR/VSTTBR).
    pub fn l0_addr(&self) -> u64 {
        self.l0_table
    }
    // ... existing methods ...
}
```

**Step 4: Add `register_sp()` and `get_sp_mut()` to sp_context.rs**

```rust
use core::cell::UnsafeCell;

struct SpStore {
    contexts: UnsafeCell<[Option<SpContext>; 4]>,
}

unsafe impl Sync for SpStore {}

static SP_STORE: SpStore = SpStore {
    contexts: UnsafeCell::new([None, None, None, None]),
};

/// Register a booted SP in the global store.
pub fn register_sp(sp: SpContext) {
    unsafe {
        let contexts = &mut *SP_STORE.contexts.get();
        for slot in contexts.iter_mut() {
            if slot.is_none() {
                *slot = Some(sp);
                return;
            }
        }
        panic!("No free SP slots");
    }
}

/// Look up an SP by partition ID (mutable, for dispatch).
pub fn get_sp_mut(sp_id: u16) -> Option<&'static mut SpContext> {
    unsafe {
        let contexts = &mut *SP_STORE.contexts.get();
        for slot in contexts.iter_mut() {
            if let Some(ref mut sp) = slot {
                if sp.sp_id() == sp_id {
                    return Some(sp);
                }
            }
        }
        None
    }
}

/// Check if a partition ID belongs to a registered SP.
pub fn is_registered_sp(sp_id: u16) -> bool {
    get_sp_mut(sp_id).is_some()
}
```

**Step 5: Build sel2**

```bash
cargo build --target aarch64-unknown-none --features sel2
```

**Step 6: Commit**

```bash
git add src/main.rs src/mm/heap.rs src/arch/aarch64/mm/mmu.rs src/sp_context.rs
git commit -m "feat(sel2): SP1 boot sequence — secure heap, Stage-2, ERET to S-EL1"
```

---

## Task 8: SPMC Dispatch — DIRECT_REQ to SP

**Files:**
- Modify: `src/spmc_handler.rs`

**Step 1: Change dispatch to support SP invocation**

Replace the `FFA_MSG_SEND_DIRECT_REQ_32` arm in `dispatch_ffa()`. The function can no longer be pure (it must call `enter_guest()` for SP dispatch), so split into two paths:

In `run_event_loop()`, change the dispatch:

```rust
#[cfg(feature = "sel2")]
pub fn run_event_loop(first_request: SmcResult8) -> ! {
    let mut request = first_request;
    loop {
        let response = dispatch_request(&request);
        request = crate::ffa::smc_forward::forward_smc8(
            response.x0, response.x1, response.x2, response.x3,
            response.x4, response.x5, response.x6, response.x7,
        );
    }
}

/// Dispatch an FF-A request. Handles both SPMC-local calls and SP invocation.
#[cfg(feature = "sel2")]
fn dispatch_request(req: &SmcResult8) -> SmcResult8 {
    // Check if DIRECT_REQ targets a registered SP
    if req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_32
        || req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_64
    {
        let dest = (req.x1 & 0xFFFF) as u16;
        if crate::sp_context::is_registered_sp(dest) {
            return dispatch_to_sp(req, dest);
        }
    }
    // Fall through to local SPMC handling
    dispatch_ffa(req)
}

/// Route a DIRECT_REQ to an SP: ERET to SP, wait for DIRECT_RESP, return it.
#[cfg(feature = "sel2")]
fn dispatch_to_sp(req: &SmcResult8, sp_id: u16) -> SmcResult8 {
    let sp = match crate::sp_context::get_sp_mut(sp_id) {
        Some(sp) => sp,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if sp.state() != crate::sp_context::SpState::Idle {
        return make_error(ffa::FFA_BUSY as u64);
    }

    // Set up SP registers with the DIRECT_REQ args
    sp.set_args(req.x0, req.x1, req.x2, req.x3, req.x4, req.x5, req.x6, req.x7);
    sp.transition_to(crate::sp_context::SpState::Running)
        .expect("SP transition to Running failed");

    // Install SP's Secure Stage-2 and ERET
    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(sp.vcpu_ctx_mut() as *mut _)
    };

    // SP trapped back — read its response from saved x0-x7
    sp.transition_to(crate::sp_context::SpState::Idle)
        .expect("SP transition to Idle failed");

    let (x0, x1, x2, x3, x4, x5, x6, x7) = sp.get_args();
    SmcResult8 { x0, x1, x2, x3, x4, x5, x6, x7 }
}
```

**Step 2: Add `new_from_vsttbr()` to SecureStage2Config**

In `src/secure_stage2.rs`:

```rust
impl SecureStage2Config {
    /// Create from a previously stored VSTTBR value (for reinstalling).
    pub fn new_from_vsttbr(vsttbr: u64) -> Self {
        let vstcr = VTCR_T0SZ_48BIT
            | VTCR_SL0_LEVEL0
            | VTCR_IRGN0_WB
            | VTCR_ORGN0_WB
            | VTCR_SH0_INNER
            | VTCR_TG0_4KB
            | VTCR_PS_48BIT;
        Self { vsttbr, vstcr }
    }
}
```

**Step 3: Update PARTITION_INFO_GET to report SP1**

In `dispatch_ffa()`, update the `FFA_PARTITION_INFO_GET` arm:

```rust
ffa::FFA_PARTITION_INFO_GET => {
    // Return count of registered SPs
    let count = if crate::sp_context::is_registered_sp(0x8001) { 1u64 } else { 0u64 };
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: count,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}
```

**Step 4: Build sel2**

```bash
cargo build --target aarch64-unknown-none --features sel2
```

**Step 5: Commit**

```bash
git add src/spmc_handler.rs src/secure_stage2.rs
git commit -m "feat(sel2): DIRECT_REQ dispatch to SP via enter_guest() + ERET"
```

---

## Task 9: Makefile Build Pipeline

**Files:**
- Modify: `Makefile`

**Step 1: Add SP build target and update build-tfa-spmc**

Add to Makefile:

```makefile
# SP Hello binary (S-EL1)
SP_HELLO_BIN := tfa/sp_hello/sp_hello.bin

build-sp-hello:
	@echo ">>> Building SP Hello (S-EL1)..."
	@mkdir -p tfa/sp_hello
	aarch64-linux-gnu-gcc -c tfa/sp_hello/start.S -o tfa/sp_hello/sp_hello.o -nostdlib
	aarch64-linux-gnu-ld -T tfa/sp_hello/linker.ld -o tfa/sp_hello/sp_hello.elf tfa/sp_hello/sp_hello.o
	aarch64-linux-gnu-objcopy -O binary tfa/sp_hello/sp_hello.elf $(SP_HELLO_BIN)
	@echo ">>> SP Hello binary: $(SP_HELLO_BIN)"
```

Update `build-tfa-spmc` target to depend on `build-sp-hello` and use the sp_layout.json with the SP binary.

Update `run-spmc` to use the new flash with SP loaded.

**Step 2: Commit**

```bash
git add Makefile
git commit -m "feat(sel2): add SP build pipeline and update Makefile"
```

---

## Task 10: BL33 Integration Test Updates

**Files:**
- Modify: `tfa/bl33_ffa_test/start.S`

**Step 1: Update Test 5 (PARTITION_INFO_GET) to expect count=1**

Change Test 5 to verify `x2 == 1` (one SP registered) instead of `x2 == 0`:

```asm
    /* x2 = count, should be 1 (SP1 registered) */
    cmp     x2, #1
    b.ne    .Lfail_5
```

**Step 2: Update Test 6 to verify SP echo**

Test 6 already sends DIRECT_REQ to 0x8001 with payload. Now that a real SP handles it (not the SPMC echo), verify the response includes the echoed payload from the SP:

The test should already pass since the SP echoes x3-x7 identically. Verify x3 is also echoed:

```asm
    /* Check echoed payload x3, x4, x5 */
    movz    x9, #0xAAAA
    cmp     x3, x9
    b.ne    .Lfail_6
    movz    x9, #0xBBBB
    cmp     x4, x9
    b.ne    .Lfail_6
    movz    x9, #0xCCCC
    cmp     x5, x9
    b.ne    .Lfail_6
```

**Step 3: Commit**

```bash
git add tfa/bl33_ffa_test/start.S
git commit -m "test(sel2): update BL33 integration tests for SP1 (count=1, echo verify)"
```

---

## Task 11: Build + Integration Test

**Step 1: Run NS unit tests**

```bash
make clean && make run
```

Expected: all 33+ test suites pass (including new test_sp_context, test_secure_stage2).

**Step 2: Build SP + TF-A + flash**

```bash
make build-sp-hello
make build-tfa-spmc
```

**Step 3: Run integration test**

```bash
make run-spmc
```

Expected output:
```
[SPMC] Initializing secure heap
[SPMC] Building Secure Stage-2 for SP1
[SPMC] Booting SP1 at 0x000000000e300000
[SP] Hello from S-EL1!
[SPMC] SP1 booted, now Idle (FFA_MSG_WAIT received)
[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT
...
BL33 FF-A Test Client:
  Test 1: FFA_VERSION .............. PASS
  Test 2: FFA_ID_GET ............... PASS
  Test 3: FFA_FEATURES(VERSION) .... PASS
  Test 4: FFA_FEATURES(0xDEAD) ..... PASS
  Test 5: PARTITION_INFO_GET ........ PASS
  Test 6: DIRECT_REQ echo .......... PASS
```

**Step 4: If TF-A FIP doesn't rebuild (cached)**

Delete cached FIP:
```bash
docker run --rm -v tfa-spmc-build-cache:/cache debian:bookworm-slim \
    rm -f /cache/build/qemu/debug/fip.bin
make build-tfa-spmc
```

---

## Task 12: Update Documentation + Final Commit

**Files:**
- Modify: `CLAUDE.md` — add test_sp_context and test_secure_stage2 to test table, update assertion counts, update roadmap
- Modify: `DEVELOPMENT_PLAN.md` — mark Sprint 4.4 Phase B complete
- Modify: `MEMORY.md` — add Phase B completion notes

**Step 1: Update docs**

Update test table with new suites, increment total assertion count, add SpContext and SecureStage2 to Core Abstractions table, update Sprint 4.4 status.

**Step 2: Commit**

```bash
git add CLAUDE.md DEVELOPMENT_PLAN.md
git commit -m "docs: update CLAUDE.md and DEVELOPMENT_PLAN.md for Sprint 4.4 Phase B"
```

---

## Verification Checklist

1. `make clean && make run` → all test suites pass (new: test_sp_context 16 assertions, test_secure_stage2 3 assertions)
2. `cargo build --target aarch64-unknown-none --features sel2` → compiles clean
3. `make build-sp-hello` → produces `tfa/sp_hello/sp_hello.bin`
4. `make build-tfa-spmc && make run-spmc` → SP prints "[SP] Hello from S-EL1!", BL33 reports 6/6 PASS
5. DIRECT_REQ to SP1 (0x8001) → SP echoes x3-x7 → BL33 verifies

# Sprint 4.4 Phase A: SPMC Event Loop + FF-A Stub Responses

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the SPMC idle WFI loop with an event loop that processes FF-A requests from Normal World via SPMD, enabling NWd → SPMC FF-A communication.

**Architecture:** After FFA_MSG_WAIT, SPMD returns the first NWd FF-A request in x0-x7. The SPMC event loop dispatches the request, computes a response, and sends it back via another SMC — which simultaneously delivers the next request. A BL33 test client exercises the full path.

**Tech Stack:** Rust (no_std), ARM64 inline assembly (SMC), QEMU virt secure=on, TF-A v2.12.0 (SPD=spmd)

---

### Task 1: Add `SmcResult8` and `forward_smc8()` to `smc_forward.rs`

**Files:**
- Modify: `src/ffa/smc_forward.rs` (add struct + function after existing `SmcResult`/`forward_smc()`)

**Step 1: Add `SmcResult8` struct**

After the existing `SmcResult` struct (line 8-13), add:

```rust
/// Full 8-register SMC result for SPMC event loop.
/// SPMD returns x0-x7 with the next FF-A request.
#[derive(Debug, Clone, Copy)]
pub struct SmcResult8 {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
}
```

**Step 2: Add `forward_smc8()` function**

After the existing `forward_smc()` function (after line 74), add:

```rust
/// Forward SMC and capture all 8 return registers (x0-x7).
/// Used by the SPMC event loop where x4-x7 carry FF-A message payloads.
#[inline(never)]
pub fn forward_smc8(
    x0: u64, x1: u64, x2: u64, x3: u64,
    x4: u64, x5: u64, x6: u64, x7: u64,
) -> SmcResult8 {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    let r3: u64;
    let r4: u64;
    let r5: u64;
    let r6: u64;
    let r7: u64;
    unsafe {
        core::arch::asm!(
            "smc #0",
            inout("x0") x0 => r0,
            inout("x1") x1 => r1,
            inout("x2") x2 => r2,
            inout("x3") x3 => r3,
            inout("x4") x4 => r4,
            inout("x5") x5 => r5,
            inout("x6") x6 => r6,
            inout("x7") x7 => r7,
            lateout("x8") _,
            lateout("x9") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
            options(nomem, nostack),
        );
    }
    SmcResult8 { x0: r0, x1: r1, x2: r2, x3: r3, x4: r4, x5: r5, x6: r6, x7: r7 }
}
```

**Step 3: Verify compilation**

Run: `cargo build --target aarch64-unknown-none --features sel2`
Expected: Builds clean (no warnings about unused — they'll be used in Task 2).

**Step 4: Verify regression**

Run: `make clean && make run` (pipe to head -100 or watch for "All tests passed")
Expected: All 30 test suites pass (~193 assertions). `SmcResult8`/`forward_smc8()` are unused in default features but should compile.

**Step 5: Commit**

```bash
git add src/ffa/smc_forward.rs
git commit -m "feat: add SmcResult8 and forward_smc8() for SPMC event loop"
```

---

### Task 2: Create `src/spmc_handler.rs` — Event Loop + FF-A Dispatch

**Files:**
- Create: `src/spmc_handler.rs`
- Modify: `src/lib.rs` (add `pub mod spmc_handler;`)

**Step 1: Create `src/spmc_handler.rs` with event loop and dispatch**

```rust
//! SPMC event loop — processes FF-A requests from Normal World via SPMD.
//!
//! After FFA_MSG_WAIT, SPMD returns the next NWd FF-A request in x0-x7.
//! The event loop dispatches each request, computes a response, and sends
//! it back via SMC — which simultaneously delivers the next request.

use crate::ffa::{self, smc_forward::SmcResult8};

/// Run the SPMC event loop. Called from `rust_main_sel2()` after init.
/// Does not return.
#[cfg(feature = "sel2")]
pub fn run_event_loop(first_request: SmcResult8) -> ! {
    let mut req = first_request;
    crate::uart_puts(b"[SPMC] Entering event loop\n");

    loop {
        let resp = dispatch_ffa(&req);
        // Send response to SPMD and receive next request
        req = crate::ffa::smc_forward::forward_smc8(
            resp.x0, resp.x1, resp.x2, resp.x3,
            resp.x4, resp.x5, resp.x6, resp.x7,
        );
    }
}

/// Dispatch an FF-A request and return the response.
/// Pure logic — no SMC calls, testable without hardware.
pub fn dispatch_ffa(req: &SmcResult8) -> SmcResult8 {
    let fid = req.x0;
    match fid {
        ffa::FFA_VERSION => handle_version(req),
        ffa::FFA_ID_GET => handle_id_get(req),
        ffa::FFA_SPM_ID_GET => handle_spm_id_get(req),
        ffa::FFA_FEATURES => handle_features(req),
        ffa::FFA_PARTITION_INFO_GET => handle_partition_info_get(req),
        ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
            handle_direct_req(req)
        }
        _ => {
            // Log unknown function ID
            crate::uart_puts(b"[SPMC] Unknown FFA call: 0x");
            crate::uart_put_hex(fid);
            crate::uart_puts(b"\n");
            make_error(ffa::FFA_NOT_SUPPORTED as u64)
        }
    }
}

// --- FF-A Handlers ---

/// FFA_VERSION: return v1.1 (0x00010001)
fn handle_version(_req: &SmcResult8) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_VERSION_1_1 as u64,
        x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}

/// FFA_ID_GET: return SPMC ID (0x8000)
fn handle_id_get(_req: &SmcResult8) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32 as u64,
        x1: 0,
        x2: ffa::FFA_SPMC_ID as u64,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}

/// FFA_SPM_ID_GET: return SPMC ID (0x8000)
fn handle_spm_id_get(_req: &SmcResult8) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32 as u64,
        x1: 0,
        x2: ffa::FFA_SPMC_ID as u64,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}

/// FFA_FEATURES: report support for queried function ID.
/// x1 = function_id to query.
fn handle_features(req: &SmcResult8) -> SmcResult8 {
    let queried_fid = req.x1;
    let supported = matches!(
        queried_fid,
        ffa::FFA_VERSION as u64
            | ffa::FFA_ID_GET as u64
            | ffa::FFA_SPM_ID_GET as u64
            | ffa::FFA_FEATURES as u64
            | ffa::FFA_PARTITION_INFO_GET as u64
            | ffa::FFA_MSG_SEND_DIRECT_REQ_32 as u64
            | ffa::FFA_MSG_SEND_DIRECT_REQ_64 as u64
            | ffa::FFA_MSG_WAIT as u64
    );
    if supported {
        SmcResult8 {
            x0: ffa::FFA_SUCCESS_32 as u64,
            x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        }
    } else {
        make_error(ffa::FFA_NOT_SUPPORTED as u64)
    }
}

/// FFA_PARTITION_INFO_GET: return count=0 (no SPs loaded yet).
/// In Phase B this will return actual SP descriptors.
fn handle_partition_info_get(_req: &SmcResult8) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32 as u64,
        x1: 0,
        x2: 0, // count = 0 (no SPs)
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}

/// FFA_MSG_SEND_DIRECT_REQ: echo x4-x7 back as DIRECT_RESP.
/// req.x1 bits [31:16] = source, bits [15:0] = destination.
/// In Phase C this will route to actual SPs.
fn handle_direct_req(req: &SmcResult8) -> SmcResult8 {
    let source = (req.x1 >> 16) & 0xFFFF;
    let dest = req.x1 & 0xFFFF;
    // Swap source/dest in response
    let resp_ids = (dest << 16) | source;

    // Use DIRECT_RESP_32 or _64 matching the request
    let resp_fid = if req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_32 as u64 {
        ffa::FFA_MSG_SEND_DIRECT_RESP_32 as u64
    } else {
        ffa::FFA_MSG_SEND_DIRECT_RESP_64 as u64
    };

    SmcResult8 {
        x0: resp_fid,
        x1: resp_ids,
        x2: 0,
        x3: req.x3, // echo payload
        x4: req.x4,
        x5: req.x5,
        x6: req.x6,
        x7: req.x7,
    }
}

/// Helper: construct FFA_ERROR response.
fn make_error(error_code: u64) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_ERROR as u64,
        x1: 0,
        x2: error_code,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    }
}
```

**Step 2: Register module in `src/lib.rs`**

Add after the existing `pub mod manifest;` line (around line 10):

```rust
pub mod spmc_handler;
```

**Step 3: Add missing FF-A constants to `src/ffa/mod.rs`**

Check if `FFA_MSG_SEND_DIRECT_REQ_64` and `FFA_MSG_SEND_DIRECT_RESP_32/64` constants exist. If not, add them after the existing constants block. The needed constants:

```rust
pub const FFA_MSG_SEND_DIRECT_REQ_64: u32 = 0xC400006F;
pub const FFA_MSG_SEND_DIRECT_RESP_32: u32 = 0x84000070;
pub const FFA_MSG_SEND_DIRECT_RESP_64: u32 = 0xC4000070;
```

**Step 4: Verify compilation**

Run: `cargo build --target aarch64-unknown-none --features sel2`
Expected: Compiles clean.

Run: `cargo build --target aarch64-unknown-none` (default features)
Expected: Compiles clean (module exists but `run_event_loop` gated by `sel2`).

**Step 5: Commit**

```bash
git add src/spmc_handler.rs src/lib.rs src/ffa/mod.rs
git commit -m "feat: add SPMC event loop with FF-A dispatch (VERSION/ID_GET/FEATURES/DIRECT_REQ)"
```

---

### Task 3: Add Unit Tests for `dispatch_ffa()`

**Files:**
- Create: `tests/test_spmc_handler.rs`
- Modify: `src/main.rs` (wire test into test harness)

**Step 1: Create `tests/test_spmc_handler.rs`**

The `dispatch_ffa()` function is pure logic (no SMC, no hardware) — it takes `SmcResult8` input and returns `SmcResult8` output. We test it in the existing bare-metal test harness.

```rust
//! Unit tests for SPMC event loop dispatch logic.

use hypervisor::ffa::{self, smc_forward::SmcResult8};
use hypervisor::spmc_handler::dispatch_ffa;

fn zero_req(fid: u64) -> SmcResult8 {
    SmcResult8 { x0: fid, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0 }
}

pub fn run_tests() {
    crate::uart_puts(b"  test_spmc_handler...\n");
    let mut pass = 0u32;

    // Test 1: FFA_VERSION returns v1.1
    let resp = dispatch_ffa(&zero_req(ffa::FFA_VERSION as u64));
    assert_eq!(resp.x0, ffa::FFA_VERSION_1_1 as u64);
    pass += 1;

    // Test 2: FFA_ID_GET returns SUCCESS + SPMC ID
    let resp = dispatch_ffa(&zero_req(ffa::FFA_ID_GET as u64));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32 as u64);
    assert_eq!(resp.x2, ffa::FFA_SPMC_ID as u64);
    pass += 2;

    // Test 3: FFA_SPM_ID_GET returns SUCCESS + SPMC ID
    let resp = dispatch_ffa(&zero_req(ffa::FFA_SPM_ID_GET as u64));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32 as u64);
    assert_eq!(resp.x2, ffa::FFA_SPMC_ID as u64);
    pass += 2;

    // Test 4: FFA_FEATURES with supported function → SUCCESS
    let mut req = zero_req(ffa::FFA_FEATURES as u64);
    req.x1 = ffa::FFA_VERSION as u64;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32 as u64);
    pass += 1;

    // Test 5: FFA_FEATURES with unsupported function → NOT_SUPPORTED
    let mut req = zero_req(ffa::FFA_FEATURES as u64);
    req.x1 = 0xDEAD; // unsupported
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR as u64);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as i32 as u64);
    pass += 2;

    // Test 6: FFA_PARTITION_INFO_GET returns count=0
    let resp = dispatch_ffa(&zero_req(ffa::FFA_PARTITION_INFO_GET as u64));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32 as u64);
    assert_eq!(resp.x2, 0); // no SPs
    pass += 2;

    // Test 7: DIRECT_REQ echoes payload, swaps source/dest
    let req = SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_REQ_32 as u64,
        x1: (0x0001 << 16) | 0x8001, // source=1, dest=0x8001
        x2: 0,
        x3: 0xAAAA,
        x4: 0xBBBB,
        x5: 0xCCCC,
        x6: 0xDDDD,
        x7: 0xEEEE,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_32 as u64);
    assert_eq!(resp.x1, (0x8001 << 16) | 0x0001); // swapped
    assert_eq!(resp.x3, 0xAAAA);
    assert_eq!(resp.x4, 0xBBBB);
    assert_eq!(resp.x5, 0xCCCC);
    assert_eq!(resp.x6, 0xDDDD);
    assert_eq!(resp.x7, 0xEEEE);
    pass += 7;

    // Test 8: Unknown function → FFA_ERROR(NOT_SUPPORTED)
    let resp = dispatch_ffa(&zero_req(0xDEADBEEF));
    assert_eq!(resp.x0, ffa::FFA_ERROR as u64);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as i32 as u64);
    pass += 2;

    crate::uart_puts(b"    ");
    crate::print_u32(pass);
    crate::uart_puts(b" assertions passed\n");
}
```

**Step 2: Wire test into `src/main.rs`**

In `rust_main()` (the test harness at ~line 22), add the test call alongside the existing test calls. Find the section where tests are called (look for `test_ffa::run_tests()` or similar) and add:

```rust
tests::test_spmc_handler::run_tests();
```

Also add the module import in the `tests` module at the top of main.rs:

```rust
mod test_spmc_handler;
```

**Step 3: Run tests**

Run: `make clean && make run` (timeout after ~10s — tests are fast)
Expected: `test_spmc_handler... 19 assertions passed` appears in output, and total assertion count increases.

**Step 4: Commit**

```bash
git add tests/test_spmc_handler.rs src/main.rs
git commit -m "test: add 19 unit tests for SPMC dispatch (VERSION/ID_GET/FEATURES/DIRECT_REQ)"
```

---

### Task 4: Update `signal_spmc_ready()` to Return `SmcResult8`

**Files:**
- Modify: `src/manifest.rs` (change `signal_spmc_ready()` return type)

**Step 1: Update `signal_spmc_ready()` to return the first NWd request**

Change the function at line 82-85 from:

```rust
#[cfg(feature = "sel2")]
pub fn signal_spmc_ready() {
    smc_forward::forward_smc(0x8400_006B, 0, 0, 0, 0, 0, 0, 0);
}
```

To:

```rust
/// Signal SPMD that SPMC init is complete via FFA_MSG_WAIT.
/// Returns the first FF-A request from Normal World.
#[cfg(feature = "sel2")]
pub fn signal_spmc_ready() -> crate::ffa::smc_forward::SmcResult8 {
    crate::ffa::smc_forward::forward_smc8(0x8400_006B, 0, 0, 0, 0, 0, 0, 0)
}
```

**Step 2: Verify compilation**

Run: `cargo build --target aarch64-unknown-none --features sel2`
Expected: Compiles (caller in `rust_main_sel2()` currently ignores return value — that's fine, we fix it in Task 5).

**Step 3: Commit**

```bash
git add src/manifest.rs
git commit -m "refactor: signal_spmc_ready() returns SmcResult8 (first NWd request)"
```

---

### Task 5: Wire Event Loop into `rust_main_sel2()`

**Files:**
- Modify: `src/main.rs` (`rust_main_sel2()` function, around lines 255-317)

**Step 1: Replace idle WFI loop with event loop call**

In `rust_main_sel2()`, find the section after `signal_spmc_ready()` (around line 308-316):

```rust
    // 6. Signal SPMD: init complete
    uart_puts_local(b"[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT\n");
    hypervisor::manifest::signal_spmc_ready();

    // 7. SPMD returned — loop waiting for world switch
    uart_puts_local(b"[SPMC] FFA_MSG_WAIT returned, entering idle loop\n");
    loop {
        unsafe { core::arch::asm!("wfi"); }
    }
```

Replace with:

```rust
    // 6. Signal SPMD: init complete, receive first NWd request
    uart_puts_local(b"[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT\n");
    let first_req = hypervisor::manifest::signal_spmc_ready();

    // 7. Enter SPMC event loop (does not return)
    hypervisor::spmc_handler::run_event_loop(first_req);
```

**Step 2: Verify compilation**

Run: `cargo build --target aarch64-unknown-none --features sel2`
Expected: Compiles clean.

**Step 3: Verify regression**

Run: `make clean && make run`
Expected: All tests pass (default features don't touch sel2 code).

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire SPMC event loop into rust_main_sel2()"
```

---

### Task 6: Create BL33 FF-A Test Client

**Files:**
- Create: `tfa/bl33_ffa_test/start.S` (assembly test client)
- Create: `tfa/bl33_ffa_test/linker.ld`

The BL33 test client runs at NS-EL2 (loaded by TF-A as BL33). It sends FF-A SMC calls through SPMD to our SPMC and verifies responses. Each test prints PASS/FAIL.

**Step 1: Create `tfa/bl33_ffa_test/linker.ld`**

```ld
ENTRY(_start)
SECTIONS
{
    . = 0x40000000;
    .text : { KEEP(*(.text.boot)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.comment) *(.eh_frame) }
}
```

**Step 2: Create `tfa/bl33_ffa_test/start.S`**

This is an assembly-only test client. It tests:
1. FFA_VERSION → expects 0x10001
2. FFA_ID_GET → expects SUCCESS, x2=0 (NWd's own ID)
3. FFA_FEATURES(FFA_VERSION) → expects SUCCESS
4. FFA_FEATURES(0xDEAD) → expects ERROR
5. FFA_PARTITION_INFO_GET → expects SUCCESS, x2=0
6. FFA_MSG_SEND_DIRECT_REQ → expects DIRECT_RESP with echoed payload

```asm
// BL33 FF-A Test Client — sends FF-A SMC calls to SPMC via SPMD
// Runs at NS-EL2. Prints PASS/FAIL for each test.

.section .text.boot
.global _start

// PL011 UART base
.equ UART_BASE, 0x09000000

// FF-A function IDs
.equ FFA_ERROR,           0x84000060
.equ FFA_SUCCESS_32,      0x84000061
.equ FFA_VERSION,         0x84000063
.equ FFA_FEATURES,        0x84000064
.equ FFA_PARTITION_INFO,  0x84000068
.equ FFA_ID_GET,          0x84000069
.equ FFA_DIRECT_REQ_32,   0x8400006F
.equ FFA_DIRECT_RESP_32,  0x84000070
.equ FFA_VERSION_1_1,     0x00010001

_start:
    // Save link registers
    mov     x20, x0            // HW_CONFIG DTB
    mov     x21, x4            // Core ID

    // Print banner
    adr     x0, str_banner
    bl      uart_print

    // ============ Test 1: FFA_VERSION ============
    adr     x0, str_t1
    bl      uart_print

    mov     x0, #FFA_VERSION
    mov     x1, #FFA_VERSION_1_1   // input version (our version)
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    // x0 should be FFA_VERSION_1_1 (0x10001)
    mov     x9, #FFA_VERSION_1_1
    cmp     x0, x9
    b.ne    fail_1
    adr     x0, str_pass
    bl      uart_print
    b       test_2
fail_1:
    adr     x0, str_fail
    bl      uart_print

    // ============ Test 2: FFA_ID_GET ============
test_2:
    adr     x0, str_t2
    bl      uart_print

    mov     x0, #FFA_ID_GET
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    // x0 should be FFA_SUCCESS_32, x2 should be our ID
    mov     x9, #FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    fail_2
    // NWd ID is typically 0 (assigned by SPMD)
    // Just check SUCCESS for now
    adr     x0, str_pass
    bl      uart_print
    b       test_3
fail_2:
    adr     x0, str_fail
    bl      uart_print

    // ============ Test 3: FFA_FEATURES(FFA_VERSION) ============
test_3:
    adr     x0, str_t3
    bl      uart_print

    mov     x0, #FFA_FEATURES
    mov     x1, #FFA_VERSION       // query FFA_VERSION support
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    mov     x9, #FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    fail_3
    adr     x0, str_pass
    bl      uart_print
    b       test_4
fail_3:
    adr     x0, str_fail
    bl      uart_print

    // ============ Test 4: FFA_FEATURES(0xDEAD) → NOT_SUPPORTED ============
test_4:
    adr     x0, str_t4
    bl      uart_print

    mov     x0, #FFA_FEATURES
    movz    x1, #0xDEAD            // unsupported function
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    mov     x9, #FFA_ERROR
    cmp     x0, x9
    b.ne    fail_4
    adr     x0, str_pass
    bl      uart_print
    b       test_5
fail_4:
    adr     x0, str_fail
    bl      uart_print

    // ============ Test 5: FFA_PARTITION_INFO_GET ============
test_5:
    adr     x0, str_t5
    bl      uart_print

    mov     x0, #FFA_PARTITION_INFO
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    mov     x9, #FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    fail_5
    // x2 = count, should be 0
    cbnz    x2, fail_5
    adr     x0, str_pass
    bl      uart_print
    b       test_6
fail_5:
    adr     x0, str_fail
    bl      uart_print

    // ============ Test 6: FFA_MSG_SEND_DIRECT_REQ ============
test_6:
    adr     x0, str_t6
    bl      uart_print

    mov     x0, #FFA_DIRECT_REQ_32
    // x1: source=0x0001 (NWd), dest=0x8001 (SP1)
    movz    x1, #0x8001
    movk    x1, #0x0001, lsl #16
    mov     x2, xzr
    movz    x3, #0xAAAA            // payload
    movz    x4, #0xBBBB
    movz    x5, #0xCCCC
    movz    x6, #0xDDDD
    movz    x7, #0xEEEE
    smc     #0

    // x0 should be FFA_DIRECT_RESP_32
    mov     x9, #FFA_DIRECT_RESP_32
    cmp     x0, x9
    b.ne    fail_6
    // Check echoed payload
    movz    x9, #0xBBBB
    cmp     x4, x9
    b.ne    fail_6
    movz    x9, #0xCCCC
    cmp     x5, x9
    b.ne    fail_6
    adr     x0, str_pass
    bl      uart_print
    b       done
fail_6:
    adr     x0, str_fail
    bl      uart_print

done:
    adr     x0, str_done
    bl      uart_print

halt:
    wfe
    b       halt

// ============ UART print subroutine ============
// x0 = pointer to null-terminated string
uart_print:
    ldr     x10, =UART_BASE
.Lprint_loop:
    ldrb    w11, [x0], #1
    cbz     w11, .Lprint_done
.Lprint_wait:
    ldr     w12, [x10, #0x18]      // UARTFR
    tbnz    w12, #5, .Lprint_wait  // wait if TXFF
    str     w11, [x10]             // UARTDR
    b       .Lprint_loop
.Lprint_done:
    ret

// ============ String data ============
.section .rodata
str_banner: .asciz "\r\n========================================\r\n  BL33 FF-A Test Client (NS-EL2)\r\n========================================\r\n\r\n"
str_t1:     .asciz "  Test 1: FFA_VERSION .............. "
str_t2:     .asciz "  Test 2: FFA_ID_GET ............... "
str_t3:     .asciz "  Test 3: FFA_FEATURES(VERSION) .... "
str_t4:     .asciz "  Test 4: FFA_FEATURES(0xDEAD) ..... "
str_t5:     .asciz "  Test 5: PARTITION_INFO_GET ........ "
str_t6:     .asciz "  Test 6: DIRECT_REQ echo .......... "
str_pass:   .asciz "PASS\r\n"
str_fail:   .asciz "FAIL\r\n"
str_done:   .asciz "\r\n  All tests complete.\r\n"
```

**Step 3: Verify assembly compiles**

Run:
```bash
aarch64-linux-gnu-as -o /tmp/bl33_test.o tfa/bl33_ffa_test/start.S && \
aarch64-linux-gnu-ld -T tfa/bl33_ffa_test/linker.ld -o /tmp/bl33_test.elf /tmp/bl33_test.o && \
aarch64-linux-gnu-objcopy -O binary /tmp/bl33_test.elf /tmp/bl33_test.bin && \
echo "OK: $(stat -c%s /tmp/bl33_test.bin) bytes"
```
Expected: Assembles and links cleanly. Binary should be < 4KB.

**Step 4: Commit**

```bash
git add tfa/bl33_ffa_test/
git commit -m "feat: add BL33 FF-A test client (6 tests: VERSION/ID_GET/FEATURES/PARTITION_INFO/DIRECT_REQ)"
```

---

### Task 7: Update Makefile for BL33 Test Client + Build Pipeline

**Files:**
- Modify: `Makefile` (update `build-tfa-spmc` and `run-spmc` targets)

**Step 1: Add BL33 test client build**

Find the `build-bl32-bl33` target in the Makefile. We need a separate target to build the FF-A test client as BL33 (instead of the trivial hello). Add a new target:

```makefile
# Build BL33 FF-A test client
build-bl33-ffa-test:
	@echo "Building BL33 FF-A test client..."
	aarch64-linux-gnu-as -o $(BUILD_DIR)/bl33_ffa_test.o tfa/bl33_ffa_test/start.S
	aarch64-linux-gnu-ld -T tfa/bl33_ffa_test/linker.ld -o $(BUILD_DIR)/bl33_ffa_test.elf $(BUILD_DIR)/bl33_ffa_test.o
	aarch64-linux-gnu-objcopy -O binary $(BUILD_DIR)/bl33_ffa_test.elf tfa/bl33_ffa_test.bin
	@echo "BL33 test client: tfa/bl33_ffa_test.bin"
```

**Step 2: Update `build-tfa-spmc` to use BL33 test client**

Modify the `build-tfa-spmc` target to depend on `build-bl33-ffa-test` (instead of `build-bl32-bl33`) and copy the test client binary as `bl33.bin`. The key change is:
1. The SPMC binary (`build-spmc`) → `tfa/bl32.bin`
2. The BL33 test client (`build-bl33-ffa-test`) → `tfa/bl33.bin`

Update the target's dependency and add a copy step:

```makefile
build-tfa-spmc: build-spmc build-bl33-ffa-test
```

And ensure the Docker run copies both `bl32.bin` (SPMC) and `bl33.bin` (test client) before running `build-tfa.sh`.

**Step 3: Add `tfa/bl33_ffa_test.bin` to `.gitignore`**

Add to `.gitignore`:
```
tfa/bl33_ffa_test.bin
```

**Step 4: Verify build pipeline**

Run: `make build-spmc` (should build SPMC binary)
Run: `make build-bl33-ffa-test` (should build test client binary)
Expected: Both produce binaries without errors.

**Step 5: Commit**

```bash
git add Makefile .gitignore
git commit -m "feat: add BL33 FF-A test client build + update build-tfa-spmc pipeline"
```

---

### Task 8: Build and Run Integration Test

**Files:**
- No new files — this is the build + verification step.

**Step 1: Build full TF-A flash image with SPMC + test client**

Run: `make build-tfa-spmc`
Expected: Docker builds TF-A with real SPMC (bl32.bin) and FF-A test client (bl33.bin). Produces `tfa/flash-spmc.bin`.

**Step 2: Run integration test**

Run: `make run-spmc` (use timeout of ~30s — TCG is slow)
Expected output should contain:

```
...TF-A banner...
[SPMC] Parsing manifest at 0x...
[SPMC] spmc_id=0x8000 version=1.1
[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT
[SPMC] Entering event loop

========================================
  BL33 FF-A Test Client (NS-EL2)
========================================

  Test 1: FFA_VERSION .............. PASS
  Test 2: FFA_ID_GET ............... PASS
  Test 3: FFA_FEATURES(VERSION) .... PASS
  Test 4: FFA_FEATURES(0xDEAD) ..... PASS
  Test 5: PARTITION_INFO_GET ........ PASS
  Test 6: DIRECT_REQ echo .......... PASS

  All tests complete.
```

**Step 3: Verify regression — unit tests**

Run: `make clean && make run`
Expected: All 30+ test suites pass, including new `test_spmc_handler` (19 assertions).

**Step 4: Verify regression — TF-A Linux boot**

Run: `make run-tfa-linux` (if flash.bin exists)
Expected: BL33 hypervisor boots Linux to BusyBox shell (Sprint 4.2 regression).

**Step 5: Final commit (if any fixups needed)**

If all passes, tag the milestone:

```bash
git add -A
git commit -m "feat: Sprint 4.4 Phase A — SPMC event loop + FF-A responses (6 integration tests, 19 unit tests)"
```

---

### Task 9: Update Documentation

**Files:**
- Modify: `CLAUDE.md` (add SPMC event loop details)
- Modify: `DEVELOPMENT_PLAN.md` (mark Sprint 4.4 Phase A complete)

**Step 1: Update CLAUDE.md**

Add to the Core Abstractions table:

```
| `SpMcHandler` | `src/spmc_handler.rs` | SPMC event loop: dispatch_ffa(), FF-A VERSION/ID_GET/FEATURES/DIRECT_REQ |
```

Add to the Build Commands section:
- Update `make run-spmc` description to mention FF-A test client

Add to the S-EL2 SPMC Boot section:
- Document the event loop pattern: `smc(response)` → receive next request
- Document BL33 test client and its 6 integration tests

**Step 2: Update DEVELOPMENT_PLAN.md**

- Mark Sprint 4.4 Phase A as complete
- Update M4 progress percentage
- Add Phase A details (SmcResult8, event loop, BL33 test client, 6 tests)

**Step 3: Commit**

```bash
git add CLAUDE.md DEVELOPMENT_PLAN.md
git commit -m "docs: update CLAUDE.md and DEVELOPMENT_PLAN.md for Sprint 4.4 Phase A"
```

---

## Summary

| Task | Description | Tests Added | Commit |
|------|-------------|-------------|--------|
| 1 | `SmcResult8` + `forward_smc8()` | — | `feat: add SmcResult8...` |
| 2 | `spmc_handler.rs` event loop + dispatch | — | `feat: add SPMC event loop...` |
| 3 | Unit tests for `dispatch_ffa()` | 19 assertions | `test: add 19 unit tests...` |
| 4 | `signal_spmc_ready()` returns `SmcResult8` | — | `refactor: signal_spmc_ready()...` |
| 5 | Wire event loop into `rust_main_sel2()` | — | `feat: wire SPMC event loop...` |
| 6 | BL33 FF-A test client assembly | 6 integration tests | `feat: add BL33 FF-A test client...` |
| 7 | Makefile build pipeline | — | `feat: add BL33 test client build...` |
| 8 | Build + run integration test | verify all pass | `feat: Sprint 4.4 Phase A...` |
| 9 | Update docs | — | `docs: update for Sprint 4.4 Phase A` |

**Total new tests**: 19 unit assertions + 6 BL33 integration tests = 25 new test points.

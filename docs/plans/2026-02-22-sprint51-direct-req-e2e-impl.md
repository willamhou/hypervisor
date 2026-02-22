# Sprint 5.1: DIRECT_REQ End-to-End Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** NS proxy forwards FFA_MSG_SEND_DIRECT_REQ through SPMD to our real SPMC, which dispatches to SP1 and returns the response — completing the first end-to-end FF-A call path.

**Architecture:** Add `tfa_boot` feature flag that sets `SPMC_PRESENT=true` at compile time. When set, `handle_msg_send_direct_req()` forwards SP-destined calls via `forward_smc8()` (8-register return) instead of stub echo. SP1 modifies x4 (adds 0x1000) to prove the call went through the real SP, not the stub.

**Tech Stack:** Rust no_std (aarch64), ARM64 assembly (BL33 test client + SP), QEMU virt secure=on, TF-A SPMD

---

### Task 1: Add `tfa_boot` feature flag

**Files:**
- Modify: `Cargo.toml:22-28`

**Step 1: Add the feature flag**

In `Cargo.toml`, add `tfa_boot` to the `[features]` section. It implies `linux_guest` because TF-A boot mode boots a Linux guest through our NS-EL2 hypervisor.

```toml
[features]
default = []
guest = []
linux_guest = []
multi_pcpu = ["linux_guest"]
multi_vm = ["linux_guest"]
sel2 = []
tfa_boot = ["linux_guest"]
```

**Step 2: Verify build compiles**

Run: `cargo build --target aarch64-unknown-none --features tfa_boot`
Expected: Build succeeds (tfa_boot implies linux_guest, which should compile fine)

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "feat: add tfa_boot feature flag (implies linux_guest)"
```

---

### Task 2: Set SPMC_PRESENT=true when `tfa_boot` enabled

**Files:**
- Modify: `src/ffa/proxy.rs:17-25`

**Step 1: Write the unit test**

In `tests/test_ffa.rs`, we can't directly test `SPMC_PRESENT` (it's private). Instead, we'll verify the behavior change in Task 4. For now, the change is small enough to verify by code review.

**Step 2: Modify `init()` to set SPMC_PRESENT under tfa_boot**

In `src/ffa/proxy.rs`, update the `init()` function:

```rust
/// Initialize FF-A proxy. Probes EL3 for a real SPMC.
///
/// Called once at boot before guest entry.
pub fn init() {
    // When booted through TF-A (tfa_boot feature), SPMD+SPMC are present
    // by construction — no runtime probing needed.
    #[cfg(feature = "tfa_boot")]
    {
        SPMC_PRESENT.store(true, Ordering::Relaxed);
        crate::uart_puts(b"[FFA] TF-A boot: SPMC present (build-time)\n");
        return;
    }

    #[cfg(not(feature = "tfa_boot"))]
    {
        if smc_forward::probe_spmc() {
            SPMC_PRESENT.store(true, Ordering::Relaxed);
            crate::uart_puts(b"[FFA] Real SPMC detected at EL3\n");
        }
    }
}
```

**Step 3: Add a public query function for testing**

Add a public accessor so tests can verify the flag state:

```rust
/// Check if a real SPMC is present (for testing/debugging).
pub fn spmc_present() -> bool {
    SPMC_PRESENT.load(Ordering::Relaxed)
}
```

**Step 4: Verify build**

Run: `cargo build --target aarch64-unknown-none --features tfa_boot`
Expected: Build succeeds

Run: `cargo build --target aarch64-unknown-none`
Expected: Build succeeds (default features, SPMC_PRESENT stays false)

**Step 5: Commit**

```bash
git add src/ffa/proxy.rs
git commit -m "feat(ffa): set SPMC_PRESENT=true under tfa_boot feature"
```

---

### Task 3: Upgrade `forward_ffa_to_spmc()` to 8-register return

**Files:**
- Modify: `src/ffa/proxy.rs:93-110`

**Step 1: Replace forward_smc with forward_smc8**

The current `forward_ffa_to_spmc()` uses `forward_smc()` (returns x0-x3 only), losing x4-x7 which carry DIRECT_REQ/RESP payload. Replace with `forward_smc8()`:

```rust
/// Forward an FF-A call transparently to the Secure World (8-register).
///
/// Uses forward_smc8() to preserve x4-x7 (needed for DIRECT_REQ/RESP payload).
fn forward_ffa_to_spmc(context: &mut VcpuContext) -> bool {
    let result = smc_forward::forward_smc8(
        context.gp_regs.x0,
        context.gp_regs.x1,
        context.gp_regs.x2,
        context.gp_regs.x3,
        context.gp_regs.x4,
        context.gp_regs.x5,
        context.gp_regs.x6,
        context.gp_regs.x7,
    );
    context.gp_regs.x0 = result.x0;
    context.gp_regs.x1 = result.x1;
    context.gp_regs.x2 = result.x2;
    context.gp_regs.x3 = result.x3;
    context.gp_regs.x4 = result.x4;
    context.gp_regs.x5 = result.x5;
    context.gp_regs.x6 = result.x6;
    context.gp_regs.x7 = result.x7;
    true
}
```

**Step 2: Verify build**

Run: `cargo build --target aarch64-unknown-none --features tfa_boot`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add src/ffa/proxy.rs
git commit -m "fix(ffa): forward_ffa_to_spmc uses 8-register SMC return"
```

---

### Task 4: Route DIRECT_REQ to real SPMC when SPMC_PRESENT

**Files:**
- Modify: `src/ffa/proxy.rs:309-348`

**Step 1: Modify handle_msg_send_direct_req**

When `SPMC_PRESENT=true` and receiver is an SP (partition ID >= 0x8000), forward via `forward_ffa_to_spmc()` instead of stub echo:

```rust
fn handle_msg_send_direct_req(context: &mut VcpuContext) -> bool {
    let sender = ((context.gp_regs.x1 >> 16) & 0xFFFF) as u16;
    let receiver = (context.gp_regs.x1 & 0xFFFF) as u16;

    // Validate sender is the calling VM
    let vm_id = crate::global::current_vm_id();
    let expected_sender = vm_id_to_partition_id(vm_id);
    if sender != expected_sender {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // If real SPMC present and receiver is an SP (ID >= 0x8000), forward
    if SPMC_PRESENT.load(Ordering::Relaxed) && receiver >= FFA_SPMC_ID {
        return forward_ffa_to_spmc(context);
    }

    // Stub path: validate receiver is a known SP, echo x3-x7
    if !stub_spmc::is_valid_sp(receiver) {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    let x3 = context.gp_regs.x3;
    let x4 = context.gp_regs.x4;
    let x5 = context.gp_regs.x5;
    let x6 = context.gp_regs.x6;
    let x7 = context.gp_regs.x7;

    let is_64bit = context.gp_regs.x0 == FFA_MSG_SEND_DIRECT_REQ_64;
    context.gp_regs.x0 = if is_64bit {
        FFA_MSG_SEND_DIRECT_RESP_64
    } else {
        FFA_MSG_SEND_DIRECT_RESP_32
    };
    context.gp_regs.x1 = ((receiver as u64) << 16) | (sender as u64);
    context.gp_regs.x3 = x3;
    context.gp_regs.x4 = x4;
    context.gp_regs.x5 = x5;
    context.gp_regs.x6 = x6;
    context.gp_regs.x7 = x7;
    true
}
```

**Step 2: Verify stub path still works**

Run: `make clean && make run`
Expected: All 34 test suites pass (~204 assertions). The `test_ffa` suite exercises DIRECT_REQ with `SPMC_PRESENT=false` (default features), so the stub echo path is validated.

**Step 3: Commit**

```bash
git add src/ffa/proxy.rs
git commit -m "feat(ffa): forward DIRECT_REQ to real SPMC when SPMC_PRESENT"
```

---

### Task 5: Modify SP Hello to prove real dispatch (x4 += 0x1000)

**Files:**
- Modify: `tfa/sp_hello/start.S:50-69`

**Step 1: Modify the SP echo to add a signature**

Currently SP1 echoes x3-x7 unchanged. Add `x4 += 0x1000` so the BL33 test can distinguish real SP dispatch from stub echo:

Replace the `.Lmsg_loop` section in `tfa/sp_hello/start.S`:

```asm
.Lmsg_loop:
    /* Check if this is a DIRECT_REQ_32 */
    ldr     x8, =FFA_DIRECT_REQ_32
    cmp     x0, x8
    b.ne    .Lunknown_msg

    /* Swap source/dest in x1 for the response.
     * x1 = (source << 16) | dest -> response: (dest << 16) | source */
    lsr     x8, x1, #16         /* x8 = source */
    and     x9, x1, #0xFFFF     /* x9 = dest (our SP ID) */
    orr     x1, x8, x9, lsl #16 /* x1 = (dest << 16) | source */

    /* Build DIRECT_RESP: echo x3,x5,x6,x7; modify x4 += 0x1000 as proof */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    /* x3 passed through unchanged */
    add     x4, x4, #0x1000     /* SP signature: x4 += 0x1000 */
    /* x5-x7 passed through unchanged */
    smc     #0

    /* After ERET from SPMC with next request */
    b       .Lmsg_loop
```

**Step 2: Rebuild SP Hello**

Run: `make build-sp-hello`
Expected: `tfa/sp_hello/sp_hello.bin` rebuilt successfully

**Step 3: Commit**

```bash
git add tfa/sp_hello/start.S
git commit -m "feat(sp): SP1 adds 0x1000 to x4 as real-dispatch proof"
```

---

### Task 6: Add BL33 Test 7 — forwarded DIRECT_REQ

**Files:**
- Modify: `tfa/bl33_ffa_test/start.S:206-257`

**Step 1: Add Test 7 after Test 6**

Insert between `.Lfail_6` and `.Ldone`. Test 7 sends DIRECT_REQ with x4=0xBBBB, expects x4=0xBBBB+0x1000=0xCBBB (proving real SP dispatch, not stub echo):

Change the `b .Ldone` at line 206 to `b .Ltest_7`, then add:

```asm
    /* ============ Test 7: DIRECT_REQ via real SP (x4 += 0x1000) ============ */
.Ltest_7:
    adr     x0, str_t7
    bl      uart_print

    ldr     x0, =FFA_DIRECT_REQ_32
    /* x1: source=0x0001 (NWd VM), dest=0x8001 (SP1) */
    movz    x1, #0x8001
    movk    x1, #0x0001, lsl #16
    mov     x2, xzr
    movz    x3, #0xAAAA            /* payload */
    movz    x4, #0xBBBB            /* SP will add 0x1000 -> 0xCBBB */
    movz    x5, #0xCCCC
    movz    x6, #0xDDDD
    movz    x7, #0xEEEE
    smc     #0

    /* x0 should be FFA_DIRECT_RESP_32 */
    ldr     x9, =FFA_DIRECT_RESP_32
    cmp     x0, x9
    b.ne    .Lfail_7
    /* x3 should be echoed unchanged */
    movz    x9, #0xAAAA
    cmp     x3, x9
    b.ne    .Lfail_7
    /* x4 should be 0xBBBB + 0x1000 = 0xCBBB (SP proof) */
    movz    x9, #0xCBBB
    cmp     x4, x9
    b.ne    .Lfail_7
    /* x5 should be echoed unchanged */
    movz    x9, #0xCCCC
    cmp     x5, x9
    b.ne    .Lfail_7
    adr     x0, str_pass
    bl      uart_print
    b       .Ldone
.Lfail_7:
    adr     x0, str_fail
    bl      uart_print
```

Also add the string for Test 7 in the `.rodata` section:

```asm
str_t7:
    .asciz "  Test 7: DIRECT_REQ real SP ....... "
```

**Step 2: Update Test 6 to also check x4 modified**

Test 6 currently checks x3-x5 as echoed. Since SP now modifies x4, Test 6 should also expect x4=0xBBBB+0x1000=0xCBBB. Update the check at line 198:

```asm
    /* x4: SP adds 0x1000 to x4 -> 0xBBBB + 0x1000 = 0xCBBB */
    movz    x9, #0xCBBB
    cmp     x4, x9
    b.ne    .Lfail_6
```

**Step 3: Build BL33 test client**

Run: `make build-bl33-ffa-test`
Expected: `tfa/bl33_ffa_test.bin` rebuilt

**Step 4: Commit**

```bash
git add tfa/bl33_ffa_test/start.S
git commit -m "test: BL33 test 7 — DIRECT_REQ via real SP (x4 += 0x1000)"
```

---

### Task 7: Update Makefile — `run-tfa-linux` uses `tfa_boot`

**Files:**
- Modify: `Makefile:230-246`

**Step 1: Change run-tfa-linux to use tfa_boot feature**

Replace `--features linux_guest` with `--features tfa_boot` in the `run-tfa-linux` target:

```makefile
# Boot: TF-A → BL32 (stub S-EL2) → BL33 (our hypervisor at NS-EL2) → Linux
run-tfa-linux:
	@test -f $(TFA_FLASH_BL33) || (echo "ERROR: $(TFA_FLASH_BL33) not found. Run 'make build-tfa-bl33' first." && exit 1)
	@echo "Building hypervisor with TF-A boot support..."
	cargo build --target aarch64-unknown-none --features tfa_boot
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting TF-A → hypervisor → Linux boot chain..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU_SEL2) -machine virt,secure=on,virtualization=on,gic-version=3 \
	    -cpu max -smp 4 -m 2G -nographic \
	    -bios $(TFA_FLASH_BL33) \
	    -device loader,file=$(BINARY_BIN),addr=0x40200000,force-raw=on \
	    -device loader,file=$(LINUX_IMAGE),addr=0x48000000,force-raw=on \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000,force-raw=on \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000,force-raw=on \
	    -device loader,file=$(LINUX_DISK),addr=0x58000000,force-raw=on \
	    -nic none
```

**Step 2: Commit**

```bash
git add Makefile
git commit -m "chore: run-tfa-linux uses tfa_boot feature flag"
```

---

### Task 8: Rebuild TF-A with updated SP + BL33 test client, run integration test

**Files:** None (build + test only)

**Step 1: Rebuild everything**

```bash
# Rebuild SP Hello (with x4 += 0x1000 signature)
make build-sp-hello

# Rebuild BL33 test client (with Test 7)
make build-bl33-ffa-test

# Delete stale FIP from Docker volume to force rebuild
docker run --rm -v tfa-spmc-build-cache:/cache debian:bookworm-slim rm -f /cache/build/qemu/release/fip.bin

# Rebuild TF-A with updated SP + BL33
make build-tfa-spmc
```

**Step 2: Run unit tests (regression check)**

```bash
make clean && make run
```

Expected: All 34 test suites pass. `test_ffa` uses default features (no `tfa_boot`), so `SPMC_PRESENT=false` and the stub echo path runs as before.

**Step 3: Run SPMC integration test**

```bash
make run-spmc
```

Expected output:
```
========================================
  BL33 FF-A Test Client (NS-EL2)
========================================

  Test 1: FFA_VERSION .............. PASS
  Test 2: FFA_ID_GET ............... PASS
  Test 3: FFA_FEATURES(VERSION) .... PASS
  Test 4: FFA_FEATURES(0xDEAD) ..... PASS
  Test 5: PARTITION_INFO_GET ........ PASS
  Test 6: DIRECT_REQ echo .......... PASS
  Test 7: DIRECT_REQ real SP ....... PASS

  All tests complete.
```

**Important**: In `make run-spmc`, BL33 sends SMC directly to SPMD (there's no NS proxy in the middle — BL33 runs at NS-EL2 and talks to SPMD at EL3 directly). Tests 1-6 validate that SPMC dispatches correctly. Test 7 validates that SP1's x4 += 0x1000 signature works.

**Step 4: Commit (final)**

```bash
git add -A
git commit -m "feat(sprint5.1): DIRECT_REQ end-to-end — NS proxy → SPMD → SPMC → SP1

- tfa_boot feature: SPMC_PRESENT=true at compile time
- forward_ffa_to_spmc: 8-register SMC return (preserves x4-x7)
- handle_msg_send_direct_req: forwards to SPMC when SPMC_PRESENT
- SP1: x4 += 0x1000 as real-dispatch proof
- BL33 Test 7: verifies x4=0xCBBB (not stub echo 0xBBBB)
- All 34 unit tests still pass (stub path unchanged)"
```

---

### Task 9: Verify run-tfa-linux still boots Linux (optional, if flash-bl33.bin uses stub BL32)

**Files:** None (test only)

**Note**: `run-tfa-linux` uses `flash-bl33.bin` which has a **stub BL32** (not our real SPMC). So even with `tfa_boot` setting `SPMC_PRESENT=true`, the proxy will try to forward to SPMD → stub BL32 which doesn't handle FF-A. This is fine because Linux doesn't send FF-A calls without `CONFIG_ARM_FFA_TRANSPORT=y` (which we haven't enabled yet — that's Sprint 5.2).

**Step 1: Run**

```bash
make run-tfa-linux
```

Expected: Linux boots to BusyBox shell as before. No FF-A calls are made by Linux, so the `SPMC_PRESENT=true` flag has no effect.

If this fails, the issue would be `init()` trying to log a message before UART is ready. Verify the boot log shows `[FFA] TF-A boot: SPMC present (build-time)` after GIC init.

---

## Verification Summary

| Check | Command | Expected |
|-------|---------|----------|
| Unit tests (regression) | `make clean && make run` | 34 suites pass, ~204 assertions |
| SPMC integration (7 tests) | `make run-spmc` | 7/7 PASS (new Test 7: x4=0xCBBB) |
| TF-A + Linux boot | `make run-tfa-linux` | Linux boots to BusyBox shell |

## Files Changed

| File | Change Type | Description |
|------|-------------|-------------|
| `Cargo.toml` | Modify | Add `tfa_boot = ["linux_guest"]` feature |
| `src/ffa/proxy.rs` | Modify | `init()` compile-time SPMC_PRESENT, `forward_ffa_to_spmc()` 8-reg, `handle_msg_send_direct_req()` forward when SPMC_PRESENT |
| `tfa/sp_hello/start.S` | Modify | x4 += 0x1000 proof signature |
| `tfa/bl33_ffa_test/start.S` | Modify | Test 7: DIRECT_REQ real SP verification |
| `Makefile` | Modify | `run-tfa-linux` uses `--features tfa_boot` |

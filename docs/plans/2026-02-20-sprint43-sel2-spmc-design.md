# Sprint 4.3 Design: Hypervisor as S-EL2 SPMC (BL32)

**Date**: 2026-02-20
**Sprint**: 4.3 (Milestone 4)
**Prerequisite**: Sprint 4.1/4.2 (TF-A build infra + BL33 boot chain)

## Goal

Hypervisor boots as BL32 at S-EL2, parses SPMC manifest, completes SPMD handshake
(FFA_MSG_WAIT), and SPMD proceeds to boot BL33. No SP loading yet.

**Verification**: `make run-spmc` shows TF-A banner + "SPMC: init complete" + BL33
"Hello from NS-EL2!" on UART.

## Architecture

```
EL3:  TF-A BL31 + SPMD
         | jumps to BL32 entry (x0=manifest, x1=hw_config, x4=core_id)
S-EL2: Our hypervisor (sel2 feature)
         |-- parse manifest (x0 = TOS_FW_CONFIG DTB)
         |-- parse HW DTB (x1 = HW_CONFIG) -> UART, GIC
         |-- init exception vectors, GIC
         |-- print "SPMC: init complete"
         '-- FFA_MSG_WAIT SMC -> SPMD unblocks BL33
         |
NS-EL2: BL33 (trivial hello binary)
```

### SPMD -> SPMC Entry Protocol

SPMD enters BL32 at S-EL2 with:
- `x0` = TOS_FW_CONFIG physical address (SPMC manifest DTB)
- `x1` = HW_CONFIG physical address (hardware DTB)
- `x4` = Core linear ID

BL32 must issue `FFA_MSG_WAIT` (0x8400006B) SMC to signal init complete.
SPMD blocks until this SMC arrives, then boots BL33.

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Entry point | Separate `boot_sel2.S` | No changes to existing boot.S |
| Linker script | Separate `linker_sel2.ld` at 0x0e100000 | build.rs selects by feature |
| Feature flag | `sel2` (mutually exclusive with `linux_guest`) | Different rust_main |
| Manifest parser | `src/manifest.rs` using `fdt` crate | Already a dependency |
| VTTBR vs VSTTBR | Deferred | No Stage-2 needed (no SPs yet) |
| Exception vectors | Reuse `exception.S` unchanged | VBAR_EL2 + HCR_EL2 identical at S-EL2 |

## Memory Layout (S-EL2 mode)

| Region | Address | Size | Purpose |
|--------|---------|------|---------|
| Secure SRAM | 0x0e000000 | 1MB | TF-A BL1/BL2 |
| **SPMC code** | **0x0e100000** | ~1MB | Our hypervisor (BL32) |
| SPMC heap | 0x0e200000 | ~14MB | Future: SP page tables |
| UART | 0x09000000 | 4KB | PL011 (shared with NS) |
| GIC | 0x08000000 | 16MB | GICv3 (shared with NS) |

## Files Changed

### New Files

**`arch/aarch64/boot_sel2.S`** — S-EL2 entry point
- Save x0 (manifest), x1 (hw_config), x4 (core_id) in callee-saved regs
- Clear BSS, set up stack (same as boot.S)
- Call `rust_main_sel2(manifest_addr, hw_config_addr, core_id)`
- Secondary CPUs: halt (single-core for now)

**`arch/aarch64/linker_sel2.ld`** — S-EL2 linker script
- Base at 0x0e100000 (SEC_DRAM_BASE)
- Same sections as linker.ld (.text.boot, .rodata, .data, .bss)

**`src/manifest.rs`** — SPMC manifest parser
- Parse TOS_FW_CONFIG DTB using `fdt` crate
- Extract: spmc_id (0x8000), maj_ver, min_ver, exec_state, load_address
- Validate: spmc_id == 0x8000, exec_state == 0 (AArch64)

### Modified Files

**`src/main.rs`** — Add `rust_main_sel2()` entry point
```rust
#[cfg(feature = "sel2")]
#[no_mangle]
pub extern "C" fn rust_main_sel2(
    manifest_addr: usize,
    hw_config_addr: usize,
    core_id: usize,
) -> ! {
    // 1. Parse manifest (TOS_FW_CONFIG)
    manifest::init(manifest_addr);
    // 2. Parse HW DTB (x1)
    dtb::init(hw_config_addr);
    // 3. Init exception vectors + GIC
    exception::init();
    gicv3::init();
    // 4. Print status
    uart_puts(b"[SPMC] init complete, signaling SPMD\n");
    // 5. FFA_MSG_WAIT -> SPMD proceeds to BL33
    manifest::signal_spmc_ready();
    // 6. Idle loop (SPMD controls execution via world switch)
    loop { unsafe { core::arch::asm!("wfi"); } }
}
```

**`Cargo.toml`** — Add `sel2` feature
```toml
[features]
sel2 = []
```

**`build.rs`** — Conditional boot file + linker script
- `sel2` feature: compile `boot_sel2.S`, link with `linker_sel2.ld`
- Default: compile `boot.S`, link with `linker.ld`

**`Makefile`** — Add `build-spmc` + `run-spmc` targets
- `build-spmc`: cargo build --features sel2, objcopy to tfa/bl32.bin
- `run-spmc`: build-tfa with real BL32, run QEMU secure=on

**`tfa/spmc_manifest.dts`** — Update binary_size
- Increase from 512KB to 2MB for real hypervisor binary

## What Does NOT Change

- `boot.S` — untouched
- `linker.ld` — stays at 0x40200000
- `exception.S` — exception vectors identical at S-EL2
- `src/ffa/proxy.rs` — not needed for minimal scope
- All existing `make run*` targets — unaffected
- All 30 test suites — still pass on default feature

## Risks

| Risk | Severity | Mitigation |
|------|----------|-----------|
| QEMU S-EL2 sysreg bugs | Medium | Use QEMU 9.2.3 (already built) |
| Manifest DTB addr=0 | Low | Check for null, fall back to defaults |
| 15MB secure DRAM too small | Low | Minimal SPMC uses <1MB |
| HW_CONFIG not passed by SPMD | Medium | Fall back to QEMU virt defaults (dtb.rs already handles this) |

## Future (Sprint 4.4)

- Load trivial SP at S-EL1, set up secure Stage-2 (VSTTBR_EL2)
- SP calls FFA_MSG_SEND_DIRECT_RESP back to SPMC
- NS -> S FF-A message path via SPMD

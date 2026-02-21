# Sprint 4.4 Phase B: SP Boot + Direct Messaging

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot a minimal Secure Partition (SP) at S-EL1 from our SPMC at S-EL2, with NWd→SP direct messaging via FF-A.

**Architecture:** TF-A BL2 loads the SP binary into SEC_DRAM. Our SPMC sets up Secure Stage-2 page tables (VSTTBR_EL2), creates an SpContext, and ERETs to S-EL1. The SP boots, calls FFA_MSG_WAIT to signal idle. On DIRECT_REQ from NWd, the SPMC ERETs to the SP; the SP processes and responds via DIRECT_RESP (SMC trap back to S-EL2).

**Tech Stack:** Rust (no_std), AArch64 assembly, TF-A v2.12.0, QEMU virt (secure=on, TCG)

---

## Data Flow

```
BL33 (NS-EL2)                    SPMC (S-EL2)                    SP (S-EL1)
     |                                |                               |
     |  FFA_MSG_SEND_DIRECT_REQ       |                               |
     |  ──────SMC──────►  EL3 SPMD    |                               |
     |                    ──────►     dispatch_ffa()                   |
     |                                |  set SpContext regs            |
     |                                |  write VSTTBR_EL2             |
     |                                |  ERET ────────────────►  SP entry
     |                                |                          process req
     |                                |                          SMC DIRECT_RESP
     |                                |  ◄──── trap (EC_SMC64)        |
     |                                |  save SpContext               |
     |                    ◄──────    return response to SPMD          |
     |  ◄──────────────               |                               |
```

## Key Insight: enter_guest() Reuse

The existing `enter_guest()` in `exception.S` uses ERET with `SPSR_EL2.M` to determine target EL. Setting `SPSR_EL2 = 0x3C5` (EL1h, AArch64, DAIF masked) targets S-EL1 — the Security state is determined by `SCR_EL3` (set by TF-A), not by S-EL2. The same exception vector table handles S-EL1 traps (lower EL AArch64, offset +0x400).

No changes to `exception.S` or `enter_guest()` are needed.

## Components

### 1. SP Binary (`tfa/sp_hello/start.S`)

Minimal AArch64 assembly running at S-EL1:
- Entry: set up stack (SP = 0x0e400000), print "[SP] Hello from S-EL1"
- Call `smc FFA_MSG_WAIT` to signal idle to SPMC
- Message loop: on return from FFA_MSG_WAIT (SPMC ERETs with DIRECT_REQ args):
  - Read x3-x7 (payload from NWd)
  - Call `smc FFA_MSG_SEND_DIRECT_RESP` with echoed x3-x7
  - Loop back to FFA_MSG_WAIT
- Linked at 0x0e300000, stack at 0x0e400000

### 2. SpContext (`src/sp_context.rs`)

Per-SP saved state, analogous to `VcpuContext` + `Vcpu`:

```rust
pub struct SpContext {
    pub vcpu_ctx: VcpuContext,    // x0-x30, SP, PC, SPSR — passed to enter_guest()
    pub sp_id: u16,               // FF-A partition ID (e.g. 0x8001)
    pub state: SpState,           // Reset, Idle, Running, Blocked
    pub entry_point: u64,         // Cold boot entry
    pub vsttbr: u64,              // Secure Stage-2 base for this SP
}

pub enum SpState { Reset, Idle, Running, Blocked }
```

Global: `SP_CONTEXTS: [Option<SpContext>; MAX_SPS]` where `MAX_SPS = 4`.

### 3. Secure Stage-2 (`src/secure_stage2.rs`)

Reuses `DynamicIdentityMapper` page-table construction logic. New `SecureStage2Config`:

```rust
pub struct SecureStage2Config {
    pub vsttbr: u64,
    pub vstcr: u64,
}

impl SecureStage2Config {
    pub fn install(&self) {
        // msr vsttbr_el2, {vsttbr}
        // msr vstcr_el2, {vstcr}
    }
}
```

Identity mappings for SP1:
- SP code/data: 0x0e300000 - 0x0e400000 (1MB, RWX)
- UART: 0x09000000 (4KB, Device-nGnRnE, for debug output)
- NOT mapped: SPMC region (0x0e100000-0x0e300000) — isolation

VSTCR_EL2 configuration mirrors VTCR_EL2: T0SZ=16, SL0=L0 or L1, PS=48-bit PA, SH/ORGN/IRGN for cacheability.

### 4. SP Boot Sequence (in `rust_main_sel2()`)

Between GIC init (step 5) and FFA_MSG_WAIT (step 6):

```
5.5a. Init secure heap at 0x0e500000
5.5b. Build Secure Stage-2 for SP1 (identity map SP region + UART)
5.5c. Create SpContext: PC=0x0e300000, SP=0x0e400000, SPSR=0x3C5, sp_id=0x8001
5.5d. Install Secure Stage-2 (write VSTTBR_EL2/VSTCR_EL2, set HCR_EL2.VM=1)
5.5e. enter_guest(&mut sp_context.vcpu_ctx) — ERET to S-EL1
5.5f. SP boots → prints → calls FFA_MSG_WAIT → traps back (EC_SMC64)
5.5g. handle_smc() sees FFA_MSG_WAIT from SP → mark SP as Idle
5.5h. Continue to step 6 (signal SPMD ready)
```

### 5. SPMC Dispatch Changes (`src/spmc_handler.rs`)

Current `dispatch_ffa()` handles all requests in a pure function. Phase B changes:

- `FFA_MSG_SEND_DIRECT_REQ` where dest is an SP ID (0x8001):
  1. Look up SpContext by partition ID
  2. Set SP's x0-x7 from request (x0=DIRECT_REQ FID, x1=source/dest, x3-x7=payload)
  3. Mark SP as Running
  4. `enter_guest(&mut sp_ctx.vcpu_ctx)` — ERET to SP
  5. SP processes, calls DIRECT_RESP (SMC) → traps back
  6. Read SP's x0-x7 from saved context → return as SmcResult8 to event loop
  7. Mark SP as Idle

- `FFA_PARTITION_INFO_GET`: return count=1, SP1 descriptor (ID=0x8001)

- `FFA_FEATURES`: add DIRECT_REQ_32/64 as supported

Note: `dispatch_ffa()` can no longer be a pure function — it must call `enter_guest()` for SP dispatch. It becomes `dispatch_ffa(&mut self)` or takes mutable SP context references.

### 6. TF-A Configuration

**`tfa/sp_layout.json`**:
```json
{
    "SP1": {
        "image": "tfa/sp_hello/sp_hello.bin",
        "pm": "tfa/sp_hello/sp_manifest.dts",
        "owner": "Plat"
    }
}
```

**`tfa/tb_fw_config.dts`** — add SP1 node:
```dts
secure-partitions {
    compatible = "arm,sp";
    sp1 {
        uuid = <0x12345678 0x12345678 0x12345678 0x12345678>;
        load-address = <0x0 0x0e300000>;
        owner = "Plat";
    };
};
```

**`tfa/sp_hello/sp_manifest.dts`** — per-SP FF-A partition manifest:
```dts
/ {
    compatible = "arm,ffa-manifest-1.0";
    ffa-version = <0x00010001>;
    uuid = <0x12345678 0x12345678 0x12345678 0x12345678>;
    id = <0x8001>;
    execution-ctx-count = <1>;
    exception-level = <2>;  /* S-EL1 */
    execution-state = <0>;  /* AArch64 */
    messaging-method = <3>; /* Direct request/response */
};
```

## Memory Layout (SEC_DRAM)

```
0x0e100000  SPMC code + data (2MB, BL32)
0x0e300000  SP1 code + data (1MB, loaded by TF-A BL2 from FIP)
0x0e400000  [reserved for SP2]
0x0e500000  Secure heap + page tables (~11MB)
0x0f000000  End of SEC_DRAM
```

## Exception Handling: Two Code Paths

1. **SP execution** (during boot and DIRECT_REQ handling):
   - `enter_guest()` → SP runs → SP does SMC → traps back → `enter_guest()` returns
   - Exit reason decoded from ESR_EL2 in the returned VcpuContext
   - This is synchronous: SPMC waits for SP to complete

2. **SPMD event loop** (normal operation):
   - `forward_smc8()` sends response to SPMD, receives next NWd request
   - This is the existing Phase A loop

The SP execution path is nested inside the event loop: receive DIRECT_REQ → enter SP → SP returns → send DIRECT_RESP back to SPMD.

## Testing

**Unit tests** (`tests/test_sp_context.rs`):
- SpContext creation, state transitions (Reset→Idle→Running→Blocked)
- SecureStage2Config construction (VSTTBR/VSTCR values)
- SP dispatch routing (DIRECT_REQ to SP ID vs SPMC ID)

**Integration test** (BL33 `tfa/bl33_ffa_test/start.S`):
- Extend existing Test 6 (DIRECT_REQ): send to SP ID 0x8001, expect echoed payload
- New: PARTITION_INFO_GET returns count=1

## Deferred to Phase C

- Secure interrupt routing (FIQ to S-EL2, injection to SP)
- Multiple SP support
- SP manifest parsing from FIP at runtime
- FFA_SECONDARY_EP_REGISTER (0x84000087)
- FFA_MEM_SHARE/LEND between NWd and SP

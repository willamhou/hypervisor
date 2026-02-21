# Sprint 4.4: SPMC Event Loop + SP Loading

**Date**: 2026-02-21
**Status**: Approved
**Prerequisite**: Sprint 4.3 (S-EL2 SPMC boot via TF-A) complete

## Overview

Sprint 4.3 established the SPMC boot at S-EL2: TF-A loads our hypervisor as BL32, it initializes, sends FFA_MSG_WAIT to SPMD, and idles in a WFI loop. Sprint 4.4 replaces that idle loop with a full SPMC event loop that processes FF-A requests from the Normal World, boots Secure Partitions at S-EL1, and enables end-to-end cross-world FF-A messaging.

## SPMD-SPMC Re-entry Mechanism (Critical)

The SPMD re-enters the SPMC by **returning from the SMC call**. After `FFA_MSG_WAIT`, the SMC instruction returns with the next FF-A request in x0-x7. This means:

- The SPMC event loop processes SMC **return values**, not interrupt-driven callbacks
- Each `smc(response)` simultaneously sends the response AND receives the next request
- The loop pattern is: `result = smc(FFA_MSG_WAIT)` → process → `result = smc(response)` → process → ...

## Phased Design

### Phase A: SPMC Event Loop + FF-A Stub Responses

**Goal**: NWd sends FF-A SMC → SPMD → SPMC processes and responds.

**Key changes**:

1. **`SmcResult8`** — Extend `forward_smc()` to return all 8 registers (x0-x7). Current `forward_smc()` returns only x0-x3. The SPMC event loop needs x4-x7 for FF-A direct message payloads.

2. **`src/spmc_handler.rs`** (NEW) — SPMC event loop module:
   ```
   pub fn run_event_loop() -> ! {
       let mut result = smc8(FFA_MSG_WAIT, 0, 0, 0, 0, 0, 0, 0);
       loop {
           let response = dispatch_ffa(result);
           result = smc8(response);
       }
   }
   ```
   `dispatch_ffa()` matches on `result.x0` (function ID) and returns the FF-A response tuple.

3. **Supported FF-A calls (stubs)**:
   | Function | ID | Response |
   |----------|----|----------|
   | FFA_VERSION | 0x84000063 | Return 0x10001 (v1.1) |
   | FFA_ID_GET | 0x84000069 | Return 0x8000 (SPMC ID) |
   | FFA_SPM_ID_GET | 0x84000085 | Return 0x8000 |
   | FFA_FEATURES | 0x84000064 | Return support bitmap |
   | FFA_PARTITION_INFO_GET | 0x84000068 | Return SP count + descriptors |
   | FFA_MSG_SEND_DIRECT_REQ | 0xC400006F | Echo x4-x7 back (stub) |
   | Unknown | * | FFA_ERROR(NOT_SUPPORTED) |

4. **BL33 test client** (`tfa/bl33_ffa_test/`) — Replace trivial `bl33_hello` with a BL33 that sends FF-A SMC calls and verifies responses. Prints PASS/FAIL for each test.

5. **Makefile**: `make run-spmc` updated to use BL33 test client.

**Acceptance criteria**:
- `make run-spmc` → BL33 sends FFA_VERSION → gets 0x10001 back
- BL33 sends FFA_ID_GET → gets 0x8000 back
- BL33 sends DIRECT_REQ → gets echo response
- All existing tests pass (`make run`)

### Phase B: Boot Trivial SP at S-EL1

**Goal**: SPMC loads and runs a minimal SP at S-EL1.

**Key changes**:

1. **`sp_hello/`** (NEW) — Trivial SP binary:
   - Entry at S-EL1, prints "[SP] Hello", calls FFA_MSG_WAIT via SMC
   - Packaged into TF-A FIP via `sp_layout.json`

2. **Secure Stage-2 page tables** (VSTTBR_EL2):
   - Reuse `DynamicIdentityMapper` for secure memory isolation
   - Map SP code/data region within SEC_DRAM (0x0e100000-0x0f000000)
   - Each SP gets isolated address space

3. **`SpContext`** struct — Per-SP execution context:
   - Saved/restored registers (x0-x30, SP_EL1, ELR_EL2, SPSR_EL2)
   - VSTTBR_EL2 for Stage-2 switching
   - SP state machine (RESET → IDLE → RUNNING → BLOCKED)

4. **SP launch sequence**:
   - Set ELR_EL2 = SP entry point, SPSR_EL2 = EL1h (AArch64, SPx)
   - Write VSTTBR_EL2 for SP's Stage-2
   - ERET from S-EL2 → S-EL1
   - SP runs, calls FFA_MSG_WAIT → traps back to S-EL2

5. **`tb_fw_config.dts`** + **`sp_layout.json`** updates:
   - SP descriptor in `secure-partitions` node (UUID, load address, size)
   - `sp_layout.json` references SP binary + manifest DTS

**Acceptance criteria**:
- SP boots at S-EL1, prints "[SP] Hello"
- SP calls FFA_MSG_WAIT → returns to SPMC event loop
- SPMC reports SP as IDLE in PARTITION_INFO_GET response
- Secure Stage-2 isolates SP memory

### Phase C: End-to-End Cross-World FF-A Messaging

**Goal**: NWd → SPMD → SPMC → SP → DIRECT_RESP → NWd.

**Key changes**:

1. **DIRECT_REQ routing** — SPMC receives DIRECT_REQ from NWd with destination SP UUID/ID. Instead of echoing, SPMC:
   - Saves NWd context
   - Loads target SP context (VSTTBR_EL2, registers)
   - Injects DIRECT_REQ into SP (set x0-x7, ERET to S-EL1)
   - SP processes, calls FFA_MSG_SEND_DIRECT_RESP
   - SPMC catches SMC trap, saves SP context
   - Returns DIRECT_RESP to SPMD (which forwards to NWd)

2. **SP context switching**:
   - `save_sp_context()` / `restore_sp_context()` for S-EL1 state
   - HCR_EL2 reconfigured between SP execution and SPMD communication
   - VTTBR_EL2/VSTTBR_EL2 switching per SP

3. **Multiple SP support** — Array of `SpContext`, routed by SP ID (0x8001, 0x8002, ...).

4. **BL33 test client extended**:
   - Sends DIRECT_REQ to SP UUID
   - Verifies DIRECT_RESP contains expected payload

**Acceptance criteria**:
- BL33 sends DIRECT_REQ(SP1, payload) → SP1 receives → SP1 sends DIRECT_RESP(modified payload) → BL33 receives
- Multiple SPs can be targeted independently
- SP context properly isolated (register state preserved across switches)

### Phase D: SP Manifest + RXTX + Memory Operations

**Goal**: Full SP lifecycle with manifest-driven configuration, mailbox communication, and secure memory management.

**Key changes**:

1. **SP manifest DTS parsing**:
   - Each SP has a manifest DTS (UUID, entry point, memory regions, messaging methods)
   - Parsed by SPMC at boot, drives SP creation and Stage-2 setup
   - `SpManifest` struct with properties from FF-A partition manifest (DEN0077A)

2. **FFA_RXTX_MAP / FFA_RXTX_UNMAP** for SPs:
   - Per-SP TX/RX buffer pair in secure memory
   - Used for PARTITION_INFO_GET responses and indirect messaging

3. **FFA_MEM_SHARE / FFA_MEM_LEND** (Secure World):
   - NWd shares memory with SP via SPMC
   - SPMC validates ownership via Stage-2 PTE SW bits
   - Maps shared pages into SP's Secure Stage-2
   - Reuses existing `src/ffa/memory.rs` ownership model

4. **FFA_SECONDARY_EP_REGISTER**:
   - SP registers secondary entry point for warm boot
   - Stored in SpContext, used when SPMC needs to re-enter SP

5. **Extended BL33 test client**:
   - RXTX_MAP, MEM_SHARE with SP, verify data visible in SP
   - PARTITION_INFO_GET returns manifest-driven SP descriptors

**Acceptance criteria**:
- SP manifest parsed, SP booted with manifest-specified entry + memory
- NWd shares page with SP, SP reads shared data correctly
- RXTX buffers functional for partition info queries
- All previous phase tests still pass

## Memory Layout (SEC_DRAM)

```
0x0e100000  SPMC code + data (2MB, loaded by TF-A as BL32)
0x0e300000  SP1 code + data (1MB, loaded by TF-A from FIP)
0x0e400000  SP2 code + data (1MB)
0x0e500000  Secure heap / page tables (remaining ~11MB)
0x0f000000  End of SEC_DRAM
```

## Files Changed (All Phases)

| File | Phase | Action | Description |
|------|-------|--------|-------------|
| `src/ffa/smc_forward.rs` | A | Modify | Add `forward_smc8()` returning x0-x7 |
| `src/spmc_handler.rs` | A | CREATE | SPMC event loop + FF-A dispatch |
| `src/main.rs` | A | Modify | Call `spmc_handler::run_event_loop()` |
| `src/lib.rs` | A | Modify | Add `pub mod spmc_handler` |
| `tfa/bl33_ffa_test/` | A | CREATE | BL33 FF-A test client |
| `Makefile` | A | Modify | Update `run-spmc` target |
| `tfa/sp_hello/` | B | CREATE | Trivial SP binary (S-EL1) |
| `src/sp_context.rs` | B | CREATE | Per-SP context + state machine |
| `src/secure_stage2.rs` | B | CREATE | Secure Stage-2 page tables |
| `tfa/sp_layout.json` | B | Modify | Add SP1 definition |
| `tfa/tb_fw_config.dts` | B | Modify | Add SP descriptor |
| `src/spmc_handler.rs` | C | Modify | DIRECT_REQ routing to SP |
| `src/sp_context.rs` | C | Modify | Context switch save/restore |
| `tfa/bl33_ffa_test/` | C | Modify | Add cross-world messaging tests |
| `src/sp_manifest.rs` | D | CREATE | SP manifest DTS parser |
| `src/spmc_handler.rs` | D | Modify | RXTX + MEM_SHARE handlers |
| `src/ffa/memory.rs` | D | Modify | Secure world ownership transitions |

## Exclusions (Out of Scope)

- **Multi-core SPMC**: Secondary CPUs halt (single-core SPMC only)
- **Interrupt forwarding**: No secure interrupt routing to SPs
- **pKVM integration**: Deferred to Phase 4.5
- **RME/CCA**: Deferred to Phase 5
- **Production SP images**: Only trivial test SPs (sp_hello)

## Relationship to Existing Code

- **`src/ffa/proxy.rs`**: NS-EL2 FF-A proxy (intercepts guest SMC at NS-EL2). Sprint 4.4's `spmc_handler.rs` is the S-EL2 counterpart. They share FF-A constants but have different dispatch models (proxy modifies guest context vs. SPMC sends SMC responses).
- **`src/ffa/stub_spmc.rs`**: Simulates SPs for NS-EL2 testing. Phase B replaces this with real SP execution at S-EL1.
- **`DynamicIdentityMapper`**: Reused for Secure Stage-2 (VSTTBR_EL2) in Phase B.
- **`VcpuArchState`**: Inspiration for `SpContext` — similar save/restore pattern but for S-EL1 instead of NS-EL1.

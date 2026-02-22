# Phase 4.5 Feasibility Research: pKVM at NS-EL2 + Our SPMC at S-EL2

**Date**: 2026-02-22
**Status**: FEASIBLE with high effort / expect debugging
**Prerequisites**: QEMU 9.2+ (done), TF-A SPMD (done), pKVM kernel build

---

## 1. pKVM Boot Model

### What is pKVM?

pKVM (protected KVM) is Android's hypervisor running at NS-EL2 in **nVHE mode** (non-Virtualization Host Extensions). It deprivileges the host kernel from EL2 to EL1 and places it behind a Stage-2 MMU, preventing the host from accessing protected guest memory.

### Boot Flow

```
1. Bootloader (TF-A BL33) enters kernel at NS-EL2
2. head.S detects EL2 via CurrentEL
3. Installs __hyp_stub_vectors at EL2, ERET drops to EL1
4. Normal kernel boot at EL1
5. kvm_arm_init() → __kvm_hyp_init installs nVHE code at EL2
6. __pkvm_init (HVC):
   - Creates EL2 Stage-1 (hypervisor address space)
   - Creates host Stage-2 in VTTBR_EL2
   - Host kernel now runs behind pKVM's Stage-2
7. pKVM intercepts all subsequent PSCI CPU_ON/SUSPEND
```

### VHE Override Required

QEMU `-cpu max` enables VHE (FEAT_VHE). pKVM requires nVHE. Two options:

| Method | How |
|--------|-----|
| CPU flag | `-cpu max,vh=off` (disable VHE at CPU level) |
| Kernel cmdline | `id_aa64mmfr1.vh=0` (override feature register) |

Without this, `kvm-arm.mode=protected` is silently ignored.

---

## 2. QEMU TCG Compatibility

### Command Line

```bash
qemu-system-aarch64 \
    -machine virt,secure=on,virtualization=on,gic-version=3 \
    -cpu max,vh=off \
    -smp 4 \
    -m 2G \
    -bios flash-full.bin \
    -device loader,file=Image,addr=0x40200000,force-raw=on \
    -append "kvm-arm.mode=protected console=ttyAMA0" \
    -nographic
```

Key differences from current `run-tfa-linux-ffa`:
- `-cpu max,vh=off`: Forces nVHE for pKVM
- `-kernel Image` or `-device loader`: pKVM kernel as BL33
- `-append "kvm-arm.mode=protected"`: Enables protected mode

### TCG Support Status

| Feature | TCG Support | Notes |
|---------|-------------|-------|
| EL0/EL1/EL2/EL3 | Yes | All exception levels emulated |
| FEAT_SEL2 (S-EL2) | Yes | Our SPMC already proves this |
| VHE→nVHE transition | Yes | ERET-based EL drop, standard |
| Host Stage-2 (VTTBR_EL2) | Yes | Same mechanism as our hypervisor |
| GICv3 virtualization (ICH_*) | Yes | Used by our GIC emulation |
| PSCI interception | Yes | HVC trap at EL2 |

### Known Limitations

- **TCG forces software MMU**: Dual Stage-2 (pKVM host S2 + guest S2) = very slow
- **No public CI**: pKVM team tests on Pixel hardware + Arm FVP, not QEMU TCG
- **LKML Sept 2025**: `kvm-unit-tests hang on Arm FVP with protected mode` — even FVP has issues
- **Google confirms QEMU testing**: Nov 2024 patches "tested on Pixel 6 and Qemu" (unclear if TCG or KVM)

---

## 3. pKVM FF-A Proxy Deep Dive (`arch/arm64/kvm/hyp/nvhe/ffa.c`)

### Architecture

```
NS-EL1 (host kernel)
  │ SMC trapped by HCR_EL2.TSC=1
  ▼
NS-EL2 (pKVM ffa.c proxy)     ← validates page ownership
  │ arm_smccc_1_2_smc() to EL3
  ▼
EL3 (TF-A SPMD)               ← world switch
  ▼
S-EL2 (our SPMC)              ← dispatches to SPs
  ▼
S-EL1 (Secure Partitions)
```

### 3.1 Initialization: `hyp_ffa_init()`

pKVM's FF-A proxy initializes during EL2 setup:

```c
int hyp_ffa_init(void *pages) {
    // 1. Check SMCCC version >= 1.2
    if (kvm_host_psci_config.smccc_version < ARM_SMCCC_VERSION_1_2)
        return 0;

    // 2. Version negotiation with SPMC via SPMD
    arm_smccc_1_2_smc({.a0 = FFA_VERSION, .a1 = FFA_VERSION_1_2}, &res);
    // Negotiate down to max supported version

    // 3. Allocate buffers from pre-reserved pages
    hyp_buffers = { .tx = tx, .rx = rx };       // Hyp's own RXTX (registered with SPMD)
    host_buffers = { /* empty, filled by do_ffa_rxtx_map */ };

    // 4. Allocate descriptor buffer for fragmented MEM_RETRIEVE_RESP
    ffa_desc_buf = { .buf = pages, .len = remaining_pages * PAGE_SIZE };
}
```

**Buffer sizing**: `hyp_ffa_proxy_pages()` calculates:
- 2 pages for hyp TX/RX (`KVM_FFA_MBOX_NR_PAGES = 1` each)
- N pages for fragmented descriptor buffer (`SG_MAX_SEGMENTS * sizeof(ffa_mem_region_addr_range)`)

**Our equivalent**: `ffa::proxy::init()` in `src/ffa/proxy.rs` — registers `PROXY_TX_BUF`/`PROXY_RX_BUF` with SPMD via `forward_smc8(FFA_RXTX_MAP, ...)`.

### 3.2 Entry Point: `kvm_host_ffa_handler()`

```c
bool kvm_host_ffa_handler(struct kvm_cpu_context *host_ctxt, u32 func_id) {
    if (!is_ffa_call(func_id))
        return false;              // Not FF-A, let caller handle

    // Version must be negotiated before any other call
    if (func_id != FFA_VERSION && !has_version_negotiated)
        return FFA_RET_INVALID_PARAMETERS;

    switch (func_id) {
    case FFA_FEATURES:       do_ffa_features(&res, host_ctxt);  break;
    case FFA_FN64_RXTX_MAP:  do_ffa_rxtx_map(&res, host_ctxt);  break;
    case FFA_RXTX_UNMAP:     do_ffa_rxtx_unmap(&res, host_ctxt); break;
    case FFA_MEM_SHARE:
    case FFA_FN64_MEM_SHARE: do_ffa_mem_xfer(FFA_FN64_MEM_SHARE, &res, host_ctxt); break;
    case FFA_MEM_LEND:
    case FFA_FN64_MEM_LEND:  do_ffa_mem_xfer(FFA_FN64_MEM_LEND, &res, host_ctxt);  break;
    case FFA_MEM_RECLAIM:    do_ffa_mem_reclaim(&res, host_ctxt); break;
    case FFA_MEM_FRAG_TX:    do_ffa_mem_frag_tx(&res, host_ctxt); break;
    case FFA_VERSION:        do_ffa_version(&res, host_ctxt);     break;
    case FFA_PARTITION_INFO_GET: do_ffa_part_get(&res, host_ctxt); break;
    default:
        if (ffa_call_supported(func_id))
            return false;      // Pass through to EL3 unmodified
        return FFA_RET_NOT_SUPPORTED;
    }
    ffa_set_retval(host_ctxt, &res);
    return true;
}
```

**Our equivalent**: `handle_ffa_call()` in `src/ffa/proxy.rs:87` — same dispatch pattern.

### 3.3 Blocked Calls: `ffa_call_supported()`

pKVM explicitly blocks these FF-A calls (returns NOT_SUPPORTED):

| Blocked Call | Reason |
|---|---|
| `FFA_FN64_MEM_DONATE` | Host could donate guest pages to Secure World |
| `FFA_MEM_DONATE` (32-bit) | Same |
| `FFA_FN64_MEM_RETRIEVE_REQ` | Host should not retrieve from Secure World |
| `FFA_MEM_RELINQUISH` | Host should not relinquish Secure memory |
| `FFA_MSG_SEND` / `FFA_MSG_POLL` / `FFA_MSG_WAIT` | Legacy indirect messaging |
| `FFA_NOTIFICATION_*` | All notification calls blocked |
| `FFA_MSG_SEND_DIRECT_REQ2` / `RESP2` | FF-A v1.2 extended messaging |
| `FFA_CONSOLE_LOG` | Debug interface |

**Key difference from our proxy**: We support notifications (BITMAP_CREATE/BIND/SET/GET) and indirect messaging (MSG_SEND2/MSG_WAIT). pKVM blocks these — they are optional per FF-A spec and pKVM takes a minimal-surface approach.

### 3.4 Memory Transfer: `__do_ffa_mem_xfer()` (MEM_SHARE/LEND)

This is the **core security function**. Prevents confused-deputy attacks.

```c
static void __do_ffa_mem_xfer(u64 func_id, struct arm_smccc_1_2_regs *res,
                              struct kvm_cpu_context *ctxt) {
    // 1. Extract parameters
    DECLARE_REG(u32, len, ctxt, 1);        // Total descriptor length
    DECLARE_REG(u32, fraglen, ctxt, 2);    // First fragment length
    DECLARE_REG(u64, addr_mbz, ctxt, 3);   // Must be zero
    DECLARE_REG(u32, npages_mbz, ctxt, 4); // Must be zero

    // 2. Validate parameters
    if (addr_mbz || npages_mbz || fraglen > len) → INVALID_PARAMETERS

    // 3. Lock host buffers, copy descriptor from host TX to hyp TX
    hyp_spin_lock(&host_buffers.lock);
    buf = hyp_buffers.tx;
    memcpy(buf, host_buffers.tx, fraglen);

    // 4. Parse FF-A composite memory region descriptor
    //    FfaMemRegion → FfaMemAccessDesc → FfaCompositeMemRegion → constituents[]
    ep_mem_access = buf + ffa_mem_desc_offset(buf, 0, hyp_ffa_version);
    offset = ep_mem_access->composite_off;
    reg = buf + offset;
    nr_ranges = (fraglen - offset) / sizeof(constituent);

    // 5. CRITICAL: Validate page ownership before forwarding
    ret = ffa_host_share_ranges(reg->constituents, nr_ranges);
    //    → for each page: __pkvm_host_share_ffa(pfn, nr_pages)
    //    → check PKVM_PAGE_OWNED → transition to PKVM_PAGE_SHARED_OWNED

    // 6. Forward to EL3/SPMD
    ffa_mem_xfer(res, func_id, len, fraglen);

    // 7. Handle fragmented response or rollback on error
    if (fraglen != len && res->a0 == FFA_MEM_FRAG_RX) {
        // Continue with do_ffa_mem_frag_tx for remaining fragments
    } else if (res->a0 != FFA_SUCCESS) {
        ffa_host_unshare_ranges(reg->constituents, nr_ranges); // Rollback
    }
}
```

### 3.5 Page State Transitions (`mem_protect.c`)

```c
// Share: OWNED → SHARED_OWNED
int __pkvm_host_share_ffa(u64 pfn, u64 nr_pages) {
    host_lock_component();
    ret = __host_check_page_state_range(phys, size, PKVM_PAGE_OWNED);
    if (!ret)
        ret = __host_set_page_state_range(phys, size, PKVM_PAGE_SHARED_OWNED);
    host_unlock_component();
    return ret;
}

// Unshare (reclaim): SHARED_OWNED → OWNED
int __pkvm_host_unshare_ffa(u64 pfn, u64 nr_pages) {
    host_lock_component();
    ret = __host_check_page_state_range(phys, size, PKVM_PAGE_SHARED_OWNED);
    if (!ret)
        ret = __host_set_page_state_range(phys, size, PKVM_PAGE_OWNED);
    host_unlock_component();
    return ret;
}
```

**Our equivalent**: `validate_page_for_share()` in `src/ffa/memory.rs` + `Stage2Walker::read_sw_bits()`/`write_sw_bits()` in `src/ffa/stage2_walker.rs`.

### 3.6 Memory Reclaim: `do_ffa_mem_reclaim()`

Reclaim is more complex because SPMD may return a **fragmented** RETRIEVE_RESP:

```c
static void do_ffa_mem_reclaim(...) {
    // 1. Send FFA_MEM_RETRIEVE_REQ to get descriptor from SPMD
    buf = hyp_buffers.tx;
    *buf = { .sender_id = HOST_FFA_ID, .handle = handle };
    ffa_retrieve_req(res, sizeof(*buf));

    // 2. Handle fragmented response — collect all fragments into ffa_desc_buf
    buf = ffa_desc_buf.buf;
    memcpy(buf, hyp_buffers.rx, fraglen);
    ffa_rx_release(res);
    for (fragoff = fraglen; fragoff < len; fragoff += fraglen) {
        ffa_mem_frag_rx(res, handle_lo, handle_hi, fragoff);
        memcpy(buf + fragoff, hyp_buffers.rx, fraglen);
        ffa_rx_release(res);
    }

    // 3. Actually reclaim
    ffa_mem_reclaim(res, handle_lo, handle_hi, flags);

    // 4. Unshare ranges (SHARED_OWNED → OWNED)
    reg = buf + offset;
    ffa_host_unshare_ranges(reg->constituents, reg->addr_range_cnt);
}
```

**Our gap**: Our SPMC does not currently handle NWd MEM_SHARE/RETRIEVE/RECLAIM forwarded through SPMD. The stub SPMC handles these locally.

### 3.7 Dual RXTX Buffer Management

```c
// Host calls FFA_RXTX_MAP → pKVM intercepts
static void do_ffa_rxtx_map(...) {
    // Map host's TX/RX pages into EL2 address space
    tx = hyp_phys_to_virt(tx_phys);  // Share host page with EL2
    rx = hyp_phys_to_virt(rx_phys);
    host_buffers.tx = tx;
    host_buffers.rx = rx;
    // NOTE: Does NOT forward to EL3 — hyp_buffers already registered
}

// PARTITION_INFO_GET → forward to EL3, copy result to host RX
static void do_ffa_part_get(...) {
    arm_smccc_1_2_smc({FFA_PARTITION_INFO_GET, uuid...}, &res);
    // Copy from hyp RX to host RX
    memcpy(host_buffers.rx, hyp_buffers.rx, partition_sz * count);
}
```

**Our equivalent**: In `src/ffa/proxy.rs`, `PROXY_TX_BUF`/`PROXY_RX_BUF` are the hyp buffers. `handle_partition_info_get()` copies from proxy RX to guest RX. The SPMC (`spmc_handler.rs`) manages `NWD_RXTX` state separately.

---

## 4. SPMD Relay Mechanism (TF-A EL3)

### What SPMD Does

The SPMD (Secure Partition Manager Dispatcher) at EL3 is a thin relay layer:

| FF-A Call | SPMD Handling |
|---|---|
| FFA_VERSION | Handled by SPMD directly (returns SPMC's version) |
| FFA_FEATURES | Forwarded to SPMC |
| FFA_MSG_SEND_DIRECT_REQ | Forwarded to SPMC via world switch |
| FFA_MEM_SHARE/LEND | Forwarded to SPMC |
| FFA_RXTX_MAP (from NWd) | Forwarded to SPMC (TF-A v2.12+) |
| FFA_PARTITION_INFO_GET | Forwarded to SPMC |
| FFA_RUN | Forwarded to SPMC |
| FFA_MSG_WAIT | SPMC→SPMD: signals SPMC init complete, returns to NWd |

### EL2 Context Save/Restore

With `CTX_INCLUDE_EL2_REGS=1` (our TF-A build), SPMD saves/restores these EL2 registers during world switch:

| Register | Purpose |
|---|---|
| VTTBR_EL2 | Stage-2 table base (pKVM's host S2 vs our Secure S2) |
| VTCR_EL2 | Stage-2 translation control |
| HCR_EL2 | Hypervisor configuration (TSC, VM, etc.) |
| VBAR_EL2 | Exception vector base |
| SCTLR_EL2 | System control |
| SP_EL2 | Stack pointer |
| TPIDR_EL2 | Thread pointer |
| ICH_*_EL2 | GIC virtual interface control |
| CNTHCTL_EL2 | Timer access control |
| VMPIDR_EL2 | Virtualized MPIDR |

This is critical: pKVM's NS Stage-2 (VTTBR_EL2) and our Secure Stage-2 (VSTTBR_EL2) are independently maintained. SPMD context-switches between them on every world switch.

### SMC Flow: NWd → SPMD → SPMC

```
pKVM (NS-EL2) executes: smc #0 (FFA_MSG_SEND_DIRECT_REQ)
  ↓ SMC exception to EL3
SPMD at EL3:
  1. Save NS-EL2 context (HCR, VTTBR, VBAR, ICH_*, ...)
  2. Restore S-EL2 context
  3. Set return registers (x0-x7) with FF-A call args
  4. ERET to S-EL2
  ↓
Our SPMC at S-EL2:
  1. forward_smc8() returns with the request
  2. dispatch_request() routes to SP
  3. ERET to SP at S-EL1
  4. SP processes, returns via SMC (FFA_MSG_SEND_DIRECT_RESP)
  5. SPMC sends response via forward_smc8() → SMC to EL3
  ↓
SPMD at EL3:
  1. Save S-EL2 context
  2. Restore NS-EL2 context
  3. ERET to NS-EL2
  ↓
pKVM (NS-EL2): receives DIRECT_RESP, returns to host (EL1)
```

---

## 5. Function-by-Function Comparison

### Dispatch Table

| pKVM (`ffa.c`) | Our Proxy (`proxy.rs`) | Our SPMC (`spmc_handler.rs`) | Notes |
|---|---|---|---|
| `kvm_host_ffa_handler()` | `handle_ffa_call()` | `dispatch_request()` | Main entry |
| `do_ffa_version()` | `handle_version()` | returns via FFA_MSG_WAIT | Version negotiation |
| `do_ffa_rxtx_map()` | `handle_rxtx_map()` | `handle_rxtx_map()` (NWd) | Buffer management |
| `do_ffa_part_get()` | `handle_partition_info_get()` | `handle_partition_info_get()` | SP enumeration |
| `__do_ffa_mem_xfer()` | `handle_mem_share()` | N/A (gap) | Page ownership |
| `do_ffa_mem_reclaim()` | `handle_mem_reclaim()` | N/A (gap) | Reclaim |
| `do_ffa_mem_frag_tx()` | N/A (not implemented) | N/A | Fragmented descriptors |
| `ffa_call_supported()` | `is_ffa_function()` | N/A | Allowlist |
| `ffa_host_share_ranges()` | Stage2Walker::write_sw_bits() | N/A | PTE SW bits |
| N/A | N/A | `dispatch_to_sp()` | ERET to SP |
| N/A | N/A | `resume_preempted_sp()` | FFA_RUN |

### Page Ownership Model

| State | pKVM (`kvm_pkvm.h`) | Our Code (`memory.rs`) | PTE SW bits |
|---|---|---|---|
| Owned | `PKVM_PAGE_OWNED` | `PageOwnership::Owned` | 0b00 |
| Shared (sender) | `PKVM_PAGE_SHARED_OWNED` | `PageOwnership::SharedOwned` | 0b01 |
| Shared (receiver) | `PKVM_PAGE_SHARED_BORROWED` | `PageOwnership::SharedBorrowed` | 0b10 |
| Donated | (blocked at proxy) | `PageOwnership::Donated` | 0b11 |

The encoding is **byte-identical** — both use ARM's recommended PTE SW bits [56:55].

---

## 6. Boot Chain for Dual Hypervisor

### Target Architecture

```
EL3:    TF-A BL31 + SPMD (world switch, SMC relay)
S-EL2:  Our SPMC (BL32) — manages SPs via Secure Stage-2
S-EL1:  SP Hello (0x8001), SP IRQ (0x8002)
NS-EL2: pKVM nVHE — manages host via NS Stage-2, FF-A proxy
NS-EL1: Linux/Android host (deprivileged)
```

### Boot Sequence

```
BL1 (EL3) → BL2 (EL1-S) → BL31/SPMD (EL3)
  → BL32/our SPMC (S-EL2): init SPs, FFA_MSG_WAIT
  → BL33/pKVM kernel (NS-EL2): head.S → drop to NS-EL1
    → kvm_arm_init → __pkvm_init (HVC)
    → pKVM takes NS-EL2, host at NS-EL1
    → FF-A driver probes → FFA_VERSION → pKVM proxy → SPMD → our SPMC
```

### Memory Layout

| Region | Address | Purpose |
|--------|---------|---------|
| Secure Flash | 0x00000000 | BL1 + FIP |
| Secure SRAM | 0x0e000000 | TF-A BL1/BL2 |
| SPMC (BL32) | 0x0e100000 | Our hypervisor (S-EL2) |
| SP Hello | 0x0e300000 | Secure Partition 1 |
| SP IRQ | 0x0e400000 | Secure Partition 2 |
| QEMU DTB | 0x40000000 | Auto-generated hardware DTB |
| pKVM kernel (BL33) | 0x40200000 | Linux Image (preloaded) |
| pKVM hyp code | Allocated by kernel | nVHE hypervisor at NS-EL2 |
| Host RAM | 0x48000000+ | Normal world memory |

### Key Difference from Current Setup

Current `run-tfa-linux-ffa`: Our hypervisor at NS-EL2 (BL33) runs Linux as a guest VM via its own Stage-2.

Target: pKVM kernel IS the BL33. It boots natively, then deprivileges itself. Our hypervisor is only at S-EL2 (BL32). No NS-EL2 hypervisor from us — pKVM takes that role.

---

## 7. Kernel Build Requirements

### Kernel Source

| Option | Version | Notes |
|--------|---------|-------|
| AOSP `android16-6.12` | 6.12.x | Canonical pKVM source, Google-maintained |
| Mainline kernel.org | 6.12+ | pKVM merged upstream, less Android-specific |
| Our existing 6.12.12 | 6.12.12 | Already built for `run-linux`, needs KVM config |

**Recommendation**: Start with our existing 6.12.12 kernel + additional KVM config. Fall back to AOSP if pKVM-specific patches are needed.

### Required Config Options

```
# KVM core (must be built-in, not module)
CONFIG_VIRTUALIZATION=y
CONFIG_KVM=y

# pKVM debug (optional but recommended)
CONFIG_NVHE_EL2_DEBUG=y
CONFIG_PROTECTED_NVHE_STACKTRACE=y

# FF-A transport (for host FF-A driver)
CONFIG_ARM_FFA_TRANSPORT=y

# Already in our defconfig
CONFIG_GIC_V3=y
CONFIG_ARM_ARCH_TIMER=y
```

### Kernel Command Line

```
kvm-arm.mode=protected console=ttyAMA0 earlycon
```

---

## 8. Integration Gaps

### What Our SPMC Already Supports

- FFA_VERSION/ID_GET/SPM_ID_GET/FEATURES handshake
- PARTITION_INFO_GET with 24-byte descriptors to NWd RX buffer
- DIRECT_REQ/RESP routing to SPs (multi-SP: SP Hello + SP IRQ)
- NWd RXTX management (SPMD forwards RXTX_MAP from NWd)
- NS interrupt preemption (FFA_INTERRUPT → FFA_RUN resume)
- Secure Stage-2 for SP isolation (per-SP VSTTBR_EL2)
- Virtual IRQ/FIQ injection (HCR_EL2.VI/VF, HF_INTERRUPT_GET/HF_FIQ_GET)

### What Needs Work

| Gap | pKVM Sends | Our SPMC Receives | Effort | Priority |
|-----|-----------|-------------------|--------|----------|
| **NWd MEM_SHARE** | `do_ffa_mem_xfer(FFA_FN64_MEM_SHARE)` → SPMD | Not handled (stub only) | Medium | P0 |
| **NWd MEM_LEND** | `do_ffa_mem_xfer(FFA_FN64_MEM_LEND)` → SPMD | Not handled | Medium | P1 |
| **NWd MEM_RECLAIM** | `do_ffa_mem_reclaim()` → SPMD | Not handled | Low | P0 |
| **Fragmented MEM_FRAG_TX** | `do_ffa_mem_frag_tx()` → SPMD | Not implemented | Medium | P2 |
| **MEM_RETRIEVE_REQ** (from SPMC to SPMD) | N/A (SPMC-initiated) | Not needed initially | Low | P3 |

### What Should Just Work

| FF-A Call | pKVM Action | SPMD Action | Our SPMC Action | Status |
|---|---|---|---|---|
| FFA_VERSION | `do_ffa_version()` → forward | Return cached version | Return via FFA_MSG_WAIT | Done |
| FFA_FEATURES | `do_ffa_features()` → forward unhandled | Forward to SPMC | `dispatch_ffa()` | Done |
| FFA_PARTITION_INFO_GET | `do_ffa_part_get()` → forward, copy RX | Forward to SPMC | Write descriptors to NWd RX | Done |
| FFA_RXTX_MAP (host) | `do_ffa_rxtx_map()` → intercept | Forward NWd RXTX_MAP to SPMC | `handle_rxtx_map()` NWd state | Done |
| FFA_DIRECT_REQ | Pass through | Forward to SPMC | `dispatch_to_sp()` → ERET | Done |
| FFA_RUN | Pass through | Forward to SPMC | `resume_preempted_sp()` | Done |

---

## 9. Risk Matrix

| # | Risk | Probability | Impact | Severity | Mitigation |
|---|------|-------------|--------|----------|------------|
| R1 | pKVM `kvm-arm.mode=protected` silently ignored (VHE active) | High | Critical | **Critical** | `-cpu max,vh=off` or `id_aa64mmfr1.vh=0`. Verify: `dmesg \| grep "Protected nVHE"` |
| R2 | pKVM `__pkvm_init` hangs on QEMU TCG | Medium | Critical | **High** | Start with `kvm-arm.mode=nvhe` (no host S2). Debug with GDB at `__pkvm_init` |
| R3 | QEMU TCG bugs in pKVM deprivilege flow (host S2 setup) | Medium | High | **High** | Compare TCG register state vs FVP. Check VTTBR_EL2 after deprivilege |
| R4 | pKVM's FF-A proxy version negotiation conflict | Low | High | **Medium** | Both use FF-A v1.1. pKVM negotiates up to v1.2 — our SPMC returns 1.1 |
| R5 | NWd MEM_SHARE not implemented in SPMC | Certain | Medium | **Medium** | Implement in `spmc_handler.rs`. Low complexity — similar to stub SPMC |
| R6 | Fragmented FF-A descriptors from pKVM | Medium | Medium | **Medium** | Initially limit to single-fragment. Add FFA_MEM_FRAG_TX/RX later |
| R7 | pKVM intercepts PSCI CPU_ON — secondary core world switch | Low | Medium | **Low** | SPMD handles multi-core context switch. Our SPMC is single-core for now |
| R8 | Performance (triple software address translation) | Certain | Low | **Low** | Accept TCG speed. Dev/test only — real deployment uses KVM hardware |
| R9 | SPMD context save/restore misses EL2 registers | Low | Critical | **Medium** | Our TF-A build has `CTX_INCLUDE_EL2_REGS=1`. Verify with GDB |
| R10 | pKVM blocks FFA_NOTIFICATION_* — our SPMC advertises them | Low | Low | **Low** | No impact — notifications are optional. pKVM simply won't call them |

---

## 10. Recommended Approach

### Phase 1: pKVM Kernel Boot (1-2 weeks)

1. Add `CONFIG_KVM=y` + pKVM config to our 6.12.12 kernel build
2. New Makefile target: `make run-pkvm` (builds flash + pKVM kernel)
3. Boot with `kvm-arm.mode=nvhe` first (simpler, no host Stage-2)
4. Verify: `dmesg | grep kvm` shows KVM initialized at EL2
5. Upgrade to `kvm-arm.mode=protected`, verify deprivilege succeeds
6. Verify: `dmesg | grep "Protected nVHE"` confirms pKVM active

### Phase 2: FF-A End-to-End (2-3 weeks)

1. Verify: pKVM's `hyp_ffa_init()` → FFA_VERSION → SPMD → our SPMC
2. Verify: PARTITION_INFO_GET returns SP descriptors through dual-proxy chain
3. Verify: DIRECT_REQ from host → pKVM → SPMD → SPMC → SP1 → DIRECT_RESP
4. Implement: NWd MEM_SHARE handling in `spmc_handler.rs`
5. Verify: Host shares memory with SP, reclaims it

### Phase 3: Protected VM (stretch goal)

1. Run a guest VM under pKVM (nested: TCG → pKVM → guest)
2. Guest issues FF-A calls through pKVM proxy chain
3. End-to-end memory isolation verification

---

## 11. References

### pKVM FF-A Proxy
- [KVM: arm64: FF-A proxy for pKVM (LWN)](https://lwn.net/Articles/929560/)
- [pKVM FF-A proxy v1 patch series (Nov 2022)](https://lists.infradead.org/pipermail/linux-arm-kernel/2022-November/790498.html)
- [pKVM FF-A proxy v3 patch series (May 2023)](http://lists.openwrt.org/pipermail/linux-arm-kernel/2023-May/836094.html)
- [pKVM fragmented FF-A descriptors (Dec 2023)](https://www.spinics.net/lists/arm-kernel/msg1065659.html)
- [pKVM SMCCC 1.2 for FF-A (July 2025)](https://lkml.org/lkml/2025/7/30/1281)
- [FFA_RXTX_MAP handling in pKVM](https://lore.kernel.org/lkml/20221116170335.2341003-8-qperret@google.com/)
- [Separate hyp FF-A buffers init (Feb 2025)](https://lore.kernel.org/all/20250226214853.3267057-1-sebastianene@google.com/T/)
- [Linux ffa.c source (torvalds/linux)](https://github.com/torvalds/linux/blob/master/arch/arm64/kvm/hyp/nvhe/ffa.c)
- [Linux mem_protect.c source](https://github.com/torvalds/linux/blob/master/arch/arm64/kvm/hyp/nvhe/mem_protect.c)

### pKVM Architecture
- [KVM: arm64: Base support for pKVM (LWN)](https://lwn.net/Articles/895790/)
- [pKVM Preamble patches (April 2024)](https://lore.kernel.org/all/20240416095638.3620345-1-tabba@google.com/T/)
- [pKVM non-protected guest Stage-2 (Nov 2024)](https://patchew.org/linux/20241104133204.85208-1-qperret@google.com/)
- [KVM ARM64 Virtualization (DeepWiki)](https://deepwiki.com/torvalds/linux/3.2-kvm-arm64-virtualization)
- [AVF Architecture (Android)](https://source.android.com/docs/core/virtualization/architecture)
- [pKVM vendor modules (Android)](https://source.android.com/docs/core/virtualization/pkvm-modules)
- [Cambridge pKVM build notes (2020)](https://www.cl.cam.ac.uk/~pes20/Stuff/pkvm/notes19-2020-06-26-pkvm-build.html)

### pKVM Boot & VHE
- [kvm-arm.mode documentation (Oct 2024)](https://patchew.org/linux/20241025093259.2216093-1-smostafa@google.com/)
- [Ignore kvm-arm.mode=protected with VHE](https://www.spinics.net/lists/kvm-arm/msg47225.html)
- [kvm-unit-tests hang on FVP protected mode (Sept 2025)](https://lkml.org/lkml/2025/9/5/1043)

### TF-A SPMD
- [TF-A Secure Partition Manager (SPMD)](https://trustedfirmware-a.readthedocs.io/en/latest/components/secure-partition-manager.html)
- [TF-A EL3 SPMC](https://trustedfirmware-a.readthedocs.io/en/latest/components/el3-spmc.html)

### QEMU
- [QEMU ARM virt machine](https://www.qemu.org/docs/master/system/arm/virt.html)
- [QEMU A-profile CPU features](https://www.qemu.org/docs/master/system/arm/emulation.html)

### Our Project
- [Phase 4 Feasibility Research](docs/research/2026-02-20-phase4-feasibility.md)

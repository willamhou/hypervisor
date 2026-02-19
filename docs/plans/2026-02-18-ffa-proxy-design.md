# FF-A v1.1 Proxy Framework Design

**Date**: 2026-02-18
**Goal**: Implement FF-A v1.1 proxy at EL2, compatible with pKVM/AVF architecture, with stub SPMC for testing.
**Status**: Design approved

---

## 1. Architecture Overview

```
Guest VM (EL1)
    │ SMC #0 (FF-A function ID in x0)
    ▼
Hypervisor FF-A Proxy (EL2)
    ├─ FFA_VERSION / FFA_ID_GET / FFA_FEATURES → local handling
    ├─ FFA_RXTX_MAP / UNMAP → local (per-VM mailbox management)
    ├─ FFA_MEM_SHARE / LEND → validate page ownership → forward to stub SPMC
    ├─ FFA_MEM_RECLAIM → update ownership → forward to stub SPMC
    ├─ FFA_MSG_SEND_DIRECT_REQ → filter → forward to stub SPMC
    ├─ FFA_MEM_DONATE → reject (NOT_SUPPORTED, matches pKVM)
    └─ unknown function ID → NOT_SUPPORTED
         ▼
    Stub SPMC (in-hypervisor)
    ├─ handle allocation (memory sharing handles)
    ├─ simulated SPs (SP1=0x8001, SP2=0x8002)
    └─ direct message echo (for testing)
```

### Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| FF-A version | v1.1 (0x00010001) | pKVM baseline, covers all core interfaces |
| Proxy model | pKVM-compatible EL2 proxy | AVF/pKVM alignment, future Secure World integration |
| Page ownership | Stage-2 PTE SW bits [56:55] | Matches pKVM exactly, no extra memory |
| SPMC | Stub (in-hypervisor) | No EL3/S-EL2 in QEMU default; replaceable later |
| FFA_MEM_DONATE | Blocked | pKVM blocks this; too dangerous for proxy model |

---

## 2. SMC Interception

### Current State

- `HCR_EL2.TSC` is NOT set — guest SMC goes directly to EL3 firmware
- Only HVC (EC=0x16) is trapped; no EC_SMC64 (0x17) handling

### Changes

1. Set `HCR_EL2.TSC = 1` in `setup_el2()` to trap all guest EL1 SMC to EL2
2. Add `EC_SMC64 = 0x17` to `defs.rs`
3. Add `ExitReason::SmcCall` variant
4. Dispatch in `handle_exception()`:
   - PSCI range (`0x84000000-0x8400000F`, `0xC4000003`) → existing `handle_psci()`
   - FF-A range (`0x84000060-0x840000FF`, `0xC4000060-0xC40000FF`) → new `handle_ffa_call()`
   - Other → return `SMC_UNKNOWN` (-1) in x0

**SMC PC advancement**: Unlike HVC, SMC traps set ELR_EL2 to the SMC instruction itself. Must advance PC by 4 after handling.

**Multi-pCPU note**: `wake_secondary_pcpus()` issues SMC from EL2 — TSC only traps EL1 SMC, so no conflict.

---

## 3. FF-A Function ID Space (v1.1)

### Locally Handled

| Function | SMC32 ID | SMC64 ID | Action |
|----------|----------|----------|--------|
| FFA_ERROR | 0x84000060 | — | Return error codes |
| FFA_SUCCESS | 0x84000061 | 0xC4000061 | Return success |
| FFA_VERSION | 0x84000063 | — | Return 0x00010001 |
| FFA_FEATURES | 0x84000064 | — | Report supported features |
| FFA_RX_RELEASE | 0x84000065 | — | Release RX buffer ownership |
| FFA_RXTX_MAP | — | 0xC4000066 | Map per-VM TX/RX buffers |
| FFA_RXTX_UNMAP | 0x84000067 | — | Unmap buffers |
| FFA_PARTITION_INFO_GET | 0x84000068 | — | Discover SPs (from stub) |
| FFA_ID_GET | 0x84000069 | — | Return caller's partition ID |

### Validated + Forwarded to Stub SPMC

| Function | SMC32 ID | SMC64 ID | Action |
|----------|----------|----------|--------|
| FFA_MSG_SEND_DIRECT_REQ | 0x8400006F | 0xC400006F | Filter + forward |
| FFA_MSG_SEND_DIRECT_RESP | 0x84000070 | 0xC4000070 | Forward response |
| FFA_MEM_LEND | 0x84000072 | 0xC4000072 | Validate ownership → forward |
| FFA_MEM_SHARE | 0x84000073 | 0xC4000073 | Validate ownership → forward |
| FFA_MEM_RETRIEVE_REQ | 0x84000074 | 0xC4000074 | Forward to stub |
| FFA_MEM_RETRIEVE_RESP | 0x84000075 | — | Forward from stub |
| FFA_MEM_RELINQUISH | 0x84000076 | — | Forward to stub |
| FFA_MEM_RECLAIM | 0x84000077 | — | Update ownership → forward |
| FFA_MEM_FRAG_TX | 0x8400007B | — | Handle fragmented descriptors |
| FFA_MEM_FRAG_RX | 0x8400007A | — | Handle fragmented descriptors |

### Blocked

| Function | SMC32 ID | SMC64 ID | Reason |
|----------|----------|----------|--------|
| FFA_MEM_DONATE | 0x84000071 | 0xC4000071 | pKVM blocks this |

---

## 4. Page Ownership Tracking (Stage-2 PTE SW bits)

ARM Stage-2 PTE bits [56:55] encode software-defined page state:

| SW[1:0] (bits 56:55) | State | Meaning |
|-----------------------|-------|---------|
| `00` | `PAGE_OWNED` | VM exclusively owns this page |
| `01` | `PAGE_SHARED_OWNED` | VM owns, shared with Secure World via FF-A |
| `10` | `PAGE_SHARED_BORROWED` | Borrowed from another entity |
| `11` | `PAGE_DONATED` | Donated away (inaccessible) |

### State Transitions

```
FFA_MEM_SHARE:   OWNED ──→ SHARED_OWNED  (VM retains RW access)
FFA_MEM_LEND:    OWNED ──→ SHARED_OWNED  (optionally remove VM write)
FFA_MEM_RECLAIM: SHARED_OWNED ──→ OWNED  (restore full access)
```

### Security Invariant

A page in state `SHARED_OWNED` or `DONATED` CANNOT be:
- Shared again via FFA_MEM_SHARE/LEND (double-share blocked)
- Donated to another VM
- Used as RXTX buffer

### DynamicIdentityMapper Changes

New methods:
- `read_sw_bits(ipa: u64) -> Option<u8>` — walk page table to leaf PTE, extract bits [56:55]
- `write_sw_bits(ipa: u64, bits: u8)` — walk page table to leaf PTE, set bits [56:55]
- Only bits [56:55] used; bits [58:57] reserved for future use

---

## 5. Per-VM RXTX Buffers

```rust
pub struct FfaMailbox {
    tx_ipa: u64,       // Guest TX buffer IPA (guest writes, proxy reads)
    rx_ipa: u64,       // Guest RX buffer IPA (proxy writes, guest reads)
    page_count: u32,   // Buffer size in pages (typically 1)
    mapped: bool,      // Whether buffers are registered
    rx_owned: bool,    // RX buffer ownership (true = proxy owns, false = VM owns)
}
```

- Identity mapping: GPA == HPA, so proxy reads/writes guest buffers directly
- `FFA_RXTX_MAP`: Validate alignment (4KB), store IPAs, set `mapped = true`
- `FFA_RXTX_UNMAP`: Clear state, set `mapped = false`
- `FFA_RX_RELEASE`: Transfer RX ownership back to proxy (`rx_owned = true`)
- TX is used by guest for FFA_PARTITION_INFO_GET descriptors, FFA_MEM_SHARE descriptors
- RX is used by proxy to return results (partition info, memory retrieve response)

---

## 6. Stub SPMC

In-hypervisor module simulating Secure World responses. Replaceable with real SMC forwarding later.

### Simulated Partitions

| Partition ID | UUID | Name | Properties |
|--------------|------|------|------------|
| 0x8001 | `{12345678-...}` | "test-sp-1" | Direct messaging, memory sharing |
| 0x8002 | `{87654321-...}` | "test-sp-2" | Direct messaging only |

### Handle Allocation

- `AtomicU64` counter, monotonically increasing
- `FFA_MEM_SHARE/LEND` → allocate handle, store in `BTreeMap<u64, MemShareInfo>` (or fixed array)
- `FFA_MEM_RECLAIM` → validate handle exists, remove entry
- `MemShareInfo`: sender_id, receiver_id, handle, page_count, share_type (share/lend)

### Direct Message Echo

- `FFA_MSG_SEND_DIRECT_REQ(src, dst, x3, x4, x5, x6, x7)`:
  - Validate dst is a known SP (0x8001 or 0x8002)
  - Return `FFA_MSG_SEND_DIRECT_RESP(dst, src, x3, x4, x5, x6, x7)` — echo x4-x7 back

---

## 7. Partition ID Scheme

| Entity | Partition ID | Convention |
|--------|-------------|------------|
| Hypervisor / Host | 0x0000 | pKVM HOST_FFA_ID |
| VM 0 | 0x0001 | Normal world VM |
| VM 1 | 0x0002 | Normal world VM |
| Stub SP 1 | 0x8001 | Bit 15 set = secure partition |
| Stub SP 2 | 0x8002 | Bit 15 set = secure partition |

`FFA_ID_GET` returns the calling VM's partition ID (vm_id + 1 for VMs, 0 for hypervisor context).

---

## 8. File Structure

```
src/
├── ffa/
│   ├── mod.rs          // Module entry, FFA function ID constants, FfaReturn type
│   ├── proxy.rs        // handle_ffa_call() dispatch, SMC routing
│   ├── memory.rs       // Page ownership validation, state machine
│   ├── mailbox.rs      // Per-VM RXTX buffer management
│   └── stub_spmc.rs    // Stub SPMC: handle alloc, SP simulation, echo
```

### Integration Points

- `src/arch/aarch64/defs.rs` — add `EC_SMC64`, `HCR_TSC`
- `src/arch/aarch64/regs.rs` — add `ExitReason::SmcCall`
- `src/arch/aarch64/hypervisor/exception.rs` — SMC dispatch, PC+4
- `src/arch/aarch64/hypervisor/pagetable.rs` — `read_sw_bits()` / `write_sw_bits()`
- `src/lib.rs` — `pub mod ffa`

---

## 9. Test Plan

| Test | Description | Assertions |
|------|-------------|------------|
| `test_ffa_version` | Guest SMC → FFA_VERSION returns 0x00010001 | 1 |
| `test_ffa_id_get` | Returns caller VM partition ID | 1 |
| `test_ffa_features` | Query supported features | 2 |
| `test_ffa_rxtx_map` | Register TX/RX buffers, verify mapped | 2 |
| `test_ffa_partition_info` | Discover stub SPs via RXTX | 3 |
| `test_ffa_mem_share` | Share page: OWNED→SHARED_OWNED, reclaim→OWNED | 4 |
| `test_ffa_mem_donate_blocked` | FFA_MEM_DONATE returns NOT_SUPPORTED | 1 |
| `test_ffa_direct_msg` | Send direct req → receive echo resp | 2 |
| `test_page_ownership_bits` | SW bits read/write on Stage-2 PTE | 3 |

~19 new assertions across 9 test suites.

---

## 10. Future: Real Secure World Integration

When integrating TF-A (EL3) + Hafnium (S-EL2):

1. Replace `stub_spmc.rs` with real SMC forwarding (`smc #0` to EL3)
2. Proxy's RXTX becomes hypervisor-private buffers (separate from host's)
3. Page ownership validation remains unchanged
4. Add `FFA_SECONDARY_EP_REGISTER` for SP boot
5. Handle `FFA_INTERRUPT` for secure interrupt forwarding

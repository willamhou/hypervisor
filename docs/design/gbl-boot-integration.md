# Design: GBL (Generic Boot Loader) Integration

## 1. Background

### 1.1 What is GBL

GBL (Generic Boot Loader) is Google's standardized Android bootloader, mandated starting with Android 16 for ARM64 devices. It replaces vendor-specific bootloaders (ABL, LK, U-Boot) with a single Rust-based UEFI application (`BOOTAA64.EFI`), distributed as a single EFI binary that can be updated independently from the device's base firmware via standard Android OTA.

Key properties:
- **UEFI application**: runs on top of UEFI-compliant firmware
- **Written in Rust**: no_std, runs as EFI app
- **Replaces ABL/vendor bootloader**: NOT the firmware itself
- **Loads kernel, pvmfw, DTB**: from Android partitions (A/B slots)
- **Fastboot protocol**: built-in for flashing and debug
- **AVB (Android Verified Boot)**: full verification chain

### 1.2 Current Boot Flow (ARM TF-A)

Our current boot chain for pKVM + SPMC:

```
┌────────────────────────────────────────────────────────────────┐
│  EL3: BL1 (ROM) → BL2 (SRAM) → BL31 (SPMD, runtime EL3)     │
│        │              │                                        │
│        │    loads:     │                                        │
│        │    - BL32 (SPMC) into SEC_DRAM 0x0e100000             │
│        │    - SP packages into SEC_DRAM 0x0e300000+            │
│        │    - BL33 (kernel) into NS_DRAM 0x40200000            │
│        │                                                       │
│  S-EL2: BL32 (our SPMC)                                       │
│        - boot_sel2.S entry                                     │
│        - Stage-1 MMU (NS=1 for NWd DRAM)                      │
│        - Secure Stage-2 for SPs                                │
│        - Boot SP1, SP2 at S-EL1                                │
│        - FFA_MSG_WAIT → return to SPMD                         │
│                                                                │
│  NS-EL2: BL33 = Linux/pKVM kernel (loaded directly by BL2)    │
│        - pKVM init at EL2                                      │
│        - kernel deprivileges to EL1                            │
│        - FF-A driver probes SPMC via SPMD SMC relay            │
│                                                                │
│  NS-EL1: Linux/Android userspace                               │
└────────────────────────────────────────────────────────────────┘
```

TF-A BL2 currently does all the heavy lifting: it loads SPMC (BL32), Secure Partitions (from FIP via `sp_layout.json`), and the kernel (BL33) from FIP or preloaded memory.

### 1.3 GBL Boot Flow (Target)

```
┌────────────────────────────────────────────────────────────────┐
│  EL3: BL1 (ROM) → BL2 (SRAM) → BL31 (SPMD, runtime EL3)     │
│        │              │                                        │
│        │    loads:     │                                        │
│        │    - BL32 (SPMC) into SEC_DRAM 0x0e100000             │
│        │    - SP packages into SEC_DRAM 0x0e300000+            │
│        │    - BL33 (UEFI firmware) into NS_DRAM                │
│        │                                                       │
│  S-EL2: BL32 (our SPMC) — IDENTICAL, no changes               │
│        - Same init, same SPs, same FFA_MSG_WAIT                │
│                                                                │
│  NS-EL2: BL33 = UEFI firmware (EDK2 / U-Boot UEFI)           │
│        │                                                       │
│        ├→ UEFI Boot Services                                   │
│        │    - EFI_BLOCK_IO_PROTOCOL (storage)                  │
│        │    - EFI_RNG_PROTOCOL (entropy)                       │
│        │    - Memory allocation services                       │
│        │                                                       │
│        ├→ GBL (BOOTAA64.EFI from android_esp_{a,b})            │
│        │    - AVB: verify boot images                          │
│        │    - Load kernel Image from boot partition            │
│        │    - Load pvmfw from pvmfw partition                  │
│        │    - Load DTB, bootconfig, vendor_boot               │
│        │    - Construct kernel cmdline                         │
│        │    - ExitBootServices()                               │
│        │    - Jump to kernel at EL2                            │
│        │                                                       │
│        ├→ pKVM kernel (loaded by GBL)                          │
│        │    - pKVM init at EL2                                 │
│        │    - Loads pvmfw into protected memory                │
│        │    - kernel deprivileges to EL1                       │
│        │    - FF-A driver probes SPMC via SPMD                 │
│        │                                                       │
│  NS-EL1: Linux/Android userspace                               │
└────────────────────────────────────────────────────────────────┘
```

## 2. Impact Analysis

### 2.1 What Changes

| Component | Current | With GBL | Impact |
|-----------|---------|----------|--------|
| BL33 | kernel Image directly | UEFI firmware | BL33 is UEFI, not kernel |
| Kernel loading | BL2 loads from FIP/preloaded | GBL loads from partition | No impact on SPMC |
| pvmfw loading | Not present | GBL loads pvmfw | New component, no SPMC impact |
| DTB | QEMU auto-generated / BL2 | GBL constructs + patches | DTB content may differ |
| Boot partition | N/A | A/B android_esp FAT32 | Storage layout change |
| Fastboot | N/A | Built into GBL | Debug/flash interface |
| AVB | N/A | GBL verifies kernel | Integrity chain |

### 2.2 What Does NOT Change

| Component | Reason |
|-----------|--------|
| **BL1 → BL2 → BL31 (SPMD)** | GBL does not replace firmware |
| **BL32 (our SPMC)** | Loaded by BL2 before GBL runs |
| **SP packages (SP1, SP2)** | Loaded by BL2 from FIP |
| **FFA_MSG_WAIT handshake** | SPMC init completes before NS world |
| **FF-A interface** | Standard, bootloader-agnostic |
| **SPMD SMC relay** | BL31 runtime, independent of bootloader |
| **S-EL2 Stage-1 MMU** | SPMC internal, no NS dependency |
| **Secure Stage-2 for SPs** | SPMC internal |

### 2.3 Summary

```
SPMC 代码改动量: 零

GBL 替换的是 NS world 的 bootloader 层,
我们的 SPMC 在 Secure world BL32 阶段初始化完成后
通过 FFA_MSG_WAIT 交还控制权,之后 NS world 用什么
bootloader 加载 kernel 对 SPMC 完全透明。
```

## 3. Architecture Layers

### 3.1 Privilege Level Mapping

```
Exception Level   Current               With GBL
─────────────────────────────────────────────────────────
EL3               TF-A BL31 + SPMD      TF-A BL31 + SPMD     (identical)
S-EL2             Our SPMC (BL32)       Our SPMC (BL32)       (identical)
S-EL1             SP1, SP2              SP1, SP2              (identical)
NS-EL2            pKVM kernel           UEFI→GBL→pKVM kernel  (boot path differs)
NS-EL1            Linux/Android         Linux/Android         (identical at runtime)
```

### 3.2 Boot Timeline

```
Phase   EL     Current                    With GBL
──────────────────────────────────────────────────────────────────
  1     EL3    BL1 cold boot              BL1 cold boot
  2     EL3    BL2 loads images           BL2 loads images
              (BL32+SPs+BL33)            (BL32+SPs+BL33=UEFI)
  3     S-EL2  SPMC init, boot SPs        SPMC init, boot SPs
  4     S-EL2  FFA_MSG_WAIT               FFA_MSG_WAIT
  5     EL3    SPMD returns to BL2        SPMD returns to BL2
  6     EL3    BL31 jumps to BL33         BL31 jumps to BL33
  ── divergence point ──────────────────────────────────────
  7a    NS-EL2 kernel starts at EL2       UEFI firmware starts
  8a    -      -                          GBL.EFI loaded from ESP
  9a    -      -                          GBL loads kernel+pvmfw+DTB
 10a    -      -                          GBL ExitBootServices()
 11a    -      -                          GBL jumps to kernel at EL2
  ── converge ──────────────────────────────────────────────
  7b    NS-EL2 pKVM init                  pKVM init
  8b    NS-EL1 kernel deprivileges        kernel deprivileges
  9b    NS-EL1 FF-A driver probes SPMC    FF-A driver probes SPMC
```

Phase 3-4 (our SPMC) is complete before divergence at Phase 7. The SPMC has no visibility into what happens in NS-EL2 during Phases 7a-11a.

## 4. New Component: pvmfw

GBL introduces **pvmfw** (protected VM firmware) loading, which is relevant to our architecture:

```
pvmfw = Protected Virtual Machine Firmware
- Loaded by GBL (previously by ABL) from the pvmfw partition
- Placed in memory by the bootloader
- pKVM protects it via Stage-2 page tables
- Runs as the first code in a protected VM (pVM) before guest kernel
- Validates guest kernel, establishes trust chain
- Does NOT interact with SPMC directly
```

pvmfw is a pKVM/AVF concept for protected VMs. It is orthogonal to our Secure world — pvmfw lives entirely in NS world, managed by pKVM at NS-EL2. No FF-A interaction is involved.

## 5. QEMU Simulation Strategy

### 5.1 Current QEMU Setup

```bash
# BL33 = kernel directly
make build-tfa-pkvm    # ARM_LINUX_KERNEL_AS_BL33=1
make run-pkvm          # QEMU -bios flash-pkvm.bin
```

### 5.2 GBL on QEMU — Options

#### Option A: UEFI firmware as BL33 (full chain)

```
BL1 → BL2 → BL31(SPMD) → BL32(SPMC) → BL33(EDK2/U-Boot UEFI)
                                              ↓
                                         GBL (EFI app)
                                              ↓
                                         pKVM kernel
```

Requirements:
- Build EDK2 or U-Boot with UEFI for `virt` machine
- Create android_esp FAT32 disk image with GBL binary
- Create boot partition with kernel + DTB
- TF-A BL33 = UEFI firmware binary

Pros: Matches real device boot flow exactly.
Cons: Complex setup (EDK2 build + partition images + GBL binary).

#### Option B: U-Boot as UEFI firmware (simpler)

```
BL33 = U-Boot (UEFI mode)
       ↓
  GBL BOOTAA64.EFI from virtio-blk FAT32
       ↓
  kernel Image
```

U-Boot has mature QEMU virt support and can serve as UEFI firmware. This is the approach documented by BayLibre for GBL development.

#### Option C: Keep current setup, defer GBL (recommended for now)

```
BL33 = kernel directly (ARM_LINUX_KERNEL_AS_BL33)
```

Rationale:
- Our SPMC code is **unaffected** by GBL
- GBL integration is a NS-world bootloader concern
- Current focus is SPMC correctness + FF-A compliance
- GBL can be added later without any SPMC changes

### 5.3 Recommendation

**Phase 4.5 (current)**: Keep `ARM_LINUX_KERNEL_AS_BL33` — kernel loaded directly by BL2. Focus on fixing pKVM FF-A issues (secondary CPU boot, PARTITION_INFO_GET).

**Phase 5+ (future)**: When targeting real device flow or Android CTS, integrate GBL:
1. Build U-Boot UEFI for QEMU virt
2. Build GBL from AOSP source
3. Create FAT32 ESP image with GBL
4. Update TF-A BL33 to U-Boot
5. No SPMC changes needed

## 6. UEFI Firmware Requirements (for GBL)

If/when we integrate GBL, the UEFI firmware (BL33) must provide:

| Protocol | Purpose |
|----------|---------|
| `EFI_BLOCK_IO_PROTOCOL` | Read kernel/pvmfw from storage |
| `EFI_RNG_PROTOCOL` | KASLR seed, stack canaries |
| `EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL` | Console logging |
| `GBL_EFI_AVB_PROTOCOL` | Public keys, rollback indexes |
| `GBL_EFI_BOOT_CONTROL_PROTOCOL` | A/B slot metadata |
| `GBL_EFI_AVF_PROTOCOL` | AVF config from DICE chain |
| Memory Allocation Services | AVB + DICE computations |

Storage layout:
- Two FAT32 partitions: `android_esp_a`, `android_esp_b` (4MB each min)
- GBL at `/EFI/BOOT/BOOTAA64.EFI` within each ESP
- Partition type GUID: `C12A7328-F81F-11D2-BA4B-00A0C93EC93B`

## 7. FF-A Interaction Comparison

### 7.1 Current (kernel as BL33)

```
pKVM kernel boots at NS-EL2
  → kvm_arm_init() → hyp_ffa_init()
    → SMC FFA_VERSION → SPMD → SPMC (returns 1.1)
    → SMC FFA_FEATURES → SPMD → SPMC
    → SMC FFA_RXTX_MAP → SPMD (registers NWd RXTX)
    → SMC FFA_PARTITION_INFO_GET → SPMD → SPMC
      → SPMC writes descriptors to NWd RX buffer (NS DRAM via Stage-1 NS=1)
    → FF-A driver registers SP partitions
```

### 7.2 With GBL

```
UEFI firmware boots at NS-EL2
  → GBL runs as EFI app
    → GBL loads kernel + pvmfw + DTB
    → ExitBootServices()
    → Jump to kernel at EL2
  → pKVM kernel boots at NS-EL2
    → EXACTLY SAME FF-A flow as above
```

The FF-A interaction is **identical** — it happens after kernel boot, not during bootloader execution. GBL does not make FF-A calls.

## 8. Future Considerations

### 8.1 DRTM (Dynamic Root of Trust for Measurement)

Android is exploring DRTM for measured boot. If adopted:
- DRTM uses PSCI or platform-specific mechanism to establish trust
- May involve Secure world measurements
- Could require SPMC to participate in attestation
- **Not yet standardized for FF-A**

### 8.2 pvmfw and Secure World

Currently pvmfw has no Secure world interaction. However, future Android versions may:
- Use FF-A for pvmfw ↔ SP communication (attestation)
- Require SPMC to expose attestation SP
- This would require new SP development, not SPMC core changes

### 8.3 GBL as UEFI Runtime

Post-ExitBootServices, GBL's UEFI runtime services may still be available. These could potentially make SMC calls. Our SPMC should handle:
- Unexpected FF-A VERSION calls (already handled)
- Multiple RXTX_MAP cycles (if UEFI runtime re-registers)
- Currently not a concern but worth monitoring

## 9. Conclusion

| Question | Answer |
|----------|--------|
| Does GBL affect our SPMC? | **No** |
| Do we need code changes? | **No** |
| Should we integrate GBL now? | **No** — defer to Phase 5+ |
| Is GBL required for correctness testing? | **No** — kernel-as-BL33 is functionally identical for FF-A |
| When should we integrate GBL? | When targeting real device flow or Android CTS compliance |

The key insight is that GBL replaces the **Normal World bootloader** (ABL/vendor-specific), while our SPMC lives in the **Secure World** (BL32). The Secure World boot chain (BL2 → BL32 → SPs → FFA_MSG_WAIT) completes entirely before GBL runs. The FF-A interface between pKVM and SPMC is bootloader-agnostic.

## References

- [GBL Overview (AOSP)](https://source.android.com/docs/core/architecture/bootloader/generic-bootloader)
- [Deploy GBL (AOSP)](https://source.android.com/docs/core/architecture/bootloader/generic-bootloader/gbl-dev)
- [AVF Architecture (AOSP)](https://source.android.com/docs/core/virtualization/architecture)
- [TF-A Firmware Design](https://trustedfirmware-a.readthedocs.io/en/stable/design/firmware-design.html)
- [TF-A Secure Partition Manager](https://trustedfirmware-a.readthedocs.io/en/latest/components/secure-partition-manager.html)
- [Android Bootflow with U-Boot and GBL (BayLibre)](https://baylibre.com/android-bootflow-experiments-with-u-boot-and-gbl/)
- [Android Generic Boot Loader (LPC 2024)](https://lpc.events/event/18/contributions/1704/)

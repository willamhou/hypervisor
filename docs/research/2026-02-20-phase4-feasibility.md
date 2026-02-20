# Phase 4 Feasibility Research: TF-A + QEMU secure=on + S-EL2 SPMC

**Date**: 2026-02-20
**Status**: FEASIBLE with moderate effort
**Prerequisite**: QEMU upgrade (6.2 -> 9.2+)

---

## 1. QEMU S-EL2 Support

### FEAT_SEL2 Timeline
- **QEMU 6.0** (Apr 2021): Added ARMv8.4-SEL2 system registers, MMU Stage-1 for S-EL2, secure Stage-2
- **QEMU 10.0** (Feb 2025): Added S-EL2 physical and virtual timers (backported to 9.2.3)

### Command Line
```bash
qemu-system-aarch64 \
    -machine virt,secure=on,virtualization=on \
    -cpu max \
    -smp 4 \
    -m 2G \
    -bios flash.bin \
    -nographic
```

Key flags:
- `secure=on`: Enables TrustZone (EL3 + secure world). **Forces TCG** (no KVM).
- `virtualization=on`: Enables EL2 (both NS-EL2 and S-EL2 when combined with secure=on).
- `-cpu max`: Enables all features including FEAT_SEL2.

### Exception Levels Available

| Level | World | Available | Notes |
|-------|-------|-----------|-------|
| EL3 | Monitor | Yes | `secure=on` enables |
| S-EL2 | Secure Hypervisor | Yes | FEAT_SEL2 + both flags |
| S-EL1 | Secure OS | Yes | Below S-EL2 |
| NS-EL2 | Normal Hypervisor | Yes | `virtualization=on` |
| NS-EL1 | Normal OS | Yes | Guest kernel |

### GIC with secure=on
- GICv3 `has-security-extensions` property enabled
- Group 0 + Secure Group 1 interrupts available
- GICD registers gain security-aware behavior

### Known QEMU Bugs
1. **[Issue #1600](https://gitlab.com/qemu-project/qemu/-/issues/1600)**: Secure S1 translation for NS page resolves to secure IPA space (FEAT_SEL2)
2. **[Issue #1103](https://gitlab.com/qemu-project/qemu/-/issues/1103)**: VTCR fields not validated for S-EL2 page table walks
3. **[Issue #788](https://gitlab.com/qemu-project/qemu/-/issues/788)**: Incorrect PAuth trapping at S-EL0/1 with S-EL2 disabled

### Performance Impact
- TCG-only: 10-50x slower than KVM
- Current `make run` already uses TCG, so relative overhead is just world-switching emulation
- PAuth emulation (QARMA5) is particularly expensive

---

## 2. TF-A Build for QEMU with SPMD

### Build Command
```bash
make CROSS_COMPILE=aarch64-linux-gnu- \
     PLAT=qemu \
     SPD=spmd \
     SPMD_SPM_AT_SEL2=1 \
     CTX_INCLUDE_EL2_REGS=1 \
     ENABLE_FEAT_SEL2=1 \
     ARM_ARCH_MINOR=5 \
     BL32=/path/to/our-spmc.bin \
     BL33=/path/to/ns-payload.bin \
     QEMU_TOS_FW_CONFIG_DTS=/path/to/spmc_manifest.dts \
     SP_LAYOUT_FILE=/path/to/sp_layout.json \
     all fip
```

### Build Flags Explained

| Flag | Value | Purpose |
|------|-------|---------|
| `PLAT=qemu` | - | Target QEMU virt Armv8-A platform |
| `SPD=spmd` | - | Select SPMD at EL3 |
| `SPMD_SPM_AT_SEL2=1` | Default when SPD=spmd | Place SPMC at S-EL2 |
| `CTX_INCLUDE_EL2_REGS=1` | - | EL2 register context save/restore for S-EL2 |
| `ENABLE_FEAT_SEL2=1` | - | Enable FEAT_SEL2 |
| `ARM_ARCH_MINOR=5` | - | Target Armv8.5+ (required for FEAT_SEL2) |
| `BL32=<path>` | Our SPMC | Re-purposed for SPMC image |
| `BL33=<path>` | NS payload | Normal world payload |
| `QEMU_TOS_FW_CONFIG_DTS` | Manifest | SPMC manifest passed via x0 |
| `SP_LAYOUT_FILE` | JSON | SP packaging for FIP |

### Official Support Status
- TF-A docs only document FVP for SPD=spmd
- **OP-TEE's qemu_v8.mk** proves it works on QEMU in practice
- OP-TEE builds and tests SPMC at EL1, EL2, and EL3 on QEMU

### Flash Image Creation
```bash
dd if=/dev/zero of=flash.bin bs=1M count=64
dd if=build/qemu/release/bl1.bin of=flash.bin bs=4096 conv=notrunc
dd if=build/qemu/release/fip.bin of=flash.bin seek=64 bs=4096 conv=notrunc
```

---

## 3. Memory Layout (QEMU virt secure=on)

| Region | Address Range | Size | Purpose |
|--------|---------------|------|---------|
| Secure Flash | 0x00000000-0x04000000 | 64MB | BL1 + FIP |
| Flash1 | 0x04000000-0x08000000 | 64MB | Config data |
| GIC | 0x08000000-0x09000000 | - | Same as non-secure |
| UART | 0x09000000 | - | Same |
| **Secure SRAM** | 0x0e000000-0x0e100000 | 1MB | TF-A BL1/BL2 |
| **Secure DRAM** | 0x0e100000-0x0f000000 | 15MB | BL32 (our SPMC) |
| NS RAM | 0x40000000+ | - | Guest memory |

### Impact on Our Hypervisor
- S-EL2 mode: linker at `0x0e100000` (secure DRAM) instead of `0x40000000`
- Feature-gated: `#[cfg(feature = "sel2")]`
- 15MB secure DRAM is sufficient for SPMC + basic SPs

---

## 4. BL32 Entry Protocol (SPMD -> SPMC)

### Register Convention
| Register | Content |
|----------|---------|
| x0 | TOS_FW_CONFIG physical address (SPMC manifest DTB) |
| x1 | HW_CONFIG physical address (hardware DTB) |
| x4 | Core linear ID |

### SPMC Manifest Template
```dts
/dts-v1/;
/ {
    compatible = "arm,ffa-core-manifest-1.0";
    #address-cells = <2>;
    #size-cells = <2>;
    attribute {
        spmc_id = <0x8000>;
        maj_ver = <0x1>;
        min_ver = <0x1>;
        exec_state = <0x0>;       /* AArch64 */
        load_address = <0x0 0x0e100000>;
        entrypoint = <0x0 0x0e100000>;
        binary_size = <0x0 0x80000>;
    };
    memory@0e100000 {
        device_type = "memory";
        reg = <0x0 0x0e100000 0x0 0x00f00000>;
    };
};
```

### SP Layout JSON (for TF-A FIP packaging)
```json
{
    "test-sp": {
        "image": { "file": "sp.bin", "offset": "0x2000" },
        "pm": { "file": "sp_manifest.dts", "offset": "0x1000" },
        "owner": "SiP"
    }
}
```

### Secondary Core Boot
- SPMC calls `FFA_SECONDARY_EP_REGISTER` (0x84000087) to register secondary entry
- SPMD stores entry point and routes secondary cores during world switch

---

## 5. Key Adaptation Work

### What We Need to Build (Sprint 4.1-4.4)

1. **New entry point** (`boot_sel2.S`): Handle SPMD handoff (x0/x1/x4)
2. **Linker script**: `0x0e100000` for S-EL2 (feature-gated `sel2`)
3. **SPMC manifest DTS**: QEMU-specific, spmc_id=0x8000
4. **SPMD protocol compliance**:
   - FFA_VERSION handshake with SPMD (not guest)
   - FFA_FEATURES declaration
   - FFA_SECONDARY_EP_REGISTER for multi-core
5. **Secure Stage-2**: VSTTBR_EL2 instead of VTTBR_EL2
6. **BL33**: Simple bare-metal for initial testing

### Our Advantages
- FF-A v1.1 protocol already implemented (VERSION/ID_GET/FEATURES/RXTX/MEM_*/notifications)
- Stub SPMC → actual SPMC is natural evolution
- FFA_SPM_ID_GET already returns 0x8000 (matches manifest)
- TF-A SPMD treats BL32 as black box

---

## 6. Local Environment Status

| Tool | Status | Notes |
|------|--------|-------|
| QEMU | 6.2.0 (Ubuntu 22.04) | **NEEDS UPGRADE** to 9.2+ |
| `aarch64-linux-gnu-gcc` | Available | Cross-compiler for TF-A |
| `dtc` | Available | Device tree compiler |
| `aarch64-none-elf-gcc` | Missing | Not needed (use linux-gnu) |
| `fiptool` | Missing | Built from TF-A source |

### QEMU Upgrade Path
- Build from source: `git clone https://gitlab.com/qemu-project/qemu.git -b v9.2.3`
- Or install via PPA/snap for newer Ubuntu versions
- Need QEMU 9.2+ for S-EL2 timer support

---

## 7. Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| No KVM with secure=on | Medium | Accept TCG speed; dev/test only |
| QEMU S-EL2 S1 translation bug (#1600) | High | May need QEMU patch or workaround |
| VTCR validation bug (#1103) | Medium | Careful VTCR configuration |
| No precedent for custom Rust SPMC | Medium | Reference Hafnium source for protocol |
| Secure DRAM only 15MB | Low | Sufficient for SPMC |
| S-EL2 timers need QEMU 9.2+ | Low | Upgrade QEMU |

---

## 8. References

- [TF-A Secure Partition Manager (v2.14.0)](https://trustedfirmware-a.readthedocs.io/en/latest/components/secure-partition-manager.html)
- [TF-A QEMU virt Platform](https://trustedfirmware-a.readthedocs.io/en/latest/plat/qemu.html)
- [TF-A Build Options](https://trustedfirmware-a.readthedocs.io/en/stable/getting_started/build-options.html)
- [TF-A FF-A Manifest Binding](https://trustedfirmware-a.readthedocs.io/en/latest/components/ffa-manifest-binding.html)
- [OP-TEE build/qemu_v8.mk](https://github.com/OP-TEE/build/blob/master/qemu_v8.mk)
- [OP-TEE SPMC Architecture](https://optee.readthedocs.io/en/latest/architecture/spmc.html)
- [Hafnium SPM Documentation](https://hafnium.readthedocs.io/en/latest/secure-partition-manager/secure-partition-manager.html)
- [QEMU virt Machine Docs](https://www.qemu.org/docs/master/system/arm/virt.html)
- [QEMU ARM CPU Features](https://qemu-project.gitlab.io/qemu/system/arm/cpu-features.html)
- [QEMU Issue #1600 - FEAT_SEL2 S1 Translation](https://gitlab.com/qemu-project/qemu/-/issues/1600)
- [QEMU Issue #1103 - VTCR Validation](https://gitlab.com/qemu-project/qemu/-/issues/1103)
- [Shrinkwrap ffa-hafnium-optee](https://shrinkwrap.docs.arm.com/en/latest/userguide/configstore/ffa-hafnium-optee.html)
- [TF-A Tech Forum: SEL2 Hafnium (Jul 2020)](https://www.trustedfirmware.org/docs/TF-A_Tech_Forum_SEL2_Hafnium_Jul_2020_v0.4.pdf)
- [Linaro LVC21-305: Virtualising OP-TEE with Hafnium at S-EL2](https://static.linaro.org/connect/lvc21/presentations/lvc21-305.pdf)

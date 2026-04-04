# hypervisor

A bare-metal Type-1 hypervisor for ARM64, written in Rust (`no_std`). Boots Linux on QEMU, manages Secure Partitions at S-EL2, and integrates with Android pKVM.

Built from scratch — no Hafnium, no KVM, no runtime dependencies.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## What It Does

```
EL3  │ TF-A BL31 + SPMD          SMC relay, world switch
─────┼───────────────────────────────────────────────────
S-EL2│ This hypervisor (SPMC)     FF-A dispatch, SP lifecycle
S-EL1│ Secure Partitions          SP1 Hello, SP2 IRQ, SP3 Relay
─────┼───────────────────────────────────────────────────
NS-EL2│ pKVM                      Protected VM management
NS-EL1│ Linux / Android           Guest OS
```

- **S-EL2 SPMC**: Replaces Hafnium as Secure Partition Manager. Boots 3 SPs, handles FF-A messaging and memory sharing, manages Secure Stage-2 page tables
- **pKVM integration**: Runs alongside Android pKVM — our SPMC at S-EL2, pKVM at NS-EL2, both on 4 physical CPUs
- **Linux guest**: Boots Linux 6.12 to BusyBox shell with 4 vCPUs, virtio-blk, virtio-net, inter-VM networking
- **FF-A v1.1**: Full firmware framework — direct/indirect messaging, memory sharing (SHARE/LEND/DONATE), fragmentation, notifications, console log

## Quick Start

```bash
# Prerequisites: Rust nightly, qemu-system-aarch64, aarch64-linux-gnu-gcc
make run              # Run 34 test suites (457 assertions)
make run-linux        # Boot Linux guest (4 vCPUs, virtio-blk)
make run-spmc         # Boot SPMC at S-EL2 (20/20 BL33 tests)
make run-pkvm-ffa-test # Boot pKVM + SPMC (35/35 E2E tests)
```

Add `RELEASE=1` for optimized builds (15-20% smaller binary).

## Test Results

| Target | Tests | Status |
|--------|-------|--------|
| `make run` | 34 suites, 457 assertions | All pass |
| `make run-spmc` | 20 BL33 integration tests | All pass |
| `make run-pkvm-ffa-test` | 35 pKVM E2E tests | All pass |
| `make run-linux` | Linux 6.12 → BusyBox shell | Boots |
| `make run-multi-vm` | 2 Linux VMs, inter-VM ping | Boots |

## Features

### Virtualization
- Stage-2 MMU with dynamic 2MB/4KB pages, VMID-tagged TLBs
- GICv3 full trap-and-emulate (GICD + GICR shadow state, List Register injection)
- SMP: 4 vCPUs cooperative + preemptive scheduling (10ms CNTHP timer)
- Multi-pCPU: 1:1 vCPU-to-pCPU affinity, PSCI boot, physical IPI
- Multi-VM: 2 VMs time-sliced, per-VM Stage-2/devices, L2 virtual switch

### FF-A v1.1 (Firmware Framework for Arm)
- Direct messaging (DIRECT_REQ/RESP, 32-bit and 64-bit)
- Indirect messaging (MSG_SEND2/MSG_WAIT, per-SP mailbox)
- Memory sharing (MEM_SHARE/LEND/DONATE/RETRIEVE/RELINQUISH/RECLAIM)
- SP-to-SP: DIRECT_REQ routing with CallStack cycle detection
- SP-to-SP: MEM_SHARE/RECLAIM between Secure Partitions
- MEM_DONATE: irrevocable ownership transfer
- Descriptor fragmentation (FRAG_TX/FRAG_RX)
- Notifications (BIND/SET/GET/INFO_GET)
- Page ownership via Stage-2 PTE software bits (pKVM-compatible)
- CONSOLE_LOG, SRI/NPI feature IDs

### S-EL2 SPMC
- TF-A boot chain: BL1 → BL2 → BL31(SPMD) → BL32(SPMC) → BL33
- 3 Secure Partitions: SP1 (Hello), SP2 (IRQ handler), SP3 (Relay)
- Per-SP Secure Stage-2 isolation
- NS interrupt preemption: IRQ during SP → FFA_INTERRUPT → FFA_RUN resume
- Secure vIRQ injection: HCR_EL2.VI + HF_INTERRUPT_GET (Hafnium-compatible)
- Cross-SP preemption, chain preemption (Blocked → Preempted)
- S-EL2 Stage-1 MMU: NS=1 for NWd DRAM access
- NWd RXTX management, PARTITION_INFO_GET forwarding

### Device Emulation
- PL011 UART (TX + RX with ring buffer)
- PL031 RTC (counter-based, PrimeCell ID)
- Virtio-blk (in-memory disk, virtio-mmio transport)
- Virtio-net + VSwitch (L2 switch, MAC learning, auto-IP)

## Architecture

```
src/
├── arch/aarch64/
│   ├── boot.S / boot_sel2.S      Entry point (NS-EL2 / S-EL2)
│   ├── exception.S               Exception vectors, context save/restore
│   └── hypervisor/exception.rs   ESR_EL2 decode, MMIO routing
├── ffa/
│   ├── proxy.rs                  FF-A v1.1 proxy (NS-EL2 mode)
│   ├── stage2_walker.rs          Page ownership via PTE SW bits
│   ├── descriptors.rs            Memory region descriptor parsing
│   └── smc_forward.rs            SMC forwarding to EL3
├── devices/                      PL011, PL031, GIC, virtio-blk/net
├── spmc_handler.rs               S-EL2 SPMC event loop + FF-A dispatch
├── sp_context.rs                 Per-SP state machine, INTID ownership
├── vm.rs / vcpu.rs               VM lifecycle, vCPU state machine
├── scheduler.rs                  Round-robin vCPU scheduler
├── vswitch.rs                    L2 virtual switch
└── sel2_mmu.rs                   S-EL2 Stage-1 identity map

tfa/
├── sp_hello/start.S              SP1: echo + memory test + share/reclaim
├── sp_irq/start.S                SP2: vIRQ handler + DIRECT_REQ
├── sp_relay/start.S              SP3: SP-to-SP DIRECT_REQ relay
└── bl33_ffa_test/start.S         BL33 integration test client (20 tests)

guest/linux/ffa-test/ffa_test.c   pKVM kernel test module (35 tests)
```

~30,000 lines (26K Rust + 3.4K assembly).

## Build Targets

```bash
# Guest boot
make run-linux          # 4 vCPUs on 1 pCPU, virtio-blk
make run-linux-smp      # 4 vCPUs on 4 pCPUs
make run-multi-vm       # 2 Linux VMs, VSwitch networking
make run-android        # Android kernel (Binder, PL031 RTC, 1GB RAM)

# S-EL2 SPMC (requires TF-A build)
make build-tfa-spmc     # Build TF-A + SPMC + SPs
make run-spmc           # 20/20 BL33 integration tests

# pKVM integration (requires AOSP kernel)
make build-pkvm-kernel  # Build android16-6.12 kernel
make build-tfa-pkvm     # Build TF-A + SPMC for pKVM
make run-pkvm           # pKVM boot to shell
make run-pkvm-ffa-test  # 35/35 E2E FF-A tests

# Development
make run                # Unit tests (34 suites)
make clippy             # Lint
make fmt                # Format
make debug              # QEMU + GDB on port 1234
```

## How It Differs

| | This project | Hafnium | KVM/ARM |
|---|---|---|---|
| Language | Rust (no_std) | C | C (kernel) |
| Role | S-EL2 SPMC | S-EL2 SPMC | NS-EL2 hypervisor |
| Boots Linux | Yes (NS-EL1) | No | Yes |
| FF-A | v1.1 (messaging + memory + notifications) | v1.1 | v1.0 proxy |
| pKVM coexistence | Yes (S-EL2 + NS-EL2) | Yes | N/A (is pKVM) |
| SP-to-SP messaging | Yes (CallStack, cycle detection) | Yes | No |
| Memory safety | Rust ownership + no_std | Manual C | Kernel facilities |
| Target | Education + research | Production | Production |

## Roadmap

- [x] NS-EL2 hypervisor — Linux boot, GIC, virtio, multi-VM
- [x] FF-A v1.1 proxy — messaging, memory sharing, notifications
- [x] S-EL2 SPMC — TF-A boot chain, 3 SPs, Secure Stage-2
- [x] pKVM integration — 4-CPU SMP, 35/35 E2E tests
- [x] SP-to-SP — DIRECT_REQ relay, MEM_SHARE/RECLAIM/DONATE
- [x] Security hardening — cross-SP isolation, IPA validation, stress tests
- [ ] **RME & CCA** — Realm Management Extension, Confidential Compute

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) for details.

## References

- [ARM Architecture Reference Manual (ARMv8-A)](https://developer.arm.com/documentation/ddi0487/latest)
- [ARM FF-A v1.1 Specification (DEN0077A)](https://developer.arm.com/documentation/den0077/latest)
- [ARM GIC Architecture Specification](https://developer.arm.com/documentation/ihi0069/latest)
- [TF-A Documentation](https://trustedfirmware-a.readthedocs.io/)
- [pKVM (Protected KVM)](https://source.android.com/docs/core/virtualization)

## License

MIT

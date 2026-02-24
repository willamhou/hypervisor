# Architecture: ARM64 Exception Levels

## The Privilege Model

ARM64 defines four exception levels, each a strict privilege boundary:

```
EL3  ─  Secure Monitor (firmware, TrustZone)
EL2  ─  Hypervisor (our code lives here)
EL1  ─  OS kernel (Linux, guest)
EL0  ─  Userspace (applications)
```

Higher EL = more privilege. Code at EL2 can configure hardware traps that intercept EL1 operations — this is the fundamental mechanism of hardware virtualization. When a guest at EL1 executes a sensitive instruction (access a system register, execute WFI, trigger an SMC), the hardware traps to EL2 where the hypervisor decides what to do.

## Why EL2?

EL2 is purpose-built for hypervisors. It provides:

- **Stage-2 translation** (`VTTBR_EL2`, `VTCR_EL2`) — a second layer of address translation that maps guest physical addresses (IPAs) to real physical addresses. The guest thinks it owns memory starting at 0x40000000; the hypervisor controls what's really there.
- **Trap configuration** (`HCR_EL2`) — a single 64-bit register that controls which guest operations trap to EL2. Want to intercept WFI? Set TWI. Want to trap SMC? Set TSC. Want to trap all system register access? There's a bit for that too.
- **Virtual interrupt injection** (`ICH_LR*_EL2`) — GICv3's virtual interface lets the hypervisor inject interrupts into the guest without modifying guest state directly.
- **VMID-tagged TLBs** (`VTTBR_EL2[63:48]`) — hardware TLB tagging so multiple VMs can coexist without flushing TLBs on every context switch.

None of this exists at EL1. A hypervisor that tried to run at EL1 would need software emulation for all of it — orders of magnitude slower.

## How QEMU Gets Us to EL2

On real hardware, the boot firmware (usually at EL3) configures HCR_EL2 and drops to EL2 before handing off to the hypervisor. We skip that complexity by using QEMU's `-machine virt` with specific flags:

```bash
qemu-system-aarch64 \
  -machine virt,virtualization=on \
  -cpu max \
  -nographic \
  -kernel hypervisor.bin
```

The key is `-machine virt,virtualization=on`. Without `virtualization=on`, QEMU starts the kernel at EL1. With it, QEMU's built-in firmware configures the CPU and enters our binary at EL2 directly.

We can verify this in `boot.S` by reading `CurrentEL`:

```armasm
mrs     x0, CurrentEL
lsr     x0, x0, #2    // Extract EL field (bits [3:2])
cmp     x0, #2        // Should be 2 (EL2)
b.ne    halt           // If not EL2, something is wrong
```

In the first version of `boot.S`, this check was present. It was later removed — if we're not at EL2, there's nothing useful we can do anyway, so silently hanging is acceptable.

## Key Registers We'll Use Later

For now, we don't touch any EL2 system registers. But here's a preview of what becomes important in later parts:

| Register | Purpose | First used in |
|----------|---------|---------------|
| `HCR_EL2` | Hypervisor Configuration Register — trap control | Part 2 (guest execution) |
| `VTTBR_EL2` | Stage-2 translation table base | Part 2 (memory) |
| `VTCR_EL2` | Stage-2 translation control | Part 2 (memory) |
| `VBAR_EL2` | Exception vector base | Part 3 (exceptions) |
| `ESR_EL2` | Exception syndrome (exit reason) | Part 3 (exceptions) |
| `ELR_EL2` | Exception link register (return address) | Part 3 (exceptions) |
| `SPSR_EL2` | Saved processor state | Part 2 (guest entry) |

In this part, we only need `CurrentEL` (to verify we're at EL2) and `SP` (the stack pointer, set up in assembly).

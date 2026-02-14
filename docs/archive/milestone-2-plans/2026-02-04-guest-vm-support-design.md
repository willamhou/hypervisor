# Guest VM Support Design

## Overview

支持加载真实的 ELF 二进制作为 guest 运行，目标是先支持 Zephyr RTOS，然后扩展到 Linux。

## Goals

1. **开发测试** - 验证 hypervisor 功能（中断注入、内存隔离等）
2. **性能评估** - 测量虚拟化开销
3. **实际部署** - 作为生产环境的 hypervisor 使用

## Architecture

### Memory Layout

```
┌─────────────────────────────────────────────────────────────┐
│                        QEMU                                 │
│  -kernel hypervisor.bin                                     │
│  -device loader,file=zephyr.elf,addr=0x48000000             │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                    Memory Layout                            │
├─────────────────────────────────────────────────────────────┤
│  0x0800_0000  GIC Distributor (GICD)                        │
│  0x0801_0000  GIC Redistributor (GICR) - GICv3              │
│  0x0900_0000  UART (PL011)                                  │
│  0x4000_0000  Hypervisor code + data                        │
│  0x4100_0000  Hypervisor heap (16MB)                        │
│  0x4800_0000  Guest load address ← QEMU loader              │
│  0x5000_0000  Guest memory end (128MB for guest)            │
└─────────────────────────────────────────────────────────────┘
```

### Boot Flow

1. QEMU 加载 hypervisor 到 `0x4000_0000`，guest 到 `0x4800_0000`
2. Hypervisor 初始化（GIC、heap、Stage-2 页表）
3. 创建 VM，映射 guest 内存区域
4. 创建 vCPU，设置入口点为 `0x4800_0000`
5. 进入 guest（eret 到 EL1）

### Stage-2 Memory Mapping

| IPA Range | PA Range | Attribute | Description |
|-----------|----------|-----------|-------------|
| `0x0800_0000 - 0x0900_0000` | Same | Device | GIC MMIO |
| `0x0900_0000 - 0x0A00_0000` | Same | Device | UART MMIO |
| `0x4800_0000 - 0x5000_0000` | Same | Normal | Guest RAM (128MB) |

Identity mapping (IPA == PA) for simplicity.

## Implementation

### New Files

```
src/
├── guest_loader.rs      [New] Guest configuration and boot logic
```

### Modified Files

```
src/
├── vm.rs                [Modify] Add guest memory mapping
├── main.rs              [Modify] Add guest boot path
Makefile                 [Modify] Add run-guest target
```

### Core Interface

```rust
// src/guest_loader.rs

/// Guest configuration
pub struct GuestConfig {
    /// Guest code load address
    pub load_addr: u64,        // 0x4800_0000
    /// Guest memory size
    pub mem_size: u64,         // 128MB
    /// Entry point (usually equals load_addr)
    pub entry_point: u64,
}

impl GuestConfig {
    /// Default Zephyr configuration
    pub const fn zephyr_default() -> Self {
        Self {
            load_addr: 0x4800_0000,
            mem_size: 128 * 1024 * 1024,
            entry_point: 0x4800_0000,
        }
    }
}

/// Boot a guest VM
pub fn run_guest(config: &GuestConfig) -> Result<(), &'static str>;
```

### Makefile Changes

```makefile
# Guest ELF path (environment variable)
GUEST_ELF ?=

# Run hypervisor with guest
run-guest: build
ifndef GUEST_ELF
	$(error GUEST_ELF is not set. Usage: make run-guest GUEST_ELF=/path/to/zephyr.elf)
endif
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(GUEST_ELF),addr=0x48000000
```

## Phased Implementation

### Phase 1: Minimal (UART only)
- Load guest to fixed address
- Map guest RAM and UART
- Guest can print "Hello World"

### Phase 2: Timer Interrupts
- Virtual timer support
- Timer interrupt injection via GICv3 LR
- Guest can run periodic tasks

### Phase 3: PSCI Support
- CPU power management
- Support CPU idle / hotplug

## Testing

### Phase 1 Test
```bash
# Build Zephyr hello_world for qemu_cortex_a53
west build -b qemu_cortex_a53 samples/hello_world

# Run with hypervisor
make run-guest GUEST_ELF=path/to/zephyr.elf
```

Expected output:
```
[HYPERVISOR] Starting guest at 0x48000000
*** Booting Zephyr OS ***
Hello World! qemu_cortex_a53
```

## Future Extensions

- Linux kernel support (requires device tree, initramfs)
- Multiple guest VMs
- Virtio devices

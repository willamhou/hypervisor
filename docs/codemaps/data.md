# Data Models Codemap

> Freshness: 2026-02-17 | Key structs, enums, and their relationships

## Core Domain Types

### VM Layer

```rust
// src/vm.rs
struct Vm {
    id: usize,                      // VM index (0..MAX_VMS)
    state: VmState,                 // lifecycle state machine
    vcpus: [Option<Vcpu>; 8],       // up to 8 vCPUs per VM
    vcpu_count: usize,
    memory_initialized: bool,
    scheduler: Scheduler,           // round-robin vCPU scheduler
    vttbr: u64,                     // Stage-2 base register
    vtcr: u64,                      // Stage-2 translation control
}

enum VmState { Uninitialized, Ready, Running, Paused, Stopped }
```

### vCPU Layer

```rust
// src/vcpu.rs
struct Vcpu {
    id: usize,
    state: VcpuState,
    context: VcpuContext,            // guest registers (shared with asm)
    virt_irq: VirtualInterruptState, // legacy IRQ/FIQ pending
    arch_state: VcpuArchState,       // full per-vCPU hardware state
}

enum VcpuState { Uninitialized, Ready, Running, Stopped }
```

### Register Context (ASM-shared, `#[repr(C)]`)

```rust
// src/arch/aarch64/regs.rs
struct VcpuContext {
    gp_regs: GeneralPurposeRegs,     // x0-x30 (31 × u64)
    sys_regs: SystemRegs,            // 17 EL1/EL2 system regs
    sp: u64,                         // guest SP_EL1
    pc: u64,                         // guest ELR_EL2
    spsr_el2: u64,                   // guest saved PSTATE
}

struct GeneralPurposeRegs { regs: [u64; 31] }

struct SystemRegs {
    sp_el1, elr_el1, spsr_el1,
    sctlr_el1, ttbr0_el1, ttbr1_el1, tcr_el1,
    mair_el1, vbar_el1, contextidr_el1,
    tpidr_el1, tpidrro_el0, tpidr_el0,
    esr_el2, far_el2, hcr_el2, cntvoff_el2: u64,
}

enum ExitReason {
    Unknown, WfiWfe, HvcCall, TrapMsrMrs,
    InstructionAbort, DataAbort, Other(u64),
}
```

### Per-vCPU Architecture State (42 fields)

```rust
// src/arch/aarch64/vcpu_arch_state.rs
struct VcpuArchState {
    // GICv3 virtual interface
    ich_lr: [u64; 4],               // 4 List Registers
    ich_vmcr: u64,                   // virtual machine control
    ich_hcr: u64,                    // hypervisor control

    // Timer
    cntv_ctl: u64,                   // virtual timer control
    cntv_cval: u64,                  // virtual timer compare value

    // Identity
    vmpidr: u64,                     // virtual MPIDR

    // EL1 system registers (23 regs)
    sctlr_el1, ttbr0_el1, ttbr1_el1, tcr_el1, mair_el1,
    vbar_el1, cpacr_el1, contextidr_el1, tpidr_el1,
    tpidrro_el0, tpidr_el0, par_el1, cntkctl_el1,
    sp_el1, elr_el1, spsr_el1, afsr0_el1, afsr1_el1,
    esr_el1, far_el1, amair_el1, mdscr_el1, sp_el0: u64,

    // PAC keys (10 regs)
    apiakeylo_el1, apiakeyhi_el1, apibkeylo_el1, apibkeyhi_el1,
    apdakeylo_el1, apdakeyhi_el1, apdbkeylo_el1, apdbkeyhi_el1,
    apgakeylo_el1, apgakeyhi_el1: u64,
}
// Methods: new(), init_for_vcpu(id), save(), restore()
```

## Device Emulation Types

### Device Dispatch (enum, no trait objects)

```rust
// src/devices/mod.rs
enum Device {
    Uart(VirtualUart),
    Gicd(VirtualGicd),
    Gicr(VirtualGicr),
    VirtioBlk(VirtioMmioTransport<VirtioBlk>),
}

struct DeviceManager {
    devices: [Option<Device>; 8],    // up to 8 devices
    count: usize,
}

trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> u32;
    fn write(&mut self, offset: u64, value: u32, size: u8);
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;
    fn contains(&self, addr: u64) -> bool;
    fn pending_irq(&self) -> bool;
    fn ack_irq(&mut self);
}
```

### GIC Emulation

```rust
// src/devices/gic/distributor.rs
struct VirtualGicd {
    ctlr: u32,                       // Distributor Control
    enabled: [u32; 32],              // ISENABLER (1024 INTIDs)
    igroupr: [u32; 32],             // Interrupt Group
    ipriorityr: [u32; 256],         // Priority (1024 INTIDs)
    icfgr: [u32; 64],              // Configuration
    ispendr: [u32; 32],            // Set-Pending
    isactiver: [u32; 32],          // Set-Active
    irouter: [u64; 988],           // Routing (SPI 32-1019)
    num_cpus: u32,
}

// src/devices/gic/redistributor.rs
struct VirtualGicr {
    state: [GicrState; 8],          // per-vCPU state
    num_vcpus: usize,
}
struct GicrState {
    ctlr: u32, waker: u32, igroupr0: u32,
    isenabler0: u32, ispendr0: u32, isactiver0: u32,
    ipriorityr: [u32; 8], icfgr: [u32; 2],
}
```

### Virtio-blk

```rust
// src/devices/virtio/mmio.rs
struct VirtioMmioTransport<T: VirtioDevice> {
    device: T,
    status: u32, device_features_sel: u32,
    driver_features: u64, driver_features_sel: u32,
    queue_sel: u32, queue_num: u32, queue: Virtqueue,
    interrupt_status: u32, config_generation: u32,
}

// src/devices/virtio/queue.rs
struct Virtqueue {
    desc_addr: u64, avail_addr: u64, used_addr: u64,
    size: u16, last_avail_idx: u16, ready: bool,
}

// src/devices/virtio/blk.rs
struct VirtioBlk {
    capacity: u64,                   // sectors
    disk_base: u64,                  // physical address of in-memory image
    disk_size: u64,                  // bytes
}
```

### UART Emulation

```rust
// src/devices/pl011/emulator.rs
struct VirtualUart {
    cr: u32, lcr_h: u32, ibrd: u32, fbrd: u32,
    ifls: u32, imsc: u32, ris: u32, dmacr: u32,
    rx_buf: [u8; 64],               // RX FIFO
    rx_head: usize, rx_tail: usize,
}
```

## Global State Types

```rust
// src/global.rs

// Per-VM atomic state (2 VMs max)
struct VmGlobalState {
    pending_sgis: [AtomicU32; 8],    // per-vCPU SGI bitmask (bits 0-15)
    pending_spis: [AtomicU32; 8],    // per-vCPU SPI bitmask
    terminal_exit: [AtomicBool; 8],  // per-vCPU terminal exit flag
    vcpu_online_mask: AtomicU64,     // bit N = vCPU N online
    current_vcpu_id: AtomicUsize,
    pending_cpu_on: PendingCpuOn,    // PSCI CPU_ON request
    preemption_exit: AtomicBool,     // CNTHP timer fired
}

struct PendingCpuOn {
    requested: AtomicBool,
    target_cpu: AtomicU64,
    entry_point: AtomicU64,
    context_id: AtomicU64,
}

// Device manager wrapper (sync strategy varies by feature)
struct GlobalDeviceManager {
    #[cfg(not(feature = "multi_pcpu"))]
    devices: UnsafeCell<DeviceManager>,  // single-pCPU: no locking
    #[cfg(feature = "multi_pcpu")]
    devices: SpinLock<DeviceManager>,    // multi-pCPU: ticket lock
}

// Lock-free SPSC ring buffer
struct UartRxRing {
    buf: UnsafeCell<[u8; 64]>,
    head: AtomicUsize,               // producer (IRQ handler)
    tail: AtomicUsize,               // consumer (run loop)
}
```

## Memory Management Types

```rust
// src/arch/aarch64/mm/mmu.rs

// Static mapper (test path, no heap)
struct IdentityMapper {
    l0_table: PageTable,
    l1_table: PageTable,
    l2_tables: [PageTable; 4],
    l2_count: usize,
}

// Dynamic mapper (linux_guest, heap-allocated)
struct DynamicIdentityMapper {
    l0_table: u64,                   // physical address of heap page
    l1_table: u64,
    l2_tables: [u64; 4],
    l2_count: usize,
}

#[repr(C, align(4096))]
struct PageTable { entries: [S2PageTableEntry; 512] }

struct S2PageTableEntry(u64);        // newtype with valid/block/page/table constructors

struct Stage2Config { vttbr: u64, vtcr: u64 }

enum MemoryAttribute { Normal, Device, ReadOnly }
struct MemoryAttributes { bits: u64 }

// src/mm/allocator.rs
struct BumpAllocator {
    next: u64, end: u64,
    allocated: u64, free_head: u64,
}
```

## Platform Discovery Types

```rust
// src/dtb.rs
struct PlatformInfo {
    uart_base: u64,                  // from arm,pl011 compatible
    gicd_base: u64,                  // from arm,gic-v3 reg[0]
    gicr_base: u64,                  // from arm,gic-v3 reg[1]
    gicr_size: u64,                  // total GICR region
    num_cpus: usize,                 // from cpus node
    ram_base: u64,                   // from /memory reg
    ram_size: u64,
}
// Default: QEMU virt values (0x0900_0000, 0x0800_0000, etc.)
// Helpers: gicr_rd_base(cpu_id), gicr_sgi_base(cpu_id)

// src/guest_loader.rs
struct GuestConfig {
    guest_type: GuestType,
    load_addr: u64, mem_size: u64,
    entry_point: u64, dtb_addr: u64,
}
enum GuestType { Zephyr, Linux }
```

## Instruction Decode Types

```rust
// src/arch/aarch64/hypervisor/decode.rs
enum MmioAccess {
    Load { reg: u8, size: u8, sign_extend: bool },
    Store { reg: u8, size: u8 },
}
// MmioAccess::decode(esr, elr) → Option<(MmioAccess, u64)>
// Two paths: ISS-based (SRT/SAS fields) and instruction-based (manual bit extraction)
```

## Synchronization Types

```rust
// src/sync.rs
struct SpinLock<T> {
    next_ticket: AtomicU32,          // ticket lock for fairness
    now_serving: AtomicU32,
    data: UnsafeCell<T>,
}
struct SpinLockGuard<'a, T> { lock: &'a SpinLock<T>, ticket: u32 }
```

## Type Relationships

```
Vm ──contains──▶ [Option<Vcpu>; 8]
Vm ──contains──▶ Scheduler
Vcpu ──contains──▶ VcpuContext       (shared with ASM, #[repr(C)])
Vcpu ──contains──▶ VcpuArchState    (42-field hw save/restore)
Vcpu ──contains──▶ VirtualInterruptState

DeviceManager ──contains──▶ [Option<Device>; 8]
Device ──variants──▶ VirtualUart | VirtualGicd | VirtualGicr | VirtioMmioTransport<VirtioBlk>
VirtioMmioTransport<T> ──contains──▶ T (VirtioBlk) + Virtqueue

GlobalDeviceManager ──wraps──▶ DeviceManager (UnsafeCell or SpinLock)
VmGlobalState ──contains──▶ PendingCpuOn

DynamicIdentityMapper ──allocates──▶ PageTable (via heap)
PlatformInfo ◀──read by── distributor, redistributor, pl011, platform, guest_loader
```

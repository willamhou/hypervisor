# ARM64 Hypervisor 开发计划

**项目版本**: v0.18.0 (Phase 18 Complete — TF-A Boot Chain: BL33 Hypervisor)
**计划制定日期**: 2026-01-26
**最后更新**: 2026-02-20
**计划类型**: 敏捷迭代，灵活调整

---

## 📊 当前进度概览

**整体完成度**: 🟢 **75%** (Milestone 0-2 + Options A-G + M3 Sprint 3.1/3.1b/3.1c/3.2 + M4 Sprint 4.1/4.2 已完成)

```
M0: 项目启动          ████████████████████ 100% ✅
M1: MVP基础虚拟化     ████████████████████ 100% ✅
M2: 增强功能          ████████████████████ 100% ✅
M3: FF-A              ██████████████████░░  90% 🔧 (Sprint 3.2 ✅, Sprint 3.3 推迟到 M4)
M4: S-EL2 SPMC        ██████████░░░░░░░░░░  50% 🔧 (Sprint 4.1/4.2 ✅, Sprint 4.3/4.4 未开始)
M4.5: pKVM 集成       ░░░░░░░░░░░░░░░░░░░░   0% ⏸️ (NS-EL2=pKVM, S-EL2=us)
M5: RME & CCA         ░░░░░░░░░░░░░░░░░░░░   0% ⏸️
Android Boot          ██████████░░░░░░░░░░  50% ✅ (Phase 2 完成)
```

**测试覆盖**: ~204 assertions / 30 test suites (100% pass)
**代码量**: ~17000 行
**Linux启动**: 4 vCPU, BusyBox shell, virtio-blk, virtio-net, multi-VM, FF-A proxy
**Android启动**: 4 vCPU, PL031 RTC, Binder IPC, minimal init, 1GB RAM
**编译警告**: 最小化

---

## 1. 项目背景

### 1.1 开发团队
- **团队规模**: 个人开发
- **技能背景**: 
  - ARM64汇编和底层系统编程：专家级
  - Rust no_std裸机开发：非常熟悉
  - ARM虚拟化技术：专家级（见需求文档）
- **时间投入**: 灵活安排，根据阶段调整

### 1.2 开发策略
- **技术路线**: 自顶向下，快速原型验证
- **开发风格**: TDD驱动，频繁验证
- **文档化**: 边开发边写详细文档
- **难题处理**: 先用简单方案绕过，标记TODO后续优化
- **开源策略**: 立即开源，从第一天开始公开开发

### 1.3 核心原则
1. **快速验证**: 每个模块尽快在QEMU上验证
2. **TDD驱动**: 先写测试用例，再实现功能
3. **文档先行**: 每个模块先写设计文档
4. **敏捷迭代**: 短周期迭代（1-2周），快速反馈
5. **渐进增强**: 先最小实现，后续持续优化

---

## 2. 里程碑定义

### Milestone 0: 项目启动（Week 1-2）✅ **已完成**
**目标**: 搭建开发框架和基础设施

**交付物**:
- [x] 项目仓库初始化（GitHub公开）
- [x] Rust构建系统（aarch64-unknown-none target）
- [x] 基础链接脚本和启动代码（汇编）
- [x] QEMU启动脚本和调试配置
- [x] CI/CD基础（GitHub Actions）
- [x] 开发环境文档（README、CONTRIBUTING）

**关键任务**:
1. **Day 1-3**: 项目脚手架
   - 创建Cargo workspace
   - 配置`.cargo/config.toml`用于aarch64裸机
   - 编写基础`boot.S`（EL2启动入口）
   - 实现串口输出（UART，用于调试）
   - "Hello from EL2!" 第一个输出

2. **Day 4-7**: 构建和测试基础设施
   - 编写QEMU启动脚本（`-machine virt -cpu cortex-a57`）
   - 配置GDB远程调试
   - 编写Makefile或构建脚本
   - 设置GitHub仓库和基础CI（cargo check, cargo clippy）

3. **Day 8-14**: 基础抽象层
   - 定义核心数据结构（`struct Hypervisor`, `struct Vcpu`等）
   - 实现日志框架（格式化输出到UART）
   - panic handler
   - 基础错误处理（`Result<T, HvError>`）
   - 全局分配器占位符（后续实现）

**验收标准**:
- [x] 在QEMU中成功启动到EL2
- [x] UART输出"Hello from EL2!"
- [x] GDB可以断点调试
- [x] CI构建通过

**预估时间**: 2周（灵活调整）
**实际完成**: 2026-01-25

---

### Milestone 1: MVP - 基础虚拟化（Week 3-10）✅ **已完成**
**目标**: 在QEMU上启动一个最小的busybox initramfs Guest

**核心模块**:
1. ✅ vCPU管理
2. ✅ Stage-2内存虚拟化
3. ✅ 异常处理
4. ✅ 基础设备模拟（UART、Timer）
5. ✅ 虚拟中断注入（基础）

#### Sprint 1.1: vCPU框架（Week 3-4）✅ **已完成**
**设计文档先行**: 
- vCPU数据结构设计（寄存器保存/恢复）
- VM entry/exit机制
- 异常向量表设计

**TDD测试用例**（先写）:
- 测试：创建vCPU结构体
- 测试：保存/恢复通用寄存器
- 测试：设置vCPU入口点
- 测试：模拟简单的EL1代码执行（空循环）

**实现任务**:
1. **定义vCPU上下文**:
   ```rust
   struct VcpuContext {
       gpr: [u64; 31],     // X0-X30
       sp_el1: u64,
       elr_el1: u64,
       spsr_el1: u64,
       // 系统寄存器...
   }
   ```

2. **异常向量表**（汇编）:
   - EL2异常向量（同步、IRQ、FIQ、SError）
   - 保存vCPU上下文到栈
   - 调用Rust异常处理函数

3. **VM entry/exit**:
   - `vcpu_run()` - 使用`eret`进入EL1
   - 捕获异常返回EL2
   - 简单的异常分发

**验收**:
- [x] 创建vCPU并设置寄存器
- [x] vCPU执行几条指令后陷入EL2
- [x] 成功保存/恢复上下文

**预估**: 2周
**实际完成**: 2026-01-25
**关键文件**: `src/vcpu.rs`, `src/arch/aarch64/regs.rs`, `arch/aarch64/exception.S`

---

#### Sprint 1.2: Stage-2内存管理（Week 5-6）✅ **已完成**
**设计文档**:
- Stage-2页表格式（4KB粒度，3级或4级）
- IPA到PA映射策略
- VMID分配机制

**TDD测试**:
- 测试：创建空Stage-2页表
- 测试：映射单个4KB页
- 测试：映射大块内存（1GB）
- 测试：查询IPA对应的PA
- 测试：修改页表权限（RO -> RW）

**实现任务**:
1. **内存分配器**:
   - 简单的bump allocator（全局静态内存池）
   - 页帧分配器（4KB页）

2. **Stage-2页表**:
   - 页表项结构（PTE）
   - 3级页表遍历（1GB -> 2MB -> 4KB）
   - `map_page()` / `unmap_page()`
   - 设置VTTBR_EL2（页表基址）

3. **物理内存布局**:
   - 为Guest分配固定物理内存（如1GB）
   - 加载Guest内核镜像到Guest内存

**验收**:
- [x] 创建Stage-2页表并配置VTTBR_EL2
- [x] Guest访问内存被正确翻译
- [x] Guest访问未映射内存触发异常
- [x] MMIO设备区域正确映射（UART, GIC）

**预估**: 2周
**实际完成**: 2026-01-25 (基础), 2026-01-26 (MMIO设备映射修复)
**关键文件**: `src/arch/aarch64/mm/mmu.rs`, `src/vm.rs`

---

#### Sprint 1.3: 异常处理和设备模拟（Week 7-10）✅ **已完成**
**设计文档**:
- ESR_EL2异常分类
- MMIO trap-and-emulate机制
- UART和Timer模拟

**TDD测试**:
- 测试：捕获Guest的HVC调用
- 测试：捕获Guest的数据异常（访问MMIO）
- 测试：模拟UART读写
- 测试：模拟Timer中断注入

**实现任务**:
1. **异常处理**:
   - 解析ESR_EL2（Exception Syndrome Register）
   - 处理常见异常：
     - Data Abort（MMIO访问）
     - HVC（Hypervisor Call）
     - WFI/WFE（等待中断/事件）

2. **MMIO模拟框架**:
   - MMIO地址范围注册
   - 读/写回调机制
   - 模拟PL011 UART：
     - 地址：0x0900_0000
     - 实现基础寄存器（DR, FR等）
     - 转发输出到Host UART

3. **虚拟Timer**:
   - 配置EL1 Physical Timer
   - 注入虚拟Timer中断（使用vGIC占位符）

4. **Guest引导**:
   - 加载Linux内核Image到Guest内存（0x4008_0000）
   - 加载initramfs（busybox）
   - 设置X0（DTB地址）、X1-X3=0
   - 跳转到内核入口

**验收**:
- [x] Guest访问UART，输出显示在Host终端
- [x] Guest执行WFI不卡死
- [x] Guest内核开始启动（看到早期启动日志）
- [x] MMIO Trap-and-Emulate完全工作
- [x] Timer中断检测成功

**预估**: 4周
**实际完成**: 2026-01-26
**关键文件**: `src/arch/aarch64/hypervisor/exception.rs`, `src/arch/aarch64/hypervisor/decode.rs`, `src/devices/pl011/emulator.rs`, `src/devices/gic/distributor.rs`, `src/arch/aarch64/peripherals/timer.rs`

**重要修复**:
- 🐛 修复 ExitReason EC 映射错误 (src/arch/aarch64/regs.rs:131-132)
- 🐛 添加 MMIO 设备区域映射 (src/vm.rs:167-176)

---

#### Sprint 1.5b: 虚拟中断注入（追加）✅ **已完成**
**完成日期**: 2026-01-26

**实现任务**:
1. [x] 虚拟中断状态管理 (VirtualInterruptState)
2. [x] HCR_EL2.VI/VF 位控制
3. [x] Vcpu 集成 (inject_irq API)
4. [x] 基础测试通过

**验收**:
- [x] Hypervisor 可以注入虚拟 IRQ
- [x] Guest unmask IRQ 后收到中断
- [x] HCR_EL2.VI 机制验证工作

**关键文件**: `src/vcpu_interrupt.rs`, `tests/test_guest_interrupt.rs`

**待完善** (Sprint 1.6 可选):
- [ ] Guest 异常向量表和 IRQ handler
- [ ] EOI (End of Interrupt) 处理
- [ ] 多次中断注入测试

---

**Milestone 1 总验收标准**:
- [x] 在QEMU (`-machine virt`) 上启动Linux内核
- [x] 内核启动到initramfs
- [x] 看到busybox shell提示符（可能无法交互，UART输入暂不实现）
- [x] Guest可以执行简单命令（如`echo`, `ls`）
- [x] MMIO 设备模拟完全工作
- [x] 虚拟中断注入基础功能工作

**预估总时间**: 8周（Week 3-10）
**实际完成**: 2026-01-26
**当前版本**: v0.3.0

---

### Milestone 2: 增强功能（Week 11-18）✅ **已完成**
**目标**: 完善虚拟化功能，支持完整Linux发行版
**实际完成**: 2026-02-13

#### Sprint 2.1: GIC虚拟化（Week 11-13）✅ **已完成**
**设计文档**:
- GICv3架构
- 虚拟中断注入机制
- Distributor和Redistributor模拟

**实现任务**:
1. **vGIC数据结构**:
   - [x] 中断状态（pending, active, enabled）
   - [x] 中断优先级和路由（GICD_IROUTER shadow state）

2. **中断注入**:
   - [x] 虚拟SGI（ICC_SGI1R_EL1 trap via TALL1）
   - [x] 虚拟PPI（CNTHP timer INTID 26, virtual timer HW=1）
   - [x] 虚拟SPI（PENDING_SPIS atomic queue per vCPU）
   - [x] ICH_LR_EL2 List Register管理
   - [x] EOImode=1 + HW=1 for timer interrupts

3. **GIC寄存器模拟**:
   - [x] GICD_*（Distributor）: trap + write-through (shadow state + forwarding to physical GICD)
   - [x] GICR_*（Redistributor）: full trap-and-emulate via VirtualGicr (all 4 GICRs)
   - [x] Stage-2 4KB selective unmap for GICR trap regions

**验收**:
- [x] Guest可以使能中断
- [x] Timer中断正确触发Guest中断处理
- [x] Guest可以接收和处理多个中断
- [x] SGI/IPI emulation for SMP (inter-vCPU signaling)
- [x] SPI routing via Aff0 (GICD_IROUTER)

**预估**: 3周
**实际完成**: ~1周
**关键文件**: `src/devices/gic/distributor.rs`, `src/devices/gic/redistributor.rs`, `src/arch/aarch64/peripherals/gicv3.rs`

---

#### Sprint 2.2: virtio设备（Week 14-16）✅ **已完成**
**设计文档**:
- virtio-mmio传输层
- virtio-blk块设备

**实现任务**:
1. **virtio-mmio框架**:
   - [x] VirtioDevice trait + VirtioMmioTransport
   - [x] virtqueue管理 (descriptor table, available/used ring)
   - [x] MMIO region at 0x0A000000 (SPI 16, INTID 48)

2. **UART RX (替代virtio-console)**:
   - [x] 全 trap-and-emulate PL011 (VirtualUart)
   - [x] RX ring buffer + 物理 INTID 33 中断
   - [x] PeriphID/PrimeCellID 寄存器 (Linux amba-pl011 probe)
   - [x] Guest可以通过UART双向交互

3. **virtio-blk**:
   - [x] 内存磁盘 (disk.img @ 0x58000000 via QEMU -device loader)
   - [x] VIRTIO_BLK_T_IN / VIRTIO_BLK_T_OUT
   - [x] flush_pending_spis_to_hardware() 低延迟SPI注入
   - [x] Linux: `virtio_blk virtio0: [vda] 4096 512-byte logical blocks`

**验收**:
- [x] Guest通过UART双向交互 (替代virtio-console方案)
- [x] 可以在Guest BusyBox shell中输入命令并执行
- [x] virtio-blk块设备正常工作

**预估**: 3周
**实际完成**: ~1周
**关键文件**: `src/devices/virtio/mod.rs`, `src/devices/virtio/blk.rs`, `src/devices/pl011/emulator.rs`
**注**: 采用UART RX替代virtio-console方案，功能等价但实现更直接

---

#### Sprint 2.3: SMP支持（Week 17-18）✅ **已完成**
**设计文档**:
- PSCI实现
- 多vCPU管理
- 抢占式调度

**实现任务**:
1. **PSCI调用**:
   - [x] CPU_ON: 通过PENDING_CPU_ON原子信号启动辅助vCPU
   - [x] CPU_OFF: 关闭CPU
   - [x] PSCI_VERSION / SYSTEM_OFF / SYSTEM_RESET

2. **多vCPU调度**:
   - [x] 4 vCPU round-robin scheduling on single pCPU (run_smp())
   - [x] Per-vCPU arch state (VcpuArchState): GIC LRs, ICH_VMCR/HCR, timer, VMPIDR, all EL1 sysregs, SP_EL0, PAC keys
   - [x] WFI cooperative yielding (TWI trap)
   - [x] CNTHP preemptive timer (INTID 26, 10ms quantum)
   - [x] SGI/IPI emulation via TALL1 trap (ICC_SGI1R_EL1)
   - [x] SPI injection before vCPU entry (PENDING_SPIS per-vCPU atomic queue)

3. **附加 SMP 基础设施**:
   - [x] VMPIDR_EL2 per-vCPU (Aff0 = vcpu_id)
   - [x] GICD_IROUTER shadow state for SPI routing
   - [x] 4 physical GICR frames via identity mapping
   - [x] ensure_cnthp_enabled() before every vCPU entry

**验收**:
- [x] Guest可以启动多个CPU（4核）: `smp: Brought up 1 node, 4 CPUs`
- [x] SMP内核正常运行（无RCU stalls, 无watchdog lockups）
- [x] SGI/IPI inter-vCPU signaling正常
- [x] 抢占式调度防止单vCPU饥饿

**预估**: 2周
**实际完成**: ~1周
**关键文件**: `src/arch/aarch64/hypervisor/exception.rs` (run_smp, handle_psci), `src/arch/aarch64/regs.rs` (VcpuArchState)

---

**Milestone 2 总验收**:
- [x] 启动完整Linux (6.12.12 defconfig arm64 + BusyBox initramfs)
- [x] 支持交互式shell (UART RX双向交互)
- [x] SMP稳定工作 (4 vCPU, 无RCU stalls)
- [x] virtio-blk块设备 (`[vda] 4096 512-byte logical blocks`)
- [x] GIC虚拟化 (GICD trap + write-through, GICR trap-and-emulate)
- [x] 文档完善 (CLAUDE.md全面更新)

**预估总时间**: 8周（Week 11-18）
**实际完成**: ~3周 (2026-01-27 至 2026-02-13)
**状态**: ✅ 已完成

**M2 附加完成项** (超出原计划):
- DynamicIdentityMapper: 堆分配 4KB 页表，split_2mb_block()
- Free-list allocator (BumpAllocator + free_head)
- DeviceManager enum dispatch (Device enum: Uart, Gicd, Gicr, VirtioBlk, VirtioNet)
- VirtualGicr per-vCPU 状态仿真
- Custom kernel build via Docker (debian:bookworm-slim)
- ~144 test assertions / 28 test suites

---

### Milestone 3: 安全扩展 - FF-A（Week 19-28）🔧 **进行中**
**目标**: 实现FF-A Hypervisor角色，支持内存共享

根据你的偏好，**先实现FF-A**（因为它是TEE和Realm的通信基础）。

#### Sprint 3.1: FF-A基础框架 + Stub SPMC ✅ **已完成**
**设计文档**: `docs/plans/2026-02-18-ffa-proxy-design.md`
**实现计划**: `docs/plans/2026-02-18-ffa-proxy-impl.md`

**实现任务**:
1. **SMC Trap Infrastructure**:
   - [x] HCR_EL2.TSC=1 (bit 19) 陷入 guest SMC 到 EL2
   - [x] EC_SMC64 (0x17) 异常类 + ExitReason::SmcCall
   - [x] handle_smc() 路由: PSCI → FF-A proxy → SMC_UNKNOWN
   - [x] is_ffa_function() 识别 SMC32/64 FF-A 调用 (low byte >= 0x60)

2. **FF-A v1.1 Constants + Basic Calls** (`src/ffa/mod.rs`, `src/ffa/proxy.rs`):
   - [x] FFA_VERSION 版本协商 (返回 v1.1 = 0x00010001)
   - [x] FFA_ID_GET 获取 partition ID (vm_id → partition_id)
   - [x] FFA_FEATURES 查询支持的特性
   - [x] FFA_PARTITION_INFO_GET 发现 SP (通过 RXTX mailbox)
   - [x] ffa_error() 32-bit 掩码 (避免 i32→u64 符号扩展)

3. **RXTX Mailbox** (`src/ffa/mailbox.rs`):
   - [x] Per-VM TX/RX buffer IPA 对注册 (FFA_RXTX_MAP)
   - [x] FFA_RXTX_UNMAP + FFA_RX_RELEASE
   - [x] UnsafeCell+Sync 全局存储模式 (非 static mut)

4. **Stub SPMC** (`src/ffa/stub_spmc.rs`):
   - [x] 2 模拟 Secure Partitions (SP1=0x8001, SP2=0x8002)
   - [x] FFA_MSG_SEND_DIRECT_REQ echo messaging (x4-x7 回传)
   - [x] Share record management + atomic handle allocation

5. **Memory Sharing** (`src/ffa/memory.rs`):
   - [x] FFA_MEM_SHARE / FFA_MEM_LEND → handle allocation
   - [x] FFA_MEM_RECLAIM → handle validation + cleanup
   - [x] FFA_MEM_DONATE → blocked (returns NOT_SUPPORTED)
   - [x] Stage-2 PTE SW bits [56:55] page ownership tracking (pKVM-compatible)
   - [x] DynamicIdentityMapper::read_sw_bits() / write_sw_bits() / walk_to_leaf()
   - [x] PageOwnership enum: Owned/SharedOwned/SharedBorrowed/Donated

6. **Tests**:
   - [x] test_ffa: 13 assertions (VERSION, ID_GET, FEATURES, RXTX, messaging, MEM_SHARE/RECLAIM)
   - [x] test_page_ownership: 4 assertions (SW bits read/write, unmapped IPA handling)
   - [x] All 4 feature configs build clean (default, linux_guest, multi_pcpu, multi_vm)

**验收**:
- [x] VM 可以发现系统中的 SP (FFA_PARTITION_INFO_GET)
- [x] 基础 FF-A 调用正常工作 (VERSION, ID_GET, FEATURES)
- [x] Direct Messaging echo 工作
- [x] 内存共享 handle 分配 + 回收
- [x] Page ownership tracking via PTE SW bits

**实际完成**: 2026-02-18
**关键文件**: `src/ffa/mod.rs`, `src/ffa/proxy.rs`, `src/ffa/mailbox.rs`, `src/ffa/stub_spmc.rs`, `src/ffa/memory.rs`

---

#### Sprint 3.1b: FF-A Validation + Descriptors + SMC Forwarding ✅ **已完成**
**设计文档**: `/home/willamhou/.claude/plans/rippling-forging-crescent.md`

**实现任务**:
1. **Page Ownership Validation** (`src/ffa/stage2_walker.rs`, `src/ffa/proxy.rs`):
   - [x] Stage2Walker: lightweight page table walker from VTTBR_EL2
   - [x] S2AP constants (S2AP_NONE/RO/RW) in defs.rs
   - [x] MEM_SHARE validates all pages are Owned → transitions to SharedOwned
   - [x] MEM_SHARE sets S2AP_RO (share) or S2AP_NONE (lend)
   - [x] MEM_RECLAIM restores Owned + S2AP_RW
   - [x] Unified handle_mem_share_or_lend() for SHARE/LEND
   - [x] MemShareRecord with multi-range support (ranges[], range_count, is_lend)
   - [x] lookup_share() for reclaim-time IPA restoration

2. **FF-A v1.1 Descriptor Parsing** (`src/ffa/descriptors.rs`):
   - [x] #[repr(C, packed)] structs: FfaMemRegion (48B), FfaMemAccessDesc (16B), FfaCompositeMemRegion (16B), FfaMemRegionAddrRange (16B)
   - [x] parse_mem_region() with bounds validation and core::ptr::read_unaligned
   - [x] Dual interface: descriptor-based (mailbox mapped) or register-based (fallback)
   - [x] build_test_descriptor() helper for unit tests
   - [x] Single-receiver only, no fragmentation support

3. **SMC Forwarding to Secure World** (`src/ffa/smc_forward.rs`):
   - [x] forward_smc(x0-x7) → SmcResult via inline asm `smc #0`
   - [x] probe_spmc() → FFA_VERSION to EL3, check for valid response
   - [x] SPMC_PRESENT runtime detection at boot (ffa::proxy::init())
   - [x] Unknown FF-A calls forwarded to SPMC when present
   - [x] Unknown SMCs in exception handler forwarded to EL3 (SMCCC pass-through)

4. **Tests**:
   - [x] test_ffa expanded: 13 → 18 assertions (+3 descriptor parsing, +1 SMC forward, +1 unknown FFA)
   - [x] Descriptor parsing: valid single-range, valid multi-range, undersized error
   - [x] SMC forward: PSCI_VERSION returns valid response from QEMU EL3
   - [x] All 4 feature configs build clean

**验收**:
- [x] Page ownership wired into MEM_SHARE/LEND/RECLAIM (pKVM-compatible)
- [x] FF-A v1.1 composite descriptor parsing from TX buffer
- [x] SMC forwarding to EL3 works (PSCI_VERSION verified)
- [x] 29 test suites, ~158 assertions, 0 failures

**实际完成**: 2026-02-19
**关键文件**: `src/ffa/stage2_walker.rs` (NEW), `src/ffa/descriptors.rs` (NEW), `src/ffa/smc_forward.rs` (NEW), `src/ffa/proxy.rs`, `src/ffa/stub_spmc.rs`, `src/arch/aarch64/hypervisor/exception.rs`

---

#### Sprint 3.1c: VM-to-VM FF-A Memory Sharing ✅ **已完成**
**设计文档**: `docs/plans/2026-02-20-vm-to-vm-ffa-design.md`

**实现任务**:
1. **VM-to-VM Memory Sharing**:
   - [x] FFA_MEM_RETRIEVE_REQ: receiver maps shared pages into own Stage-2 via PER_VM_VTTBR
   - [x] FFA_MEM_RELINQUISH: receiver unmaps pages, restores sender S2AP
   - [x] Stage2Walker::map_page() / unmap_page() for cross-VM page mapping
   - [x] PER_VM_VTTBR global: per-VM L0 table PA for cross-VM Stage-2 access
   - [x] ShareInfoFull: extended share records with receiver tracking + mark_retrieved/mark_relinquished
   - [x] Receiver validation (is_valid_receiver, receiver != sender, known SP or VM)

2. **Tests**:
   - [x] test_ffa expanded: 18 → 27 assertions (+9 VM-to-VM sharing tests, later 27 → 44 in Sprint 3.2)
   - [x] Full lifecycle: SHARE → RETRIEVE → RELINQUISH → RECLAIM
   - [x] Error cases: retrieve non-existent, relinquish already-relinquished, wrong receiver

**验收**:
- [x] VM-to-VM memory sharing complete lifecycle
- [x] Dynamic Stage-2 page mapping across VMs
- [x] 30 test suites, ~171 assertions, 0 failures

**实际完成**: 2026-02-20
**关键文件**: `src/ffa/proxy.rs`, `src/ffa/stub_spmc.rs`, `src/ffa/stage2_walker.rs`, `src/global.rs`

---

#### Sprint 3.2: NS-EL2 完善（Week 22-25）✅ **已完成**
**目标**: 完善 NS-EL2 hypervisor 的 FF-A 实现和 Stage-2 安全性，为后续 S-EL2 适配做准备

**实现任务**:
1. **2MB Block → 4KB 拆分** (安全修复):
   - [x] Stage2Walker `set_s2ap()` 检测 2MB block PTE 并拆分为 L3 table
   - [x] FF-A MEM_SHARE 按 4KB 粒度修改权限（不影响整个 2MB 区域）
   - [x] `write_sw_bits()` 同样支持 block 拆分
   - [x] 测试: 验证拆分后 PTE 权限正确 (test_page_ownership 4→9 assertions)

2. **FF-A Indirect Messaging**:
   - [x] FFA_MSG_SEND2 (TX→RX buffer copy, indirect message header parsing)
   - [x] FFA_MSG_WAIT (non-blocking, check msg_pending)
   - [x] Per-VM mailbox msg_pending + msg_sender_id tracking
   - [x] handle_rx_release() clears msg_pending

3. **FF-A Notifications** (v1.1):
   - [x] FFA_NOTIFICATION_BITMAP_CREATE / BITMAP_DESTROY
   - [x] FFA_NOTIFICATION_BIND / UNBIND (0x8400007F/80)
   - [x] FFA_NOTIFICATION_SET (0x84000081) — validates bind, ORs into pending
   - [x] FFA_NOTIFICATION_GET (0x84000082) — returns and clears pending bits
   - [x] FFA_NOTIFICATION_INFO_GET (0x84000083) — scans for pending endpoints
   - [x] Per-partition 64-bit bitmaps, 8 endpoint slots, NotifBind records

4. **补充 FF-A 调用**:
   - [x] FFA_SPM_ID_GET (0x84000085) — 返回 SPMC ID (0x8000)
   - [x] FFA_RUN (0x8400006D) — NOT_SUPPORTED stub (no real SPMC)

5. **测试**:
   - [x] test_ffa expanded: 27 → 44 assertions (+17: 3 supplemental + 8 notifications + 6 indirect messaging)
   - [x] Page-aligned buffer fix (`#[repr(C, align(4096))] struct PageBuf`)

**验收**:
- [x] FF-A MEM_SHARE 仅修改目标 4KB 页权限，不影响 2MB 区域内其他页
- [x] Indirect messaging 和 notifications 基础框架可用
- [x] 测试覆盖率提升: ~171 → ~204 assertions

**实际完成**: 2026-02-20
**关键文件**: `src/ffa/notifications.rs` (NEW), `src/ffa/proxy.rs`, `src/ffa/mailbox.rs`, `src/ffa/mod.rs`, `tests/test_ffa.rs`

---

#### Sprint 3.3: 真实 SPMC 集成 (与 M4 合并) ⏸️ **推迟**
**注**: 原 Sprint 3.2/3.3 中的"真实 SPMC 集成"已合并到 Milestone 4（QEMU secure=on + S-EL2 适配）。
我们的 hypervisor 将**自身作为 SPMC** 运行在 S-EL2，而不是集成 Hafnium。

---

**Milestone 3 总验收**:
- [x] FF-A Hypervisor proxy 基础框架 + stub SPMC ✅
- [x] VM-to-VM 内存共享完整生命周期 (SHARE → RETRIEVE → RELINQUISH → RECLAIM) ✅
- [x] Page ownership validation + S2AP permission control ✅
- [x] FF-A v1.1 descriptor parsing + SMC forwarding ✅
- [x] Integration test: 11 assertions with real Stage-2 page tables (make run-multi-vm) ✅
- [x] 2MB block → 4KB 拆分（Stage2Walker 安全修复）✅
- [x] FF-A indirect messaging + notifications ✅
- [x] FFA_SPM_ID_GET + FFA_RUN ✅

**预估总时间**: 10周（Week 19-28）
**状态**: 🔧 进行中 (Sprint 3.1/3.1b/3.1c/3.2 ✅, Sprint 3.3 推迟到 M4, ~90%)

---

### Milestone 4: QEMU secure=on + S-EL2 SPMC（Week 29-36）🔧 **进行中**
**目标**: 将我们的 hypervisor 适配为 S-EL2 SPMC（替代 Hafnium），通过 TF-A boot chain 启动

**架构目标**:
```
EL3:    TF-A BL31 + SPMD (SMC relay, world switch)
S-EL2:  我们的 hypervisor (SPMC 角色, BL32)
S-EL1:  Secure Partitions (bare-metal SP)
NS-EL2: 暂时空闲（后续 pKVM 占据）
NS-EL1: Linux guest (当前 hypervisor 功能降级为 SPMC)
```

**关键依赖**: QEMU `secure=on` 不支持 KVM 加速（必须 TCG 全模拟），速度慢 10-50x

#### Sprint 4.1: 构建 TF-A + QEMU secure=on（Week 29-30）✅ **已完成**

**实现任务**:
1. **交叉编译 ARM Trusted Firmware (TF-A)**:
   - [x] `PLAT=qemu, SPD=spmd, SPMD_SPM_AT_SEL2=1`
   - [x] `CTX_INCLUDE_EL2_REGS=1` (保存/恢复 EL2 寄存器用于 S-EL2)
   - [x] 生成 `flash.bin` (BL1 + FIP: BL2/BL31/BL32/BL33)
   - [x] Docker 构建脚本 (`scripts/build-tfa.sh`, `scripts/build-bl32-bl33.sh`, `scripts/build-qemu.sh`)

2. **QEMU secure=on 启动验证**:
   - [x] `-machine virt,secure=on,virtualization=on`
   - [x] `-bios flash.bin` 替代 `-kernel`
   - [x] BL32 = trivial S-EL2 binary prints "Hello from S-EL2!" then FFA_MSG_WAIT
   - [x] BL33 = trivial NS-EL2 binary prints "Hello from NS-EL2!"

**验收**:
- [x] TF-A v2.12.0 编译成功并生成 flash.bin
- [x] QEMU 9.2.3 编译成功 (`tools/qemu-system-aarch64`)
- [x] QEMU secure=on 下 BL32 + BL33 成功启动
- [x] 串口输出可见: "Hello from S-EL2!" + "Hello from NS-EL2!"

**实际完成**: 2026-02-20
**关键文件**: `tfa/bl32_hello/start.S`, `tfa/bl33_hello/start.S`, `scripts/build-tfa.sh`, `scripts/build-bl32-bl33.sh`, `scripts/build-qemu.sh`

**重要修复**:
- FFA_MSG_WAIT = 0x8400006B (NOT 0x84000071 which is FFA_MEM_DONATE_32)
- SPMD hangs if BL32 doesn't issue FFA_MSG_WAIT after init

---

#### Sprint 4.2: BL33 = 我们的 Hypervisor（Week 30-31）✅ **已完成**

**实现任务**:
1. **适配 TF-A boot chain**:
   - [x] `boot.S` 入口点无需修改 — `adrp` 位置无关，BL31 传 x0=DTB (兼容 QEMU `-kernel`)
   - [x] 链接基址从 0x40000000 → 0x40200000 (避免 QEMU `-bios` 模式 DTB 占据 0x40000000-0x40100000)
   - [x] `PRELOADED_BL33_BASE=0x40200000` — BL2 跳过 FIP 中 BL33，使用固定地址作入口点
   - [x] QEMU `-device loader,file=hypervisor.bin,addr=0x40200000` 预加载 binary

2. **构建 + 验证**:
   - [x] `make build-tfa-bl33` — 生成 `tfa/flash-bl33.bin` (无 BL33 in FIP)
   - [x] `make run-tfa-linux` — 完整 boot chain:
     BL1→BL2→BL31(SPMD)→BL32(stub S-EL2)→BL33(hypervisor@0x40200000)→Linux→BusyBox shell
   - [x] 4 vCPUs, virtio-blk, virtio-net, auto-IP — 所有 Linux guest 功能正常
   - [x] `make run` 回归测试 — 193 assertions 全部通过

**验收**:
- [x] 我们的 hypervisor 通过 TF-A boot chain 启动（`make run-tfa-linux`）
- [x] 现有功能（Linux guest boot）完全正常
- [x] 回归测试通过 (193 unit test assertions)

**实际完成**: 2026-02-20
**关键文件**: `arch/aarch64/linker.ld` (base 0x40200000), `Makefile` (build-tfa-bl33/run-tfa-linux), `scripts/build-tfa.sh` (PRELOADED_BL33_BASE)

**重要修复**:
- QEMU DTB at 0x40000000 in `-bios` mode — 1MB DTB overlaps hypervisor binary. QEMU 9.2+ treats ROM overlap as fatal error. Fixed by moving linker base to 0x40200000.
- `.gitignore` updated with `tfa/flash-bl33.bin`

---

#### Sprint 4.3: Hypervisor 适配 S-EL2 (SPMC 角色)（Week 32-34）⏸️ **未开始**
**核心**: 将同一份代码编译为 BL32（S-EL2），作为 SPMC 接收 SPMD 转发的 FF-A 调用

**实现任务**:
1. **S-EL2 入口点和初始化**:
   - [ ] 新的 `boot_sel2.S` 入口（SPMD 跳转到 BL32 的方式不同于 BL33）
   - [ ] 处理 SPMD 传递的参数 (x0=TOS_FW_CONFIG, x1=HW_CONFIG, x4=core_id)
   - [ ] 解析 SPMC manifest (DTS 格式)
   - [ ] `#[cfg(feature = "sel2")]` feature flag 区分 NS-EL2 和 S-EL2 模式

2. **SPMD ↔ SPMC 协议**:
   - [ ] FFA_SECONDARY_EP_REGISTER (0x84000087) — 注册辅助核入口点
   - [ ] FFA_VERSION 响应（作为 SPMC 回复 SPMD 的版本查询）
   - [ ] FFA_FEATURES 响应（向 SPMD 声明支持的功能）
   - [ ] 处理 SPMD 转发的 FF-A 内存操作

3. **Secure Stage-2 页表**:
   - [ ] VSTTBR_EL2 替代 VTTBR_EL2（Secure 世界用 VSTTBR）
   - [ ] Secure 内存区域隔离（TZASC 配置）
   - [ ] SP 的 Stage-2 隔离

4. **构建系统**:
   - [ ] `make build-spmc` — 编译 BL32 binary（S-EL2 入口）
   - [ ] SP manifest 模板 (DTS)
   - [ ] `sp_layout.json` for TF-A FIP 打包

**验收**:
- [ ] 我们的 hypervisor 作为 BL32 在 S-EL2 启动
- [ ] SPMD ↔ SPMC FF-A 握手成功 (VERSION/FEATURES)
- [ ] FFA_SECONDARY_EP_REGISTER 注册辅助核入口

**预估**: 2-3 周

---

#### Sprint 4.4: 从 S-EL2 启动最小 SP（Week 35-36）⏸️ **未开始**

**实现任务**:
1. **SP 加载和启动**:
   - [ ] 从 FIP 中读取 SP binary 和 manifest
   - [ ] 为 SP 创建 Secure Stage-2 映射
   - [ ] 跳转到 S-EL1 执行 SP

2. **SP 通信**:
   - [ ] FFA_MSG_SEND_DIRECT_REQ 从 NS → S (经 SPMD → 我们的 SPMC → SP)
   - [ ] FFA_MSG_SEND_DIRECT_RESP 返回
   - [ ] 完整 SMC 路径: NS-EL1 → (NS-EL2 trap) → EL3 SPMD → S-EL2 SPMC → S-EL1 SP

3. **Secure 中断路由**:
   - [ ] FIQ 路由到 S-EL2
   - [ ] 注入到 SP (S-EL1)

**验收**:
- [ ] 最小 bare-metal SP 在 S-EL1 成功启动
- [ ] NS → SP 直接消息传递正常
- [ ] 完整的跨世界 FF-A 路径验证

**预估**: 2 周

---

**Milestone 4 总验收**:
- [x] TF-A boot chain 正常 (BL1 → BL2 → BL31/SPMD → BL32/SPMC → BL33) ✅ Sprint 4.1/4.2
- [ ] 我们的 hypervisor 同时支持 NS-EL2 和 S-EL2 (SPMC) 模式
- [ ] SPMD ↔ SPMC 协议握手成功
- [ ] NS → SP 的 FF-A 直接消息传递正常
- [ ] 为 pKVM 集成做好准备（NS-EL2 空闲，可被 pKVM 占据）

**预估总时间**: 6-8 周（Week 29-36）
**状态**: 🔧 进行中 (Sprint 4.1/4.2 ✅, Sprint 4.3/4.4 ⏸️)

---

### Milestone 4.5: pKVM 集成（Week 37-42）⏸️ **未开始**
**目标**: 在 NS-EL2 运行 pKVM，我们的 hypervisor 在 S-EL2 作为 SPMC，验证完整 FF-A 端到端路径

**架构目标**:
```
EL3:    TF-A BL31 + SPMD
S-EL2:  我们的 hypervisor (SPMC, BL32) → 管理 Secure Partitions
S-EL1:  Secure Partitions
NS-EL2: pKVM (Linux KVM protected mode) → 管理 Normal World VMs
NS-EL1: Linux/Android guest
```

#### Sprint 4.5.1: 构建 pKVM Kernel（Week 37-38）⏸️ **未开始**

**实现任务**:
1. **pKVM Kernel 配置**:
   - [ ] Linux 6.6+ 开启 `CONFIG_KVM=y`, `CONFIG_KVM_ARM_HOST=y`
   - [ ] `CONFIG_VIRTUALIZATION=y`, `CONFIG_HAVE_KVM=y`
   - [ ] pKVM protected mode 相关 config
   - [ ] Docker 构建脚本

2. **Boot Chain 适配**:
   - [ ] pKVM 作为 BL33 运行在 NS-EL2
   - [ ] TF-A BL31 将控制权交给 pKVM
   - [ ] pKVM 初始化 Stage-2 页表保护

**验收**:
- [ ] pKVM kernel 编译成功
- [ ] pKVM 通过 TF-A boot chain 启动到 NS-EL2

**预估**: 2 周

---

#### Sprint 4.5.2: FF-A 端到端集成（Week 39-42）⏸️ **未开始**

**实现任务**:
1. **pKVM FF-A Proxy ↔ 我们的 SPMC**:
   - [ ] pKVM trap guest SMC → pKVM FF-A proxy 验证 → `smc #0` → SPMD(EL3) → 我们的 SPMC(S-EL2)
   - [ ] FFA_MEM_SHARE 端到端: guest page ownership → pKVM S2AP → SPMD relay → SPMC SP mapping
   - [ ] FFA_MSG_SEND_DIRECT_REQ: guest → pKVM → SPMD → SPMC → SP → 返回

2. **双 Hypervisor 协调**:
   - [ ] pKVM (NS-EL2) 和我们的 SPMC (S-EL2) 各自管理独立的 Stage-2
   - [ ] EL3 SPMD 负责世界切换（context save/restore）
   - [ ] Secure 中断路由 (FIQ → S-EL2，IRQ → NS-EL2)

3. **端到端验证**:
   - [ ] Android VM (pKVM protected) 通过 FF-A 与 SP 共享内存
   - [ ] 全程经过 pKVM proxy → SPMD → 我们的 SPMC
   - [ ] 内存隔离: pKVM protected VM 的页不可被 host 直接访问

**验收**:
- [ ] pKVM guest 的 FF-A MEM_SHARE 经过完整路径
- [ ] NS → S 直接消息传递正常
- [ ] 两个 hypervisor 和平共处

**预估**: 4 周

---

**Milestone 4.5 总验收**:
- [ ] pKVM 在 NS-EL2 运行，我们的 hypervisor 在 S-EL2 运行
- [ ] 完整 FF-A 路径: NS-EL1 → NS-EL2(pKVM) → EL3(SPMD) → S-EL2(SPMC) → S-EL1(SP)
- [ ] 内存共享端到端验证
- [ ] 这是开源领域罕见的完整 ARM 安全架构实现

**预估总时间**: 4-6 周（Week 37-42）
**状态**: ⏸️ 未开始

---

### Milestone 5: 安全扩展 - RME & CCA（Week 43-58+）⏸️ **未开始**
**目标**: 实现Realm Manager (RMM)，支持Realm VM启动Guest OS

#### Sprint 5.1: GPT和内存隔离（Week 37-40）⏸️ **未开始**
**设计文档**:
- Granule Protection Table (GPT)机制
- 四世界内存隔离（Root, Secure, Realm, Normal）

**实现任务**:
1. **GPT配置**（需EL3支持）:
   - 与EL3固件协同配置GPT
   - 标记物理内存页为不同世界

2. **Realm内存分配器**:
   - 分配Realm专用物理页
   - 确保页标记为Realm

3. **基础隔离测试**:
   - Normal访问Realm内存触发异常
   - Secure访问Realm内存触发异常

**验收**:
- [ ] GPT配置成功
- [ ] 跨世界非法访问被硬件阻止

**预估**: 4周

---

#### Sprint 5.2: RTT和Realm创建（Week 41-44）⏸️ **未开始**
**设计文档**:
- Realm Translation Table (RTT)结构
- RMI接口实现（CREATE, DESTROY等）

**实现任务**:
1. **RTT管理**:
   - RTT页表创建（类似Stage-2，但用于Realm）
   - RTT walk和映射

2. **RMI接口**:
   - `RMI_REALM_CREATE`: 创建Realm结构
   - `RMI_REC_CREATE`: 创建Realm vCPU (REC)
   - `RMI_RTT_CREATE`: 分配RTT页表
   - `RMI_DATA_CREATE`: 分配Realm内存页

3. **Realm元数据**:
   - Realm ID (RID)
   - Realm配置（测量、策略）

**TDD测试**:
- 测试：通过RMI创建Realm
- 测试：分配RTT并建立映射
- 测试：创建多个REC

**验收**:
- [ ] Normal World Hypervisor通过RMI创建Realm
- [ ] RTT正确建立
- [ ] Realm结构完整

**预估**: 4周

---

#### Sprint 5.3: Realm运行和RSI（Week 45-48）⏸️ **未开始**
**设计文档**:
- RMI_REC_ENTER/EXIT机制
- RSI接口（Realm调用RMM）

**实现任务**:
1. **RMI_REC_ENTER**:
   - 切换到Realm EL1
   - 执行Realm vCPU
   - 处理Realm exit（异常、MMIO等）

2. **RMI_REC_EXIT**:
   - 保存Realm上下文
   - 返回Normal World Hypervisor

3. **RSI接口**:
   - `RSI_VERSION`
   - `RSI_IPA_STATE_SET`: 管理IPA状态（Protected/Unprotected）
   - `RSI_HOST_CALL`: Realm请求Host服务（受限）

4. **Realm异常处理**:
   - Realm的Data Abort、HVC等
   - MMIO转发到Host Hypervisor

**验收**:
- [ ] Realm vCPU成功运行
- [ ] Realm可以执行代码并exit
- [ ] RSI接口正常工作

**预估**: 4周

---

#### Sprint 5.4: Realm启动Guest OS（Week 49-52+）⭐ ⏸️ **未开始**
**设计文档**:
- Realm Guest启动流程
- 内存初始化和设备传递

**实现任务**:
1. **加载Realm Guest镜像**:
   - 通过RMI_DATA_CREATE拷贝内核镜像到Realm内存
   - 加载initramfs

2. **设备支持**:
   - 虚拟UART（MMIO trap到Host）
   - 虚拟Timer
   - virtio设备（通过Unprotected IPA）

3. **启动Realm Guest**:
   - 设置入口点
   - 配置DTB（包含virtio设备）
   - 执行`RMI_REC_ENTER`

4. **调试和稳定性**:
   - Realm Guest启动日志
   - 处理各种exit原因
   - 内存管理bug修复

**验收** ⭐:
- [ ] Realm VM中启动Linux内核
- [ ] 内核启动到busybox shell
- [ ] Realm Guest可以与Host通过virtio通信
- [ ] 内存隔离正确（无法访问Normal内存）

**预估**: 4周+（可能需要更多时间调试）
**状态**: ⏸️ 未开始

---

#### Sprint 5.5: 测量和认证（Week 53-56，可选）⏸️ **未开始**
**设计文档**:
- Realm测量（Measurement）
- 远程认证初步接口

**实现任务**:
1. **RSI_MEASUREMENT_READ**:
   - 计算Realm初始状态的hash
   - 返回测量值

2. **RSI_ATTESTATION_TOKEN_INIT**（占位符）:
   - 生成简单的attestation token
   - 包含测量值和签名（模拟）

**验收**:
- [ ] Realm可以读取自己的测量值
- [ ] 预留完整认证接口

**预估**: 4周（长期目标，可推迟）

---

**Milestone 5 总验收**:
- [ ] 完整RMM实现（RMI + RSI基础）
- [ ] Realm VM成功启动Guest OS
- [ ] 四世界协调稳定（Root/Normal/Secure/Realm）
- [ ] 在ARM FVP上验证通过

**预估总时间**: 16-20周（Week 37-52+）
**状态**: ⏸️ 未开始

---

## 3. 开发节奏

### 3.1 敏捷迭代模式

采用**1-2周短迭代**，每个迭代包括：
- **Day 1**: Sprint计划，确定本周目标
- **Day 2-6**: 开发和测试
  - TDD: 先写测试，再实现
  - 每日提交代码到GitHub
  - 持续集成（CI自动测试）
- **Day 7**: Sprint回顾
  - 验收本周成果
  - 更新文档
  - 发布周报（博客或GitHub Discussion）
  - 调整下周计划

### 3.2 灵活性原则

- **时间弹性**: 每个Sprint可根据实际情况延长或缩短
- **优先级调整**: 遇到阻塞时，可跳过当前模块，先做其他部分
- **技术债管理**: 使用`TODO:`, `FIXME:`, `HACK:`标记，定期回顾
- **快速绕过**: 对于复杂问题，先用简单方案（如静态配置），标记后续优化

### 3.3 文档节奏

每完成一个Sprint，输出以下文档：
- **设计文档**: `docs/design/<module>.md`（Sprint开始前）
- **API文档**: Rust doc comments（开发中）
- **测试报告**: Sprint结束时总结测试覆盖率
- **周报/博客**: 记录进展、挑战、解决方案（公开分享）

---

## 4. 质量保证

### 4.1 TDD测试策略

每个模块遵循**红-绿-重构**循环：
1. **红**: 先写失败的测试
2. **绿**: 实现功能使测试通过
3. **重构**: 优化代码，保持测试通过

**测试层次**:
- **单元测试**: 测试单个函数/模块（Rust `#[test]`）
- **集成测试**: 测试模块间交互（`tests/`目录）
- **端到端测试**: 在QEMU上启动Guest，验证完整流程

**测试覆盖率目标**:
- 核心模块（vCPU, Stage-2, RMM）: >80%
- 其他模块: >60%

### 4.2 持续集成（CI）

GitHub Actions配置：
- **每次提交**: 
  - `cargo check`（编译检查）
  - `cargo clippy`（代码质量）
  - `cargo test`（单元测试）
- **每日构建**:
  - 完整QEMU测试（启动Guest）
  - 覆盖率报告
- **每周构建**:
  - FVP测试（安全特性）
  - 性能基准测试

### 4.3 代码审查

虽然是个人开发，但保持自我审查习惯：
- 每个PR（即使自己合并）写清楚说明
- 定期回顾代码（每月一次）
- 邀请社区Review（开源后）

---

## 5. 风险管理

### 5.1 技术风险

| 风险 | 影响 | 缓解措施 | 应急计划 |
|------|------|----------|----------|
| **RME硬件稀缺** | 高 | 优先在FVP上开发和验证 | 如果FVP不够用，先完成其他模块 |
| **多世界同步复杂** | 高 | 分阶段实现，先两世界再三世界 | 降级：先实现Normal+Secure，Realm推迟 |
| **QEMU限制** | 中 | 查阅QEMU文档，提issue | 自己patch QEMU或用FVP |
| **时间不足** | 中 | 灵活调整优先级 | 降低某些里程碑的验收标准 |
| **技术难题** | 中 | 参考KVM/Xen源码，咨询社区 | 标记TODO，先绕过 |

### 5.2 进度风险

- **应对措施**:
  - 每月评估进度，与计划对比
  - 如果落后>2周，重新评估优先级
  - 砍掉非核心功能（如virtio-blk可延后）

### 5.3 资源风险

- **开发硬件**: 
  - 主力：QEMU（免费）
  - 辅助：ARM FVP（免费，需注册）
  - 可选：云端ARM64机器（AWS Graviton，按需）

- **学习资源**:
  - ARM Architecture Reference Manual（官方免费）
  - 开源项目：KVM, Xen, TF-A, OP-TEE（参考）

---

## 6. 社区和开源

### 6.1 立即开源策略

- **从第一天开始公开**:
  - GitHub仓库：`https://github.com/<你的用户名>/hypervisor`
  - 许可证：MIT + Apache 2.0双授权
  - README说明项目目标和当前状态

- **透明开发**:
  - 所有commits公开
  - Issue tracker开放
  - GitHub Discussions作为论坛

### 6.2 社区建设节奏

- **前3个月（Milestone 0-1）**: 
  - 专注开发，偶尔发博客
  - 欢迎issue和讨论，但不强求贡献

- **3-6个月（Milestone 2-3）**:
  - MVP完成后，写详细的"快速入门"
  - 在Reddit、HN、ARM社区分享
  - 开始接受PR（如果有）

- **6个月后（Milestone 4+）**:
  - 定期技术博客（月度）
  - 参加相关会议（KVM Forum, FOSDEM虚拟或现场）
  - 寻找合作者

### 6.3 文档外化

- **开发者博客系列**（建议主题）:
  1. "从零开始写ARM64 Hypervisor（一）：启动到EL2"
  2. "深入理解Stage-2页表"
  3. "实现FF-A内存共享的挑战"
  4. "Realm Management Extension实战"
  5. "多世界虚拟化的性能优化"

- **文档结构**:
  ```
  docs/
  ├── getting-started.md       # 快速上手
  ├── architecture/            # 架构设计
  │   ├── overview.md
  │   ├── vcpu.md
  │   ├── memory.md
  │   └── security.md
  ├── developer-guide/         # 开发者指南
  │   ├── build.md
  │   ├── testing.md
  │   └── contributing.md
  └── api/                     # API参考（rustdoc生成）
  ```

---

## 7. 时间估算总结

基于个人开发、灵活时间投入：

| Milestone | 描述 | 预估周数 | 累计周数 | 状态 |
|-----------|------|----------|----------|------|
| M0 | 项目启动 | 2周 | 2周 | ✅ 已完成 |
| M1 | MVP - 基础虚拟化 | 8周 | 10周 | ✅ 已完成 |
| M2 | 增强功能 | 8周 | 18周 | ✅ 已完成 |
| M3 | FF-A 实现 + NS-EL2 完善 | 10周 | 28周 | ✅ 核心完成 (Sprint 3.1/3.1b/3.1c/3.2 ✅, ~90%) |
| Android | Android Boot (4 phases) | 4-8周 | — | ✅ Phase 2 完成 (PL031 RTC + Init) |
| M4 | S-EL2 SPMC (QEMU secure=on + TF-A) | 6-8周 | 36周 | 🔧 Sprint 4.1/4.2 ✅ (50%) |
| M4.5 | pKVM 集成 (NS-EL2=pKVM, S-EL2=us) | 4-6周 | 42周 | ⏸️ 未开始 |
| M5 | RME & CCA | 16-20周 | 58-62周 | ⏸️ 未开始 |

**总计**: 约12-14个月（灵活调整）
**当前进度**: 20周 / 52-56周 = **约37%** (按预估周数)
**实际开发时长**: ~4周 (2026-01-25 至 2026-02-20)

---

## 8. 成功标准

### 8.1 技术成功标准

- [x] **M1 MVP**: QEMU启动busybox ✅ **已完成 2026-01-26**
- [x] **M2 增强**: 4 vCPU Linux + virtio-blk + virtio-net + UART RX + GIC emulation ✅ **已完成 2026-02-13**
- [x] **M3 FF-A**: VM 与 SP 内存共享 + 2MB block 拆分 + notifications ✅ **核心完成 2026-02-20** (proxy + stub SPMC + VM-to-VM + 2MB split + notifications + indirect messaging)
- [x] **Android Phase 1**: Linux 6.6.126 + Android config boots to BusyBox shell ✅ **已完成 2026-02-19**
- [x] **Android Phase 2**: PL031 RTC + Android init + 1GB RAM + binderfs ✅ **已完成 2026-02-19**
- [ ] **M4 S-EL2**: 我们的 hypervisor 作为 SPMC 在 S-EL2 运行 (TF-A boot chain) 🔧 **Sprint 4.1/4.2 ✅** (BL33 via TF-A boots Linux)
- [ ] **M4.5 pKVM**: pKVM(NS-EL2) + 我们的 SPMC(S-EL2) + FF-A 端到端 ⏸️ **未开始**
- [ ] **M5 CCA**: Realm VM 启动 Guest OS ⏸️ **未开始**

### 8.2 工程成功标准

- [ ] 代码质量：通过clippy无警告
- [ ] 测试覆盖率：核心模块>80%
- [ ] 文档完善：每个模块有设计文档
- [ ] CI/CD：自动化测试和构建

### 8.3 社区成功标准

- [ ] GitHub stars > 100（6个月）
- [ ] 有外部贡献者提PR（9个月）
- [ ] 技术博客被转载或讨论（6个月）
- [ ] 在技术会议分享（12个月）

---

## 9. 下一步行动

### 🎯 当前位置：M4 Sprint 4.2 ✅ → M4 Sprint 4.3 准备 (Hypervisor 适配 S-EL2 SPMC)
**可行性研究**: `docs/research/2026-02-20-phase4-feasibility.md` — FEASIBLE with moderate effort
**Sprint 4.1/4.2 完成**: TF-A boot chain + hypervisor as BL33 → Linux BusyBox shell

**Phase 8+ 候选方向** (选择一个):

**选项 A**: GICD 全仿真 ✅ **已完成**
- [x] 4KB unmap GICD 区域 (0x08000000) — 16 x 4KB pages
- [x] 全 trap-and-emulate 所有 GICD 寄存器 (VirtualGicd + write-through)
- [x] 消除 guest 对物理 GICD 的直接访问
- [x] GICR2 workaround 移除 — 全部 4 个 GICR 均为 trap-and-emulate
- **已完成**: 2026-02-14

**选项 B**: 多 pCPU 支持 ✅ **已完成**
- [x] Per-pCPU run loop (1:1 vCPU-to-pCPU affinity)
- [x] PSCI CPU_ON boot for secondary pCPUs
- [x] 跨 CPU IPI (physical SGI via ICC_SGI1R_EL1)
- [x] Per-CPU context via TPIDR_EL2, SpinLock-protected DeviceManager
- [x] Physical GICR programming for SGIs/PPIs
- **已完成**: 2026-02-15

**选项 C**: Virtio-net + VSwitch ✅ **已完成**
- [x] VirtioMmioTransport<VirtioNet> @ 0x0a000200 (SPI 17, INTID 49)
- [x] TX/RX virtqueue, 12-byte virtio_net_hdr_v1, process_tx/inject_rx
- [x] L2 VSwitch: MAC 学习 (16 entries), 广播/多播泛洪, 无自回环
- [x] NetRxRing SPSC ring buffer (9 slots, Acquire/Release atomics)
- [x] virtio_slot(n) MMIO 槽位抽象 (slot 0=blk, slot 1=net, stride=0x200)
- [x] Per-VM MAC (52:54:00:00:00:{id+1}), auto-IP (10.0.0.{id+1}/24 via ifconfig)
- [x] drain_net_rx() in run loops, inject_net_rx() in GlobalDeviceManager
- [x] Guest DTB: virtio_mmio@a000200 节点 (SPI 0x11, edge-triggered)
- [x] 3 new test suites: test_net_rx_ring (8), test_vswitch (6), test_virtio_net (8)
- [x] 修复: inject_rx descriptor 泄漏 (undersized → used ring len=0)
- [x] 修复: inject_rx 性能 (byte-by-byte → copy_nonoverlapping)
- [x] 修复: initramfs auto-IP (busybox ifconfig symlink + shell arithmetic)
- [x] 修复: 链接脚本丢失 (build.rs + relocation-model=static)
- **已完成**: 2026-02-18

**选项 D**: FF-A Proxy + Stub SPMC ✅ **已完成 (Phase 1+2)**
- [x] SMC Trap (HCR_TSC=1) + EC_SMC64 + handle_smc() routing
- [x] FFA_VERSION / FFA_ID_GET / FFA_FEATURES / FFA_PARTITION_INFO_GET
- [x] RXTX Mailbox (FFA_RXTX_MAP/UNMAP/RX_RELEASE)
- [x] Stub SPMC (2 SPs, echo messaging, share records)
- [x] Direct Messaging (FFA_MSG_SEND_DIRECT_REQ)
- [x] Memory Sharing (FFA_MEM_SHARE/LEND/RECLAIM, MEM_DONATE blocked)
- [x] Page Ownership (Stage-2 PTE SW bits [56:55], pKVM-compatible)
- [x] Page ownership validation wired into share/reclaim (Stage2Walker from VTTBR)
- [x] S2AP permission modification (RO for share, NONE for lend, RW for reclaim)
- [x] FF-A v1.1 composite memory region descriptor parsing (from TX buffer)
- [x] SMC forwarding to EL3 (forward_smc + probe_spmc + SMCCC pass-through)
- [x] 2 test suites: test_ffa (27), test_page_ownership (4)
- [x] VM-to-VM memory sharing (MEM_RETRIEVE_REQ/RELINQUISH, PER_VM_VTTBR, dynamic Stage-2 mapping)
- [ ] 真实 SPMC 集成 (Hafnium/OP-TEE, multi-endpoint sharing)
- **已完成 (Phase 1)**: 2026-02-18, **(Phase 2)**: 2026-02-19, **(Phase 3 VM-to-VM)**: 2026-02-20

**选项 E**: 完善测试覆盖 ✅ **已完成**
- [x] 接入 test_guest_interrupt (之前导出但未调用)
- [x] 为 GICD/GICR emulation, MMIO decode, global state 添加专项测试
- [x] 消除 test_guest_irq.rs 的 TODO placeholder (替换为 SGI/SPI bitmask 测试)
- [x] 新增 test_decode (9), test_gicd (8), test_gicr (8), test_global (6), test_device_routing (6)
- [x] 扩展 test_dynamic_pagetable (+2 4KB unmap 断言)
- [ ] QEMU integration test 框架 (自动化 make run-linux 验证) — 留待后续
- **结果**: 12→19 test suites, 40→~85 assertions

**选项 F**: 多 VM 支持 ✅ **已完成**
- [x] 多个 VM 实例，独立 Stage-2 页表和 VMID (VTTBR_EL2 bits[63:48])
- [x] 跨 VM 内存隔离 (VM0: 0x48000000-256MB, VM1: 0x68000000-256MB)
- [x] Per-VM DeviceManager (`DEVICES[MAX_VMS]`), 独立 virtio-blk
- [x] Per-VM global state (`VmGlobalState`: SGIs, SPIs, online mask)
- [x] Two-level scheduler: 外层 VM 轮转 + 内层 vCPU 轮转
- [x] `multi_vm` feature flag + `make run-multi-vm` target
- [x] 4 new test suites: vm_state_isolation, vmid_vttbr, multi_vm_devices, vm_activate
- **已完成**: 2026-02-16

**选项 G**: DTB 运行时解析 + 平台抽象
- [x] 从 DTB 动态发现 UART/GIC/RAM 地址 (取代 platform.rs 硬编码) — `src/dtb.rs` (fdt crate v0.1.5)
- [x] 动态 SMP_CPUS (从 DTB cpu 节点读取) — `platform::num_cpus()`, `MAX_SMP_CPUS=8` 编译期容量
- [x] `gicr_rd_base(cpu_id)` / `gicr_sgi_base(cpu_id)` 运行时计算 GICR 帧地址
- [x] DTB test suite (`test_dtb`, 8 assertions)
- [ ] 动态 heap 大小 (基于可用 RAM)
- [ ] 支持非 QEMU virt 平台 (Raspberry Pi 4, 树莓派 CM4)
- **已完成 (核心)**: 2026-02-17 — 剩余: 动态 heap + 非 QEMU 平台

**选项 H**: 性能优化 + 诊断
- [ ] 结构化日志 (DEBUG/INFO/WARN/ERROR 级别，per-module 控制)
- [ ] VMExit 性能计数器 (每类 exit 的次数和延迟)
- [ ] panic handler 增强 (栈回溯、寄存器 dump)
- [ ] 动态 preemption quantum (自适应调度时间片)
- **收益**: 调试效率、性能可观测性

**选项 I**: 完善系统寄存器仿真
- [ ] 扩展 MSR/MRS trap 覆盖 (当前未处理的返回 RAZ/WI)
- [ ] PMU 寄存器仿真 (基础 perf counter)
- [ ] Debug 寄存器完整仿真 (BRK, Watchpoint)
- [ ] SVE/SME context save/restore (当前仅跳过指令)
- [ ] MTE (Memory Tagging Extension) tag save/restore
- **收益**: Guest 兼容性，支持更多 Linux 功能

**选项 J**: PSCI 完善 + 电源管理
- [ ] CPU_SUSPEND 实际实现 (power state 管理)
- [ ] SYSTEM_RESET 实际重启 (QEMU reset)
- [ ] Multi-pCPU CPU_OFF 实际下电 (pCPU WFI 休眠)
- [ ] PSCI MIGRATE 支持
- **收益**: 真实电源管理，接近硬件行为

**选项 K**: 指令解码扩展
- [ ] LDP/STP (load/store pair) 解码 — Linux 常用于 MMIO
- [ ] LDAR/STLR (load-acquire/store-release) 解码
- [ ] ISV=0 fallback 增强 (当前仅 LDR/STR)
- [ ] 错误 MMIO 指令的诊断报告 (当前静默 None)
- **收益**: 减少 guest 异常，支持更多设备驱动

**选项 L**: Stage-2 内存增强
- [ ] 1GB huge page 支持 (减少 TLB miss)
- [ ] Copy-on-Write (CoW) 页面 (内存效率)
- [ ] Guest 内存 balloon (动态伸缩)
- [ ] Stage-2 权限细化 (R/W/X 分离，W^X 保护)
- **收益**: 内存效率、安全隔离

---

### 已完成的里程碑历史

**Milestone 0** (2026-01-25): 项目启动 ✅
**Milestone 1** (2026-01-26): MVP 基础虚拟化 ✅ — QEMU 启动 BusyBox
**Milestone 2** (2026-02-13): 增强功能 ✅ — 4 vCPU Linux + virtio-blk + GIC emulation
**Code Review** (2026-02-15): 8 issues fixed (CRITICAL+HIGH+MEDIUM) ✅ — TERMINAL_EXIT, SpinLock SEV, per-CPU state, LR re-queue

**开发实现阶段**:
- Phase 1: Initramfs (BusyBox, DTB chosen 节点)
- Phase 2: GICD_IROUTER (SPI 路由, shadow state)
- Phase 3: Virtio-MMIO Transport (VirtioDevice trait, VirtioMmioTransport)
- Phase 4: Virtio-blk (内存磁盘, VIRTIO_BLK_T_IN/OUT)
- Phase 5: 4 vCPU SMP (PSCI CPU_ON, SGI emulation, CNTHP preemption)
- Phase 6: 基础设施 (Allocator, 4KB pages, DeviceManager, UART RX)
- Phase 7: GICR Trap-and-Emulate (VirtualGicr per-vCPU 状态)
- Phase 8: GICD Full Trap-and-Emulate (write-through to physical GICD, GICR2 workaround 移除)
- Phase 9: Multi-pCPU (4 vCPUs on 4 physical CPUs, PSCI boot, TPIDR_EL2, SpinLock devices)
- Phase 10: Multi-VM (per-VM Stage-2/VMID, two-level scheduler, per-VM DeviceManager)
- Phase 11: DTB Runtime Parsing (fdt crate, PlatformInfo, gicr_rd_base/sgi_base helpers)
- Phase 12: Virtio-net + VSwitch (L2 switch, NetRxRing SPSC, auto-IP, 3 test suites)
- Phase 13: FF-A v1.1 Proxy + Stub SPMC (SMC trap, VERSION/ID_GET/FEATURES, RXTX mailbox, direct messaging, memory sharing, page ownership PTE SW bits, 2 test suites)
- Phase 14: FF-A Validation + Descriptors + SMC Forwarding (Stage2Walker page ownership validation, S2AP permission control, FF-A v1.1 descriptor parsing, SMC forwarding to EL3, SMCCC pass-through)
- Phase 14.5: VM-to-VM FF-A Memory Sharing (MEM_RETRIEVE_REQ/RELINQUISH, PER_VM_VTTBR, Stage2Walker map/unmap, ShareInfoFull, 9 new test cases)
- Phase 15: Android Boot Phase 1 ✅ **已完成** — Linux 6.6.126 + Android config (Binder IPC) boots to BusyBox shell, `make run-android`
- Phase 16: Android Boot Phase 2 ✅ **已完成** — PL031 RTC emulation, Android DTB, minimal init (shell), Binder+binderfs, 1GB RAM, 30 test suites
- Phase 17: Sprint 3.2 NS-EL2 完善 ✅ **已完成** — 2MB block→4KB split, FF-A notifications (BIND/SET/GET/INFO_GET, 8 endpoints), indirect messaging (MSG_SEND2/MSG_WAIT), SPM_ID_GET + RUN, 44 FF-A test assertions
- Phase 18: Sprint 4.1 TF-A + QEMU 构建基础设施 ✅ **已完成** — TF-A v2.12.0 (SPD=spmd), QEMU 9.2.3 build, BL32 trivial S-EL2 (FFA_MSG_WAIT), `make run-sel2`
- Phase 19: Sprint 4.2 BL33 Hypervisor via TF-A ✅ **已完成** — Linker base 0x40200000, PRELOADED_BL33_BASE, `make run-tfa-linux` (full chain: BL1→BL2→BL31→BL32→BL33→Linux→BusyBox)

---

### Android Boot (并行方向) ✅ **Phase 2 完成**

**目标**: 在 hypervisor 上启动完整 Android (AOSP)，分 4 个阶段

**设计文档**: `docs/plans/2026-02-19-android-boot-design.md`
**实现计划**: `docs/plans/2026-02-19-android-boot-impl.md`

#### Phase 1: Android-configured kernel + BusyBox shell ✅ **已完成 2026-02-19**
- [x] 构建 upstream Linux 6.6.126 LTS + Android config fragment (Binder IPC, Binderfs)
- [x] Docker 构建脚本 (`guest/android/build-kernel.sh`) — 41MB Image
- [x] Config fragment (`guest/android/android-virt.config`) — defconfig + Binder + BTF disabled
- [x] `make run-android` Makefile target (`QEMU_FLAGS_ANDROID` with `-m 2G`)
- [x] 复用现有 Linux DTB + BusyBox initramfs
- [x] 验证: `Linux version 6.6.126` + `smp: Brought up 1 node, 4 CPUs` + virtio-blk/net
- [x] Bug fixes: `probe_spmc()` QEMU crash + stale VTTBR in FF-A tests

#### Phase 2: Android minimal init ✅ **已完成 2026-02-19**
- [x] PL031 RTC emulation (`src/devices/pl031.rs`, ~120 LOC) — counter-based time from CNTVCT/CNTFRQ, PrimeCell ID, 4 unit tests
- [x] Android ramdisk (`guest/android/init.sh` shell script + `init.rc`) — mounts proc/sysfs/devtmpfs, checks binder+binderfs+RTC, prints system info, spawns shell
- [x] 独立 Android DTB (`guest/android/guest.dts`) — PL031 node at 0x9010000 (SPI 2), `androidboot.hardware=virt`
- [x] RAM 增加到 1GB guest (`LINUX_MEM_SIZE=1GB`, QEMU `-m 2G`)
- [x] Docker build script (`guest/android/build-initramfs.sh`) — 1.2MB initramfs
- [x] 设计文档: `docs/plans/2026-02-19-android-phase2-impl.md`
- [x] 验证: PL031 RTC detected + system clock set, Binder IPC + binderfs, 4 CPUs, 929MB RAM, shell prompt

#### Phase 3: Android system partition ⏸️ **未开始**
- [ ] 切换到 AOSP kernel source (`common-android15-6.6` + Clang/LLVM)
- [ ] 多个 virtio-blk (system.img, vendor.img)
- [ ] `android_guest` feature flag + 设备布局重排
- [ ] servicemanager + logd 启动

#### Phase 4: Full Android boot ⏸️ **未开始**
- [ ] 完整 AOSP 服务
- [ ] SELinux permissive
- [ ] `adb shell` via virtio-net

---

## 10. 附录

### 10.1 开发环境

**硬件**:
- 开发机：x86_64 Linux（任何发行版）
- 调试：QEMU 7.0+（aarch64-softmmu）
- 可选：ARM FVP（安全特性验证）

**软件**:
- Rust toolchain（nightly）
- aarch64交叉编译工具链（aarch64-linux-gnu-gcc）
- GDB（gdb-multiarch）
- QEMU（支持ARM虚拟化）

**安装命令**（Ubuntu/Debian）:
```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default nightly
rustup target add aarch64-unknown-none

# 交叉编译工具
sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu

# QEMU
sudo apt install qemu-system-aarch64

# GDB
sudo apt install gdb-multiarch
```

### 10.2 参考资源

**ARM官方文档**:
- ARM Architecture Reference Manual ARMv8/ARMv9（必读）
- ARM RME Specification
- FF-A Specification v1.1/v1.2
- GICv3/v4 Architecture Specification

**开源项目**:
- KVM/ARM（Linux内核）: 参考vCPU和Stage-2实现
- TF-A（ARM Trusted Firmware-A）: 参考EL3和SPM
- OP-TEE: 参考TEE OS
- TF-RMM: 参考RMM实现（官方reference）
- Hafnium: 参考Secure Partition Manager

**书籍和课程**:
- "ARM System Developer's Guide"
- OSDev Wiki（Hypervisor开发）
- MIT 6.828（OS课程，虽然x86但思路通用）

### 10.3 工具推荐

- **代码编辑**: VS Code + rust-analyzer
- **版本控制**: Git + GitHub
- **文档**: Markdown + mdBook（生成在线文档）
- **图表**: draw.io（架构图）
- **性能分析**: perf（Linux）, ARM DS（ARM开发工具）

---

## 11. 总结

这份开发计划基于你的技术背景（ARM64专家 + Rust熟练）和偏好（TDD、敏捷、快速原型）量身定制：

**核心策略**:
1. **自顶向下 + 快速原型**: 快速搭建框架，尽早验证
2. **TDD驱动**: 先写测试，保证质量
3. **分阶段实现安全特性**: FF-A → S-EL2/TEE → RME（符合你的偏好）
4. **立即开源**: 从第一天开始公开，建立社区
5. **灵活调整**: 敏捷迭代，根据实际情况调整计划

**预期成果**:
- 12-14个月后，拥有一个**支持传统虚拟化和机密计算的ARM64 Hypervisor**
- 填补开源领域的空白
- 建立活跃的开发者社区

**第一步**: 创建GitHub仓库，写下第一行代码：`"Hello from EL2!"`

祝开发顺利！🚀

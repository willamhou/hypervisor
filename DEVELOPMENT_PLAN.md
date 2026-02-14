# ARM64 Hypervisor 开发计划

**项目版本**: v0.7.0 (Phase 7 Complete)
**计划制定日期**: 2026-01-26
**最后更新**: 2026-02-13
**计划类型**: 敏捷迭代，灵活调整

---

## 📊 当前进度概览

**整体完成度**: 🟢 **55%** (Milestone 0-2 已完成)

```
M0: 项目启动          ████████████████████ 100% ✅
M1: MVP基础虚拟化     ████████████████████ 100% ✅
M2: 增强功能          ████████████████████ 100% ✅ (当前位置)
M3: FF-A              ░░░░░░░░░░░░░░░░░░░░   0% ⏸️
M4: Secure EL2        ░░░░░░░░░░░░░░░░░░░░   0% ⏸️
M5: RME & CCA         ░░░░░░░░░░░░░░░░░░░░   0% ⏸️
```

**测试覆盖**: 40 assertions / 12 test suites (100% pass)
**代码量**: ~10000+ 行
**Linux启动**: 4 vCPU, BusyBox shell, virtio-blk
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
   - [x] GICD_*（Distributor）: passthrough + shadow state (IROUTER[988], ISENABLER, IPRIORITYR, ICFGR, IGROUPR)
   - [x] GICR_*（Redistributor）: full trap-and-emulate via VirtualGicr (GICR0/1/3), GICR2 passthrough (QEMU workaround)
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
- [x] GIC虚拟化 (GICD shadow + GICR trap-and-emulate)
- [x] 文档完善 (CLAUDE.md全面更新)

**预估总时间**: 8周（Week 11-18）
**实际完成**: ~3周 (2026-01-27 至 2026-02-13)
**状态**: ✅ 已完成

**M2 附加完成项** (超出原计划):
- DynamicIdentityMapper: 堆分配 4KB 页表，split_2mb_block()
- Free-list allocator (BumpAllocator + free_head)
- DeviceManager enum dispatch (Device enum: Uart, Gicd, Gicr, VirtioBlk)
- VirtualGicr per-vCPU 状态仿真
- Custom kernel build via Docker (debian:bookworm-slim)
- 40 test assertions / 12 test suites

---

### Milestone 3: 安全扩展 - FF-A（Week 19-28）⏸️ **未开始**
**目标**: 实现FF-A Hypervisor角色，支持内存共享

根据你的偏好，**先实现FF-A**（因为它是TEE和Realm的通信基础）。

#### Sprint 3.1: FF-A基础框架（Week 19-21）⏸️ **未开始**
**设计文档**:
- FF-A规范解读（v1.1）
- Hypervisor endpoint设计
- 与SPM交互流程

**实现任务**:
1. **FF-A数据结构**:
   - `struct FfaPartition`（表示VM或SP）
   - `struct FfaMessage`（消息缓冲区）
   - Endpoint ID管理（16-bit ID）

2. **基础FF-A调用**:
   - `FFA_VERSION`: 版本协商
   - `FFA_ID_GET`: 获取自己的ID
   - `FFA_FEATURES`: 查询支持的特性
   - `FFA_PARTITION_INFO_GET`: 发现SP

3. **SMC路由**:
   - Guest发起SMC调用
   - 解析Function ID（0x8400_00xx）
   - 转发到SPM或本地处理

**TDD测试**:
- 测试：VM调用FFA_VERSION，收到正确响应
- 测试：枚举SP列表
- 测试：查询特定SP的属性

**验收**:
- [ ] VM可以发现系统中的SP
- [ ] 基础FF-A调用正常工作

**预估**: 3周

---

#### Sprint 3.2: Direct Messaging（Week 22-24）⏸️ **未开始**
**设计文档**:
- Direct request/response消息流
- 寄存器传递约定（X0-X7）

**实现任务**:
1. **FFA_MSG_SEND_DIRECT_REQ**:
   - VM发送请求到SP
   - Hypervisor转发SMC到SPMC
   - 等待SP响应

2. **FFA_MSG_SEND_DIRECT_RESP**:
   - 接收SP的响应
   - 返回给VM

3. **上下文切换**:
   - 保存VM上下文
   - 等待SP响应期间调度其他vCPU

**TDD测试**:
- 测试：VM向SP发送简单请求（echo）
- 测试：SP返回响应，VM收到正确数据
- 测试：多个VM并发调用不冲突

**验收**:
- [ ] VM成功与SP通信（Direct Messaging）
- [ ] 消息正确传递，数据完整

**预估**: 3周

---

#### Sprint 3.3: 内存共享（Week 25-28）⭐ ⏸️ **未开始**
**设计文档**:
- FF-A内存共享语义（share, lend, donate）
- 内存描述符格式
- 权限管理

**实现任务**:
1. **FFA_MEM_SHARE**:
   - VM共享内存页给SP
   - 构建内存描述符（memory region descriptor）
   - 调用SPM分配内存句柄

2. **FFA_MEM_RETRIEVE_REQ/RESP**:
   - SP检索共享内存
   - 映射到SP的地址空间

3. **FFA_MEM_RELINQUISH/RECLAIM**:
   - 内存回收流程
   - 清理Stage-2映射

4. **权限控制**:
   - RO/RW/RWX权限
   - 多方共享（VM1 -> SP1, SP2）

**TDD测试**:
- 测试：VM共享1页给SP，SP成功访问
- 测试：权限控制（RO页不可写）
- 测试：共享后reclaim，SP不可访问
- 测试：多方共享场景

**验收**:
- [ ] VM和SP通过共享内存高效传输数据
- [ ] 权限控制正确
- [ ] 内存生命周期管理正确（无泄漏）

**预估**: 4周

---

**Milestone 3 总验收**:
- [ ] FF-A Hypervisor角色完整实现
- [ ] VM可以通过FF-A与SP通信
- [ ] 内存共享机制工作正常
- [ ] 通过FF-A conformance测试（如果有）

**预估总时间**: 10周（Week 19-28）
**状态**: ⏸️ 未开始

---

### Milestone 4: 安全扩展 - Secure EL2（Week 29-36）⏸️ **未开始**
**目标**: 实现Secure Hypervisor，运行在S-EL2

#### Sprint 4.1: 世界切换框架（Week 29-31）⏸️ **未开始**
**设计文档**:
- Normal/Secure世界状态机
- SCR_EL3.NS位切换
- 上下文保存/恢复（EL2 vs S-EL2）

**实现任务**:
1. **世界切换基础设施**:
   - EL3 Monitor代码（如果自定义）或ARM TF-A集成
   - SMC调用陷入EL3
   - 切换NS位和VTTBR/VSTTBR

2. **双实例架构**:
   - Normal World Hypervisor状态
   - Secure World Hypervisor状态
   - 共享代码路径，独立数据

3. **安全上下文**:
   - 保存/恢复Secure寄存器（VSTTBR_EL2, etc.）
   - S-EL2异常向量表

**TDD测试**:
- 测试：从Normal World通过SMC切换到Secure World
- 测试：上下文正确保存
- 测试：返回Normal World，状态不变

**验收**:
- [ ] 成功在Normal和Secure之间切换
- [ ] 两个世界的Hypervisor独立运行
- [ ] 上下文隔离正确

**预估**: 3周

---

#### Sprint 4.2: TEE VM管理（Week 32-34）⏸️ **未开始**
**设计文档**:
- Secure VM（S-VM）生命周期
- S-EL2的Stage-2页表（VSTTBR_EL2）

**实现任务**:
1. **Secure Stage-2页表**:
   - 独立的页表结构（用于S-EL1 Guest）
   - Secure内存区域分配

2. **S-VM创建和运行**:
   - 创建Secure vCPU
   - 加载TEE OS镜像（OP-TEE）
   - 启动S-VM

3. **Secure中断路由**:
   - FIQ路由到S-EL2
   - 注入到S-VM

**验收**:
- [ ] 在S-EL2创建和运行vCPU
- [ ] Secure内存隔离正确
- [ ] 为OP-TEE集成做好准备

**预估**: 3周

---

#### Sprint 4.3: OP-TEE集成（Week 35-36）⏸️ **未开始**
**设计文档**:
- OP-TEE启动流程
- TA加载和调用

**实现任务**:
1. **OP-TEE作为S-VM**:
   - 加载OP-TEE OS到Secure内存
   - 配置设备树（DTB for OP-TEE）
   - 启动OP-TEE

2. **Normal World Client**:
   - 通过FF-A从Normal VM调用TA
   - 完整的调用链：Normal VM -> Hypervisor (FF-A) -> OP-TEE -> TA

**验收**:
- [ ] OP-TEE成功启动
- [ ] Normal World应用通过FF-A调用TA
- [ ] TA执行并返回结果

**预估**: 2周

---

**Milestone 4 总验收**:
- [ ] Secure Hypervisor运行在S-EL2
- [ ] OP-TEE作为S-VM运行
- [ ] Normal World和Secure World通过FF-A通信
- [ ] TA可以被调用并执行

**预估总时间**: 8周（Week 29-36）
**状态**: ⏸️ 未开始

---

### Milestone 5: 安全扩展 - RME & CCA（Week 37-52+）⏸️ **未开始**
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
| M3 | FF-A实现 | 10周 | 28周 | ⏸️ 未开始 |
| M4 | Secure EL2 & TEE | 8周 | 36周 | ⏸️ 未开始 |
| M5 | RME & CCA | 16-20周 | 52-56周 | ⏸️ 未开始 |

**总计**: 约12-14个月（灵活调整）
**当前进度**: 18周 / 52-56周 = **约33%** (按预估周数)
**实际开发时长**: ~3周 (2026-01-25 至 2026-02-13)

---

## 8. 成功标准

### 8.1 技术成功标准

- [x] **M1 MVP**: QEMU启动busybox ✅ **已完成 2026-01-26**
- [x] **M2 增强**: 4 vCPU Linux + virtio-blk + UART RX + GIC emulation ✅ **已完成 2026-02-13**
- [ ] **M3 FF-A**: VM与SP内存共享成功 ⏸️ **未开始**
- [ ] **M4 TEE**: OP-TEE运行并可调用TA ⏸️ **未开始**
- [ ] **M5 CCA**: Realm VM启动Guest OS ⏸️ **未开始**

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

### 🎯 当前位置：Milestone 2 已完成 ✅

**Phase 8+ 候选方向** (选择一个):

**选项 A**: GICD 全仿真 ⭐
- [ ] 4KB unmap GICD 区域 (0x08000000)
- [ ] 全 trap-and-emulate 所有 GICD 寄存器
- [ ] 消除 guest 对物理 GICD 的直接访问
- **收益**: 完全虚拟化的 GIC Distributor

**选项 B**: 多 pCPU 支持
- [ ] Per-pCPU run loop
- [ ] vCPU affinity 和迁移
- [ ] 跨 CPU IPI
- **收益**: 真正并行执行，显著性能提升

**选项 C**: Virtio-net
- [ ] 新增 virtio-mmio 网络设备
- [ ] TX/RX virtqueue
- [ ] TAP/网络后端
- **收益**: Guest 网络功能

**选项 D**: FF-A (Milestone 3)
- [ ] FFA_VERSION / FFA_ID_GET / FFA_FEATURES
- [ ] SMC 路由框架
- [ ] Direct Messaging + 内存共享
- **收益**: 进入安全扩展阶段

**选项 E**: 完善测试覆盖
- [ ] 接入 test_timer, test_guest_interrupt
- [ ] 为 GICR emulation, virtio-blk, UART RX 添加专项测试
- [ ] QEMU integration test 框架
- **收益**: 提升质量保证

---

### 已完成的里程碑历史

**Milestone 0** (2026-01-25): 项目启动 ✅
**Milestone 1** (2026-01-26): MVP 基础虚拟化 ✅ — QEMU 启动 BusyBox
**Milestone 2** (2026-02-13): 增强功能 ✅ — 4 vCPU Linux + virtio-blk + GIC emulation

**开发实现阶段**:
- Phase 1: Initramfs (BusyBox, DTB chosen 节点)
- Phase 2: GICD_IROUTER (SPI 路由, shadow state)
- Phase 3: Virtio-MMIO Transport (VirtioDevice trait, VirtioMmioTransport)
- Phase 4: Virtio-blk (内存磁盘, VIRTIO_BLK_T_IN/OUT)
- Phase 5: 4 vCPU SMP (PSCI CPU_ON, SGI emulation, CNTHP preemption)
- Phase 6: 基础设施 (Allocator, 4KB pages, DeviceManager, UART RX)
- Phase 7: GICR Trap-and-Emulate (VirtualGicr per-vCPU 状态)

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

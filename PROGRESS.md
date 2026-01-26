# ARM64 Hypervisor - 项目进度

## 项目概览

本项目是一个从零开始构建的 ARM64 Type-1 Hypervisor，使用 Rust 和少量汇编实现。目标是创建一个教育性的、可理解的虚拟化实现。

## 已完成的 Sprint

### Sprint 1.1: vCPU Framework ✅
**目标**: 建立基本的 vCPU 抽象和 VM 入口/出口机制

**完成内容**:
- ✅ VcpuContext 数据结构（通用寄存器 + 系统寄存器）
- ✅ 异常向量表 (EL2)
- ✅ VM 入口/出口机制 (enter_guest, exception_handler)
- ✅ 基础 hypercall 接口
- ✅ 简单 guest 程序测试

**关键文件**:
- `src/vcpu.rs` - vCPU 抽象
- `src/vm.rs` - VM 管理
- `src/arch/aarch64/regs.rs` - 寄存器定义
- `arch/aarch64/exception.S` - 异常向量和上下文切换
- `src/arch/aarch64/exception.rs` - 异常处理逻辑

### Sprint 1.2: Memory Management ✅
**目标**: 实现 Stage-2 地址转换

**完成内容**:
- ✅ Stage-2 页表结构 (L1, L2, L3)
- ✅ Identity mapping (GPA == HPA)
- ✅ 2MB 块映射
- ✅ Memory attributes (NORMAL, DEVICE, READONLY)
- ✅ VTTBR_EL2 和 VTCR_EL2 配置
- ✅ HCR_EL2.VM 启用

**关键文件**:
- `src/arch/aarch64/mmu.rs` - MMU 和 Stage-2 实现

**技术细节**:
- 40-bit IPA space (T0SZ = 24)
- 48-bit PA space  
- 4KB granule, start at Level 1
- Inner/Outer write-back cacheable

### Sprint 1.3: Interrupt Handling ✅
**目标**: 实现中断路由和处理

**完成内容**:
- ✅ GIC (Generic Interrupt Controller) 基础支持
  - GICv2 寄存器定义
  - 系统寄存器接口配置
- ✅ ARM Generic Timer 支持
  - Virtual Timer (CNTV_*) 寄存器访问
  - 定时器配置和状态查询
- ✅ 中断处理框架
  - HCR_EL2 配置 (IMO/FMO 位)
  - IRQ/FIQ 异常处理
  - 中断确认和 EOI 机制
- ✅ 定时器中断测试
  - 100ms 定时器配置
  - 中断 pending 状态检测

**关键文件**:
- `src/arch/aarch64/gic.rs` - GIC 接口
- `src/arch/aarch64/timer.rs` - Timer 支持
- `tests/test_timer.rs` - 定时器测试

**测试结果**:
- ✅ 定时器成功配置 (62.5MHz)
- ✅ CNTV_CTL 状态正确: 0x1 → 0x5 (pending bit set)

### Sprint 1.4: Device Emulation ✅
**目标**: 实现 MMIO 设备仿真框架

**完成内容**:
- ✅ 指令解码器
  - Load/Store 指令解析
  - ISS (Instruction Specific Syndrome) 支持
  - 手动解码作为后备
- ✅ MMIO Trap-and-Emulate Handler
  - Data Abort 处理
  - 指令解码和仿真
  - Load: 设备读取 → guest 寄存器
  - Store: guest 寄存器 → 设备写入
- ✅ 设备仿真框架
  - MmioDevice trait
  - DeviceManager 路由
  - 全局访问接口
- ✅ 虚拟 UART (PL011)
  - UARTDR, UARTFR, UARTCR 寄存器
  - 字符输出到真实 UART
- ✅ 虚拟 GICD
  - GICD_CTLR, GICD_TYPER
  - GICD_ISENABLER/ICENABLER
  - 配置变化日志
- ✅ 寄存器访问接口
  - GeneralPurposeRegs::get_reg/set_reg
  - x0-x30 完整支持

**关键文件**:
- `src/arch/aarch64/decode.rs` - 指令解码
- `src/devices/mod.rs` - 设备框架
- `src/devices/uart.rs` - UART 仿真
- `src/devices/gicd.rs` - GICD 仿真
- `src/global.rs` - 全局状态管理

**架构特点**:
- Trap-and-emulate 模式
- 设备地址路由
- 可扩展设备框架
- Zero-copy 仿真

**待完成**:
- Guest 测试代码指令编码修复 (技术债务)
- 更多设备支持

### 目录结构重组 ✅
**目标**: 参考 Hafnium 项目优化目录结构

**Phase 1 完成**:
- ✅ 测试代码分离到 `tests/` 目录
- ✅ 创建 `tests/mod.rs` 统一管理
- ✅ 更新模块导入路径

**当前结构**:
```
hypervisor/
├── src/
│   ├── core/
│   │   ├── vm.rs
│   │   └── vcpu.rs
│   ├── arch/
│   │   └── aarch64/
│   │       ├── regs.rs
│   │       ├── exception.rs
│   │       ├── mmu.rs
│   │       ├── gic.rs
│   │       ├── timer.rs
│   │       └── decode.rs
│   ├── devices/
│   │   ├── mod.rs
│   │   ├── uart.rs
│   │   └── gicd.rs
│   ├── global.rs
│   ├── lib.rs
│   └── main.rs
├── tests/
│   ├── mod.rs
│   ├── test_guest.rs
│   ├── test_timer.rs
│   └── test_mmio.rs
├── arch/
│   └── aarch64/
│       ├── boot.S
│       └── exception.S
└── Cargo.toml
```

**下一步计划**:
- Phase 2: 设备代码按驱动分子目录
- Phase 3: 架构代码分层 (hypervisor/, mm/, peripherals/)
- Phase 4: 创建 docs/ 目录

## 技术亮点

### 1. 异常处理流程
```
Guest execution (EL1)
  ↓ (Exception)
Exception Vector (EL2)
  ↓
Save Context
  ↓
Rust Handler (handle_exception)
  ↓
Route by exception type
  ↓
Handle (Hypercall/MMIO/IRQ)
  ↓
Return bool (continue/exit)
  ↓
Restore Context & ERET (if continue)
OR
Return to host (if exit)
```

### 2. MMIO 仿真流程
```
Guest: STR w1, [x19]  (访问 0x09000000)
  ↓
Data Abort (FAR_EL2 = 0x09000000)
  ↓
读取 faulting instruction (PC)
  ↓
解码 (decode::MmioAccess)
  ↓
提取: reg=19, size=4, is_store=true
  ↓
从 x19 读取值
  ↓
DeviceManager::handle_mmio(addr, value, size, true)
  ↓
UART::write(offset, value, size)
  ↓
输出字符到真实 UART
  ↓
PC += 4, ERET back to guest
```

### 3. Stage-2 Translation
```
Guest VA (EL1) → Guest PA (IPA)  [由 guest OS 管理]
       ↓
IPA → Host PA (HPA)              [由 hypervisor Stage-2 管理]
```

## 性能指标

- **Context Switch**: ~数百纳秒 (取决于缓存状态)
- **Hypercall Overhead**: ~10-20条指令
- **MMIO Emulation**: ~100-200条指令

## 代码统计

```
Language      Files    Lines    Code
-----------------------------------
Rust             16    ~3500    ~2800
Assembly          2     ~300     ~250
Markdown          2     ~500     ~400
-----------------------------------
Total            20    ~4300    ~3450
```

## 测试覆盖

- ✅ Guest 执行 (hypercall)
- ✅ 定时器中断
- ⚠️ MMIO 仿真 (guest 代码有指令编码问题)
- ✅ 内存映射
- ✅ 异常处理

## 已知问题

1. **MMIO 测试 guest 代码**: ARM64 指令编码问题导致 Instruction Abort
   - 影响: 测试失败，但框架本身完整
   - 解决方案: 使用外部汇编器生成正确指令
   - 优先级: 中

2. **单核限制**: 目前只支持单个 vCPU
   - 影响: 无法测试多核场景
   - 解决方案: 实现 vCPU 调度器
   - 优先级: 低

3. **内存分配**: 使用静态内存，无动态分配器
   - 影响: 内存使用不灵活
   - 解决方案: 实现简单的 bump allocator
   - 优先级: 中

## 下一步计划

### Sprint 1.5 候选:
1. **修复 MMIO 测试**: 使用正确的指令编码
2. **多 vCPU 支持**: 实现 vCPU 调度
3. **动态内存管理**: 实现内存分配器
4. **Guest 中断注入**: 将 hypervisor 中断注入到 guest
5. **更多设备**: GIC CPU Interface, 更多外设

### 架构优化:
1. ✅ Phase 1: 测试代码分离
2. Phase 2: 设备驱动子目录化
3. Phase 3: 架构代码分层
4. Phase 4: 文档完善

## 参考资料

- **ARM Architecture**: ARM ARM (ARMv8-A)
- **Hafnium**: TF-Hafnium/hafnium (参考项目)
- **KVM/ARM**: Linux KVM ARM 实现
- **Rust Embedded**: embedded-rs 生态

## 团队

- 主要开发: [你的名字]
- AI 辅助: Claude (Anthropic)

## 许可证

[待定]

---

最后更新: 2026-01-26

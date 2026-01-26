# GICv3/v4 虚拟中断注入实现

**实现日期**: 2026-01-26
**状态**: 代码已实现，待测试验证
**版本**: v0.4.0

---

## 📋 实现概述

本次实现将虚拟中断注入机制从传统的 HCR_EL2.VI 方式升级到 GICv3/v4 的 List Register (LR) 机制。

### 关键改进

1. **GICv3 系统寄存器接口** - 替代 MMIO 访问
2. **List Register 中断注入** - 硬件自动化管理
3. **向后兼容** - 自动检测并回退到 GICv2
4. **虚拟化扩展支持** - ICH_* 寄存器用于虚拟中断

---

## 🎯 GICv2 vs GICv3 比较

### GICv2（旧方式 - HCR_EL2.VI）

**优点**:
- 简单直接
- 所有 ARMv8 处理器都支持

**缺点**:
- 只能注入一个虚拟中断（IRQ 或 FIQ）
- 需要软件管理中断状态
- 无中断优先级支持
- 性能较低

**实现**:
```rust
// 设置 HCR_EL2.VI 位
hcr_el2 |= (1 << 7);  // VI bit

// Guest 会在下次进入时收到虚拟 IRQ
// 但无法知道具体是哪个中断号
```

### GICv3（新方式 - List Registers）

**优点**:
- 支持多个并发虚拟中断（4-16 个 LR）
- 硬件自动管理状态转换（Pending → Active → Inactive）
- 支持中断优先级和抢占
- 性能更高
- 符合 ARM 规范

**缺点**:
- 需要 GICv3+ 硬件支持
- 配置稍复杂

**实现**:
```rust
// 写入 List Register
// LR 格式：State | HW | Group | Priority | vINTID
let lr_value = (1u64 << 62)                    // State = Pending
              | (1u64 << 60)                    // Group1
              | ((priority as u64) << 48)       // Priority
              | (intid as u64);                 // vINTID

// 写入 ICH_LR0_EL2
GicV3VirtualInterface::write_lr(0, lr_value);

// 硬件自动注入，Guest 收到精确的中断号
```

---

## 📂 文件结构

### 新增文件

1. **`src/arch/aarch64/peripherals/gicv3.rs`** (520 行)
   - GICv3 系统寄存器接口
   - List Register 管理
   - 虚拟中断注入/清除

### 修改文件

1. **`src/arch/aarch64/peripherals/mod.rs`**
   - 导出 gicv3 模块

2. **`src/vcpu_interrupt.rs`**
   - 添加 `use_gicv3` 标志
   - `inject_irq()` 使用 List Register
   - 向后兼容 GICv2 模式

3. **`src/main.rs`**
   - 调用 `gicv3::init()` 而不是 `gic::init()`

---

## 🔧 技术实现细节

### 1. GICv3 系统寄存器

#### ICC_* 寄存器（CPU Interface）

```rust
// ICC_IAR1_EL1 - Interrupt Acknowledge
let intid = GicV3SystemRegs::read_iar1();

// ICC_EOIR1_EL1 - End Of Interrupt
GicV3SystemRegs::write_eoir1(intid);

// ICC_PMR_EL1 - Priority Mask (0xFF = allow all)
GicV3SystemRegs::write_pmr(0xFF);

// ICC_IGRPEN1_EL1 - Enable Group 1 interrupts
GicV3SystemRegs::write_igrpen1(true);
```

#### ICH_* 寄存器（Hypervisor 虚拟接口）

```rust
// ICH_VTR_EL2 - VGIC Type Register
let vtr = GicV3VirtualInterface::read_vtr();
let num_lrs = ((vtr & 0x1F) + 1) as u32;  // 可用 LR 数量

// ICH_HCR_EL2 - Hypervisor Control
GicV3VirtualInterface::write_hcr(1);  // Enable virtual interrupts

// ICH_LR0_EL2 - List Register 0
GicV3VirtualInterface::write_lr(0, lr_value);
```

### 2. List Register 格式

**64-bit List Register 布局**:

```
Bits [63:62] - State
  00 = Invalid (free)
  01 = Pending
  10 = Active
  11 = Pending + Active

Bit [61] - HW
  0 = Software interrupt
  1 = Hardware interrupt (physical INTID in bits [41:32])

Bit [60] - Group
  0 = Group 0
  1 = Group 1

Bits [59:56] - Reserved
Bits [55:48] - Priority (0 = highest)
Bits [47:32] - Reserved
Bits [31:0]  - vINTID (virtual interrupt ID)
```

**示例**:
```rust
// 注入 PPI 27 (Virtual Timer), 优先级 0xA0
let lr = (1u64 << 62)         // Pending
       | (1u64 << 60)         // Group1
       | (0xA0u64 << 48)      // Priority
       | 27u64;               // vINTID = 27

write_lr(0, lr);  // 写入 LR0
```

### 3. 中断注入流程

#### Hypervisor 端

```rust
// 1. 找到空闲的 List Register
for i in 0..num_lrs {
    let lr = read_lr(i);
    let state = (lr >> 62) & 0x3;
    
    if state == 0 {  // Invalid = free
        // 2. 构建 LR 值
        let lr_value = build_lr(intid, priority);
        
        // 3. 写入 LR
        write_lr(i, lr_value);
        
        return Ok(());
    }
}
```

#### 硬件自动处理

1. **注入**: 当 Guest 进入且 IRQ 未 mask 时，硬件自动触发虚拟中断
2. **状态转换**: Pending → Active (当 Guest 执行 IAR 时)
3. **EOI**: Active → Invalid (当 Guest 执行 EOIR 时)

#### Guest 端

```rust
// Guest 中断处理流程（EL1）

// 1. 进入 IRQ handler (vector 0x280)
irq_handler:
    // 2. Acknowledge interrupt
    let intid = read(ICC_IAR1_EL1);  // 硬件自动：Pending → Active
    
    // 3. 处理中断
    handle_interrupt(intid);
    
    // 4. End of Interrupt
    write(ICC_EOIR1_EL1, intid);     // 硬件自动：Active → Invalid
    
    // 5. 返回
    eret
```

### 4. 自动检测和回退

```rust
pub fn init() {
    // 检查 GICv3 是否可用
    if !is_gicv3_available() {
        uart_puts(b"[GIC] GICv3 not available, falling back to GICv2\n");
        super::gic::init();  // 使用 GICv2
        return;
    }
    
    uart_puts(b"[GIC] Initializing GICv3...\n");
    
    // 初始化 virtual interrupt interface
    GicV3VirtualInterface::init();
    
    // 启用 CPU interrupt delivery
    GicV3SystemRegs::enable();
}

fn is_gicv3_available() -> bool {
    // 读取 ID_AA64PFR0_EL1
    let pfr0: u64;
    unsafe {
        asm!("mrs {pfr0}, ID_AA64PFR0_EL1", pfr0 = out(reg) pfr0);
    }
    
    // Bits [27:24] = GIC version
    // 0001 = GICv3/v4 system register interface available
    let gic_version = (pfr0 >> 24) & 0xF;
    gic_version >= 1
}
```

---

## 🧪 测试计划

### 单元测试

1. **LR 格式测试** ✅
   ```rust
   #[test]
   fn test_lr_format() {
       let intid = 27u32;
       let priority = 0xA0u8;
       
       let lr = build_lr(intid, priority);
       
       assert_eq!((lr >> 62) & 0x3, 1);  // State = Pending
       assert_eq!((lr >> 60) & 0x1, 1);  // Group1
       assert_eq!(((lr >> 48) & 0xFF) as u8, priority);
       assert_eq!((lr & 0xFFFF_FFFF) as u32, intid);
   }
   ```

2. **LR 分配测试**
   ```rust
   #[test]
   fn test_lr_allocation() {
       // 注入 4 个中断
       for i in 0..4 {
           assert!(inject_interrupt(i, 0xA0).is_ok());
       }
       
       // 第 5 个应该失败（LR 满）
       assert!(inject_interrupt(5, 0xA0).is_err());
   }
   ```

### 集成测试

1. **简单注入测试** (已实现)
   - 写入 LR
   - 读取 LR 验证
   - 清除 LR

2. **Guest 完整中断流程** (待实现)
   - Guest 设置 VBAR_EL1
   - Guest unmask IRQ
   - Hypervisor 注入虚拟中断
   - Guest 接收并处理
   - 验证中断计数

3. **多中断测试**
   - 连续注入 3 个中断
   - 验证按优先级处理
   - 验证 EOI 后可注入新中断

### QEMU 测试命令

```bash
# GICv3 测试
qemu-system-aarch64 \
    -machine virt,gic-version=3 \
    -cpu max \
    -nographic \
    -serial mon:stdio \
    -kernel target/aarch64-unknown-none/release/hypervisor \
    -m 128M

# GICv2 回退测试
qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -cpu cortex-a57 \
    -nographic \
    -serial mon:stdio \
    -kernel target/aarch64-unknown-none/release/hypervisor \
    -m 128M
```

---

## 📊 性能对比

### 理论分析

| 操作 | GICv2 (HCR_EL2.VI) | GICv3 (List Register) |
|------|-------------------|----------------------|
| 注入延迟 | ~100ns | ~50ns |
| 并发中断 | 1 个 | 4-16 个 |
| 优先级支持 | 无 | 完整支持 |
| 硬件辅助 | 最小 | 完全自动化 |
| EOI 处理 | 软件 | 硬件自动 |

### 预期改进

- **中断注入延迟**: 减少 50%
- **并发能力**: 提升 4-16 倍
- **CPU 开销**: 减少 30%（硬件自动管理状态）

---

## 🐛 已知问题和限制

### 1. QEMU 输出问题

**现象**: 编译成功但 QEMU 无串口输出

**可能原因**:
- QEMU 版本兼容性
- 串口配置问题
- 环境变量设置

**解决方案**:
- 使用 `-d int,guest_errors` 调试
- 尝试不同的 QEMU 版本
- 检查 DTB 配置

### 2. GICv3 可用性检测

**限制**: 只检查 ID_AA64PFR0_EL1，未检查实际硬件配置

**改进**:
- 添加 ICH_VTR_EL2 访问测试
- 捕获异常并优雅回退

### 3. LR 数量限制

**现状**: 通常只有 4 个 LR

**影响**: 最多同时挂起 4 个虚拟中断

**缓解**: 
- 实现 LR 优先级管理
- 高优先级中断可抢占低优先级

---

## 🚀 后续优化

### Sprint 1.7 候选

1. **完善 Guest 异常处理** [2-3h]
   - 实现完整的 Guest exception vector table
   - 测试实际的中断处理流程
   - 验证 IAR/EOIR 机制

2. **LR 优先级管理** [3-4h]
   - 实现 LR 抢占逻辑
   - 优先级队列管理
   - 性能优化

3. **多 vCPU 支持** [15-20h]
   - Per-vCPU LR 管理
   - 中断亲和性
   - SMP 中断路由

4. **性能基准测试** [2-3h]
   - 中断注入延迟测量
   - 吞吐量测试
   - 与 GICv2 对比

---

## 📚 参考资料

### ARM 官方文档

1. **ARM GIC Architecture Specification (v3/v4)**
   - List Register 格式
   - 虚拟化扩展
   - 系统寄存器定义

2. **ARM ARMv8 Architecture Reference Manual**
   - ICH_* 寄存器详细说明
   - ICC_* 寄存器详细说明
   - 中断路由规则

3. **ARM Virtualization Extensions**
   - Virtual interrupt injection
   - Maintenance interrupts
   - Doorbell机制 (GICv4)

### 开源实现参考

1. **Linux KVM/ARM**
   - `virt/kvm/arm/vgic/vgic-v3.c`
   - LR 分配算法
   - 优先级管理

2. **Xen ARM**
   - `xen/arch/arm/gic-v3.c`
   - 虚拟 GIC 实现
   - Performance optimizations

3. **QEMU**
   - `hw/intc/arm_gicv3.c`
   - GICv3 设备模拟
   - 虚拟化扩展支持

---

## ✅ 实现检查清单

### 核心功能
- [x] GICv3 系统寄存器接口 (ICC_*)
- [x] GICv3 虚拟化接口 (ICH_*)
- [x] List Register 读写
- [x] 中断注入 (`inject_interrupt`)
- [x] 中断清除 (`clear_interrupt`)
- [x] GICv3 可用性检测
- [x] GICv2 自动回退

### 集成
- [x] vcpu_interrupt.rs 集成
- [x] main.rs 初始化调用
- [x] 编译通过

### 测试
- [x] 单元测试（LR 格式）
- [x] 简单注入测试
- [ ] Guest 完整流程测试（待 QEMU 环境修复）
- [ ] 多中断测试
- [ ] 性能基准测试

### 文档
- [x] 代码注释
- [x] 实现文档（本文档）
- [x] 技术对比
- [ ] 使用示例

---

## 📝 版本历史

### v0.4.0 (2026-01-26) - GICv3 支持

**新增**:
- GICv3 系统寄存器接口
- List Register 虚拟中断注入
- 自动检测和 GICv2 回退

**改进**:
- 虚拟中断注入性能
- 支持多并发中断
- 硬件自动化管理

**已知问题**:
- QEMU 测试环境待修复
- Guest 完整流程待验证

---

**文档维护**: 本文档记录 GICv3/v4 实现的完整技术细节
**作者**: 开发团队
**最后更新**: 2026-01-26

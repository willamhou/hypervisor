# ARM64 Hypervisor 项目需求文档

**项目版本**: v0.2.0  
**文档创建日期**: 2026-01-26  
**最后更新日期**: 2026-01-26

---

## 1. 项目概述

### 1.1 项目目标
开发一个基于ARM64架构的Type-1（裸机）Hypervisor，主要用于虚拟化操作系统场景。项目采用Rust语言实现，追求高度模块化和可扩展的插件化架构，注重工程质量、代码可读性和可维护性。

**重要扩展（v0.2.0新增）**: 项目将实现完整的ARM安全架构支持，包括：
- **TEE支持**: 作为Secure Hypervisor运行在Secure EL2，管理可信执行环境
- **FF-A集成**: 实现完整的Firmware Framework for Armv8-A Hypervisor角色
- **CCA支持**: 实现生产级的ARM Confidential Compute Architecture和Realm Management Extension (RME)
- **多世界协调**: 统一管理Normal World、Secure World和Realm World的虚拟化

### 1.2 应用场景
- **主要场景**: 虚拟化完整的Guest操作系统（Linux、Android等）
- **安全场景（新增）**: 
  - 机密计算（Confidential Computing）和隐私保护
  - 金融、医疗等安全敏感行业的隔离虚拟化
  - 多租户云环境中的可信执行
  - TEE应用开发和测试平台
- **目标用户**: ARM虚拟化技术专家、系统软件研发人员、安全研究人员、开源社区贡献者
- **使用环境**: 服务器虚拟化、开发测试环境、云计算基础设施、机密计算平台

### 1.3 项目定位
- **Hypervisor类型**: Type-1（裸机型），直接运行在硬件上，无需Host OS
- **技术背景要求**: 面向具有ARM虚拟化专家级知识的开发者
- **项目属性**: 开源项目，长期研发，注重社区建设

---

## 2. 功能性需求

### 2.1 核心虚拟化能力

#### 2.1.1 CPU虚拟化
- **必需特性**:
  - 支持ARMv8 EL2异常级别，实现基本的CPU虚拟化
  - vCPU创建、调度和上下文切换
  - 异常（Exception）和陷入（Trap）处理
  - PSCI（Power State Coordination Interface）支持，用于vCPU电源管理

- **高级特性**:
  - 完整的SMP支持（对称多处理）
  - vCPU到物理CPU的灵活调度，支持负载均衡
  - CPU热插拔支持
  - 性能计数器虚拟化

#### 2.1.2 内存虚拟化
- **必需特性**:
  - Stage-2页表管理（Guest物理地址到Host物理地址转换）
  - VMID（Virtual Machine Identifier）管理
  - 内存隔离和保护机制
  - 基础内存分配和回收

- **渐进式增强**:
  - 初期实现静态内存分配
  - 后续支持内存过载分配
  - 长期目标：内存气球、页面共享（KSM）、内存压缩等高级特性

#### 2.1.3 中断虚拟化
- **必需特性**:
  - 支持GICv3/GICv4（Generic Interrupt Controller）虚拟化扩展
  - 虚拟中断注入和处理
  - 中断路由和优先级管理
  - 支持LPI（Locality-specific Peripheral Interrupt）

### 2.2 Guest操作系统支持

#### 2.2.1 支持的Guest OS
- **第一阶段**（MVP）: Linux内核（主流版本，如5.x+、6.x+）
- **第二阶段**: 
  - 多种GNU/Linux发行版
  - Android操作系统
- **长期规划**: 
  - BSD系统家族
  - RTOS（如FreeRTOS、Zephyr）

#### 2.2.2 引导和生命周期管理
- **必需功能**:
  - Guest OS引导加载（支持Linux Image、设备树DTB）
  - Guest虚拟机创建、启动、暂停、恢复、关闭
  - Guest状态保存和恢复（快照功能）

### 2.3 设备虚拟化

#### 2.3.1 设备虚拟化策略（混合方式）
项目采用灵活的混合设备虚拟化策略，根据设备类型和性能需求选择合适的方式：

- **全模拟**（Trap-and-Emulate）:
  - 用于简单设备或调试场景
  - UART、RTC等低速设备

- **半虚拟化设备**（Para-virtualization）:
  - 主要采用virtio标准
  - virtio-net（网络）
  - virtio-blk（块设备）
  - virtio-console（控制台）

- **设备直通**（Device Passthrough）:
  - 支持IOMMU（SMMU v3）
  - 将物理设备直接分配给Guest
  - 用于高性能网卡、GPU等设备

#### 2.3.2 必需的虚拟设备
- 虚拟串口（UART）用于Guest控制台
- 虚拟定时器（Generic Timer）
- 虚拟GIC（中断控制器）
- 虚拟块设备（存储）
- 虚拟网络设备

### 2.4 ARM64特定特性支持

- **基础虚拟化（必需）**:
  - ARMv8-A Exception Level 2 (EL2)
  - Stage-2地址转换
  - Virtualization Host Extensions (VHE) 支持（后期）

- **中断虚拟化（必需）**:
  - GICv3虚拟化扩展
  - GICv4 LPI虚拟化（可选）

- **高级特性（渐进式）**:
  - 嵌套虚拟化（VHE）- 允许在虚拟机中运行虚拟机
  - Pointer Authentication（指针认证）支持
  - Memory Tagging Extension (MTE) 支持
  - SVE（Scalable Vector Extension）虚拟化（长期）

### 2.5 安全架构扩展（v0.2.0新增）

#### 2.5.1 TEE（Trusted Execution Environment）支持

本项目将实现**Secure Hypervisor**功能，运行在ARM TrustZone的Secure EL2异常级别。

##### 核心能力

- **Secure World虚拟化**:
  - 在Secure EL2运行Hypervisor代码
  - 管理Secure World中的虚拟机（S-VM）
  - 支持从EL3（Monitor模式）到S-EL2的世界切换
  - Secure和Non-secure状态的上下文管理

- **TEE OS支持**:
  - **首要支持**: OP-TEE（Open Portable TEE）
    - 支持OP-TEE作为Secure Guest运行
    - 实现OP-TEE需要的SMC调用转发
    - Trusted Application (TA) 加载和生命周期管理
  - **未来扩展**: 
    - Trusty（Google的TEE OS）
    - 其他符合GlobalPlatform TEE规范的OS

- **EL3固件协作**:
  - **初期**: 依赖ARM Trusted Firmware (ATF/TF-A)提供EL3运行时服务
    - 使用标准的PSCI、SMCCC (SMC Calling Convention)
    - 与TF-A的SPM (Secure Partition Manager) 协同
  - **长期**: 开发自定义的轻量级EL3 Monitor，减少依赖复杂度

##### 架构设计

- **统一代码库**: 
  - Hypervisor核心代码可在EL2（Normal World）和S-EL2（Secure World）两种模式下运行
  - 通过条件编译和运行时配置区分Normal/Secure行为
  - 共享内存管理、调度、中断等核心模块

- **隔离机制**:
  - 依赖TrustZone硬件隔离（NS位）
  - Secure和Non-secure内存严格分离
  - 设备资源分区（Secure外设、Non-secure外设）

##### 实现阶段

- **Phase 3-4（中期目标）**: 
  - 基础S-EL2支持和EL3切换
  - OP-TEE集成和TA运行
  - Secure World虚拟机生命周期管理

#### 2.5.2 FF-A（Firmware Framework for Armv8-A）支持

实现**FF-A Hypervisor**角色，管理Secure Partitions (SP)并协调Normal World和Secure World的通信。

##### FF-A核心功能

- **Hypervisor角色实现**:
  - 作为FF-A的Hypervisor endpoint
  - 管理VM到SP的消息路由
  - 实现FF-A v1.1/v1.2规范的核心接口

- **SP管理**:
  - Secure Partition发现和枚举
  - SP生命周期管理（初始化、运行、销毁）
  - SP资源管理（内存、中断、权限）

- **消息传递**:
  - **Direct Messaging**: VM与SP的直接消息
    - `FFA_MSG_SEND_DIRECT_REQ/RESP`
    - 低延迟的同步调用
  - **Indirect Messaging**: 通过共享缓冲区的异步消息
    - `FFA_MSG_SEND/WAIT`
    - 支持大数据量传输
  - **中断处理**: FF-A中断虚拟化和路由

- **内存共享**:
  - `FFA_MEM_SHARE/LEND/DONATE`: VM与SP之间的内存共享
  - 细粒度权限控制（RO、RW、Execute）
  - 支持内存reclaim机制

##### FF-A与SPM集成

- **与TF-A SPM协同**:
  - Hypervisor通过SMC与SPMC (Secure Partition Manager Core)通信
  - 代理VM的FF-A调用到SPMC
  - 处理SPMC返回的响应和中断

- **虚拟化FF-A接口**:
  - 每个VM有独立的FF-A视图
  - 虚拟化VMID和SP ID映射
  - 隔离不同VM的FF-A资源

##### 实现阶段

- **Phase 3-4（中期目标）**:
  - FF-A v1.1基础接口
  - Direct messaging和基础内存共享
  - 与OP-TEE通过FF-A通信

- **Phase 5+（长期）**:
  - FF-A v1.2/v2.0新特性
  - Indirect messaging和高级内存管理
  - 完整的中断虚拟化

#### 2.5.3 RME和CCA支持（Realm Management Extension & Confidential Compute Architecture）

实现**生产级CCA平台**，支持ARM v9的机密计算能力，运行完整的Realm Manager (RMM)。

##### CCA核心概念

ARM CCA引入了第四个安全状态：**Realm World**，位于Normal和Secure之外，提供硬件级的机密性保护。

- **四层架构**:
  - **EL3**: Monitor/Root固件（TF-A）
  - **Realm EL2**: Realm Manager (RMM)，本项目实现的核心
  - **Realm EL1**: Realm VM的Guest OS
  - **Realm EL0**: Realm VM的应用程序

##### RMM实现（核心目标）

- **Realm生命周期管理**:
  - **创建**: 
    - 通过`RMI_REALM_CREATE`由Normal World Hypervisor触发
    - 初始化Realm的RTT（Realm Translation Table）
    - 分配Realm ID (RID)
  - **运行**:
    - vCPU调度和执行
    - Realm VM的Stage-2地址转换（使用RTT）
    - Realm异常处理和VM exit处理
  - **销毁**:
    - 内存清零和资源回收
    - 防止数据泄露

- **内存管理**:
  - **Granule Protection Table (GPT)**:
    - 硬件强制的内存访问控制
    - 每个物理页（granule）标记为Normal、Secure、Realm或Root
    - 防止跨世界的非授权访问
  - **Realm Translation Table (RTT)**:
    - Realm专用的Stage-2页表
    - 支持多级页表（4KB、16KB、64KB页）
    - Protected IPA空间管理
  - **内存加密**:
    - 依赖硬件内存加密扩展（可选）
    - Realm内存与Normal/Secure隔离

- **RMI（Realm Management Interface）**:
  - Normal World Hypervisor调用RMM的接口
  - 关键命令：
    - `RMI_REALM_CREATE/DESTROY`
    - `RMI_REC_CREATE/DESTROY` (Realm Execution Context，即vCPU)
    - `RMI_REC_ENTER/EXIT`
    - `RMI_RTT_CREATE/DESTROY`
    - `RMI_DATA_CREATE` (分配内存给Realm)

- **RSI（Realm Service Interface）**:
  - Realm Guest调用RMM的接口
  - 关键服务：
    - `RSI_ATTESTATION_TOKEN_INIT/CONTINUE`: 远程认证
    - `RSI_MEASUREMENT_READ`: 读取度量值
    - `RSI_HOST_CALL`: 与Host Hypervisor通信（受限）
    - `RSI_IPA_STATE_SET`: 管理IPA状态（Protected/Unprotected）

##### 远程认证（Remote Attestation）

- **初期（Phase 3-4）**: 预留接口
  - 定义RSI_ATTESTATION相关API
  - 基础的度量（measurement）收集

- **长期（Phase 5+）**: 完整实现
  - 生成符合CCA规范的Attestation Token
  - 集成硬件Root of Trust（如DICE、TPM）
  - 支持PSA（Platform Security Architecture）认证流程
  - 与Veraison等认证框架集成

##### 多世界协调（Normal/Secure/Realm）

本项目的核心挑战和创新点在于**三个安全世界的统一管理**：

- **世界切换管理**:
  - **Normal ↔ Secure**: 通过SMC和NS位切换
  - **Normal ↔ Realm**: 通过RMI调用和GPT切换
  - **Secure ↔ Realm**: 受限切换，仅EL3可协调
  - **上下文保存/恢复**: 
    - 通用寄存器、系统寄存器
    - GIC状态（中断）
    - Stage-2页表指针（VTTBR_EL2/VSTTBR_EL2）
    - 定时器状态

- **内存管理**:
  - **GPT协调**: 
    - 配置每个物理页的安全属性
    - 与EL3固件协同管理GPT
  - **跨世界共享**（显式共享策略）:
    - 仅允许显式标记的共享区域
    - 严格的权限检查（只读、只写、禁止执行）
    - 使用FF-A机制在Normal/Secure间共享
    - Realm的共享内存标记为Unprotected IPA

- **中断处理**:
  - **中断路由**:
    - Normal World中断 → EL2处理或注入到Normal VM
    - Secure中断 → 陷入EL3，路由到S-EL2或S-EL1
    - Realm中断 → RMM处理或注入到Realm VM
  - **中断注入**:
    - 虚拟化GIC（vGIC）分别为三个世界维护
    - 中断优先级和屏蔽管理
  - **跨世界中断**:
    - Secure调用产生的FIQ
    - FF-A通知中断

- **DMA和IOMMU（SMMU）**:
  - **SMMU配置**:
    - 设备隔离到特定世界
    - Stream ID和Context ID管理
  - **DMA隔离**:
    - Normal设备只能访问Normal内存
    - Secure设备访问Secure内存
    - Realm设备限制（一般禁止或通过bounce buffer）

##### 统一代码库架构

为了同时支持Normal EL2、Secure EL2和Realm EL2，采用**统一代码库、多实例运行**策略：

```rust
// 伪代码示例
enum SecurityWorld {
    Normal,
    Secure,
    Realm,
}

struct HypervisorContext {
    world: SecurityWorld,
    vttbr: u64,  // Stage-2页表基址（Normal/Secure）或RTT（Realm）
    vmid: u16,
    // ... 其他上下文
}

impl HypervisorContext {
    fn handle_vm_exit(&mut self) {
        match self.world {
            SecurityWorld::Normal => { /* Normal world处理 */ },
            SecurityWorld::Secure => { /* Secure world处理 */ },
            SecurityWorld::Realm => { /* Realm world处理，遵循RMI/RSI */ },
        }
    }
}
```

- **共享模块**:
  - CPU虚拟化核心（vCPU调度、上下文切换）
  - 内存分配器（需要world-aware）
  - 中断管理框架
  - 日志和调试设施

- **特化模块**:
  - Normal World: 标准VM管理、virtio设备
  - Secure World: TEE管理、FF-A处理
  - Realm World: RMM核心逻辑、RTT管理、RSI实现

##### 实现阶段

- **Phase 3-4（中期目标）**:
  - 基础RME支持和GPT配置
  - RMM核心功能（Realm创建、REC管理、RTT）
  - 基本的RMI和RSI接口
  - 在FVP上验证Realm VM启动

- **Phase 5+（长期，生产级CCA）**:
  - 完整的RMI/RSI规范实现
  - 远程认证和度量
  - 性能优化（减少世界切换开销）
  - 在支持RME的真实硬件上验证
  - 安全审计和认证（符合ARM CCA认证要求）

#### 2.5.4 安全特性总结

| 特性 | 实现阶段 | 关键技术点 |
|------|----------|------------|
| **TEE支持** | Phase 3-4 | S-EL2实现、OP-TEE集成、EL3协同 |
| **FF-A** | Phase 3-4 | Hypervisor角色、消息传递、内存共享 |
| **RME基础** | Phase 3-4 | RMM核心、RTT、基础RMI/RSI |
| **远程认证** | Phase 5+ | Attestation Token、度量、集成认证框架 |
| **生产级CCA** | Phase 5+ | 完整规范、性能优化、安全认证 |

这些安全特性的加入，使本Hypervisor项目成为ARM平台上为数不多的**同时支持传统虚拟化和机密计算的开源Hypervisor**。

---

## 3. 非功能性需求

### 3.1 性能要求

#### 3.1.1 优先级排序
1. **工程质量优先**: 代码可读性、可维护性、可扩展性
2. **功能完整性**: 确保核心功能稳定可靠
3. **性能优化**: 在保证前两者的基础上进行性能优化

#### 3.1.2 性能目标（渐进式）
- **初期**: 功能正确性为主，性能可接受
- **中期**: 
  - VM exit延迟 < 5μs
  - 上下文切换开销 < 10μs
  - 内存虚拟化开销 < 10%
- **长期**: 接近或达到KVM在ARM64上的性能水平

### 3.2 安全性和隔离性

#### 3.2.1 基础安全（必需）
- EL2/EL1权限隔离，防止Guest访问Hypervisor资源
- Stage-2页表强制内存隔离
- 设备访问控制，防止Guest直接操作物理设备

#### 3.2.2 增强安全（重点）
- **内存安全**:
  - Stage-2页表权限细粒度控制（R/W/X）
  - 防止Guest通过DMA攻击（IOMMU/SMMU保护）
  - 敏感数据加密（可选，长期）
  - **新增**: GPT（Granule Protection Table）硬件隔离
  - **新增**: RTT（Realm Translation Table）内存保护

- **攻击面最小化**:
  - Hypervisor代码量精简
  - 最小权限原则
  - 安全编码规范（Rust内存安全特性）
  - **新增**: 多世界隔离最小化跨世界接口

- **安全审计**:
  - 关键操作日志记录
  - 安全事件追踪机制
  - 支持安全审计工具集成
  - **新增**: Realm度量和远程认证

#### 3.2.3 机密计算安全（v0.2.0新增）

- **硬件隔离机制**:
  - TrustZone NS位强制Normal/Secure隔离
  - RME GPT硬件强制四世界隔离
  - SMMU设备级DMA隔离

- **内存保护策略**:
  - **禁止隐式共享**: 默认禁止跨世界内存访问
  - **显式共享**: 
    - Normal/Secure通过FF-A `FFA_MEM_SHARE`显式共享
    - Realm通过Unprotected IPA机制共享
    - 严格的权限验证（读写执行权限）
  - **加密保护**: 
    - Realm内存硬件加密（依赖硬件特性）
    - 防止物理内存攻击

- **远程认证**:
  - **初期**: 预留RSI_ATTESTATION接口
  - **长期**: 
    - 完整的CCA Attestation Token生成
    - 度量Realm的初始状态和运行时完整性
    - 集成硬件Root of Trust
    - 支持PSA和Veraison认证协议

#### 3.2.4 安全性分析和验证

- **安全工具链**:
  - **静态分析**: Rust clippy、cargo-audit
  - **动态分析**: AddressSanitizer（用户态工具）
  - **模糊测试**: cargo-fuzz对关键接口进行fuzz
  - **形式化验证**: 对关键模块（如RTT管理、世界切换）进行形式化验证

- **威胁建模**:
  - 定义攻击面（Guest攻击Hypervisor、跨世界攻击）
  - STRIDE威胁分析
  - 针对性的安全测试用例

- **安全认证目标（长期）**:
  - 符合ARM CCA认证要求
  - 通过PSA Certified Level 2/3
  - 通过Common Criteria EAL4+
  - 开源社区安全审计

#### 3.2.5 安全性与性能平衡

- **关键路径优化**:
  - 世界切换路径：目标 < 2μs
  - RMI调用延迟：目标 < 5μs
  - 虚拟中断注入：目标 < 3μs

- **可配置安全等级**:
  - **Strict模式**: 最严格检查，适合生产环境
  - **Balanced模式**: 平衡性能和安全（默认）
  - **Performance模式**: 最小检查，仅开发测试使用

- **零拷贝优化**:
  - FF-A内存共享避免不必要的拷贝
  - 使用硬件保护而非软件校验

#### 3.2.6 未来增强
- 形式化验证关键代码路径（重点：RMM核心、世界切换）
- 符合安全认证标准（ARM CCA、PSA Certified、Common Criteria）
- 支持更多硬件安全特性（MTE、BTI、PAC）

### 3.3 实时性

#### 3.3.1 当前需求
- **初期**: 不作为首要目标
- **架构设计**: 预留实时性扩展能力

#### 3.3.2 长期规划
- 支持软实时特性（可预测的中断延迟）
- 为硬实时场景预留接口（RTOS Guest支持）
- vCPU固定绑定物理核心（CPU pinning）

### 3.4 多核和SMP支持

#### 3.4.1 完整SMP支持（重点）
- **必需特性**:
  - 支持多个物理CPU核心
  - Guest SMP支持（多vCPU）
  - vCPU灵活调度，非强制绑定物理核
  
- **调度策略**:
  - 负载均衡算法
  - vCPU亲和性配置
  - CPU热插拔支持

#### 3.4.2 并发和同步
- 多核间TLB同步
- 锁优化（spinlock、RCU等）
- 无锁数据结构（部分场景）

### 3.5 可测试性和可调试性

#### 3.5.1 测试策略（多层次）
1. **QEMU模拟器**:
   - 主要开发和测试环境
   - 快速迭代和调试
   - 支持aarch64 virt机型

2. **真实硬件测试**:
   - 树莓派4/5（开发板）
   - ARM64服务器（如Ampere Altra、AWS Graviton）
   - 验证性能和硬件兼容性

3. **完善调试体系**:
   - GDB调试支持（QEMU + 真实硬件）
   - JTAG/SWD硬件调试器支持
   - 内置trace和日志系统
   - 性能分析工具（perf、tracing）

#### 3.5.2 测试覆盖
- 单元测试（Rust test框架）
- 集成测试（Guest OS启动测试）
- 性能基准测试
- 压力测试和稳定性测试

### 3.6 可维护性和可扩展性

#### 3.6.1 代码组织（插件化架构）
- **高度模块化设计**:
  ```
  hypervisor/
  ├── core/           # 核心Hypervisor框架
  ├── arch/           # 架构相关代码（ARM64）
  ├── mm/             # 内存管理
  │   ├── stage2/     # Stage-2页表
  │   ├── gpt/        # Granule Protection Table（RME）
  │   └── rtt/        # Realm Translation Table（RME）
  ├── cpu/            # CPU虚拟化
  ├── irq/            # 中断管理
  ├── devices/        # 设备虚拟化
  │   ├── virtio/
  │   └── passthrough/
  ├── security/       # 安全扩展（v0.2.0新增）
  │   ├── tee/        # TEE支持
  │   ├── ffa/        # FF-A实现
  │   ├── rmm/        # Realm Manager
  │   ├── rmi/        # Realm Management Interface
  │   ├── rsi/        # Realm Service Interface
  │   └── smc/        # SMC调用处理
  ├── plugins/        # 插件系统
  └── tools/          # 配套工具
  ```

- **插件机制**:
  - 设备虚拟化插件动态加载
  - 可扩展的设备模型接口
  - 第三方设备支持能力

#### 3.6.2 API设计
- 清晰的内部模块接口
- 稳定的对外API（用户态工具）
- 版本化和兼容性管理

---

## 4. 技术选型

### 4.1 开发语言

#### 4.1.1 主语言：Rust
- **选择理由**:
  - 内存安全（无需GC，零成本抽象）
  - 现代化工具链（Cargo、rustfmt、clippy）
  - 强大的类型系统和错误处理
  - 适合系统级编程

- **使用范围**:
  - Hypervisor核心代码
  - 设备模拟器
  - 用户态管理工具

#### 4.1.2 汇编语言
- **使用场景**:
  - EL2异常入口/出口处理
  - 上下文切换关键路径
  - MMIO访问等硬件相关操作

### 4.2 工具链和依赖

#### 4.2.1 编译工具链
- Rust nightly（支持裸机开发特性）
- LLVM/Clang（交叉编译支持）
- aarch64-unknown-none 或自定义target

#### 4.2.2 关键依赖库
- `no_std` Rust生态系统
- 设备树解析库（dtb/fdt）
- 虚拟化相关库（virtio实现）
- 日志和调试库
- **安全扩展相关**:
  - ARM TF-A库接口（SMC调用）
  - FF-A规范实现库
  - 加密库（用于认证，如RustCrypto）

#### 4.2.3 开发工具
- **仿真和测试平台**:
  - QEMU（7.0+，支持aarch64虚拟化、TrustZone、RME模拟）
  - ARM FVP（Fixed Virtual Platform）- RME和CCA验证关键工具
  - 真实硬件（支持RME的ARM v9处理器）
- **调试工具**:
  - GDB（多架构调试，支持EL2/S-EL2）
  - JTAG/SWD硬件调试器
  - ARM DS（Development Studio）
- **开发辅助**:
  - Rust Analyzer（IDE支持）
  - 代码覆盖率工具（llvm-cov）
  - 安全分析工具（clippy、cargo-audit、cargo-fuzz）

---

## 5. 项目规划

### 5.1 时间计划

#### 5.1.1 项目属性
- **类型**: 长期项目（6-12个月或更长）
- **性质**: 开源研发项目
- **发布节奏**: 迭代式开发，定期发布里程碑版本

#### 5.1.2 里程碑规划

**Phase 1: 基础框架（Month 1-3）**
- 项目脚手架和构建系统
- EL2初始化和异常处理框架
- 基础Stage-2页表实现
- 简单的UART输出

**Phase 2: 最小可行产品/MVP（Month 4-6）**
- 完整的vCPU管理
- 内存虚拟化基本功能
- GIC虚拟化
- 能够启动一个简单的Linux Guest（busybox + 最小内核）

**Phase 3: 功能增强（Month 7-9）**
- 多vCPU和SMP支持
- virtio设备支持（net、blk）
- 设备树传递
- 启动标准Linux发行版

**Phase 3.5: 安全扩展基础（Month 9-12）** ⭐ **v0.2.0新增重点**
- TEE支持：S-EL2实现，OP-TEE集成
- FF-A基础：Hypervisor角色、消息传递
- RME基础：RMM核心、基础RMI/RSI、Realm创建和运行
- 在QEMU和FVP上验证安全特性

**Phase 4: 性能和稳定性（Month 13-15）**
- 性能优化和profiling（包括安全路径）
- 压力测试和bug修复
- 完善文档和示例（包括安全文档）
- 社区推广

**Phase 5+: 高级特性（长期）**
- 设备直通和SMMU
- 嵌套虚拟化（VHE）
- 实时性增强
- **生产级CCA（重点）**:
  - 完整的RMI/RSI规范实现
  - 远程认证（Attestation Token、度量）
  - FF-A高级特性（v1.2、间接消息）
  - 多TEE OS支持
- **安全审计和认证**:
  - ARM CCA认证
  - PSA Certified
  - 形式化验证
  - 独立安全审计

### 5.2 交付物

#### 5.2.1 代码交付
- Hypervisor核心代码（MIT/Apache 2.0双授权）
- 管理工具和实用程序
- 测试套件和基准测试
- 示例配置和Guest镜像

#### 5.2.2 文档交付（开源+社区）
- **设计文档**:
  - 架构设计文档
  - 各模块详细设计文档
  - ARM64虚拟化技术总结
  - **安全架构文档**（v0.2.0新增）:
    - TEE/FF-A/RME架构设计
    - 多世界协调机制
    - 安全威胁模型和缓解措施
    - 远程认证流程

- **开发者文档**:
  - API参考文档
  - 插件开发指南
  - 代码贡献指南

- **用户文档**:
  - 快速入门指南
  - 安装和配置手册
  - 故障排除FAQ
  - **安全使用指南**（v0.2.0新增）:
    - TEE应用开发指南
    - Realm VM创建和管理
    - 远程认证集成

- **社区建设**:
  - 技术博客系列
  - 在线教程和视频
  - 定期社区会议

#### 5.2.3 社区建设
- GitHub开源仓库
- 问题追踪和讨论区
- CI/CD自动化测试
- 贡献者社区建设

---

## 6. 风险评估

### 6.1 技术风险

| 风险项 | 描述 | 影响 | 缓解措施 |
|--------|------|------|----------|
| ARM硬件复杂性 | ARM64虚拟化扩展细节多，文档分散 | 高 | 深入研读ARM Architecture Reference Manual，参考现有实现（KVM） |
| 调试困难 | 裸机Hypervisor调试复杂 | 中 | 优先使用QEMU开发，建立完善的日志系统 |
| 性能优化难度 | 达到生产级性能需要深度优化 | 中 | 工程质量优先，性能优化分阶段进行 |
| Rust生态限制 | no_std环境下可用库有限 | 低 | 必要时自行实现，或参考已有项目 |
| **安全特性复杂度** | **TEE/FF-A/RME规范复杂，实现难度大** | **高** | **分阶段实现，先基础后高级；使用FVP充分验证** |
| **硬件依赖** | **RME需要ARM v9硬件，可用性有限** | **中** | **优先在FVP验证，逐步迁移到真实硬件** |
| **多世界同步问题** | **三世界协调存在复杂的竞态条件** | **中** | **严格的锁机制，充分的并发测试** |
| **安全漏洞风险** | **安全代码路径出现漏洞影响严重** | **高** | **代码审查、安全测试、形式化验证** |

### 6.2 项目风险

| 风险项 | 描述 | 影响 | 缓解措施 |
|--------|------|------|----------|
| 时间投入不足 | 长期项目需持续投入 | 中 | 合理安排里程碑，允许灵活调整 |
| 范围蔓延 | 功能需求不断增加 | 中 | 严格区分MVP和增强特性，分阶段实现 |
| 社区参与度 | 开源项目依赖社区活跃度 | 低 | 高质量文档和示例，积极推广和互动 |

---

## 7. 成功标准

### 7.1 MVP阶段成功标准
- [ ] 能够在QEMU上启动简单Linux Guest（busybox）
- [ ] Guest可以正常运行用户态程序
- [ ] 基本的虚拟设备（UART、Timer）工作正常
- [ ] 代码结构清晰，有基础文档

### 7.2 完整功能阶段成功标准
- [ ] 支持标准Linux发行版（Ubuntu、Debian等）启动
- [ ] 多vCPU和SMP稳定工作
- [ ] virtio网络和存储设备功能完整
- [ ] 在真实ARM64硬件上验证通过
- [ ] 性能达到可接受水平（对比KVM差距<30%）

### 7.3 安全扩展阶段成功标准（v0.2.0新增）
- [ ] **TEE支持**: OP-TEE在Secure EL2成功运行，可加载TA
- [ ] **FF-A**: 实现Hypervisor角色，支持VM与SP的消息传递和内存共享
- [ ] **RME基础**: 在FVP上成功创建和运行Realm VM
- [ ] **多世界协调**: Normal/Secure/Realm三世界稳定切换和共存
- [ ] **性能目标**: 
  - 世界切换延迟 < 2μs
  - RMI调用延迟 < 5μs
  - FF-A消息传递延迟 < 10μs

### 7.4 成熟项目阶段成功标准
- [ ] 社区有外部贡献者参与
- [ ] 文档完善，用户可自行上手
- [ ] 通过持续集成和测试覆盖
- [ ] 有实际应用案例
- [ ] 技术博客和教程被广泛传播

### 7.5 生产级CCA阶段成功标准（长期目标）
- [ ] **完整RMM**: 实现所有RMI/RSI接口，符合ARM RMM规范
- [ ] **远程认证**: 生成有效的CCA Attestation Token，通过Veraison验证
- [ ] **真实硬件验证**: 在ARM v9硬件（如Neoverse V2/N2）上运行
- [ ] **安全认证**: 通过ARM CCA认证、PSA Certified Level 2+
- [ ] **性能生产级**: 
  - Realm VM性能开销 < 15%（对比Normal VM）
  - 世界切换延迟 < 1μs
  - 支持100+并发Realm VM
- [ ] **应用案例**: 有真实的机密计算应用部署（如机密容器、TEE数据库）

---

## 8. 附录

### 8.1 参考资料

#### 8.1.1 ARM官方文档
- ARM Architecture Reference Manual ARMv8/ARMv9 (ARM DDI 0487)
- ARM Generic Interrupt Controller Architecture Specification
- Server Base System Architecture (SBSA)
- PSCI (Power State Coordination Interface) Specification
- **安全扩展文档**（v0.2.0新增）:
  - ARM TrustZone Technology
  - ARM Firmware Framework for Armv8-A (FF-A) Specification v1.1/v1.2
  - ARM Confidential Compute Architecture (CCA)
  - Realm Management Extension (RME) Architecture Specification
  - Realm Management Monitor (RMM) Specification
  - ARM Security Model (Realm Management Interface and Realm Service Interface)

#### 8.1.2 开源项目参考
- KVM/ARM：Linux内核中的ARM虚拟化实现
- Xen on ARM：Type-1 Hypervisor参考
- ACRN Hypervisor：嵌入式场景Type-1 Hypervisor
- Rust VMM项目：Rust虚拟化组件库
- crosvm：Chrome OS虚拟机监视器（Rust实现）
- **安全相关项目**（v0.2.0新增）:
  - ARM Trusted Firmware-A (TF-A)：EL3固件和SPM实现
  - OP-TEE：开源TEE OS
  - Hafnium：ARM开源的Secure Partition Manager
  - TF-RMM：ARM官方RMM参考实现
  - Veraison：远程认证框架

#### 8.1.3 技术博客和论文
- KVM Forum 演讲稿（ARM虚拟化相关）
- FOSDEM、Linux Plumbers Conference技术分享
- 学术论文（虚拟化性能优化、安全性）

### 8.2 术语表

| 术语 | 全称 | 说明 |
|------|------|------|
| EL0/1/2/3 | Exception Level | ARM异常级别，EL2用于Hypervisor |
| S-EL0/1/2 | Secure Exception Level | Secure World的异常级别 |
| Stage-2 | Second Stage Translation | Guest物理地址到Host物理地址转换 |
| GIC | Generic Interrupt Controller | ARM通用中断控制器 |
| VMID | Virtual Machine Identifier | 虚拟机标识符 |
| PSCI | Power State Coordination Interface | 电源状态协调接口 |
| SMMU | System Memory Management Unit | ARM的IOMMU实现 |
| virtio | Virtual I/O | 半虚拟化设备标准 |
| VHE | Virtualization Host Extensions | 虚拟化主机扩展（ARMv8.1+） |
| SMP | Symmetric Multi-Processing | 对称多处理 |
| MVP | Minimum Viable Product | 最小可行产品 |
| **TEE** | **Trusted Execution Environment** | **可信执行环境** |
| **FF-A** | **Firmware Framework for Armv8-A** | **ARMv8-A固件框架** |
| **SP** | **Secure Partition** | **安全分区，运行在Secure EL0/1** |
| **SPM** | **Secure Partition Manager** | **安全分区管理器** |
| **SPMC** | **SPM Core** | **SPM核心组件** |
| **RME** | **Realm Management Extension** | **Realm管理扩展（ARMv9）** |
| **CCA** | **Confidential Compute Architecture** | **机密计算架构** |
| **RMM** | **Realm Manager** | **Realm管理器，运行在Realm EL2** |
| **RMI** | **Realm Management Interface** | **Normal World调用RMM的接口** |
| **RSI** | **Realm Service Interface** | **Realm Guest调用RMM的接口** |
| **GPT** | **Granule Protection Table** | **硬件强制的内存保护表** |
| **RTT** | **Realm Translation Table** | **Realm专用的Stage-2页表** |
| **REC** | **Realm Execution Context** | **Realm的vCPU** |
| **IPA** | **Intermediate Physical Address** | **Guest物理地址** |
| **PA** | **Physical Address** | **真实物理地址** |
| **TA** | **Trusted Application** | **运行在TEE中的应用** |
| **SMC** | **Secure Monitor Call** | **EL3调用指令** |
| **SMCCC** | **SMC Calling Convention** | **SMC调用规范** |
| **FVP** | **Fixed Virtual Platform** | **ARM官方仿真平台** |
| **PSA** | **Platform Security Architecture** | **平台安全架构标准** |

### 8.3 变更记录

| 版本 | 日期 | 变更说明 | 作者 |
|------|------|----------|------|
| v0.1.0 | 2026-01-26 | 初始需求文档创建 | 项目团队 |
| v0.2.0 | 2026-01-26 | 重大更新：增加TEE、FF-A、RME/CCA完整支持 | 项目团队 |

---

## 9. 总结

本需求文档定义了一个雄心勃勃但务实的ARM64 Hypervisor开发项目：

**核心特点**:
- Type-1裸机Hypervisor，面向虚拟化操作系统场景
- 纯Rust实现，追求现代化和内存安全
- 插件化架构，高度模块化和可扩展
- 工程质量优先，重视代码可维护性

**技术重点**:
- 完整SMP支持，灵活的vCPU调度
- 混合设备虚拟化策略（模拟+半虚拟化+直通）
- **强化的安全性（v0.2.0）**:
  - TEE支持：Secure Hypervisor（S-EL2）
  - FF-A集成：完整Hypervisor角色实现
  - CCA支持：生产级RMM和机密计算能力
  - 多世界协调：Normal/Secure/Realm统一管理
- 为未来实时性和嵌套虚拟化预留扩展能力

**项目定位**:
- 长期开源项目，注重社区建设
- 渐进式开发，分阶段交付里程碑
- 既是生产级工具，也是学习和研究平台

**v0.2.0重大更新**：增加了对ARM最新安全技术的全面支持，使本项目成为**为数不多的同时支持传统虚拟化和机密计算的开源Hypervisor**。这不仅是技术创新，也填补了开源领域在ARM安全虚拟化方面的空白。

这份需求文档将作为项目开发的指导性文件，随着项目进展持续更新和完善。

# 你的 Android 手机里藏着一个 Hypervisor：pKVM 完全解析

> 为什么 Google 要在你的手机芯片上跑两个 hypervisor？从 Pixel 6 到 SESIP Level 5 认证，Android 安全架构的一次大跃迁。

---

从 Android 13 开始，每一部 Pixel 手机的 CPU 里都在跑一个你看不见的 hypervisor：**pKVM（Protected KVM）**。它不是后台服务，不是 App，而是一段运行在 hypervisor 特权级（EL2，比操作系统内核的 EL1 更高）的代码，连 Android 内核的内存访问都受它管控。

2025 年 8 月，pKVM 获得了 **SESIP Level 5** 认证。SESIP 是针对 IoT/移动平台的安全评估标准（不同于 Common Criteria 全评估，但其 Level 5 对应 CC 中最高级别的漏洞分析 AVA_VAN.5）。据 Google 安全博客称，这是首个面向消费电子大规模部署且达到此级别的软件系统。

这篇文章解释 pKVM 是什么、为什么需要它、它怎么工作，以及它和 TrustZone 的关系。

## 一个根本性的问题：内核被 root 了怎么办？

传统 Android 的安全模型有一个假设：**内核是可信的**。所有的权限隔离——App 沙箱、SELinux、文件加密——都建立在"内核不会被攻破"这个前提上。

但现实是，Android 内核包含上千万行代码，加上厂商驱动、GPU 驱动、基带接口，攻击面巨大。每年的 Android 安全公告里都有 kernel 层面的提权漏洞（CVE）。一旦攻击者拿到 kernel 权限：

- 所有 App 的内存都可以读取
- 所有文件加密密钥都暴露
- 生物识别数据、DRM 密钥、AI 推理的个人数据——全部透明

pKVM 的回答是：**即使内核被完全攻破，受保护虚拟机（pVM）里的数据也无法被访问。**

需要强调的是：pKVM 保护的是 **pVM 内部**的工作负载，不是所有 App 数据。普通 App 仍然运行在 Host 的 EL1 上，内核被 root 后这些数据仍然暴露。pKVM 缩小的是"内核提权后的最大损失范围"——把最敏感的操作（AI 推理、密钥管理、DRM）搬进 pVM，即使 Host 沦陷也保得住这部分。

## pKVM 到底做了什么？

pKVM 是 Google 对 Linux KVM 的扩展，已经合并进 Linux 主线内核。它的核心创新用一句话概括：

> **让 ARM 的第二级地址翻译（Stage-2）不仅在 Guest 运行时生效，在 Host 上下文中也持续生效。**

什么意思？ARM 虚拟化扩展提供了两级地址翻译：

```
Guest 虚拟地址 → Stage-1（Guest 控制）→ Guest 物理地址
    → Stage-2（Hypervisor 控制）→ 真实物理地址
```

标准 KVM（VHE 模式，ARMv8.1+）里，Host 内核直接运行在 EL2，天然不受 Stage-2 约束，可以自由访问所有物理内存——包括 Guest 的。

pKVM 的核心改变是：**放弃 VHE，把 Host 内核降级到 EL1**，使其也受 Stage-2 管控。EL2 只运行精简的 pKVM hypervisor 代码。当一个 pVM 启动时，它的内存页面从 Host 的 Stage-2 映射中**移除**。Host 内核的 CPU 访问这些页面会触发缺页异常——就像这些内存不存在一样。

这个设计带来约 3-5% 的性能开销（多一次 EL1↔EL2 跳转），Google 认为这是值得的安全权衡。

```
                ┌─────────────────────────────┐
   EL2          │  pKVM (掌控所有 Stage-2 页表) │
                │  Host 和 Guest 的内存都受管控  │
                └──────────┬──────────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
   ┌──────────┐     ┌──────────┐     ┌──────────┐
   │ Host     │     │ pVM 1    │     │ pVM 2    │
   │ Android  │     │ Microdroid│    │ AI 推理  │
   │ (EL1)    │     │ (EL1)    │     │ (EL1)    │
   └──────────┘     └──────────┘     └──────────┘
   ↑ 不能访问 pVM 内存    彼此隔离         彼此隔离
```

这和传统虚拟化的区别是：**Normal World 内的最小信任基从整个 Host 内核缩小到了 EL2 的 pKVM 代码**（信任根仍在 EL3 的 TF-A 固件）。Host 变成了一个"不完全受信任的管理器"——它可以调度 pVM（决定什么时候给 CPU 时间），但不能读取 pVM 里的数据。

**重要限制**：pKVM 的 Stage-2 只约束 CPU 的内存访问。要防御 DMA 攻击（攻击者通过 GPU、NIC 等设备直接读写物理内存），还需要 IOMMU/SMMU 硬件配合。Pixel 手机上配置了 SMMUv3 + pKVM 的 IOMMU 驱动，两者缺一不可。此外，pKVM 不防御微架构侧信道攻击（Spectre 类）——ARM v8.5+ 引入了部分缓解措施，但并非完全消除。

## pKVM vs TrustZone：不是替代，是共存

很多人会问：ARM 不是已经有 TrustZone 了么？为什么还需要 pKVM？

TrustZone 把 CPU 分成两个世界：

| | Normal World | Secure World |
|---|---|---|
| 运行内容 | Android, Linux, Apps | TEE OS, 密钥管理, DRM |
| 权限控制 | OS/Hypervisor | TrustZone 硬件隔离 |
| 优点 | 功能丰富 | 攻击面极小 |
| 缺点 | 内核被攻破则全部暴露 | 功能受限，无法跑 Linux |

TrustZone 的问题不是安全性不够，而是**功能太受限**。Secure World 不能跑完整的 Linux 环境，各芯片厂商（高通、三星、联发科）的 TEE OS 互不兼容，第三方代码（DRM 库、AI 模型）必须在高特权环境中运行但缺乏隔离。

pKVM 解决的是另一个问题：**在 Normal World 内部建立隔离**。它不替代 TrustZone，而是和 TrustZone 并行：

```
            Normal World              Secure World
           ┌──────────────┐         ┌──────────────┐
    EL0    │  Apps         │         │              │
           ├──────────────┤         ├──────────────┤
    EL1    │  Android 内核 │         │  安全分区     │
           │  + pVM (隔离)  │        │  (TEE)       │
           ├──────────────┤         ├──────────────┤
    EL2    │  pKVM         │         │  SPMC        │
           │  (NS-EL2)     │         │  (S-EL2)     │
           └──────┬────────┘         └──────┬───────┘
                  │       ┌──────┐          │
    EL3           └───────│ TF-A │──────────┘
                          │ SPMD │
                          └──────┘
```

pKVM 在 NS-EL2，SPMC（安全分区管理器）在 S-EL2。两者通过 EL3 的 TF-A 固件协调，使用 FF-A（Firmware Framework for Arm）协议通信。

S-EL2 这个位置目前的生产实现是 Google 的 Hafnium（20 万行 C）。我用约 3 万行代码（26K Rust + 3.4K ARM64 汇编）做了一个实验性实现，在 QEMU 上和 pKVM 跑在同一颗虚拟芯片上。如果你对 S-EL2 SPMC 的内部细节感兴趣，文末有链接。

## AVF：完整的虚拟化框架

pKVM 只是 hypervisor，Google 在它之上构建了完整的 **AVF（Android Virtualization Framework）**：

```
App / 系统服务
    ↕ RpcBinder (跨 VM 通信)
VirtualizationService（管理 pVM 生命周期）
    ↕
crosvm（用户态 VMM，通过 /dev/kvm 控制 VM）
    ↕
pKVM（EL2，内存隔离核心）
    ↕
pvmfw（Protected VM Firmware，验证 pVM 启动链）
    ↕
Microdroid（pVM 里的精简 Android，支持 NDK 子集）
```

- **crosvm**：Google 从 ChromeOS 移植过来的用户态 VMM，Rust 写的
- **pvmfw**：pVM 的 bootloader，用 DICE 密钥派生链验证启动完整性
- **Microdroid**：跑在 pVM 里的精简 Android，开发者可以把 APK 的敏感逻辑放进去

## pVM 里跑什么？

| 用途 | 说明 |
|------|------|
| **CompOS** | 首个生产用例。在 pVM 中编译 Android 系统 JAR，防止编译结果被篡改 |
| **Gemini Nano** | 端侧 AI 推理。个人数据在 pVM 内处理，即使主 OS 被攻击也无法泄露 |
| **生物识别** | 指纹、人脸数据处理 |
| **DRM (Widevine)** | 流媒体内容解密 |
| **Linux Terminal** | Android 16 新功能——在 pVM 里跑完整 Linux 桌面 |

## 哪些手机支持？

| 芯片 | 支持 pKVM？ | 备注 |
|------|-----------|------|
| Google Tensor G1-G4 | 是 | Pixel 6 起全系支持 |
| Samsung Exynos 2500 | 是（预期） | Galaxy Z Flip 7 |
| MediaTek Dimensity 9400+ | 是 | |
| Qualcomm Snapdragon 8 系列 | **仅 protected** | 使用 Gunyah，不支持非保护模式 |

高通的情况比较特殊。高通用自研的 **Gunyah Hypervisor**（Type-1，从零设计的独立 codebase）实现 AVF，目前只实现了 protected VM 模式，不支持非保护模式。这是安全性和功能性的不同权衡——非保护模式需要 Host 能访问 VM 内存（比如 GPU 加速），但这也降低了隔离保证。高通选择了更保守的安全路线。

代价是 Android 16 的 Linux Terminal 功能在骁龙 8 系列芯片上无法使用（Linux Terminal 需要非保护模式来提供图形加速）。

**Android 15 的 CDD（兼容性定义文档）要求新的 GMS 认证设备支持 AVF。** 具体用 pKVM 还是 Gunyah 还是其他 hypervisor 都行，只要满足 AVF 接口规范（通过 pKVM Vendor Module 机制）。

## SESIP Level 5：什么意思？

2025 年 8 月，pKVM 获得 SESIP（Security Evaluation Standard for IoT Platforms）**Level 5** 认证：

- Level 5 对应 Common Criteria 中的 **AVA_VAN.5**——最高级别的漏洞分析和渗透测试
- 意味着系统被评估为能抵御"高度熟练、具备内部知识、资金充足的攻击者"
- 评测机构：DEKRA（全球认可的网络安全认证实验室）
- **全球首个**消费电子软件达到此级别。之前只有智能卡和 HSM 达到过

这不是自我声明，是独立第三方的实际渗透测试结论。

## 总结

pKVM 代表的是 Android 安全架构的一次范式转移：

**旧模型**：信任内核 → 内核被攻破 → 全部暴露
**新模型**：不信任内核 → 内核被攻破 → pVM 数据依然安全

技术实现上，它利用 ARM 的 Stage-2 地址翻译把 Host 内核"关进笼子"，同时和 TrustZone/SPMC 在 EL3 之下和平共处。

## 延伸阅读

如果你对 pKVM 的"另一半"——S-EL2 的 SPMC 怎么工作——感兴趣，我在 QEMU 上做了一个实验性的 Rust 实现（非生产级），在模拟器上跑通了 pKVM 的 FF-A 接口：

- **代码**：https://github.com/willamhou/hypervisor
- **详细博客**：https://willamhou.github.io/hypervisor/

`make run` 可以在 QEMU 上运行测试套件，不需要真实硬件。

---

*参考资料：[Google Security Blog](https://security.googleblog.com/2025/08/Android-pKVM-Certified-SESIP-Level-5.html)、[AOSP AVF Documentation](https://source.android.com/docs/core/virtualization)、[ARM FF-A Specification](https://developer.arm.com/documentation/den0077/latest)、[LWN: KVM for Android](https://lwn.net/Articles/836693/)*

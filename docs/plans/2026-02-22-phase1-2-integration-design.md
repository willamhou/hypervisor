# Phase 1+2 Integration Design: NS-EL2 Proxy Forwarding + pKVM

Date: 2026-02-22
Status: Approved

## Overview / 概述

将 NS-EL2 hypervisor 的 FF-A proxy 与 S-EL2 SPMC 真正连通，实现端到端的 FF-A 调用路径。
分两个阶段：Phase 1 (BL33=我们的 NS hypervisor)、Phase 2 (BL33=pKVM)。

### 设计决策

1. **验收标准**: 完整验收 — Linux FF-A driver + MEM_SHARE 端到端
2. **SP 能力**: 最小 SP — 接收 MEM_SHARE + 读写共享页面 + DIRECT_REQ 返回结果
3. **Stub 策略**: 渐进式 — 保留 stub SPMC，SPMC_PRESENT 标志切换行为
4. **实施方案**: End-to-End Slice — 先跑通 DIRECT_REQ，再逐步加 MEM_SHARE

## Architecture / 架构

### Phase 1 Boot Chain (BL33 = 我们的 NS hypervisor)

```
TF-A:  BL1 → BL2 → BL31(SPMD) → BL32(our SPMC @ S-EL2) → BL33(our hypervisor @ NS-EL2) → Linux @ NS-EL1

FF-A 调用路径 (Linux DIRECT_REQ → SP):
  Linux(NS-EL1) → SMC → hypervisor(NS-EL2, proxy) → SMC → SPMD(EL3) → ERET → SPMC(S-EL2) → ERET → SP(S-EL1)
  SP(S-EL1) → SMC(DIRECT_RESP) → SPMC(S-EL2) → SMC → SPMD(EL3) → ERET → hypervisor(NS-EL2) → ERET → Linux(NS-EL1)
```

### Phase 2 Boot Chain (BL33 = pKVM)

```
TF-A:  BL1 → BL2 → BL31(SPMD) → BL32(our SPMC @ S-EL2) → BL33(pKVM @ NS-EL2) → Linux @ NS-EL1

FF-A 调用路径（从 SPMD 视角完全相同）:
  Linux(NS-EL1) → SMC → pKVM(NS-EL2, proxy) → SMC → SPMD(EL3) → ERET → SPMC(S-EL2) → ERET → SP(S-EL1)
```

### 关键洞察

SPMD 不关心 NS-EL2 是谁 — Phase 1 和 Phase 2 对 SPMC 来说协议完全一致。
因此：SPMC 的改动在两个阶段之间共享，Phase 2 主要是 BL33 替换。

### 各组件变更范围

| 组件 | Phase 1 变更 | Phase 2 变更 |
|------|-------------|-------------|
| SPMC (`spmc_handler.rs`) | 增加 RXTX、MEM_SHARE/RETRIEVE/RECLAIM | 无（已完成） |
| SP (`tfa/sp_hello/`) | 增加 MEM_SHARE_TEST 命令 | 无 |
| NS Proxy (`ffa/proxy.rs`) | probe_spmc 修复、转发 DIRECT_REQ/PARTITION_INFO/MEM_SHARE | N/A（被 pKVM 替代） |
| BL33 | 我们的 hypervisor + Linux | pKVM 内核 |
| Linux guest 内核 | `CONFIG_ARM_FFA_TRANSPORT=y` | 相同（在 pKVM 下运行） |

## NS Proxy Forwarding Policy / NS 代理转发策略

当 `SPMC_PRESENT=true` 时，以下调用的行为变化：

| FF-A 调用 | SPMC_PRESENT=false (stub) | SPMC_PRESENT=true (真实转发) |
|-----------|--------------------------|------------------------------|
| FFA_VERSION | 本地返回 0x10001 | 本地（proxy 自身版本） |
| FFA_ID_GET | 本地（VM partition ID） | 本地 |
| FFA_FEATURES | 本地 | 本地 |
| FFA_RXTX_MAP/UNMAP | 本地（proxy mailbox） | 本地 |
| FFA_RX_RELEASE | 本地 | 本地 |
| FFA_PARTITION_INFO_GET | 本地（stub SP 列表） | **转发** → 合并 VM 信息 |
| FFA_MSG_SEND_DIRECT_REQ | 本地（echo） | **转发**（目标是 SP 时） |
| FFA_MEM_SHARE/LEND | 本地验证 + stub 记录 | 本地验证 PTE → **转发** |
| FFA_MEM_RETRIEVE_REQ | 本地（stub） | **转发** |
| FFA_MEM_RELINQUISH | 本地（stub） | **转发** |
| FFA_MEM_RECLAIM | 本地（stub 恢复） | **转发** → 本地恢复 PTE |
| FFA_SPM_ID_GET | 本地（0x8000） | 本地 |
| Notifications | 本地（stub） | 暂时本地 |

### SPMC_PRESENT 检测机制

不用运行时探测（QEMU firmware 会崩溃），改用**编译时 feature flag**：

```rust
// Cargo.toml
[features]
tfa_boot = ["linux_guest"]  // 表示通过 TF-A 启动，SPMD+SPMC 必然存在

// proxy::init()
#[cfg(feature = "tfa_boot")]
SPMC_PRESENT.store(true, Ordering::Relaxed);
```

### 关键修复：8 寄存器转发

当前 `forward_ffa_to_spmc()` 使用 `forward_smc()`（只返回 x0-x3），
丢失了 DIRECT_REQ/RESP 的 x4-x7 payload。必须改用 `forward_smc8()`。

## SPMC Enhancement / SPMC 增强

### 现有 SPMC 能力

- 事件循环: `forward_smc8()` 与 SPMD 往返
- 分发: VERSION, ID_GET, SPM_ID_GET, FEATURES, PARTITION_INFO_GET (仅计数), DIRECT_REQ (路由到 SP)
- SPMD 框架消息: FFA_VERSION_REQ/RESP
- SP 管理: SpContext 状态机, Secure Stage-2, SPKG 启动

### 需要新增

**MEM_SHARE 处理（Sprint 5.3 新增）**:

```
NS proxy → SMC → SPMD → SPMC.dispatch_ffa():
  1. 解析 MEM_SHARE 参数（寄存器传递：sender, receiver, IPA, page_count）
  2. 记录共享（SpmcShareStore: handle → share info）
  3. 返回 SUCCESS + handle

SP 通过 DIRECT_REQ 触发 MEM_RETRIEVE_REQ:
  1. SPMC 查找 handle
  2. 将共享页映射到 SP 的 Secure Stage-2（identity mapping）
  3. 返回 SUCCESS

SP 调用 MEM_RELINQUISH:
  1. SPMC 从 SP Stage-2 移除映射
  2. 返回 SUCCESS

NS proxy 调用 MEM_RECLAIM → SPMD → SPMC:
  1. SPMC 验证 SP 已 relinquish
  2. 删除共享记录
  3. 返回 SUCCESS
```

**新 SPMC 组件**:

| 组件 | 位置 | 说明 |
|------|------|------|
| `SpmcShareStore` | `src/spmc_handler.rs` | 共享记录: handle→(sender, receiver, ranges, retrieved) |
| MEM_SHARE dispatch | `src/spmc_handler.rs` | dispatch_ffa() 新增 match arms |
| `map_ns_page()` | `src/secure_stage2.rs` | 将 NS PA 映射到 SP 的 Secure Stage-2 |
| `unmap_page()` | `src/secure_stage2.rs` | 移除映射 |

**NS 内存描述符传递方案**: NS proxy 从 TX buffer 提取描述符信息，通过寄存器传递给 SPMC
（x3=base_ipa, x4=page_count, x5=handle）。避免 SPMC 直接读取 NS 内存。

## SP Upgrade / SP 升级

### 保持汇编，扩展命令

SP 逻辑足够简单，暂不升级到 Rust。在现有 `start.S` 基础上增加命令分发：

```
x3 = command_id:
  0x01 = ECHO（现有：原样返回 x4-x7）
  0x02 = MEM_SHARE_TEST（接收共享页，读写验证，返回结果）

CMD_MEM_SHARE_TEST 流程:
  1. 收到 DIRECT_REQ(cmd=0x02, handle=x4, expected=x5)
  2. 调用 FFA_MEM_RETRIEVE_REQ(handle) → SPMC 映射共享页
  3. 读取共享页首 8 字节，验证 == expected
  4. 写入新值到共享页（证明双向访问）
  5. 返回 DIRECT_RESP(status=0, read_value, written_value)
```

## Sprint Plan / 冲刺计划

### Sprint 5.1 — DIRECT_REQ 端到端

**目标**: BL33 测试客户端发送 DIRECT_REQ，通过真实 SPMC 路由到 SP1，echo 响应返回。

| 文件 | 变更 | 行数 |
|------|------|------|
| `Cargo.toml` | 增加 `tfa_boot` feature | ~3 |
| `src/ffa/proxy.rs` | init(): SPMC_PRESENT=true when tfa_boot | ~5 |
| `src/ffa/proxy.rs` | handle_msg_send_direct_req(): 转发到 SPMC | ~10 |
| `src/ffa/proxy.rs` | forward_ffa_to_spmc() → 8 寄存器 | ~15 |
| `Makefile` | run-tfa-linux 使用 --features tfa_boot | ~2 |
| `tfa/bl33_ffa_test/` | 增加测试 7: 真实转发 DIRECT_REQ | ~20 |

**验证**: `make run-spmc` → 7/7 PASS
**区分 stub vs 真实 SP**: SP 修改 x4（如 x4 += 0x1000），stub echo 不改。

### Sprint 5.2 — PARTITION_INFO_GET + Linux FF-A 配置

**目标**: NS proxy 转发 PARTITION_INFO_GET。Linux 内核启用 FF-A driver 发现 SP1。

| 文件 | 变更 | 行数 |
|------|------|------|
| `src/ffa/proxy.rs` | handle_partition_info_get(): 转发到 SPMC | ~30 |
| `src/spmc_handler.rs` | PARTITION_INFO_GET: 寄存器返回 SP1 描述符 | ~15 |
| `guest/linux/` | FF-A 内核配置 (CONFIG_ARM_FFA_TRANSPORT=y) | ~5 |
| `Makefile` | build-linux-ffa 目标 | ~5 |

**验证**:
1. `make run-spmc` — BL33 测试仍通过（回归验证）
2. `make run-tfa-linux` — `ls /sys/bus/arm_ffa/devices/` 显示 SP1

### Sprint 5.3 — MEM_SHARE 端到端

**目标**: BL33 测试共享页面 → SP1 retrieve + 读写 → NS reclaim。

| 文件 | 变更 | 行数 |
|------|------|------|
| `src/spmc_handler.rs` | SpmcShareStore + MEM_SHARE/RETRIEVE/RELINQUISH/RECLAIM | ~120 |
| `src/secure_stage2.rs` | map_ns_page() / unmap_page() | ~35 |
| `src/ffa/proxy.rs` | handle_mem_share(): 验证 PTE → 提取描述符 → 转发 | ~25 |
| `src/ffa/proxy.rs` | handle_mem_reclaim(): 转发 → 本地恢复 PTE | ~15 |
| `tfa/sp_hello/start.S` | CMD_MEM_SHARE_TEST (MEM_RETRIEVE + 读写共享页) | ~50 |
| `tfa/bl33_ffa_test/` | 测试 8: MEM_SHARE E2E | ~40 |
| `tests/test_spmc_handler.rs` | 新增 ~12 assertions | ~40 |

**E2E 验证流程 (BL33 test 8)**:
```
BL33: 写 0xDEAD 到页面 P
BL33: FFA_MEM_SHARE(P, receiver=SP1) → handle H
BL33: FFA_DIRECT_REQ(SP1, cmd=MEM_SHARE_TEST, handle=H, expected=0xDEAD)
  → proxy 转发 → SPMD → SPMC → SP1
  → SP1: MEM_RETRIEVE(H) → SPMC 映射 P → SP Stage-2
  → SP1: 读取 P (期望 0xDEAD), 写入 P = 0xBEEF
  → SP1: DIRECT_RESP(ok, read=0xDEAD, written=0xBEEF)
BL33: 验证 resp.x4==0xDEAD, resp.x5==0xBEEF
BL33: 读取 P, 验证 P==0xBEEF (SP 写入的)
BL33: FFA_MEM_RECLAIM(H) → 成功
```

### Sprint 5.4 — Linux FF-A 集成测试

**目标**: Linux 通过 FF-A driver 发现 SP1，发送 DIRECT_REQ，执行 MEM_SHARE。

| 文件 | 变更 | 行数 |
|------|------|------|
| `guest/linux/` | FF-A 内核 + 测试模块/脚本 | ~20 |
| `Makefile` | run-tfa-linux-ffa 目标 | ~5 |
| initramfs | FF-A 测试脚本 | ~15 |

**验证**: Linux 启动 → FF-A driver 探测成功 → dmesg 显示 SP1 注册

### Sprint 5.5 — pKVM 作为 BL33 (Phase 2)

**目标**: 用 pKVM 替换 NS hypervisor。Linux 在 pKVM 下启动，FF-A 调用到达 SPMC。

| 文件 | 变更 | 行数 |
|------|------|------|
| `guest/pkvm/` | pKVM 内核构建 (CONFIG_KVM + CONFIG_ARM_FFA) | 新目录 |
| `Makefile` | build-pkvm, run-pkvm 目标 | ~20 |
| TF-A 配置 | BL33 = pKVM 内核 | 配置变更 |

**关键**: SPMC 无需任何改动。pKVM 有自己的 FF-A proxy。

## Test Plan / 测试计划

| Sprint | 单元测试 | 集成测试 |
|--------|---------|---------|
| 5.1 | 现有 25 + forward_smc8 测试 | BL33 7/7 (新: 转发 DIRECT_REQ) |
| 5.2 | +4 (PARTITION_INFO 寄存器) | Linux FF-A 设备发现 |
| 5.3 | +12 (SpmcShareStore + MEM_SHARE) | BL33 8/8 (新: MEM_SHARE E2E) |
| 5.4 | — | Linux FF-A DIRECT_REQ + MEM_SHARE |
| 5.5 | — | pKVM FF-A → SP |

## Risk Assessment / 风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| Sprint 5.1: proxy SMC 转发格式不对 | 低 | BL33 测试客户端快速验证 |
| Sprint 5.2: Linux FF-A driver 要求 RXTX buffer | 中 | 先试寄存器模式，不行再加 RXTX |
| Sprint 5.3: Secure Stage-2 映射 NS 内存失败 | 高 | QEMU secure=on 可能限制 NS 访问，需仔细测试 |
| Sprint 5.3: MEM_SHARE 描述符寄存器传递不够 | 中 | 限制单次 share 1 个 range |
| Sprint 5.5: pKVM 构建复杂度 | 高 | 先验证 QEMU TCG 支持 |

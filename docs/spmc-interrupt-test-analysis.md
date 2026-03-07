# SPMC 中断测试方案分析

## 1. 测试架构分层

```
┌──────────────────────────────────────────────────────────┐
│  Layer 1: 单元测试 (make run, 无硬件依赖, 秒级反馈)       │
│  ├─ test_sp_context   (42 assertions) — 状态机 + IRQ 队列 │
│  ├─ test_spmc_handler (54 assertions) — FF-A 分发逻辑      │
│  └─ test_ffa          (44 assertions) — NS-EL2 FF-A proxy  │
├──────────────────────────────────────────────────────────┤
│  Layer 2: BL33 E2E (make run-spmc, 真实硬件路径, ~30min)   │
│  ├─ Test 9:  NS 中断抢占 + FFA_RUN 恢复                   │
│  ├─ Test 10: 多 SP DIRECT_REQ (SP1 + SP2)                 │
│  ├─ Test 11: Secure vIRQ 注入 (CNTHP → VI → HVC)          │
│  ├─ Test 12: MEM_SHARE + MEM_RECLAIM                      │
│  └─ Test 13: MEM_SHARE lifecycle E2E (SP-initiated)        │
├──────────────────────────────────────────────────────────┤
│  Layer 3: 全链路 (make run-tfa-linux / run-pkvm)           │
│  └─ Linux FF-A 驱动发现 + DIRECT_REQ 回显                  │
└──────────────────────────────────────────────────────────┘
```

## 2. SP 状态机

### 2.1 状态定义

```
Reset(0) ── SP 已加载, 未启动
Idle(1)  ── SP 空闲, 等待消息 (FFA_MSG_WAIT 后)
Running(2) ── SP 正在执行 (SPMC ERET 到 SP)
Blocked(3) ── SP 阻塞等待事件 (预留, 当前未使用)
Preempted(4) ── SP 被 NS 中断抢占, 需 FFA_RUN 恢复
```

### 2.2 合法转换 (7 条)

```
Reset ──→ Idle         SP boot 完成, 调用 FFA_MSG_WAIT
Idle  ──→ Running      dispatch_to_sp() 接收 DIRECT_REQ
Running ──→ Idle       handle_sp_exit() SP 返回 DIRECT_RESP
Running ──→ Blocked    SP 主动阻塞 (预留)
Blocked ──→ Running    事件到达, 唤醒 SP (预留)
Running ──→ Preempted  FIQ/NS IRQ 抢占, SP_IRQ_PREEMPTED=true
Preempted ──→ Running  resume_preempted_sp() via FFA_RUN
```

### 2.3 状态转换测试覆盖

| 转换 | 单元测试 | BL33 E2E | 触发方式 |
|------|---------|---------|---------|
| Reset → Idle | T4-5 | SP boot | SPMC 启动 SP 后接收 MSG_WAIT |
| Idle → Running | T6-7 | Test 6-7 | dispatch_to_sp() |
| Running → Idle | T8-9 | Test 6-7 | handle_sp_exit() 正常返回 |
| Running → Blocked | **未测试** | **未测试** | SP 不使用阻塞态 |
| Blocked → Running | **未测试** | **未测试** | SP 不使用阻塞态 |
| Running → Preempted | T17-18 | Test 9 | FIQ/NS IRQ 抢占 |
| Preempted → Running | T19-20 | Test 9 | FFA_RUN 恢复 |

### 2.4 非法转换测试覆盖

| 非法转换 | 单元测试 | 备注 |
|---------|---------|------|
| Reset → Running | T10 | 跳过 Idle |
| Preempted → Idle | T21 | 必须经过 Running |
| Idle → Preempted | **未测试** | 只有 Running 可以被抢占 |
| Idle → Blocked | **未测试** | |
| Reset → Preempted | **未测试** | |
| Reset → Blocked | **未测试** | |
| Blocked → Idle | **未测试** | |
| Blocked → Preempted | **未测试** | |
| Preempted → Blocked | **未测试** | |

## 3. 中断路径

### 3.1 四条中断路径及覆盖

#### 路径 A: 属于当前 SP 的 INTID

```
物理 IRQ (e.g. INTID 29) 在 SP2 执行期间触发
    ↓
exception.rs IRQ handler
    ├─ find_sp_for_intid(29) → 0x8002
    ├─ owner_id == current_sp_id → 是当前 SP
    ├─ set_pending_irq_for(0x8002, 29)
    ├─ set_hcr_vi(true)           ← HCR_EL2.VI=1
    └─ return true                ← 继续执行 SP
         ↓
    ERET 时硬件检测 VI=1
         ↓
    自动跳转 VBAR_EL1 + 0x280 (Current EL SPx IRQ)
         ↓
    SP2 IRQ handler: HVC(0xFF04, HF_INTERRUPT_GET)
         ↓
    exception.rs HVC handler
    ├─ take_pending_irq_for(0x8002) → 29
    ├─ set x0 = 29
    ├─ set_hcr_vi(false)          ← 清除 VI
    └─ return true
         ↓
    SP2 获得 INTID=29, 继续处理
```

**测试**: BL33 Test 11 验证 x5==29 (SP2 返回捕获的 INTID)

#### 路径 B: 属于其他 SP 的 INTID (跨 SP 抢占)

```
INTID 29 在 SP1 (0x8001) 执行期间触发
    ↓
exception.rs IRQ handler
    ├─ current_sp_id = 0x8001
    ├─ find_sp_for_intid(29) → 0x8002 (不同 SP)
    ├─ set_pending_irq_for(0x8002, 29)
    ├─ set_sp_irq_preempted(true)     ← 抢占 SP1
    └─ return false                    ← 退出到 SPMC
         ↓
    handle_sp_exit() 检测 SP_IRQ_PREEMPTED
         ├─ SP1: Running → Preempted
         ├─ find_sp_with_pending_irq() → 0x8002
         ├─ dispatch_interrupt_to_sp(0x8002)
         │   ├─ SP2: Idle → Running
         │   ├─ inject_pending_virq(SP2)  ← VI=1
         │   ├─ enter_guest(SP2)
         │   └─ SP2 处理中断, 返回 Idle
         └─ 返回 FFA_INTERRUPT 给 NWd
              ↓
         NWd 调用 FFA_RUN(SP1) → SP1: Preempted → Running
```

**测试**: BL33 Test 11 隐式覆盖 (CNTHP 可能在 SP1 或 SP2 执行时触发)

#### 路径 C: CNTHP 轮询定时器 (INTID 26)

```
CNTHP 定时器到期 (10ms, INTID 26)
    ↓
exception.rs IRQ handler
    ├─ intid == 26
    ├─ 应答 + 关闭 CNTHP
    ├─ first_owned_intid_for(current_sp_id)
    │   ├─ SP1 (无 owned): → None → 不注入, 不抢占
    │   └─ SP2 (owns 29):  → Some(29)
    │       ├─ set_pending_irq_for(0x8002, 29)
    │       └─ set_hcr_vi(true)
    ├─ 重新 arm CNTHP
    └─ return true   ← 继续执行当前 SP
```

**测试**: BL33 Test 11 间接验证 (SP2 的 INTID 29 通过 CNTHP 轮询注入)

**关键修复**: CNTHP 不抢占无 owned INTID 的 SP, 防止 SP1 被误抢占

#### 路径 D: NS Group 1 FIQ 抢占

```
NS Group 1 中断触发 (物理 FIQ at S-EL2)
    ↓
exception.rs FIQ handler (sel2 feature)
    ├─ 不读 ICC_IAR1_EL1 (只返回 Secure Group 1)
    ├─ NS 中断保持 pending, NWd 恢复后处理
    ├─ set_sp_irq_preempted(true)
    └─ return false   ← 退出到 SPMC 事件循环
         ↓
    handle_sp_exit(): SP Running → Preempted
         ↓
    返回 FFA_INTERRUPT → SPMD → NWd
         ↓
    NWd 处理 NS 中断, 然后 FFA_RUN 恢复 SP
```

**测试**: BL33 Test 9 (SP1 slow-path 被 NS FIQ 抢占, FFA_RUN 循环恢复)

### 3.2 中断相关组件覆盖矩阵

```
                        单元测试        E2E (BL33)
                        ─────────       ──────────
SpContext 状态机          ████████████    ████████
pending_irq 队列          ████████████    ████
INTID ownership           ████████████    ████████
CPU ownership             ████████████    ████ (pKVM)

FIQ → PREEMPTED           ○ (不可测)     ████████
HCR_EL2.VI 注入           ○ (不可测)     ████████
HF_INTERRUPT_GET (HVC)    ○ (不可测)     ████████
CNTHP 轮询                ○ (不可测)     ████████
cross-SP 抢占              ○ (不可测)     ████ (隐式)
嵌套抢占                   ○              ○
多 CPU 竞争                ○              ○
Blocked 状态               ○              ○
IRQ 队列满溢出             ████            ○

████ = 已覆盖   ○ = 未覆盖
```

## 4. 测试缺口

### 4.1 单元测试可补充项 (不依赖硬件)

#### G1: 状态机非法转换完整覆盖

当前只测试了 2 条非法转换 (Reset→Running, Preempted→Idle), 剩余 7 条未测试。

```rust
// 补充: 从 Idle 的非法转换
assert!(ctx.transition_to(SpState::Preempted).is_err());
assert!(ctx.transition_to(SpState::Blocked).is_err());

// 补充: 从 Reset 的非法转换
assert!(ctx.transition_to(SpState::Preempted).is_err());
assert!(ctx.transition_to(SpState::Blocked).is_err());

// 补充: 从 Blocked 的非法转换
assert!(ctx.transition_to(SpState::Idle).is_err());
assert!(ctx.transition_to(SpState::Preempted).is_err());

// 补充: 从 Preempted 的非法转换
assert!(ctx.transition_to(SpState::Blocked).is_err());
```

**工作量**: ~10 分钟. **价值**: 防止未来修改状态机时引入非法路径.

#### G2: try_transition (CAS) 失败语义

验证原子 CAS 在状态不匹配时返回正确的当前状态:

```rust
// SP in Idle, CAS expect Running → fail with Err(Idle)
let result = ctx.try_transition(SpState::Running, SpState::Preempted);
assert_eq!(result, Err(SpState::Idle));

// SP in Running, CAS expect Idle → fail with Err(Running)
let result = ctx.try_transition(SpState::Idle, SpState::Preempted);
assert_eq!(result, Err(SpState::Running));
```

**工作量**: ~5 分钟. **价值**: 验证多 CPU dispatch 的拒绝逻辑正确.

#### G3: pending_irq 队列满溢出

```rust
ctx.set_pending_irq(1);
ctx.set_pending_irq(2);
ctx.set_pending_irq(3);
ctx.set_pending_irq(4);
ctx.set_pending_irq(5);  // 第 5 个应被丢弃 (log warning)
// 验证前 4 个正常取出
assert_eq!(ctx.take_pending_irq(), Some(1));
assert_eq!(ctx.take_pending_irq(), Some(2));
assert_eq!(ctx.take_pending_irq(), Some(3));
assert_eq!(ctx.take_pending_irq(), Some(4));
assert_eq!(ctx.take_pending_irq(), None);  // 第 5 个已丢失
```

**工作量**: ~5 分钟. **价值**: 验证溢出行为一致, 防止静默数据丢失.

#### G4: FFA_RUN 状态校验 (Preempted 路径)

当前 test_spmc_handler 只测试了 FFA_RUN 的两种情况:
- SP 不存在 → INVALID_PARAMETERS (Test 35)
- SP in Idle → DENIED (Test 36)

缺少第三种: SP in Preempted → 通过状态校验。`dispatch_ffa()` 中
FFA_RUN 分支 (line 1072-1088) 在非 sel2 模式下会校验 SP 状态,
Preempted 时返回 NOT_SUPPORTED (因为无法真正 enter_guest),
可以验证状态校验逻辑:

```rust
// 手动将 SP 转换到 Preempted:
//   with_sp_locked(sp_id, |sp| {
//       sp.transition_to(Idle); sp.transition_to(Running);
//       sp.transition_to(Preempted);
//   });
// FFA_RUN → 通过 Preempted 检查, 返回 NOT_SUPPORTED (非 sel2)
let mut req = zero_req(ffa::FFA_RUN);
req.x1 = 0x8001 << 16;
let resp = dispatch_ffa(&req);
assert_eq!(resp.x0, ffa::FFA_ERROR);
assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
```

**工作量**: ~15 分钟. **价值**: 验证 FFA_RUN 接受 Preempted 状态.

#### G5: global 函数测试

`find_sp_for_intid()`, `find_sp_with_pending_irq()`, `set_pending_irq_for()`,
`take_pending_irq_for()` 这些 global helper 目前没有直接单元测试:

```rust
// 注册 SP with owned_intids = [29, 0, 0, 0]
register_sp(sp);
assert_eq!(find_sp_for_intid(29), Some(0x8002));
assert_eq!(find_sp_for_intid(27), None);

set_pending_irq_for(0x8002, 29);
assert_eq!(find_sp_with_pending_irq(), Some(0x8002));
assert_eq!(take_pending_irq_for(0x8002), Some(29));
assert_eq!(find_sp_with_pending_irq(), None);
```

**工作量**: ~15 分钟. **价值**: 验证 SpStore 遍历逻辑 + SpinLock 交互.

### 4.2 BL33 E2E 可增强项

#### G6: 交替 SP 调用稳定性

快速交替调用 SP1 和 SP2, 验证状态机在快速切换下不卡死:

```
REQ→SP1→RESP → REQ→SP2→RESP → REQ→SP1→RESP  (交替 N 次)
```

验证: 每次 x4 += 0x1000 正确, 无 hang.

#### G7: 抢占后跨 SP 调用

SP2 被 NS 中断抢占后, 不 FFA_RUN 恢复 SP2, 而是先调用 SP1:

```
REQ→SP2(slow)→INTERRUPT
  → REQ→SP1(fast)→RESP    ← SP2 仍 Preempted, SP1 正常
  → RUN→SP2→RESP           ← 恢复 SP2
```

验证: Preempted SP 不影响其他 SP 的正常分发.

#### G8: 连续 slow-path 序列化

两个 SP 依次进入 slow-path:

```
REQ→SP1(slow)→INTERRUPT → RUN→SP1→RESP
REQ→SP2(slow)→INTERRUPT → RUN→SP2→RESP
```

验证: CNTHP arm/disarm 在多轮中正确复用.

### 4.3 不建议测试的项

| 项 | 原因 |
|----|------|
| Blocked 状态 | 当前 SP 实现不使用阻塞态, 测试无实际价值 |
| 真正的多 CPU 并发竞争 | TCG 单线程无法模拟, 需 FPGA/硬件 |
| SP PSTATE.I 屏蔽下的 VI 行为 | 纯硬件语义, 代码无法控制 |

## 5. 优先级排序

| 优先级 | 项 | 类型 | 工作量 | ROI |
|--------|---|------|--------|-----|
| P1 | G1: 非法转换矩阵 | 单元 | 10min | 高 — 防御性, 防回归 |
| P1 | G2: CAS 失败语义 | 单元 | 5min | 高 — 多 CPU 安全 |
| P1 | G3: IRQ 队列溢出 | 单元 | 5min | 高 — 边界条件 |
| P2 | G5: global 函数测试 | 单元 | 15min | 中 — 覆盖 SpStore 遍历 |
| P2 | G6: 交替 SP 调用 | E2E | 30min | 中 — 稳定性验证 |
| P3 | G4: 模拟中断分发 | 单元 | 1h | 高 — 但需重构 |
| P3 | G7: 抢占后跨 SP | E2E | 1h | 中 — 真实场景 |
| P3 | G8: 连续 slow-path | E2E | 30min | 低 — 已间接覆盖 |

## 6. Review 发现

### 6.1 G4 工作量下调

原始评估 G4 (模拟中断分发) 需要"1 小时, 需重构"。

Review 代码后发现 `dispatch_ffa()` 的 FFA_RUN 分支 (`spmc_handler.rs:1072-1088`)
在非 sel2 模式下已经有完整的状态校验逻辑:

```rust
// spmc_handler.rs:1072-1088
ffa::FFA_RUN => {
    let sp_id = ((req.x1 >> 16) & 0xFFFF) as u16;
    if !is_registered_sp(sp_id) {
        return make_error(FFA_INVALID_PARAMETERS);
    }
    let state = state_of(sp_id);
    if state != Preempted {
        return make_error(FFA_DENIED);       // ← Test 36 覆盖 (Idle)
    }
    // sel2 模式: dispatch_request() 在此之前已处理
    // 非 sel2: 返回 NOT_SUPPORTED (无 enter_guest)
    make_error(FFA_NOT_SUPPORTED)             // ← 未测试
}
```

只需用 `with_sp_locked()` 手动将 SP 转到 Preempted 状态，即可在
单元测试中验证第三条路径。不需要任何重构。

**结论**: G4 从 P3 (1h) 提升到 **P2 (15min)**。

### 6.2 G5 部分已覆盖

Review 发现 `find_sp_for_intid()` 已被 test_spmc_handler T39-40 测试:

```rust
// test_spmc_handler.rs:223-224 (已有)
assert_eq!(find_sp_for_intid(29), Some(0x8002));
assert_eq!(find_sp_for_intid(99), None);
```

但以下 global 函数仍无直接测试:
- `find_sp_with_pending_irq()` — 遍历 SpStore 找有 pending IRQ 的 SP
- `set_pending_irq_for()` — 按 SP ID 设置 pending IRQ
- `take_pending_irq_for()` — 按 SP ID 消费 pending IRQ
- `first_owned_intid_for()` — 按 SP ID 查第一个 owned INTID

这些函数在中断路径的异常处理代码中被调用 (exception.rs),
是 IRQ 路由决策的核心。建议补充。

### 6.3 test_spmc_handler 测试顺序依赖

Review 发现 test_spmc_handler 中注册的 SP 会影响后续测试:
- T13-14 注册 SP1 (0x8001)
- T37-38 注册 SP2 (0x8002, owned_intids=[29])
- 注册后 SP 处于 Idle 状态 (SpContext::new 创建的是 Reset, 但
  dispatch_ffa 的 DIRECT_REQ echo 路径不走 sel2 的 enter_guest,
  直接返回 — 不改变 SP 状态)

这意味着 G4 的测试需要在 T37 之后执行（SP 已注册），
且需要先手动将 SP 状态从 Reset 转到 Idle → Running → Preempted。
当前的 SP 是通过 `SpContext::new()` 创建的，初始状态是 Reset,
而 `dispatch_ffa` 的 echo 路径不执行真实的 ERET,
所以 SP 的状态机停留在 Reset。

**方案**: 在 test_spmc_handler 末尾用 `with_sp_locked` 手动
推进状态机: `Reset→Idle→Running→Preempted`, 然后调用
`dispatch_ffa(FFA_RUN)` 验证。

### 6.4 路径 B (跨 SP 抢占) 验证不充分

BL33 Test 11 实际上只测试了 "DIRECT_REQ → SP2(slow) → CNTHP →
inject VI → SP2 自己处理" 这条路径 (路径 A + C)。

路径 B 的 "INTID 属于其他 SP → 抢占当前 SP → dispatch_interrupt_to_sp"
没有被**显式**测试——它只在 CNTHP 恰好在 SP1 执行期间触发时才隐式触发,
但 BL33 测试流程中 SP1 只走 fast-path (无 slow loop), 不会触发 CNTHP。

**建议**: 增加 BL33 Test 14 显式测试路径 B:

```
1. DIRECT_REQ → SP1(slow) ← SP1 执行期间 CNTHP 触发
2. CNTHP handler 发现 SP1 无 owned INTID → 不注入
3. NS FIQ 抢占 SP1 → FFA_INTERRUPT
4. FFA_RUN → SP1 继续 → DIRECT_RESP
```

或者更复杂的跨 SP 场景:

```
1. 配置 SP2 owned INTID 29
2. DIRECT_REQ → SP1(slow)
3. CNTHP 触发 → 检测 SP1 无 owned → 不注入 (已修复)
4. 如果物理 INTID 29 在 SP1 期间触发 → 路径 B
   (但这难以在 TCG 下确定性触发)
```

**结论**: 路径 B 在 TCG 下难以确定性触发，可暂时接受隐式覆盖。

### 6.5 修正后的优先级排序

| 优先级 | 项 | 类型 | 工作量 | ROI |
|--------|---|------|--------|-----|
| P1 | G1: 非法转换矩阵 (7 条) | 单元 | 10min | 高 — 防御性 |
| P1 | G2: CAS 失败语义 | 单元 | 5min | 高 — 多 CPU 安全 |
| P1 | G3: IRQ 队列溢出 | 单元 | 5min | 高 — 边界条件 |
| P2 | G4: FFA_RUN Preempted 路径 | 单元 | 15min | 高 — 无需重构 |
| P2 | G5: global 函数 (4 个) | 单元 | 15min | 中 — IRQ 路由核心 |
| P2 | G6: 交替 SP 调用 | E2E | 30min | 中 — 稳定性 |
| P3 | G7: 抢占后跨 SP | E2E | 1h | 中 — 路径 B 覆盖 |
| P3 | G8: 连续 slow-path | E2E | 30min | 低 — 已间接覆盖 |

P1 合计 ~20 分钟, P2 合计 ~1 小时, 全部补充后新增 ~20 assertions。

## 7. 关键文件索引

| 文件 | 角色 |
|------|------|
| `src/sp_context.rs` | SP 状态机, pending_irq 队列, INTID ownership, CPU ownership |
| `src/spmc_handler.rs` | SPMC 事件循环, dispatch_to_sp, inject_pending_virq, handle_sp_exit, resume_preempted_sp, dispatch_interrupt_to_sp |
| `src/arch/aarch64/hypervisor/exception.rs` | 异常处理: FIQ→PREEMPTED, IRQ→INTID 路由, HVC(HF_INTERRUPT_GET) |
| `tfa/sp_hello/start.S` | SP1: 同步 SP, fast/slow/memory 路径, 无中断处理 |
| `tfa/sp_irq/start.S` | SP2: VBAR_EL1 IRQ handler, HVC(0xFF04), busy-loop + irq_handled |
| `tfa/bl33_ffa_test/start.S` | BL33 测试: Test 9 (抢占), Test 10 (多 SP), Test 11 (vIRQ) |
| `tests/test_sp_context.rs` | 单元: 状态机 + IRQ 队列 + CPU ownership (42 assertions) |
| `tests/test_spmc_handler.rs` | 单元: FF-A 分发 + 内存共享 (54 assertions) |

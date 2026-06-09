# 我能拦你的 SMC,你拦不了我的——HCR_EL2.TSC 的非对称语义

Guest 在 EL1 执行了一条 `smc #0`,trap 进我这边的 EL2 异常 handler。ESR_EL2.EC = 0x17(SMC64 from lower EL),我看到这是一条 FF-A 调用,在 hypervisor 这边解析、决定要不要转发给 SPMD。

要转发,hypervisor 自己也得发一条 `smc #0`——到 EL3。

我执行那一条 SMC 时,**不会** trap 回 EL2 自己。它直接进 EL3,SPMD 处理完返回到我。

同一条机器指令(`smc #0`)、同一颗 CPU、相同的 `HCR_EL2.TSC` 设置——guest 发它 trap,我发它通过。这种非对称语义不是 bug,是 EL2 这一层之所以能当 hypervisor 的本质原因。

这篇讲为什么 `HCR_EL2.TSC` 是这样设计、以及一系列类似 trap 位(TWI / TWE / TGE / ...)共享的"只拦下方"规律。

---

## TSC 的字面意思

`HCR_EL2.TSC` 是 Hypervisor Configuration Register 第 19 位,**T**rap **S**MC **C**alls。文档原话(ARM ARM D13.2.46):

> Traps EL1 execution of SMC instructions to EL2 when EL2 is enabled in the current Security state.

注意那句关键词:**EL1 execution**。这一位**只**拦 EL1(以及 EL0,如果 EL0 有权限发 SMC 的话——通常没有)。EL2 自己执行的 SMC 不受影响,直接走原本的路径——也就是 trap 到 EL3。

整张 HCR_EL2 的设计都遵循这条规律:**HCR_EL2 是 EL2 用来管 EL1/EL0 的开关。它不会反过来约束 EL2 自己。**

---

## 为什么非对称

ARM 的特权级模型是单向信任的。EL0 信任 EL1,EL1 信任 EL2,EL2 信任 EL3。下一级要做受限的事得通过上一级——`HCR_EL2.TSC` 这种 trap 位是 EL2 决定"哪些下层操作我要审查、哪些放行"的工具。

如果 EL2 自己也被 TSC 拦,会出现死锁:hypervisor 想转发 FF-A 调用给 SPMD,但它发的 SMC 又 trap 回它自己。**没有人能调 EL3**。整个安全世界就跟 normal 世界之间完全断绝通信。

非对称的语义让 EL2 保留"我有路径出去"的能力——它可以决定要不要拦 EL1,但它自己永远能跟 EL3 沟通。这是 trust hierarchy 的工程后果。

类似的非对称规则在 HCR_EL2 里到处都是:

| Bit | 作用 | 谁被拦? |
|---|---|---|
| TSC(19) | SMC | EL1/EL0 |
| TWI(13) | WFI | EL1/EL0 |
| TWE(14) | WFE | EL1/EL0 |
| TGE(27) | 把异常路由到 EL2 而不是 EL1 | 影响 EL0 路由,但 EL2 自己不受 |
| TIDCP(20) | IMPLEMENTATION DEFINED 系统寄存器访问 | EL1 |

每一位都是"EL2 给自己留出口、把下方拦住"的同一套思路的变体。

---

## 在 FF-A proxy 里这件事的具体表现

我的 hypervisor 当 NS-EL2 时做 FF-A proxy——guest 的 FF-A SMC trap 进来,我解析、可能修改参数,然后转发给 SPMD。代码大致这样:

```rust
// src/arch/aarch64/hypervisor/exception.rs (简化)
fn handle_smc_exception(ctx: &mut VcpuContext) -> bool {
    let fid = ctx.gp_regs.x0 as u32;
    if is_ffa_function(fid) {
        // 解析 FF-A 调用
        let result = ffa::proxy::handle_ffa_call(ctx);
        ctx.gp_regs.x0 = result;
        advance_pc(ctx);
        return true;
    }
    // 不认识的 SMC — 透明转发到 EL3
    let res = ffa::smc_forward::forward_smc8(
        ctx.gp_regs.x0, ctx.gp_regs.x1, ...,
    );
    ctx.gp_regs.x0 = res.x0; ...;
    advance_pc(ctx);
    true
}
```

`forward_smc8` 这一段做的就是 EL2 自己发 SMC:

```rust
// src/ffa/smc_forward.rs
pub fn forward_smc8(...) -> SmcResult8 {
    let res = SmcResult8::default();
    unsafe {
        asm!(
            "smc #0",
            inout("x0") fid => res.x0,
            // ... 完整的 SMCCC 寄存器约定
        );
    }
    res
}
```

这一条 `smc #0` 在 EL2 执行,**不会**回到 `handle_smc_exception`。它直接进 EL3,SPMD 处理,返回。如果 TSC 拦 EL2 自己,这里就死循环了——我们永远转发不出去。

---

## ELR_EL2 的细节:SMC trap 不会自动前进 PC

HCR_TSC 还有一个跟其他 trap 不同的小细节——**`ELR_EL2` 在 SMC trap 进来时指向 SMC 指令本身**,不是下一条指令。Hypervisor 处理完得手动把 PC 往前推 4 个字节。

```rust
fn advance_pc(ctx: &mut VcpuContext) {
    ctx.elr_el2 += 4;  // SMC 是 32 位指令
}
```

为什么是这种语义?因为 HVC trap 和 SVC trap 都会把 ELR 指向"下一条"——异常被认为是"在执行 HVC/SVC 期间发生",retiring 时回到后面。SMC 在 ARM ARM 里被分类为 **synchronous exception**,跟一般同步异常一样:`ELR_EL2 = 故障指令地址`。

漏掉 `advance_pc` 的话,ERET 回 guest 之后会再执行同一条 SMC,**死循环 trap**。我第一次写 SMC handler 漏了这一句,然后用 GDB 看到 `ELR_EL2` 一直停在同一个 PC 值,才意识到问题。

(HVC 也是 synchronous exception,但 HVC 的 ELR 是指向**下一条**的——这个差别在 ARM ARM D1.10 里有专门一段讲。)

---

## 把这条规律记下来

任何 trap 位语义不清的时候,**先问"它拦不拦 EL2 自己"**。绝大多数情况下不拦——HCR_EL2 / CPTR_EL2 / MDCR_EL2 这些都是 EL2 管下层用的。如果你想限制 EL2 自己,得到 EL3 的 SCR_EL3 那一层(比如 `SCR_EL3.SMD=1` 能禁掉所有 SMC 包括 EL2 发的)。

把这条规律写进代码注释里:

```rust
// HCR_TSC = 1 << 19. Traps guest SMC to EL2 as EC_SMC64 (0x17).
// EL2's own `smc #0` is unaffected — that's what lets us forward.
// ELR_EL2 points to the SMC instruction itself, advance by 4 after handling.
const HCR_TSC: u64 = 1 << 19;
```

下一次维护这段代码的人(可能是几个月后的你自己),不用再去翻 ARM ARM 才能知道这一位的语义边界。

---

下一篇我想讲 `ICC_SGI1R_EL1` 那笔糊涂账——TargetList 在 bits[15:0] 不是[23:16],INTID 在 bits[27:24] 不是 [3:0],位域不在你以为的位置,写反了 SGI 发不出去。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十八篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*

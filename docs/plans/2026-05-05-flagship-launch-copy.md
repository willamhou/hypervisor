# 2026-05-05 Flagship Launch Copy

## Hacker News

### Title

`Building a Rust ARM64 SPMC that Replaces Hafnium and Runs Beside pKVM`

### URL

Use the canonical article URL, not the repo URL.

### First Comment

```text
Author here. The goal was to understand ARM's Secure world by rebuilding the S-EL2 control plane instead of modifying Hafnium.

The unusual part is that this runs beside pKVM on the same physical CPUs. pKVM owns Normal world at NS-EL2, this project owns Secure world at S-EL2, and TF-A at EL3 relays FF-A traffic between them. The full Linux -> pKVM -> TF-A -> SPMC -> Secure Partition -> back path currently passes 35/35 end-to-end tests.

A few things that surprised me:

1. SPMD is per-CPU. Secondary CPU bring-up was blocked until every Secure-side CPU did its own FFA_MSG_WAIT handshake. I only fully understood that after reading TF-A source.

2. Secure vs Non-Secure is not just permissions. It is a physical address-space split. I had a bug where the Secure side wrote to the "right" numeric address and pKVM still read zeros because each side was hitting a different physical alias.

3. Rust's debug codegen can become a hardware issue below the OS. A NEON instruction emitted around read_volatile triggered an EL3 FP/SIMD trap and silently hung the system until I changed the TF-A config.

If you want the quick path, `make run` executes the bare-metal test suites on QEMU in a few seconds. Happy to answer questions about FF-A, S-EL2, pKVM coexistence, or no_std Rust at EL2.
```

## Rust Forum

### Title

`Project update: a no_std Rust ARM64 SPMC at Secure EL2`

### Body

```text
I built a bare-metal ARM64 Secure Partition Manager Core (SPMC) at S-EL2 in no_std Rust.

It replaces Hafnium on the Secure side, runs beside Android pKVM on the same chip, and currently passes 35/35 end-to-end tests through Linux -> pKVM -> TF-A -> SPMC -> Secure Partition -> back.

Current scope:

- boots Linux 6.12 to a BusyBox shell
- manages 3 Secure Partitions at S-EL1
- implements FF-A v1.1 direct messaging, indirect messaging, memory sharing, and notifications
- runs on 4 physical CPUs beside pKVM

The strongest Rust-specific win was state management. The SP lifecycle is an enum-based state machine (Reset, Idle, Running, Blocked, Preempted). When I added preemption in nested SP-to-SP call chains, the compiler forced me to revisit all transition sites and caught two missing cases before runtime.

I wrote up the architecture, the hardest bugs, and why the Secure/Normal split is trickier than it first looks here:
<canonical article URL>

Repo:
https://github.com/willamhou/hypervisor
```

## X Thread

### Tweet 1

```text
I built an ARM64 hypervisor that runs beside Android pKVM on the same chip.

pKVM owns Normal world at NS-EL2.
Mine owns Secure world at S-EL2.
TF-A at EL3 relays FF-A between them.

30K lines of no_std Rust.
35/35 end-to-end tests.

Thread:
```

### Tweet 2

```text
The strangest bug: Secure and Non-Secure are separate physical address spaces, not just permission labels.

I had a case where the Secure side wrote to the “right” address and pKVM still read zeros because each side was hitting a different physical alias.

Same address. Different memory.
```

### Tweet 3

```text
The most Rust-specific win was the Secure Partition state machine.

Reset -> Idle -> Running -> Blocked -> Preempted

When nested SP-to-SP preemption changed the graph, match exhaustiveness forced me to revisit every transition and caught two bugs before runtime.
```

### Tweet 4

```text
Write-up:
<canonical article URL>

Code:
https://github.com/willamhou/hypervisor

`make run` exercises the bare-metal test suites on QEMU in a few seconds.
```

## Zhihu Summary

### Title

`我用 3 万行 Rust 重写了 ARM Secure World 的 Hypervisor，还让它和 pKVM 跑在同一颗芯片上`

### Short Summary

```text
大多数人理解 Hypervisor，默认只有一层虚拟化控制面。但在现代 ARM 芯片上，Normal world 和 Secure world 可以各自拥有自己的 EL2。

这个项目做的事，是在 S-EL2 用 no_std Rust 重写 Secure world 的 SPMC，替换 Hafnium，并且和 Android 的 pKVM 同时跑在同一颗芯片上。Linux 侧发起一条 FF-A 请求后，请求会穿过 pKVM、TF-A、SPMC、Secure Partition，再完整返回。现在这条全链路已经通过 35/35 个端到端测试。

这篇文章重点写三件事：
1. 为什么 “两套 Hypervisor 共存” 比单一 Hypervisor 难很多
2. 为什么 Secure / Non-Secure 不只是权限问题，而是物理地址空间分裂
3. Rust 在状态机和控制流密集的 EL2 场景里，具体帮我避免了什么 bug

长文：
<canonical article URL>

项目地址：
https://github.com/willamhou/hypervisor
```

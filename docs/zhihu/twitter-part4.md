# Twitter Thread — Part 4: Rust 裸机四大坑

## 中文版

用 Rust 写 ARM64 裸机 hypervisor，踩了四个大坑。每个花了至少一天，每个都不报错：

1/ Debug 模式静默挂死。反汇编发现 `read_volatile` 的对齐检查被编译成了 NEON `cnt`，TF-A 默认 trap FP/SIMD。

2/ 写成功了但读回全是零。同一个地址，`NS=0` 落 Secure DRAM，`NS=1` 落 Non-Secure DRAM——两条总线路径。

3/ 间歇性解析失败。SPMC 在另一个 pCPU 上原地解析 pKVM 写过的 TX buffer，读出来的字节对不上。DSB + 本地拷贝后再解析才稳——自洽的快照好 debug，新鲜的脏读不好 debug。

4/ Secondary CPU 永远起不来。SPMD 是 per-CPU 的，每个物理 CPU 都要自己 `FFA_MSG_WAIT` 握手，然后留在自己的事件循环里——规范里一个字没写。

共同点：在你这一层看起来全对。错的是你对底下那层的心智模型。

知乎长文 👇
[链接]

## English version

Four silent bugs from writing a bare-metal ARM64 hypervisor in Rust. No panic, no fault output, just CPU stuck:

1/ Debug mode hangs at first `read_volatile`. Rust compiled the alignment check to NEON `cnt`. TF-A traps FP/SIMD from EL2 by default.

2/ Writes succeed but reads return zeros. Same physical address: `NS=0` goes to Secure DRAM, `NS=1` goes to Non-Secure DRAM. Two bus paths, one address.

3/ Intermittent parse failures. SPMC parses NWd TX buffer in place on a different pCPU than pKVM wrote it; bytes disagree. Fix: DSB + copy to local before parsing. Self-consistent snapshot is debuggable; fresh dirty reads aren't.

4/ Secondary CPUs hang forever. SPMD is per-CPU: each physical core has to call `FFA_MSG_WAIT` itself and then stay in its own event loop. The spec doesn't mention any of this.

Common thread: your layer looks correct. Your mental model of the layer below isn't.

Full write-up (Chinese) 👇
[链接]

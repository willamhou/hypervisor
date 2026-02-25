# Twitter Posts — Part 0

## 中文版

```
一个人 + Claude Code，30 天从零写了一个 ARM64 裸机 hypervisor。

之前带队做过同样的事——3-4 人，10 个月，Rust，世界上第一个过 CCRC 认证的 Rust SPMC。

这次：193 个 commit，Linux 4 核启动，virtio I/O，FF-A v1.1，TF-A 引导链，pKVM 集成。

写了一个系列记录整个过程。第一篇：为什么要做这件事，以及一个晚上的 boot.S 如何改变了我的判断。

👇
```

## English Version

```
Solo developer + Claude Code. 30 days. Built a bare-metal ARM64 Type-1 hypervisor from scratch in Rust.

I'd done this before — led a 3-4 person team, 10 months, world's first CCRC-certified Rust SPMC.

This time: 193 commits. Boots Linux with 4 CPUs, virtio I/O, FF-A v1.1, TF-A secure boot chain, pKVM integration at S-EL2.

Writing a series documenting the entire journey — technical deep-dives interleaved with AI pair programming reflections.

Part 0: why I did it, and how one evening's boot.S changed my mind.

🧵👇
```

## 配图建议

终端截图，选一个：
- `Protected hVHE mode initialized successfully`（pKVM 启动成功）
- BusyBox shell 提示符（Linux guest 启动到交互）
- `make run` 测试输出（33 test suites, ~282 assertions）

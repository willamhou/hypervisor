# Twitter Posts — Part 3

## 中文版

```
四个 CPU、一块磁盘 — 让 Linux 启动。

这是整个项目代码密度最高的一周：GICv3 虚拟化（GICD write-through + GICR trap-and-emulate + List Register 注入）、virtio-blk 磁盘、round-robin 调度器 + CNTHP 10ms 抢占、PSCI CPU_ON 唤醒 secondary vCPU。

每个子系统单独都不复杂，但必须同时正确工作，Linux 才能启动。差一个就是沉默的挂起。

三个踩坑故事：
• ICC_SGI1R_EL1 位域 — ARM 手册写法让人把 TargetList 和 INTID 搞反
• GIC List Register HW=1 — vtimer 中断消失（EOI 没到物理中断控制器）
• CNTHP 定时器被 guest 关掉 — 每次 vCPU 进入都要重新使能

系列第五篇（第三篇技术文）。

👇
```

## English Version

```
4 CPUs, 1 disk — booting Linux on a bare-metal hypervisor.

The densest week of the project: GICv3 virtualization (GICD write-through + GICR trap-and-emulate + List Register injection), virtio-blk storage, round-robin scheduler + CNTHP 10ms preemption, PSCI CPU_ON for secondary vCPUs.

Each subsystem is simple alone. But they must all work simultaneously, or Linux hangs — silently.

Three debugging war stories:
• ICC_SGI1R_EL1 bit fields — ARM manual formatting makes you swap TargetList and INTID
• GIC List Register HW=1 — vtimer interrupts vanish (EOI never reaches physical GIC)
• CNTHP timer silently disabled by guest — must re-enable before every vCPU entry

Part 3 of "Scratch a Rust Hypervisor."

🧵👇
```

## 配图建议

选一个：
- `run_one_iteration()` 调度循环的代码截图（pick vCPU → inject SGI → arm timer → run → handle exit）
- GICv3 虚拟化架构图（GICD write-through / GICR emulate / ICC→ICV redirect / LR inject）
- Linux boot 全流程 ASCII 图（DTB parse → GIC init → timer → virtio-blk → scheduler → BusyBox shell）

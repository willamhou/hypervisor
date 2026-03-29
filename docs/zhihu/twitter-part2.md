# Twitter Posts — Part 2

## 中文版

```
陷入-模拟-恢复：Hypervisor 的心跳。

Guest 执行 → 碰到特权操作 → 硬件陷入 EL2 → Hypervisor 模拟 → ERET 回去。Linux 每秒发生成千上万次。

50 行汇编的 enter_guest() 是整个系统最关键的函数。保存宿主、恢复 guest、ERET。如果 VcpuContext 的字段偏移量差一个字节，guest 会飞到随机地址。

文中两个踩坑故事：
• HPFAR_EL2 vs FAR_EL2 — guest MMU 开了之后 MMIO 全部消失（一天的调试）
• SPSR_EL2 不能碰 — "好心"清除中断屏蔽位 → guest 自旋锁死锁

系列第四篇（第二篇技术文）。

👇
```

## English Version

```
Trap-Emulate-Resume: the heartbeat of a hypervisor.

Guest runs → hits privileged op → hardware traps to EL2 → hypervisor emulates → ERET back. Thousands of times per second for Linux.

enter_guest() is 50 lines of assembly and the most critical function in the system. Save host, restore guest, ERET. If a VcpuContext field offset is off by one byte, the guest jumps to a random address.

Two debugging war stories:
• HPFAR_EL2 vs FAR_EL2 — all MMIO devices vanish when guest MMU turns on
• Never touch SPSR_EL2 — "helpfully" clearing the IRQ mask bit → guest spinlock deadlock

Part 2 of the "Scratch a Rust Hypervisor" series.

🧵👇
```

## 配图建议

选一个：
- `enter_guest()` 汇编代码高亮截图（`stp` 保存 + `ldp` 恢复 + `eret`）
- 文中的 trap-emulate-resume ASCII 架构图
- `ExitReason` 枚举 + `handle_exception()` match 的代码截图

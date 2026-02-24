# Part 0: Prologue — Why and How

> From a 10-month production hypervisor to a 1-month AI-assisted rebuild in Rust.

In January 2026, I started writing a bare-metal ARM64 hypervisor from scratch in Rust. Not as an academic exercise — I'd done this before professionally. But this time, my pair programming partner wasn't human. It was Claude Code.

30 days. 193 commits. From the first `boot.S` to booting Linux with 4 CPUs, virtio storage, inter-VM networking, FF-A firmware framework, TF-A Secure World boot chain, and pKVM integration at S-EL2.

This series documents the entire journey, interleaving technical deep-dives with reflections on AI-assisted systems programming — what worked, what didn't, and what surprised me.

## What This Series Covers

| Part | Topic | Key Milestone |
|------|-------|---------------|
| 0 | Prologue | Background, motivation, workflow |
| 1 | First Boot | "Hello from EL2!" |
| 2 | vCPU | Guest execution via ERET |
| 3 | Exceptions & GICv3 | Interrupt virtualization |
| 4 | Boot Linux | BusyBox shell prompt |
| 5 | SMP | 4 vCPUs on 4 physical CPUs |
| 6 | Multi-VM | 2 VMs with virtio-net networking |
| 7 | FF-A | Firmware Framework for Arm v1.1 |
| 8 | TF-A Boot Chain | BL1→BL2→BL31→BL32→BL33 |
| 9 | S-EL2 SPMC | Replacing Hafnium |
| 10 | pKVM | The final architecture |

## How to Read

Each Part follows the same structure:

- **Architecture**: The ARM concepts and design decisions
- **Implementation**: Code walkthrough with commit links
- **Testing**: Test strategy and key assertions
- **Debugging Notes**: Real bugs, real fixes — the war stories
- **AI Collaboration Notes**: How Claude Code helped (or didn't)

Start here, then proceed to [Part 1](../part1-first-boot/README.md), or jump to any Part that interests you.

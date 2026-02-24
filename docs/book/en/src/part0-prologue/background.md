# Personal Background

I'm currently co-founder of an AI startup. Before that, I worked at both big tech companies and startups, always gravitating toward the lowest layers of the stack — kernels, hypervisors, firmware. The kind of code where a wrong bit in a register means a silent hang, and "printf debugging" means writing bytes to a UART by hand.

## The First Hypervisor

At one of the big tech companies, I led a team of 3–4 engineers to build a bare-metal ARM64 hypervisor from scratch — in Rust. The full journey — from the first line of code to passing CCRC security certification to commercial deployment — took about a year. Development alone was around 10 months. To our knowledge, it was the world's first CCRC-certified security hypervisor (SPMC) written in Rust.

That experience left a mark. A few things I still carry:

The complexity is never where you expect it. The trap-emulate-resume loop? That's the easy part. You write it in a weekend. Then you spend the next 9 months on everything around it — getting GICv3 list register injection right, handling dozens of ESR_EL2 exception classes, debugging a Stage-2 page table corruption that only manifests on the 4th vCPU under SMP stress. The core loop is maybe 5% of the work.

The ARM architecture manual is 12,000 pages, and you will need about 3,000 of them. When something goes wrong at EL2, there's no debugger, no stack trace, no helpful error message. Just a silent hang. Or worse, a hang that happens once every 200 boots. You learn to read ESR_EL2 syndrome values the way a doctor reads an EKG.

Rust worked. We bet on it early — `no_std`, custom targets, hand-rolled register access. The ecosystem was immature (still is — no off-the-shelf GIC drivers, no ARM intrinsics crate). But the payoff was real: zero use-after-free bugs in 10 months of development. In a C hypervisor, that would be unheard of.

Testing saved us constantly. Without a solid test harness, every change is a coin flip. Bugs at EL2 don't crash with a nice message — they corrupt guest state silently, and you don't notice until three features later when something completely unrelated breaks.

## The Dual Background

The AI side of my career gave me something unexpected: a tolerance for imprecision. ML engineers prototype fast and iterate. Systems engineers read the spec three times before writing a line. Both are right, depending on the stage.

This project needed both. The hypervisor demands precision — get a register bit wrong and the guest hangs. But the AI-assisted workflow demands willingness to try things that might not work, throw away what doesn't, and move on.

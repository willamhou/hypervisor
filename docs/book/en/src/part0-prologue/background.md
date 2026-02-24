# Personal Background

I live at the intersection of AI and systems software.

I'm currently co-founder of an AI startup. Before that, I worked at both big tech companies and startups, always gravitating toward the lowest layers of the stack — kernels, hypervisors, firmware.

## The First Hypervisor

At one of the big tech companies, I led a team of 3–4 engineers to build a bare-metal ARM64 hypervisor from scratch — in Rust. The full journey — from the first line of code to passing CCRC security certification to commercial deployment — took about a year. Development alone was around 10 months. To our knowledge, it was the world's first CCRC-certified security hypervisor (SPMC) written in Rust.

That experience taught me a few things:

- **Hypervisors are deceptively complex.** The core loop (trap → emulate → resume) is simple. Everything around it — SMP, interrupt virtualization, device emulation, memory management — is where the real complexity lives.
- **The ARM architecture manual is your bible.** When something goes wrong at EL2, there's no debugger, no stack trace, no helpful error message. Just a silent hang or a cryptic ESR_EL2 value.
- **Rust is the right language for this domain.** Memory safety without a runtime, zero-cost abstractions over hardware registers, and `no_std` support made it viable for bare-metal firmware. But the ecosystem was (and still is) immature — no off-the-shelf GIC drivers, no ARM intrinsics crate, everything hand-rolled.
- **Testing is everything.** Without a solid test harness, you're flying blind. Every bug at this level can manifest as a completely unrelated symptom.

## The Dual Background

The AI side of my career gave me something the systems side didn't: a habit of rapid prototyping and iteration. ML engineers think in experiments — try something, measure it, adjust. Systems engineers think in specifications — read the manual, implement exactly, verify.

This project combines both mindsets. The hypervisor demands precision. The AI-assisted workflow demands willingness to experiment.

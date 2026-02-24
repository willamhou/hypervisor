# Project Motivation

## The Idea That Wouldn't Die

Mid-2025. I kept circling back to the same thought: what if I rebuilt the hypervisor from scratch?

The [original one](background.md) was also in Rust — a 3–4 person team, 10 months, the world's first CCRC-certified Rust SPMC. We'd proven Rust was viable for production hypervisors. But we'd also felt, viscerally, how much raw effort it takes. ARM's architecture surface area is enormous. Every corner case — the obscure GICR register that only matters when a specific CPU is in a specific power state, the Stage-2 fault that only triggers when both guest MMU and hypervisor MMU are on — has to be handled correctly. There's no "close enough."

Could one person replicate the core of that journey? Probably. But it would take months. I had a startup to run.

I shelved it.

## Claude Code Changed the Equation

Late 2025, I was using Claude Code for day-to-day application work — API endpoints, frontend, nothing exotic. Then one evening, on a whim, I asked it to write a `boot.S` for an ARM64 EL2 entry point. Stack setup, BSS zeroing, jump to Rust.

It got it right. Not approximately right — actually right. Correct use of `adr` vs `ldr`, proper `.section .text.boot` placement, `wfe` halt loop. I'd spent years working with engineers who got this wrong on their first try.

So I pushed harder. "Write me a Stage-2 identity map with 2MB blocks." Correct. "Trap WFI via HCR_EL2.TWI and handle it in the exception vector." Correct, including the PC advance.

It understood the difference between EL1 and EL2. It could reason about `VTTBR_EL2` bits. It knew that `HPFAR_EL2` gives you the IPA while `FAR_EL2` gives you the VA. (Though it would later get confused about exactly when this matters — more on that in Part 4.)

The idea came back. Not "can AI write a hypervisor?" — that's the wrong question. The right question was: **can AI compress what took a team of 3–4 engineers 10 months into something a solo developer can do in weeks?**

## The Experiment

I set a simple goal: rebuild the core hypervisor journey from scratch, in Rust, using Claude Code as a pair programmer. Not a toy — a real Type-1 hypervisor that boots Linux, handles SMP, does virtio I/O, and implements the ARM FF-A firmware framework.

The rules:
- **Rust only** (plus necessary ARM64 assembly for boot and exception vectors)
- **No existing hypervisor code** — no forking Hafnium, no copying from the production version
- **AI as pair programmer** — Claude Code for planning, implementation, testing, debugging
- **Everything documented** — every commit, every design decision, every bug

January 26, 2026: first commit. February 24, 2026: pKVM boots with our SPMC at S-EL2, FF-A v1.1 fully functional.

30 days. 193 commits. Let's see how it happened.

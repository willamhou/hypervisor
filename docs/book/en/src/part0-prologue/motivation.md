# Project Motivation

## The Idea That Wouldn't Die

Mid-2025. I'd been thinking about rebuilding a hypervisor from scratch. The [original one](background.md) I led was also in Rust — a 3–4 person team, 10 months, the world's first CCRC-certified Rust SPMC. We'd proven Rust was viable for production hypervisors. But we'd also learned how much raw effort it takes: the ARM architecture surface area is enormous, and every corner case must be handled correctly.

Could one person, working alone, replicate the core of that journey? Maybe. But writing a bare-metal hypervisor solo — with all the inline assembly, custom linker scripts, and architecture-specific register manipulation — felt like a multi-month commitment. I had a startup to run.

## Claude Code Changed the Equation

Late 2025, Anthropic shipped a major update to Claude Code. I'd been using it for application-level work, but the improvements in systems programming capability caught my attention. It could reason about ARM architecture registers. It could write coherent `no_std` Rust. It understood the difference between EL1 and EL2.

The idea reignited.

Not "can AI write a hypervisor?" — that's the wrong question. The right question was: **can AI compress what took a team of 3–4 engineers 10 months into something a solo developer can do in weeks?**

## The Experiment

I set a simple goal: rebuild the core hypervisor journey from scratch, in Rust, using Claude Code as a pair programmer. Not a toy — a real Type-1 hypervisor that boots Linux, handles SMP, does virtio I/O, and implements the ARM FF-A firmware framework.

The rules:
- **Rust only** (plus necessary ARM64 assembly for boot and exception vectors)
- **No existing hypervisor code** — no forking Hafnium, no copying from the C version
- **AI as pair programmer** — Claude Code for planning, implementation, testing, debugging
- **Everything documented** — every commit, every design decision, every bug

January 26, 2026: first commit. February 24, 2026: pKVM boots with our SPMC at S-EL2, FF-A v1.1 fully functional.

30 days. 193 commits. Let's see how it happened.

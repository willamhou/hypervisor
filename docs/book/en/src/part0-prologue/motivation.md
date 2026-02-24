# Project Motivation

## The Idea That Wouldn't Die

Mid-2025. I'd been thinking about rewriting a hypervisor in Rust. The original one I built was in C — the industry default for this kind of work. But C at this level is a minefield: undefined behavior, manual memory management, no type safety for register-width values. Rust's `no_std` ecosystem had matured enough that the idea was technically feasible.

I shelved it. Writing a bare-metal hypervisor solo in Rust — with all the inline assembly, custom linker scripts, and architecture-specific register manipulation — felt like a multi-month commitment. I had a startup to run.

## Claude Code Changed the Equation

Late 2025, Anthropic shipped a major update to Claude Code. I'd been using it for application-level work, but the improvements in systems programming capability caught my attention. It could reason about ARM architecture registers. It could write coherent `no_std` Rust. It understood the difference between EL1 and EL2.

The idea reignited.

Not "can AI write a hypervisor?" — that's the wrong question. The right question was: **can AI compress a 10-month development cycle into something a solo developer can do in weeks?**

## The Experiment

I set a simple goal: rebuild the core hypervisor journey from scratch, in Rust, using Claude Code as a pair programmer. Not a toy — a real Type-1 hypervisor that boots Linux, handles SMP, does virtio I/O, and implements the ARM FF-A firmware framework.

The rules:
- **Rust only** (plus necessary ARM64 assembly for boot and exception vectors)
- **No existing hypervisor code** — no forking Hafnium, no copying from the C version
- **AI as pair programmer** — Claude Code for planning, implementation, testing, debugging
- **Everything documented** — every commit, every design decision, every bug

January 26, 2026: first commit. February 24, 2026: pKVM boots with our SPMC at S-EL2, FF-A v1.1 fully functional.

30 days. 193 commits. Let's see how it happened.

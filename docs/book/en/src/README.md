# Scratch a Rust Hypervisor

> Building a bare-metal ARM64 Type-1 hypervisor from scratch in Rust, using AI pair programming.

This is the story of rebuilding a production-grade hypervisor — a journey that previously took 10 months — in about 1 month, using Claude Code and vibe coding.

From the first `boot.S` to pKVM integration with FF-A v1.1, every commit is documented. Each chapter covers the technical implementation alongside the AI collaboration process.

## What You'll Learn

- ARM64 virtualization (EL2, Stage-2, GICv3, PSCI)
- Rust no_std bare-metal programming
- Linux kernel boot process under a hypervisor
- ARM FF-A firmware framework and Secure World
- AI-assisted systems programming workflow

## The Repository

All code is at [github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor) — 191 commits, 33 test suites, ~282 assertions.

## How to Read

**Top-down**: Start with Part 0 for context, then follow sequentially or jump to any Part.

**Each Part**: Overview → Architecture → Implementation → Testing → Debugging → AI Notes.

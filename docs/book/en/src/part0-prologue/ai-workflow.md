# AI Workflow — Vibe Coding with Claude Code

## What is Vibe Coding?

The term gets a bad rap. People hear "vibe coding" and think "letting AI write code you don't understand." That's not what happened here.

Vibe coding, as I practiced it, means: **you own the architecture, the AI owns the keystrokes.** You describe what you want at the level of ARM concepts — "add GICR trap-and-emulate for per-vCPU redistributor state" — and the AI translates that into Rust code. You review every line. You reject what's wrong. You know *why* it's right when it's right, because you've done this before.

The "vibe" part is real, though. There were genuinely moments where I'd describe a feature in two sentences and get back 200 lines of working code that I'd only need to adjust in minor ways. That's a qualitatively different experience from writing it yourself. You stay in the design headspace instead of context-switching between architecture and syntax.

For application code, this is table stakes now. For bare-metal systems programming — ARM64 registers, `no_std` Rust, memory-mapped I/O — it was genuinely surprising that it worked as well as it did.

## The Development Loop

In theory, every feature followed this cycle:

```
1. Plan    → Design doc with architecture constraints
2. Test    → Write test cases first (TDD)
3. Implement → AI generates code, human reviews
4. Debug   → AI proposes fixes, human validates against ARM manual
5. Review  → Code review for correctness and style
6. Commit  → Conventional commit with clear message
```

In practice? Steps got skipped. A lot. Early features (Part 1-2) had almost no upfront design — I just described what I wanted and iterated until it worked. TDD was aspirational for bare-metal code where the "test" is "boot QEMU and see if it hangs." The clean six-step loop emerged gradually, starting around Part 3 (exception handling), when the codebase got complex enough that skipping design meant wasting hours.

The git history shows this honestly. Early commits are messy `feat:` blobs. Later commits follow the pattern: `docs:` (design), `feat:` (implementation), `fix:` (debugging), `docs:` (update CLAUDE.md). The discipline grew with the project.

## CLAUDE.md — The Project Brain

The single most important file in the project is [`CLAUDE.md`](https://github.com/willamhou/hypervisor/blob/main/CLAUDE.md). Claude Code reads it at the start of every session. It's a living document — part architecture guide, part institutional memory, part "don't make this mistake again" list.

What's in it:
- **Architecture overview**: privilege model, core abstractions, exception flow
- **Build commands**: every `make` target with feature flag documentation
- **Memory layout**: address map for hypervisor, guests, devices
- **Critical gotchas**: "use HPFAR_EL2, not FAR_EL2 for IPA", "never modify guest SPSR_EL2", "inject_spi() must not acquire DEVICES lock"
- **Test inventory**: all 33 test suites with assertion counts

This file grew organically. The pattern was always the same: we'd hit a bug, spend an hour debugging it, fix it, then add a line to CLAUDE.md so the AI wouldn't make the same mistake in the next session. By the end it was over 500 lines — essentially a compressed architecture reference that no human would bother writing from scratch, but that accumulated naturally through pain.

## Agent Orchestration

Claude Code supports specialized agents. The ones that earned their keep:

| Agent | Role | Verdict |
|-------|------|---------|
| **planner** | Design architecture, break down features | Essential for Part 5+ (SMP). Overkill for early parts. |
| **tdd-guide** | Write tests first, then implement | Worked well for pure-logic code (FF-A parsing). Useless for hardware interaction. |
| **code-reviewer** | Review for correctness and style | Caught real bugs. Worth running every time. |
| **build-error-resolver** | Fix compilation errors | Mixed. Good for type errors. Bad for linker errors. |
| **architect** | Evaluate design trade-offs | Used sparingly. Helpful for multi-VM memory layout. |

The agent separation matters more than you'd think. The planner doesn't see test code. The reviewer doesn't see design docs. This focus keeps each agent from drowning in irrelevant context.

## What AI Was Good At

**Boilerplate and structure.** Register definitions, 50-line `match` arms, test harnesses. The kind of code that's tedious but not intellectually hard. AI crushed this.

**Cross-referencing specs.** "Implement FF-A v1.1 Table 5.19 descriptor parsing" → working code, including the correct struct offsets and `read_unaligned` for packed layouts. The AI could hold the spec in context and translate it directly.

**Pattern replication.** Once one virtio device (virtio-blk) worked end-to-end, adding virtio-net was nearly automatic. The AI recognized the pattern and applied it with minor variations.

**Debugging hypotheses.** Given an ESR_EL2 value and symptoms, the AI was often right about the category of bug — "this looks like a Stage-2 permission fault, check S2AP bits." Not always right about the specific fix, but right about where to look.

## What AI Was Bad At

**The HPFAR_EL2 bug.** This is the canonical example. When the guest MMU is on, `FAR_EL2` gives you the guest *virtual* address, not the IPA. You need `HPFAR_EL2` for the IPA. The AI kept suggesting fixes to the MMIO dispatch layer when the real bug was in the IPA extraction. This cost a full day. (Part 4 has the full story.)

**Concurrency.** The `inject_spi()` deadlock on multi-pCPU required understanding that the function was called from *inside* a `DEVICES` lock, so it couldn't re-acquire that lock to look up the SPI route. The AI proposed "just add a lock" three times before I explained the re-entrancy issue.

**Build system integration.** TF-A's build (Makefiles → Docker → FIP packaging → sp_layout.json → UUID byte-swapping) had too many moving parts. The AI could fix any one piece in isolation but couldn't reason about the whole pipeline.

**False confidence.** Sometimes AI-generated code passed all tests but was correct by accident — the test didn't cover the edge case that mattered. The human's job was to ask "is this right for the right reason?" and add the missing test.

## The Numbers

Rough estimates from 30 days of development:

- **193 commits** (~6.4/day)
- **~282 test assertions** across 33 test suites
- **Human contribution**: ~30% — architecture decisions, ARM manual lookups, debugging integration issues, reviewing every AI-generated line
- **AI contribution**: ~70% — code generation, test writing, boilerplate, initial debugging hypotheses
- **CLAUDE.md**: 0 → 500+ lines of accumulated context

The 30/70 split is misleading, though. The 30% human work was disproportionately *load-bearing*. The AI couldn't have done the project alone. But I couldn't have done it in 30 days alone either.

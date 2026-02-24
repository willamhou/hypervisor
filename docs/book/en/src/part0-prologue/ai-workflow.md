# AI Workflow — Vibe Coding with Claude Code

## What is Vibe Coding?

Vibe coding is programming by intent rather than by keystroke. You describe what you want at a high level — "add GICR trap-and-emulate for per-vCPU redistributor state" — and the AI translates that into working code. You stay in the architecture and design space. The AI handles the implementation details.

For application code, this is table stakes. For bare-metal systems programming, it's a different beast entirely. The AI needs to understand ARM architecture registers, exception level semantics, memory-mapped I/O conventions, and the subtleties of `no_std` Rust. Getting this right required a specific workflow.

## The Development Loop

Every feature followed this cycle:

```
1. Plan    → Design doc with architecture constraints
2. Test    → Write test cases first (TDD)
3. Implement → AI generates code, human reviews
4. Debug   → AI proposes fixes, human validates against ARM manual
5. Review  → Code review for correctness and style
6. Commit  → Conventional commit with clear message
```

This is visible in the git history. Most features have a `docs:` commit (design), then `feat:` commits (implementation), then `fix:` commits (debugging), then `docs:` again (update CLAUDE.md).

## CLAUDE.md — The Project Brain

The single most important file in the project is [`CLAUDE.md`](https://github.com/willamhou/hypervisor/blob/main/CLAUDE.md). It's a living document that Claude Code reads at the start of every session. It contains:

- **Architecture overview**: Privilege model, core abstractions, exception flow
- **Build commands**: Every `make` target with feature flag documentation
- **Memory layout**: Address map for hypervisor, guests, devices
- **Critical implementation details**: Hard-won knowledge like "use HPFAR_EL2, not FAR_EL2" and "never modify guest SPSR_EL2"
- **Test inventory**: All 33 test suites with assertion counts

This file grew organically. Every time we hit a bug caused by missing context, the fix included updating CLAUDE.md so the AI wouldn't make the same mistake twice. By the end, it was over 500 lines — essentially a compressed architecture reference.

## Agent Orchestration

Claude Code supports specialized agents for different tasks. The workflow used:

| Agent | Role | When |
|-------|------|------|
| **planner** | Design architecture, break down features | Before any implementation |
| **tdd-guide** | Write tests first, then implement | Every new feature |
| **code-reviewer** | Review for correctness, security, style | After every implementation |
| **build-error-resolver** | Fix compilation errors | When `make` fails |
| **architect** | Evaluate design trade-offs | Major architectural decisions |

These agents run as sub-processes with focused contexts. The planner doesn't see test code. The code-reviewer doesn't see design docs. This separation keeps each agent's context window focused.

## What AI Was Good At

- **Boilerplate and structure**: Register definitions, match arms, test harnesses
- **Cross-referencing specs**: "Implement FF-A v1.1 Table 5.19 descriptor parsing" → working code
- **Pattern application**: Once one virtio device worked, adding a second was nearly automatic
- **Debugging hypotheses**: Given an ESR_EL2 value and symptoms, AI could narrow down likely causes

## What AI Was Bad At

- **Subtle architecture bugs**: The HPFAR_EL2 vs FAR_EL2 bug took a full day. AI kept suggesting fixes to the wrong layer.
- **Concurrency reasoning**: The `inject_spi()` deadlock on multi-pCPU required human understanding of the lock acquisition order.
- **TF-A integration**: The build system (Makefiles, Docker volumes, FIP packaging) had too many moving parts for AI to reason about holistically.
- **"It works but I don't know why"**: Sometimes AI-generated code passed tests but the human had to verify it was correct for the right reasons, not just accidentally.

## The Numbers

- **193 commits** in 30 days (~6.4 commits/day)
- **~282 test assertions** across 33 test suites
- **Estimated human:AI ratio**: 30% human (architecture decisions, ARM manual lookups, debugging) / 70% AI (code generation, test writing, boilerplate)
- **CLAUDE.md**: grew from 0 to 500+ lines — the accumulated context

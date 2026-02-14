# Milestone 2 Implementation Plans

## Overview

**Goal:** Complete Milestone 2 with GICv3, Dynamic Memory, Multi-vCPU Support, and API Documentation.

**Target Architecture:** ARMv8.4+, GICv3

**Total estimated time: 23-32 hours**

## Sprint Plans

| Sprint | Feature | Time | Plan File |
|--------|---------|------|-----------|
| 2.1 | GICv3 Virtual Interface | 3-4h | [sprint-2.1-gic-cpu-interface.md](2026-02-04-sprint-2.1-gic-cpu-interface.md) |
| 2.2 | Dynamic Memory Management | 4-6h | [sprint-2.2-dynamic-memory.md](2026-02-04-sprint-2.2-dynamic-memory.md) |
| 2.3 | Multi-vCPU Support | 15-20h | [sprint-2.3-multi-vcpu.md](2026-02-04-sprint-2.3-multi-vcpu.md) |
| 2.4 | API Documentation | 1-2h | [sprint-2.4-api-documentation.md](2026-02-04-sprint-2.4-api-documentation.md) |

## Dependencies

```
Sprint 2.1 (GICv3)  ─────┐
                         ├──> Sprint 2.3 (Multi-vCPU)
Sprint 2.2 (Memory) ─────┘

Sprint 2.4 (Docs) - Independent, can run anytime
```

## Execution

Each plan follows the TDD workflow:
1. Write failing test
2. Verify test fails
3. Write minimal implementation
4. Verify test passes
5. Commit

**To execute a sprint:**
- Use `superpowers:executing-plans` skill
- Or use `superpowers:subagent-driven-development` for this session

## Key Technical Decisions

- **GICv3 over GICv2**: Use List Registers for hardware-assisted interrupt injection
- **ARMv8.4+**: Target nested virtualization and enhanced VMID features
- **Bump Allocator**: Simple forward-only allocation suitable for hypervisor
- **Round-Robin Scheduler**: Simple but correct multi-vCPU scheduling

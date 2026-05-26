---
title: "Field Notes: Booting a Full NS→Secure ARM Chain on a Real ARM Server with Stock QEMU"
published: true
description: "TF-A → our S-EL2 SPMC → Secure Partitions → FF-A client, 20/20 E2E on a bare ARM64 box — no /dev/kvm, no sudo, no custom QEMU. Plus: why KVM can't save the Secure world, and where Android AVF actually stops."
tags: rust, arm, hypervisor, qemu
cover_image:
canonical_url: https://willamhou.github.io/hypervisor/
---

My ARM64 Type-1 hypervisor has a Secure-world personality: an S-EL2 **SPMC** (Secure Partition Manager Core) that runs as TF-A's BL32, manages Secure Partitions at S-EL1, and speaks ARM's FF-A v1.1 protocol. Until now I'd only ever exercised that chain under QEMU TCG on an x86 dev box.

This week I got a real ARM64 Linux server (aarch64, `/dev/kvm` present, SVE2, kernel 6.11). The obvious question:

> On real ARM hardware, can I finally use KVM acceleration — maybe even run Android's AVF?

The answer is more interesting than yes/no. These are the field notes from getting the full chain up, in the order it actually happened, traps included.

{% github willamhou/hypervisor %}

## Counterintuitive #1: a real ARM box + /dev/kvm still can't help the Secure world

It's tempting to assume x86 → "emulation only", ARM → "KVM, full speed". The Secure world is the exception. The chain I need to boot is:

```
EL3    TF-A BL31 + SPMD       ← secure monitor
S-EL2  our SPMC               ← manages Secure Partitions
S-EL1  SP1 / SP2 / SP3        ← the partitions
NS-EL2 pKVM / test client
NS-EL1 Linux/Android
```

The hard fact: **QEMU's `secure=on` (EL3, Secure world, S-EL2) is TCG-only. KVM cannot virtualize EL3 or the Secure world** — there is no "nested Secure virtualization" on ARM. So this NS→Secure chain runs under TCG regardless of whether the host is x86 or ARM.

What's `/dev/kvm` good for here, then? Only the **pure Normal-world** paths (accelerating NS-EL2 pKVM, or AVF's crosvm) — and only with extra conditions (more below).

Correction #1: **TCG for the Secure world isn't a compromise — it's the only correct way, and it's entirely sufficient.**

## Trap #1: the box had no QEMU, and no sudo

`make run` wants `qemu-system-aarch64`. It wasn't in `PATH`, or anywhere on the system. Worse:

- no passwordless `sudo` (can't install packages non-interactively)
- not in the `docker` group (every TF-A/QEMU build target uses Docker)
- not in the `kvm` group (`/dev/kvm` → Permission denied)

I first tried a **fully root-free** route: micromamba (a single static binary) + conda-forge to install QEMU. Download and extract were fine; the `linux-aarch64` channel was not:

```
qemu =* * does not exist
```

conda-forge's ARM64 channel ships only **`qemu.qmp` (a Python lib), not the QEMU emulator itself**. The x86 channel has it; arm64 doesn't. Worth remembering: don't assume conda gives you `qemu-system` on arm64.

Building from source? Missing `glib`, `pixman`, `meson`, `ninja` — none installed.

The plainest path won. One sudo line from someone who had it:

```bash
sudo apt install -y qemu-system-arm   # this package also provides qemu-system-aarch64
```

That gave me QEMU **8.2.2**. Remember that version — there's a payoff later.

## Normal world first: make run, 34/34

Basic unit-test suites (NS-EL2 + TCG, no Secure world):

```bash
qemu-system-aarch64 -machine virt,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic -kernel target/.../hypervisor
```

Clean boot to EL2 — GICv3, 16 MB heap, timer all up — and **all 34 test suites ran with zero failures.**

Two gotchas worth noting: the final suite `guest_interrupt` enters a guest and **never returns** by design, so wrap QEMU in `timeout --foreground 90 ...` or it hangs. And `[INIT] DTB: parse failed, using defaults` is expected — in `-kernel` mode QEMU passes DTB address 0, and the code falls back to virt defaults.

## Trap #2: I joined the docker group but my session couldn't see it

The Secure world needs Docker. After `sudo usermod -aG docker $USER`, `/etc/group` confirmed it:

```
docker:x:988:wilamhou
```

…yet `id` in my shell still didn't list `docker`. Group changes only take effect for **newly logged-in processes**, and my shell's parent process predated the change — re-logging into the UI doesn't restart that process.

Rather than restart the whole session, `sg` (switch group) reads `/etc/group` at runtime:

```bash
sg docker -c 'docker ps'              # works immediately, no re-login
sg docker -c 'make build-tfa-spmc'
```

`sg docker -c '<cmd>'` runs the entire command as the docker group, including the `docker run` children it forks. Handy whenever you've just been added to a group and don't want to log out.

## Build + boot the full Secure chain

One command builds TF-A + the SPMC + all three SPs (compiled inside Docker, ~10–20 min the first time):

```bash
sg docker -c 'make build-tfa-spmc'
```

A landmine here, in passing: at S-EL2, `CPTR_EL3.TFP` traps all FP/SIMD, and Rust **debug** builds emit NEON (`cnt v0.8b`) for `read_volatile` alignment checks — which **silently hangs** the moment it executes. The repo's `[profile.dev]` already sets `opt-level = 1`, so the `sel2` build emits no NEON and the trap is defused before it bites.

With artifacts in place (`flash-spmc.bin` 64 MB, `hypervisor_spmc.bin`, SP1/2/3), run it:

```bash
qemu-system-aarch64 -machine virt,secure=on,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic -bios tfa/flash-spmc.bin -nic none
```

The whole four-level privilege stack comes up:

```
NOTICE:  BL31: v2.12.0(debug)          ← EL3 TF-A + SPMD
[SPMC] Running at EL2                   ← S-EL2, our SPMC
[SPMC] spmc_id=0x8000 version=1.1
[SPMC] S-EL2 Stage-1 MMU enabled (NS DRAM mapped)
[SP] Hello from S-EL1!                  ← SP1
[SP2] Hello from S-EL1!                 ← SP2
[SP3] Hello from S-EL1 (sp_relay)       ← SP3
...
  Test 1: FFA_VERSION .............. PASS
  ...
  Test 20: SP-to-SP MEM_RECLAIM .... PASS
  All tests complete.
```

**20/20 BL33 tests pass** — FF-A discovery, DIRECT_REQ (incl. multi-SP), preemption + FFA_RUN, secure vIRQ injection, full MEM_SHARE/LEND lifecycles, and SP↔SP relay + cycle detection + memory share/reclaim. The complete chain works.

## The biggest payoff: stock QEMU 8.2.2 was enough

The Makefile carries this note:

> Local QEMU 9.2+ for S-EL2 targets (secure=on requires newer QEMU)

implying you must compile QEMU 9.2.3 from source (a 20–40 minute job) for Secure-world targets. In practice — **Ubuntu's stock QEMU 8.2.2 runs S-EL2 just fine; that "needs 9.2+" note is overly cautious.** The custom-QEMU build step was unnecessary.

(Mechanically, `QEMU_SEL2` is "use `tools/qemu-system-aarch64` if present, else fall back to system qemu". By not building a custom one, it picks up 8.2.2 automatically.)

## So can you run Android AVF under QEMU?

The other big question of the day. It splits into two layers:

| Layer | What it is | Runs under QEMU? |
|---|---|---|
| **pKVM** (EL2 hypervisor) | protected KVM, `kvm-arm.mode=protected` | ✅ Yes. `make run-pkvm` boots an AOSP kernel in pKVM mode to BusyBox under TCG, FF-A working |
| **crosvm / pVM** (EL0 creates protected VMs) | a VMM creating pVMs via `/dev/kvm` | ❌ Not currently — fails with `failed to create IRQ chip` |

Why crosvm stalls: it calls `/dev/kvm` **inside** the guest to create a pVM, which needs the guest kernel's KVM to actually work — and QEMU TCG can't create `KVM_DEV_TYPE_ARM_VGIC_V3` (the vGICv3 device). To make it work you either run QEMU with `-accel kvm` **plus nested virtualization** (giving the guest a real EL2), but this host has `kvm.nested` disabled; or run crosvm **natively** on the host, blocked by `/dev/kvm` permissions. None of this involves my hypervisor — it's the boundary of Android's own pKVM + crosvm under pure TCG.

In one line: **Android's pKVM hypervisor layer runs under QEMU today; full AVF's pVM-creation layer does not under TCG** — you need nested-virt KVM or native KVM access.

## Reusable takeaways

1. **The Secure world (EL3/S-EL2) is always TCG.** KVM can't help, and TCG is plenty — don't agonize over acceleration here.
2. **conda-forge's arm64 channel has no `qemu-system` binary.** Don't burn time there.
3. **`sg docker -c '...'`** uses a freshly-added group without logging out.
4. **Stock QEMU 8.2.2 handles S-EL2 `secure=on`** — no need to compile 9.2.3.
5. **S-EL2 Rust builds must be `opt-level >= 1`**, or debug-mode NEON alignment checks hang silently.
6. Wrap `run` / `run-spmc` in `timeout` — they end in an idle/blocking state.

The real upside of running on ARM hardware isn't the Secure world (that's TCG by design) — it's the *future* possibility of unlocking AVF's crosvm layer, which needs one of two doors opened: nested virtualization, or native `/dev/kvm`. That's the next post.

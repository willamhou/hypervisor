# 2026-05-05 Flagship Article Plan

## Goal

Publish one English flagship article that does three jobs at once:

- explain the project to systems readers in one sitting
- drive high-intent traffic to the repo and architecture docs
- create a reusable canonical link for HN, Rust Forum, X, and Zhihu summaries

This should be a **compressed version** of [docs/blog-post.md](../blog-post.md), not a brand new article from scratch.

## Audience

Primary audience:

- Rust systems programmers
- hypervisor / virtualization engineers
- ARM firmware and security people
- curious low-level builders who will actually click into source code

Not the target:

- general-programming readers who want a broad intro to virtualization
- product audiences

## Recommended Title

**Building a Rust ARM64 SPMC that Replaces Hafnium and Runs Beside pKVM**

Why this one:

- front-loads `Rust`, `ARM64`, `SPMC`, `Hafnium`, and `pKVM`
- tells experts immediately why the project is unusual
- is less marketing-heavy than `Two Hypervisors, One SoC`
- works as a blog title, HN link title, and forum post title with minimal edits

## Backup Titles

1. `Two Hypervisors, One SoC: Replacing Hafnium with 30K Lines of Rust`
2. `A no_std Rust Hypervisor at S-EL2: Replacing Hafnium and Booting Linux`
3. `What It Took to Build a Rust SPMC for ARM64 Secure EL2`

## Positioning

Use this framing in the opening:

> Hafnium already exists. The interesting question is not "can Rust replace it" in theory, but what you learn by building the Secure-world hypervisor yourself: boot flow, FF-A, page ownership, cross-world memory, and the bugs the spec does not prepare you for.

The article should feel like:

- rigorous engineering write-up
- first-person build log with technical authority
- specific enough that an expert can evaluate whether the claims are real

It should not feel like:

- a generic "Rust is great" post
- a repo launch announcement with no technical substance
- a tutorial trying to teach all of ARM security architecture

## Target Length

`1,500-2,200` words.

That is long enough to feel substantial, but short enough for HN / Rust Forum readers to finish.

## Article Structure

### 1. Hook

Open with the unusual system shape:

- one chip
- two hypervisors
- pKVM at `NS-EL2`
- this project at `S-EL2`
- FF-A messages relayed by EL3 firmware
- Linux guest talking to Secure Partitions through the whole stack

End the intro with one concrete proof point:

- `35/35` end-to-end tests pass through Linux -> pKVM -> TF-A -> SPMC -> SPs

### 2. Why build this instead of using Hafnium

Keep this tight:

- learning by rebuilding the Secure-world control plane
- smaller codebase and auditability
- Rust state machines and ownership checks matter more at EL2 than in app code

Do not spend too much time on ideology here. Move quickly into the system.

### 3. What the system actually does

One diagram or short block:

- EL3: TF-A + SPMD
- S-EL2: this SPMC
- S-EL1: 3 Secure Partitions
- NS-EL2: pKVM
- NS-EL1: Linux / Android

Then 4 bullets:

- boots Linux
- manages Secure Partitions
- supports FF-A v1.1 flows
- coexists with pKVM on the same physical CPUs

### 4. Three technical sections only

Do not include every highlight from the long-form draft. Keep the flagship piece selective.

Recommended three:

1. `SPMD is per-CPU`
- this is non-obvious
- it explains why the system is hard in a way specs do not
- it signals that the work is real

2. `The NS bit and the invisible write`
- same address, different memory universe
- extremely memorable
- easy to understand even for non-ARM specialists

3. `Rust state machines at S-EL2`
- enum-based SP lifecycle
- compile-time transition coverage
- concrete Rust value without sounding ideological

Optional swap:

- if you want more "wow" factor, replace the Rust section with `The silent SIMD trap`

### 5. War stories

Keep this to **two** bugs, not four.

Best pair:

- `The NS bit and the invisible write`
- `The silent SIMD trap`

Why:

- one bug is architecture-specific
- one bug is compiler/codegen-specific
- together they give the piece range

### 6. Numbers

Use a short scoreboard:

- ~30K LOC
- 1 dependency
- `make run` -> `34` test suites / `457` assertions
- `35/35` pKVM E2E tests
- Linux boots to BusyBox

### 7. Try it

End with:

- repo link
- `make run`
- pointer to `ARCHITECTURE.md`

Do not bury the CTA in too many links.

## Source Mapping

Reuse existing sections instead of rewriting from zero:

- opening + architecture: [docs/blog-post.md](../blog-post.md)
- technical details: [docs/devto-post.md](../devto-post.md)
- Chinese summary for concise phrasing: [docs/zhihu/summary-two-hypervisors.md](../zhihu/summary-two-hypervisors.md)

Suggested cut list from the current long draft:

- trim `Technical Highlights` down from three-plus sections to at most three
- reduce `War Stories` from four to two
- remove deep detail that belongs in the full write-up or book

## Canonical Link Strategy

Publish the full English piece on your canonical long-form home first:

- preferred: GitHub Pages / personal site
- secondary mirrors: dev.to, forum posts, social summaries

Reason:

- you want one stable URL for HN and future references
- mirrors can summarize and point back

## Launch Sequence

### Day 0

- final edit the article
- verify repo README top section is aligned with the article
- ensure demo asset and architecture link both work

### Day 1

- publish the article on your canonical site
- submit to HN with the article URL, not the repo URL
- immediately leave a first comment with context and 3-4 concrete lessons

### Day 2

- post a shorter Rust-focused version to `users.rust-lang.org` `announcements`
- post a 3-4 tweet X thread with one bug teaser and one architecture hook

### Day 3

- publish or adapt the Chinese summary for Zhihu
- point back to the same repo and architecture entry points

## Distribution Copy

### HN Title

`Building a Rust ARM64 SPMC that Replaces Hafnium and Runs Beside pKVM`

### HN First Comment

Use this structure:

1. why build this instead of using Hafnium
2. what surprised you most technically
3. one or two bugs that show where the real complexity was
4. quick run instructions

### Rust Forum Title

`Project update: a no_std Rust ARM64 SPMC at Secure EL2`

Body shape:

- one paragraph on what it is
- short bullet list of capabilities
- one paragraph on what Rust changed technically
- canonical link + repo link

### X Thread

Post 4 tweets max:

1. system hook: two hypervisors, one chip
2. hardest bug: NS bit / invisible write
3. Rust-specific lesson: state machine or SIMD trap
4. article + repo link

### Zhihu

Do not translate sentence-for-sentence.

Use the Chinese version to emphasize:

- why replacing Hafnium is interesting
- why two hypervisors on one chip is counterintuitive
- the two most vivid bug stories

## Success Metrics

Track for the first 7 days:

- GitHub stars delta
- clones and unique cloners
- top referrers
- article comments
- HN discussion quality
- Rust Forum replies from people who actually understand the domain

Success is not raw pageviews alone. The best sign is:

- qualified readers clicking into `ARCHITECTURE.md`, `src/`, and the repo root

## Next Asset After This

After the flagship article, the best follow-up is not another broad overview.

Publish one focused technical post:

- `The NS Bit and the Invisible Write`

That topic is memorable, specific, and easy to reference later from HN, Zhihu, and Rust communities.

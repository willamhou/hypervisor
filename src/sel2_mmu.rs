//! S-EL2 Stage-1 MMU — minimal identity map for SPMC
//!
//! Enables the S-EL2 MMU so that writes to Non-Secure DRAM (e.g. NWd RXTX
//! buffers at 0x4xxxxxxx) go through a Stage-1 descriptor with NS=1, reaching
//! the correct Non-Secure physical address space. Without this, S-EL2 (MMU off)
//! accesses Secure physical alias → NWd reads zeros → PARTITION_INFO_GET fails.
//!
//! Layout (AArch64 4KB granule, 48-bit VA):
//! ```
//! L0[0] → L1 (table descriptor)
//!
//! L1[0] → L2_LOW (table, fine-grained for first 1GB)
//! L1[1] = 1GB block, NS=1, Normal WB, XN  (0x4000_0000..0x7FFF_FFFF)
//! L1[2] = 1GB block, NS=1, Normal WB, XN  (0x8000_0000..0xBFFF_FFFF)
//!
//! L2_LOW[64..79]   = 2MB Device nGnRnE, NS=0, XN  (0x0800_0000..0x09FF_FFFF)
//! L2_LOW[112..127] = 2MB Normal WB,  NS=0          (0x0E00_0000..0x0FFF_FFFF)
//! ```

#![cfg(feature = "sel2")]

use core::sync::atomic::{AtomicU64, Ordering};

// ── Page table entry bits (Stage-1 AArch64) ─────────────────────────────
const PTE_VALID: u64 = 1 << 0;
const PTE_TABLE: u64 = 1 << 1; // L0/L1 table descriptor
const PTE_BLOCK: u64 = 0 << 1; // L1/L2 block descriptor (bit[1]=0, bit[0]=1)

// AttrIndx[4:2] — index into MAIR_EL2
const ATTR_DEVICE_NGNRNE: u64 = 0 << 2; // MAIR index 0
const ATTR_NORMAL_WB: u64 = 1 << 2; // MAIR index 1

// NS bit [5] — 0=Secure, 1=Non-Secure
const NS_BIT: u64 = 1 << 5;

// AP[7:6] — 00 = EL2 R/W (privileged)
const AP_RW: u64 = 0b00 << 6;

// SH[9:8] — 11 = Inner Shareable
const SH_ISH: u64 = 0b11 << 8;

// AF[10] — Access Flag (must be 1 to avoid access flag fault)
const AF: u64 = 1 << 10;

// XN[54] — Execute-never (UXN for Stage-1 EL2)
const XN: u64 = 1 << 54;

// ── Common attribute combos ─────────────────────────────────────────────
/// Device-nGnRnE, Secure, R/W, AF, XN.
/// SH field is ignored by hardware for Device memory (treated as OSH).
const DEVICE_S: u64 = ATTR_DEVICE_NGNRNE | AP_RW | SH_ISH | AF | XN;

/// Normal Write-Back, Secure, R/W, ISh, AF.
/// XN intentionally omitted: SPMC code at 0x0E100000 executes from this range.
const NORMAL_S: u64 = ATTR_NORMAL_WB | AP_RW | SH_ISH | AF;

/// Normal Write-Back, Non-Secure, R/W, ISh, AF, XN
const NORMAL_NS_XN: u64 = ATTR_NORMAL_WB | NS_BIT | AP_RW | SH_ISH | AF | XN;

// ── Static page tables (zeroed in .bss) ─────────────────────────────────
// 512 entries × 8 bytes = 4096 bytes = 1 page each.
// Using AtomicU64 for interior mutability without UnsafeCell; writes are
// single-threaded during init, reads are after DSB+ISB.

#[repr(C, align(4096))]
struct PageTable([AtomicU64; 512]);

impl PageTable {
    const fn new() -> Self {
        // AtomicU64::new(0) in const context
        const ZERO: AtomicU64 = AtomicU64::new(0);
        PageTable([ZERO; 512])
    }
}

/// # Safety
/// These are only written during single-threaded `init_sel2_stage1()`,
/// then read by hardware via TTBR0_EL2 after barriers.
static S1_L0: PageTable = PageTable::new();
static S1_L1: PageTable = PageTable::new();
static S1_L2_LOW: PageTable = PageTable::new();

// ── MAIR / TCR constants ────────────────────────────────────────────────

/// MAIR_EL2:
///   Attr0 = 0x00 (Device-nGnRnE)
///   Attr1 = 0xFF (Normal, Inner/Outer WB-RW-Allocate)
const MAIR_VALUE: u64 = 0x0000_0000_0000_FF00;

/// TCR_EL2 (non-VHE, EL2 regime):
///   T0SZ  = 16  [5:0]  → 48-bit VA (2^48)
///   IRGN0 = 01  [9:8]  → Inner WB-RA-WA
///   ORGN0 = 01  [11:10]→ Outer WB-RA-WA
///   SH0   = 11  [13:12]→ Inner Shareable
///   TG0   = 00  [15:14]→ 4KB granule
///   PS    = 101 [18:16]→ 48-bit PA
const TCR_VALUE: u64 = {
    let t0sz: u64 = 16;
    let irgn0: u64 = 0b01 << 8;
    let orgn0: u64 = 0b01 << 10;
    let sh0: u64 = 0b11 << 12;
    let tg0: u64 = 0b00 << 14;
    let ps: u64 = 0b101 << 16;
    t0sz | irgn0 | orgn0 | sh0 | tg0 | ps
};

// ── SCTLR_EL2 bits ─────────────────────────────────────────────────────
const SCTLR_M: u64 = 1 << 0; // MMU enable
const SCTLR_C: u64 = 1 << 2; // Data cache enable
const SCTLR_I: u64 = 1 << 12; // Instruction cache enable

/// Install S-EL2 Stage-1 MMU on a secondary CPU.
///
/// Page tables were already filled by primary CPU's `init_sel2_stage1()`.
/// This just programs MAIR/TCR/TTBR0 and enables SCTLR_EL2.
pub fn install_sel2_stage1_secondary() {
    // SAFETY: Secondary CPU installs already-initialized tables and EL2 MMU registers for itself.
    unsafe {
        // TLB invalidate + barriers
        core::arch::asm!("tlbi alle2", "dsb ish", "isb", options(nostack, nomem),);

        // Configure MMU registers (same values as primary)
        let ttbr0 = &S1_L0 as *const _ as u64;
        core::arch::asm!(
            "msr mair_el2, {mair}",
            "msr tcr_el2, {tcr}",
            "msr ttbr0_el2, {ttbr0}",
            "isb",
            mair = in(reg) MAIR_VALUE,
            tcr = in(reg) TCR_VALUE,
            ttbr0 = in(reg) ttbr0,
            options(nostack, nomem),
        );

        // Enable MMU + caches
        let mut sctlr: u64;
        core::arch::asm!("mrs {}, sctlr_el2", out(reg) sctlr, options(nostack, nomem));
        sctlr |= SCTLR_M | SCTLR_C | SCTLR_I;
        core::arch::asm!(
            "msr sctlr_el2, {sctlr}",
            "isb",
            sctlr = in(reg) sctlr,
            options(nostack, nomem),
        );
    }
}

/// Enable S-EL2 Stage-1 MMU with a minimal identity map.
///
/// Must be called single-threaded, before any access to Non-Secure DRAM
/// (e.g. NWd RXTX buffers). Safe to call after exception vectors are
/// installed and manifest/DTB are parsed (both are in Secure DRAM which
/// works without MMU).
///
/// # Safety
/// Writes to static page tables and system registers. Must only be called
/// once, from the SPMC boot path.
pub fn init_sel2_stage1() {
    // ── 1. Fill page tables ─────────────────────────────────────────

    // L0[0] → L1 table
    let l1_pa = &S1_L1 as *const _ as u64;
    S1_L0.0[0].store(l1_pa | PTE_VALID | PTE_TABLE, Ordering::Relaxed);

    // L1[0] → L2_LOW table (first 1GB, fine-grained)
    let l2_pa = &S1_L2_LOW as *const _ as u64;
    S1_L1.0[0].store(l2_pa | PTE_VALID | PTE_TABLE, Ordering::Relaxed);

    // L1[1] = 1GB block at 0x4000_0000, NS=1, Normal WB, XN
    S1_L1.0[1].store(
        0x4000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
        Ordering::Relaxed,
    );

    // L1[2] = 1GB block at 0x8000_0000, NS=1, Normal WB, XN
    S1_L1.0[2].store(
        0x8000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
        Ordering::Relaxed,
    );

    // L2_LOW: Device blocks for GIC + UART (0x0800_0000..0x09FF_FFFF)
    // L2 index = addr >> 21. 0x0800_0000 >> 21 = 64, 0x09E0_0000 >> 21 = 79
    for idx in 64..=79 {
        let addr = (idx as u64) << 21;
        S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | DEVICE_S, Ordering::Relaxed);
    }

    // L2_LOW: Normal Secure blocks for SPMC + SPs + heap (0x0E00_0000..0x0FFF_FFFF)
    // 0x0E00_0000 >> 21 = 112, 0x0FE0_0000 >> 21 = 127
    for idx in 112..=127 {
        let addr = (idx as u64) << 21;
        S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | NORMAL_S, Ordering::Relaxed);
    }

    // ── 2. Barriers + TLB invalidate ────────────────────────────────
    // SAFETY: One-time bootstrap sequence updates page tables/registers before concurrent execution.
    unsafe {
        // Ensure all table writes are visible before enabling MMU
        core::arch::asm!(
            "dsb  ishst", // ensure stores are complete
            "tlbi alle2", // invalidate all S-EL2 TLB entries
            "dsb  ish",   // wait for TLB invalidation
            "isb",        // synchronize context
            options(nostack, nomem),
        );

        // ── 3. Configure MMU registers ──────────────────────────────
        let ttbr0 = &S1_L0 as *const _ as u64;
        core::arch::asm!(
            "msr mair_el2, {mair}",
            "msr tcr_el2, {tcr}",
            "msr ttbr0_el2, {ttbr0}",
            "isb",
            mair = in(reg) MAIR_VALUE,
            tcr = in(reg) TCR_VALUE,
            ttbr0 = in(reg) ttbr0,
            options(nostack, nomem),
        );

        // ── 4. Enable MMU + caches ──────────────────────────────────
        let mut sctlr: u64;
        core::arch::asm!("mrs {}, sctlr_el2", out(reg) sctlr, options(nostack, nomem));
        sctlr |= SCTLR_M | SCTLR_C | SCTLR_I;
        core::arch::asm!(
            "msr sctlr_el2, {sctlr}",
            "isb",
            sctlr = in(reg) sctlr,
            options(nostack, nomem),
        );
    }
}

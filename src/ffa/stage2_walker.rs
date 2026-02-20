//! Lightweight Stage-2 page table walker reconstructed from VTTBR_EL2.
//!
//! The `DynamicIdentityMapper` is leaked via `core::mem::forget()` in `vm.rs`,
//! so no global reference exists at SMC dispatch time. However, `walk_to_leaf_ptr()`
//! only uses `self.l0_table` (the L0 page table physical address), which survives
//! in `VTTBR_EL2` bits [47:1]. This module reconstructs a minimal walker from
//! that register for page ownership validation during FF-A memory operations.

use crate::arch::aarch64::defs::*;

/// Lightweight Stage-2 page table walker.
///
/// Does NOT own the page tables — they were leaked by `DynamicIdentityMapper`
/// and survive for the VM's lifetime.
pub struct Stage2Walker {
    l0_table: u64,
}

impl Stage2Walker {
    /// Reconstruct from current VTTBR_EL2.
    ///
    /// VTTBR_EL2: bits [47:1] = page table base (L0 PA), bits [63:48] = VMID.
    /// Valid at SMC handling time since we are at EL2 and Stage-2 is active.
    pub fn from_vttbr() -> Self {
        let vttbr: u64;
        unsafe {
            core::arch::asm!("mrs {}, vttbr_el2", out(reg) vttbr, options(nomem, nostack));
        }
        Self {
            l0_table: vttbr & PTE_ADDR_MASK,
        }
    }

    /// Create from an explicit L0 table address (for testing).
    pub fn new(l0_table: u64) -> Self {
        Self { l0_table }
    }

    /// Check if a Stage-2 page table is configured.
    ///
    /// Returns false if L0 table address is 0 (no Stage-2, e.g. unit test mode).
    pub fn has_stage2(&self) -> bool {
        self.l0_table != 0
    }

    /// Read SW bits [56:55] from the leaf PTE for a given IPA.
    pub fn read_sw_bits(&self, ipa: u64) -> Option<u8> {
        let pte = self.walk_to_leaf(ipa)?;
        Some(((pte >> PTE_SW_SHIFT) & 0x3) as u8)
    }

    /// Write SW bits [56:55] on the leaf PTE for a given IPA.
    ///
    /// No TLB invalidation needed — SW bits don't affect hardware translation.
    pub fn write_sw_bits(&self, ipa: u64, bits: u8) -> Result<(), &'static str> {
        let leaf_ptr = self.walk_to_leaf_ptr(ipa).ok_or("IPA not mapped")?;
        unsafe {
            let mut pte = core::ptr::read_volatile(leaf_ptr);
            pte = (pte & !PTE_SW_MASK) | (((bits as u64) & 0x3) << PTE_SW_SHIFT);
            core::ptr::write_volatile(leaf_ptr, pte);
        }
        Ok(())
    }

    /// Read S2AP bits [7:6] from the leaf PTE for a given IPA.
    pub fn read_s2ap(&self, ipa: u64) -> Option<u8> {
        let pte = self.walk_to_leaf(ipa)?;
        Some(((pte >> S2AP_SHIFT) & 0x3) as u8)
    }

    /// Write S2AP bits [7:6] on the leaf PTE + TLB invalidation.
    ///
    /// Unlike SW bits, S2AP affects hardware translation and requires a
    /// TLB invalidation after modification.
    ///
    /// NOTE: If the leaf PTE is a 2MB block, this changes permissions for
    /// all 512 pages within it. Callers should ensure the IPA falls on an
    /// already-split 4KB page or a 2MB-aligned region.
    pub fn set_s2ap(&self, ipa: u64, s2ap: u8) -> Result<(), &'static str> {
        let leaf_ptr = self.walk_to_leaf_ptr(ipa).ok_or("IPA not mapped")?;
        unsafe {
            let mut pte = core::ptr::read_volatile(leaf_ptr);
            pte = (pte & !S2AP_MASK) | (((s2ap as u64) & 0x3) << S2AP_SHIFT);
            core::ptr::write_volatile(leaf_ptr, pte);
        }
        Self::tlbi_ipa(ipa);
        Ok(())
    }

    /// Walk page table to the leaf PTE value.
    fn walk_to_leaf(&self, ipa: u64) -> Option<u64> {
        let ptr = self.walk_to_leaf_ptr(ipa)?;
        Some(unsafe { core::ptr::read_volatile(ptr) })
    }

    /// Walk page table to the leaf PTE pointer for a given IPA.
    ///
    /// Duplicated from `DynamicIdentityMapper::walk_to_leaf_ptr()` (mmu.rs).
    /// The walk logic only uses `self.l0_table`, making this reconstruction safe.
    fn walk_to_leaf_ptr(&self, ipa: u64) -> Option<*mut u64> {
        // L0
        let l0_idx = ((ipa >> 39) & PT_INDEX_MASK) as usize;
        let l0_entry = unsafe { *(self.l0_table as *const u64).add(l0_idx) };
        if l0_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return None;
        }

        // L1
        let l1_table = l0_entry & PTE_ADDR_MASK;
        let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
        let l1_entry = unsafe { *(l1_table as *const u64).add(l1_idx) };
        if l1_entry & PTE_VALID == 0 {
            return None;
        }
        // L1 block (1GB)
        if l1_entry & PTE_TABLE == 0 {
            return Some(unsafe { (l1_table as *mut u64).add(l1_idx) });
        }

        // L2
        let l2_table = l1_entry & PTE_ADDR_MASK;
        let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
        let l2_ptr = unsafe { (l2_table as *mut u64).add(l2_idx) };
        let l2_entry = unsafe { core::ptr::read_volatile(l2_ptr) };
        if l2_entry & PTE_VALID == 0 {
            return None;
        }
        // L2 block (2MB)
        if l2_entry & PTE_TABLE == 0 {
            return Some(l2_ptr);
        }

        // L3 (4KB page)
        let l3_table = l2_entry & PTE_ADDR_MASK;
        let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;
        let l3_ptr = unsafe { (l3_table as *mut u64).add(l3_idx) };
        let l3_entry = unsafe { core::ptr::read_volatile(l3_ptr) };
        if l3_entry & PTE_VALID == 0 {
            return None;
        }
        Some(l3_ptr)
    }

    /// Create a 4KB page mapping in this VM's Stage-2 at the given IPA.
    ///
    /// Identity mapping: IPA == PA. Walks L0->L1->L2->L3, allocating L2/L3
    /// tables from the heap as needed. The L0->L1 link must already exist
    /// (created by `DynamicIdentityMapper::new()`).
    ///
    /// # Arguments
    /// * `ipa` - Guest intermediate physical address (4KB-aligned)
    /// * `s2ap` - Stage-2 access permissions (2 bits): 0b00=None, 0b01=RO, 0b11=RW
    /// * `sw_bits` - Software-defined PTE bits [56:55] for page ownership tracking
    ///
    /// # Errors
    /// Returns an error if:
    /// - L0 entry is invalid (no L1 table)
    /// - L1 entry is a 1GB block (won't split)
    /// - L2 entry is a 2MB block (won't split existing blocks for cross-VM mapping)
    /// - L3 entry is already valid (page already mapped)
    /// - Heap allocation fails
    #[allow(dead_code)]
    pub fn map_page(&self, ipa: u64, s2ap: u8, sw_bits: u8) -> Result<(), &'static str> {
        // L0: must be a valid table descriptor (L0->L1 link from DynamicIdentityMapper)
        let l0_idx = ((ipa >> 39) & PT_INDEX_MASK) as usize;
        let l0_entry =
            unsafe { core::ptr::read_volatile((self.l0_table as *const u64).add(l0_idx)) };
        if l0_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return Err("L0 entry not a valid table");
        }
        let l1_table = l0_entry & PTE_ADDR_MASK;

        // L1: get or create L2 table
        let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
        let l1_ptr = unsafe { (l1_table as *mut u64).add(l1_idx) };
        let l1_entry = unsafe { core::ptr::read_volatile(l1_ptr) };

        let l2_table = if l1_entry & PTE_VALID == 0 {
            // L1 entry invalid: allocate a new L2 table
            let l2 = crate::mm::heap::alloc_page().ok_or("Failed to allocate L2 table")?;
            unsafe {
                core::ptr::write_bytes(l2 as *mut u8, 0, PAGE_SIZE_4KB as usize);
            }
            let l1_desc = l2 | PTE_VALID | PTE_TABLE;
            unsafe {
                core::ptr::write_volatile(l1_ptr, l1_desc);
            }
            l2
        } else if l1_entry & PTE_TABLE != 0 {
            // L1 entry is a valid table descriptor -> L2 table address
            l1_entry & PTE_ADDR_MASK
        } else {
            // L1 entry is a 1GB block -- won't split
            return Err("L1 entry is a 1GB block");
        };

        // L2: get or create L3 table
        let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
        let l2_ptr = unsafe { (l2_table as *mut u64).add(l2_idx) };
        let l2_entry = unsafe { core::ptr::read_volatile(l2_ptr) };

        let l3_table = if l2_entry & PTE_VALID == 0 {
            // L2 entry invalid: allocate a new L3 table
            let l3 = crate::mm::heap::alloc_page().ok_or("Failed to allocate L3 table")?;
            unsafe {
                core::ptr::write_bytes(l3 as *mut u8, 0, PAGE_SIZE_4KB as usize);
            }
            let l2_desc = l3 | PTE_VALID | PTE_TABLE;
            unsafe {
                core::ptr::write_volatile(l2_ptr, l2_desc);
            }
            l3
        } else if l2_entry & PTE_TABLE != 0 {
            // L2 entry is a valid table descriptor -> L3 table address
            l2_entry & PTE_ADDR_MASK
        } else {
            // L2 entry is a 2MB block -- won't split for cross-VM mapping
            return Err("L2 entry is a 2MB block");
        };

        // L3: write page entry (must not already be mapped)
        let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;
        let l3_ptr = unsafe { (l3_table as *mut u64).add(l3_idx) };
        let l3_entry = unsafe { core::ptr::read_volatile(l3_ptr) };
        if l3_entry & PTE_VALID != 0 {
            return Err("L3 entry already mapped");
        }

        // Build the L3 page descriptor:
        //   PA (identity-mapped) | MemAttrIndx=0b1111 | SH=Inner | AF=1 | S2AP | SW | Valid+Page
        // Normal memory base attrs (without S2AP): MemAttrIndx[5:2]=0b1111, SH[9:8]=0b11, AF[10]=1
        let normal_attrs: u64 = (0b1111 << 2) | (0b11 << 8) | (1 << 10);
        let s2ap_bits = ((s2ap as u64) & 0x3) << S2AP_SHIFT;
        let sw = ((sw_bits as u64) & 0x3) << PTE_SW_SHIFT;
        let pa = ipa & !PAGE_MASK_4KB;
        let page_entry = pa | normal_attrs | s2ap_bits | sw | PTE_TABLE | PTE_VALID;
        unsafe {
            core::ptr::write_volatile(l3_ptr, page_entry);
        }

        Self::tlbi_ipa(ipa);
        Ok(())
    }

    /// Remove a 4KB page mapping from this VM's Stage-2.
    ///
    /// Walks L0->L1->L2->L3 and zeroes the leaf L3 PTE. Does not free
    /// intermediate page tables (leaked tables are acceptable).
    ///
    /// # Errors
    /// Returns an error if the IPA is not mapped as a 4KB page (e.g., unmapped,
    /// 2MB block, or 1GB block).
    #[allow(dead_code)]
    pub fn unmap_page(&self, ipa: u64) -> Result<(), &'static str> {
        // Walk to the L3 PTE. We need to ensure we reach an L3 page entry
        // specifically, not a 2MB block or 1GB block.
        let l3_ptr = self
            .walk_to_l3_ptr(ipa)
            .ok_or("IPA not mapped as 4KB page")?;

        // Zero the L3 entry to invalidate the mapping
        unsafe {
            core::ptr::write_volatile(l3_ptr, 0u64);
        }

        Self::tlbi_ipa(ipa);
        Ok(())
    }

    /// Walk page table to the L3 PTE pointer for a given IPA.
    ///
    /// Unlike `walk_to_leaf_ptr()`, this only returns a pointer if the walk
    /// reaches a valid L3 page entry. Returns `None` for 2MB blocks, 1GB blocks,
    /// or unmapped IPAs.
    fn walk_to_l3_ptr(&self, ipa: u64) -> Option<*mut u64> {
        // L0
        let l0_idx = ((ipa >> 39) & PT_INDEX_MASK) as usize;
        let l0_entry =
            unsafe { core::ptr::read_volatile((self.l0_table as *const u64).add(l0_idx)) };
        if l0_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return None;
        }

        // L1: must be a table descriptor (not a 1GB block)
        let l1_table = l0_entry & PTE_ADDR_MASK;
        let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
        let l1_entry = unsafe { core::ptr::read_volatile((l1_table as *const u64).add(l1_idx)) };
        if l1_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return None;
        }

        // L2: must be a table descriptor (not a 2MB block)
        let l2_table = l1_entry & PTE_ADDR_MASK;
        let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
        let l2_entry = unsafe { core::ptr::read_volatile((l2_table as *const u64).add(l2_idx)) };
        if l2_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return None;
        }

        // L3: must be a valid page entry
        let l3_table = l2_entry & PTE_ADDR_MASK;
        let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;
        let l3_ptr = unsafe { (l3_table as *mut u64).add(l3_idx) };
        let l3_entry = unsafe { core::ptr::read_volatile(l3_ptr) };
        if l3_entry & PTE_VALID == 0 {
            return None;
        }

        Some(l3_ptr)
    }

    /// Invalidate a single IPA from Stage-2 TLB.
    fn tlbi_ipa(ipa: u64) {
        let ipa_shifted = (ipa >> 12) & 0x0000_00FF_FFFF_FFFF;
        unsafe {
            core::arch::asm!(
                "dsb ishst",
                "tlbi ipas2e1is, {ipa}",
                "dsb ish",
                "isb",
                ipa = in(reg) ipa_shifted,
                options(nostack),
            );
        }
    }
}

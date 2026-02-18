//! ARM64 Memory Management Unit (MMU) and Stage-2 Translation
//!
//! This module implements Stage-2 page tables for guest physical to host
//! physical address translation.
//!
//! Page Table Levels (for 4KB granule, 48-bit IPA):
//! - Level 0: 512GB regions (entry covers bits [47:39])
//! - Level 1: 1GB blocks (entry covers bits [38:30])
//! - Level 2: 2MB blocks (entry covers bits [29:21])
//! - Level 3: 4KB pages (entry covers bits [20:12])

use crate::arch::aarch64::defs::*;
use crate::arch::traits::{Stage2Mapper, MemoryType};

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;

/// Stage-2 page table entry
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct S2PageTableEntry(u64);

impl S2PageTableEntry {
    /// Create an invalid entry
    pub const fn invalid() -> Self {
        Self(0)
    }

    /// Create a block entry (Level 1 or 2)
    pub const fn block(addr: u64, attrs: MemoryAttributes) -> Self {
        let entry = (addr & PTE_ADDR_MASK) // Address bits [47:12]
            | PTE_VALID                      // Valid bit
            | (0 << 1)                       // Block entry (not table)
            | (attrs.bits() << 2);           // Attributes
        Self(entry)
    }

    /// Create a page entry (Level 3)
    pub const fn page(addr: u64, attrs: MemoryAttributes) -> Self {
        let entry = (addr & PTE_ADDR_MASK)
            | PTE_VALID
            | PTE_TABLE                      // Page entry
            | (attrs.bits() << 2);
        Self(entry)
    }

    /// Create a table entry (points to next level)
    pub const fn table(next_level_addr: u64) -> Self {
        let entry = (next_level_addr & PTE_ADDR_MASK)
            | PTE_VALID
            | PTE_TABLE;
        Self(entry)
    }

    /// Check if entry is valid
    pub fn is_valid(&self) -> bool {
        (self.0 & PTE_VALID) != 0
    }

    /// Check if entry is a table descriptor
    pub fn is_table(&self) -> bool {
        self.is_valid() && ((self.0 & (PTE_VALID | PTE_TABLE)) == (PTE_VALID | PTE_TABLE))
    }

    /// Get physical address from entry
    pub fn addr(&self) -> u64 {
        self.0 & PTE_ADDR_MASK
    }

    /// Get raw value
    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// Memory attributes for Stage-2 translation
#[derive(Clone, Copy, Debug)]
pub struct MemoryAttributes {
    bits: u64,
}

impl MemoryAttributes {
    // Stage-2 Block Descriptor format (after << 2 in block()):
    // bits [5:2]  = MemAttr[3:0]
    // bits [7:6]  = S2AP[1:0] (00=None, 01=RO, 10=WO, 11=RW)
    // bits [9:8]  = SH[1:0] (00=Non-shareable, 11=Inner shareable)
    // bit  [10]   = AF (Access Flag, must be 1)

    /// Normal memory, write-back cacheable, read-write
    pub const NORMAL: Self = Self {
        bits: (0b1111 << 0)  // MemAttr[3:0] = Normal, Write-back
            | (0b11 << 4)     // S2AP[1:0] = Read-Write
            | (0b11 << 6)     // SH[1:0] = Inner shareable
            | (1 << 8),       // AF = 1
    };

    /// Device memory (MMIO), read-write
    pub const DEVICE: Self = Self {
        bits: (0b0000 << 0)  // MemAttr[3:0] = Device-nGnRnE
            | (0b11 << 4)     // S2AP[1:0] = Read-Write
            | (0b00 << 6)     // SH[1:0] = Non-shareable
            | (1 << 8),       // AF = 1
    };

    /// Read-only memory
    pub const READONLY: Self = Self {
        bits: (0b1111 << 0)  // MemAttr[3:0] = Normal
            | (0b01 << 4)     // S2AP[1:0] = Read-Only
            | (0b11 << 6)     // SH[1:0] = Inner shareable
            | (1 << 8),       // AF = 1
    };

    /// Get raw bits
    pub const fn bits(&self) -> u64 {
        self.bits
    }
}

/// Stage-2 Page Table
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [S2PageTableEntry; 512],
}

impl PageTable {
    /// Create a new empty page table
    pub const fn new() -> Self {
        Self {
            entries: [S2PageTableEntry::invalid(); 512],
        }
    }

    /// Get entry at index
    pub fn entry(&self, index: usize) -> S2PageTableEntry {
        self.entries[index]
    }

    /// Set entry at index
    pub fn set_entry(&mut self, index: usize, entry: S2PageTableEntry) {
        self.entries[index] = entry;
    }

    /// Get physical address of this table
    pub fn addr(&self) -> u64 {
        self as *const _ as u64
    }
}

/// Stage-2 Translation configuration
pub struct Stage2Config {
    /// Virtual Translation Table Base Register (VTTBR_EL2)
    pub vttbr: u64,

    /// Virtual Translation Control Register (VTCR_EL2)
    pub vtcr: u64,
}

impl Stage2Config {
    /// Create Stage-2 configuration (VMID 0)
    pub fn new(page_table_addr: u64) -> Self {
        Self::new_with_vmid(page_table_addr, 0)
    }

    /// Create Stage-2 configuration with explicit VMID
    ///
    /// VTTBR_EL2 format: VMID in bits [63:48], page table base in bits [47:1]
    pub fn new_with_vmid(page_table_addr: u64, vmid: u16) -> Self {
        // VTCR_EL2 configuration for 4KB granule, 48-bit IPA
        let vtcr = VTCR_T0SZ_48BIT
            | VTCR_SL0_LEVEL0
            | VTCR_IRGN0_WB
            | VTCR_ORGN0_WB
            | VTCR_SH0_INNER
            | VTCR_TG0_4KB
            | VTCR_PS_48BIT;

        // VTTBR_EL2: VMID[63:48] | page table base[47:1]
        let vttbr = (page_table_addr & 0x0000_FFFF_FFFF_FFFE)
            | ((vmid as u64) << 48);

        Self { vttbr, vtcr }
    }

    /// Install this configuration to hardware registers
    pub fn install(&self) {
        unsafe {
            core::arch::asm!(
                "msr vtcr_el2, {vtcr}",
                "isb",
                vtcr = in(reg) self.vtcr,
                options(nostack, nomem),
            );

            core::arch::asm!(
                "msr vttbr_el2, {vttbr}",
                "isb",
                vttbr = in(reg) self.vttbr,
                options(nostack, nomem),
            );
        }
    }
}

/// Simple identity mapper for guest memory
///
/// Page table hierarchy: L0 -> L1 -> L2 (2MB blocks)
pub struct IdentityMapper {
    l0_table: PageTable,
    l1_table: PageTable,
    l2_tables: [PageTable; 4],
    l2_count: usize,
}

impl IdentityMapper {
    /// Create a new identity mapper
    pub const fn new() -> Self {
        Self {
            l0_table: PageTable::new(),
            l1_table: PageTable::new(),
            l2_tables: [
                PageTable::new(),
                PageTable::new(),
                PageTable::new(),
                PageTable::new(),
            ],
            l2_count: 0,
        }
    }

    /// Reset the mapper to clear all existing mappings
    pub fn reset(&mut self) {
        for i in 0..512 {
            self.l0_table.entries[i] = S2PageTableEntry(0);
        }
        for i in 0..512 {
            self.l1_table.entries[i] = S2PageTableEntry(0);
        }
        for l2 in &mut self.l2_tables {
            for i in 0..512 {
                l2.entries[i] = S2PageTableEntry(0);
            }
        }
        self.l2_count = 0;
    }

    /// Map a memory region with identity mapping
    pub fn map_region(&mut self, start: u64, size: u64, attrs: MemoryAttributes) {
        let num_blocks = (size + BLOCK_SIZE_2MB - 1) / BLOCK_SIZE_2MB;

        for i in 0..num_blocks {
            let addr = start + i * BLOCK_SIZE_2MB;
            self.map_2mb_block(addr, attrs);
        }
    }

    /// Map a single 2MB block
    fn map_2mb_block(&mut self, addr: u64, attrs: MemoryAttributes) {
        let l0_index = ((addr >> 39) & PT_INDEX_MASK) as usize;
        let l1_index = ((addr >> 30) & PT_INDEX_MASK) as usize;
        let l2_index = ((addr >> 21) & PT_INDEX_MASK) as usize;

        // Ensure L0 entry points to L1 table
        if !self.l0_table.entry(l0_index).is_valid() {
            let l1_addr = self.l1_table.addr();
            self.l0_table.set_entry(l0_index, S2PageTableEntry::table(l1_addr));
        }

        // Check if L1 entry exists, allocate L2 table if needed
        let l2_table_idx = if !self.l1_table.entry(l1_index).is_valid() {
            if self.l2_count >= self.l2_tables.len() {
                return;
            }

            let idx = self.l2_count;
            let l2_addr = self.l2_tables[idx].addr();
            self.l1_table.set_entry(l1_index, S2PageTableEntry::table(l2_addr));
            self.l2_count += 1;
            idx
        } else {
            let l1_entry = self.l1_table.entry(l1_index);
            let l2_addr = l1_entry.addr();

            let mut found_idx = None;
            for i in 0..self.l2_count {
                if self.l2_tables[i].addr() == l2_addr {
                    found_idx = Some(i);
                    break;
                }
            }

            match found_idx {
                Some(idx) => idx,
                None => return,
            }
        };

        // Set L2 entry (2MB block)
        self.l2_tables[l2_table_idx].set_entry(l2_index, S2PageTableEntry::block(addr, attrs));
    }

    /// Get the configuration for this mapper
    pub fn config(&self) -> Stage2Config {
        Stage2Config::new(self.l0_table.addr())
    }

    /// Get L0 table address (initial lookup level)
    pub fn l0_addr(&self) -> u64 {
        self.l0_table.addr()
    }
}

/// Memory attribute enum for DynamicIdentityMapper
#[derive(Clone, Copy, Debug)]
pub enum MemoryAttribute {
    /// Normal memory, write-back cacheable
    Normal,
    /// Device memory (MMIO)
    Device,
    /// Read-only memory
    ReadOnly,
}

/// Dynamic identity mapper using heap allocation for page tables
pub struct DynamicIdentityMapper {
    l0_table: u64,
    l1_table: u64,
    l2_tables: [u64; 4],
    l2_count: usize,
}

impl DynamicIdentityMapper {
    /// Create a new dynamic identity mapper
    pub fn new() -> Self {
        let l0 = crate::mm::heap::alloc_page()
            .expect("Failed to allocate L0 table");
        let l1 = crate::mm::heap::alloc_page()
            .expect("Failed to allocate L1 table");

        unsafe {
            core::ptr::write_bytes(l0 as *mut u8, 0, PAGE_SIZE);
            core::ptr::write_bytes(l1 as *mut u8, 0, PAGE_SIZE);
            // Link L0[0] -> L1
            let l0_ptr = l0 as *mut u64;
            *l0_ptr = l1 | (PTE_VALID | PTE_TABLE); // Valid + Table descriptor
        }

        Self {
            l0_table: l0,
            l1_table: l1,
            l2_tables: [0; 4],
            l2_count: 0,
        }
    }

    /// Map a memory region with identity mapping
    pub fn map_region(&mut self, ipa: u64, size: u64, attr: MemoryAttribute) -> Result<(), &'static str> {
        let mut offset = 0;

        while offset < size {
            let current_ipa = ipa + offset;
            let l1_idx = ((current_ipa >> 30) & PT_INDEX_MASK) as usize;
            let l2_table = self.get_or_create_l2(l1_idx)?;
            let l2_idx = ((current_ipa >> 21) & PT_INDEX_MASK) as usize;
            let entry = self.make_block_entry(current_ipa, attr);

            unsafe {
                let l2_ptr = l2_table as *mut u64;
                *l2_ptr.add(l2_idx) = entry;
            }

            offset += BLOCK_SIZE_2MB;
        }
        Ok(())
    }

    /// Get or create L2 table for given L1 index
    fn get_or_create_l2(&mut self, l1_idx: usize) -> Result<u64, &'static str> {
        let l1_entry = unsafe {
            let l1_ptr = self.l1_table as *const u64;
            *l1_ptr.add(l1_idx)
        };

        // Check if valid table entry already exists
        if l1_entry & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
            return Ok(l1_entry & !PAGE_OFFSET_MASK);
        }

        // Need to allocate new L2 table
        if self.l2_count >= 4 {
            return Err("Too many L2 tables");
        }

        let l2 = crate::mm::heap::alloc_page()
            .ok_or("Failed to allocate L2 table")?;

        unsafe {
            core::ptr::write_bytes(l2 as *mut u8, 0, PAGE_SIZE);
        }

        self.l2_tables[self.l2_count] = l2;
        self.l2_count += 1;

        // Create table descriptor and write to L1
        let l1_entry = l2 | (PTE_VALID | PTE_TABLE);
        unsafe {
            let l1_ptr = self.l1_table as *mut u64;
            *l1_ptr.add(l1_idx) = l1_entry;
        }

        Ok(l2)
    }

    /// Create a 2MB block entry
    fn make_block_entry(&self, pa: u64, attr: MemoryAttribute) -> u64 {
        let attr_bits = match attr {
            MemoryAttribute::Normal => {
                (0b1111 << 2) | (0b11 << 6) | (0b11 << 8) | (1 << 10)
            }
            MemoryAttribute::Device => {
                (0b0000 << 2) | (0b11 << 6) | (0b00 << 8) | (1 << 10)
            }
            MemoryAttribute::ReadOnly => {
                (0b1111 << 2) | (0b01 << 6) | (0b11 << 8) | (1 << 10)
            }
        };
        (pa & !BLOCK_MASK_2MB) | attr_bits | PTE_VALID
    }

    /// Map a single 4KB page (identity mapping: IPA == PA).
    ///
    /// If the target L2 entry is a 2MB block, it is first split into 512 x 4KB
    /// page entries preserving the original mapping attributes.
    pub fn map_4kb_page(&mut self, ipa: u64, attr: MemoryAttribute) -> Result<(), &'static str> {
        let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
        let l2_table = self.get_or_create_l2(l1_idx)?;
        let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
        let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;

        let l2_entry = unsafe { *(l2_table as *const u64).add(l2_idx) };

        let l3_table = if l2_entry & PTE_VALID != 0 && l2_entry & PTE_TABLE == 0 {
            // L2 entry is a 2MB block — split into L3 table
            self.split_2mb_block(l2_table, l2_idx, l2_entry)?
        } else if l2_entry & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
            // L2 entry already points to an L3 table
            l2_entry & PTE_ADDR_MASK
        } else {
            // L2 entry invalid — create fresh L3 table (all invalid entries)
            let l3 = crate::mm::heap::alloc_page()
                .ok_or("Failed to allocate L3 table")?;
            unsafe { core::ptr::write_bytes(l3 as *mut u8, 0, PAGE_SIZE); }
            let l3_desc = l3 | PTE_VALID | PTE_TABLE;
            unsafe { *(l2_table as *mut u64).add(l2_idx) = l3_desc; }
            l3
        };

        // Write the 4KB page entry (L3 page descriptor: bit[1]=1 means page at L3)
        let page_entry = self.make_page_entry(ipa & !PAGE_MASK_4KB, attr);
        unsafe { *(l3_table as *mut u64).add(l3_idx) = page_entry; }

        // TLB invalidate for this IPA
        Self::tlbi_ipa(ipa);

        Ok(())
    }

    /// Remove a 4KB page mapping (mark L3 entry invalid).
    /// If the L2 entry is a 2MB block, it is first split into an L3 table.
    pub fn unmap_4kb_page(&mut self, ipa: u64) -> Result<(), &'static str> {
        let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
        let l1_entry = unsafe { *(self.l1_table as *const u64).add(l1_idx) };
        if l1_entry & (PTE_VALID | PTE_TABLE) != (PTE_VALID | PTE_TABLE) {
            return Err("L1 entry not valid");
        }
        let l2_table = l1_entry & PTE_ADDR_MASK;
        let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
        let l2_entry = unsafe { *(l2_table as *const u64).add(l2_idx) };

        let l3_table = if l2_entry & PTE_VALID != 0 && l2_entry & PTE_TABLE == 0 {
            // L2 entry is a 2MB block — split into L3 first
            self.split_2mb_block(l2_table, l2_idx, l2_entry)?
        } else if l2_entry & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
            // L2 entry already points to an L3 table
            l2_entry & PTE_ADDR_MASK
        } else {
            return Err("L2 entry not valid");
        };

        let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;
        unsafe { *(l3_table as *mut u64).add(l3_idx) = 0; }
        Self::tlbi_ipa(ipa);
        Ok(())
    }

    /// Split a 2MB block entry into 512 x 4KB page entries.
    ///
    /// Uses break-before-make: invalidate L2 entry → TLB flush → write new table.
    fn split_2mb_block(&self, l2_table: u64, l2_idx: usize, block_entry: u64) -> Result<u64, &'static str> {
        let block_pa = block_entry & !BLOCK_MASK_2MB;
        let block_attr_bits = block_entry & BLOCK_MASK_2MB & !0x3; // strip valid+type bits

        // Allocate L3 table
        let l3 = crate::mm::heap::alloc_page()
            .ok_or("Failed to allocate L3 table for split")?;

        // Fill L3 with 512 page entries preserving original attributes.
        // L3 page descriptor: [PA | attrs | bit1=1(page) | bit0=1(valid)]
        unsafe {
            let l3_ptr = l3 as *mut u64;
            for i in 0..512u64 {
                let pa = block_pa + i * PAGE_SIZE_4KB;
                // Page descriptor: same attr bits as block, but bit[1] = 1 (page)
                let page = pa | block_attr_bits | PTE_TABLE | PTE_VALID;
                *l3_ptr.add(i as usize) = page;
            }
        }

        // Break-before-make: invalidate old L2 entry
        unsafe { *(l2_table as *mut u64).add(l2_idx) = 0; }
        Self::tlbi_all();

        // Write new L2 table descriptor pointing to L3
        let l2_desc = l3 | PTE_VALID | PTE_TABLE;
        unsafe { *(l2_table as *mut u64).add(l2_idx) = l2_desc; }
        Self::tlbi_all();

        Ok(l3)
    }

    /// Create a 4KB page entry (L3 level).
    fn make_page_entry(&self, pa: u64, attr: MemoryAttribute) -> u64 {
        let attr_bits = match attr {
            MemoryAttribute::Normal => {
                (0b1111 << 2) | (0b11 << 6) | (0b11 << 8) | (1 << 10)
            }
            MemoryAttribute::Device => {
                (0b0000 << 2) | (0b11 << 6) | (0b00 << 8) | (1 << 10)
            }
            MemoryAttribute::ReadOnly => {
                (0b1111 << 2) | (0b01 << 6) | (0b11 << 8) | (1 << 10)
            }
        };
        // L3 page: bit[1] = 1 (page), bit[0] = 1 (valid)
        (pa & !PAGE_MASK_4KB) | attr_bits | PTE_TABLE | PTE_VALID
    }

    /// Invalidate all Stage-2 TLB entries.
    fn tlbi_all() {
        unsafe {
            core::arch::asm!(
                "dsb ishst",
                "tlbi vmalls12e1is",
                "dsb ish",
                "isb",
                options(nostack),
            );
        }
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

    /// Get VTTBR value (L0 table address)
    pub fn vttbr(&self) -> u64 {
        self.l0_table
    }

    /// Get the configuration for this mapper
    pub fn config(&self) -> Stage2Config {
        Stage2Config::new(self.l0_table)
    }

    // ── Page Ownership (SW bits) ─────────────────────────────────────

    /// Read the SW bits [56:55] from the leaf PTE for a given IPA.
    ///
    /// Walks the page table to find the leaf entry (L2 block or L3 page)
    /// and returns the 2-bit SW field, or None if the IPA is not mapped.
    pub fn read_sw_bits(&self, ipa: u64) -> Option<u8> {
        let pte = self.walk_to_leaf(ipa)?;
        Some(((pte >> PTE_SW_SHIFT) & 0x3) as u8)
    }

    /// Write the SW bits [56:55] on the leaf PTE for a given IPA.
    ///
    /// Walks the page table to find the leaf entry and updates the SW field.
    /// Returns Err if the IPA is not mapped.
    pub fn write_sw_bits(&mut self, ipa: u64, bits: u8) -> Result<(), &'static str> {
        let leaf_ptr = self.walk_to_leaf_ptr(ipa).ok_or("IPA not mapped")?;
        unsafe {
            let mut pte = core::ptr::read_volatile(leaf_ptr);
            pte = (pte & !PTE_SW_MASK) | (((bits as u64) & 0x3) << PTE_SW_SHIFT);
            core::ptr::write_volatile(leaf_ptr, pte);
        }
        // No TLB invalidation needed — SW bits don't affect hardware translation
        Ok(())
    }

    /// Walk page table to the leaf PTE value for a given IPA.
    fn walk_to_leaf(&self, ipa: u64) -> Option<u64> {
        let ptr = self.walk_to_leaf_ptr(ipa)?;
        Some(unsafe { core::ptr::read_volatile(ptr) })
    }

    /// Walk page table to the leaf PTE pointer for a given IPA.
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
}

impl Default for DynamicIdentityMapper {
    fn default() -> Self {
        Self::new()
    }
}

// ── Stage2Mapper trait implementation ─────────────────────────────────

impl Stage2Mapper for DynamicIdentityMapper {
    fn map_region(&mut self, ipa: u64, size: u64, mem_type: MemoryType) -> Result<(), &'static str> {
        let attr = match mem_type {
            MemoryType::Normal => MemoryAttribute::Normal,
            MemoryType::Device => MemoryAttribute::Device,
            MemoryType::ReadOnly => MemoryAttribute::ReadOnly,
        };
        self.map_region(ipa, size, attr)
    }

    fn reset(&mut self) {
        // DynamicIdentityMapper doesn't support reset (heap-allocated).
        // Callers should create a new instance instead.
    }

    fn install(&self) {
        self.config().install();
    }

    fn root_table_addr(&self) -> u64 {
        self.l0_table
    }
}

/// Initialize Stage-2 translation from a Stage2Config (used by DynamicIdentityMapper).
pub fn init_stage2_from_config(config: &Stage2Config) {
    // Enable Stage-2 translation in HCR_EL2
    unsafe {
        let mut hcr: u64;
        core::arch::asm!(
            "mrs {hcr}, hcr_el2",
            hcr = out(reg) hcr,
            options(nostack, nomem),
        );
        hcr |= HCR_VM;
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr,
            options(nostack, nomem),
        );
    }
    config.install();
    unsafe {
        core::arch::asm!(
            "tlbi vmalls12e1is",
            "dsb sy",
            "isb",
            options(nostack, nomem),
        );
    }
}

/// Initialize Stage-2 translation for a VM
pub fn init_stage2(mapper: &IdentityMapper) {
    let config = mapper.config();
    init_stage2_from_config(&config);
}

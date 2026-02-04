//! ARM64 Memory Management Unit (MMU) and Stage-2 Translation
//! 
//! This module implements Stage-2 page tables for guest physical to host
//! physical address translation.
//! 
//! ARM64 Stage-2 Translation:
//! - IPA (Intermediate Physical Address) = Guest Physical Address
//! - PA (Physical Address) = Host Physical Address
//! - Translation: IPA -> PA via Stage-2 page tables
//! 
//! Page Table Levels (for 4KB granule, 40-bit IPA):
//! - Level 1: 1GB blocks
//! - Level 2: 2MB blocks
//! - Level 3: 4KB pages

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
    /// 
    /// # Arguments
    /// * `addr` - Physical address (must be aligned)
    /// * `attrs` - Memory attributes
    pub const fn block(addr: u64, attrs: MemoryAttributes) -> Self {
        let entry = (addr & 0x0000_FFFF_FFFF_F000) // Address bits [47:12]
            | (1 << 0)  // Valid bit
            | (0 << 1)  // Block entry (not table)
            | (attrs.bits() << 2); // Attributes
        Self(entry)
    }
    
    /// Create a page entry (Level 3)
    pub const fn page(addr: u64, attrs: MemoryAttributes) -> Self {
        let entry = (addr & 0x0000_FFFF_FFFF_F000) // Address bits [47:12]
            | (1 << 0)  // Valid bit
            | (1 << 1)  // Page entry
            | (attrs.bits() << 2); // Attributes
        Self(entry)
    }
    
    /// Create a table entry (points to next level)
    pub const fn table(next_level_addr: u64) -> Self {
        let entry = (next_level_addr & 0x0000_FFFF_FFFF_F000) // Next level address
            | (1 << 0)  // Valid bit
            | (1 << 1); // Table descriptor
        Self(entry)
    }
    
    /// Check if entry is valid
    pub fn is_valid(&self) -> bool {
        (self.0 & 1) != 0
    }
    
    /// Check if entry is a table descriptor
    pub fn is_table(&self) -> bool {
        self.is_valid() && ((self.0 & 0b11) == 0b11)
    }
    
    /// Get physical address from entry
    pub fn addr(&self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_F000
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
    /// Normal memory, write-back cacheable
    pub const NORMAL: Self = Self {
        bits: (0b1111 << 0)  // MemAttr[3:0] = Normal, Write-back
            | (0b11 << 6)     // SH[1:0] = Inner shareable
            | (0b11 << 8),    // AF=1, Accessed flag
    };
    
    /// Device memory (MMIO)
    pub const DEVICE: Self = Self {
        bits: (0b0000 << 0)  // MemAttr[3:0] = Device-nGnRnE
            | (0b00 << 6)     // SH[1:0] = Non-shareable
            | (0b11 << 8),    // AF=1, Accessed flag
    };
    
    /// Read-only memory
    pub const READONLY: Self = Self {
        bits: (0b1111 << 0)  // MemAttr[3:0] = Normal
            | (0b11 << 6)     // SH[1:0] = Inner shareable
            | (0b11 << 8)     // AF=1
            | (1 << 7),       // S2AP[1] = Read-only
    };
    
    /// Get raw bits
    pub const fn bits(&self) -> u64 {
        self.bits
    }
}

/// Stage-2 Page Table
/// 
/// This represents a single level of the page table hierarchy.
/// For simplicity, we use a fixed-size array.
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
    /// Create Stage-2 configuration
    /// 
    /// # Arguments
    /// * `page_table_addr` - Physical address of level 1 page table
    pub fn new(page_table_addr: u64) -> Self {
        // VTCR_EL2 configuration:
        // - T0SZ = 24 (40-bit IPA space)
        // - SL0 = 1 (Start at level 1)
        // - IRGN0 = 0b01 (Inner write-back cacheable)
        // - ORGN0 = 0b01 (Outer write-back cacheable)
        // - SH0 = 0b11 (Inner shareable)
        // - TG0 = 0b00 (4KB granule)
        let vtcr = (24 << 0)      // T0SZ[5:0]
            | (1 << 6)            // SL0[1:0] = 1
            | (0b01 << 8)         // IRGN0[1:0]
            | (0b01 << 10)        // ORGN0[1:0]
            | (0b11 << 12)        // SH0[1:0]
            | (0b00 << 14)        // TG0[1:0]
            | (0b101 << 16);      // PS[2:0] = 0b101 (48-bit PA space)
        
        // VTTBR_EL2: Page table base address
        // Bits [47:1] contain the page table address (must be aligned)
        let vttbr = page_table_addr & 0x0000_FFFF_FFFF_FFFE;
        
        Self { vttbr, vtcr }
    }
    
    /// Install this configuration to hardware registers
    pub fn install(&self) {
        unsafe {
            // Set VTCR_EL2
            core::arch::asm!(
                "msr vtcr_el2, {vtcr}",
                "isb",
                vtcr = in(reg) self.vtcr,
                options(nostack, nomem),
            );
            
            // Set VTTBR_EL2
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
/// This creates a 1:1 mapping where Guest PA == Host PA
/// for a specified memory region.
pub struct IdentityMapper {
    /// Level 1 page table (covers 512GB)
    l1_table: PageTable,
    
    /// Level 2 page tables (we'll allocate as needed)
    /// For simplicity, we pre-allocate a few
    l2_tables: [PageTable; 4],
    
    /// Number of L2 tables allocated
    l2_count: usize,
}

impl IdentityMapper {
    /// Create a new identity mapper
    pub const fn new() -> Self {
        Self {
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
    
    /// Map a memory region with identity mapping
    /// 
    /// # Arguments
    /// * `start` - Start address (must be aligned to 2MB)
    /// * `size` - Size in bytes (must be multiple of 2MB)
    /// * `attrs` - Memory attributes
    pub fn map_region(&mut self, start: u64, size: u64, attrs: MemoryAttributes) {
        // For simplicity, we use 2MB blocks (Level 2)
        let block_size = 2 * 1024 * 1024; // 2MB
        let num_blocks = (size + block_size - 1) / block_size;
        
        for i in 0..num_blocks {
            let addr = start + i * block_size;
            self.map_2mb_block(addr, attrs);
        }
    }
    
    /// Map a single 2MB block
    fn map_2mb_block(&mut self, addr: u64, attrs: MemoryAttributes) {
        // Calculate indices
        let l1_index = ((addr >> 30) & 0x1FF) as usize; // Bits [38:30]
        let l2_index = ((addr >> 21) & 0x1FF) as usize; // Bits [29:21]
        
        // Check if L1 entry exists
        if !self.l1_table.entry(l1_index).is_valid() {
            // Allocate L2 table
            if self.l2_count >= self.l2_tables.len() {
                // Out of L2 tables (would need dynamic allocation)
                return;
            }
            
            let l2_addr = self.l2_tables[self.l2_count].addr();
            self.l1_table.set_entry(l1_index, S2PageTableEntry::table(l2_addr));
            self.l2_count += 1;
        }
        
        // Get L2 table index
        let l2_table_idx = if l1_index == 0 { 0 } else { l1_index - 1 };
        if l2_table_idx >= self.l2_count {
            return;
        }
        
        // Set L2 entry (2MB block)
        self.l2_tables[l2_table_idx].set_entry(l2_index, S2PageTableEntry::block(addr, attrs));
    }
    
    /// Get the configuration for this mapper
    pub fn config(&self) -> Stage2Config {
        Stage2Config::new(self.l1_table.addr())
    }
    
    /// Get L1 table address
    pub fn l1_addr(&self) -> u64 {
        self.l1_table.addr()
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
///
/// This creates a 1:1 mapping where Guest PA == Host PA
/// for a specified memory region, but uses dynamic allocation
/// instead of static arrays.
pub struct DynamicIdentityMapper {
    /// Level 1 page table address (dynamically allocated)
    l1_table: u64,
    /// Level 2 page table addresses
    l2_tables: [u64; 4],
    /// Number of L2 tables allocated
    l2_count: usize,
}

impl DynamicIdentityMapper {
    /// Create a new dynamic identity mapper
    pub fn new() -> Self {
        let l1 = crate::mm::heap::alloc_page()
            .expect("Failed to allocate L1 table");

        // Zero-initialize the page table
        unsafe {
            core::ptr::write_bytes(l1 as *mut u8, 0, 4096);
        }

        Self {
            l1_table: l1,
            l2_tables: [0; 4],
            l2_count: 0,
        }
    }

    /// Map a memory region with identity mapping
    ///
    /// # Arguments
    /// * `ipa` - Start address (Intermediate Physical Address)
    /// * `size` - Size in bytes
    /// * `attr` - Memory attributes
    pub fn map_region(&mut self, ipa: u64, size: u64, attr: MemoryAttribute) -> Result<(), &'static str> {
        let block_size = 0x20_0000u64; // 2MB
        let mut offset = 0;

        while offset < size {
            let current_ipa = ipa + offset;
            let l1_idx = ((current_ipa >> 30) & 0x1FF) as usize;
            let l2_table = self.get_or_create_l2(l1_idx)?;
            let l2_idx = ((current_ipa >> 21) & 0x1FF) as usize;
            let entry = self.make_block_entry(current_ipa, attr);

            unsafe {
                let l2_ptr = l2_table as *mut u64;
                *l2_ptr.add(l2_idx) = entry;
            }

            offset += block_size;
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
        if l1_entry & 0x3 == 0x3 {
            return Ok(l1_entry & !0xFFF);
        }

        // Need to allocate new L2 table
        if self.l2_count >= 4 {
            return Err("Too many L2 tables");
        }

        let l2 = crate::mm::heap::alloc_page()
            .ok_or("Failed to allocate L2 table")?;

        // Zero-initialize
        unsafe {
            core::ptr::write_bytes(l2 as *mut u8, 0, 4096);
        }

        self.l2_tables[self.l2_count] = l2;
        self.l2_count += 1;

        // Create table descriptor and write to L1
        let l1_entry = l2 | 0x3; // Valid + Table
        unsafe {
            let l1_ptr = self.l1_table as *mut u64;
            *l1_ptr.add(l1_idx) = l1_entry;
        }

        Ok(l2)
    }

    /// Create a 2MB block entry
    fn make_block_entry(&self, pa: u64, attr: MemoryAttribute) -> u64 {
        // S2 descriptor for 2MB block:
        // [1:0] = 01 (Block)
        // [5:2] = MemAttr
        // [7:6] = S2AP (Access permissions)
        // [9:8] = SH (Shareability)
        // [10] = AF (Access flag)
        let attr_bits = match attr {
            MemoryAttribute::Normal => {
                // MemAttr=0b1111 (Normal WB), S2AP=0b11 (RW), SH=0b11 (Inner shareable), AF=1
                (0b1111 << 2) | (0b11 << 6) | (0b11 << 8) | (1 << 10)
            }
            MemoryAttribute::Device => {
                // MemAttr=0b0000 (Device-nGnRnE), S2AP=0b11 (RW), SH=0b00, AF=1
                (0b0000 << 2) | (0b11 << 6) | (0b00 << 8) | (1 << 10)
            }
            MemoryAttribute::ReadOnly => {
                // MemAttr=0b1111 (Normal), S2AP=0b01 (RO), SH=0b11, AF=1
                (0b1111 << 2) | (0b01 << 6) | (0b11 << 8) | (1 << 10)
            }
        };
        (pa & !0x1F_FFFF) | attr_bits | 0x1 // Block descriptor
    }

    /// Get VTTBR value (L1 table address)
    pub fn vttbr(&self) -> u64 {
        self.l1_table
    }

    /// Get the configuration for this mapper
    pub fn config(&self) -> Stage2Config {
        Stage2Config::new(self.l1_table)
    }
}

impl Default for DynamicIdentityMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize Stage-2 translation for a VM
pub fn init_stage2(mapper: &IdentityMapper) {
    let config = mapper.config();
    
    // Enable Stage-2 translation in HCR_EL2
    unsafe {
        let mut hcr: u64;
        core::arch::asm!(
            "mrs {hcr}, hcr_el2",
            hcr = out(reg) hcr,
            options(nostack, nomem),
        );
        
        // Set VM bit to enable Stage-2 translation
        hcr |= 1 << 0; // VM bit
        
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr,
            options(nostack, nomem),
        );
    }
    
    // Install page tables
    config.install();
}

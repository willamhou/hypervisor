//! FF-A v1.1 Composite Memory Region Descriptor parsing (DEN0077A).
//!
//! Defines `#[repr(C, packed)]` structs matching the FF-A v1.1 specification
//! and a parsing function to extract memory region address ranges from the
//! TX buffer during MEM_SHARE / MEM_LEND operations.

/// Maximum number of address ranges per parsed descriptor.
pub const MAX_ADDR_RANGES: usize = 16;

/// FF-A v1.1 Memory Region Descriptor (DEN0077A Table 5.19).
///
/// Top-level structure placed in the TX buffer for MEM_SHARE/MEM_LEND.
/// Size: 48 bytes.
#[repr(C, packed)]
pub struct FfaMemRegion {
    /// Sender endpoint ID
    pub sender_id: u16,
    /// Memory region attributes (Table 5.22)
    pub attributes: u16,
    /// Reserved (MBZ)
    pub reserved_0: u32,
    /// Flags (Table 5.20)
    pub flags: u32,
    /// Handle (0 on initial share, assigned by SPMC on return)
    pub handle: u64,
    /// Tag (application-defined)
    pub tag: u64,
    /// Reserved (MBZ)
    pub reserved_1: u32,
    /// Number of memory access permission descriptors
    pub receiver_count: u32,
    /// Offset from start of this struct to first FfaMemAccessDesc
    pub receivers_offset: u32,
    /// Reserved (MBZ)
    pub reserved_2: u32,
}

/// FF-A v1.1 Memory Access Permission Descriptor (DEN0077A Table 5.21).
///
/// One per receiver. Follows FfaMemRegion at the specified offset.
/// Size: 16 bytes.
#[repr(C, packed)]
pub struct FfaMemAccessDesc {
    /// Receiver endpoint ID
    pub receiver_id: u16,
    /// Memory access permissions (Table 5.23)
    pub permissions: u8,
    /// Flags
    pub flags: u8,
    /// Offset from start of FfaMemRegion to the composite descriptor
    pub composite_offset: u32,
    /// Reserved
    pub reserved: u64,
}

/// FF-A v1.1 Composite Memory Region Descriptor (DEN0077A Table 5.24).
///
/// Contains the total page count and is followed by address range descriptors.
/// Size: 16 bytes (header only).
#[repr(C, packed)]
pub struct FfaCompositeMemRegion {
    /// Total page count across all address ranges
    pub total_page_count: u32,
    /// Number of address range descriptors that follow
    pub address_range_count: u32,
    /// Reserved
    pub reserved: u64,
}

/// FF-A v1.1 Memory Region Address Range Descriptor (DEN0077A Table 5.25).
///
/// One per contiguous memory range within the composite region.
/// Size: 16 bytes.
#[repr(C, packed)]
pub struct FfaMemRegionAddrRange {
    /// Base IPA of this range (must be page-aligned)
    pub address: u64,
    /// Number of 4KB pages in this range
    pub page_count: u32,
    /// Reserved
    pub reserved: u32,
}

/// Parsed result of a composite memory region descriptor.
pub struct ParsedMemRegion {
    pub sender_id: u16,
    pub receiver_id: u16,
    pub flags: u32,
    pub ranges: [(u64, u32); MAX_ADDR_RANGES],
    pub range_count: usize,
    pub total_page_count: u32,
}

impl ParsedMemRegion {
    const fn new() -> Self {
        Self {
            sender_id: 0,
            receiver_id: 0,
            flags: 0,
            ranges: [(0, 0); MAX_ADDR_RANGES],
            range_count: 0,
            total_page_count: 0,
        }
    }
}

/// Parse the TX buffer contents as an FF-A v1.1 composite memory region descriptor.
///
/// Validates structure sizes, bounds, and extracts address ranges.
/// Does NOT support fragmented descriptors (requires total_length == fragment_length).
///
/// # Safety
///
/// `tx_ptr` must point to a valid, identity-mapped TX buffer of at least
/// `total_length` bytes.
pub unsafe fn parse_mem_region(tx_ptr: *const u8, total_length: u32) -> Result<ParsedMemRegion, i32> {
    let total = total_length as usize;

    // Validate minimum size for the top-level region header
    let region_size = core::mem::size_of::<FfaMemRegion>();
    if total < region_size {
        return Err(crate::ffa::FFA_INVALID_PARAMETERS);
    }

    // Read FfaMemRegion header (use read_unaligned for packed struct safety)
    let sender_id = core::ptr::read_unaligned(tx_ptr as *const u16);
    let attributes = core::ptr::read_unaligned(tx_ptr.add(2) as *const u16);
    let _ = attributes; // reserved for future use
    let flags = core::ptr::read_unaligned(tx_ptr.add(8) as *const u32);
    let receiver_count = core::ptr::read_unaligned(tx_ptr.add(32) as *const u32);
    let receivers_offset = core::ptr::read_unaligned(tx_ptr.add(36) as *const u32);

    // Only support single-receiver share for now
    if receiver_count == 0 || receiver_count > 1 {
        return Err(crate::ffa::FFA_INVALID_PARAMETERS);
    }

    // Validate receiver descriptor bounds
    let access_offset = receivers_offset as usize;
    let access_end = access_offset + core::mem::size_of::<FfaMemAccessDesc>();
    if access_end > total {
        return Err(crate::ffa::FFA_INVALID_PARAMETERS);
    }

    // Read FfaMemAccessDesc
    let access_ptr = tx_ptr.add(access_offset);
    let receiver_id = core::ptr::read_unaligned(access_ptr as *const u16);
    let composite_offset = core::ptr::read_unaligned(access_ptr.add(4) as *const u32);

    // Validate composite descriptor bounds
    let comp_offset = composite_offset as usize;
    let comp_end = comp_offset + core::mem::size_of::<FfaCompositeMemRegion>();
    if comp_end > total {
        return Err(crate::ffa::FFA_INVALID_PARAMETERS);
    }

    // Read FfaCompositeMemRegion
    let comp_ptr = tx_ptr.add(comp_offset);
    let total_page_count = core::ptr::read_unaligned(comp_ptr as *const u32);
    let address_range_count = core::ptr::read_unaligned(comp_ptr.add(4) as *const u32);

    if address_range_count == 0 {
        return Err(crate::ffa::FFA_INVALID_PARAMETERS);
    }

    // Read address ranges
    let ranges_offset = comp_end;
    let range_size = core::mem::size_of::<FfaMemRegionAddrRange>();
    let count = (address_range_count as usize).min(MAX_ADDR_RANGES);

    let mut result = ParsedMemRegion::new();
    result.sender_id = sender_id;
    result.receiver_id = receiver_id;
    result.flags = flags;
    result.total_page_count = total_page_count;

    for i in 0..count {
        let range_off = ranges_offset + i * range_size;
        if range_off + range_size > total {
            return Err(crate::ffa::FFA_INVALID_PARAMETERS);
        }
        let range_ptr = tx_ptr.add(range_off);
        let address = core::ptr::read_unaligned(range_ptr as *const u64);
        let page_count = core::ptr::read_unaligned(range_ptr.add(8) as *const u32);

        // Validate page-aligned
        if address & 0xFFF != 0 {
            return Err(crate::ffa::FFA_INVALID_PARAMETERS);
        }

        result.ranges[i] = (address, page_count);
        result.range_count += 1;
    }

    Ok(result)
}

/// Build a minimal FfaMemRegion descriptor in a buffer for testing.
///
/// Returns the total descriptor length.
///
/// # Safety
///
/// `buf` must point to at least 128 bytes of writable memory.
pub unsafe fn build_test_descriptor(
    buf: *mut u8,
    sender_id: u16,
    receiver_id: u16,
    ranges: &[(u64, u32)],
) -> u32 {
    core::ptr::write_bytes(buf, 0, 128);

    // FfaMemRegion header (48 bytes)
    // sender_id at offset 0
    core::ptr::write_unaligned(buf as *mut u16, sender_id);
    // receiver_count at offset 32
    core::ptr::write_unaligned(buf.add(32) as *mut u32, 1);
    // receivers_offset at offset 36 (right after the 48-byte header)
    let recv_off: u32 = 48;
    core::ptr::write_unaligned(buf.add(36) as *mut u32, recv_off);

    // FfaMemAccessDesc (16 bytes) at offset 48
    let access_ptr = buf.add(recv_off as usize);
    core::ptr::write_unaligned(access_ptr as *mut u16, receiver_id);
    // composite_offset at +4 (from start of FfaMemRegion)
    let comp_off: u32 = 48 + 16; // after access desc
    core::ptr::write_unaligned(access_ptr.add(4) as *mut u32, comp_off);

    // FfaCompositeMemRegion (16 bytes) at offset 64
    let comp_ptr = buf.add(comp_off as usize);
    let total_pages: u32 = ranges.iter().map(|(_, c)| *c).sum();
    core::ptr::write_unaligned(comp_ptr as *mut u32, total_pages);
    core::ptr::write_unaligned(comp_ptr.add(4) as *mut u32, ranges.len() as u32);

    // FfaMemRegionAddrRange (16 bytes each) starting at offset 80
    let ranges_start = comp_off as usize + 16;
    for (i, &(addr, count)) in ranges.iter().enumerate() {
        let range_ptr = buf.add(ranges_start + i * 16);
        core::ptr::write_unaligned(range_ptr as *mut u64, addr);
        core::ptr::write_unaligned(range_ptr.add(8) as *mut u32, count);
    }

    (ranges_start + ranges.len() * 16) as u32
}

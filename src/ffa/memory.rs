//! FF-A Memory sharing — page ownership validation via Stage-2 PTE SW bits.

/// Page ownership state (maps to PTE SW bits [56:55]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum PageOwnership {
    Owned = 0b00,
    SharedOwned = 0b01,
    SharedBorrowed = 0b10,
    Donated = 0b11,
}

impl PageOwnership {
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x3 {
            0b00 => Self::Owned,
            0b01 => Self::SharedOwned,
            0b10 => Self::SharedBorrowed,
            0b11 => Self::Donated,
            _ => unreachable!(),
        }
    }
}

/// Validate that a page can be shared (must be in OWNED state).
pub fn validate_page_for_share(sw_bits: u8) -> Result<(), i32> {
    let state = PageOwnership::from_bits(sw_bits);
    match state {
        PageOwnership::Owned => Ok(()),
        _ => Err(crate::ffa::FFA_DENIED),
    }
}

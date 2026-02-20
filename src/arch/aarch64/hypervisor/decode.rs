/// ARM64 instruction decoder for MMIO emulation
///
/// This module decodes load/store instructions that cause data aborts
/// when accessing MMIO regions.

/// Decoded load/store instruction
#[derive(Debug, Clone, Copy)]
pub enum MmioAccess {
    /// Load instruction: LDR, LDRB, LDRH, etc.
    Load {
        reg: u8,  // Destination register (0-30)
        size: u8, // Access size in bytes (1, 2, 4, 8)
        sign_extend: bool,
    },
    /// Store instruction: STR, STRB, STRH, etc.
    Store {
        reg: u8,  // Source register (0-30)
        size: u8, // Access size in bytes (1, 2, 4, 8)
    },
}

impl MmioAccess {
    /// Decode an instruction that caused a data abort
    ///
    /// # Arguments
    /// * `insn` - The 32-bit instruction encoding
    /// * `iss` - Instruction Specific Syndrome from ESR_EL2
    ///
    /// # Returns
    /// * `Some(MmioAccess)` if successfully decoded
    /// * `None` if instruction is not a supported load/store
    pub fn decode(insn: u32, iss: u32) -> Option<Self> {
        // ISS encoding for data abort (ISS[24] = 1 means ISV is valid)
        let isv = (iss >> 24) & 1;
        if isv == 0 {
            // ISS not valid, need to decode instruction manually
            return Self::decode_instruction(insn);
        }

        // ISS is valid, extract fields
        let sas = (iss >> 22) & 0x3; // Size: 00=byte, 01=half, 10=word, 11=double
        let srt = (iss >> 16) & 0x1F; // Source/dest register
        let _sf = (iss >> 15) & 1; // 0=32-bit, 1=64-bit
        let _ar = (iss >> 14) & 1; // Acquire/Release
        let wnr = (iss >> 6) & 1; // Write not Read: 0=read, 1=write
        let sext = (iss >> 23) & 1; // Sign extend

        let size = match sas {
            0 => 1, // Byte
            1 => 2, // Halfword
            2 => 4, // Word
            3 => 8, // Doubleword
            _ => return None,
        };

        if wnr == 1 {
            // Store (write)
            Some(MmioAccess::Store {
                reg: srt as u8,
                size: size as u8,
            })
        } else {
            // Load (read)
            Some(MmioAccess::Load {
                reg: srt as u8,
                size: size as u8,
                sign_extend: sext != 0,
            })
        }
    }

    /// Decode instruction manually when ISV is not valid
    fn decode_instruction(insn: u32) -> Option<Self> {
        // Check instruction encoding
        // ARM64 load/store instructions have specific bit patterns

        // LDR/STR (immediate, unsigned offset)
        // Encoding: op1|1|op2|op3|Rn|Rt where op1=[11,10], op2=[1,0]
        let _op0 = (insn >> 28) & 0xF;
        let _op1 = (insn >> 26) & 0x3;
        let _op2 = (insn >> 23) & 0x3;
        let _op3 = (insn >> 22) & 0x3;

        // Load/Store register (unsigned immediate)
        // xx|111|0|01|xx|...... where xx is size
        if (insn & 0x3B000000) == 0x39000000 {
            let size_bits = (insn >> 30) & 0x3;
            let size = 1u8 << size_bits;
            let rt = (insn & 0x1F) as u8;
            let is_load = (insn >> 22) & 1;

            if is_load == 1 {
                Some(MmioAccess::Load {
                    reg: rt,
                    size,
                    sign_extend: false,
                })
            } else {
                Some(MmioAccess::Store { reg: rt, size })
            }
        } else {
            // Unsupported instruction
            None
        }
    }

    /// Get the register number
    pub fn reg(&self) -> u8 {
        match self {
            MmioAccess::Load { reg, .. } => *reg,
            MmioAccess::Store { reg, .. } => *reg,
        }
    }

    /// Get the access size in bytes
    pub fn size(&self) -> u8 {
        match self {
            MmioAccess::Load { size, .. } => *size,
            MmioAccess::Store { size, .. } => *size,
        }
    }

    /// Check if this is a load instruction
    pub fn is_load(&self) -> bool {
        matches!(self, MmioAccess::Load { .. })
    }

    /// Check if this is a store instruction
    pub fn is_store(&self) -> bool {
        matches!(self, MmioAccess::Store { .. })
    }
}

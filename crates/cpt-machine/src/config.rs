//! Machine configuration — the single place where CPT hardware constants live.
//!
//! Many of these are still unverified against the CPT system source; they are centralized here
//! so a confirmed value is a one-line change rather than a hunt through the codebase. Device
//! port ranges will be added as those devices are implemented.

/// How the bank-select I/O port encodes the chosen block in the byte written to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankSelectEncoding {
    /// The low `bits` of the written byte form the block index (e.g. 3 bits -> 0..=7).
    LowBits { bits: u8 },
}

impl BankSelectEncoding {
    /// Decode a byte written to the bank-select port into a block index.
    pub fn decode(self, value: u8) -> usize {
        match self {
            BankSelectEncoding::LowBits { bits } => {
                let mask = if bits >= 8 { 0xFF } else { (1u16 << bits) as u8 - 1 };
                (value & mask) as usize
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MachineConfig {
    /// I/O port that selects which physical 32K block is mapped into the 8000-FFFF window.
    /// [VERIFY] — placeholder value until confirmed from the long-call handler source.
    pub bank_select_port: u8,

    /// How the byte written to `bank_select_port` encodes the block index. [VERIFY]
    pub bank_select_encoding: BankSelectEncoding,

    /// Number of physical 32K blocks available for bank 1.
    /// 8 blocks = 256K of switchable RAM (256K / 32K). [CONFIRMED: 256K common machine]
    pub block_count: usize,

    /// Which block is selected into bank 1 at power-on / reset.
    pub initial_block: usize,
}

impl Default for MachineConfig {
    /// A reasonable default for a 256K machine (8 switchable blocks). The bank-select port
    /// address and encoding are placeholders pending the system source.
    fn default() -> Self {
        MachineConfig {
            bank_select_port: 0x40, // [VERIFY] placeholder
            bank_select_encoding: BankSelectEncoding::LowBits { bits: 3 }, // 8 blocks -> 3 bits
            block_count: 8,
            initial_block: 0,
        }
    }
}

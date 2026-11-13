//! The CPT two-bank memory model.
//!
//! The 8080's 64K address space is split into two 32K halves:
//!   * **bank 0** (0x0000-0x7FFF): resident RAM, always present (monitor, long-call handler,
//!     tables, work-block heap live here).
//!   * **bank 1** (0x8000-0xFFFF): a window onto one of several physical 32K blocks, chosen by
//!     writing the bank-select I/O port.
//!
//! De-selected blocks retain their contents, which is exactly what CPT's long-call mechanism
//! relies on: it saves the current block, swaps in another, calls into it, then restores the
//! original block. (Req 2.)

use crate::config::MachineConfig;

/// Size of each bank / physical block (32 KiB).
pub const BANK_SIZE: usize = 0x8000;

/// The boundary address between bank 0 and bank 1.
pub const BANK1_BASE: u16 = 0x8000;

pub struct BankedMemory {
    /// Resident bank 0 (0x0000-0x7FFF).
    bank0: Box<[u8; BANK_SIZE]>,
    /// Physical 32K blocks; one is mapped into bank 1 at a time.
    blocks: Vec<Box<[u8; BANK_SIZE]>>,
    /// Index into `blocks` currently mapped to the 0x8000-0xFFFF window.
    selected: usize,
}

impl BankedMemory {
    /// Create banked memory with `block_count` zeroed 32K blocks and the initial block
    /// selected, per the config.
    pub fn new(config: &MachineConfig) -> Self {
        assert!(config.block_count >= 1, "need at least one bank-1 block");
        let blocks = (0..config.block_count)
            .map(|_| Box::new([0u8; BANK_SIZE]))
            .collect();
        let selected = config.initial_block.min(config.block_count - 1);
        BankedMemory {
            bank0: Box::new([0u8; BANK_SIZE]),
            blocks,
            selected,
        }
    }

    /// The number of physical 32K blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The currently selected bank-1 block index.
    pub fn selected_block(&self) -> usize {
        self.selected
    }

    /// Select which physical block is mapped into bank 1. Out-of-range indices are clamped to
    /// the available blocks (the hardware has a fixed set of blocks; a decode that names a
    /// missing one cannot map anything new).
    pub fn select_block(&mut self, index: usize) {
        if index < self.blocks.len() {
            self.selected = index;
        }
    }

    /// Read a byte at a CPU address, routing to bank 0 or the selected bank-1 block.
    pub fn read(&self, addr: u16) -> u8 {
        if addr < BANK1_BASE {
            self.bank0[addr as usize]
        } else {
            self.blocks[self.selected][(addr - BANK1_BASE) as usize]
        }
    }

    /// Write a byte at a CPU address. Bank 0 is all RAM (no ROM protection); bank-1 writes
    /// land in the selected block.
    pub fn write(&mut self, addr: u16, val: u8) {
        if addr < BANK1_BASE {
            self.bank0[addr as usize] = val;
        } else {
            self.blocks[self.selected][(addr - BANK1_BASE) as usize] = val;
        }
    }

    // ---- loading helpers (used by the image loader / tests) ----

    /// All of bank 0 (0000H-7FFFH).
    pub fn bank0(&self) -> &[u8; BANK_SIZE] {
        &self.bank0
    }

    /// Load bytes into bank 0 starting at `offset` (0-based within bank 0).
    pub fn load_bank0(&mut self, offset: u16, bytes: &[u8]) {
        let start = offset as usize;
        self.bank0[start..start + bytes.len()].copy_from_slice(bytes);
    }

    /// Load bytes into a specific physical block starting at `offset` (0-based within the 32K
    /// block, i.e. where 0 == CPU address 0x8000).
    pub fn load_block(&mut self, block: usize, offset: u16, bytes: &[u8]) {
        let start = offset as usize;
        self.blocks[block][start..start + bytes.len()].copy_from_slice(bytes);
    }

    /// Direct read from a specific physical block (for the debugger's bank-aware memory view,
    /// Req 9.5) — independent of which block is currently selected.
    pub fn peek_block(&self, block: usize, offset: u16) -> u8 {
        self.blocks[block][offset as usize]
    }
}

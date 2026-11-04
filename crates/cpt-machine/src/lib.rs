//! CPT workstation hardware model.
//!
//! This crate ties the CPT-specific hardware to the generic [`i8080_cpu`] core by implementing
//! its [`Bus`] trait. The first piece is the banked memory + bank-select port (Req 2); display,
//! keyboard, and the data disk are added as later increments, each behind its port range in
//! [`MachineConfig`].

pub mod config;
pub mod memory;

pub use config::{BankSelectEncoding, MachineConfig};
pub use memory::{BankedMemory, BANK1_BASE, BANK_SIZE};

use i8080_cpu::Bus;

/// The CPT machine: banked memory + device I/O, presented to the CPU as a [`Bus`].
pub struct Machine {
    pub mem: BankedMemory,
    config: MachineConfig,
    /// Record of writes to unmapped I/O ports, for debugging while the port map is unknown.
    pub unmapped_io: Vec<(u8, u8)>,
}

impl Machine {
    pub fn new(config: MachineConfig) -> Self {
        let mem = BankedMemory::new(&config);
        Machine {
            mem,
            config,
            unmapped_io: Vec::new(),
        }
    }

    /// The configuration this machine was built with.
    pub fn config(&self) -> &MachineConfig {
        &self.config
    }

    /// The currently selected bank-1 block (for the debugger).
    pub fn selected_block(&self) -> usize {
        self.mem.selected_block()
    }
}

impl Bus for Machine {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem.read(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.mem.write(addr, val);
    }

    fn io_in(&mut self, port: u8) -> u8 {
        // No readable device ports modeled yet; reading the bank-select port returns the
        // current selection for convenience/debugging. Everything else reads as 0.
        if port == self.config.bank_select_port {
            self.mem.selected_block() as u8
        } else {
            0
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        if port == self.config.bank_select_port {
            let index = self.config.bank_select_encoding.decode(val);
            self.mem.select_block(index);
        } else {
            // Unmapped port: record it so we can discover the real port map later, but don't
            // crash. (Devices will claim their ports in future increments.)
            self.unmapped_io.push((port, val));
        }
    }
}

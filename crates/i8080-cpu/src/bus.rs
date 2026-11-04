//! The CPU's view of the outside world.
//!
//! The 8080 core is hardware-agnostic: it reaches memory and I/O only through this trait, so
//! it knows nothing about banking, devices, or the CPT machine. The emulator's `cpt-machine`
//! crate implements `Bus` to provide banked memory and device ports.

pub trait Bus {
    /// Read a byte from the 16-bit memory address space.
    fn read(&mut self, addr: u16) -> u8;
    /// Write a byte to the 16-bit memory address space.
    fn write(&mut self, addr: u16, val: u8);
    /// Read a byte from an 8-bit I/O port (the 8080 `IN` instruction).
    fn io_in(&mut self, port: u8) -> u8;
    /// Write a byte to an 8-bit I/O port (the 8080 `OUT` instruction).
    fn io_out(&mut self, port: u8, val: u8);
}

/// A simple flat 64K RAM `Bus`, handy for unit tests and the conformance harness.
pub struct FlatMemory {
    pub mem: Box<[u8; 0x10000]>,
    /// Last value written to each I/O port (for test inspection).
    pub io: [u8; 256],
}

impl Default for FlatMemory {
    fn default() -> Self {
        FlatMemory {
            mem: Box::new([0; 0x10000]),
            io: [0; 256],
        }
    }
}

impl FlatMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load bytes into memory starting at `addr`.
    pub fn load(&mut self, addr: u16, bytes: &[u8]) {
        let start = addr as usize;
        self.mem[start..start + bytes.len()].copy_from_slice(bytes);
    }
}

impl Bus for FlatMemory {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }
    fn write(&mut self, addr: u16, val: u8) {
        self.mem[addr as usize] = val;
    }
    fn io_in(&mut self, port: u8) -> u8 {
        self.io[port as usize]
    }
    fn io_out(&mut self, port: u8, val: u8) {
        self.io[port as usize] = val;
    }
}

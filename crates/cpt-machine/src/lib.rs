//! CPT workstation hardware model.
//!
//! This crate ties the CPT-specific hardware to the generic [`i8080_cpu`] core by implementing
//! its [`Bus`] trait. It provides the banked memory + bank-select port (Req 2), a [`Device`]
//! interface for peripherals, and an executive loop that runs the CPU while ticking devices
//! and delivering interrupts.

pub mod config;
pub mod cpt_display;
pub mod cpt_floppy;
pub mod cpt_interrupts;
pub mod cpt_keyboard;
pub mod cpt_system;
pub mod cpt_usart;
pub mod demo;
pub mod device;
pub mod disk;
pub mod disk_controller;
pub mod display;
pub mod exec;
pub mod keyboard;
pub mod memory;
pub mod serial;
pub mod speaker;
pub mod timer;

pub use config::{BankSelectEncoding, MachineConfig};
pub use device::{Device, DmaRequest};
pub use disk::{DiskImage, Drive, Geometry, Station};
pub use cpt_display::CptScreen;
pub use cpt_floppy::CptFloppy;
pub use cpt_interrupts::{InterruptAck, Pit8253};
pub use cpt_keyboard::CptKeyboard;
pub use cpt_system::CptSystem;
pub use cpt_usart::Usart8251;
pub use disk_controller::DiskController;
pub use display::{DisplayDevice, Layout, PageSnapshot, COLS, ROWS};
pub use keyboard::KeyboardDevice;
pub use serial::{pump as serial_pump, LoopbackTransport, SerialDevice, Transport};
pub use speaker::{wav_bytes, SpeakerDevice, SpeakerEvent, ToneRenderer};
pub use timer::TimerDevice;
pub use exec::Executive;
pub use memory::{BankedMemory, BANK1_BASE, BANK_SIZE};

use i8080_cpu::Bus;

/// The CPT machine: banked memory + a set of port-mapped devices, presented to the CPU as a
/// [`Bus`].
pub struct Machine {
    pub mem: BankedMemory,
    config: MachineConfig,
    devices: Vec<Box<dyn Device>>,
    /// Memory-mapped display window: (base address, length, device index), if a display device
    /// is attached. CPU reads/writes in `[base, base+len)` route to that device's RAM instead
    /// of banked memory.
    display_window: Option<(u16, u16, usize)>,
    /// Record of writes to unmapped I/O ports, for debugging while the port map is unknown.
    pub unmapped_io: Vec<(u8, u8)>,
    /// Unmapped I/O ports that were read (each listed once), for the same reason.
    pub unmapped_in: Vec<u8>,
    /// Memory ranges being watched (debugging; empty normally, which costs nothing).
    watches: Vec<Watch>,
    /// Accesses to watched memory since the host last took them (see [`Machine::add_watch`]).
    pub watch_hits: Vec<WatchHit>,
    /// True while a device's DMA transfer is going through the memory path.
    dma_active: bool,
}

/// A memory range to watch, for debugging: every matching access is logged in
/// [`Machine::watch_hits`]. The host (`cpt-run --watch`) takes the log after each instruction
/// and knows which instruction it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    /// First and last address watched (inclusive).
    pub lo: u16,
    pub hi: u16,
    pub reads: bool,
    pub writes: bool,
    /// For addresses of 8000H and up: only this block (None = whichever is selected).
    pub block: Option<usize>,
}

/// One logged access to watched memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchHit {
    pub addr: u16,
    pub write: bool,
    pub value: u8,
    /// The block selected at 8000H–FFFFH when it happened.
    pub block: usize,
    /// It was a device's DMA transfer, not the CPU.
    pub dma: bool,
}

/// The log stops growing here, in case a host never takes it.
const WATCH_LOG_LIMIT: usize = 1_000_000;

impl Machine {
    pub fn new(config: MachineConfig) -> Self {
        let mem = BankedMemory::new(&config);
        Machine {
            mem,
            config,
            devices: Vec::new(),
            display_window: None,
            unmapped_io: Vec::new(),
            unmapped_in: Vec::new(),
            watches: Vec::new(),
            watch_hits: Vec::new(),
            dma_active: false,
        }
    }

    /// Watch a range of memory: every read and/or write in it is logged in `watch_hits`. The
    /// CPU's instruction fetches are reads too, so a range holding code logs those.
    pub fn add_watch(&mut self, w: Watch) {
        self.watches.push(w);
    }

    /// Log an access if it falls in a watched range.
    fn note_access(&mut self, addr: u16, write: bool, value: u8) {
        if self.watch_hits.len() >= WATCH_LOG_LIMIT {
            return;
        }
        let block = self.mem.selected_block();
        for w in &self.watches {
            if addr < w.lo || addr > w.hi || (write && !w.writes) || (!write && !w.reads) {
                continue;
            }
            if addr >= BANK1_BASE && w.block.is_some_and(|b| b != block) {
                continue;
            }
            self.watch_hits.push(WatchHit { addr, write, value, block, dma: self.dma_active });
            return;
        }
    }

    fn read_raw(&mut self, addr: u16) -> u8 {
        if let Some((idx, offset)) = self.display_hit(addr) {
            if let Some(d) = self.devices[idx].as_any_mut().downcast_mut::<DisplayDevice>() {
                return d.read_cell(offset);
            }
        }
        self.mem.read(addr)
    }

    fn write_raw(&mut self, addr: u16, val: u8) {
        if let Some((idx, offset)) = self.display_hit(addr) {
            if let Some(d) = self.devices[idx].as_any_mut().downcast_mut::<DisplayDevice>() {
                d.write_cell(offset, val);
                return;
            }
        }
        self.mem.write(addr, val);
    }

    /// Attach the memory-mapped display device, mapping its character RAM at `config.display_base`
    /// (within bank 0). Returns the device index.
    pub fn attach_display(&mut self, display: DisplayDevice) -> usize {
        let base = self.config.display_base;
        let len = display.ram_len() as u16;
        assert!(
            (base as u32 + len as u32) <= BANK1_BASE as u32,
            "display RAM window must fit within bank 0 (0x0000-0x7FFF)"
        );
        let index = self.devices.len();
        self.devices.push(Box::new(display));
        self.display_window = Some((base, len, index));
        index
    }

    /// The memory-mapped display device, if attached.
    pub fn display_mut(&mut self) -> Option<&mut DisplayDevice> {
        let idx = self.display_window?.2;
        self.devices[idx].as_any_mut().downcast_mut::<DisplayDevice>()
    }

    /// If `addr` falls in the display window, return `(device_index, ram_offset)`.
    fn display_hit(&self, addr: u16) -> Option<(usize, usize)> {
        let (base, len, idx) = self.display_window?;
        if addr >= base && addr < base.wrapping_add(len) {
            Some((idx, (addr - base) as usize))
        } else {
            None
        }
    }

    /// Attach a device. Panics if any of its ports collide with the bank-select port or with a
    /// previously attached device, since overlapping port routing would be ambiguous.
    pub fn attach_device(&mut self, device: Box<dyn Device>) {
        for &p in device.ports() {
            assert_ne!(
                p, self.config.bank_select_port,
                "device port {:#04X} collides with the bank-select port",
                p
            );
            assert!(
                self.device_index_for_port(p).is_none(),
                "device port {:#04X} is already claimed by another device",
                p
            );
        }
        self.devices.push(device);
    }

    /// The configuration this machine was built with.
    pub fn config(&self) -> &MachineConfig {
        &self.config
    }

    /// The currently selected bank-1 block (for the debugger).
    pub fn selected_block(&self) -> usize {
        self.mem.selected_block()
    }

    /// Advance every device by `cycles` CPU cycles (called by the executive each step).
    pub fn tick_devices(&mut self, cycles: u32) {
        for d in &mut self.devices {
            d.tick(cycles);
        }
        for i in 0..self.devices.len() {
            self.service_dma(i);
        }
    }

    /// Carry out any DMA transfers device `i` has pending, through the normal memory path
    /// (so banked memory and the display window behave as for CPU accesses).
    fn service_dma(&mut self, i: usize) {
        while let Some(req) = self.devices[i].take_dma() {
            self.dma_active = true;
            match req {
                DmaRequest::WriteMemory { addr, data } => {
                    for (k, b) in data.iter().enumerate() {
                        self.write(addr.wrapping_add(k as u16), *b);
                    }
                }
                DmaRequest::ReadMemory { addr, len } => {
                    let data: Vec<u8> =
                        (0..len).map(|k| self.read(addr.wrapping_add(k as u16))).collect();
                    self.devices[i].dma_read_done(&data);
                }
            }
            self.dma_active = false;
        }
    }

    /// Poll devices for a pending interrupt, returning the first RST vector requested.
    pub fn poll_irq(&mut self) -> Option<u8> {
        for d in &mut self.devices {
            if let Some(vec) = d.take_irq() {
                return Some(vec);
            }
        }
        None
    }

    /// Borrow an attached device by index (for host/UI access, e.g. the display page buffer).
    pub fn device_mut(&mut self, index: usize) -> Option<&mut (dyn Device + 'static)> {
        match self.devices.get_mut(index) {
            Some(b) => Some(&mut **b),
            None => None,
        }
    }

    /// Number of attached devices.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    fn device_index_for_port(&self, port: u8) -> Option<usize> {
        self.devices
            .iter()
            .position(|d| d.ports().contains(&port))
    }
}

impl Bus for Machine {
    fn read(&mut self, addr: u16) -> u8 {
        let v = self.read_raw(addr);
        if !self.watches.is_empty() {
            self.note_access(addr, false, v);
        }
        v
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.write_raw(addr, val);
        if !self.watches.is_empty() {
            self.note_access(addr, true, val);
        }
    }

    fn io_in(&mut self, port: u8) -> u8 {
        if port == self.config.bank_select_port {
            return self.mem.selected_block() as u8;
        }
        if let Some(i) = self.device_index_for_port(port) {
            return self.devices[i].io_in(port);
        }
        // Unmapped port: record it (once) for port-map discovery; reads float to 0.
        if !self.unmapped_in.contains(&port) {
            self.unmapped_in.push(port);
        }
        0
    }

    fn io_out(&mut self, port: u8, val: u8) {
        if port == self.config.bank_select_port {
            let index = self.config.bank_select_encoding.decode(val);
            self.mem.select_block(index);
            return;
        }
        let owner = self.device_index_for_port(port);
        if let Some(i) = owner {
            self.devices[i].io_out(port, val);
            self.service_dma(i);
        } else {
            // Unmapped port: record it so we can discover the real port map later, but don't crash.
            self.unmapped_io.push((port, val));
        }
        for d in &mut self.devices {
            d.observe_out(port, val);
        }
    }
}

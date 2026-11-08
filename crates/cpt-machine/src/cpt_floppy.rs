//! The real CPT floppy hardware, as found in the boot code of a real program disk
//! (`.kiro/specs/cpt8000-emulator/program-disk.md`).
//!
//! | Port | Chip / function                                                                   |
//! |------|-----------------------------------------------------------------------------------|
//! | 10H  | Intel 8257 DMA, channel 0 address (low byte, then high byte)                      |
//! | 11H  | 8257 channel 0 terminal count (low, then high: bits 13-0 = count-1, 15-14 = mode)  |
//! | 18H  | 8257 mode set (also resets the low/high byte flip-flop)                            |
//! | 22H  | drive select latch (bit 0 = drive) [VERIFY other bits]                            |
//! | 23H  | out: 3-phase head stepper (bits 0-2, one-hot 1/2/4; 1->2->4 = step in); bit 3 ?   |
//! |      | in : bit 1 = FD1771 interrupt request (operation done), bit 2 = head at track 0   |
//! | 24H  | WD FD1771: command (out) / status (in)                                             |
//! | 25H  | FD1771 track register                                                             |
//! | 26H  | FD1771 sector register                                                            |
//! | 27H  | FD1771 data register                                                              |
//!
//! The CPU moves the head itself by sequencing the stepper phases on port 23H, so the FD1771's
//! own seek/step commands only update its track register here. Sectors are transferred by DMA:
//! a Read Sector command copies the sector to the 8257's address (up to its count) through the
//! machine, then raises the interrupt-request bit. Geometry: 77 tracks x 16 sectors (IDs 0-15)
//! x 256 bytes, FM; an image is the sectors in track order, each track in sector-ID order.
//!
//! What is modeled is what the boot code uses; timing (rotation, head settle) is not.

use crate::device::{Device, DmaRequest};
use crate::disk::{DiskImage, Geometry};
use std::any::Any;

pub const TRACKS: usize = 77;
pub const SECTORS: usize = 16;
pub const SECTOR_SIZE: usize = 256;

/// The disk geometry of a CPT program disk.
pub fn cpt_geometry() -> Geometry {
    Geometry { block_size: SECTOR_SIZE, block_count: TRACKS * SECTORS }
}

// FD1771 status bits (Type II commands).
const ST_BUSY: u8 = 0x01;
const ST_RNF: u8 = 0x10; // record not found
const ST_NOT_READY: u8 = 0x80;

pub struct CptFloppy {
    ports: [u8; 10],
    // --- 8257 DMA, channel 0 ---
    dma_addr: u16,
    dma_count: u16,
    dma_high: bool, // the first/last flip-flop: false = next byte is the low byte
    dma_mode: u8,
    // --- FD1771 ---
    status: u8,
    track_reg: u8,
    sector_reg: u8,
    data_reg: u8,
    intrq: bool,
    // --- drives and head ---
    drive_sel: u8,
    drives: [Option<DiskImage>; 4],
    head_track: [u8; 4],
    phase: u8,
    pending: Vec<DmaRequest>,
    /// A write waiting for its data from memory: (drive, linear sector).
    pending_write: Option<(usize, usize)>,
    /// Count of sectors read, for diagnostics.
    pub sectors_read: u64,
    /// Every (track, sector) read, in order, for diagnostics.
    pub read_log: Vec<(u8, u8)>,
    /// Every command, for diagnostics: (command, track reg, sector reg, head track, DMA
    /// address, DMA count, resulting status).
    pub cmd_log: Vec<(u8, u8, u8, u8, u16, u16, u8)>,
    /// CPU cycles a sector read/write takes before it completes (0 = instantly). The boot
    /// code times a read (see `program-disk.md`), so instant completion is not faithful.
    /// Real timing (rotation, sector order on the track) is [VERIFY]; this is a fixed delay.
    pub op_delay_cycles: u32,
    /// Cycles left before the current command completes, if one is in progress.
    busy_left: Option<u32>,
    /// What the in-progress command will do when it completes.
    deferred: Vec<DmaRequest>,
    /// RST vector raised when INTRQ rises (RST 3: the main system's handler at 06BAH reads 23H
    /// and 24H). `None` = polled only.
    pub irq_vector: Option<u8>,
    /// INTRQ rose and the interrupt has not been taken yet.
    irq_edge: bool,
}

impl CptFloppy {
    pub fn new() -> Self {
        CptFloppy {
            ports: [0x10, 0x11, 0x18, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x12],
            dma_addr: 0,
            dma_count: 0,
            dma_high: false,
            dma_mode: 0,
            status: 0,
            track_reg: 0,
            sector_reg: 0,
            data_reg: 0,
            intrq: false,
            drive_sel: 0,
            drives: [None, None, None, None],
            head_track: [0; 4],
            phase: 1,
            pending: Vec::new(),
            pending_write: None,
            sectors_read: 0,
            read_log: Vec::new(),
            cmd_log: Vec::new(),
            op_delay_cycles: 0,
            busy_left: None,
            deferred: Vec::new(),
            irq_vector: Some(3),
            irq_edge: false,
        }
    }

    /// Raise INTRQ (and, once per rising edge, the interrupt request).
    fn raise_intrq(&mut self) {
        if !self.intrq {
            self.irq_edge = true;
        }
        self.intrq = true;
    }

    /// Insert a disk image (77 x 16 x 256 bytes) in drive 0 or 1.
    pub fn mount(&mut self, drive: usize, image: DiskImage) {
        self.drives[drive & 3] = Some(image);
    }

    pub fn image(&self, drive: usize) -> Option<&DiskImage> {
        self.drives[drive & 3].as_ref()
    }

    /// The selected drive. The system probes selects 0-3 (routine 0C74H), so the latch holds
    /// a drive number in bits 0-1 [VERIFY other bits].
    fn drive(&self) -> usize {
        (self.drive_sel & 3) as usize
    }

    /// The physical track the selected drive's head is on.
    pub fn head_track(&self) -> u8 {
        self.head_track[self.drive()]
    }

    fn linear(&self) -> Option<usize> {
        let t = self.head_track() as usize;
        let s = self.sector_reg as usize;
        // The head must be on the track the controller is looking for.
        if t != self.track_reg as usize || t >= TRACKS || s >= SECTORS {
            return None;
        }
        Some(t * SECTORS + s)
    }

    /// Bytes the DMA channel will move: count-1 in the low 14 bits.
    fn dma_len(&self) -> usize {
        (self.dma_count & 0x3FFF) as usize + 1
    }

    fn command(&mut self, cmd: u8) {
        self.intrq = false;
        let drive = self.drive();
        if cmd & 0xF0 == 0xD0 {
            // Force interrupt: stop, and interrupt if any condition bit is set.
            self.status &= !ST_BUSY;
            if cmd & 0x0F != 0 {
                self.raise_intrq();
            }
            return;
        }
        if self.drives[drive].is_none() {
            self.status = ST_NOT_READY;
            self.raise_intrq();
            return;
        }
        match cmd >> 4 {
            // Type I: restore / seek / step. The head itself is moved by the CPU through the
            // stepper phases, so these only update the track register.
            0x0 => self.track_reg = 0,
            0x1 => self.track_reg = self.data_reg,
            0x2..=0x7 => {}
            // Type II: read sector.
            0x8 | 0x9 => match self.linear() {
                Some(lin) => {
                    let mut buf = vec![0u8; SECTOR_SIZE];
                    self.drives[drive].as_ref().unwrap().read_block(lin, &mut buf);
                    let n = self.dma_len().min(SECTOR_SIZE);
                    buf.truncate(n);
                    self.pending.push(DmaRequest::WriteMemory { addr: self.dma_addr, data: buf });
                    self.sectors_read += 1;
                    self.read_log.push((self.track_reg, self.sector_reg));
                    self.status = 0;
                }
                None => self.status = ST_RNF,
            },
            // Type II: write sector (data fetched from memory by DMA).
            0xA | 0xB => match self.linear() {
                Some(lin) => {
                    let n = self.dma_len().min(SECTOR_SIZE);
                    self.pending.push(DmaRequest::ReadMemory { addr: self.dma_addr, len: n });
                    self.pending_write = Some((drive, lin));
                    self.status = 0;
                }
                None => self.status = ST_RNF,
            },
            // Type III: read address -> the ID field of the next sector under the head.
            0xC => {
                let id = vec![self.head_track(), 0, self.sector_reg, 1, 0, 0];
                self.pending.push(DmaRequest::WriteMemory { addr: self.dma_addr, data: id });
                self.status = 0;
            }
            _ => self.status = 0,
        }
        self.cmd_log.push((cmd, self.track_reg, self.sector_reg, self.head_track(), self.dma_addr,
                           self.dma_count, self.status));
        let is_transfer = matches!(cmd >> 4, 0x8..=0xB);
        if is_transfer && self.op_delay_cycles > 0 {
            // Busy until the sector "comes round": hold the transfer and the interrupt request.
            self.deferred = std::mem::take(&mut self.pending);
            self.busy_left = Some(self.op_delay_cycles);
            self.status |= ST_BUSY;
        } else {
            self.raise_intrq();
        }
    }

    /// The CPU drives a **three-phase** head stepper through bits 0-2 (one-hot 1, 2, 4); the boot
    /// code steps in with 1 -> 2 -> 4 -> 1 and out with 4 -> 2 -> 1 -> 4. Bit 3 is ORed into
    /// every write and is something else (head load / step enable?) [VERIFY]. Each move to the
    /// next phase in the cycle moves the head one track. Track 0 is reached on phase 1 (the boot
    /// code's homing loop stops only when the track-0 sensor is on AND the phase is 1).
    fn step_phases(&mut self, val: u8) {
        let p = val & 0x07;
        if p.count_ones() != 1 {
            return;
        }
        let next = |x: u8| if x == 4 { 1 } else { x << 1 };
        let prev = |x: u8| if x == 1 { 4 } else { x >> 1 };
        let d = self.drive();
        if p == next(self.phase) && (self.head_track[d] as usize) < TRACKS - 1 {
            self.head_track[d] += 1;
        } else if p == prev(self.phase) && self.head_track[d] > 0 {
            self.head_track[d] -= 1;
        }
        self.phase = p;
    }
}

impl Default for CptFloppy {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for CptFloppy {
    fn ports(&self) -> &[u8] {
        &self.ports[..9]
    }

    fn io_in(&mut self, port: u8) -> u8 {
        match port {
            0x23 => {
                let t0 = (self.head_track() == 0) as u8;
                (self.intrq as u8) << 1 | t0 << 2
            }
            0x24 => {
                self.intrq = false; // reading status clears the interrupt request
                // Not Ready follows the selected drive (the system probes drives with this).
                if self.drives[self.drive()].is_none() {
                    self.status | ST_NOT_READY
                } else {
                    self.status & !ST_NOT_READY
                }
            }
            0x25 => self.track_reg,
            0x26 => self.sector_reg,
            0x27 => self.data_reg,
            0x10 => (self.dma_addr & 0xFF) as u8,
            0x11 => (self.dma_count & 0xFF) as u8,
            _ => 0,
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        match port {
            0x10 => {
                if self.dma_high {
                    self.dma_addr = (self.dma_addr & 0x00FF) | (val as u16) << 8;
                } else {
                    self.dma_addr = (self.dma_addr & 0xFF00) | val as u16;
                }
                self.dma_high = !self.dma_high;
            }
            0x11 => {
                if self.dma_high {
                    self.dma_count = (self.dma_count & 0x00FF) | (val as u16) << 8;
                } else {
                    self.dma_count = (self.dma_count & 0xFF00) | val as u16;
                }
                self.dma_high = !self.dma_high;
            }
            0x18 => {
                self.dma_mode = val;
                self.dma_high = false;
            }
            0x22 => self.drive_sel = val,
            0x23 => self.step_phases(val),
            0x24 => self.command(val),
            0x25 => self.track_reg = val,
            0x26 => self.sector_reg = val,
            0x27 => self.data_reg = val,
            _ => {}
        }
    }

    fn tick(&mut self, cycles: u32) {
        if let Some(left) = self.busy_left {
            if left <= cycles {
                self.busy_left = None;
                self.pending.append(&mut self.deferred);
                self.status &= !ST_BUSY;
                self.raise_intrq();
            } else {
                self.busy_left = Some(left - cycles);
            }
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        // Once per rising edge of INTRQ, and only while it is still up (a polled status read
        // clears it first, so the polled boot loader never gets a stray interrupt).
        if self.irq_edge && self.intrq {
            self.irq_edge = false;
            return self.irq_vector;
        }
        None
    }

    fn take_dma(&mut self) -> Option<DmaRequest> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    fn dma_read_done(&mut self, data: &[u8]) {
        if let Some((drive, lin)) = self.pending_write.take() {
            let mut buf = vec![0u8; SECTOR_SIZE];
            buf[..data.len().min(SECTOR_SIZE)].copy_from_slice(&data[..data.len().min(SECTOR_SIZE)]);
            if let Some(img) = self.drives[drive].as_mut() {
                img.write_block(lin, &buf);
            }
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

//! The disk controller device: exposes drives A and B to the CPU via I/O ports.
//!
//! IMPORTANT: the exact port map and command protocol the real CPT used are **unknown** until
//! the B-tree filer source is available ([VERIFY]). Everything in this file is a **placeholder
//! protocol** — a plausible, self-consistent interface so the rest of the system can be built
//! and tested. It is isolated here so replacing it with the real protocol is a localized
//! change; the `Drive`/`DiskImage` backing store it sits on is real and unaffected.
//!
//! ## Placeholder protocol (five consecutive ports from `base`)
//! | offset | port        | dir | meaning                                                   |
//! |--------|-------------|-----|-----------------------------------------------------------|
//! | +0     | STATION     | out | select drive: 0 = A, 1 = B                                |
//! | +1     | BLOCK_LO    | out | low byte of the target block number                       |
//! | +2     | BLOCK_HI    | out | high byte of the target block number                      |
//! | +3     | COMMAND     | out | 1 = begin read, 2 = begin write, 3 = flush                 |
//! |        | STATUS      | in  | bit0 = last op OK, bit1 = busy(unused), bit7 = no media    |
//! | +4     | DATA        | in/out | stream bytes of the current block (auto-advancing)     |
//!
//! A read command loads the selected block into an internal buffer; successive DATA reads
//! return its bytes. A write command arms the buffer; successive DATA writes fill it, and the
//! block is committed when a full block has been written.

use crate::device::Device;
use crate::disk::{Drive, DiskImage, Geometry, Station};
use std::any::Any;

/// Commands written to the COMMAND port.
const CMD_READ: u8 = 1;
const CMD_WRITE: u8 = 2;
const CMD_FLUSH: u8 = 3;

/// STATUS bits.
const ST_OK: u8 = 0x01;
const ST_NO_MEDIA: u8 = 0x80;

pub struct DiskController {
    base: u8,
    ports: [u8; 5],
    geom: Geometry,

    drive_a: Drive,
    drive_b: Drive,

    // --- protocol state ---
    station: Station,
    block: u16,
    status: u8,
    /// Transfer buffer for the current block, and the cursor into it.
    buffer: Vec<u8>,
    cursor: usize,
    /// True while a write is in progress (DATA writes fill the buffer).
    writing: bool,
    /// Per-station activity timer (in CPU cycles). Set when a station is read/written and
    /// counted down by `tick`; while non-zero the drive's LED is considered lit. This keeps a
    /// brief access visible on screen instead of flickering for a single instruction.
    activity: [u32; 2],
}

/// How long the activity LED stays lit after a disk access, in CPU cycles. Chosen so a single
/// access is visible for a fraction of a second at ~2 MHz. (Cosmetic; not hardware-accurate.)
const ACTIVITY_HOLD_CYCLES: u32 = 100_000;

impl DiskController {
    /// Create a controller claiming five consecutive ports starting at `base`.
    pub fn new(base: u8, geom: Geometry) -> Self {
        DiskController {
            base,
            ports: [base, base + 1, base + 2, base + 3, base + 4],
            geom,
            drive_a: Drive::new(),
            drive_b: Drive::new(),
            station: Station::A,
            block: 0,
            status: 0,
            buffer: vec![0; geom.block_size],
            cursor: 0,
            writing: false,
            activity: [0; 2],
        }
    }

    fn station_index(station: Station) -> usize {
        match station {
            Station::A => 0,
            Station::B => 1,
        }
    }

    /// Mark the selected station as active (lights its LED for a hold window).
    fn mark_active(&mut self) {
        let i = Self::station_index(self.station);
        self.activity[i] = ACTIVITY_HOLD_CYCLES;
    }

    /// True if `station`'s activity LED should be lit (recent read/write).
    pub fn is_active(&self, station: Station) -> bool {
        self.activity[Self::station_index(station)] > 0
    }

    /// Mount an image in a station (runtime-swappable; ejects any current image first).
    pub fn mount(&mut self, station: Station, image: DiskImage) {
        self.drive(station).eject();
        self.drive(station).mount(image);
    }

    /// Eject a station's image (flushing it), returning it.
    pub fn eject(&mut self, station: Station) -> Option<DiskImage> {
        self.drive(station).eject()
    }

    pub fn is_mounted(&self, station: Station) -> bool {
        match station {
            Station::A => self.drive_a.is_mounted(),
            Station::B => self.drive_b.is_mounted(),
        }
    }

    /// True if the mounted image in `station` has been written since mount (for the UI's
    /// "offer download on eject" decision).
    pub fn is_modified(&self, station: Station) -> bool {
        let drive = match station {
            Station::A => &self.drive_a,
            Station::B => &self.drive_b,
        };
        drive.image().map(|img| img.is_modified()).unwrap_or(false)
    }

    fn drive(&mut self, station: Station) -> &mut Drive {
        match station {
            Station::A => &mut self.drive_a,
            Station::B => &mut self.drive_b,
        }
    }

    fn selected(&mut self) -> &mut Drive {
        let s = self.station;
        self.drive(s)
    }

    fn do_command(&mut self, cmd: u8) {
        let block = self.block as usize;
        let bs = self.geom.block_size;
        // Any read/write/flush command counts as drive activity.
        if matches!(cmd, CMD_READ | CMD_WRITE | CMD_FLUSH) {
            self.mark_active();
        }
        match cmd {
            CMD_READ => {
                let mut buf = vec![0u8; bs];
                let ok = self
                    .selected()
                    .image_mut()
                    .map(|img| img.read_block(block, &mut buf))
                    .unwrap_or(false);
                if ok {
                    self.buffer = buf;
                    self.cursor = 0;
                    self.writing = false;
                    self.status = ST_OK;
                } else {
                    self.status = if self.is_selected_empty() { ST_NO_MEDIA } else { 0 };
                }
            }
            CMD_WRITE => {
                if self.is_selected_empty() {
                    self.status = ST_NO_MEDIA;
                } else {
                    self.buffer = vec![0u8; bs];
                    self.cursor = 0;
                    self.writing = true;
                    self.status = ST_OK;
                }
            }
            CMD_FLUSH => {
                let ok = self
                    .selected()
                    .image_mut()
                    .map(|img| img.flush().is_ok())
                    .unwrap_or(false);
                self.status = if ok { ST_OK } else { ST_NO_MEDIA };
            }
            _ => self.status = 0,
        }
    }

    fn is_selected_empty(&mut self) -> bool {
        !self.selected().is_mounted()
    }

    /// A DATA-port write: push a byte into the write buffer; commit the block when full.
    fn data_write(&mut self, val: u8) {
        if !self.writing {
            return;
        }
        self.mark_active();
        if self.cursor < self.buffer.len() {
            self.buffer[self.cursor] = val;
            self.cursor += 1;
        }
        if self.cursor == self.buffer.len() {
            // Block complete: commit to the mounted image.
            let block = self.block as usize;
            let buf = std::mem::take(&mut self.buffer);
            let ok = self
                .selected()
                .image_mut()
                .map(|img| img.write_block(block, &buf))
                .unwrap_or(false);
            self.buffer = buf;
            self.cursor = 0;
            self.writing = false;
            self.status = if ok { ST_OK } else { 0 };
        }
    }

    /// A DATA-port read: return the next byte of the read buffer (auto-advancing).
    fn data_read(&mut self) -> u8 {
        if self.cursor < self.buffer.len() {
            self.mark_active();
            let b = self.buffer[self.cursor];
            self.cursor += 1;
            b
        } else {
            0
        }
    }
}

impl Device for DiskController {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    /// Decay the per-station activity timers so the LEDs turn off a short while after I/O stops.
    fn tick(&mut self, cycles: u32) {
        for a in &mut self.activity {
            *a = a.saturating_sub(cycles);
        }
    }

    fn io_in(&mut self, port: u8) -> u8 {
        let off = port.wrapping_sub(self.base);
        match off {
            3 => self.status, // STATUS shares the COMMAND port offset (in = status)
            4 => self.data_read(),
            _ => 0,
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        let off = port.wrapping_sub(self.base);
        match off {
            0 => self.station = if val & 1 == 0 { Station::A } else { Station::B },
            1 => self.block = (self.block & 0xFF00) | val as u16,
            2 => self.block = (self.block & 0x00FF) | ((val as u16) << 8),
            3 => self.do_command(val),
            4 => self.data_write(val),
            _ => {}
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

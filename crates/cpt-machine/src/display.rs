//! The video display device: memory-mapped character RAM plus the vertical-retrace interrupt.
//!
//! The CPT display is a character display of 80 columns by ~60 rows, structured into regions
//! (title / top-half text / typing-line ruler / bottom-half text / status). The CPU writes
//! character codes into a memory-mapped RAM region (in bank 0); the display card reads them.
//! We model the "dual-ported" nature simply: it's a plain RAM region that the CPU writes and
//! the host UI reads — no real concurrency, so no dual-port machinery is needed.
//!
//! Crucially, the device raises a periodic **vertical-retrace interrupt**. CPT system software
//! queues scroll/screen actions and executes them in the retrace handler, so without this IRQ
//! the screen would never scroll or update. The emulator does not scroll anything itself — it
//! provides the RAM and the retrace IRQ, and the real CPT retrace handler does the work.
//!
//! Font/glyph rendering is NOT handled here; the device exposes character codes and the UI
//! rasterizes them through the display board's font ROM (a later layer).

use crate::device::Device;
use std::any::Any;

/// Columns across the display. [CONFIRMED: 80]
pub const COLS: usize = 80;

/// Total rows in the display frame. ~60; exact value [VERIFY].
pub const ROWS: usize = 60;

/// Total bytes of display RAM (one character code per cell). Attributes, if any, are [VERIFY].
pub const DISPLAY_RAM_SIZE: usize = COLS * ROWS;

/// The structural regions of the screen, top to bottom. Row counts are placeholders pending
/// the system source; the structure (small bottom region, large top region, typing line near
/// the bottom) is confirmed. [VERIFY exact splits]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Row of the single title line.
    pub title_row: usize,
    /// First and last row (inclusive) of the top-half text region.
    pub top_text: (usize, usize),
    /// Row of the typing-line ruler.
    pub typing_row: usize,
    /// First and last row (inclusive) of the (small, ~8-line) bottom-half text region.
    pub bottom_text: (usize, usize),
    /// First and last row (inclusive) of the status lines.
    pub status: (usize, usize),
}

impl Default for Layout {
    /// Placeholder layout: title(0) / top(1..=44) / typing(45) / bottom(46..=53) / status(54..=55).
    /// The bottom region is intentionally small (~8 lines) per the hardware. [VERIFY]
    fn default() -> Self {
        Layout {
            title_row: 0,
            top_text: (1, 44),
            typing_row: 45,
            bottom_text: (46, 53), // ~8 lines
            status: (54, 55),
        }
    }
}

/// A snapshot of the display for the host UI to render: raw character codes plus the layout so
/// the UI knows which rows are title / text / ruler / status.
pub struct PageSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub layout: Layout,
    /// Row-major character codes, `rows * cols` bytes.
    pub cells: Vec<u8>,
}

impl PageSnapshot {
    /// The character code at (row, col).
    pub fn cell(&self, row: usize, col: usize) -> u8 {
        self.cells[row * self.cols + col]
    }

    /// Render the frame as human-readable text for a terminal, with labeled region dividers so
    /// the title / top-half / typing-line / bottom-half / status structure is visible. Non-
    /// printable character codes render as '.'; code 0 renders as space (blank cell).
    pub fn render_text(&self) -> String {
        let l = self.layout;
        let mut out = String::new();
        let rule = |out: &mut String, label: &str| {
            out.push_str(&format!("     +{:-<width$}+  {}\n", "", label, width = self.cols));
        };

        rule(&mut out, "title");
        for row in 0..self.rows {
            // Insert region dividers at the boundaries.
            if row == l.top_text.0 {
                rule(&mut out, "top-half text");
            } else if row == l.typing_row {
                rule(&mut out, "TYPING LINE");
            } else if row == l.bottom_text.0 {
                rule(&mut out, "bottom-half text");
            } else if row == l.status.0 {
                rule(&mut out, "status");
            }

            let line: String = (0..self.cols)
                .map(|c| {
                    let ch = self.cell(row, c);
                    match ch {
                        0 => ' ',
                        0x20..=0x7E => ch as char,
                        _ => '.',
                    }
                })
                .collect();
            out.push_str(&format!("{:>3}: |{}|\n", row, line));
        }
        rule(&mut out, "");
        out
    }
}

pub struct DisplayDevice {
    /// Character RAM, row-major, `ROWS * COLS` bytes.
    ram: Vec<u8>,
    layout: Layout,

    // --- vertical-retrace interrupt ---
    retrace_period: u32,
    retrace_vector: u8,
    cycle_accum: u32,
    retrace_pending: bool,
    /// Count of retrace interrupts *raised* (consumed via `take_irq`). Note this counts IRQs
    /// the device requested, not necessarily ones the CPU serviced: if the retrace handler runs
    /// longer than one retrace period, interrupts raised while it executes (with interrupts
    /// disabled) are not accepted by the CPU, so serviced scrolls can be fewer than this count.
    pub retrace_count: u64,
}

impl DisplayDevice {
    pub fn new(retrace_period: u32, retrace_vector: u8) -> Self {
        DisplayDevice {
            ram: vec![0; DISPLAY_RAM_SIZE],
            layout: Layout::default(),
            retrace_period,
            retrace_vector,
            cycle_accum: 0,
            retrace_pending: false,
            retrace_count: 0,
        }
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Write a character code at a RAM offset (used by the Machine's memory-mapped routing).
    pub fn write_cell(&mut self, offset: usize, val: u8) {
        if offset < self.ram.len() {
            self.ram[offset] = val;
        }
    }

    /// Read a character code at a RAM offset.
    pub fn read_cell(&self, offset: usize) -> u8 {
        self.ram.get(offset).copied().unwrap_or(0)
    }

    /// Size of the display RAM (so the Machine can size its address window).
    pub fn ram_len(&self) -> usize {
        self.ram.len()
    }

    /// Produce a snapshot for the UI to render.
    pub fn snapshot(&self) -> PageSnapshot {
        PageSnapshot {
            cols: COLS,
            rows: ROWS,
            layout: self.layout,
            cells: self.ram.clone(),
        }
    }
}

impl Device for DisplayDevice {
    // The display RAM is memory-mapped, not port-mapped, so it claims no I/O ports. The Machine
    // routes its address window to `write_cell`/`read_cell` directly.
    fn ports(&self) -> &[u8] {
        &[]
    }
    fn io_in(&mut self, _port: u8) -> u8 {
        0
    }
    fn io_out(&mut self, _port: u8, _val: u8) {}

    /// Accumulate cycles; raise a retrace IRQ each time a frame's worth elapses.
    fn tick(&mut self, cycles: u32) {
        self.cycle_accum += cycles;
        while self.cycle_accum >= self.retrace_period {
            self.cycle_accum -= self.retrace_period;
            self.retrace_pending = true;
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        if self.retrace_pending {
            self.retrace_pending = false;
            self.retrace_count += 1;
            Some(self.retrace_vector)
        } else {
            None
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

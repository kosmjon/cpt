//! The CPT's printer port: a parallel daisy-wheel interface. Printer A is at 42H/43H (printer B
//! would be at 44H/45H; a workstation drives one printer directly).
//!
//! What the software shows [CONFIRMED by disassembly and by running cpt.img; the meaning of
//! each line is HYPOTHESIS, read from how the driver uses it]:
//! * **42H (out): the control register.** Idle it holds 41H. A command is a pulse: the driver
//!   (9458H on cpt.img) writes `(strobe AND BEH) OR 41H`, then 41H again, with interrupts off.
//!   Strobes seen: 80H at the start of a print (probably *restore*), 08H after each character
//!   (the character strobe), 02H and 04H (carriage / paper feed, which is which is open).
//! * **43H (out): the data latch.** Before a strobe the driver writes a 12-bit word as two
//!   bytes, high then low: for a character, its print-wheel position from the wheel table
//!   (9632H or A60DH), with 08H added to the high byte for bold [HYPOTHESIS].
//! * **43H (in): status.** Bit 7 is waited for after restore (printer ready); bit 6 before each
//!   character (wheel ready); bits 4 and 2 before other commands; bit 0 is read after strobe
//!   04H. With no printer the port reads 00H and the CPT says "THE PRINTER SEEMS TO BE OFF."
//!   (message 8.51).
//!
//! The model is a printer that is always ready: it reads `status` (all ready bits by default)
//! and logs every strobe with the data word latched before it. Nothing is timed.

use crate::device::Device;
use std::any::Any;
use std::collections::{BTreeMap, HashMap};

/// The control register's idle value: bits 6 and 0 stay set (select lines [HYPOTHESIS]).
pub const CONTROL_IDLE: u8 = 0x41;

/// The strobe bits a command pulse can carry (the driver masks with BEH).
pub const STROBE_MASK: u8 = 0xBE;

/// Strobes as the driver uses them.
pub const STROBE_RESTORE: u8 = 0x80;
pub const STROBE_CHARACTER: u8 = 0x08;
/// Carriage move: the word's low 10 bits in 1/60 inch, bit 10 (400H) set = to the left.
pub const STROBE_CARRIAGE: u8 = 0x10;
/// Paper feed: the word's low 10 bits in 1/48 inch (8 = one line at 6 lines per inch).
pub const STROBE_PAPER: u8 = 0x20;

/// Horizontal units per character column (1/60 inch; pica, 10 per inch).
pub const UNITS_PER_COLUMN: i32 = 6;
/// Vertical units per text line (1/48 inch; 6 lines per inch).
pub const UNITS_PER_LINE: i32 = 8;

/// Status bit 7: the printer is on and ready (waited for after restore).
pub const STATUS_ON: u8 = 0x80;

/// The status a ready printer reports: ready (7), wheel ready (6), and bits 5, 4 and 2 (the
/// other "ready" lines the driver waits on). Bits 1 and 0 are left clear.
pub const STATUS_READY: u8 = 0xF4;

/// Status bit 1 flips when a new sheet is loaded. At the end of a page the CPT shows "PLEASE
/// PREPARE THE PRINTER FOR A NEW PAGE." (8.57) and waits, polling every 0.1 s, for this bit to
/// differ from what it was when the wait began (A219H on cpt.img) [CONFIRMED by running;
/// the switch it stands for is HYPOTHESIS].
pub const STATUS_SHEET: u8 = 0x02;

/// With `auto_sheet`, this many status reads in a row with no command between them (2 s of
/// the CPT's 0.1 s polling) count as the CPT waiting for paper, and a sheet is loaded. While
/// printing, the CPT sends a command after every one to three reads.
pub const SHEET_IDLE_READS: u32 = 20;

/// The "strobe" logged when a sheet is loaded (not a real strobe; [`render_text`] starts a
/// new page there).
pub const SHEET_LOADED: u8 = 0x00;

/// At most this many commands are kept (a long document is a few thousand).
const LOG_LIMIT: usize = 1_000_000;

/// One command to the printer: the strobe pulsed and the data word latched before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrinterCommand {
    pub strobe: u8,
    pub word: u16,
}

pub struct CptPrinter {
    ports: [u8; 2],
    base: u8,
    control: u8,
    data: [u8; 2],
    /// What 43H reads. [`STATUS_READY`] by default; 00H is "no printer".
    pub status: u8,
    /// Every command, in order.
    pub log: Vec<PrinterCommand>,
    /// Status reads (for reports).
    pub status_reads: u64,
    /// Load the next sheet by itself when the CPT waits for one (see [`SHEET_IDLE_READS`]).
    /// On by default, like an operator who always feeds the next page.
    pub auto_sheet: bool,
    idle_reads: u32,
    /// Any command received yet (so there is a page to change; the log may have been taken).
    printed: bool,
    /// Sheets loaded (by `auto_sheet` or [`CptPrinter::load_sheet`]).
    pub sheets: u64,
}

impl CptPrinter {
    /// A printer with its control register at `base` and data / status at `base + 1`.
    pub fn new(base: u8) -> Self {
        CptPrinter {
            ports: [base, base + 1],
            base,
            control: CONTROL_IDLE,
            data: [0; 2],
            status: STATUS_READY,
            log: Vec::new(),
            status_reads: 0,
            auto_sheet: true,
            idle_reads: 0,
            printed: false,
            sheets: 0,
        }
    }

    /// Load a new sheet: flip the sheet line (status bit 1) and log [`SHEET_LOADED`].
    pub fn load_sheet(&mut self) {
        self.status ^= STATUS_SHEET;
        self.sheets += 1;
        if self.log.len() < LOG_LIMIT {
            self.log.push(PrinterCommand { strobe: SHEET_LOADED, word: 0 });
        }
    }

    /// Printer A at 42H/43H.
    pub fn cpt() -> Self {
        CptPrinter::new(0x42)
    }

    /// The commands logged since the last call.
    pub fn take_log(&mut self) -> Vec<PrinterCommand> {
        std::mem::take(&mut self.log)
    }
}

impl Device for CptPrinter {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        if port == self.base + 1 {
            self.status_reads += 1;
            let s = self.status;
            if self.auto_sheet && self.status & STATUS_ON != 0 && self.printed {
                self.idle_reads += 1;
                if self.idle_reads >= SHEET_IDLE_READS {
                    self.idle_reads = 0;
                    self.load_sheet();
                }
            }
            s
        } else {
            self.control
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        self.idle_reads = 0;
        if port == self.base {
            // A strobe is a line going high.
            let rising = val & !self.control & STROBE_MASK;
            self.printed |= rising != 0;
            if rising != 0 && self.log.len() < LOG_LIMIT {
                let word = u16::from_be_bytes(self.data);
                self.log.push(PrinterCommand { strobe: rising, word });
            }
            self.control = val;
        } else {
            self.data = [self.data[1], val];
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The print-wheel table the driver translates characters with, turned around: which character
/// each character word strikes. The table is in the driver's block of memory; its first byte
/// is its format [CONFIRMED by disassembly, cpt.img 8F94H–9022H]:
/// * 1: one byte per character code (the word's low byte).
/// * 2: two bytes (low, high) per code from 20H.
/// * 3: a count, then two bytes per code from 20H; a word of 0 means "not on this wheel".
/// * anything else: it is the first code, then a count, then two bytes per code, sent
///   complemented; FFFFH (stored 0000H) means "not on this wheel". Both program disks use this
///   one, from 20H, with 90H codes.
///
/// Codes F0H and up are looked up as code − 50H, so a code is reported in the 20H–AFH range.
#[derive(Debug, Clone, Default)]
pub struct PrintWheel {
    chars: HashMap<u16, u8>,
}

impl PrintWheel {
    /// Read a table starting at `t[0]` (the format byte).
    pub fn from_table(t: &[u8]) -> Option<PrintWheel> {
        let mut chars = HashMap::new();
        let mut add = |word: u16, code: u8| {
            chars.entry(word).or_insert(code);
        };
        let word_at = |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*t.get(i)?, *t.get(i + 1)?])) };
        match *t.first()? {
            0 => return None,
            1 => {
                for code in 0x20..=0xAFu8 {
                    add(*t.get(code as usize)? as u16, code);
                }
            }
            2 => {
                for i in 0..0x90usize {
                    add(word_at(2 + 2 * i)?, 0x20 + i as u8);
                }
            }
            3 => {
                for i in 0..*t.get(1)? as usize {
                    let w = word_at(2 + 2 * i)?;
                    if w != 0 {
                        add(w, 0x20 + i as u8);
                    }
                }
            }
            first => {
                for i in 0..*t.get(1)? as usize {
                    let w = !word_at(2 + 2 * i)?;
                    if w != 0xFFFF {
                        add(w, first.wrapping_add(i as u8));
                    }
                }
            }
        }
        if chars.is_empty() {
            None
        } else {
            Some(PrintWheel { chars })
        }
    }

    /// Find the table in a 32K block of memory (`block[0]` is at 8000H) by the driver's code
    /// that loads it: `CALL 00EEH; LXI H,table; CPI 02H; JNZ` (cpt.img 8F84H, cpt2489 92F1H).
    pub fn find(block: &[u8]) -> Option<PrintWheel> {
        block.windows(10).find_map(|w| {
            if w[0] == 0xCD && w[1] == 0xEE && w[2] == 0x00 && w[3] == 0x21 && w[6] == 0xFE && w[7] == 0x02 && w[8] == 0xC2 {
                let addr = u16::from_le_bytes([w[4], w[5]]);
                let off = addr.checked_sub(0x8000)? as usize;
                PrintWheel::from_table(block.get(off..)?)
            } else {
                None
            }
        })
    }

    /// The character code a character word strikes. A word not in the table is tried with bit
    /// 11 flipped, which the driver sets for bold [HYPOTHESIS].
    pub fn code_for(&self, word: u16) -> Option<u8> {
        self.chars.get(&word).or_else(|| self.chars.get(&(word ^ 0x0800))).copied()
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

/// How a CPT character code prints in plain text: ASCII as itself, the required space (A0H)
/// and the coded space (90H) as spaces, anything else (the wheel's special characters) as `?`.
fn text_char(code: u8) -> char {
    match code {
        0x20..=0x7E => code as char,
        // The required space and the coded space print as spaces.
        0xA0 | 0x90 => ' ',
        _ => '?',
    }
}

/// Combining low line: follows each underlined character in [`render_text`]'s output.
pub const UNDERLINE: char = '\u{0332}';

/// One column of a printed line: the character struck there, and whether it is underlined.
#[derive(Clone, Copy)]
struct PrintCell {
    ch: char,
    underline: bool,
}

const BLANK: PrintCell = PrintCell { ch: ' ', underline: false };

/// Lay the printer's commands out as plain text pages: each character goes in the column and
/// line its carriage and paper position fall in (6 units per column, 8 per line). A restore
/// after something was printed starts a new page; pages are separated by a form feed (0CH).
///
/// Underlining: the CPT prints it as an underscore struck at each underlined position,
/// spaces included (JP-M24's `M E M O R A N D U M`). An underscore struck over a character,
/// or a character struck over an underscore, marks that column underlined, and the output has
/// a combining low line ([`UNDERLINE`], U+0332) after it, so the underline is continuous; an
/// underscore on a blank column is an underlined space. Any other overstrike that lands on a
/// used column moves to the next free one.
pub fn render_text(log: &[PrinterCommand], wheel: &PrintWheel) -> String {
    let mut pages: Vec<BTreeMap<i32, Vec<PrintCell>>> = Vec::new();
    let mut page: BTreeMap<i32, Vec<PrintCell>> = BTreeMap::new();
    let (mut x, mut y) = (0i32, 0i32);
    let signed = |w: u16| {
        let n = (w & 0x3FF) as i32;
        if w & 0x400 != 0 { -n } else { n }
    };
    for c in log {
        if c.strobe == SHEET_LOADED || c.strobe & STROBE_RESTORE != 0 {
            if !page.is_empty() {
                pages.push(std::mem::take(&mut page));
            }
            y = 0;
            if c.strobe != SHEET_LOADED {
                x = 0;
            }
            continue;
        }
        if c.strobe & STROBE_CARRIAGE != 0 {
            x = (x + signed(c.word)).max(0);
        }
        if c.strobe & STROBE_PAPER != 0 {
            y = (y + signed(c.word)).max(0);
        }
        if c.strobe & STROBE_CHARACTER != 0 {
            let ch = wheel.code_for(c.word).map(text_char).unwrap_or('?');
            let row = (y + UNITS_PER_LINE / 2) / UNITS_PER_LINE;
            let mut col = ((x + UNITS_PER_COLUMN / 2) / UNITS_PER_COLUMN) as usize;
            let line = page.entry(row).or_default();
            if ch == ' ' {
                continue;
            }
            if line.len() <= col {
                line.resize(col + 1, BLANK);
            }
            if ch == '_' {
                // Underlining: whatever is in this column (a character, or nothing yet).
                line[col].underline = true;
                continue;
            }
            loop {
                if line.len() <= col {
                    line.resize(col + 1, BLANK);
                }
                let here = line[col].ch;
                if here == ' ' {
                    line[col].ch = ch;
                    break;
                }
                if here == ch {
                    break; // struck again (bold): the same character
                }
                col += 1;
            }
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push('\u{0C}');
        }
        let last = page.keys().next_back().copied().unwrap_or(0);
        for row in 0..=last {
            if let Some(line) = page.get(&row) {
                // Trailing blanks go, but not underlined ones (they are part of the underline).
                let end = line.iter().rposition(|c| c.ch != ' ' || c.underline).map_or(0, |i| i + 1);
                for cell in &line[..end] {
                    out.push(cell.ch);
                    if cell.underline {
                        out.push(UNDERLINE);
                    }
                }
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complemented table from 20H: space, '!', '"' with entries 0001, 0002, 0003.
    fn wheel() -> PrintWheel {
        PrintWheel::from_table(&[0x20, 3, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00]).unwrap()
    }

    #[test]
    fn a_strobe_logs_the_word_latched_before_it() {
        let mut p = CptPrinter::cpt();
        assert_eq!(p.io_in(0x43), STATUS_READY);
        p.io_out(0x43, 0xF9);
        p.io_out(0x43, 0xC1);
        p.io_out(0x42, CONTROL_IDLE | STROBE_CHARACTER);
        p.io_out(0x42, CONTROL_IDLE);
        assert_eq!(p.take_log(), vec![PrinterCommand { strobe: STROBE_CHARACTER, word: 0xF9C1 }]);
        p.io_out(0x42, CONTROL_IDLE | STROBE_RESTORE);
        p.io_out(0x42, CONTROL_IDLE | STROBE_RESTORE);
        assert_eq!(p.log.len(), 1, "held high: one pulse");
    }

    #[test]
    fn a_sheet_is_loaded_when_the_cpt_polls_without_printing() {
        let mut p = CptPrinter::cpt();
        for _ in 0..SHEET_IDLE_READS * 2 {
            p.io_in(0x43);
        }
        assert_eq!(p.sheets, 0, "nothing printed yet: no page to change");
        p.io_out(0x42, CONTROL_IDLE | STROBE_RESTORE);
        p.io_out(0x42, CONTROL_IDLE);
        for _ in 0..SHEET_IDLE_READS - 1 {
            assert_eq!(p.io_in(0x43) & STATUS_SHEET, 0);
            p.io_in(0x43);
            p.io_out(0x43, 0); // printing: a command between reads
        }
        assert_eq!(p.sheets, 0);
        for _ in 0..SHEET_IDLE_READS {
            p.io_in(0x43);
        }
        assert_eq!(p.sheets, 1);
        assert_eq!(p.io_in(0x43) & STATUS_SHEET, STATUS_SHEET, "the sheet line flipped");
        assert_eq!(p.log.last().unwrap().strobe, SHEET_LOADED);
        p.auto_sheet = false;
        for _ in 0..SHEET_IDLE_READS * 2 {
            p.io_in(0x43);
        }
        assert_eq!(p.sheets, 1, "auto_sheet off: only load_sheet loads one");
        p.load_sheet();
        assert_eq!(p.io_in(0x43) & STATUS_SHEET, 0);

        let mut off = CptPrinter::cpt();
        off.status = 0;
        off.io_out(0x42, CONTROL_IDLE | STROBE_RESTORE);
        for _ in 0..SHEET_IDLE_READS * 3 {
            assert_eq!(off.io_in(0x43), 0, "a printer that is off loads no paper");
        }
    }

    #[test]
    fn the_complemented_table_maps_words_back_to_codes() {
        let w = wheel();
        assert_eq!(w.code_for(0xFFFE), Some(0x20));
        assert_eq!(w.code_for(0xFFFC), Some(0x22));
        assert_eq!(w.code_for(0xF7FD), Some(0x21), "bit 11 flipped (bold)");
        assert_eq!(w.code_for(0x1234), None);
    }

    #[test]
    fn find_uses_the_drivers_reference_to_the_table() {
        let mut block = vec![0u8; 0x200];
        block[0x10..0x1A].copy_from_slice(&[0xCD, 0xEE, 0x00, 0x21, 0x00, 0x81, 0xFE, 0x02, 0xC2, 0x00]);
        block[0x100..0x108].copy_from_slice(&[0x20, 3, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00]);
        assert_eq!(PrintWheel::find(&block).unwrap().code_for(0xFFFD), Some(0x21));
    }

    #[test]
    fn underscores_struck_over_text_underline_it_continuously() {
        // A wheel with 'A' (0001), ' ' (0002) and '_' (0003) at codes 41H, 20H, 5FH.
        let mut t = vec![0x20u8, 0x40];
        for code in 0x20..0x60u8 {
            let w: u16 = match code { 0x41 => 1, 0x20 => 2, 0x5F => 3, _ => 0 };
            t.extend_from_slice(&w.to_le_bytes());
        }
        let w = PrintWheel::from_table(&t).unwrap();
        let ch = |word: u16| PrinterCommand { strobe: STROBE_CHARACTER, word: !word };
        let car = |word| PrinterCommand { strobe: STROBE_CARRIAGE, word };
        // "A A" struck, back to the start, then "___" over it (the spaces too), then a plain "A".
        let log = vec![
            ch(1), car(6), car(6), ch(1), car(6),
            car(0x400 | 18), ch(3), car(6), ch(3), car(6), ch(3), car(6), car(6), ch(1),
        ];
        let u = UNDERLINE;
        assert_eq!(render_text(&log, &w), format!("A{u} {u}A{u} A\n"));
        // An underscore on its own is an underlined blank, kept at the end of the line.
        assert_eq!(render_text(&[ch(1), car(6), ch(3)], &w), format!("A {u}\n"));
    }

    #[test]
    fn render_places_characters_by_carriage_and_paper_position() {
        let w = wheel();
        let ch = |word| PrinterCommand { strobe: STROBE_CHARACTER, word };
        let car = |word| PrinterCommand { strobe: STROBE_CARRIAGE, word };
        let feed = |word| PrinterCommand { strobe: STROBE_PAPER, word };
        let log = vec![
            PrinterCommand { strobe: STROBE_RESTORE, word: 0 },
            car(12), ch(0xFFFD), car(6), ch(0xFFFC),
            feed(16), car(0x400 | 18), ch(0xFFFD),
            PrinterCommand { strobe: STROBE_RESTORE, word: 0 },
            ch(0xFFFC),
        ];
        assert_eq!(render_text(&log, &w), "  !\"\n\n!\n\u{0C}\"\n");
    }
}

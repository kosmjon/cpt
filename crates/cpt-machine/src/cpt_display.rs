//! Decoder for the real CPT display list (the "full page" screen built by the program disk).
//!
//! The display hardware walks a byte stream in bank 0 on its own; the CPU only edits it (and
//! queues 2-byte patches for the retrace handler at 0D5CH to apply). The format below was
//! read out of the real system code; each rule notes where it came from. Rules marked
//! [HYPOTHESIS] are inferred from how the software uses a code, not from the hardware.
//!
//! The screen is black on white. Per the user, the title row, the ruler and the status row are
//! white on black, and the document text and the typing line (just above the ruler) are black
//! on white.
//!
//! Stream format:
//! - `00`-`AF`: a visible character, one column each. `20`-`7E` are ASCII; `A0` is a required
//!   (non-breaking) space (the printer path at 1A38H turns it into 20H); `80`-`8F` are special
//!   glyphs from the character ROM: the ruler uses `80` (column tick), `81` (every 5th), `82`
//!   (every 10th), `83` (a margin), `84` (a tab stop) and `87` (the cursor's column) (builder
//!   at E4E0H; confirmed by typing). `89` is treated as a tab/word break by the word code at
//!   863DH [HYPOTHESIS: a tab character in text].
//! - `0D`: end of row.
//! - `B0`-`FF`: zero width (the software's own walker at 8580H/860AH never counts them as a
//!   column), except the structure codes below.
//! - `B0`: a per-character attribute slot: the next character is white on black. The title and
//!   status rows have one before every character (the title two), and text rows have none.
//!   [HYPOTHESIS, chosen because it gives the user's reverse rows.]
//! - `80`-`8F` (the ruler glyphs) are drawn white on black by the character ROM itself
//!   [HYPOTHESIS: the ruler has no slots but is reverse on the real screen].
//! - `B1`: end of the screen (the walker at 8580H substitutes B1 for a link to 0000H).
//! - `B2`: filler (skipped like B0 by 8580H; pads the 16-byte cells below the status row).
//! - `B3`: starts a document's format line (its own ruler, drawn with the ruler glyphs)
//!   [HYPOTHESIS: seen only there, in the letter RC-L1].
//! - `BD`: comes before a hyphen at a line break in document text (`con` `BD` `-` `0D`)
//!   [HYPOTHESIS: marks a hyphen the system added, as opposed to one typed].
//! - `B4`/`B5`: half-line down/up (sub/superscript; the printer path at 9C64H moves +2/-2).
//! - `C0`: starts every row and ends every status message.
//! - `C1 xx`: horizontal scroll for the rows that follow: the first visible column is
//!   `NOT xx` (routine BAC6H steps it to keep the cursor in an 80-column window). `C1 FF`
//!   (no scroll) brackets the title and status rows, which never scroll.
//! - `C2 lo hi`: jump to `hi lo`; a jump to 0000H ends the screen.
//! - `C3 a a b b`: a disarmed jump (E6BAH flips C2 <-> C3 to hide/show an optional row);
//!   display continues at `b b` (which the builder sets to the byte after it).
//! - `C4`: the cursor: the blink routine at 93C0H swaps a B0 slot with C4 and back. The next
//!   character is shown inverted, so on a white-on-black row it is one black-on-white cell.

/// Columns in the visible window (the builder writes 80-column rows; BAC6H scrolls by 50H).
pub const WINDOW_COLS: usize = 80;

/// One visible character cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The character ROM code (00-AF).
    pub code: u8,
    /// White on black (the screen's normal is black on white).
    pub reverse: bool,
}

/// Why the walk stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// A `B1` end mark (or a jump to 0000H) at this address.
    EndMark(u16),
    /// A jump back to an address already jumped to: the filler loop after the last row.
    Loop(u16),
    /// The row limit was reached.
    RowLimit,
    /// The byte limit was reached (a malformed or unterminated list).
    ByteLimit,
}

/// A decoded screen: rows of visible cells (already clipped to the scroll window).
#[derive(Clone, Debug)]
pub struct CptScreen {
    pub rows: Vec<Vec<Cell>>,
    pub ending: Ending,
    /// Codes met that this decoder has no rule for (each listed once).
    pub unknown_codes: Vec<u8>,
}

/// Walk the display list starting at `head`, reading bank-0 memory through `read`.
pub fn render<F: FnMut(u16) -> u8>(mut read: F, head: u16, max_rows: usize) -> CptScreen {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut col = 0usize; // logical column within the row, before scrolling
    let mut scroll = 0usize;
    // Attributes for the next visible character, set by the slot codes before it.
    let mut slot_reverse = false;
    let mut cursor = false;
    // Whether this row uses attribute slots (B0/C4), i.e. is a white-on-black row.
    let mut row_has_slots = false;
    let mut unknown: Vec<u8> = Vec::new();
    let mut jumped_to: Vec<u16> = Vec::new();
    let mut addr = head;

    let ending = 'walk: {
        for _ in 0..0x10000 {
            let b = read(addr);
            let next = addr.wrapping_add(1);
            match b {
                0x0D => {
                    rows.push(std::mem::take(&mut row));
                    col = 0;
                    slot_reverse = false;
                    cursor = false;
                    row_has_slots = false;
                    if rows.len() >= max_rows {
                        break 'walk Ending::RowLimit;
                    }
                }
                0xB0 => {
                    slot_reverse = true;
                    row_has_slots = true;
                }
                0xC4 => cursor = true,
                0xC0 => {}
                0xB1 => break 'walk Ending::EndMark(addr),
                0xC1 => {
                    scroll = (!read(next)) as usize;
                    addr = addr.wrapping_add(2);
                    continue;
                }
                0xC2 | 0xC3 => {
                    let at = if b == 0xC2 { next } else { addr.wrapping_add(3) };
                    let to = read(at) as u16 | (read(at.wrapping_add(1)) as u16) << 8;
                    if to == 0 {
                        break 'walk Ending::EndMark(addr);
                    }
                    if jumped_to.contains(&to) {
                        break 'walk Ending::Loop(to);
                    }
                    jumped_to.push(to);
                    addr = to;
                    continue;
                }
                0xB2..=0xBF | 0xC5..=0xFF => {
                    let known = matches!(b, 0xB2 | 0xB3 | 0xB4 | 0xB5 | 0xBD);
                    if !known && !unknown.contains(&b) {
                        unknown.push(b);
                    }
                }
                _ => {
                    let rom_inverse = (0x80..=0x8F).contains(&b);
                    // The cursor cell is the opposite of its row: black on white in a slotted
                    // (white-on-black) row, white on black in a text row.
                    let reverse = if cursor { !row_has_slots } else { slot_reverse || rom_inverse };
                    if col >= scroll && col < scroll + WINDOW_COLS {
                        row.push(Cell { code: b, reverse });
                    }
                    slot_reverse = false;
                    cursor = false;
                    col += 1;
                }
            }
            addr = next;
        }
        Ending::ByteLimit
    };
    if !row.is_empty() {
        rows.push(row);
    }
    CptScreen { rows, ending, unknown_codes: unknown }
}

/// A printable stand-in for a character ROM code. The CPT glyph shapes are unknown; the ruler
/// and tab codes get look-alike ASCII so the screen reads naturally.
pub fn glyph(code: u8) -> char {
    match code {
        0x20..=0x7E => code as char,
        0xA0 => ' ',
        0x80 => '.', // ruler: column tick
        0x81 => ':', // ruler: every 5th column
        0x82 => '|', // ruler: every 10th column
        0x83 => 'M', // ruler: a margin (left and right)
        0x84 => '>', // ruler: a tab stop
        0x87 => '^', // ruler: the cursor's column (moves as you type)
        0x89 => '>', // [HYPOTHESIS] a tab in text
        _ => '?',
    }
}

impl CptScreen {
    /// The screen as plain text, one line per row, each padded to the window width.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for row in &self.rows {
            let line: String = row.iter().map(|c| glyph(c.code)).collect();
            s.push_str(&format!("{:<w$}\n", line, w = WINDOW_COLS));
        }
        s
    }

    /// The screen with ANSI colours: black on white for normal cells, white on black for
    /// reverse ones, as on the CPT's own display. Each row is padded to the window width.
    pub fn to_ansi(&self) -> String {
        const NORMAL: &str = "\x1b[30;47m";
        const REVERSE: &str = "\x1b[37;40m";
        let mut s = String::new();
        for row in &self.rows {
            let mut cur: Option<bool> = None;
            for c in row {
                if cur != Some(c.reverse) {
                    s.push_str(if c.reverse { REVERSE } else { NORMAL });
                    cur = Some(c.reverse);
                }
                s.push(glyph(c.code));
            }
            // Pad: a row's colour carries on to its right edge on the real screen.
            let pad_reverse = row.last().map(|c| c.reverse).unwrap_or(false);
            if row.len() < WINDOW_COLS {
                s.push_str(if pad_reverse { REVERSE } else { NORMAL });
                s.push_str(&" ".repeat(WINDOW_COLS - row.len()));
            }
            s.push_str("\x1b[0m\n");
        }
        s
    }

    /// Runs of reverse-video cells as (row, first column, last column).
    pub fn reverse_runs(&self) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        for (r, row) in self.rows.iter().enumerate() {
            let mut start: Option<usize> = None;
            for (c, cell) in row.iter().enumerate() {
                match (cell.reverse, start) {
                    (true, None) => start = Some(c),
                    (false, Some(s)) => {
                        out.push((r, s, c - 1));
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                out.push((r, s, row.len() - 1));
            }
        }
        out
    }

    /// Whether row `r` is all white on black (and not empty).
    pub fn row_is_reverse(&self, r: usize) -> bool {
        self.rows.get(r).map_or(false, |row| !row.is_empty() && row.iter().all(|c| c.reverse))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a list placed at `base` in an otherwise-zero 64K space.
    fn run(base: u16, bytes: &[u8], head: u16) -> CptScreen {
        let mut mem = vec![0u8; 0x10000];
        mem[base as usize..base as usize + bytes.len()].copy_from_slice(bytes);
        render(|a| mem[a as usize], head, 100)
    }

    fn text(s: &CptScreen, r: usize) -> String {
        s.rows[r].iter().map(|c| glyph(c.code)).collect()
    }

    fn rev(s: &CptScreen, r: usize) -> Vec<bool> {
        s.rows[r].iter().map(|c| c.reverse).collect()
    }

    #[test]
    fn rows_links_and_end_mark() {
        // head: C2 -> 0x1010; row "AB"; C0 0D blank row; B1 end.
        let mut l = vec![0xC2, 0x10, 0x10];
        l.resize(0x10, 0);
        l.extend([0xC0, b'A', b'B', 0x0D, 0xC0, 0x0D, 0xB1, 0x0D]);
        let s = run(0x1000, &l, 0x1000);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(text(&s, 0), "AB");
        assert_eq!(text(&s, 1), "");
        assert_eq!(s.ending, Ending::EndMark(0x1016));
    }

    #[test]
    fn jump_to_zero_ends_and_loops_are_detected() {
        let s = run(0x2000, &[b'X', 0x0D, 0xC2, 0x00, 0x00], 0x2000);
        assert_eq!(s.ending, Ending::EndMark(0x2002));
        // C0 0D C2 -> itself: the filler loop at 7FFBH.
        let s = run(0x7FFB, &[0xC0, 0x0D, 0xC2, 0xFB, 0x7F], 0x7FFB);
        assert_eq!(s.ending, Ending::Loop(0x7FFB));
        assert_eq!(s.rows.len(), 2); // the row once, then once more before the repeat is seen
    }

    #[test]
    fn disarmed_jump_continues_at_second_pointer() {
        // C3 <skip-to> <continue-at>: continue at 0x3005 (the byte after it).
        let s = run(0x3000, &[0xC3, 0x40, 0x30, 0x05, 0x30, b'S', 0x0D, 0xB1], 0x3000);
        assert_eq!(text(&s, 0), "S");
        // Armed (C2) it jumps over the row.
        let mut l = vec![0xC2, 0x40, 0x30, 0x05, 0x30, b'S', 0x0D];
        l.resize(0x40, 0);
        l.extend([b'T', 0x0D, 0xB1]);
        let s = run(0x3000, &l, 0x3000);
        assert_eq!(text(&s, 0), "T");
    }

    #[test]
    fn c1_scrolls_the_window() {
        // C1 FD: first visible column = NOT FD = 2.
        let s = run(0x4000, &[0xC1, 0xFD, b'a', b'b', b'c', b'd', 0x0D, 0xB1], 0x4000);
        assert_eq!(text(&s, 0), "cd");
        let s = run(0x4000, &[0xC1, 0xFF, b'a', b'b', 0x0D, 0xB1], 0x4000);
        assert_eq!(text(&s, 0), "ab");
    }

    #[test]
    fn rows_are_clipped_to_the_window() {
        let mut l: Vec<u8> = (0..100).map(|i| b'0' + (i % 10) as u8).collect();
        l.extend([0x0D, 0xB1]);
        let s = run(0x4000, &l, 0x4000);
        assert_eq!(s.rows[0].len(), WINDOW_COLS);
    }

    #[test]
    fn slot_rows_are_reverse_and_text_rows_are_not() {
        // A status-style row (B0 before each character), a title-style row (two slots) and a
        // plain text row.
        let s = run(0x5000, &[
            0xC0, 0xB0, b'a', 0xB0, b'b', 0x0D,
            0xC0, 0xB0, 0xB0, b'T', 0xB0, 0xB0, b'i', 0x0D,
            0xC0, b'H', b'i', 0x0D, 0xB1,
        ], 0x5000);
        assert!(s.row_is_reverse(0));
        assert!(s.row_is_reverse(1));
        assert_eq!(rev(&s, 2), [false, false]);
        // A character in a slot row without its slot is black on white.
        let s = run(0x5000, &[0xC0, 0xB0, b'a', b'1', 0xB0, b'b', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [true, false, true]);
    }

    #[test]
    fn the_ruler_glyphs_are_white_on_black() {
        let s = run(0x6000, &[0xC0, 0x83, 0x80, 0x81, 0x87, 0x82, 0x84, 0x83, 0x0D, 0xB1], 0x6000);
        assert_eq!(text(&s, 0), "M.:^|>M");
        assert!(s.row_is_reverse(0));
    }

    #[test]
    fn the_cursor_is_one_inverted_cell() {
        // The blink routine swaps a B0 slot for C4: on a white-on-black row the cursor
        // character is black on white.
        let s = run(0x5000, &[0xC0, 0xB0, b'a', 0xC4, b'b', 0xB0, b'c', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [true, false, true]);
        // In the title's double slots, one swapped slot inverts the character too.
        let s = run(0x5000, &[0xC0, 0xB0, 0xB0, b'a', 0xC4, 0xB0, b'b', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [true, false]);
        // In a text row (no slots) a cursor cell is white on black.
        let s = run(0x5000, &[0xC0, b'a', 0xC4, b'b', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [false, true]);
    }

    #[test]
    fn ansi_output_uses_black_on_white_and_white_on_black() {
        let s = run(0x5000, &[0xC0, 0xB0, b'a', 0x0D, 0xC0, b'b', 0x0D, 0xB1], 0x5000);
        let a = s.to_ansi();
        assert!(a.contains("\x1b[37;40ma"), "reverse row: {:?}", a);
        assert!(a.contains("\x1b[30;47mb"), "normal row: {:?}", a);
    }

    #[test]
    fn zero_width_codes_and_unknown_codes() {
        let s = run(0x6000, &[0xC0, b'x', 0xB2, b'y', 0xB4, b'z', 0xA0, 0xD7, 0x0D, 0xB1], 0x6000);
        assert_eq!(text(&s, 0), "xyz ");
        assert_eq!(s.unknown_codes, vec![0xD7]);
    }
}

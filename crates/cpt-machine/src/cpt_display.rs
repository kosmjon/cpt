//! Decoder for the real CPT display list (the "full page" screen built by the program disk).
//!
//! The display hardware walks a byte stream in bank 0 on its own; the CPU only edits it (and
//! queues 2-byte patches for the retrace handler at 0D5CH to apply). The format below was
//! read out of the real system code; each rule notes where it came from. Rules marked
//! [HYPOTHESIS] are inferred from how the software uses a code, not from the hardware.
//!
//! Stream format:
//! - `00`-`AF`: a visible character, one column each. `20`-`7E` are ASCII; `A0` is a required
//!   (non-breaking) space (the printer path at 1A38H turns it into 20H); `80`-`8F` are special
//!   glyphs from the character ROM: the ruler uses `80` (column tick), `81` (every 5th), `82`
//!   (every 10th), `83` (a margin) and `87` (the cursor's column, which moves as you type)
//!   (builder at E4E0H; confirmed by typing). `84` and `89` are
//!   treated as tab/word breaks by the word code at 863DH [HYPOTHESIS: tab glyphs].
//! - `0D`: end of row.
//! - `B0`-`FF`: zero width (the software's own walker at 8580H/860AH never counts them as a
//!   column), except the structure codes below.
//! - `B0`: an empty per-character attribute slot; status rows have one before every
//!   character, the top line two. [HYPOTHESIS: it also ends the attribute it replaces, which
//!   is what makes the one-character cursor work.]
//! - `B1`: end of the screen (the walker at 8580H substitutes B1 for a link to 0000H).
//! - `B2`: filler (skipped like B0 by 8580H).
//! - `B4`/`B5`: half-line down/up (sub/superscript; the printer path at 9C64H moves +2/-2).
//! - `C0`: starts every row and ends every status message [HYPOTHESIS: attributes off].
//! - `C1 xx`: horizontal scroll for the rows that follow: the first visible column is
//!   `NOT xx` (routine BAC6H steps it to keep the cursor in an 80-column window).
//! - `C2 lo hi`: jump to `hi lo`; a jump to 0000H ends the screen.
//! - `C3 a a b b`: a disarmed jump (E6BAH flips C2 <-> C3 to hide/show an optional row);
//!   display continues at `b b` (which the builder sets to the byte after it).
//! - `C4`: the cursor: the blink routine at 93C0H flips a B0 slot to C4 and back.
//!   [HYPOTHESIS: C4-CF set attributes; bit 2 = reverse video (the cursor), bit 3 = underline.]

/// Columns in the visible window (the builder writes 80-column rows; BAC6H scrolls by 50H).
pub const WINDOW_COLS: usize = 80;

/// One visible character cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The character ROM code (00-AF).
    pub code: u8,
    pub reverse: bool,
    pub underline: bool,
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
    let mut reverse = false;
    let mut underline = false;
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
                    if rows.len() >= max_rows {
                        break 'walk Ending::RowLimit;
                    }
                }
                0xB0 | 0xC0 => {
                    reverse = false;
                    underline = false;
                }
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
                0xC4..=0xCF => {
                    reverse = b & 0x04 != 0;
                    underline = b & 0x08 != 0;
                }
                0xB2..=0xBF | 0xD0..=0xFF => {
                    let known = matches!(b, 0xB2 | 0xB4 | 0xB5);
                    if !known && !unknown.contains(&b) {
                        unknown.push(b);
                    }
                }
                _ => {
                    if col >= scroll && col < scroll + WINDOW_COLS {
                        row.push(Cell { code: b, reverse, underline });
                    }
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
        0x87 => '^', // ruler: the cursor's column (moves as you type)
        0x84 | 0x89 => '>', // [HYPOTHESIS] tab glyphs
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

    /// The screen with ANSI escapes for reverse video and underline (for a terminal).
    pub fn to_ansi(&self) -> String {
        let mut s = String::new();
        for row in &self.rows {
            let mut cur = (false, false);
            for c in row {
                if (c.reverse, c.underline) != cur {
                    s.push_str("\x1b[0m");
                    if c.reverse { s.push_str("\x1b[7m"); }
                    if c.underline { s.push_str("\x1b[4m"); }
                    cur = (c.reverse, c.underline);
                }
                s.push(glyph(c.code));
            }
            s.push_str("\x1b[0m");
            for _ in row.len()..WINDOW_COLS {
                s.push(' ');
            }
            s.push('\n');
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

    #[test]
    fn rows_links_and_end_mark() {
        // head: C2 -> 0x1010; row "AB"; C0 0D blank row; B1 end.
        let mut l = vec![0xC2, 0x10, 0x10];
        l.resize(0x10, 0);
        l.extend([0xC0, b'A', 0xB0, b'B', 0x0D, 0xC0, 0x0D, 0xB1, 0x0D]);
        let s = run(0x1000, &l, 0x1000);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(text(&s, 0), "AB");
        assert_eq!(text(&s, 1), "");
        assert_eq!(s.ending, Ending::EndMark(0x1017));
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
    fn cursor_reverses_one_character() {
        // Status-row style: a B0 slot before every character; the cursor slot is C4.
        let s = run(0x5000, &[0xC0, 0xB0, b'a', 0xC4, b'b', 0xB0, b'c', 0x0D, 0xB1], 0x5000);
        let rev: Vec<bool> = s.rows[0].iter().map(|c| c.reverse).collect();
        assert_eq!(rev, [false, true, false]);
        assert_eq!(s.reverse_runs(), vec![(0, 1, 1)]);
        assert!(s.to_ansi().contains("\x1b[7mb"));
    }

    #[test]
    fn zero_width_codes_and_ruler_glyphs() {
        let s = run(0x6000, &[0xC0, 0x87, 0x80, 0xB2, 0x81, 0xB4, 0x82, 0x83, 0xA0, 0xD7, 0x0D, 0xB1], 0x6000);
        assert_eq!(text(&s, 0), "^.:|M ");
        assert_eq!(s.unknown_codes, vec![0xD7]);
    }
}

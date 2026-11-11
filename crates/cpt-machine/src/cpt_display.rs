//! Decoder for the real CPT display list (the "full page" screen built by the program disk).
//!
//! The display hardware walks a byte stream in bank 0 on its own; the CPU only edits it (and
//! queues 2-byte patches for the retrace handler at 0D5CH to apply). The format below was
//! read out of the real system code; each rule notes where it came from. Rules marked
//! [HYPOTHESIS] are inferred from how the software uses a code, not from the hardware.
//!
//! The screen is black on white. Per the user, the title line and the ruler are white on black
//! with no black-on-white cells at all, the status (column, line, unit) shows at the right end
//! of the title line, and the document text and the typing line (just above the ruler) are
//! black on white.
//!
//! Stream format:
//! - `00`-`AF`: a visible character, one column each. `20`-`7E` are ASCII; `A0` is a required
//!   (non-breaking) space (the printer path at 1A38H turns it into 20H); `80`-`8F` are special
//!   glyphs from the character ROM: the ruler uses `80` (column tick), `81` (every 5th), `82`
//!   (every 10th), `83` (a margin), `84` (a tab stop) and `87` (the cursor's column) (builder
//!   at E4E0H; confirmed by typing). `89` is the **tab character in text**: the TAB key's
//!   insert (C891H-C8BDH, block 1) fills to the next tab stop or margin with A0 and ends with
//!   89 (84 in a format line). It never appears in a ruler or format line. It shows as a black
//!   half diamond pointing right (per the user), black on white, one column wide.
//!   `90` is a **coded space** (CODE + SPACE) [CONFIRMED by typing]: a space kept through
//!   justification, and one that keeps the words on either side together on a line. It shows
//!   as a small faint dot (per the user; the browser draws the inserted blank line's dot at
//!   mid-height and the coded space's a little lower).
//!   `8D` is a **coded return** [CONFIRMED by disassembly and by typing]: CODE + RETURN's
//!   handler (B657H, block 2) writes 8D and then ends the line, so the line ends `8D 0D`. On a
//!   document's first line (C0) or a margin line (B3) it writes no 8D (a plain RETURN). It
//!   shows as a black half diamond pointing left (per the user), one column wide.
//! - `0D`: end of row.
//! - `B0`-`FF`: zero width (the software's own walker at 8580H/860AH never counts them as a
//!   column), except the structure codes below.
//! - `B0`: a per-character attribute slot: plain (C4 in the same place underlines). A row
//!   whose first character has a slot (the title and status rows) is white on black from edge
//!   to edge, including the few characters that have no slot of their own. In document text
//!   slots appear only around underlined characters, and the row stays black on white.
//!   [HYPOTHESIS for how it is coded; the result is the user's.]
//! - Ruler rows are white on black: the main ruler (a C0 row of ruler glyphs 80-87) and a
//!   document's format line (a row with B3) [HYPOTHESIS: the ruler has no slots but is
//!   reverse on the real screen]. The glyphs themselves are not reverse: double spacing's
//!   **system-inserted blank line** is a lone `80` (the column tick) in a text row (`C2 80`),
//!   shown as a tiny, faint dot on a white line (per the user).
//! - `B1`: end of the screen (the walker at 8580H substitutes B1 for a link to 0000H).
//! - `B2`: filler (skipped like B0 by 8580H; pads the 16-byte cells below the status row).
//! - `B3`: starts a document's format line (its own ruler, drawn with the ruler glyphs)
//!   [HYPOTHESIS: seen only there, in the letter RC-L1].
//! - `BD`: comes before a hyphen at a line break in document text (`con` `BD` `-` `0D`)
//!   [HYPOTHESIS: marks a hyphen the system added, as opposed to one typed].
//! - `B4`/`B5`: half-line down/up (sub/superscript; the printer path at 9C64H moves +2/-2).
//! - `C0`: starts every row the system builds and ends every status message. Document lines
//!   (after a document's first) are separate buffers without it. A bare `C0 0D` is a filler
//!   row with no document text, for example above the start of the document. The screen shows
//!   those as empty white-on-black lines, except the typing line (the row above the main
//!   ruler), which stays black on white (per the user).
//! - `C1 xx`: horizontal scroll for the rows that follow: the first visible column is
//!   `NOT xx` (routine BAC6H steps it to keep the cursor in an 80-column window). `C1 FF`
//!   (no scroll) brackets the title and status rows, which never scroll.
//! - `C2 lo hi`: jump to `hi lo`; a jump to 0000H ends the screen.
//! - `C3 a a b b`: a disarmed jump (E6BAH flips C2 <-> C3 to hide/show an optional row);
//!   display continues at `b b` (which the builder sets to the byte after it). The optional
//!   row is the status row (column, line, unit). The screen shows its characters at the right
//!   end of the title row, so the decoder merges it there instead of adding a row (per the
//!   user; how the hardware does it is unknown).
//! - `C4`: **underline** the next character. A document's underlined text has a C4 before each
//!   character (JP-M24's `M E M O R A N D U M`), and B0 before the plain ones. The cursor and a
//!   menu's selection are the same attribute: the blink routine at 93C0H swaps a B0 slot with
//!   C4 and back, so they show as a blinking underline (per the user).

/// Columns in the visible window (the builder writes 80-column rows; BAC6H scrolls by 50H).
pub const WINDOW_COLS: usize = 80;

/// Rows on the screen: the title line, 45 rows through the typing line (04ABH = 45), the
/// ruler, and 7 rows below it (04ACH = 7). The display list can hold more rows of text after
/// those (the TOC's ran to 80 when it was below the ruler), but they are not on the screen.
pub const SCREEN_ROWS: usize = 54;

/// One visible character cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The character ROM code (00-AF).
    pub code: u8,
    /// White on black (the screen's normal is black on white).
    pub reverse: bool,
    /// Underlined (a C4 before it: underlined text, or the cursor / menu selection).
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
    /// The status row (column, line, unit) was merged into the right end of the title row.
    pub status_merged: bool,
}

/// Finish a row: a row with attribute slots is white on black from edge to edge; other rows
/// keep their per-cell colours (the ruler glyphs).
fn finish_row(row: &mut [Cell], has_slots: bool) {
    if has_slots {
        for cell in row.iter_mut() {
            cell.reverse = true;
        }
    }
}

/// The ruler glyphs (column ticks, margins, tab stops, the cursor's column).
fn is_ruler_glyph(code: u8) -> bool {
    (0x80..=0x87).contains(&code)
}

/// A row drawn white on black as a ruler: a document's format line (it has a B3), or the main
/// ruler (a C0 row of nothing but ruler glyphs, at least two). A ruler glyph anywhere else is
/// black on white: double spacing's system-inserted blank line is a single 80 (the column
/// tick) in a text row, which the CPT shows as a tiny dot on a white line (per the user).
fn is_ruler_row(row: &[Cell], c0: bool, b3: bool) -> bool {
    b3 || (c0 && row.len() >= 2 && row.iter().all(|c| is_ruler_glyph(c.code)))
}

/// Merge the status row into the title row: its characters (not its blanks) replace the
/// title's at the same columns, so the column, line and unit show at the title's right end.
fn merge_status(title: &mut Vec<Cell>, status: &[Cell]) {
    for (i, cell) in status.iter().enumerate() {
        if cell.code == 0x20 || cell.code == 0xA0 {
            continue;
        }
        while title.len() <= i {
            title.push(Cell { code: 0x20, reverse: true, underline: false });
        }
        title[i] = *cell;
    }
}

/// Walk the display list starting at `head`, reading bank-0 memory through `read`.
pub fn render<F: FnMut(u16) -> u8>(mut read: F, head: u16, max_rows: usize) -> CptScreen {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut col = 0usize; // logical column within the row, before scrolling
    let mut scroll = 0usize;
    // The next visible character is underlined (a C4 before it).
    let mut underline = false;
    // Whether this row uses attribute slots (B0), i.e. is a white-on-black row.
    let mut row_has_slots = false;
    // The row being built is the optional status row (reached through a disarmed C3 jump).
    let mut status_row = false;
    let mut status_merged = false;
    // The row being built has a C0 (rows the system builds have one; document lines after
    // their first do not).
    let mut row_c0 = false;
    // The row being built is a document's format line (it has a B3).
    let mut row_b3 = false;
    // Per finished row: (a bare C0 filler row, the main ruler: C0 then only ruler glyphs).
    let mut kinds: Vec<(bool, bool)> = Vec::new();
    let mut unknown: Vec<u8> = Vec::new();
    let mut jumped_to: Vec<u16> = Vec::new();
    let mut addr = head;

    let ending = 'walk: {
        for _ in 0..0x10000 {
            let b = read(addr);
            let next = addr.wrapping_add(1);
            match b {
                0x0D => {
                    let mut done = std::mem::take(&mut row);
                    let is_ruler = is_ruler_row(&done, row_c0, row_b3);
                    finish_row(&mut done, row_has_slots || is_ruler);
                    if status_row && !rows.is_empty() {
                        merge_status(&mut rows[0], &done);
                        status_merged = true;
                    } else {
                        // A bare `C0 0D`: a filler row, with no document text.
                        let filler = row_c0 && done.is_empty() && !row_has_slots;
                        let ruler = is_ruler && row_c0;
                        kinds.push((filler, ruler));
                        rows.push(done);
                    }
                    status_row = false;
                    col = 0;
                    underline = false;
                    row_has_slots = false;
                    row_c0 = false;
                    row_b3 = false;
                    if rows.len() >= max_rows {
                        break 'walk Ending::RowLimit;
                    }
                }
                // A slot before the row's first character makes it a slotted (white-on-black)
                // row: the title and status rows. Slots further in are underlined text's.
                0xB0 => {
                    if col == 0 {
                        row_has_slots = true;
                    }
                }
                0xC4 => underline = true,
                0xC0 => row_c0 = true,
                0xB3 => row_b3 = true,
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
                    // A disarmed C3 shows the optional row after it: the status row, which
                    // the screen shows at the right end of the title row.
                    if b == 0xC3 && row.is_empty() {
                        status_row = true;
                    }
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
                    // Colours are settled when the row ends: white on black for slotted rows
                    // and ruler rows, black on white for everything else.
                    if col >= scroll && col < scroll + WINDOW_COLS {
                        row.push(Cell { code: b, reverse: false, underline });
                    }
                    underline = false;
                    col += 1;
                }
            }
            addr = next;
        }
        Ending::ByteLimit
    };
    if !row.is_empty() {
        let is_ruler = is_ruler_row(&row, row_c0, row_b3);
        finish_row(&mut row, row_has_slots || is_ruler);
        rows.push(row);
        kinds.push((false, false));
    }
    black_out_filler_rows(&mut rows, &kinds);
    CptScreen { rows, ending, unknown_codes: unknown, status_merged }
}

/// Filler rows (a bare `C0 0D`: no document text there, e.g. above the start of the
/// document) show as empty white-on-black lines, per the user. The typing line (the row just
/// above the main ruler) stays black on white even when it is empty.
fn black_out_filler_rows(rows: &mut [Vec<Cell>], kinds: &[(bool, bool)]) {
    let typing = kinds.iter().position(|k| k.1).and_then(|r| r.checked_sub(1));
    for (i, (row, &(filler, _))) in rows.iter_mut().zip(kinds).enumerate() {
        if filler && Some(i) != typing {
            *row = vec![Cell { code: 0x20, reverse: true, underline: false }; WINDOW_COLS];
        }
    }
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
        0x8B => '®', // "CPT® WORD PROCESSING SOFTWARE" (start-up message)
        0x8C => '©', // "COPYRIGHT© 1983 BY CPT CORPORATION"
        // The tab character in text (the TAB key's insert at C8BBH, block 1, puts it where a
        // tab run ends): a black half diamond pointing right (per the user).
        0x89 => '▸',
        // A coded space (CODE + SPACE): a space that justification keeps as it is and that
        // keeps the words either side of it on one line. A small faint dot (per the user).
        0x90 => '·',
        // A coded return (CODE + RETURN ends the line with 8D before the 0D): a black half
        // diamond pointing left (per the user).
        0x8D => '◂',
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
            let mut cur: Option<(bool, bool)> = None;
            for c in row {
                if cur != Some((c.reverse, c.underline)) {
                    s.push_str("\x1b[0m");
                    s.push_str(if c.reverse { REVERSE } else { NORMAL });
                    if c.underline {
                        s.push_str("\x1b[4m");
                    }
                    cur = Some((c.reverse, c.underline));
                }
                s.push(glyph(c.code));
            }
            if cur.is_some_and(|(_, u)| u) {
                s.push_str("\x1b[24m");
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
        // The bare `C0 0D` filler row is an empty white-on-black line.
        assert_eq!(text(&s, 1), " ".repeat(WINDOW_COLS));
        assert!(s.row_is_reverse(1));
        assert_eq!(s.ending, Ending::EndMark(0x1016));
    }

    #[test]
    fn filler_rows_are_black_but_the_typing_line_and_document_blanks_are_not() {
        // Title; two filler rows; a document line; a blank document line (no C0); the typing
        // line (filler, just above the ruler); the ruler; B1.
        let mut l = vec![0xC0, 0xB0, b'T', 0x0D];
        l.extend([0xC0, 0x0D, 0xC0, 0x0D]);
        l.extend([b'D', b'o', b'c', 0x0D]);
        l.extend([0x0D]);
        l.extend([0xC0, 0x0D]);
        l.extend([0xC0, 0x87, 0x80, 0x81, 0x83, 0x0D, 0xB1]);
        let s = run(0x2000, &l, 0x2000);
        assert_eq!(s.rows.len(), 7);
        assert!(s.row_is_reverse(1) && s.row_is_reverse(2), "filler rows above the document");
        assert_eq!(text(&s, 3), "Doc");
        assert!(s.rows[4].is_empty(), "a blank document line stays black on white");
        assert!(s.rows[5].is_empty(), "the empty typing line stays black on white");
        assert!(s.row_is_reverse(6), "the ruler");
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
        // A slotted row is white on black throughout, even where a character has no slot
        // (the title's column 40 and right end, the status row's numbers).
        let s = run(0x5000, &[0xC0, 0xB0, b'a', b'1', 0xB0, b'b', b' ', b' ', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [true; 5]);
    }

    #[test]
    fn the_status_row_merges_into_the_title_row() {
        // Title row (slots), a text row, then C3 (disarmed) -> the status row right after it,
        // then a row below. The status row's characters land at the title's right end.
        let mut l = vec![0xC0];
        for &c in b"IN 1 X    " {
            l.extend([0xB0, c]);
        }
        l.extend([0x0D, 0xC0, b'T', 0x0D]);
        let c3 = 0x6000 + l.len() as u16;
        let after = c3 + 5;
        l.extend([0xC3, 0x00, 0x00, after as u8, (after >> 8) as u8, 0xC1, 0xFF, 0xC0]);
        for &c in b"       7 1" {
            l.extend([0xB0, c]);
        }
        l.extend([0x0D, 0xC0, b'B', 0x0D, 0xB1]);
        let s = run(0x6000, &l, 0x6000);
        assert!(s.status_merged);
        assert_eq!(s.rows.len(), 3, "title, text, the row below: no separate status row");
        assert_eq!(text(&s, 0), "IN 1 X 7 1");
        assert!(s.row_is_reverse(0));
        assert_eq!(text(&s, 1), "T");
        assert_eq!(text(&s, 2), "B");
    }

    #[test]
    fn the_ruler_glyphs_are_white_on_black() {
        let s = run(0x6000, &[0xC0, 0x83, 0x80, 0x81, 0x87, 0x82, 0x84, 0x83, 0x0D, 0xB1], 0x6000);
        assert_eq!(text(&s, 0), "M.:^|>M");
        assert!(s.row_is_reverse(0));
    }

    fn under(s: &CptScreen, r: usize) -> Vec<bool> {
        s.rows[r].iter().map(|c| c.underline).collect()
    }

    #[test]
    fn c4_underlines_the_next_character() {
        // Underlined text in a document row: a C4 before each underlined character (JP-M24's
        // "M E M O"), B0 before the plain ones; the colours don't change.
        let s = run(0x5000, &[0xC0, b'x', 0xC4, b'M', 0xC4, b' ', 0xC4, b'E', 0xB0, b'y', 0x0D, 0xB1], 0x5000);
        assert_eq!(text(&s, 0), "xM Ey");
        assert_eq!(under(&s, 0), [false, true, true, true, false]);
        assert_eq!(rev(&s, 0), [false; 5], "B0 slots inside a text row don't make it white on black");
        // The cursor or a menu's selection: the blink routine swaps a B0 slot for C4, so on a
        // white-on-black row it is an underline (blinking as the CPT swaps it back).
        let s = run(0x5000, &[0xC0, 0xB0, b'a', 0xC4, b'b', 0xB0, b'c', 0x0D, 0xB1], 0x5000);
        assert_eq!(rev(&s, 0), [true, true, true]);
        assert_eq!(under(&s, 0), [false, true, false]);
        // In the title's double slots, one swapped slot underlines the character too.
        let s = run(0x5000, &[0xC0, 0xB0, 0xB0, b'a', 0xC4, 0xB0, b'b', 0x0D, 0xB1], 0x5000);
        assert_eq!(under(&s, 0), [false, true]);
        assert!(s.row_is_reverse(0));
    }

    #[test]
    fn a_system_inserted_blank_line_is_black_on_white() {
        // Double spacing: text, the inserted blank line (a lone 80 after a jump), text, then
        // a format line (B3) and the main ruler, which stay white on black.
        let s = run(0x5000, &[
            0xC0, b's', b'd', 0x0D,
            0x80, 0x0D,
            b's', b'd', 0x0D,
            0xB3, 0x83, 0x80, 0x84, 0x83, 0x0D,
            0xC0, 0x87, 0x80, 0x81, 0x83, 0x0D, 0xB1,
        ], 0x5000);
        assert_eq!(text(&s, 1), ".");
        assert_eq!(rev(&s, 1), [false], "the inserted line's dot is on a white line");
        assert!(!s.row_is_reverse(0) && !s.row_is_reverse(2));
        assert!(s.row_is_reverse(3), "the format line");
        assert!(s.row_is_reverse(4), "the main ruler");
    }

    #[test]
    fn a_coded_return_is_a_half_diamond_pointing_left() {
        let s = run(0x5000, &[0xC0, b'a', b'b', 0x0D, 0x8D, 0x0D, b'c', 0x8D, 0x0D, 0xB1], 0x5000);
        assert_eq!(text(&s, 1), "◂");
        assert_eq!(text(&s, 2), "c◂");
        assert_eq!(rev(&s, 2), [false; 2]);
    }

    #[test]
    fn a_coded_space_is_a_dot_one_column_wide() {
        let s = run(0x5000, &[0xC0, b'a', b'b', 0x90, b'c', b'd', 0x0D, 0xB1], 0x5000);
        assert_eq!(text(&s, 0), "ab·cd");
        assert_eq!(rev(&s, 0), [false; 5]);
        assert!(s.unknown_codes.is_empty());
    }

    #[test]
    fn the_end_of_a_tab_run_is_a_half_diamond_pointing_right() {
        // JP-M24's "3." A0 89 "Your": the tab's end keeps its column, black on white; the
        // ruler's own tab stop (84) stays white on black.
        let s = run(0x5000, &[0xC0, b'3', b'.', 0xA0, 0x89, b'Y', 0x0D, 0xC0, 0x80, 0x84, 0x80, 0x0D, 0xB1], 0x5000);
        assert_eq!(text(&s, 0), "3. ▸Y");
        assert_eq!(rev(&s, 0), [false; 5]);
        assert_eq!(text(&s, 1), ".>.");
        assert!(s.row_is_reverse(1));
    }

    #[test]
    fn ansi_output_uses_black_on_white_and_white_on_black() {
        let s = run(0x5000, &[0xC0, 0xB0, b'a', 0x0D, 0xC0, b'b', 0x0D, 0xB1], 0x5000);
        let a = s.to_ansi();
        assert!(a.contains("\x1b[37;40ma"), "reverse row: {:?}", a);
        assert!(a.contains("\x1b[30;47mb"), "normal row: {:?}", a);
        let s = run(0x5000, &[0xC0, b'a', 0xC4, b'b', 0x0D, 0xB1], 0x5000);
        assert!(s.to_ansi().contains("\x1b[30;47m\x1b[4mb\x1b[24m"), "underlined: {:?}", s.to_ansi());
    }

    #[test]
    fn zero_width_codes_and_unknown_codes() {
        let s = run(0x6000, &[0xC0, b'x', 0xB2, b'y', 0xB4, b'z', 0xA0, 0xD7, 0x0D, 0xB1], 0x6000);
        assert_eq!(text(&s, 0), "xyz ");
        assert_eq!(s.unknown_codes, vec![0xD7]);
    }
}

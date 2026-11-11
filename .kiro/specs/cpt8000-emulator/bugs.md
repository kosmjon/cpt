* line spacing > 1 reveals white-on-black dot lines.
  * fixed: the inserted blank line is a lone 80H (the ruler's column tick) in a text row. Only
    ruler rows are white on black now, and the browser draws that 80H as a tiny faint dot on
    a white line.
* certain other pages reveal black areas and unrecognized glyphs.
* special code for underscore not working on screen or print
  * screen: fixed. C4H before a character underlines it (JP-M24's M E M O R A N D U M); the
    cursor and menu selection are the same code, so they are a blinking underline.
  * print: underline (and bold) still print as plain characters.
* title line should be white on black
  * unknown: the display list has no white-on-black code on that row (it is `C0` and plain
    text), so it is left black on white. See program-disk.md, "Unknowns in how documents show".
* white-on-black `>` before JP-M24's list items 3, 4 and 5
  * fixed: that is 89H, the tab character in text (the TAB key fills with required spaces and
    ends with 89H). It is not in the margin line, and its real glyph is unknown, so it now
    shows as a plain space.
* title line not cleared when new TOC loaded?
* margin lines appearing in the middle of text?
  * as designed, as far as the software shows: a recorded margin line (CODE + M) is stored as
    B3H, ruler glyphs, 0DH, with no "hide" code, and the CPT gives it an ordinary screen row
    (the status row, which it does hide, uses a switchable C3H link instead). Open: whether the
    display hardware skipped B3H rows. See program-disk.md, "Unknowns in how documents show".
* map more real keyboard keys
** Esc to SKIP: done
** delete to ERASE: done (ERASE is key 4FH; it blanks the character under the cursor and
   moves on, so "abcd" with the cursor on c becomes "ab d")
** insert to INSERT: waiting for INSERT's key number (not identified on the real system yet)
** others?
* CODE+RETURN shows a `?`
  * it's a coded return: CODE + RETURN's handler (B657H, block 2) ends the line with 8DH
    before the usual 0DH (on a document's first line or a margin line it acts as a plain
    RETURN). Its real glyph is unknown; it now shows as a faint arrow pointing left.

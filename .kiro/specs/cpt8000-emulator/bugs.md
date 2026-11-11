* line spacing > 1 reveals white-on-black dot lines.
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
* map more real keyboard keys
** Esc to SKIP
** delete to ERASE
** insert to INSERT
** others?

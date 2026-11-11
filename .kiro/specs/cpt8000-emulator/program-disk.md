# The real CPT program disk (`cpt.td0`)

`cpt.td0` (in this folder) is a TeleDisk image of what is believed to be a real CPT program
disk. `tools/td0.py` decodes it and verifies every CRC (image header, all 77 track headers, all
1232 sector data blocks), so the decoded bytes are exactly what TeleDisk read.

```
python tools/td0.py .kiro/specs/cpt8000-emulator/cpt.td0                 # summary
python tools/td0.py .kiro/specs/cpt8000-emulator/cpt.td0 --raw disk/cpt.img --json disk/cpt.json
```

`disk/` is gitignored; it is rebuilt from the `.td0` by the second command.

## Physical format **[CONFIRMED from the image]**

| | |
|---|---|
| Drive | 8-inch, single-sided |
| Encoding | FM (single density), 500 kbps rate code |
| Tracks | 77 (cylinders 0–76), head 0 only |
| Sectors | **16 per track, 256 bytes each**, numbered **0–15** (not 1-based) |
| Interleave | none: recorded 0,1,2,…,15 (track 5 has 6–9 recorded out of order: 9,7,8,6) |
| Capacity | 77 × 16 × 256 = **315,392 bytes** |
| Odd marks | track 1 sector 0 has a **deleted-data** address mark |

This is **not** the IBM 3740 26 × 128 layout the emulator's disk currently assumes
(`Geometry::default()` = 2002 blocks of 128 bytes). It is a CPT-specific format.
TeleDisk header: version 1.5, no comment block, uncompressed (`TD`).

## Contents (first look) **[HYPOTHESIS unless marked]**

- **Track 0 is filesystem metadata, not code.**
  - Sector 0 looks like a volume label: a block of small binary fields and the text
    `6  5617  8100  H2` (part number / version / date?).
  - Sector 3 (`55 55 …` then `FF FF …`) and sector 6 (`… 55 55 55 …`) look like allocation
    bitmaps.
  - Sector 13 holds `OP.00.1`, which looks like a file or document name.
  - This fits the B-tree filing system (Segment 12) described in the requirements.
- **Tracks 1–67 hold data; 68–76 are blank (all zeros).**
- **There is plenty of 8080 code** **[CONFIRMED]**: fragments decode as sensible 8080
  sequences (e.g. `INX H; MOV A,M; CMA; MOV C,A`), and the image has ~5,800 CALL, ~2,100 JMP,
  ~2,100 RET, 186 EI and 136 DI opcode bytes.
- **Readable messages:** `*** PATCH PAGE ERROR ***`, `PROCESSING THE CONTROL PAGE`.
- **Probable character / keyboard tables:**
  - track 4 sector 11: `aAbBcGdDeEfZgHhUiIjKkLlMmNnJoOpPqPrRsStWuTvYwFxXyCzV` (pairs:
    a character and a mapped code?)
  - track 14 sector 8: `#]m%P&@NO?=}-\-0123456789;'F>J[$/AWVDQ~BEMG{^X<R`S*YLC`, a
    character list in a non-alphabetical order (key-matrix order? checklist K1).
  - tracks 2 and 8: `nopqrstuvw!HJKMNOPQRSTUVW\]^_abcdefg"<$>&'…`.
- **Text is probably not stored as ASCII.** 20H (space) is by far the most common byte
  (~41,000 of 315,000), yet long printable runs are rare. Document text is likely kept in a
  CPT internal character code, with ASCII used for names and messages; the tables above may be
  the code converters (checklist D4: character-code encoding).
- **Candidate I/O ports.** Port numbers following `OUT` (D3) and `IN` (DB) bytes cluster at
  **00–05** and **22–25** (seen in both directions), with 10H/11H/18H for output. These are
  pointers only: data bytes can look like `OUT nn`. Confirm them in disassembly (checklist M1,
  K1, F1, R1).

## The filesystem on track 0 **[decoded; meanings are HYPOTHESIS where marked]**

Multi-byte numbers are **big-endian** throughout. Sector numbers are **linear** (0–1231):
linear = track × 16 + sector.

### Sector 0: volume label
- `+05..+06` = `04D0` = **1232**, the total sector count **[CONFIRMED by arithmetic]**.
- `+29` = `10` = **16** sectors per track; `+2A..+2B` = `0100` = **256** bytes per sector;
  `+2C` = `4D` = **77** tracks **[CONFIRMED: they match the physical format]**.
- `+37..` text `6  5617  8100  H2` (part number / version / date?) [HYPOTHESIS].
- Other fields (`00 00 10`, `00 00 14`, `00 00 1D`, … in 3-byte groups around `+1B`) look like
  sector addresses of system structures [HYPOTHESIS].

### Sectors 3 and 6: the allocation map
**2 bits per sector**, most significant pair first: sector 3's 256 bytes then the first 52
bytes of sector 6 cover exactly 1232 sectors. The rest of sector 6 is `55` (code `01`) padding
for sector numbers past the end of the disk. The codes match the contents almost exactly:

| Code | Count | Where | Contents | Meaning [HYPOTHESIS] |
|---|---|---|---|---|
| `01` | 16 | exactly track 0 | metadata | reserved (filesystem track) |
| `11` | 544 | all of tracks 1–32, and single sectors every ~24 within the `10` runs | 524 hold data | system area, and file index/header blocks |
| `10` | 361 | runs from track 33 on | all 361 hold data | allocated file data |
| `00` | 311 | the rest | 150 blank, 161 stale data | free (deleted files leave their data) |

### The document directory: a B-tree **[CONFIRMED structure]**

`python tools/cptfs.py disk/cpt.img` walks it and lists the documents.

- **Node:** a level byte in ASCII (`'0'` = leaf, `'1'` = inner), then 23-byte items:
  a **16-character name** (space-padded), a **4-byte field**, and a **3-byte pointer**.
  Items are sorted by name; an inner node's first item has an empty name (everything below the
  next key). Empty item slots occur in leaves (leaf 902's first slot is empty).
- **Root:** track 0 sector 13, level `'1'`: `[ ptr 898 ]` `OP.00.1` `[ ptr 897 ]`.
- **Leaves:** sector 902 (`D` … `OP.00.0`) and sector 899 (`OP.00.1` … `WN`), level `'0'`.
- **Pointers are logical sectors (track × 16 + s), skewed by 3 within a track and shifted by
  2 per track:** physical sector = (3 × s + 2 × track) mod 16 [CONFIRMED]. Logical 897
  (track 56 s 1) is physical sector 3 = linear 899, and logical 898 is physical sector 6 =
  linear 902 (on track 56 the shift is 112 mod 16 = 0). The physical recording has no
  interleave, so the skew is the filesystem's own.
  - The first analysis used plain skew 3, which works only on tracks that are multiples of 8.
    This disk's directory and LEGAL happen to sit on tracks 56 and 48, which hid the shift.
    cpt2489.img's directory (leaves on tracks 34, 60 and 64) showed it.
  - The real system's reads match the full rule for every document tried (table under "The
    filer reads header blocks directly", and IN DIRECTIONS on cpt2489: 918 = t57 s6 → t57 s4).
  - So **a leaf's pointer is the document's first (header) block**.

The 14 documents:

| Name | 4-byte field | Pointer | | Name | 4-byte field | Pointer |
|---|---|---|---|---|---|---|
| D | 000117DA | 899 | | OP.00.1 | 0000024E | 887 |
| HM-M1.2 | 00000799 | 923 | | OP.01.0 | 000001D5 | 890 |
| LEGAL | 000117D4 | 768 | | PLASTIC | 000117D3 | 792 |
| LEX | 0001011C | 673 | | RC-L1 | 00010B7A | 675 |
| MID | 00000017 | 552 | | STD | 000117D7 | 744 |
| OP | 00000402 | 892 | | T | 000117D5 | 720 |
| OP.00.0 | 00000259 | 545 | | WN | 0001002F | 553 |

**The 4-byte field** [CONFIRMED by measuring all 43 documents on both program disks, and a
page filed by the system]: its **low 16 bits are the document's length in bytes**, counted
from the `01` of the header through the `03` end mark. The six print-wheel tables with
nearly consecutive values (`000117D3` … `000117DA`) are simply tables of nearly the same
length (6099–6106 bytes), which is why they looked like a sequence. The high 16 bits are
0 or 1 and their meaning isn't known (1 for RC-L1, LEGAL, WN, LEX, CONTROL and the C-ASCII
tables; 0 for MID, OP and HM-M1.2).

**Not yet understood:**
- **The field's high word** (above).

### Documents

- **A document's first text block** starts `01`, the document's name, `02`, then the text:
  plain ASCII, lines ending `0D` (some `8D 0D`). E.g. sector 883: `OP.00.1` "OPERATION 0 …
  WORD PROCESSING … TYPE OF PRINTER 8X00 STANDARD (Rotary IV-IX)". 22 such header blocks exist,
  more than the 14 directory entries (old or deleted versions survive in free space).
- **List blocks** (e.g. 884, 887, 890, 894): 3-byte items `flag, lo, hi` with flag 01/03/05
  and a **little-endian** 16-bit value that climbs by a few bytes per item (064B, 0650, 0654,
  …), ending at `FE`. Most likely a line index: the byte offset of each line in the document.
- A document's blocks are **interleaved** with other documents', not consecutive.
- Contents seen: printer setup pages (`[000=STANDARD]`, `[1=EXPAND BLANK LINES]`), a memo
  ("Memo to Messrs. Bell and Phillips"), a `D-Page 8100/8515/8525` page, form letters.

## The boot loader **[CONFIRMED by disassembly]**

**Track 1 sector 0** (the sector with the deleted-data mark, linear 16) is boot code built to
run at **0000H**: it starts `JMP 00DAH`. Disassemble it with
`cpt-run --disasm disk/cpt.img 0x1000 256 0`.

Stage 1 (0000H): `LXI SP,0800H`, then reads track 1 sector 1 into **0100H** (retrying on
error). Its read routine shows the disk hardware:

| Port | Use in the code | Reading |
|---|---|---|
| 10H, 11H, 18H | `OUT 10H` = address low then high; `OUT 11H` = `FF` then `40` (count 40FFH); `OUT 18H` = `71H` | An **Intel 8257 DMA controller** at base 10H: channel 0 address, channel 0 count (40FFH = 256 bytes, DMA write = disk to memory), mode register |
| 24H | out `88H` (Read Sector command); in: status, tested with `9FH` | **WD/FD1771** floppy controller command/status (the single-density 8-inch controller, matching the FM disk) |
| 25H, 26H | out track (D), sector (E); stage 1 also reads 25H back | FD1771 track and sector registers; data would be 27H (DMA moves the data) |
| 22H | out drive/side select (B) | drive select latch |
| 23H | out: bit patterns rotating 1→2→4→8 plus bit 3, with a delay | the CPU **drives the head stepper motor phases directly** |
| 23H | in: bit 1 polled until set; bit 2 used during homing | controller status (operation done / track 0?) |

Stage 2 (0100H):
- **Port 03H is the bank-select port**: it writes 3, 2, 1, then 0 to `OUT 03H`, clearing each
  bank in turn (a routine pushes zeros down through memory). 03H is also the most frequent
  `OUT` port in the whole image. The emulator's placeholder is 40H.
- Reads one whole track (from the sector named by a pointer at offset 15H of the loaded data, up
  to sector 15) to **E000H**, or to **6000H** depending on a configuration byte (at 016AH,
  value 3 in this image; < 3 selects E000H), then jumps there with `PUSH H; RET`.

### Stage 2, in detail **[CONFIRMED by disassembly]**

1. Reads **track 0 sector 0, the volume label, into 0200H**.
2. Takes the two bytes at **label +15H** (`01 04` here): track 1, sector 4.
3. Reads that track from that sector to sector 15 (12 sectors, 3 KB, linear 20–31) to
   **6000H** (the configuration byte at 016AH is 3; < 3 would select E000H).
4. Jumps to 6000H with B = drive, C = 3, D = 02 (the label's page), A = the byte saved at entry.

## Stage 3: the system loader at 6000H **[CONFIRMED by disassembly, partly traced]**

Disassemble with `cpt-run --disasm disk/cpt.img 0x1400 0xC00 0x6000`.

- Reads fields of the volume label through D = 02 (label +13H, label +32H).
- Selects each bank with `OUT 03H` and writes into it at 8000H (bank set-up).
- Loads the main system (routine at 6529H; not traced yet).
- Hands over with `OUT 23H` = 0 and **`OUT 35H`** (a port not seen in stages 1–2), then jumps
  with `PCHL` to **0040H or 0000H** depending on a flag (61A1H bit 0, from label +13H/+32H).
- Helper at 64E0H: a block move (A = count, BC = source, DE = destination).

### Display memory is a linked list of lines **[CONFIRMED in real code]**

The routine at 611AH shows a two-character status code (here built from B = 37H `7` and
C = 45H `E`, i.e. "E7") by copying 12 bytes of display data to **7FEFH**, near the top of
bank 0:

```
7FEF: C0 'E' '7' 0D      a line containing "E7"
7FF3: C0 0D              an empty line
7FF5: C2 F3 7F           C2 + address: a link to the line at 7FF3
7FF8: C2 EF 7F           a link to the line at 7FEF
```

and elsewhere the loader writes `C2 FB 7F C0 0D C2 FB 7F` at 7FF8H. So the display memory
holds **lines linked by pointers**: `C0` appears to start a line, `0D` ends it, and
`C2 lo hi` links to another line. The screen appears to start from a fixed point at the top of
bank 0 (7FF8H). This confirms the "page as a linked list of lines" hypothesis in the
requirements (checklist D1/D2/D4); the meaning of the other C-codes is still to be found.

## Running the real boot **[CONFIRMED by running it]**

`cpt-run --cpt-boot disk/cpt.img [<cycles>] [<break addr> | -] [<read delay cycles>]` boots this
image on a model of the real hardware: `CptFloppy` (8257 DMA + FD1771 + stepper, in
`cpt-machine/src/cpt_floppy.rs`) and `MachineConfig::cpt_real()` (bank select on 03H). The boot
ROM is not available, so it loads track 1 sector 0 to 0000H and starts there with A = 0, D = 0.
The image is never written back.

What running it showed:

- **The stepper is three-phase:** bits 0–2 of port 23H, one-hot 1/2/4, stepping in 1→2→4→1 and
  out 4→2→1→4; bit 3 is ORed into every write and is something else (head load / step enable?).
  Homing stops when the track-0 sensor (port 23H in, bit 2) is on **and** the phase is 1.
- **Two floppy controller addresses are supported.** Stage 1 writes 1 to port 25H and reads it
  back. If it sticks, the controller is at **22H–26H**; if not, stage 1 rewrites the port bytes
  of its own IN/OUT instructions to **34H, 35H, 3CH, 3DH, 3EH** (a second address set, likely
  another board revision). The stage 3 driver does the same (routine 64F0H).
- **The loader reads in skew-3 order** (t4 s8, 11, 14, 1, 4, …), as the directory analysis
  predicted, 600 sectors in all, switching banks through port 03H as it loads.
- **The loader times the disk.** Routine 65FFH reads sector E, starts reading E+1, waits a fixed
  31,080 cycles (15.5 ms at 2 MHz) and checks whether the second read has already finished; if
  it has, the loader shows **"E1"** in the display list and stops in an error loop at 60CDH.
  With instant reads that is what happens. With reads taking 40,000 cycles (20 ms, a guess) the
  check passes. The real meaning (drive speed? physical sector order?) is [VERIFY]; a faithful
  model would simulate the disk's rotation.
- **The boot then completes:** the loader jumps to the main system, which runs with interrupts
  enabled (SP 0F39H) and immediately programs:

| Port | Written | Reading |
|---|---|---|
| 1FH | `74H`, `B4H` | **Intel 8253** control words: counter 1, then counter 2, mode 2, 16-bit binary |
| 1DH | `80 3E` = 16000 | 8253 counter 1: 2 MHz / 16000 = **125 Hz** |
| 1EH | `20 4E` = 20000 | 8253 counter 2: 2 MHz / 20000 = **100 Hz**, the task-switch timer |
| 00H | `20`, `84`, `40` | `20H` is the 8259 interrupt controller's "end of interrupt" command: an **8259** at 00H/01H [HYPOTHESIS] |
| 35H | `00` | written by the loader at hand-over (secondary-address control port?) |

So the 100 Hz task timer is real, but it is a **programmable 8253**, not the fixed timer the
emulator currently models, and its 2 MHz input confirms the 2 MHz clock assumption.

## Interrupts **[CONFIRMED by disassembly of the running system]**

After the boot, the RST vectors at 0000H–0038H (8 bytes apart, so plain 8080 RST interrupts,
not an 8259 call table) hold:

| RST | Handler | Evidence | Source |
|---|---|---|---|
| 0, 1, 4 | `EI; RET` | none | unused |
| 2 | 0D07H | reads **port 01H** (bit 7 = data ready), then **port 02H** (data, inverted) | **keyboard** |
| 3 | 06BAH | reads 23H and 24H (floppy request and status) | **floppy controller** (FD1771 INTRQ) |
| 5 | 096CH | counts ticks (13EEH/13EFH), runs a routine every 5th | a periodic tick |
| 6 | 0D5CH | drains a queue at 0504H of (address, 2 bytes) writes | **display vertical retrace** (applies queued display updates) |
| 7 | 116FH | 16-bit tick count at 04A6H, inspects the interrupted address on the stack | the **task-switch tick** |

- **Port 00H is the interrupt acknowledge**, not an 8259: each handler ends by writing the bit of
  its own RST number (RST 2 → `04H`, RST 3 → `08H`, RST 5 → `20H`, RST 7 → `80H`). The init
  writes `84H` and `40H` once.
- **The 8253 counters drive RST 5 and RST 7.** With counter 1 → RST 5 and counter 2 → RST 7 (the
  pairing is a guess: RST 7 has the task-switch logic, and counter 2 is the 100 Hz one), the
  handlers acknowledge exactly as many interrupts as the counters produce.
- Modeled in `cpt-machine/src/cpt_interrupts.rs` (`Pit8253` at 1CH–1FH, `InterruptAck` at 00H),
  attached by `--cpt-boot` along with a 60 Hz retrace on RST 6 (the rate is a guess).

## The system draws its screen **[CONFIRMED by running it]**

With the retrace interrupt running, the main system reads three more sectors (interrupt-driven,
acknowledged with `08H`), writes port **21H** (`00` three times, unknown), and builds its screen
in the display memory in bank 0. The list head at **7FF8H** is `C2 06 50`, a link to 5006H.
Found there so far:

- The **typing-line ruler**: `87 80 80 80 81 80 80 80 80 82 80 80 80 80 81 … 83`: 87H/83H at the
  ends (margins), 80H dots, 81H every fifth position, 82H every tenth. These are the screen's
  own graphic characters (codes 80H and up).
- Rows of `B0 B0 20` patterns, and a row containing `1` and `0` (the status band?).
- (Corrected below: every row does end with `0D`; `B0` is a zero-width slot, so the `B0 B0 20`
  rows are blank 80-column rows.)

## The display list format **[decoded from the real code; HYPOTHESIS where marked]**

The display hardware walks the list on its own; the CPU only edits it. The retrace handler
(RST 6, 0D5CH) just applies queued 2-byte patches (queue at 0504H: flag, count, then
address + 2 bytes per entry), e.g. setting the head link at 7FF9H. Display memory is 256-byte
pages in bank 0 (4F00H–7FFFH), each page's last 3 bytes (xxFDH) holding a `C2` link to the next
(163FH/166EH). The format was read from the screen builder (E4E0H, block 0), the program's own
walker over display memory (8580H, block 0), the scroll routine (BAC6H, block 2) and the cursor
blink (93C0H, block 0). `cpt_machine::cpt_display` decodes it; `cpt-run --cpt-boot` prints the
screen.

| Code | Width | Meaning | Evidence |
|---|---|---|---|
| `00`–`AF` | 1 | a character-ROM glyph; `20`–`7E` ASCII | walker counts columns only below B0 (860AH) |
| `A0` | 1 | required space | print path turns it into 20H (1A38H) |
| `90` | 1 | **coded space** [CONFIRMED by typing: CODE + SPACE between `ab` and `cd` gives `61 62 90 63 64`]. Per the user: a space kept through justification (before or after words and lines) and one that keeps the words on either side together on a line; shown as a small faint dot. The browser draws it a little below mid-height, the inserted blank line's dot (80) at mid-height | a decoded printout shows it as a space |
| `8D` | 1 | **coded return** [CONFIRMED by disassembly and by typing]: CODE + RETURN's handler (B657H, block 2) writes 8D and then ends the line, so the line ends `8D 0D`. On a document's first line (C0) or a margin line (B3) it writes no 8D and acts as a plain RETURN (B6FAH). **Its real glyph is unknown**: the emulator shows a faint arrow pointing left (the user's choice), one column wide | plain RETURN (code B6H) is editor index 07H; CODE + RETURN is index 26H (CAA2H) |
| `80`–`82` | 1 | ruler glyphs: column tick, every 5th, every 10th. A lone `80` in a text row is double spacing's **system-inserted blank line** [CONFIRMED by typing: CODE + L, 2, then a line and RETURN gives `C2 80`]. The glyphs are not reverse by themselves: only ruler rows (the main ruler, a format line) are white on black, so the inserted line shows as a tiny faint dot on a white line (per the user) | ruler built at E4E0H |
| `83` | 1 | ruler: a margin (left and right; one code for both). Per the user, both are a flat-topped vertical bar (the browser draws it 3 px wide) | **[CONFIRMED by typing]**: margin set (key 71H) adds one at the cursor |
| `87` | 1 | ruler: the cursor's column, on the main ruler only. It takes the place of whatever mark is in that column (the left margin when the cursor is in column 1). Per the user, a wide vertical bar with a pointed top (the browser draws it 7 px wide, under a 3-step point) | **[CONFIRMED by typing]**: moves with the status row's column |
| `84` | 1 | ruler: a tab stop. Per the user, a short, narrow T (the browser draws it so) | **[CONFIRMED by typing]**: tab set (key 75H) puts 84 at the cursor |
| `89` | 1 | the **tab character in text** [CONFIRMED by disassembly]: the TAB key's insert (C891H–C8BDH, block 1) scans the format line for the next tab stop (84) or margin (83), fills with A0 and ends with 89 (with 84 instead when editing a format line). It never appears in a ruler or format line. **Its glyph is unknown**: the emulator shows a plain black-on-white space | treated as a tab/word break (8552H, 863DH); e.g. JP-M24's list items `3.` A0 89 `Your estimate…` |
| `0D` | – | end of row | every row, builder and messages |
| `B0` | 0 | per-character attribute slot: **plain** (C4 in its place underlines). A row whose first character has a slot is **white on black** [HYPOTHESIS]: one slot per column in the status row, two in the title row. In document text slots appear only around underlined characters, and the row stays black on white | walker skips it; cursor toggles it; gives exactly the reversed rows the user remembers |
| `B1` | 0 | end of screen | walker substitutes B1 for a link to 0000H |
| `B2` | 0 | filler | walker skips it; pads the 16-byte cells below the status row |
| `B3` | 0 | start of a **margin line** (format line) recorded in a document, CODE + M [CONFIRMED on disk]: every one on both disks (7 on cpt.img, 6 on cpt2489) is `B3`, ruler glyphs starting with a margin (`83`), `0D`, and nothing else. The row is white on black, like the ruler | seen before RC-L1's and JP-M24's format lines; 29 places test for it (`CPI B3H`), two write it (block 2 B335H, B966H) |
| `BD` | 0 | [HYPOTHESIS] marks a hyphen at a line break (one the system added) | `con` `BD` `-` `0D` in RC-L1's text |
| `B4` / `B5` | 0 | half-line down / up (sub/superscript) | printer path moves +2 / −2 (9C64H); prompt text `{` `}` become B4/B5 (CFB7H) |
| `C0` | 0 | starts every row; ends every status message. [HYPOTHESIS] attributes off | builder; messages are `text C0 0D` |
| `C1 xx` | 0 | horizontal scroll: first visible column = `NOT xx`, 80-column window | BAC6H steps it, writes the ruler's and text area's together |
| `C2 lo hi` | 0 | jump to `hi lo` (0000H ends the screen) | walker, builder |
| `C3 a a b b` | 0 | disarmed jump: continue at `b b` (the byte after) | E6BAH flips C2↔C3 to hide/show an optional status row |
| `C4` | 0 | **underline** the next character [CONFIRMED]: JP-M24 (cpt2489) has a C4 before each character of its underlined heading `M E M O R A N D U M`. The cursor and a menu's selection are the same attribute, so they are a **blinking underline** (per the user) | blink at 93C0H swaps a slot B0↔C4 every 25 ticks |
| `C5`–`CF` | 0 | unknown; C6, C7, CA are built by the line copier at 8820H (block 1) | not yet traced |

**Unknowns in how documents show** (left as they are until more is known):
- **A document's title line.** Its first line (JP-M24's `Memo - Phillips to Bell re Test
  Cell Project  7/10/87`) is its title: the patch-page reader skips it, and it comes just
  before the document's format line. The user remembers it as white on black. But in the
  display list it is an ordinary row, `C0` followed by plain text, with no slot (B0) or other
  code that would make it white on black, so the emulator shows it black on white. What made
  it white on black on the real screen, if anything did, is unknown.
- **The tab character's glyph (89H).** See the table: shown as a plain space.
- **Whether recorded margin lines were hidden.** Nothing marks one to be hidden
  [CONFIRMED]:
  - On disk, a margin line is `B3`, ruler glyphs, `0D`, with no other code (all 13 on both
    disks).
  - On screen the CPT gives it a row of its own. JP-M24's is reached by an ordinary jump (`C2`)
    into its line buffer, after `C1 FF` (no horizontal scroll), like a text row. The one row
    the CPT hides and shows, the status row, is reached through a disarmed jump (`C3`) that
    E6BAH flips; a margin line never is.

  So the emulator shows margin lines where the CPT puts them, in the middle of the text. Still
  open: whether the display hardware itself skipped a row starting with `B3`. That seems
  unlikely, because the CPT counts the margin line as one of its screen rows, so hiding it
  would leave a gap, but the hardware isn't documented.
- **The coded return's glyph (8DH).** See the table: shown as a faint left arrow.

**Reverse video and the status [user's memory, with a HYPOTHESIS for how it is coded].** The
screen is black on white. The user remembers:
- the title line and the ruler as white on black, with no black-on-white cells in the title
  line;
- the status (column, line, unit) at the right end of the title line, not as a row of its
  own;
- the document text and the typing line (the row just above the ruler) as black on white.

The display data has no row or region code for these. Three things in the data do fit:
- The title and status rows put a `B0` slot before most characters, and text rows have none.
  So a row with any `B0` is white on black from edge to edge. That includes the cells
  without a slot: the title's column 40 and last two columns, and the status row's numbers.
  An earlier rule (`B0` = only the next character) left those black on white, and the user
  confirmed that was wrong.
- The ruler is made only of glyphs 80H–87H, so the character ROM draws those glyphs inverse.
- The status row is the optional row behind the `C3` switch. Its characters fall at columns
  64–80, the title line's right end. How the hardware puts it there is unknown; the decoder
  merges it into the title row.
- **Rows with no document text are empty black lines.** For example, the rows above the
  start of the document, which fill the whole upper screen after PAGE, DOWN. They are bare
  `C0 0D` filler rows. Document lines, blank ones included, are separate buffers reached by a
  link and have no `C0` (only a document's first line keeps the `C0` from its header). So
  a bare `C0 0D` row is drawn white on black. The exception is the typing line (the row
  above the ruler), which stays black on white even when it is empty.

The character ROM is not on the disk. The glyph shapes for 80H–AFH (ruler ticks, tab marks,
the return mark 8DH?) are unknown and drawn with ASCII look-alikes.

Early in the start-up the screen has 58 rows. Once the system has started (about 35 s of
emulated time), the editing screen has **48 rows**:
- the top message/prompt line (it briefly shows the copyright, "CPT CORPORATION.  ALL RIGHTS
  STRICTLY …", cut off at the right of the window);
- 44 text rows and the typing line (row 46);
- the ruler;
- the status row: the cursor's column at column 67, the line number at column 72, and a unit
  or mode name at the right (CHAR, WORD, LINE, PARA, PAGE) or HOLD at the left.

The top line's prompt has two `B0` slots per character, room for per-character attributes.
7FFBH–7FFFH (`C0 0D C2 FB 7F`) is a blank row that loops to itself: the loader points the head
there to show an empty screen.

## Segments, banks and functions **[CONFIRMED by disassembly]**

The system is split into **segments**, and code calls across banks through one dispatcher.
`python tools/cptsegs.py <memory dump> [--calls]` decodes the map from a `cpt-run --cpt-boot`
memory dump.

- **A far call** is `CALL 0046H` followed by two inline bytes: segment, then function.
  0046H jumps to the dispatcher at **12DEH**.
- **The segment table** is in bank 0 at **0174H**, with the segment count (128) at 0173H.
  Each entry is 4 bytes:
  - a present flag;
  - a bank byte b: the segment is in block b − 1 at 8000H–FFFFH (`OUT 03H, b−1`), or bank 0
    only when b is 0;
  - the address of the segment's function table (little-endian).
- **A function table** is a count byte, then a 2-byte address per function (0000 = none).
- **The dispatcher** checks the segment number against the count and the present flag. If
  either fails it returns at once, doing nothing. Otherwise it saves the caller's bank,
  selects the segment's bank, rewrites its own return to the function's address, and returns
  into it. The function returns through **134EH**, which restores the caller's bank.
- **1374H** (segment 01 function 04) looks up a function's address the same way: L =
  function, H = segment.

After start-up, 32 of the 128 segments are present: 7 in bank 0, and the rest in blocks 0, 1
and 2. The code has 423 far-call sites (242 distinct functions). 25 of the segments they call
aren't present on this program disk (03, 19, 1C, 1F–22, 24, 26, 27, 2B, 33, 38, 41–44,
46–49, 66, 69, 78, 7A), so those calls do nothing. They're probably options this disk wasn't
built with, such as communications or other machines' hardware [HYPOTHESIS].

Functions identified so far:

| Seg.func | Address | What it does |
|---|---|---|
| 01.00 | 0CDDH | drive table lookup |
| 01.01 | 07DDH | start a disk request |
| 01.02 | 087CH | wait for a disk request, with retries (the task's yield loop) |
| 01.04 | 1374H | look up a far function's address |
| 02.02 | 1632H | allocate a 256-byte display page (the `C2` page links) |
| 0C.29 | 8CE2H (block 2) | the filer: find a document / probe a drive (start-up calls it for drives 0 and 1) |
| 3A.00 | E4D0H (block 0) | build the screen's display list |
| 3A.0B | E6BAH (block 0) | swap the optional status row's jump between C2 and C3 |
| 3D.0A | D74CH (block 2) | the keyboard task: take the next key from the ring at 058BH |

The second program disk, cpt2489 (with TTY), has its own map, in hex and decimal, in
[segments-cpt2489.md](segments-cpt2489.md). It adds segment 43 (2BH), serial communications
through an 8251 USART at 4AH/4BH (receive on RST 1, transmit on RST 4). The emulator models the
8251, and `cpt-run --serial-listen` connects it to a terminal. TTY mode works: reception to the screen or
to a document, and transmission from the keyboard ("Using TTY mode" in that document). The
emulator's 8251 holds its transmit request off until `10H` is written to port 00H; without that,
TTY mode corrupted its own control block.

The system's own entry points are a jump table at 0040H–006CH. For example: 0040H start-up
(0D98H), 0046H the far-call dispatcher, 0052H yield (1143H), 0058H wait (11B4H), 005BH
allocate a display page, 005EH delay (1603H), 0061H show a message (151AH), 0064H the fatal
trap (15C1H).

## The editor's key dispatch **[CONFIRMED by tracing the running system]**

How a key reaches its handler, found by stopping the emulator at each step after a key press:

1. **The keyboard task's reader** (D74CH, segment 3D function 0A) translates the key number
   through the key table and returns a class and an index. The class says whether the key is
   a character, a function key (code A0H–DEH), or a keypad key (DFH–EFH), combined with the
   CODE/SHIFT state.
2. **"Get the next key"** (D9A7H, segment 3D function 00) calls it through a wrapper (DEB2H,
   3D.0B) that maps the class through a 9-byte table at DEE5H. It also replays the keyboard
   program (PROG) from a buffer at 0564H. It returns C = class and B = index. **The index is
   not the function code − A0H**; this step remaps it.
3. **The editor loop** (C765H, segment 23 function 00) branches on the class: function keys
   go to C87EH, the keypad to C82EH, characters to the typing path.
4. **The function dispatcher** (C901H) jumps through a table at **C916H**: 63 two-byte
   handler addresses indexed by B (0000 = the key does nothing in the editor).

The indices found, by pressing each key and stopping at the dispatcher's `PCHL` (C915H):

| Index | Key | Handler | What the handler does |
|---|---|---|---|
| 01 | 2DH OUT | C994H | returns command 2 to the command level |
| 02 | 3DH IN | C997H | returns command 1 |
| 03 | 3FH | D665H | far call 28.02 |
| 05 | 7DH PRINT | C9A0H | returns command 3 |
| 06 | 73H HOLD | C9A3H | toggles 04AFH, shows HOLD |
| 07 | 14H RETURN | C9AFH | new line |
| 08 | 76H | C9C2H | **MAR REL** (per the user): sets flag 04B4H and redraws the ruler. With CODE held, 76H opens the DC84H layer instead |
| 0A | 71H MAR SET | C9CFH | margin set |
| 0B | 75H CLEAR TAB SET | C9D5H | toggles a tab stop |
| 0D | 74H | C9E7H | |
| 0E | 06H backspace | C9FAH | cursor back one column |
| 10–14 | 3EH CHAR, 2EH WORD, 7EH LINE, 0EH PARA, 5FH PAGE | CA36H | selects the unit (0–4, in 04ECH) and shows its name |
| 15 | CODE + 7FH | CA44H | |
| 16 | 7FH | CA51H | |
| 17 | 2FH SKIP | BC22H | |
| 18 | 0FH UP | BCBAH | |
| 19 | 1FH DOWN | BD4FH | |
| 1A | CODE + 06H | B693H | cursor back to column 1 (a carriage return, text unchanged) |
| 1C | **05H** | CA60H | returns **command 4** to the command level: brings in the document named on the typing line (below) |
| 1D | CODE + 05H | CA63H | returns command 5 |
| 21 | 69H | CA7DH | moves the cursor to column 80 |
| 26 | CODE + 14H | CAA2H | new line |
| 29 | CODE + 0FH | CAA6H | |
| 32 | CODE + 3FH | CAD3H | |
| 37 | 5CH | D68FH | far call 3B.00 (shows `.00`: decimal alignment?) |
| 39 | **01H** | CB09H | far call **38**.00, **not present** on this disk |
| 3A | CODE + 01H | CB15H | far call 38.00 with the unit, not present |

The keypad keys 1CH and 4CH (codes E5H, E0H) also go to segment 38. Indices 27H and 33H go to
segment **2B**, which isn't present either. So keys 01H, 1CH and 4CH do nothing on this disk
because their feature isn't installed. Segment 38 handles keypad keys and key 5CH shows `.00`,
so it may be a math (column arithmetic) feature [HYPOTHESIS].

**The remap tables** in "get the next key" turn the reader's result into these indices.
`python tools/cptkeys.py <memory dump>` lists them all.

| Table | Used for | Notes |
|---|---|---|
| DBF6H | plain function keys: a byte per code A0H + n | 0 = ignore the key |
| DC59H | CODE + a function or keypad key: (value, index) pairs | e.g. CODE + IN → 2EH (builds CONTENTS), CODE + CLEAR TAB SET → 0CH, CODE + MAR SET → 09H |
| DC14H | CODE + a character key, upper-cased | the menus, below |
| DC84H | a character typed after **CODE + key 76H** | a third layer: CODE held, key 76H sets a flag (2595H) and the next character uses this table |

A second jump table at **CB9EH** handles indices 51H and up. Most of its entries are CCF1H,
the error beep (message 1). SHIFT makes no difference to any function key.

CODE + a letter opens the CPT's menus and modes, seen on the title line:

| Chord | Index | Effect |
|---|---|---|
| CODE + L | 1F | `LINE SPACING  1 2 3` |
| CODE + P | 3E | `PROGRAM  Display Load Label` (the keyboard program's menu) |
| CODE + O | 36 | `OPERATION NUMBER`: restarts into another operation (system build) on the program disk (see "Operations") |
| CODE + H | 1E | `Automatic Wraparound Manual` (hyphenation) |
| CODE + S | 22 | `Direct Control page` |
| CODE + A | 28 | `1-20` |
| CODE + U, T, R | 30, 24, 2C | `CODE:` prompts (code entry) |
| CODE + N | 37 | `.00` (decimal alignment?) |
| CODE + C | 1B | centers the line, then a new line |
| CODE + J | 2D | moves the line right, probably flush right [HYPOTHESIS], then a new line |
| CODE + M | 2F | inserts a format line (a ruler) |
| CODE + X | 2B | changes the case of the character under the cursor |
| CODE + backspace | 1A | the cursor back to column 1 |
| CODE + I, K | 27, 33 | segment 2B, not on this disk. On the TTY disk (cpt2489) CODE + I loads a control page (`CTTY` turns communications on) and CODE + K opens the communications menu |
| CODE + D, E, V, W | 3B, 3C, 25, 31 | nothing visible yet |

Two features this disk doesn't have:
- **Segment 41** handles key **0DH** and CODE + 0DH. That's very likely **WORDPAK**: IN, OUT
  and PRINT are 3DH, 2DH and 7DH, and WORDPAK sits in the same panel column [HYPOTHESIS].
- **Segment 38** handles key 01H, the keypad keys E0H/E5H, CODE + 01H and CODE + 4CH. It may
  be a math (column arithmetic) feature [HYPOTHESIS].

**ERASE, INSERT and MOVE don't appear as separate keys in any of the four tables.** Every
function code a key produces is listed above. What the remaining keys do:

- **4FH** (code B4H, index 0FH, CA17H) is mapped as **ERASE** for now [HYPOTHESIS]. With the
  cursor on text it blanks the character under it and moves on, whatever unit is selected.
- **74H** is **TAB**: it moves the cursor to the next tab stop (column 80 with none set).
  CODE + 74H inserts a tab character (`>` on screen).
- **CODE + OUT** shows `LINE LIMIT 0`; **CODE + PRINT** shows `PRINTER A B C`.
- **05H reads the typing line as a document name and brings that document in** at the
  typing line, in place of the name [CONFIRMED by running it and by disassembly]:
  - Typing `wn` over the start of the typing line ("P. O. Box 7004") and pressing 05H reads
    one sector from the disk (t34s15, WN's data) and the line becomes `WORDNET.  RATE: 9600`,
    WN's text. `mid` gives `  5617  8100  H2` (MID's text). `legal` reads LEGAL's 27 sectors
    and fills the upper screen with columns of numbers (LEGAL is a format document, its
    contents are numeric settings) [HYPOTHESIS for the reason LEGAL shows as numbers].
  - The name ends at the first space. It may start with a drive (`1:`) and is parsed into
    260AH (length), 260BH (drive − 1) and 260CH (up to 16 characters) by F120H–F17FH; F18AH
    keeps a copy at 261CH (the last name used) [CONFIRMED by disassembly; the drive prefix
    not yet run].
  - The path: the handler returns command 4 to the loop around the editor (EF07H, block 1),
    which calls EAA8H (segment 31 function 00). EAA8H refuses (beeps) on an empty filler row
    or a format line, allows up to 5 levels (counter 2633H), parks the typing line in the
    one-line buffer at 4F00H (pointer 04E7H; it's linked to the ruler at 50D4H), allocates
    two 256-byte buffers (2631H, 262EH), then ECE3H copies and checks the name. Segment 0C
    function 00 (stub F298H) loads the document with A = 1 and is called again with A = 0
    when the level closes.
  - The far-call stubs it uses: F26EH = 23.0D, F274H = 23.11, F27AH = 23.15, F280H = 23.12,
    F286H = 00.00, F28CH = 12.07, F292H = 12.08, F298H = 0C.00, F29EH = 23.03,
    F2A4H = 23.1E, F2AAH = 29.01 (show a message). (Segment 23's table pointer, B23EH, is
    its count byte; the addresses start at B23FH.)
  - **A bad name is what made the line vanish** in the earlier tests. With "Mr. Carl Wales"
    on the typing line the name is `Mr.`, ECE3H rejects it, shows message 6.53
    `THE #PAGE LABEL CONTAINS AN IMPROPER CHARACTER.` (`#` is filled in from 2639H), plays
    the two-tone error sound (2000 Hz then 1000 Hz) and leaves through EC74H. That exit frees
    the buffers and drops the level back to 0 but doesn't put the parked line back from
    4F00H, so it's gone from the document [CONFIRMED by running it; whether the real machine
    also loses the line here is VERIFY].
  - Messages 6.51–6.55 around it all talk about "page labels", so the name is probably
    called a page label in CPT's terms [HYPOTHESIS].
  - **CODE + 05H** (command 5) closes the level. Scrolling with UP/DOWN inside the level
    works like normal editing.

  This is a "bring in a stored document" (merge or recall) key, not MOVE or INSERT. Per the
  user it is the key under WORDPAK, labelled with a staple shape (a bar with a leg down at
  each end); it's mapped as STAPLE in the browser panel. It fits that column: IN, OUT, PRINT
  and 05H are the four keys whose handlers return a command (1–4) to the command level. Its
  number doesn't follow the column's low-digit pattern (that would give 1DH), but PAGE (5FH)
  breaks the pattern too.

## Operations (CODE + O) **[CONFIRMED by disassembly, by running it and from the OP documents]**

A program disk can hold several **operations**: separately built configurations of the
system, each with its own set of features. They're probably built with the "BUILD AN
OPERATION" screen (on cpt2489's `OP` page), which writes them to the program disk. The `OP`
document lists the operations on the disk and their features.

**CODE + O** (editor index 36, far call 28.05 = segment 40 function 5, A68DH in block 0 on
cpt.img) switches between them:

1. It shows `OPERATION NUMBER` on the title line and reads a number.
2. It finds a drive with a disk (A6B4H: tries drives 0–3).
3. It reads that disk's boot sector into a fresh page (A6D1H–A731H) and copies it to 0000H
   with interrupts off (A737H–A73FH).
4. It jumps to 0000H with **D = the drive** and **E = the operation number**. A number of
   20 or more becomes 0 (A74EH).

So choosing an operation reboots the machine into that build. The boot code takes the
operation number in E. A plain power-up boots operation 0, since the emulator starts the
boot sector with D = 0 and E = 0.

Running it on cpt.img:
- **CODE + O, `1`, RETURN** restarts and loads a different set of sectors. The chime plays
  twice, at 125M and 152M cycles, which may be a second restart [VERIFY].
- **Operations 0 and 2** load the normal build. There's no operation 2 on this disk, so it
  seems to fall back to 0 [VERIFY].

**cpt.img's operations** (its `OP` document, "CONFIGURED FOR: PSA", 3/28/84, by Tracy, "8100
w/Rotary 5", "SPELLING VERIFIER, BACKGROUND SEARCH, & OUTLINER ADDED"). Both are WORD
PROCESSING, with printer type 8X00 STANDARD (Rotary IV–IX) and English hyphenation and
system messages:

| Operation | Features |
|---|---|
| 0 | Keyboard to Printer, Control Page Printing, Footnotes, Output to Disk, Move, Screen Sort, Text Insertion, Background Search/Locate/Replace, Shortcut, MathPak, DU30, Outliner |
| 1 | Spelling Verifier, Control Page Printing, Output to Disk, Move, Background Search/Locate/Replace, Outliner |

Older copies on the same disk:
- `OP.00.0` ("PACIFIC SOUTHWEST AIRMOTIVE", March 5, 1984) is operation 0 without the
  Outliner, and with plain Search/Locate/Replace.
- `OP.00.1` and `OP.01.0` are single-operation pages, each ending "TOUCH RETURN TO CONTINUE".

**cpt2489's operation** (its `OP` document, "BUILD AN OPERATION", "CONFIGURED FOR: PSA", by T.
Reed, 12/30/85, "H2 8100"): only **operation 0**. It's 8100 WORD PROCESSING with English
system messages and hyphenation, **COMMUNICATIONS TTY** with English communications keywords,
and printer type STANDARD (Rotary IV–IX). Its features:
- Keyboard to Printer, and Control Page Printing with Footnotes and Output to Disk.
- "All of the following": Move, Screen Sort, Text Insertion, Search/Locate/Replace, DU30.
- "ESTIMATED: WORKBLOCKS 37 MEMORY PORTIONS USED 113".

The page ends "THE OPERATION HAS BEEN WRITTEN TO THE PROGRAM DISK."

**What this tells us:**
- **Move** and **Text Insertion** are in every operation, so the MOVE and INSERT keys should
  work in the running system. They haven't been identified yet ("The editor's key
  dispatch").
- **MathPak** (operation 0 on cpt.img only) is a likely match for the missing segment 56
  (38H), which keys 01H, 1CH and 4CH and CODE + 01H call. cpt2489 has no MathPak, and
  segment 56 isn't present there either.
- **Spelling Verifier** (operation 1) fits the `LEX` dictionary document.
- **Communications TTY** is segment 43 (2BH) on cpt2489.
- **Which segments make up a feature** can be found by comparing the segment maps of
  operations 0 and 1. That comparison hasn't been done yet (the run was stopped).

**Not yet done:**
- The segment maps of operation 1 against operation 0.
- What operation 1's second chime is.
- Whether the operation's features are read from the `OP` page or built into the sectors
  that operation loads.

## Disk utilities (DU30?) **[the messages CONFIRMED; the utilities' code is NOT on these disks]**

Per the user, CPT had a way to erase a data disk. cpt2489 has a whole disk-utility suite
in its message file; cpt.img has none of it.

**The text** (cpt2489, logical sectors 655–664, among Screen Sort's strings):
- **DISK ERASURE:** "----- WARNING -----", "ALL INFORMATION ON DISK WILL BE DESTROYED",
  "Please insert the disk to be erased", "Insert any additional disks to be erased",
  "The disk in station# cannot be erased", "- ERASURE COMPLETED -".
- **PAGE ERASURE:** "Please select the type of erasure", "Please enter the page range to be
  erased", "The specified page does not exist", "PAGE ERASED:", "Would you like to continue
  page erasure?".
- **DISK DUPLICATION** ("Please place a BLANK disk in station 2", "The disk CANNOT be
  duplicated"), **DISK RECONSTRUCTION**, **DISK BACKUP** and **DISK RESTORE** (also to a tape
  cartridge: "Please place a cartridge in backup unit", "Please assign a name to the backup
  tape").
- **SRS:** SRS DISK BACKUP, SRS DISK RESTORE, SRS REMOTE SYSTEM LOAD, and "To open a section,
  use the SRS Key". SRS is also a destination in CODE + PRINT's `PRINTER A B C SRS` on this
  disk. It's probably CPT's shared-resource network [HYPOTHESIS].
- "PLEASE SELECT THE NEXT OPERATION", and the Screen Sort messages ("SORTING CANNOT PROCEED",
  "Sorting Finished", "The record is too large", "Arranging").
- "Something is wrong with the IN instruction in the control page" (and the same for OUT).

**Its format** is not the numbered messages' (`3.01`, `6.53`, …). It's a string table: 4-byte
entries (id low, id high, offset low, offset high) ending `00 00`, then the strings, each
ending CR. DISK ERASURE is id 012DH of its group. The ids overlap the system's message
numbers (012DH = 301 = message 3.01, a P-page message), so they're local to the program that
uses them.

**DU30** is in the feature lists of both disks' operation 0 ("Operations"). Its strings
sitting next to Screen Sort's suggest it's **Disk Utilities** [HYPOTHESIS]. But cpt.img lists
DU30 and has none of this text, so that's unconfirmed.

**What doesn't start them** [CONFIRMED by running cpt2489, with `--brk-read L660` (the erase
strings) after start-up]:
- **Keys:** CODE + X, OUT, PRINT, STOP, SKIP, HOLD, ADJ, D, E, V, W. Keys 0DH, 01H, CODE +
  0DH and CODE + 01H (nothing on this disk).
- **Operations:** CODE + O 1, 9 and 19. The boot code keeps only 0–19 (`STA 00D8H`; stage 2
  clamps 20 and up to 0), and cpt2489 has only operation 0.
- **Keys held at power-up:** CODE, STOP, SHIFT, LOCK.
- **A control page** whose keyword line is `DISK ERASURE.`, filed with OUT and run with
  CODE + I. CODE + I treats every page as a communications control page and shows
  "INITIALIZATION INCOMPLETE".

Not yet tried: keys 1CH, 4CH and 5CH, and CODE + staple.

**Where the code might be:**
- Neither the strings nor code using them is in memory during a normal session.
- The boot reads the region they're in, but the code is relocated as it loads, so on-disk
  bytes don't match memory.
- The text is segment 30, a message module, not an overlay of code (see below). The code
  that would use it isn't on these disks.

**Three number spaces overlap, so searching for a number in the code misleads.** Each of
these has its own ids, and the same 16-bit value can mean different things:
1. **Numbered messages**, shown by far call 29.01 with DE = the number (e.g. 028DH = 6.53).
   The resident ones are at block 0 D740H on cpt2489, followed by a (number, location) table
   for those kept on disk. The on-disk entries are [number low][number high], optionally the
   number as text (`6.53`) and A1H, then the text, ending 00.
2. **Prompt strings** (the title-line menus such as `LINE LIMIT`, `PRINTER A B C SRS`,
   `=Yes =No`), passed in DE to a prompt routine. For example, segment 40's menu function
   28.03 (A25DH on cpt.img) dispatches on A (00D0H) and calls A7BCH with DE = 02BDH or 02BEH.
   Those are the CODE + PRINT and CODE + OUT prompts, not messages 7.01/7.02. The resident
   prompts are in segment 42's area (block 0, B7xxH–BBxxH on cpt2489).
3. **The utility string table** (above), with its own 4-byte index.

**Keys into segment 40 (28H)** [CONFIRMED by the dispatch tables]:
- CODE + OUT is editor index 2AH, CAADH → 28.03, which shows `LINE LIMIT 0`.
- CODE + PRINT is index 20H, CA77H → 28.03, which shows `PRINTER A B C`.
- CODE + O is index 36H → 28.05; STOP (index 03) → 28.02; index 31H → 28.06.
- 28.04 is reached from indices 38H and 60H, whose keys aren't known.

`tools/cptkeys.py` prints the CODE + function-key table's values 01H–03H as "(no key)". They
are the function codes of IN, OUT and PRINT (A1H–A3H).

**How prompts and string tables are found** [CONFIRMED by disassembly, cpt2489]:
- **29.03 shows a prompt** (B595H). It calls 2A.01 with A = 2 for the table's address, then
  walks a chain of entries [length low][length high][id low][id high][text …], stepping by
  each length until the id in DE matches. A zero length means not found (error 2).
- **2A.01 takes a table number 0–12** (BFB8H) and returns its address from a pointer list at
  BFE3H:

  | Table | Address | What it is |
  |---|---|---|
  | 0 | C736H | |
  | 1 | D796H | the numbered-message directory |
  | 2 | B6E8H | the title-line prompts |
  | 3 | BFFDH | `TEXT:`, `VARIABLE:` … |
  | 4 | C12CH | |

  For a null entry (tables 5–12) it asks far call **26.00**, which returns the address from a
  second list at 9F5DH:

  | Table | Address | What it is |
  |---|---|---|
  | 5 | A6FAH | |
  | 6 | A85BH | |
  | 7 | A188H | |
  | 8 | 9F75H | the code-set prompts `C0-9:` … `C250-255:` |
  | 9 | A379H | |
  | 10 | A4CDH | |
  | 11, 12 | null | |

- **Segment 38 (26H)**, present only on cpt2489, is therefore the communications and code-set
  prompt tables. On cpt.img it's absent, so tables 5–12 don't exist there.
- **All of these tables are resident.** There's no loader for string tables from disk, and
  none of the tables holds the utility strings.

**The utility text is segment 30 (1EH), a message module operation 0 never loads**
[CONFIRMED from the disk and by disassembly; corrects an earlier reading of it as a program]:
- **Every code module on the disk starts with a header.**

  | Bytes | Contents |
  |---|---|
  | +0–3 | `00 00 00 00` |
  | +4 | **segment number** |
  | +5 | `01` |
  | +6–7 | size (2 bytes) |
  | +8–10 | 3 bytes that look like a date or version, e.g. `06 0B 53` |
  | +14 | **function count** |
  | +20 on | one 2-byte offset per function |

  The modules the boot loads are exactly the segments in the segment map: 04, 05, 07, 08, 09,
  0A, 0B, 0E, 12, 13, 14, 23, 28, 29, 2E, 2F, 30, 31, 36, 3A, 3D, 3E, 3F, 1A, 26, 2A, 2B and
  so on. Segment 35's header, for example, gives 27H = 39 functions, matching the map.
- **The module** is at logical 655 (track 40), **segment 30 (1EH)**, size 0F95H (3989
  bytes), 10 functions (offsets in the code part, which starts at module byte 40):

  | Function | Offset | What it is |
  |---|---|---|
  | 0 | 0791H | **table lookup**: DE = a message id → HL = the address of its group's table. It divides the id by 100 and indexes a list at 07ACH with the quotient |
  | 1 | 0000H | **show a message**: HL = id. Looks the string up (function 2) and, if found, displays it through 0F8FH |
  | 2 | 0010H | **find a string**: HL = id → HL = the string's address, or 0 |
  | 3 | 013AH | table of ids 0C9H–0EAH (201–234): `DUPLICATION`, "Please select the duplication type", "Please place a BLANK disk in station 2" … |
  | 4 | 049FH | ids 12DH–14AH (301–330): `DISK ERASURE` |
  | 5 | 0040H | ids 259H–260H (601–608): `DISK RECONSTRUCTION` |
  | 6 | 07C6H | ids 191H–19CH (401–412): `DISK BACKUP` |
  | 7 | 0911H | ids 1F5H–202H (501–514): `DISK RESTORE` |
  | 8 | 0BD5H | ids 386H–3A1H (902–929): the sort messages |
  | 9 | 0A73H | ids 4B1H–4BBH (1201–1211): `SRS REMOTE SYSTEM LOAD`, SRS disk backup / restore |

  Each table is entries of `[id low][id high][offset low][offset high]`, ending `00 00`,
  then the CR-separated strings. The groups are therefore **one per hundred**: 2 duplication,
  3 erasure, 4 backup, 5 restore, 6 reconstruction, 9 sort, 12 SRS.
- **It contains no utility logic:** nothing that erases, copies or repairs a disk. The only
  code is the three helpers above. The display helper (0F8FH) far-calls segment **22H (34)**,
  which isn't on this disk either.
- **Nothing refers to it.** Segment 30 isn't in cpt2489's segment table, operation 0's boot
  doesn't read its sectors, and no code on either disk far-calls segment 1EH. cpt.img has no
  segment 30 module at all.
- So segment 30 is the **message module for the disk-utility operations**, the equivalent of
  the "SYSTEM MESSAGES ENGLISH" setting on the OP page. The operations' code isn't on this
  disk. The messages are there because they were built into this program disk even though
  its only operation (0) doesn't use them [HYPOTHESIS].
- **Other unloaded modules:** an older copy of segment 12 (0CH) at logical 96 (track 6),
  and a second version of segment 19 (13H) at logical 574 (tracks 35–38, with its relocation
  list at 614, which looked like a page named "F").

**How a CPT user reached them** (per a description the user found online): CODE + O with
operation number 1–7:

| Number | Operation |
|---|---|
| 1 | word processing |
| 2 | word processing with less RAM but more printer options |
| 3, 4 | reserved |
| 5 | Work Disk Duplication |
| 6 | Disk Erase |
| 7 | Work Disk Repair |

That matches segment 30's message groups: DISK DUPLICATION (2), DISK ERASURE (3) and DISK
RECONSTRUCTION (6, "Work Disk Repair"), plus backup (4), restore (5), sort (9) and SRS (12).

**On cpt2489, CODE + O 5, 6 and 7 all boot the normal editor** [CONFIRMED by running it, with
`--brk-read L655`]:
- They're identical to each other and to 1, 9 and 19: 656 reads, then the same screen.
- Any non-zero number takes one path. The boot reads t3s5–6 (code with a relocation list)
  instead of operation 0's t5s5–6.
- It then reads logical 911–912, right before `OP.00.0`. These hold binary tables of
  (number, address) entries naming segments such as 3DH, 3EH, 3FH, 04H and 1AH, which looks
  like an operation's module list [HYPOTHESIS].
- After that it loads the same main system as operation 0. The utility module is never read.

So the operation lookup finds no definition for 5–7 and falls back. cpt2489 was built with
only one operation ("BUILD AN OPERATION … OPERATION 0"). It carries segment 30's messages,
but not the operations' code. [HYPOTHESIS: the fallback reading.] Whether "1" in that numbering
is the disks' operation 0 isn't known.

**The operation table and load maps** [CONFIRMED by disassembly of boot stages 2 and 3]:
- **Stage 2** (track 1 sector 1 at 0100H) clamps the operation number to 0–19 and reads
  the disk label into page 0. Label bytes 15H–16H give the stage-3 location: track 1 sector
  4, giving t1s4–15 at 6000H. It jumps there with A = the operation number.
- **Stage 3** keeps the number at 61A4H. Its lookup (6141H) reads an **operation table at
  6B00H** (track 1 sector 15), 8 bytes per operation:
  - Byte 0 is non-zero if the operation exists (probably its module count). It goes to
    61A5H, and the boot later checks it against 81H.
  - Bytes 1–4 are copied to 61A8H. Bytes 3–4 are the **logical sector of the operation's
    load map**.
  - The byte before the table (6AFFH) goes to 61A2H.
  - **A missing operation falls back to operation 0**. If 0 is missing too, it's error 5.

  | Disk | Operation | Entry | Load map |
  |---|---|---|---|
  | cpt.img | 0 | `21 00 00 02 24 00 00 00` | logical 548 (t34s0) |
  | cpt.img | 1 | `1C 00 00 03 75 00 00 00` | logical 885 |
  | cpt2489 | 0 | `22 00 00 03 8F 00 00 00` | logical 911 |

  cpt2489 has no other operations, so CODE + O 5, 6 and 7 run operation 0. That's why they
  all read logical 911.
- **A load map** is a 16-byte header, then one 12-byte entry per module. There are as many
  entries as the operation's count byte: 22H = 34 for cpt2489's operation 0. The module
  loader (61ACH) reads header and entries, then for each entry copies it into a disk
  request (664AH: drive, then entry bytes 0–3, so bytes 2–3 are the sector), reads the
  module's header, and loads it [CONFIRMED by disassembly and by checking every entry]:

  | Bytes | Contents |
  |---|---|
  | 0–1 | `00 00` |
  | 2–3 | **logical sector of the module, big-endian** |
  | 4 | **bank byte**: 0 = bank 0, b = block b − 1 (`OUT 03H`) |
  | 5–6 | **function-table address** (the segment map's "table") |
  | 7 | a byte (01 for segment 36H, otherwise 00) |
  | 8–9 | a second address, probably the module's bank-0 data area [HYPOTHESIS] |
  | 10 | **segment number**: must equal the module header's +4, or it's error 6 |
  | 11 | **format flag**: must equal the header's +5 (01 for normal modules), or it's error 6 |

  - Entries for segments 01, 02 and 25 have flags 05 and 02 and no module header at their
    sector. That's another format, not checked here.
  - Every other entry's sector holds that segment's module, e.g. 05 → 60, 23 → 259,
    29 → 337, 3D → 453, 2B → 875.
  - Segment 30 (1EH) isn't in the map.
- **The segment check means a module can only be loaded as its own segment.** The utility
  module (header segment 1EH) can only be segment 30.
- **How the main program starts:**
  1. Stage 3 jumps to 0040H with A = the operation number.
  2. Start-up (0D98H) stores the number at **0E71H**, runs its init list (0E65H) and starts
     the timers. Then, with H = the operation number, it far-calls **07.00** (13D4H).
  3. Segment 07's function 00 (1CF1H) puts a non-zero operation number into the 0395H text
     buffer, then far-calls a fixed list of start-up hooks: **04.00** (the main program),
     44.08, 0E.05, 49.00, 43.00, 22.01, 1F.00, 20.00, 21.00, 19.00, 69.00, 41.0D, 46.03,
     33.00, 7A.02, 78.02, 1C.00. A missing segment's hook just returns.
  4. **Segment 04** is a tiny module (two copies on the disk, at logical 58 and 528, both the
     same) that far-calls **23.17**, the editor's command level.

  Segment 1EH isn't in the hook list, and no code on either disk far-calls it.

**The experiment that was planned, and why it was dropped.** The plan was to add an
operation 6 to a copy of cpt2489 (a load map with segment 1EH added, plus a copy of segment
04 that calls it instead of the editor). Reading the module showed it can't work: segment
30 is only messages, so there is no program to start. Calling it would at most display text,
and its display helper needs segment 22H, which this disk doesn't have either. Nothing was
patched or booted. (The load-map format above, decoded for this plan, is still right.)

**Where the utilities' code must be** [HYPOTHESIS]:
- **Not on cpt.img or cpt2489.** Their load maps and every module on both disks have been
  checked; the code that erases, duplicates or repairs a disk isn't among them.
- **Operations 5–7 (and 3–4) were separate builds,** with their own load maps and launcher
  modules, probably on a utility program disk. cpt2489's start-up hook list names many
  segments that aren't on it (1CH, 1FH, 20H, 21H, 22H, 33H, 41H, 43H, 44H, 46H, 49H, 69H,
  78H, 7AH), which are optional features and options not built into this operation.
- **To get them:** a disk image with those operations, such as a work-disk utility or
  "operations" disk. cpt.td0, cpt2365.td0 and cpt2489.td0 aren't it.

**Next steps:**
- Look for another CPT disk image with operations 5–7. If one turns up, its segment-30-style
  message module and operation table should match this format.
- Indices 38H and 60H (28.04), whose keys aren't known yet.

## Control pages **[CONFIRMED from the documents; partly by running them]**

A control page is a document that tells a function how to run. Its first line is a title,
and the next line names the function:
- `CTTY`, `TRT`, `TRT1`: `COMMUNICATIONS.` (rate, code sets, lines per page, parity). CODE +
  I, then the name, runs them (see `segments-cpt2489.md`).
- `CONTROL`: "CONTROL PAGE FOR MAILBOX / TRANSMISSION FROM KP/SCREEN / TRANSMISSION FILE: 2
  MAILBOX".
- Screen Sort and the disk utilities mention IN and OUT "instructions in the control page", so
  their control pages have IN/OUT lines too [HYPOTHESIS].

## Filing a document with OUT **[CONFIRMED by running it]**

On both disks, typing two lines, pressing OUT, typing a name and RETURN files the page.
IN with the same name then brings it back ("TEST" on the typing line, "hello world" below).
OUT reads the allocation map (t0s3), a directory sector and a leaf before writing. Filing to
a disk in station 2 and rebooting with the saved image shows the page survives ("Data disks"
below). Filing to station 1, the program disk, was only tested within one session.

**CODE + I is for communications only.** Running a page that isn't a valid communications
control page with CODE + I (e.g. a page whose keyword is `DISK ERASURE.` or `XYZZY.`) shows
**INITIALIZATION INCOMPLETE**. That's a communications message: it sits between "TTY
COMMUNICATIONS ENABLED" and "COMMUNICATIONS DISABLED" in the message file.

## Data disks (station 2) **[CONFIRMED by running it]**

A second disk in station 2 holds documents. It uses the program disk's filesystem, with
less on it. `python tools/mkdatadisk.py OUT.img` makes a blank one, and `cpt-run --cpt-boot
… --disk2 OUT.img --save-disk2 NEW.img` mounts it and saves it back.

**Choosing the station.** The IN and OUT prompts show the station as a digit (`OUT        1
name`). Typing a digit just lands in the name. To change the station, **backspace twice
over the digit and type the new one**: `{2D}{06}{06}2zz{14}` gives `OUT        2 ZZ`. (Backspace
here only moves the cursor back.) A `2:name` prefix is rejected by OUT as an improper
character in the page label (message 6.53). IN with a `n:name` prefix was read in the code
(F120H, drive = the digit − 1) but not run [VERIFY].

**An unformatted disk is formatted by the first write.** A blank disk of all zeros in
station 2, OUT to it, wrote exactly four sectors: the label, the allocation map, the
directory root and the page's first block. IN then read the page back. The same page filed
to the formatted-but-empty disk below gives a byte-identical disk.

**The formatted, empty data disk** (what `mkdatadisk.py` makes; all other sectors zero):

| Sector | Contents |
|---|---|
| t0s0 (label) | only 16 bytes: `00 01 01 02 00 04 D0 00 00 01 00 00 0F 00 00 00`. The program disk's label has `05` at +1 and `01` at +0FH, and a geometry block, a text and more after +10H. +5..6 = 04D0H = 1232 sectors |
| t0s3, t0s6 (allocation map) | 2 bits per logical sector (0 free, 1 reserved, 2 data, 3 system/index). Logical 0–15 (track 0) are reserved (`55 55 55 55`), the rest free. The system writes no padding past the last sector (the program disk has it) |
| t0s13 (directory root) | a **leaf** (`30`, ASCII '0') with no entries. The first 23-byte item slot stays empty, as in the program disk's leaves |

**After the first page,** filed as `ZZ` (24 bytes):
- The map marks logical sector 16 (**t1s2**, by the skew rule) as 3 (system/index), and
  holds the page's `01 ZZ 02 C0 … 03` block.
- The root leaf gets an item in its second slot: the name padded to 16, the field
  (`00 01 00 18`: length 24) and the pointer `00 00 10` (logical 16).
- Only t0s3, t0s13 and t1s2 changed. The label was already there.

**Directory depth: the split works** [CONFIRMED by filing 14 pages, in shuffled order, to a
blank data disk]. A node has 11 item slots (a level byte, then 11 × 23 bytes). The root
leaf's first slot is always empty, so it holds ten pages; the eleventh splits it:
- The root becomes an inner node (`31`) with two items: an unnamed one pointing at the
  **lower** leaf, and one named for the first name in the **upper** leaf, pointing at that.
  Here: unnamed → logical 28 (`P01`–`P06`, with its own empty first slot), `P07` → logical
  27 (`P07`–`P14`, no empty slot).
- The two leaves are allocated right after the eleventh page's block (logical 27 and 28),
  and pages filed later continue after them (29, 30, 31). Leaves are marked 3 in the map,
  like page blocks. So 14 pages use logical 16–31: 14 blocks and two leaves.
- All 14 pages list in order and read back after a fresh boot, including the last name of
  the lower leaf and the first of the upper. A name that doesn't exist gives an empty
  screen, not an error.
- Each of these pages is 12 bytes (`01 P07 02 …`), field `0001000C`. The page typed before
  OUT is not accumulated across OUTs.
- Boot test `a_data_disk_directory_splits_when_a_leaf_fills` walks the saved directory with
  twelve pages and checks every name and its pointer.

Not yet seen: a second split (the inner root filling, 11 leaves), and deleting pages.

**It persists.** Saved with `--save-disk2` and mounted again after a fresh boot, IN 2 ZZ
brings the page back (boot test `a_blank_data_disk_takes_a_page_and_keeps_it`).

## Patch pages **[CONFIRMED by disassembly and by running a real patch page]**

**A working patch page** (`p1.txt` in the repository root, for cpt2489) [CONFIRMED,
2026-11-03]. Typed on the CPT and OUT as `.P1` on the program disk, it is applied at the next
start-up, and OUT to an empty drive 2 then shows `6.11 PUT IN A DISK YOU DUMMY!!` instead of
`THE DISK IS NOT IN PLACE.` What the test showed:
- **The first line is a title and is skipped.** The reader hands the interpreter the text
  after the first line (the TP printer table also starts with description lines). A page whose
  first line is `!xxxxx` fails with error 1: the `!` scan reaches the end of the page.
- **`@segment:offset` counts from the end of the segment's function table.** The interpreter
  calls 01.04 (through 3CF0H on cpt2489) with L = FFH, which returns the table address + 1 +
  2 × the function count. For segment 42 (2AH) that's B6E8H on cpt2489 and AC50H on cpt.img.
- The message texts are resident in segment 2A: 5.1 at CB44H and 6.11 at CD39H on cpt2489
  (offsets 5212 and 5713), and BAF5H and BCEAH on cpt.img (offsets 3749 and 4250). So a page
  for cpt.img needs those offsets instead.
- The checksum rule below is right: `$138` for this page.

The rest of this section is the analysis from before the test.

Per the user, a program disk could carry hidden "patch pages": documents in a special syntax
that patch the code after it has been loaded, to fix bugs. The system has this mechanism, but
**neither disk has a patch page**, so start-up runs the check and applies nothing.

**Where it runs.** Start-up calls a patch processor once, after the main system is in memory.
Addresses are in bank 0 and differ between the two disks:

| | cpt.img | cpt2489 |
|---|---|---|
| entry | 29BDH | 2CECH |
| scan for `!` | 2A25H | 2D54H |
| common exit | 2A96H | 2DC5H |
| entered at | 41.2M cycles | 40.5M cycles |
| left at (A = 0, no error) | 41.3M cycles | 44.7M cycles |

On both disks it leaves without reaching the `!` scan.

**How it finds patch pages.** It looks documents up by name with a request block that holds a
key range:
- **`.P` to `.P9`** (at 2D71H on cpt.img). So patch pages are documents named `.P`, `.P1` …
  `.P9`. The leading dot is probably what hides them from the user [HYPOTHESIS].
- An alternative block (2D94H), used when bit 1 of the mode byte (3B30H) is set: the name
  `P` + four A0H bytes + `P`, up to the same plus `9`. A0H is a non-printing (hidden) space.

On both disks the lookup finds nothing, and a full search of both images finds those names
only in the code's own request blocks.

**The syntax** (from the interpreter at 2A25H–2BD8H on cpt.img; not yet seen in a real page):
- **`!` + five characters** starts a section. The five characters must match the five at
  0395H, the start of the message buffer `xxxxx  *** PATCH PAGE ERROR ***`, or it's an error.
  At patch time those five are literally `xxxxx` on both disks. They're probably a release-ID
  placeholder that the build would stamp [HYPOTHESIS].
- **`@segment:offset (`** sets where the following values go: the segment's base plus the
  offset. Several `:`-separated numbers after the segment are added together.
- **Values** follow, separated by any non-digit (e.g. `,`), stored a byte at a time.
  **`)`** ends the group.
  - Numbers are **decimal** (the parser at 2B9FH multiplies by ten).
  - `-` negates a number.
  - A **`*`** prefix makes a two-byte value: `*n` is the number n, and `*segment:offset`
    is that segment's base plus the offset (an address).
- **`$` + number** ends the page. In the checking pass (mode 0), a checksum accumulated over
  the group values (2C59H, into 3B31H) must equal that number, or it's error 2. A `!` where a
  group is expected is error 3.
- **Errors** are shown as `xxxxx  *** PATCH PAGE ERROR ***` with the error number placed at
  03B5H (1CF6H). The `xxxxx` would be replaced by the page or section ID.

**An example patch page** (syntax only; the addresses are placeholders). It is a document
named `.P1` whose text is (the first line is the skipped title):

```
EXAMPLE PATCH PAGE
!xxxxx
@35:120 (0,0,0)
@35:200 (195,*35:300)
$11
```

- `!xxxxx` is the section header. Its five characters match the ones at 0395H on these
  disks.
- `@35:120 (0,0,0)` writes three bytes of 0 (NOP) at segment 35's base + 120.
  - `@35:` names **segment 35**, which is 23H, the editor. Numbers in a patch page are
    decimal, which fits the user's memory of decimal segment numbers.
  - The address is the segment's base (looked up through 39C1H, which selects the
    segment's bank if needed) plus the offset after the `:`.
  - The interpreter reads one more character after the offset's terminator, here the `(`,
    so the space before it matters.
- `@35:200 (195,*35:300)` writes `195` (C3H, `JMP`) at segment 35 + 200, then
  `*35:300`. A `*` value with a `:` is an address: segment 35's base + 300, stored as two
  bytes, low byte first. So the group plants `JMP <segment 35 + 300>`. A plain `*n` is a
  two-byte number.
- `$11` is the checksum. It's taken over every character from each `@` through its `)`,
  including the spaces; the line ends between groups don't count. For each character c (a 0
  counts as 1), sum = (c XOR sum) + 1, kept to a byte. The two groups above give 67, then 11.
- The one call checks the whole page first: the header, then the checksum (error 2 if wrong).
  It then scans for the header again and applies the groups.
- Offsets are relative to the segment's base, the address `cptsegs.py` prints as the
  table, so a real patch would be computed from the segment's code. The values here are
  placeholders, chosen only to show the syntax.

**The system's own messages about patch pages** [CONFIRMED from the message file, both
disks]:
- 3.01 is "P PAGE DOES NOT MATCH SOFTWARE LEVEL. P PAGE DID NOT LOAD." So the five
  characters after `!` are the **software level** the page was written for.
- 3.02 is "P PAGE HAS BEEN ALTERED. P PAGE DID NOT LOAD." That's the checksum failing.

**The same syntax holds printer tables.** The `TP` document on cpt2489 ("tp.50045 rev [gen:8]
usa 88 … v2.07 … This Printer Table should be written to page label "TP" of the program
disk") is a `!` line followed by `@( … )` groups of decimal numbers (character widths). So
the `!`/`@` interpreter is the system's general loader for tables kept as pages, and patch
pages are one use of it. In `@(`, a group with no address, the values seem to go to the
table being loaded [HYPOTHESIS].

**Later reuse of 0395H.** At 42.6M cycles (cpt.img) a different routine (36E0H–37FEH) builds
a volume ID in the same buffer: the label's 16-character name, then `-` and three numbers
from the label. By the time the system is settled the patch message is gone from memory,
which is why it isn't in a post-start-up dump.

**Next steps, if wanted:**
- Write a small `.P` page in this syntax into a copy of a disk image (via the filer's
  directory format, see "The directory"), boot it, and see whether the patch lands and how
  errors show. That would confirm the syntax.
- Find the five-character ID check's intent: whether `xxxxx` is a placeholder a build would
  stamp with a release ID.
- Confirm what 39C1H returns as a segment's "base": its function table or the start of its
  code.

## The printer **[CONFIRMED by disassembly and by printing on both disks; line names HYPOTHESIS]**

Per the user, a workstation drives one printer directly. Sharing a printer meant saving a
document to the SRS (Shared Resource System, a shared hard disk server) so that the
workstation with the printer could print it. CODE + PRINT's `PRINTER A B C SRS` picks the
destination.

**The port.** A parallel daisy-wheel interface, much like a Diablo HyType II:

| Port | Use |
|---|---|
| 42H out | control register: idle 41H; a command is a pulse `(strobe AND BEH) OR 41H`, then 41H (9458H on cpt.img, interrupts off) |
| 43H out | data latch: a 12-bit word, written high byte then low byte before the strobe |
| 43H in | status |
| 44H/45H | the same for printer B (selected by the printer index; A1F2H–A218H) |

| Strobe | Command | Word |
|---|---|---|
| 80H | restore, at the start of each page | none |
| 08H | strike a character | the print-wheel table entry, complemented (bit 11 flipped for bold [HYPOTHESIS]) |
| 10H | carriage move | low 10 bits in 1/60 inch (6 = one pica column); bit 10 (400H) = to the left |
| 20H | paper feed | low 10 bits in 1/48 inch (8 = one line at 6 per inch) |
| 02H, 04H | other commands seen in the driver, not used by a plain print | |

| Status bit | Use |
|---|---|
| 7 | on and ready; waited for after restore. 00H (no printer) gives "THE PRINTER SEEMS TO BE OFF." (8.51) |
| 6 | waited for before each character |
| 4, 2 | waited for before other commands (masks 14H, 10H) |
| 1 | **new sheet**: at the end of a page the CPT shows "PLEASE PREPARE THE PRINTER FOR A NEW PAGE." (8.57) and polls every 0.1 s until this bit differs from what it was (A219H) |
| 0 | read after strobe 04H |

The other printer messages are 8.53 (new ribbon), 8.54 (reporting problems), 8.58 (does not
respond) and 8.59 (waiting); which status bits raise them hasn't been traced.

**The print-wheel table** maps character codes to words. The driver finds it with
`CALL 00EEH; LXI H,table; CPI 02H; JNZ`: 9632H on cpt.img and 999AH on cpt2489, both in
block 1. Its first byte is its format: 1 = a byte per code; 2 = two bytes per code from 20H;
3 = a count, then two bytes per code from 20H; anything else = the first code, then a count,
then two bytes per code, sent complemented. Both disks use the last (from 20H, 90H codes,
87 distinct characters). Codes F0H and up are looked up as code − 50H.

**In the emulator** (`crates/cpt-machine/src/cpt_printer.rs`, in `CptSystem` as printer A):
- `CptPrinter` reads as always ready (status F4H) and logs every strobe with its word.
- With `auto_sheet` (on by default) it loads the next sheet when the CPT waits for one: 20
  status reads in a row with no command between them flip status bit 1. A printer that is
  off (bit 7 clear) loads nothing.
- `PrintWheel::find` reads the table from memory; `render_text` lays the characters out by
  carriage and paper position (6 units per column, 8 per line), a form feed between pages.
- `cpt-run --cpt-boot` reports the printer and shows the printout. `--printer-text <file>`
  saves it, `--printer-log <file>` saves the raw commands, `--printer-status 00` unplugs it.
- `--watch-io 42-45` logs every IN and OUT on those ports with the instruction that did it
  (added to trace this).
- The browser page has a printer pane that shows the printout growing line by line, with
  ON/OFF, TEAR OFF and SAVE (see `crates/cpt-wasm/README.md`, "The printer").

Example: `{3D}rc-l1{14}{ms 8000}{7D}{ms 3000}{14}` (IN RC-L1, PRINT, RETURN) prints the
two-page letter to Carl Wales on cpt.img (boot test
`print_sends_the_letter_to_the_daisy_wheel_printer`).

**Ports 60H–65H** are a different device: code in block 2 ECD0H–EE80H on cpt.img polls 61H
about a thousand times during start-up, uses DMA channel 1 (12H/13H, mode 73H on 18H), and
compares replies with 06H, 13H, 15H, 17H and 1FH (ACK, XOFF, NAK, ETB, …). It may be the SRS
link [HYPOTHESIS]. It isn't modelled; with nothing there start-up carries on.

## The speaker **[CONFIRMED by disassembly and by running it]**

The speaker is driven by the 8253's **counter 0** (port 1CH; control port 1FH). Counter 0
raises no interrupt. The sound routine at **153AH** takes a sound number in A and plays every
entry for that number in the table at **1581H**. Each entry is 7 bytes:
- the sound number;
- the "on" control word (`36H`: counter 0, LSB then MSB, mode 3 = square wave), or 00 for a
  rest;
- the "off" control word (`30H`: counter 0, mode 0 = silent);
- the count (2 bytes, little-endian), giving 2 MHz / count;
- the duration (2 bytes) in ticks of the 100 Hz task timer (routine 1603H waits on 04A6H).

The table ends with `FF`.

| Sound | Notes | Heard when |
|---|---|---|
| 0 | 1,333 Hz for 50 ms | a key click: IN, PAGE (typed letters are silent) |
| 1 | 2,000 Hz for 100 ms, then 1,000 Hz for 250 ms | start-up, about 22 s in |
| 2 | a rest | |
| 3 | 1,600 Hz for 100 ms, then 1,333 Hz for 100 ms | not yet seen |
| 4 | 256 Hz for 250 ms | not yet seen (an error buzz?) |
| 63H | two empty entries | |

The tones confirm the 8253 runs at the CPU's 2 MHz. Played in the browser, the user confirmed
the start-up chime and key clicks sound as they remember them from the real CPT. No separate speaker on/off port has been
found; mode 0 alone silences it. `cpt-run --cpt-boot` lists the speaker changes ("speaker (6
changes): …"), and the browser plays them.

## The keyboard **[CONFIRMED by disassembly and by typing]**

- **RST 2** (handler 0D07H). IN 01H bit 7 means an event is ready, and IN 02H gives it,
  inverted. The handler acknowledges with `OUT 00H, 04H`.
- **Events:** key number 01H–7FH = press, +80H = release, and 80H alone = all keys up. Event
  00H (an idle line) is ignored. Key 3FH also sets ten flags at 04F1H.
- **The ring.** The handler puts events in a 128-byte ring at 058BH (write index 14ECH; if the
  ring is full, 80H goes in instead). The editor task (D776H, block 2) takes them out (read
  index 258FH).
- **The table.** The task translates through the table at DEFFH (block 2): the unshifted value
  is at `(k & 0FH) * 8 + (k >> 4)` and the shifted one 7FH further on. Table value 80H = CODE,
  81H = SHIFT, 82H = LOCK, all held as state. The table is in
  `cpt_machine::cpt_keyboard::KEY_TABLE`.

Key numbers, from the table and a sweep that typed each function key into the running system:

| Key | Table value | Effect seen |
|---|---|---|
| 63 62 53 52 43 42 33 32 23 | a s d f g h j k l | (home row; the other letters and digits follow a typewriter layout) |
| 10H | 20H | space |
| 14H | B6H | **RETURN** (new line; the line count goes up) |
| 06H | B5H | **backspace**: moves the cursor back one column without erasing; the next character typed overwrites (so "Hello", backspace, "world" gives "Hellworld"). Not ERASE |
| 4FH | B4H | a space [HYPOTHESIS: required space] |
| 07H, 77H | 80H | CODE |
| 70H | 81H | SHIFT |
| 72H | 82H | LOCK |
| 3EH, 2EH, 7EH, 0EH, 5FH | ABH, ACH, ADH, AEH, AFH | unit: CHAR, WORD, LINE, PARA, PAGE (shown on the status row) |
| 3DH, 2DH, 7DH | A1H, A2H, A3H | **IN**, **OUT**, **PRINT**: open a prompt on the top line (`OUT        1 WORLD`: a number and the name typed, upper-cased) |
| 73H | AAH | HOLD (status row) |
| 71H | B9H | margin set (83 on the ruler) |
| 75H | B8H | **CLEAR TAB SET**: toggles a tab stop at the cursor's column (84 on the ruler); a second press there clears it. A momentary key, not a locking one (per the user, then confirmed) |
| 69H, 74H | DFH, B7H | moves the cursor to column 85 (the window scrolls with `C1`) |
| 18 49 19 59 4A 1A 5A 4B 1B 5B | E6 E7 E8 E9 EA EB EC ED EE EF | keypad 0–9 |
| 6AH, 6BH, 6CH | E4H, E2H, E3H | keypad `.` `,` `-` |
| 5CH | E1H | deletes the next character and shows `.00` on the top line [VERIFY] |
| 2FH | B0H | removes the typed word from the typing line [VERIFY] |
| 0FH | A5H | **UP**, by the selected unit: the text moves up (on through the document) |
| 1FH | A6H | **DOWN**, by the selected unit: the text moves down (the upper screen into the lower) |
| 02H | A8H | **PROG**, the keyboard program (from the user's memory, then confirmed): CODE + PROG starts recording keys ("PROG" on the title line), CODE + PROG again ends it, and PROG plays it back. How a program is erased isn't known yet |
| 2FH | B0H | **SKIP**, by the selected unit: discards text after the cursor. PAGE then SKIP empties the lower screen (the display list then ends at the ruler); LINE then SKIP drops one line |
| 3FH | A4H | **STOP** [HYPOTHESIS: same key column; the keyboard interrupt handler treats it specially] |
| 7FH | B1H | **ADJ** [HYPOTHESIS: same key column; it rejoined lines and left the typing line blank] |
| 01 05 0D 1C 4C 6F 76 | B3 B2 BD E5 E0 FE BA | nothing visible when typed alone or with CODE (1C, 4F and 5C with CODE beep) |

The special keys' numbers follow the keypad's columns: the keys in one column share the low
digit. IN, OUT and PRINT are 3DH, 2DH and 7DH. STOP, SKIP, ADJ, UP and DOWN are 3FH, 2FH, 7FH,
0FH and 1FH. By that pattern WORDPAK would be 0DH [HYPOTHESIS; not mapped].

### Auto-repeat **[CONFIRMED by disassembly and by running it]**

The keyboard sends only a press and a release for each key; the software does the repeating.
The keyboard task (segment 3D function 0A, D74CH, block 2 on cpt.img) repeats a held key
while no other key event arrives, **if the key's code is on a repeat list**.

- **Timing.** When a new key arrives (D80AH), the delay before the first repeat is set to
  **1EH = 30 ticks**. After each repeat (D7E0H) it's **3 ticks**. The task yields (0052H)
  until the tick counter at 04A6H has moved on by the delay since the key was last taken
  (2591H).
- **The tick** is counted in the RST 7 handler (116FH), the task-switch timer, which is
  100 Hz in the emulator [HYPOTHESIS for the real rate]. That gives **300 ms, then a repeat
  every 30 ms** (about 33 a second).
  - Measured in the emulator: SPACE repeats every 30 ms, and UP about every 37 ms (scrolling
    takes longer than a tick).
  - The first repeat came after about 200–250 ms, sooner than 300 ms [VERIFY: where the
    delay is counted from].
- **The list** is entry 2 of a table at DEF5H (lookup DDC9H with L = 02): a count, then the
  codes. The held key's code is the key-table value, shifted if SHIFT is held (D919H).
  cpt.img's list, at DFE2H, has 17 codes:

| Code | Key (number) | Notes |
|---|---|---|
| 20H | SPACE (10H) | |
| 2EH | `.` (20H) | |
| 2DH | `-` (16H) | |
| 8FH | SHIFT + `-` (16H) | probably the underscore / underline character [HYPOTHESIS] |
| 2AH | `*` (SHIFT + 37H) | |
| 78H, 58H | `x` and `X` (61H) | the only letters: for x-ing out text [HYPOTHESIS] |
| A5H | **UP** (0FH) | |
| A6H | **DOWN** (1FH) | |
| B0H | **SKIP** (2FH) | |
| B1H | **ADJ** (7FH) | |
| B4H | **ERASE** (4FH) [HYPOTHESIS for the name] | the one function key whose handler blanks a character and moves on, so repeating fits |
| B5H | **backspace** (06H) | |
| B6H | **RETURN** (14H) | |
| A8H | **PROG** (02H) | a held PROG plays the keyboard program again and again |
| 00H | (none) | 00 = no key in the key table, so it never matches |
| FBH | (none on this keyboard) | only from SHIFT + 7AH / 5EH, which aren't keys (their entries spill over from the table's end). The alternate key table (below) has it on SHIFT + IN (3DH) |

  Keys **not** on the list don't repeat. Among them: the other letters and digits, CHAR,
  WORD, LINE, PARA, PAGE, IN, OUT, PRINT, STOP, HOLD, MAR SET, MAR REL, CLEAR TAB SET, CODE
  and the staple key. A held letter types once [CONFIRMED by running it].

- **An alternate set.** DDC9H uses a second table at E049H instead of DEF5H when the flag at
  E203H (toggled by E1D9H) or 04C6H is set. It has its own key table (E053H) and repeat list
  (E15EH, 16 codes: the same without the 00). It's probably another keyboard layout
  [HYPOTHESIS].
- **The browser panel** holds a drawn key down for as long as the pointer is pressed, so
  this repeat works there too (`cpt.js`). A quick click is still a 40 ms tap.

**Not yet confirmed:** the real tick rate, and the 200–250 ms first repeat measured in the
emulator against the 300 ms the code suggests.

**The system checks disk timing too [CONFIRMED by running it].** It isn't only the loader.
With every read after the first 32 shortened, the start-up marquee changes from the copyright
notice ("CPT® WORD PROCESSING SOFTWARE. COPYRIGHT© 1983 BY CPT CORPORATION. ALL RIGHTS
STRICTLY RESERVED.") to "THE DISK IN STATION 2 SEEMS TO BE FAULTY." Reads of 2,000, 4,000,
10,000 and 20,000 cycles all give the message; 32,000 passes.
- **Why:** a real 8-inch disk at 360 rpm takes about 31 ms between consecutive logical
  sectors in the skew-3 order, and the software expects that.
- **Consequence:** start-up can't be sped up with faster reads. The browser runs the
  emulation faster than real time instead.
- **Glyphs:** the marquee shows `8B` = ® and `8C` = ©.

**Modelled timing [HYPOTHESIS]:** one event is latched at a time, and the next comes 1 ms
(2,000 cycles) after the previous one is read. The real keyboard link's timing is unknown.

## Opening documents **[CONFIRMED by running the real system]**

**IN** (key 3DH), a document name and RETURN (key 14H) open a document through the system's
own filer, for example `cpt-run --cpt-boot … --keys "{3D}rc-l1{14}"`. Every document in the
directory opens:

| Document | What it is |
|---|---|
| LEGAL, STD, T, PLASTIC | print-wheel tables ("T.017-5 REV (G-1-1) USA LEGAL 03/31/81"; numbers per character) |
| D | "D-Page 8100/8515/8525 07/01/83", a printer/settings page with field markers |
| OP | the OPERATION page: printer type "8X00 STANDARD (Rotary IV-IX)", hyphenation language, system messages, keyboard-to-printer |
| WN | the WordNet control page (communications; RATE: 9600) |
| LEX | a spelling dictionary ("LEX for Miscellaneous Disk") |
| RC-L1 | a letter ("Letter to Carl Wales", 8/19/87) with its own format line |
| HM-M1.2 | a memo ("Memo to Messrs. Bell and Phillips", October 21, 1987) |
| MID | its first line is the volume label text, `5617  8100  H2` |

**Screen layout with a document open.** The typing line (row 46) holds the current line. The
lines before it fill the rows above, and the next 7 lines go below the ruler and status row
(rows 49–55). So the typing line is mid-screen, with text both above and below.
- **Below the status row:** the rows are built from 16-byte cells at 7E00H–7FFFH, chained
  by `C2` links and padded with `B2`.
- **UP** (key 0FH) and **DOWN** (key 1FH) move through the document by the selected unit
  (CHAR, WORD, LINE, PARA, PAGE), or by one line with no unit selected. The names are the
  user's:
  - UP moves the text up, on through the document. PAGE then UP goes to the end of the
    page (line 79 in RC-L1).
  - DOWN moves the text down. After CODE + IN and RETURN the table of contents fills the
    upper screen; PAGE then DOWN scrolls it completely into the lower screen, back to line 0.
  - Key 7FH moved two lines and left the typing line blank [VERIFY].
- **The screen is 54 rows [CONFIRMED from the system's variables].** That's the title line,
  45 rows through the typing line (04ABH = 45), the ruler, and 7 rows below it (04ACH = 7).
  Before the system sets up its layout these hold 55 and 1. The display list can hold more
  rows of text after the 7 below the ruler (the TOC's ran to row 80), but they aren't on the
  screen. An earlier 80-row limit showed them, and the user saw a lower half with far too
  many lines.
- **CODE + IN** (no name) builds a table of contents document, CONTENTS: one line per
  document with its first line and name ("LETTER TO CARL WALES . . RC-L1").
- **The ruler** takes the format of the current line: in RC-L1 the right margin moves to 60.
- **The letter's format line** is a ruler stored in the document, starting with `B3`, and is
  shown white on black.

**The filer reads header blocks directly.** It did not re-read the directory; it had cached
it at start-up. The sectors it read:

| Document | Leaf "pointer" | Sectors read (track, physical sector) | Logical sector of the first read |
|---|---|---|---|
| LEGAL | 768 (t48, logical 0) | t48 s0, s3, s6, s9 | 0 |
| T | 720 (t45, logical 0) | t45 s10, s13, s0, s3 | 14 |
| MID | 552 (t34, logical 8) | t34 s12 | 4 |
| OP | 892 (t55, logical 12) | t55 s2, s5, s8 | 6 |
| LEX | 673 (t42, logical 1) | t42 s7, s10 | 13 |
| RC-L1 | 675 (t42, logical 3) | t42 s13, s0 | 15 |

Every read is a document's `01 name 02` header block and the blocks after it, in skew-3 order.
The first read is exactly the leaf's pointer under the full rule, physical = (3 × s + 2 ×
track) mod 16: T 720 (t45 s0) → s10, MID 552 → t34 s12, OP 892 → t55 s2, LEX 673 → t42 s7,
RC-L1 675 → t42 s13 [CONFIRMED]. (This table was first read with plain skew 3, which made the
pointers look like they missed the header block.)

**Start-up needed two floppy fixes.** The main system reads interrupt-driven: INTRQ raises
**RST 3** (handler 06BAH), and it waits for that in a yield loop at 087CH. It also finds its
drives by selecting 0–3 on port 22H and reading FD1771 status bit 7 (**Not Ready**, routine
0C74H). Without those, the only task waited forever, or the system took the empty drives for
disks, failed to read them, and stopped on **error 1** in its fatal trap at 15C1H. The trap
saves A and the registers at 1691H–169AH and spins at 15DBH.

`--cpt-boot` takes a fifth argument, a file to save memory to at the end (bank 0, then each
32K block), for offline disassembly: `cpt-run --cpt-boot disk/cpt.img 80000000 - 40000
disk/boot-mem.bin`.

## Next steps

1. Finish the filesystem: what a directory leaf pointer points to (not the header block; see
   "Opening documents"),
   how a document's blocks chain (list blocks, the code-`11` blocks among the `10` data), the
   4-byte field, and whether the system area (tracks 1–32) is one structure or several segments.
   Finding the filer's read code in the system area (Segment 12) would settle most of this.
2. ~~Decode the display memory format~~ (done above). Still open: C5–CF, the line attribute for
   reverse video, and the top line's two slots per column.
3. ~~Attach a keyboard~~ (done above). Still open: confirming the reverse-video rule (the
   status row's number boxes), the cursor blink (93C0H, only when 22F9H is set), CODE chords,
   names for the rest of the function keys; whether OUT can save a document (writes to the
   image).
4. Replace the fixed read delay with a rotation model once the timing check is understood, and
   confirm which 8253 counter drives which RST.
5. Disassemble the main code from its entry points and from every `IN`/`OUT` to confirm the
   rest of the port map (keyboard, display, serial, timer, speaker) and the interrupt vectors,
   then update the placeholders in `MachineConfig`.
6. Decode the keyboard code tables.
7. ~~Show the decoded screen in the web UI~~ (done: insert the image in drive 1; see
   `crates/cpt-wasm/README.md`). Open: accepting the `.td0` directly in the page, and the
   speaker (8253 counter 0 was programmed to 1500 when a document opened; probably a beep).

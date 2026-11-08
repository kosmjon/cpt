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
- **Pointers are logical sectors with a skew of 3 within a track:**
  physical sector = (3 × logical sector) mod 16, i.e. logical 897 (track 56 sector 1) is
  physical sector 3 = linear 899, and logical 898 (sector 2) is physical sector 6 = linear 902.
  Both root→leaf pointers resolve this way, and so do `D` (→ its header at 905) and `LEGAL`
  (→ 768). The physical recording has no interleave, so the skew is the filesystem's own.

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

**Not yet understood:**
- **What a leaf pointer points to.** Under skew 3 only `D` and `LEGAL` land on their own
  document's header block; `OP` and `OP.01.0` land on list blocks (below); others land in other
  documents' text or in code. No other skew does better (the best, 7, matches 4 of 14). The
  pointer is probably the root of each document's own index, whose format is still unknown.
- **The 4-byte field.** Six of the documents have nearly consecutive values `000117D3` …
  `000117DA`, which suggests a creation sequence number or date stamp rather than a size.

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
| `80`–`82` | 1 | ruler glyphs: column tick, every 5th, every 10th | ruler built at E4E0H |
| `83` | 1 | ruler: a margin (left and right) | **[CONFIRMED by typing]**: margin set (key 71H) adds one at the cursor |
| `87` | 1 | ruler: the cursor's column | **[CONFIRMED by typing]**: moves with the status row's column |
| `84` | 1 | ruler: a tab stop | **[CONFIRMED by typing]**: tab set (key 75H) puts 84 at the cursor |
| `89` | 1 | [HYPOTHESIS] a tab character in text | treated as a tab/word break (8552H, 863DH) |
| `0D` | – | end of row | every row, builder and messages |
| `B0` | 0 | empty per-character attribute slot (one per column in status rows, two in the top line) | walker skips it; cursor toggles it |
| `B1` | 0 | end of screen | walker substitutes B1 for a link to 0000H |
| `B2` | 0 | filler | walker skips it |
| `B4` / `B5` | 0 | half-line down / up (sub/superscript) | printer path moves +2 / −2 (9C64H); prompt text `{` `}` become B4/B5 (CFB7H) |
| `C0` | 0 | starts every row; ends every status message. [HYPOTHESIS] attributes off | builder; messages are `text C0 0D` |
| `C1 xx` | 0 | horizontal scroll: first visible column = `NOT xx`, 80-column window | BAC6H steps it, writes the ruler's and text area's together |
| `C2 lo hi` | 0 | jump to `hi lo` (0000H ends the screen) | walker, builder |
| `C3 a a b b` | 0 | disarmed jump: continue at `b b` (the byte after) | E6BAH flips C2↔C3 to hide/show an optional status row |
| `C4` | 0 | the cursor. [HYPOTHESIS] reverse video until the next `B0`/`C0` | blink at 93C0H swaps a slot B0↔C4 every 25 ticks |
| `C5`–`CF` | 0 | [HYPOTHESIS] more attributes (bit 2 reverse, bit 3 underline); C6, C7, CA are built by the line copier at 8820H (block 1) | not yet traced |

Reverse video for a whole line (white on black against the normal black on white) is likely an
attribute code at the start of the row and not reset by per-character `B0` slots; this can only
be confirmed once a document, a cursor or a highlighted prompt is on the screen, so after the
keyboard is attached [VERIFY]. The character ROM itself is not on the disk; the glyph shapes for
80H–AFH (tab markers, ruler ticks, the return mark 8DH?) are unknown and drawn with ASCII
look-alikes.

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
| 06H | B5H | backspace (deletes the previous character) |
| 4FH | B4H | a space [HYPOTHESIS: required space] |
| 07H, 77H | 80H | CODE |
| 70H | 81H | SHIFT |
| 72H | 82H | LOCK |
| 3EH, 2EH, 7EH, 0EH, 5FH | ABH, ACH, ADH, AEH, AFH | unit: CHAR, WORD, LINE, PARA, PAGE (shown on the status row) |
| 3DH, 2DH, 7DH | A1H, A2H, A3H | **IN**, **OUT**, **PRINT**: open a prompt on the top line (`OUT        1 WORLD`: a number and the name typed, upper-cased) |
| 73H | AAH | HOLD (status row) |
| 71H | B9H | margin set (83 on the ruler) |
| 75H | B8H | tab set (84 on the ruler) |
| 69H, 74H | DFH, B7H | moves the cursor to column 85 (the window scrolls with `C1`) |
| 18 49 19 59 4A 1A 5A 4B 1B 5B | E6 E7 E8 E9 EA EB EC ED EE EF | keypad 0–9 |
| 6AH, 6BH, 6CH | E4H, E2H, E3H | keypad `.` `,` `-` |
| 5CH | E1H | deletes the next character and shows `.00` on the top line [VERIFY] |
| 2FH | B0H | removes the typed word from the typing line [VERIFY] |
| 01 02 05 0D 0F 1C 1F 3F 4C 6F 76 7F | B3 A8 B2 BD A5 E5 A6 A4 E0 FE BA B1 | nothing visible when typed alone (chords with CODE not yet tried) |

**Modelled timing [HYPOTHESIS]:** one event is latched at a time, and the next comes 1 ms
(2,000 cycles) after the previous one is read. The real keyboard link's timing is unknown.

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

1. Finish the filesystem: what a directory leaf pointer points to (each document's index?),
   how a document's blocks chain (list blocks, the code-`11` blocks among the `10` data), the
   4-byte field, and whether the system area (tracks 1–32) is one structure or several segments.
   Finding the filer's read code in the system area (Segment 12) would settle most of this.
2. ~~Decode the display memory format~~ (done above). Still open: C5–CF, the line attribute for
   reverse video, and the top line's two slots per column.
3. ~~Attach a keyboard~~ (done above). Still open: reverse video (none seen yet, even in the
   IN/OUT/PRINT prompts) and the cursor blink (93C0H, only when 22F9H is set); CODE chords;
   names for the rest of the function keys; whether OUT can save a document (writes to the
   image).
4. Replace the fixed read delay with a rotation model once the timing check is understood, and
   confirm which 8253 counter drives which RST.
5. Disassemble the main code from its entry points and from every `IN`/`OUT` to confirm the
   rest of the port map (keyboard, display, serial, timer, speaker) and the interrupt vectors,
   then update the placeholders in `MachineConfig`.
6. Decode the keyboard code tables.
7. Show the decoded screen in the web UI.

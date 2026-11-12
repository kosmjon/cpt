# cpt-wasm — the CPT 8000 in a browser

A WebAssembly boundary that runs the real CPT system (booted from a real program disk image)
in a browser, with its screen on a canvas inside a styled CRT case, two 8-inch drives, the
special-key panel, POWER and RESET. No `wasm-bindgen`: the module uses a plain C ABI, and the
JS glue reads the decoded screen straight out of wasm linear memory.

The page powers up idle with a black screen, waiting for a program disk in drive 1, as the
real machine did. (The module still contains the earlier demo machine and its exports, used
by the `check.mjs`, `check-kbd.mjs`, `check-disk.mjs` and `check-audio.mjs` Node checks; the
page no longer runs it. `run-web.bat` in the repository root builds and serves the page.)

## Build and run

```sh
# 1. Build the wasm module (requires: rustup target add wasm32-unknown-unknown)
cargo build -p cpt-wasm --target wasm32-unknown-unknown --release

# 2. Copy it next to the web files
cp target/wasm32-unknown-unknown/release/cpt_wasm.wasm crates/cpt-wasm/web/

# 3. Serve the web/ directory over HTTP (fetch() of .wasm needs a server, not file://)
#    run-web.bat uses tools/serve_web.py, which sends Cache-Control: no-cache so a reload
#    after a rebuild gets the new cpt.js and cpt_wasm.wasm:
python tools/serve_web.py 8080 crates/cpt-wasm/web
#    Any static server works, but one without that header can leave the browser running
#    old copies until a hard reload (Ctrl+F5), e.g.:
#    npx serve crates/cpt-wasm/web
#    python -m http.server --directory crates/cpt-wasm/web 8080
```

Open the served page and insert a CPT program disk image in drive 1 (see "The real CPT
system" below).

## Quick check without a browser

```sh
node crates/cpt-wasm/web/check.mjs   # instantiates the wasm and prints decoded rows
```

## Exports (C ABI)

Display: `cpt_init`, `cpt_run_frame`, `cpt_page_ptr`, `cpt_page_len`, `cpt_cols`, `cpt_rows`,
`cpt_retraces`, and layout-row accessors. The page buffer at `cpt_page_ptr()` is
`cpt_page_len()` bytes of character codes, row-major.

Disk drives (stations 0 = A, 1 = B):
- `cpt_xfer_reserve(len) -> ptr` / `cpt_xfer_ptr()` / `cpt_xfer_len()` — a scratch buffer for
  moving disk bytes between JS and wasm.
- `cpt_disk_mount(station)` — mount the current transfer-buffer bytes as a disk in `station`.
- `cpt_disk_eject(station) -> len` — flush + detach; the image bytes land in the transfer
  buffer for download.
- `cpt_disk_mounted(station) -> 0|1` — bay state for the UI.

The web page shows two drive bays beside the screen with Insert (file picker) / Eject
(download) buttons.

## The real CPT system

The page powers up idle with a black screen. Insert a real CPT program disk image in
**drive 1** and it boots the real CPT software. The image must be the raw 77 × 16 × 256 =
315,392-byte form. Make the images from the repository's TeleDisk files with
`setup-disks.bat` (or `python tools/setup_disks.py`) in the repository root; it writes
`disk/cpt.img`, `disk/cpt2365.img` and `disk/cpt2489.img`.

**A disk for drive 2.** `web/blank-data.img` (kept in the repository, and linked under the
screen) is a blank, formatted data disk. Insert it in drive 2, then to file a page on it:
press OUT, backspace twice over the station digit, type `2`, then the page name and Enter.
Eject the disk to download it with the page on it. (`tools/mkdatadisk.py` makes it.)

**Which disk:**
- **`cpt.img`** is the main program disk.
- **`cpt2489.img`** is the TTY disk, with serial communications (CODE + I, then `ctty`). It
  boots here too. Its 8251 serial port is in the module, but the page has no serial
  connection, because a browser can't open a TCP socket. For a terminal, use `cpt-run
  --serial-listen` (see `segments-cpt2489.md`).

**Start-up.** The software checks disk timing. Reads must take about 31 ms or more, as on a
real 8-inch drive, or it reports "THE DISK IN STATION 2 SEEMS TO BE FAULTY". So the page keeps
real disk timing and instead runs the emulation faster than real time until the system has
settled (about 37 s of emulated time). That takes under a second in a browser, with a
progress note under the screen. After that it runs in real time.

**Screen.** The decoded display list, black on white, with the title line and ruler (and
documents' format lines) white on black. The status (column, line, unit) shows at the right
end of the title line. Rows with no document text, such as the rows above the start of the
document, are empty black lines; the typing line stays black on white. The screen is 54
rows: the title line, 45 rows through the typing line, the ruler and 7 rows below it (the
system's own layout values at 04ABH and 04ACH). The character ROM's glyph shapes are unknown,
so its special glyphs are drawn as ASCII stand-ins (`.` `:` `|` ruler ticks, `M` margins, `^`
the cursor column, `>` tabs).

**Keyboard.** The PC keyboard sends physical key presses and releases, as the CPT keyboard
did; the CPT software tracks its own SHIFT, LOCK and CODE.

| PC key | CPT key |
|---|---|
| letters, digits, `- = ; ' , . /` | the CPT key that types the same character unshifted |
| Shift | SHIFT |
| Caps Lock | LOCK |
| Alt | CODE |
| Enter | RETURN |
| Backspace | backspace |
| ↓ / ↑ | CPT DOWN / UP (by the selected unit, or a line) |
| Esc | SKIP |
| Tab | TAB (always: the page doesn't use Tab to move between its buttons) |
| Delete | ERASE (blanks the character under the cursor and moves on) |
| Insert | nothing yet: the CPT's INSERT key isn't identified |
| `\` | the CPT's `|` / `\` key |
| numeric keypad 0–9 `.` `-` | the CPT keypad |

PC auto-repeat isn't passed on. A held PC key is one press and one release, as on the real
keyboard, and the CPT's own auto-repeat (below) does the repeating.

**The special-key panel.** These keys send their real key numbers: CODE, IN, OUT, PRINT,
CHAR, WORD, LINE, PARA, PAGE, HOLD, MAR SET, MAR REL, CLEAR TAB SET, UP, DOWN, SKIP, STOP,
ADJ, PROG, ERASE, and the staple key.
- **UP and DOWN** move by the selected unit. UP moves the text up (on through the document)
  and DOWN moves it down. For example, CODE + IN and Enter build the table of contents in the
  upper screen; then PAGE, DOWN scrolls it all into the lower screen.
- **SKIP** discards text after the cursor by the selected unit. PAGE then SKIP empties the
  lower screen.
- **CODE + MAR REL + a letter** (the Outliner on cpt.img: O turns it on or off, Q numbers
  again from the first label). The CPT needs MAR REL still held when the letter is pressed.
  Latch CODE on the panel (or hold Alt), **hold the mouse button down on MAR REL**, and press
  the letter on the PC keyboard. A plain click on MAR REL releases it (and the latched CODE)
  before the letter arrives, so the letter is just typed. See `program-disk.md`, "The
  Outliner".
- **CLEAR TAB SET** puts a tab stop at the cursor's column, or clears the one there.
- **MAR REL** is the margin release.
- **PROG** is the keyboard program. Latch CODE and press PROG to start recording keys ("PROG"
  shows on the title line), CODE + PROG again to end it, and PROG to play it back.
- **The staple key** is under WORDPAK, drawn as a staple. It takes the text on the typing
  line as a document name and brings that document in at the typing line, e.g. type `wn` at
  the start of a line, then press it.
- **ERASE** (4FH) is a hypothesis: it blanks the character under the cursor and moves on.
- **STOP and ADJ** are mapped from the key-column pattern and haven't been checked against
  the real machine.
- **CODE** latches: click it, then the other key, for a chord.
- **INSERT, MOVE and WORDPAK** are not yet identified on the real system, and their tooltips
  say so. WORDPAK's code isn't on these disks.

**Auto-repeat.** A panel key is held down for as long as the mouse button or a finger is
on it, so the CPT's own auto-repeat works:
- **Timing:** after about 300 ms, a repeat every 30 ms.
- **Which keys:** only those on the CPT's repeat list. These are UP, DOWN, SKIP, ADJ, ERASE,
  PROG, RETURN, backspace, space, `.`, `-`, `*` and `x` (see `program-disk.md`,
  "Auto-repeat").
- **Taps:** a quick click is a 40 ms tap. Enter or Space on a focused key button is a tap
  too.

To open a document, press IN, type its name and press Enter, e.g. `rc-l1` (a letter) or
`hm-m1.2` (a memo). Then press ↓ and ↑ to move through it.

**Drives.** Disks go in and out of either drive at any time. Once the system has booted, the
program disk can come out of drive 1 and the system keeps running. A disk the system wrote to
is offered as a download when it is ejected.

**POWER and RESET** (under the special keys).
- **POWER off** stops the machine and leaves the screen dark. The disks stay in their drives,
  and a disk can still be inserted while the power is off.
- **POWER on** and **RESET** boot a fresh system from the disk in drive 1. If drive 1 is
  empty, the screen stays black, waiting for a program disk, as at page load. RESET does
  nothing with the power off.

Checks without a browser, both using `disk/cpt.img` (skipped if it is missing):

```sh
node crates/cpt-wasm/web/check-real.mjs   # real-cpt.js + wasm: start-up, keys, LEGAL
node crates/cpt-wasm/web/check-page.mjs   # cpt.js on a fake DOM: insert, start, type, held-key repeat, eject, power
node crates/cpt-wasm/web/check-print.mjs  # PRINT: the printout grows line by line, two pages, survives power off
```

Exports: `cpt_real_insert(drive)`, `cpt_real_eject(drive)`, `cpt_real_start` (boot from
drive 1: power on or reset), `cpt_real_halt` (power off), `cpt_real_running`, `cpt_real_run`,
`cpt_real_cycles`, `cpt_real_settled_cycles`, `cpt_real_screen` (with `cpt_real_codes_ptr`,
`cpt_real_attrs_ptr`, `cpt_real_cols`, `cpt_real_max_rows`), `cpt_real_key`,
`cpt_real_key_for_char`, `cpt_real_drive_state`, `cpt_real_disk_bytes`. While no system is
running, the disks wait in their drives inside the module.

## Speaker

The real system's speaker plays through the Web Audio API. It is the 8253's counter 0: the
system plays a note by putting it in mode 3 (a square wave of 2 MHz / count) and silences it
with mode 0 (see `program-disk.md`, "The speaker"). Each frame the page takes the tone changes
(`cpt_real_speaker_take`, `cpt_real_speaker_ptr`, `cpt_real_tone_hz`) and `speaker-audio.js`
schedules them on one square-wave oscillator, timed by the emulated clock.

- Browsers only allow audio after a user gesture, so sound starts with your first click or key
  on the page (inserting the disk is one).
- Start-up plays a chime (2,000 Hz then 1,000 Hz). Start-up otherwise runs faster than real
  time; the page drops to real time while a tone sounds (`cpt_real_speaker_on`), so notes keep
  their true length.
- IN, PAGE and other keys click (1,333 Hz for 50 ms). Nothing plays with the power off.

`check-page.mjs` checks the chime's pitches and note lengths against a mock audio context.
The earlier demo machine's melody (CODE + WORDPAK) is gone from the page with the demo; its
audio path is still in the module and checked by `check-audio.mjs`.

## The printer

Below the workstation is printer A, the daisy-wheel printer on the CPT's parallel port (see
`program-disk.md`, "The printer"). Bring in a document (IN), press PRINT and RETURN, and the
document comes out on the paper line by line, at the pace the CPT sends it (about 110
characters a second; the emulated printer is always ready). Each page is a sheet; the
emulated printer loads the next sheet when the CPT asks for one. A red block marks the print
head while it prints. When printing reaches the bottom of the paper window, the paper
scrolls up to keep the line being printed in view. Scroll up to read and it stops following;
scroll back down to the print head, or start a new print, and it follows again.

- **ON / OFF** switches the printer. Off, the CPT says "THE PRINTER SEEMS TO BE OFF."
- **TEAR OFF** clears the paper. **SAVE** downloads the printout as `printout.txt` (pages
  separated by form feeds).
- The printout stays through POWER and RESET, like paper.

The module's side: after each run it moves the printer's commands to a log kept outside the
system (`cpt_real_printer_version` changes when there is more), and `cpt_real_printout`
renders the log as text into the transfer buffer, through the print-wheel table it reads
from the system's memory. `cpt_real_printer_clear` and `cpt_real_printer_set_on` are the
buttons. Underlined text is underlined on the paper (the printout marks it with combining
low lines, U+0332, which SAVE keeps). Bold and the wheel's special characters print as plain
characters.

How it works:
- `cpt_run_frame` leaves the frame's speaker events in a buffer of f64 pairs (CPU cycle,
  divisor). `cpt_speaker_count()` / `cpt_speaker_ptr()` read it; `cpt_cpu_hz()`,
  `cpt_tone_hz()` and `cpt_cycle_now()` give the clocks. `cpt_play_melody()` assembles and
  starts the demo (screen, disks and keyboard are left alone; it restarts only the CPU).
- `web/speaker-audio.js` (`SpeakerAudio`) schedules those events on one square-wave
  oscillator: pitch = tone clock / (2 * divisor), divisor 0 = silence. Times come from the
  emulated clock, so the tempo is right whatever the frame pacing; if the page falls behind it
  re-anchors a little ahead of "now".
- `node crates/cpt-wasm/web/check-audio.mjs` checks the scheduling (against a mock audio
  context) and the real wasm melody end to end, with no browser.

This worked in a real browser when the page ran the demo.

## Notes

- Glyphs are drawn with a monospaced web font for now; the authentic CPT character-ROM font is
  a `[VERIFY]` item awaiting a ROM dump and drops in as the glyph source later.

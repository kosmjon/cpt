# CPT / MOL80

Tools for preserving and emulating the software of CPT Corporation's dedicated word
processing systems (CPT 8000 / 9000 and related), which were written in **MOL80** — a
Machine-Oriented Language for the Intel 8080, used by CPT (Eden Prairie, MN) in the late
1970s and early 1980s.

![The real CPT word-processing software running in the browser emulator: a letter on the
CPT screen with the title line, the ruler and the reverse-video rows, next to the
special-key panel and the POWER and RESET buttons](CPT%208000%20Emulator.png)

*The original CPT software, booted from a real program disk image, running in the browser
emulator (`run-web.bat`).*

## Components

### `mol80asm` — MOL80 assembler / transpiler (working)

A Rust crate that lexes, parses, and assembles MOL80 source to Intel 8080 machine code, and
can transpile it to standard 8080 assembly for cross-checking.

```
cd mol80asm
cargo test                                   # run the test suite
cargo run -- examples/sample.mol             # hex listing of assembled bytes
cargo run -- --asm examples/sample.mol       # transpile to standard 8080 assembly
cargo run -- --tokens examples/sample.mol    # token stream
cargo run -- --ast examples/sample.mol       # parsed AST
cargo run -- --bin out.bin examples/sample.mol   # write a raw binary image
```

Implemented (the MOL80 "Layer 1" primitives that map one-to-one to 8080 opcodes, plus a few
multi-instruction forms):
- Column-oriented source (label field, instruction field, column-based comments)
- `src:dst` moves, `n:A` / `nn:HL` immediates, MVI/LXI/MOV families
- ALU ops targeting A (ADD/SUB and the tentative logical spellings `+*`/`-*`/`**`)
- 16-bit pair ops (INX/DCX/DAD, `HL:SP`=SPHL), `/HL`=PCHL
- Flag tests and conditional jumps (`Fx=0?/1? /target`)
- Structured conditional blocks (`cond ? ( ... )`), lowered to compare + branch + label
- `CALL(target)`, `RTN` (and conditional `Fx=v? RTN`), stack `rp:STK` / `STK:rp`
- The dual-move exchange `a:b/b:a` (XCHG, or a push/copy/pop expansion)
- Binary output, hex listing, and standard-8080 transpilation

Every 8080 instruction MOL80 uses now has its scan-confirmed spelling (EI/DI, rotates,
carry arithmetic, LDAX/STAX, XTHL, RST, conditional calls, NOP/HLT, ...); opcodes MOL80
never uses are rejected. Placeholder directives (`ORG`/`DB`/`DW`/`DS`, `EQU`, `VAR`) are
our own convention until the manual confirms the real spellings.

Not yet implemented (awaiting the official MOL80 manual / real source):
- The MOL80 "Layer 2" macro/expansion set, including the `using` `<reg>` clause and
  `(HL+offset)` page-aligned array addressing
- Confirmed directive spellings and radix markers

### CPT 8000/9000 emulator (the real system boots and runs)

An Intel 8080 emulator plus a model of the CPT workstation hardware. It boots **real CPT
program disks**, decoded from TeleDisk images of the original 8-inch floppies, and runs the
original CPT word-processing software. You can use it from the command line or in a browser.
The hardware model was worked out from the software itself: what it reads, writes and expects
from each port. Built as a cargo workspace of focused crates:

| Crate | Status | What it is |
|-------|--------|------------|
| `crates/i8080-isa` | done | Shared opcode metadata + disassembler (used by the emulator and the assembler's transpiler) |
| `crates/i8080-cpu` | done | Pure 8080 CPU core; passes the CP/M diagnostics incl. the exhaustive 8080EXM exerciser |
| `crates/cpt-machine` | working | The CPT hardware model (`CptSystem`): banked memory, FD1771 floppy + 8257 DMA, 8253 timer and speaker, keyboard, display list decoder, 8251 serial port. Also the generic machine used by CP/M |
| `crates/cpt-run` | working | CLI runner: `--cpt-boot` boots a real disk with scripted keys and prints the decoded screen, plus a disassembler and trace/breakpoint tools |
| `crates/cpt-wasm` | working | The real CPT in a browser: screen in a CRT case, two 8-inch drives, the special-key panel, POWER/RESET, speaker (see its README) |

#### The program disks

Three TeleDisk images are in `.kiro/specs/cpt8000-emulator/`:

| Image | What it is |
|---|---|
| `cpt.td0` | "Program disk for CPT 8100", 14 documents |
| `cpt2365.td0` | the same disk (decodes byte for byte the same as `cpt.td0`) |
| `cpt2489.td0` | "Program disk for 8100, 8515/25 CPT word processor w/TTY", 28 documents, with TTY communications |

The emulator and its tests use flat images (77 tracks × 16 sectors × 256 bytes). They aren't
kept in the repository: they're decoded from the `.td0` files into `disk/` (gitignored). After
cloning, run this once (it needs only Python):

```
setup-disks.bat
```

or `python tools/setup_disks.py`. It makes `disk/cpt.img`, `disk/cpt2365.img` and
`disk/cpt2489.img`, and a blank data disk, `disk/blank-data.img`. Without the images, the
real-disk tests print a note and skip. `tools/td0.py` handles both normal and "advanced"
(LZHUF-compressed) TeleDisk files, and checks every CRC.

A blank, formatted **data disk** for station 2 is also kept in the repository, as
`crates/cpt-wasm/web/blank-data.img` (the web page links it). It holds no CPT software.

#### In a browser

```
run-web.bat
```

This builds the wasm module, copies it next to the page and serves it at
http://localhost:8080 (needs `rustup target add wasm32-unknown-unknown` and Python). Insert
`disk/cpt.img` in drive 1.
- **Start-up:** the system starts as it did on the real machine, in under a second. Real disk
  timing is kept, but the emulation runs faster than real time until it settles.
- **Keyboard:** type on the PC keyboard. Alt = CODE, ↑/↓ = UP/DOWN, Caps Lock = LOCK.
- **Special-key panel:** use it for IN, OUT, PRINT, the units, SKIP, PROG, CLEAR TAB SET and
  the rest. A held key repeats where the CPT repeats it.
- **Open a document:** press IN, type `rc-l1`, Enter.

`crates/cpt-wasm/README.md` has the details.

#### From the command line

```
# Boot, type IN rc-l1 RETURN after start-up, and print the decoded screen:
cargo run -p cpt-run --release -- --cpt-boot disk/cpt.img 110000000 - 40000 --keys "{3D}rc-l1{14}" --keys-at 80000000

# Save memory after start-up (bank 0, then each 32K block) and decode the segment map:
cargo run -p cpt-run --release -- --cpt-boot disk/cpt.img 82000000 - 40000 disk/mem.bin
python tools/cptsegs.py disk/mem.bin

# Disassemble part of a file:
cargo run -p cpt-run -- --disasm <file> <offset> <len> <origin>
```

- **Arguments:** `--cpt-boot <image> <cycles> <breakpoint|-> <read delay> [<mem out>]`. The
  read delay must be about 32,000 cycles or more; the software checks disk timing.
- **Key scripts:** `{XX}` taps key number XX (hex), `{+XX}`/`{-XX}` press/release, `{ms N}`
  waits. Other options: `--screen-at`, `--brk-after`, `--profile`.
- **Memory watchpoints:** `--watch [r:|w:]ADDR[-END|+LEN][@BLOCK]` (hex; repeatable)
  reports which instructions read or wrote that memory: a table by instruction (with counts,
  addresses, values and cycles) and the first accesses in order. `--watch-from <cycle>`
  starts recording late, and `--watch-stop` halts at the first access with the usual
  breakpoint report. Example: `--watch w:0E71` shows that the start-up code at 0D9CH stores
  the operation number there. DMA writes are labelled, and instruction fetches count as reads.
- **A second disk:** `--disk2 <img>` mounts a disk in station 2 (for example one made by
  `tools/mkdatadisk.py`), and `--save-disk1 <out>` / `--save-disk2 <out>` write a drive's
  image back at the end, with whatever the CPT wrote to it. To file to station 2, press
  OUT, backspace twice over the station digit and type `2`.
- **Sector-read breakpoint:** `--brk-read <t41s14|L660|670>` stops as the read of a sector is
  issued (physical track/sector, logical as directory pointers number it, or linear), with
  the code and stack that asked for it.
- **Serial:** the 8251 can be a Telnet server (`--serial-listen 127.0.0.1:2323`), or take
  scripted input (`--serial-in`).
- **Printer:** PRINT goes to an emulated daisy-wheel printer. The report shows the printout;
  `--printer-text out.txt` saves it, `--printer-log` saves the raw commands, and
  `--printer-status 00` unplugs the printer. `--watch-io 42-45` logs port I/O.

#### Tools

| Tool | What it does |
|---|---|
| `tools/setup_disks.py` | Make `disk/*.img` from the repository's `.td0` files, plus a blank data disk (`setup-disks.bat` runs it) |
| `tools/td0.py` | Decode a TeleDisk image (incl. LZHUF advanced compression) to a raw image |
| `tools/cptfs.py` | List a CPT disk: volume label, allocation map, document directory (program or data disks) |
| `tools/mkdatadisk.py` | Make a blank, formatted CPT data disk image (`--zero` for an unformatted one) |
| `tools/cptsegs.py` | The segment / bank / function map from a memory dump (hex and decimal) |
| `tools/cptkeys.py` | The keyboard remap tables from a memory dump |

#### What works on the real system

- **Start-up** from drive 1 with real disk timing.
- **The screen:** reverse-video title line, ruler and format lines, the status at the title
  line's right end, 54 rows.
- **Documents:** opening them (IN), scrolling with UP/DOWN by unit, PAGE, SKIP, the units
  CHAR, WORD, LINE, PARA and PAGE, and the table of contents (CODE + IN).
- **Editing:** typing, RETURN, backspace, tabs, CLEAR TAB SET, MAR SET/REL, HOLD, the
  keyboard program (PROG), the CODE + letter menus, and the staple key (brings in the
  document named on the typing line).
- **Keys:** the CPT's own auto-repeat for the keys on its repeat list.
- **The speaker:** the start-up chime and key clicks.
- **The TTY disk:** CODE + I, `ctty` turns communications on. Received text shows on the
  screen, or (CODE + K, Select, Reception, Disk) is filed as a document. Transmission from
  the keyboard sends what is typed, with even parity. Over TCP or with scripted input; see
  `segments-cpt2489.md`, "Using TTY mode".

#### Known gaps

- **Unidentified keys:** INSERT, MOVE and WORDPAK. WORDPAK's segment isn't on these disks.
  ERASE (4FH) is a hypothesis.
- **TTY:** the Stop and Status menus and the Screen and Disk transmissions are untried, and
  the browser has no serial connection. See `segments-cpt2489.md`, "Communications: open
  questions".
- **Disk utilities (Work Disk Duplication, Disk Erase, Work Disk Repair):** reached on the
  real machine with CODE + O and operation 5, 6 or 7. The `cpt2489` disk has their message
  text, but not their code, so these three operations aren't available from the disks we
  have. They would need another disk image.
- **Printing:** plain text only, from `cpt-run` and in the browser's printer pane. Bold,
  underline and the wheel's special characters aren't rendered, and the SRS link (ports
  60H–65H) isn't modelled.
- **Glyphs:** the character ROM is unknown, so special glyphs are ASCII stand-ins.
- **Patch pages:** the mechanism is decoded (documents `.P`–`.P9`), but neither disk has one,
  but a .P1 page was added to a copy of a program disk .img and it loaded the patch correctly so the syntax has been proven.

## Running CP/M 2.2 over the emulated serial port

A side project that exercises the emulator end to end: stock Digital Research CP/M 2.2 boots
on the emulated machine, with its **BIOS written in MOL80** (`cpm/bios.mol`). The console is
the emulated serial port, which `cpt-run` exposes as a TCP socket; you connect to it with a
Telnet client (e.g. MobaXterm, PuTTY, or `telnet`) as a VT100-style terminal.

Prerequisites: Rust (cargo), Python 3, and Node.js, all on the PATH.

### 1. Build the disks

From the repo root:

```
powershell -File cpm/build-cpm.ps1
```

This produces, under `cpm/build/` (gitignored):

| File | What it is |
|------|------------|
| `boot.bin` | Boot ROM (`cpm/boot.mol`), loaded at reset |
| `cpm.img` | Bootable system disk for drive 1 (A:): loader, CCP, BDOS, BIOS on the system tracks, plus the DRI utilities (ASM, DDT, DUMP, ED, LOAD, PIP, STAT, SUBMIT, XSUB) |
| `data.img` | Blank formatted data disk for drive 2 (B:) |

The CCP and BDOS are assembled from the vendored DRI source in `cpm/vendor/` by a small
Python 8080 assembler (`cpm/tools/asm80.py`); the boot ROM, loader and BIOS are MOL80,
assembled by `mol80asm`. Rebuilding overwrites both images, including any files you saved
on them.

### 2. Start the emulator

```
cargo run -p cpt-run -- --serial-listen 127.0.0.1:2323 --disk1 cpm/build/cpm.img --disk2 cpm/build/data.img cpm/build/boot.bin
```

It waits for a connection (`serial: listening on 127.0.0.1:2323 ...`). Any free port works.

### 3. Connect a terminal

Open a **Telnet** session to `127.0.0.1`, port `2323`. In MobaXterm: Session → Telnet,
host `127.0.0.1`, port `2323`. You should see:

```
CP/M 2.2 on CPT (MOL80 BIOS)
A>
```

Try `DIR`, `STAT`, `STAT *.*`, `B:`, or `PIP B:=A:STAT.COM`.

How the serial link behaves:
- `--serial-listen` acts as a Telnet server: it tells the client the guest does the echoing
  and to send each keystroke immediately, and it strips Telnet control bytes from the input.
  Leave the client's local echo at its default; you should see each character once.
- The BIOS drops the LF (or NUL) a Telnet client sends after CR, so Enter gives one prompt.
- For a raw TCP client with no Telnet negotiation, add `--serial-raw` after the address.

### Disks and persistence

- Disk images are file-backed. The BIOS flushes after every write, so files you `SAVE` or
  `PIP` persist even if you kill the emulator.
- Make another blank data disk with `python cpm/tools/cpmfs.py my.img mkblank`.
- List a disk from the host with `python cpm/tools/cpmfs.py cpm/build/cpm.img ls`, or copy
  host files onto it with `python cpm/tools/cpmfs.py cpm/build/cpm.img add FILE...`
  (while the emulator is not running).
- Geometry: 8-inch IBM 3740 layout, 77 tracks × 26 sectors × 128 bytes, 2 system tracks,
  1K blocks, 64 directory entries, no sector skew.

### Stopping, and "failed to remove file"

Ctrl+C in the emulator's terminal stops it. Closing the Telnet session may leave it running.
A running `cpt-run` locks `target\debug\cpt-run.exe`, so a rebuild fails with
`error: failed to remove file ...cpt-run.exe`. Stop it first:

```
Get-Process cpt-run | Stop-Process
```

### Tests

`cargo test -p cpt-run --test cpm` boots CP/M in-process and checks `DIR`, `SAVE`, `STAT`,
PIP to drive 2, and Telnet line endings. These tests need `cpm/build/` (run
`build-cpm.ps1` first) and skip themselves if it's missing.

More detail: `.kiro/specs/cpt8000-emulator/cpm-over-serial.md`.

## Specs

Design and planning documents live under `.kiro/specs/`:
- `mol80-assembler/`: requirements, design, task plan for the assembler.
- `cpt8000-emulator/`: requirements, design, task plan for the emulator, and the findings
  about the real system:
  - `program-disk.md` covers the disk format and directory, boot, memory and banks,
    segments, the display list, the keyboard (key numbers, dispatch, auto-repeat), the
    speaker, the staple key and patch pages.
  - `segments-cpt2489.md` is the TTY disk's segment map (hex and decimal) and the serial
    communications.
  - `boot-sequence.md`, `memory-map.md`, `special-keys.md` and `cpm-over-serial.md`.
- `SOURCE-CODE-CHECKLIST.md`: open questions to resolve against the MOL80 manual and the
  original CPT system source.

Items throughout the specs are tagged `[CONFIRMED]`, `[ASSUMED]`, `[HYPOTHESIS]`, or
`[VERIFY]` to distinguish what is known from what still needs confirmation.

## Status

- **The emulator** boots and runs the real CPT software from the original program disks, on
  the command line and in a browser. Most editing features work (see "What works on the
  real system").
  - The open work is the unidentified keys, the rest of TTY, the SRS link and the
    character ROM.
  - `cargo test --workspace` runs 368 tests with no warnings. The real-disk boot tests skip
    themselves unless `setup-disks.bat` has made `disk/cpt.img` and `disk/cpt2489.img`.
  - The browser has Node checks in `crates/cpt-wasm/web/check*.mjs`.
- **The assembler** front end and code generation are working and tested for the confirmed
  MOL80 subset. The MOL80 macro/expansion layer is designed and awaits the full manual and
  the original system source.
- **CP/M 2.2** runs on the generic emulated machine over a Telnet serial console.

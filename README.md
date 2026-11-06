# CPT / MOL80

Tools for preserving and emulating the software of CPT Corporation's dedicated word
processing systems (CPT 8000 / 9000 and related), which were written in **MOL80** — a
Machine-Oriented Language for the Intel 8080, used by CPT (Eden Prairie, MN) in the late
1970s and early 1980s.

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

### CPT 8000/9000 emulator (in progress)

An Intel 8080 emulator plus a model of the CPT workstation hardware. The complete CPT
word-processing system source will be compiled with `mol80asm`, linked and bin-packed into a
system image, and run on the emulated machine. Built as a cargo workspace of focused crates:

| Crate | Status | What it is |
|-------|--------|------------|
| `crates/i8080-isa` | done | Shared opcode metadata + disassembler (used by the emulator and the assembler's transpiler) |
| `crates/i8080-cpu` | done | Pure 8080 CPU core; passes the CP/M diagnostics incl. the exhaustive 8080EXM exerciser |
| `crates/cpt-machine` | in progress | Banked memory (bank 0 + switchable 32K blocks), bank-select port, `Device` trait, executive loop with interrupts, memory-mapped display device with the vertical-retrace interrupt |
| `crates/cpt-run` | done | CLI trace runner: load a binary, run it on the machine, print a disassembled trace |
| `crates/cpt-wasm` | proof | WebAssembly boundary + canvas web page: runs the machine in a browser and paints the page-white display in a CRT case (see its README) |

Still to come: keyboard device, file-backed data disk, the link-editor (`cpt-link`) that
bin-packs segments into a system image, and the full React UI.

```
# Run a binary on the emulated machine with a disassembled trace:
cargo run -p cpt-run -- --trace --steps 100 program.bin

# End-to-end: assemble MOL80, then run the result on the machine:
cargo run -p mol80asm -- --bin out.bin mol80asm/examples/sample.mol
cargo run -p cpt-run -- --trace --steps 20 out.bin

# Display demo: draws the title / text / typing-line ruler / status regions, and the
# vertical-retrace interrupt handler scrolls the text (give it enough steps to retrace):
cargo run -p cpt-run -- --demo --steps 40000
```

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
- `mol80-assembler/` — requirements, design, task plan for the assembler
- `cpt8000-emulator/` — requirements, design, task plan for the emulator
- `SOURCE-CODE-CHECKLIST.md` — open questions to resolve against the MOL80 manual and the
  original CPT system source

Items throughout the specs are tagged `[CONFIRMED]`, `[ASSUMED]`, `[HYPOTHESIS]`, or
`[VERIFY]` to distinguish what is known from what still needs confirmation.

## Status

The assembler front end and code generation are working and tested for the confirmed MOL80
subset. The emulator and the MOL80 macro/expansion layer are designed and awaiting the full
manual and the original system source.

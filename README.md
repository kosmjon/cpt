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

Not yet implemented (awaiting the official MOL80 manual / real source):
- The MOL80 "Layer 2" macro/expansion set, including the `using` `<reg>` clause and
  `(HL+offset)` page-aligned array addressing
- Data-definition directives, the full 256-entry opcode table, confirmed radix markers

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

Still to come: keyboard device, file-backed data disk, the link-editor (`cpt-link`) that
bin-packs segments into a system image, the wasm boundary, and the React UI.

```
# Run a binary on the emulated machine with a disassembled trace:
cargo run -p cpt-run -- --trace --steps 100 program.bin

# End-to-end: assemble MOL80, then run the result on the machine:
cargo run -p mol80asm -- --bin out.bin mol80asm/examples/sample.mol
cargo run -p cpt-run -- --trace --steps 20 out.bin
```

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

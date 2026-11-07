# Source-Code Investigation Checklist

Both specs (`mol80-assembler/` and `cpt8000-emulator/`) are drafted. The remaining open
questions are blocked on access to the original CPT/MOL80 **source code** (and, ideally, the
MOL80 manual and a display-board font-ROM dump). When that material is available, work down
this checklist; each item points at where the answer lands in the specs.

Status tags used in the specs:
- **[CONFIRMED]** verified with the user / from a scan or photo
- **[HYPOTHESIS]** a recalled-but-unconfirmed idea to verify
- **[VERIFY]** open question for the source/manual/ROM
- **[ASSUMED]** a working default we'll proceed on

---

## MOL80 assembler — resolve from the manual + a real source listing

- [ ] 1. Logical-operator spelling: confirm `+*`=AND, `-*`=OR, `**`=XOR.
- [x] 2. Rotate mnemonics/direction — CONFIRMED from the scan: `A<*1:A` = RLC (7),
      `A>*1:A` = RRC (15), `A,FC<*1:FC,A` = RAL (23), `FC,A>*1:A,FC` = RAR (31).
- [ ] 3. Radix markers for binary and hex literals.
- [ ] 4. Data-definition directives (byte / word / string / reserve-storage) syntax — we use
      placeholder `DB`/`DW`/`DS` for now; confirm the real MOL80 spellings.
- [ ] 5. Assembler directives: `ORG`, EQU, any others — we use placeholder `ORG`, `NAME EQU expr`
      and `NAME VAR addr` (direct-memory variables: `s:A`=LDA, `A:s`=STA, `s:HL`=LHLD,
      `HL:s`=SHLD; the opcodes are confirmed, the VAR keyword is ours)
      (implemented in the lexer; absolute constants, `+`/`-` only); confirm the
      real spelling and whether EQU/others exist.
      (Note: I/O via `A:PORT(n)`/`PORT(n):A` is CONFIRMED from the scan, not a placeholder.)
- [ ] 6. Inline-comment start column (currently disabled; `*` full-line comments are CONFIRMED
      and likely also the block/header-comment form — confirm whether there is anything more).
- [ ] 7. Whether `?` conditional blocks nest; whether an "else" form exists; and whether a
      `cond ? ( ... )` block may span **multiple source lines**. We currently accept a multi-line
      body, but the user believes MOL80 required the whole construct on **one line**. Confirm,
      then restrict the parser and fix the multi-line uses in `monitor/monitor.mol`.
- [x] 8. The full table: MOL80 mnemonic ↔ opcode byte ↔ standard 8080 — CONFIRMED from the
      scan for every opcode MOL80 uses (EI/DI, rotates, DAA/CMA/STC/CMC, NOP/HLT as
      `PAUSE`/`WAIT`, ORA A/XRA A as `0:FC<F>`/`0:A`, LDAX/STAX, XTHL, RST as `CALL(0..7)`,
      ADC/ACI/SBI, conditional calls; CMP/CPI only inside `A=x?` conditions). Opcodes never
      used in MOL80 are listed and rejected: 143, 159, 167, and the undocumented 8, 16, 32,
      40, 48, 56, 203, 217, 237, 253. Remaining unconfirmed rows are tracked in 8a and item 1.
- [ ] 8a. Residual unconfirmed rows: SBB r (152..158) assumed `A-FC-r:A` by symmetry with
      SBI; LXI BC/DE/SP (`nn:BC`, `nn:DE`, `nn:SP`) by the regular encoding (only `nn:HL`
      confirmed); whether `MVI A,0` (62 with operand 0) has a spelling, since `0:A` is XRA A.
- [ ] 9. Operand expression syntax (e.g. `LABEL+2` arithmetic on labels/constants).
- [ ] 9a. Conditional assembly: did MOL80 have IF/ELSE/ENDIF (or similar) for building model
      variants from shared source? Spelling and semantics. (Needed for build flavors, emulator
      task 9c.7.)
- [ ] 9b. **Module / object-module model** (gates the linker — see "Linker is blocked" below):
      - [ ] 9b-i.  Local vs. public symbols within a module: MOL80 had **module-local symbols**;
                   what is the rule for which symbols are local vs. visible to other modules?
      - [ ] 9b-ii. The **intra-segment call** syntax (a call within the same segment, resolved
                   to a relocatable address) vs. a cross-segment **long-call** (through the
                   bank-0 handler). Knowing this determines the relocation/fixup record set.
      - [ ] 9b-iii. **Relocation model:** most code was relocatable, so the assembler must emit
                   relocation records (which byte positions hold addresses that shift when the
                   module is placed). What is relocatable vs. absolute? Segment-relative or
                   fully relocatable? This is the substance of the object-module format.
      - [ ] 9b-iv. The object-module file format MOL80's tools produced (symbols, relocations,
                   segment tag, entry points) — i.e. exactly what the linker consumes.
- [ ] 10. **Layer-2 expansion / pseudo-instruction set** (the big one): all MOL80 syntax that
      expands to more than one 8080 instruction. Including:
      - [ ] 10a. The `using` `<reg>` clause and `(HL+offset)` page-aligned array addressing;
            the exact expansion for each form. Example (a STORE, per src:dst):
            `A:(HL+3)<L>` -> `MVI L,3` / `MOV M,A`.
      - [ ] 10b. Which registers are legal as the `using` scratch; `(HL-n)`? n>255 behavior?
      - [ ] 10c. Any other pseudo-instructions / macro forms the manual documents.
      - [ ] 10d. Which pair swaps (beyond DE<->HL) are legal and their exact expansions.

> NOTE: the alphabetical scan documents only Layer-1 primitives (one MOL80 form -> one 8080
> opcode). Layer-2 expansions need the full manual. The encoder already supports
> multi-instruction output; Layer 2 should be built as parameterized expansion rules.

> Best single validator: a real MOL80 source listing with labels, data, and comments. It
> lets us confirm the grammar and (via the transpile-to-8080 + third-party-assembler
> round-trip) verify the opcode table independently.

---

## CPT 8000/9000 emulator — resolve from the system source

> Reconstruction model: we have the COMPLETE CPT word-processing system source (what was on a
> Program Disk). We compile it with `mol80asm`, link it with `cpt-link` (bin-pack segments +
> generate bank-0 tables) into a system image, load that into simulated memory, and run from
> the main entry point. No disk/ROM dumps. Most items below now come from the source itself.

### Memory & banking
- [ ] M1. Bank-select I/O port address and bank-number encoding.
- [ ] M2. Bank-0-vs-swappable-pool split (block count is 8 for a 256K machine: 256K / 32K).
- [ ] M3. Bank-switch timing (effect on next fetch; any guard against self-swap-out).

### Link / image build (from the system source)

> **The linker (`cpt-link`) is blocked and should not be built yet.** A linker is defined by the
> object-module format — local/public symbols, relocation records, the intra-segment-call vs.
> long-call distinction — and all of that is determined by how MOL80 actually worked (see
> assembler items 9b-i..iv), which we don't know. Bin-packing is the only source-independent
> part and is a small piece. Building the linker now would mean inventing (and later discarding)
> an object format and relocation model. The one useful groundwork that is NOT a guess: teach
> `mol80asm` to emit **relocation records** (positions holding segment-relative addresses),
> which any relocatable scheme needs regardless of MOL80's exact format. Defer the rest until
> the manual/source arrives.

- [ ] S1. Segment → (bank, offset) table format the long-call handler reads (linker generates it).
- [ ] S2. Long-call calling convention (how segment# and subroutine name are passed).
- [ ] S3. Which segments are `bank0`-tagged vs. relocatable; the tagging mechanism; bank-0 footprint.
- [ ] S4. Fixed-address table contents/format; the main entry point.
- [ ] S5. Work-block heap location/size (from the allocator source).
- [ ] S6. Any bin-packing constraints (segments that must share a bank, alignment, etc.).
- [ ] S7. Relocatable object format `mol80asm` should emit for the linker.
- [ ] S8. Build control files and model variants: how CPT listed the modules/segments of a
      program disk, and how it produced variants (minimal vs full, CPT 8000 vs Phoenix) —
      per-model source files, conditional assembly, or per-product build lists. The
      `cpt-build` manifest (emulator task 9c) should mirror it.

### Display
- [ ] D1. "Special display memory": dedicated display-board region vs. window into banked RAM.
- [ ] D2. Page model: confirm/deny the linked-list-of-lines structure; node format.
- [x] D3. Font ROM: NOT obtainable — a faithful substitute monospaced font is the permanent
      plan. (Only the character cell size remains useful to confirm.)
- [ ] D4. Character-code encoding (code->glyph) AND the **in-band control/attribute codes** the
      video hardware interpreted — notably the per-line **black-on-white vs. white-on-black
      (inverse)** code. User will provide exact codes.
- [ ] D5. Whether 80×54 includes/excludes the status band and ruler row.
- [ ] D6. Status-band fields and layout (observed `IN`, doc id, page/line).
- [ ] D7. Ruler / margin-marker / tab encoding in display memory.
- [ ] D8. Which physical row is the typing line; how document-scroll is realized.
- [ ] D9. Phoenix bitmapped display: framebuffer location/banking, resolution, pixel format,
      retrace interrupt, and any other hardware differences from the 8000 (memory map, ports,
      devices). Needed for the `phoenix` machine profile (emulator task 15).

### Keyboard
- NOTE: keyboard is confirmed **N-key rollover, interrupt-driven** (make/break events). The
  emulator models that; only the codes/ports/encoding below remain [VERIFY].
- [ ] K1. Per-key **codes / scan matrix** (layout + labels already known from the keyboard
      photo: left function column incl. `MAR SET`/`MAR REL`; typewriter block; center
      editing/navigation cluster with unit selectors `CHAR`/`WORD`/`LINE`/`PARA`/`PAGE`,
      `UP`/`DOWN` navigation, `INSERT`/`ERASE`/`MOVE`/`SKIP`/`ADJ`, `IN`/`OUT`,
      `PRINT`/`STOP`/`PROG`; numeric keypad).
- [ ] K2. `CODE`-key shifted meanings (what `CODE`+key combinations produce).
- [ ] K3. Exact glyphs on the special symbol/format keys at the typewriter block's top-left.

### Serial port (USART)
- [ ] R1. Serial port addresses, USART chip type, status/control bit layout, and the RX
      interrupt vector. (The device + TCP transport are built; only these specifics are
      [VERIFY]. Bisync etc. are CPT software, not emulated.)

### Data disk (simple, file-backed — no 8-inch floppy hardware emulation)
- [ ] F1. The I/O ports/commands the B-tree filer (Segment 12) source uses.
- [ ] F2. The filer's sector/block model (determines the backing host-file layout).

### Timer interrupt (multitasking monitor)
- [ ] T1. Timer RST vector and exact frequency. Working assumption (implemented): 100 Hz,
      **not programmable** (no ports), RST 4 placeholder, 2 MHz clock placeholder. Drives the
      monitor's task switching.
- [x] T2. The task-switch timer is a **separate** interrupt from the display vertical retrace —
      CONFIRMED. Two independent periodic sources (distinct RST vectors); the retrace (task 6.3)
      and the timer (task 8c) coexist. Their individual frequencies/vectors remain [VERIFY] (T1).

### Speaker / tone output
- [ ] P1. The real speaker port(s). Working guess (implemented): ports 0x80 (divisor low) and
      0x81 (divisor high).
- [ ] P2. Generation method: 1-bit output **bit-banged** by the CPU in a timed loop, vs. a
      programmable timer/divisor chip given a frequency. Working guess (implemented): a 16-bit
      divider on a 1 MHz tone clock, tone = clock / (2 * divisor). If the source shows
      bit-banging or a different divider layout, replace the register interface only.
- [x] P3. **Number of audio voices — one channel (CONFIRMED by the user).** The hardware has a
      single tone output, used for beeps and tones. The two-part invention (Bach, F major) was
      a **trick done in software on a special demo disk**, not the full word-processing system
      and not a second tone generator: one channel plays apparent polyphony by alternating the
      voices rapidly or mixing them in a tight CPU loop (as the Apple II and PC speaker did).
      So the emulator models **one** tone output; the two-voice case is off the table.
- [ ] P4. The **music data format** (from the demo disk, if it ever turns up): how a tune is
      encoded (note table, durations), and how the single channel is driven to fake two voices.
      Not needed for the word-processing system; only of interest for reproducing the demo.

### Timing & misc
- [ ] B1. 8080 vs. 8085 details, clock speed, interrupt sources, cycle-accuracy target.
- [ ] B2. Printer interface (Selectric-based?), if in scope.
- (Boot/IPL removed: the system is delivered as a linked image, not booted from a disk.)

---

## What's already locked (no source needed)

Architecture and the pieces grounded in the scan, the CPT 9000 photo, and your recollection:
- MOL80 grammar shape (column-oriented: label in cols 1–10, instruction field, column-based
  comments); `:` assign, `/` jump, flag tests `Fx=0?/1?`, and the `cond ? ( ... )` structured
  conditional-block lowering.
- Assembler pipeline (Rust: lexer → parser → two-pass codegen), data-driven ISA table with a
  `verified` flag, and binary / listing / transpile outputs.
- Emulator layering (Rust `i8080-cpu` behind a `Bus` trait → `cpt-machine` → `cpt-wasm` →
  React UI), with all hardware unknowns isolated behind `MachineConfig` and device traits.
- Banked memory model (resident bank 0 all-RAM at 0000–7FFF; switchable bank 1 at 8000–FFFF
  with persistent de-selected blocks), and the principle that monitor / long-call / loader /
  editor are **firmware run on the emulated CPU**, not emulator features.
- Display layering: font ROM (glyph pixels) vs. scanned display memory (character codes) vs.
  linked-list page model (firmware) — three separate things the emulator must not conflate.

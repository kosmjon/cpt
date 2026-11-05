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
- [ ] 2. Rotate mnemonics/direction: `>` meaning; which of RLC/RRC/RAL/RAR each form is.
- [ ] 3. Radix markers for binary and hex literals.
- [ ] 4. Data-definition directives (byte / word / string / reserve-storage) syntax.
- [ ] 5. Assembler directives: ORG, EQU, any others.
- [ ] 6. Inline-comment start column; the block-comment (file-header) form.
- [ ] 7. Whether `?` conditional blocks nest; whether an "else" form exists.
- [ ] 8. The full verified table: MOL80 mnemonic ↔ opcode byte ↔ standard 8080 (256 entries).
- [ ] 9. Operand expression syntax (e.g. `LABEL+2` arithmetic on labels/constants).
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
- [ ] S1. Segment → (bank, offset) table format the long-call handler reads (linker generates it).
- [ ] S2. Long-call calling convention (how segment# and subroutine name are passed).
- [ ] S3. Which segments are `bank0`-tagged vs. relocatable; the tagging mechanism; bank-0 footprint.
- [ ] S4. Fixed-address table contents/format; the main entry point.
- [ ] S5. Work-block heap location/size (from the allocator source).
- [ ] S6. Any bin-packing constraints (segments that must share a bank, alignment, etc.).
- [ ] S7. Relocatable object format `mol80asm` should emit for the linker.

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

### Data disk (simple, file-backed — no 8-inch floppy hardware emulation)
- [ ] F1. The I/O ports/commands the B-tree filer (Segment 12) source uses.
- [ ] F2. The filer's sector/block model (determines the backing host-file layout).

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

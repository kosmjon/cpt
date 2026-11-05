# MOL80 Assembler — Requirements

## Overview

MOL80 ("Machine-Oriented Language for the 8080") was the implementation language
used by CPT Corporation (Eden Prairie, MN) for the software of its dedicated word
processing systems (CPT 8000 and related models) in the late 1970s and early 1980s.
It is a higher-level assembler for the Intel 8080: in addition to one-to-one machine
instructions, it provides structured control constructs (e.g. conditional blocks).

This spec covers **`mol80asm`**, a tool that parses MOL80 source and emits Intel 8080
machine code. It is the foundation for a later, separate spec — the CPT 8000 emulator —
which will execute the code this tool produces.

> Status legend used throughout these specs:
> - **[CONFIRMED]** — verified with the user or from the instruction scan.
> - **[ASSUMED]** — a reasonable default we are proceeding on; may change.
> - **[VERIFY]** — open question to resolve against the official MOL80 manual or real source.

## Known facts about MOL80 (from the alphabetical instruction scan + user)

- The instruction scan (`mol80/docs/mol80-alphabetical.jpeg`) lists each instruction as
  a **decimal opcode byte** paired with its **MOL80 mnemonic**. These opcode values match
  the standard Intel 8080 opcode map (e.g. 195 = JMP, 201 = RET, 205 = CALL,
  62 = MVI A, 33 = LXI H). **[CONFIRMED]**
- `:` is the move/assign operator, written **source `:` destination**.
  Example: `C:(HL)` = `MOV M,C` (store C into memory addressed by HL). **[CONFIRMED]**
- `(HL)` denotes memory addressed by the HL register pair (the 8080 "M" operand). **[CONFIRMED]**
- `STK` (no parens) denotes the stack for push/pop: `HL:STK` = PUSH H, `STK:HL` = POP H.
  **[CONFIRMED]** (The parenthesized `(STK)` only survives in the rare return synonym below.)
- Return is written **`RTN`** (primary), with conditional forms `Fx=0? RTN` / `Fx=1? RTN`
  (RZ/RNZ/RC/RNC/RM/RP/RPE/RPO). `/(STK)` is a rarely-used synonym for `RTN`. **[CONFIRMED]**
- `/HL` is a register-indirect jump to the address in HL = `PCHL` (opcode 233 = 0xE9).
  **[CONFIRMED]**
- A **dual-move** on one line, `a:b/b:a`, means an **exchange** of the two register pairs.
  `DE:HL/HL:DE` is the dedicated case and encodes to the single `XCHG` opcode (235 = 0xEB).
  Other pair swaps (e.g. `BC:HL/HL:BC`) have no dedicated 8080 instruction and **expand to a
  multi-instruction sequence** (PUSH/copy/POP). **[CONFIRMED]**
- A single MOL80 mnemonic may therefore map to **one or several** 8080 instructions; the
  encoder emits a byte *sequence* per instruction, not always a single opcode. **[CONFIRMED]**
- 16-bit pair ops: `rp+1:rp` = INX, `rp-1:rp` = DCX, `HL+rp:HL` = DAD, `HL*2:HL` = DAD H,
  `HL:SP` = SPHL. **[CONFIRMED from scan]**
- **I/O ports:** `A:PORT(n)` = OUT n (opcode 211 = 0xD3), `PORT(n):A` = IN n (opcode 219 =
  0xDB) — consistent with the `src:dst` rule (A written to / read from port n).
  **[CONFIRMED from scan]**
- **Assembler directives (our-convention placeholders; real MOL80 directive syntax is
  [VERIFY]):** `ORG addr` sets the location counter (forward ORG zero-fills; backward is an
  error); `DB item,...` defines bytes (numbers and `"strings"`); `DW value,...` defines
  little-endian words; `DS count` reserves zero bytes. These let us write real programs (e.g.
  the CP/M BIOS) now; spellings adjust when the manual confirms MOL80's directive syntax.

### MOL80 modules, relocation, and the object format (gates the linker)

- MOL80 code was organized into **modules** with **module-local symbols** — symbols private to a
  module, distinct from those visible/exported to other modules. The exact local-vs-public rule
  is **[VERIFY]**. **[CONFIRMED that local symbols exist]**
- There was a **special syntax to mark an intra-segment call** (a call within the same segment,
  which resolves to a relocatable in-segment address) as distinct from a cross-segment
  **long-call** (routed through the bank-0 long-call handler). Exact syntax **[VERIFY]**.
  **[CONFIRMED that the distinction exists]**
- **Most code had to be relocatable**, so the assembler must emit **relocation records** (which
  byte positions hold addresses that shift when the module is placed), not just a flat absolute
  binary. The current assembler emits absolute bytes only; a relocation model is future work.
  **[CONFIRMED]**
- **Linker dependency:** the link-editor (`cpt-link`, in the emulator spec) is **blocked** on
  these facts — the object-module format (symbols, relocations, segment tag, entry points) is
  defined by them. The linker should not be built until they are known. The one piece of
  non-speculative groundwork is emitting relocation records from `mol80asm`. **[CONFIRMED]**

### MOL80 has TWO layers (important architectural finding)

- **Layer 1 — primitives:** the MOL80 syntax that maps one-to-one to a single 8080 opcode.
  The alphabetical scan (`mol80/docs/mol80-alphabetical.jpeg`) documents **only this layer**.
  This is what the assembler implements today. **[CONFIRMED]**
- **Layer 2 — expansions / pseudo-instructions:** additional legal MOL80 syntax that expands
  to **more than one** 8080 instruction. The exchange `a:b/b:a` is one example already built;
  there is more, to be recovered from the official manual. **[CONFIRMED that it exists]**
- **The `using` construct** (`<reg>` suffix): a documented Layer-2 feature. Example from the
  user: `A:(HL+3)<L>` is a **store** (per the `source:destination` rule — A is the source,
  `(HL+3)` the destination) and expands to two instructions —
  ```
  3:L        (MVI L,3)    set L = 3, indexing into the page-aligned block
  A:(HL)     (MOV M,A)    store A into memory at HL
  ```
  Semantics: `(HL+offset)` indexes into a **256-byte page-aligned work block** pointed to by
  HL; because the block is page-aligned, adding an offset is just setting the low byte, so the
  assembler loads the offset into a scratch register. The `<L>` clause names the register that
  is **used and destroyed** by the expansion. **[CONCEPT CONFIRMED; exact syntax/rules VERIFY]**
  - Open details [VERIFY]: full set of `using`/expansion forms; which registers are legal in
    `<...>`; behavior of `(HL+n)` for n>255 or non-aligned blocks; whether `(HL-n)` exists;
    the exact expansion each form produces.
- **Architectural readiness:** the encoder already emits multi-instruction byte sequences (the
  exchange proves the mechanism). The Layer-2 macro system should be a set of **parameterized
  expansion rules** keyed by MOL80 form (offset and `using` register are parameters), NOT a
  flat opcode table. To be implemented against the manual, not from memory. **[DESIGN NOTE]**
- Arithmetic/logic operators target A: `+` ADD, `-` SUB. **[CONFIRMED]**
- Logical operators (tentative): `+*` AND, `-*` OR, `**` XOR. **[VERIFY]**
- Rotates appear as `FC,A>*1:A,FC` style, wrapping the operation in the carry flag
  (`FC`), indicating a rotate-through-carry. Exact RLC/RRC/RAL/RAR mapping TBD. **[VERIFY]**
- Jumps use `/`. `/s` is an unconditional JMP to label/target `s`. **[CONFIRMED]**
- Conditional control uses a test followed by `?`:
  - `Fx=0? /s` / `Fx=1? /s` — conditional jump on a PSW flag, where
    x ∈ { C=carry, P=parity, S=sign, Z=zero }. e.g. `FZ=1? /s` = JZ. **[CONFIRMED]**
  - `A=B? ( ... )` — a **structured conditional block**. The condition is evaluated;
    if false the generated code jumps over the parenthesized instruction block.
    Example: `A=B? (D:B  E:C)` executes the two moves only when A equals B. **[CONFIRMED]**
- There is **no `;`** in the language; every separator that looks like `;` in the scan
  is actually `:`. **[CONFIRMED]**
- Numeric literals exist in **binary, hexadecimal, and decimal**. Radix markers TBD. **[VERIFY]**
- Source is **column-oriented** with fixed fields on a line: **columns 1–10 = label field**
  (a label does **not** end with a colon; this makes `:` unambiguously the assign operator),
  a middle **instruction field**, and a **comment field** that begins at a fixed column and
  runs to end of line with **no delimiter character** (comment text is identified purely by
  its column position, e.g. column ~40). **[CONFIRMED]** (exact comment column **[VERIFY]**)
- Source also supports a **block-comment** form (e.g. a header block at the top of a source
  file). Exact form **[VERIFY]**. **[CONFIRMED that it exists]**
- **Full-line comments begin with `*`** — a line whose first non-blank character is `*` is a
  comment. This matches real MOL80 per the user and is now a confirmed feature (and likely the
  block/header-comment mechanism: a run of `*` lines). **[CONFIRMED]**
- The earlier **inline comment column (~40) is NOT currently applied**: hard-truncating lines at
  a guessed column broke legitimate long lines (e.g. `DB` with a long string). `COMMENT_COL` is
  set effectively-off until the manual confirms the real inline-comment column. **[VERIFY]**
- "Segment" (e.g. Segment 12 = B-tree disk filing system) was an **organizing construct
  of the CPT software library**, not a MOL80 language keyword. The assembler does not need
  a built-in segment/link model for v1. **[CONFIRMED]**

## Open items to resolve against the official manual / real source [VERIFY]

1. Exact logical-operator spelling (`+*`/`-*`/`**` for AND/OR/XOR).
2. Rotate mnemonics and direction (`>` meaning; RLC vs RRC vs RAL vs RAR).
3. Radix markers for binary and hex literals.
4. Data-definition directives (byte/word/string/reserve storage) syntax.
5. Assembler directives: origin (ORG), symbol definition (EQU), and any others.
6. Exact comment column (the fixed start column of the inline comment field) and the exact
   block-comment form (how a header/block comment is delimited or recognized).
7. Whether the `?` conditional blocks may nest and whether an "else" form exists.
8. The full, verified MOL80-mnemonic ↔ opcode ↔ standard-8080 table (256 entries).
9. Expression syntax in operands (arithmetic on labels/constants, e.g. `LABEL+2`).

---

## Requirements

### Requirement 1: Parse column-oriented MOL80 source

**User Story:** As a developer preserving CPT software, I want the assembler to read
MOL80 source files with their original column-oriented layout, so that historical
listings assemble without reformatting.

#### Acceptance Criteria

1. WHEN a source line has non-blank text in its first 10 columns THEN the assembler SHALL
   treat that text as a label definition for the current address.
2. WHEN a label is defined THEN the assembler SHALL NOT require or accept a trailing colon
   on the label.
3. WHEN a line's first 10 columns are blank THEN the assembler SHALL treat the remaining
   content as an instruction, directive, or comment with no label.
4. WHEN a label is defined more than once THEN the assembler SHALL report a duplicate-label
   error identifying both locations.
5. WHEN a line has content at or beyond the comment column THEN the assembler SHALL treat
   that trailing text as an inline comment (no delimiter character) and ignore it during code
   generation. *(Exact comment column [VERIFY].)*
6. WHEN the source contains a block comment (e.g. a file header block) THEN the assembler
   SHALL ignore its content during code generation. *(Exact block-comment form [VERIFY].)*

### Requirement 2: Encode one-to-one 8080 instructions

**User Story:** As a developer, I want every MOL80 mnemonic that maps directly to an 8080
instruction to assemble to the correct opcode byte(s), so that output runs on a real or
emulated 8080.

#### Acceptance Criteria

1. WHEN a recognized MOL80 instruction is assembled THEN the assembler SHALL emit the exact
   opcode byte defined for it in the MOL80 instruction table.
2. WHEN an instruction takes an 8-bit immediate (e.g. `n:A`) THEN the assembler SHALL emit
   the opcode followed by one byte of immediate data.
3. WHEN an instruction takes a 16-bit immediate or address (e.g. `nn:HL`, `/s`) THEN the
   assembler SHALL emit the opcode followed by two bytes in 8080 little-endian order.
4. WHEN the move operator `:` is used (`src:dst`) THEN the assembler SHALL emit the MOV/MVI/
   store/load opcode whose destination is the right operand and source is the left operand.
5. WHEN an arithmetic or logical operator targeting A is used (`A+B:A`, `A+*C:A`, etc.) THEN
   the assembler SHALL emit the matching ADD/SUB/ANA/ORA/XRA-family opcode.
6. WHEN an instruction or operand is not recognized THEN the assembler SHALL report an error
   with the source line and column, and SHALL NOT emit a byte for it.

### Requirement 3: Resolve labels and symbolic addresses

**User Story:** As a developer, I want to reference labels for jumps, calls, and data, so
that code is relocatable and readable.

#### Acceptance Criteria

1. WHEN a label is referenced as a jump/call target THEN the assembler SHALL substitute the
   label's resolved 16-bit address at its use site.
2. WHEN a referenced label is never defined THEN the assembler SHALL report an
   undefined-label error naming the label and its use site.
3. WHEN forward references occur (label used before defined) THEN the assembler SHALL resolve
   them via a two-pass (or equivalent) strategy.
4. WHEN an origin directive sets the assembly address THEN subsequent label addresses SHALL
   reflect that origin. *(Directive syntax [VERIFY].)*

### Requirement 4: Synthesize structured conditional blocks

**User Story:** As a developer, I want MOL80's `cond ? ( ... )` conditional blocks to
generate correct comparison and branch code, so that higher-level MOL80 control flow
assembles faithfully.

#### Acceptance Criteria

1. WHEN a flag test of the form `Fx=0?` or `Fx=1?` precedes `/s` THEN the assembler SHALL
   emit the matching 8080 conditional jump (JC/JNC/JZ/JNZ/JP/JM/JPE/JPO) for flag
   x ∈ {C,P,S,Z} and the tested value.
2. WHEN a conditional block `cond ? ( body )` is assembled THEN the assembler SHALL emit the
   comparison for `cond`, a conditional branch that skips `body` when the condition is
   false, the code for `body`, and an internally generated skip-target label after `body`.
3. WHEN the condition is a register comparison (e.g. `A=B`) THEN the assembler SHALL emit the
   appropriate compare instruction followed by the inverted conditional branch, so that
   `body` executes only when the condition holds.
4. WHEN conditional blocks are nested THEN each SHALL receive a distinct internal label and
   assemble correctly. *(Nesting support [VERIFY].)*
5. WHEN internal labels are generated THEN they SHALL NOT collide with user-defined labels.

### Requirement 5: Define data constants and storage

**User Story:** As a developer, I want to define byte/word constants, strings, and reserved
storage, so that programs can embed tables and data.

#### Acceptance Criteria

1. WHEN a byte-constant directive is used THEN the assembler SHALL emit the given byte(s) in
   order. *(Syntax [VERIFY].)*
2. WHEN a word-constant directive is used THEN the assembler SHALL emit each word as two
   little-endian bytes.
3. WHEN a string constant is used THEN the assembler SHALL emit one byte per character using
   the system's character encoding. *(Encoding [VERIFY] — CPT display/char set.)*
4. WHEN a reserve-storage directive is used THEN the assembler SHALL advance the location
   counter by the reserved size without emitting initialized bytes (or emit fill bytes, TBD).
5. WHEN numeric literals are written in binary, hex, or decimal THEN the assembler SHALL parse
   each to its correct value. *(Radix markers [VERIFY].)*

### Requirement 6: Produce verifiable output

**User Story:** As a developer validating a reverse-engineered toolchain, I want outputs I
can inspect and diff against known-good references, so that I can trust the assembler.

#### Acceptance Criteria

1. WHEN assembly succeeds THEN the assembler SHALL write a binary image of the generated bytes.
2. WHEN requested THEN the assembler SHALL also emit a human-readable listing that pairs each
   source line with its address and generated bytes.
3. WHEN requested THEN the assembler SHALL emit a transpilation to standard Intel 8080
   assembly mnemonics, so output can be cross-checked against an independent 8080 assembler.
4. WHEN assembly fails THEN the assembler SHALL exit with a non-zero status and SHALL NOT
   write a partial binary as if it were complete.

### Requirement 7: Report errors usefully

**User Story:** As a developer, I want precise diagnostics, so that I can fix source quickly.

#### Acceptance Criteria

1. WHEN any error is reported THEN the message SHALL include the source file, line number, and
   column (where applicable).
2. WHEN multiple independent errors exist THEN the assembler SHALL report as many as it can in
   a single run rather than stopping at the first.
3. WHEN an error message references an instruction THEN it SHALL show the offending text.

### Requirement 8: Instruction table is data-driven and testable

**User Story:** As a maintainer resolving open decode questions, I want the MOL80↔opcode
mapping expressed as reviewable data with tests, so that corrections from the manual are
low-risk.

#### Acceptance Criteria

1. WHEN the instruction table is defined THEN it SHALL be expressed as a single
   authoritative data structure mapping MOL80 mnemonic ↔ opcode byte ↔ standard 8080 mnemonic.
2. WHEN the table is built THEN automated tests SHALL assert that opcode bytes are unique and
   fall in 0–255.
3. WHEN an entry is marked unverified THEN it SHALL be distinguishable (so [VERIFY] items are
   tracked in code, not just docs).

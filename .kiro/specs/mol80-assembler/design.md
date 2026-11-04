# MOL80 Assembler — Design

## Context

`mol80asm` turns MOL80 source into Intel 8080 machine code. MOL80 is richer than a plain
one-line-per-opcode assembler: it has an assign operator (`:`), arithmetic/logic operators
targeting A, flag tests, and a structured conditional-block construct (`cond ? ( ... )`).
That rules out a naive line → opcode lookup and calls for a real lexer + recursive-descent
parser producing an AST, followed by label resolution and code generation.

## Language choice

**Rust**, for both this tool and the later emulator.

- The instruction table and operand encoding are a natural fit for Rust enums + exhaustive
  pattern matching; the compiler flags unhandled operand shapes.
- Clean byte-level output with no GC.
- Shared crate potential with the emulator (opcode metadata, disassembly) and a path to
  WebAssembly for the emulator's browser UI.
- Good testing story for the data-driven instruction table.

**Fallback noted:** if grammar questions stay volatile while we await the manual, a throwaway
Python prototype of the lexer/parser could iterate faster; we would port to Rust once the
grammar stabilizes. Default plan is Rust from the start.

## High-level pipeline

```
source (.mol) ──► Lexer ──► tokens ──► Parser ──► AST
                                                   │
                                                   ▼
                                          Pass 1: layout & symbols
                                          (assign addresses, build symbol table,
                                           allocate internal labels for cond blocks)
                                                   │
                                                   ▼
                                          Pass 2: code generation
                                          (resolve references, emit bytes)
                                                   │
                      ┌────────────────────────────┼────────────────────────────┐
                      ▼                             ▼                             ▼
                 binary image                 listing (addr+bytes)        standard-8080 transpile
```

## Modules (crate layout)

```
mol80asm/
  Cargo.toml
  src/
    main.rs            # CLI entry: args, file IO, output selection, exit codes
    lib.rs             # library surface, orchestrates pipeline
    lexer.rs           # char stream -> Token stream, column-aware
    token.rs           # Token, TokenKind, source spans
    ast.rs             # AST node types
    parser.rs          # recursive-descent parser: tokens -> AST
    symbols.rs         # symbol table, address assignment, internal-label gen
    isa/
      mod.rs           # ISA facade
      table.rs         # the authoritative MOL80 <-> opcode <-> 8080 table (data-driven)
      encode.rs        # AST instruction -> opcode bytes
    codegen.rs         # two-pass driver, emits bytes + fixups
    emit/
      binary.rs        # raw binary image
      listing.rs       # human-readable listing
      transpile.rs     # standard Intel 8080 assembly text
    diag.rs            # diagnostics: collect, format with file/line/col
  tests/
    isa_table.rs       # uniqueness / range / verified-flag assertions
    encode_snapshots.rs# known MOL80 line -> expected bytes
    control_blocks.rs  # cond-block synthesis
    fixtures/          # sample MOL80 + expected bytes/listings
```

## Lexing (column-aware)

The lexer is **column-aware**, because MOL80 lines have fixed fields:

```
col:  1        10                      COMMENT_COL                    EOL
      |--------|-----------------------|-------------------------------|
      label        instruction field        inline comment (no delimiter)
```

- Columns 1–10 non-blank → emit a `Label` token for the leading identifier.
- From column 10 up to `COMMENT_COL` → instruction-field tokens: identifiers, registers,
  `(HL)`, `(STK)`, numbers, and operators `:` `/` `?` `=` `(` `)` `+` `-` `+*` `-*` `**` `>` `,`.
- From `COMMENT_COL` to end of line → an **inline comment**, identified purely by column with
  **no delimiter character**; the lexer discards it. `COMMENT_COL` is a single named constant
  ([VERIFY] — user recalls ~column 40) so adjusting it is a one-line change.
- A **block-comment** form (e.g. a file-header block) is also recognized and discarded.
  Exact form [VERIFY]; isolated to one lexer rule.
- Operators are matched longest-first (`+*`, `-*`, `**` before `+`, `-`, `*`).
- Numbers: decimal, hex, and binary. **Radix markers [VERIFY]** — until confirmed, the lexer
  will accept a documented placeholder scheme (e.g. trailing `H` for hex, `B` for binary,
  bare for decimal) behind a single `parse_number` function so changing it is a one-line fix.

One consequence of column-based comments: the lexer must not treat the comment field as
instruction tokens even when it contains operator-like characters. It slices each line into
the three zones by column *before* tokenizing the instruction field.

`TokenKind` enum covers: `Label`, `Ident`, `Reg(Reg)`, `Mem` (`(HL)`), `Stack` (`(STK)`),
`Num(i64)`, `Flag(FlagKind)`, and the operator/punctuation kinds. Every token carries a
`Span { line, col, len }` for diagnostics.

## AST

```
Program      = Vec<Line>
Line         = { label: Option<Ident>, item: Item, span }
Item         =
    | Instr(Instr)                 # a concrete machine instruction
    | CondJump { flag, value, target }      # Fx=0?/Fx=1? /s  -> conditional JMP
    | CondBlock { cond: Condition, body: Vec<Line> }   # cond ? ( ... )
    | Data(DataDef)                # byte/word/string/reserve   [VERIFY syntax]
    | Directive(Directive)         # org/equ/...                [VERIFY syntax]
    | Empty                        # blank / comment-only line

Instr        = { op: MolOp, operands: Operands }   # abstract form keyed into ISA table
Operands     = encodes left/right of `:`, arithmetic form, etc.
Condition    =
    | FlagTest { flag: FlagKind, value: 0|1 }     # Fx=0? / Fx=1?
    | RegCompare { lhs, rhs }                      # A=B?  (and other comparisons)
FlagKind     = C | P | S | Z
Reg          = A|B|C|D|E|H|L | HL | ... (pairs as needed)
```

`CondBlock` holds a nested `Vec<Line>` so blocks can nest (pending [VERIFY] on nesting). An
"else" form is not modeled yet (also [VERIFY]).

## Parsing

Recursive descent over the token stream.

- A line optionally starts with a `Label` token; the rest is one `Item`.
- Instruction parsing recognizes operand shapes and maps them to an abstract `MolOp` +
  `Operands`, which the ISA layer encodes. The parser does **not** hardcode opcode bytes.
- `?` triggers condition parsing; a following `(` opens a `CondBlock` whose body is parsed as
  a line sequence until the matching `)`. A following `/` parses as `CondJump`.
- Unknown operand shapes produce a diagnostic with span and recovery to the next line, so
  multiple errors surface in one run (Req 7.2).

## Symbol resolution & the two passes

**Pass 1 (layout):** walk the AST maintaining a location counter (start at ORG, default 0).
For each line: record `label → address`; for each `Item`, compute its byte size without
emitting. `CondBlock`/`CondJump` sizes include the synthesized compare + branch; each block
is assigned a fresh internal label (e.g. `__L0001`) in a namespace that cannot collide with
user identifiers (Req 4.5). Duplicate user labels → error (Req 1.4).

**Pass 2 (codegen):** walk again, now emitting bytes. Label references resolve against the
symbol table; undefined references → error (Req 3.2). Addresses emit little-endian (Req 2.3).

Two passes handle forward references (Req 3.3). Alternative single-pass-with-fixups is
equivalent; two-pass chosen for clarity and better diagnostics.

### Conditional-block lowering

`cond ? ( body )` lowers to:

```
            <emit comparison for cond>          ; e.g. CMP B for A=B
            <emit conditional branch to __Lk if cond is FALSE>   ; inverted sense
            <emit body>
   __Lk:    <fall through>
```

`Fx=v? /s` lowers directly to the corresponding conditional JMP (no block). Mapping:

| Flag test        | Standard 8080 |
|------------------|---------------|
| `FZ=1? /s`       | JZ            |
| `FZ=0? /s`       | JNZ           |
| `FC=1? /s`       | JC            |
| `FC=0? /s`       | JNC           |
| `FS=1? /s`       | JM            |
| `FS=0? /s`       | JP            |
| `FP=1? /s`       | JPE           |
| `FP=0? /s`       | JPO           |

For `RegCompare { A=B }` the inverted branch is JNZ (skip body when not equal); other
comparison operators map analogously once their MOL80 spelling is confirmed [VERIFY].

## The ISA table (data-driven — Req 8)

A single table is the authoritative source of truth. Conceptual row:

```
IsaEntry {
    opcode: u8,                 // 0..=255
    mol: MolPattern,            // how it appears in MOL80 (operand shape)
    i8080: &'static str,        // standard mnemonic, for transpile + cross-check
    len: u8,                    // total instruction length in bytes (1..=3)
    verified: bool,             // false for [VERIFY] rows awaiting the manual
}
```

Seeded from the alphabetical scan (opcodes already align with the standard 8080 map). Rows
whose MOL80 spelling is still uncertain (logicals, rotates) are `verified: false`. Tests
(Req 8.2) assert opcode uniqueness and 0–255 range; unverified rows are queryable so the
[VERIFY] backlog lives in code, not only in docs.

Encoding (`isa/encode.rs`) matches an AST `Instr` against `MolPattern`s to select the row and
append immediate/address bytes. Transpilation (`emit/transpile.rs`) reuses the same table's
`i8080` field, so a single table drives both binary and standard-asm output — any fix applies
to both.

## Outputs

- **Binary** (`emit/binary.rs`): the generated byte image; nothing partial is written on
  failure (Req 6.4) — bytes are buffered and only flushed after a clean assembly.
- **Listing** (`emit/listing.rs`): `ADDR  BYTES   SOURCE` per line (Req 6.2).
- **Transpile** (`emit/transpile.rs`): standard Intel 8080 mnemonics for independent
  cross-checking against another assembler (Req 6.3) — a key correctness lever while the
  decode table is still partly unverified.

## Diagnostics

`diag.rs` collects diagnostics rather than panicking on the first. Each carries file, line,
column, severity, message, and the offending text. The CLI prints all collected diagnostics
and exits non-zero if any error occurred (Req 7, Req 6.4).

## Testing strategy

- **ISA table tests:** opcode uniqueness, range, verified-flag inventory.
- **Encoding snapshots:** curated `MOL80 line → expected bytes` cases drawn from the scan
  (e.g. `C:(HL)` → `0x71`, `/s FOO` → `0xC3 lo hi`, `n:A` with n=5 → `0x3E 0x05`).
- **Control-block tests:** `A=B? ( ... )` and `Fx=v? /s` lower to expected byte sequences.
- **Round-trip (once real source arrives):** MOL80 → transpiled 8080 → third-party assembler
  → compare bytes against `mol80asm`'s binary. Strong independent correctness signal.
- **Golden fixtures** under `tests/fixtures/`.

## Risks & open questions

- Several decode details are [VERIFY] (logicals, rotates, radix markers, data/directive/
  comment syntax, nesting/else, full 256-entry table). All are isolated behind the ISA table,
  `parse_number`, and single lexer rules so manual-driven corrections are low-risk and local.
- `CondBlock` semantics beyond the single example (`A=B? (...)`) — the comparison/branch
  mapping for operators other than `=` needs confirmation.
- Character encoding for string constants depends on the CPT character set (shared with the
  emulator spec).

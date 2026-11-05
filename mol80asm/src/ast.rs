//! Abstract syntax tree for MOL80.
//!
//! The parser produces this AST from the token stream; codegen (future work) walks it with
//! the ISA table to emit 8080 bytes. Encoding is deliberately NOT decided here — the AST
//! captures the *shape* of each instruction and leaves opcode selection to the ISA layer.

use crate::token::{FlagKind, Reg, Span};

/// A whole program: a sequence of lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub lines: Vec<Line>,
}

/// One source line: an optional label plus one item.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub label: Option<String>,
    pub item: Item,
    pub span: Span,
}

/// The content of a line after any label.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Blank or comment-only line (possibly just a label).
    Empty,
    /// A data-movement instruction: `src : dst`.
    Move { src: Operand, dst: Operand },
    /// An arithmetic/logic operation targeting A: e.g. `A + B : A`.
    AluOp {
        op: AluOp,
        /// Left operand of the operation (usually A).
        lhs: Operand,
        /// Right operand (register, memory, or immediate).
        rhs: Operand,
        /// Destination (usually A).
        dst: Operand,
    },
    /// An unconditional jump: `/ target`.
    Jump { target: JumpTarget },
    /// A flag-test conditional jump: `Fx=0? /target` or `Fx=1? /target`.
    CondJump {
        flag: FlagKind,
        /// The flag value tested (0 or 1).
        value: u8,
        target: JumpTarget,
    },
    /// A structured conditional block: `cond ? ( body )`.
    CondBlock {
        cond: Condition,
        body: Vec<Line>,
    },
    /// A subroutine call: `CALL(target)`.
    Call { target: JumpTarget },
    /// An unconditional return. Primary spelling `RTN`; `/(STK)` is a rare synonym.
    Return,
    /// A conditional return on a PSW flag: `Fx=0? RTN` / `Fx=1? RTN`.
    CondReturn { flag: FlagKind, value: u8 },
    /// Push a register pair onto the stack: `rp:STK`.
    Push { rp: Reg },
    /// Pop a register pair from the stack: `STK:rp`.
    Pop { rp: Reg },
    /// Register-indirect jump to the address in HL: `/HL` (PCHL).
    JumpHL,
    /// Exchange of two register pairs via the dual-move form `a:b/b:a`.
    ///
    /// `DE:HL/HL:DE` is the canonical case and maps to the single XCHG opcode. Other pair
    /// swaps have no dedicated 8080 instruction and expand to a push/copy/pop sequence; the
    /// encoder decides. `first`/`second` are the two pairs being swapped.
    Exchange { first: Reg, second: Reg },
    /// 16-bit increment of a register pair: `rp+1:rp` (INX).
    IncPair { rp: Reg },
    /// 16-bit decrement of a register pair: `rp-1:rp` (DCX).
    DecPair { rp: Reg },
    /// 16-bit add into HL: `HL+rp:HL` (DAD); `HL*2:HL` is DAD H.
    AddPair { rp: Reg },

    // --- assembler directives (our-convention placeholder syntax; real MOL80 directive syntax
    //     is [VERIFY] from the manual) ---
    /// `ORG addr` — set the location counter to `addr`.
    Org { addr: u16 },
    /// `DB item, item, ...` — define bytes (numbers 0..=255 and/or "strings").
    DefineBytes { items: Vec<DataItem> },
    /// `DW value, ...` — define 16-bit words (emitted little-endian).
    DefineWords { values: Vec<u16> },
    /// `DS count` — reserve `count` zero bytes.
    DefineSpace { count: u16 },
}

/// An item in a `DB` directive: a single byte value or a string (each char becomes a byte).
#[derive(Debug, Clone, PartialEq)]
pub enum DataItem {
    Byte(u8),
    Str(String),
}

/// Operands that appear on either side of `:` or in ALU operations.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// A register / memory / stack operand.
    Reg(Reg),
    /// An immediate numeric value.
    Imm(i64),
    /// A symbolic reference (label/symbol name), resolved later.
    Symbol(String),
    /// An I/O port: `PORT(n)`. `A:PORT(n)` = OUT n, `PORT(n):A` = IN n. [CONFIRMED from scan]
    Port(u8),
}

/// Target of a jump: an address, a label, or (future) an expression.
#[derive(Debug, Clone, PartialEq)]
pub enum JumpTarget {
    Label(String),
    Addr(i64),
}

/// Arithmetic/logical operators targeting A.
///
/// Logical spellings are tentative (`+*`=AND, `-*`=OR, `**`=XOR) and marked unverified in the
/// requirements; they are represented here so parsing can proceed and be corrected centrally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    Add,
    Sub,
    /// `+*` — logical AND (tentative).
    And,
    /// `-*` — logical OR (tentative).
    Or,
    /// `**` — logical XOR (tentative).
    Xor,
}

/// A condition for a `cond ? ( ... )` block.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// A PSW flag test: `Fx=0?` / `Fx=1?`.
    Flag { flag: FlagKind, value: u8 },
    /// A register comparison: e.g. `A=B`.
    Compare { lhs: Operand, rhs: Operand },
}

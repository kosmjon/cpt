//! Tokens and source spans produced by the lexer.

/// A location in the source: 1-based line and column, plus a length in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

impl Span {
    pub fn new(line: u32, col: u32, len: u32) -> Self {
        Span { line, col, len }
    }
}

/// 8080 registers and register pairs, plus the MOL80 memory/stack operands.
///
/// MOL80 writes memory-at-HL as `(HL)` and the stack as `(STK)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
    /// `(HL)` — the 8080 "M" operand (memory addressed by HL).
    M,
    /// Register pair HL.
    HL,
    /// Register pair BC.
    BC,
    /// Register pair DE.
    DE,
    /// Stack pointer.
    SP,
    /// `(STK)` — the stack (PUSH/POP family).
    Stk,
    /// Program status word (A + flags), used by PUSH/POP PSW.
    Psw,
}

/// The PSW flags MOL80 can test: `FC`, `FP`, `FS`, `FZ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    /// Carry.
    C,
    /// Parity.
    P,
    /// Sign.
    S,
    /// Zero.
    Z,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// A label defined in columns 1..LABEL_WIDTH (does not end with a colon).
    Label(String),
    /// A bare identifier in the instruction field (label reference, symbol, etc.).
    Ident(String),
    /// A register / memory / stack operand keyword.
    Reg(Reg),
    /// A flag name: `FC`, `FP`, `FS`, `FZ`.
    Flag(FlagKind),
    /// A numeric literal (already parsed to its value).
    Num(i64),

    // --- operators / punctuation ---
    /// `:` assign/move (source : destination).
    Colon,
    /// `/` jump.
    Slash,
    /// `?` conditional.
    Question,
    /// `=` comparison (as in `A=B?` or `FZ=1?`).
    Equal,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// `+` add.
    Plus,
    /// `-` subtract.
    Minus,
    /// `+*` logical AND (tentative).
    PlusStar,
    /// `-*` logical OR (tentative).
    MinusStar,
    /// `**` logical XOR (tentative).
    StarStar,
    /// `*` (bare star; e.g. appears in rotate forms).
    Star,
    /// `>` direction marker (appears in rotate forms).
    Gt,

    /// End of a logical line.
    Eol,
    /// End of input.
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}

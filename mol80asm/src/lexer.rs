//! Column-aware lexer for MOL80.
//!
//! MOL80 source is column-oriented with three zones per line:
//! ```text
//! col: 1        LABEL_WIDTH            COMMENT_COL                  EOL
//!      |--------|----------------------|------------------------------|
//!      label        instruction field       inline comment (no delimiter)
//! ```
//! A label occupies the first `LABEL_WIDTH` columns and does NOT end with a colon, which
//! leaves `:` unambiguous as the assign operator. The comment field is identified purely by
//! column position (no delimiter character).
//!
//! Several lexical details are still unverified against the official manual; each is isolated
//! behind a single constant/function so a confirmed value is a one-line change:
//!   * `COMMENT_COL`  — where the inline comment field begins. [VERIFY ~40]
//!   * `parse_number` — radix markers for hex/binary/decimal. [VERIFY]

use crate::diag::Diagnostics;
use crate::token::{FlagKind, Reg, Span, Token, TokenKind};

/// Width of the leading label field (columns 1..=LABEL_WIDTH). A non-blank first column
/// marks a label. [CONFIRMED: labels live in the first ~10 chars, no trailing colon.]
pub const LABEL_WIDTH: usize = 10;

/// Column (1-based) at which the inline comment field begins. Everything from here to end of
/// line is a comment with no delimiter. [VERIFY — user recalls ~column 40.]
pub const COMMENT_COL: usize = 40;

pub struct Lexer<'a> {
    diags: &'a mut Diagnostics,
}

impl<'a> Lexer<'a> {
    pub fn new(diags: &'a mut Diagnostics) -> Self {
        Lexer { diags }
    }

    /// Tokenize an entire source string into a flat token stream (lines separated by `Eol`,
    /// terminated by `Eof`).
    pub fn tokenize(&mut self, src: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        for (idx, raw_line) in src.lines().enumerate() {
            let line_no = (idx + 1) as u32;
            self.tokenize_line(raw_line, line_no, &mut tokens);
            tokens.push(Token::new(
                TokenKind::Eol,
                Span::new(line_no, (raw_line.chars().count() + 1) as u32, 0),
            ));
        }
        let last_line = src.lines().count().max(1) as u32;
        tokens.push(Token::new(TokenKind::Eof, Span::new(last_line, 1, 0)));
        tokens
    }

    fn tokenize_line(&mut self, line: &str, line_no: u32, out: &mut Vec<Token>) {
        // Work in chars so column numbers are character-based.
        let chars: Vec<char> = line.chars().collect();

        // --- Zone 1: label field (columns 1..=LABEL_WIDTH) ---
        if !chars.is_empty() && !chars[0].is_whitespace() {
            let end = LABEL_WIDTH.min(chars.len());
            let label: String = chars[..end].iter().collect::<String>().trim_end().to_string();
            if !label.is_empty() {
                out.push(Token::new(
                    TokenKind::Label(label.clone()),
                    Span::new(line_no, 1, label.chars().count() as u32),
                ));
            }
        }

        // --- Zone 3 boundary: strip the inline comment field (column >= COMMENT_COL) ---
        // Columns are 1-based, so the comment starts at index COMMENT_COL-1.
        let instr_end = (COMMENT_COL - 1).min(chars.len());

        // --- Zone 2: instruction field (after the label field, up to the comment column) ---
        let instr_start = LABEL_WIDTH.min(instr_end);
        self.tokenize_instruction(&chars, instr_start, instr_end, line_no, out);
    }

    fn tokenize_instruction(
        &mut self,
        chars: &[char],
        start: usize,
        end: usize,
        line_no: u32,
        out: &mut Vec<Token>,
    ) {
        let mut i = start;
        while i < end {
            let c = chars[i];
            let col = (i + 1) as u32;

            if c.is_whitespace() {
                i += 1;
                continue;
            }

            // Multi-char operators, matched longest-first.
            if let Some((kind, width)) = self.match_operator(chars, i, end) {
                out.push(Token::new(kind, Span::new(line_no, col, width as u32)));
                i += width;
                continue;
            }

            // Numeric literal.
            if c.is_ascii_digit() {
                let (value, width) = self.lex_number(chars, i, end, line_no);
                out.push(Token::new(TokenKind::Num(value), Span::new(line_no, col, width as u32)));
                i += width;
                continue;
            }

            // Identifier / register / flag keyword.
            if is_ident_start(c) {
                let mut j = i;
                while j < end && is_ident_part(chars[j]) {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let kind = classify_word(&word);
                out.push(Token::new(kind, Span::new(line_no, col, (j - i) as u32)));
                i = j;
                continue;
            }

            // `(HL)` / `(STK)` are handled as word-like below, but a bare `(` is an operator.
            // Anything we reach here is unexpected.
            self.diags
                .push(crate::diag::Diagnostic::error(
                    format!("unexpected character '{}'", c),
                    Span::new(line_no, col, 1),
                )
                .with_snippet(c.to_string()));
            i += 1;
        }
    }

    /// Try to match an operator token at position `i`. Longest-match first so `+*`/`-*`/`**`
    /// beat `+`/`-`/`*`.
    fn match_operator(&self, chars: &[char], i: usize, end: usize) -> Option<(TokenKind, usize)> {
        let c = chars[i];
        let next = if i + 1 < end { Some(chars[i + 1]) } else { None };

        match (c, next) {
            ('+', Some('*')) => Some((TokenKind::PlusStar, 2)),
            ('-', Some('*')) => Some((TokenKind::MinusStar, 2)),
            ('*', Some('*')) => Some((TokenKind::StarStar, 2)),
            ('+', _) => Some((TokenKind::Plus, 1)),
            ('-', _) => Some((TokenKind::Minus, 1)),
            ('*', _) => Some((TokenKind::Star, 1)),
            (':', _) => Some((TokenKind::Colon, 1)),
            ('/', _) => Some((TokenKind::Slash, 1)),
            ('?', _) => Some((TokenKind::Question, 1)),
            ('=', _) => Some((TokenKind::Equal, 1)),
            ('>', _) => Some((TokenKind::Gt, 1)),
            (',', _) => Some((TokenKind::Comma, 1)),
            // `(HL)` / `(STK)` / `(BC)` / `(DE)` are lexed as a parenthesized register group.
            ('(', _) => {
                // Look ahead for a `)`; if the inside is a known mem/stack keyword, emit a Reg.
                let mut j = i + 1;
                while j < end && chars[j] != ')' {
                    j += 1;
                }
                if j < end && chars[j] == ')' {
                    let inside: String = chars[i + 1..j].iter().collect::<String>();
                    if let Some(reg) = parenthesized_reg(&inside) {
                        return Some((TokenKind::Reg(reg), (j - i + 1)));
                    }
                }
                Some((TokenKind::LParen, 1))
            }
            (')', _) => Some((TokenKind::RParen, 1)),
            _ => None,
        }
    }

    /// Lex a numeric literal starting at `i`. Returns (value, width-in-chars).
    ///
    /// Radix markers are unverified; the placeholder scheme is:
    ///   * trailing `H` → hexadecimal (e.g. `3EH`)
    ///   * trailing `B` → binary (e.g. `1010B`)
    ///   * otherwise     → decimal
    /// All radix handling lives here so a confirmed scheme is a one-function change. [VERIFY]
    fn lex_number(&mut self, chars: &[char], i: usize, end: usize, line_no: u32) -> (i64, usize) {
        let mut j = i;
        while j < end && (chars[j].is_ascii_alphanumeric()) {
            j += 1;
        }
        let text: String = chars[i..j].iter().collect();
        let width = j - i;

        let value = parse_number(&text).unwrap_or_else(|| {
            self.diags.push(
                crate::diag::Diagnostic::error(
                    format!("invalid numeric literal '{}'", text),
                    Span::new(line_no, (i + 1) as u32, width as u32),
                )
                .with_snippet(text.clone()),
            );
            0
        });
        (value, width)
    }
}

/// Parse a MOL80 numeric literal to its value. Placeholder radix scheme (see `lex_number`).
/// Isolated as a free function so tests and the lexer share one definition. [VERIFY]
pub fn parse_number(text: &str) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let upper = t.to_ascii_uppercase();
    if let Some(body) = upper.strip_suffix('H') {
        i64::from_str_radix(body, 16).ok()
    } else if let Some(body) = upper.strip_suffix('B') {
        i64::from_str_radix(body, 2).ok()
    } else {
        t.parse::<i64>().ok()
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '.'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Classify a bare word as a register, flag, or generic identifier.
fn classify_word(word: &str) -> TokenKind {
    match word.to_ascii_uppercase().as_str() {
        "A" => TokenKind::Reg(Reg::A),
        "B" => TokenKind::Reg(Reg::B),
        "C" => TokenKind::Reg(Reg::C),
        "D" => TokenKind::Reg(Reg::D),
        "E" => TokenKind::Reg(Reg::E),
        "H" => TokenKind::Reg(Reg::H),
        "L" => TokenKind::Reg(Reg::L),
        "HL" => TokenKind::Reg(Reg::HL),
        "BC" => TokenKind::Reg(Reg::BC),
        "DE" => TokenKind::Reg(Reg::DE),
        "SP" => TokenKind::Reg(Reg::SP),
        "PSW" => TokenKind::Reg(Reg::Psw),
        // `STK` (no parens) is the stack operand used for push/pop: `HL:STK`, `STK:HL`.
        "STK" => TokenKind::Reg(Reg::Stk),
        "FC" => TokenKind::Flag(FlagKind::C),
        "FP" => TokenKind::Flag(FlagKind::P),
        "FS" => TokenKind::Flag(FlagKind::S),
        "FZ" => TokenKind::Flag(FlagKind::Z),
        _ => TokenKind::Ident(word.to_string()),
    }
}

/// Map the inside of a `(...)` group to a register operand, if recognized.
fn parenthesized_reg(inside: &str) -> Option<Reg> {
    match inside.trim().to_ascii_uppercase().as_str() {
        "HL" => Some(Reg::M),
        "STK" => Some(Reg::Stk),
        "BC" => Some(Reg::BC),
        "DE" => Some(Reg::DE),
        _ => None,
    }
}

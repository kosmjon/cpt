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
//!   * `NAME EQU expr` — named absolute constants (our-convention placeholder). [VERIFY]
//!     The name is in the label field; `expr` is numbers/earlier equates joined by `+`/`-`.
//!     Resolved here in a pre-pass: uses become numeric tokens, including inside `PORT(...)`.

use crate::diag::Diagnostics;
use crate::token::{FlagKind, Reg, Span, Token, TokenKind};
use std::collections::HashMap;

/// Width of the leading label field (columns 1..=LABEL_WIDTH). A non-blank first column
/// marks a label. [CONFIRMED: labels live in the first ~10 chars, no trailing colon.]
pub const LABEL_WIDTH: usize = 10;

/// Column (1-based) at which a column-based inline comment field begins — everything from here
/// to end of line would be treated as a comment with no delimiter.
///
/// The real MOL80 inline-comment column is [VERIFY] (user recalls ~40). Hard-truncating at a
/// guessed column breaks legitimate long lines (e.g. a `DB` with a long string), so for now we
/// set this past any reasonable line length (effectively disabled) and rely on full-line `*`
/// comments for hand-written source. When the manual confirms the real column, set it here.
pub const COMMENT_COL: usize = 1_000;

pub struct Lexer<'a> {
    diags: &'a mut Diagnostics,
    /// Named constants from `NAME EQU value` lines, collected in a pre-pass so a constant can be
    /// used anywhere in the file (before or after its definition). Uses are replaced by their
    /// value as numeric tokens, so they behave exactly like literals (absolute, never
    /// relocated). EQU syntax is our-convention [VERIFY]; see the module notes.
    /// `(value, is_var)`: `NAME EQU v` is a constant (immediate); `NAME VAR a` names a memory
    /// address, so `NAME:A` / `A:NAME` / `NAME:HL` / `HL:NAME` are direct loads/stores.
    equates: HashMap<String, (i64, bool)>,
}

impl<'a> Lexer<'a> {
    pub fn new(diags: &'a mut Diagnostics) -> Self {
        Lexer { diags, equates: HashMap::new() }
    }

    /// Tokenize an entire source string into a flat token stream (lines separated by `Eol`,
    /// terminated by `Eof`).
    pub fn tokenize(&mut self, src: &str) -> Vec<Token> {
        // Pre-pass: gather every `NAME EQU value` so uses can appear before the definition.
        self.collect_equates(src);

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

        // Full-line comment: if the first non-blank character is '*', ignore the whole line.
        // (Our-convention placeholder for writing .mol files by hand; the confirmed MOL80
        // comment model is column-based. [VERIFY] the real full-line/block comment form.)
        if let Some(first) = chars.iter().find(|c| !c.is_whitespace()) {
            if *first == '*' {
                return;
            }
        }

        // `NAME EQU value` defines a constant (handled entirely in the pre-pass). It produces no
        // tokens: it is not an address label and emits no bytes.
        if split_equ_line(&chars).is_some() {
            return;
        }

        // --- Zone 1: label field (columns 1..=LABEL_WIDTH) ---
        if !chars.is_empty() && !chars[0].is_whitespace() {
            let end = LABEL_WIDTH.min(chars.len());
            let label: String = chars[..end].iter().collect::<String>().trim_end().to_string();
            if !label.is_empty() {
                if self.equates.contains_key(&label) {
                    self.diags.push(crate::diag::Diagnostic::error(
                        format!("label '{}' conflicts with an EQU of the same name", label),
                        Span::new(line_no, 1, label.chars().count() as u32),
                    ));
                }
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

            // Double-quoted string literal (for DB). Runs to the closing quote (no escapes).
            if c == '"' {
                let mut j = i + 1;
                while j < end && chars[j] != '"' {
                    j += 1;
                }
                let s: String = chars[i + 1..j.min(end)].iter().collect();
                if j < end && chars[j] == '"' {
                    out.push(Token::new(
                        TokenKind::Str(s),
                        Span::new(line_no, col, (j - i + 1) as u32),
                    ));
                    i = j + 1;
                } else {
                    self.diags.push(crate::diag::Diagnostic::error(
                        "unterminated string literal",
                        Span::new(line_no, col, (end - i) as u32),
                    ));
                    i = end;
                }
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

                // Special form: PORT(n) — an I/O port operand. If `PORT` is immediately
                // followed by `(`, consume `(n)` and emit a single Port token.
                if word.eq_ignore_ascii_case("PORT") && j < end && chars[j] == '(' {
                    if let Some((port, consumed)) = self.lex_port_tail(chars, j, end, line_no) {
                        out.push(Token::new(
                            TokenKind::Port(port),
                            Span::new(line_no, col, (j - i + consumed) as u32),
                        ));
                        i = j + consumed;
                        continue;
                    }
                }

                // A bare identifier that names an EQU constant becomes its numeric value.
                let kind = match classify_word(&word) {
                    TokenKind::Ident(name) => match self.equates.get(&name) {
                        Some(&(v, false)) => TokenKind::Num(v),
                        Some(&(v, true)) => TokenKind::Var(v),
                        None => TokenKind::Ident(name),
                    },
                    other => other,
                };
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
            ('<', _) => Some((TokenKind::Lt, 1)),
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

    /// Lex the `(n)` tail of a `PORT(n)` operand, with `lp` at the `(`. Returns the port number
    /// (0..=255) and the number of chars consumed (the `(`, the digits, and the `)`), or None if
    /// the form is malformed (caller then falls back to treating `PORT` as an identifier).
    fn lex_port_tail(
        &mut self,
        chars: &[char],
        lp: usize,
        end: usize,
        line_no: u32,
    ) -> Option<(u8, usize)> {
        // Find the matching ')'.
        let mut j = lp + 1;
        while j < end && chars[j] != ')' {
            j += 1;
        }
        if j >= end || chars[j] != ')' {
            return None; // no closing paren
        }
        let inside: String = chars[lp + 1..j].iter().collect::<String>();
        let inside = inside.trim();
        let consumed = j - lp + 1; // '(' .. ')'
        let span = Span::new(line_no, (lp + 1) as u32, inside.chars().count() as u32);
        match self.eval_expr(inside) {
            Ok(value) if (0..=255).contains(&value) => Some((value as u8, consumed)),
            Ok(value) => {
                self.diags.push(crate::diag::Diagnostic::error(
                    format!("port number {} out of range (0..=255)", value),
                    span,
                ));
                Some((0, consumed))
            }
            Err(msg) => {
                self.diags.push(crate::diag::Diagnostic::error(
                    format!("invalid port '{}': {}", inside, msg),
                    span,
                ));
                Some((0, consumed))
            }
        }
    }

    /// Pre-pass: collect every `NAME EQU expr` line into `self.equates`, in source order (an
    /// equate may only reference earlier equates).
    fn collect_equates(&mut self, src: &str) {
        for (idx, raw_line) in src.lines().enumerate() {
            let line_no = (idx + 1) as u32;
            let chars: Vec<char> = raw_line.chars().collect();
            match chars.iter().find(|c| !c.is_whitespace()) {
                None => continue,
                Some('*') => continue,
                _ => {}
            }
            let Some(equ) = split_equ_line(&chars) else { continue };
            let span = Span::new(line_no, 1, chars.len().max(1) as u32);
            let Some(name) = equ.name else {
                self.diags.push(crate::diag::Diagnostic::error("EQU requires a name in the label field", span));
                continue;
            };
            if !is_valid_equ_name(&name) {
                self.diags.push(crate::diag::Diagnostic::error(
                    format!("'{}' is not a valid or is a reserved name for EQU", name),
                    span,
                ));
                continue;
            }
            if self.equates.contains_key(&name) {
                self.diags.push(crate::diag::Diagnostic::error(
                    format!("duplicate EQU '{}'", name),
                    span,
                ));
                continue;
            }
            match self.eval_expr(&equ.expr) {
                Ok(v) => {
                    self.equates.insert(name, (v, equ.is_var));
                }
                Err(msg) => self.diags.push(crate::diag::Diagnostic::error(
                    format!("invalid EQU value for '{}': {}", name, msg),
                    span,
                )),
            }
        }
    }

    /// Evaluate `term (+|- term)*` where a term is a number or an earlier equate.
    fn eval_expr(&self, text: &str) -> Result<i64, String> {
        let mut total: i64 = 0;
        let mut sign: i64 = 1;
        let mut term = String::new();
        let flush = |term: &mut String, sign: i64, total: &mut i64, this: &Self| -> Result<(), String> {
            let v = this.eval_term(term.trim())?;
            *total = total
                .checked_add(sign.checked_mul(v).ok_or("arithmetic overflow")?)
                .ok_or("arithmetic overflow")?;
            term.clear();
            Ok(())
        };
        for ch in text.chars() {
            if ch == '+' || ch == '-' {
                flush(&mut term, sign, &mut total, self)?;
                sign = if ch == '+' { 1 } else { -1 };
            } else {
                term.push(ch);
            }
        }
        flush(&mut term, sign, &mut total, self)?;
        Ok(total)
    }

    fn eval_term(&self, t: &str) -> Result<i64, String> {
        let first = t.chars().next().ok_or_else(|| "missing operand".to_string())?;
        if first.is_ascii_digit() {
            parse_number(t).ok_or_else(|| format!("invalid numeric literal '{}'", t))
        } else if is_ident_start(first) && t.chars().all(is_ident_part) {
            self.equates
                .get(t)
                .map(|&(v, _)| v)
                .ok_or_else(|| format!("undefined equate '{}'", t))
        } else {
            Err(format!("invalid operand '{}'", t))
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

struct EquLine {
    /// True for `NAME VAR addr` (a memory variable), false for `NAME EQU value`.
    is_var: bool,
    name: Option<String>,
    expr: String,
}

/// Recognize `NAME EQU expr`: the name sits in the label field, `EQU` starts the instruction
/// field (column LABEL_WIDTH+1 onward, after optional blanks).
fn split_equ_line(chars: &[char]) -> Option<EquLine> {
    if chars.len() <= LABEL_WIDTH {
        return None;
    }
    let rest: String = chars[LABEL_WIDTH..].iter().collect();
    let rest = rest.trim_start();
    let head = rest.get(..3)?;
    let is_var = head.eq_ignore_ascii_case("VAR");
    if !is_var && !head.eq_ignore_ascii_case("EQU") {
        return None;
    }
    let tail = &rest[3..];
    if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
        return None;
    }
    let name: String = chars[..LABEL_WIDTH].iter().collect::<String>().trim().to_string();
    let name = if chars[0].is_whitespace() || name.is_empty() { None } else { Some(name) };
    Some(EquLine { is_var, name, expr: tail.trim().to_string() })
}

/// An EQU name must be an identifier that is not a register, flag, or directive keyword.
fn is_valid_equ_name(name: &str) -> bool {
    let mut it = name.chars();
    match it.next() {
        Some(c) if is_ident_start(c) => {}
        _ => return false,
    }
    if !name.chars().all(is_ident_part) {
        return false;
    }
    if !matches!(classify_word(name), TokenKind::Ident(_)) {
        return false;
    }
    !["CALL", "RTN", "ORG", "DB", "DW", "DS", "EQU", "VAR", "PORT", "PAUSE", "WAIT", "NOT", "DEC"]
        .iter()
        .any(|k| name.eq_ignore_ascii_case(k))
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
        // `STK` (no parens) is the stack operand used for push/pop: `HL:STK`, `STK:HL`.
        "STK" => TokenKind::Reg(Reg::Stk),
        "FC" => TokenKind::Flag(FlagKind::C),
        "FP" => TokenKind::Flag(FlagKind::P),
        "FS" => TokenKind::Flag(FlagKind::S),
        "FZ" => TokenKind::Flag(FlagKind::Z),
        "FI" => TokenKind::Flag(FlagKind::I),
        _ => TokenKind::Ident(word.to_string()),
    }
}

/// Map the inside of a `(...)` group to a register operand, if recognized.
fn parenthesized_reg(inside: &str) -> Option<Reg> {
    match inside.trim().to_ascii_uppercase().as_str() {
        "HL" => Some(Reg::M),
        "STK" => Some(Reg::Stk),
        "BC" => Some(Reg::AtBC),
        "DE" => Some(Reg::AtDE),
        _ => None,
    }
}

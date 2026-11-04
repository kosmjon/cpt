//! Recursive-descent parser: token stream -> AST.
//!
//! Grammar (confirmed subset of MOL80):
//! ```text
//! program   := line*
//! line      := label? item EOL
//! item      := move | aluop | jump | condjump | condblock | <empty>
//! move      := operand ':' operand
//! aluop     := operand aluspelling operand ':' operand
//! jump      := '/' jumptarget
//! condjump  := flagtest '?' '/' jumptarget
//! condblock := condition '?' '(' line* ')'
//! condition := flagtest | operand '=' operand
//! flagtest  := FLAG '=' NUM
//! operand   := REG | NUM | IDENT
//! jumptarget:= IDENT | NUM
//! ```
//!
//! Error recovery resynchronizes to the next EOL so a single run can report multiple errors.

use crate::ast::*;
use crate::diag::{Diagnostic, Diagnostics};
use crate::token::{FlagKind, Reg, Span, Token, TokenKind};

pub struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    diags: &'a mut Diagnostics,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token>, diags: &'a mut Diagnostics) -> Self {
        Parser { tokens, pos: 0, diags }
    }

    pub fn parse_program(&mut self) -> Program {
        let mut lines = Vec::new();
        while !self.at_eof() {
            if let Some(line) = self.parse_line() {
                lines.push(line);
            }
        }
        Program { lines }
    }

    // ---- token helpers ----

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn at_eol(&self) -> bool {
        matches!(self.peek(), TokenKind::Eol | TokenKind::Eof)
    }

    fn eat_eol(&mut self) {
        if matches!(self.peek(), TokenKind::Eol) {
            self.advance();
        }
    }

    /// Skip tokens up to and including the next EOL (error recovery).
    fn recover_to_eol(&mut self) {
        while !matches!(self.peek(), TokenKind::Eol | TokenKind::Eof) {
            self.advance();
        }
        self.eat_eol();
    }

    fn error(&mut self, msg: impl Into<String>, span: Span) {
        self.diags.push(Diagnostic::error(msg, span));
    }

    // ---- grammar ----

    fn parse_line(&mut self) -> Option<Line> {
        let start_span = self.peek_span();

        // Optional leading label.
        let label = if let TokenKind::Label(name) = self.peek() {
            let name = name.clone();
            self.advance();
            Some(name)
        } else {
            None
        };

        // Empty (or label-only) line.
        if self.at_eol() {
            self.eat_eol();
            return Some(Line {
                label,
                item: Item::Empty,
                span: start_span,
            });
        }

        let item = match self.parse_item() {
            Some(item) => item,
            None => {
                self.recover_to_eol();
                return Some(Line {
                    label,
                    item: Item::Empty,
                    span: start_span,
                });
            }
        };

        // Expect EOL after an item.
        if !self.at_eol() {
            let sp = self.peek_span();
            self.error("expected end of line", sp);
            self.recover_to_eol();
        } else {
            self.eat_eol();
        }

        Some(Line { label, item, span: start_span })
    }

    fn parse_item(&mut self) -> Option<Item> {
        // '/' begins an unconditional jump, or special register-indirect/return forms.
        if matches!(self.peek(), TokenKind::Slash) {
            self.advance();
            // `/(STK)` -> RET (rare synonym). The lexer emits `(STK)` as Reg(Stk).
            if matches!(self.peek(), TokenKind::Reg(Reg::Stk)) {
                self.advance();
                return Some(Item::Return);
            }
            // `/HL` -> PCHL (jump to address in HL).
            if matches!(self.peek(), TokenKind::Reg(Reg::HL)) {
                self.advance();
                return Some(Item::JumpHL);
            }
            let target = self.parse_jump_target()?;
            return Some(Item::Jump { target });
        }

        // Bare keyword items: `CALL(target)` and `RTN`.
        if let TokenKind::Ident(name) = self.peek() {
            if name.eq_ignore_ascii_case("CALL") {
                return self.parse_call();
            }
            if name.eq_ignore_ascii_case("RTN") {
                self.advance();
                return Some(Item::Return);
            }
        }

        // A flag test may begin a conditional: `Fx = v ? ...`.
        if let TokenKind::Flag(flag) = self.peek() {
            let flag = *flag;
            return self.parse_flag_conditional(flag);
        }

        // Otherwise parse an operand and decide based on what follows.
        let first = self.parse_operand()?;

        match self.peek() {
            // `src : dst` — possibly a push/pop (`STK`), or the first half of a dual-move
            // exchange `a:b/b:a`.
            TokenKind::Colon => {
                self.advance();
                let dst = self.parse_operand()?;
                // Dual-move exchange: `a:b / b:a`.
                if matches!(self.peek(), TokenKind::Slash) {
                    return self.parse_exchange_tail(first, dst);
                }
                Some(make_move_or_stack(first, dst))
            }
            // Arithmetic. `*` (bare star) only appears in the 16-bit `HL*2:HL` (DAD H) form.
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::PlusStar
            | TokenKind::MinusStar
            | TokenKind::StarStar
            | TokenKind::Star => {
                self.parse_arith(first)
            }
            // Comparison conditional: `A = B ? ( ... )`
            TokenKind::Equal => {
                self.advance();
                let rhs = self.parse_operand()?;
                self.expect_question()?;
                let cond = Condition::Compare { lhs: first, rhs };
                self.parse_block_body(cond)
            }
            other => {
                let sp = self.peek_span();
                self.error(format!("unexpected token after operand: {:?}", other), sp);
                None
            }
        }
    }

    /// Parse an arithmetic form after the first operand and operator. Dispatches to the 8-bit
    /// ALU (`A+B:A`, `A-1:A`), 16-bit pair ops (`DE+1:DE`, `HL+BC:HL`, `HL*2:HL`), or reports
    /// an error. `first` is the already-parsed left operand; the operator is the current token.
    fn parse_arith(&mut self, first: Operand) -> Option<Item> {
        // Bare `*` is only valid in `HL*2:HL` (DAD H).
        if matches!(self.peek(), TokenKind::Star) {
            self.advance(); // '*'
            let rhs = self.parse_operand()?;
            self.expect_colon()?;
            let dst = self.parse_operand()?;
            // Only `HL*2:HL` is defined.
            if let (Operand::Reg(Reg::HL), Operand::Imm(2), Operand::Reg(Reg::HL)) =
                (&first, &rhs, &dst)
            {
                return Some(Item::AddPair { rp: Reg::HL });
            }
            let sp = self.peek_span();
            self.error("only 'HL*2:HL' uses '*'", sp);
            return None;
        }

        let op = self.parse_alu_op();
        let rhs = self.parse_operand()?;
        self.expect_colon()?;
        let dst = self.parse_operand()?;

        // 16-bit pair operations: lhs and dst are the same register pair.
        if let (Operand::Reg(r), Operand::Reg(rd)) = (&first, &dst) {
            if is_pair(*r) && r == rd {
                return self.pair_arith(op, *r, &rhs);
            }
        }

        // Otherwise an 8-bit ALU op targeting a register (A for the full ops; r for INR/DCR).
        Some(Item::AluOp { op, lhs: first, rhs, dst })
    }

    /// Resolve a 16-bit pair arithmetic form `rp <op> rhs : rp`.
    fn pair_arith(&mut self, op: AluOp, rp: Reg, rhs: &Operand) -> Option<Item> {
        match (op, rhs) {
            // rp + 1 : rp  -> INX ;  rp - 1 : rp -> DCX
            (AluOp::Add, Operand::Imm(1)) => Some(Item::IncPair { rp }),
            (AluOp::Sub, Operand::Imm(1)) => Some(Item::DecPair { rp }),
            // HL + rp2 : HL -> DAD rp2
            (AluOp::Add, Operand::Reg(r2)) if rp == Reg::HL && is_pair(*r2) => {
                Some(Item::AddPair { rp: *r2 })
            }
            _ => {
                let sp = self.peek_span();
                self.error("unsupported 16-bit pair operation", sp);
                None
            }
        }
    }

    /// Parse the tail of a dual-move exchange after seeing `a:b` and a following `/`.
    /// Expects `/ c : d`, and recognizes the swap when `(c,d) == (b,a)`.
    fn parse_exchange_tail(&mut self, a: Operand, b: Operand) -> Option<Item> {
        self.advance(); // '/'
        let c = self.parse_operand()?;
        self.expect_colon()?;
        let d = self.parse_operand()?;
        // A proper exchange has the second move mirror the first: a:b / b:a.
        if c == b && d == a {
            if let (Operand::Reg(p1), Operand::Reg(p2)) = (&a, &b) {
                return Some(Item::Exchange { first: *p1, second: *p2 });
            }
        }
        let sp = self.peek_span();
        self.error("dual-move must be a mirrored exchange 'a:b/b:a'", sp);
        None
    }

    fn expect_colon(&mut self) -> Option<()> {
        if matches!(self.peek(), TokenKind::Colon) {
            self.advance();
            Some(())
        } else {
            let sp = self.peek_span();
            self.error("expected ':'", sp);
            None
        }
    }

    /// Parse `CALL(target)`.
    fn parse_call(&mut self) -> Option<Item> {
        self.advance(); // CALL
        if !matches!(self.peek(), TokenKind::LParen) {
            let sp = self.peek_span();
            self.error("expected '(' after CALL", sp);
            return None;
        }
        self.advance(); // '('
        let target = self.parse_jump_target()?;
        if !matches!(self.peek(), TokenKind::RParen) {
            let sp = self.peek_span();
            self.error("expected ')' to close CALL target", sp);
            return None;
        }
        self.advance(); // ')'
        Some(Item::Call { target })
    }

    /// Parse `Fx = v ?` then either `/target` (CondJump) or `( body )` (CondBlock).
    fn parse_flag_conditional(&mut self, flag: FlagKind) -> Option<Item> {
        self.advance(); // flag
        if !matches!(self.peek(), TokenKind::Equal) {
            let sp = self.peek_span();
            self.error("expected '=' after flag name", sp);
            return None;
        }
        self.advance(); // '='
        let value = match self.peek() {
            TokenKind::Num(n @ (0 | 1)) => {
                let v = *n as u8;
                self.advance();
                v
            }
            _ => {
                let sp = self.peek_span();
                self.error("flag test value must be 0 or 1", sp);
                return None;
            }
        };
        self.expect_question()?;

        // After `Fx=v?`: a conditional jump `/target`, a conditional return `RTN`, or a block.
        if matches!(self.peek(), TokenKind::Slash) {
            self.advance();
            // `/(STK)` after a flag test would be an odd conditional-return synonym; the
            // common conditional-return spelling is `Fx=v? RTN`, handled below.
            let target = self.parse_jump_target()?;
            Some(Item::CondJump { flag, value, target })
        } else if matches!(self.peek(), TokenKind::LParen) {
            self.parse_block_body(Condition::Flag { flag, value })
        } else if matches!(self.peek(), TokenKind::Ident(ref n) if n.eq_ignore_ascii_case("RTN")) {
            self.advance();
            Some(Item::CondReturn { flag, value })
        } else {
            let sp = self.peek_span();
            self.error("expected '/' (jump), 'RTN' (return), or '(' (block) after '?'", sp);
            None
        }
    }

    /// Parse `( line* )` as a conditional-block body. Blocks may span multiple lines.
    fn parse_block_body(&mut self, cond: Condition) -> Option<Item> {
        if !matches!(self.peek(), TokenKind::LParen) {
            let sp = self.peek_span();
            self.error("expected '(' to open conditional block", sp);
            return None;
        }
        self.advance(); // '('

        let mut body = Vec::new();
        loop {
            // Skip EOLs inside the block.
            while matches!(self.peek(), TokenKind::Eol) {
                self.advance();
            }
            match self.peek() {
                TokenKind::RParen => {
                    self.advance();
                    break;
                }
                TokenKind::Eof => {
                    let sp = self.peek_span();
                    self.error("unterminated conditional block: missing ')'", sp);
                    break;
                }
                _ => {
                    // Parse one inner item (inner lines may themselves carry labels).
                    let span = self.peek_span();
                    let label = if let TokenKind::Label(name) = self.peek() {
                        let n = name.clone();
                        self.advance();
                        Some(n)
                    } else {
                        None
                    };
                    match self.parse_item() {
                        Some(item) => body.push(Line { label, item, span }),
                        None => {
                            // Recover within the block: skip to EOL but stay inside parens.
                            while !matches!(
                                self.peek(),
                                TokenKind::Eol | TokenKind::RParen | TokenKind::Eof
                            ) {
                                self.advance();
                            }
                        }
                    }
                }
            }
        }

        Some(Item::CondBlock { cond, body })
    }

    fn expect_question(&mut self) -> Option<()> {
        if matches!(self.peek(), TokenKind::Question) {
            self.advance();
            Some(())
        } else {
            let sp = self.peek_span();
            self.error("expected '?'", sp);
            None
        }
    }

    fn parse_alu_op(&mut self) -> AluOp {
        let op = match self.peek() {
            TokenKind::Plus => AluOp::Add,
            TokenKind::Minus => AluOp::Sub,
            TokenKind::PlusStar => AluOp::And,
            TokenKind::MinusStar => AluOp::Or,
            TokenKind::StarStar => AluOp::Xor,
            _ => unreachable!("parse_alu_op called on non-operator"),
        };
        self.advance();
        op
    }

    fn parse_operand(&mut self) -> Option<Operand> {
        match self.peek().clone() {
            TokenKind::Reg(r) => {
                self.advance();
                Some(Operand::Reg(r))
            }
            TokenKind::Num(n) => {
                self.advance();
                Some(Operand::Imm(n))
            }
            TokenKind::Ident(name) => {
                self.advance();
                Some(Operand::Symbol(name))
            }
            other => {
                let sp = self.peek_span();
                self.error(format!("expected an operand, found {:?}", other), sp);
                None
            }
        }
    }

    fn parse_jump_target(&mut self) -> Option<JumpTarget> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Some(JumpTarget::Label(name))
            }
            TokenKind::Num(n) => {
                self.advance();
                Some(JumpTarget::Addr(n))
            }
            other => {
                let sp = self.peek_span();
                self.error(format!("expected a jump target (label or address), found {:?}", other), sp);
                None
            }
        }
    }
}

fn is_pair(r: Reg) -> bool {
    matches!(r, Reg::BC | Reg::DE | Reg::HL | Reg::SP)
}

/// Turn a `src : dst` into a `Move`, or into a stack `Push`/`Pop` when one side is `(STK)`.
///
/// `rp:(STK)` -> Push rp ; `(STK):rp` -> Pop rp. A `(STK):(STK)` is nonsensical and falls
/// through to a Move, which codegen will reject.
fn make_move_or_stack(src: Operand, dst: Operand) -> Item {
    match (&src, &dst) {
        (Operand::Reg(rp), Operand::Reg(Reg::Stk)) if *rp != Reg::Stk => {
            Item::Push { rp: *rp }
        }
        (Operand::Reg(Reg::Stk), Operand::Reg(rp)) if *rp != Reg::Stk => {
            Item::Pop { rp: *rp }
        }
        _ => Item::Move { src, dst },
    }
}

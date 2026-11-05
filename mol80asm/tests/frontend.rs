//! Front-end (lexer + parser) tests against the confirmed MOL80 grammar subset.

use mol80asm::ast::*;
use mol80asm::parse_source;
use mol80asm::token::{FlagKind, Reg};

/// Parse helper that asserts there were no diagnostics, returning the program.
fn parse_ok(src: &str) -> Program {
    let out = parse_source("test.mol", src);
    assert!(
        out.diags.is_empty(),
        "unexpected diagnostics:\n{}",
        out.diags.render()
    );
    out.program
}

/// A label must sit in the first columns; here `START` begins at column 1.
#[test]
fn label_in_label_field() {
    let p = parse_ok("START     5:A\n");
    assert_eq!(p.lines.len(), 1);
    assert_eq!(p.lines[0].label.as_deref(), Some("START"));
    assert_eq!(
        p.lines[0].item,
        Item::Move { src: Operand::Imm(5), dst: Operand::Reg(Reg::A) }
    );
}

/// An instruction with no label (blank label field) parses with label = None.
#[test]
fn no_label_when_field_blank() {
    // 10 leading spaces, then the instruction.
    let p = parse_ok("          A:B\n");
    assert_eq!(p.lines[0].label, None);
    assert_eq!(
        p.lines[0].item,
        Item::Move { src: Operand::Reg(Reg::A), dst: Operand::Reg(Reg::B) }
    );
}

/// `C:(HL)` is a store into memory-at-HL (the 8080 "M" operand). Confirmed example.
#[test]
fn move_to_memory_m() {
    let p = parse_ok("          C:(HL)\n");
    assert_eq!(
        p.lines[0].item,
        Item::Move { src: Operand::Reg(Reg::C), dst: Operand::Reg(Reg::M) }
    );
}

/// ALU operators, including the tentative logical spellings.
#[test]
fn alu_operators() {
    let p = parse_ok(
        "          A+B:A\n          A+*C:A\n          A-*D:A\n          A**E:A\n          A-1:A\n",
    );
    let ops: Vec<AluOp> = p
        .lines
        .iter()
        .map(|l| match &l.item {
            Item::AluOp { op, .. } => *op,
            other => panic!("expected AluOp, got {:?}", other),
        })
        .collect();
    assert_eq!(ops, vec![AluOp::Add, AluOp::And, AluOp::Or, AluOp::Xor, AluOp::Sub]);
}

/// `FZ=1? /DONE` is a conditional jump on the zero flag.
#[test]
fn flag_conditional_jump() {
    let p = parse_ok("          FZ=1? /DONE\n");
    assert_eq!(
        p.lines[0].item,
        Item::CondJump {
            flag: FlagKind::Z,
            value: 1,
            target: JumpTarget::Label("DONE".into())
        }
    );
}

/// `/LOOP` is an unconditional jump.
#[test]
fn unconditional_jump() {
    let p = parse_ok("          /LOOP\n");
    assert_eq!(
        p.lines[0].item,
        Item::Jump { target: JumpTarget::Label("LOOP".into()) }
    );
}

/// The structured conditional block `A=B? ( D:B  E:C )`.
#[test]
fn conditional_block() {
    let p = parse_ok("          A=B? (D:B  E:C)\n");
    match &p.lines[0].item {
        Item::CondBlock { cond, body } => {
            assert_eq!(
                *cond,
                Condition::Compare {
                    lhs: Operand::Reg(Reg::A),
                    rhs: Operand::Reg(Reg::B)
                }
            );
            assert_eq!(body.len(), 2);
            assert_eq!(
                body[0].item,
                Item::Move { src: Operand::Reg(Reg::D), dst: Operand::Reg(Reg::B) }
            );
            assert_eq!(
                body[1].item,
                Item::Move { src: Operand::Reg(Reg::E), dst: Operand::Reg(Reg::C) }
            );
        }
        other => panic!("expected CondBlock, got {:?}", other),
    }
}

/// A flag-test conditional block (not just a jump): `FC=0? ( A:B )`.
#[test]
fn flag_conditional_block() {
    let p = parse_ok("          FC=0? (A:B)\n");
    match &p.lines[0].item {
        Item::CondBlock { cond, body } => {
            assert_eq!(*cond, Condition::Flag { flag: FlagKind::C, value: 0 });
            assert_eq!(body.len(), 1);
        }
        other => panic!("expected CondBlock, got {:?}", other),
    }
}

/// Full-line comments: a line whose first non-blank char is '*' is ignored entirely. It still
/// occupies a (blank) line in the AST, like any blank line, but contributes no instruction and
/// its operator-like characters are never parsed.
#[test]
fn full_line_star_comment_is_ignored() {
    let src = "* this is a comment with / and : operators\n          A:B\n";
    let p = parse_ok(src); // no diagnostics: the comment text was not parsed as code
    // Exactly one real instruction survives; the comment line is just an Empty line.
    let real: Vec<_> = p
        .lines
        .iter()
        .filter(|l| !matches!(l.item, Item::Empty))
        .collect();
    assert_eq!(real.len(), 1);
    assert_eq!(
        real[0].item,
        Item::Move { src: Operand::Reg(Reg::A), dst: Operand::Reg(Reg::B) }
    );
}

/// Numeric radix placeholder scheme: decimal, `H` hex, `B` binary.
#[test]
fn numeric_radix_placeholders() {
    use mol80asm::lexer::parse_number;
    assert_eq!(parse_number("62"), Some(62));
    assert_eq!(parse_number("3EH"), Some(0x3E));
    assert_eq!(parse_number("1010B"), Some(0b1010));
    assert_eq!(parse_number(""), None);
}

/// Errors are collected (not panicked) and reported with position; recovery continues.
#[test]
fn errors_are_collected_and_recovery_continues() {
    // First line is malformed (operand then a stray '?'), second line is valid.
    let out = parse_source("test.mol", "          A @ B\n          A:B\n");
    assert!(out.diags.has_errors(), "expected at least one error");
    // Recovery should still have produced the second, valid line as a Move.
    let has_move = out.program.lines.iter().any(|l| {
        matches!(
            l.item,
            Item::Move { src: Operand::Reg(Reg::A), dst: Operand::Reg(Reg::B) }
        )
    });
    assert!(has_move, "parser should recover and parse the valid line");
}

/// Multiple lines, labels, and blocks together (the example program shape).
#[test]
fn program_shape() {
    let src = "\
START     5:A
LOOP      A-1:A
          FZ=1? /DONE
          /LOOP
DONE      A=B? (D:B  E:C)
";
    let p = parse_ok(src);
    assert_eq!(p.lines.len(), 5);
    assert_eq!(p.lines[0].label.as_deref(), Some("START"));
    assert_eq!(p.lines[1].label.as_deref(), Some("LOOP"));
    assert_eq!(p.lines[4].label.as_deref(), Some("DONE"));
    assert!(matches!(p.lines[4].item, Item::CondBlock { .. }));
}

//! Transpile MOL80 to standard Intel 8080 assembly text.
//!
//! This reuses the ISA table's standard-8080 mnemonics (via `find_mnemonic`) and mirrors the
//! codegen lowering for the structured constructs, so the transpiled assembly is intended to
//! assemble to the SAME bytes as `mol80asm`'s binary. That makes it the basis for the
//! cross-check validation loop: MOL80 -> standard asm -> third-party assembler -> diff bytes.
//!
//! Internal labels use the same `@Lnnnn` scheme as codegen. (Note: `@` is not a legal symbol
//! character in most 8080 assemblers, so the cross-check tooling should map `@Lnnnn` to a
//! legal name such as `QL nnnn`; that substitution is a detail of the external harness.)

use crate::ast::*;
use crate::isa::table::{cond_jump, cond_return, pair_name, Pattern};
use crate::isa::find_mnemonic;
use crate::token::{FlagKind, Reg};

/// Transpile a whole program into standard 8080 assembly text.
pub fn transpile(program: &Program) -> String {
    let mut out = String::new();
    let mut internal = 0u32;
    for line in &program.lines {
        transpile_line(line, &mut out, &mut internal);
    }
    out
}

fn fresh_label(internal: &mut u32) -> String {
    let n = format!("@L{:04}", *internal);
    *internal += 1;
    n
}

fn transpile_line(line: &Line, out: &mut String, internal: &mut u32) {
    if let Some(label) = &line.label {
        out.push_str(&format!("{}:\n", label));
    }
    transpile_item(&line.item, out, internal);
}

fn transpile_item(item: &Item, out: &mut String, internal: &mut u32) {
    match item {
        Item::Empty => {}

        Item::Move { src, dst } => {
            emit(out, &move_mnemonic(src, dst));
        }

        Item::AluOp { op, lhs, rhs, dst } => {
            emit(out, &alu_mnemonic(*op, lhs, rhs, dst));
        }

        Item::Jump { target } => emit(out, &format!("JMP {}", target_str(target))),

        Item::CondJump { flag, value, target } => {
            let info = cond_jump(*flag, *value);
            emit(out, &format!("{} {}", info.i8080, target_str(target)));
        }

        Item::CondBlock { cond, body } => {
            let end = fresh_label(internal);
            match cond {
                Condition::Flag { flag, value } => {
                    let inverted = if *value == 1 { 0 } else { 1 };
                    let info = cond_jump(*flag, inverted);
                    emit(out, &format!("{} {}", info.i8080, end));
                }
                Condition::Compare { lhs, rhs } => {
                    // Only `A = <reg>` supported; CMP r then JNZ @end.
                    if let (Operand::Reg(Reg::A), Operand::Reg(r)) = (lhs, rhs) {
                        emit(out, &format!("CMP {}", reg_str(*r)));
                        let jnz = cond_jump(FlagKind::Z, 0);
                        emit(out, &format!("{} {}", jnz.i8080, end));
                    } else {
                        emit(out, "; <unsupported comparison>");
                    }
                }
            }
            for l in body {
                transpile_line(l, out, internal);
            }
            out.push_str(&format!("{}:\n", end));
        }

        Item::Call { target } => emit(out, &format!("CALL {}", target_str(target))),

        Item::Return => emit(out, "RET"),

        Item::CondReturn { flag, value } => {
            emit(out, cond_return(*flag, *value).i8080);
        }

        Item::Push { rp } => emit(out, &format!("PUSH {}", pair_name(*rp))),

        Item::Pop { rp } => emit(out, &format!("POP {}", pair_name(*rp))),

        Item::JumpHL => emit(out, "PCHL"),

        Item::Exchange { first, second } => transpile_exchange(*first, *second, out),

        Item::IncPair { rp } => emit(out, &format!("INX {}", pair_name(*rp))),

        Item::DecPair { rp } => emit(out, &format!("DCX {}", pair_name(*rp))),

        Item::AddPair { rp } => emit(out, &format!("DAD {}", pair_name(*rp))),

        // Directives transpile to the standard 8080 assembler spellings.
        Item::Org { addr } => emit(out, &format!("ORG {:04X}H", addr)),
        Item::DefineSpace { count } => emit(out, &format!("DS {}", count)),
        Item::DefineWords { values } => {
            let list: Vec<String> = values.iter().map(|v| format!("{:04X}H", v)).collect();
            emit(out, &format!("DW {}", list.join(",")));
        }
        Item::DefineBytes { items } => {
            // Render bytes as hex and strings quoted.
            let parts: Vec<String> = items
                .iter()
                .map(|it| match it {
                    crate::ast::DataItem::Byte(b) => format!("{:02X}H", b),
                    crate::ast::DataItem::Str(s) => format!("'{}'", s),
                })
                .collect();
            emit(out, &format!("DB {}", parts.join(",")));
        }
    }
}

/// Transpile a pair exchange, mirroring the encoder's expansion so the asm matches the bytes.
fn transpile_exchange(first: Reg, second: Reg, out: &mut String) {
    let is = |a: Reg, b: Reg, x: Reg, y: Reg| (a == x && b == y) || (a == y && b == x);
    if is(first, second, Reg::DE, Reg::HL) {
        emit(out, "XCHG");
        return;
    }
    let other = if first == Reg::HL {
        Some(second)
    } else if second == Reg::HL {
        Some(first)
    } else {
        None
    };
    match other {
        Some(Reg::BC) => {
            emit(out, "PUSH B");
            emit(out, "MOV B,H");
            emit(out, "MOV C,L");
            emit(out, "POP H");
        }
        Some(Reg::DE) => {
            // DE<->HL would be XCHG above; this arm is unreachable in practice.
            emit(out, "XCHG");
        }
        _ => emit(out, "; <unsupported exchange>"),
    }
}

fn emit(out: &mut String, instr: &str) {
    out.push_str(&format!("    {}\n", instr));
}

fn move_mnemonic(src: &Operand, dst: &Operand) -> String {
    match (src, dst) {
        (Operand::Reg(Reg::A), Operand::Port(n)) => format!("OUT {:02X}H", n),
        (Operand::Port(n), Operand::Reg(Reg::A)) => format!("IN {:02X}H", n),
        (Operand::Reg(Reg::HL), Operand::Reg(Reg::SP)) => "SPHL".to_string(),
        (Operand::Reg(s), Operand::Reg(d)) => {
            find_mnemonic(Pattern::Move { src: *s, dst: *d })
                .map(|m| m.to_string())
                .unwrap_or_else(|| format!("; <bad MOV {:?}<-{:?}>", d, s))
        }
        (Operand::Imm(n), Operand::Reg(d)) if is_pair(*d) => {
            template(Pattern::LxiImm { rp: *d }, "{nn}", &hex16(*n))
        }
        (Operand::Imm(n), Operand::Reg(d)) => {
            template(Pattern::MviImm { dst: *d }, "{n}", &hex8(*n))
        }
        (Operand::Symbol(name), Operand::Reg(d)) if is_pair(*d) => {
            template(Pattern::LxiImm { rp: *d }, "{nn}", name)
        }
        (Operand::Symbol(name), Operand::Reg(d)) => {
            template(Pattern::MviImm { dst: *d }, "{n}", name)
        }
        _ => format!("; <unsupported move {:?}:{:?}>", src, dst),
    }
}

fn alu_mnemonic(op: AluOp, lhs: &Operand, rhs: &Operand, dst: &Operand) -> String {
    // INR/DCR special case.
    if let (Operand::Reg(r), Operand::Imm(1), Operand::Reg(rd)) = (lhs, rhs, dst) {
        if r == rd {
            match op {
                AluOp::Add => {
                    return find_mnemonic(Pattern::Inr { reg: *r })
                        .map(str::to_string)
                        .unwrap_or_default()
                }
                AluOp::Sub => {
                    return find_mnemonic(Pattern::Dcr { reg: *r })
                        .map(str::to_string)
                        .unwrap_or_default()
                }
                _ => {}
            }
        }
    }
    if let (Operand::Reg(Reg::A), Operand::Reg(r), Operand::Reg(Reg::A)) = (lhs, rhs, dst) {
        return find_mnemonic(Pattern::Alu { op, rhs: *r })
            .map(str::to_string)
            .unwrap_or_else(|| format!("; <bad ALU {:?}>", op));
    }
    format!("; <unsupported ALU {:?}>", op)
}

fn template(pat: Pattern, placeholder: &str, value: &str) -> String {
    find_mnemonic(pat)
        .map(|m| m.replace(placeholder, value))
        .unwrap_or_else(|| format!("; <bad {:?}>", pat))
}

fn target_str(t: &JumpTarget) -> String {
    match t {
        JumpTarget::Label(name) => name.clone(),
        JumpTarget::Addr(a) => hex16(*a),
    }
}

fn reg_str(r: Reg) -> &'static str {
    match r {
        Reg::A => "A",
        Reg::B => "B",
        Reg::C => "C",
        Reg::D => "D",
        Reg::E => "E",
        Reg::H => "H",
        Reg::L => "L",
        Reg::M => "M",
        _ => "?",
    }
}

fn is_pair(r: Reg) -> bool {
    matches!(r, Reg::BC | Reg::DE | Reg::HL | Reg::SP)
}

fn hex8(n: i64) -> String {
    format!("0{:02X}H", (n as u16) & 0xFF)
}

fn hex16(n: i64) -> String {
    format!("0{:04X}H", (n as u16) & 0xFFFF)
}

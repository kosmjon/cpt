//! Encode AST instructions into 8080 opcode bytes using the ISA table.
//!
//! This is the bridge between the parser's AST and machine code. It recognizes the operand
//! *shape* of an AST item, finds the matching [`IsaEntry`], and appends immediate/address
//! bytes in 8080 little-endian order. Symbol/address resolution for immediates is handled by
//! codegen (future work); here immediates that are concrete values are emitted directly.

use crate::ast::{AluOp, Operand};
use crate::isa::table::{isa, Pattern};
use crate::token::Reg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// No table entry matches this operand shape.
    Unsupported(String),
    /// An immediate was required but the operand was not a concrete value.
    NeedsImmediate(String),
    /// An immediate value did not fit the instruction's width.
    ImmediateOutOfRange { value: i64, bits: u32 },
}

/// Emitted bytes for one instruction.
pub type Bytes = Vec<u8>;

/// Encode a `src : dst` move/immediate/load.
///
/// Dispatches to the MOV family (two registers), the MVI family (immediate into a register),
/// or the LXI family (16-bit immediate into a register pair).
pub fn encode_move(src: &Operand, dst: &Operand) -> Result<Bytes, EncodeError> {
    match (src, dst) {
        // `HL:SP` -> SPHL (load SP from HL).
        (Operand::Reg(Reg::HL), Operand::Reg(Reg::SP)) => {
            Ok(vec![crate::isa::table::SPHL])
        }
        // Register/memory to register/memory -> MOV.
        (Operand::Reg(s), Operand::Reg(d)) => {
            find_opcode(Pattern::Move { src: *s, dst: *d })
                .map(|op| vec![op])
                .ok_or_else(|| EncodeError::Unsupported(format!("MOV {:?}<-{:?}", d, s)))
        }
        // Immediate into a register pair (16-bit) -> LXI, else into a register (8-bit) -> MVI.
        (Operand::Imm(n), Operand::Reg(d)) => encode_imm_to_reg(*n, *d),
        // A symbolic immediate (e.g. `nn:HL`) is a 16-bit load into a pair; defer value.
        (Operand::Symbol(_), Operand::Reg(d)) if is_pair(*d) => {
            // Placeholder address (0) until codegen resolves symbols.
            encode_lxi(*d, 0)
        }
        (Operand::Symbol(_), Operand::Reg(d)) => {
            // Symbolic 8-bit immediate into a register -> MVI with placeholder.
            encode_mvi(*d, 0)
        }
        _ => Err(EncodeError::Unsupported(format!(
            "move {:?} : {:?}",
            src, dst
        ))),
    }
}

/// Encode an ALU op `A <op> rhs : A`.
///
/// Only the A-targeted register/memory forms are supported here (ADD/SUB/ANA/ORA/XRA) plus
/// the `r+1:r` / `r-1:r` increment/decrement special cases, which the parser also lowers as
/// `AluOp` with an immediate 1.
pub fn encode_alu(
    op: AluOp,
    lhs: &Operand,
    rhs: &Operand,
    dst: &Operand,
) -> Result<Bytes, EncodeError> {
    // INR/DCR: `r + 1 : r` or `r - 1 : r` where lhs == dst and rhs is immediate 1.
    if let (Operand::Reg(r), Operand::Imm(1), Operand::Reg(rd)) = (lhs, rhs, dst) {
        if r == rd {
            match op {
                AluOp::Add => {
                    return find_opcode(Pattern::Inr { reg: *r })
                        .map(|o| vec![o])
                        .ok_or_else(|| EncodeError::Unsupported(format!("INR {:?}", r)));
                }
                AluOp::Sub => {
                    return find_opcode(Pattern::Dcr { reg: *r })
                        .map(|o| vec![o])
                        .ok_or_else(|| EncodeError::Unsupported(format!("DCR {:?}", r)));
                }
                _ => {}
            }
        }
    }

    // Standard A-targeted ALU with a register/memory rhs.
    if let (Operand::Reg(Reg::A), Operand::Reg(r), Operand::Reg(Reg::A)) = (lhs, rhs, dst) {
        return find_opcode(Pattern::Alu { op, rhs: *r })
            .map(|o| vec![o])
            .ok_or_else(|| EncodeError::Unsupported(format!("ALU {:?} A,{:?}", op, r)));
    }

    Err(EncodeError::Unsupported(format!(
        "alu {:?} {:?},{:?} -> {:?}",
        op, lhs, rhs, dst
    )))
}

fn encode_imm_to_reg(n: i64, d: Reg) -> Result<Bytes, EncodeError> {
    if is_pair(d) {
        encode_lxi(d, n)
    } else {
        encode_mvi(d, n)
    }
}

fn encode_mvi(d: Reg, n: i64) -> Result<Bytes, EncodeError> {
    let op = find_opcode(Pattern::MviImm { dst: d })
        .ok_or_else(|| EncodeError::Unsupported(format!("MVI {:?}", d)))?;
    let byte = fit_u8(n)?;
    Ok(vec![op, byte])
}

fn encode_lxi(rp: Reg, nn: i64) -> Result<Bytes, EncodeError> {
    let op = find_opcode(Pattern::LxiImm { rp })
        .ok_or_else(|| EncodeError::Unsupported(format!("LXI {:?}", rp)))?;
    let (lo, hi) = fit_u16(nn)?;
    Ok(vec![op, lo, hi])
}

fn find_opcode(pat: Pattern) -> Option<u8> {
    isa().iter().find(|e| e.pattern == pat).map(|e| e.opcode)
}

/// Look up the standard Intel 8080 mnemonic template for an operand pattern.
pub fn find_mnemonic(pat: Pattern) -> Option<&'static str> {
    isa().iter().find(|e| e.pattern == pat).map(|e| e.i8080)
}

/// Encode an exchange of two register pairs (the MOL80 dual-move `a:b/b:a`).
///
/// This is the key case where one MOL80 mnemonic may map to MULTIPLE 8080 instructions:
///   * DE <-> HL has a dedicated opcode, XCHG (0xEB) — a single byte.
///   * Any other pair swap has no 8080 instruction, so it expands to a push/copy/pop
///     sequence. For `X <-> HL` (X in {BC,DE}) we use:
///         PUSH X ; MOV Xhi,H ; MOV Xlo,L ; POP H
///     which leaves HL holding X's old value and X holding HL's old value.
pub fn encode_exchange(first: Reg, second: Reg) -> Result<Bytes, EncodeError> {
    use crate::isa::table::{pop_opcode, push_opcode, XCHG};

    let pair = |a: Reg, b: Reg, x: Reg, y: Reg| (a == x && b == y) || (a == y && b == x);

    // DE <-> HL: single XCHG.
    if pair(first, second, Reg::DE, Reg::HL) {
        return Ok(vec![XCHG]);
    }

    // X <-> HL for X in {BC, DE}: push/copy/pop expansion.
    let other = if first == Reg::HL {
        Some(second)
    } else if second == Reg::HL {
        Some(first)
    } else {
        None
    };
    if let Some(x) = other {
        let (hi, lo) = match x {
            Reg::BC => (Reg::B, Reg::C),
            Reg::DE => (Reg::D, Reg::E),
            _ => return Err(EncodeError::Unsupported(format!("exchange HL<->{:?}", x))),
        };
        let push = push_opcode(x).ok_or_else(|| EncodeError::Unsupported("push".into()))?;
        let pop_h = pop_opcode(Reg::HL).unwrap();
        let mov_hi = find_opcode(Pattern::Move { src: Reg::H, dst: hi })
            .ok_or_else(|| EncodeError::Unsupported("mov hi".into()))?;
        let mov_lo = find_opcode(Pattern::Move { src: Reg::L, dst: lo })
            .ok_or_else(|| EncodeError::Unsupported("mov lo".into()))?;
        // PUSH X ; MOV Xhi,H ; MOV Xlo,L ; POP H
        return Ok(vec![push, mov_hi, mov_lo, pop_h]);
    }

    Err(EncodeError::Unsupported(format!(
        "exchange {:?}<->{:?}",
        first, second
    )))
}

fn is_pair(r: Reg) -> bool {
    matches!(r, Reg::BC | Reg::DE | Reg::HL | Reg::SP)
}

/// Encode a 16-bit INX (increment pair).
pub fn encode_inx(rp: Reg) -> Result<Bytes, EncodeError> {
    find_opcode(Pattern::Inx { rp })
        .map(|o| vec![o])
        .ok_or_else(|| EncodeError::Unsupported(format!("INX {:?}", rp)))
}

/// Encode a 16-bit DCX (decrement pair).
pub fn encode_dcx(rp: Reg) -> Result<Bytes, EncodeError> {
    find_opcode(Pattern::Dcx { rp })
        .map(|o| vec![o])
        .ok_or_else(|| EncodeError::Unsupported(format!("DCX {:?}", rp)))
}

/// Encode a 16-bit DAD (add pair into HL).
pub fn encode_dad(rp: Reg) -> Result<Bytes, EncodeError> {
    find_opcode(Pattern::Dad { rp })
        .map(|o| vec![o])
        .ok_or_else(|| EncodeError::Unsupported(format!("DAD {:?}", rp)))
}

fn fit_u8(n: i64) -> Result<u8, EncodeError> {
    // Accept 0..=255 or signed -128..=127.
    if (0..=255).contains(&n) {
        Ok(n as u8)
    } else if (-128..=127).contains(&n) {
        Ok((n as i16 as u16 & 0xFF) as u8)
    } else {
        Err(EncodeError::ImmediateOutOfRange { value: n, bits: 8 })
    }
}

fn fit_u16(n: i64) -> Result<(u8, u8), EncodeError> {
    if (0..=0xFFFF).contains(&n) {
        let v = n as u16;
        Ok((v as u8, (v >> 8) as u8)) // little-endian: low byte first
    } else {
        Err(EncodeError::ImmediateOutOfRange { value: n, bits: 16 })
    }
}

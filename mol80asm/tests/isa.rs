//! ISA table + encoder tests (Req 8.2, 8.3, and encoding snapshots from the scan).

use mol80asm::ast::{AluOp, Operand};
use mol80asm::isa::{encode_alu, encode_move};
use mol80asm::isa::table::isa;
use mol80asm::token::Reg;

/// Opcodes in the regular generated families must be unique (no two patterns share a byte).
/// (LXI rows are intentionally allowed to coexist with others on distinct bytes; this checks
/// that nothing in the generated set collides.)
#[test]
fn opcodes_unique() {
    let table = isa();
    let mut seen = std::collections::HashMap::new();
    for e in table {
        if let Some(prev) = seen.insert(e.opcode, e.pattern) {
            panic!(
                "opcode 0x{:02X} assigned to two patterns: {:?} and {:?}",
                e.opcode, prev, e.pattern
            );
        }
    }
}

/// Instruction lengths are 1..=3 and consistent with the pattern family.
#[test]
fn lengths_are_sane() {
    for e in isa() {
        assert!((1..=3).contains(&e.len), "bad len for {:?}", e.pattern);
    }
}

/// Report how many rows remain unverified (the [VERIFY] backlog lives in code).
#[test]
fn verified_inventory() {
    let total = isa().len();
    let unverified = isa().iter().filter(|e| !e.verified).count();
    // Logical ALU rows (ANA/ORA/XRA = 3 ops x 8 regs = 24) + BC/DE/SP LXI (3) are tentative.
    assert_eq!(unverified, 24 + 3, "unverified count changed: {}/{}", unverified, total);
}

// --- Encoding snapshots: confirmed MOL80 lines -> expected bytes (from the scan) ---

#[test]
fn mov_c_to_memory() {
    // C:(HL) == MOV M,C == 0x71 (decimal 113 in the scan).
    let bytes = encode_move(&Operand::Reg(Reg::C), &Operand::Reg(Reg::M)).unwrap();
    assert_eq!(bytes, vec![0x71]);
}

#[test]
fn mov_a_to_b() {
    // A:B == MOV B,A == 0x47 (decimal 71).
    let bytes = encode_move(&Operand::Reg(Reg::A), &Operand::Reg(Reg::B)).unwrap();
    assert_eq!(bytes, vec![0x47]);
}

#[test]
fn mvi_a_immediate() {
    // 5:A == MVI A,5 == 0x3E 0x05 (opcode 62 decimal).
    let bytes = encode_move(&Operand::Imm(5), &Operand::Reg(Reg::A)).unwrap();
    assert_eq!(bytes, vec![0x3E, 0x05]);
}

#[test]
fn lxi_h_immediate() {
    // 0x1234:HL == LXI H,0x1234 == 0x21 0x34 0x12 (little-endian; opcode 33 decimal).
    let bytes = encode_move(&Operand::Imm(0x1234), &Operand::Reg(Reg::HL)).unwrap();
    assert_eq!(bytes, vec![0x21, 0x34, 0x12]);
}

#[test]
fn add_b_to_a() {
    // A+B:A == ADD B == 0x80 (decimal 128).
    let bytes = encode_alu(
        AluOp::Add,
        &Operand::Reg(Reg::A),
        &Operand::Reg(Reg::B),
        &Operand::Reg(Reg::A),
    )
    .unwrap();
    assert_eq!(bytes, vec![0x80]);
}

#[test]
fn ana_b_to_a() {
    // A+*B:A == ANA B == 0xA0 (decimal 160) — tentative spelling, verified opcode.
    let bytes = encode_alu(
        AluOp::And,
        &Operand::Reg(Reg::A),
        &Operand::Reg(Reg::B),
        &Operand::Reg(Reg::A),
    )
    .unwrap();
    assert_eq!(bytes, vec![0xA0]);
}

#[test]
fn xra_b_to_a() {
    // A**B:A == XRA B == 0xA8 (decimal 168).
    let bytes = encode_alu(
        AluOp::Xor,
        &Operand::Reg(Reg::A),
        &Operand::Reg(Reg::B),
        &Operand::Reg(Reg::A),
    )
    .unwrap();
    assert_eq!(bytes, vec![0xA8]);
}

#[test]
fn ora_b_to_a() {
    // A-*B:A == ORA B == 0xB0 (decimal 176).
    let bytes = encode_alu(
        AluOp::Or,
        &Operand::Reg(Reg::A),
        &Operand::Reg(Reg::B),
        &Operand::Reg(Reg::A),
    )
    .unwrap();
    assert_eq!(bytes, vec![0xB0]);
}

#[test]
fn inr_c() {
    // C+1:C == INR C == 0x0C (decimal 12).
    let bytes = encode_alu(
        AluOp::Add,
        &Operand::Reg(Reg::C),
        &Operand::Imm(1),
        &Operand::Reg(Reg::C),
    )
    .unwrap();
    assert_eq!(bytes, vec![0x0C]);
}

#[test]
fn dcr_a() {
    // A-1:A == DCR A == 0x3D (decimal 61).
    let bytes = encode_alu(
        AluOp::Sub,
        &Operand::Reg(Reg::A),
        &Operand::Imm(1),
        &Operand::Reg(Reg::A),
    )
    .unwrap();
    assert_eq!(bytes, vec![0x3D]);
}

//! Confirmed from the scan: `A+FC+r:A` = ADC r (136..143), `A+FC+n:A` = ACI n (206),
//! `A-FC-n:A` = SBI n (222). `A-FC-r:A` = SBB r (152..159) is inferred by symmetry [VERIFY].

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

fn asm_err(src: &str) -> String {
    let out = assemble_source("t.mol", src);
    assert!(out.assembly.is_none(), "expected failure");
    out.diags.render()
}

const REGS: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];

#[test]
fn adc_registers() {
    // ADC A (143) is never used in MOL80, so only B..(HL) -> 136..142.
    let src: String = REGS[..7].iter().map(|r| format!("          A+FC+{}:A\n", r)).collect();
    assert_eq!(asm(&src), (136..=142).collect::<Vec<u8>>());
}

#[test]
fn sbb_registers() {
    // SBB A (159) is never used in MOL80, so only B..(HL) -> 152..158.
    let src: String = REGS[..7].iter().map(|r| format!("          A-FC-{}:A\n", r)).collect();
    assert_eq!(asm(&src), (152..=158).collect::<Vec<u8>>());
}

#[test]
fn unused_opcodes_are_rejected() {
    // 159 (SBB A) and 167 (ANA A) have no MOL80 spelling.
    assert!(asm_err("          A-FC-A:A\n").contains("159"));
    assert!(asm_err("          A+*A:A\n").contains("167"));
    assert!(asm_err("          A+FC+A:A\n").contains("143"));
    // ORA A / XRA A exist, but only under their scan spellings.
    assert!(asm_err("          A-*A:A\n").contains("0:FC<F>"));
    assert!(asm_err("          A**A:A\n").contains("0:A"));
}

#[test]
fn aci_and_sbi() {
    assert_eq!(asm("          A+FC+5:A\n          A-FC-0FFH:A\n"), vec![206, 5, 222, 0xFF]);
}

#[test]
fn equate_as_carry_operand() {
    assert_eq!(asm("K         EQU 3\n          A+FC+K:A\n"), vec![206, 3]);
}

#[test]
fn plain_add_and_sub_unchanged() {
    assert_eq!(asm("          A+B:A\n          A-5:A\n"), vec![0x80, 0xD6, 5]);
}

#[test]
fn must_target_a() {
    assert!(asm_err("          A+FC+B:B\n").contains("target A"));
}

#[test]
fn transpiles() {
    let t = transpile_source("t.mol", "          A+FC+B:A\n          A-FC-(HL):A\n          A+FC+1:A\n          A-FC-2:A\n")
        .asm
        .unwrap();
    for m in ["ADC B", "SBB M", "ACI 01H", "SBI 02H"] {
        assert!(t.contains(m), "missing {} in {}", m, t);
    }
}

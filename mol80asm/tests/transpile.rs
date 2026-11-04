//! Transpiler tests + control-flow (CALL/RET/PUSH/POP) codegen tests.

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

fn tr(src: &str) -> String {
    let out = transpile_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.asm.unwrap()
}

// --- control-flow codegen ---

#[test]
fn push_pop_opcodes() {
    // HL:STK -> PUSH H (E5) ; STK:HL -> POP H (E1)
    assert_eq!(asm("          HL:STK\n"), vec![0xE5]);
    assert_eq!(asm("          STK:HL\n"), vec![0xE1]);
    // BC -> C5/C1, DE -> D5/D1, PSW -> F5/F1
    assert_eq!(asm("          BC:STK\n"), vec![0xC5]);
    assert_eq!(asm("          STK:DE\n"), vec![0xD1]);
    assert_eq!(asm("          PSW:STK\n"), vec![0xF5]);
}

#[test]
fn call_and_return() {
    // CALL(SUB) with SUB right after the 3-byte call = 0x0003 -> CD 03 00 ; RTN -> C9
    let bytes = asm("          CALL(SUB)\nSUB       RTN\n");
    assert_eq!(bytes, vec![0xCD, 0x03, 0x00, 0xC9]);
}

#[test]
fn return_synonym_and_conditional() {
    // RTN and the rare /(STK) synonym both assemble to RET (C9).
    assert_eq!(asm("          RTN\n"), vec![0xC9]);
    assert_eq!(asm("          /(STK)\n"), vec![0xC9]);
    // Conditional returns: FZ=1? RTN -> RZ (C8); FC=0? RTN -> RNC (D0); FS=1? RTN -> RM (F8).
    assert_eq!(asm("          FZ=1? RTN\n"), vec![0xC8]);
    assert_eq!(asm("          FC=0? RTN\n"), vec![0xD0]);
    assert_eq!(asm("          FS=1? RTN\n"), vec![0xF8]);
}

// --- transpiler ---

#[test]
fn transpile_basic_instructions() {
    let out = tr("          5:A\n          A:B\n          C:(HL)\n");
    assert!(out.contains("MVI A,005H"));
    assert!(out.contains("MOV B,A"));
    assert!(out.contains("MOV M,C"));
}

#[test]
fn transpile_control_flow() {
    let out = tr("          HL:STK\n          CALL(SUB)\n          STK:HL\nSUB       RTN\n");
    assert!(out.contains("PUSH H"));
    assert!(out.contains("CALL SUB"));
    assert!(out.contains("POP H"));
    assert!(out.contains("RET"));
    assert!(out.contains("SUB:"));
}

#[test]
fn transpile_conditional_block() {
    let out = tr("          A=B? (D:B  E:C)\n");
    // CMP B ; JNZ @L0000 ; MOV B,D ; MOV C,E ; @L0000:
    assert!(out.contains("CMP B"));
    assert!(out.contains("JNZ @L0000"));
    assert!(out.contains("MOV B,D"));
    assert!(out.contains("MOV C,E"));
    assert!(out.contains("@L0000:"));
}

#[test]
fn transpile_flag_jump() {
    assert!(tr("          FZ=1? /X\nX         A:B\n").contains("JZ X"));
    assert!(tr("          FC=0? /X\nX         A:B\n").contains("JNC X"));
    assert!(tr("          FS=1? /X\nX         A:B\n").contains("JM X"));
    assert!(tr("          FP=0? /X\nX         A:B\n").contains("JPO X"));
}

/// The transpiler's lowering mirrors codegen's: the number of generated internal labels and
/// the instruction order match, so the assembly is intended to re-assemble to the same bytes.
/// Here we check structural consistency: every label defined in the MOL80 appears in the asm.
#[test]
fn transpile_preserves_labels() {
    let src = "START     5:A\nLOOP      /LOOP\nDONE      /START\n";
    let out = tr(src);
    assert!(out.contains("START:"));
    assert!(out.contains("LOOP:"));
    assert!(out.contains("DONE:"));
    assert!(out.contains("JMP LOOP"));
    assert!(out.contains("JMP START"));
}

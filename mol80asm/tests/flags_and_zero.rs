//! Forms confirmed from the scan: `1:FI` (EI, 251), `0:FI` (DI, 243),
//! `0:FC<F>` (ORA A, 183: clear carry, destroying flags), `0:A` (XRA A, 175).

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

#[test]
fn enable_and_disable_interrupts() {
    assert_eq!(asm("          1:FI\n          0:FI\n"), vec![251, 243]);
}

#[test]
fn clear_carry_destroying_flags_is_ora_a() {
    assert_eq!(asm("          0:FC<F>\n"), vec![183]);
}

#[test]
fn zero_a_is_xra_a() {
    assert_eq!(asm("          0:A\n"), vec![175]);
}

#[test]
fn other_zero_loads_are_still_mvi() {
    assert_eq!(asm("          0:B\n          5:A\n"), vec![0x06, 0x00, 0x3E, 0x05]);
}

#[test]
fn clear_carry_without_f_annotation_is_error() {
    assert!(asm_err("          0:FC\n").contains("<F>"));
}

#[test]
fn bad_flag_assignments_are_errors() {
    assert!(asm_err("          2:FI\n").contains("flag"));
    assert!(asm_err("          1:FZ\n").contains("flag"));
    assert!(asm_err("          FI=1? RTN\n").contains("cannot be tested"));
    assert!(asm_err("          B:C<F>\n").contains("<F>"));
}

#[test]
fn transpiles_to_8080_mnemonics() {
    let t = transpile_source("t.mol", "          1:FI\n          0:FI\n          0:FC<F>\n          0:A\n")
        .asm
        .unwrap();
    for m in ["EI", "DI", "ORA A", "XRA A"] {
        assert!(t.contains(m), "missing {} in {}", m, t);
    }
}

// PAUSE = NOP (0), WAIT = HLT (118), NOT(A):A = CMA (47), 1:FC = STC (55), NOT(FC):FC = CMC (63).

#[test]
fn pause_and_wait() {
    assert_eq!(asm("          PAUSE\n          WAIT\n"), vec![0, 118]);
}

#[test]
fn complement_a_set_and_complement_carry() {
    assert_eq!(asm("          NOT(A):A\n          1:FC\n          NOT(FC):FC\n"), vec![47, 55, 63]);
}

#[test]
fn labels_on_keyword_items() {
    assert_eq!(asm("IDLE      WAIT\n          /IDLE\n"), vec![118, 0xC3, 0x00, 0x00]);
}

#[test]
fn unsupported_not_forms_are_errors() {
    assert!(asm_err("          NOT(B):B\n").contains("NOT"));
    assert!(asm_err("          NOT(A):B\n").contains("NOT"));
}

#[test]
fn keywords_cannot_be_equates() {
    assert!(asm_err("WAIT      EQU 1\n").contains("reserved"));
}

#[test]
fn transpiles_new_forms() {
    let t = transpile_source("t.mol", "          PAUSE\n          WAIT\n          NOT(A):A\n          1:FC\n          NOT(FC):FC\n")
        .asm
        .unwrap();
    for m in ["NOP", "HLT", "CMA", "STC", "CMC"] {
        assert!(t.contains(m), "missing {} in {}", m, t);
    }
}

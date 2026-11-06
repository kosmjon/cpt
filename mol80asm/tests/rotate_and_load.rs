//! Confirmed from the scan: `(BC):A` = LDAX B (10), `(DE):A` = LDAX D (26),
//! `A<*1:A` = RLC (7), `A>*1:A` = RRC (15), `A,FC<*1:FC,A` = RAL (23),
//! `FC,A>*1:A,FC` = RAR (31), `DEC(A):A` = DAA (39).

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
fn load_a_through_bc_and_de() {
    assert_eq!(asm("          (BC):A\n          (DE):A\n"), vec![10, 26]);
}

#[test]
fn rotates() {
    let src = "          A<*1:A\n          A>*1:A\n          A,FC<*1:FC,A\n          FC,A>*1:A,FC\n";
    assert_eq!(asm(src), vec![7, 15, 23, 31]);
}

#[test]
fn rotate_with_label_and_inside_block() {
    assert_eq!(asm("ROT       A<*1:A\n          FC=1? (\n          A>*1:A\n          )\n"),
        vec![7, 0xD2, 0x05, 0x00, 15]);
}

#[test]
fn decimal_adjust() {
    assert_eq!(asm("          DEC(A):A\n"), vec![39]);
}

#[test]
fn unsupported_shapes_are_errors() {
    assert!(asm_err("          A<*2:A\n").len() > 0);
    assert!(asm_err("          DEC(B):B\n").contains("DEC"));
}

#[test]
fn transpiles() {
    let t = transpile_source(
        "t.mol",
        "          (BC):A\n          (DE):A\n          A<*1:A\n          A>*1:A\n          A,FC<*1:FC,A\n          FC,A>*1:A,FC\n          DEC(A):A\n",
    )
    .asm
    .unwrap();
    for m in ["LDAX B", "LDAX D", "RLC", "RRC", "RAL", "RAR", "DAA"] {
        assert!(t.contains(m), "missing {} in {}", m, t);
    }
}
